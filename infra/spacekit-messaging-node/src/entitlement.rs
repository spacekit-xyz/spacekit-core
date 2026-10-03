//! Entitlement checks for paid channels.
//!
//! A paid channel is a gateway group with `visibility = "paid"` and a
//! `listing_id` on the entitlement-ledger contract, created by the group's
//! creator with `file_id` = the group id. A subscriber is admitted when:
//!
//! 1. `OP_VERIFY_LISTING` says the entitlement is valid *for that listing*
//!    (buyer DID, Kyber key hash, not expired, not revoked); and
//! 2. `OP_GET_LISTING` shows the listing was created by the group's creator
//!    for this group.
//!
//! Both are needed: anyone can create a listing under any unused id or for
//! any file id, so a check by file id or listing id alone can be satisfied by
//! a listing the channel owner never made.
//!
//! The checks are read-only contract calls to a compute node:
//! `POST {compute}/api/contracts/{contract}/call` with the raw call data,
//! answered with the raw return data.

use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use futures::FutureExt;

pub const OP_GET_LISTING: u8 = 0x05;
pub const OP_VERIFY_LISTING: u8 = 0x08;

/// Status of one entitlement, as the ledger reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntitlementStatus {
    Valid,
    Expired,
    WrongBuyer,
    WrongFile,
    Revoked,
    WrongPk,
    /// The entitlement is for another listing, or the listing was not made
    /// by the channel's creator for this channel.
    WrongListing,
    /// The check could not be made (network, node or contract error). Not a
    /// verdict: callers keep existing members and refuse new ones.
    Unavailable(String),
}

impl EntitlementStatus {
    pub fn from_byte(b: u8) -> Self {
        match b {
            1 => Self::Valid,
            0 => Self::Expired,
            2 => Self::WrongBuyer,
            3 => Self::WrongFile,
            4 => Self::Revoked,
            5 => Self::WrongPk,
            6 => Self::WrongListing,
            other => Self::Unavailable(format!("unknown status {other}")),
        }
    }

    pub fn is_valid(&self) -> bool {
        matches!(self, Self::Valid)
    }

    /// The ledger answered, and the answer is no.
    pub fn is_denied(&self) -> bool {
        !matches!(self, Self::Valid | Self::Unavailable(_))
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Valid => "valid",
            Self::Expired => "expired",
            Self::WrongBuyer => "wrong_buyer",
            Self::WrongFile => "wrong_channel",
            Self::Revoked => "revoked",
            Self::WrongPk => "wrong_public_key",
            Self::WrongListing => "wrong_listing",
            Self::Unavailable(_) => "unavailable",
        }
    }
}

/// One entitlement to check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyRequest {
    pub entitlement_id: [u8; 32],
    pub buyer_did: String,
    /// The channel's listing.
    pub listing_id: String,
    /// Who must have created the listing (the channel's creator).
    pub publisher_did: String,
    /// What the listing must be for (the channel's group id).
    pub file_id: String,
    pub buyer_pk_hash: [u8; 32],
}

type VerifyFn = dyn Fn(VerifyRequest) -> BoxFuture<'static, EntitlementStatus> + Send + Sync;

/// Checks entitlements. Cheap to clone.
#[derive(Clone)]
pub struct EntitlementVerifier {
    verify: Arc<VerifyFn>,
}

impl std::fmt::Debug for EntitlementVerifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EntitlementVerifier")
    }
}

impl EntitlementVerifier {
    /// A verifier backed by any async function (tests, other transports).
    pub fn from_fn<F, Fut>(f: F) -> Self
    where
        F: Fn(VerifyRequest) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = EntitlementStatus> + Send + 'static,
    {
        Self {
            verify: Arc::new(move |req| f(req).boxed()),
        }
    }

    /// Read-only calls to `{compute_url}/api/contracts/{contract}/call`.
    pub fn http(compute_url: &str, contract: &str) -> Self {
        let base = compute_url.trim_end_matches('/').to_string();
        let contract = contract.trim().to_string();
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();
        Self::from_fn(move |req: VerifyRequest| {
            let url = format!("{base}/api/contracts/{contract}/call");
            let http = http.clone();
            async move {
                let call = |body: Vec<u8>| {
                    let http = http.clone();
                    let url = url.clone();
                    async move {
                        let resp = http
                            .post(&url)
                            .header("content-type", "application/octet-stream")
                            .body(body)
                            .send()
                            .await
                            .map_err(|e| e.to_string())?;
                        if !resp.status().is_success() {
                            let status = resp.status();
                            let text = resp.text().await.unwrap_or_default();
                            return Err(format!("HTTP {status}: {text}"));
                        }
                        resp.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
                    }
                };
                let status = match call(build_verify_listing_payload(&req)).await {
                    Ok(bytes) => parse_verify_output(&bytes),
                    Err(e) => return EntitlementStatus::Unavailable(e),
                };
                if !status.is_valid() {
                    return status;
                }
                match call(build_get_listing_payload(&req.listing_id)).await {
                    Ok(bytes) => listing_matches(&bytes, &req),
                    Err(e) => EntitlementStatus::Unavailable(e),
                }
            }
        })
    }

    /// `SPACEKIT_COMPUTE_NODE_URL` and `SPACEKIT_ENTITLEMENT_CONTRACT_ID`
    /// (the same variables the storage node uses). `None` if either is unset.
    pub fn from_env() -> Option<Self> {
        let url = std::env::var("SPACEKIT_COMPUTE_NODE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let contract = std::env::var("SPACEKIT_ENTITLEMENT_CONTRACT_ID")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        Some(Self::http(&url, &contract))
    }

    pub async fn verify(&self, req: VerifyRequest) -> EntitlementStatus {
        (self.verify)(req).await
    }
}

fn append_string(buf: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    buf.extend_from_slice(&(b.len() as u16).to_le_bytes());
    buf.extend_from_slice(b);
}

/// `[OP_VERIFY_LISTING][entitlement_id:32][buyer_did:str][listing_id:str][buyer_pk_hash:32]`,
/// strings as `[len:u16le][utf8]`.
pub fn build_verify_listing_payload(req: &VerifyRequest) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 32 + 4 + req.buyer_did.len() + req.listing_id.len() + 32);
    out.push(OP_VERIFY_LISTING);
    out.extend_from_slice(&req.entitlement_id);
    append_string(&mut out, &req.buyer_did);
    append_string(&mut out, &req.listing_id);
    out.extend_from_slice(&req.buyer_pk_hash);
    out
}

pub fn build_get_listing_payload(listing_id: &str) -> Vec<u8> {
    let mut out = vec![OP_GET_LISTING];
    append_string(&mut out, listing_id);
    out
}

fn read_string(data: &[u8], pos: &mut usize) -> Option<String> {
    let len = u16::from_le_bytes(data.get(*pos..*pos + 2)?.try_into().ok()?) as usize;
    *pos += 2;
    let s = std::str::from_utf8(data.get(*pos..*pos + len)?).ok()?.to_string();
    *pos += len;
    Some(s)
}

/// `[1][publisher_did:str][file_id:str]…` (OP_GET_LISTING): the listing must be
/// the channel creator's, for this channel.
pub fn listing_matches(bytes: &[u8], req: &VerifyRequest) -> EntitlementStatus {
    let Some((&1, record)) = bytes.split_first() else {
        return EntitlementStatus::WrongListing;
    };
    let mut pos = 0;
    match (read_string(record, &mut pos), read_string(record, &mut pos)) {
        (Some(publisher), Some(file)) if publisher == req.publisher_did && file == req.file_id => {
            EntitlementStatus::Valid
        }
        (Some(_), Some(_)) => EntitlementStatus::WrongListing,
        _ => EntitlementStatus::Unavailable("malformed listing record".into()),
    }
}

pub fn parse_verify_output(bytes: &[u8]) -> EntitlementStatus {
    match bytes {
        [1, status, ..] => EntitlementStatus::from_byte(*status),
        _ => EntitlementStatus::Unavailable("malformed verify response".into()),
    }
}

/// Parse 32 bytes of hex (with or without `0x`).
pub fn parse_hash32(s: &str) -> Option<[u8; 32]> {
    let bytes = hex::decode(s.trim().trim_start_matches("0x")).ok()?;
    bytes.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_payload_layout_and_listing_check() {
        let req = VerifyRequest {
            entitlement_id: [0xAA; 32],
            buyer_did: "did:x".into(),
            listing_id: "chan:1".into(),
            publisher_did: "did:pub".into(),
            file_id: "grp:1".into(),
            buyer_pk_hash: [0xBB; 32],
        };
        let p = build_verify_listing_payload(&req);
        assert_eq!(p[0], OP_VERIFY_LISTING);
        assert_eq!(&p[1..33], &[0xAA; 32]);
        assert_eq!(&p[33..35], &5u16.to_le_bytes());
        assert_eq!(&p[35..40], b"did:x");
        assert_eq!(&p[40..42], &6u16.to_le_bytes());
        assert_eq!(&p[42..48], b"chan:1");
        assert_eq!(&p[48..], &[0xBB; 32]);
        assert_eq!(parse_verify_output(&[1, 1]), EntitlementStatus::Valid);
        assert_eq!(parse_verify_output(&[1, 6]), EntitlementStatus::WrongListing);
        assert!(!parse_verify_output(&[0]).is_denied());

        let record = |publisher: &str, file: &str| {
            let mut r = vec![1u8];
            append_string(&mut r, publisher);
            append_string(&mut r, file);
            r.extend_from_slice(&[0u8; 16]);
            r
        };
        assert_eq!(listing_matches(&record("did:pub", "grp:1"), &req), EntitlementStatus::Valid);
        assert_eq!(listing_matches(&record("did:squatter", "grp:1"), &req), EntitlementStatus::WrongListing);
        assert_eq!(listing_matches(&record("did:pub", "grp:2"), &req), EntitlementStatus::WrongListing);
        assert!(parse_hash32(&format!("0x{}", "ab".repeat(32))).is_some());
        assert!(parse_hash32("abcd").is_none());
    }
}
