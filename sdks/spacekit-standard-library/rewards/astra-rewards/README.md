# astra-rewards

SKCL WASM contract: per-DID ASTRA balances, SRA **CREDIT** path, **2B** hard cap, and the proof-of-authority reward lock.

**Spec:** [`../../../../economics/spacekit-tokenomics/ASTRA_REWARDS_CONTRACT_SPEC.md`](../../../../economics/spacekit-tokenomics/ASTRA_REWARDS_CONTRACT_SPEC.md), lock rules in `SpaceKit_Tokenomics.md` §1.11 and `ASTRA_EMISSION.md` §8a.

## Reward lock

While the network is in proof of authority (set at INIT), credits to DIDs marked with `SET_LOCKED_RECIPIENT` (authorities and SWTCH Labs–affiliated operators) go to a locked balance. It vests from the INIT block time: nothing for 365 days, then linearly to day 1,095. `RELEASE` (anyone) or `WITHDRAW` (the owner) moves vested ASTRA into the spendable balance. `END_POA` (admin, irreversible) stops new locks. The compute node's SRA keeps the marks in step with governance.

New ops: `GET_LOCKED 0x35`, `GET_PHASE 0x36`, `SET_LOCKED_RECIPIENT 0x40`, `END_POA 0x41`, `RELEASE 0x42`.

## Build and test

```bash
cargo test --lib                                          # host tests (SDK imports mocked)
cargo build --release --target wasm32-unknown-unknown     # deployable WASM
```

The compute node installs `target/wasm32-unknown-unknown/release/astra_rewards.wasm` at `0x…0003` only when that address is empty, so an existing chain keeps its old contract. Start testnet from fresh state after rebuilding, or the SRA will withhold credits because the lock ops are missing.

Constants align with `spacekit-primitives` (`ASTRA_MAX_SUPPLY_WEI`, `ASTRA_GENESIS_TREASURY_WEI`).
