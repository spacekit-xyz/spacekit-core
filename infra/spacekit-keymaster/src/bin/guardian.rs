use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use base64::Engine;
use spacekit_keymaster::guardian::GuardianState;
use spacekit_keymaster::pq_crypto::{kem_generate, signer_generate};
use spacekit_keymaster::types::{DecryptRequest, DecryptResponse, GuardianInfo, Hex32};
use tracing_subscriber::EnvFilter;
use zeroize::Zeroizing;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "8781")]
    port: u16,
    #[arg(long, default_value = "meridian")]
    operator: String,
    #[arg(long, env = "KEYMASTER_COORDINATOR_URL", default_value = "http://127.0.0.1:8780")]
    coordinator_url: String,
}

#[derive(serde::Deserialize)]
struct EnrollReq {
    subject: Hex32,
    signer_pk_b64: String,
    keystore_id: Hex32,
    cooldown_s: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();

    let (kem_sk, kem_pk) = kem_generate()?;
    let coord_pk = fetch_coordinator_pk(&args.coordinator_url).await?;
    let endpoint = format!("http://127.0.0.1:{}", args.port);
    let audit_sk = signer_generate()?.0;

    let state = Arc::new(GuardianState::new(
        args.operator,
        endpoint.clone(),
        kem_sk,
        kem_pk,
        coord_pk,
        audit_sk,
    ));

    register_with_coordinator(&args.coordinator_url, &state.info).await?;

    let app = Router::new()
        .route("/v1/guardian/info", get(info))
        .route("/v1/guardian/decrypt", post(decrypt))
        .route("/v1/guardian/admin/enroll", post(enroll))
        .route("/v1/guardian/admin/retire", post(retire))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    tracing::info!("SKKM guardian listening on {endpoint}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn fetch_coordinator_pk(url: &str) -> anyhow::Result<Vec<u8>> {
    let resp: serde_json::Value = reqwest::get(format!("{url}/v1/coordinator/info"))
        .await?
        .json()
        .await?;
    let pk_b64 = resp["coordinator_pk_b64"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing coordinator pk"))?;
    Ok(base64::engine::general_purpose::STANDARD.decode(pk_b64)?)
}

async fn register_with_coordinator(url: &str, info: &GuardianInfo) -> anyhow::Result<()> {
    let _ = reqwest::Client::new()
        .post(format!("{url}/v1/coordinator/guardians/register"))
        .json(info)
        .send()
        .await?;
    Ok(())
}

async fn info(State(s): State<Arc<GuardianState>>) -> Json<GuardianInfo> {
    Json(s.info.clone())
}

async fn decrypt(
    State(s): State<Arc<GuardianState>>,
    Json(req): Json<DecryptRequest>,
) -> Result<Json<DecryptResponse>, (StatusCode, String)> {
    s.decrypt(req)
        .map(Json)
        .map_err(|e| (StatusCode::FORBIDDEN, e.to_string()))
}

async fn enroll(
    State(s): State<Arc<GuardianState>>,
    Json(req): Json<EnrollReq>,
) -> StatusCode {
    s.enroll_subject(req.subject, req.signer_pk_b64, req.keystore_id, req.cooldown_s);
    StatusCode::NO_CONTENT
}

#[derive(serde::Deserialize)]
struct RetireReq {
    keystore_id: Hex32,
}

async fn retire(
    State(s): State<Arc<GuardianState>>,
    Json(req): Json<RetireReq>,
) -> StatusCode {
    s.retire_generation(req.keystore_id);
    StatusCode::NO_CONTENT
}
