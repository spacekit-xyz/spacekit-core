# **SpaceKit Standard Library**

The **SpaceKit Standard Library** is the official collection of **smart contracts** for the SpaceKit ecosystem.  
These contracts run natively in both the **SpaceKit Compute Node** and the **SpaceKit‑JS VM**, providing developers with safe, reusable building blocks for decentralized applications, agents, and compute workflows.

This library plays a role similar to **OpenZeppelin** in Solidity:  
a foundation of primitives you can import directly into your SpaceKit projects.

**Note:** This Standard Library is a work in progress. These smart contracts have not been audited or tested for production. We are working on it and will publish to crates.io when ready. Pardon our dust.

---

## **📦 What’s Included**

Each contract in the Standard Library is:

- implemented as an independent Rust crate  
- compiled to deterministic `wasm32-unknown-unknown`  
- compatible with the SpaceKit Contract Language (SKCL)  
- ready for deployment in the SpaceKit Compute VM and SpaceKit‑JS VM  

Modules include (this list matches the workspace members in the root `Cargo.toml`):

Wire-format helpers for `handle` arguments and responses live in the sibling
**[`spacekit-contract-sdk`](../spacekit-contract-sdk/)** crate
(`spacekit_contract_sdk::wire`) so any contract that already depends on the SDK
can import them without an extra crate.

### Access control
- **astra-access-control** — Access control primitives (`access/`)

### Tokens & Finance
- **sk-erc20**, **sk-erc721**, **sk-erc1155**, **sk-erc8004** — Token standards (`tokens/`)
- **astra-escrow**, **astra-payment-router** — Escrow and payment routing in ASTRA (`payments/`)
- **spacekit-paymaster** — ERC-4337–inspired paymaster for sponsored agent execution: holds native ASTRA deposited by sponsors and pays permitted callers under a policy (per-call and daily limits, allowed DIDs and ops, expiry) (`payments/`)
- **payable-demo** — Payable / payment integration demo: receiving ASTRA attached to a call (`payments/`)
- **spacekit-reputation** — Reputation primitive (`reputation/`)
- **app-store** — App distribution & licensing (`app-store/`)

### Marketplace
- **astra-entitlement-ledger** — Entitlement / marketplace ledger for paid content, channels and app listings. Purchase and renewal pay the listing price in ASTRA straight to the publisher's address; the ledger keeps no funds (`marketplace/`)

### Storage & infra
- **sk-storage-box** (spacekit-box) — Blob/store contract using `spacekit_storage` → `storage/`
- **sk-video-tube** (spacekit-tube) — Video blob + metadata contract → `storage/`
- **compression-service** — Compression utility → `services/`

### System & identity
- **spacekit-did-registry** — DID registry (`system/`)
- **spacekit-session-keys** — Session keys for delegated agent execution (`system/`)
- **spacekit-shared-vault** — Shared vault for multi-signer file access control; it holds no funds (`system/`)

### Agents & AI
- **spacekit-agent** — Kit on-chain agent (CHAT, ANALYZE, SUMMARIZE, CODE_REVIEW, CLASSIFY, STATUS) → `agents/`
- **spacekit-agent-microgpt** — Micro-GPT next-token agent (uses `microgpt_forward` host primitive; Rust + AssemblyScript) → `agents/spacekit-agent-microgpt`
- **spacekit-growformer-agent** — Growformer brain on-chain agent (`agents/`)
- **spacekit-growformer-sentiment-analysis**, **spacekit-growformer-crypto-analysis**, **spacekit-growformer-fintech-analysis** — Growformer-backed domain analysis agents (`agents/`)
- **routekit-agent** — RouteKit agent (Growformer + web search + messaging + remote storage) → `agents/routekit-agent`
- **spacekit-agent-inference-market**, **spacekit-inference-mesh** — Agent inference (`agents/`)
- **spacekit-intent-classifier** — Intent classification (`agents/`)
- **spacekit-spacetime** — Spacetime discussion forum (`agents/`)

### Payments in contracts
ASTRA is the only currency. A contract receives ASTRA as the value attached to a call and can pay only from its own balance (`transfer_u128`, or `payment_transfer` with asset `ASTRA`); it can never spend its caller's balance. Other assets are refused, and the old vault charge (`payment_vault_charge`) is always refused. Paid operations use the SDK's `collect_fee(price_wei, payee)`, which forwards everything attached to the payee: the standard-library agents and the anchor contract charge their fees in ASTRA (priced in micro-ASTRA) and pay them to the treasury contract.

---

## **🧱 Repository Structure**

```
spacekit-standard-library/
  ├─ access/              # Access control (astra-access-control)
  ├─ agents/              # AI agents, Growformer, RouteKit, inference, spacetime, …
  ├─ app-store/           # App distribution & licensing
  ├─ marketplace/         # Entitlements (astra-entitlement-ledger)
  ├─ payments/            # Escrow, router, paymaster, payable demo
  ├─ reputation/          # Reputation (spacekit-reputation)
  ├─ services/            # Infra/utility (e.g. compression-service)
  ├─ storage/             # Storage primitives (spacekit-box, spacekit-tube)
  ├─ system/              # DID registry, session keys, shared vault
  ├─ tokens/              # ERC-20, ERC-721, ERC-1155, ERC-8004
  └─ README.md
```

Each top-level directory contains one or more standalone contract crates with their own `Cargo.toml`, source code, and tests.

---

## **🔨 Building the Contracts**

All contracts in the Standard Library compile to WASM using Rust’s `wasm32-unknown-unknown` target.

```bash
# Build all contracts in release mode
cargo build --target wasm32-unknown-unknown --release
```

This produces `.wasm` artifacts suitable for:

- SpaceKit Compute Node  
- SpaceKit‑JS VM  
- SpaceKit deployment pipelines  

---

## **📚 Using the Standard Library in Your Project**

You can import any contract crate directly into your SpaceKit application or agent project.  
Each module is designed to be composable and safe by default.

See the [SpaceKit documentation](https://spacekit.xyz/docs) for import syntax, deployment, and usage examples.

---

## **🛡 License**

This project is licensed under **Apache 2.0**.  
See the `LICENSE` file for details.

---

## **🚀 About SpaceKit**

SpaceKit.xyz is a decentralized compute platform and developer OS for building smart contracts, agents, and distributed applications using Rust, WASM, and the SpaceKit Contract Language.
