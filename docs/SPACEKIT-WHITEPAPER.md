# SpaceKit Whitepaper

> **Noncanonical draft.** The current technical narrative is
> [`spacekit-whitepaper/SpaceKit-Whitepaper.md`](spacekit-whitepaper/SpaceKit-Whitepaper.md).
> This shorter draft is retained pending editorial consolidation and must not
> override executable behavior or the sources indexed by [`README.md`](README.md).

**Version:** 1.0 (draft)  
**Owner:** SWTCH Labs LLC  
**Status:** Testnet deployed; full-system security audit in progress  
**Stack:** Rust (nodes, CLI, contracts) and TypeScript (`spacekit-js`, `@spacekit/sdk`)

Canonical economics: [`../economics/spacekit-tokenomics/`](../economics/spacekit-tokenomics/).
Consensus status: [`../infra/spacekit-compute-node/documentation/SPACEKIT_CONSENSUS_UNIFIED.md`](../infra/spacekit-compute-node/documentation/SPACEKIT_CONSENSUS_UNIFIED.md).
Host parity: [`../infra/spacekit-compute-node/documentation/VM_PARITY.md`](../infra/spacekit-compute-node/documentation/VM_PARITY.md).

---

## Abstract

SpaceKit is a post-quantum-aware blockchain and decentralized infrastructure platform where WebAssembly smart contracts invoke **narrow AI inference**, durable storage, encrypted messaging, and multi-rail payments through **policy-gated host modules**. Layer 1 validators run **SwtchVM** (`spacekit-compute-node`, Wasmtime); Layer 2 clients run the same contracts in browsers and Node.js via **SpaceKit-JS**, with simulate / relay / execute modes and L1 finalization.

Operators earn **ASTRA** (2 billion hard cap) for measured consensus, compute, storage, and messaging service. **Growformer** — SWTCH Labs’ narrow-AI substrate — is integrated across the CLI, JS VM, and compute node so agents train, deploy, and infer inside the same stack that settles on-chain.

The **testnet is live today**. Production mainnet depends on completing a **full-system security audit** (consensus, VM host boundary, identity, storage ACL, payments, and operator APIs). This document describes the architecture as built; where Rust and JS VMs differ, we say so explicitly.

---

## 1. System map

SpaceKit is not only a chain. It is a **network of services** tied together by DIDs and WASM execution policy:

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Developers & users                                                      │
│  spacekit-cli · browsers · Node/Bun (@spacekit/spacekit-js + sdk)       │
└───────────────────────────────┬─────────────────────────────────────────┘
                                │
        ┌───────────────────────┼───────────────────────┐
        ▼                       ▼                       ▼
┌───────────────┐     ┌─────────────────┐     ┌─────────────────┐
│ Compute node  │     │ Storage node    │     │ Messaging node  │
│ SwtchVM L1    │     │ Facts, CAS,     │     │ P2P DMs/groups  │
│ consensus     │     │ workspaces, MCP │     │ file sharing    │
└───────┬───────┘     └────────┬────────┘     └────────┬────────┘
        │                      │                       │
        └──────────────────────┼───────────────────────┘
                               ▼
                    ┌─────────────────────┐
                    │ spacekit-did        │
                    │ SPHINCS+ · Kyber KEM│
                    │ registry (WIP)      │
                    └─────────────────────┘
```

**Settlement:** ASTRA balances and **AstraRewards** credits on L1; optional **x402** (USDC) and **SpaceKit Pay** rails convert verified receipts into VM credits without minting ASTRA ([`spacekit-payments`](spacekit-payments/)).

**Content plane:** **FactPackage** and **AppPackage** artifacts live on the storage node (content-addressed graphs, PQ envelopes, deployment receipts). Contracts reference facts by hash; the host enforces policy before reads and writes.

Current documentation authority and service references are indexed in
[`README.md`](README.md). The older Universe architecture is historical.

---

## 2. Identity (SpaceKit DID)

Identity is **layered**, not a single crate:

| Step | Component | Behavior |
|------|-----------|----------|
| Bootstrap | `spacekit init` | `~/.spacekit/`, Kyber KEM keys, placeholder `did:spacekit:user:{uuid}` |
| Testnet DID | `spacekit did create` | SPHINCS+ wallet, `did:spacekit:testnet:0x…`, optional `POST /v1/did/register` on compute node |
| Library | `spacekit-did` | `QuantumResistantWallet`, VC helpers, registry traits |
| Persistence | `spacekit-did-registry` (WASM, WIP) | REGISTER / RESOLVE / ROTATE on SwtchVM storage |
| Bridge (experimental) | `spacekit-did-onchain` | EVM/Solana reference — not the production registry |

**Important:** `did:spacekit:*` is a **custom method**, not W3C DID Core compliance. **Kyber material** from `init` and **SPHINCS+ signing keys** from the wallet are related but **not interchangeable** — operators must follow the runbook for which key signs which operation.

The host blocks contract writes to `did:document:` (and `native:`, `genesis:`) storage prefixes so on-chain code cannot overwrite identity documents.

Detail: [`spacekit-did/ARCHITECTURE.md`](spacekit-did/ARCHITECTURE.md).

---

## 3. SpaceKitVM — host modules, SKTCS, contracts

### 3.1 Isometric execution

The same `wasm32-unknown-unknown` artifact is intended to run on:

- **L1:** `spacekit-compute-node` / SwtchVM (Wasmtime, authoritative state)
- **L2:** `spacekit-js` (WebAssembly in browser, Node, or Bun)

**Parity is incremental.** Before production, validate each contract on both runtimes using [`VM_PARITY.md`](spacekit-compute-node/documentation/VM_PARITY.md). Today, L2 is ahead for agent, messaging, payments, remote storage, session, and several token helpers; L1 registers symbols but may return stubs until wired.

### 3.2 Core host modules (policy surface)

Contracts interact with the outside world only through **host imports**. Nine modules form the **core policy surface** (each gated by **SKTCS** manifests — see below):

| Module | Role |
|--------|------|
| `env` | Caller DID, timestamps, logging, events, base storage, balance, transfer |
| `spacekit_agent` | Growformer brain load, generation, converse, codegen |
| `spacekit_storage` | Contract-scoped KV |
| `spacekit_contract` | Nested contract calls (max depth 8) |
| `spacekit_messaging` | Operator-fulfilled messaging (no outbound HTTP from WASM) |
| `spacekit_payments` | ASTRA transfers, vault charges |
| `spacekit_remote_storage` | Durable reads/writes on storage node |
| `spacekit_session` | Delegated session keys for agents |
| `spacekit_crypto` | Hashes, PQ verify hooks |

**Extended imports** (same VM family, additional manifests): `sk_erc20`, `sk_erc721`, `spacekit_reputation`, `spacekit_fact`, `spacekit_tools` (effect-queue web search), `spacekit_paymaster`, compression helpers, and legacy LLM paths (deprecated). Application contracts declare only what they need; the VM rejects undeclared imports.

### 3.3 SKTCS (SpaceKit Tool-Call Spec)

**SKTCS** replaces ad-hoc tool wiring with a **VM-internal manifest** (`tool-manifest.json` or WASM custom section `spacekit:tools`). Principles:

1. **Contracts propose, the VM decides** — tool calls are effects; the host validates parameters and capability constraints.
2. **Pay-before-execute** — vault charges settle before storage, network, or inference effects run.
3. **Deterministic audit trail** — each fulfilled effect is traceable for verification (Verkle witnesses on L2).

Spec: [`SPACEKIT-TOOL-CALL-SPEC.md`](SPACEKIT-TOOL-CALL-SPEC.md).

### 3.4 Contract toolchain

- **`spacekit-contract-sdk`** — `no_std` SDK, `spacekit_contract!` macro, Growformer and payment helpers
- **SKCL** — `spacekit-contract-lang` compiles `.scl` to Rust + WASM
- **`spacekit-standard-library`** — OpenZeppelin-style contracts (tokens, agents, DID registry, paymaster, app-store, RouteKit agent)

Contracts **must not** open arbitrary HTTP from WASM; external APIs are fulfilled by **messaging node operators** or storage/compute services under policy.

---

## 4. Layer 1 — spacekit-compute-node

Validators and standalone operators run the compute node: **SwtchVM** execution, optional **GPU** (`wgpu`), optional **storage-integration** (pin WASM, remote brain load), and **consensus** when enabled.

### 4.1 Execution and HTTP API

- Authoritative blocks, receipts, Merkle / **Verkle** state roots (contract KV under `kv:` namespace)
- Standalone binary: health, status, DID registration, execute intent, payments hooks, consensus routes — see [`documentation/BINARY_STANDALONE.md`](spacekit-compute-node/documentation/BINARY_STANDALONE.md)
- **Growformer inference** optional via Cargo feature `growformer-inference` (native `growformer` runtime linked to `spacekit_agent` imports)

### 4.2 Consensus (as-built)

SpaceKit uses **one BFT backbone**, not two competing consensus tiers:

| Piece | Crate / type | Role |
|-------|----------------|------|
| PBFT coordinator | `ConsensusCoordinator` in compute-node | Votes, quorum, finality |
| Facade | `spacekit-unified-consensus` / `UnifiedConsensusHost` | Reputation-weighted API; count-mode on testnet |
| Spacetime extension | `spacekit-spacetime-consensus` (feature `spacetime-consensus`) | Cl(1,3) rotors, fingerprints, tiered finality, fraud proofs — **augments** PBFT |

**Weighted reputation thresholds** are implemented but **not authoritative** until a governance hard fork sets `use_weighted_threshold`. **Growformer on the consensus hot path** is explicitly **not** the default: planned uses are telemetry and parameter ratification, not per-block inference gating.

Aspirational design (GPU committees, cross-chain finality): [`SPACEKIT_CONSENSUS_UNIFIED.md`](spacekit-compute-node/documentation/SPACEKIT_CONSENSUS_UNIFIED.md) — distinguish narrative from shipped code when reading audits.

### 4.3 Compliance posture

HIPAA-like patterns may appear in examples; **this repository does not certify HIPAA, SOC 2, or clinical use**. Operators run compliance programs independently.

---

## 5. Layer 2 — spacekit-js

SpaceKit-JS is the **L2 VM and dApp SDK**: JSON-RPC, EIP-1193 compatibility, IndexedDB state, rollup bundles, quantum-Verkle witnesses, and full host adapters including **Growformer**.

### 5.1 Execution modes

| Mode | Authority | Description |
|------|-----------|-------------|
| **Simulate** | None (read-only) | Copy-on-write; no side effects — `eth_call` equivalent |
| **Relay** | L1 validators | Simulate locally, submit signed intent, poll canonical receipt |
| **Execute** | Local VM | State-mutating local execution (dev / testing) |

If L1 diverges from local simulation, clients emit **`receipt:diverged`** so dApps can reconcile.

### 5.2 State sync

L2 pulls canonical KV via `HeaderSyncClient.syncStateSnapshot()`, verified against the header state root, with on-demand `pullStorageValue()`.

### 5.3 @spacekit/sdk

React provider, wallet, explorer hooks, token adapters (ERC-20 / ERC-721 patterns on the VM), and Kyber helpers — [`spacekit-sdk/README.md`](spacekit-sdk/README.md).

---

## 6. Storage node

`spacekit-storage-node` is the **durable artifact and agent data plane**:

- Custom ACID engine, **multi-model Serializable** transactions, idempotency keys, **TTL sandboxes**, SSE change feeds
- **PQ envelopes** (KEM + AES-GCM), CAS blobs, **FactPackage** dependency graphs, NFT collections, Git-style repos
- **Workspaces** with federation export/import and DID-signed migration v2
- HTTP API + optional **libp2p** mesh; hybrid → strict auth rollout

WASM contracts reach storage through **`spacekit_remote_storage`** on L2 (full adapters) and storage-integration on L1 (incremental).

---

## 7. Messaging node

`spacekit-messaging-node` provides **quantum-resistant P2P messaging** (embeddable library or standalone node):

- Direct and group chats, encrypted attachments, DID-based directory
- Multiple **post-quantum KEM** options and **SPHINCS+** identity signatures
- **No WASM HTTP egress** — contracts enqueue messaging effects; operators deliver

Messaging earns **10%** of annual operator emission (default split). See [`spacekit-messaging-node/TOKENOMICS.md`](spacekit-messaging-node/TOKENOMICS.md).

---

## 8. Agent factory — Growformer substrate

There is no separate “agent factory” binary. **Agent factory** denotes the **integrated Growformer substrate** — narrow AI services owned and developed by **SWTCH Labs LLC** alongside SpaceKit — wired through train → package → deploy → infer.

### 8.1 Corporate and technical relationship

**Growformer** and **SpaceKit** are both SWTCH Labs LLC technologies. Growformer provides **narrow AI** (domain brains, `.bin` artifacts, training and inference CLIs). SpaceKit integrates Growformer so smart contracts and operators can call inference **natively** via `spacekit_agent` under SKTCS policy, rather than bolting on opaque external APIs.

### 8.2 Runtime integrations

| Runtime | Integration |
|---------|-------------|
| **spacekit-cli** | Embedded Growformer (`spacekit agent` — train, merge, infer, exec); `GrowformerModelManager`; content/licensing hooks for entitled apps; deploy pipelines that pin brains to storage |
| **spacekit-js** | `src/growformer/runtime.ts` — full inference for browser/Node agents; parity tests against reference brains |
| **spacekit-compute-node** | Optional `growformer-inference` feature — native inference on L1; without it, agent imports may stub until parity work completes |
| **Contracts** | `spacekit-standard-library` agents (`spacekit-growformer-agent`, domain variants, `routekit-agent`) compiled to WASM |
| **Consensus (optional)** | `spacekit-spacetime-consensus` `growformer-hook` — parameter ratification telemetry, not default block gating |

### 8.3 Lifecycle

1. **Train / merge** brains locally (`spacekit agent train`, Growformer CLI flags).
2. **Deploy** WASM + brain artifacts via CLI storage deploy → CAS + deployment receipt on storage node.
3. **Execute** on L2 for UX (instant simulate) or **relay** to L1 for authoritative settlement.
4. **Fulfill** tool effects (search, messaging, remote storage) per SKTCS manifest and operator policy.

Agent contracts that need production inference on L1 should be validated against [`VM_PARITY.md`](spacekit-compute-node/documentation/VM_PARITY.md) after enabling `growformer-inference`.

---

## 9. Payments and economics

### 9.1 Three economic primitives

| Primitive | Mints ASTRA? | Purpose |
|-----------|--------------|---------|
| **ASTRA** | Yes (capped) | Gas, staking, governance, operator rewards via SRA → AstraRewards |
| **SpaceKit Pay** | No | Non-custodial stablecoin routing for AI/service settlement |
| **x402** | No | HTTP 402 USDC on Base; facilitator verification → FeeRouter credits |

Implementation: [`spacekit-payments`](spacekit-payments/) (`FeeRouter`, `AusdVault` for dev vault charges, x402 middleware). **Rails are not interchangeable** — each has distinct verification and treasury paths.

User-facing stablecoin charges may use vault semantics in dev; canonical public
spec is v2 in
[`../economics/spacekit-tokenomics/SpaceKit_Tokenomics.md`](../economics/spacekit-tokenomics/SpaceKit_Tokenomics.md)
(no aUSD product).

### 9.2 ASTRA emission (canonical)

Canonical reference:
[`../economics/spacekit-tokenomics/ASTRA_EMISSION.md`](../economics/spacekit-tokenomics/ASTRA_EMISSION.md).
Operator emission has three protocol-level properties:

1. **Hard cap** — total ever-emitted ASTRA ≤ **2,000,000,000** (not governable).
2. **Decay over time** — annual mint rate falls on a **4-year halving** curve toward an asymptotic operator total.
3. **Per-category rates** — consensus, compute, storage, and messaging share each year’s budget by measured contribution.

| Parameter | Value |
|-----------|-------|
| **Hard cap** | **2,000,000,000 ASTRA** |
| **Decimals** | 18 (atomic unit: 1 wei-ASTRA = 10⁻¹⁸ ASTRA) |
| **Inflation / auto-burn** | None |
| **Genesis treasury** | **350,000,000 ASTRA (17.5%)** — minted at INIT; **not** on the halving curve |
| **Bootstrap stake pool** | **50,000,000 ASTRA (2.5%)** — drawn from treasury at genesis; one-time; vesting (e.g. 4-year linear) |
| **Year-1 operator emission** | **200,000,000 ASTRA (10% of cap)** |
| **Halving period** | **4 years** (governance may adjust between 2–8 years with super-majority + 30-day notice) |
| **Public sale** | **None** |

Mint authority is **protocol-only**, capped at total supply. The schedule is set at genesis and adjusted only where governance allows (see §9.10); the **2B cap is not**.

### 9.3 Decay curve

Operator emission follows a **Bitcoin-style halving curve** adapted for **continuous service** (not per-block rewards):

- **Annual emission at year *t*:** `E(t) = 200,000,000 × 0.5^(t/4)` ASTRA
- **Continuous form:** `emission_rate(t) = initial_rate × exp(-t × ln(2) / 4)`
- **Asymptotic operator total:** `initial × 4 / ln(2) ≈ 1.154 × 200M ≈ **1,154,000,000 ASTRA**`

By year 100, annual emission is negligible (~23 ASTRA/year in the projection table); cumulative operator emission approaches **~1.15B**, not the full 2B cap.

### 9.4 Year-by-year projection

| Year | Annual emission (ASTRA) | Cumulative emitted (ASTRA) | % of cap used |
|------|-------------------------|----------------------------|---------------|
| 1 | 200,000,000 | 200,000,000 | 10.0% |
| 2 | 168,179,283 | 368,179,283 | 18.4% |
| 4 | 100,000,000 | 631,775,138 | 31.6% |
| 8 | 50,000,000 | 884,962,720 | 44.2% |
| 12 | 25,000,000 | 1,011,553,851 | 50.6% |
| 16 | 12,500,000 | 1,074,830,418 | 53.7% |
| 20 | 6,250,000 | 1,106,477,704 | 55.3% |
| 30 | 1,562,500 | 1,135,635,772 | 56.8% |
| 40 | 390,625 | 1,144,776,290 | 57.2% |
| 50 | 97,656 | 1,147,609,961 | 57.4% |
| 100 | 23.4 | 1,148,000,000+ | ~57.4% |

*Cumulative column reflects operator emission only; treasury INIT mint is additional (§9.8).*

### 9.5 Per-category allocation (default)

| Category | Share | Rationale | Year-1 ASTRA |
|----------|-------|-----------|----------------|
| Consensus validation | 40% | Highest network-protective value | 80,000,000 |
| Compute service | 30% | Primary resource for contracts and dApps | 60,000,000 |
| Storage service | 20% | Durable, persistent capacity | 40,000,000 |
| Messaging service | 10% | Communication layer | 20,000,000 |

Governance may adjust shares so each category stays **between 5% and 60%** and shares sum to **100%**. Sub-weights within a category (e.g. proposals vs votes) are also governable.

### 9.6 Per-epoch and per-event rewards

Within each category, rewards are **proportional to measured resource** in the active period:

```
ASTRA_per_event = (annual_category_emission / annual_total_resource_in_category)
                × resource_consumed_by_this_event
```

**Epoch model (default: one day, 365 epochs/year):**

1. At epoch start: `epoch_emission_per_category = annual_emission_per_category / epochs_per_year`, scaled by `0.5^(t/4)` for network age *t*.
2. Events accumulate resource units during the epoch.
3. At epoch end: each event earns `(event_resource / epoch_total_resource) × epoch_emission_per_category`.

**Rules:** idle epochs **roll over** allocation; no epoch may **overshoot** its fixed budget (high activity dilutes per-event reward within the epoch).

**On-chain epoch limit (per category):**

```
epoch_emission(t, category) =
  (200_000_000 × category_share / epochs_per_year) × 0.5^(t / 4)
```

`category_share` = 0.4, 0.3, 0.2, 0.1 for consensus, compute, storage, messaging. Implemented in **fixed-point** arithmetic in the SRA for deterministic validator agreement.

**Incremental per-event accrual within an epoch:**

```
event_emission = epoch_remaining_allocation × (event_resource / epoch_remaining_resource)
```

### 9.7 Resource measurement (default)

| Category | Measured units (summary) |
|----------|---------------------------|
| **Consensus** | Accepted block proposals; votes on finalized proposals (reputation-weighted when active); block envelope signatures; validator uptime in assigned slots |
| **Compute** | Gas served on contract execution; successful calls (base unit + gas component) |
| **Storage** | Bytes-hours durable storage; successful reads/writes (writes include bytes component); on-time storage proof attestations |
| **Messaging** | Successful message deliveries; recipients served with resolved keys; group broadcast units per recipient |

Exact weights and conversion formulas are **protocol parameters** — adjustable by governance with second-order analysis (e.g. shifting share toward storage may incentivize storage-heavy behavior).

### 9.8 Treasury, bootstrap, and cap accounting

**Treasury (350M at INIT)** — multi-sig under SWTCH Labs; counts against the 2B cap; **cannot be expanded** after genesis. Used for development, audits, bug bounties, operational reserves, ecosystem grants (with legal review), and similar protocol needs.

**Bootstrap (50M)** — initial validator stake from treasury; fixed per-validator allocation to meet minimum stake; **one-time**, not refilled; vesting applies.

**Total cap accounting:**

| Allocation | ASTRA | % of 2B cap |
|------------|-------|-------------|
| Operator emission (asymptotic) | ~1,154,000,000 | ~57.7% |
| Genesis treasury (incl. bootstrap source) | 350,000,000 | 17.5% |
| **Projected total minted** | **~1,504,000,000** | **~75.2%** |
| **Protocol reserve (headroom)** | **~496,000,000** | **~24.8%** |

Reserve may only be allocated by **on-chain governance** (e.g. slower decay curve, ecosystem program with disclosure, bootstrap refill) — not unilaterally by SWTCH Labs.

### 9.9 Cap enforcement (AstraRewards)

The **AstraRewards** contract is the backstop:

- `total_emitted` tracks cumulative minted ASTRA via **CREDIT**.
- Each credit requires `total_emitted + amount ≤ 2_000_000_000 × 10^18` wei-ASTRA.
- Over-cap credits are **rejected in full** (no partial mint).
- **No code path** mints above the cap — not for admin, governance, or emergency keys.

The decay curve should make hitting the cap unlikely in practice; the contract enforces it regardless.

### 9.10 Governance over the schedule

| Adjustable | Not adjustable |
|------------|----------------|
| Per-category shares (5–60% each, sum 100%) | **2B hard cap** |
| Resource weighting within categories | **350M treasury INIT** (genesis-only) |
| Treasury spending (within 350M) | Automatic burn (none exists) |
| Bootstrap refill from reserve (proposal) | |
| Halving period (2–8 y, super-majority + 30-day notice) | |
| Initial annual rate (50M–350M, same quorum) | |

### 9.11 Implementation (monorepo)

| Component | Location |
|-----------|----------|
| Constants (`ASTRA_MAX_SUPPLY_WEI`, `ASTRA_GENESIS_TREASURY_WEI`, `ASTRA_INITIAL_ANNUAL_EMISSION_WEI`, …) | `spacekit-primitives::v1::sdk::token` |
| **AstraRewards** WASM | `spacekit-standard-library/rewards/astra-rewards` |
| **SRA** (Service Reward Accumulator) | `spacekit-service-rewards`, `spacekit-compute-node/src/service_reward_accumulator.rs` — hooks `mine_block`; enable `[compute.sra_config] enabled = true` |
| SRA → on-chain CREDIT | `SraHost` → `OP_CREDIT` via `SwtchvmRuntime::call_contract_public`; `apply_credits_onchain = false` for audit-only runs |
| Service log schema | `spacekit-log` (`EventKind::Service`) |

**Testnet note:** legacy per-node `enable_token_minting` calculators are **not**
the production model. Mainnet-aligned emission uses **SRA + AstraRewards** only
([`../economics/spacekit-tokenomics/operator-guides/README.md`](../economics/spacekit-tokenomics/operator-guides/README.md)).

### 9.12 Honest limitations

Parameters (40/30/20/10, 200M year-1, 4-year halving, 350M treasury) are **calibrated for testnet**, not yet validated under sustained mainnet load — governance may tune within bounds after observation.

- **Service growth** affects per-operator economics: slow growth keeps per-event ASTRA higher early; fast growth dilutes per-event rewards while the schedule still converges to the asymptote.
- **Treasury is finite** — long-term protocol funding may require governance to use reserve headroom or external development funding, analogous to declining block rewards in other networks.
- **Category-share changes** can be gamed — proposals need second-order analysis before shifting incentives.

### 9.13 How users pay vs how operators earn

- **Users** spend ASTRA (gas), x402 USDC, or Pay-routed stablecoins for services.
- **Operators earn** newly emitted ASTRA only through **measured service** in the four categories — not through passive holding. Validators **stake** for Sybil resistance and slashing exposure; stake does **not** pay yield by itself.

---

## 10. Consensus summary

- **Safety backbone:** PBFT in `ConsensusCoordinator` (2/3 quorum).
- **Optional spacetime layer:** geometric aggregation, behavioral fingerprints, fraud proofs — does not remove PBFT quorum checks.
- **Governance proposals:** `UnifiedSWTCHConsensus` queue for manifest and parameter proposals (distinct from vote collection facade).
- **Audit focus:** PQ signature pipelines, finality ordering (`pq_finalize_after_propose`), weighted-threshold fork behavior, and spacetime side effects on finalize.

---

## 11. MCP and operator tooling

**Model Context Protocol** servers expose storage and compute operations to external agents over **stdio JSON-RPC** (versioned tools, BLAKE3 idempotency keys).

| Surface | Location | Examples |
|---------|----------|----------|
| Storage MCP | `spacekit-storage-node` feature `mcp` | `tx_*`, `sandbox_*`, `workspace_*`, `graph_traverse.v1`, `upload_token_mint.v1` |
| Compute MCP | `spacekit-compute-node/src/mcp.rs` | Submit transaction, mine block, read VM state |
| Gateway | `spacekit-gateway` | Proxies multiple stdio MCP backends |

**SKTCS** governs in-VM tool policy; **MCP** governs off-node operator automation — complementary, not duplicate.

Guides: [`spacekit-storage-node/documentation/guides/mcp.md`](spacekit-storage-node/documentation/guides/mcp.md).

**spacekit-cli** remains the primary human operator interface: `network`, `storage`, `message`, `contract`, `task`, `operator`, `workspace`, `repo`, `fact` — see [`spacekit-cli/COMMANDS.md`](spacekit-cli/COMMANDS.md).

---

## 12. Security and privacy

### 12.1 Cryptography by layer (resolved matrix)

SpaceKit uses **post-quantum-aware** stacks; algorithms differ by layer by design:

| Layer | Typical algorithms | Notes |
|-------|-------------------|--------|
| CLI `init` / envelopes | Kyber (and related KEMs) | Key agreement, file envelopes |
| DID wallet / VC | SPHINCS+ (SHAKE-256-128s-simple) | Primary signing for `spacekit-did` |
| L1 transaction auth | ECDSA secp256k1 (+ optional Dilithium dual-sign) | Validator and tx pipeline default |
| L2 client tx (relay) | Ed25519 (with dev-mode flags) | Must match deployment policy before mainnet |
| Storage / messaging | PQ KEM suites + AEAD | Algorithm selectable per deployment |
| State proofs | Quantum-Verkle witnesses | Light-client verification |

“Post-quantum throughout” means **PQ options are first-class**, not that every byte on every path uses the same algorithm.

### 12.2 Host and storage boundaries

- Protected prefixes: `native:`, `did:document:`, `genesis:`
- SKTCS capability scopes: rate limits, key prefixes, allowed recipients
- Optional WASM instruction metering and gas tables
- Storage auth: hybrid → strict; DID headers; upload tokens for browser uploads

### 12.3 Testnet and audit status

The **testnet is deployed** and exercised by operators and internal dApps. **Mainnet readiness requires a full-system audit**, including:

- Consensus coordinator and spacetime extension interaction
- SwtchVM host import surface and SKTCS enforcement
- VM parity (L1 vs L2) for production agent contracts
- DID registration, resolve, and registry contract completion
- Storage ACL, federation handoff, and MCP tool authorization
- Payments FeeRouter, x402 verification, and treasury configuration
- Supply cap enforcement in AstraRewards + SRA accounting

No HIPAA, SOC 2, or financial regulatory certification is implied by this software.

---

## 13. FactPackage and AppPackage

**FactPackage** — content-addressed artifact with dependency graph, PQ metadata, and policy hooks; traversed by storage MCP `graph_traverse.v1` and referenced from contracts via `spacekit_fact`.

**AppPackage** — application bundle (WASM, brains, UI assets) deployed to storage with a deployment receipt; marketplace and Agent Hub flows index `deployment_id` → `file_id`s.

Together they separate **immutable content** from **mutable contract state** while keeping verification hash-linked.

---

## 14. Roadmap and risks

| Area | Status | Risk |
|------|--------|------|
| Testnet | **Live** | Operational bugs, config drift |
| Security audit | **In progress** | Unknown critical issues until complete |
| VM L1/L2 parity | **Incremental** | Agent contracts may behave differently on Rust VM |
| DID registry on-chain | **WIP** | Resolve/register gaps vs CLI expectations |
| Weighted consensus | **Post-fork** | Misconfiguration if enabled early |
| Emission parameters | **Calibrated** | Governance may adjust shares within bounds after observation |
| Regulatory | **Under review** | ASTRA characterized as utility; separate legal opinion sought |

---

## References

- **ASTRA emission (canonical):** [`../economics/spacekit-tokenomics/ASTRA_EMISSION.md`](../economics/spacekit-tokenomics/ASTRA_EMISSION.md)
- Tokenomics v2: [`../economics/spacekit-tokenomics/SpaceKit_Tokenomics.md`](../economics/spacekit-tokenomics/SpaceKit_Tokenomics.md)
- AstraRewards + SRA: [`ASTRA_REWARDS_CONTRACT_SPEC.md`](../economics/spacekit-tokenomics/ASTRA_REWARDS_CONTRACT_SPEC.md), [`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](../economics/spacekit-tokenomics/SERVICE_REWARD_ACCUMULATOR_SPEC.md)
- Documentation index: [`README.md`](README.md)
- Tool-call spec: [`SPACEKIT-TOOL-CALL-SPEC.md`](SPACEKIT-TOOL-CALL-SPEC.md)
- VM parity: [`spacekit-compute-node/documentation/VM_PARITY.md`](spacekit-compute-node/documentation/VM_PARITY.md)
- Identity: [`spacekit-did/ARCHITECTURE.md`](spacekit-did/ARCHITECTURE.md)

**© 2026 SWTCH Labs LLC. SpaceKit™ is a trademark of SWTCH Labs LLC.**
