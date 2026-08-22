# Astra Escrow

Escrow contract for the SpaceKit VM. Holds escrow terms (token, payer, payee, amount, arbiter) and supports create, release, and refund. Uses `spacekit_storage`.

## Operations

| Op | Name    | Description |
|----|---------|-------------|
| 1  | CREATE  | Create escrow (escrow_id, token_contract, payer, payee, amount, arbiter) |
| 2  | RELEASE | Mark escrow as released |
| 3  | REFUND  | Mark escrow as refunded |
| 4  | GET     | Read full escrow record |

Status values: OPEN (1), RELEASED (2), REFUNDED (3). Input/output use length-prefixed strings and little-endian u64 where applicable (see `src/lib.rs`).

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/astra_escrow.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
