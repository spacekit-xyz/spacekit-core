# SpaceKit Box

Dropbox-style blob storage contract for the SpaceKit VM. Stores and retrieves blobs by name using `spacekit_storage`. Emits events on put/delete.

## Operations

| Op | Name   | Description |
|----|--------|-------------|
| 1  | PUT    | Store a blob by name (name, blob bytes) |
| 2  | GET    | Load blob by name → blob bytes |
| 3  | DELETE | Remove blob by name |

Input format: op byte, then length-prefixed UTF-8 name and length-prefixed bytes for PUT (see `src/lib.rs`).

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/spacekit_box.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
