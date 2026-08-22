# `.gf.toml` — Growformer project manifest (draft)

Portable description of an on-chain-ish **Growformer** agent: WASM contract + `.bin` brain + deploy metadata. **Full CLI** reads this manifest for **`spacekit brain-registry build`** (`cargo build --features full`); the default **thin** binary still uses **`init` / `brain push` / `storage`** with explicit flags or shell helpers.

## Minimal file

```toml
gf_version = 1

[project]
name = "my-route-agent"
slug = "my-route-agent"

# Identity that owns uploads / vault billing (thin CLI `--owner-did`, website deploy helpers).
owner_did = "did:spacekit:user:REPLACE"

[growformer]
# Key passed to `growformer_load_brain_from_storage_key` (contract VM storage seeds this from deployment).
brain_storage_key = "chat_brain"

# Optional human-readable label for dashboards.
agent_label = "Support bot v1"

[artifacts]

# Paths are relative to the directory containing `.gf.toml` (recommended layout).
wasm_path = "./target/wasm32-unknown-unknown/release/my_agent.wasm"
brain_bin_path = "./brains/router.bin"

[deploy]

# Resolved by thin CLI via `connections.storage.url` after `spacekit init`, or explicit flags.
storage_url = "http://127.0.0.1:3030"

# Receipt filename after `spacekit storage deploy`.
receipt_json = "./out/deploy-receipt.json"

[extras]
# Free-form table for UI / orchestration hooks (routing rules, frontier operator DIDs).
# tooling_prefix = "/api/agent"
# registry_topics — optional strings matching BRAIN_REGISTRY_AND_STORAGE_SYNC §4 (Gossipsub shards).
# registry_topics = [ "spacekit/brain-registry/v1/public/deadbeef/embed" ]
```

## Field reference

| Key | Required | Purpose |
| --- | --------- | ------- |
| `gf_version` | yes | Bump when breaking semantics. |
| `project.name` / `slug` | yes | Human + machine IDs. |
| `project.owner_did` | yes for deploy scripts | Charges + storage ACLs. |
| `growformer.brain_storage_key` | yes | Matches contract constant (see `spacekit-growformer-agent`, `routekit-agent`). Storage-wide **catalog + topical replication** between nodes: `BRAIN_REGISTRY_AND_STORAGE_SYNC.md`. |
| `artifacts.wasm_path` | yes for local CI | Produced `cdylib` WASM. |
| `artifacts.brain_bin_path` | yes for brain push | Passed to **`spacekit brain push`** (`thin` CLI) or bundled in `storage deploy`. |
| `deploy.storage_url` | optional | Override default storage base URL. |
| `deploy.receipt_json` | optional | Written by deploy helpers for website-api lookup. |
| `extras.registry_topics` | no | Passed into **`brain-registry build`** output `topics[]` (Gossip shards). |

## CLI alignment (thin)

Today you can approximate this manifest manually:

```bash
spacekit init                                     # ~/.spacekit + PQ keys (thin CLI)
cargo build --target wasm32-unknown-unknown       # WASM
spacekit brain push ./brains/router.bin \
  --owner-did "$(grep owner_did .gf.toml | cut -d'\"' -f2)"
spacekit storage deploy \
  --wasm ./target/wasm32-unknown-unknown/release/my_agent.wasm \
  --bin ./brains/router.bin \
  --owner-did "$(grep owner_did .gf.toml | cut -d'\"' -f2)"
```

## Full CLI — brain registry (`cargo build --features full`)

Brain manifest **build** / **publish** live only on the **embedded** CLI (not the default thin binary). After `storage deploy`:

```bash
spacekit brain-registry build \
  --gf-toml ./.gf.toml \
  --receipt ./out/deploy-receipt.json \
  --network-context local \
  --crate-name routekit-agent \
  --out ./out/brain-registry-manifest.json

spacekit brain-registry publish \
  --manifest ./out/brain-registry-manifest.json \
  --collection brain_registry \
  --storage-url "http://127.0.0.1:3030"
```

- **Publish** sends `Authorization: DID <publisher>` (`--publisher-did`, else manifest `publisher_did`, else `~/.spacekit/config.toml`). Uses **`spacekit-storage-node`** document semantics (**Fact**/SPHINCS+ verification is storage-native; simulator HTTP mirrors URL shape only).
- Default manifest document **id** = SHA-256 hex of compact manifest JSON UTF-8.
- Thin CLI **brain push / storage deploy / init** unchanged.

Parsing `.gf.toml` for **producer tooling** (`brain-registry build`) is implemented in **full CLI**; thinner clients may remain shell/grep wrappers for other fields until unified.

## Vault & storage costing (thin pattern)

Charges in agent WASM use **`payment_vault_charge`** (fire-and-forget until execution flush); amounts are conventionally **ASCII micro-units** of the treasury asset (`"200"` = `0.002` AUSD in RouteKit tiers). Prefer **tier constants** mirrored between `.gf.toml` `extras.*` hints and Rust `COST_*` literals so explorers and auditors can reconcile.

Suggested mapping for RouteKit-tier agents deployed from this repo:

| RouteKit tier (placeholder) | `payment_vault_charge` arg |
| ---------------------------- | ---------------------------- |
| local completion | `"100"` |
| search (+ JSON hydrate) | `"200"` |
| search + local synthesis | `"300"` |
| frontier escalate | `"5000"` |

Exact economics are **deployment-specific**: wire **operator quotes** (`extras.frontier_charge_hint`) beside these defaults when publishing a `.gf.toml`.
