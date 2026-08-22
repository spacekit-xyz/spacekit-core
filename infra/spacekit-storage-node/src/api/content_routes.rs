//! Content monetization HTTP hooks (settlement inbox for SpaceKit Pay).

#![deny(clippy::all)]

use std::convert::Infallible;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use warp::{Filter, Rejection, Reply};

use crate::content_settlement::{ContentSettlementStore, SettlementReceipt};
use crate::storage_facade::Facade;

#[derive(Debug, Deserialize)]
pub struct PushSettlementRequest {
    pub tx_hash: String,
    pub amount: String,
    #[serde(default = "default_asset")]
    pub asset: String,
    pub payer_did: String,
    pub beneficiary_did: String,
    pub scope: String,
    #[serde(default)]
    pub settled_at: Option<i64>,
}

fn default_asset() -> String {
    "ASTRA".to_string()
}

#[derive(Debug, Serialize)]
pub struct PushSettlementResponse {
    pub ok: bool,
    pub inbox_appended: bool,
    pub already_processed: bool,
}

fn with_facade(
    facade: Arc<Facade>,
) -> impl Filter<Extract = (Arc<Facade>,), Error = Infallible> + Clone {
    warp::any().map(move || facade.clone())
}

fn settlement_secret_ok(headers: warp::http::HeaderMap) -> bool {
    let expected = match std::env::var("SPACEKIT_CONTENT_SETTLEMENT_SECRET") {
        Ok(s) if !s.trim().is_empty() => s,
        _ => return true,
    };
    headers
        .get("x-spacekit-settlement-secret")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == expected)
        .unwrap_or(false)
}

pub fn build_routes(
    facade: Arc<Facade>,
) -> impl Filter<Extract = (Box<dyn Reply>,), Error = Rejection> + Clone {
    let push = warp::path!("api" / "content" / "settlements")
        .and(warp::post())
        .and(with_facade(facade))
        .and(warp::header::headers_cloned())
        .and(warp::body::json())
        .and_then(handle_push_settlement);

    push.map(|reply| -> Box<dyn Reply> { Box::new(reply) })
}

async fn handle_push_settlement(
    facade: Arc<Facade>,
    headers: warp::http::HeaderMap,
    req: PushSettlementRequest,
) -> Result<warp::reply::Response, Rejection> {
    if !settlement_secret_ok(headers) {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "unauthorized"})),
            warp::http::StatusCode::UNAUTHORIZED,
        )
        .into_response());
    }
    let data_dir = facade
        .cas_data_dir()
        .ok_or_else(|| warp::reject::not_found())?;
    let store = ContentSettlementStore::new(data_dir);
    if store.is_inbox_processed(&req.tx_hash) {
        return Ok(warp::reply::json(&PushSettlementResponse {
            ok: true,
            inbox_appended: false,
            already_processed: true,
        })
        .into_response());
    }
    let receipt = SettlementReceipt {
        tx_hash: req.tx_hash,
        amount: req.amount,
        asset: req.asset,
        payer_did: req.payer_did,
        beneficiary_did: req.beneficiary_did,
        scope: req.scope,
        settled_at: req
            .settled_at
            .unwrap_or_else(|| chrono::Utc::now().timestamp()),
    };
    store
        .push_settlement_inbox(&receipt)
        .map_err(|_| warp::reject::reject())?;
    Ok(warp::reply::json(&PushSettlementResponse {
        ok: true,
        inbox_appended: true,
        already_processed: false,
    })
    .into_response())
}
