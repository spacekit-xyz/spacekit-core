//! Tree-level diff and merge operations.
//!
//! These work entirely on `TreeSnapshot` (path → hash) and never touch blob
//! content. For content-level operations see [`crate::blob`].

use alloc::string::ToString;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::types::*;

// ---------------------------------------------------------------------------
// diff_trees: ordered merge-join in O(n + m).
// ---------------------------------------------------------------------------

/// Compute the changeset that turns `base` into `head`.
///
/// The result is sorted by path (because both inputs are sorted).
///
/// # Complexity
/// O(n + m) over the combined entry counts; no allocation beyond the output.
pub fn diff_trees(base: &TreeSnapshot, head: &TreeSnapshot) -> Vec<TreeChange> {
    let mut out = Vec::new();
    let mut a = base.entries.iter().peekable();
    let mut b = head.entries.iter().peekable();

    loop {
        match (a.peek(), b.peek()) {
            (None, None) => break,
            (Some((p, h)), None) => {
                out.push(TreeChange::Removed {
                    path: (*p).clone(),
                    hash: **h,
                });
                a.next();
            }
            (None, Some((p, h))) => {
                out.push(TreeChange::Added {
                    path: (*p).clone(),
                    hash: **h,
                });
                b.next();
            }
            (Some((pa, ha)), Some((pb, hb))) => match pa.cmp(pb) {
                Ordering::Less => {
                    out.push(TreeChange::Removed {
                        path: (*pa).clone(),
                        hash: **ha,
                    });
                    a.next();
                }
                Ordering::Greater => {
                    out.push(TreeChange::Added {
                        path: (*pb).clone(),
                        hash: **hb,
                    });
                    b.next();
                }
                Ordering::Equal => {
                    if ha != hb {
                        out.push(TreeChange::Modified {
                            path: (*pa).clone(),
                            old_hash: **ha,
                            new_hash: **hb,
                        });
                    }
                    a.next();
                    b.next();
                }
            },
        }
    }
    out
}

// ---------------------------------------------------------------------------
// apply_tree_diff: replay a changeset onto a base tree.
// ---------------------------------------------------------------------------

/// Apply a sequence of [`TreeChange`]s to a base snapshot.
///
/// Each change is validated against the current state: removing or modifying
/// a non-existent path, or providing the wrong `old_hash`, returns an error.
pub fn apply_tree_diff(
    base: &TreeSnapshot,
    changes: &[TreeChange],
) -> Result<TreeSnapshot, ApplyError> {
    let mut tree = base.clone();
    for change in changes {
        match change {
            TreeChange::Added { path, hash } => {
                if tree.entries.contains_key(path) {
                    return Err(ApplyError::PathAlreadyExists { path: path.clone() });
                }
                tree.entries.insert(path.clone(), *hash);
            }
            TreeChange::Removed { path, hash } => match tree.entries.get(path).copied() {
                None => return Err(ApplyError::PathNotFound { path: path.clone() }),
                Some(actual) if actual != *hash => {
                    return Err(ApplyError::HashMismatch {
                        path: path.clone(),
                        expected: *hash,
                        actual,
                    });
                }
                Some(_) => {
                    tree.entries.remove(path);
                }
            },
            TreeChange::Modified {
                path,
                old_hash,
                new_hash,
            } => match tree.entries.get(path).copied() {
                None => return Err(ApplyError::PathNotFound { path: path.clone() }),
                Some(actual) if actual != *old_hash => {
                    return Err(ApplyError::HashMismatch {
                        path: path.clone(),
                        expected: *old_hash,
                        actual,
                    });
                }
                Some(_) => {
                    tree.entries.insert(path.clone(), *new_hash);
                }
            },
        }
    }
    Ok(tree)
}

// ---------------------------------------------------------------------------
// detect_exact_renames: pair Removed+Added with identical content hashes.
// ---------------------------------------------------------------------------

/// Detect exact (content-identical) renames within a changeset.
///
/// Pairs each [`TreeChange::Removed`] with a [`TreeChange::Added`] that has the
/// same hash. Returns the detected [`Rename`]s plus the changes that were *not*
/// consumed by a rename (unpaired adds/removes and all modifications), so the
/// caller can present `R old -> new` lines and ordinary add/remove/modify lines
/// without double-counting.
///
/// When several adds/removes share one hash, pairings are made greedily in the
/// input order; leftovers fall through as plain adds/removes.
pub fn detect_exact_renames(changes: &[TreeChange]) -> (Vec<Rename>, Vec<TreeChange>) {
    use alloc::collections::BTreeMap;

    // Bucket removed and added paths by hash, preserving order.
    let mut removed_by_hash: BTreeMap<Hash, Vec<usize>> = BTreeMap::new();
    let mut added_by_hash: BTreeMap<Hash, Vec<usize>> = BTreeMap::new();
    for (i, c) in changes.iter().enumerate() {
        match c {
            TreeChange::Removed { hash, .. } => removed_by_hash.entry(*hash).or_default().push(i),
            TreeChange::Added { hash, .. } => added_by_hash.entry(*hash).or_default().push(i),
            TreeChange::Modified { .. } => {}
        }
    }

    let mut consumed = alloc::vec![false; changes.len()];
    let mut renames = Vec::new();
    for (hash, rem_idxs) in &removed_by_hash {
        if let Some(add_idxs) = added_by_hash.get(hash) {
            let pairs = rem_idxs.len().min(add_idxs.len());
            for p in 0..pairs {
                let ri = rem_idxs[p];
                let ai = add_idxs[p];
                consumed[ri] = true;
                consumed[ai] = true;
                renames.push(Rename {
                    from: changes[ri].path().to_string(),
                    to: changes[ai].path().to_string(),
                    hash: *hash,
                });
            }
        }
    }

    let remaining = changes
        .iter()
        .enumerate()
        .filter(|(i, _)| !consumed[*i])
        .map(|(_, c)| c.clone())
        .collect();
    (renames, remaining)
}

// ---------------------------------------------------------------------------
// merge_trees: three-way tree merge with git-style rules.
// ---------------------------------------------------------------------------

/// Three-way merge of `ours` and `theirs` against a common ancestor `base`.
///
/// See [`MergeConflict`] for the kinds of conflicts produced. Conflicted
/// paths are excluded from the result tree.
///
/// # Complexity
/// O(n + m + k) over the combined entry counts via parallel iteration of all
/// three sorted maps.
pub fn merge_trees(base: &TreeSnapshot, ours: &TreeSnapshot, theirs: &TreeSnapshot) -> MergeResult {
    let mut tree = TreeSnapshot::new();
    let mut conflicts = Vec::new();

    // Walk all three sorted maps in lockstep, picking the smallest path each
    // round. Any combination of "present in base / ours / theirs" is then
    // dispatched to `merge_one`.
    let mut a = base.entries.iter().peekable();
    let mut b = ours.entries.iter().peekable();
    let mut c = theirs.entries.iter().peekable();

    loop {
        // Find the smallest path among the three iterators (if any).
        let next_path = {
            let pa = a.peek().map(|(p, _)| *p);
            let pb = b.peek().map(|(p, _)| *p);
            let pc = c.peek().map(|(p, _)| *p);
            match (pa, pb, pc) {
                (None, None, None) => break,
                _ => [pa, pb, pc]
                    .into_iter()
                    .flatten()
                    .min()
                    .expect("at least one iterator non-empty")
                    .clone(),
            }
        };

        // Pull the entry for `next_path` out of each iterator that has it.
        let take = |it: &mut core::iter::Peekable<
            alloc::collections::btree_map::Iter<'_, alloc::string::String, Hash>,
        >,
                    target: &str|
         -> Option<Hash> {
            match it.peek() {
                Some((p, h)) if p.as_str() == target => {
                    let h = **h;
                    it.next();
                    Some(h)
                }
                _ => None,
            }
        };

        let bh = take(&mut a, &next_path);
        let oh = take(&mut b, &next_path);
        let th = take(&mut c, &next_path);

        merge_one(&next_path, bh, oh, th, &mut tree, &mut conflicts);
    }

    MergeResult { tree, conflicts }
}

/// Decide the fate of a single path given its (base, ours, theirs) hashes.
///
/// The 8-cell truth table on Option presence is exhaustive; convergent and
/// no-op edits are recognized so they don't produce conflicts.
fn merge_one(
    path: &str,
    base: Option<Hash>,
    ours: Option<Hash>,
    theirs: Option<Hash>,
    tree: &mut TreeSnapshot,
    conflicts: &mut Vec<MergeConflict>,
) {
    match (base, ours, theirs) {
        // Path absent everywhere — caller wouldn't reach this branch, but
        // be safe.
        (None, None, None) => {}

        // Added on theirs only.
        (None, None, Some(th)) => {
            tree.entries.insert(path.to_string(), th);
        }
        // Added on ours only.
        (None, Some(ou), None) => {
            tree.entries.insert(path.to_string(), ou);
        }
        // Added on both: convergent if same hash, otherwise an AddAdd
        // conflict — there's no shared base to use as a tiebreaker.
        (None, Some(ou), Some(th)) => {
            if ou == th {
                tree.entries.insert(path.to_string(), ou);
            } else {
                conflicts.push(MergeConflict::AddAdd {
                    path: path.to_string(),
                    our_hash: ou,
                    their_hash: th,
                });
            }
        }

        // Both deleted: stays deleted.
        (Some(_), None, None) => {}

        // Ours deleted, theirs present: clean delete iff theirs didn't
        // change the file from base; otherwise ModifyDelete.
        (Some(ba), None, Some(th)) => {
            if ba == th {
                // theirs unchanged, ours deleted -> delete wins.
            } else {
                conflicts.push(MergeConflict::ModifyDelete {
                    path: path.to_string(),
                    modified_hash: th,
                    modifier: Side::Theirs,
                });
            }
        }
        // Symmetric.
        (Some(ba), Some(ou), None) => {
            if ba == ou {
                // ours unchanged, theirs deleted -> delete wins.
            } else {
                conflicts.push(MergeConflict::ModifyDelete {
                    path: path.to_string(),
                    modified_hash: ou,
                    modifier: Side::Ours,
                });
            }
        }

        // Present in all three: the interesting case.
        (Some(ba), Some(ou), Some(th)) => {
            if ou == th {
                // Convergent edit (or both unchanged): take it.
                tree.entries.insert(path.to_string(), ou);
            } else if ou == ba {
                // Ours unchanged, take theirs.
                tree.entries.insert(path.to_string(), th);
            } else if th == ba {
                // Theirs unchanged, take ours.
                tree.entries.insert(path.to_string(), ou);
            } else {
                // Genuine content conflict: both sides changed differently.
                conflicts.push(MergeConflict::Content {
                    path: path.to_string(),
                    base_hash: ba,
                    our_hash: ou,
                    their_hash: th,
                });
            }
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn h(b: u8) -> Hash {
        [b; 32]
    }

    fn snap(pairs: &[(&str, u8)]) -> TreeSnapshot {
        TreeSnapshot::from_pairs(pairs.iter().map(|(p, b)| (p.to_string(), h(*b))))
    }

    // ---- diff_trees -------------------------------------------------------

    #[test]
    fn diff_empty_trees_is_empty() {
        assert!(diff_trees(&TreeSnapshot::new(), &TreeSnapshot::new()).is_empty());
    }

    #[test]
    fn diff_addition() {
        let d = diff_trees(&snap(&[]), &snap(&[("a", 1)]));
        assert_eq!(
            d,
            vec![TreeChange::Added {
                path: "a".into(),
                hash: h(1)
            }]
        );
    }

    #[test]
    fn diff_removal() {
        let d = diff_trees(&snap(&[("a", 1)]), &snap(&[]));
        assert_eq!(
            d,
            vec![TreeChange::Removed {
                path: "a".into(),
                hash: h(1)
            }]
        );
    }

    #[test]
    fn diff_modification() {
        let d = diff_trees(&snap(&[("a", 1)]), &snap(&[("a", 2)]));
        assert_eq!(
            d,
            vec![TreeChange::Modified {
                path: "a".into(),
                old_hash: h(1),
                new_hash: h(2),
            }]
        );
    }

    #[test]
    fn diff_unchanged_path_is_omitted() {
        let d = diff_trees(&snap(&[("a", 1)]), &snap(&[("a", 1)]));
        assert!(d.is_empty());
    }

    #[test]
    fn diff_mixed_changes_are_sorted_by_path() {
        // base: a=1, c=3   head: b=2, c=4
        let d = diff_trees(&snap(&[("a", 1), ("c", 3)]), &snap(&[("b", 2), ("c", 4)]));
        assert_eq!(
            d,
            vec![
                TreeChange::Removed {
                    path: "a".into(),
                    hash: h(1)
                },
                TreeChange::Added {
                    path: "b".into(),
                    hash: h(2)
                },
                TreeChange::Modified {
                    path: "c".into(),
                    old_hash: h(3),
                    new_hash: h(4),
                },
            ]
        );
    }

    // ---- apply_tree_diff --------------------------------------------------

    #[test]
    fn apply_round_trips_with_diff() {
        let base = snap(&[("a", 1), ("c", 3)]);
        let head = snap(&[("b", 2), ("c", 4), ("d", 5)]);
        let changes = diff_trees(&base, &head);
        let reconstructed = apply_tree_diff(&base, &changes).unwrap();
        assert_eq!(reconstructed, head);
    }

    #[test]
    fn apply_rejects_remove_of_missing_path() {
        let base = snap(&[]);
        let err = apply_tree_diff(
            &base,
            &[TreeChange::Removed {
                path: "x".into(),
                hash: h(1),
            }],
        )
        .unwrap_err();
        assert_eq!(err, ApplyError::PathNotFound { path: "x".into() });
    }

    #[test]
    fn apply_rejects_add_over_existing_path() {
        let base = snap(&[("a", 1)]);
        let err = apply_tree_diff(
            &base,
            &[TreeChange::Added {
                path: "a".into(),
                hash: h(2),
            }],
        )
        .unwrap_err();
        assert_eq!(err, ApplyError::PathAlreadyExists { path: "a".into() });
    }

    #[test]
    fn apply_rejects_modify_with_wrong_old_hash() {
        let base = snap(&[("a", 1)]);
        let err = apply_tree_diff(
            &base,
            &[TreeChange::Modified {
                path: "a".into(),
                old_hash: h(9),
                new_hash: h(2),
            }],
        )
        .unwrap_err();
        assert_eq!(
            err,
            ApplyError::HashMismatch {
                path: "a".into(),
                expected: h(9),
                actual: h(1),
            }
        );
    }

    // ---- merge_trees ------------------------------------------------------

    #[test]
    fn merge_clean_when_only_one_side_modifies() {
        // base: a=1   ours: a=1   theirs: a=2  →  a=2, no conflict
        let r = merge_trees(&snap(&[("a", 1)]), &snap(&[("a", 1)]), &snap(&[("a", 2)]));
        assert!(r.is_clean());
        assert_eq!(r.tree, snap(&[("a", 2)]));
    }

    #[test]
    fn merge_clean_when_both_sides_make_same_edit() {
        let r = merge_trees(&snap(&[("a", 1)]), &snap(&[("a", 2)]), &snap(&[("a", 2)]));
        assert!(r.is_clean());
        assert_eq!(r.tree, snap(&[("a", 2)]));
    }

    #[test]
    fn merge_content_conflict_when_both_sides_differ() {
        let r = merge_trees(&snap(&[("a", 1)]), &snap(&[("a", 2)]), &snap(&[("a", 3)]));
        assert_eq!(r.conflicts.len(), 1);
        assert_eq!(
            r.conflicts[0],
            MergeConflict::Content {
                path: "a".into(),
                base_hash: h(1),
                our_hash: h(2),
                their_hash: h(3),
            }
        );
        // Conflicted path is *not* in the merged tree.
        assert!(!r.tree.entries.contains_key("a"));
    }

    #[test]
    fn merge_addadd_conflict_when_both_add_different_content() {
        let r = merge_trees(&snap(&[]), &snap(&[("a", 2)]), &snap(&[("a", 3)]));
        assert_eq!(
            r.conflicts,
            vec![MergeConflict::AddAdd {
                path: "a".into(),
                our_hash: h(2),
                their_hash: h(3),
            }]
        );
        assert!(r.tree.is_empty());
    }

    #[test]
    fn merge_addadd_clean_when_both_add_same_content() {
        let r = merge_trees(&snap(&[]), &snap(&[("a", 7)]), &snap(&[("a", 7)]));
        assert!(r.is_clean());
        assert_eq!(r.tree, snap(&[("a", 7)]));
    }

    #[test]
    fn merge_modify_delete_when_ours_modifies_theirs_deletes() {
        let r = merge_trees(&snap(&[("a", 1)]), &snap(&[("a", 2)]), &snap(&[]));
        assert_eq!(
            r.conflicts,
            vec![MergeConflict::ModifyDelete {
                path: "a".into(),
                modified_hash: h(2),
                modifier: Side::Ours,
            }]
        );
    }

    #[test]
    fn merge_modify_delete_when_theirs_modifies_ours_deletes() {
        let r = merge_trees(&snap(&[("a", 1)]), &snap(&[]), &snap(&[("a", 2)]));
        assert_eq!(
            r.conflicts,
            vec![MergeConflict::ModifyDelete {
                path: "a".into(),
                modified_hash: h(2),
                modifier: Side::Theirs,
            }]
        );
    }

    #[test]
    fn merge_clean_delete_when_one_side_deletes_other_unchanged() {
        // ours unchanged from base, theirs deleted → delete wins
        let r = merge_trees(&snap(&[("a", 1)]), &snap(&[("a", 1)]), &snap(&[]));
        assert!(r.is_clean());
        assert!(r.tree.is_empty());

        // symmetric
        let r = merge_trees(&snap(&[("a", 1)]), &snap(&[]), &snap(&[("a", 1)]));
        assert!(r.is_clean());
        assert!(r.tree.is_empty());
    }

    // ---- detect_exact_renames --------------------------------------------

    #[test]
    fn rename_pairs_identical_hash_add_and_remove() {
        // a (hash 1) removed, b (hash 1) added → rename a->b.
        let changes = diff_trees(&snap(&[("a", 1)]), &snap(&[("b", 1)]));
        let (renames, rest) = detect_exact_renames(&changes);
        assert_eq!(renames.len(), 1);
        assert_eq!(renames[0].from, "a");
        assert_eq!(renames[0].to, "b");
        assert!(rest.is_empty());
    }

    #[test]
    fn rename_leaves_unrelated_changes_alone() {
        // a->b rename plus an independent modify of c.
        let changes = diff_trees(&snap(&[("a", 1), ("c", 3)]), &snap(&[("b", 1), ("c", 4)]));
        let (renames, rest) = detect_exact_renames(&changes);
        assert_eq!(renames.len(), 1);
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].path(), "c");
        assert!(matches!(rest[0], TreeChange::Modified { .. }));
    }

    #[test]
    fn rename_not_detected_when_content_differs() {
        let changes = diff_trees(&snap(&[("a", 1)]), &snap(&[("b", 2)]));
        let (renames, rest) = detect_exact_renames(&changes);
        assert!(renames.is_empty());
        assert_eq!(rest.len(), 2);
    }

    #[test]
    fn merge_combines_independent_changes() {
        // base: a=1,b=2  ours: a=1,b=2,c=3  theirs: a=9,b=2
        // Expected clean merge: a=9 (theirs), b=2, c=3 (ours).
        let r = merge_trees(
            &snap(&[("a", 1), ("b", 2)]),
            &snap(&[("a", 1), ("b", 2), ("c", 3)]),
            &snap(&[("a", 9), ("b", 2)]),
        );
        assert!(r.is_clean(), "conflicts: {:?}", r.conflicts);
        assert_eq!(r.tree, snap(&[("a", 9), ("b", 2), ("c", 3)]));
    }
}
