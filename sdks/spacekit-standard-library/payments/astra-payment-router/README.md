# Astra Payment Router

Payment split / router contract for the SpaceKit VM. Stores and returns split configuration. Uses `spacekit_storage`.

## Operations

| Op | Name     | Description |
|----|----------|-------------|
| 1  | SET_SPLIT | Set split configuration |
| 2  | GET_SPLIT | Get split configuration |

Input format: op byte, then length-prefixed strings as defined in `src/lib.rs`.

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/astra_payment_router.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
