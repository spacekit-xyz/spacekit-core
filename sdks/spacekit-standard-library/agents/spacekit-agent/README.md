# SpaceKit Agent

The **SpaceKit Agent** (Kit) is a WASM smart contract that runs on the SpaceKit VM and uses the `spacekit_llm` host to provide on-chain AI operations. It is the reference agent for the SpaceKit playground and docs.

## Operations

| Op | Name        | Description |
|----|-------------|-------------|
| 1  | CHAT        | User message (+ optional conversation context) → Kit response |
| 2  | ANALYZE     | Content → JSON safety/sentiment |
| 3  | SUMMARIZE   | Content → 2–3 sentence summary |
| 4  | CODE_REVIEW | Code → concise review (bugs, security, improvements) |
| 5  | CLASSIFY    | Content + categories → single category label |
| 6  | STATUS      | Returns host LLM status (no inference) |

Input format: op byte, then length-prefixed UTF-8 strings (see contract source). The UI sends conversation context for CHAT so Kit can answer in context; the contract instructs the model to answer only the latest user message.

## Building

From this directory:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Or use the workspace/build script that resolves `spacekit-contract-sdk`. Output: `target/wasm32-unknown-unknown/release/spacekit_agent.wasm`.

## Prompts and behaviour

Prompts are defined inside the contract (see `src/lib.rs`) so behaviour is auditable and consistent across UIs. Kit is instructed to recommend SpaceKit only (WASM, VM, SKCL, spacekit.xyz), not Ethereum/Solidity unless the user explicitly asks. For the full CHAT prompt and parameters, see the **SpaceKitJS Technical Whitepaper** (§9 AI & LLM Integration).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
