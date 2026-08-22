# Filling the Spec Gaps & Build Plan

**Goal:** (1) Fill the network spec gaps for **Messaging Node**, **Storage Node**, and **Witness protocol**. (2) Define the build order: RouteKit relay first, then Witness (and related components), with clear dependencies and milestones.

---

## Part 1 — How to fill the spec gaps

The three components are **already implemented** in the monorepo (Messaging Node, Storage Node) or partially (Verkle witness per-block in Compute Node; cross-chain witness operators are Phase 3). The gap is **formal specification** so that protocol and architecture docs are the single source of truth. Use the following placement and outline.

### Where to add the specs

| Component | Protocol spec (SPACEKIT-INTENT-PROTOCOL-SPEC.md) | Architecture (ARCHITECTURE.md or network doc) |
|-----------|--------------------------------------------------|-----------------------------------------------|
| **Messaging Node** | New §2.x: Messaging Node — role in intent lifecycle (status, agent alerts), wire format for intent status, relation to relay polling. | New section: Messaging Node — responsibilities, topology (clients ↔ relay ↔ Compute Node), intent status streaming, agent notifications, v2 gossip for relay decentralisation. |
| **Storage Node** | Reference only: “Storage Node provides data availability and content-addressed storage; see Architecture.” | New section: Storage Node — p2p CDN, content-addressed storage, fact storage, data availability for contract state and model weights, sub-network state, operator model (ASTRA). |
| **Witness protocol** | New §: Witness protocol — proof semantics (what is being attested), who consumes proofs (on-chain verifiers, external chains). | New section: Witness protocol — proof generation (Verkle per-block), anchor format per chain (BTC, ETH, Solana, SpaceKit), witness operator role, unrolling/verification protocol, incentives (ASTRA), relation to quantum-verkle and stateless sync. |

### Suggested section content (copy into the actual spec/arch docs)

**Messaging Node (protocol spec §2.x)**

- **Purpose:** Real-time coordination between clients, RouteKit relay, and Compute Node without polling.
- **Intent status:** Push-based updates for `intent_id` (e.g. `accepted` | `submitted` | `executed` | `failed`). Replaces or complements `GET /v1/intent/:id` polling.
- **Agent notifications:** When an agent-signed intent is executed (or fails), the granting user can receive a notification over the Messaging Node (topic/channel TBD).
- **Wire format:** Define at least: topic naming (e.g. `intent/status/{intent_id}` or `actor/{actor_id}/intents`), message envelope (intent_id, status, timestamp, optional receipt), and auth (DID or session token).
- **Relay decentralisation (v2):** Messaging Node as the backbone for gossip-based intent propagation; relay becomes one of many participants. Mark as future.

**Storage Node (architecture)**

- **Role:** p2p content-addressed CDN; data availability for contract state, rollup bundles, fact packages, and (optionally) model weight manifests.
- **Operator model:** Storage operators run nodes; rewards (ASTRA) tied to usage, not just uptime.
- **Sub-network state:** Sharding and availability for public/private sub-networks; quantum-verkle state roots and proofs can be stored or referenced.
- **APIs:** Reference existing storage-node APIs (e.g. documents, kit-content, kit-protocol-state) or link to spacekit-storage-node README for implementation details.

**Witness protocol (architecture + protocol)**

- **Per-block witness (existing):** Every SpaceKit block can carry a Verkle witness (multi-proof over accessed state). Used for stateless sync and light-client verification. Already in Compute Node and spacekit-js.
- **Cross-chain anchoring:** SpaceKit state roots (or block commitments) are anchored to Bitcoin, Ethereum/EVM, Solana, and SpaceKit mainnet. Format per chain: e.g. OP_RETURN / calldata / account data / SpaceKit native.
- **Witness operators:** Independent nodes that (1) observe SpaceKit proof/commitment output, (2) submit or verify anchors on one or more external chains, (3) optionally “unroll” proofs so that external chains can verify SpaceKit state without running a full node. Incentives: ASTRA for correct verification/submission.
- **Unrolling/verification protocol:** Define the minimal flow: proof emission from SpaceKit → witness picks up → witness submits to target chain in chain-specific format → verification contract or opcode on target chain. Security: witness does not need to be trusted for correctness if the target chain verifies the proof; witness is for availability and batching.

Filling the gaps is therefore **documentation work** in the right sections of the protocol spec and architecture doc; implementation of Messaging and Storage already exists. Witness has per-block Verkle in place; the net new build is the **cross-chain witness service** (see Part 2).

---

## Part 2 — Build order: RouteKit → Witness → …

Dependencies and rationale:

1. **RouteKit relay** does not depend on Messaging Node, Storage Node, or Witness for v1. It validates intents and forwards to the Compute Node over HTTP. So RouteKit is the first deliverable.
2. **Messaging Node** is already built. The next step is **integration** with the intent lifecycle: relay and/or Compute Node publish intent status; clients subscribe via Messaging Node instead of (or in addition to) polling. This improves UX and prepares v2 relay decentralisation.
3. **Witness** has two parts: (a) Per-block Verkle witness — done in Compute Node and spacekit-js. (b) **Cross-chain witness operators** — not yet built; this is the “Witness” build that unlocks Phase 3 (enterprise, cross-chain anchoring). So after RouteKit and Messaging integration, the next component build is the witness service/protocol.
4. **Storage Node** is already built and used by Compute Node and apps. No RouteKit dependency. Filling the Storage Node spec is doc-only unless we later add “fetch from Storage” for some RouteKit concern (e.g. model catalog); currently we use LiteLLM URL for prices.

### Proposed sequence

| Phase | What | Outcome |
|-------|-----|--------|
| **1. RouteKit node** | Build the Rust relay: classifier, routing engine, provider adapters, model prices (LiteLLM 6h), intent validation (schema + signature), `/v1/complete` and `POST /v1/intent`, health, cost tracker. | Single endpoint for AI routing and intent submission; developers can ship against RouteKit. |
| **2. Messaging integration** | Specify intent-status and agent-notification channels (see Part 1). Implement: relay or Compute Node publishes status to Messaging Node; clients (and RouteKit if needed) subscribe. Optional: RouteKit consumes status for its own metrics. | No more polling for intent status; agent alerts; foundation for v2 gossip. |
| **3. Encrypted intent envelope (v1.1)** | Envelope format: relay sees only metadata; Compute Node decrypts. Implement in spacekit-js (build envelope), relay (forward without decrypting), Compute Node (decrypt and submit). | Institutional-ready; no front-running surface at relay. |
| **4. Witness protocol (spec + service)** | (a) Write Witness protocol section in architecture (anchor format, witness role, unrolling). (b) Build **witness service**: subscribes to SpaceKit proof/commitment stream, submits anchors to one or more target chains (BTC/ETH/Solana), optional verification contract on target. | Cross-chain anchoring live; enterprise pitch; ASTRA demand for witness operators. |
| **5. Storage Node spec only** | Add Storage Node section to architecture (and reference in protocol spec). No new code in RouteKit; Storage Node implementation already exists. | Spec complete; operators and integrators have one place to read. |

So the order is: **RouteKit node → Messaging integration → Encrypted envelope → Witness (spec then service)**. Storage Node is spec-only in parallel or right after Messaging.

### Dependency sketch

```
RouteKit relay (no deps on Messaging/Storage/Witness for v1)
    │
    ├──► Messaging integration (intent status, agent alerts)
    │         └── Messaging Node already exists
    │
    ├──► Encrypted envelope (relay + Compute Node + spacekit-js)
    │
    └──► Witness: spec first, then cross-chain witness service
              └── Per-block Verkle already in Compute Node / spacekit-js
              └── New: witness operator service + anchor format per chain
```

### What to build in which repo/crate

| Component | Where | Notes |
|-----------|-------|--------|
| RouteKit relay | `routekit/` (this repo) | Rust; `src/main.rs`, router, providers, prices, intent validation. |
| Intent status over Messaging | spacekit-compute-node (or relay) + spacekit-messaging-node | Publish events when intent state changes; define topic schema. Clients subscribe via existing Messaging Node API. |
| Encrypted envelope | spacekit-js (build), routekit (forward), spacekit-compute-node (decrypt) | Shared envelope format in intent-schema or protocol spec. |
| Witness protocol spec | ARCHITECTURE.md or new WITNESS-PROTOCOL.md in spacekit repo | Anchor format, witness operator behaviour, unrolling. |
| Cross-chain witness service | New crate e.g. `spacekit-witness` or module in compute-node | Subscribes to proof stream; submits to BTC/ETH/Solana; optional verification contracts. |
| Storage Node spec | ARCHITECTURE.md (or network doc) | Describe existing storage-node; no new crate. |

---

## Summary

- **Fill spec gaps** by adding formal sections for Messaging Node, Storage Node, and Witness protocol in the protocol spec and architecture doc, using the outlines in Part 1. Messaging and Storage are implemented; Witness has per-block Verkle done and cross-chain witness operators specified then built.
- **Build order:** (1) **RouteKit node** first. (2) **Messaging integration** for intent status and agent notifications. (3) **Encrypted intent envelope** for institutional security. (4) **Witness protocol** — spec then **cross-chain witness service**. (5) **Storage Node** — spec only. That sequence fills the spec gaps and delivers the components in dependency order.
