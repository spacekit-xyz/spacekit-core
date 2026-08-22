# SpaceKit Smart Contracts

This is the top‑level reference for SpaceKit smart contracts, including the
Solidity‑inspired SpaceKit Contract Language (SKCL 💀), the runtime ABI, and build/test flow.

## Components
- **SKCL compiler**: `spacekit-contract-lang` (workspace member under **`spacekit-contract-sdk/contract-lang/`**)
- **Contract SDK**: `spacekit-contract-sdk` (workspace root, same repository)
- **Runtime**: `spacekit-compute-node` (SpaceKit WCVM)
- **Spec**: `spacekit-contract-sdk/contract-lang/SKCL_V1.md`
- **Website overview**: `SPACEKIT_CONTRACTS_OVERVIEW.md`

## Build flow
1) **Generate contract crate**
```
cargo run --manifest-path spacekit-contract-sdk/Cargo.toml -p spacekit-contract-lang -- \
  contract-lang/examples/astra_token.scl \
  spacekit-compute-node/contracts
```

2) **Build WASM artifacts**
```
bash spacekit-compute-node/scripts/build_contracts.sh
```

3) **Run harness**
```
cargo test --manifest-path spacekit-compute-node/Cargo.toml --test wasm_contracts
```

## ABI details (Solidity‑style)
- Static types (bool/u64/u128/address) → 32‑byte words (big‑endian)
- Dynamic strings → offset + length + padded bytes
- Function selector → Keccak‑256 first 4 bytes
- Event topic → Keccak‑256 of event signature

## DID + Quantum integration
- **DID identity**: `spacekit-did` for DID verification across compute/storage.
- **Post-quantum crypto**: `spacekit-primitives` (Kyber/Dilithium/SPHINCS+).
- **Zero-knowledge storage**: `spacekit-storage-node` encrypts at rest with user keys.
 - **Execution receipts**: WCVM attaches PQ signatures to execution results when `SPACEKIT_NODE_DID` is set.

## DID-gated contract calls
SKCL supports `require did` per function, which injects:
- `env.get_caller_did` (runtime host function)
- `env.verify_did` (quantum DID verification)

**Policy file (runtime)**
Set `SPACEKIT_CONTRACT_POLICIES=contract_policies.json` to enforce by selector/opcode.
Example:
```
{
  "default": { "require_did_opcodes": [1], "require_did_selectors": ["0xa9059cbb"] },
  "0xabc123...": { "require_did_opcodes": [2] }
}
```
The compiler emits `contract_policies.json` per contract; copy or merge into your runtime policy.
The compiler also emits `$contract:ContractName` placeholder keys for Solidity-style deployment workflows.

## Production signals
- ✅ ABI selectors/topics are Keccak‑256 compatible
- ✅ Events emitted via `env.emit_event`
- ✅ DID‑gated calls supported (`require did`)
- ✅ PQ execution receipts enabled with `SPACEKIT_NODE_DID`
- ✅ Policy files generated and merged (`contract_policies.merged.json`)

## Key docs
- `spacekit-contract-sdk/contract-lang/README.md` (language syntax, compiler output)
- `spacekit-contract-sdk/README.md` (runtime helpers, entrypoints)
- `spacekit-compute-node/documentation/SWTCH_WASM_RUNTIME.md` (SpaceKit WCVM runtime + ABI)
