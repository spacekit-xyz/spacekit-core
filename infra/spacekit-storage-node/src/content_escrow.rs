//! astra-escrow integration for content purchases (hold → release on grant, refund on failure).

#![deny(clippy::all)]

use anyhow::{anyhow, Result};

pub const OP_CREATE: u8 = 1;
pub const OP_RELEASE: u8 = 2;
pub const OP_REFUND: u8 = 3;
pub const OP_GET: u8 = 4;

pub const STATUS_OPEN: u8 = 1;
pub const STATUS_RELEASED: u8 = 2;
pub const STATUS_REFUNDED: u8 = 3;

pub fn escrow_contract_configured() -> bool {
    std::env::var("SPACEKIT_ESCROW_CONTRACT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some()
}

pub fn append_string(buf: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    buf.extend_from_slice(&(b.len() as u16).to_le_bytes());
    buf.extend_from_slice(b);
}

/// Escrow id for a pending purchase (stable, unique).
pub fn escrow_id_for_pending(pending_id: &str) -> String {
    format!("content-pending:{pending_id}")
}

pub fn build_create_escrow_payload(
    escrow_id: &str,
    token_contract: &str,
    payer_did: &str,
    payee_did: &str,
    amount_units: u64,
    arbiter_did: &str,
) -> Vec<u8> {
    let mut out = vec![OP_CREATE];
    append_string(&mut out, escrow_id);
    append_string(&mut out, token_contract);
    append_string(&mut out, payer_did);
    append_string(&mut out, payee_did);
    out.extend_from_slice(&amount_units.to_le_bytes());
    append_string(&mut out, arbiter_did);
    out
}

pub fn build_release_payload(escrow_id: &str) -> Vec<u8> {
    let mut out = vec![OP_RELEASE];
    append_string(&mut out, escrow_id);
    out
}

pub fn build_refund_payload(escrow_id: &str) -> Vec<u8> {
    let mut out = vec![OP_REFUND];
    append_string(&mut out, escrow_id);
    out
}

pub fn parse_escrow_get_status(bytes: &[u8]) -> Result<u8> {
    if bytes.is_empty() || bytes[0] != 1 {
        return Err(anyhow!("malformed escrow get response"));
    }
    let status = *bytes
        .last()
        .ok_or_else(|| anyhow!("escrow record missing status"))?;
    Ok(status)
}

#[derive(Debug, Clone)]
pub struct EscrowClientConfig {
    pub compute_url: String,
    pub contract_id: String,
}

impl EscrowClientConfig {
    pub fn from_env() -> Option<Self> {
        let compute_url = std::env::var("SPACEKIT_COMPUTE_NODE_URL")
            .or_else(|_| std::env::var("SPACEKIT_COMPUTE_URL"))
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let contract_id = std::env::var("SPACEKIT_ESCROW_CONTRACT_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        Some(Self {
            compute_url: compute_url.trim_end_matches('/').to_string(),
            contract_id,
        })
    }
}

#[cfg(feature = "reqwest")]
pub struct EscrowClient {
    config: EscrowClientConfig,
    http: reqwest::Client,
}

#[cfg(feature = "reqwest")]
impl EscrowClient {
    pub fn new(config: EscrowClientConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn from_env() -> Option<Self> {
        EscrowClientConfig::from_env().map(Self::new)
    }

    async fn call_contract(&self, payload: Vec<u8>) -> Result<Vec<u8>> {
        let url = format!(
            "{}/api/contracts/{}/call",
            self.config.compute_url, self.config.contract_id
        );
        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(payload)
            .send()
            .await
            .map_err(|e| anyhow!("escrow contract call failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("escrow HTTP {status}: {body}"));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn create_open(
        &self,
        escrow_id: &str,
        payer_did: &str,
        payee_did: &str,
        amount_units: u64,
        arbiter_did: &str,
    ) -> Result<()> {
        let token = std::env::var("SPACEKIT_ESCROW_TOKEN").unwrap_or_else(|_| "ASTRA".into());
        let payload = build_create_escrow_payload(
            escrow_id,
            &token,
            payer_did,
            payee_did,
            amount_units,
            arbiter_did,
        );
        let bytes = self.call_contract(payload).await?;
        if bytes.first() == Some(&1) {
            Ok(())
        } else {
            Err(anyhow!("escrow create failed"))
        }
    }

    pub async fn release(&self, escrow_id: &str) -> Result<()> {
        let bytes = self.call_contract(build_release_payload(escrow_id)).await?;
        if bytes.first() == Some(&1) {
            Ok(())
        } else {
            Err(anyhow!("escrow release failed"))
        }
    }

    pub async fn refund(&self, escrow_id: &str) -> Result<()> {
        let bytes = self.call_contract(build_refund_payload(escrow_id)).await?;
        if bytes.first() == Some(&1) {
            Ok(())
        } else {
            Err(anyhow!("escrow refund failed"))
        }
    }
}
