//! x402 Payment Protocol Integration
//!
//! Implements the HTTP 402 Payment Required flow:
//! 1. Server returns 402 with `PaymentRequirement` in the body
//! 2. Client pays (EIP-3009 USDC authorization)
//! 3. Client retries with `X-PAYMENT` header
//! 4. Server relays to facilitator for verification + on-chain settlement
//! 5. On success, server processes the request and credits the contract
//!
//! The actual x402 protocol handling (EIP-3009 signing, facilitator relay) is
//! provided by the `x402-rs` / `x402-axum` ecosystem crates. This module
//! provides SpaceKit-specific glue: price tag generation from contract metadata,
//! payment-to-VM-credit conversion, and facilitator response parsing.

use crate::types::*;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

/// Response body for a 402 Payment Required.
/// Serialized as JSON in the HTTP response for x402-compatible clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X402Response {
    pub x402_version: u32,
    pub accepts: Vec<X402PaymentOption>,
}

/// A single payment option within the 402 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X402PaymentOption {
    pub scheme: String,
    pub network: String,
    pub chain_id: u64,
    pub asset: String,
    pub amount: String,
    pub pay_to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl X402Response {
    /// Build a standard USDC-on-Base 402 response from a `PaymentRequirement`.
    pub fn from_requirement(req: &PaymentRequirement) -> Self {
        let network = req.network.unwrap_or(PaymentNetwork::BaseSepolia);
        Self {
            x402_version: 2,
            accepts: vec![X402PaymentOption {
                scheme: "eip3009".to_string(),
                network: network.name().to_string(),
                chain_id: network.chain_id(),
                asset: "USDC".to_string(),
                amount: req.amount.clone(),
                pay_to: req.pay_to.clone(),
                description: req.description.clone(),
            }],
        }
    }
}

/// Parsed payment proof from the `X-PAYMENT` header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X402PaymentProof {
    pub scheme: String,
    pub network: String,
    pub payload: serde_json::Value,
}

/// Result from the facilitator's verification endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacilitatorResult {
    pub success: bool,
    pub tx_hash: Option<String>,
    pub error: Option<String>,
}

/// Verify an x402 payment proof by relaying to the facilitator service.
#[cfg(feature = "x402")]
pub async fn verify_payment(
    facilitator_url: &str,
    payment_header: &str,
    requirement: &PaymentRequirement,
) -> Result<PaymentReceipt> {
    let proof: X402PaymentProof = serde_json::from_str(payment_header)
        .map_err(|e| anyhow::anyhow!("Invalid X-PAYMENT header: {}", e))?;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/verify", facilitator_url.trim_end_matches('/')))
        .json(&serde_json::json!({
            "payment": proof,
            "requirement": {
                "amount": requirement.amount,
                "pay_to": requirement.pay_to,
                "network": requirement.network.map(|n| n.name()),
            },
        }))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Facilitator request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Facilitator returned {}: {}", status, body);
    }

    let result: FacilitatorResult = resp
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Invalid facilitator response: {}", e))?;

    if !result.success {
        anyhow::bail!(
            "Payment verification failed: {}",
            result.error.unwrap_or_default()
        );
    }

    let tx_hash = result.tx_hash.unwrap_or_else(|| "unknown".to_string());
    info!("x402 payment verified: tx={}", tx_hash);

    Ok(PaymentReceipt {
        tx_hash,
        amount: requirement.amount.clone(),
        asset: PaymentAsset::USDC,
        network: requirement.network,
        settled_at: chrono::Utc::now().timestamp(),
    })
}

/// Build a 402 JSON response body for a given price.
pub fn build_402_body(
    config: &PaymentConfig,
    price_usdc: &str,
    description: Option<&str>,
) -> String {
    let req = PaymentRequirement {
        amount: price_usdc.to_string(),
        asset: PaymentAsset::USDC,
        pay_to: config.pay_to_address.clone(),
        network: Some(PaymentNetwork::select(config.testnet)),
        description: description.map(|s| s.to_string()),
    };
    let resp = X402Response::from_requirement(&req);
    serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_402_response_serialization() {
        let req = PaymentRequirement {
            amount: "0.01".to_string(),
            asset: PaymentAsset::USDC,
            pay_to: "0x1234567890123456789012345678901234567890".to_string(),
            network: Some(PaymentNetwork::BaseSepolia),
            description: Some("Contract execution fee".to_string()),
        };
        let resp = X402Response::from_requirement(&req);
        let json = serde_json::to_string_pretty(&resp).unwrap();
        assert!(json.contains("eip3009"));
        assert!(json.contains("84532"));
        assert!(json.contains("0.01"));
    }

    #[test]
    fn test_build_402_body() {
        let config = PaymentConfig {
            pay_to_address: "0xabc".to_string(),
            testnet: true,
            ..Default::default()
        };
        let body = build_402_body(&config, "0.05", Some("test"));
        assert!(body.contains("0.05"));
        assert!(body.contains("0xabc"));
    }
}
