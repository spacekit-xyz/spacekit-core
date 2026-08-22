# SpaceKit Smart Contracts (SKCL 💀) — Overview

SpaceKit Smart Contract Language (SKCL 💀) is a Solidity‑inspired language that compiles to WASM for the SpaceKit Compute Node Virtual Machine. It provides deterministic execution, DID‑native access control, and post‑quantum security, while preserving Solidity‑style ABI selectors and event topics.

## What you get
- **Solidity‑style ABI parity**: selectors and event topics use Keccak‑256
- **WASM runtime**: contracts run on SpaceKit WCVM
- **DID‑gated functions**: `require did` enforces identity checks
- **Quantum receipts**: execution results can be SPHINCS+ signed
- **Developer tooling**: compiler, SDK, ABI outputs, and test harness

## Quickstart
Generate a contract (from monorepo root):
```
cargo run --manifest-path spacekit-contract-sdk/Cargo.toml -p spacekit-contract-lang -- \
  contract-lang/examples/astra_token.scl \
  spacekit-compute-node/contracts
```

Or from inside **`spacekit-contract-sdk/`**:
```
cargo run -p spacekit-contract-lang -- contract-lang/examples/astra_token.scl ../spacekit-compute-node/contracts
```

Build WASM artifacts:
```
bash spacekit-compute-node/scripts/build_contracts.sh
```

Run contract tests:
```
cargo test --manifest-path spacekit-compute-node/Cargo.toml --test wasm_contracts
```

## Core components
- **SKCL 💀 compiler**: `spacekit-contract-lang` (crate in **`spacekit-contract-sdk/contract-lang/`**)
- **Contract SDK**: `spacekit-contract-sdk` (same repo / workspace root)
- **Runtime**: `spacekit-compute-node` (SpaceKit Compute Node Virtual Machine)

## ABI and events (v1)
- Static types → 32‑byte words (big‑endian)
- `string` → dynamic encoding (offset + length + padded bytes)
- Function selector → Keccak‑256 first 4 bytes
- Event topic → Keccak‑256 of event signature

## Policy and security
- **DID gating**: `require did` in SKCL 💀
- **Runtime policy**: `SPACEKIT_CONTRACT_POLICIES=contract_policies.json`
- **Quantum signing**: set `SPACEKIT_NODE_DID` to enable PQ execution receipts

## Spec and docs
- SKCL 💀 spec: `spacekit-contract-sdk/contract-lang/SKCL_V1.md`
- Integration guide: `SPACEKIT_CONTRACTS.md`
