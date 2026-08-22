use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Bytes;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use base64::Engine;
use spacekit_keymaster::coordinator::CoordinatorState;
use spacekit_keymaster::registry::RegistryState;
use spacekit_keymaster::types::{
    CoverageStatus, EntitlementStatus, GuardianInfo, Manifest, Placement, SlaQuote, SlaTier,
    StartRecoveryResponse, SubjectIdentity,
};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "8780")]
    port: u16,
    #[arg(long, env = "KEYMASTER_STORAGE_URL", default_value = "http://127.0.0.1:3030")]
    storage_url: String,
}

#[derive(serde::Deserialize)]
struct PlacementsReq {
    count: usize,
}

#[derive(serde::Deserialize)]
struct RecoveryStartReq {
    subject: String,
    keystore_id: String,
}

#[derive(serde::Deserialize)]
struct RetireReq {
    subject: String,
    keystore_id: String,
}

#[derive(serde::Deserialize)]
struct StoragePutReq {
    bytes_b64: String,
    placements: Vec<Placement>,
}

#[derive(serde::Deserialize)]
struct StorageGetReq {
    placements: Vec<Placement>,
}

#[derive(serde::Deserialize)]
struct PayReq {
    quote: SlaQuote,
}

#[derive(serde::Deserialize)]
struct QuoteReq {
    tier: SlaTier,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();

    let registry = RegistryState::new();
    let state = Arc::new(CoordinatorState::new(args.storage_url, registry)?);

    let app = Router::new()
        .route("/v1/coordinator/info", get(coordinator_info))
        .route("/v1/coordinator/subjects/register", post(register_subject))
        .route("/v1/coordinator/manifest", post(put_manifest).get(get_manifest_root))
        .route("/v1/coordinator/manifest/:subject", get(get_manifest))
        .route("/v1/coordinator/placements", post(placements))
        .route("/v1/coordinator/recovery/start", post(recovery_start))
        .route("/v1/coordinator/coverage/:subject", get(coverage))
        .route("/v1/coordinator/audit", get(audit))
        .route("/v1/coordinator/retire", post(retire))
        .route("/v1/coordinator/destroy/:subject", post(destroy))
        .route("/v1/coordinator/storage/put", post(storage_put))
        .route("/v1/coordinator/storage/get", post(storage_get))
        .route("/v1/coordinator/guardians/register", post(register_guardian))
        .route("/v1/coordinator/guardians", get(list_guardians))
        .route("/v1/coordinator/payments/quote/:subject", post(quote))
        .route("/v1/coordinator/payments/pay", post(pay))
        .route("/v1/coordinator/payments/status/:subject", get(payment_status))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    tracing::info!("SKKM coordinator listening on http://127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn coordinator_info(State(s): State<Arc<CoordinatorState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "coordinator_pk_b64": s.coordinator_pk_b64() }))
}

async fn register_subject(
    State(s): State<Arc<CoordinatorState>>,
    Json(id): Json<SubjectIdentity>,
) -> Result<StatusCode, (StatusCode, String)> {
    s.register_subject(id);
    Ok(StatusCode::NO_CONTENT)
}

async fn put_manifest(
    State(s): State<Arc<CoordinatorState>>,
    body: Bytes,
) -> Result<StatusCode, (StatusCode, String)> {
    let raw = std::str::from_utf8(&body).map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    s.put_manifest_raw(raw)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_manifest_root() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

async fn get_manifest(
    State(s): State<Arc<CoordinatorState>>,
    Path(subject): Path<String>,
) -> Result<Json<Manifest>, StatusCode> {
    s.get_manifest(&subject)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

async fn placements(
    State(s): State<Arc<CoordinatorState>>,
    Json(req): Json<PlacementsReq>,
) -> Result<Json<Vec<Vec<Placement>>>, (StatusCode, String)> {
    s.request_placements(req.count)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn recovery_start(
    State(s): State<Arc<CoordinatorState>>,
    Json(req): Json<RecoveryStartReq>,
) -> Result<Json<StartRecoveryResponse>, (StatusCode, String)> {
    s.start_recovery(&req.subject, &req.keystore_id)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn coverage(
    State(s): State<Arc<CoordinatorState>>,
    Path(subject): Path<String>,
) -> Json<CoverageStatus> {
    Json(s.coverage(&subject))
}

async fn audit(State(s): State<Arc<CoordinatorState>>) -> Json<Vec<spacekit_keymaster::types::AuditRecord>> {
    Json(s.audit_log())
}

async fn retire(
    State(s): State<Arc<CoordinatorState>>,
    Json(req): Json<RetireReq>,
) -> Result<StatusCode, (StatusCode, String)> {
    s.retire_generation(&req.subject, &req.keystore_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn destroy(
    State(s): State<Arc<CoordinatorState>>,
    Path(subject): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    s.destroy(&subject)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn storage_put(
    State(s): State<Arc<CoordinatorState>>,
    Json(req): Json<StoragePutReq>,
) -> Result<StatusCode, (StatusCode, String)> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&req.bytes_b64)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    s.put_object(&bytes, &req.placements)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn storage_get(
    State(s): State<Arc<CoordinatorState>>,
    Json(req): Json<StorageGetReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let bytes = s
        .get_object(&req.placements)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "bytes_b64": base64::engine::general_purpose::STANDARD.encode(bytes)
    })))
}

async fn register_guardian(
    State(s): State<Arc<CoordinatorState>>,
    Json(info): Json<GuardianInfo>,
) -> StatusCode {
    s.register_guardian(info);
    StatusCode::NO_CONTENT
}

async fn list_guardians(
    State(s): State<Arc<CoordinatorState>>,
) -> Json<Vec<GuardianInfo>> {
    Json(s.list_guardians())
}

async fn quote(
    State(s): State<Arc<CoordinatorState>>,
    Path(subject): Path<String>,
    Json(req): Json<QuoteReq>,
) -> Result<Json<SlaQuote>, (StatusCode, String)> {
    s.payments()
        .quote(&subject, req.tier)
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

async fn pay(
    State(s): State<Arc<CoordinatorState>>,
    Json(req): Json<PayReq>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let (tx, paid_until) = s
        .payments()
        .pay(&req.quote)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(serde_json::json!({ "tx": tx, "paid_until": paid_until })))
}

async fn payment_status(
    State(s): State<Arc<CoordinatorState>>,
    Path(subject): Path<String>,
) -> Json<EntitlementStatus> {
    Json(s.payments().status(&subject))
}
