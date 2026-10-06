# tokenomics_update (source drop)

**Status:** Integrated into parent [`spacekit-tokenomics/`](../) — May 2026.

This folder was the authoritative source drop for emission schedule v1.0, AstraRewards, and the Service Reward Accumulator. Canonical copies now live at:

| Source file | Canonical location |
|-------------|-------------------|
| `astra-emission-schedule.md` | [`../ASTRA_EMISSION.md`](../ASTRA_EMISSION.md) |
| `astra-rewards-contract-spec.md` | [`../ASTRA_REWARDS_CONTRACT_SPEC.md`](../ASTRA_REWARDS_CONTRACT_SPEC.md) |
| `service-reward-accumulator-spec.md` | [`../SERVICE_REWARD_ACCUMULATOR_SPEC.md`](../SERVICE_REWARD_ACCUMULATOR_SPEC.md) |
| `AstraRewards.rs` | Superseded: rewards now run natively in the node (`spacekit-compute-node` `native_rewards.rs`, system address `0x…0003`); there is no AstraRewards WASM contract. |

Keep this directory for diff history. **Edit canonical files above**, not copies here, unless doing a deliberate re-import.
