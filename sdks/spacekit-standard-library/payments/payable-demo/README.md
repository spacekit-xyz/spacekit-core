# Payable Demo

Demo contract that records attached value and allows withdrawals. Used to demonstrate `msg_value` and balance handling on the SpaceKit VM.

## Operations

| Op | Name           | Description |
|----|----------------|-------------|
| 1  | DEPOSIT       | Record deposited value (caller + value) |
| 2  | TOTAL_RECEIVED| Get total received by contract |
| 3  | WITHDRAW      | Withdraw to caller |
| 4  | BALANCE       | Get balance for an account |

Input format: op byte, then length-prefixed strings / u64 as in `src/lib.rs`.

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/payable_demo.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
