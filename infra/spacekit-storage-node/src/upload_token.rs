//! Short-lived upload credentials for browser and batch clients (Stream A item 4).
//!
//! Tokens are HMAC-signed payloads: `skut1.<hex(payload)>.<hex(mac)>`.
//! Configure signing material via `SPACEKIT_UPLOAD_TOKEN_SECRET` or
//! `{data_dir}/.upload_token_secret` (32+ bytes recommended).

#![deny(clippy::all)]

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

const TOKEN_PREFIX: &str = "skut1.";
const MAX_TTL_SECONDS: u64 = 86_400;

/// Operations a token may authorize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UploadOp {
    PutBlob,
    GetBlob,
    PutFact,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadTokenClaims {
    /// Acting DID (`sub`).
    pub sub: String,
    pub op: UploadOp,
    /// BLAKE3 hex, fact id, or `*` for any resource of this op.
    pub resource: String,
    pub exp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintUploadTokenRequest {
    pub operation: UploadOp,
    #[serde(default = "default_resource")]
    pub resource: String,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
}

fn default_resource() -> String {
    "*".to_string()
}

fn default_ttl() -> u64 {
    900
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintUploadTokenResponse {
    pub token: String,
    pub expires_at: u64,
    pub operation: UploadOp,
    pub resource: String,
}

fn derive_mac_key(secret: &[u8]) -> [u8; 32] {
    blake3::derive_key("spacekit-upload-token-v1", secret)
}

const UPLOAD_TOKEN_SECRET_FILE: &str = ".upload_token_secret";

/// Normalize operator-provided secret (trim; accept 64-char hex or raw string).
pub fn normalize_secret_bytes(raw: &str) -> Vec<u8> {
    let trimmed = raw.trim();
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(bytes) = hex::decode(trimmed) {
            return bytes;
        }
    }
    trimmed.as_bytes().to_vec()
}

/// Load HMAC secret from env or `{data_dir}/.upload_token_secret`.
pub fn load_signing_secret(data_dir: Option<&Path>) -> Option<Vec<u8>> {
    if let Ok(s) = std::env::var("SPACEKIT_UPLOAD_TOKEN_SECRET") {
        let t = s.trim();
        if !t.is_empty() {
            return Some(normalize_secret_bytes(t));
        }
    }
    let dir = data_dir?;
    let path = dir.join(UPLOAD_TOKEN_SECRET_FILE);
    let raw = std::fs::read_to_string(&path).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(normalize_secret_bytes(t))
    }
}

/// If `SPACEKIT_UPLOAD_TOKEN_SECRET` is set, persist it under `data_dir` for restarts.
pub fn persist_upload_token_secret_from_env(data_dir: &Path) -> std::io::Result<()> {
    let Ok(s) = std::env::var("SPACEKIT_UPLOAD_TOKEN_SECRET") else {
        return Ok(());
    };
    let t = s.trim();
    if t.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(data_dir.join(UPLOAD_TOKEN_SECRET_FILE), t.as_bytes())
}

pub fn mint_upload_token(
    secret: &[u8],
    issuer_did: &str,
    req: &MintUploadTokenRequest,
    now: u64,
) -> Result<MintUploadTokenResponse> {
    if !issuer_did.starts_with("did:") {
        return Err(anyhow!("invalid issuer DID"));
    }
    let ttl = req.ttl_seconds.min(MAX_TTL_SECONDS).max(1);
    let claims = UploadTokenClaims {
        sub: issuer_did.to_string(),
        op: req.operation,
        resource: req.resource.clone(),
        exp: now.saturating_add(ttl),
    };
    let token = sign_claims(secret, &claims)?;
    Ok(MintUploadTokenResponse {
        token,
        expires_at: claims.exp,
        operation: claims.op,
        resource: claims.resource,
    })
}

pub fn sign_claims(secret: &[u8], claims: &UploadTokenClaims) -> Result<String> {
    let payload = serde_json::to_vec(claims)?;
    let mac_key = derive_mac_key(secret);
    let mac = blake3::keyed_hash(&mac_key, &payload);
    Ok(format!(
        "{}{}.{}",
        TOKEN_PREFIX,
        hex::encode(&payload),
        hex::encode(mac.as_bytes())
    ))
}

pub fn verify_upload_token(secret: &[u8], token: &str, now: u64) -> Result<UploadTokenClaims> {
    let rest = token
        .strip_prefix(TOKEN_PREFIX)
        .ok_or_else(|| anyhow!("unknown token prefix"))?;
    let (payload_hex, mac_hex) = rest
        .split_once('.')
        .ok_or_else(|| anyhow!("malformed token"))?;
    let payload = hex::decode(payload_hex).context("payload hex")?;
    let mac = hex::decode(mac_hex).context("mac hex")?;
    let mac_key = derive_mac_key(secret);
    let expected = blake3::keyed_hash(&mac_key, &payload);
    if mac.as_slice() != expected.as_bytes() {
        return Err(anyhow!("invalid token signature"));
    }
    let claims: UploadTokenClaims = serde_json::from_slice(&payload)?;
    if claims.exp < now {
        return Err(anyhow!("token expired"));
    }
    if !claims.sub.starts_with("did:") {
        return Err(anyhow!("invalid token subject"));
    }
    Ok(claims)
}

fn parse_did_header(value: &str) -> Option<String> {
    let raw = if let Some(rest) = value.strip_prefix("DID ") {
        rest.trim()
    } else if let Some(rest) = value.strip_prefix("Bearer ") {
        rest.trim()
    } else {
        return None;
    };
    if raw.starts_with("did:") && raw.len() > 10 {
        Some(raw.to_string())
    } else {
        None
    }
}

fn parse_upload_token_header(value: &str) -> Option<&str> {
    value
        .strip_prefix("UploadToken ")
        .or_else(|| value.strip_prefix("Upload-Token "))
        .map(str::trim)
        .filter(|t| t.starts_with(TOKEN_PREFIX))
}

/// Extract DID from `Authorization` when present (DID or verified upload token).
pub fn optional_requester_did(
    auth: Option<&str>,
    secret: Option<&[u8]>,
    now: u64,
) -> Option<String> {
    let header = auth?;
    if let Some(did) = parse_did_header(header) {
        return Some(did);
    }
    if header.starts_with(TOKEN_PREFIX) {
        let secret = secret?;
        return verify_upload_token(secret, header, now).ok().map(|c| c.sub);
    }
    if let Some(token) = parse_upload_token_header(header) {
        let secret = secret?;
        return verify_upload_token(secret, token, now).ok().map(|c| c.sub);
    }
    None
}

/// Authorize a blob write: DID header or upload token scoped to `PutBlob` + hash.
pub fn authorize_blob_write(
    auth: Option<&str>,
    hash: &str,
    secret: Option<&[u8]>,
    now: u64,
) -> Option<String> {
    let header = auth?;
    if let Some(did) = parse_did_header(header) {
        return Some(did);
    }
    let token = header
        .strip_prefix(TOKEN_PREFIX)
        .map(str::trim)
        .or_else(|| parse_upload_token_header(header))?;
    let secret = secret?;
    let claims = verify_upload_token(secret, token, now).ok()?;
    if claims.op != UploadOp::PutBlob {
        return None;
    }
    if claims.resource != "*" && claims.resource != hash {
        return None;
    }
    Some(claims.sub)
}

/// Authorize a blob read: DID header or upload token scoped to `GetBlob` + hash.
pub fn authorize_blob_read(
    auth: Option<&str>,
    hash: &str,
    secret: Option<&[u8]>,
    now: u64,
) -> Option<String> {
    let header = auth?;
    if let Some(did) = parse_did_header(header) {
        return Some(did);
    }
    let token = header
        .strip_prefix(TOKEN_PREFIX)
        .map(str::trim)
        .or_else(|| parse_upload_token_header(header))?;
    let secret = secret?;
    let claims = verify_upload_token(secret, token, now).ok()?;
    if claims.op != UploadOp::GetBlob {
        return None;
    }
    if claims.resource != "*" && claims.resource != hash {
        return None;
    }
    Some(claims.sub)
}

/// Authorize fact POST via upload token (`PutFact`, resource `*` or fact id placeholder).
pub fn authorize_fact_post(auth: Option<&str>, secret: Option<&[u8]>, now: u64) -> Option<String> {
    let header = auth?;
    if let Some(did) = parse_did_header(header) {
        return Some(did);
    }
    let token = header
        .strip_prefix(TOKEN_PREFIX)
        .map(str::trim)
        .or_else(|| parse_upload_token_header(header))?;
    let secret = secret?;
    let claims = verify_upload_token(secret, token, now).ok()?;
    if claims.op != UploadOp::PutFact {
        return None;
    }
    Some(claims.sub)
}
