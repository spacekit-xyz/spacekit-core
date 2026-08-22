//! Entitlement-ledger and payment-receipt integration for content access (Sprint 2).
//!
//! Listing IDs: `content:{content_id_hex}` (PPV), `channel:{channel_did}` (subscription).
//! File ID in OP_VERIFY matches `content_id_hex` for per-content entitlements.

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const OP_CREATE_LISTING: u8 = 0x01;
pub const OP_PURCHASE: u8 = 0x02;
pub const OP_VERIFY: u8 = 0x03;
pub const OP_REVOKE: u8 = 0x04;
pub const OP_GET_LISTING: u8 = 0x05;
pub const OP_GET_ENTITLEMENT: u8 = 0x06;
/// Publisher-only approve/grant (no payment).
pub const OP_GRANT: u8 = 0x07;

pub const PRICING_ONE_TIME: u8 = 1;
pub const PRICING_SUBSCRIPTION: u8 = 2;

pub const STATUS_VALID: u8 = 1;
pub const STATUS_EXPIRED: u8 = 0;
pub const STATUS_WRONG_BUYER: u8 = 2;
pub const STATUS_WRONG_FILE: u8 = 3;
pub const STATUS_REVOKED: u8 = 4;
pub const STATUS_WRONG_PK: u8 = 5;

/// Decoded entitlement record from OP_GET_ENTITLEMENT.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitlementRecord {
    pub buyer_did: String,
    pub listing_id: String,
    pub granted_at: u64,
    pub expires_at: u64,
    pub status: u8,
    pub buyer_pk_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntitlementVerifyStatus {
    Valid,
    Expired,
    WrongBuyer,
    WrongFile,
    Revoked,
    WrongPk,
    NotFound,
    Unconfigured,
    RpcError(String),
}

impl EntitlementVerifyStatus {
    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

pub fn content_listing_id(content_id_hex: &str) -> String {
    format!("content:{content_id_hex}")
}

pub fn channel_listing_id(channel_did: &str) -> String {
    format!("channel:{channel_did}")
}

pub fn append_string(buf: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    buf.extend_from_slice(&(b.len() as u16).to_le_bytes());
    buf.extend_from_slice(b);
}

pub fn build_create_listing_payload(
    listing_id: &str,
    file_id: &str,
    price: u64,
    token: &str,
    pricing_type: u8,
    period: u64,
) -> Vec<u8> {
    let mut out = vec![OP_CREATE_LISTING];
    append_string(&mut out, listing_id);
    append_string(&mut out, file_id);
    out.extend_from_slice(&price.to_le_bytes());
    append_string(&mut out, token);
    out.push(pricing_type);
    out.extend_from_slice(&period.to_le_bytes());
    out
}

pub fn buyer_pk_hash_from_bytes(pk: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(pk);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn build_purchase_payload(listing_id: &str, buyer_pk_hash: &[u8; 32]) -> Vec<u8> {
    let mut out = vec![OP_PURCHASE];
    append_string(&mut out, listing_id);
    out.extend_from_slice(buyer_pk_hash);
    out
}

/// Build `OP_GRANT` — publisher approves `recipient_did` for a listing (no payment).
pub fn build_grant_payload(
    listing_id: &str,
    recipient_did: &str,
    buyer_pk_hash: &[u8; 32],
) -> Vec<u8> {
    let mut out = vec![OP_GRANT];
    append_string(&mut out, listing_id);
    append_string(&mut out, recipient_did);
    out.extend_from_slice(buyer_pk_hash);
    out
}

pub fn parse_purchase_result(bytes: &[u8]) -> Result<[u8; 32]> {
    if bytes.len() < 33 || bytes[0] != 1 {
        return Err(anyhow!(
            "unexpected purchase result (len={}, first={})",
            bytes.len(),
            bytes.first().copied().unwrap_or(255)
        ));
    }
    let mut id = [0u8; 32];
    id.copy_from_slice(&bytes[1..33]);
    Ok(id)
}

pub fn build_verify_payload(
    entitlement_id: &[u8; 32],
    buyer_did: &str,
    file_id: &str,
    buyer_pk_hash: &[u8; 32],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 32 + 4 + buyer_did.len() + file_id.len() + 32);
    out.push(OP_VERIFY);
    out.extend_from_slice(entitlement_id);
    append_string(&mut out, buyer_did);
    append_string(&mut out, file_id);
    out.extend_from_slice(buyer_pk_hash);
    out
}

pub fn build_get_entitlement_payload(entitlement_id: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(33);
    out.push(OP_GET_ENTITLEMENT);
    out.extend_from_slice(entitlement_id);
    out
}

pub fn parse_entitlement_id_hex(s: &str) -> Result<[u8; 32]> {
    let hex_str = s.trim_start_matches("0x");
    let bytes = hex::decode(hex_str).map_err(|e| anyhow!("invalid entitlement id: {e}"))?;
    if bytes.len() != 32 {
        return Err(anyhow!("entitlement id must be 32 bytes"));
    }
    let mut id = [0u8; 32];
    id.copy_from_slice(&bytes);
    Ok(id)
}

pub fn decode_entitlement_record(raw: &[u8]) -> Result<EntitlementRecord> {
    let mut pos = 0usize;
    let buyer_did = read_string(raw, &mut pos)?;
    let listing_id = read_string(raw, &mut pos)?;
    let granted_at = read_u64(raw, &mut pos)?;
    let expires_at = read_u64(raw, &mut pos)?;
    let status = raw
        .get(pos)
        .copied()
        .ok_or_else(|| anyhow!("truncated entitlement record"))?;
    pos += 1;
    let buyer_pk_hash = if pos + 32 <= raw.len() {
        let mut h = [0u8; 32];
        h.copy_from_slice(&raw[pos..pos + 32]);
        h
    } else {
        [0u8; 32]
    };
    Ok(EntitlementRecord {
        buyer_did,
        listing_id,
        granted_at,
        expires_at,
        status,
        buyer_pk_hash,
    })
}

fn read_string(data: &[u8], pos: &mut usize) -> Result<String> {
    if *pos + 2 > data.len() {
        return Err(anyhow!("truncated string length"));
    }
    let len = u16::from_le_bytes([data[*pos], data[*pos + 1]]) as usize;
    *pos += 2;
    if *pos + len > data.len() {
        return Err(anyhow!("truncated string body"));
    }
    let s = std::str::from_utf8(&data[*pos..*pos + len])?.to_string();
    *pos += len;
    Ok(s)
}

fn read_u64(data: &[u8], pos: &mut usize) -> Result<u64> {
    if *pos + 8 > data.len() {
        return Err(anyhow!("truncated u64"));
    }
    let v = u64::from_le_bytes(data[*pos..*pos + 8].try_into().unwrap());
    *pos += 8;
    Ok(v)
}

pub fn status_from_byte(b: u8) -> EntitlementVerifyStatus {
    match b {
        STATUS_VALID => EntitlementVerifyStatus::Valid,
        STATUS_EXPIRED => EntitlementVerifyStatus::Expired,
        STATUS_WRONG_BUYER => EntitlementVerifyStatus::WrongBuyer,
        STATUS_WRONG_FILE => EntitlementVerifyStatus::WrongFile,
        STATUS_REVOKED => EntitlementVerifyStatus::Revoked,
        STATUS_WRONG_PK => EntitlementVerifyStatus::WrongPk,
        _ => EntitlementVerifyStatus::RpcError(format!("unknown status {b}")),
    }
}

#[derive(Debug, Clone)]
pub struct EntitlementClientConfig {
    pub compute_url: String,
    pub contract_id: String,
}

impl EntitlementClientConfig {
    pub fn from_env() -> Option<Self> {
        let compute_url = std::env::var("SPACEKIT_COMPUTE_NODE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let contract_id = std::env::var("SPACEKIT_ENTITLEMENT_CONTRACT_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        Some(Self {
            compute_url: compute_url.trim_end_matches('/').to_string(),
            contract_id,
        })
    }
}

#[cfg(feature = "reqwest")]
pub struct EntitlementClient {
    config: EntitlementClientConfig,
    http: reqwest::Client,
}

#[cfg(feature = "reqwest")]
impl EntitlementClient {
    pub fn new(config: EntitlementClientConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn from_env() -> Option<Self> {
        EntitlementClientConfig::from_env().map(Self::new)
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
            .map_err(|e| anyhow!("compute call failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("compute HTTP {status}: {body}"));
        }
        Ok(resp.bytes().await?.to_vec())
    }

    pub async fn verify(
        &self,
        entitlement_id: &[u8; 32],
        buyer_did: &str,
        file_id: &str,
        buyer_pk_hash: &[u8; 32],
    ) -> EntitlementVerifyStatus {
        let payload = build_verify_payload(entitlement_id, buyer_did, file_id, buyer_pk_hash);
        match self.call_contract(payload).await {
            Ok(bytes) if bytes.len() >= 2 && bytes[0] == 1 => status_from_byte(bytes[1]),
            Ok(_) => EntitlementVerifyStatus::RpcError("malformed verify response".into()),
            Err(e) => EntitlementVerifyStatus::RpcError(e.to_string()),
        }
    }

    pub async fn get_entitlement(
        &self,
        entitlement_id: &[u8; 32],
    ) -> Result<Option<EntitlementRecord>> {
        let payload = build_get_entitlement_payload(entitlement_id);
        let bytes = self.call_contract(payload).await?;
        if bytes.len() < 2 || bytes[0] != 1 {
            return Ok(None);
        }
        decode_entitlement_record(&bytes[1..]).map(Some)
    }

    pub async fn create_listing(
        &self,
        listing_id: &str,
        file_id: &str,
        price: u64,
        token: &str,
        pricing_type: u8,
        period: u64,
    ) -> Result<()> {
        let payload =
            build_create_listing_payload(listing_id, file_id, price, token, pricing_type, period);
        let bytes = self.call_contract(payload).await?;
        if bytes.first() == Some(&1) {
            Ok(())
        } else {
            Err(anyhow!("create listing failed"))
        }
    }

    pub async fn purchase_listing(
        &self,
        listing_id: &str,
        buyer_pk_hash: &[u8; 32],
    ) -> Result<[u8; 32]> {
        let payload = build_purchase_payload(listing_id, buyer_pk_hash);
        let bytes = self.call_contract(payload).await?;
        parse_purchase_result(&bytes)
    }
}

/// AppLicenseNFT configured via `SPACEKIT_LICENSE_CONTRACT_ID`.
pub fn license_contract_configured() -> bool {
    crate::content_license::license_contract_configured()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnChainGrantCheck {
    pub source: String,
    pub entitlement_id_hex: Option<String>,
    pub listing_id: Option<String>,
    pub expires_at: Option<u64>,
}

/// Resolve on-chain grant for content (entitlement ledger). Returns None if unconfigured or invalid.
#[cfg(feature = "reqwest")]
pub async fn on_chain_content_grant(
    requester_did: &str,
    content_id_hex: &str,
    entitlement_id_hex: Option<&str>,
) -> Option<OnChainGrantCheck> {
    let client = EntitlementClient::from_env()?;
    if let Some(ent_hex) = entitlement_id_hex {
        if let Ok(ent_id) = parse_entitlement_id_hex(ent_hex) {
            let pk_hash = [0u8; 32];
            let status = client
                .verify(&ent_id, requester_did, content_id_hex, &pk_hash)
                .await;
            if status.is_valid() {
                return Some(OnChainGrantCheck {
                    source: "entitlement_ledger".into(),
                    entitlement_id_hex: Some(ent_hex.to_string()),
                    listing_id: Some(content_listing_id(content_id_hex)),
                    expires_at: client
                        .get_entitlement(&ent_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|e| e.expires_at),
                });
            }
        }
    }
    if let Some(client) = crate::content_license::LicenseClient::from_env() {
        if client
            .has_license(requester_did, content_id_hex)
            .await
            .unwrap_or(false)
        {
            return Some(OnChainGrantCheck {
                source: "app_license_nft".into(),
                entitlement_id_hex: None,
                listing_id: Some(content_listing_id(content_id_hex)),
                expires_at: None,
            });
        }
    }

    None
}

#[cfg(not(feature = "reqwest"))]
pub async fn on_chain_content_grant(
    _requester_did: &str,
    _content_id_hex: &str,
    _entitlement_id_hex: Option<&str>,
) -> Option<OnChainGrantCheck> {
    None
}

#[cfg(test)]
mod grant_wire_tests {
    use super::*;

    #[test]
    fn build_grant_payload_layout() {
        let pk = [0xabu8; 32];
        let bytes = build_grant_payload("listing-1", "did:spacekit:alice", &pk);
        assert_eq!(bytes[0], OP_GRANT);
        // listing_id string
        assert_eq!(&bytes[1..3], &(9u16).to_le_bytes());
        assert_eq!(&bytes[3..12], b"listing-1");
        // recipient_did
        let did = b"did:spacekit:alice";
        let did_off = 12;
        assert_eq!(
            &bytes[did_off..did_off + 2],
            &(did.len() as u16).to_le_bytes()
        );
        assert_eq!(&bytes[did_off + 2..did_off + 2 + did.len()], did);
        let pk_off = did_off + 2 + did.len();
        assert_eq!(&bytes[pk_off..pk_off + 32], &pk);
    }
}
