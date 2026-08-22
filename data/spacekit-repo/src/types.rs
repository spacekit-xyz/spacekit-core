//! JSON-shaped types persisted in refs and commit [`FactPackage`] payloads.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `FactContent::Json.schema` for [`crate::commit::build_commit_fact_package`].
pub const SCHEMA_COMMIT_V1: &str = "spacekit:repo:commit:v1";

/// Default POSIX mode for a tracked regular file (`0o644`).
pub const DEFAULT_FILE_MODE: u32 = 0o100_644;
/// POSIX mode for an executable regular file (`0o755`).
pub const EXEC_FILE_MODE: u32 = 0o100_755;
/// POSIX mode marker for a symbolic link (git uses `0o120000`).
pub const SYMLINK_MODE: u32 = 0o120_000;

/// Serializable commit body embedded in [`spacekit_primitives::v1::fact::FactContent::Json`].
///
/// `tree` maps workspace-relative POSIX paths → lowercase hex BLAKE3 digests (64 chars).
///
/// `modes` records POSIX file modes for paths that differ from
/// [`DEFAULT_FILE_MODE`] (executables, symlinks); paths absent from the map are
/// assumed to be plain `0o644` files. It is `#[serde(default)]` so commits
/// written before mode tracking still deserialize.
///
/// The committer fields default to the author fields when empty, mirroring git's
/// author/committer split (e.g. for `cherry-pick` / `rebase` / `commit --amend`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CommitContent {
    /// Must be [`SCHEMA_COMMIT_V1`] for new commits.
    pub schema: String,
    pub tree: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modes: BTreeMap<String, u32>,
    pub message: String,
    pub author_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author_email: String,
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub committer_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub committer_email: String,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub committer_timestamp: u64,
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

impl CommitContent {
    pub fn new(
        tree: BTreeMap<String, String>,
        message: String,
        author_name: String,
        timestamp: u64,
    ) -> Self {
        Self {
            schema: SCHEMA_COMMIT_V1.to_string(),
            tree,
            modes: BTreeMap::new(),
            message,
            author_name,
            author_email: String::new(),
            timestamp,
            committer_name: String::new(),
            committer_email: String::new(),
            committer_timestamp: 0,
        }
    }

    /// Effective committer name (falls back to the author when unset).
    pub fn effective_committer_name(&self) -> &str {
        if self.committer_name.is_empty() {
            &self.author_name
        } else {
            &self.committer_name
        }
    }

    /// Effective committer timestamp (falls back to the author timestamp).
    pub fn effective_committer_timestamp(&self) -> u64 {
        if self.committer_timestamp == 0 {
            self.timestamp
        } else {
            self.committer_timestamp
        }
    }

    /// POSIX mode recorded for `path`, defaulting to [`DEFAULT_FILE_MODE`].
    pub fn mode_for(&self, path: &str) -> u32 {
        self.modes.get(path).copied().unwrap_or(DEFAULT_FILE_MODE)
    }
}

/// Stored at `GET/PUT .../api/documents/repos/<name>/<suffix>` — see CLI conventions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoConfigJson {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_branch")]
    pub default_branch: String,
}

fn default_branch() -> String {
    "main".to_string()
}

/// Ref document payload (`tip` = hex-encoded [`spacekit_primitives::v1::fact::FactID`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoRefJson {
    pub tip: String,
}
