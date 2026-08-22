//! Warp middleware for x402 payment gating.
//!
//! Wraps warp routes so that requests without a valid `X-PAYMENT` header
//! receive a 402 response with payment requirements. Requests with a valid
//! header are verified via the facilitator and the resulting credit is
//! injected into the request context.

use crate::fee_router::FeeRouter;
use crate::types::*;
use crate::x402::build_402_body;
use std::sync::Arc;
use tracing::warn;
use warp::http::StatusCode;
use warp::{Filter, Rejection, Reply};

/// Payment gate configuration for a specific route.
#[derive(Debug, Clone)]
pub struct PaymentGate {
    /// Price in USDC for this endpoint.
    pub price_usdc: String,
    /// Human-readable description of what's being purchased.
    pub description: String,
    /// DID of the contract/service that receives the credit.
    pub beneficiary_did: String,
}

/// Create a warp filter that enforces x402 payment on a route.
///
/// If the `X-PAYMENT` header is missing, returns 402 with payment requirements.
/// If present, verifies via the facilitator and injects a `Credit` into the
/// request for downstream handlers to use.
pub fn require_payment(
    gate: PaymentGate,
    config: PaymentConfig,
    fee_router: Arc<FeeRouter>,
) -> impl Filter<Extract = (Credit,), Error = Rejection> + Clone {
    let gate = Arc::new(gate);
    let config = Arc::new(config);

    warp::header::optional::<String>("x-payment").and_then(move |payment_header: Option<String>| {
        let gate = gate.clone();
        let config = config.clone();
        let fee_router = fee_router.clone();

        async move {
            match payment_header {
                None => {
                    // No payment header → return 402
                    Err(warp::reject::custom(PaymentRequired {
                        body: build_402_body(&config, &gate.price_usdc, Some(&gate.description)),
                    }))
                }
                Some(header) => {
                    // Verify payment via facilitator
                    let requirement = PaymentRequirement {
                        amount: gate.price_usdc.clone(),
                        asset: PaymentAsset::USDC,
                        pay_to: config.pay_to_address.clone(),
                        network: Some(PaymentNetwork::select(config.testnet)),
                        description: Some(gate.description.clone()),
                    };

                    #[cfg(feature = "x402")]
                    {
                        match crate::x402::verify_payment(
                            &config.facilitator_url,
                            &header,
                            &requirement,
                        )
                        .await
                        {
                            Ok(receipt) => {
                                match fee_router
                                    .process_payment(receipt, &gate.beneficiary_did)
                                    .await
                                {
                                    Ok(credit) => Ok(credit),
                                    Err(e) => {
                                        warn!("Fee routing failed: {}", e);
                                        Err(warp::reject::custom(PaymentFailed {
                                            reason: format!("Credit application failed: {}", e),
                                        }))
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Payment verification failed: {}", e);
                                Err(warp::reject::custom(PaymentFailed {
                                    reason: format!("Verification failed: {}", e),
                                }))
                            }
                        }
                    }
                    #[cfg(not(feature = "x402"))]
                    {
                        Err(warp::reject::custom(PaymentFailed {
                            reason: "x402 feature not enabled".to_string(),
                        }))
                    }
                }
            }
        }
    })
}

/// Rejection type: 402 Payment Required.
#[derive(Debug)]
pub struct PaymentRequired {
    pub body: String,
}
impl warp::reject::Reject for PaymentRequired {}

/// Rejection type: payment verification or credit application failed.
#[derive(Debug)]
pub struct PaymentFailed {
    pub reason: String,
}
impl warp::reject::Reject for PaymentFailed {}

/// Recovery handler that converts payment rejections into proper HTTP responses.
pub async fn handle_payment_rejection(err: Rejection) -> Result<impl Reply, Rejection> {
    if let Some(pr) = err.find::<PaymentRequired>() {
        let json = warp::reply::json(
            &serde_json::from_str::<serde_json::Value>(&pr.body).unwrap_or_default(),
        );
        return Ok(warp::reply::with_status(json, StatusCode::PAYMENT_REQUIRED));
    }
    if let Some(pf) = err.find::<PaymentFailed>() {
        let json = warp::reply::json(&serde_json::json!({ "error": pf.reason }));
        return Ok(warp::reply::with_status(json, StatusCode::PAYMENT_REQUIRED));
    }
    Err(err)
}
