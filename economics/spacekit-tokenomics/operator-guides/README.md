# Operator earning guides

These documents describe **legacy testnet reward calculators** in each node crate. **Production economics** use the **Service Reward Accumulator (SRA)** + **AstraRewards** contract per **[`../Service_Reward_Accumulator_Spec.md`](../Service_Reward_Accumulator_Spec.md)** and **[`../AstraRewards_Contract_Spec.md`](../AstraRewards_Contract_Spec.md)**.

Macro emission (halving curve, 350M treasury, 40/30/20/10 category split): **[`../ASTRA_EMISSION.md`](../ASTRA_EMISSION.md)**.

| Node type | Implementation guide | Crate |
|-----------|------------------------|-------|
| **Storage** | [`../../spacekit-storage-node/documentation/whitepaper/tokenomics.md`](../../spacekit-storage-node/documentation/whitepaper/tokenomics.md) | `spacekit-storage-node` |
| **Compute** | [`../../spacekit-compute-node/documentation/SPACEKIT_BLOCKCHAIN_REWARDS.md`](../../spacekit-compute-node/documentation/SPACEKIT_BLOCKCHAIN_REWARDS.md) | `spacekit-compute-node` |
| **Messaging** | [`../../spacekit-messaging-node/TOKENOMICS.md`](../../spacekit-messaging-node/TOKENOMICS.md) | `spacekit-messaging-node` |
| **Validators** | *Validator operations guide (TBD)* | `spacekit-compute-node` / consensus crates |

When implementation defaults change, update both the node guide **and** [`ASTRA_EMISSION.md`](../ASTRA_EMISSION.md) §4.

**Enable SRA on compute-node:** set `[compute.sra_config] enabled = true` in `config.toml` and disable legacy `[compute.token_reward_config] enable_token_minting = false`. Build the on-chain contract:

```bash
cargo build -p astra-rewards --release --target wasm32-unknown-unknown
```

The node loads `astra_rewards.wasm` at system address `0x…0003` and submits `OP_CREDIT` from the faucet/admin account (`0x…0001`) after each mined block.
