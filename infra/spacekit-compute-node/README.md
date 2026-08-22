# SpaceKit Compute Node

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)

Distributed **compute** plane for the SpaceKit workspace: Wasmtime-backed WASM
execution, optional GPU features, P2P networking, payments hooks, and consensus
modules. Storage integration targets a separately maintained proprietary
service through its public API boundary.

Cross-cutting references are indexed in
[`docs/README.md`](../../docs/README.md).

### What this crate is

**One-line:** a **Rust library** (`spacekit_compute_node`) plus an optional **CLI binary** (`spacekit-compute-node`, Cargo feature **`standalone`**) that exposes HTTP control paths and ties together compute, identity configuration, and workspace crates.

**Breadth is gated by Cargo features** (`gpu`, `storage-integration`, `standalone`, etc.). Default presets pull in GPU and storage integration; slimmer builds are possible for experimentation.

**Search tip:** identifiers and paths usually use the bare token `SWTCH`; prose may add quotes (`'SWTCH'`) for emphasis—use both patterns if you need an exhaustive repo search.

**For new configuration and examples, prefer `did:spacekit:…`.** Normalize remaining legacy strings as you touch files; do not mix both DID methods in the same deployment without a deliberate migration story.

### When to use this node

- **Use it for:** WASM execution with SpaceKit host imports; experiments bridging to storage-node; P2P + payments scaffolding; PQ-heavy workspaces already committed to this monorepo.
- **Probably overkill for:** a single HTTP worker that only needs generic compute; production chains where you require a mature, externally audited execution spec you do not control here.
- **Not yet a fit (today):** turnkey regulated workloads (**no HIPAA/SOC2 certification** is implied—see **Compliance posture** below); high-stakes validator economies without an external consensus audit; workloads needing guarantees this repo does not document.

### At a glance (directional—not a benchmark)

| Concern | Bacalhau / similar decentralized batch WASM | Akash / GPU marketplace-style offload | SpaceKit compute node |
|--------|---------------------------------------------|--------------------------------------|------------------------|
| WASM + **custom host syscalls** (storage, DID, payments) | Varies by deployment | Typically not this ABI | **In-tree** (`spacekitvm`, adapters—see [`documentation/VM_PARITY.md`](documentation/VM_PARITY.md)) |
| **Optional GPU** in the same crate path as contracts | Job-dependent | Often separate from app WASM | Cargo feature **`gpu`** (wgpu); **you** validate on your hardware |
| **PQ crypto** wired through workspace primitives | Not the default framing | Uncommon | Many **KEMs** + **SPHINCS+** signing paths—enable flags and **review threat model** |
| **Tight coupling** to SpaceKit storage node | No | No | Optional **`storage-integration`** |

Other ecosystems (Render-style GPU rental, io.net-style aggregators, privacy-focused stacks) optimize different slices; the row above highlights **programmable WASM + custom hosts + optional GPU + monorepo storage**. Benchmark and integrate for *your* workload.

### Production status (honest)

| Area | State |
|------|--------|
| **WASM runtime + host functions** | Substantial code landed; **compare with [`documentation/VM_PARITY.md`](documentation/VM_PARITY.md)** vs `spacekit-js` before relying on cross-language behavior. |
| **GPU** | Feature-gated; performance is **deployment-specific**—benchmark your kernels; no audited multipliers are claimed in this README. |
| **Network consensus (PBFT + spacetime)** | **As-built:** `ConsensusCoordinator` + [`spacekit-unified-consensus`](../../consensus/spacekit-unified-consensus/README.md) via `UnifiedConsensusHost` (feature `spacetime-consensus`). **Aspirational narrative:** [`documentation/SPACEKIT_CONSENSUS_UNIFIED.md`](documentation/SPACEKIT_CONSENSUS_UNIFIED.md) (GPU committees, reputation on hot path, etc. — not all shipped). No substitute for a formal BFT audit. |
| **Payments (x402 / SpaceKit Pay / ASTRA)** | Multiple rails appear in code and docs; they are **not interchangeable**. Canonical economics: [`spacekit-tokenomics`](../../economics/spacekit-tokenomics/). Operator rewards: [`SPACEKIT_BLOCKCHAIN_REWARDS.md`](documentation/SPACEKIT_BLOCKCHAIN_REWARDS.md). |
| **Standalone HTTP API** | **`spacekit-compute-node`** serves selected routes (health, status, onboarding helper, DID helpers, payments, execute intent, etc.). Authoritative wiring: **[`documentation/BINARY_STANDALONE.md`](documentation/BINARY_STANDALONE.md)** (links through to `src/bin/standalone.rs`). |
| **Operator probes** | `GET /health`, `GET /status`, `GET /v1/node/identity` for non-secret snapshots. |

### Compliance posture

Illustrative contracts and docs may mention **HIPAA-like** access patterns or research-market metaphors. That is **not** certification. This repository does **not** ship HIPAA, SOC 2, or clinical validation. Use **HIPAA-aligned design patterns** only under your own compliance program and counsel.

### Before production deployment

Work through [`documentation/RUNBOOK.md`](documentation/RUNBOOK.md). Summary:

1. **Identity and keys** — Configure `[identity]` and SpaceKit CLI `*.hex` keys. **Signing paths in the wallet use SPHINCS+ today; the CLI ships Kyber material for KEM-aligned workflows—the two are not interchangeable.** See the runbook for how loads apply at startup.
2. **VM parity** — Rust vs JS host imports ([`documentation/VM_PARITY.md`](documentation/VM_PARITY.md)).
3. **Secrets** — Never commit live configs; restrict key files (`chmod 600`).
4. **Observability** — Logs, metrics (`production_metrics` / Prometheus where enabled).
5. **Network edge** — TLS and rate limits at reverse proxy.
6. **Supply chain** — `scripts/audit-compute-node.sh` / `cargo audit`.
7. **Load and resilience** — Soak tests, restarts (`scripts/smoke-http.sh` for HTTP smoke only).
8. **Operations** — Incident + rotation runbooks.

### Quick start (~3 minutes)

Use a local config (the repo does not require `config.toml` at root until you copy it):

```bash
git clone https://github.com/spacekit-xyz/spacekit-core.git
cd spacekit/spacekit-compute-node

cp examples/config.toml config.toml   # or start from repo config.toml; edit identity/paths
cargo build --release --features standalone

./target/release/spacekit-compute-node --config config.toml start
# In another terminal (profile default compute HTTP is 9000):

curl -s http://127.0.0.1:9000/health
```

Identity defaults expect SpaceKit CLI keys under `~/.spacekit/keys/*.hex` when using that layout (see [`config.toml`](config.toml)). **`~` in those paths is expanded on load on Unix** (see [`documentation/BINARY_STANDALONE.md`](documentation/BINARY_STANDALONE.md)).

### Network consensus (as-built)

**Not two-tier consensus.** `ConsensusCoordinator` + `ReputationWeightedConsensus`
(via `UnifiedConsensusHost`) is a complete BFT surface in count mode today.
[`spacekit-spacetime-consensus`](../../consensus/spacekit-spacetime-consensus/README.md) is an
**optional** reference extension (feature `spacetime-consensus`) that augments
PBFT — it does not replace quorum. Layering and call-site split: [`spacekit-unified-consensus/README.md`](../../consensus/spacekit-unified-consensus/README.md).

Two names matter — do not conflate them:

| Name | Role |
|------|------|
| **`UnifiedSWTCHConsensus`** (`src/consensus.rs`) | Governance proposal queue (block / metrics / hybrid proposals to L1 manifest validation). Used on `POST /v1/consensus/propose` for `submit_block_proposal`. |
| **`UnifiedConsensusHost`** (`src/unified_consensus_host.rs`) | PBFT facade over **`ConsensusCoordinator`** + spacetime extension ([`spacekit-unified-consensus`](../../consensus/spacekit-unified-consensus/README.md)). |

**Dependency direction:** application → `UnifiedConsensusHost` → `ConsensusCoordinator` (P2P, votes, finality) → `spacekit-spacetime-consensus` (rotors, PQ envelopes, fingerprints, fraud proofs).

**What runs today (feature `spacetime-consensus`, binary `standalone`):**

- P2P votes → coordinator → **`observe_vote_round`** telemetry (non-gating, `debug!`).
- PQ finalize → coordinator `check_finality` → facade **`has_consensus`** tripwire (count-mode).
- Spacetime side effects on finalize: [`src/spacetime_integration.rs`](src/spacetime_integration.rs).
- Parameter ratification / fraud proof HTTP routes on the coordinator (spacetime types).

**Post-fork:** when `FacadeConfig::use_weighted_threshold = true`, flip finalize ordering to
**host-first**; see comments in `pq_finalize_after_propose` in [`src/bin/standalone.rs`](src/bin/standalone.rs).

**Growformer-through-host (planned, in order):**

1. **Telemetry only** — observe inference like vote telemetry; no gating.
2. **Parameter ratification routing** — proposals through host + coordinator (governance path).
3. **Not by default:** per-block Growformer inside `collect_weighted_votes` (ML latency on consensus hot path).

**Failure mode (design target):** Growformer unreachable → log, continue with last-ratified thresholds; do not stall consensus.

**Still to build on this foundation:** `MLReputationEngine` as `ReputationSource`, `GET /v1/consensus/weighted-votes`, P2P fingerprint attestation gossip, post-fork weighted threshold.

Detail: [`spacekit-unified-consensus/README.md`](../../consensus/spacekit-unified-consensus/README.md).

### Architecture

Long-form: [`documentation/SPACEKIT_ARCHITECTURE.md`](documentation/SPACEKIT_ARCHITECTURE.md).

Diagram assets: the storage node carries SVG overviews in its `documentation/assets/` tree; **this crate does not yet mirror those diagrams**—prefer the linked architecture docs until SVG parity lands. That gap is tracked under **[Unreleased]** in [`CHANGELOG.md`](CHANGELOG.md) so it does not read as an un-owned footnote.

### SKCL (contracts → WASM)

SKCL is documented in **[`documentation/SKCL.md`](documentation/SKCL.md)**
(compiler location, maturity, rationale). The current contract workspace is
[`sdks/spacekit-standard-library`](../../sdks/spacekit-standard-library/).

### Cargo features

This crate’s **feature flags are smaller** than `spacekit-storage-node` (different concerns). There is **no** `aws-secrets` or `rate-limit-spacekit` here—those live on the storage crate.

| Feature | Meaning |
|---------|---------|
| `default` | `storage-integration` + `gpu` |
| `standalone` | CLI (`clap`, tracing) + TOML/YAML config + **`storage-integration`** (required for the binary preset today) |
| `gpu` | wgpu-backed GPU path |
| `storage-integration` | Depends on `spacekit-storage-node` |
| `spacetime-consensus` | [`spacekit-spacetime-consensus`](../../consensus/spacekit-spacetime-consensus/README.md) + [`spacekit-unified-consensus`](../../consensus/spacekit-unified-consensus/README.md); enables `UnifiedConsensusHost`, PQ finisher, fingerprint/finality/fraud HTTP routes (with `standalone`) |
| `growformer-inference` | Native Growformer on the Rust VM (heavier build; optional) |
| `python-compression` | **Experimental:** enables PyO3 bridge so WASM can call **Python-backed compress/decompress host functions** (`python_compress` / `python_decompress` in [`src/spacekitvm/swtchvm_node.rs`](src/spacekitvm/swtchvm_node.rs)); omit unless you are exercising that path |

```bash
cargo build --release --features standalone
cargo build --release --no-default-features --features standalone   # drops default gpu; still includes storage via standalone
```

### Binary commands

| Command | Use |
|---------|-----|
| **`start`** | Primary operator path: HTTP server + node lifecycle (see [`BINARY_STANDALONE.md`](documentation/BINARY_STANDALONE.md)). |
| **`status`** | Builds config, instantiates node, prints JSON status. |
| **`production-test`** | Runs the in-tree production testing suite harness. |
| **`register`**, **`gpu-info`**, **`test`** | **Not production-complete** at the time of writing—handlers are stubs or minimal logging. Prefer `start` / `status` / `production-test`; confirm in [`documentation/BINARY_STANDALONE.md`](documentation/BINARY_STANDALONE.md) before scripting. |

Example:

```bash
# `--port` overrides `[network].rpc_port` from config.toml when you need a one-off listener (omit it to stay config-driven).
./target/release/spacekit-compute-node --config config.toml start --port 8080
./target/release/spacekit-compute-node --config config.toml status
```

### Selected HTTP endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | Liveness |
| GET | `/status` | Node snapshot (DID, limits, CLI KEM metadata) |
| GET | `/v1/node/identity` | Same snapshot for tooling |
| GET | `/v1/onboarding/balance?did=` | **Website onboarding helper:** returns a placeholder balance JSON for reachability checks; **not** a canonical ledger balance until wired to your backend. |
| POST | `/v1/did/register` | **Development helper:** validates posted keys and returns a synthesized DID document in-process; **does not** replace an on-chain or production DID registry—wire [`SwtchvmRuntime`](src/spacekitvm/) / your registry before relying on it. |
| POST | `/v1/payments/*` | Payment rails (see `spacekit-payments`) |
| POST | `/v1/execute` | Execution intent (payments-aware) |

Full route list and handlers: [`documentation/BINARY_STANDALONE.md`](documentation/BINARY_STANDALONE.md).

### Configuration

- Examples: [`examples/config.toml`](examples/config.toml)
- Operator copy: [`config.toml`](config.toml)

Minimal identity block:

```toml
[identity]
did = "did:spacekit:compute:your-node"
quantum_algorithm = "Kyber1024"   # align with CLI keygen; sizes inferred from .hex if mismatched
private_key_path = "~/.spacekit/keys/private_key.hex"
public_key_path = "~/.spacekit/keys/public_key.hex"
```

### Testing

```bash
cargo test -p spacekit-compute-node
cargo test -p spacekit-compute-node --features standalone --bin spacekit-compute-node
cargo test -p spacekit-compute-node --test wasm_contracts
```

Test counts **vary by features and workspace state**. Tests are **necessary, not sufficient**, for production—especially across WASM, GPU, P2P, and payments together.

### Performance posture

Hot paths favor **native Rust + Wasmtime JIT** for contract execution, with **optional GPU** for data-parallel kernels you explicitly route—**architectural**, not a proof of speedup vs every other stack. **Do not treat old README multiplier tables as validated benchmarks.** If you publish numbers, ship a **reproducible harness** (hardware, commit SHA, commands, dataset).

### Security and cryptography

PQ algorithms and cipher choices surface through `spacekit-primitives`, `spacekit-did`, and related crates—see [`documentation/SPACEKIT_SECURITY_GUIDE.md`](documentation/SPACEKIT_SECURITY_GUIDE.md). **“Quantum-resistant” describes mechanisms, not a universal proof** that every code path and deployment is secure.

### Troubleshooting

- **Config errors:** validate TOML manually; watch startup logs (`RUST_LOG=debug`).
- **Keys:** confirm both `.hex` files exist or omit them intentionally; see runbook.
- **HTTP:** `curl -s http://127.0.0.1:<rpc_port>/health` (port from `config.toml` `[network].rpc_port`).

### Illustrative vignettes (moved)

Older narrative Rust fragments (non-compiling “use case” sketches) live in **[`documentation/USE_CASES.md`](documentation/USE_CASES.md)** so this README stays factual.

### Contributing

Match the tone of sibling crates: **prefer precise claims**, cite code paths, and extend tests for behavior you rely on.

### License

This crate is licensed under **[GNU AGPL-3.0 or later](LICENSE)**. That applies to the **`spacekit_compute_node`** library, the **`spacekit-compute-node`** binary (`standalone` feature), and examples shipped in this crate. Running a modified version as a **network service** generally requires you to **offer corresponding source** to users who interact with it—read the license and obtain counsel for your deployment.

This posture is intended to keep execution-layer infrastructure **open to inspection and improvement**, similar in spirit to strong-copyleft Ethereum execution clients, while recognizing that AGPL’s triggers differ from LGPL’s.

**Other crates in the monorepo are not automatically AGPL** (the included
storage node retains proprietary terms; SDKs and compilers are often
permissive).
Workspace overview: **[`LICENSING.md`](../../docs/LICENSING.md)**.

For **commercial terms** (e.g. deployments where you cannot satisfy AGPL obligations), contact **[SpaceKit](https://spacekit.xyz)**.

### Links

- **SpaceKit:** [spacekit.xyz](https://spacekit.xyz)
- **Monorepo:** https://github.com/spacekit-xyz/spacekit-core

### Whitepaper

Canonical spec (SpaceKit naming):
**[`SpaceKit-Whitepaper.md`](../../docs/spacekit-whitepaper/SpaceKit-Whitepaper.md)**.
Older SWTCH-branded material remains under the same historical documentation
directory. **Not** an audited security or compliance certificate—see also
[`CHANGELOG.md`](CHANGELOG.md).

### Acknowledgments

NIST PQ programs, Wasmtime, Rust crypto communities, and contributors across SpaceKit repos.

### What to read next

| If you want to… | Start here |
|-----------------|------------|
| Deploy or harden for production | [`documentation/RUNBOOK.md`](documentation/RUNBOOK.md) |
| Write or build contracts / SKCL | [`documentation/SKCL.md`](documentation/SKCL.md) · [`sdks/spacekit-standard-library`](../../sdks/spacekit-standard-library/) |
| Understand **as-built** network consensus | [`spacekit-unified-consensus/README.md`](../../consensus/spacekit-unified-consensus/README.md) · [`spacekit-spacetime-consensus/README.md`](../../consensus/spacekit-spacetime-consensus/README.md) |
| Aspirational / whitepaper-style consensus doc | [`documentation/SPACEKIT_CONSENSUS_UNIFIED.md`](documentation/SPACEKIT_CONSENSUS_UNIFIED.md) (not the implementation map) |
| Align Rust VM with `spacekit-js` | [`documentation/VM_PARITY.md`](documentation/VM_PARITY.md) |
| Browse all deep-dives | [`documentation/README.md`](documentation/README.md) |
| See shipped milestones and moves | [`CHANGELOG.md`](CHANGELOG.md) |

---

Built for post-quantum decentralized compute (product: **SpaceKit Compute Node**, binary **`spacekit-compute-node`**, library crate **`spacekit_compute_node`**).

**Canonical product entry:** [spacekit.xyz](https://spacekit.xyz).
