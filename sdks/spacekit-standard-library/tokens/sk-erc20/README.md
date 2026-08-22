# SK ERC-20 (SpaceUSD)

ERC-20–style token contract for the SpaceKit VM. Demo token (SpaceUSD / SUSD) for the playground; separate from native ASTRA. Uses `spacekit_storage` for balances and total supply.

## Operations

| Op | Name         | Description |
|----|--------------|-------------|
| 1  | MINT        | Mint amount to DID |
| 2  | TRANSFER    | Transfer amount from caller to recipient |
| 3  | BALANCE     | Get balance for DID |
| 4  | TOTAL_SUPPLY| Get total supply |
| 5  | METADATA    | Get name/symbol/decimals |

Input format: op byte, then length-prefixed strings and little-endian u64 where applicable (see `src/lib.rs`).

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/sk_erc20_contract.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
