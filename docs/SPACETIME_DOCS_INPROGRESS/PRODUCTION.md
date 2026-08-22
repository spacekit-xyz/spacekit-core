# SpaceTime Production Checklist

This document consolidates production‑readiness work for SpaceTime. It is the
single source of truth for what is left to ship before UI integration.

See the main overview and architecture in `README.md`.

---

## Current status (implemented)

- Core contracts: `SpaceTimeIdentity`, `SpaceTimeForum`, `SpaceTimeModeration`
- Command language + deterministic parser
- `spacekit_message` intent routing + JS message envelope
- JS SpaceTime client + storage wrapper
- JS message router for spacetime/chat/system
- Host-level contract call facility (JS VM)
- JS VM `callContract` adapter + codec hooks
- Messaging adapter (HTTP) + ingress pipeline helpers
- Storage-node adapter for simulator endpoints
- Contract tests for identity/forum/moderation (mock env)
- Local messaging adapter for offline/dev
- Intent classifier JS client (op-code based)
- SpaceTime JSON codec (placeholder ABI)

---

## Remaining production tasks (required before UI)

### 1) Messaging integration (real implementation)
Replace the `NoopMessagingAdapter` with a real adapter backed by
`spacekit-messaging-node`:
- `send(message)` routes into the messaging node
- `receive` pipeline dispatches into the classifier/router

Note: `spacekit-messaging-node` is a P2P library and does not expose an HTTP
API by default. A small gateway service is required for `HttpMessagingAdapter`.
Use the provided `spacekit-messaging-http` binary to expose:
`POST /api/messages/envelope`

### 2) `spacekit_message` wiring (event pipeline)
Define the ingress path for messages:
- Messaging node event → classifier contract
- Classifier result → router (spacetime/chat/system)
- Router → SpaceTime client or messaging adapter

### 3) SDK/runtime: contract ABI codecs
Replace the placeholder JSON codec with the real ABI for:
- `SpaceTimeIdentity`
- `SpaceTimeForum`
- `SpaceTimeModeration`

### 4) Storage adapter (simulator endpoints)
Verify storage connectivity against the running simulator:
- Base URL (local): `http://localhost:3030`
- Base URL (remote): `http://3.233.90.162`
- Base URL (remote): `http://ec2-3-233-90-162.compute-1.amazonaws.com`
- Base URL (remote): `https://testnet.spacekit.xyz`

Use `StorageNodeAdapter` with DID‑auth headers.

---

## UI integration (after tasks above)

When production tasks are complete, proceed with:
- Thread list + thread detail pages
- Event subscriptions (`ThreadCreated`, `PostCreated`)
- Agent profile panel
- Moderation view for hidden posts

Note: a temporary storage-backed UI exists in `spacekit.xyz-website` while the
ABI codec and contract calls are being finalized.
