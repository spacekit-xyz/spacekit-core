# astra-entitlement-ledger

Entitlement ledger WASM (`OP_CREATE_LISTING`, `OP_PURCHASE`, `OP_GRANT`, `OP_VERIFY`, …).

## Build

Must target `wasm32-unknown-unknown` (plain `cargo build --release` will fail: no `#[panic_handler]` on host).

```bash
# from repo / workspace root
rustup target add wasm32-unknown-unknown

cargo build -p astra-entitlement-ledger \
  --target wasm32-unknown-unknown \
  --release \
  --manifest-path spacekit-standard-library/Cargo.toml
```

Artifact:

```
spacekit-standard-library/target/wasm32-unknown-unknown/release/astra_entitlement_ledger.wasm
```

(or workspace `target/…` if you build from the monorepo root)

## Deploy (compute node)

```bash
spacekit contract deploy \
  --contract target/wasm32-unknown-unknown/release/astra_entitlement_ledger.wasm \
  --name astra-entitlement-ledger \
  --owner-did <your-did>
```

Then set `SPACEKIT_ENTITLEMENT_CONTRACT_ID` to the returned contract id on storage + clients.
`SPACEKIT_COMPUTE_NODE_URL` must point at the compute node that hosts that contract.
