# SpaceKit Quantum-Resistant Identity Library

Rust library for **post-quantum (SPHINCS+) signing**, local wallets, and credential helpers used across SpaceKit. It is one layer in a larger platform—not a standalone DID network.

> **System context:** DIDs are created locally by [`spacekit init`](spacekit-cli) and enrolled on testnet via the CLI + compute node. Read **[ARCHITECTURE.md](ARCHITECTURE.md)** before auditing or integrating.

## Role in the SpaceKit stack

| Step | Component | What happens |
|------|-----------|----------------|
| 1 | **`spacekit init`** | Creates `~/.spacekit`, KEM keys, placeholder `did:spacekit:user:{uuid}` |
| 2 | **`spacekit did create`** (optional) | Builds `did:spacekit:testnet:…` with SPHINCS+; POSTs to compute node if running |
| 3 | **Testnet running** | Compute node validates registration, registers validator DID for consensus |
| 4 | **This crate** | Shared `SphincsPlus`, `QuantumResistantWallet`, VC types used by CLI and nodes |
| 5 | **Registry contract** (WIP) | `spacekit-did-registry` WASM—persistent resolve/rotate via SpaceKitVM |

Chain reference code: [`spacekit-did-onchain`](../spacekit-did-onchain) (experimental, not production registry).

## Quick start (library only)

```toml
[dependencies]
spacekit-did = "0.1.0"
```

```rust
use spacekit_did::QuantumResistantWallet;

let wallet = QuantumResistantWallet::new();
println!("DID: {}", wallet.identity_doc.did.as_ref());

let message = "Hello, quantum-resistant world!";
let signature = wallet.sign_content(message).unwrap();
assert!(wallet.verify_content(message, &signature).unwrap());
```

For CLI identity setup: `spacekit init`, then `spacekit network up`, then `spacekit did create` when you need a testnet-registry-shaped DID.

## What this crate includes

- **SPHINCS+** — `SphincsPlus` (SHAKE-256-128s-simple), `no_std` capable
- **Wallet** — `QuantumResistantWallet`, credentials, key rotation (local)
- **Registry traits** — `VerifiableDataRegistry`, `DidResolver` (bring your own backend)
- **VPN-style VCs** — `VcIssuer` / `SpacekitVcVerifier` (signature + expiry + issuer keys via resolver)

## What is not in this crate

See **[ARCHITECTURE.md](ARCHITECTURE.md)** for the full list. Summary:

- No `spacekit init` (CLI)
- No networked registry or resolve server
- No compute-node HTTP registration
- Not W3C DID/VC compliant (`did:spacekit:*` is a custom method)
- Wallet `verify_credential` does not resolve issuer keys from a registry (use `SpacekitVcVerifier` or your registry integration)

## API overview

| Area | Types |
|------|--------|
| Crypto | `SphincsPlus`, `QuantumKeyPair`, `CryptoError` |
| Identity | `QuantumResistantWallet`, `DecentralizedIdentifier`, `VerifiableCredential` |
| Registry | `VerifiableDataRegistry`, `DidResolver`, `SpacekitDidResolver` |
| Wallet | `DidWallet`, `InMemoryDidWallet`, `LocalDid` |
| VPN access | `DidBasedVcIssuer`, `SpacekitVcVerifier`, `VpnPolicy` |

Constants: `spacekit_did::VERSION`, `spacekit_did::DEFAULT_DID_METHOD` (`"spacekit:testnet"`).

## Examples

```bash
cargo run --example basic_usage
cargo run --bin spacekit-did-demo
```

End-to-end testnet flows: **spacekit-cli** + **spacekit-compute-node** (not examples in this repo).

## Security & audit

- Report issues: [SECURITY.md](SECURITY.md)
- Audit scope and known gaps: [ARCHITECTURE.md](ARCHITECTURE.md)

## Development

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

## License

Apache-2.0 — [LICENSE](LICENSE). Copyright (c) 2026 SWTCH Labs LLC.

## Links

- **Architecture (read first):** [ARCHITECTURE.md](ARCHITECTURE.md)
- **Source:** [spacekit-xyz/spacekit-core](https://github.com/spacekit-xyz/spacekit-core/tree/main/identity/spacekit-did)
- **On-chain (experimental):** [spacekit-did-onchain](spacekit-did-onchain)
