# Contract SDK ↔ JS host modules

This reference ties together:

- **`spacekit-contract-sdk`** (Rust, `no_std`) — symbols your WASM links against.
- **SpacekitVM-JS** (`createHost` / `SpacekitVm`) — provides the WASM imports and fulfills async effects.

For the authoritative import list surfaced to contracts, see `HOST_IMPORT_MODULES` and `HOST_ABI_VERSION` in `src/vm/abi.ts`.

## Return codes (`main`)

`main` uses `positive` lengths for successful output sized reads via `get_result`, and **`<= 0` error codes** aligned with Rust `ContractError` where applicable.

Extended agent-tool errors:

| Code | Name (Rust `ContractError`) | Meaning |
| ----- | ---------------------------- | ------- |
| `-12` | `ToolNotConfigured` | Adapter missing (`-1` from host tool import). |
| `-100` | `NeedsTools` | Matches `STATUS_NEEDS_TOOLS`; host must fulfill pending effects and **re-enter** WASM (see below). |

## Effect queue vs fire-and-forget

Host behavior is implemented in `src/host.ts`, effect loop in `src/vm/spacekitvm.ts` (`fulfillToolEffects`, `flushSideEffects`).

| WASM module | Function | Rust SDK module | Fulfillment pattern | Pending when |
| ------------ | --------- | ----------------- | -------------------- | ------------- |
| `spacekit_tools` | `web_search` | `spacekit_contract_sdk::tools::web_search` | Async effect → cache → re-run | Host returns `-3`; contract returns `-100`; VM fulfills via `MessagingAdapter.requestResponse` (`SPACEKIT_WEB_SEARCH_TOPIC`). Requires `toolOperatorDid` + `messaging.requestResponse`. |
| `spacekit_remote_storage` | `remote_storage_put`, `remote_storage_get` | `remote_storage::*` | Effect queue | Same `-3` / `-100` flow. **`createStorageNodeRemoteBlobAdapter`** implements **`RemoteStorageAdapter`** (`put`/`get` raw bytes ↔ `value_hex` documents). **`createSpaceTimeStorageNode`** targets JSON blobs only—not this interface. |
| `spacekit_messaging` | `messaging_send` | `messaging::messaging_send` | Buffered; flushed **after** run | Never blocks contract; returns `-1` if no `MessagingAdapter`. |
| `spacekit_payments` | `payment_transfer`, `payment_vault_charge` | `payments::*` | Buffered flush | Same as messaging. |
| `spacekit_session` | `session_create`, `session_validate`, `session_revoke` | `session_keys::*` | In-memory host (`SessionHostState`); owner = caller on create/revoke; validate checks delegate = caller | Synchronous; returns `-2` on malformed input / policy violation. |
| `spacekit_paymaster` | `paymaster_set_policy`, `paymaster_sponsor_charge`, `paymaster_budget` | `paymaster::*` | Policy + budget enforced in `PaymasterHostState`; `sponsor_charge` also buffers optional `PaymentAdapter.sponsorVaultCharge` | `allowed_dids` / `allowed_ops` must be non-empty JSON arrays for charges to succeed; budget is decimal string micro-units (digits only). |

**Re-entry cap:** `MAX_TOOL_ROUNDS` (`src/tools/effect_manager.ts`), default four rounds per transaction.

**Nested sync contract calls:** `callContractInternal` does **not** run the async effect loop — avoid effect-queue tools from nested `contractCall` paths until async syscalls land.

IndexedDB **`StorageNodeAdapter`** sync paths and simulator vs Rust storage distinction: **`storage-sync.md`**.

### Messaging endpoints (operator / dev)

- Envelope sends: default `POST …/api/messages/envelope`.
- Tool intents (search etc.): `POST …/api/messages/tool-request` (`IntentMessagingToolAdapter`).
- Optional forward URL on nodes: **`SPACEKIT_TOOL_REQUEST_FORWARD_URL`**.

## Well-known Rust modules (crate root re-exports)

| Module | Typical use |
| ------ | ----------- |
| `spacekit_contract_sdk::tools` | `web_search` |
| `spacekit_contract_sdk::messaging` | `messaging_send` |
| `spacekit_contract_sdk::remote_storage` | `remote_storage_put`, `remote_storage_get` |
| `spacekit_contract_sdk::payments` | `payment_transfer`, `payment_vault_charge` |
| `spacekit_contract_sdk::session_keys` | `session_create`, `session_validate`, `session_revoke` |
| `spacekit_contract_sdk::paymaster` | `sponsor_charge`, `set_policy`, `budget` |

Lower-level WASM modules (`env`, `spacekit_storage`, `spacekit_agent`, tokens, ERC contracts, …) are unchanged from `host-abi-contracts.md` / existing SDK docs.

## Example contract

Minimal agent that links all four agent modules ships as `spacekit-standard-library/agents/routekit-agent` (see **`README.md`** in that crate for wire format, opcodes, and host imports).

## Brain registry × storage refs

Deployments that seed VM storage (`brain_storage_key` → `.bin`) align with **`BRAIN_REGISTRY_AND_STORAGE_SYNC.md`**: manifests carry **`storage_ref`** values that map to **`remote_storage_get`** payloads on subscribers; tiers and topics stay out of WASM.

For **signature-bearing** manifests and **Facts**, use **`spacekit-storage-node`** (SPHINCS+ / policy pipelines, P2P). The **`spacekit-simulator` HTTP** `/api/documents` layer is an in-memory shim for API tests only (**`storage-sync.md`**).
