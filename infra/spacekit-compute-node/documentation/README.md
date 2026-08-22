# SpaceKit Compute Node — documentation index

This folder holds **deep-dive** material for the compute crate. Treat older prose below as **technical narrative**—verify claims against code before staking or shipping regulated workloads.

**If you landed here first:** the crate’s **canonical operator surface** is the repo-root **[`README.md`](../README.md)**. It carries **production gating** (honest status table, pre-production checklist), **when to use / not yet**, **Cargo features**, **identity defaults**, **selected HTTP routes**, **testing posture**, and **links**—without the long-form narrative in this tree.

## Start here

| Doc | Purpose | Status |
|-----|---------|--------|
| **[`README.md`](../README.md)** (repo root) | Operator entry: features, production honesty, endpoints, quick start | **Stable** — prefer this for evals |
| [`SPACEKIT_PROJECT_SUMMARY.md`](SPACEKIT_PROJECT_SUMMARY.md) | High-level project summary and narrative stack map | Narrative — cross-check with root README |

## Release history & milestones

| Doc | Purpose | Status |
|-----|---------|--------|
| [`CHANGELOG.md`](../CHANGELOG.md) | Phased delivery notes, relocations (e.g. SKCL, USE_CASES), VM tweaks | **Stable** — version-oriented history |
| [`archive/`](archive/) | Superseded **SWTCH**-prefixed and milestone markdown | **Historical** — context only |

---

## Operator / production

| Doc | Purpose | Status |
|-----|---------|--------|
| [`RUNBOOK.md`](RUNBOOK.md) | Incidents, keys, audits, smoke scripts | Stable |
| [`VM_PARITY.md`](VM_PARITY.md) | Inventory of Rust VM `env` imports vs `spacekit-js`; gaps called out | **Active work** — parity evolves with hosts |
| [`DEPLOYMENT_GUIDE.md`](DEPLOYMENT_GUIDE.md) | Deployment patterns and containers | Maintenance |
| [`DEPLOYMENT_STATUS.md`](DEPLOYMENT_STATUS.md) | Point-in-time deployment checklist snapshots | Historical snapshot |
| [`SPACEKIT_MONITORING_OPERATIONS.md`](SPACEKIT_MONITORING_OPERATIONS.md) | Prometheus / metrics operator notes | Maintenance |

---

## Architecture & runtime

| Doc | Purpose | Status |
|-----|---------|--------|
| [`BINARY_STANDALONE.md`](BINARY_STANDALONE.md) | CLI binary wiring (`standalone` feature): config load, HTTP route composition, pointers into `src/bin/standalone.rs` | Stable operator reference |
| [`SPACEKIT_ARCHITECTURE.md`](SPACEKIT_ARCHITECTURE.md) | Layered system architecture (compute, crypto, network) | Design narrative |
| [`SPACEKIT_WASM_RUNTIME.md`](SPACEKIT_WASM_RUNTIME.md) | WASM execution story, host surface, GPU hooks | Design narrative |
| [`SPACEKIT_CONSENSUS_UNIFIED.md`](SPACEKIT_CONSENSUS_UNIFIED.md) | Unified consensus committees, migration story | **Design / narrative — not a formal BFT proof**; do not size stake from this doc alone |
| [`SPACEKIT_DID_INTEGRATION.md`](SPACEKIT_DID_INTEGRATION.md) | DID + compute integration (registry, verification patterns) | Design narrative |

---

## Integrations (compute + X)

Cross-cutting stories where compute meets another subsystem or research track.

| Doc | Purpose | Status |
|-----|---------|--------|
| [`SPACEKIT_STORAGE_COMPLETE.md`](SPACEKIT_STORAGE_COMPLETE.md) | Compute ↔ storage-node integration, storage contracts on the VM | Design narrative — verify against `spacekit-storage-node` |
| [`SPACEKIT_DISTRIBUTED_ML.md`](SPACEKIT_DISTRIBUTED_ML.md) | Distributed ML / federation framing on the stack | Research narrative |
| [`TRM_INTEGRATION.md`](TRM_INTEGRATION.md) | TRM (recursive reasoning) integration notes | Experimental — verify module boundaries in code |

---

## Security & economics

| Doc | Purpose | Status |
|-----|---------|--------|
| [`SPACEKIT_SECURITY_GUIDE.md`](SPACEKIT_SECURITY_GUIDE.md) | Threat framing, PQ surfaces, operational security | Design narrative — not a certification |
| [`SPACEKIT_BLOCKCHAIN.md`](SPACEKIT_BLOCKCHAIN.md) | **Core chain / node behavior** — execution and chain-layer mechanics (not primarily tokenomics) | Design narrative |
| [`SPACEKIT_BLOCKCHAIN_REWARDS.md`](SPACEKIT_BLOCKCHAIN_REWARDS.md) | **Compute operator rewards** (implementation guide) | Canonical macro economics: [`spacekit-tokenomics`](../../../economics/spacekit-tokenomics/) |
| [`SPACEKIT_CROSS_CHAIN_TX.md`](SPACEKIT_CROSS_CHAIN_TX.md) | **Cross-chain bridges / messages** — interoperability flows (distinct from single-chain rewards) | Design narrative |

---

## Smart contract language (SKCL)

**SKCL** is the Solidity-inspired language that targets **WASM** for this workspace’s VM. It is one of the most differentiated surfaces (custom host imports, identity and payment hooks), so it gets more than a single table row:

- **Reader doc:** [`SKCL.md`](SKCL.md) — compiler location (workspace paths vary), **honest maturity**, how it relates to `spacekit-js` parity, and pointers to the monorepo contracts overview.
- **Contracts overview (monorepo):**
  [`sdks/spacekit-standard-library`](../../../sdks/spacekit-standard-library/).
- **Illustrative only:** [`USE_CASES.md`](USE_CASES.md) — **non-compiling** vignettes from an older README; strategic sketches, not SDK truth.

| Doc | Purpose | Status |
|-----|---------|--------|
| [`SKCL.md`](SKCL.md) | SKCL front door inside this repo | Maintenance |
| [`USE_CASES.md`](USE_CASES.md) | Historical pseudo-code vignettes | **Illustrative — does not compile** |

---

## Monorepo

| Resource | Purpose |
|----------|---------|
| [`sdks/spacekit-standard-library`](../../../sdks/spacekit-standard-library/) | Current contract workspace pending the contracts-domain migration |
| [`docs/README.md`](../../../docs/README.md) | Cross-cutting architecture and documentation authority |

The storage-node implementation is included under
[`infra/spacekit-storage-node`](../../spacekit-storage-node/) and retains its
package-specific proprietary license.

---

**Canonical product entry:** [spacekit.xyz](https://spacekit.xyz). **License (this crate):** [AGPL-3.0 or later](../LICENSE). **Workspace policy & heterogeneous crates:** [`LICENSING.md`](../../../docs/LICENSING.md).
