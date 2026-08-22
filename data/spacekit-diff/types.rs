//! Public data types used by the diff/merge module.
//!
//! Every type here is plain data: no I/O, no clever borrowing. The hash type
//! is a fixed 32-byte array so callers can plug in BLAKE3 or SHA-256 without
//! the library caring which one.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A 32-byte content hash. Chosen to fit BLAKE3 / SHA-256 / SHA3-256 outputs.
pub type Hash = [u8; 32];

/// A snapshot of a repository tree: an ordered map from path to content hash.
///
/// This is the unit of comparison for [`crate::diff_trees`] and
/// [`crate::merge_trees`]. A SpaceKit "commit" is a FactPackage referencing a
/// serialized `TreeSnapshot`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TreeSnapshot {
    /// Path → hash. `BTreeMap` so iteration is in sorted order, which the
    /// diff and merge implementations rely on for their O(n+m) merge-join.
    pub entries: BTreeMap<String, Hash>,
}

impl TreeSnapshot {
    /// Empty tree.
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Convenience builder.
    pub fn from_pairs<I, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (S, Hash)>,
        S: Into<String>,
    {
        let mut entries = BTreeMap::new();
        for (p, h) in pairs {
            entries.insert(p.into(), h);
        }
        Self { entries }
    }

    /// Insert or replace a single entry.
    pub fn insert(&mut self, path: impl Into<String>, hash: Hash) -> Option<Hash> {
        self.entries.insert(path.into(), hash)
    }

    /// Number of paths in the tree.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if the tree has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A single change between two tree snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeChange {
    /// Path exists in `head` but not in `base`.
    Added {
        /// The path that was added.
        path: String,
        /// Hash of the added content.
        hash: Hash,
    },
    /// Path exists in `base` but not in `head`.
    Removed {
        /// The path that was removed.
        path: String,
        /// Hash of the content that was at that path before removal.
        hash: Hash,
    },
    /// Path exists in both with different hashes.
    Modified {
        /// The path whose content changed.
        path: String,
        /// Hash of the content in `base`.
        old_hash: Hash,
        /// Hash of the content in `head`.
        new_hash: Hash,
    },
}

impl TreeChange {
    /// The path this change refers to (useful for sorting / filtering).
    pub fn path(&self) -> &str {
        match self {
            TreeChange::Added { path, .. }
            | TreeChange::Removed { path, .. }
            | TreeChange::Modified { path, .. } => path,
        }
    }
}

/// An exact rename detected by pairing a [`TreeChange::Removed`] with a
/// [`TreeChange::Added`] that share the same content hash.
///
/// This is the cheap, content-identical form of rename detection (git's
/// "exact rename" pass). Similarity-based rename detection of *modified*
/// content is intentionally out of scope here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    /// Old path (where the content was removed from).
    pub from: String,
    /// New path (where the identical content was added).
    pub to: String,
    /// The shared content hash.
    pub hash: Hash,
}

/// A line-level edit hunk between two blob versions.
///
/// `old_start` / `new_start` are 0-based line indices in the original /
/// resulting file. Each `lines` vec contains the actual line text (with the
/// trailing newline preserved if the source line had one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffHunk {
    /// Lines that match in both files.
    Equal {
        /// 0-based starting line in the original file.
        old_start: usize,
        /// 0-based starting line in the new file.
        new_start: usize,
        /// The matching line text (with trailing newlines preserved).
        lines: Vec<String>,
    },
    /// Lines added in `new`, not present in `old`.
    Insert {
        /// 0-based starting line in the new file.
        new_start: usize,
        /// The inserted line text.
        lines: Vec<String>,
    },
    /// Lines removed from `old`, not present in `new`.
    Delete {
        /// 0-based starting line in the original file.
        old_start: usize,
        /// The deleted line text.
        lines: Vec<String>,
    },
    /// A delete immediately followed by an insert at the same location:
    /// `old_lines` were replaced by `new_lines`.
    Replace {
        /// 0-based starting line in the original file.
        old_start: usize,
        /// The lines that were removed.
        old_lines: Vec<String>,
        /// 0-based starting line in the new file.
        new_start: usize,
        /// The lines that took their place.
        new_lines: Vec<String>,
    },
}

/// Which side of a three-way merge a fact came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The "ours" side (typically the local branch).
    Ours,
    /// The "theirs" side (typically the incoming branch).
    Theirs,
}

/// A conflict produced by [`crate::merge_trees`].
///
/// Conflicted paths are *not* placed into the resulting [`MergeResult::tree`];
/// the caller is expected to resolve each conflict (e.g. via
/// [`crate::merge_blobs`] for `Content` conflicts) and insert the resolved
/// hash itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeConflict {
    /// Both sides modified the same path to different content.
    Content {
        /// The conflicted path.
        path: String,
        /// Hash of the content at the common ancestor.
        base_hash: Hash,
        /// Hash of the content on our side.
        our_hash: Hash,
        /// Hash of the content on their side.
        their_hash: Hash,
    },
    /// One side modified the file, the other deleted it.
    /// `modified_hash` is the hash of the surviving (modified) version,
    /// and `modifier` indicates which side did the modification.
    ModifyDelete {
        /// The conflicted path.
        path: String,
        /// Hash of the modified version (on the side that didn't delete).
        modified_hash: Hash,
        /// Which side performed the modification (the other deleted).
        modifier: Side,
    },
    /// Both sides added the same path with different content (no shared base
    /// for that path).
    AddAdd {
        /// The conflicted path.
        path: String,
        /// Hash of the content on our side.
        our_hash: Hash,
        /// Hash of the content on their side.
        their_hash: Hash,
    },
}

impl MergeConflict {
    /// The path this conflict refers to.
    pub fn path(&self) -> &str {
        match self {
            MergeConflict::Content { path, .. }
            | MergeConflict::ModifyDelete { path, .. }
            | MergeConflict::AddAdd { path, .. } => path,
        }
    }
}

/// Result of a three-way tree merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    /// The auto-merged tree. Conflicted paths are excluded; resolve them and
    /// insert manually.
    pub tree: TreeSnapshot,
    /// Conflicts that the caller must resolve.
    pub conflicts: Vec<MergeConflict>,
}

impl MergeResult {
    /// Convenience: true iff the merge produced no conflicts.
    pub fn is_clean(&self) -> bool {
        self.conflicts.is_empty()
    }
}

/// Result of a three-way blob (content) merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobMergeResult {
    /// The merged content. If `has_conflicts` is true, this contains
    /// git-style conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`) inline.
    pub content: Vec<u8>,
    /// True iff at least one chunk could not be auto-merged.
    pub has_conflicts: bool,
}

/// Errors from [`crate::apply_tree_diff`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyError {
    /// Tried to remove or modify a path that's not in the base tree.
    PathNotFound {
        /// The missing path.
        path: String,
    },
    /// A `Removed` or `Modified` change's `old_hash` doesn't match what's
    /// actually at that path in the base tree.
    HashMismatch {
        /// The path with mismatched hash.
        path: String,
        /// The hash the change said should be there.
        expected: Hash,
        /// The hash that's actually there.
        actual: Hash,
    },
    /// Tried to add a path that already exists in the base tree.
    PathAlreadyExists {
        /// The conflicting path.
        path: String,
    },
}
