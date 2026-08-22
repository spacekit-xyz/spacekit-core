//! DID-aware access evaluation for CAS blobs and FactPackages (Stream A).
//!
//! Operators configure enforcement via [`BlobFactAuthMode`] on [`crate::api::ServerConfig`]
//! or env `SPACEKIT_BLOB_FACT_AUTH` (`permissive` | `strict` | `hybrid`).

#![deny(clippy::all)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use spacekit_primitives::v1::fact::{
    AccessCondition, AccessPolicy, AttributeRequirements, ConditionType, FactPackage,
};
use spacekit_primitives::v1::identity::QuantumDID;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How `/blobs` and `/facts` enforce DID identity and FactPackage policies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlobFactAuthMode {
    /// Legacy behaviour: blobs/facts do not require `Authorization: DID`.
    #[default]
    Permissive,
    /// All blob/fact reads and writes require a DID; facts enforce policy + author match on write.
    Strict,
    /// Facts use strict rules; blobs require DID only on write (read stays open).
    Hybrid,
}

impl BlobFactAuthMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Permissive => "permissive",
            Self::Strict => "strict",
            Self::Hybrid => "hybrid",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "strict" => Some(Self::Strict),
            "hybrid" => Some(Self::Hybrid),
            "permissive" => Some(Self::Permissive),
            _ => None,
        }
    }

    pub fn from_env() -> Self {
        std::env::var("SPACEKIT_BLOB_FACT_AUTH")
            .ok()
            .and_then(|s| Self::parse(&s))
            .unwrap_or(Self::Permissive)
    }

    pub fn facts_require_did(self) -> bool {
        matches!(self, Self::Strict | Self::Hybrid)
    }

    pub fn blobs_require_did_on_read(self) -> bool {
        matches!(self, Self::Strict)
    }

    pub fn blobs_require_did_on_write(self) -> bool {
        matches!(self, Self::Strict | Self::Hybrid)
    }
}

/// Optional DID → role assignments for [`AccessPolicy::RoleBased`].
pub type RoleAssignments = HashMap<String, HashSet<String>>;

/// Evaluate read access for a stored fact's policy.
pub fn fact_allows_reader(policy: &AccessPolicy, requester_did: &str, author_did: &str) -> bool {
    fact_allows_reader_with_roles(policy, requester_did, author_did, None, unix_now())
}

/// Same as [`fact_allows_reader`] with optional role registry and explicit clock.
pub fn fact_allows_reader_with_roles(
    policy: &AccessPolicy,
    requester_did: &str,
    author_did: &str,
    roles: Option<&RoleAssignments>,
    now: u64,
) -> bool {
    if requester_did == author_did {
        return true;
    }
    let Ok(requester) = QuantumDID::parse(requester_did) else {
        return false;
    };
    match policy {
        AccessPolicy::Public => true,
        AccessPolicy::Private(authorized) => authorized.contains(&requester),
        AccessPolicy::RoleBased(required_roles) => {
            if let Some(map) = roles {
                if let Some(have) = map.get(requester_did) {
                    return required_roles.iter().any(|r| have.contains(r));
                }
            }
            required_roles.iter().any(|r| r == requester_did)
        }
        AccessPolicy::AttributeBased(req) => attribute_allows(req, requester_did),
        AccessPolicy::Dynamic(_) => false,
        AccessPolicy::Conditional(conditions) => conditions
            .iter()
            .any(|c| eval_access_condition(c, requester_did, author_did, now, None)),
    }
}

fn attribute_allows(req: &AttributeRequirements, requester_did: &str) -> bool {
    if let Some(min) = req.minimum_trust_score {
        if min > 0.0 {
            return false;
        }
    }
    for (key, value) in &req.required_attributes {
        if key == "did" && value == requester_did {
            return true;
        }
    }
    req.required_attributes.is_empty()
}

fn eval_access_condition(
    condition: &AccessCondition,
    requester_did: &str,
    author_did: &str,
    now: u64,
    data_dir: Option<&Path>,
) -> bool {
    match condition.condition_type {
        ConditionType::TimeWindow => time_window_allows(&condition.parameters, now),
        ConditionType::TrustLevel => {
            if condition
                .parameters
                .get("subscription_required")
                .map(|s| s == "true")
                .unwrap_or(false)
            {
                let channel = condition
                    .parameters
                    .get("channel_id")
                    .map(String::as_str)
                    .unwrap_or("");
                return crate::content_grants::ContentGrantStore::from_env_or_data_dir(
                    data_dir.unwrap_or_else(|| Path::new(".")),
                )
                .has_channel_subscription(requester_did, channel);
            }
            let min = condition
                .parameters
                .get("minimum")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            min <= 0.0 || requester_did == author_did
        }
        ConditionType::ReputationThreshold => {
            let min = condition
                .parameters
                .get("minimum")
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            min <= 0.0
        }
        ConditionType::PaymentRequired => {
            let content_id = condition
                .parameters
                .get("content_id")
                .map(String::as_str)
                .unwrap_or("");
            crate::content_access::payment_grant_satisfied(data_dir, requester_did, content_id)
        }
        ConditionType::LocationBased
        | ConditionType::DeviceType
        | ConditionType::NetworkCondition
        | ConditionType::MultiFactor => false,
    }
}

fn time_window_allows(params: &HashMap<String, String>, now: u64) -> bool {
    let start = params
        .get("not_before")
        .or_else(|| params.get("start"))
        .and_then(|s| s.parse::<u64>().ok());
    let end = params
        .get("not_after")
        .or_else(|| params.get("end"))
        .and_then(|s| s.parse::<u64>().ok());
    match (start, end) {
        (Some(s), Some(e)) => now >= s && now <= e,
        (Some(s), None) => now >= s,
        (None, Some(e)) => now <= e,
        (None, None) => true,
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// POST /facts: caller must match package author when enforcement is on.
pub fn fact_post_allowed(author_did: &str, requester_did: &str) -> bool {
    author_did == requester_did
}

/// Canonical bytes for SPHINCS+ verification (matches [`crate::fact_storage`]).
pub fn create_fact_verification_message(fact: &FactPackage) -> Result<Vec<u8>> {
    let mut message = Vec::new();
    message.extend_from_slice(&fact.fact_id);
    message.extend_from_slice(&fact.metadata.checksum);
    message.extend_from_slice(&serde_json::to_vec(&fact.author)?);
    message.extend_from_slice(&fact.created_at.to_le_bytes());
    Ok(message)
}

/// In `strict` mode, facts must carry a non-empty signature.
pub fn fact_requires_signature(mode: BlobFactAuthMode) -> bool {
    matches!(mode, BlobFactAuthMode::Strict)
}

/// Verify SPHINCS+ when the node has quantum support configured.
pub async fn verify_fact_signature(
    crypto: &Arc<crate::quantum::QuantumCrypto>,
    fact: &FactPackage,
) -> Result<bool> {
    if fact.signature.signature_bytes.is_empty() {
        return Ok(false);
    }
    let message = create_fact_verification_message(fact)?;
    crypto
        .verify_signature(&message, &fact.signature, &fact.author)
        .await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlobRefEntry {
    fact_id: String,
    author_did: String,
    policy: AccessPolicy,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BlobRefManifest {
    refs: Vec<BlobRefEntry>,
}

fn blob_ref_path(data_dir: &Path, hash: &str) -> PathBuf {
    let prefix = &hash[..2.min(hash.len())];
    data_dir
        .join("blob_refs")
        .join(prefix)
        .join(format!("{hash}.json"))
}

/// Record that a fact references a CAS blob (used on fact ingest).
pub async fn register_blob_ref(
    data_dir: &Path,
    blob_hash: &str,
    fact_id_hex: &str,
    author_did: &str,
    policy: &AccessPolicy,
) -> Result<()> {
    let path = blob_ref_path(data_dir, blob_hash);
    let mut manifest = if path.exists() {
        let raw = tokio::fs::read(&path).await?;
        serde_json::from_slice(&raw).unwrap_or_default()
    } else {
        BlobRefManifest::default()
    };
    if !manifest.refs.iter().any(|r| r.fact_id == fact_id_hex) {
        manifest.refs.push(BlobRefEntry {
            fact_id: fact_id_hex.to_string(),
            author_did: author_did.to_string(),
            policy: policy.clone(),
        });
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, serde_json::to_vec_pretty(&manifest)?).await?;
    Ok(())
}

/// Register all tree hashes from a repo commit fact.
pub async fn register_commit_tree_refs(
    data_dir: &Path,
    fact_id_hex: &str,
    author_did: &str,
    policy: &AccessPolicy,
    tree: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    for hash in tree.values() {
        if hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
            register_blob_ref(data_dir, hash, fact_id_hex, author_did, policy).await?;
        }
    }
    Ok(())
}

/// Strict-mode blob read: authenticated DID must be allowed by at least one referencing fact policy.
pub async fn blob_allows_reader(
    data_dir: &Path,
    blob_hash: &str,
    requester_did: &str,
) -> Result<bool> {
    let path = blob_ref_path(data_dir, blob_hash);
    if !path.exists() {
        // Orphan CAS object: any authenticated reader may fetch (content-addressed upload path).
        return Ok(true);
    }
    let raw = tokio::fs::read(&path).await?;
    let manifest: BlobRefManifest = serde_json::from_slice(&raw)?;
    if manifest.refs.is_empty() {
        return Ok(true);
    }
    Ok(manifest
        .refs
        .iter()
        .any(|r| fact_allows_reader(&r.policy, requester_did, &r.author_did)))
}
