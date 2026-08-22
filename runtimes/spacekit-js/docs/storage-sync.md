# Storage & Sync

SpacekitVM supports **local persistence** (browser IndexedDB), **periodic snapshots**, **SpaceTime Storage Node** sync, and **contract-visible** persistent blobs via WASM `remote_storage_*` imports (same network as adapters; see Contract SDK notes below).

For **brain registry placement** across storage replicas, topical gossip meshes, and compute-node tier boundaries (local/testnet/production-shaped), see **`BRAIN_REGISTRY_AND_STORAGE_SYNC.md`**.

## IndexedDB (local)

Use `IndexedDbStorageAdapter` for browser persistence:

```ts
const storage = new IndexedDbStorageAdapter("spacekitvm", "kv");
await storage.init();
```

## Auto-sync + snapshots

Use `VmAutoSync` to periodically persist VM state:

```ts
const autosync = new VmAutoSync(vm, { storage });
await autosync.initFromSnapshot();
autosync.start();
```

## Remote sync (`spacekit-storage-node`)

`StorageNodeAdapter` connects to the HTTP storage node:

```ts
const remote = new StorageNodeAdapter({
  baseUrl: "http://localhost:3030",
  did: "did:spacekit:demo",
});
await syncWithStorageNode(storage, remote);
```

Treat this path as **off-chain-but-durable KV / blob tier** backing website deploy receipts, `.gf.toml` deployments, or operator caches.

### Contract ↔ storage (WASM contracts)

Agents use **`remote_storage_put`** / **`remote_storage_get`** (`spacekit_remote_storage` WASM module):

- **`put`** uploads bytes; the host fulfills the effect asynchronously and returns a **content-addressable ref string** written into the Wasm buffer (`STATUS_NEEDS_TOOLS` / `-100` re-exec semantics — see **`CONTRACT_SDK_HOST_REFERENCE.md`**).
- **`get`** retrieves by **that ref** (again effect-queued).

**Session pattern:** For multi-turn workflows (e.g. RouteKit **`CONVERSE`**), clients must **persist the returned ref** returned from the contract and pass it back on each turn — there is no stable “friendly key → bytes” syscall in the Wasm surface today.

Configure the VM host with **`createStorageNodeRemoteBlobAdapter({ baseUrl, did, collection? })`** from `storage.ts` / package root export: it implements **`RemoteStorageAdapter`** (`put(Uint8Array)` → SHA-256-based ref, `get` reads `document.data.value_hex`).

### Production storage node vs simulator vs in-memory

| Backend | Behavior | Signing / replication |
| --------|----------|------------------------|
| **`spacekit-storage-node`** | WAL + DB, PQ envelopes on streams, DID auth on **`/api/documents`**, fact packages with **SPHINCS+** verification paths, libp2p / cross-server routing (see repo README). | **Authoritative** for signed manifests you store as Fact packages or audited document policies. |
| **`spacekit-simulator` HTTP** (`http_gateway.rs`) | In-process **`HashMap`** document store + optional `~/.spacekit/data/documents.json`; mimics **`/api/documents/...`** for local dev only. | **No** Fact pipeline, **no** real multi-node replication — do not infer registry trust from simulator behavior alone. |
| **`createSpaceTimeStorage` / `createInMemoryStorage`** | Pure in-process JSON / KV helpers. | N/A |

Use the **Rust storage node** when exercising **brain-registry manifests**, entitlement policies, and **topics-based replica** semantics; pair the simulator with integration tests only for **HTTP/API shape**.

## Messaging + search (effects)

 **`web_search`** is resolved through the Messaging Node **tool-request** path (`SPACEKIT_WEB_SEARCH_TOPIC`) when **`toolOperatorDid`** + **`requestResponse`** are configured (`IntentMessagingToolAdapter`). Search results arrive as UTF-8 JSON consumed by routing contracts (`routekit-agent` SEARCH / PIPELINE opcodes).

Payments and plain **`messaging_send`** are flushed **after** the Wasm invocation; **`web_search`** and **`remote_storage_*`** participate in the **effect manager** (pending → fulfill → rerun).

## Entitlement-gated content delivery

The **entitlement protocol** enables paid content (DataPackages, AgentPackages, etc.) to be
released to a buyer without exposing the content to unauthorized parties. See
[`ENTITLEMENT_PROTOCOL.md`](./ENTITLEMENT_PROTOCOL.md) for the full specification.

**End-to-end flow:**

1. Publisher encrypts content to the storage node's server key and uploads via `POST /files/envelope-upload`.
2. Publisher calls `OP_CREATE_LISTING` on the `astra-entitlement-ledger` contract, binding a `listing_id` to the `file_id` and price.
3. Buyer calls `OP_PURCHASE` with payment attached; receives a 32-byte `entitlement_id`.
4. Buyer calls `POST /files/{file_id}/rewrap` on the storage node, passing `entitlement-id`, `buyer-did`, and `buyer-public-key` headers.
5. Storage node verifies the entitlement via `OP_VERIFY` on the compute node, then decrypts the file with its server key and re-encrypts a fresh PQ envelope to the buyer's Kyber public key.
6. Buyer decrypts the envelope locally with their Kyber secret key.

The JS helper `purchaseAndDownload()` (exported from `@spacekit/spacekit-js`) orchestrates steps 3–6.

## Conflict resolution

Default merge strategy when syncing VM snapshots with the storage helper is **LWW** (last-write-wins) using version stamps on the merged records.
