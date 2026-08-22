# Standalone binary internals (`spacekit-compute-node`)

**Source file:** [`src/bin/standalone.rs`](../src/bin/standalone.rs)

The **`standalone`** Cargo feature builds the **`spacekit-compute-node`** CLI. It also enables **`spacetime-consensus`**, which links `spacekit-spacetime-consensus` and optional **`spacetime_transition`** on [`BlockData`](../src/consensus.rs) (validated in [`spacetime_integration`](../src/spacetime_integration.rs) when present). Library consumers can opt in with `--features spacetime-consensus` without pulling it into **`default`**. Verkle aux proofs in the spacetime crate remain behind that crate’s **`verkle`** feature (off by default).

That binary wires:

- **Configuration:** loads `config.toml` (path from `--config`), expands leading `~/` in `[identity]` key paths on Unix (see `expand_tilde_path` / `normalize_config_paths` in the same file).
- **Node lifecycle:** constructs [`SwtchComputeNode`](../src/bin/standalone.rs) (HTTP + P2P + consensus coordinator hooks), delegates heavy compute to the **`spacekit_compute_node`** library.
- **Identity startup:** applies `[identity]` DID string + optional CLI Kyber `.hex` material as documented in [`RUNBOOK.md`](RUNBOOK.md).

## CLI subcommands (`Commands` enum)

| Subcommand | Role |
|------------|------|
| `start` | Full server: `ComputeNode::start`, network registration, optional HTTP listener (`tokio::spawn` + `warp::serve`) when `[network].enable_http_api` is true or not set. |
| `status` | Builds node from config, prints JSON status (`get_node_status`). |
| `production-test` | Runs [`ProductionTestingSuite`](../src/testing/mod.rs) against a `ComputeNode`. |
| `register`, `gpu-info`, `test` | **Incomplete / stub** at last review—confirm current bodies before relying on them. |

Global flags include **`--config`**, **`--port`** (HTTP **`[network].rpc_port`** override for `start`), **`--p2p-port`**, **`--bootstrap`**, **`--node-did`**, **`--verbose`**. Subcommand **`start`** adds **`--no-http`** to disable binding the HTTP stack regardless of config (sets the same effect as `[network] enable_http_api = false`).

## HTTP API (Warp)

When **`[network].enable_http_api`** is **`true`** (the default), all routes are composed in **`SwtchComputeNode::start_http_server`** in `standalone.rs`: health, status, SwtchVM dev surface (`SwtchvmNode::http_dev_api_routes`: `/faucet`, `POST /rpc`, account/block/rollup, etc.), `/v1/*` (onboarding balance, DID register/resolve, state anchor, keymaster, consensus, payments, execute intent, etc.). When **`enable_http_api`** is **`false`** or you pass **`start --no-http`**, none of these listeners bind — P2P and compute still start. There is no separate router crate—**this function is the authoritative list** until an OpenAPI export exists.

### Subscriber / light-client sync (`GET /v1/sync/subscriber`)

Thin clients (browsers, agents, secondary validators) can poll:

- **`GET /v1/sync/subscriber`** — JSON bundle built by [`subscriber_sync::build_subscriber_sync_bundle`](../src/subscriber_sync.rs): `wire_version`, `[compute] chain_id`, SwtchVM **head** (`number`, `hash_hex`, `parent_hash_hex`, `state_root_hex`, `timestamp`), optional **`l1_manifest`** from [`SwtchvmNode::read_l1_manifest`](../src/spacekitvm/swtchvm_node.rs), and **`endpoints`** hints (`/block/header/{n}`, `/l1/manifest`, `/v1/consensus/propose`, finality query).

This complements **spacetime** rotor-chain verification (`spacekit-spacetime-consensus`, `light_client.rs`), which is a separate wire format for transition proofs.

### Unified consensus propose (`POST /v1/consensus/propose`)

Submits a proposal to the in-process [`UnifiedSWTCHConsensus`](../src/consensus.rs) (same types as the `spacekit` CLI `consensus submit` path).

When **`spacetime-consensus`** is enabled (it is part of the **`standalone`** feature set for this binary), clients may include **`block.spacetime_transition`**, optional **`block.consensus_votes`**, and **`block.signed_block_envelope`** in the JSON body. Set **`finalize": true`** on propose to run the PQ finisher after submit (Dilithium votes + one SPHINCS+ block envelope). Or call **`POST /v1/consensus/finalize`** with `{ "proposal_id": "…", "round": 0, "view": 0 }` once the coordinator reports finality.

**Body (JSON):**

| Field | Type | Description |
|-------|------|-------------|
| `type` | string | `"block"`, `"metrics"`, or `"hybrid"`. |
| `proposer_did` | string? | Defaults to the node’s configured DID. |
| `announce` | bool | If `true` and `type` is `block`, also broadcast `BlockAnnounce` via [`ConsensusCoordinator`](../src/consensus_coordinator.rs). |
| `use_swtchvm_head` | bool | Fill missing block fields from [`SwtchvmNode::get_latest_block`](../src/spacekitvm/swtchvm_node.rs) (next height, parent = head hash, state root = head state root). |
| `use_l1_snapshot_manifest` | bool | When building `block.l1_manifest`, prefer the on-disk L1 manifest if **`checkpoint.height` == `block_number`** and **`checkpoint.state_root_hex`** matches the proposal state root (see [`merge_l1_manifest_for_proposal`](../src/subscriber_sync.rs)); otherwise use [`minimal_l1_manifest_for_proposal`](../src/spacekitvm/l1_checkpoint.rs). Ignored if the client sends an explicit `block.l1_manifest`. |
| `block` | object? | `block_number`, `parent_hash`, `transactions`, `state_root`, `chain_id`, `l1_manifest`, optional **`spacetime_transition`** (when built with `spacetime-consensus`) — optional when `use_swtchvm_head` is `true`. |
| `metrics` | object? | Required for `metrics` / `hybrid`: `cpu_utilization`, `memory_utilization`, `network_utilization`, `storage_utilization`. |

**Responses:** `200` with `{ "status": "submitted", "proposal_id": "<uuid>" }`, or `400` with `{ "error": "..." }`.

**Example — anchor block proposal to current SwtchVM head:**

```bash
curl -sS -X POST "http://127.0.0.1:9000/v1/consensus/propose" \
  -H "Content-Type: application/json" \
  -d '{"type":"block","use_swtchvm_head":true,"use_l1_snapshot_manifest":false,"block":{"transactions":[]}}'
```

**Example — subscriber snapshot:**

```bash
curl -sS "http://127.0.0.1:9000/v1/sync/subscriber"
```

**Automated test:** `tests/subscriber_sync_bundle.rs` exercises the sync bundle builder against `SwtchvmNode::new`; unit tests for manifest merge live in [`subscriber_sync.rs`](../src/subscriber_sync.rs) (`merge_l1_manifest_for_proposal`).

## Related library surfaces

- WASM / host imports: [`src/spacekitvm/swtchvm_node.rs`](../src/spacekitvm/swtchvm_node.rs)
- Core compute library entry: [`src/lib.rs`](../src/lib.rs)

When this README’s parent [**`README.md`**](../README.md) says “see standalone,” it means **this doc first**, then the Rust source for exact signatures.
