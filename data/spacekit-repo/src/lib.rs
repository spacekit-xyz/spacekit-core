//! SpaceKit repository protocol: commit payloads, ref documents, and [`FactPackage`] builders.
#![forbid(unsafe_code)]

mod commit;
pub mod types;

pub use commit::{
    build_commit_fact_package, commit_canonical_json, commit_fact_id, commit_signing_message,
    hex_tree_from_snapshot, parse_commit_from_fact_package, recompute_commit_fact_id,
    tree_snapshot_from_commit, verify_commit_fact_id, CommitError,
};
pub use spacekit_diff::{
    detect_exact_renames, diff_blobs, diff_trees, merge_blobs, merge_trees, unified_diff,
    BlobMergeResult, DiffHunk, MergeConflict, MergeResult, Rename, Side, TreeChange, TreeSnapshot,
};
pub use types::{
    CommitContent, RepoConfigJson, RepoRefJson, DEFAULT_FILE_MODE, EXEC_FILE_MODE,
    SCHEMA_COMMIT_V1, SYMLINK_MODE,
};
