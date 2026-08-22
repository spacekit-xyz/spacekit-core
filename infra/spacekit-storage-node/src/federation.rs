//! Federation helpers — workspace blob manifests and cross-node CAS pull (Phase 3).

#![deny(clippy::all)]

use anyhow::Result;
use spacekit_repo::types::RepoRefJson;
use std::collections::BTreeSet;
use std::path::Path;

use crate::database::Database;
use crate::workspace::WorkspaceContent;

pub fn is_valid_blake3_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn blob_path(data_dir: &Path, hash: &str) -> std::path::PathBuf {
    let prefix = &hash[..2.min(hash.len())];
    data_dir.join("blobs").join(prefix).join(hash)
}

/// Collect BLAKE3 hashes referenced by associated repo tips (`heads/main`).
pub fn collect_workspace_blob_hashes(
    cas: &Path,
    db: &Database,
    owner_did: &str,
    content: &WorkspaceContent,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for repo in &content.associated_repos {
        let collection = crate::repo_commit::ref_collection(repo);
        let ref_id = crate::repo_commit::ref_document_id("main");
        let Some(doc) = db
            .get_document(owner_did, &collection, &ref_id)
            .ok()
            .flatten()
        else {
            continue;
        };
        let Ok(ref_json) = serde_json::from_value::<RepoRefJson>(doc.data) else {
            continue;
        };
        let tip = ref_json.tip.trim();
        if tip.is_empty() {
            continue;
        }
        let path = crate::repo_commit::fact_path(cas, tip);
        if !path.exists() {
            continue;
        }
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        let Ok(pkg) = serde_json::from_slice::<spacekit_primitives::v1::fact::FactPackage>(&raw)
        else {
            continue;
        };
        if let Ok(commit) = spacekit_repo::parse_commit_from_fact_package(&pkg) {
            for hash in commit.tree.values() {
                if is_valid_blake3_hex(hash) {
                    out.insert(hash.clone());
                }
            }
        }
    }
    out
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BlobReplicateReport {
    pub fetched: usize,
    pub skipped_existing: usize,
    pub failed: Vec<String>,
}

/// Pull missing blobs from a remote storage node HTTP API (`GET /blobs/{hash}`).
#[cfg(feature = "api-server")]
pub async fn replicate_blobs_from_source(
    dest_cas: &Path,
    source_base_url: &str,
    hashes: &[String],
    source_authorization: Option<&str>,
) -> Result<BlobReplicateReport> {
    let client = reqwest::Client::new();
    let base = source_base_url.trim_end_matches('/');
    let mut report = BlobReplicateReport::default();
    for hash in hashes {
        if !is_valid_blake3_hex(hash) {
            report.failed.push(format!("{hash}: invalid hash"));
            continue;
        }
        let dest_path = blob_path(dest_cas, hash);
        if dest_path.exists() {
            report.skipped_existing += 1;
            continue;
        }
        let url = format!("{base}/blobs/{hash}");
        let mut req = client.get(&url);
        if let Some(auth) = source_authorization {
            req = req.header("Authorization", auth);
        }
        match req.send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    if let Some(parent) = dest_path.parent() {
                        tokio::fs::create_dir_all(parent).await?;
                    }
                    tokio::fs::write(&dest_path, &bytes).await?;
                    report.fetched += 1;
                }
                Err(e) => report.failed.push(format!("{hash}: read body: {e}")),
            },
            Ok(resp) => report
                .failed
                .push(format!("{hash}: HTTP {}", resp.status())),
            Err(e) => report.failed.push(format!("{hash}: {e}")),
        }
    }
    Ok(report)
}

#[cfg(feature = "api-server")]
pub async fn write_blob_file(dest_cas: &Path, hash: &str, body: &[u8]) -> Result<()> {
    let path = blob_path(dest_cas, hash);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, body).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo_commit::{apply_repo_tree, commit_from_tree};
    use std::collections::BTreeMap;

    #[tokio::test]
    async fn collect_hashes_from_repo_tip() {
        let dir = tempfile::tempdir().unwrap();
        let data_dir = dir.path().to_path_buf();
        let db_path = data_dir.join("db.json");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.initialize().unwrap();
        let owner = "did:spacekit:fed:owner";
        let hash = hex::encode([9u8; 32]);
        let tree = BTreeMap::from([("a.txt".to_string(), hash.clone())]);
        let commit = commit_from_tree(tree, "init".into(), "author".into(), 1);
        apply_repo_tree(&data_dir, &db, owner, "myrepo", "main", commit, &[])
            .await
            .unwrap();
        let content = WorkspaceContent {
            workspace_id: "ws".into(),
            owner_did: owner.into(),
            collaborators: vec![],
            associated_repos: vec!["myrepo".into()],
            quotas: Default::default(),
            default_access_policy: spacekit_primitives::v1::fact::AccessPolicy::Public,
            status: crate::workspace::WorkspaceStatus::Active,
            created_at: 1,
            updated_at: 1,
        };
        let hashes = collect_workspace_blob_hashes(&data_dir, &db, owner, &content);
        assert!(hashes.contains(&hash));
    }
}
