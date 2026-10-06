//! In-process HTTP gateway logic for browser clients (Hermes / website messaging UI).
//! Matches the SpaceKit simulator contract: envelope POST + SSE broadcast, groups CRUD, PQ keys.
//!
//! ## Groups and channels across nodes
//!
//! Every user runs (or is served by) one messaging node, which admits that
//! user's DID. A group lives on its creator's node, which is authoritative
//! for its membership; other nodes hold replicas:
//!
//! - The creator's node announces the group (`GroupInfo`, with a version that
//!   only goes up) over gossip. A node accepts an announcement only from the
//!   group's creator and only if its version is newer than the replica's.
//! - A user joins or leaves by sending a `GroupJoinRequest` to the creator's
//!   node, which updates the membership and announces the new version.
//! - Senders address a known group's envelope to its current members, and
//!   receivers drop group envelopes from non-members.
//!
//! Three visibilities:
//!
//! - `public`: anyone may join; listed to everyone.
//! - `private`: invitation only; listed to members.
//! - `paid`: a channel with a `listing_id` on the entitlement-ledger contract,
//!   listed to everyone. A user joins by presenting an entitlement for that
//!   listing (bought with `OP_PURCHASE`, or granted by the publisher). The
//!   node checks the entitlement against that listing (`OP_VERIFY_LISTING`)
//!   and that the listing is the creator's, for this group (`OP_GET_LISTING`). The creator's node re-checks
//!   subscriptions periodically and removes members whose entitlement has
//!   expired or been revoked. Invited members (comped by the creator) are not
//!   re-checked.
//!
//! Message content is opaque to the gateway; clients encrypt it (spacekit-js
//! `ChannelKeyring` rotates a channel key whenever members leave).
//!
//! Identity binding: a node's DID is bound to the libp2p peer id that first
//! used it (trust on first use, kept across restarts); messages claiming that
//! DID from another peer are dropped.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use crate::entitlement::{parse_hash32, EntitlementStatus, EntitlementVerifier, VerifyRequest};

pub const VISIBILITY_PUBLIC: &str = "public";
pub const VISIBILITY_PRIVATE: &str = "private";
pub const VISIBILITY_PAID: &str = "paid";

const MAX_GROUP_NAME: usize = 200;
const MAX_GROUP_DESCRIPTION: usize = 4_000;
const MAX_GROUP_MEMBERS: usize = 10_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub creator_did: String,
    pub description: String,
    pub visibility: String,
    pub member_dids: Vec<String>,
    pub created_at: String,
    /// Entitlement-ledger listing of a `paid` channel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listing_id: Option<String>,
    /// Bumped by the creator's node on every change.
    #[serde(default)]
    pub version: u64,
    /// Tombstone: a deleted group keeps its id and version so stale replicas
    /// cannot bring it back.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deleted: bool,
    /// Co-admins: may add and remove members (their signed operations are
    /// applied by every node), so membership does not depend on the creator
    /// being online.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub admin_dids: Vec<String>,
    /// Removed by the creator or an admin; not re-admitted by joining.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub banned_dids: Vec<String>,
}

impl GroupInfo {
    pub fn is_member(&self, did: &str) -> bool {
        self.member_dids.iter().any(|m| m == did)
    }

    pub fn is_admin(&self, did: &str) -> bool {
        self.creator_did == did || self.admin_dids.iter().any(|a| a == did)
    }

    pub fn is_banned(&self, did: &str) -> bool {
        self.banned_dids.iter().any(|b| b == did)
    }

    fn validate(&self) -> Result<(), GatewayError> {
        if self.id.trim().is_empty() || self.id.len() > 200 {
            return Err(GatewayError::BadRequest("invalid group id".into()));
        }
        if self.name.trim().is_empty() || self.name.len() > MAX_GROUP_NAME {
            return Err(GatewayError::BadRequest(format!(
                "name required (at most {MAX_GROUP_NAME} bytes)"
            )));
        }
        if self.description.len() > MAX_GROUP_DESCRIPTION {
            return Err(GatewayError::BadRequest("description too long".into()));
        }
        if self.member_dids.len() > MAX_GROUP_MEMBERS {
            return Err(GatewayError::BadRequest("too many members".into()));
        }
        match self.visibility.as_str() {
            VISIBILITY_PUBLIC | VISIBILITY_PRIVATE => {
                if self.listing_id.is_some() {
                    return Err(GatewayError::BadRequest(
                        "listing_id is only valid for paid channels".into(),
                    ));
                }
            }
            VISIBILITY_PAID => {
                if self.listing_id.as_deref().map_or(true, |l| l.trim().is_empty()) {
                    return Err(GatewayError::BadRequest(
                        "a paid channel needs a listing_id".into(),
                    ));
                }
            }
            other => {
                return Err(GatewayError::BadRequest(format!(
                    "visibility must be public, private or paid (got {other})"
                )))
            }
        }
        Ok(())
    }
}

/// A paid member's entitlement, kept by the channel creator's node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Subscription {
    pub entitlement_id: String,
    pub buyer_pk_hash: String,
    pub verified_at: String,
}

/// Join or leave (by the member), or add/remove (by an admin), signed by
/// the sender's node and gossiped to every node that holds the group.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupJoinRequest {
    pub group_id: String,
    pub did: String,
    /// `join` (default) or `leave` by `did` itself; `add` or `remove` of
    /// `did` by an admin.
    #[serde(default = "default_join")]
    pub action: String,
    #[serde(default)]
    pub entitlement_id: Option<String>,
    #[serde(default)]
    pub buyer_pk_hash: Option<String>,
    /// Strictly increasing per sender and group (a replayed request is
    /// refused); also makes repeated requests distinct on gossip.
    #[serde(default)]
    pub nonce: u64,
}

/// A member admitted by this node without the creator: a paid subscriber
/// whose entitlement this node checked on chain, a public-group joiner, or
/// someone an admin added. Paid entries are re-checked like the creator's.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OverlayMember {
    #[serde(default)]
    pub subscription: Option<Subscription>,
    pub admitted_at: String,
}

/// One message in the receive history. Messages from other nodes keep the
/// sender's signed original, so this node can serve them to members that
/// were offline (backfill) and they can check the sender's signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub payload: Value,
    #[serde(default)]
    pub signed: Option<crate::identity::SignedMessage>,
}

fn default_join() -> String {
    "join".to_string()
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Registry {
    #[serde(default)]
    pq_keys: BTreeMap<String, (String, String)>,
    #[serde(default)]
    groups: BTreeMap<String, GroupInfo>,
    /// group id -> member DID -> subscription (creator's node only).
    #[serde(default)]
    subscriptions: BTreeMap<String, BTreeMap<String, Subscription>>,
    /// DID -> libp2p peer id it was first seen from (unsigned legacy peers).
    #[serde(default)]
    did_peers: BTreeMap<String, String>,
    /// group id -> DID -> members admitted by this node (see OverlayMember).
    #[serde(default)]
    overlay: BTreeMap<String, BTreeMap<String, OverlayMember>>,
    /// "group|sender" -> last join/leave/add/remove nonce accepted.
    #[serde(default)]
    request_nonces: BTreeMap<String, u64>,
    /// group id -> DIDs an admin removed, as seen by this node before the
    /// creator's next announcement.
    #[serde(default)]
    local_bans: BTreeMap<String, BTreeSet<String>>,
}

impl Registry {
    /// Who a group's messages go to and are accepted from: the creator's
    /// list plus this node's overlay, minus anyone banned.
    fn effective_members(&self, group: &GroupInfo) -> Vec<String> {
        if group.deleted {
            return Vec::new();
        }
        let mut out: Vec<String> = group.member_dids.clone();
        if let Some(extra) = self.overlay.get(&group.id) {
            for did in extra.keys() {
                if !out.contains(did) {
                    out.push(did.clone());
                }
            }
        }
        let local = self.local_bans.get(&group.id);
        out.retain(|d| {
            d == &group.creator_did
                || !(group.is_banned(d) || local.is_some_and(|b| b.contains(d)))
        });
        out
    }
}

#[derive(Debug, Clone)]
pub struct GatewayState {
    pub event_tx: broadcast::Sender<String>,
    registry: Arc<Mutex<Registry>>,
    history: Arc<Mutex<VecDeque<HistoryEntry>>>,
    history_capacity: usize,
    store_path: Option<PathBuf>,
    /// Append-only receive history (JSON lines), reloaded on start.
    history_path: Option<PathBuf>,
}

impl GatewayState {
    pub fn new(event_capacity: usize) -> Self {
        let (event_tx, _) = broadcast::channel(event_capacity);
        Self {
            event_tx,
            registry: Arc::new(Mutex::new(Registry::default())),
            history: Arc::new(Mutex::new(VecDeque::with_capacity(event_capacity))),
            history_capacity: event_capacity.max(1),
            store_path: None,
            history_path: None,
        }
    }

    /// Like `new`, with groups, subscriptions, keys and DID bindings kept in
    /// a JSON file (loaded now, rewritten after every change).
    pub fn with_store(event_capacity: usize, path: PathBuf) -> anyhow::Result<Self> {
        let mut state = Self::new(event_capacity);
        if path.exists() {
            let bytes = std::fs::read(&path)?;
            let registry: Registry = serde_json::from_slice(&bytes)?;
            state.registry = Arc::new(Mutex::new(registry));
        } else if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        // Receive history: keep the newest `event_capacity` entries.
        let history_path = path.with_file_name("gateway-history.jsonl");
        if let Ok(text) = std::fs::read_to_string(&history_path) {
            let entries: Vec<HistoryEntry> = text
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            let skip = entries.len().saturating_sub(state.history_capacity);
            let kept: VecDeque<HistoryEntry> = entries.into_iter().skip(skip).collect();
            // Compact the file to what was kept.
            let body: String = kept
                .iter()
                .filter_map(|e| serde_json::to_string(e).ok())
                .map(|l| l + "\n")
                .collect();
            let _ = std::fs::write(&history_path, body);
            state.history = Arc::new(Mutex::new(kept));
        }
        state.history_path = Some(history_path);
        state.store_path = Some(path);
        Ok(state)
    }

    fn persist(&self, registry: &Registry) {
        let Some(path) = &self.store_path else {
            return;
        };
        let result = serde_json::to_vec_pretty(registry)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| {
                let tmp = path.with_extension("json.tmp");
                std::fs::write(&tmp, bytes)?;
                std::fs::rename(&tmp, path)?;
                Ok(())
            });
        if let Err(e) = result {
            tracing::warn!("gateway: could not save {}: {e}", path.display());
        }
    }

    /// Store and broadcast an event (also written to the history file).
    pub async fn ingest(&self, payload: Value) {
        self.ingest_entry(HistoryEntry { payload, signed: None }).await;
    }

    async fn ingest_entry(&self, entry: HistoryEntry) {
        let mut history = self.history.lock().await;
        while history.len() >= self.history_capacity {
            history.pop_front();
        }
        history.push_back(entry.clone());
        drop(history);
        if let Some(path) = &self.history_path {
            if let Ok(line) = serde_json::to_string(&entry) {
                use std::io::Write;
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
            }
        }
        let _ = self.event_tx.send(entry.payload.to_string());
    }

    /// Keep the signed original of a message this node sent, for backfill.
    pub async fn attach_signed(&self, message_id: &str, signed: crate::identity::SignedMessage) {
        let mut history = self.history.lock().await;
        if let Some(e) = history
            .iter_mut()
            .rev()
            .find(|e| e.payload.get("message_id").and_then(Value::as_str) == Some(message_id))
        {
            e.signed = Some(signed);
        }
    }

    async fn seen_message(&self, message_id: &str) -> bool {
        self.history
            .lock()
            .await
            .iter()
            .any(|e| e.payload.get("message_id").and_then(Value::as_str) == Some(message_id))
    }

    /// Ingest an envelope that arrived from another node: dropped if already
    /// seen (replay, or a backfill copy), or if it is for a group this node
    /// knows and its sender is not a member.
    pub async fn ingest_remote(&self, payload: Value) -> bool {
        self.ingest_remote_signed(payload, None).await
    }

    pub async fn ingest_remote_signed(
        &self,
        payload: Value,
        signed: Option<crate::identity::SignedMessage>,
    ) -> bool {
        if let Some(id) = payload.get("message_id").and_then(Value::as_str) {
            if self.seen_message(id).await {
                return false;
            }
        }
        if let Some(group_id) = payload.get("group_id").and_then(Value::as_str) {
            let sender = payload
                .get("sender")
                .and_then(|s| s.get("did"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let registry = self.registry.lock().await;
            if let Some(group) = registry.groups.get(group_id) {
                if !registry.effective_members(group).iter().any(|m| m == sender) {
                    return false;
                }
            }
        }
        self.ingest_entry(HistoryEntry { payload, signed }).await;
        true
    }

    /// Return the bounded receive history visible to one DID.
    pub async fn history_for_did(&self, did: &str) -> Vec<Value> {
        self.history
            .lock()
            .await
            .iter()
            .filter(|e| payload_matches_did(&e.payload.to_string(), did))
            .map(|e| e.payload.clone())
            .collect()
    }

    /// Signed originals of messages for `requester` newer than `since`
    /// (RFC 3339), for backfill: direct messages to them, and group messages
    /// in groups they are (effectively) a member of. At most `limit`.
    pub async fn backfill_for(
        &self,
        requester: &str,
        since: &str,
        limit: usize,
    ) -> Vec<crate::identity::SignedMessage> {
        let registry = self.registry.lock().await;
        let member_of = |group_id: &str| {
            registry
                .groups
                .get(group_id)
                .is_some_and(|g| registry.effective_members(g).iter().any(|m| m == requester))
        };
        let history = self.history.lock().await;
        history
            .iter()
            .filter(|e| e.payload.get("type").and_then(Value::as_str) == Some("message"))
            .filter(|e| {
                e.payload
                    .get("created_at")
                    .and_then(Value::as_str)
                    .is_some_and(|t| t > since)
            })
            .filter(|e| match e.payload.get("group_id").and_then(Value::as_str) {
                Some(g) => member_of(g),
                None => payload_matches_did(&e.payload.to_string(), requester),
            })
            .filter_map(|e| e.signed.clone())
            .take(limit)
            .collect()
    }

    /// Newest `created_at` in the history (RFC 3339), for asking peers what
    /// was missed.
    pub async fn latest_seen(&self) -> Option<String> {
        self.history
            .lock()
            .await
            .iter()
            .filter(|e| e.payload.get("type").and_then(Value::as_str) == Some("message"))
            .filter_map(|e| e.payload.get("created_at").and_then(Value::as_str))
            .max()
            .map(str::to_string)
    }

    /// Effective members of a known group (see `Registry::effective_members`).
    pub async fn effective_members(&self, group_id: &str) -> Option<Vec<String>> {
        let registry = self.registry.lock().await;
        registry.groups.get(group_id).map(|g| registry.effective_members(g))
    }

    /// Bind `did` to the libp2p peer it was first seen from. False if the DID
    /// is already bound to a different peer (the message should be dropped).
    pub async fn bind_peer(&self, did: &str, peer: &str) -> bool {
        let mut registry = self.registry.lock().await;
        match registry.did_peers.get(did) {
            Some(bound) => bound == peer,
            None => {
                registry
                    .did_peers
                    .insert(did.to_string(), peer.to_string());
                self.persist(&registry);
                true
            }
        }
    }

    /// Groups this node is authoritative for (tombstones included), for
    /// re-announcing to peers that were not online for earlier announcements.
    pub async fn owned_groups(&self, local_did: &str) -> Vec<GroupInfo> {
        self.registry
            .lock()
            .await
            .groups
            .values()
            .filter(|g| g.creator_did == local_did)
            .cloned()
            .collect()
    }

    pub async fn subscription(&self, group_id: &str, did: &str) -> Option<Subscription> {
        self.registry
            .lock()
            .await
            .subscriptions
            .get(group_id)
            .and_then(|m| m.get(did))
            .cloned()
    }

    /// Bump the version, save, and tell SSE subscribers: the members, plus
    /// anyone who was a member before this change.
    async fn commit_group(&self, registry: &mut Registry, mut group: GroupInfo, before: &[String]) -> GroupInfo {
        group.version += 1;
        registry.groups.insert(group.id.clone(), group.clone());
        self.persist(registry);
        self.emit_group_event(&group, before).await;
        group
    }

    async fn emit_group_event(&self, group: &GroupInfo, before: &[String]) {
        let mut participants: BTreeSet<String> = group.member_dids.iter().cloned().collect();
        participants.extend(before.iter().cloned());
        participants.insert(group.creator_did.clone());
        self.ingest(json!({
            "type": if group.deleted { "group_deleted" } else { "group" },
            "group_id": group.id,
            "group": group,
            "participants": participants,
            "created_at": Utc::now().to_rfc3339(),
        }))
        .await;
    }
}

#[derive(Debug, Deserialize)]
pub struct EnvelopeContext {
    pub did: String,
    pub timestamp: u64,
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Envelope {
    pub kind: String,
    pub payload: Value,
    pub context: EnvelopeContext,
}

#[derive(Debug, Deserialize)]
pub struct EnvelopeRequest {
    pub message: Envelope,
    pub conversation_type: Option<String>,
    pub recipient_did: Option<String>,
    #[serde(default)]
    pub recipient_dids: Vec<String>,
    pub group_id: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct EnvelopeResponse {
    pub status: String,
    pub conversation_id: String,
    pub created_at: String,
    pub message_id: String,
}

#[derive(Debug, Deserialize)]
pub struct RegisterKeyRequest {
    pub did: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    pub algorithm: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub creator_did: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_public")]
    pub visibility: String,
    #[serde(default)]
    pub member_dids: Vec<String>,
    /// Required for `paid` channels.
    #[serde(default)]
    pub listing_id: Option<String>,
}

fn default_public() -> String {
    VISIBILITY_PUBLIC.to_string()
}

#[derive(Debug, Deserialize)]
pub struct JoinGroupRequest {
    pub did: String,
    /// Paid channels: the entitlement (hex) bought for the channel's listing.
    #[serde(default)]
    pub entitlement_id: Option<String>,
    /// Paid channels: SHA-256 of the subscriber's Kyber public key (hex).
    #[serde(default)]
    pub buyer_pk_hash: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    /// A dependency (the entitlement check) could not answer.
    #[error("unavailable: {0}")]
    Unavailable(String),
}

pub fn payload_matches_did(payload: &str, did: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return false;
    };
    if value
        .get("sender")
        .and_then(|s| s.get("did"))
        .and_then(|d| d.as_str())
        == Some(did)
    {
        return true;
    }
    value
        .get("participants")
        .and_then(|p| p.as_array())
        .is_some_and(|arr| arr.iter().any(|item| item.as_str() == Some(did)))
}

#[derive(Debug, Deserialize)]
pub struct DeleteMessageRequest {
    pub message_id: String,
    pub deleted_by: String,
    #[serde(default)]
    pub participants: Vec<String>,
    pub conversation_type: Option<String>,
    pub group_id: Option<String>,
}

/// Broadcast a delete to SSE subscribers and return the event payload for persistence.
pub async fn broadcast_delete_message(
    state: &GatewayState,
    req: DeleteMessageRequest,
) -> Result<Value, GatewayError> {
    if req.message_id.trim().is_empty() {
        return Err(GatewayError::BadRequest("message_id required".into()));
    }
    if req.deleted_by.trim().is_empty() {
        return Err(GatewayError::BadRequest("deleted_by required".into()));
    }

    let mut participants = req.participants;
    if !participants.iter().any(|d| d == &req.deleted_by) {
        participants.push(req.deleted_by.clone());
    }

    let created_at = Utc::now().to_rfc3339();
    let event_payload = json!({
        "type": "delete",
        "message_id": req.message_id,
        "deleted_by": req.deleted_by,
        "conversation_type": req.conversation_type,
        "group_id": req.group_id,
        "participants": participants,
        "created_at": created_at,
    });

    let event_str = event_payload.to_string();
    let _ = state.event_tx.send(event_str);
    Ok(event_payload)
}

/// Deliver a chat/spacetime envelope to SSE subscribers (simulator-compatible).
///
/// For a group this node knows, the sender must be a member and the envelope
/// goes to the group's current members. Unknown group ids keep the ad-hoc
/// behaviour (the listed recipients).
pub async fn send_envelope(
    state: &GatewayState,
    req: EnvelopeRequest,
) -> Result<(EnvelopeResponse, Value), GatewayError> {
    let message = req.message;
    let content = if message.kind == "chat" {
        match message.payload {
            Value::String(text) => text,
            _ => {
                return Err(GatewayError::BadRequest(
                    "Payload must be a string for chat".into(),
                ))
            }
        }
    } else if message.kind == "spacetime" {
        serde_json::to_string(&message.payload)
            .map_err(|e| GatewayError::BadRequest(format!("spacetime payload encode: {}", e)))?
    } else {
        return Err(GatewayError::BadRequest(format!(
            "Unsupported message kind: {}",
            message.kind
        )));
    };

    let sender_did = message.context.did;
    let created_at = Utc::now().to_rfc3339();
    let conversation_id = format!("conv:{}", Uuid::new_v4().simple());
    let message_id = format!("{}:{}", conversation_id, created_at);

    let known_group = match &req.group_id {
        Some(group_id) => {
            let registry = state.registry.lock().await;
            registry
                .groups
                .get(group_id)
                .map(|g| (g.deleted, registry.effective_members(g)))
        }
        None => None,
    };
    let mut participants = match &known_group {
        Some((true, _)) => return Err(GatewayError::NotFound("Group not found".into())),
        Some((false, members)) => {
            if !members.contains(&sender_did) {
                return Err(GatewayError::Forbidden(
                    "only members can post to this group".into(),
                ));
            }
            members.clone()
        }
        None => {
            let mut participants = req.recipient_dids;
            if let Some(single) = req.recipient_did.clone() {
                if !participants.contains(&single) {
                    participants.push(single);
                }
            }
            participants
        }
    };
    if !participants.contains(&sender_did) {
        participants.push(sender_did.clone());
    }

    let conv_type = req.conversation_type.clone().unwrap_or_else(|| {
        if participants.len() > 2 || req.group_id.is_some() {
            "group".to_string()
        } else {
            "direct".to_string()
        }
    });

    let event_payload = json!({
        "type": "message",
        "message_id": message_id,
        "conversation_id": conversation_id,
        "conversation_type": conv_type,
        "group_id": req.group_id,
        "sender": { "did": sender_did },
        "content": content,
        "created_at": created_at,
        "participants": participants,
    });

    state.ingest(event_payload.clone()).await;
    Ok((
        EnvelopeResponse {
            status: "ok".to_string(),
            conversation_id: conversation_id.clone(),
            created_at: created_at.clone(),
            message_id,
        },
        event_payload,
    ))
}

pub async fn register_pq_key(
    state: &GatewayState,
    req: RegisterKeyRequest,
) -> Result<Value, GatewayError> {
    let alg = req.algorithm.unwrap_or_else(|| "kyber1024".to_string());
    let mut registry = state.registry.lock().await;
    registry
        .pq_keys
        .insert(req.did.clone(), (req.public_key.clone(), alg.clone()));
    state.persist(&registry);
    Ok(json!({ "status": "ok", "did": req.did, "algorithm": alg }))
}

pub async fn get_pq_key(state: &GatewayState, did: &str) -> Result<Value, GatewayError> {
    let registry = state.registry.lock().await;
    match registry.pq_keys.get(did) {
        Some((pk, alg)) => Ok(json!({
            "did": did,
            "publicKey": pk,
            "algorithm": alg,
        })),
        None => Err(GatewayError::NotFound(format!(
            "No PQ key registered for {}",
            did
        ))),
    }
}

pub async fn create_group(
    state: &GatewayState,
    req: CreateGroupRequest,
) -> Result<GroupInfo, GatewayError> {
    create_group_with_id(state, None, req).await
}

pub async fn create_group_with_id(
    state: &GatewayState,
    id: Option<String>,
    req: CreateGroupRequest,
) -> Result<GroupInfo, GatewayError> {
    if req.creator_did.trim().is_empty() {
        return Err(GatewayError::BadRequest("creator_did required".into()));
    }

    let id = id.unwrap_or_else(|| format!("grp:{}", Uuid::new_v4().simple()));
    let mut members = req.member_dids;
    if !members.contains(&req.creator_did) {
        members.insert(0, req.creator_did.clone());
    }
    members.dedup();
    let group = GroupInfo {
        id: id.clone(),
        name: req.name,
        creator_did: req.creator_did,
        description: req.description,
        visibility: req.visibility,
        member_dids: members,
        created_at: Utc::now().to_rfc3339(),
        listing_id: req.listing_id.filter(|l| !l.trim().is_empty()),
        version: 0,
        deleted: false,
        admin_dids: Vec::new(),
        banned_dids: Vec::new(),
    };
    group.validate()?;
    let mut registry = state.registry.lock().await;
    if registry.groups.contains_key(&id) {
        return Err(GatewayError::BadRequest("group id already exists".into()));
    }
    Ok(state.commit_group(&mut registry, group, &[]).await)
}

pub async fn list_groups(state: &GatewayState, viewer_did: Option<&str>) -> Value {
    let registry = state.registry.lock().await;
    let list: Vec<&GroupInfo> = registry
        .groups
        .values()
        .filter(|g| !g.deleted)
        .filter(|g| {
            if g.visibility != VISIBILITY_PRIVATE {
                return true;
            }
            viewer_did.is_some_and(|did| g.is_member(did))
        })
        .collect();
    json!({ "groups": list })
}

pub async fn get_group(state: &GatewayState, group_id: &str) -> Result<GroupInfo, GatewayError> {
    state
        .registry
        .lock()
        .await
        .groups
        .get(group_id)
        .filter(|g| !g.deleted)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))
}

/// Join without an entitlement (creator's node). Public groups admit anyone;
/// private groups only members; paid channels need `join_paid_group`.
pub async fn join_group(
    state: &GatewayState,
    group_id: &str,
    did: &str,
) -> Result<Value, GatewayError> {
    let mut registry = state.registry.lock().await;
    let group = registry
        .groups
        .get(group_id)
        .filter(|g| !g.deleted)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    if group.is_member(did) {
        return Ok(json!({ "status": "joined", "group_id": group_id, "version": group.version }));
    }
    match group.visibility.as_str() {
        VISIBILITY_PRIVATE => {
            return Err(GatewayError::Forbidden(
                "Private group — invitation required".into(),
            ))
        }
        VISIBILITY_PAID => {
            return Err(GatewayError::Forbidden(
                "Paid channel — join with an entitlement for its listing".into(),
            ))
        }
        _ => {}
    }
    if group.member_dids.len() >= MAX_GROUP_MEMBERS {
        return Err(GatewayError::Forbidden("group is full".into()));
    }
    let before = group.member_dids.clone();
    let mut updated = group;
    updated.member_dids.push(did.to_string());
    let updated = state.commit_group(&mut registry, updated, &before).await;
    Ok(json!({ "status": "joined", "group_id": group_id, "version": updated.version }))
}

/// Join a paid channel with an entitlement (creator's node). The entitlement
/// is checked on chain before the member is admitted and kept for re-checks.
pub async fn join_paid_group(
    state: &GatewayState,
    group_id: &str,
    did: &str,
    entitlement_id: &str,
    buyer_pk_hash: &str,
    verifier: Option<&EntitlementVerifier>,
) -> Result<Value, GatewayError> {
    let group = get_group(state, group_id).await?;
    if group.visibility != VISIBILITY_PAID {
        return join_group(state, group_id, did).await;
    }
    let verifier = verifier.ok_or_else(|| {
        GatewayError::Unavailable("entitlement checks are not configured on this node".into())
    })?;
    let entitlement = parse_hash32(entitlement_id)
        .ok_or_else(|| GatewayError::BadRequest("entitlement_id must be 32 bytes of hex".into()))?;
    let pk_hash = parse_hash32(buyer_pk_hash)
        .ok_or_else(|| GatewayError::BadRequest("buyer_pk_hash must be 32 bytes of hex".into()))?;

    // Checked without holding the registry lock.
    let status = verifier
        .verify(VerifyRequest {
            entitlement_id: entitlement,
            buyer_did: did.to_string(),
            listing_id: group.listing_id.clone().unwrap_or_default(),
            publisher_did: group.creator_did.clone(),
            file_id: group.id.clone(),
            buyer_pk_hash: pk_hash,
        })
        .await;
    match status {
        EntitlementStatus::Valid => {}
        EntitlementStatus::Unavailable(reason) => {
            return Err(GatewayError::Unavailable(format!(
                "entitlement check failed: {reason}"
            )))
        }
        denied => {
            return Err(GatewayError::Forbidden(format!(
                "entitlement is not valid for this channel ({})",
                denied.as_str()
            )))
        }
    }

    let mut registry = state.registry.lock().await;
    let current = registry
        .groups
        .get(group_id)
        .filter(|g| !g.deleted)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    registry
        .subscriptions
        .entry(group_id.to_string())
        .or_default()
        .insert(
            did.to_string(),
            Subscription {
                entitlement_id: hex::encode(entitlement),
                buyer_pk_hash: hex::encode(pk_hash),
                verified_at: Utc::now().to_rfc3339(),
            },
        );
    if current.is_member(did) {
        state.persist(&registry);
        return Ok(json!({ "status": "joined", "group_id": group_id, "version": current.version }));
    }
    if current.member_dids.len() >= MAX_GROUP_MEMBERS {
        return Err(GatewayError::Forbidden("group is full".into()));
    }
    let before = current.member_dids.clone();
    let mut updated = current;
    updated.member_dids.push(did.to_string());
    let updated = state.commit_group(&mut registry, updated, &before).await;
    Ok(json!({ "status": "joined", "group_id": group_id, "version": updated.version }))
}

/// Add a member without conditions (creator's node). In a paid channel this
/// comps the member: no entitlement, no re-checks.
pub async fn invite_to_group(
    state: &GatewayState,
    group_id: &str,
    did: &str,
) -> Result<Value, GatewayError> {
    let mut registry = state.registry.lock().await;
    let group = registry
        .groups
        .get(group_id)
        .filter(|g| !g.deleted)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    if group.is_member(did) {
        return Ok(json!({ "status": "invited", "group_id": group_id, "version": group.version }));
    }
    if group.member_dids.len() >= MAX_GROUP_MEMBERS {
        return Err(GatewayError::Forbidden("group is full".into()));
    }
    let before = group.member_dids.clone();
    let mut updated = group;
    updated.member_dids.push(did.to_string());
    let updated = state.commit_group(&mut registry, updated, &before).await;
    Ok(json!({ "status": "invited", "group_id": group_id, "version": updated.version }))
}

/// Remove a member (creator's node): kicked, left, or subscription ended.
pub async fn remove_member(
    state: &GatewayState,
    group_id: &str,
    did: &str,
) -> Result<GroupInfo, GatewayError> {
    let mut registry = state.registry.lock().await;
    let group = registry
        .groups
        .get(group_id)
        .filter(|g| !g.deleted)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    if group.creator_did == did {
        return Err(GatewayError::BadRequest(
            "the creator cannot be removed; delete the group instead".into(),
        ));
    }
    if let Some(subs) = registry.subscriptions.get_mut(group_id) {
        subs.remove(did);
    }
    if !group.is_member(did) {
        state.persist(&registry);
        return Ok(group);
    }
    let before = group.member_dids.clone();
    let mut updated = group;
    updated.member_dids.retain(|m| m != did);
    Ok(state.commit_group(&mut registry, updated, &before).await)
}

/// Delete a group (creator's node). Leaves a tombstone so replicas drop it.
pub async fn delete_group(state: &GatewayState, group_id: &str) -> Result<Value, GatewayError> {
    let mut registry = state.registry.lock().await;
    let group = registry
        .groups
        .get(group_id)
        .filter(|g| !g.deleted)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    registry.subscriptions.remove(group_id);
    let before = group.member_dids.clone();
    let mut tombstone = group;
    tombstone.deleted = true;
    tombstone.member_dids.clear();
    state.commit_group(&mut registry, tombstone, &before).await;
    Ok(json!({ "status": "deleted", "group_id": group_id }))
}

/// Delete, returning the tombstone to announce.
pub async fn delete_group_announced(
    state: &GatewayState,
    group_id: &str,
) -> Result<GroupInfo, GatewayError> {
    delete_group(state, group_id).await?;
    state
        .registry
        .lock()
        .await
        .groups
        .get(group_id)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))
}

/// Accept a group announcement from another node: only from the group's
/// creator, only if newer than our replica, never for a group we own.
pub async fn apply_remote_group(
    state: &GatewayState,
    local_did: &str,
    sender_did: &str,
    group: GroupInfo,
) -> bool {
    if group.creator_did != sender_did || group.creator_did == local_did {
        return false;
    }
    if !group.deleted && group.validate().is_err() {
        return false;
    }
    let mut registry = state.registry.lock().await;
    let before = match registry.groups.get(&group.id) {
        Some(existing) if existing.version >= group.version => return false,
        Some(existing) if existing.creator_did != group.creator_did => return false,
        Some(existing) => existing.member_dids.clone(),
        None => Vec::new(),
    };
    registry.groups.insert(group.id.clone(), group.clone());
    state.persist(&registry);
    drop(registry);
    state.emit_group_event(&group, &before).await;
    true
}

/// Handle a signed join/leave/add/remove request on any node holding the
/// group. `sender_did` is the DID whose signature the request carries.
///
/// - On the creator's node the canonical membership changes and
///   `Ok(Some(group))` is returned for announcing.
/// - On every other node, members are admitted to this node's overlay
///   without waiting for the creator: a paid joiner once this node has
///   checked the entitlement on chain, a public joiner as is, anyone an admin
///   adds; an admin's removal applies locally right away.
///
/// A request whose nonce is not above the sender's last one for this group
/// is refused (replay).
pub async fn handle_join_request(
    state: &GatewayState,
    local_did: &str,
    sender_did: &str,
    req: &GroupJoinRequest,
    verifier: Option<&EntitlementVerifier>,
) -> Result<Option<GroupInfo>, GatewayError> {
    let Ok(group) = get_group(state, &req.group_id).await else {
        return Ok(None);
    };
    match req.action.as_str() {
        "join" | "leave" if req.did != sender_did => {
            return Err(GatewayError::Forbidden("a member joins or leaves only for itself".into()))
        }
        "add" | "remove" if !group.is_admin(sender_did) => {
            return Err(GatewayError::Forbidden("only the creator or an admin can add or remove".into()))
        }
        "join" | "leave" | "add" | "remove" => {}
        other => return Err(GatewayError::BadRequest(format!("unknown action {other}"))),
    }
    {
        let mut registry = state.registry.lock().await;
        let key = format!("{}|{}", req.group_id, sender_did);
        let last = registry.request_nonces.get(&key).copied().unwrap_or(0);
        if req.nonce <= last {
            return Err(GatewayError::BadRequest("replayed or out-of-order request".into()));
        }
        registry.request_nonces.insert(key, req.nonce);
        state.persist(&registry);
    }

    if group.creator_did == local_did {
        let version_before = group.version;
        match req.action.as_str() {
            "leave" => {
                remove_member(state, &req.group_id, &req.did).await?;
            }
            "join" => {
                if group.is_banned(&req.did) {
                    return Err(GatewayError::Forbidden("removed from this group".into()));
                }
                match (&req.entitlement_id, &req.buyer_pk_hash) {
                    (Some(ent), Some(pk)) if group.visibility == VISIBILITY_PAID => {
                        join_paid_group(state, &req.group_id, &req.did, ent, pk, verifier).await?;
                    }
                    _ => {
                        join_group(state, &req.group_id, &req.did).await?;
                    }
                }
            }
            "add" => {
                set_banned(state, &req.group_id, &req.did, false).await?;
                invite_to_group(state, &req.group_id, &req.did).await?;
            }
            _ /* remove */ => {
                remove_member(state, &req.group_id, &req.did).await?;
                set_banned(state, &req.group_id, &req.did, true).await?;
            }
        }
        let after = get_group(state, &req.group_id).await.ok();
        return Ok(after.filter(|g| g.version != version_before));
    }

    // A replica: admit or remove in this node's overlay.
    match req.action.as_str() {
        "join" => {
            if group.is_banned(&req.did) {
                return Err(GatewayError::Forbidden("removed from this group".into()));
            }
            let subscription = match group.visibility.as_str() {
                VISIBILITY_PUBLIC => None,
                VISIBILITY_PAID => {
                    let (Some(ent), Some(pk)) = (&req.entitlement_id, &req.buyer_pk_hash) else {
                        return Err(GatewayError::BadRequest("a paid channel needs an entitlement".into()));
                    };
                    Some(check_entitlement(&group, &req.did, ent, pk, verifier).await?)
                }
                // Private groups: only the creator or an admin adds members.
                _ => return Ok(None),
            };
            overlay_admit(state, &group, &req.did, subscription).await;
        }
        "leave" => overlay_remove(state, &group.id, &req.did).await,
        "add" => {
            {
                let mut registry = state.registry.lock().await;
                if let Some(b) = registry.local_bans.get_mut(&group.id) {
                    b.remove(&req.did);
                }
            }
            overlay_admit(state, &group, &req.did, None).await;
        }
        _ /* remove */ => {
            overlay_remove(state, &group.id, &req.did).await;
            let mut registry = state.registry.lock().await;
            registry
                .local_bans
                .entry(group.id.clone())
                .or_default()
                .insert(req.did.clone());
            state.persist(&registry);
        }
    }
    Ok(None)
}

/// Check an entitlement for a paid group (listing + publisher, see
/// `entitlement`). The verified subscription on success.
async fn check_entitlement(
    group: &GroupInfo,
    did: &str,
    entitlement_id: &str,
    buyer_pk_hash: &str,
    verifier: Option<&EntitlementVerifier>,
) -> Result<Subscription, GatewayError> {
    let verifier = verifier.ok_or_else(|| {
        GatewayError::Unavailable("entitlement checks are not configured on this node".into())
    })?;
    let entitlement = parse_hash32(entitlement_id)
        .ok_or_else(|| GatewayError::BadRequest("entitlement_id must be 32 bytes of hex".into()))?;
    let pk_hash = parse_hash32(buyer_pk_hash)
        .ok_or_else(|| GatewayError::BadRequest("buyer_pk_hash must be 32 bytes of hex".into()))?;
    match verifier
        .verify(VerifyRequest {
            entitlement_id: entitlement,
            buyer_did: did.to_string(),
            listing_id: group.listing_id.clone().unwrap_or_default(),
            publisher_did: group.creator_did.clone(),
            file_id: group.id.clone(),
            buyer_pk_hash: pk_hash,
        })
        .await
    {
        EntitlementStatus::Valid => Ok(Subscription {
            entitlement_id: hex::encode(entitlement),
            buyer_pk_hash: hex::encode(pk_hash),
            verified_at: Utc::now().to_rfc3339(),
        }),
        EntitlementStatus::Unavailable(reason) => Err(GatewayError::Unavailable(format!(
            "entitlement check failed: {reason}"
        ))),
        denied => Err(GatewayError::Forbidden(format!(
            "entitlement is not valid for this channel ({})",
            denied.as_str()
        ))),
    }
}

async fn overlay_admit(state: &GatewayState, group: &GroupInfo, did: &str, subscription: Option<Subscription>) {
    let before = state.effective_members(&group.id).await.unwrap_or_default();
    let mut registry = state.registry.lock().await;
    registry.overlay.entry(group.id.clone()).or_default().insert(
        did.to_string(),
        OverlayMember {
            subscription,
            admitted_at: Utc::now().to_rfc3339(),
        },
    );
    state.persist(&registry);
    let group = registry.groups.get(&group.id).cloned();
    drop(registry);
    if let Some(g) = group {
        if !before.iter().any(|m| m == did) {
            state.emit_group_event(&g, &before).await;
        }
    }
}

async fn overlay_remove(state: &GatewayState, group_id: &str, did: &str) {
    let before = state.effective_members(group_id).await.unwrap_or_default();
    let mut registry = state.registry.lock().await;
    let removed = registry
        .overlay
        .get_mut(group_id)
        .is_some_and(|m| m.remove(did).is_some());
    state.persist(&registry);
    let group = registry.groups.get(group_id).cloned();
    drop(registry);
    if let (true, Some(g)) = (removed, group) {
        state.emit_group_event(&g, &before).await;
    }
}

/// Ban or unban (creator's node; canonical, announced with the group).
pub async fn set_banned(state: &GatewayState, group_id: &str, did: &str, banned: bool) -> Result<GroupInfo, GatewayError> {
    let mut registry = state.registry.lock().await;
    let group = registry
        .groups
        .get(group_id)
        .filter(|g| !g.deleted)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    if group.creator_did == did || group.is_banned(did) == banned {
        return Ok(group);
    }
    let before = group.member_dids.clone();
    let mut updated = group;
    if banned {
        updated.banned_dids.push(did.to_string());
    } else {
        updated.banned_dids.retain(|b| b != did);
    }
    Ok(state.commit_group(&mut registry, updated, &before).await)
}

/// Make `did` an admin or not (creator's node).
pub async fn set_admin(state: &GatewayState, group_id: &str, did: &str, admin: bool) -> Result<GroupInfo, GatewayError> {
    let mut registry = state.registry.lock().await;
    let group = registry
        .groups
        .get(group_id)
        .filter(|g| !g.deleted)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    if group.creator_did == did || group.admin_dids.iter().any(|a| a == did) == admin {
        return Ok(group);
    }
    let before = group.member_dids.clone();
    let mut updated = group;
    if admin {
        updated.admin_dids.push(did.to_string());
    } else {
        updated.admin_dids.retain(|a| a != did);
    }
    Ok(state.commit_group(&mut registry, updated, &before).await)
}

/// Re-check every paid subscription of the channels this node owns and remove
/// members whose entitlement expired or was revoked. A check that cannot be
/// made keeps the member. Returns the groups whose membership changed.
pub async fn revalidate_paid_members(
    state: &GatewayState,
    local_did: &str,
    verifier: &EntitlementVerifier,
) -> Vec<GroupInfo> {
    let checks: Vec<(GroupInfo, String, Subscription)> = {
        let registry = state.registry.lock().await;
        registry
            .subscriptions
            .iter()
            .filter(|(group_id, _)| {
                registry.groups.get(*group_id).is_some_and(|g| {
                    !g.deleted && g.creator_did == local_did && g.visibility == VISIBILITY_PAID
                })
            })
            .flat_map(|(group_id, subs)| {
                let group = registry.groups.get(group_id).cloned();
                subs.iter().filter_map(move |(did, sub)| {
                    group.clone().map(|g| (g, did.clone(), sub.clone()))
                })
            })
            .collect()
    };
    let mut lapsed = Vec::new();
    for (group, did, sub) in checks {
        let group_id = group.id.clone();
        let (Some(entitlement_id), Some(buyer_pk_hash)) =
            (parse_hash32(&sub.entitlement_id), parse_hash32(&sub.buyer_pk_hash))
        else {
            lapsed.push((group_id, did));
            continue;
        };
        let status = verifier
            .verify(VerifyRequest {
                entitlement_id,
                buyer_did: did.clone(),
                listing_id: group.listing_id.clone().unwrap_or_default(),
                publisher_did: group.creator_did.clone(),
                file_id: group_id.clone(),
                buyer_pk_hash,
            })
            .await;
        if status.is_denied() {
            tracing::info!("gateway: {did} left {group_id}: entitlement {}", status.as_str());
            lapsed.push((group_id, did));
        } else if status.is_valid() {
            let mut registry = state.registry.lock().await;
            if let Some(s) = registry
                .subscriptions
                .get_mut(&group_id)
                .and_then(|m| m.get_mut(&did))
            {
                s.verified_at = Utc::now().to_rfc3339();
            }
        }
    }
    let mut changed: BTreeMap<String, GroupInfo> = BTreeMap::new();
    for (group_id, did) in lapsed {
        if let Ok(group) = remove_member(state, &group_id, &did).await {
            changed.insert(group_id, group);
        }
    }
    if !changed.is_empty() {
        let registry = state.registry.lock().await;
        state.persist(&registry);
    }

    // Paid members this node admitted itself (overlay): same check, any group.
    let overlay_checks: Vec<(GroupInfo, String, Subscription)> = {
        let registry = state.registry.lock().await;
        registry
            .overlay
            .iter()
            .filter_map(|(gid, m)| registry.groups.get(gid).map(|g| (g.clone(), m)))
            .filter(|(g, _)| !g.deleted && g.visibility == VISIBILITY_PAID)
            .flat_map(|(g, m)| {
                m.iter()
                    .filter_map(|(did, o)| o.subscription.clone().map(|s| (g.clone(), did.clone(), s)))
                    .collect::<Vec<_>>()
            })
            .collect()
    };
    for (group, did, sub) in overlay_checks {
        let (Some(entitlement_id), Some(buyer_pk_hash)) =
            (parse_hash32(&sub.entitlement_id), parse_hash32(&sub.buyer_pk_hash))
        else {
            overlay_remove(state, &group.id, &did).await;
            continue;
        };
        let status = verifier
            .verify(VerifyRequest {
                entitlement_id,
                buyer_did: did.clone(),
                listing_id: group.listing_id.clone().unwrap_or_default(),
                publisher_did: group.creator_did.clone(),
                file_id: group.id.clone(),
                buyer_pk_hash,
            })
            .await;
        if status.is_denied() {
            tracing::info!("gateway: {did} dropped from {} here: entitlement {}", group.id, status.as_str());
            overlay_remove(state, &group.id, &did).await;
        }
    }
    changed.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat(sender: &str, text: &str) -> Envelope {
        Envelope {
            kind: "chat".to_string(),
            payload: Value::String(text.to_string()),
            context: EnvelopeContext {
                did: sender.to_string(),
                timestamp: 1,
                source: None,
            },
        }
    }

    fn create(name: &str, creator: &str, visibility: &str, listing: Option<&str>) -> CreateGroupRequest {
        CreateGroupRequest {
            name: name.to_string(),
            creator_did: creator.to_string(),
            description: String::new(),
            visibility: visibility.to_string(),
            member_dids: vec![],
            listing_id: listing.map(str::to_string),
        }
    }

    const ASTOR: &str = "did:spacekit:user:astor";
    const LUNA: &str = "did:spacekit:user:luna";
    const KAI: &str = "did:spacekit:user:kai";

    #[tokio::test]
    async fn send_direct_envelope_broadcasts_to_recipient_sse_filter() {
        let state = GatewayState::new(16);
        let mut rx = state.event_tx.subscribe();

        let resp = send_envelope(
            &state,
            EnvelopeRequest {
                message: chat(ASTOR, "hello"),
                conversation_type: Some("direct".to_string()),
                recipient_did: Some(LUNA.to_string()),
                recipient_dids: vec![],
                group_id: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(resp.0.status, "ok");
        assert!(!resp.0.message_id.is_empty());

        let payload = rx.try_recv().unwrap();
        assert!(payload_matches_did(&payload, LUNA));
        assert!(payload.contains("hello"));
        let history = state.history_for_did(LUNA).await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["message_id"], resp.0.message_id);
    }

    #[tokio::test]
    async fn pq_key_register_and_lookup() {
        let state = GatewayState::new(4);
        register_pq_key(
            &state,
            RegisterKeyRequest {
                did: LUNA.to_string(),
                public_key: "abc123".to_string(),
                algorithm: Some("kyber1024".to_string()),
            },
        )
        .await
        .unwrap();

        let got = get_pq_key(&state, LUNA).await.unwrap();
        assert_eq!(got["publicKey"], "abc123");
    }

    #[tokio::test]
    async fn group_create_list_join() {
        let state = GatewayState::new(8);
        let group = create_group(&state, create("Team", ASTOR, "public", None))
            .await
            .unwrap();
        assert_eq!(group.version, 1);

        let listed = list_groups(&state, Some(LUNA)).await;
        assert_eq!(listed["groups"].as_array().unwrap().len(), 1);

        join_group(&state, &group.id, LUNA).await.unwrap();
        let updated = get_group(&state, &group.id).await.unwrap();
        assert!(updated.is_member(LUNA));
        assert_eq!(updated.version, 2);
    }

    #[tokio::test]
    async fn ad_hoc_group_envelope_includes_listed_recipients() {
        let state = GatewayState::new(8);
        let mut rx = state.event_tx.subscribe();
        send_envelope(
            &state,
            EnvelopeRequest {
                message: chat(ASTOR, "group hi"),
                conversation_type: Some("group".to_string()),
                recipient_did: None,
                recipient_dids: vec![LUNA.to_string(), ASTOR.to_string()],
                group_id: Some("grp:test".to_string()),
            },
        )
        .await
        .unwrap();
        let payload = rx.try_recv().unwrap();
        assert!(payload_matches_did(&payload, LUNA));
    }

    #[tokio::test]
    async fn known_group_envelopes_go_to_members_and_only_members_post() {
        let state = GatewayState::new(16);
        let group = create_group(&state, create("Team", ASTOR, "public", None))
            .await
            .unwrap();
        join_group(&state, &group.id, LUNA).await.unwrap();
        let (_, event) = send_envelope(
            &state,
            EnvelopeRequest {
                message: chat(ASTOR, "hi team"),
                conversation_type: None,
                recipient_did: None,
                // Ignored for a known group: members are the audience.
                recipient_dids: vec![KAI.to_string()],
                group_id: Some(group.id.clone()),
            },
        )
        .await
        .unwrap();
        let participants: Vec<&str> = event["participants"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(participants.contains(&LUNA));
        assert!(!participants.contains(&KAI));

        let err = send_envelope(
            &state,
            EnvelopeRequest {
                message: chat(KAI, "let me in"),
                conversation_type: None,
                recipient_did: None,
                recipient_dids: vec![],
                group_id: Some(group.id.clone()),
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, GatewayError::Forbidden(_)));

        // A non-member's envelope arriving from the network is dropped.
        let spoof = json!({
            "type": "message", "group_id": group.id, "sender": { "did": KAI },
            "content": "x", "participants": [ASTOR, LUNA],
        });
        assert!(!state.ingest_remote(spoof).await);
    }

    #[tokio::test]
    async fn create_group_with_fixed_id() {
        let state = GatewayState::new(4);
        let id = "grp:fixed123".to_string();
        let group = create_group_with_id(&state, Some(id.clone()), create("Fixed", ASTOR, "public", None))
            .await
            .unwrap();
        assert_eq!(group.id, id);
        assert!(create_group_with_id(&state, Some(id), create("Again", ASTOR, "public", None))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn visibility_rules() {
        let state = GatewayState::new(16);
        assert!(create_group(&state, create("x", ASTOR, "paid", None)).await.is_err());
        assert!(create_group(&state, create("x", ASTOR, "public", Some("l"))).await.is_err());
        assert!(create_group(&state, create("x", ASTOR, "secret", None)).await.is_err());

        let private = create_group(&state, create("Inner", ASTOR, "private", None))
            .await
            .unwrap();
        assert!(matches!(
            join_group(&state, &private.id, LUNA).await,
            Err(GatewayError::Forbidden(_))
        ));
        assert_eq!(list_groups(&state, Some(LUNA)).await["groups"].as_array().unwrap().len(), 0);
        invite_to_group(&state, &private.id, LUNA).await.unwrap();
        assert_eq!(list_groups(&state, Some(LUNA)).await["groups"].as_array().unwrap().len(), 1);

        let paid = create_group(&state, create("Premium", ASTOR, "paid", Some("channel:astor")))
            .await
            .unwrap();
        assert!(matches!(
            join_group(&state, &paid.id, LUNA).await,
            Err(GatewayError::Forbidden(_))
        ));
        // Paid channels are discoverable.
        assert_eq!(list_groups(&state, None).await["groups"].as_array().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn paid_channel_admits_valid_entitlements_and_drops_lapsed_ones() {
        let state = GatewayState::new(32);
        let paid = create_group(&state, create("Premium", ASTOR, "paid", Some("channel:astor")))
            .await
            .unwrap();
        let valid = Arc::new(std::sync::Mutex::new(true));
        let flag = valid.clone();
        let verifier = EntitlementVerifier::from_fn(move |req: VerifyRequest| {
            let ok = *flag.lock().unwrap();
            async move {
                if req.file_id.starts_with("grp:") && ok {
                    EntitlementStatus::Valid
                } else {
                    EntitlementStatus::Expired
                }
            }
        });
        let ent = "11".repeat(32);
        let pk = "22".repeat(32);

        // Not configured / malformed / wrong channel.
        assert!(matches!(
            join_paid_group(&state, &paid.id, LUNA, &ent, &pk, None).await,
            Err(GatewayError::Unavailable(_))
        ));
        assert!(matches!(
            join_paid_group(&state, &paid.id, LUNA, "zz", &pk, Some(&verifier)).await,
            Err(GatewayError::BadRequest(_))
        ));

        // Valid: admitted through a join request handled by the creator's node.
        let req = GroupJoinRequest {
            group_id: paid.id.clone(),
            did: LUNA.to_string(),
            action: "join".into(),
            entitlement_id: Some(ent.clone()),
            buyer_pk_hash: Some(pk.clone()),
            nonce: 1,
        };
        let changed = handle_join_request(&state, ASTOR, LUNA, &req, Some(&verifier))
            .await
            .unwrap()
            .expect("membership changed");
        assert!(changed.is_member(LUNA));
        assert!(state.subscription(&paid.id, LUNA).await.is_some());
        // The same signed request again is a replay.
        assert!(matches!(
            handle_join_request(&state, ASTOR, LUNA, &req, Some(&verifier)).await,
            Err(GatewayError::BadRequest(_))
        ));
        // Someone joining on another's behalf is refused.
        let mut forged = req.clone();
        forged.nonce = 9;
        assert!(handle_join_request(&state, ASTOR, KAI, &forged, Some(&verifier)).await.is_err());

        // A comped member is never re-checked.
        invite_to_group(&state, &paid.id, KAI).await.unwrap();

        // Still valid: nothing changes.
        assert!(revalidate_paid_members(&state, ASTOR, &verifier).await.is_empty());
        // Subscription lapses: removed, everyone (including the ex-member) is told.
        let mut rx = state.event_tx.subscribe();
        *valid.lock().unwrap() = false;
        let changed = revalidate_paid_members(&state, ASTOR, &verifier).await;
        assert_eq!(changed.len(), 1);
        assert!(!changed[0].is_member(LUNA));
        assert!(changed[0].is_member(KAI));
        assert!(state.subscription(&paid.id, LUNA).await.is_none());
        let event = rx.try_recv().unwrap();
        assert!(payload_matches_did(&event, LUNA));
        assert!(event.contains("\"type\":\"group\""));
    }

    #[tokio::test]
    async fn replicas_accept_only_newer_announcements_from_the_creator() {
        let owner = GatewayState::new(16);
        let replica = GatewayState::new(16);
        let group = create_group(&owner, create("Team", ASTOR, "public", None))
            .await
            .unwrap();
        assert!(apply_remote_group(&replica, LUNA, ASTOR, group.clone()).await);
        // Replayed or stale.
        assert!(!apply_remote_group(&replica, LUNA, ASTOR, group.clone()).await);
        // Someone other than the creator.
        let mut forged = group.clone();
        forged.version = 99;
        forged.member_dids.push(KAI.into());
        assert!(!apply_remote_group(&replica, LUNA, KAI, forged).await);
        // Our own group is never overwritten from outside.
        assert!(!apply_remote_group(&owner, ASTOR, ASTOR, group.clone()).await);

        join_group(&owner, &group.id, LUNA).await.unwrap();
        let v2 = get_group(&owner, &group.id).await.unwrap();
        assert!(apply_remote_group(&replica, LUNA, ASTOR, v2).await);
        assert!(get_group(&replica, &group.id).await.unwrap().is_member(LUNA));

        let tomb = delete_group_announced(&owner, &group.id).await.unwrap();
        assert!(tomb.deleted);
        assert!(apply_remote_group(&replica, LUNA, ASTOR, tomb).await);
        assert!(get_group(&replica, &group.id).await.is_err());
        assert_eq!(list_groups(&replica, Some(LUNA)).await["groups"].as_array().unwrap().len(), 0);
    }

    /// A replica admits paid and public joiners itself (after checking the
    /// chain), applies admins' operations, and serves signed backfill.
    #[tokio::test]
    async fn replicas_admit_without_the_creator_and_follow_admins() {
        let owner = GatewayState::new(32);
        let replica = GatewayState::new(32);
        let paid = create_group(&owner, create("Premium", ASTOR, "paid", Some("chan:a")))
            .await
            .unwrap();
        let public = create_group(&owner, create("Open", ASTOR, "public", None)).await.unwrap();
        let mut paid_with_admin = set_admin(&owner, &paid.id, KAI, true).await.unwrap();
        assert!(paid_with_admin.is_admin(KAI));
        for g in [paid_with_admin.clone(), public.clone()] {
            assert!(apply_remote_group(&replica, LUNA, ASTOR, g).await);
        }
        let verifier = EntitlementVerifier::from_fn(|req: VerifyRequest| async move {
            if req.publisher_did == ASTOR && req.listing_id == "chan:a" {
                EntitlementStatus::Valid
            } else {
                EntitlementStatus::WrongListing
            }
        });
        let join = |group: &str, did: &str, nonce: u64, ent: bool| GroupJoinRequest {
            group_id: group.into(),
            did: did.into(),
            action: "join".into(),
            entitlement_id: ent.then(|| "11".repeat(32)),
            buyer_pk_hash: ent.then(|| "22".repeat(32)),
            nonce,
        };
        // The creator is offline; the replica (Luna's node) admits Luna itself.
        assert!(handle_join_request(&replica, LUNA, LUNA, &join(&paid.id, LUNA, 1, true), Some(&verifier))
            .await
            .unwrap()
            .is_none());
        assert!(replica.effective_members(&paid.id).await.unwrap().contains(&LUNA.to_string()));
        assert!(handle_join_request(&replica, LUNA, LUNA, &join(&paid.id, "x", 2, true), Some(&verifier)).await.is_err());
        // Without an entitlement, no.
        assert!(handle_join_request(&replica, LUNA, "did:z", &join(&paid.id, "did:z", 1, false), Some(&verifier)).await.is_err());
        // Public: as is.
        handle_join_request(&replica, LUNA, "did:z", &join(&public.id, "did:z", 2, false), None).await.unwrap();
        assert!(replica.effective_members(&public.id).await.unwrap().contains(&"did:z".to_string()));

        // Luna can now post, and Astor's node accepts Luna's message once
        // Astor's node has admitted her too (it checks the same entitlement).
        let msg = json!({"type":"message","message_id":"m1","group_id":paid.id,"sender":{"did":LUNA},"content":"x","created_at":"2026-01-01T00:00:01Z","participants":[]});
        assert!(!owner.ingest_remote(msg.clone()).await, "not yet admitted on the owner's node");
        handle_join_request(&owner, ASTOR, LUNA, &join(&paid.id, LUNA, 1, true), Some(&verifier)).await.unwrap();
        assert!(owner.ingest_remote(msg.clone()).await);
        assert!(!owner.ingest_remote(msg).await, "a second copy is a replay");

        // An admin (Kai) removes Luna: every node applies it at once.
        let remove = GroupJoinRequest { action: "remove".into(), ..join(&paid.id, LUNA, 5, false) };
        assert!(handle_join_request(&replica, LUNA, LUNA, &remove, None).await.is_err(), "not an admin");
        handle_join_request(&replica, LUNA, KAI, &remove, None).await.unwrap();
        assert!(!replica.effective_members(&paid.id).await.unwrap().contains(&LUNA.to_string()));
        let changed = handle_join_request(&owner, ASTOR, KAI, &remove, None).await.unwrap().unwrap();
        assert!(changed.is_banned(LUNA) && !changed.is_member(LUNA));
        // Banned: rejoining is refused everywhere once the replica has the
        // creator's new version.
        paid_with_admin = changed;
        assert!(apply_remote_group(&replica, LUNA, ASTOR, paid_with_admin).await);
        assert!(handle_join_request(&replica, LUNA, LUNA, &join(&paid.id, LUNA, 6, true), Some(&verifier)).await.is_err());
    }

    #[tokio::test]
    async fn backfill_serves_signed_originals_to_members_only() {
        let state = GatewayState::new(32);
        let sender = crate::identity::NodeIdentity::generate();
        let group = create_group(&state, create("Team", ASTOR, "public", None)).await.unwrap();
        join_group(&state, &group.id, &sender.did).await.unwrap();
        let payload = json!({"type":"message","message_id":"m1","group_id":group.id,"sender":{"did":sender.did},
            "content":"hi","created_at":"2026-01-01T00:00:02Z","participants":[ASTOR, sender.did]});
        let signed = sender.sign(payload.to_string());
        assert!(state.ingest_remote_signed(payload, Some(signed.clone())).await);
        assert_eq!(state.backfill_for(ASTOR, "2026-01-01T00:00:01Z", 10).await, vec![signed]);
        assert!(state.backfill_for(ASTOR, "2026-01-01T00:00:03Z", 10).await.is_empty());
        assert!(state.backfill_for(KAI, "", 10).await.is_empty(), "not a member");
        assert_eq!(state.latest_seen().await.unwrap(), "2026-01-01T00:00:02Z");
    }

    #[tokio::test]
    async fn leave_and_peer_binding_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway.json");
        let state = GatewayState::with_store(16, path.clone()).unwrap();
        let group = create_group(&state, create("Team", ASTOR, "public", None))
            .await
            .unwrap();
        join_group(&state, &group.id, LUNA).await.unwrap();
        let leave = GroupJoinRequest {
            group_id: group.id.clone(),
            did: LUNA.into(),
            action: "leave".into(),
            entitlement_id: None,
            buyer_pk_hash: None,
            nonce: 2,
        };
        let changed = handle_join_request(&state, ASTOR, LUNA, &leave, None).await.unwrap().unwrap();
        assert!(!changed.is_member(LUNA));
        assert!(remove_member(&state, &group.id, ASTOR).await.is_err());

        assert!(state.bind_peer(LUNA, "peer-a").await);
        assert!(state.bind_peer(LUNA, "peer-a").await);
        assert!(!state.bind_peer(LUNA, "peer-b").await);

        let reloaded = GatewayState::with_store(16, path).unwrap();
        assert_eq!(get_group(&reloaded, &group.id).await.unwrap(), changed);
        assert!(!reloaded.bind_peer(LUNA, "peer-b").await);
        assert_eq!(reloaded.owned_groups(ASTOR).await.len(), 1);
    }
}
