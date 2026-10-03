//! HTTP gateway for SpaceKit Messaging Node.
//! Browser-facing envelope + SSE + groups (simulator-compatible).
//!
//! Security model:
//!
//! - **Who the node is.** The node's DID is `did:spacekit:<address>` of its
//!   k256 key (`private_key` in the config). Everything it gossips is signed
//!   with that key; everything it accepts from other nodes must carry a valid
//!   signature by the DID it claims. Unsigned messages from older nodes are
//!   accepted only with `SPACEKIT_MESSAGING_ALLOW_UNSIGNED=1` (then bound to
//!   their libp2p peer on first use, as before).
//! - **Who may use the node.** The HTTP API acts as the node's DID, so every
//!   route except `/health` and public key lookups requires
//!   `Authorization: Bearer <token>` (or `?token=` on the stream, for
//!   EventSource). The token is `SPACEKIT_MESSAGING_API_TOKEN`, or one the
//!   node generates on first start into `<storage>/api-token` (mode 0600).
//! - **Abuse.** Each remote DID is rate limited (sustained 20 messages per
//!   second, bursts of 100); excess is dropped.

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::signal;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

use axum::response::sse::{Event, KeepAlive, Sse};
use spacekit_messaging_node::entitlement::EntitlementVerifier;
use spacekit_messaging_node::gateway::{
    self, CreateGroupRequest, EnvelopeRequest, GatewayError, GatewayState, GroupInfo,
    GroupJoinRequest, JoinGroupRequest, RegisterKeyRequest,
};
use spacekit_messaging_node::network_p2p::P2PMessage;
use spacekit_messaging_node::{GatewayNetworkEvent, MessagingConfig, MessagingNode};

/// Messages served per backfill answer.
const BACKFILL_LIMIT: usize = 200;

#[derive(Clone)]
struct AppState {
    gateway: Arc<GatewayState>,
    node: Arc<MessagingNode>,
    local_did: String,
    api_token: Arc<String>,
    verifier: Option<EntitlementVerifier>,
    allow_unsigned: bool,
}

#[derive(Debug, serde::Deserialize)]
struct StreamQuery {
    did: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct GroupQuery {
    did: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct HistoryQuery {
    did: String,
}

#[derive(Debug, serde::Deserialize)]
struct AdminRequest {
    did: String,
    #[serde(default = "default_true")]
    admin: bool,
}

fn default_true() -> bool {
    true
}

fn gateway_status(err: &GatewayError) -> StatusCode {
    match err {
        GatewayError::BadRequest(_) => StatusCode::BAD_REQUEST,
        GatewayError::NotFound(_) => StatusCode::NOT_FOUND,
        GatewayError::Forbidden(_) => StatusCode::FORBIDDEN,
        GatewayError::Unavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|v| matches!(v.trim(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

/// The API token: from the environment, or generated once into the
/// storage directory (readable only by the owner).
fn load_api_token(storage_dir: &std::path::Path) -> Result<String> {
    if let Ok(t) = std::env::var("SPACEKIT_MESSAGING_API_TOKEN") {
        if t.trim().len() >= 16 {
            return Ok(t.trim().to_string());
        }
        anyhow::bail!("SPACEKIT_MESSAGING_API_TOKEN must be at least 16 characters");
    }
    let path = storage_dir.join("api-token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        if !t.trim().is_empty() {
            return Ok(t.trim().to_string());
        }
    }
    std::fs::create_dir_all(storage_dir)?;
    let mut bytes = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
    let token = hex::encode(bytes);
    std::fs::write(&path, &token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    info!("API token written to {}", path.display());
    Ok(token)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let http_listen: SocketAddr = std::env::var("SPACEKIT_MESSAGING_HTTP_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3031".to_string())
        .parse()
        .unwrap();

    let messaging_config = match std::env::var("SPACEKIT_MESSAGING_CONFIG") {
        Ok(path) => MessagingConfig::from_file(&path)?,
        Err(_) => MessagingConfig::default(),
    };
    messaging_config.validate()?;
    let local_did = messaging_config.node_did.clone();
    let storage_dir = std::path::PathBuf::from(&messaging_config.storage.storage_path);
    let store_path = storage_dir.join("gateway.json");
    let api_token = load_api_token(&storage_dir)?;
    let allow_unsigned = env_flag("SPACEKIT_MESSAGING_ALLOW_UNSIGNED");
    let node = Arc::new(MessagingNode::new(messaging_config).await?);
    if node.identity().is_none() {
        warn!(
            "this node cannot sign: set private_key to a k256 key and node_did to its \
             did:spacekit:<address>; peers will drop its messages"
        );
    }
    node.start().await?;
    let gateway = Arc::new(GatewayState::with_store(1000, store_path)?);
    let verifier = EntitlementVerifier::from_env();
    if verifier.is_none() {
        info!(
            "paid channels: set SPACEKIT_COMPUTE_NODE_URL and SPACEKIT_ENTITLEMENT_CONTRACT_ID \
             to admit subscribers"
        );
    }
    let state = AppState {
        gateway: gateway.clone(),
        node: node.clone(),
        local_did: local_did.clone(),
        api_token: Arc::new(api_token),
        verifier: verifier.clone(),
        allow_unsigned,
    };
    tokio::spawn(network_events(state.clone()));
    tokio::spawn(group_maintenance(state.clone()));
    let cors = CorsLayer::new().allow_origin(Any).allow_headers(Any);

    let protected = Router::new()
        .route("/api/messages/envelope", post(handle_envelope))
        .route("/api/messages/stream", get(stream_messages))
        .route("/api/messages/history", get(message_history))
        .route("/api/messages/register-key", post(register_pq_key))
        .route("/api/messages/groups", post(create_group).get(list_groups))
        .route("/api/messages/groups/:id", get(get_group))
        .route("/api/messages/groups/:id/join", post(join_group))
        .route("/api/messages/groups/:id/invite", post(invite_to_group))
        .route("/api/messages/groups/:id/remove", post(remove_member))
        .route("/api/messages/groups/:id/leave", post(leave_group))
        .route("/api/messages/groups/:id/delete", post(delete_group))
        .route("/api/messages/groups/:id/admins", post(set_admin))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_token));

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/messages/keys/:did", get(get_pq_key))
        .merge(protected)
        .with_state(state)
        .layer(cors);

    info!("HTTP gateway listening on {}", http_listen);
    let listener = tokio::net::TcpListener::bind(http_listen).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = signal::ctrl_c().await;
        })
        .await?;
    node.stop().await?;
    Ok(())
}

/// `Authorization: Bearer <token>`, or `?token=` (EventSource cannot set
/// headers). Compared in constant time.
async fn require_token(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);
    let query = request.uri().query().and_then(|q| {
        q.split('&')
            .find_map(|kv| kv.strip_prefix("token="))
            .map(str::to_string)
    });
    let presented = header.or(query).unwrap_or_default();
    let expected = state.api_token.as_bytes();
    let ok = presented.len() == expected.len()
        && presented
            .as_bytes()
            .iter()
            .zip(expected)
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0;
    if !ok {
        return (StatusCode::UNAUTHORIZED, "a valid API token is required").into_response();
    }
    next.run(request).await
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let status = state.node.get_status().await;
    let mut payload = health_payload(&state.local_did, &status);
    payload["signing"] = serde_json::json!(state.node.identity().is_some());
    Json(payload)
}

fn health_payload(
    local_did: &str,
    status: &spacekit_messaging_node::NodeStatus,
) -> serde_json::Value {
    serde_json::json!({
        "status": if status.is_running { "healthy" } else { "starting" },
        "service": "spacekit-messaging-http",
        "version": env!("CARGO_PKG_VERSION"),
        "did": local_did,
        "p2p_running": status.is_running,
        "peer_count": status.active_connections,
        "messages_received": status.messages_received_today,
    })
}

async fn handle_envelope(
    State(state): State<AppState>,
    Json(req): Json<EnvelopeRequest>,
) -> Result<Json<gateway::EnvelopeResponse>, (StatusCode, String)> {
    if req.message.context.did != state.local_did {
        return Err((
            StatusCode::FORBIDDEN,
            "envelope sender must match this node's admitted DID".into(),
        ));
    }
    let (response, event) = gateway::send_envelope(&state.gateway, req)
        .await
        .map_err(|e| (gateway_status(&e), e.to_string()))?;
    let recipient_dids = event["participants"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|did| did.as_str())
        .filter(|did| *did != state.local_did)
        .map(str::to_owned)
        .collect();
    let signed = state
        .node
        .publish_gateway_envelope(
            response.message_id.clone(),
            state.local_did.clone(),
            recipient_dids,
            event,
        )
        .await
        .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    if let Some(signed) = signed {
        state.gateway.attach_signed(&response.message_id, signed).await;
    }
    Ok(Json(response))
}

async fn message_history(
    State(state): State<AppState>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if query.did != state.local_did {
        return Err((
            StatusCode::FORBIDDEN,
            "history is only available for this node's admitted DID".into(),
        ));
    }
    let messages = state.gateway.history_for_did(&query.did).await;
    Ok(Json(serde_json::json!({ "messages": messages })))
}

async fn register_pq_key(
    State(state): State<AppState>,
    Json(req): Json<RegisterKeyRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if req.did != state.local_did {
        return Err((StatusCode::FORBIDDEN, "keys are registered for this node's DID".into()));
    }
    gateway::register_pq_key(&state.gateway, req)
        .await
        .map(Json)
        .map_err(|e| (gateway_status(&e), e.to_string()))
}

async fn get_pq_key(
    State(state): State<AppState>,
    Path(did): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    gateway::get_pq_key(&state.gateway, &did)
        .await
        .map(Json)
        .map_err(|e| (gateway_status(&e), e.to_string()))
}

type HandlerError = (StatusCode, String);

fn err(e: GatewayError) -> HandlerError {
    (gateway_status(&e), e.to_string())
}

fn forbidden(message: &str) -> HandlerError {
    (StatusCode::FORBIDDEN, message.to_string())
}

/// Announce a group this node owns. Failure to reach peers is logged, not
/// fatal: groups are re-announced periodically.
async fn announce(state: &AppState, group: GroupInfo) {
    if let Err(e) = state.node.publish_gateway_group(group).await {
        warn!("could not announce group: {e}");
    }
}

/// Only this node's own DID may act through this gateway.
fn require_local(state: &AppState, did: &str) -> Result<(), HandlerError> {
    if did == state.local_did {
        Ok(())
    } else {
        Err(forbidden("only this node's admitted DID can act through this gateway"))
    }
}

/// The group, if this node is its creator's node.
async fn owned_group(state: &AppState, id: &str) -> Result<GroupInfo, HandlerError> {
    let group = gateway::get_group(&state.gateway, id).await.map_err(err)?;
    if group.creator_did != state.local_did {
        return Err(forbidden("only the group creator's node can do this"));
    }
    Ok(group)
}

async fn create_group(
    State(state): State<AppState>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<GroupInfo>, HandlerError> {
    require_local(&state, &req.creator_did)?;
    let group = gateway::create_group(&state.gateway, req).await.map_err(err)?;
    announce(&state, group.clone()).await;
    Ok(Json(group))
}

async fn list_groups(
    State(state): State<AppState>,
    Query(q): Query<GroupQuery>,
) -> Json<serde_json::Value> {
    Json(gateway::list_groups(&state.gateway, q.did.as_deref()).await)
}

async fn get_group(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<GroupInfo>, HandlerError> {
    gateway::get_group(&state.gateway, &id)
        .await
        .map(Json)
        .map_err(err)
}

fn request(id: &str, did: &str, action: &str, req: Option<&JoinGroupRequest>) -> GroupJoinRequest {
    GroupJoinRequest {
        group_id: id.to_string(),
        did: did.to_string(),
        action: action.to_string(),
        entitlement_id: req.and_then(|r| r.entitlement_id.clone()),
        buyer_pk_hash: req.and_then(|r| r.buyer_pk_hash.clone()),
        nonce: chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64,
    }
}

/// Apply a request here, then gossip it so every node holding the group
/// applies it too (the creator's node makes it canonical).
async fn apply_and_publish(state: &AppState, req: GroupJoinRequest) -> Result<(), HandlerError> {
    if let Some(updated) = gateway::handle_join_request(
        &state.gateway,
        &state.local_did,
        &state.local_did,
        &req,
        state.verifier.as_ref(),
    )
    .await
    .map_err(err)?
    {
        announce(state, updated).await;
    }
    state
        .node
        .publish_gateway_join(req)
        .await
        .map_err(|e| (StatusCode::SERVICE_UNAVAILABLE, e.to_string()))
}

/// Join a group. This node admits its own user right away where it can
/// (paid: after checking the entitlement on chain; public); other nodes do
/// the same when the signed request reaches them.
async fn join_group(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<JoinGroupRequest>,
) -> Result<Json<serde_json::Value>, HandlerError> {
    require_local(&state, &req.did)?;
    let group = gateway::get_group(&state.gateway, &id).await.map_err(err)?;
    if group.visibility == gateway::VISIBILITY_PAID
        && (req.entitlement_id.is_none() || req.buyer_pk_hash.is_none())
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "a paid channel needs entitlement_id and buyer_pk_hash".into(),
        ));
    }
    if group.visibility == gateway::VISIBILITY_PRIVATE && group.creator_did != state.local_did {
        return Err(forbidden("Private group — invitation required"));
    }
    apply_and_publish(&state, request(&id, &req.did, "join", Some(&req))).await?;
    let members = state.gateway.effective_members(&id).await.unwrap_or_default();
    let joined = members.iter().any(|m| m == &req.did);
    Ok(Json(serde_json::json!({
        "status": if joined { "joined" } else { "pending" },
        "group_id": id,
        "creator_did": group.creator_did,
    })))
}

async fn leave_group(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<JoinGroupRequest>,
) -> Result<Json<serde_json::Value>, HandlerError> {
    require_local(&state, &req.did)?;
    let group = gateway::get_group(&state.gateway, &id).await.map_err(err)?;
    if group.creator_did == state.local_did {
        return Err((
            StatusCode::BAD_REQUEST,
            "the creator cannot leave; delete the group instead".into(),
        ));
    }
    apply_and_publish(&state, request(&id, &req.did, "leave", None)).await?;
    Ok(Json(serde_json::json!({ "status": "left", "group_id": id })))
}

/// Creator or admin: add a member (comped in a paid channel).
async fn invite_to_group(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<JoinGroupRequest>,
) -> Result<Json<serde_json::Value>, HandlerError> {
    let group = gateway::get_group(&state.gateway, &id).await.map_err(err)?;
    if !group.is_admin(&state.local_did) {
        return Err(forbidden("only the creator or an admin can add members"));
    }
    apply_and_publish(&state, request(&id, &req.did, "add", None)).await?;
    Ok(Json(serde_json::json!({ "status": "invited", "group_id": id })))
}

/// Creator or admin: remove (and ban) a member.
async fn remove_member(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<JoinGroupRequest>,
) -> Result<Json<serde_json::Value>, HandlerError> {
    let group = gateway::get_group(&state.gateway, &id).await.map_err(err)?;
    if !group.is_admin(&state.local_did) {
        return Err(forbidden("only the creator or an admin can remove members"));
    }
    apply_and_publish(&state, request(&id, &req.did, "remove", None)).await?;
    Ok(Json(serde_json::json!({ "status": "removed", "group_id": id })))
}

/// Creator: make a DID an admin, or not.
async fn set_admin(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<AdminRequest>,
) -> Result<Json<GroupInfo>, HandlerError> {
    owned_group(&state, &id).await?;
    let group = gateway::set_admin(&state.gateway, &id, &req.did, req.admin)
        .await
        .map_err(err)?;
    announce(&state, group.clone()).await;
    Ok(Json(group))
}

async fn delete_group(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, HandlerError> {
    owned_group(&state, &id).await?;
    let tombstone = gateway::delete_group_announced(&state.gateway, &id)
        .await
        .map_err(err)?;
    announce(&state, tombstone).await;
    Ok(Json(serde_json::json!({ "status": "deleted", "group_id": id })))
}

/// Per-sender token bucket: `RATE` per second, bursts of `BURST`.
struct RateLimiter {
    buckets: HashMap<String, (f64, Instant)>,
}

impl RateLimiter {
    const RATE: f64 = 20.0;
    const BURST: f64 = 100.0;

    fn allow(&mut self, key: &str) -> bool {
        let now = Instant::now();
        if self.buckets.len() > 50_000 {
            self.buckets.retain(|_, (_, t)| now.duration_since(*t).as_secs() < 60);
        }
        let (tokens, last) = self
            .buckets
            .entry(key.to_string())
            .or_insert((Self::BURST, now));
        *tokens = (*tokens + now.duration_since(*last).as_secs_f64() * Self::RATE).min(Self::BURST);
        *last = now;
        if *tokens >= 1.0 {
            *tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Signed messages are accepted; unsigned ones only from legacy peers when
/// allowed (bound to their libp2p peer on first use).
async fn accept(state: &AppState, verified: bool, did: &str, peer: Option<&str>) -> bool {
    if verified {
        return true;
    }
    if !state.allow_unsigned {
        warn!("gateway: dropped unsigned message claiming {did}");
        return false;
    }
    match peer {
        Some(peer) => state.gateway.bind_peer(did, peer).await,
        None => false,
    }
}

/// Traffic from other nodes.
async fn network_events(state: AppState) {
    let mut events = state.node.subscribe_gateway_events();
    let mut limiter = RateLimiter {
        buckets: HashMap::new(),
    };
    loop {
        let event = match events.recv().await {
            Ok(event) => event,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                warn!("gateway: dropped {n} network events (slow consumer)");
                continue;
            }
            Err(_) => return,
        };
        let sender = match &event {
            GatewayNetworkEvent::Envelope { sender_did, .. }
            | GatewayNetworkEvent::Group { sender_did, .. }
            | GatewayNetworkEvent::Join { sender_did, .. }
            | GatewayNetworkEvent::BackfillRequest { sender_did, .. } => sender_did.clone(),
            GatewayNetworkEvent::BackfillResponse { .. } => "backfill".to_string(),
        };
        if !limiter.allow(&sender) {
            warn!("gateway: rate limit: dropped a message from {sender}");
            continue;
        }
        match event {
            GatewayNetworkEvent::Envelope {
                peer,
                sender_did,
                payload,
                verified,
                signed,
            } => {
                let claimed = payload
                    .get("sender")
                    .and_then(|s| s.get("did"))
                    .and_then(|d| d.as_str());
                if claimed != Some(sender_did.as_str())
                    || !accept(&state, verified, &sender_did, Some(&peer)).await
                {
                    continue;
                }
                state.gateway.ingest_remote_signed(payload, signed).await;
            }
            GatewayNetworkEvent::Group {
                peer,
                sender_did,
                group,
                verified,
            } => {
                if !accept(&state, verified, &sender_did, Some(&peer)).await {
                    continue;
                }
                gateway::apply_remote_group(&state.gateway, &state.local_did, &sender_did, group)
                    .await;
            }
            GatewayNetworkEvent::Join {
                peer,
                sender_did,
                request,
                verified,
            } => {
                if !accept(&state, verified, &sender_did, Some(&peer)).await {
                    continue;
                }
                match gateway::handle_join_request(
                    &state.gateway,
                    &state.local_did,
                    &sender_did,
                    &request,
                    state.verifier.as_ref(),
                )
                .await
                {
                    Ok(Some(group)) => announce(&state, group).await,
                    Ok(None) => {}
                    Err(e) => info!(
                        "gateway: {} of {} in {} by {} refused: {e}",
                        request.action, request.did, request.group_id, sender_did
                    ),
                }
            }
            GatewayNetworkEvent::BackfillRequest {
                sender_did,
                since,
                verified,
            } => {
                // Only a signed request: the answer goes to that DID.
                if !verified {
                    continue;
                }
                let messages = state
                    .gateway
                    .backfill_for(&sender_did, &since, BACKFILL_LIMIT)
                    .await;
                if !messages.is_empty() {
                    if let Err(e) = state.node.publish_backfill_response(sender_did, messages).await {
                        warn!("gateway: backfill answer failed: {e}");
                    }
                }
            }
            GatewayNetworkEvent::BackfillResponse { messages, .. } => {
                for signed in messages {
                    // Each message must be an envelope signed by its sender
                    // and addressed to this node's DID.
                    let Ok(P2PMessage::GatewayEnvelope {
                        sender_did,
                        recipient_dids,
                        payload,
                        ..
                    }) = serde_json::from_str::<P2PMessage>(&signed.body)
                    else {
                        continue;
                    };
                    if !signed.signed_by(&sender_did)
                        || !recipient_dids.iter().any(|d| d == &state.local_did)
                        || payload.get("sender").and_then(|s| s.get("did")).and_then(|d| d.as_str())
                            != Some(sender_did.as_str())
                    {
                        continue;
                    }
                    state.gateway.ingest_remote_signed(payload, Some(signed)).await;
                }
            }
        }
    }
}

/// Periodically: drop paid members whose subscription lapsed, re-announce
/// the groups this node owns (gossip has no history, so peers that came
/// online later learn them this way), and ask peers for missed messages.
async fn group_maintenance(state: AppState) {
    let secs = std::env::var("SPACEKIT_MESSAGING_GROUP_REFRESH_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s >= 10)
        .unwrap_or(300);
    // Let the swarm connect before the first round.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(secs));
    loop {
        tick.tick().await;
        if let Some(verifier) = &state.verifier {
            for group in
                gateway::revalidate_paid_members(&state.gateway, &state.local_did, verifier).await
            {
                announce(&state, group).await;
            }
        }
        for group in state.gateway.owned_groups(&state.local_did).await {
            announce(&state, group).await;
        }
        let since = state.gateway.latest_seen().await.unwrap_or_default();
        if let Err(e) = state.node.publish_backfill_request(since).await {
            warn!("gateway: backfill request failed: {e}");
        }
    }
}

async fn stream_messages(
    State(state): State<AppState>,
    Query(query): Query<StreamQuery>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.gateway.event_tx.subscribe();
    let did_filter = query.did;
    let stream = BroadcastStream::new(rx).filter_map(move |message| {
        let did_filter = did_filter.clone();
        match message {
            Ok(payload) => {
                if let Some(did) = did_filter {
                    if !gateway::payload_matches_did(&payload, &did) {
                        return None;
                    }
                }
                Some(Ok(Event::default().data(payload)))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(10)))
}

#[cfg(test)]
mod tests {
    #[test]
    fn health_reports_messaging_gateway() {
        let status = spacekit_messaging_node::NodeStatus {
            is_running: true,
            active_connections: 2,
            active_groups: 0,
            active_direct_conversations: 0,
            registered_users: 0,
            messages_sent_today: 0,
            messages_received_today: 1,
            direct_messages_sent_today: 0,
            direct_messages_received_today: 1,
            started_at: chrono::Utc::now(),
            last_activity: chrono::Utc::now(),
        };
        let response = super::health_payload("did:spacekit:test:node", &status);
        assert_eq!(response["status"], "healthy");
        assert_eq!(response["service"], "spacekit-messaging-http");
        assert_eq!(response["peer_count"], 2);
        assert_eq!(response["messages_received"], 1);
    }

    #[test]
    fn rate_limiter_allows_bursts_then_throttles() {
        let mut l = super::RateLimiter {
            buckets: Default::default(),
        };
        let allowed = (0..150).filter(|_| l.allow("did:x")).count();
        assert!((100..=103).contains(&allowed), "{allowed}");
        assert!(l.allow("did:y"));
    }
}
