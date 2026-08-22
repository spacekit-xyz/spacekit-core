# SpaceKit Agent Inference Market

Marketplace contract for agent inference jobs on the SpaceKit VM. Agents set prices; callers create jobs and receive results. Uses `spacekit_storage`.

## Operations

| Op | Name           | Description |
|----|----------------|-------------|
| 1  | SET_AGENT_PRICE| Set price for an agent (DID) |
| 2  | GET_AGENT_PRICE| Get price for an agent |
| 3  | CREATE_JOB     | Create an inference job |
| 4  | SUBMIT_RESULT  | Submit job result (e.g. by agent) |
| 5  | GET_JOB        | Get job details and result |

Input format: op byte, then length-prefixed strings and other args as in `src/lib.rs`.

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/spacekit_agent_inference_market.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
