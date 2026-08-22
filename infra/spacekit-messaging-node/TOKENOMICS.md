# Messaging node — ASTRA economics

**Canonical specs:** [`../../economics/spacekit-tokenomics/`](../../economics/spacekit-tokenomics/)

| Document | Purpose |
|----------|---------|
| [`SpaceKit_Tokenomics.md`](../../economics/spacekit-tokenomics/SpaceKit_Tokenomics.md) | Full v2 spec (§1.4 messaging earning) |
| [`ASTRA_EMISSION.md`](../../economics/spacekit-tokenomics/ASTRA_EMISSION.md) | Emission schedule; messaging **10%** of annual emission |
| [`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](../../economics/spacekit-tokenomics/SERVICE_REWARD_ACCUMULATOR_SPEC.md) | `messaging.*` log events → CREDIT |

---

## Status

The messaging crate (`spacekit-messaging-node`) delivers quantum-resistant P2P messaging but **does not yet implement operator ASTRA reward minting**. Reputation and access-control docs describe behavior without token payouts.

**Target model** (SRA — see [`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](../../economics/spacekit-tokenomics/SERVICE_REWARD_ACCUMULATOR_SPEC.md) §3.4):

- **10%** of annual operator emission (20M ASTRA in year 1).
- Rewards for `messaging.message.delivered`, `messaging.broadcast.sent`, `messaging.key.resolved` log events.
- Per-epoch proportional split (not yet implemented in this crate).

**User-paid fees** (not emission): messaging service fees in compute-node bonding curve default to **0.001 ASTRA** base — see `spacekit-compute-node/src/pricing/bonding_curve.rs`.

---

## Legacy references

Older docs may reference **SWTCHX** staking for gated communities. That is not
the current ASTRA model. Use
[`ASTRA.md`](../../economics/spacekit-tokenomics/ASTRA.md) for public ASTRA
language.

---

## Related

- [`README.md`](./README.md) — messaging node features
- [`REPUTATION_FLOW.md`](./REPUTATION_FLOW.md) — reputation (non-token)
- [`operator-guides/README.md`](../../economics/spacekit-tokenomics/operator-guides/README.md) — all operator guides
