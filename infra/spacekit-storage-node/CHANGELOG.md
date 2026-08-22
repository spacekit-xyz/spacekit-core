# SpaceKit Storage Node — Changelog

Historical release / phase log. **[README.md](../README.md)** summarizes **Phase 11
(shipped)**, **Phase 12 (in progress)**, **Phase 13 (planned)**; completed work for
historical milestones **Phase 1–11** stays here so the README stays shorter.

Conventions: phases are sequential; checked items shipped, unchecked items did
not (and were either dropped or rolled forward into a later phase). Wire-format
breaking changes call themselves out explicitly.

---

## Phase 11 — Agentic readiness (completed milestone)

Sandboxes · multi-model ACID transactions · MCP · idempotency · change feed.

- [x] **`storage_facade` seam** — every read/write goes through one module that
  owns the ACID transaction manager, sandbox manager, idempotency cache,
  per-DID rate limiter, and change feed (`src/storage_facade.rs`).
- [x] **Multi-model ACID transactions** spanning relational + document +
  vector + FTS (`/api/transactions/*`, Serializable via global commit lock).
  Real apply/revert defaults **on** (`enable_real_transactions=true`); opt out with
  config or `SPACEKIT_ENABLE_REAL_TRANSACTIONS=false`. Guide:
  [`documentation/guides/multi-model-transactions.md`](documentation/guides/multi-model-transactions.md).
- [x] **Ephemeral sandboxes** for agents (`/api/sandboxes/*`): TTL-bounded
  isolated workspaces with per-mod-type conflict policy (3-way merge for repo
  trees, optimistic-reject for relational rows, last-writer-wins for
  vector/FTS, `If-Match` for documents), `?dry_run=true` preview commits, and
  per-sandbox quota counters (`bytes_written`, `vector_ops`, `fact_puts`).
  Guide: [`documentation/guides/sandboxes.md`](documentation/guides/sandboxes.md).
- [x] **Sandbox disk + ACL** — with `data_dir`, snapshots under `sandboxes/`
  (`boot_epoch.txt`, `state/<id>.json`), startup reconciliation for stuck
  `committing`, `failed` state on replay errors, optional `collaborator_dids` on
  create, and `Authorization: DID` checks on read/extend vs owner-only
  commit/discard (`sandboxes_failed` on `GET /api/agentic/health`).
- [x] **Workspace sandbox quotas** — `workspace_id` on sandboxes; create path caps
  `max_bytes_written` and enforces workspace storage sum via `Facade::create_sandbox`.
- [x] **Agentic demo loop** — `agentic_client_demo` runs workspace → sandbox →
  `RepoTree` → commit; prints `GET /api/agentic/health` `enable_real_transactions`.
- [x] **Workspace CLI + guide** — `spacekit workspace create/show/list` →
  `/api/workspaces/*`; [`documentation/guides/workspaces.md`](documentation/guides/workspaces.md).
- [x] **Strict fact signatures** — `POST /facts` in `strict` mode requires non-empty
  SPHINCS+ signature verified via node `QuantumCrypto` (`access_policy::verify_fact_signature`).
- [x] **ENHANCEMENTS gaps A/B/C** — opt-in blob/fact DID auth (`access_policy`,
  `SPACEKIT_BLOB_FACT_AUTH`), `TransactionModification::RepoTree` + repo commit
  apply (`repo_commit`, `cas_data_dir`), `spacekit:workspace:v1` + `/api/workspaces/*`;
  tests in `tests/enhancements_gaps.rs`.
- [x] **Phase 2 Stream A (partial)** — upload tokens (`POST /api/upload-tokens`,
  `Authorization: UploadToken`), `RoleBased` registry + `Conditional` time windows
  in `access_policy`; guide [`documentation/guides/upload-tokens.md`](documentation/guides/upload-tokens.md).
- [x] **MCP workspace + upload tools** — `workspace_create/get/list.v1`,
  `upload_token_mint.v1`; `sandbox_create.v1` schema documents `workspace_id`.
- [x] **Operator metrics + federation handoff** — `GET /api/agentic/metrics` (Prometheus),
  `GET /api/workspaces/{id}/export`, MCP `workspace_export.v1`; network profile
  `[runtime] blob_fact_auth`; guide [`federation-workspace-handoff.md`](documentation/guides/federation-workspace-handoff.md).
- [x] **Workspace import** — `POST /api/workspaces/import`, MCP `workspace_import.v1`,
  `spacekit workspace export/import`; `reject` | `replace` conflict policy.
- [x] **Federation blob replication** — export bundles include `referenced_blob_hashes`;
  `POST /api/blobs/replicate`; import `replicate_blobs_from`; MCP `blobs_replicate.v1`;
  `tests/hybrid_auth.rs` for hybrid mode contract.
- [x] **Signed workspace handoff** — `handoff_signature` on export bundles (HMAC via
  `SPACEKIT_HANDOFF_SECRET` or `{data_dir}/.handoff_secret`); verify on import;
  `SPACEKIT_REQUIRE_HANDOFF_SIGNATURE`; health/metrics fields; `src/handoff.rs`.
- [x] **Hybrid auth soak** — `examples/hybrid_auth_soak.rs` live HTTP checklist;
  staging guide metrics table and `[runtime] blob_fact_auth` profile notes.
- [x] **Strict auth soak** — `examples/strict_auth_soak.rs` (blob GET auth, SPHINCS+ facts).
- [x] **Stream D/E docs** — `operator-abuse-policy.md`, `operator-discovery.md`,
  `federation-roadmap.md`, `federation-design.md`, `federation-testing.md`;
  `src/operator_manifest.rs` (`spacekit:operator:v1`).
- [x] **CLI operator publish** — `spacekit operator publish` / `operator fact-id`.
- [x] **`GET /api/operators/self`** — published manifest or runtime synthesis;
  `spacekit operator show`; `SPACEKIT_PUBLIC_HTTP_URL` from network supervisor.
- [x] **Phase 2 readiness** — [`phase-2-readiness.md`](documentation/guides/phase-2-readiness.md).
- [x] **DID-signed migration (phases 1–6)** — `src/migration.rs`: v2 manifests, SPHINCS+
  source/destination counter-sign, `spacekit:migration_record:v1` audit facts,
  export version negotiation via `SPACEKIT_MIGRATION_DEST_URL`, CLI
  `spacekit migration verify` / `sign` / `keygen`; `migration_auth_soak` example;
  workspace owner keys in `.migration_signer_keys/`; `SPACEKIT_MIGRATION_SCENARIO`;
  spec [`DID-MIGRATION.md`](DID-MIGRATION.md), guide
  [`did-signed-migration.md`](documentation/guides/did-signed-migration.md).
- [x] **Upload token secret fix** — persist env to `data_dir/.upload_token_secret`
  on node start; `upload_tokens_configured` on agentic health; hex secret normalization;
  `[runtime] upload_token_secret` in network profile; staging guide
  [`blob-fact-auth-staging.md`](documentation/guides/blob-fact-auth-staging.md).
- [x] **Tx modification → sandbox journal** — `POST /api/transactions/{id}/modifications`
  with optional `X-Sandbox-Id` appends to the transaction log then mirrors into
  the sandbox journal under the same ACL as extend (collaborators included);
  MCP `tx_record_modification.v1` with optional `sandbox_id` / `caller_did`;
  operators should roll back the transaction if the mirror step fails after the
  append (documented in sandbox + transaction guides).
- [x] **Idempotency keys** — `Idempotency-Key` header with BLAKE3 body
  fingerprint, block-and-return on in-flight (default 30s wait, `422` on
  fingerprint mismatch). TTL configurable per-route (24h default, 7d max).
- [x] **Per-DID token-bucket rate limiting** — `Authorization: DID <did>`
  alongside the legacy IP rate limiter; integrates with the optional
  `rate-limit-spacekit` feature for cluster coordination.
- [x] **Change feed** — `GET /api/changes` SSE with monotonic `seq`,
  `Last-Event-ID` resume, per-subscriber bounded queue with
  disconnect-on-overflow, and disk-backed JSONL log (`<data_dir>/change_log.jsonl`)
  for restart-safe resume. Guide:
  [`documentation/guides/change-feed.md`](documentation/guides/change-feed.md).
- [x] **In-process MCP server** — `spacekit-storage-node mcp` subcommand,
  JSON-RPC stdio, with versioned tool catalog (`tx_*.v1`, `sandbox_*.v1`,
  `graph_traverse.v1`, observability tools), deterministic idempotency-key
  derivation (`BLAKE3("mcp:" || tool_name || ":" || canonical_json(args))[..16]`),
  and a depth-capped BFS over the `FactPackage.dependencies` DAG. Guide:
  [`documentation/guides/mcp.md`](documentation/guides/mcp.md).
- [x] **Operator snapshot** — `GET /api/agentic/health` exposes
  `enable_real_transactions`, tx commit path totals (stub finalize vs real
  apply ok/err), idempotency cache hit rate, per-DID rate-limit rejections
  (total + last 60s), change-feed live subscribers + dropped count + current
  `seq`, sandbox counts by state + summed quotas. The
  `tx_commits_stub_finalize_total` field is a permanent regression signal —
  after real-apply is the default it should read ~0; a sustained non-zero
  value is a bug.
- [x] **Downstream client demo** at
  [`examples/agentic_client_demo.rs`](examples/agentic_client_demo.rs)
  showing the canonical request shape (`Authorization: DID` +
  `Idempotency-Key` + `X-Sandbox-Id`), plus
  `POST /api/transactions/{id}/modifications` mirroring into the sandbox journal,
  `GET .../journal`, rolling back the open transaction, and sandbox dry-run /
  commit.
- [x] **Lint contract** — new agentic Rust modules use `#![deny(clippy::all)]`
  so that surface stays lint-clean while legacy code remains at default warn
  level.

---

## Phase 10 — Cross-Service Surface (servers, groups, apps, global users)

- [x] **Global user registry + presence** (`/api/users/*`).
- [x] **Servers / members / invitations / topic subscriptions**
  (`/api/servers/*`).
- [x] **Groups / feeds / subscriptions** (`/api/groups/*`).
- [x] **App marketplace** backed by `app_storage.rs` (`/api/apps/*`).
- [x] **Cross-server P2P routing** (`server_routing.rs`,
  `server_message_routing.rs`).
- [x] **Optional distributed rate limiting** via `rate-limit-spacekit` feature.

---

## Phase 9 — Repository Hosting (Git-style, CAS-backed)

- [x] **Content-addressed blob store** at `/blobs/{hash}` (PUT/GET/HEAD +
  `POST /blobs/exists`).
- [x] **Commits as `FactPackage` JSON** with schema `spacekit:repo:commit:v1`
  via `/facts`, `/facts/{id}`, `/facts/batch`.
- [x] **DID-scoped mutable refs** at
  `/api/documents/repos/{name}/refs/heads/{branch}`.
- [x] **Sibling crates**: [`spacekit-repo`](../spacekit-repo/) (commit types +
  deterministic `fact_id`) and [`spacekit-diff`](../spacekit-diff/)
  (`no_std` tree/blob diff + diff3 merge).
- [x] **`spacekit repo` CLI** — `init / add / status / commit / push / pull /
  branch / checkout / log / diff / clone`.
- [x] **Browser parity** via `createStorageNodeRemoteBlobAdapter` (same wire
  format).

---

## Phase 8 — Envelope Encryption + AWS Secrets

- [x] **Envelope encryption module** (`src/envelope.rs`) — client-side encrypt,
  server-side opaque storage, streaming download.
- [x] **`/files/envelope-upload`, `/files/{id}/stream`, `/files/{id}/rewrap`** —
  per-recipient header rewrap without re-encrypting bulk data.
- [x] **Ephemeral session keypairs** (`/files/{id}/session-key`) for secure
  private-key transmission.
- [x] **PQ server keypair via AWS Secrets Manager** (`aws-secrets` feature,
  `QUANTUM_KEYPAIR_SECRET_NAME`); single source of truth in production.
- [x] **`POST /api/rotate-server-key`** to rotate the server PQ keypair and
  rewrap every envelope header.
- [x] **Encrypted local-keypair fallback** (AES-256-GCM keyed off
  `SPACEKIT_NODE_DID`) with auto-migration from legacy plaintext.

---

## Phase 7 — NFT Collection Management

- [x] **Collection management** — full NFT collection lifecycle.
- [x] **Royalty system** — multi-beneficiary royalty splits and enforcement.
- [x] **Rarity calculation** — trait-based rarity scoring and ranking.
- [x] **Marketplace integration** — sale tracking, price history, analytics.
- [x] **Minting management** — batch minting with supply tracking.
- [x] **Collection queries** — filtering, sorting, pagination.
- [x] **Multi-standard support** — ERC-721, ERC-1155, SPL, custom.
- [x] **Database persistence** — full collection metadata with WAL logging.

---

## Phase 6 — Advanced Fact Storage

- [x] **Quantum-safe fact storage** — SPHINCS+ signature verification system.
- [x] **Multi-policy access control** — Public, Private, Role-based,
  Attribute-based, Dynamic, Conditional.
- [x] **Policy-based encryption** — encryption decisions driven by access
  requirements.
- [x] **Comprehensive indexing** — multi-dimensional with O(1) lookups for
  hot keys.
- [x] **Content integrity** — SHA-256 + Blake3 hash verification.
- [x] **Dependency management** — non-recursive verification + trust scoring.
- [x] **Multi-format support** — Text, Numerical, Boolean, JSON, Binary,
  Reference, Aggregation.
- [x] **Compression** — Gzip, Zstd, Lz4, Brotli.

---

## Phase 5 — Sharding, Full-Text Search, Vector Search

- [x] **Horizontal sharding** — consistent hashing, range, and list-based
  sharding.
- [x] **Cross-shard queries** — parallel execution.
- [x] **Shard rebalancing** — automatic load balancing.
- [x] **Full-text search** — TF-IDF ranking with inverted indexes.
- [x] **Vector search** — semantic similarity with cosine distance.
- [x] **P2P shard integration** — shard discovery and routing via P2P network.

---

## Phase 4 — Enterprise Database (ACID, JOINs, Query Planner, HA)

- [x] **ACID transactions** — isolation levels and savepoints.
- [x] **JOIN operations** — Inner, Left, Right, Full Outer.
- [x] **Query planner** — cost-based optimization.
- [x] **High availability** — leader election, health monitoring, failover.
- [x] **Advanced indexing** — B-tree, hash, composite.
- [x] **Subqueries** — IN, NOT IN, EXISTS, NOT EXISTS.
- [x] **EXPLAIN/ANALYZE** — query execution plan analysis.

---

## Phase 3 — Cross-Service DID Resolution

- [x] **DID-to-peer mapping** across SpaceKit services.
- [x] **Service registry** with health and reputation.
- [x] **Reputation system** — service quality tracking (0.0–5.0).
- [x] **Automatic service detection** via libp2p `identify`.

---

## Phase 2 — Messaging Integration

- [x] **Hybrid discovery modes** — pure P2P, hybrid, messaging-only.
- [x] **Messaging node registration** + bootstrap.
- [x] **Health monitoring** of messaging peers.

---

## Phase 1 — Core Infrastructure

- [x] Quantum-resistant encryption with full data-at-rest implementation.
- [x] DID-based access control.
- [x] Basic P2P networking via libp2p.
- [x] Custom in-memory + persistent JSON database (no required external deps).
- [x] WAL crash recovery + encrypted backups.
- [x] HTTP API server (Warp).
