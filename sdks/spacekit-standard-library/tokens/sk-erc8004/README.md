# SK ERC-8004 (Agent Registry)

ERC-8004–style agent registry for the SpaceKit VM. Complements an ERC-721 agent NFT contract: stores agent profile, feedback, and validation records keyed by agent_id (e.g. token_id from the ERC-721). Does not mint or transfer tokens. Uses `spacekit_storage`.

## Operations

| Op | Name                | Description |
|----|---------------------|-------------|
| 1  | AGENT_PROFILE_SET   | Set profile for agent_id |
| 2  | AGENT_PROFILE_GET   | Get profile for agent_id |
| 3  | AGENT_FEEDBACK_SUBMIT | Submit feedback for agent_id |
| 4  | AGENT_FEEDBACK_GET  | Get feedback for agent_id |
| 5  | AGENT_VALIDATION_SET| Set validation record for agent_id |
| 6  | AGENT_VALIDATION_GET| Get validation record for agent_id |

Input format: op byte, then length-prefixed strings/bytes as in `src/lib.rs`.

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/sk_erc8004_contract.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
