//! First-class workspace documents (`spacekit:workspace:v1`) as facts (Stream C).

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
use spacekit_primitives::v1::fact::FactID;
use spacekit_primitives::v1::fact::{
    AccessPolicy, CollectionMethod, DataSource, FactCategory, FactContent, FactMetadata,
    FactPackage, KnowledgeDomain, LicenseType, ProofType, VerificationLevel, VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;

/// `FactContent::Json.schema` for workspace documents.
pub const SCHEMA_WORKSPACE_V1: &str = "spacekit:workspace:v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceCollaborator {
    pub did: String,
    /// e.g. `owner`, `admin`, `agent`, `viewer`
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceQuotas {
    #[serde(default)]
    pub max_sandbox_bytes: u64,
    #[serde(default)]
    pub max_storage_bytes: u64,
}

impl Default for WorkspaceQuotas {
    fn default() -> Self {
        Self {
            max_sandbox_bytes: 64 * 1024 * 1024,
            max_storage_bytes: 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceContent {
    pub workspace_id: String,
    pub owner_did: String,
    pub collaborators: Vec<WorkspaceCollaborator>,
    #[serde(default)]
    pub associated_repos: Vec<String>,
    pub quotas: WorkspaceQuotas,
    #[serde(default = "default_access_public")]
    pub default_access_policy: AccessPolicy,
    pub status: WorkspaceStatus,
    pub created_at: u64,
    pub updated_at: u64,
}

fn default_access_public() -> AccessPolicy {
    AccessPolicy::Public
}

/// How import behaves when the destination already has this workspace id for the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceImportConflict {
    #[default]
    Reject,
    Replace,
}

impl WorkspaceImportConflict {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "reject" => Some(Self::Reject),
            "replace" => Some(Self::Replace),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceImportResult {
    pub fact_id: String,
    pub workspace_id: String,
    pub owner_did: String,
    pub created: bool,
    pub replaced: bool,
    #[serde(default)]
    pub blob_replication: Option<crate::federation::BlobReplicateReport>,
    /// Hex fact id of `spacekit:migration_record:v1` when import persisted a signed manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_record_fact_id: Option<String>,
}

/// Portable export for federation / operator migration (Phase 3 handoff).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceExportBundle {
    pub schema: String,
    pub fact_id: String,
    pub owner_did: String,
    pub workspace_id: String,
    pub content: WorkspaceContent,
    pub exported_at: u64,
    /// BLAKE3 hashes from associated repo `heads/main` tips (federation CAS replication).
    #[serde(default)]
    pub referenced_blob_hashes: Vec<String>,
    /// HMAC-SHA3 over canonical export JSON (excluding this field). See [`crate::handoff`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_signature: Option<String>,
    /// DID-signed migration record (layer 2). See [`crate::migration`] and `DID-MIGRATION.md`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migration_manifest: Option<crate::migration::MigrationManifest>,
}

pub fn workspace_fact_id(owner_did: &str, workspace_id: &str) -> FactID {
    let mut h = Sha256::new();
    h.update(b"spacekit-workspace-v1\0");
    h.update(owner_did.as_bytes());
    h.update(b"\0");
    h.update(workspace_id.as_bytes());
    h.finalize().into()
}

pub fn build_workspace_fact_package(content: WorkspaceContent) -> Result<FactPackage> {
    let author = QuantumDID::parse(&content.owner_did).map_err(|_| anyhow!("invalid owner DID"))?;
    let fact_id = workspace_fact_id(&content.owner_did, &content.workspace_id);
    let content_value = serde_json::to_value(&content)?;
    let metadata = FactMetadata {
        category: FactCategory::Technical,
        tags: vec![
            "spacekit-workspace".to_string(),
            format!("workspace:{}", content.workspace_id),
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
        created_at: content.created_at,
        expires_at: None,
        content: FactContent::Json {
            data: content_value,
            schema: Some(SCHEMA_WORKSPACE_V1.to_string()),
        },
        metadata,
        author: author.clone(),
        signature: SPHINCSSignature::new(
            Vec::new(),
            "SPHINCS+-SHAKE-256-128s-simple".to_string(),
            Vec::new(),
        ),
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: Vec::new(),
            verification_timestamp: content.updated_at,
            verifier: Some(author),
        },
        dependencies: Vec::new(),
        citations: Vec::new(),
        confidence_score: 1.0,
        access_policy: content.default_access_policy.clone(),
        encryption: None,
    })
}

/// Whether `did` may create sandboxes or act as owner for workspace-scoped resources.
pub fn workspace_allows_actor(ws: &WorkspaceContent, did: &str) -> bool {
    if ws.status != WorkspaceStatus::Active {
        return false;
    }
    if ws.owner_did == did {
        return true;
    }
    ws.collaborators.iter().any(|c| c.did == did)
}

/// Apply workspace quota caps to a new sandbox's [`crate::sandbox::SandboxConfig`].
pub fn cap_sandbox_config(cfg: &mut crate::sandbox::SandboxConfig, quotas: &WorkspaceQuotas) {
    if quotas.max_sandbox_bytes > 0 {
        cfg.max_bytes_written = cfg.max_bytes_written.min(quotas.max_sandbox_bytes);
    }
    // Workspace vector/fact caps are not modeled yet; keep sandbox defaults.
}

/// Write workspace fact + index (overwrites existing fact file for same fact id).
pub async fn upsert_workspace_fact(
    cas: &std::path::Path,
    db: &crate::database::Database,
    pkg: &FactPackage,
    content: &WorkspaceContent,
) -> Result<String> {
    let fact_id_hex = hex::encode(pkg.fact_id);
    let path = crate::repo_commit::fact_path(cas, &fact_id_hex);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, serde_json::to_vec(pkg)?).await?;
    let now = chrono::Utc::now();
    let index = crate::database::DocumentRecord {
        owner_did: content.owner_did.clone(),
        collection: "workspace_index".to_string(),
        id: content.workspace_id.clone(),
        data: serde_json::to_value(content)?,
        created_at: now,
        updated_at: now,
        blob_ref: None,
    };
    db.upsert_document(&index)?;
    Ok(fact_id_hex)
}

pub fn parse_workspace_from_fact(pkg: &FactPackage) -> Result<WorkspaceContent> {
    match &pkg.content {
        FactContent::Json { data, schema } => {
            if schema.as_deref() != Some(SCHEMA_WORKSPACE_V1) && data.get("workspace_id").is_none()
            {
                return Err(anyhow!("not a workspace fact"));
            }
            Ok(serde_json::from_value(data.clone())?)
        }
        _ => Err(anyhow!("workspace facts must be JSON content")),
    }
}
