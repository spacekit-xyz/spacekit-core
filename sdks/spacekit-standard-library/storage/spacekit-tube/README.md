# SpaceKit Tube

Video-style storage contract for the SpaceKit VM. Stores video blobs and metadata by ID using `spacekit_storage`. Emits events on publish/delete.

## Operations

| Op | Name     | Description |
|----|----------|-------------|
| 1  | PUBLISH  | Publish video (id, blob, metadata) |
| 2  | GET_VIDEO| Load video blob by id |
| 3  | GET_META | Load metadata only by id |
| 4  | DELETE   | Remove video and metadata by id |

Input format: op byte, then length-prefixed strings/bytes as in `src/lib.rs`.

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/spacekit_tube.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
