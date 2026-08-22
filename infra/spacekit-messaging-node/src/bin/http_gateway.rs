//! HTTP gateway for SpaceKit Messaging Node.
//! Browser-facing envelope + SSE + groups (simulator-compatible).

use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use axum::response::sse::{Event, KeepAlive, Sse};
use spacekit_messaging_node::gateway::{
    self, CreateGroupRequest, EnvelopeRequest, GatewayError, GatewayState, JoinGroupRequest,
    RegisterKeyRequest,
};
use spacekit_messaging_node::{MessagingConfig, MessagingNode};

#[derive(Clone)]
struct AppState {
    gateway: Arc<GatewayState>,
    node: Arc<MessagingNode>,
    local_did: String,
    history_token: String,
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

fn gateway_status(err: &GatewayError) -> StatusCode {
    match err {
        GatewayError::BadRequest(_) => StatusCode::BAD_REQUEST,
        GatewayError::NotFound(_) => StatusCode::NOT_FOUND,
        GatewayError::Forbidden(_) => StatusCode::FORBIDDEN,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let http_listen: SocketAddr = std::env::var("SPACEKIT_MESSAGING_HTTP_LISTEN")
        .unwrap_or_else(|_| "0.0.0.0:3031".to_string())
        .parse()
        .unwrap();

    let messaging_config = match std::env::var("SPACEKIT_MESSAGING_CONFIG") {
        Ok(path) => MessagingConfig::from_file(&path)?,
        Err(_) => MessagingConfig::default(),
    };
    messaging_config.validate()?;
    let local_did = messaging_config.node_did.clone();
    let history_token = messaging_config.private_key.clone();
    let node = Arc::new(MessagingNode::new(messaging_config).await?);
    node.start().await?;
    let gateway = Arc::new(GatewayState::new(1000));
    let state = AppState {
        gateway: gateway.clone(),
        node: node.clone(),
        local_did,
        history_token,
    };
    let mut received = node.subscribe_gateway_envelopes();
    tokio::spawn(async move {
        while let Ok(payload) = received.recv().await {
            gateway.ingest(payload).await;
        }
    });
    let cors = CorsLayer::new().allow_origin(Any).allow_headers(Any);

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/messages/envelope", post(handle_envelope))
        .route("/api/messages/stream", get(stream_messages))
        .route("/api/messages/history", get(message_history))
        .route("/api/messages/register-key", post(register_pq_key))
        .route("/api/messages/keys/:did", get(get_pq_key))
        .route("/api/messages/groups", post(create_group).get(list_groups))
        .route("/api/messages/groups/:id", get(get_group))
        .route("/api/messages/groups/:id/join", post(join_group))
        .route("/api/messages/groups/:id/invite", post(invite_to_group))
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

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let status = state.node.get_status().await;
    Json(health_payload(&state.local_did, &status))
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
    state
        .node
        .publish_gateway_envelope(
            response.message_id.clone(),
            state.local_did.clone(),
            recipient_dids,
            event,
        )
        .await
        .map_err(|error| (StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?;
    Ok(Json(response))
}

async fn message_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let expected = format!("Bearer {}", state.history_token);
    if headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(expected.as_str())
    {
        return Err((
            StatusCode::UNAUTHORIZED,
            "valid node authorization is required for message history".into(),
        ));
    }
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

async fn create_group(
    State(state): State<AppState>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<gateway::GroupInfo>, (StatusCode, String)> {
    gateway::create_group(&state.gateway, req)
        .await
        .map(Json)
        .map_err(|e| (gateway_status(&e), e.to_string()))
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
) -> Result<Json<gateway::GroupInfo>, (StatusCode, String)> {
    gateway::get_group(&state.gateway, &id)
        .await
        .map(Json)
        .map_err(|e| (gateway_status(&e), e.to_string()))
}

async fn join_group(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<JoinGroupRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    gateway::join_group(&state.gateway, &id, &req.did)
        .await
        .map(Json)
        .map_err(|e| (gateway_status(&e), e.to_string()))
}

async fn invite_to_group(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<JoinGroupRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    gateway::invite_to_group(&state.gateway, &id, &req.did)
        .await
        .map(Json)
        .map_err(|e| (gateway_status(&e), e.to_string()))
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
}
