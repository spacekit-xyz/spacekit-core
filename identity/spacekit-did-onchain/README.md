# SpaceKit DID — On-chain (experimental)

Experimental **EVM contracts**, **Solana program**, and **off-chain bridge helpers** for SpaceKit quantum DIDs.

The production identity library is **[`spacekit-did`](../spacekit-did)** (crates.io). This repository is for chain integration work only.

## Contents

| Path | Description |
|------|-------------|
| [`bridges/`](bridges/) | Rust crate `spacekit-did-bridges` — payload builders, hashes, local SPHINCS+ verify |
| [`quantum-evm-contracts/`](quantum-evm-contracts/) | Solidity registry contracts |
| [`programs/quantum-did-solana/`](programs/quantum-did-solana/) | Anchor program |
| [`examples/`](examples/) | `evm_integration`, `solana_integration` walkthroughs |

## Status

- **Off-chain bridges** — run and test; message formats may not match deployed contracts yet.
- **On-chain verify** — SPHINCS+ verification is **not implemented** on-chain; see [programs/EXPERIMENTAL.md](programs/EXPERIMENTAL.md) and [quantum-evm-contracts/EXPERIMENTAL.md](quantum-evm-contracts/EXPERIMENTAL.md).
- Always verify quantum signatures with **`spacekit-did`** before trusting chain state.

## Quick start

```bash
# Rust bridges + examples
cd bridges
cargo test
cargo run --example evm_integration
cargo run --example solana_integration

# EVM contracts
cd quantum-evm-contracts
npm ci
npx hardhat compile

# Solana (requires Anchor toolchain)
anchor build
```

## Dependencies

- [`spacekit-did`](../spacekit-did) — wallets, credentials, `SphincsPlus`

## Roadmap

See [POTENTIAL_INTEGRATIONS.md](POTENTIAL_INTEGRATIONS.md).

## License

Apache-2.0 — see [LICENSE](LICENSE).
