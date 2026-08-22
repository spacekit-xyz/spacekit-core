# SK ERC-721

ERC-721–style NFT contract for the SpaceKit VM. Basic collectibles with mint, transfer, owner, and token URI. Uses `spacekit_storage`.

## Operations

| Op | Name        | Description |
|----|-------------|-------------|
| 1  | MINT        | Mint token to owner (optional URI) |
| 2  | TRANSFER    | Transfer token to new owner |
| 3  | OWNER_OF    | Get owner DID of token_id |
| 4  | SET_TOKEN_URI | Set URI for token_id |
| 5  | TOKEN_URI   | Get URI for token_id |
| 6  | TOTAL_SUPPLY | Get total minted count |

Input format: op byte, then length-prefixed strings and little-endian u64 for token_id/amounts (see `src/lib.rs`).

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/sk_erc721_contract.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
