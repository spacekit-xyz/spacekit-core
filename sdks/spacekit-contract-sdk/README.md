# SpaceKit Contract SDK

Minimal `no_std` SDK for building SpaceKit WASM smart contracts. Contracts compile to `wasm32-unknown-unknown` and link against the SpaceKit host ABI.

- **`spacekit-js`** — full `createImports` surface + adapters (preferred for agent contracts today).
- **`spacekit-compute-node`** — registers the same **import module names** incrementally (`swtchvm_node.rs`). **`spacekit_contract!` is compile-time only** (it does not “run” on the node); the node must expose WASM imports such as `spacekit_agent`, `spacekit_payments`, etc. Growformer **inference** and several effect paths may still be **stubs** on the Rust VM — use JS or extend the node for production agent behaviour.

**Repository:** [spacekit-xyz/spacekit-core](https://github.com/spacekit-xyz/spacekit-core/tree/main/sdks/spacekit-contract-sdk)

---
Note: This SDK is a work in progress and is not yet published to crates.io. It is only available from the SpaceKit GitHub repository. Pardon our dust.
---

## What it provides

- **`SpacekitContract`** trait and **`spacekit_contract!`** macro — export `main(input_ptr, input_len)` and `get_result(dest_ptr, max_len)` for the host.
- **Events** — `emit_event_bytes` / `emit_event_signature` (host: `env.emit_event`).
- **DID** — `get_caller_did_string`, `verify_did_string`.
- **Payments** — `msg_value_u64`, `get_balance_of`, `transfer_to`, `require_payment`, `block_timestamp`.
- **(Deprecated) LLM** — `llm_call`, `llm_get_status`, `llm_status` constants; host module `spacekit_llm`.
- **Growformer** — `growformer_generation`, `growformer_converse`, `growformer_codegen`, `growformer_brain_info`, `growformer_reset_conversation`; host module `spacekit_agent`.
- **Storage** — `storage_set`, `storage_get`, string helpers; host module `spacekit_storage`.
- **Errors** — `ContractError` with `Failed`, `InvalidInput`, `StorageError`, `HostError`, `LlmError`, `LlmNotReady`, `GrowformerNotReady`, `GrowformerError`, `InsufficientPayment`, `InsufficientBalance`, `Unauthorized`.
- **Wire** — `wire` module: cursor-based LE `read_u8` / `read_u16` / `read_u32` / `read_u64`, length-prefixed `read_string` / `read_string_max`, `read_bytes_u16` / `read_bytes_u32`, `read_fixed` / `read_bytes32`, `expect_consumed`, and matching `write_*` helpers for building response buffers.
- **Prelude** — `prelude::env` (`caller`, `msg_value`, `require_payment`, `transfer`, `balance_of`, `block_timestamp`, `emit`, `call`).

### SKCL compiler (same repository)

This workspace also ships **`spacekit-contract-lang`** — the SpaceKit Contract Language (SKCL) compiler that emits Rust + `Cargo.toml` wired to this SDK. Source and spec live under **`contract-lang/`** (see `contract-lang/README.md` and `contract-lang/SKCL_V1.md`). Run it with:

```bash
cargo run -p spacekit-contract-lang -- contract-lang/examples/astra_token.scl <output_dir>
```


## Usage

Add to `Cargo.toml`:

```toml
[dependencies]
spacekit-contract-sdk = { git = "https://github.com/spacekit-xyz/spacekit-core" }
# or from crates.io when published:
# spacekit-contract-sdk = "0.1"
```

Contract example:

```rust
#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use spacekit_contract_sdk::{SpacekitContract, ContractError, spacekit_contract};

struct Example;

impl SpacekitContract for Example {
    type Error = ContractError;
    fn init() -> Self { Self }
    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        Ok(input.to_vec())
    }
}

spacekit_contract!(Example);
```

Build (use `panic = "abort"` in your contract’s `[profile.release]` for `wasm32`):

```bash
cargo build --target wasm32-unknown-unknown --release
```

---

## Host ABI (contract expectations)

The VM must provide:

| Module | Functions |
|--------|-----------|
| **env** | `emit_event`, `get_caller_did`, `verify_did`, `msg_value`, `get_balance`, `transfer`, `get_timestamp` |
| **spacekit_llm** | `llm_inference`, `llm_status` |
| **spacekit_agent** | `agent_growformer_status`, `agent_growformer_load_brain_from_storage`, `agent_growformer_generation`, `agent_growformer_converse`, `agent_growformer_codegen`, `agent_growformer_brain_info`, `agent_growformer_reset_conversation` |
| **spacekit_storage** | `storage_save`, `storage_load` |

### Payment Host Functions

Contracts can interact with native ASTRA balances:

```rust
use spacekit_contract_sdk::prelude::*;

// Require minimum payment
let paid = env::require_payment(1000)?; // at least 1000 ASTRA

// Check a balance
let bal = env::balance_of(&address_bytes);

// Transfer ASTRA
env::transfer(&recipient_bytes, 500)?;

// Get current timestamp
let ts = env::block_timestamp();
```

Both the Rust compute node (`swtchvm_node.rs`) and the JS VM (`spacekit-js/src/host.ts`) implement these host functions with identical signatures.

See [SpaceKitJS Technical Whitepaper](https://github.com/spacekit-xyz/spacekit-js) for the full host ABI and execution model.

---

## License

Apache-2.0. See the repository for the full [LICENSE](https://github.com/spacekit-xyz/spacekit-core/blob/main/sdks/spacekit-contract-sdk/LICENSE) file.
