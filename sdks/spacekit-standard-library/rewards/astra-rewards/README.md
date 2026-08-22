# astra-rewards

SKCL WASM contract: per-DID ASTRA balances, SRA **CREDIT** path, **2B** hard cap.

**Spec:** [`../../../../economics/spacekit-tokenomics/ASTRA_REWARDS_CONTRACT_SPEC.md`](../../../../economics/spacekit-tokenomics/ASTRA_REWARDS_CONTRACT_SPEC.md)

Build for deployment:

```bash
cargo build -p astra-rewards --release --target wasm32-unknown-unknown
```

Constants align with `spacekit-primitives` (`ASTRA_MAX_SUPPLY_WEI`, `ASTRA_GENESIS_TREASURY_WEI`).
