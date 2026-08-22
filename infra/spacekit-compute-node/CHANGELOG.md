# Changelog

All notable changes and delivery milestones for **spacekit-compute-node** are tracked here. Phase narratives and historical README status sections were moved from the root README into this file.

The format is loosely chronological by engineering phase; append dated entries under `[Unreleased]` or version headings as releases are tagged.

## [Unreleased]

- Rust VM: **`spacekit_llm`** imports are **not registered** (deprecated on compute-node); use Growformer / `spacekit_agent` or `spacekit-js` for legacy LLM-linked WASM.
- Rust VM: **`spacekit_contract.contract_call`** — nested contract execution (max depth 8), `SwtchvmStoreData.executing_contract` + shared depth counter; SDK **`contract_call_raw`** / **`prelude::env::call`** / **`prelude::env::call_with_output`**; integration tests **`tests/contract_call_nested.rs`** (WAT), **`tests/stdlib_wasm_compute_vm.rs`** (stdlib release WASMs e.g. `routekit_agent.wasm`).
- VM / host parity (Rust): `env.abort` stub; `env.log_output` alias + memory-backed `env.log`; `metering.usegas`. JS: `env.log` alias for `log_output`. See [`documentation/VM_PARITY.md`](documentation/VM_PARITY.md) checklist for remaining gaps.
- **`spacekit-contract-sdk` agent imports on compute node:** `spacekit_agent` (Growformer symbols; inference stubs `-1`, brain load via storage node when enabled), `spacekit_messaging` / `spacekit_payments` / `spacekit_remote_storage` / `spacekit_tools` (`-1` until wired) — enables **`routekit-agent`** WASM to **instantiate**; full behaviour still requires JS VM or further Rust integration.
- **License:** `spacekit-compute-node` is **AGPL-3.0-or-later** (replacing manifest `MIT` + prior proprietary `LICENSE` text). Workspace overview: [`LICENSING.md`](../LICENSING.md).
- **TODO:** SVG architecture diagrams for `spacekit-compute-node` (`documentation/assets/`), parity with `spacekit-storage-node` layout—text fallback remains in README until shipped.
- Documentation: [`documentation/RUNBOOK.md`](documentation/RUNBOOK.md) — ASTRA / SwtchVM faucet, balance checks, TypeScript flow, and distinction vs `spacekit-js` + standalone HTTP.
- Documentation: [`documentation/BINARY_STANDALONE.md`](documentation/BINARY_STANDALONE.md) consolidates standalone-binary wiring; root README links there instead of repeating `standalone.rs`.
- Whitepaper links in README / archived checklist prefer **`spacekit-whitepaper/SpaceKit-Whitepaper.md`**; **`SWTCH-Whitepaper.md`** remains as legacy revision with a header pointer to the canonical doc.
- Documentation: phase milestones and whitepaper checklist moved from `README.md` to this file.
- Deployment focus: see **Roadmap: production deployment** in `README.md` for the live checklist toward production.
- Standalone node: load CLI Kyber `.hex` keys + apply configured DID; ops endpoints `/status` and `/v1/node/identity`; scripts `scripts/audit-compute-node.sh`, `scripts/smoke-http.sh`; runbook and VM parity notes under `documentation/`.
- `spacekit-did`: `QuantumResistantWallet::apply_config_did`.
- Rust VM: register `env.get_timestamp` for parity with `spacekit-js`.
- README + `documentation/README.md`: rewritten for sober production gating, SpaceKit naming guidance, and removal of unsubstantiated “first” / benchmark claims; illustrative vignettes moved to `documentation/USE_CASES.md`; SKCL spin-out `documentation/SKCL.md`.
- README polish: identity Kyber vs SPHINCS+ sentence; CLI/HTTP stubs clarified; Cargo features reconciled with `Cargo.toml` + note vs storage-node features; quick start with `cp examples/config.toml`, health curl, and tilde-path wording; comparison rows (Bacalhau/Akash-style); performance architectural sentence; direct whitepaper link; intent-based “what to read next”; monorepo URL without hedge.

---

## Historical implementation milestones (from prior README)

### Implementation snapshot

**Feature areas previously summarized as implemented:**

- Real WASM execution with gas metering and deterministic execution
- Quantum-resistant security (post-quantum algorithms, SPHINCS+ signatures)
- GPU execution engine (WebGPU, CUDA, OpenCL) with hybrid CPU/GPU paths
- Resource monitoring and cost calculation
- Network integration (P2P, service discovery, reputation)
- Storage-oriented contracts and cross-node communication
- Unified consensus layer (overhead/cost claims were roadmap metrics in docs)
- Library tests (counts vary by crate; run `cargo test` for current numbers)

### Phase 3 — Storage integration

- Collaborative storage (multi-party ownership, threshold cryptography, approvals)
- Specialized contracts (domain-specific storage narratives)
- Storage primitives (ABI / access control)
- Cross-node communication (discovery, health, load balancing)

### Phase 5.5 — Unified consensus

- UnifiedSWTCHConsensus engine (block + metrics validation narrative)
- Validator committees (block / metrics / hybrid specialization)
- Unified voting for multiple proposal types with BFT considerations
- Consensus migration manager (dual → parallel → unified rollout narrative)
- Economic optimization tracking (documented targets)

### Phase 6 — Identity, KeyMaster, multi-node coordination

#### KeyMaster Service (`keymaster.rs`)

Escrow-oriented management of storage node Kyber1024 secret keys:

- Register/recover on boot; encrypted escrow on compute node
- Key rotation: `POST /v1/keymaster/rotate`
- AES-256-GCM persistence keyed from compute node identity
- API: `POST /v1/keymaster/register`, `POST /v1/keymaster/rotate`

#### Verkle state anchoring (`state_anchor.rs`)

- `QuantumTree`-based roots over DID documents; EVM calldata generation
- API: `POST /v1/state/anchor`

#### SharedVault contract (`spacekit-shared-vault`)

- WASM vault opcodes for multi-sig style file access control

#### Rollup bundle hash

- `quantum_state_roots` alignment between Rust bundle hashing and JS sequencer serialization

#### Storage node key lifecycle

- Encrypted local keys, rotation API, envelope rewrap behavior

#### DID-authenticated artifact access

- Requester DID forwarding and registry resolution narratives

#### Multi-node bootstrap & consensus (sub-phases 1–4)

1. **Config wiring** — `--bootstrap`, `--p2p-port`; full `NetworkConfig` in standalone
2. **State snapshot** — `GET /v1/state/snapshot`
3. **Block propagation** — length-prefixed JSON over TCP; `P2PMessage` enum
4. **Consensus finality** — `ConsensusCoordinator`, validator registry, `ConsensusVote`, APIs under `/v1/consensus/`

### Phase 7 — Unified payment system

#### `spacekit-payments` crate

- Payment config, fee routing, x402 (402 Payment Required), aUSD vault, warp middleware

#### Compute node payment endpoints

- `GET /v1/payments/config`, `POST /v1/payments/verify`, aUSD charge/credit/balance routes

#### Contract SDK payment host functions

- `env` imports: `get_balance`, `transfer`, `get_timestamp`, etc., with JS VM updates in `spacekit-js` (`host.ts`) for parity work

#### Payment flow (high level)

```
Client                     Compute Node                 On-Chain
  │                            │                            │
  │─── GET /v1/execute ───────▶│                            │
  │◀── 402 + X402Response ────│                            │
  │                            │                            │
  │─── GET + X-PAYMENT ──────▶│                            │
  │                            │─── verify via facilitator ▶│
  │                            │◀── receipt + tx_hash ──────│
  │                            │                            │
  │                            │ FeeRouter:                 │
  │                            │  amount → net + fee        │
  │                            │  net → ASTRA credit        │
  │                            │  fee → treasury credit     │
  │                            │                            │
  │◀── 200 + result ──────────│                            │
```

### Archived test listing (illustrative)

Prior README included a sample `cargo test` listing (~63 tests). **Do not treat this as a frozen guarantee** — always run:

```bash
cargo test --features standalone
```

---

## SWTCH whitepaper alignment (historical checklist)

This implementation was documented as aligning with the technical narratives in the monorepo whitepapers—prefer **[SpaceKit-Whitepaper.md](../spacekit-whitepaper/SpaceKit-Whitepaper.md)**; **[SWTCH-Whitepaper.md](../spacekit-whitepaper/SWTCH-Whitepaper.md)** is a legacy SWTCH-branded revision (see banner there). Treat the bullets below as **design intent**, not an audited compliance certificate:

- Quantum-resistant foundation (PQ algorithms, DID / identity narrative)
- Distributed confidence recovery / VPoS / economic participation themes
- Platform architecture (WASM runtime, P2P, DID registry, application layer)
- Token economics (staking, fees, incentives — verify against live deployments)
- Developer experience (CLI, SDKs, REST/WebSocket, container examples)

---

## Prior “production status” summary (archived marketing copy)

Earlier README text asserted broad “production-ready” status, GPU/storage/consensus feature completeness, and line-count metrics. **Production suitability depends on your environment**, verification (tests, audits, load), and parity with other runtimes (e.g. `spacekit-js` VM). Use the README deployment checklist and operational discipline instead of this archived summary alone.
