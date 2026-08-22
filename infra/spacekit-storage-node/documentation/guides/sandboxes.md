# Sandboxes (Phase 1)

Sandboxes are the agent-facing primitive for **ephemeral, isolated
workspaces**. An agent calls `POST /api/sandboxes`, receives a sandbox id,
and either:

- **Sandbox-native writes** — routes that already accept `X-Sandbox-Id` record
  into the sandbox journal directly, or
- **Transaction log + mirror** — `POST /api/transactions/{id}/modifications`
  with optional `X-Sandbox-Id` appends the same modification to the active
  `TransactionManager` log **and** mirrors a copy into the sandbox journal
  (same ACL as extend; see below).

The agent then commits or discards the sandbox cleanly.

## Lifecycle

```
                   ┌──────────────────────┐
                   │  POST /api/sandboxes │
                   └──────────┬───────────┘
                              │ 201 Created   { id, expires_at, quotas, journal: [] }
                              ▼
                       ┌────────────┐
                       │   Active   │  ◀── X-Sandbox-Id: <id>     records into journal
                       └─────┬──────┘
                             │
       ┌─────────────────────┼─────────────────────┐
       ▼                     ▼                     ▼
  POST /commit         POST /commit?dry_run=true  POST /discard
  (or TTL elapses)
       │                     │                     │
       ▼                     ▼                     ▼
   Committed             Active              Discarded / Expired
```

Agents may also `POST /api/sandboxes/{id}/extend` to push out the TTL while
they're still working.

Create body may include **`collaborator_dids`**: an array of extra DIDs that may
call `GET` (sandbox + journal), `POST .../extend`, and
`POST /api/transactions/{tx_id}/modifications` when **`X-Sandbox-Id`** matches
this sandbox (journal mirror path). Only the **owner** may commit or discard.

Optional **`workspace_id`** ties the sandbox to a `spacekit:workspace:v1` fact.
The caller (`Authorization: DID`) must be the workspace owner or a collaborator;
`max_bytes_written` is capped to the workspace `max_sandbox_bytes` quota, and
create fails when active sandboxes for that workspace already sum to
`max_storage_bytes`. See [workspaces.md](./workspaces.md).

### Transaction modifications and `X-Sandbox-Id`

`POST /api/transactions/{transaction_id}/modifications` requires
`Authorization: DID <did>`, `Idempotency-Key`, and a JSON body:

```json
{
  "modification": { "...": "TransactionModification variant" },
  "conflict_policy": "Reject",
  "bytes_written": 0
}
```

`conflict_policy` and `bytes_written` are optional (`bytes_written` defaults to
0). Idempotency is keyed under the logical route
**`POST /api/transactions/modifications`** (same fingerprint rules as other
agentic writes).

If **`X-Sandbox-Id`** is present and non-empty, the façade appends to the
transaction first, then checks sandbox ACL (**owner + collaborators**) and
appends the same entry to that sandbox’s journal. If the sandbox step fails
after the transaction append succeeds (for example `403` / storage error), the
HTTP client should **`POST /api/transactions/{id}/rollback`** (or otherwise
undo the open transaction) so the live transaction log does not diverge from the
sandbox journal — the server does not auto-compensate.

## `Committing` and restarts

`POST .../commit` moves the sandbox to **`committing`** while the
`TransactionManager` runs; the TTL reaper skips that state so a commit cannot
be torn down mid-flight.

### On-disk snapshots (when the API server has `data_dir`)

If the node is started with a configured **`data_dir`** (the usual API server
path in `StorageNode`), the facade persists sandboxes under:

| Path | Role |
|------|------|
| `<data_dir>/sandboxes/boot_epoch.txt` | Monotonic counter bumped once per process start |
| `<data_dir>/sandboxes/state/<sandbox-id>.json` | Atomic JSON snapshot of the full [`Sandbox`](../../src/sandbox.rs) (metadata + journal) |

While committing, the snapshot records **`commit_started_boot_epoch`** equal to
the current epoch. After a crash, the next boot has a higher epoch; the facade
runs a **reconciliation pass** that finds rows still in `committing` with an
older epoch, replays the journal through `TransactionManager::commit`, and on
failure transitions the sandbox to **`failed`** (see `failure_reason` in the
snapshot and `sandboxes_failed` on `GET /api/agentic/health`). Operators may
`POST .../discard` a failed sandbox to clear it.

`POST /api/sandboxes` remains the only create path; everything else requires
**`Authorization: DID <did>`** (or `X-DID` in tests): **read** (`GET` sandbox,
`GET` journal) and **extend** are allowed for the **owner** and
**`collaborator_dids`** from the create body (including the tx-modification
mirror path above); **commit** and **discard** are **owner-only** (anonymous
owner keeps the legacy open policy for local dev).

Without `data_dir` / a non-API facade, sandboxes stay **in-memory only** as before.

## Conflict Policy

Sandboxes commit by replaying the journal through the
[`TransactionManager`](../../src/transaction.rs). Different write types use
different conflict policies; **the default is "reject on conflict, return
409 with structured diff"** so agents have to consciously decide silent
merges.

| Mod kind                     | Default policy           | Justification                                           |
|------------------------------|--------------------------|---------------------------------------------------------|
| Repo tree (`RepoTree` mod)   | `ThreeWayMerge`          | `TransactionModification::RepoTree` commits via `spacekit:repo:commit:v1` + ref update (requires `cas_data_dir` on the facade). |
| Relational rows              | `Reject` (optimistic)    | No meaningful three-way merge for arbitrary row schemas; agents must opt in. |
| Vector embedding upserts     | `LastWriterWins` (opt-in)| `(index_id, document_id)` is a unique key; later upserts strictly replace. |
| FTS doc index                | `LastWriterWins` (opt-in)| Same as vector — last index wins. |
| Document store               | `OptimisticIfMatch`      | Agents pass `If-Match: <etag>` for concurrent-safe updates. |

The policy is recorded per journal entry. A future operator-policy file
will allow per-DID overrides; for now the defaults are baked in.

## Dry-Run Commit

`POST /api/sandboxes/{id}/commit?dry_run=true` runs the replay through the
transaction manager, reports any conflicts, and immediately rolls back. No
state changes. This is the primitive agents need for **speculative
reasoning**: "I think this commit will work; check, then commit." Cheap to
add since the transaction infrastructure already supports rollback.

The dry-run path is observable via `tx_trace.v1` — the trace shows the
replay attempt with `revert:*` entries documenting that nothing was
applied.

## Quota Accounting

Each sandbox tracks three counters:

- `bytes_written` — sum of all `bytes_written` recorded by the writer.
- `vector_ops` — count of `UpsertEmbedding` / `RemoveEmbedding`.
- `fact_puts` — count of `InsertFact`.

`SandboxConfig` provides hard caps (`max_bytes_written`, `max_vector_ops`,
`max_fact_puts`). When a counter exceeds its cap the sandbox does **not**
auto-fail — it logs a warning and the next `GET /api/sandboxes/{id}` shows
the elevated counters. Agents that want hard enforcement should poll the
GET endpoint or check the response from each write.

This separation lets aggressive exploration phases continue past the cap
while signalling the operator policy; auto-fail would cause unrecoverable
agent loops.

## CAS-Backed Journal (Future)

The plan calls for sandbox journal entries to be stored as `FactPackage`s
in the CAS, with the sandbox ref pointing at the journal head. Until then,
when disk persistence is enabled the **entire journal is embedded** in each
`state/<id>.json` snapshot (simple and correct; large journals cost more I/O).

- The Phase 1 milestone still gates the **per-entry CAS chain** behind a
  `p99 < 50ms` benchmark for append-heavy workloads.
- A future discard becomes "delete the ref"; the CAS GC reaps unreferenced facts
  at a later sweep.

When the benchmark passes, journal tails can move to CAS without a wire-format
change to the HTTP API.

## SandboxReaper

A background task in the storage node lifecycle calls
`SandboxManager::reap()` every 60 seconds:

- Active sandboxes whose `expires_at` is in the past transition to
  `Expired` and have their journals cleared.
- When disk persistence is on, each reap updates the sandbox snapshot on disk.
- Committed / discarded sandboxes remain in memory (and on disk if configured) so
  `GET /api/sandboxes/{id}/journal` still works for post-mortem.
