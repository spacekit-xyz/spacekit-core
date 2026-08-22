//! Blob-level (line-by-line) diff and three-way merge.
//!
//! - [`diff_blobs`] implements Myers' algorithm
//!   ("An O(ND) Difference Algorithm and Its Variations", 1986), the same
//!   line-diff git uses internally. Output is a coalesced sequence of
//!   `Equal` / `Insert` / `Delete` / `Replace` hunks.
//!
//! - [`merge_blobs`] implements the diff3 algorithm: run Myers twice
//!   (`base→ours`, `base→theirs`), find common stable points, classify each
//!   chunk between them as convergent / take-ours / take-theirs / conflict,
//!   and emit git-style conflict markers for the unresolved chunks.
//!
//! All functions take `&[u8]` and produce `Vec<u8>` / `Vec<DiffHunk>` — no
//! UTF-8 assumptions on the file as a whole, only per-line lossy decode for
//! the human-readable strings inside `DiffHunk`.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::types::*;

// ---------------------------------------------------------------------------
// Helpers: line splitting and binary detection.
// ---------------------------------------------------------------------------

/// Bytes of prefix scanned for binary detection.
const BINARY_PROBE: usize = 8 * 1024;

/// Heuristic: a NUL byte in the first 8 KiB means "treat as binary".
/// Same heuristic git uses by default.
fn is_binary(data: &[u8]) -> bool {
    let n = data.len().min(BINARY_PROBE);
    data[..n].contains(&0)
}

/// Split `data` into lines, preserving the trailing `\n` of each line.
/// The last line is included even if it has no trailing newline. Concatenating
/// the resulting slices reproduces `data` exactly.
fn split_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, &b) in data.iter().enumerate() {
        if b == b'\n' {
            out.push(&data[start..=i]);
            start = i + 1;
        }
    }
    if start < data.len() {
        out.push(&data[start..]);
    }
    out
}

/// Convert a borrowed line to an owned `String` for use in `DiffHunk`s,
/// using `from_utf8_lossy` so non-UTF-8 inputs don't panic.
fn line_to_string(line: &[u8]) -> String {
    String::from_utf8_lossy(line).into_owned()
}

// ---------------------------------------------------------------------------
// Myers' diff algorithm.
// ---------------------------------------------------------------------------

/// One step in a Myers edit script, indexing into the original (`old_idx`)
/// or new (`new_idx`) sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditOp {
    Equal { old_idx: usize, new_idx: usize },
    Insert { new_idx: usize },
    Delete { old_idx: usize },
}

/// Run Myers' algorithm and return the edit script, ordered from start to
/// end of both sequences.
///
/// Memory: O(D · (N+M)) for the V-array snapshots in `trace`, where D is the
/// final edit distance. Plenty fast for typical text files; for very large
/// or very-different inputs you'd want the linear-space variant.
fn myers_diff<T: PartialEq>(old: &[T], new: &[T]) -> Vec<EditOp> {
    let n = old.len();
    let m = new.len();

    // Trivial cases.
    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        return (0..m).map(|j| EditOp::Insert { new_idx: j }).collect();
    }
    if m == 0 {
        return (0..n).map(|i| EditOp::Delete { old_idx: i }).collect();
    }

    let max = n + m;
    // V is indexed by k ∈ [-max, max], so we offset by `max` and allocate
    // 2*max + 1 cells.
    let offset = max as isize;
    let mut v: Vec<isize> = vec![0; 2 * max + 1];
    // Snapshot of V before each d-iteration. `trace[d]` holds V at edit
    // distance d-1 (and `trace[0]` is the initial all-zero V).
    let mut trace: Vec<Vec<isize>> = Vec::new();

    let mut found = false;
    let mut final_d: isize = 0;

    'outer: for d in 0..=max as isize {
        trace.push(v.clone());

        let mut k = -d;
        while k <= d {
            let idx = (k + offset) as usize;
            // Move that extends the furthest: insertion (from k+1) vs
            // deletion (from k-1). At the boundaries, only one is valid.
            let mut x: isize = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                v[idx + 1] // insertion: y advances, x same
            } else {
                v[idx - 1] + 1 // deletion: x advances
            };
            let mut y: isize = x - k;

            // Walk diagonals as long as the lines match.
            while (x as usize) < n && (y as usize) < m && old[x as usize] == new[y as usize] {
                x += 1;
                y += 1;
            }

            v[idx] = x;

            if x as usize >= n && y as usize >= m {
                found = true;
                final_d = d;
                break 'outer;
            }
            k += 2;
        }
    }

    debug_assert!(found, "Myers always terminates within d ≤ N+M");
    if !found {
        return Vec::new();
    }

    // Backtrack through `trace` to reconstruct the edit script.
    let mut ops: Vec<EditOp> = Vec::new();
    let mut x = n as isize;
    let mut y = m as isize;

    for d in (0..=final_d).rev() {
        if d == 0 {
            // Base case: walk diagonals all the way to the origin.
            while x > 0 && y > 0 {
                ops.push(EditOp::Equal {
                    old_idx: (x - 1) as usize,
                    new_idx: (y - 1) as usize,
                });
                x -= 1;
                y -= 1;
            }
            break;
        }

        let v_prev = &trace[d as usize];
        let k = x - y;
        let idx = (k + offset) as usize;
        let prev_k = if k == -d || (k != d && v_prev[idx - 1] < v_prev[idx + 1]) {
            k + 1 // came from above: insertion
        } else {
            k - 1 // came from left: deletion
        };
        let prev_idx = (prev_k + offset) as usize;
        let prev_x = v_prev[prev_idx];
        let prev_y = prev_x - prev_k;

        // Diagonal moves recorded at the *end* of the d-th step (suffix
        // matches).
        while x > prev_x && y > prev_y {
            ops.push(EditOp::Equal {
                old_idx: (x - 1) as usize,
                new_idx: (y - 1) as usize,
            });
            x -= 1;
            y -= 1;
        }

        // The single edit at this distance.
        if x == prev_x {
            ops.push(EditOp::Insert {
                new_idx: (y - 1) as usize,
            });
        } else {
            ops.push(EditOp::Delete {
                old_idx: (x - 1) as usize,
            });
        }
        x = prev_x;
        y = prev_y;
    }

    ops.reverse();
    ops
}

// ---------------------------------------------------------------------------
// diff_blobs: public entry, with hunk coalescing.
// ---------------------------------------------------------------------------

/// Line-level diff between two blobs.
///
/// For binary inputs (per [`is_binary`]) returns a single `Replace` hunk
/// that summarizes sizes rather than line content.
pub fn diff_blobs(old: &[u8], new: &[u8]) -> Vec<DiffHunk> {
    if old == new {
        // Common fast-path: identical content. We still emit a single Equal
        // hunk so callers iterating hunks see something consistent.
        let lines = split_lines(old).iter().map(|l| line_to_string(l)).collect();
        return vec![DiffHunk::Equal {
            old_start: 0,
            new_start: 0,
            lines,
        }];
    }

    if is_binary(old) || is_binary(new) {
        return vec![DiffHunk::Replace {
            old_start: 0,
            old_lines: vec![format!("<binary content, {} bytes>", old.len())],
            new_start: 0,
            new_lines: vec![format!("<binary content, {} bytes>", new.len())],
        }];
    }

    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let ops = myers_diff(&old_lines, &new_lines);
    coalesce(&ops, &old_lines, &new_lines)
}

/// Coalesce a per-line edit script into multi-line `DiffHunk`s, merging
/// `Delete` immediately followed by `Insert` into a single `Replace`.
fn coalesce(ops: &[EditOp], old_lines: &[&[u8]], new_lines: &[&[u8]]) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut i = 0;

    while i < ops.len() {
        match ops[i] {
            EditOp::Equal { old_idx, new_idx } => {
                let o0 = old_idx;
                let n0 = new_idx;
                let mut lines = Vec::new();
                while let Some(EditOp::Equal { old_idx, .. }) = ops.get(i) {
                    lines.push(line_to_string(old_lines[*old_idx]));
                    i += 1;
                }
                hunks.push(DiffHunk::Equal {
                    old_start: o0,
                    new_start: n0,
                    lines,
                });
            }
            EditOp::Delete { old_idx } => {
                let o0 = old_idx;
                let mut del_lines = Vec::new();
                while let Some(EditOp::Delete { old_idx }) = ops.get(i) {
                    del_lines.push(line_to_string(old_lines[*old_idx]));
                    i += 1;
                }
                // Coalesce a Delete-run immediately followed by an
                // Insert-run into a single Replace hunk.
                if let Some(EditOp::Insert { new_idx }) = ops.get(i) {
                    let n0 = *new_idx;
                    let mut ins_lines = Vec::new();
                    while let Some(EditOp::Insert { new_idx }) = ops.get(i) {
                        ins_lines.push(line_to_string(new_lines[*new_idx]));
                        i += 1;
                    }
                    hunks.push(DiffHunk::Replace {
                        old_start: o0,
                        old_lines: del_lines,
                        new_start: n0,
                        new_lines: ins_lines,
                    });
                } else {
                    hunks.push(DiffHunk::Delete {
                        old_start: o0,
                        lines: del_lines,
                    });
                }
            }
            EditOp::Insert { new_idx } => {
                let n0 = new_idx;
                let mut ins_lines = Vec::new();
                while let Some(EditOp::Insert { new_idx }) = ops.get(i) {
                    ins_lines.push(line_to_string(new_lines[*new_idx]));
                    i += 1;
                }
                hunks.push(DiffHunk::Insert {
                    new_start: n0,
                    lines: ins_lines,
                });
            }
        }
    }
    hunks
}

// ---------------------------------------------------------------------------
// merge_blobs: diff3 implementation.
// ---------------------------------------------------------------------------

/// Three-way line-level merge of `ours` and `theirs` against `base`.
///
/// On unresolved chunks, embeds git-style conflict markers in the output:
///
/// ```text
/// <<<<<<< ours
/// our changed lines
/// =======
/// their changed lines
/// >>>>>>> theirs
/// ```
pub fn merge_blobs(base: &[u8], ours: &[u8], theirs: &[u8]) -> BlobMergeResult {
    // Cheap fast paths handle the most common cases without running diff at
    // all — and also make the algorithm correct for trivial inputs where
    // diff3's chunk model is overkill.
    if ours == theirs {
        return BlobMergeResult {
            content: ours.to_vec(),
            has_conflicts: false,
        };
    }
    if base == ours {
        // Ours unchanged → take theirs.
        return BlobMergeResult {
            content: theirs.to_vec(),
            has_conflicts: false,
        };
    }
    if base == theirs {
        // Theirs unchanged → take ours.
        return BlobMergeResult {
            content: ours.to_vec(),
            has_conflicts: false,
        };
    }

    // Binary content can't be line-merged. Emit a single whole-file
    // conflict block so the caller's diff/merge UI still has something to
    // surface.
    if is_binary(base) || is_binary(ours) || is_binary(theirs) {
        let mut content = Vec::new();
        content.extend_from_slice(b"<<<<<<< ours\n");
        content.extend_from_slice(ours);
        if !content.ends_with(b"\n") {
            content.push(b'\n');
        }
        content.extend_from_slice(b"=======\n");
        content.extend_from_slice(theirs);
        if !content.ends_with(b"\n") {
            content.push(b'\n');
        }
        content.extend_from_slice(b">>>>>>> theirs\n");
        return BlobMergeResult {
            content,
            has_conflicts: true,
        };
    }

    let base_lines = split_lines(base);
    let our_lines = split_lines(ours);
    let their_lines = split_lines(theirs);

    // Two LCS pairings: which base lines also survive in ours / theirs.
    let our_pairs = lcs_pairs(&myers_diff(&base_lines, &our_lines));
    let their_pairs = lcs_pairs(&myers_diff(&base_lines, &their_lines));

    diff3_merge(
        &base_lines,
        &our_lines,
        &their_lines,
        &our_pairs,
        &their_pairs,
    )
}

/// Pull just the matched (base_idx, target_idx) pairs out of an edit script.
/// These are the points where a base line survives unchanged on the target
/// side — the candidates for diff3 sync points.
fn lcs_pairs(ops: &[EditOp]) -> Vec<(usize, usize)> {
    ops.iter()
        .filter_map(|op| match op {
            EditOp::Equal { old_idx, new_idx } => Some((*old_idx, *new_idx)),
            _ => None,
        })
        .collect()
}

/// The diff3 chunking loop.
///
/// Walks the two LCS pair lists in parallel, repeatedly finding the next
/// base index that's matched in *both* (a "stable" sync point). The lines
/// strictly between consecutive sync points form a chunk on each of the
/// three sides; that chunk is then classified.
fn diff3_merge(
    base: &[&[u8]],
    ours: &[&[u8]],
    theirs: &[&[u8]],
    our_pairs: &[(usize, usize)],
    their_pairs: &[(usize, usize)],
) -> BlobMergeResult {
    let mut content: Vec<u8> = Vec::new();
    let mut has_conflicts = false;

    let mut oi = 0usize; // index into our_pairs
    let mut ti = 0usize; // index into their_pairs

    let mut base_pos = 0usize;
    let mut our_pos = 0usize;
    let mut their_pos = 0usize;

    loop {
        // Skip any pair whose base index is behind base_pos. This happens
        // because our_pairs / their_pairs are independent; one side may
        // have advanced past a stale pair.
        while oi < our_pairs.len() && our_pairs[oi].0 < base_pos {
            oi += 1;
        }
        while ti < their_pairs.len() && their_pairs[ti].0 < base_pos {
            ti += 1;
        }

        // Find the next base index that's matched on both sides. We may
        // need to advance one cursor several times to catch up.
        let mut sync: Option<(usize, usize, usize)> = None;
        while oi < our_pairs.len() && ti < their_pairs.len() {
            let (b_o, j) = our_pairs[oi];
            let (b_t, k) = their_pairs[ti];
            if b_o == b_t {
                // But the matched our_idx / their_idx must also be ≥ our
                // current our_pos / their_pos respectively, otherwise this
                // sync point would require us to "go back" on one side
                // (which happens when ordering of LCS pairs gets crossed).
                // Skip stale ones.
                if j < our_pos {
                    oi += 1;
                    continue;
                }
                if k < their_pos {
                    ti += 1;
                    continue;
                }
                sync = Some((b_o, j, k));
                break;
            } else if b_o < b_t {
                oi += 1;
            } else {
                ti += 1;
            }
        }

        let (next_base, next_our, next_their) = match sync {
            Some(s) => s,
            None => (base.len(), ours.len(), theirs.len()),
        };

        // Process the chunk strictly *before* the sync point.
        let base_chunk = &base[base_pos..next_base];
        let our_chunk = &ours[our_pos..next_our];
        let their_chunk = &theirs[their_pos..next_their];

        if !base_chunk.is_empty() || !our_chunk.is_empty() || !their_chunk.is_empty() {
            let base_eq_our = slices_eq(base_chunk, our_chunk);
            let base_eq_their = slices_eq(base_chunk, their_chunk);
            let our_eq_their = slices_eq(our_chunk, their_chunk);

            if our_eq_their {
                // Convergent (or both unchanged from base).
                emit_lines(&mut content, our_chunk);
            } else if base_eq_our {
                // Ours unchanged from base → take theirs' edit.
                emit_lines(&mut content, their_chunk);
            } else if base_eq_their {
                // Theirs unchanged from base → take ours' edit.
                emit_lines(&mut content, our_chunk);
            } else {
                // True conflict: both sides edited the same span differently.
                has_conflicts = true;
                content.extend_from_slice(b"<<<<<<< ours\n");
                emit_lines(&mut content, our_chunk);
                ensure_newline(&mut content);
                content.extend_from_slice(b"=======\n");
                emit_lines(&mut content, their_chunk);
                ensure_newline(&mut content);
                content.extend_from_slice(b">>>>>>> theirs\n");
            }
        }

        match sync {
            None => break,
            Some(_) => {
                // The sync line itself is shared content. Emit it (with its
                // own trailing newline if it had one).
                content.extend_from_slice(base[next_base]);
                base_pos = next_base + 1;
                our_pos = next_our + 1;
                their_pos = next_their + 1;
                oi += 1;
                ti += 1;
            }
        }
    }

    BlobMergeResult {
        content,
        has_conflicts,
    }
}

fn emit_lines(buf: &mut Vec<u8>, lines: &[&[u8]]) {
    for line in lines {
        buf.extend_from_slice(line);
    }
}

/// Append `\n` to `buf` if it doesn't already end with one. Used between
/// chunks of a conflict block to keep marker lines flush-left.
fn ensure_newline(buf: &mut Vec<u8>) {
    if !buf.is_empty() && !buf.ends_with(b"\n") {
        buf.push(b'\n');
    }
}

fn slices_eq(a: &[&[u8]], b: &[&[u8]]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

// ---------------------------------------------------------------------------
// unified_diff: render a git-style unified diff with `context` lines.
// ---------------------------------------------------------------------------

/// One classified output line for unified rendering.
struct UniLine {
    /// `b' '` context, `b'-'` removed, `b'+'` added.
    tag: u8,
    old_no: usize,
    new_no: usize,
    text: String,
    /// Whether the source line ended with `\n`.
    had_newline: bool,
}

fn push_text_lines(
    out: &mut Vec<UniLine>,
    tag: u8,
    lines: &[String],
    old_no: &mut usize,
    new_no: &mut usize,
) {
    for l in lines {
        let had_newline = l.ends_with('\n');
        let text = l.trim_end_matches('\n').to_string();
        let (o, n) = match tag {
            b' ' => {
                let pair = (*old_no, *new_no);
                *old_no += 1;
                *new_no += 1;
                pair
            }
            b'-' => {
                let o = *old_no;
                *old_no += 1;
                (o, 0)
            }
            _ => {
                let n = *new_no;
                *new_no += 1;
                (0, n)
            }
        };
        out.push(UniLine {
            tag,
            old_no: o,
            new_no: n,
            text,
            had_newline,
        });
    }
}

/// Produce a git-style unified diff between `old` and `new`.
///
/// `old_label` / `new_label` become the `---` / `+++` header paths. `context`
/// is the number of unchanged lines kept around each change (git default: 3).
/// Returns an empty string when the inputs are identical.
pub fn unified_diff(
    old: &[u8],
    new: &[u8],
    old_label: &str,
    new_label: &str,
    context: usize,
) -> String {
    if old == new {
        return String::new();
    }

    // Binary short-circuit: report a one-line summary rather than bytes.
    if is_binary(old) || is_binary(new) {
        return format!(
            "--- {}\n+++ {}\nBinary files differ ({} -> {} bytes)\n",
            old_label,
            new_label,
            old.len(),
            new.len()
        );
    }

    // Flatten coalesced hunks into per-line tagged records (1-based numbers).
    let hunks = diff_blobs(old, new);
    let mut lines: Vec<UniLine> = Vec::new();
    let mut old_no = 1usize;
    let mut new_no = 1usize;
    for h in &hunks {
        match h {
            DiffHunk::Equal { lines: ls, .. } => {
                push_text_lines(&mut lines, b' ', ls, &mut old_no, &mut new_no)
            }
            DiffHunk::Delete { lines: ls, .. } => {
                push_text_lines(&mut lines, b'-', ls, &mut old_no, &mut new_no)
            }
            DiffHunk::Insert { lines: ls, .. } => {
                push_text_lines(&mut lines, b'+', ls, &mut old_no, &mut new_no)
            }
            DiffHunk::Replace {
                old_lines,
                new_lines,
                ..
            } => {
                push_text_lines(&mut lines, b'-', old_lines, &mut old_no, &mut new_no);
                push_text_lines(&mut lines, b'+', new_lines, &mut old_no, &mut new_no);
            }
        }
    }

    // Indices of changed lines.
    let changed: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.tag != b' ')
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return String::new();
    }

    // Group changed lines into hunks, padding by `context` and merging groups
    // that overlap once padded.
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let n = lines.len();
    for &ci in &changed {
        let start = ci.saturating_sub(context);
        let end = (ci + context + 1).min(n);
        match groups.last_mut() {
            Some((_, prev_end)) if start <= *prev_end => {
                if end > *prev_end {
                    *prev_end = end;
                }
            }
            _ => groups.push((start, end)),
        }
    }

    let mut out = String::new();
    out.push_str(&format!("--- {}\n+++ {}\n", old_label, new_label));
    for (start, end) in groups {
        let slice = &lines[start..end];
        let old_count = slice.iter().filter(|l| l.tag != b'+').count();
        let new_count = slice.iter().filter(|l| l.tag != b'-').count();
        let old_start = slice
            .iter()
            .find(|l| l.tag != b'+')
            .map(|l| l.old_no)
            .unwrap_or(0);
        let new_start = slice
            .iter()
            .find(|l| l.tag != b'-')
            .map(|l| l.new_no)
            .unwrap_or(0);
        out.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            old_start, old_count, new_start, new_count
        ));
        for l in slice {
            out.push(l.tag as char);
            out.push_str(&l.text);
            out.push('\n');
            if !l.had_newline {
                out.push_str("\\ No newline at end of file\n");
            }
        }
    }
    out
}

// silence dead_code lint when ToString isn't otherwise referenced
#[allow(dead_code)]
fn _force_use_to_string(_s: &str) -> String {
    _s.to_string()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- helpers ----------------------------------------------------------

    fn lines(hunks: &[DiffHunk]) -> Vec<(&'static str, &[String])> {
        // (kind, lines reference) for quick assertions
        hunks
            .iter()
            .map(|h| match h {
                DiffHunk::Equal { lines, .. } => ("eq", lines.as_slice()),
                DiffHunk::Insert { lines, .. } => ("ins", lines.as_slice()),
                DiffHunk::Delete { lines, .. } => ("del", lines.as_slice()),
                DiffHunk::Replace { new_lines, .. } => ("rep", new_lines.as_slice()),
            })
            .collect()
    }

    // ---- split_lines / is_binary -----------------------------------------

    #[test]
    fn split_lines_keeps_newlines() {
        let r = split_lines(b"a\nb\nc");
        assert_eq!(r, vec![&b"a\n"[..], &b"b\n"[..], &b"c"[..]]);
    }

    #[test]
    fn split_lines_empty_input() {
        let r: Vec<&[u8]> = split_lines(b"");
        assert!(r.is_empty());
    }

    #[test]
    fn binary_detection() {
        assert!(!is_binary(b"plain text"));
        assert!(is_binary(b"has\0null"));
    }

    // ---- Myers diff -------------------------------------------------------

    #[test]
    fn myers_identical() {
        let a = vec!["a", "b", "c"];
        let ops = myers_diff(&a, &a);
        // All Equal, no Insert/Delete.
        assert!(ops.iter().all(|op| matches!(op, EditOp::Equal { .. })));
        assert_eq!(ops.len(), 3);
    }

    #[test]
    fn myers_pure_insert() {
        let a: Vec<&str> = vec![];
        let b = vec!["x", "y"];
        let ops = myers_diff(&a, &b);
        assert_eq!(
            ops,
            vec![EditOp::Insert { new_idx: 0 }, EditOp::Insert { new_idx: 1 },]
        );
    }

    #[test]
    fn myers_pure_delete() {
        let a = vec!["x", "y"];
        let b: Vec<&str> = vec![];
        let ops = myers_diff(&a, &b);
        assert_eq!(
            ops,
            vec![EditOp::Delete { old_idx: 0 }, EditOp::Delete { old_idx: 1 },]
        );
    }

    #[test]
    fn myers_replace_middle() {
        let a = vec!["a", "b", "c"];
        let b = vec!["a", "X", "c"];
        let ops = myers_diff(&a, &b);
        // Order in the script: Equal(a), Delete(b), Insert(X), Equal(c)
        // (or Equal(a), Insert(X), Delete(b), Equal(c) — both valid)
        // Verify reconstruction rather than exact ordering.
        let mut out: Vec<&&str> = Vec::new();
        for op in &ops {
            match op {
                EditOp::Equal { old_idx, .. } => out.push(&a[*old_idx]),
                EditOp::Insert { new_idx } => out.push(&b[*new_idx]),
                EditOp::Delete { .. } => {}
            }
        }
        assert_eq!(out, vec![&"a", &"X", &"c"]);
    }

    // ---- diff_blobs -------------------------------------------------------

    #[test]
    fn diff_blobs_identical() {
        let h = diff_blobs(b"a\nb\n", b"a\nb\n");
        assert_eq!(h.len(), 1);
        assert!(matches!(h[0], DiffHunk::Equal { .. }));
    }

    #[test]
    fn diff_blobs_insert_at_end() {
        let h = diff_blobs(b"a\nb\n", b"a\nb\nc\n");
        let kinds: Vec<&str> = lines(&h).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, vec!["eq", "ins"]);
        if let DiffHunk::Insert { lines, new_start } = &h[1] {
            assert_eq!(new_start, &2);
            assert_eq!(lines, &vec!["c\n".to_string()]);
        } else {
            panic!("expected Insert");
        }
    }

    #[test]
    fn diff_blobs_delete_at_start() {
        let h = diff_blobs(b"a\nb\nc\n", b"b\nc\n");
        let kinds: Vec<&str> = lines(&h).into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, vec!["del", "eq"]);
    }

    #[test]
    fn diff_blobs_replace_coalesced() {
        let h = diff_blobs(b"a\nb\nc\n", b"a\nX\nc\n");
        let kinds: Vec<&str> = lines(&h).into_iter().map(|(k, _)| k).collect();
        // Expect Equal, Replace, Equal (Delete+Insert coalesced).
        assert_eq!(kinds, vec!["eq", "rep", "eq"]);
    }

    #[test]
    fn diff_blobs_binary() {
        let h = diff_blobs(b"text\0bin", b"different\0bin");
        assert_eq!(h.len(), 1);
        assert!(matches!(h[0], DiffHunk::Replace { .. }));
    }

    // ---- merge_blobs ------------------------------------------------------

    #[test]
    fn merge_blobs_both_unchanged() {
        let r = merge_blobs(b"a\n", b"a\n", b"a\n");
        assert!(!r.has_conflicts);
        assert_eq!(r.content, b"a\n");
    }

    #[test]
    fn merge_blobs_only_ours_changed() {
        let r = merge_blobs(b"a\n", b"A\n", b"a\n");
        assert!(!r.has_conflicts);
        assert_eq!(r.content, b"A\n");
    }

    #[test]
    fn merge_blobs_only_theirs_changed() {
        let r = merge_blobs(b"a\n", b"a\n", b"A\n");
        assert!(!r.has_conflicts);
        assert_eq!(r.content, b"A\n");
    }

    #[test]
    fn merge_blobs_convergent_edit() {
        let r = merge_blobs(b"a\n", b"X\n", b"X\n");
        assert!(!r.has_conflicts);
        assert_eq!(r.content, b"X\n");
    }

    #[test]
    fn merge_blobs_independent_edits_different_regions() {
        // base:  line1 line2 line3
        // ours:  L1    line2 line3      (changed first line)
        // their: line1 line2 L3         (changed last line)
        // expected: L1 line2 L3
        let base = b"line1\nline2\nline3\n";
        let ours = b"L1\nline2\nline3\n";
        let theirs = b"line1\nline2\nL3\n";
        let r = merge_blobs(base, ours, theirs);
        assert!(
            !r.has_conflicts,
            "got: {:?}",
            String::from_utf8_lossy(&r.content)
        );
        assert_eq!(r.content, b"L1\nline2\nL3\n");
    }

    #[test]
    fn merge_blobs_conflict_same_line_different_change() {
        let base = b"a\nb\nc\n";
        let ours = b"a\nOUR\nc\n";
        let theirs = b"a\nTHEIR\nc\n";
        let r = merge_blobs(base, ours, theirs);
        assert!(r.has_conflicts);
        let s = String::from_utf8(r.content).unwrap();
        assert!(s.contains("<<<<<<< ours"));
        assert!(s.contains("OUR"));
        assert!(s.contains("======="));
        assert!(s.contains("THEIR"));
        assert!(s.contains(">>>>>>> theirs"));
        // Surrounding context preserved
        assert!(s.starts_with("a\n"));
        assert!(s.ends_with("c\n"));
    }

    #[test]
    fn merge_blobs_insertion_in_one_side_only() {
        let base = b"a\nb\n";
        let ours = b"a\nINSERTED\nb\n";
        let theirs = b"a\nb\n";
        let r = merge_blobs(base, ours, theirs);
        assert!(!r.has_conflicts);
        assert_eq!(r.content, b"a\nINSERTED\nb\n");
    }

    #[test]
    fn merge_blobs_no_trailing_newline_preserved_when_clean() {
        let base = b"a";
        let ours = b"a";
        let theirs = b"a";
        let r = merge_blobs(base, ours, theirs);
        assert!(!r.has_conflicts);
        assert_eq!(r.content, b"a");
    }

    // ---- unified_diff -----------------------------------------------------

    #[test]
    fn unified_diff_identical_is_empty() {
        assert_eq!(unified_diff(b"a\nb\n", b"a\nb\n", "a", "b", 3), "");
    }

    #[test]
    fn unified_diff_single_change_has_header_and_markers() {
        let d = unified_diff(b"a\nb\nc\n", b"a\nX\nc\n", "old.txt", "new.txt", 3);
        assert!(d.starts_with("--- old.txt\n+++ new.txt\n"), "got: {d}");
        assert!(d.contains("@@ -1,3 +1,3 @@\n"), "got: {d}");
        assert!(d.contains("\n-b\n"), "got: {d}");
        assert!(d.contains("\n+X\n"), "got: {d}");
        assert!(d.contains(" a\n"));
        assert!(d.contains(" c\n"));
    }

    #[test]
    fn unified_diff_context_limits_emitted_lines() {
        // 10 unchanged lines, change line 6; with context=1 only lines 5..7 show.
        let mut old = String::new();
        for i in 1..=10 {
            old.push_str(&format!("L{i}\n"));
        }
        let new = old.replace("L6\n", "X6\n");
        let d = unified_diff(old.as_bytes(), new.as_bytes(), "a", "b", 1);
        assert!(d.contains(" L5\n"));
        assert!(d.contains("-L6\n"));
        assert!(d.contains("+X6\n"));
        assert!(d.contains(" L7\n"));
        assert!(!d.contains("L1\n"), "context=1 should not include L1: {d}");
        assert!(
            !d.contains("L10\n"),
            "context=1 should not include L10: {d}"
        );
    }

    #[test]
    fn unified_diff_marks_missing_trailing_newline() {
        let d = unified_diff(b"a\n", b"a", "a", "b", 3);
        assert!(d.contains("\\ No newline at end of file\n"), "got: {d}");
    }

    #[test]
    fn unified_diff_binary_summary() {
        let d = unified_diff(b"a\0b", b"a\0c", "a", "b", 3);
        assert!(d.contains("Binary files differ"), "got: {d}");
    }
}
