# Operator earning guides

These documents describe **legacy testnet reward calculators** in each node crate. **Production economics** use the **Service Reward Accumulator (SRA)** + the node's **native rewards** system calls (`0x…0003`) per **[`../SERVICE_REWARD_ACCUMULATOR_SPEC.md`](../SERVICE_REWARD_ACCUMULATOR_SPEC.md)** and **[`../ASTRA_REWARDS_CONTRACT_SPEC.md`](../ASTRA_REWARDS_CONTRACT_SPEC.md)**.

Macro emission (halving curve, 350M treasury, 40/30/20/10 category split): **[`../ASTRA_EMISSION.md`](../ASTRA_EMISSION.md)**.

| Node type | Implementation guide | Crate |
|-----------|------------------------|-------|
| **Storage** | [`../../spacekit-storage-node/documentation/whitepaper/tokenomics.md`](../../spacekit-storage-node/documentation/whitepaper/tokenomics.md) | `spacekit-storage-node` |
| **Compute** | [`../../spacekit-compute-node/documentation/SPACEKIT_BLOCKCHAIN_REWARDS.md`](../../spacekit-compute-node/documentation/SPACEKIT_BLOCKCHAIN_REWARDS.md) | `spacekit-compute-node` |
| **Messaging** | [`../../spacekit-messaging-node/TOKENOMICS.md`](../../spacekit-messaging-node/TOKENOMICS.md) | `spacekit-messaging-node` |
| **Validators** | *Validator operations guide (TBD)* | `spacekit-compute-node` / consensus crates |

When implementation defaults change, update both the node guide **and** [`ASTRA_EMISSION.md`](../ASTRA_EMISSION.md) §4.

**Enable SRA on compute-node:** set `[compute.sra_config] enabled = true` in `config.toml` and disable legacy `[compute.token_reward_config] enable_token_minting = false`. No contract build is needed.

The SRA places the rewards system calls to `0x…0003` (INIT, CREDIT, CREDIT_LOCKED, END_POA) at the start of a block, and the node executes them itself (`native_rewards.rs`). CREDIT mints straight into the recipient address's native balance. Every importer re-derives and checks these calls.
