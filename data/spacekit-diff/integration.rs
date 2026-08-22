//! Integration tests: realistic end-to-end flows and diff3 edge cases that
//! aren't easy to express alongside the unit tests.

use spacekit_diff::*;

fn h(byte: u8) -> Hash {
    [byte; 32]
}

// ---------------------------------------------------------------------------
// End-to-end: simulate the CLI workflow described in the spec.
// ---------------------------------------------------------------------------

#[test]
fn end_to_end_three_way_merge_and_apply() {
    // Construct a base, ours, theirs with mixed adds/removes/modifies and a
    // single content conflict on README.md. Then:
    //   1. merge_trees                  → MergeResult { tree, conflicts }
    //   2. merge_blobs on the conflict  → resolved content
    //   3. add the resolved hash into the merged tree
    //   4. diff base→merged             → final changeset
    //   5. apply_tree_diff(base, ...)   → reconstructs merged tree
    let base = TreeSnapshot::from_pairs([
        ("README.md".to_string(), h(1)),
        ("src/lib.rs".to_string(), h(10)),
        ("docs/old.md".to_string(), h(20)),
    ]);

    let ours = TreeSnapshot::from_pairs([
        ("README.md".to_string(), h(2)),         // modified by ours
        ("src/lib.rs".to_string(), h(10)),       // unchanged
        ("docs/old.md".to_string(), h(20)),      // unchanged
        ("src/feature.rs".to_string(), h(30)),   // added by ours
    ]);

    let theirs = TreeSnapshot::from_pairs([
        ("README.md".to_string(), h(3)),         // modified by theirs (conflict)
        ("src/lib.rs".to_string(), h(11)),       // modified by theirs only
        // docs/old.md deleted by theirs (was unchanged on ours)
        ("docs/new.md".to_string(), h(40)),      // added by theirs
    ]);

    let merge = merge_trees(&base, &ours, &theirs);

    // Conflict: README.md modified on both sides differently.
    assert_eq!(merge.conflicts.len(), 1);
    assert_eq!(merge.conflicts[0].path(), "README.md");
    match &merge.conflicts[0] {
        MergeConflict::Content { base_hash, our_hash, their_hash, .. } => {
            assert_eq!(*base_hash, h(1));
            assert_eq!(*our_hash, h(2));
            assert_eq!(*their_hash, h(3));
        }
        _ => panic!("expected Content conflict"),
    }

    // Auto-merged tree: everything except README.md.
    assert!(!merge.tree.entries.contains_key("README.md"));
    assert_eq!(merge.tree.entries.get("src/lib.rs"), Some(&h(11))); // theirs
    assert_eq!(merge.tree.entries.get("src/feature.rs"), Some(&h(30))); // ours
    assert!(!merge.tree.entries.contains_key("docs/old.md")); // theirs deleted, ours unchanged → delete wins
    assert_eq!(merge.tree.entries.get("docs/new.md"), Some(&h(40))); // theirs added

    // Caller resolves the conflict (here we just pick a fictional resolved hash).
    let mut final_tree = merge.tree;
    final_tree.insert("README.md", h(99));

    // Now compute the changeset base→final_tree and apply it: the result
    // should equal final_tree exactly (round-trip property).
    let changes = diff_trees(&base, &final_tree);
    let reconstructed = apply_tree_diff(&base, &changes).unwrap();
    assert_eq!(reconstructed, final_tree);
}

// ---------------------------------------------------------------------------
// diff3 edge cases.
// ---------------------------------------------------------------------------

#[test]
fn diff3_independent_inserts_at_adjacent_positions_merge_cleanly() {
    // base:  a   b   c
    // ours:  a O b   c     (insert O before b)
    // their: a   b T c     (insert T after b)
    // expected: a O b T c — both sides edit non-overlapping regions.
    let base = b"a\nb\nc\n";
    let ours = b"a\nO\nb\nc\n";
    let theirs = b"a\nb\nT\nc\n";
    let r = merge_blobs(base, ours, theirs);
    assert!(
        !r.has_conflicts,
        "expected clean merge, got: {}",
        String::from_utf8_lossy(&r.content)
    );
    assert_eq!(r.content, b"a\nO\nb\nT\nc\n");
}

#[test]
fn diff3_modify_one_line_delete_another_in_separate_regions() {
    // base:  one  two  three  four
    // ours:  ONE  two  three  four    (modify line 1)
    // their: one  two         four    (delete line 3)
    // expected: ONE two four
    let base = b"one\ntwo\nthree\nfour\n";
    let ours = b"ONE\ntwo\nthree\nfour\n";
    let theirs = b"one\ntwo\nfour\n";
    let r = merge_blobs(base, ours, theirs);
    assert!(!r.has_conflicts);
    assert_eq!(r.content, b"ONE\ntwo\nfour\n");
}

#[test]
fn diff3_overlapping_edits_produce_conflict_with_context_preserved() {
    // base:  a b c d e
    // ours:  a B C d e   (modify b and c)
    // their: a X Y d e   (modify b and c differently)
    let base = b"a\nb\nc\nd\ne\n";
    let ours = b"a\nB\nC\nd\ne\n";
    let theirs = b"a\nX\nY\nd\ne\n";
    let r = merge_blobs(base, ours, theirs);
    assert!(r.has_conflicts);
    let s = String::from_utf8(r.content).unwrap();
    // Surrounding shared context should be intact.
    assert!(s.starts_with("a\n"), "got: {s}");
    assert!(s.ends_with("d\ne\n"), "got: {s}");
    // Both sides' versions appear in the conflict block.
    assert!(s.contains("B\nC"));
    assert!(s.contains("X\nY"));
    assert!(s.contains("<<<<<<< ours"));
    assert!(s.contains(">>>>>>> theirs"));
}

#[test]
fn diff3_one_side_deletes_inside_others_modification_region() {
    // base:  a b c d e
    // ours:  a B c D e   (modify b and d, leaving c untouched)
    // their: a   c   e   (delete b and d)
    // The deletes and modifies overlap on b and d — the chunks aren't
    // independent. Expect a conflict.
    let base = b"a\nb\nc\nd\ne\n";
    let ours = b"a\nB\nc\nD\ne\n";
    let theirs = b"a\nc\ne\n";
    let r = merge_blobs(base, ours, theirs);
    assert!(r.has_conflicts);
}

#[test]
fn diff3_convergent_insertion_at_same_spot() {
    // Both sides insert the same line in the same place — should merge cleanly.
    let base = b"a\nz\n";
    let ours = b"a\nMID\nz\n";
    let theirs = b"a\nMID\nz\n";
    let r = merge_blobs(base, ours, theirs);
    assert!(!r.has_conflicts);
    assert_eq!(r.content, b"a\nMID\nz\n");
}

#[test]
fn diff_blobs_handles_completely_different_files() {
    // Pathological case: nothing in common. Should still produce a valid
    // diff (likely a single Replace or a Delete + Insert).
    let h = diff_blobs(b"x\ny\nz\n", b"a\nb\nc\n");
    let total_old: usize = h
        .iter()
        .map(|hunk| match hunk {
            DiffHunk::Equal { lines, .. } | DiffHunk::Delete { lines, .. } => lines.len(),
            DiffHunk::Replace { old_lines, .. } => old_lines.len(),
            _ => 0,
        })
        .sum();
    let total_new: usize = h
        .iter()
        .map(|hunk| match hunk {
            DiffHunk::Equal { lines, .. } | DiffHunk::Insert { lines, .. } => lines.len(),
            DiffHunk::Replace { new_lines, .. } => new_lines.len(),
            _ => 0,
        })
        .sum();
    assert_eq!(total_old, 3);
    assert_eq!(total_new, 3);
}

#[test]
fn merging_blobs_after_tree_merge_resolves_content_conflict() {
    // Full simulated workflow: tree merge surfaces a Content conflict, then
    // the caller fetches the three blobs and runs merge_blobs to attempt
    // automatic line-level resolution.
    let base_blob = b"intro\nbody\noutro\n";
    let our_blob = b"INTRO\nbody\noutro\n"; // ours edits intro
    let their_blob = b"intro\nbody\nOUTRO\n"; // theirs edits outro

    // (Tree-level: imagine these are the only files in three otherwise-empty
    // snapshots, and tree-merge would surface a Content conflict.)
    let blob_merge = merge_blobs(base_blob, our_blob, their_blob);
    assert!(!blob_merge.has_conflicts, "ought to be a clean diff3 merge");
    assert_eq!(blob_merge.content, b"INTRO\nbody\nOUTRO\n");
}
