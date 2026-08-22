//! Repo commit persistence for sandbox/transaction apply (Stream B).

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use chrono::Utc;
use spacekit_primitives::v1::fact::FactPackage;
use spacekit_repo::types::{RepoRefJson, SCHEMA_COMMIT_V1};
use spacekit_repo::{build_commit_fact_package, CommitContent};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::database::DocumentRecord;

pub fn fact_dir(data_dir: &Path, fact_id_hex: &str) -> PathBuf {
    let prefix = &fact_id_hex[..2.min(fact_id_hex.len())];
    data_dir.join("facts").join(prefix)
}

pub fn fact_path(data_dir: &Path, fact_id_hex: &str) -> PathBuf {
    fact_dir(data_dir, fact_id_hex).join(format!("{fact_id_hex}.json"))
}

pub fn ref_collection(repo_name: &str) -> String {
    format!("repos/{repo_name}/refs")
}

pub fn ref_document_id(branch: &str) -> String {
    format!("heads/{branch}")
}

/// Persist a [`FactPackage`] to CAS-backed fact storage and mirror `fact_index`.
pub async fn persist_fact_package(
    data_dir: &Path,
    db: &crate::database::Database,
    fact: &FactPackage,
) -> Result<String> {
    let fact_id_hex = hex::encode(fact.fact_id);
    let path = fact_path(data_dir, &fact_id_hex);
    if path.exists() {
        return Ok(fact_id_hex);
    }
    let serialized = serde_json::to_vec(fact)?;
    let dir = fact_dir(data_dir, &fact_id_hex);
    tokio::fs::create_dir_all(&dir).await?;
    tokio::fs::write(&path, &serialized).await?;

    let author_did = fact.author.to_string();
    let schema_opt = match &fact.content {
        spacekit_primitives::v1::fact::FactContent::Json { schema, .. } => schema.clone(),
        _ => None,
    };
    let index_data = serde_json::json!({
        "fact_id": fact_id_hex,
        "version": fact.version,
        "created_at": fact.created_at,
        "author": author_did,
        "schema": schema_opt,
        "tags": fact.metadata.tags,
        "category": fact.metadata.category,
    });
    let now = Utc::now();
    let index_doc = DocumentRecord {
        owner_did: author_did,
        collection: "fact_index".to_string(),
        id: fact_id_hex.clone(),
        data: index_data,
        created_at: now,
        updated_at: now,
        blob_ref: None,
    };
    db.upsert_document(&index_doc)?;

    if schema_opt.as_deref() == Some(SCHEMA_COMMIT_V1) {
        if let Ok(commit) = spacekit_repo::parse_commit_from_fact_package(fact) {
            crate::access_policy::register_commit_tree_refs(
                data_dir,
                &fact_id_hex,
                &fact.author.to_string(),
                &fact.access_policy,
                &commit.tree,
            )
            .await?;
        }
    }

    Ok(fact_id_hex)
}

/// Apply a repo tree commit: fact + branch ref document.
pub async fn apply_repo_tree(
    data_dir: &Path,
    db: &crate::database::Database,
    owner_did: &str,
    repo_name: &str,
    branch: &str,
    commit: CommitContent,
    parent_fact_ids: &[String],
) -> Result<(String, Option<DocumentRecord>)> {
    let parents: Vec<spacekit_primitives::v1::fact::FactID> = parent_fact_ids
        .iter()
        .filter_map(|h| {
            let bytes = hex::decode(h).ok()?;
            if bytes.len() != 32 {
                return None;
            }
            let mut id = [0u8; 32];
            id.copy_from_slice(&bytes);
            Some(id)
        })
        .collect();

    let pkg = build_commit_fact_package(owner_did, parents, commit)?;
    let fact_id_hex = hex::encode(pkg.fact_id);

    persist_fact_package(data_dir, db, &pkg).await?;

    let collection = ref_collection(repo_name);
    let ref_id = ref_document_id(branch);
    let old_ref = db
        .get_document(owner_did, &collection, &ref_id)
        .ok()
        .flatten();

    let ref_doc = DocumentRecord {
        owner_did: owner_did.to_string(),
        collection,
        id: ref_id,
        data: serde_json::to_value(RepoRefJson {
            tip: fact_id_hex.clone(),
        })?,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        blob_ref: None,
    };
    db.upsert_document(&ref_doc)?;
    Ok((fact_id_hex, old_ref))
}

pub async fn revert_repo_tree(
    data_dir: &Path,
    db: &crate::database::Database,
    owner_did: &str,
    repo_name: &str,
    branch: &str,
    old_ref: Option<DocumentRecord>,
    applied_fact_id_hex: &str,
) -> Result<()> {
    let collection = ref_collection(repo_name);
    let ref_id = ref_document_id(branch);
    match old_ref {
        Some(doc) => {
            db.upsert_document(&doc)?;
        }
        None => {
            let _ = db.delete_document(owner_did, &collection, &ref_id);
        }
    }
    // Best-effort: leave fact file in CAS; ref rollback is the consistency boundary.
    let _ = applied_fact_id_hex;
    let _ = data_dir;
    Ok(())
}

/// Build [`CommitContent`] from a path→hash tree map.
pub fn commit_from_tree(
    tree: BTreeMap<String, String>,
    message: String,
    author_name: String,
    timestamp: u64,
) -> CommitContent {
    CommitContent::new(tree, message, author_name, timestamp)
}
