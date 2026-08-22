//! In-process HTTP gateway logic for browser clients (Hermes / website messaging UI).
//! Matches the SpaceKit simulator contract: envelope POST + SSE broadcast, groups CRUD, PQ keys.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub creator_did: String,
    pub description: String,
    pub visibility: String,
    pub member_dids: Vec<String>,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct GatewayState {
    pub event_tx: broadcast::Sender<String>,
    pub pq_keys: Arc<Mutex<HashMap<String, (String, String)>>>,
    pub groups: Arc<Mutex<HashMap<String, GroupInfo>>>,
    history: Arc<Mutex<VecDeque<Value>>>,
    history_capacity: usize,
}

impl GatewayState {
    pub fn new(event_capacity: usize) -> Self {
        let (event_tx, _) = broadcast::channel(event_capacity);
        Self {
            event_tx,
            pq_keys: Arc::new(Mutex::new(HashMap::new())),
            groups: Arc::new(Mutex::new(HashMap::new())),
            history: Arc::new(Mutex::new(VecDeque::with_capacity(event_capacity))),
            history_capacity: event_capacity.max(1),
        }
    }

    /// Store and broadcast an envelope received from another process.
    pub async fn ingest(&self, payload: Value) {
        let mut history = self.history.lock().await;
        while history.len() >= self.history_capacity {
            history.pop_front();
        }
        history.push_back(payload.clone());
        drop(history);
        let _ = self.event_tx.send(payload.to_string());
    }

    /// Return the bounded receive history visible to one DID.
    pub async fn history_for_did(&self, did: &str) -> Vec<Value> {
        self.history
            .lock()
            .await
            .iter()
            .filter(|payload| payload_matches_did(&payload.to_string(), did))
            .cloned()
            .collect()
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
}

fn default_public() -> String {
    "public".to_string()
}

#[derive(Debug, Deserialize)]
pub struct JoinGroupRequest {
    pub did: String,
}

#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
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

    let mut participants = req.recipient_dids;
    if let Some(single) = req.recipient_did.clone() {
        if !participants.contains(&single) {
            participants.push(single);
        }
    }
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
    state
        .pq_keys
        .lock()
        .await
        .insert(req.did.clone(), (req.public_key.clone(), alg.clone()));
    Ok(json!({ "status": "ok", "did": req.did, "algorithm": alg }))
}

pub async fn get_pq_key(state: &GatewayState, did: &str) -> Result<Value, GatewayError> {
    let keys = state.pq_keys.lock().await;
    match keys.get(did) {
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
    if req.name.trim().is_empty() {
        return Err(GatewayError::BadRequest("name required".into()));
    }
    if req.creator_did.trim().is_empty() {
        return Err(GatewayError::BadRequest("creator_did required".into()));
    }

    let id = id.unwrap_or_else(|| format!("grp:{}", Uuid::new_v4().simple()));
    let mut members = req.member_dids;
    if !members.contains(&req.creator_did) {
        members.insert(0, req.creator_did.clone());
    }
    let group = GroupInfo {
        id: id.clone(),
        name: req.name,
        creator_did: req.creator_did,
        description: req.description,
        visibility: req.visibility,
        member_dids: members,
        created_at: Utc::now().to_rfc3339(),
    };
    state.groups.lock().await.insert(id, group.clone());
    Ok(group)
}

pub async fn list_groups(state: &GatewayState, viewer_did: Option<&str>) -> Value {
    let groups = state.groups.lock().await;
    let list: Vec<&GroupInfo> = groups
        .values()
        .filter(|g| {
            if g.visibility == "public" {
                return true;
            }
            if let Some(did) = viewer_did {
                return g.member_dids.iter().any(|m| m == did);
            }
            false
        })
        .collect();
    json!({ "groups": list })
}

pub async fn get_group(state: &GatewayState, group_id: &str) -> Result<GroupInfo, GatewayError> {
    state
        .groups
        .lock()
        .await
        .get(group_id)
        .cloned()
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))
}

pub async fn join_group(
    state: &GatewayState,
    group_id: &str,
    did: &str,
) -> Result<Value, GatewayError> {
    let mut groups = state.groups.lock().await;
    let group = groups
        .get_mut(group_id)
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    if group.visibility == "private" && !group.member_dids.iter().any(|m| m == did) {
        return Err(GatewayError::Forbidden(
            "Private group — invitation required".into(),
        ));
    }
    if !group.member_dids.iter().any(|m| m == did) {
        group.member_dids.push(did.to_string());
    }
    Ok(json!({ "status": "joined", "group_id": group_id }))
}

pub async fn invite_to_group(
    state: &GatewayState,
    group_id: &str,
    did: &str,
) -> Result<Value, GatewayError> {
    let mut groups = state.groups.lock().await;
    let group = groups
        .get_mut(group_id)
        .ok_or_else(|| GatewayError::NotFound("Group not found".into()))?;
    if !group.member_dids.iter().any(|m| m == did) {
        group.member_dids.push(did.to_string());
    }
    Ok(json!({ "status": "invited", "group_id": group_id }))
}

pub async fn delete_group(state: &GatewayState, group_id: &str) -> Result<Value, GatewayError> {
    let mut groups = state.groups.lock().await;
    if groups.remove(group_id).is_none() {
        return Err(GatewayError::NotFound("Group not found".into()));
    }
    Ok(json!({ "status": "deleted", "group_id": group_id }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn send_direct_envelope_broadcasts_to_recipient_sse_filter() {
        let state = GatewayState::new(16);
        let mut rx = state.event_tx.subscribe();

        let resp = send_envelope(
            &state,
            EnvelopeRequest {
                message: Envelope {
                    kind: "chat".to_string(),
                    payload: Value::String("hello".to_string()),
                    context: EnvelopeContext {
                        did: "did:spacekit:user:astor".to_string(),
                        timestamp: 1,
                        source: None,
                    },
                },
                conversation_type: Some("direct".to_string()),
                recipient_did: Some("did:spacekit:user:luna".to_string()),
                recipient_dids: vec![],
                group_id: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(resp.0.status, "ok");
        assert!(!resp.0.message_id.is_empty());

        let payload = rx.try_recv().unwrap();
        assert!(payload_matches_did(&payload, "did:spacekit:user:luna"));
        assert!(payload.contains("hello"));
        let history = state.history_for_did("did:spacekit:user:luna").await;
        assert_eq!(history.len(), 1);
        assert_eq!(history[0]["message_id"], resp.0.message_id);
    }

    #[tokio::test]
    async fn pq_key_register_and_lookup() {
        let state = GatewayState::new(4);
        register_pq_key(
            &state,
            RegisterKeyRequest {
                did: "did:spacekit:user:luna".to_string(),
                public_key: "abc123".to_string(),
                algorithm: Some("kyber1024".to_string()),
            },
        )
        .await
        .unwrap();

        let got = get_pq_key(&state, "did:spacekit:user:luna").await.unwrap();
        assert_eq!(got["publicKey"], "abc123");
    }

    #[tokio::test]
    async fn group_create_list_join() {
        let state = GatewayState::new(4);
        let group = create_group(
            &state,
            CreateGroupRequest {
                name: "Team".to_string(),
                creator_did: "did:spacekit:user:astor".to_string(),
                description: String::new(),
                visibility: "public".to_string(),
                member_dids: vec![],
            },
        )
        .await
        .unwrap();

        let listed = list_groups(&state, Some("did:spacekit:user:luna")).await;
        assert_eq!(listed["groups"].as_array().unwrap().len(), 1);

        join_group(&state, &group.id, "did:spacekit:user:luna")
            .await
            .unwrap();
        let updated = get_group(&state, &group.id).await.unwrap();
        assert!(updated
            .member_dids
            .contains(&"did:spacekit:user:luna".to_string()));
    }

    #[tokio::test]
    async fn group_envelope_includes_all_members() {
        let state = GatewayState::new(8);
        let mut rx = state.event_tx.subscribe();

        send_envelope(
            &state,
            EnvelopeRequest {
                message: Envelope {
                    kind: "chat".to_string(),
                    payload: Value::String("group hi".to_string()),
                    context: EnvelopeContext {
                        did: "did:spacekit:user:astor".to_string(),
                        timestamp: 1,
                        source: None,
                    },
                },
                conversation_type: Some("group".to_string()),
                recipient_did: None,
                recipient_dids: vec![
                    "did:spacekit:user:luna".to_string(),
                    "did:spacekit:user:astor".to_string(),
                ],
                group_id: Some("grp:test".to_string()),
            },
        )
        .await
        .unwrap();

        let payload = rx.try_recv().unwrap();
        assert!(payload_matches_did(&payload, "did:spacekit:user:luna"));
    }

    #[tokio::test]
    async fn create_group_with_fixed_id() {
        let state = GatewayState::new(4);
        let id = "grp:fixed123".to_string();
        let group = create_group_with_id(
            &state,
            Some(id.clone()),
            CreateGroupRequest {
                name: "Fixed".to_string(),
                creator_did: "did:spacekit:user:astor".to_string(),
                description: String::new(),
                visibility: "public".to_string(),
                member_dids: vec![],
            },
        )
        .await
        .unwrap();
        assert_eq!(group.id, id);
    }
}
