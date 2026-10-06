# SpaceKit tokenomics (documentation index)

Canonical economics specifications live in
**[`economics/spacekit-tokenomics/`](../../economics/spacekit-tokenomics/)**.

| Document | Description |
|----------|-------------|
| [`SpaceKit_Tokenomics.md`](../../economics/spacekit-tokenomics/SpaceKit_Tokenomics.md) | v2 spec — ASTRA |
| [`ASTRA_EMISSION.md`](../../economics/spacekit-tokenomics/ASTRA_EMISSION.md) | Halving curve, treasury 350M, category shares |
| [`ASTRA_REWARDS_CONTRACT_SPEC.md`](../../economics/spacekit-tokenomics/ASTRA_REWARDS_CONTRACT_SPEC.md) | Rewards system calls (now executed natively by the node at `0x…0003`) |
| [`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](../../economics/spacekit-tokenomics/SERVICE_REWARD_ACCUMULATOR_SPEC.md) | Protocol SRA |
| [`ASTRA.md`](../../economics/spacekit-tokenomics/ASTRA.md) | Public ASTRA narrative |
| `spacekit-compute-node` `native_rewards.rs` | Native rewards implementation (replaces the AstraRewards WASM contract) |
| [`operator-guides/README.md`](../../economics/spacekit-tokenomics/operator-guides/README.md) | Per-node earning implementation guides |

**Website:** [`spacekit.xyz/economics`](https://spacekit.xyz/economics) (summary)

**Payments:** ASTRA is the only currency. SpaceKit Pay was retired; see [`PAYMENTS_AND_CURRENCY.md`](../PAYMENTS_AND_CURRENCY.md).

**Legacy:** v1.0 dual-token material is historical and must not override the
current economics tree.

**Code constant:** `ASTRA_MAX_SUPPLY_WEI` in `spacekit-primitives` (`2_000_000_000` tokens × 10¹⁸).
