# SpaceKit Intent Classifier

WASM smart contract that uses the `spacekit_llm` host to classify user message intent into a category. Used by the SpaceKit playground and other UIs for routing. Runs on the SpaceKit VM.

## Operations

| Op | Name     | Description |
|----|----------|-------------|
| 1  | CLASSIFY | Classify content into one of given categories → single category label |
| 2  | STATUS   | Returns host LLM status (no inference) |

Input format: op byte, then length-prefixed UTF-8 strings (content and categories list). See `src/lib.rs`. The contract builds a short prompt and calls the LLM; temperature and max tokens are set for deterministic classification.

## Building

From this directory or workspace root:

```bash
cargo build --target wasm32-unknown-unknown --release
```

Output: `target/wasm32-unknown-unknown/release/spacekit_intent_classifier.wasm` (or similar from workspace).

## License

Apache 2.0. See the root of the SpaceKit Standard Library for details.
