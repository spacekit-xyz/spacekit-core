# spacekit-agent-microgpt

Micro-GPT agent contract for SpaceKit. Uses the **microgpt_forward** host primitive for deterministic next-token prediction (no LLM, pure on-chain forward pass).

## Builds

- **Rust** (WASM): `cargo build --release` from the workspace root; output in `target/wasm32-unknown-unknown/release/spacekit_agent_microgpt.wasm`.
- **AssemblyScript** (WASM): from this directory run `npm install && npm run build`; output in `build/spacekit_agent_microgpt.wasm`.

## ABI

Input: `[op: u8][token_id: u32 LE][pos_id: u32 LE]` (9 bytes).

| Opcode           | Value | Description                    |
|------------------|-------|--------------------------------|
| OP_NEXT_TOKEN    | 1     | Next token from (token_id, pos_id) |
| OP_CHAT_STEP     | 2     | Same as OP_NEXT_TOKEN (for host-driven chat loops) |

Output: `[next_token_id: u8]` (1 byte). Token IDs are in `[0, VOCAB_SIZE)` (VOCAB_SIZE = 8).

## Host requirement

The SpaceKit VM must provide the **spacekit_microgpt** host module with:

- `microgpt_forward(token_id: u32, pos_id: u32, out_ptr: u32) -> void` — writes 8 × f32 logits at `out_ptr`.

See `spacekit-js` host integration and `MICROGPT.md` in the JS package.

## Usage (host-side)

Encode input and call the contract:

```ts
const op = 1; // OP_NEXT_TOKEN
const tokenId = 0;
const posId = 0;
const input = new Uint8Array(9);
input[0] = op;
new DataView(input.buffer).setUint32(1, tokenId, true);
new DataView(input.buffer).setUint32(5, posId, true);
const result = await vm.callContract(contractId, input);
const nextTokenId = result[0];
```
