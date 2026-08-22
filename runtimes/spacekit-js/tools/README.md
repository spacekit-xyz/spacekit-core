# SpaceKit-JS Agent Tools

The SpaceKit-JS VM provides host import modules that WASM smart contracts
(agents) can call as "tools." Each tool bridges the synchronous WASM boundary
using either an **effect queue** (for operations that return data) or
**fire-and-forget buffering** (for side-effects flushed after execution).

## Design Principle: No Outbound HTTP from Contracts

Contracts never make direct outbound HTTP calls. All external API access
(LLM inference, third-party APIs, etc.) goes through the **Messaging Node**.
The contract sends a request message to an operator's Compute Node, which
holds provider API keys and makes the call server-side. The response comes
back as a signed message the contract can verify.

```
Browser (spacekit-js)
  └── Contract (WASM)
        │
        ├── agent_messaging("routekit.completion.request", payload)
        │         │
        │         ▼
        │   Messaging Node (p2p relay)
        │         │
        │         ▼
        │   Compute Node (operator)
        │         │  has provider API keys
        │         │  makes external HTTP call
        │         │  signs response
        │         ▼
        │   agent_messaging("routekit.completion.response", result)
        │         │
        └─────────┘  contract receives signed result
```

No CORS issues. No user API keys exposed. Operators earn ASTRA, pay providers
in fiat, keep the margin.

## Host Function Set

| Function | Purpose |
|----------|---------|
| `growformer_generation` | Local brain inference (free / gas only) |
| `growformer_load_brain` | Load weights from storage node |
| `web_search` | Real-time web data |
| `agent_messaging` | Inter-agent + external API via operators |
| `agent_storage` | Persistent state on storage nodes |
| `agent_agent` | Compose agent pipelines (Growformer) |

## Available Tool Modules

| Module | Host Functions | Pattern |
|--------|---------------|---------|
| `spacekit_tools` | `web_search` | Effect queue |
| `spacekit_messaging` | `messaging_send` | Fire-and-forget |
| `spacekit_remote_storage` | `remote_storage_put`, `remote_storage_get` | Effect queue |
| `spacekit_payments` | `payment_transfer`, `payment_vault_charge` | Fire-and-forget |

## How It Works

### Effect Queue (async tools)

Tools that need network I/O and return data to the contract use an
effect-queue re-execution loop:

1. Contract calls a tool host import (e.g. `web_search`)
2. Host checks its result cache -- if a cached result exists, it writes
   it into contract memory and returns bytes written
3. If no cached result, the host records a `ToolEffect` and returns
   `PENDING (-3)`
4. Contract sees PENDING and returns `STATUS_NEEDS_TOOLS (-100)`
5. The VM fulfills all pending effects asynchronously
6. The VM re-executes the contract -- this time cache hits succeed
7. Capped at 4 re-execution rounds to prevent loops

### Fire-and-Forget (side-effect tools)

Tools like `messaging_send` and `payment_transfer` don't return data.
The host buffers the side-effect and returns success immediately. After
contract execution completes, the VM flushes all buffered effects.

## Host Function Signatures

All functions follow the existing return convention:

- `> 0` -- bytes written to destination buffer
- `-1` -- adapter not configured
- `-2` -- error
- `-3` -- PENDING (effect recorded, contract should return NEEDS_TOOLS)

### spacekit_tools

```
web_search(query_ptr, query_len, max_results, dest_ptr, max_len) -> i32
```

Query is UTF-8. Pending effects are fulfilled with `MessagingAdapter.requestResponse(toolOperatorDid, SPACEKIT_WEB_SEARCH_TOPIC, payload)` where `payload` is the same JSON object `{"query","maxResults"}`. The operator responds with `result_utf8` = JSON array of `{title, url, snippet}`.

Configure `toolOperatorDid` and a `MessagingAdapter` that implements `requestResponse` (e.g. `IntentMessagingToolAdapter` against `POST …/api/messages/tool-request`).

### spacekit_messaging

```
messaging_send(recipient_ptr, recipient_len, payload_ptr, payload_len) -> i32
```

Fire-and-forget. Returns 1 on success. Messages are buffered and sent
to the SpaceKit Messaging Node after contract execution completes.

This is the primary channel for external API access. Contracts send
structured request messages to operator Compute Nodes, which fulfill
them and return signed responses.

### spacekit_remote_storage

```
remote_storage_put(data_ptr, data_len, ref_dest, ref_max) -> i32
remote_storage_get(ref_ptr, ref_len, dest, max) -> i32
```

Content-addressed storage on the SpaceTime Storage Node. `put` returns
a ref string; `get` retrieves data by ref.

### spacekit_payments

```
payment_transfer(to_ptr, to_len, asset_ptr, asset_len, amount: i64) -> i32
payment_vault_charge(amount_ptr, amount_len, beneficiary_ptr, beneficiary_len) -> i32
```

Fire-and-forget. Payments are buffered and submitted after execution.
Users deposit AUSD into the vault; operators charge via vault_charge
for inference-as-a-service.

## TypeScript Adapters

Each tool module is backed by a TypeScript adapter interface. Concrete
implementations are provided:

| Adapter | Module |
|---------|--------|
| `IntentMessagingToolAdapter` | `spacekit_tools` (`web_search`) + `spacekit_messaging` (`messaging_send`) |
| **`createStorageNodeRemoteBlobAdapter`** | WASM `spacekit_remote_storage` — opaque bytes as `value_hex` on **`/api/documents`** |
| `createSpaceTimeStorageNode` | JSON `putBlob` only (same HTTP API shape; **not** a `RemoteStorageAdapter`) |
| `HttpPaymentAdapter` / `NoopPaymentAdapter` | `spacekit_payments` |

WASM `remote_storage_*` receives **raw `Uint8Array`** from the contract. Use **`createStorageNodeRemoteBlobAdapter`** against **spacekit-storage-node** (full crypto, facts, P2P) or the simulator document shim—**not** `createSpaceTimeStorageNode`, which JSON-stringifies app objects.

### Example: Configuring tools on the VM

```typescript
import {
  SpacekitVm,
  IntentMessagingToolAdapter,
  HttpPaymentAdapter,
  createStorageNodeRemoteBlobAdapter,
} from "@spacekit/spacekit-js";

const messaging = new IntentMessagingToolAdapter({
  baseUrl: "https://messaging.spacekit.xyz",
  callerDid: "did:spacekit:myagent",
});

const vm = new SpacekitVm({
  toolOperatorDid: "did:spacekit:operator:search-coordinator",
  messaging,
  remoteStorage: createStorageNodeRemoteBlobAdapter({
    baseUrl: "https://storage.spacekit.xyz",
    did: "did:spacekit:myagent",
    collection: "spacetime",
  }),
  payment: new HttpPaymentAdapter({
    endpoint: "https://payments.spacekit.xyz/api",
  }),
});
```

## Security

- **web_search**: Routed through the Messaging Node (`tool-request`) to `toolOperatorDid`; no direct search HTTP from the runtime.
- **messaging**: Rate limiting should be enforced in the adapter.
- **remote_storage**: Quota enforcement per contract.
- **payments**: Vault balance checks before charge execution.
- **Re-execution cap**: Max 4 tool rounds per transaction.
