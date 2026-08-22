# spacekit-diff

A standalone Rust library for diffing and three-way merging **tree snapshots** (path → content-hash manifests) and **blob content** (line-level). Pure computation — no I/O, no crypto, no network. Designed for SpaceKit's CLI, browser (via WASM), and contract use cases.

## What it does

Four operations, all deterministic and side-effect-free:

1. **`diff_trees(base, head)`** — compare two manifests, emit `Added` / `Removed` / `Modified` changes.
2. **`apply_tree_diff(base, changes)`** — apply a changeset to a manifest, producing the head (validates each change against current state).
3. **`diff_blobs(old, new)`** — Myers line-level diff producing `Equal` / `Insert` / `Delete` / `Replace` hunks.
4. **`merge_trees(base, ours, theirs)`** — three-way manifest merge with git-style rules; surfaces `Content`, `ModifyDelete`, `AddAdd` conflicts.
5. **`merge_blobs(base, ours, theirs)`** — diff3 line-level merge; emits `<<<<<<<` / `=======` / `>>>>>>>` markers on overlap.

## Module layout

```
spacekit-diff/
├── Cargo.toml          # zero deps, no_std + alloc, optional `std` feature
├── src/
│   ├── lib.rs          # crate root, public re-exports
│   ├── types.rs        # TreeSnapshot, TreeChange, DiffHunk, MergeConflict, ...
│   ├── tree.rs         # diff_trees, apply_tree_diff, merge_trees
│   └── blob.rs         # Myers diff_blobs, diff3 merge_blobs, line splitting
└── tests/
    └── integration.rs  # end-to-end scenarios (CLI workflow simulation)
```

The split is intentional: `types.rs` is the data model only, `tree.rs` and `blob.rs` are independent and can be reasoned about separately.

## `no_std` story

The crate is `#![cfg_attr(not(test), no_std)]` and pulls in `alloc` for `Vec`, `String`, and `BTreeMap`. Library code uses only `core::` and `alloc::` paths; `std` shows up only in unit tests (via `assert_eq!` and friends, which work fine because Cargo lets the test harness link std even when the lib doesn't).

There are **zero runtime dependencies**. `Cargo.toml` defines an optional `std` feature for callers that want to opt back in (e.g. for `std::error::Error` impls on `ApplyError`, which can be added later without breaking the API).

The release profile sets `lto = true` and `opt-level = "z"` to keep WASM bundle size small.

### WASM build

The crate is structured for `wasm32-unknown-unknown` but **WASM build verification was not performed in this environment** because the available toolchain (`apt install rustc cargo` → 1.75.0) doesn't have rustup, so additional targets can't be installed. To verify locally:

```bash
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
```

The crate should build cleanly because (a) no dependencies, (b) no `std` references, (c) only `alloc` collections, (d) no platform-specific syscalls or FFI.

## How callers integrate it

### CLI (`spacekit repo diff`, `spacekit repo merge`)

```rust
use spacekit_diff::{diff_trees, merge_trees, diff_blobs, merge_blobs, TreeChange};

// Diff
let changes = diff_trees(&base_tree, &head_tree);
for c in &changes {
    match c {
        TreeChange::Modified { path, old_hash, new_hash } => {
            let old_blob = storage.fetch(old_hash)?;
            let new_blob = storage.fetch(new_hash)?;
            let hunks = diff_blobs(&old_blob, &new_blob);
            display_hunks(path, &hunks);
        }
        TreeChange::Added   { path, .. } => println!("A  {}", path),
        TreeChange::Removed { path, .. } => println!("D  {}", path),
    }
}

// Merge
let result = merge_trees(&base, &ours, &theirs);
for conflict in &result.conflicts {
    if let MergeConflict::Content { path, base_hash, our_hash, their_hash } = conflict {
        let merged = merge_blobs(
            &storage.fetch(base_hash)?,
            &storage.fetch(our_hash)?,
            &storage.fetch(their_hash)?,
        );
        write_working_copy(path, &merged.content)?;
        // user resolves, then caller inserts the resolved hash into result.tree
    }
}
```

### Browser (spacekit-js via WASM)

Same flow, but blobs come from envelope-decrypted downloads and the WASM module is invoked from JS. Because the API is just `&[u8]` and owned types, `wasm-bindgen` wrappers are straightforward — wrap each top-level function and JSON-serialize the result types.

### Contracts

A future verification contract can call `merge_trees` to check that a proposed merge commit is consistent with its three parents. Because the crate is `no_std` + `alloc` with no allocator-sensitive code paths beyond standard collections, it should drop into a contract runtime that provides `alloc`.

## Design choices worth knowing

**Conflicted paths are excluded from `MergeResult.tree`.** When `merge_trees` reports a conflict, the path is emitted in `conflicts` and *not* placed in `tree`. The caller resolves the conflict (typically by running `merge_blobs` and getting user input) and inserts the resolved hash. This is cleaner than a "default to ours" policy and matches how a CLI would actually drive a merge.

**Edit-script coalescing.** `diff_blobs` runs Myers to get a flat edit op stream, then groups consecutive `Delete`+`Insert` pairs into a single `Replace` hunk. This makes diff display and patch application simpler.

**Binary heuristic.** A NUL byte in the first 8 KiB marks a blob as binary, matching git's behavior. Binary diffs become a single `Replace` summary; binary three-way merges become a single whole-file conflict block.

**Line splitting preserves trailing `\n`.** Each line in the `Vec<&[u8]>` returned by `split_lines` includes its terminating newline (if present), so concatenation is lossless. The last line is included even without a trailing newline. This matters for merge output to round-trip correctly.

**Diff3 sync points.** `merge_blobs` finds shared base-line indices that appear in both the base→ours and base→theirs Myers traces, walks chunks between sync points, and classifies them: convergent (both sides identical), one-sided (other side equals base), or conflict.

## Build and test

```bash
cargo build                 # compiles cleanly
cargo test                  # 39 unit + 8 integration tests, all passing
cargo build --release       # exercises lto + opt-level="z" for WASM size
```

`cargo clippy` is recommended but was not run in the build environment used for this implementation (the bundled `cargo` lacked the clippy subcommand). On a normal rustup install: `cargo clippy --all-targets -- -D warnings`.

## Test coverage at a glance

- **`diff_trees`**: empty / add / remove / modify / mixed / sort-by-path
- **`apply_tree_diff`**: round-trip with `diff_trees`, plus three error paths (path-not-found, hash-mismatch, path-already-exists)
- **`merge_trees`**: every cell of the 3-way truth table — both-unchanged, one-side-modify, convergent edits, content conflict, modify-vs-delete (both directions), clean delete (when other side matches base), add-add convergent, add-add conflict, independent-changes combination
- **Myers**: identical / pure-insert / pure-delete / replace-middle
- **`diff_blobs`**: identical fast path, insert at end, delete at start, replace coalescing, binary
- **`merge_blobs`**: both unchanged, only-ours, only-theirs, convergent, independent regions, overlapping conflict with markers, insert on one side, no-trailing-newline preservation
- **Integration**: full CLI workflow (merge surfaces conflict → caller resolves → diff/apply round-trips), diff3 stress cases (independent inserts, adjacent inserts, modify+delete in separate regions, overlapping edits with context, delete inside modification region, convergent insertion), completely different files

## License

(set per project policy)
