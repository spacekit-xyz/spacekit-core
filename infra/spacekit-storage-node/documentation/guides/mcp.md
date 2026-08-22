# MCP Server (Phase 5)

The storage node ships an in-process **Model Context Protocol** server that
wraps the [`Facade`](../../src/storage_facade.rs) as a catalogue of
agent-callable tools. Agents speak JSON-RPC 2.0 over stdio; future
transports include SSE.

## Running

```bash
cargo run --bin spacekit-storage-node --features standalone -- mcp \
    --data-dir /var/lib/spacekit \
    --did did:spacekit:mcp:operator \
    --enable-real-transactions
```

The binary opens stdin/stdout, reads one JSON-RPC request per line, writes
one response per line, and continues until EOF.

## Tool Catalog

Tools are versioned: every name ends in `.v1`. When a v2 ships, the v1
descriptor remains in the catalogue for one deprecation cycle so production
agents that hard-code signatures don't break on upgrade.

| Tool                  | Purpose                                                         |
|-----------------------|-----------------------------------------------------------------|
| `tx_begin.v1`         | Open a new ACID transaction.                                    |
| `tx_commit.v1`        | Commit the transaction (Serializable on the write path).        |
| `tx_rollback.v1`      | Discard the transaction's modification log.                     |
| `tx_trace.v1`         | Per-step modification log + timing + subsystem.                 |
| `sandbox_create.v1`   | Open an ephemeral sandbox; optional `workspace_id` + quotas.   |
| `sandbox_commit.v1`   | Commit a sandbox; `dry_run=true` previews conflicts.            |
| `sandbox_discard.v1`  | Discard a sandbox and its journal.                              |
| `sandbox_journal.v1`  | Inspect the sandbox's journal entries + quotas.                 |
| `workspace_create.v1` | Create `spacekit:workspace:v1` (owner, collaborators, quotas).  |
| `workspace_get.v1`    | Load one workspace for an owner DID.                              |
| `workspace_list.v1`   | List workspace index rows for an owner.                         |
| `workspace_export.v1` | Export federation handoff bundle.                               |
| `workspace_import.v1` | Import bundle on destination node.                              |
| `blobs_replicate.v1`  | Pull CAS blobs from a remote node by hash.                      |
| `upload_token_mint.v1`| Mint `Authorization: UploadToken` for blob/fact uploads.          |
| `graph_traverse.v1`   | BFS over the `FactPackage.dependencies` DAG.                    |

`tools/list` returns the full catalog with input schemas. Production agents
should pin to the version they tested against.

## Idempotency Keys

The MCP layer derives a **deterministic** idempotency key from the
canonical JSON of `(tool_name, arguments)`:

```text
BLAKE3("mcp:" || tool_name || ":" || canonical_json(args))[..16]
```

This matters because UUIDv4 auto-generation per call would defeat the
storage node's idempotency cache: a retry of the same logical operation
produces a *new* key, misses the cache, and re-runs the operation. With
deterministic derivation, retries naturally hit cache.

Agents that want to disambiguate calls intentionally — e.g. two distinct
"insert this row" operations — can pass an explicit `idempotency_key`
argument. UUIDv4 fallback is **never** used.

`canonical_json` sorts object keys, escapes string content, and emits no
insignificant whitespace, so re-ordering arguments produces the same key.

## Observability Tools

Agents debug their own failures. Two tools surface internal state:

- `tx_trace.v1` — returns the modification log, per-step timing, and
  subsystem the apply hit. Read this when a commit fails.
- `sandbox_journal.v1` — returns the sandbox's journal entries and quota
  counters. Read this when a sandbox commit returns 409, or to budget
  remaining writes.

Both are read-only; they don't count against the per-DID rate limiter.

## Graph Traversal

`graph_traverse.v1` walks the `FactPackage.dependencies` DAG. This is the
minimal "graph query" the assessment flagged as missing. ~200 lines covers
~80% of relationship reasoning agents need without writing recursive
subqueries.

The server enforces **`max_depth` ≤ 50**; larger values return an MCP
`invalid_params` error so agents cannot OOM the node with an unbounded frontier.

```json
{
    "name": "graph_traverse.v1",
    "arguments": {
        "start_fact_id": "fact:123",
        "max_depth": 6,
        "direction": "forward"
    }
}
```

`direction: "reverse"` walks the inverse edges (which fact packages depend
on `start_fact_id`). The forward path is O(visited * fan-out); the reverse
path is O(N) since the storage node doesn't maintain a reverse-dependency
index. A future tool wraps this with a typed-edge filter once
`FactPackage` grows typed edges.

## Wire Example

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' \
    | spacekit-storage-node mcp --data-dir /tmp/sk
```

Returns:

```json
{
    "jsonrpc": "2.0",
    "id": 1,
    "result": {
        "tools": [
            {"name": "tx_begin.v1", "description": "...", "input_schema": {...}, "version": 1},
            ...
        ]
    }
}
```
