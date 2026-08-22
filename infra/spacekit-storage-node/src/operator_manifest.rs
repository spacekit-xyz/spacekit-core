//! Operator discovery manifest (`spacekit:operator:v1`) — Stream E preview.

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
use spacekit_primitives::v1::fact::FactID;
use spacekit_primitives::v1::fact::{
    AccessPolicy, CollectionMethod, DataSource, FactCategory, FactContent, FactMetadata,
    FactPackage, KnowledgeDomain, LicenseType, ProofType, VerificationLevel, VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;

pub const SCHEMA_OPERATOR_V1: &str = "spacekit:operator:v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorManifestContent {
    pub operator_did: String,
    pub display_name: String,
    /// Public HTTP base (no trailing slash), e.g. `http://127.0.0.1:3030`.
    pub storage_http_url: String,
    /// Active blob/fact auth: `permissive` | `hybrid` | `strict`.
    pub blob_fact_auth: String,
    /// URI to operator content policy (Stream D).
    pub content_policy_uri: Option<String>,
    /// e.g. `workspaces`, `sandboxes`, `mcp`, `federation_export`.
    #[serde(default)]
    pub supported_features: Vec<String>,
    pub published_at: u64,
    /// `v1` (HMAC export handoff) and/or `v2` (DID-signed migration manifest).
    #[serde(default = "default_migration_versions")]
    pub supported_migration_versions: Vec<String>,
    #[serde(default)]
    pub did_signature_capable: bool,
    /// Hex-encoded SPHINCS+ public key for migration signature verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sphincs_public_key_hex: Option<String>,
}

fn default_migration_versions() -> Vec<String> {
    vec!["v1".to_string()]
}

pub fn operator_fact_id(operator_did: &str) -> FactID {
    let mut h = Sha256::new();
    h.update(b"spacekit-operator-v1\0");
    h.update(operator_did.as_bytes());
    h.finalize().into()
}

/// `GET /api/operators/self` response envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorSelfResponse {
    pub schema: String,
    pub operator_did: String,
    pub manifest: OperatorManifestContent,
    /// `published_fact` when loaded from CAS; `runtime` when synthesized from node config.
    pub manifest_source: String,
    /// Hex fact id when `manifest_source=published_fact`.
    pub fact_id: Option<String>,
}

pub const SCHEMA_OPERATOR_SELF_V1: &str = "spacekit:operator:self:v1";

/// CAS path for the deterministic operator manifest fact.
pub fn operator_fact_storage_path(
    data_dir: &std::path::Path,
    operator_did: &str,
) -> std::path::PathBuf {
    let fact_id_hex = hex::encode(operator_fact_id(operator_did));
    let prefix = &fact_id_hex[..2.min(fact_id_hex.len())];
    data_dir
        .join("facts")
        .join(prefix)
        .join(format!("{fact_id_hex}.json"))
}

/// Load a published `spacekit:operator:v1` manifest from CAS, if present.
pub async fn load_published_operator_manifest(
    cas_data_dir: &std::path::Path,
    operator_did: &str,
) -> Result<Option<OperatorManifestContent>> {
    let path = operator_fact_storage_path(cas_data_dir, operator_did);
    let raw = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow!("read operator manifest: {e}")),
    };
    let pkg: FactPackage = serde_json::from_slice(&raw)?;
    match &pkg.content {
        FactContent::Json { schema, data } => {
            if schema.as_deref() != Some(SCHEMA_OPERATOR_V1) {
                return Err(anyhow!("fact at operator id is not spacekit:operator:v1"));
            }
            Ok(Some(serde_json::from_value(data.clone())?))
        }
        _ => Err(anyhow!("operator manifest fact is not JSON content")),
    }
}

/// Runtime fallback when no manifest fact has been published yet.
pub fn synthetic_operator_manifest(
    operator_did: &str,
    storage_http_url: String,
    blob_fact_auth: &str,
    upload_tokens_configured: bool,
    handoff_signing_configured: bool,
    migration_signing_configured: bool,
    sphincs_public_key_hex: Option<String>,
) -> OperatorManifestContent {
    let mut features = vec![
        "workspaces".to_string(),
        "sandboxes".to_string(),
        "federation_export".to_string(),
    ];
    if upload_tokens_configured {
        features.push("upload_tokens".to_string());
    }
    if handoff_signing_configured {
        features.push("handoff_signature".to_string());
    }
    let mut versions = vec!["v1".to_string()];
    if migration_signing_configured {
        versions.push("v2".to_string());
    }
    OperatorManifestContent {
        operator_did: operator_did.to_string(),
        display_name: operator_did.to_string(),
        storage_http_url,
        blob_fact_auth: blob_fact_auth.to_string(),
        content_policy_uri: None,
        supported_features: features,
        published_at: chrono::Utc::now().timestamp() as u64,
        supported_migration_versions: versions,
        did_signature_capable: migration_signing_configured,
        sphincs_public_key_hex,
    }
}

pub fn build_operator_fact_package(content: OperatorManifestContent) -> Result<FactPackage> {
    if content.storage_http_url.is_empty() {
        return Err(anyhow!("storage_http_url required"));
    }
    let author =
        QuantumDID::parse(&content.operator_did).map_err(|_| anyhow!("invalid operator DID"))?;
    let fact_id = operator_fact_id(&content.operator_did);
    let content_value = serde_json::to_value(&content)?;
    let metadata = FactMetadata {
        category: FactCategory::Technical,
        tags: vec![
            "spacekit-operator".to_string(),
            format!("operator:{}", content.display_name),
        ],
        domain: KnowledgeDomain::ComputerScience,
        source: DataSource::UserInput {
            application: author.clone(),
            user: author.clone(),
        },
        collection_method: CollectionMethod::Manual,
        verification_level: VerificationLevel::SelfClaimed,
        license: LicenseType::MIT,
        size_bytes: content_value.to_string().len() as u64,
        checksum: fact_id,
    };
    Ok(FactPackage {
        fact_id,
        version: 1,
        created_at: content.published_at,
        expires_at: None,
        content: FactContent::Json {
            data: content_value,
            schema: Some(SCHEMA_OPERATOR_V1.to_string()),
        },
        metadata,
        author: author.clone(),
        signature: SPHINCSSignature::new(Vec::new(), "sphincs-128s".to_string(), Vec::new()),
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: Vec::new(),
            verification_timestamp: content.published_at,
            verifier: Some(author),
        },
        dependencies: Vec::new(),
        citations: Vec::new(),
        confidence_score: 1.0,
        access_policy: AccessPolicy::Public,
        encryption: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_manifest_roundtrip() {
        let content = OperatorManifestContent {
            operator_did: "did:spacekit:op:alpha".into(),
            display_name: "Alpha Node".into(),
            storage_http_url: "http://127.0.0.1:3030".into(),
            blob_fact_auth: "hybrid".into(),
            content_policy_uri: Some("https://example.com/policy".into()),
            supported_features: vec!["workspaces".into(), "federation_export".into()],
            published_at: 100,
            supported_migration_versions: vec!["v1".into(), "v2".into()],
            did_signature_capable: true,
            sphincs_public_key_hex: Some("aa".repeat(64)),
        };
        let pkg = build_operator_fact_package(content.clone()).unwrap();
        match &pkg.content {
            FactContent::Json { schema, data } => {
                assert_eq!(schema.as_deref(), Some(SCHEMA_OPERATOR_V1));
                let restored: OperatorManifestContent =
                    serde_json::from_value(data.clone()).unwrap();
                assert_eq!(restored, content);
            }
            _ => panic!("expected Json content"),
        }
    }
}
