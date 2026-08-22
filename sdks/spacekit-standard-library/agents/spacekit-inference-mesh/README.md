# SpaceKit Inference Mesh

Mesh contract for inference donors and job assignment on the SpaceKit VM. Donors register; jobs are created and assigned; results are set. Uses `spacekit_storage`.

## Operations

| Op | Name           | Description |
|----|----------------|-------------|
| 1  | REGISTER_DONOR | Register as inference donor |
| 2  | GET_DONOR      | Get donor info |
| 3  | CREATE_JOB     | Create inference job |
| 4  | ASSIGN_JOB     | Assign job to donor |
| 5  | SET_JOB_RESULT | Set result for a job |
| 6  | GET_JOB        | Get job details and result |

Input format: op byte, then length-prefixed strings and other args as in `src/lib.rs`.

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/spacekit_inference_mesh.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
