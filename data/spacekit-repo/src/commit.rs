//! Commit [`FactPackage`] construction and deterministic [`FactID`] (`SHA-256`).

use crate::types::{CommitContent, SCHEMA_COMMIT_V1};
use serde_json::json;
use sha2::{Digest, Sha256};
use spacekit_diff::{Hash as BlobHash, TreeSnapshot};
use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
use spacekit_primitives::v1::fact::FactID;
use spacekit_primitives::v1::fact::{
    CollectionMethod, DataSource, FactCategory, FactContent, FactMetadata, FactPackage,
    KnowledgeDomain, LicenseType, ProofType, VerificationLevel, VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;

#[derive(Debug, thiserror::Error)]
pub enum CommitError {
    #[error("invalid schema (expected {SCHEMA_COMMIT_V1})")]
    BadSchema,
    #[error("invalid author DID")]
    InvalidAuthorDid,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid tree hash at path `{path}`: {reason}")]
    BadTreeHash { path: String, reason: String },
    #[error("unsupported fact content for repo commit")]
    NotJsonCommit,
}

/// Canonical JSON for hashing (deterministic key order via `serde_json::Map` insertion).
///
/// Includes file modes and the author/committer identity fields so they are
/// covered by the deterministic [`FactID`] (and therefore by the commit
/// signature). Fields are emitted in a fixed order regardless of struct layout.
pub fn commit_canonical_json(content: &CommitContent) -> Result<Vec<u8>, serde_json::Error> {
    let tree_obj = serde_json::to_value(&content.tree)?;
    let modes_obj = serde_json::to_value(&content.modes)?;
    let v = json!({
        "schema": content.schema,
        "tree": tree_obj,
        "modes": modes_obj,
        "message": content.message,
        "author_name": content.author_name,
        "author_email": content.author_email,
        "timestamp": content.timestamp,
        "committer_name": content.committer_name,
        "committer_email": content.committer_email,
        "committer_timestamp": content.committer_timestamp,
    });
    serde_json::to_vec(&v)
}

/// Deterministic [`FactID`] from author, parent commit ids (sorted), and canonical commit JSON.
pub fn commit_fact_id(
    author_did: &str,
    parents: &[FactID],
    body: &CommitContent,
) -> Result<FactID, CommitError> {
    let mut h = Sha256::new();
    h.update(b"spacekit-repo-commit-v1\0");
    h.update(author_did.as_bytes());
    h.update(b"\0");
    let mut p = parents.to_vec();
    p.sort_unstable();
    for id in &p {
        h.update(id);
    }
    h.update(b"\0");
    h.update(&commit_canonical_json(body)?);
    Ok(h.finalize().into())
}

/// Workspace tree for diff algorithms.
pub fn tree_snapshot_from_commit(content: &CommitContent) -> Result<TreeSnapshot, CommitError> {
    let mut snap = TreeSnapshot::new();
    for (path, hex_str) in &content.tree {
        let bytes = hex::decode(hex_str).map_err(|e| CommitError::BadTreeHash {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        if bytes.len() != 32 {
            return Err(CommitError::BadTreeHash {
                path: path.clone(),
                reason: format!("expected 32 bytes, got {}", bytes.len()),
            });
        }
        let mut hash: BlobHash = [0u8; 32];
        hash.copy_from_slice(&bytes);
        snap.insert(path.clone(), hash);
    }
    Ok(snap)
}

pub fn hex_tree_from_snapshot(snap: &TreeSnapshot) -> std::collections::BTreeMap<String, String> {
    snap.entries
        .iter()
        .map(|(p, h)| (p.clone(), hex::encode(h)))
        .collect()
}

/// Builds a [`FactPackage`] for `POST /facts` (signature is placeholder unless filled by caller later).
pub fn build_commit_fact_package(
    author_did: &str,
    parents: Vec<FactID>,
    commit: CommitContent,
) -> Result<FactPackage, CommitError> {
    if commit.schema != SCHEMA_COMMIT_V1 {
        return Err(CommitError::BadSchema);
    }
    let author = QuantumDID::parse(author_did).map_err(|_| CommitError::InvalidAuthorDid)?;
    let fact_id = commit_fact_id(author_did, &parents, &commit)?;
    let content_value = serde_json::to_value(&commit)?;
    let tree_bytes: u64 = commit
        .tree
        .values()
        .map(|s| s.len() as u64)
        .sum::<u64>()
        .saturating_add(commit.tree.len() as u64);
    let metadata = FactMetadata {
        category: FactCategory::Technical,
        tags: vec![
            "spacekit-repo".to_string(),
            "repo-commit".to_string(),
            format!("schema:{}", commit.schema),
        ],
        domain: KnowledgeDomain::ComputerScience,
        source: DataSource::UserInput {
            application: author.clone(),
            user: author.clone(),
        },
        collection_method: CollectionMethod::Manual,
        verification_level: VerificationLevel::SelfClaimed,
        license: LicenseType::MIT,
        size_bytes: tree_bytes,
        checksum: fact_id,
    };
    let signature = SPHINCSSignature::new(
        Vec::new(),
        "SPHINCS+-SHAKE-256-128s-simple".to_string(),
        Vec::new(),
    );
    let created_at = commit.timestamp;
    Ok(FactPackage {
        fact_id,
        version: 1,
        created_at,
        expires_at: None,
        content: FactContent::Json {
            data: content_value,
            schema: Some(SCHEMA_COMMIT_V1.to_string()),
        },
        metadata,
        author: author.clone(),
        signature,
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: Vec::new(),
            verification_timestamp: created_at,
            verifier: Some(author),
        },
        dependencies: parents,
        citations: Vec::new(),
        confidence_score: 1.0,
        access_policy: spacekit_primitives::v1::fact::AccessPolicy::Public,
        encryption: None,
    })
}

pub fn parse_commit_from_fact_package(pkg: &FactPackage) -> Result<CommitContent, CommitError> {
    match &pkg.content {
        FactContent::Json { data, schema } => {
            if schema.as_deref() != Some(SCHEMA_COMMIT_V1)
                && data.get("schema").and_then(|v| v.as_str()) != Some(SCHEMA_COMMIT_V1)
            {
                return Err(CommitError::BadSchema);
            }
            Ok(serde_json::from_value(data.clone())?)
        }
        _ => Err(CommitError::NotJsonCommit),
    }
}

/// Recompute the deterministic [`FactID`] a commit package *should* have, from
/// its parsed content (`author_name` is the author DID) and its parent
/// dependencies. Used to verify object integrity on fetch.
pub fn recompute_commit_fact_id(pkg: &FactPackage) -> Result<FactID, CommitError> {
    let content = parse_commit_from_fact_package(pkg)?;
    commit_fact_id(&content.author_name, &pkg.dependencies, &content)
}

/// True iff the package's stored [`FactID`] matches the recomputed one, i.e. the
/// commit content / ancestry has not been tampered with in transit or at rest.
pub fn verify_commit_fact_id(pkg: &FactPackage) -> Result<bool, CommitError> {
    Ok(recompute_commit_fact_id(pkg)? == pkg.fact_id)
}

/// The exact bytes a commit signature must cover: the deterministic
/// [`FactID`]. Signing this commits the signer to the author, ancestry, tree,
/// modes, message, and timestamps simultaneously.
pub fn commit_signing_message(pkg: &FactPackage) -> [u8; 32] {
    pkg.fact_id
}
