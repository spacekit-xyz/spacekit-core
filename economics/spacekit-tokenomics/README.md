# Spacekit Tokenomics

Canonical home for SpaceKit economic specifications: **ASTRA** emission, **AstraRewards**, **Service Reward Accumulator (SRA)**, **SpaceKit Pay**, and **x402**.

**Source of truth:** canonical v2 (May 2026). 

| Document | Purpose |
|----------|---------|
| [`SpaceKit_Tokenomics.md`](./SpaceKit_Tokenomics.md) | v2 spec — ASTRA, SpaceKit Pay, x402 |
| [`ASTRA.md`](./ASTRA.md) | Public ASTRA overview |
| [`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md) | Halving curve, treasury (350M), bootstrap, category shares |
| [`ASTRA_REWARDS_CONTRACT_SPEC.md`](./ASTRA_REWARDS_CONTRACT_SPEC.md) | On-chain balance ledger + 2B cap |
| [`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](./SERVICE_REWARD_ACCUMULATOR_SPEC.md) | Protocol SRA → CREDIT pipeline |

---

## ASTRA at a glance

| Parameter | Value |
|-----------|-------|
| Hard cap | **2,000,000,000 ASTRA** |
| Genesis treasury | **350,000,000** (17.5%, minted at INIT) |
| Year-1 operator emission | **200,000,000** (halving every 4 years) |
| Asymptotic operator emission | **~1.15B** |
| Category split (default) | Consensus **40%**, compute **30%**, storage **20%**, messaging **10%** |
| Public sale | **None** |

---

## Three economic primitives

| Primitive | Emits ASTRA? |
|-----------|--------------|
| **ASTRA** (SRA + AstraRewards) | Yes — operator service only |
| **SpaceKit Pay** | No |
| **x402** | No |

---

## Code

| Artifact | Location |
|----------|----------|
| `ASTRA_MAX_SUPPLY_WEI`, `ASTRA_GENESIS_TREASURY_WEI`, … | `spacekit-primitives::v1::sdk::token` |
| **AstraRewards** WASM contract | `spacekit-standard-library/rewards/astra-rewards` |
| **SRA** (target) | `spacekit-compute-node` block execution |

---

## Website

| Page | URL |
|------|-----|
| Economics overview | [`/economics`](https://spacekit.xyz/economics) |
| ASTRA docs | [`/docs`](https://spacekit.xyz/docs) → Tokens & payments |

---

## Versioning

- **v2.0** — current (no aUSD; 2B cap; halving emission; AstraRewards + SRA).
- **v1.0** — archived under [`archive/`](./archive/).
- Internal memos and deck revision drafts — archived under [`archive/`](./archive/).
