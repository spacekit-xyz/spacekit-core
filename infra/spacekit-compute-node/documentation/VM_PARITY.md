# Rust compute VM vs `spacekit-js` host parity

Contracts compiled with **`spacekit-contract-sdk`**, the SKCL compiler (**`spacekit-contract-lang`** in `spacekit-contract-sdk/contract-lang`), and WASM emitted from **`spacekit-standard-library`** examples expect a consistent **`env`** (and related) **WASM import surface** across runtimes. This document inventories the main **`env`** symbols and known gaps—but **does not certify** that every contract in those repos runs unchanged on the Rust node until imports match.

## Readiness vs latest SDK / SKCL / standard-library WASM

The Rust compute node **can load and run** WASM that only uses imports implemented in [`swtchvm_node.rs`](../src/spacekitvm/swtchvm_node.rs) (and linked adapters). **Treat parity as incremental:** extend the tables below, register missing symbols, and prefer **one WASM artifact exercised on Rust and JS** before claiming production readiness for a given contract toolchain revision.

## TODO checklist (remaining parity work)

| Item | Status | Notes |
|------|--------|--------|
| `env.abort` | **Done** | Stub on Rust; logs ptrs / line / col (matches JS stub intent). |
| `env.log` / `env.log_output` | **Done** | Aliases on both hosts; Rust reads `(ptr, len)` from linear memory into execution logs. |
| `metering.usegas` | **Done** | Saturating add to execution `gas_used` on Rust. |
| `spacekit_contract.contract_call` | **Done (Rust VM)** | Nested sync `main` / `get_result` on callee code from state; max depth **8**; `prelude::env::call` uses host in SDK. JS still needs `ctx.contractCall` adapter for app-driven calls. |
| **`spacekit_agent` (Growformer)** | **Partial** | Imports registered: `load_brain_from_storage` checks storage node when `storage-integration` is on; **`generation` / `converse` / `codegen` return `-1`** (no Rust Growformer runtime yet); `brain_info` returns JSON stub; `status` = `0`. |
| **`spacekit_messaging`**, **`spacekit_payments`**, **`spacekit_remote_storage`**, **`spacekit_tools`** | **Stub (`-1`)** | Symbols link so **`routekit-agent` WASM loads**; contracts get **`ToolNotConfigured` / host errors** until wired to messaging, payments, SpaceTime remote storage, and search (parity with JS adapters). |
| **`spacekit_session`**, **`spacekit_paymaster`** | **JS done / Rust stub** | **`spacekit-js`**: in-memory `SessionHostState` + `PaymasterHostState` (`host.ts` `createImports`). **Rust compute VM**: still stub / missing until `swtchvm_node.rs` registers matching behavior. |
| `env.storage_read` / `storage_write` vs JS `(keyLen, outputPtr, …)` | **Gap** | Rust `storage_read` signature/behavior still differs from JS `storage_read` / `storage_load`; align keys, lengths, and return conventions. |
| `env.storage_save` / `storage_load` | **Gap** | Present on JS `env`; Rust only exposes `storage_read` / `storage_write` under **`env`** today (`spacekit_storage` has save/load). |
| `env.python_compress` / `python_decompress` | **Gap** | JS imports under **`env`**; Rust registers under **`swtch_compress`** — add **`env`** aliases or document toolchain convention. |
| `env.reputation_*` / `spacekit_reputation` | **Gap** | Implemented on JS; not registered on Rust VM — add stubs or real adapters. |
| `sk_erc20`, `sk_erc721`, `spacekit_fact`, `spacekit_session`, … | **Gap** | Compare full `createImports` in `spacekit-js/src/host.ts` with `swtchvm_node.rs`. |
| Integration tests | **Partial** | `tests/contract_call_nested.rs` (WAT); **`tests/stdlib_wasm_compute_vm.rs`** loads `spacekit-standard-library/target/wasm32-unknown-unknown/release/*.wasm` (override: `SPACEKIT_STDLIB_WASM_DIR`). Same binary on JS still TODO where noted. |

## L2 security model parity

| Item | JS (spacekit-js) | Rust (spacekit-compute-node) | Notes |
|------|-------------------|------------------------------|-------|
| **`simulateCall()`** | **Done** | N/A (L1 executes authoritatively) | Copy-on-write overlay; no side effects |
| **`relayTransaction()`** | **Done** | Accepts via `vm_submit` / `vm_submitSigned` RPC | Simulate locally → submit to L1 → poll receipt |
| **`vm_simulate` RPC** | **Done** | N/A | Read-only JSON-RPC method |
| **`vm_snapshot` RPC** | **Done** | N/A | Exports full KV for state sync |
| **`ContractCallMode: "simulate"`** | **Done** | N/A | Added to `createContractCaller` |
| **`receipt:diverged` event** | **Done** | N/A | Fires when L1 receipt differs from local sim |
| **Tx signature verification** | **Done** (Ed25519, `devMode` flag) | **Done** (ECDSA secp256k1 recovery, `SPACEKIT_DEV_MODE` env) | Both default to dev mode for backwards compat |
| **`contract_kv` in state root** | Already included (flat storage) | **Done** — added to `state_merkle_entries` | Prefixed with `kv:` namespace on Rust |
| **Protected storage prefixes** | **Done** — enforced in `storageWrite` | N/A (L1 state writes are protocol-controlled) | `native:`, `did:document:`, `genesis:` |
| **State snapshot sync** | **Done** — `HeaderSyncClient.syncStateSnapshot()` | Serves via `vm_snapshot` RPC | Verifies stateRoot against header |

## `env` module (representative)

| Import | spacekit-js (`host.ts` `baseEnv`) | Rust (`swtchvm_node.rs` `func_wrap("env", ...)`) | Notes |
|--------|-----------------------------------|--------------------------------------------------|-------|
| `abort` | stub | yes (stub) | Logs warning; does not terminate the host process. |
| `storage_read` | via adapters | yes | **Signature vs JS** — still under review (see checklist). |
| `storage_write` | yes (+ aliases) | yes | JS also exposes `storage_save` / `storage_load` on **`env`** and `spacekit_storage`. |
| `get_caller_did` | yes | yes | |
| `verify_did` | yes | yes | |
| `log` / `log_output` | both (`log` aliases `log_output`) | both (shared impl) | Memory-backed `(ptr, len)` payload. |
| `emit_event` | yes | yes | |
| `get_caller` | — | yes | Extra on Rust |
| `get_block_number` | — | yes | Extra on Rust |
| `msg_value` | yes | yes | |
| `get_balance` | yes | yes | JS maps 20-byte addr → synthetic DID for token adapter |
| `transfer` | yes | yes | Return convention aligned (0 = success) |
| `get_timestamp` | yes (Unix `Date.now`/1000) | yes (Unix wall clock) | |
| Reputation helpers | yes (`env` + `spacekit_reputation`) | — | **Not on Rust yet** (checklist). |
| `python_compress` / `python_decompress` | yes (`env`) | — under **`env`** | Registered as **`swtch_compress`** on Rust today (checklist). |

## Other import namespaces

- **`spacekit_llm`**: **Deprecated on the Rust compute VM** (not registered). Legacy WASM that still imports `spacekit_llm` will fail to instantiate; use **`spacekit_agent`** / Growformer on supported toolchains, or run on **`spacekit-js`** where `spacekit_llm` may still exist for older demos.
- **`spacekit_agent`**, **`spacekit_messaging`**, **`spacekit_payments`**, **`spacekit_remote_storage`**, **`spacekit_tools`**: Registered on the compute node for SDK/agent WASM **link compatibility**; most effect paths return **`-1`** until integrated (see checklist).
- **`spacekit_storage`**, **`spacekit_contract`**, **`metering`**: Rust registers **`metering.usegas`**, **`spacekit_contract.contract_call`** (nested execution, max depth 8), and **`spacekit_storage`** save/load; compare remaining symbols in `createImports` (`spacekit-js/src/host.ts`) with `swtchvm_node.rs`.
- **`swtch_bridge`**: Rust registers bridge helpers; JS may differ.

## Maintenance process

1. When adding a host function in either runtime, update this file and add a contract or integration test that executes the same WASM on both sides where feasible.
2. Prefer **aliases** on both hosts when historical WASM used inconsistent names (`log` vs `log_output`).
