# spacekit-repo

Rust types and helpers for **SpaceKit repository commits** embedded in `FactPackage` JSON (`spacekit:repo:commit:v1`), plus re-exports of [`spacekit-diff`](../spacekit-diff/) tree **and blob** operations.

Commit content carries the `tree` (path → BLAKE3 hex), POSIX file `modes` (executable bit, default `0o644` omitted), `message`, and author/committer identity + timestamps. Canonical JSON over all of these yields the deterministic `FactID`, which is what a commit signature covers.

Key helpers:
- `build_commit_fact_package` / `parse_commit_from_fact_package` — construct/parse commits.
- `commit_fact_id` / `recompute_commit_fact_id` / `verify_commit_fact_id` — deterministic id + integrity check (re-hash a fetched commit and compare).
- `commit_signing_message` — the exact bytes (the fact-id) a signer should sign.
- Re-exported diff/merge: `diff_trees`, `merge_trees`, `diff_blobs`, `merge_blobs`, `unified_diff` (git-style patch rendering), `detect_exact_renames` (content-identical rename pairing).

**User-facing documentation** for hosting repos on the storage node (HTTP APIs, `spacekit repo` CLI, security notes):  
**[../spacekit-storage-node/documentation/guides/spacekit-repository-hosting.md](../spacekit-storage-node/documentation/guides/spacekit-repository-hosting.md)**

Build (crate is excluded from the repo root workspace; use manifest path):

```bash
cargo check --manifest-path spacekit-repo/Cargo.toml
```
