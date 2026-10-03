# Spacekit Tokenomics

Canonical home for SpaceKit economic specifications: **ASTRA** emission, native rewards (system address `0x…0003`), and the **Service Reward Accumulator (SRA)**. ASTRA is the only currency on the network. SpaceKit Pay (and its x402 rail) was retired; see [`docs/PAYMENTS_AND_CURRENCY.md`](../../docs/PAYMENTS_AND_CURRENCY.md).

**Source of truth:** canonical v2 (May 2026). 

| Document | Purpose |
|----------|---------|
| [`SpaceKit_Tokenomics.md`](./SpaceKit_Tokenomics.md) | v2 spec — ASTRA |
| [`ASTRA.md`](./ASTRA.md) | Public ASTRA overview |
| [`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md) | Halving curve, treasury (350M), bootstrap, category shares |
| [`ASTRA_REWARDS_CONTRACT_SPEC.md`](./ASTRA_REWARDS_CONTRACT_SPEC.md) | Rewards system calls (INIT / CREDIT / CREDIT_LOCKED / END_POA) + 2B cap, now executed natively by the node |
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

## One economic primitive

| Primitive | Emits ASTRA? |
|-----------|--------------|
| **ASTRA** (SRA + native rewards) | Yes — operator service only |

Payments for services, apps, content and channels are ASTRA transfers on the SpaceKit chain. Paid listings use the `astra-entitlement-ledger` contract, which pays the price straight to the publisher.

---

## Code

| Artifact | Location |
|----------|----------|
| `ASTRA_MAX_SUPPLY_WEI`, `ASTRA_GENESIS_TREASURY_WEI`, … | `spacekit-primitives::v1::sdk::token` |
| **Native rewards** (system address `0x…0003`) | `spacekit-compute-node` `native_rewards.rs` |
| **Treasury** contract (`0x…0004`) | Holds the 350M minted by INIT; M-of-N spends |
| **SRA** (target) | `spacekit-compute-node` block execution |

---

## Website

| Page | URL |
|------|-----|
| Economics overview | [`/economics`](https://spacekit.xyz/economics) |
| ASTRA docs | [`/docs`](https://spacekit.xyz/docs) → Tokens & payments |

---

## Versioning

- **v2.0** — current (no aUSD; 2B cap; halving emission; SRA + native rewards; SpaceKit Pay and x402 retired).
- **v1.0** — archived under [`archive/`](./archive/).
- Internal memos and deck revision drafts — archived under [`archive/`](./archive/).
