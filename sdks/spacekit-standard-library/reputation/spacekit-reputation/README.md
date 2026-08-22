# SpaceKit Reputation

On-chain reputation contract for the SpaceKit VM. Stores and aggregates reputation scores per DID (subject). Uses `spacekit_storage`.

## Operations

| Op | Name   | Description |
|----|--------|-------------|
| 1  | SUBMIT | Submit a score for a subject DID (subject_did, score: u8) |
| 2  | GET    | Get aggregated reputation for subject → [avg: u64, count: u64] |

Scores are accumulated (sum and count); average can be computed as sum/count. Input format: op byte, then length-prefixed string and u8 (see `src/lib.rs`).

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/spacekit_reputation.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
