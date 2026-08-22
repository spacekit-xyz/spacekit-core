# Host ABI + Contract Development

Spacekit contracts are compiled to `wasm32-unknown-unknown` and expect a deterministic
host ABI. The JS host implements the import modules required by the Rust SDK.

## Import modules
Core modules implemented by the host:
- `env`
- `spacekit_storage`
- `sk_erc20`
- `sk_erc721`
- `spacekit_reputation`
- `spacekit_fact`
- `spacekit_llm`
- `spacekit_agent` — Growformer brain runtime (optional). Host implements `agent_growformer_*` imports; initialize with `initGrowformerHost()` from `@spacekit/spacekit-js` and load a `.bin` brain before calling contracts that use `spacekit_contract_sdk::growformer_*`.

## Streaming a Growformer brain

`initGrowformerHostWithBrainFromUrl` reads `Response.body` incrementally, applies
a 1 GiB default transfer limit, and preallocates from a valid `Content-Length`.
Callers can set a lower or higher limit, monitor progress, cancel, and verify an
artifact digest:

```ts
await initGrowformerHostWithBrainFromUrl(
  "https://node.example/v1/brain/export?compressed=true",
  {
    maxBytes: 2 * 1024 * 1024 * 1024,
    expectedSha256Hex: manifest.sha256,
    signal: abortController.signal,
    onProgress: ({ phase, bytesReceived, totalBytes }) => {
      console.log(phase, bytesReceived, totalBytes);
    },
  },
);
```

Use `fetchGrowformerBrainBytes` when download/verification and host
initialization need separate lifecycle steps. If a runtime does not expose a
readable response stream, the helper falls back to `arrayBuffer()` while still
enforcing the configured limit.

Transport streaming avoids an extra full download buffer, but the current
wasm-bindgen boundary still copies one contiguous `Uint8Array` into WebAssembly
and Growformer still materializes the full deserialized model. Independently
lazy specialists require a future versioned brain format.

## Storage semantics
Some legacy contracts call a 2-arg `storage_read` and only get length. Newer contracts
use a 4-arg read with output buffer. The host supports both.

## Events
Contracts can emit events; the host collects them into receipts. See VM receipts and
`vm_receiptProof` for inclusion proofs.

## Payable calls
Contracts can read attached value via `env.msg_value()` (u64). The VM can attach value
when submitting transactions (JS VM and compute-node).

## ABI versioning
Use `HOST_ABI_VERSION` and `vm_hostAbi` to ensure deterministic execution across
browser and compute-node runtimes.
