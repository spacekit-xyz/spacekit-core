# Multi-Model Transactions (Phase 0)

The storage node's [`TransactionManager`](../../src/transaction.rs) coordinates ACID writes across **four** subsystems:

- **Relational** — `Database` rows (users, files, facts, contact messages, encrypted users, file-access grants).
- **Document store** — DID-scoped `DocumentRecord`s (`/api/documents/{collection}/{id}`).
- **Vector index** — `VectorIndex` embeddings.
- **Full-text index** — `FullTextIndex` documents.

A single `BEGIN` opens one consistency boundary across all four. The
[`Facade`](../../src/storage_facade.rs) seam threads each subsystem's
`apply` / `revert` callback through the manager so the transaction module
itself has no compile-time dependency on the typed subsystem APIs.

## Isolation Contract

Phase 0 commits to one isolation guarantee, written down so future relaxations
are explicit:

> **Serializable on the write path** via a global commit `Mutex` in
> `TransactionManager`. While one transaction is in `apply_modifications`,
> every other commit queues. Reads run lock-free against the most recently
> committed snapshot.

Trade-offs:

- Pro: simplest correct semantics; no per-row locking; `BEGIN` -> `COMMIT`
  windows can interleave freely until they hit the global commit lock.
- Pro: deterministic test surface — Phase 2's `tests/cross_module_tx.rs`
  asserts Serializable directly.
- Con: a long-running commit blocks every other commit. Phase 4's change
  feed publishes `tx.committed` so observability dashboards can graph the
  global commit lock duration.

Future work (out of scope this milestone): per-table optimistic concurrency,
MVCC reads, and a "Read Committed" mode for analytic batches.

## Modification Log

Every write inside a transaction records a `TransactionModification` enum
variant. The variants below are the entire enrolment surface; if you add a
new write path, you must extend this enum and wire `apply_one` /
`revert_one`:

| Variant                    | Subsystem | Apply                                       | Revert                                         |
|----------------------------|-----------|---------------------------------------------|------------------------------------------------|
| `InsertUser`               | DB        | `Database::insert_user`                     | (no `delete_user`; documented as advisory)     |
| `UpdateUser`               | DB        | `Database::update_user`                     | re-`update_user` with old value                |
| `InsertFile` / `UpdateFile`| DB        | `Database::insert_file_metadata`            | re-insert old or `delete_file_metadata`        |
| `DeleteFile`               | DB        | `Database::delete_file_metadata`            | `insert_file_metadata` with old value          |
| `InsertFact` / `DeleteFact`| DB        | `Database::insert_fact_metadata` / `remove` | inverse                                        |
| `InsertEncUser`            | DB        | `Database::insert_enc_user`                 | advisory                                       |
| `InsertMessage`            | DB        | `Database::insert_message`                  | advisory                                       |
| `InsertEncMessage`         | DB        | `Database::insert_enc_message`              | advisory                                       |
| `UpsertFileAccessGrant`    | DB        | `Database::upsert_file_access_grant`        | restore old grant or remove                    |
| `RemoveFileAccessGrant`    | DB        | `Database::remove_file_access_grant`        | re-upsert old grant                            |
| `PutDocument`              | docs      | `Database::upsert_document`                 | restore old or `delete_document`               |
| `DeleteDocument`           | docs      | `Database::delete_document`                 | restore old via `upsert_document`              |
| `UpsertEmbedding`          | vector    | `VectorIndex::add_embedding` callback        | re-add prior embedding or remove               |
| `RemoveEmbedding`          | vector    | `VectorIndex::remove_embedding` callback     | re-add old embedding                           |
| `IndexDoc`                 | fts       | `FullTextIndex::index_document` callback     | `FullTextIndex::remove_document` callback      |
| `UnindexDoc`               | fts       | `FullTextIndex::remove_document` callback    | re-index from old payload                      |

The trace endpoint `GET /api/transactions/{id}/trace` returns per-step
`TraceEntry` rows so an agent debugging a failed commit can see the
subsystem, action, key, elapsed_micros, and any error message.

## Recording modifications over HTTP

After `POST /api/transactions` returns a `transaction_id`, agents append steps
with:

`POST /api/transactions/{transaction_id}/modifications`

Use `Authorization: DID <did>`, `Idempotency-Key`, and a JSON body
`{ "modification": …, "conflict_policy": …, "bytes_written": … }` (see
[`agentic_routes.rs`](../../src/api/agentic_routes.rs) `RecordTxModificationRequest`).
Optional header **`X-Sandbox-Id`** mirrors each append into that sandbox’s
journal for replay at commit time; ACL matches the sandbox guide (owner +
`collaborator_dids`). If mirroring fails after the transaction row was updated,
roll back the transaction to avoid divergence. See
[`documentation/guides/sandboxes.md`](sandboxes.md).

## Append-Only Blob Caveat

Repository-hosted blobs (`/blobs/{hash}`) are **content-addressed** and
**immutable** — there is no "delete the blob" operation. A transaction that
performs a `PUT /blobs` and then rolls back leaves the blob on disk; the GC
sweeper reclaims unreferenced blobs at a later time. Rolling back a commit
that *referenced* a blob does not re-introduce a reference; the blob simply
becomes unreferenced.

## Runtime Rollout Flag

The new transactional write path ships behind a runtime flag, not a Cargo
`cfg`:

```toml
# StorageNodeConfig
enable_real_transactions = true    # Phase 1+ default; set false to keep stub finalize.
# Or: SPACEKIT_ENABLE_REAL_TRANSACTIONS=false
```

or via env:

```bash
SPACEKIT_ENABLE_REAL_TRANSACTIONS=true ./spacekit-storage-node start ...
```

When the flag is `false`:

- `BEGIN` / `COMMIT` / `ROLLBACK` succeed and the modification log is
  recorded for observability.
- `apply_modifications` skips the actual subsystem mutations.
- Existing legacy direct-write paths in `src/api/mod.rs` continue to mutate
  the database as before (the [`Facade`](../../src/storage_facade.rs) is a
  pass-through in this mode).

This delivers the same safety net as a Cargo feature flag without a `cfg`
matrix or two divergent write paths in `src/api/mod.rs`.

## Future Relaxation Path

When operators report contention on the global commit lock:

1. Per-row optimistic version checks for `Database::update_user` and
   relational rows. The `TransactionModification::Update*` variants already
   carry `old_value`, so the apply step can compare against the live row
   before writing.
2. MVCC snapshot reads keyed off a monotonic commit sequence number
   (Phase 4's `ChangeFeed::current_seq` already provides one).
3. Per-modification-type concurrency (e.g. vector upserts can run under
   "last writer wins" since `(index_id, document_id)` is the key).

Until those land, treat the global commit lock as the consistency invariant.
