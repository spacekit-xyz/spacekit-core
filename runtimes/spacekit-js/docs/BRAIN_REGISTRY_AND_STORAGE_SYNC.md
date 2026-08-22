# Brain registry, storage-node placement, and topical replication

This note ties **brain catalog metadata** + **heavy brain/WASM payloads** to **spacekit-storage-node**, relates **spacekit-compute-node** tiers (local / testnet-shaped / production), and outlines how **subscriber** storage nodes could replicate **without** duplicating the whole network—using **topic- or context-scoped sync**, aligned with networking code that already exists in-tree.

Companion docs: **`storage-sync.md`** (VM `remote_storage_*` + refs), **`GF_PROJECT_SPEC.md`** (brain keys + deploy), **`routekit-agent`** / **`README.md`** (runtime brain keys like `ROUTER_BRAIN_KEY`). Example manifest: **`examples/brain-registry-manifest.v1.example.json`**.

## 1. What “brain registry” should store vs leave as blobs

| Concern | Best home | Notes |
| --------|-----------|------|
| **Brain weights `.bin`, packaged WASM** | **Storage node** blobs (content-addressed refs) | Contracts use WASM `remote_storage_*`; browsers wire **`RemoteStorageAdapter`** via **`createStorageNodeRemoteBlobAdapter`** (opaque bytes ↔ `value_hex`). Same HTTP path works against **Rust node** or simulator stub—**trust semantics differ** (§1a). |
| **Registry rows** | **Small signed manifests** pinned on storage (same layer) | Each row links: **semantic brain key / version**, **publisher DID**, **`content_hash`/`ref`**, optional **topology tag** (`local`, `staging`, `prod`), **`Capabilities`/`tier`** hints—not the multi‑GB artifact inline. |
| **Execution proofs / run receipts** | **Compute-centric** tier | **`spacekit-compute-node`** is the operational place for workloads and proofs; **`SPACEKIT_UNIVERSE_ARCHITECTURE.md`** (`spacekit-compute-node/README.md`) already separates packaged agents on storage from compute as runtime source of truth. |

**Do not** use the compute ledger alone as the bulky registry of every brain binary—the storage network is purpose-built for large, fetch-by-ref artifacts; compute should **consume** refs it trusts.

### 1a. Production **`spacekit-storage-node`** vs simulator HTTP vs VM wiring

| Layer | What it is | Trust / signatures / replication |
| ----- | ----------- | ---------------------------------- |
| **`spacekit-storage-node` (Rust)** | Primary network service: WAL-backed DB, PQ **envelope** handling for streams/uploads, DID-gated **`/api/documents`**, **Fact** packages with **SPHINCS+** verification pipelines, optional **libp2p** + **cross-server topic** routing (`network.rs`, `server_routing.rs`). | Treat as **canonical** when pinning **brain-registry manifests**, binary refs, ACLs. Production **subscriber sync** should subscribe to topical meshes here—not to simulator state. |
| **Simulator HTTP** (`spacekit-simulator/src/http_gateway.rs`) | **`HashMap`** document store (+ optional **`~/.spacekit/data/documents.json`** persistence) exposing the same **`/api/documents/{collection}/{id}`** routes for CI/dev. | **Compatibility stub only**: no Fact verification, **no** real multi-peer replication semantics. Validates HTTP clients (e.g. `StorageNodeAdapter`, JS blob adapter); **does not model** PQ signing or topical gossip correctness. |
| **SpacekitVM-JS adapters** | VM **`remote_storage_*`** needs **`RemoteStorageAdapter`** with raw bytes — use **`createStorageNodeRemoteBlobAdapter`** (`storage.ts`). **`createSpaceTimeStorageNode`** JSON-stringifies app objects (`putBlob`) and is **not** that interface. **`createSpaceTimeStorage` + `createInMemoryStorage`** stay entirely in-memory. See **`storage-sync.md`**. | Point production browsers at **`baseUrl`** of a **Rust** storage node when testing signed manifests end-to-end. |

Brain weights and WASM blobs still land as **`value_hex`** (or richer envelope) payloads the **Rust** stack controls; manifests reference those refs plus optional detached signatures (fact pipeline integration TBD).

## 2. Relationship to environments (local → testnet → mainnet-shaped)

Operational shape:

1. **Storage**: same protocol; **different bootstraps** (base URL / DHT anchors / pinned trust roots).
2. **Compute**: binds to the storage + messaging URLs appropriate to that tier; agents resolve **deployment-seeded VM storage keys** (e.g. `routekit_router`, `chat_brain`) populated from deploy receipts referencing **storage refs**.
3. **Registry manifests** SHOULD carry **`network_context`** / **`deployment_id`** so a node never applies a staging ref when configured for prod.

Bridging tiers is intentionally **explicit** (export manifest, replay pin, governance approval)—not silent global merge.

## 3. Storage-node replication mechanics (today’s code hooks)

Rough map to the Rust tree:

| Mechanism | Where | Implication for brain sync |
|-----------|-------|----------------------------|
| **libp2p + gossipsub** (discovery, DID-flavored topic) | `spacekit-storage-node/src/network.rs` — e.g. `DID_TOPIC` | Suitable pattern for **`topic = f(registry_shard)`** gossip: advertise **manifest CIDs/ref strings**, not full binaries on every gossip message when binaries are deduped by content addressing. |
| **Chunk/file announce + retrieve** | Same module + demos (`examples/p2p_network_demo.rs` conceptually demonstrates announce/retrieve flows) | After a subscriber learns a brain ref via gossip, pull **exact bytes** lazily (`replication_factor` and reward logic in demos are placeholders for topology policy). |
| **Cross-server topic subscribe** | `spacekit-storage-node/src/server_routing.rs` + API routes (`…/servers/{id}/subscribe`) | Good fit for **contextual overlays**: federation of storage clusters each subscribing only to **`brain/catalog/{org}`** style topics routed through designated bridge peers. |

No single “download everything everywhere” subsystem is mandated by this design; replication should stay **intent-driven**.

## 4. Subscriber nodes: topical / contextual replication

Goals:

1. **Reduce bandwidth** — only replicate catalogs and blobs needed for subscribed **context** (publisher DID, geography, SLA tier, RouteKit lineage, …).
2. **Contain blast radius** — bad or oversized publishes stay within a topic shard until policy pulls them.
3. **Allow hierarchical fan-out** — “root” manifests per context; optionally **delegated shards** beneath them.

Suggested **topic naming** convention (Gossipsub mesh or federated routing):

```
spacekit/brain-registry/v1/{scope}/{publisher_did_shard}/{tier}
```

- **`scope`** — e.g. `public`, `org:<id>`, `geo:<code>`.
- **`publisher_did_shard`** — hash prefix of issuer DID so meshes don’t become one mega-channel.
- **`tier`** — e.g. `embed`, `edge`, `frontier`; matches product SKUs.

**Synchronization payloads** should be predominantly:

- **manifest announcements** (signed JSON or CBOR pointing at **storage refs** already written with `HTTP PUT`/node-native upload); and  
- occasional **summaries** (Merkle root over manifest set in `(scope, timeframe)`) so peers can reconcile **incrementally**.

Full brain binaries propagate only along **subscriber interest** (“this scope + this DID + embeddings tier”) plus **explicit pin/join policy**, not implicitly to all nodes.

### Where “pure contract” WASM still helps

An optional **`brain-registry` agent** WASM can anchor **publisher intent** + **immutable hash** inside the VM’s trust model—but **heavy bytes** remain on **storage-node**; the registry contract or manifest only cites **refs** and **publisher signatures**.

## 5. Messaging node vs storage gossip (division of labor)

Use **messaging** for **intent** and **latency-sensitive operator tools** (`tool-request` / envelopes).  

Use **storage + P2P/topic gossip** for **durable catalogs and artifact fan-out**.

Avoid duplicating gigabyte artifacts through messaging payloads; gossip **refs + attestations**.

## 6. Brain registry manifest v1 (JSON)

Tooling, operators, and optional WASM registry agents should share this **minimal** shape. **Heavy bytes are never inline**—only **`storage_ref`** strings returned by the storage node (or equivalent content addressing).

| Field | Required | Description |
| ----- | -------- | ----------- |
| `manifest_version` | yes | `1` for this revision. |
| `artifact_kind` | yes | e.g. `growformer_brain`, `wasm_agent`, `bundle` (brain + wasm). |
| `publisher_did` | yes | Issuer accountable for pins and billing. |
| `brain_storage_key` | yes for Growformer manifests | Matches VM seed key (`spacekit-growformer-agent`, **`ROUTER_BRAIN_KEY`** / `.gf.toml` `growformer.brain_storage_key`). |
| `network_context` | yes | Logical lane: **`local`** \| **`staging`** \| **`production`** (or deployment-specific enums). Consumers MUST reject mismatches vs their configured lane. |
| `artifacts.*.storage_ref` | yes where applicable | Opaque refs from storage `put`; used by `remote_storage_get` workflows and hydrators. |
| `topics` | no | Intended Gossipsub / federated shards (see §4); duplicated in `.gf.toml` `extras.registry_topics`. |
| `project_slug` | no | Mirrors `.gf.toml` `project.slug`. |
| `issued_at` | yes | RFC3339 UTC. |
| `compatibility.gf_version` | no | `.gf.toml` `gf_version`. |
| `compatibility.min_spacekit_abi` | no | Future: `HOST_ABI_VERSION` integer from `abi.ts`. |
| `artifacts.*.sha256_hex` | no | Offline verification helper once blob is fetched. |

**Signatures (recommended next step):** add a top-level `signatures[]` array (alg name, signer DID, canonical-JSON digest, detached signature bytes base64). Prefer ingesting manifests through **`spacekit-storage-node` Fact workflows** once those signatures exist so **`SPHINCS+`** and policy gates apply uniformly; the simulator lacks that path. Until then, manifests are provenance-sensitive documents—pin only via trusted publish paths.

**JS ref encoding:** Wasm `remote_storage_*` tooling often uses **`createStorageNodeRemoteBlobAdapter`**, yielding refs **`{refPrefix}:{sha256_hex}`** (default prefix `blob:`). Manifest `artifacts.*.storage_ref` SHOULD echo whatever the production deploy / Rust node returned so subscribers resolve the identical document row.

Canonical example file: **`docs/examples/brain-registry-manifest.v1.example.json`** (adjust `storage_ref` after deploy).

### Pinning policy (subscriber node sketch)

Declarative knobs a storage replica could honor (YAML-style for readability only):

```yaml
subscriber_topics:
  - "spacekit/brain-registry/v1/public/*/embed"
max_artifact_mb_by_tier:
  embed: 512
  edge: 2048
network_context_allowlist: ["local"]   # dev laptop
trust_publishers_prefix: ["did:spacekit:org:approved"]
```

## 7. Open engineering follow-ups

- **`signatures[]`** + canonical byte encoding spec; ingest through **Rust** Fact, AppPackage, WebPackage, AgentPackage as all facts package rooted for decentralized marketplace and storage using **SPHINCS+**. we have this unified in spacekit-primitives.
- **Publisher hook**: automated **Gossipsub publish** after manifest document write (distinct from **`spacekit brain-registry publish`** CLI which only does HTTP PUT).
- **Thin vs full CLI**: **`brain-registry build|publish`** are **`--features full` only**; thin keeps **`init`, `brain push`, `storage` HTTP** unchanged.
- **Simulator / bridge**: integration tests so **staging** gossip domains cannot poison **production** pinning without explicit relay policy.
