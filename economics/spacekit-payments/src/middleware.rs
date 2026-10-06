//! Warp middleware for pay-per-request routes, priced in ASTRA.
//!
//! A request without an `X-PAYMENT` header gets `402 Payment Required` with
//! the price as a `PaymentRequirement`. The client pays with an ASTRA
//! transfer on the SpaceKit chain and retries with `X-PAYMENT: <tx hash>`.
//! The gate checks that transaction on the chain (`PaymentVerifier`) and
//! hands the receipt to the route. Each transaction pays for one request.

use crate::types::*;
use crate::verify::PaymentVerifier;
use std::sync::Arc;
use tracing::warn;
use warp::http::StatusCode;
use warp::{Filter, Rejection, Reply};

/// Price and payee of a gated route.
#[derive(Debug, Clone)]
pub struct PaymentGate {
    /// Price in ASTRA wei.
    pub price_wei: u128,
    /// Address that must receive the payment.
    pub pay_to: String,
    /// Chain id the payment must be made on.
    pub chain_id: Option<String>,
    /// Human-readable description of what is being bought.
    pub description: String,
}

impl PaymentGate {
    pub fn requirement(&self) -> PaymentRequirement {
        PaymentRequirement {
            amount_wei: self.price_wei.to_string(),
            asset: PaymentAsset::ASTRA,
            pay_to: self.pay_to.clone(),
            chain_id: self.chain_id.clone(),
            description: Some(self.description.clone()),
        }
    }
}

/// A warp filter that serves the route only to requests that paid.
pub fn require_payment(
    gate: PaymentGate,
    verifier: Arc<PaymentVerifier>,
) -> impl Filter<Extract = (PaymentReceipt,), Error = Rejection> + Clone {
    let requirement = Arc::new(gate.requirement());
    warp::header::optional::<String>("x-payment").and_then(move |header: Option<String>| {
        let requirement = requirement.clone();
        let verifier = verifier.clone();
        async move {
            let Some(tx_hash) = header else {
                return Err(warp::reject::custom(PaymentRequired {
                    requirement: (*requirement).clone(),
                }));
            };
            verifier.verify(tx_hash.trim(), &requirement).map_err(|e| {
                warn!("payment {tx_hash} refused: {e}");
                warp::reject::custom(PaymentFailed {
                    reason: e.to_string(),
                })
            })
        }
    })
}

/// Rejection: no payment was offered.
#[derive(Debug)]
pub struct PaymentRequired {
    pub requirement: PaymentRequirement,
}
impl warp::reject::Reject for PaymentRequired {}

/// Rejection: the offered payment does not check out.
#[derive(Debug)]
pub struct PaymentFailed {
    pub reason: String,
}
impl warp::reject::Reject for PaymentFailed {}

/// Turns payment rejections into `402` responses.
pub async fn handle_payment_rejection(err: Rejection) -> Result<impl Reply, Rejection> {
    if let Some(pr) = err.find::<PaymentRequired>() {
        let json = warp::reply::json(&serde_json::json!({
            "error": "payment required",
            "accepts": [pr.requirement],
        }));
        return Ok(warp::reply::with_status(json, StatusCode::PAYMENT_REQUIRED));
    }
    if let Some(pf) = err.find::<PaymentFailed>() {
        let json = warp::reply::json(&serde_json::json!({ "error": pf.reason }));
        return Ok(warp::reply::with_status(json, StatusCode::PAYMENT_REQUIRED));
    }
    Err(err)
}
