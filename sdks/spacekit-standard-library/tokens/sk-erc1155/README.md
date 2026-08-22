# SK ERC-1155

ERC-1155–style multi-token contract for the SpaceKit VM. Balances per (account, token_id), batch balance queries, mint, burn, and per-token URI. Uses `spacekit_storage`.

## Operations

| Op | Name             | Description |
|----|------------------|-------------|
| 1  | BALANCE_OF       | Get balance for account and token id |
| 2  | BALANCE_OF_BATCH | Get balances for n (account, id) pairs |
| 3  | MINT             | Mint amount of token id to account |
| 4  | BURN             | Burn amount of token id from account |
| 5  | URI              | Get URI for token id |

Input format: op byte, then length-prefixed strings (u16 len) and little-endian u64 for id/amounts (see `src/lib.rs`).

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

From workspace root use `-p sk-erc1155-contract` so the package is selected. Output is at the workspace root: `target/wasm32-unknown-unknown/release/sk_erc1155_contract.wasm`.

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
