//! `spacekit-diff` — pure-computation diff and three-way merge for SpaceKit.
//!
//! This crate is intentionally `no_std + alloc` and pulls in zero external
//! dependencies, so it can be compiled to `wasm32-unknown-unknown` for use in
//! browsers and (potentially) inside SpaceKit verification contracts.
//!
//! It works on two layers:
//!
//! - **Trees**: a [`TreeSnapshot`] is a sorted map of `path → 32-byte hash`.
//!   Use [`diff_trees`] to compute a changeset, [`apply_tree_diff`] to replay
//!   one, and [`merge_trees`] for a git-style three-way merge.
//!
//! - **Blobs**: raw byte slices (typically file contents fetched separately
//!   from a storage node). Use [`diff_blobs`] for line-level diffs (Myers'
//!   algorithm) and [`merge_blobs`] for line-level three-way merges (diff3
//!   with conflict markers).
//!
//! No function in this crate touches I/O, the network, or cryptography —
//! callers (CLI, browser, contract) handle fetching content and computing
//! hashes themselves.

#![cfg_attr(not(test), no_std)]
#![deny(missing_docs)]
#![deny(unsafe_code)]

extern crate alloc;

pub mod blob;
pub mod tree;
pub mod types;

// Re-export the public surface so callers can write `spacekit_diff::diff_trees`
// directly without remembering which submodule a thing lives in.
pub use crate::blob::{diff_blobs, merge_blobs, unified_diff};
pub use crate::tree::{apply_tree_diff, detect_exact_renames, diff_trees, merge_trees};
pub use crate::types::{
    ApplyError, BlobMergeResult, DiffHunk, Hash, MergeConflict, MergeResult, Rename, Side,
    TreeChange, TreeSnapshot,
};
