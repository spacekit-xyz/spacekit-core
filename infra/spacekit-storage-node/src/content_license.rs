//! AppLicenseNFT integration for per-content licenses (Sprint 3 / Phase 3 prep).
//!
//! WASM contract (`app_license_nft.rs`) exposes `main` with opcodes:
//! - `OP_MINT` (0x01): mint license; `version` field stores `content_id_hex`
//! - `OP_HAS_LICENSE` (0x02): check ownership for content_id

#![deny(clippy::all)]

use anyhow::{anyhow, Result};

pub const OP_MINT: u8 = 0x01;
pub const OP_HAS_LICENSE: u8 = 0x02;

pub fn license_contract_configured() -> bool {
    std::env::var("SPACEKIT_LICENSE_CONTRACT_ID")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .is_some()
}

pub fn append_string(buf: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    buf.extend_from_slice(&(b.len() as u16).to_le_bytes());
    buf.extend_from_slice(b);
}

pub fn build_mint_payload(owner_did: &str, content_id_hex: &str, price_units: u64) -> Vec<u8> {
    let mut out = vec![OP_MINT];
    append_string(&mut out, owner_did);
    append_string(&mut out, content_id_hex);
    out.extend_from_slice(&price_units.to_le_bytes());
    out
}

pub fn build_has_license_payload(buyer_did: &str, content_id_hex: &str) -> Vec<u8> {
    let mut out = vec![OP_HAS_LICENSE];
    append_string(&mut out, buyer_did);
    append_string(&mut out, content_id_hex);
    out
}

/// Parse mint result: 8-byte token id LE.
pub fn parse_mint_result(bytes: &[u8]) -> Result<u64> {
    if bytes.len() < 8 {
        return Err(anyhow!("mint result too short"));
    }
    Ok(u64::from_le_bytes(bytes[0..8].try_into().unwrap()))
}

/// Parse has_license result: single byte 1 = licensed.
pub fn parse_has_license_result(bytes: &[u8]) -> bool {
    bytes.first() == Some(&1)
}

#[derive(Debug, Clone)]
pub struct LicenseClientConfig {
    pub compute_url: String,
    pub contract_id: String,
}

impl LicenseClientConfig {
    pub fn from_env() -> Option<Self> {
        let compute_url = std::env::var("SPACEKIT_COMPUTE_NODE_URL")
            .or_else(|_| std::env::var("SPACEKIT_COMPUTE_URL"))
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let contract_id = std::env::var("SPACEKIT_LICENSE_CONTRACT_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        Some(Self {
            compute_url: compute_url.trim_end_matches('/').to_string(),
            contract_id,
        })
    }
}

#[cfg(feature = "reqwest")]
pub struct LicenseClient {
    config: LicenseClientConfig,
    http: reqwest::Client,
}

#[cfg(feature = "reqwest")]
impl LicenseClient {
    pub fn new(config: LicenseClientConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn from_env() -> Option<Self> {
        LicenseClientConfig::from_env().map(Self::new)
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
            .map_err(|e| anyhow!("license contract call failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("license HTTP {status}: {body}"));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn mint(
        &self,
        owner_did: &str,
        content_id_hex: &str,
        price_units: u64,
    ) -> Result<u64> {
        let payload = build_mint_payload(owner_did, content_id_hex, price_units);
        let bytes = self.call_contract(payload).await?;
        parse_mint_result(&bytes)
    }

    pub async fn has_license(&self, owner_did: &str, content_id_hex: &str) -> Result<bool> {
        let payload = build_has_license_payload(owner_did, content_id_hex);
        let bytes = self.call_contract(payload).await?;
        Ok(parse_has_license_result(&bytes))
    }
}

/// On-chain AppLicenseNFT check when configured.
#[cfg(feature = "reqwest")]
pub async fn on_chain_has_content_license(requester_did: &str, content_id_hex: &str) -> bool {
    let Some(client) = LicenseClient::from_env() else {
        return false;
    };
    client
        .has_license(requester_did, content_id_hex)
        .await
        .unwrap_or(false)
}

#[cfg(not(feature = "reqwest"))]
pub async fn on_chain_has_content_license(_requester_did: &str, _content_id_hex: &str) -> bool {
    false
}
