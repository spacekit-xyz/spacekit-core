# SpaceKit Contract Language (SKCL)

SKCL is a **Solidity-inspired** language used in this monorepo to author contracts that compile to **WebAssembly** for execution on the SpaceKit compute VM (Wasmtime) and tooling aligned with [`spacekit-js`](../spacekit-js/) for parity testing against [`spacekit-js`](../../spacekit-js/).

## Where the compiler lives

The compiler crate path and naming have shifted across workspace iterations. Authoritative entry points:

- Workspace references under **`spacekit-contract-sdk/contract-lang`** (`spacekit-contract-lang`) / contract build scripts as invoked from repo root (see [`docs/SPACEKIT_CONTRACTS.md`](../../docs/SPACEKIT_CONTRACTS.md)).
- Generated artifacts and crates under [`contracts/`](../contracts/) (when present in your checkout).
- Build helper: [`scripts/build_contracts.sh`](../scripts/build_contracts.sh).

## Maturity (honest)

- Treat SKCL as **rolling compiler + ABI surface**: not a drop-in replacement for the full Solidity language or toolchain.
- Prefer **small, exercised examples** that pass `cargo test --test wasm_contracts` over claims about “complete Solidity coverage.”
- Grammar, attribute semantics, and GPU / DID hooks evolve with the VM—track [`documentation/VM_PARITY.md`](VM_PARITY.md) when changing host imports.

## Why SKCL instead of only Solidity?

Directional reasons—not a verdict on other ecosystems:

- **Deterministic WASM + custom host imports** (storage, payments, DID helpers) are first-class in this stack.
- **PQ-centric crypto and identity hooks** are woven into the SpaceKit execution model rather than retrofitted onto an EVM-only toolchain.

For general-purpose EVM deployment, use Solidity and standard compilers; SKCL targets **this** runtime and workspace.

## Quick commands (adjust paths to match your tree)

```bash
cargo test --test wasm_contracts --manifest-path ../Cargo.toml   # from compute-node, if wired in workspace
bash scripts/build_contracts.sh
```

Optional runtime env:

- `SPACEKIT_CONTRACT_POLICIES` — DID-gated policy file for contracts.
- `SPACEKIT_NODE_DID` — node identity context for PQ receipt experiments.

Deeper narrative (historical): [`SPACEKIT_WASM_RUNTIME.md`](SPACEKIT_WASM_RUNTIME.md), [`SPACEKIT_DEVELOPER_GUIDE.md`](SPACEKIT_DEVELOPER_GUIDE.md).
