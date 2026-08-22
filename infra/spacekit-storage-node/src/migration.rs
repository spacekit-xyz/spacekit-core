//! DID-signed workspace migration manifests (`spacekit:migration:v1` / `v2`).
//!
//! Spec: [`DID-MIGRATION.md`](../DID-MIGRATION.md). Layer 1 remains HMAC on the export
//! bundle ([`crate::handoff`]); this module adds layer 2 (SPHINCS+ on the migration record).

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::workspace::WorkspaceExportBundle;

pub const SCHEMA_MIGRATION: &str = "spacekit:migration:v1";
pub const SCHEMA_VERSION_V1: &str = "spacekit:migration:v1";
pub const SCHEMA_VERSION_V2: &str = "spacekit:migration:v2";
pub const SPHINCS_ALG: &str = "sphincs-128s";

const OPERATOR_KEYPAIR_FILE: &str = ".operator_sphincs_keypair";
const MIGRATION_SIGNER_KEYS_DIR: &str = ".migration_signer_keys";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorSigningKeypair {
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationScenario {
    OperatorInitiated,
    UserInitiated,
    Bilateral,
}

impl MigrationScenario {
    pub fn required_signer_roles(&self) -> &'static [&'static str] {
        match self {
            MigrationScenario::OperatorInitiated => &["source_operator"],
            MigrationScenario::UserInitiated => &["workspace_owner"],
            MigrationScenario::Bilateral => {
                &["source_operator", "destination_operator", "workspace_owner"]
            }
        }
    }

    /// Roles that must be present on the inbound bundle before import (destination signs locally).
    pub fn required_signer_roles_at_import(&self) -> &'static [&'static str] {
        match self {
            MigrationScenario::OperatorInitiated => &["source_operator"],
            MigrationScenario::UserInitiated => &["workspace_owner"],
            MigrationScenario::Bilateral => &["source_operator", "workspace_owner"],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DidMigrationSignature {
    pub signer_role: String,
    pub signer_did: String,
    pub signature_algorithm: String,
    pub signed_payload_hash: String,
    pub signature: String,
    pub signed_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationManifest {
    pub schema: String,
    pub schema_version: String,
    pub migration_id: String,
    pub source_operator_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_operator_url: Option<String>,
    pub workspace_id: String,
    pub workspace_did: String,
    pub manifest_hash: String,
    pub blob_count: u64,
    pub fact_count: u64,
    pub initiated_at: u64,
    pub expires_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hmac_key_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hmac_signature: Option<String>,
    #[serde(default)]
    pub did_signatures: Vec<DidMigrationSignature>,
}

fn append_length_prefixed(buf: &mut Vec<u8>, field: &str) {
    let b = field.as_bytes();
    buf.extend_from_slice(&(b.len() as u32).to_be_bytes());
    buf.extend_from_slice(b);
}

pub fn canonical_signed_payload(m: &MigrationManifest) -> Vec<u8> {
    let mut buf = Vec::new();
    append_length_prefixed(&mut buf, &m.schema);
    append_length_prefixed(&mut buf, &m.schema_version);
    append_length_prefixed(&mut buf, &m.migration_id);
    append_length_prefixed(&mut buf, &m.source_operator_url);
    append_length_prefixed(
        &mut buf,
        m.destination_operator_url.as_deref().unwrap_or(""),
    );
    append_length_prefixed(&mut buf, &m.workspace_id);
    append_length_prefixed(&mut buf, &m.workspace_did);
    append_length_prefixed(&mut buf, &m.manifest_hash);
    append_length_prefixed(&mut buf, &m.blob_count.to_string());
    append_length_prefixed(&mut buf, &m.fact_count.to_string());
    append_length_prefixed(&mut buf, &m.initiated_at.to_string());
    append_length_prefixed(&mut buf, &m.expires_at.to_string());
    buf
}

pub fn signed_payload_hash_hex(m: &MigrationManifest) -> String {
    format!(
        "blake3:{}",
        hex::encode(blake3::hash(&canonical_signed_payload(m)).as_bytes())
    )
}

fn message_hash_bytes(m: &MigrationManifest) -> [u8; 32] {
    *blake3::hash(&canonical_signed_payload(m)).as_bytes()
}

pub fn negotiated_migration_version(local: &[String], remote: &[String]) -> &'static str {
    let local_v2 = local.iter().any(|v| v == "v2" || v == SCHEMA_VERSION_V2);
    let remote_v2 = remote.iter().any(|v| v == "v2" || v == SCHEMA_VERSION_V2);
    if local_v2 && remote_v2 {
        SCHEMA_VERSION_V2
    } else {
        SCHEMA_VERSION_V1
    }
}

pub fn migration_requires_did_signatures() -> bool {
    std::env::var("SPACEKIT_REQUIRE_MIGRATION_ATTESTATION")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Load or create operator SPHINCS+ keypair under `data_dir`.
pub fn load_or_create_operator_keypair(data_dir: &Path) -> Result<OperatorSigningKeypair> {
    let path = data_dir.join(OPERATOR_KEYPAIR_FILE);
    if path.exists() {
        let raw = std::fs::read(&path)?;
        return Ok(serde_json::from_slice(&raw)?);
    }
    let (public_key, secret_key) =
        spacekit_primitives::v1::crypto::quantum::generate_sphincs_keypair(SPHINCS_ALG)?;
    let kp = OperatorSigningKeypair {
        public_key,
        secret_key,
    };
    std::fs::create_dir_all(data_dir)?;
    std::fs::write(&path, serde_json::to_vec_pretty(&kp)?)?;
    Ok(kp)
}

pub fn load_operator_keypair(data_dir: Option<&Path>) -> Option<OperatorSigningKeypair> {
    let dir = data_dir?;
    let path = dir.join(OPERATOR_KEYPAIR_FILE);
    let raw = std::fs::read(&path).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// Per-DID SPHINCS+ key path for non-operator roles (`workspace_owner`, etc.).
pub fn migration_signer_key_path(data_dir: &Path, signer_did: &str) -> std::path::PathBuf {
    let id = hex::encode(blake3::hash(signer_did.as_bytes()).as_bytes());
    data_dir
        .join(MIGRATION_SIGNER_KEYS_DIR)
        .join(format!("{id}.json"))
}

pub fn load_migration_signer_keypair(
    data_dir: &Path,
    signer_did: &str,
) -> Option<OperatorSigningKeypair> {
    let path = migration_signer_key_path(data_dir, signer_did);
    let raw = std::fs::read(&path).ok()?;
    serde_json::from_slice(&raw).ok()
}

pub fn save_migration_signer_keypair(
    data_dir: &Path,
    signer_did: &str,
    keypair: &OperatorSigningKeypair,
) -> Result<()> {
    let path = migration_signer_key_path(data_dir, signer_did);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(keypair)?)?;
    Ok(())
}

/// Create or load a per-DID migration signing key (e.g. `workspace_owner`).
pub fn load_or_create_migration_signer_keypair(
    data_dir: &Path,
    signer_did: &str,
) -> Result<OperatorSigningKeypair> {
    if let Some(kp) = load_migration_signer_keypair(data_dir, signer_did) {
        return Ok(kp);
    }
    let (public_key, secret_key) =
        spacekit_primitives::v1::crypto::quantum::generate_sphincs_keypair(SPHINCS_ALG)?;
    let kp = OperatorSigningKeypair {
        public_key,
        secret_key,
    };
    save_migration_signer_keypair(data_dir, signer_did, &kp)?;
    Ok(kp)
}

/// Load signing key for a migration role: operator keypair or per-DID file.
pub fn load_signing_keypair_for_role(
    data_dir: &Path,
    role: &str,
    signer_did: &str,
    operator_did: Option<&str>,
) -> Option<OperatorSigningKeypair> {
    if role == "source_operator" || role == "destination_operator" {
        if operator_did == Some(signer_did) {
            if let Some(kp) = load_operator_keypair(Some(data_dir)) {
                return Some(kp);
            }
        }
    }
    if role == "workspace_owner" {
        return load_migration_signer_keypair(data_dir, signer_did);
    }
    load_migration_signer_keypair(data_dir, signer_did)
}

/// Ensure `{data_dir}/.operator_sphincs_keypair` exists (auto-generated if missing).
pub fn ensure_operator_keypair(data_dir: &Path) -> std::io::Result<()> {
    load_or_create_operator_keypair(data_dir)
        .map(|_| ())
        .map_err(|e| std::io::Error::other(e.to_string()))
}

pub const SCHEMA_MIGRATION_RECORD: &str = "spacekit:migration_record:v1";

pub fn migration_record_fact_id(migration_id: &str) -> spacekit_primitives::v1::fact::FactID {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"spacekit-migration-record-v1\0");
    h.update(migration_id.as_bytes());
    h.finalize().into()
}

pub fn migration_record_storage_path(data_dir: &Path, migration_id: &str) -> std::path::PathBuf {
    let fact_id_hex = hex::encode(migration_record_fact_id(migration_id));
    let prefix = &fact_id_hex[..2.min(fact_id_hex.len())];
    data_dir
        .join("facts")
        .join(prefix)
        .join(format!("{fact_id_hex}.json"))
}

pub fn build_migration_record_package(
    manifest: &MigrationManifest,
    author_operator_did: &str,
) -> Result<spacekit_primitives::v1::fact::FactPackage> {
    use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
    use spacekit_primitives::v1::fact::{
        AccessPolicy, CollectionMethod, DataSource, FactCategory, FactContent, FactMetadata,
        FactPackage, KnowledgeDomain, LicenseType, ProofType, VerificationLevel, VerificationProof,
    };
    use spacekit_primitives::v1::identity::QuantumDID;

    let author = QuantumDID::parse(author_operator_did)
        .map_err(|_| anyhow!("invalid operator DID for migration record"))?;
    let fact_id = migration_record_fact_id(&manifest.migration_id);
    let data = serde_json::to_value(manifest)?;
    let metadata = FactMetadata {
        category: FactCategory::Technical,
        tags: vec![
            "spacekit-migration".to_string(),
            format!("workspace:{}", manifest.workspace_id),
        ],
        domain: KnowledgeDomain::ComputerScience,
        source: DataSource::UserInput {
            application: author.clone(),
            user: author.clone(),
        },
        collection_method: CollectionMethod::Manual,
        verification_level: VerificationLevel::SelfClaimed,
        license: LicenseType::MIT,
        size_bytes: data.to_string().len() as u64,
        checksum: fact_id,
    };
    Ok(FactPackage {
        fact_id,
        version: 1,
        created_at: manifest.initiated_at,
        expires_at: Some(manifest.expires_at),
        content: FactContent::Json {
            data,
            schema: Some(SCHEMA_MIGRATION_RECORD.to_string()),
        },
        metadata,
        author: author.clone(),
        signature: SPHINCSSignature::new(Vec::new(), SPHINCS_ALG.to_string(), Vec::new()),
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: Vec::new(),
            verification_timestamp: manifest.initiated_at,
            verifier: Some(author),
        },
        dependencies: Vec::new(),
        citations: Vec::new(),
        confidence_score: 1.0,
        access_policy: AccessPolicy::Public,
        encryption: None,
    })
}

/// Persist the migration manifest as a public fact (audit trail).
pub async fn persist_migration_record(
    cas: &Path,
    manifest: &MigrationManifest,
    author_operator_did: &str,
) -> Result<String> {
    let pkg = build_migration_record_package(manifest, author_operator_did)?;
    let fact_id_hex = hex::encode(pkg.fact_id);
    let path = migration_record_storage_path(cas, &manifest.migration_id);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, serde_json::to_vec(&pkg)?).await?;
    Ok(fact_id_hex)
}

/// Apply negotiated schema version before signing.
pub fn apply_negotiated_schema_version(manifest: &mut MigrationManifest, version: &str) {
    manifest.schema_version = version.to_string();
    if version != SCHEMA_VERSION_V2 {
        manifest.did_signatures.clear();
    }
}

/// Fetch `supported_migration_versions` from a remote operator (`GET /api/operators/self`).
#[cfg(feature = "api-server")]
pub async fn fetch_remote_migration_versions(operator_base_url: &str) -> Result<Vec<String>> {
    let base = operator_base_url.trim_end_matches('/');
    let url = format!("{base}/api/operators/self");
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| anyhow!("fetch operator self: {e}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("operator self HTTP {}", resp.status()));
    }
    let body: crate::operator_manifest::OperatorSelfResponse = resp
        .json()
        .await
        .map_err(|e| anyhow!("parse operator self: {e}"))?;
    Ok(body.manifest.supported_migration_versions)
}

#[cfg(not(feature = "api-server"))]
pub async fn fetch_remote_migration_versions(_operator_base_url: &str) -> Result<Vec<String>> {
    Err(anyhow!("remote operator fetch requires api-server feature"))
}

pub fn build_manifest_from_export(
    bundle: &WorkspaceExportBundle,
    source_operator_url: &str,
    source_operator_did: &str,
    destination_operator_url: Option<String>,
    ttl_seconds: u64,
) -> Result<MigrationManifest> {
    let export_bytes = crate::handoff::export_signing_bytes(bundle)?;
    let manifest_hash = format!(
        "blake3:{}",
        hex::encode(blake3::hash(&export_bytes).as_bytes())
    );
    let migration_id = hex::encode(
        blake3::hash(
            format!(
                "{}:{}:{}:{}",
                source_operator_did, bundle.workspace_id, bundle.exported_at, manifest_hash
            )
            .as_bytes(),
        )
        .as_bytes(),
    );
    let now = bundle.exported_at;
    Ok(MigrationManifest {
        schema: SCHEMA_MIGRATION.to_string(),
        schema_version: SCHEMA_VERSION_V1.to_string(),
        migration_id,
        source_operator_url: source_operator_url.trim_end_matches('/').to_string(),
        destination_operator_url,
        workspace_id: bundle.workspace_id.clone(),
        workspace_did: bundle.owner_did.clone(),
        manifest_hash,
        blob_count: bundle.referenced_blob_hashes.len() as u64,
        fact_count: 1,
        initiated_at: now,
        expires_at: now.saturating_add(ttl_seconds.max(3600)),
        hmac_key_id: None,
        hmac_signature: bundle.handoff_signature.clone(),
        did_signatures: Vec::new(),
    })
}

pub fn sign_manifest_role(
    manifest: &mut MigrationManifest,
    role: &str,
    signer_did: &str,
    keypair: &OperatorSigningKeypair,
    now: u64,
) -> Result<()> {
    let hash_bytes = message_hash_bytes(manifest);
    let sig = spacekit_primitives::v1::crypto::quantum::sign_sphincs_detached(
        &hash_bytes,
        SPHINCS_ALG,
        &keypair.public_key,
        &keypair.secret_key,
    )?;
    manifest.did_signatures.push(DidMigrationSignature {
        signer_role: role.to_string(),
        signer_did: signer_did.to_string(),
        signature_algorithm: SPHINCS_ALG.to_string(),
        signed_payload_hash: signed_payload_hash_hex(manifest),
        signature: hex::encode(sig.signature_bytes),
        signed_at: now,
    });
    // Only source export attestation upgrades the manifest to v2. Destination counter-sign
    // on a v1 inbound bundle must not force v2 (would require source_operator on import).
    if role == "source_operator" {
        manifest.schema_version = SCHEMA_VERSION_V2.to_string();
    }
    Ok(())
}

pub fn verify_signature_entry(
    manifest: &MigrationManifest,
    entry: &DidMigrationSignature,
    public_key: &[u8],
) -> Result<bool> {
    if entry.signed_payload_hash != signed_payload_hash_hex(manifest) {
        return Ok(false);
    }
    let hash_bytes = message_hash_bytes(manifest);
    let sig = spacekit_primitives::v1::crypto::quantum::SPHINCSSignature::new(
        hex::decode(&entry.signature).map_err(|e| anyhow!("signature hex: {e}"))?,
        entry.signature_algorithm.clone(),
        public_key.to_vec(),
    );
    spacekit_primitives::v1::crypto::quantum::verify_sphincs_signature(&hash_bytes, &sig)
        .map_err(|e| anyhow!("verify: {e}"))
}

pub fn has_required_signers(manifest: &MigrationManifest, scenario: MigrationScenario) -> bool {
    has_required_signer_roles(manifest, scenario.required_signer_roles())
}

pub fn has_required_signers_at_import(
    manifest: &MigrationManifest,
    scenario: MigrationScenario,
) -> bool {
    has_required_signer_roles(manifest, scenario.required_signer_roles_at_import())
}

fn has_required_signer_roles(manifest: &MigrationManifest, roles: &[&str]) -> bool {
    for role in roles {
        let ok = manifest
            .did_signatures
            .iter()
            .any(|s| s.signer_role == *role);
        if !ok {
            return false;
        }
    }
    true
}

pub fn validate_migration_manifest(
    manifest: &MigrationManifest,
    scenario: MigrationScenario,
    role_to_pubkey: impl Fn(&str, &str) -> Option<Vec<u8>>,
) -> Result<()> {
    if manifest.schema_version == SCHEMA_VERSION_V2 || migration_requires_did_signatures() {
        if manifest.did_signatures.is_empty() {
            return Err(anyhow!(
                "migration requires DID signatures (schema_version v2 or SPACEKIT_REQUIRE_MIGRATION_ATTESTATION)"
            ));
        }
        for entry in &manifest.did_signatures {
            let pk = role_to_pubkey(&entry.signer_role, &entry.signer_did).ok_or_else(|| {
                anyhow!(
                    "no public key for signer {} ({})",
                    entry.signer_did,
                    entry.signer_role
                )
            })?;
            if !verify_signature_entry(manifest, entry, &pk)? {
                return Err(anyhow!(
                    "invalid DID signature for {} ({})",
                    entry.signer_did,
                    entry.signer_role
                ));
            }
        }
        if !has_required_signers(manifest, scenario) {
            return Err(anyhow!(
                "missing required signer roles for migration scenario"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_payload_deterministic() {
        let m = MigrationManifest {
            schema: SCHEMA_MIGRATION.to_string(),
            schema_version: SCHEMA_VERSION_V1.to_string(),
            migration_id: "mid".into(),
            source_operator_url: "http://a".into(),
            destination_operator_url: None,
            workspace_id: "ws".into(),
            workspace_did: "did:spacekit:owner".into(),
            manifest_hash: "blake3:aa".into(),
            blob_count: 2,
            fact_count: 1,
            initiated_at: 100,
            expires_at: 200,
            hmac_key_id: None,
            hmac_signature: None,
            did_signatures: vec![],
        };
        assert_eq!(canonical_signed_payload(&m), canonical_signed_payload(&m));
    }

    #[test]
    fn sign_verify_roundtrip() {
        let (pk, sk) =
            spacekit_primitives::v1::crypto::quantum::generate_sphincs_keypair(SPHINCS_ALG)
                .unwrap();
        let kp = OperatorSigningKeypair {
            public_key: pk,
            secret_key: sk,
        };
        let mut m = MigrationManifest {
            schema: SCHEMA_MIGRATION.to_string(),
            schema_version: SCHEMA_VERSION_V1.to_string(),
            migration_id: "mid".into(),
            source_operator_url: "http://a".into(),
            destination_operator_url: None,
            workspace_id: "ws".into(),
            workspace_did: "did:spacekit:owner".into(),
            manifest_hash: "blake3:bb".into(),
            blob_count: 0,
            fact_count: 1,
            initiated_at: 1,
            expires_at: 2,
            hmac_key_id: None,
            hmac_signature: None,
            did_signatures: vec![],
        };
        sign_manifest_role(&mut m, "source_operator", "did:spacekit:op:a", &kp, 10).unwrap();
        let entry = m.did_signatures.first().unwrap();
        assert!(verify_signature_entry(&m, entry, &kp.public_key).unwrap());
        m.workspace_id = "tampered".into();
        assert!(!verify_signature_entry(&m, entry, &kp.public_key).unwrap());
    }

    #[test]
    fn workspace_owner_sign_verify_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let owner = "did:spacekit:owner:mig-test";
        let kp = load_or_create_migration_signer_keypair(dir.path(), owner).unwrap();
        let mut m = MigrationManifest {
            schema: SCHEMA_MIGRATION.to_string(),
            schema_version: SCHEMA_VERSION_V1.to_string(),
            migration_id: "mid".into(),
            source_operator_url: "http://a".into(),
            destination_operator_url: None,
            workspace_id: "ws".into(),
            workspace_did: owner.into(),
            manifest_hash: "blake3:cc".into(),
            blob_count: 0,
            fact_count: 1,
            initiated_at: 1,
            expires_at: 2,
            hmac_key_id: None,
            hmac_signature: None,
            did_signatures: vec![],
        };
        sign_manifest_role(&mut m, "workspace_owner", owner, &kp, 5).unwrap();
        let entry = m.did_signatures.first().unwrap();
        assert!(verify_signature_entry(&m, entry, &kp.public_key).unwrap());
        assert!(has_required_signers(&m, MigrationScenario::UserInitiated));
    }

    #[test]
    fn version_negotiation() {
        assert_eq!(
            negotiated_migration_version(&["v1".into(), "v2".into()], &["v2".into()]),
            SCHEMA_VERSION_V2
        );
        assert_eq!(
            negotiated_migration_version(&["v1".into()], &["v2".into()]),
            SCHEMA_VERSION_V1
        );
    }
}
