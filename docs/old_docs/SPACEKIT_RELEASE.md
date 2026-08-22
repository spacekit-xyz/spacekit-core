# SpaceKit Browser-VM Demo Release

> Owner: SpaceKit Team  
> Scope: `spacekit-js` Browser VM demo + `@spacekit.xyz-website` Playground + `@spacekit/sdk`  
> Last updated: 2026-01-28

---

## 0) Production 1.0 Release Plan

### Phase 1: Critical Security & Build (1-2 days)

#### spacekit-js
- [x] Fix signature verification in `genesis.ts:301, 318` ✅ (2026-01-28)
- [x] Add LICENSE file (MIT) ✅ (2026-01-28)
- [x] Update package.json (remove `private: true`, add license, repository, keywords, engines) ✅ (2026-01-28)

#### spacekit-sdk
- [x] Add build scripts (typecheck, lint, test stubs) ✅ (2026-01-28)
- [x] Update package.json (version 1.0, metadata, peerDependenciesMeta) ✅ (2026-01-28)
- [x] Add root README.md ✅ (2026-01-28)
- [x] Add LICENSE file ✅ (2026-01-28)
- [x] Add custom error classes (`SpacekitError`, `ValidationError`, `NetworkError`, `VmError`, `CryptoError`, `StorageError`) ✅ (2026-01-28)

#### spacekit.xyz-website
- [x] Add CSP headers to index.html ✅ (2026-01-28)
- [x] Update package.json version to 1.0.0 ✅ (2026-01-28)
- [x] Configure Vite build optimizations ✅ (2026-01-28)
- [x] Add production logger utility ✅ (2026-01-28)
- [x] Fix mobile file upload silent failures (FileStorage + Video) ✅ (2026-01-28)
- [x] Hide LLM/Kit AI in BrowserOS on mobile (desktop only) ✅ (2026-01-28)
- [x] Add LLM temperature/maxTokens controls ✅ (2026-01-28)
- [x] Hide NET for demo accounts (Alice/Bob) ✅ (2026-01-28)
- [x] Add HEIC to JPEG conversion with progress modal ✅ (2026-01-28)
- [x] Add MOV video compression with ffmpeg.wasm + progress modal ✅ (2026-01-28)

### Phase 2: Quality & Testing (3-5 days)

#### spacekit-js
- [ ] Add test framework (vitest)
- [ ] Unit tests for core VM operations
- [ ] Integration tests for JSON-RPC endpoints
- [ ] Target: >80% coverage for core modules

#### spacekit-sdk
- [x] Add input validation for DIDs, balances, contract IDs ✅ (2026-01-28)
- [x] Fix silent error swallowing in SpacekitClient.ts (added debug/warn logging) ✅ (2026-01-28)
- [x] Add proper network error handling in tokens.ts ✅ (2026-01-28)
- [ ] Unit tests for encoding utilities
- [ ] Unit tests for token adapters

#### spacekit.xyz-website
- [ ] Remove/gate console.logs (155 instances) — logger utility created, migration pending
- [ ] Add error logging service integration
- [ ] E2E smoke tests for critical flows

### Phase 3: Documentation & Polish (2-3 days)

#### All packages
- [ ] Add CHANGELOG.md
- [ ] Complete package.json metadata (homepage, bugs, contributors)
- [ ] Add CONTRIBUTING.md

#### spacekit-js
- [ ] Generate API docs (TypeDoc)
- [ ] Add migration guide

#### spacekit-sdk
- [ ] Document all error types
- [ ] Add troubleshooting guide
- [ ] Add examples directory

### Phase 4: Publishing (1 day)

- [ ] Publish `@spacekit/spacekit-js` to npm
- [ ] Publish `@spacekit/sdk` to npm
- [ ] Update website to use published packages
- [ ] Deploy website to production
- [ ] Tag release and publish notes

### Production Readiness Scorecard

| Component | Current | Target | Status |
|-----------|---------|--------|--------|
| spacekit-js | 8.5/10 | 9/10 | 🟢 Phase 1 Complete (sig verify, LICENSE, package.json) |
| spacekit-sdk | 8.5/10 | 9/10 | 🟢 Phase 1 Complete (errors, validation, network handling) |
| spacekit.xyz-website | 9/10 | 9/10 | 🟢 Phase 1 Complete (HEIC, MOV, mobile, LLM controls) |

---

## 1) Browser‑VM Demo Release Checklist (MVP)

### Build + artifacts
- [ ] Run `npm run build` in `spacekit-compute-node/spacekit-js`
- [ ] Run `npm run demo:browser` to update demo bundles
- [ ] Verify `contracts/artifacts/*.wasm` are present and served
- [ ] Confirm SW cache list includes WASM + JS/CSS assets

### Hosting + TLS
- [ ] HTTPS enabled (WASM + storage APIs require secure context)
- [ ] `.wasm` served with `application/wasm`
- [ ] Long‑cache static assets (JS/CSS/WASM/images) with cache‑busting hashes
- [ ] Cloudflare rules confirmed (cache + security headers)

### Compute‑node (if remote mode enabled)
- [ ] JSON‑RPC is reachable (`/rpc`) with correct CORS allowlist
- [ ] WS endpoint enabled (optional for live updates)
- [ ] Rate limits + API keys/JWT configured
- [ ] Validate `vm_deploy`, `vm_submit`, `vm_mine`, `vm_blocks`

### Storage‑node (optional)
- [ ] `spacekit-storage-node` deployed and reachable
- [ ] `/api/documents` access works with DID auth
- [ ] CORS allowlist + API key policy verified
- [ ] Test `StorageNodeAdapter` sync + WASM fetch by doc ID

### UI defaults
- [ ] Default RPC/WS/Storage URLs are correct
- [ ] Seed balances behavior verified for demo wallets
- [ ] Fee policy displayed and consistent across tabs
- [x] Metering toggle + gas limit controls reviewed
- [ ] Metering stats panel verified (total/avg/last gas)
- [ ] Header sync badges verified (Dashboard + Explorer)
- [x] P2P tab themed with grouped panels + tabs
- [ ] Testnet faucet button verified (RPC + REST)

### QA
- [ ] Chrome / Firefox / Safari smoke tests for WASM + IndexedDB
- [ ] Local VM + remote compute‑node modes tested
- [ ] NFT mint/transfer + gallery verified
- [ ] Signed tx flow works (Ed25519; PQ optional)
- [ ] Storage sync + refresh behavior verified

### Release
- [ ] Tag release and publish notes
- [ ] Provide “Reset demo” playbook for support

---

## 2) Gaps To Flag (Immediate Issues)

These are acknowledged gaps that we should track and resolve.  
We will *defer DDoS work* due to Cloudflare protection.

- [x] Gas metering (WASM instruction counting) integrated (tuning pending)
- [x] Schema migration for IndexedDB block store (versioned meta)
- [x] Identity restore on session recovery (session metadata wired)
- [x] ErrorBoundary wrapping of Playground tabs
- [ ] Security audit / threat model (draft docs added)

### Immediate Issue Workstreams
- **Gas metering**: instruction‑level metering added via wasm‑metering; cost table tuning pending.
- **Schema migration**: versioned blockstore schema + migration path added (meta schema version).
- **Identity restore**: session metadata wired and restored on init.
- **Error boundaries**: wrap each Playground tab and add per‑tab reset hooks.
- **Security audit**: threat model + audit scope drafts created; schedule internal review.

---

## 3) Testnet Connectivity — Full Browser ↔ Testnet Sync

### Goal
Allow browsers to **join the public SpaceKit Testnet** and **sync chain state** in a trust‑minimized way, not just send RPC transactions.

### Current Status (Summary)
- ✅ JSON‑RPC and EIP‑1193 compatibility exist
- ✅ Storage node sync adapter exists (cache‑first)
- ✅ Single Storage Node + Compute Node can run on AWS (initial deployment target)
- ✅ Header sync client + IndexedDB cache (Playground + `spacekit-js`)
- ✅ P2P networking for browser peers (Playground UI + simulator bridge)
- ✅ Browser validation + fork choice (Playground only)
- ✅ Snapshot verification + proof checks (Playground only)
- 🟡 Finality gating + header signature enforcement (Playground only)
- ✅ Faucet REST + JSON‑RPC wired in simulator + compute‑node
- ⬜ Production consensus / validator set / finality rules

### Network Groups + Access Control
- **Global testnet group (default)**: all browser nodes connect to a shared public testnet.
- **Custom groups (future)**: users create isolated testnet groups with allowlists.
- **Admission control**: compute‑node maintains allowlist/denylist for browser peers.

### Definition of “Full Sync” for Browsers
- Ability to discover peers and fetch blocks / headers
- Validate block headers and receipts (Merkle proofs)
- Sync state snapshots from trusted sources (or verified proofs)
- Handle forks (reorg rules) and stay consistent with testnet tip

---

## 4) Phase‑2 Execution Plan (True Browser Sync)

### Phase 2.1 — P2P Networking (WebRTC + WebSocket)
**Goal:** Let browser nodes discover peers and exchange blocks/headers.  
Deliverables:
- [ ] libp2p integration (or custom WebRTC signaling via WS)
- [ ] Peer discovery (mDNS for local + DHT for wide area)
- [ ] WebRTC transport for browser‑to‑browser
- [ ] WS transport for browser‑to‑server (bootstrap nodes)
- [x] Browser WebRTC handshake + data channel (client)
- [x] Bootstrap + signaling WS servers (simulator)
- [x] Peer connection management + scoring (Playground)
- [x] ICE server configuration (STUN/TURN inputs in Playground)
- [x] Peer list refresh + auto-connect (Playground)
- [ ] NAT traversal infra (STUN/TURN deployment + credentials)

### Phase 2.2 — Consensus + Fork Choice (PoA → PoS)
**Goal:** Define how a browser knows the canonical chain.  
Deliverables:
- [ ] PoA for initial testnet (validator set in genesis)
- [x] Block proposer signature checks (Playground header signer registry)
- [x] Finality quorum gating (Playground attestation quorum)
- [x] Fork choice rules + reorg handling (Playground)
- [ ] Validator registration contract (future PoS)

### Phase 2.3 — State Sync + Snapshots
**Goal:** Enable a browser to sync chain state efficiently.  
Deliverables:
- [x] Snapshot format (state root + chunk hashes)
- [x] Merkle proof format for state entries
- [x] Snapshot download protocol
- [x] Incremental sync (delta updates, chunk diff + apply delta)
- [x] Resume support after interruption

### Phase 2.4 — Block Propagation + Validation
**Goal:** Reliable block exchange and verification.  
Deliverables:
- [x] Block announce + request/response protocol (Playground)
- [x] Block/header validation pipeline (tx/receipt roots + block hash)
- [x] Peer block/header cache (local persistence)
- [x] Snapshot stateRoot check for peer cache (Playground)
- [x] State proof verification UI (Playground)
- [x] Peer chain store + canonical tip selection (Playground)
- [x] Fork-choice + reorg detection (Playground)
- [x] Receipt proof verification UI (Playground)
- [x] Attestation registry + auto-verify (Playground)
- [x] Header signature verification (Playground)
- [x] Canonical chain apply + replay list (Playground)
- [x] Canonical replay + reorg diff (Playground)
- [x] Peer cache pruning controls (Playground)
- [x] Orphan block handling + retry (Playground)
- [x] Apply canonical chain to Explorer (read-only)
- [x] Orphan block handling + reorg workflow (Playground)
- [x] Cache and retry strategy (Playground)

### Phase 2.5 — Security and Resilience
**Goal:** Prevent abuse and ensure deterministic execution in browsers.  
Deliverables:
- [ ] Rate limits and mempool limits
- [ ] Host ABI determinism rules
- [ ] Sandbox quotas + memory caps
- [ ] Basic threat model + audit plan

---

## 5) Testnet Sync Milestones

- **M1: Remote Testnet Mode (RPC only)**  
  Browsers can submit + query via compute‑node RPC. (Already possible.)

- **M2: Header Sync + Proof Verification**  
  Browser syncs headers + verifies receipts with Merkle proofs. (UI added)

- **M3: Snapshot Sync**  
  Browser pulls verified state snapshot and becomes a light client. (Snapshot verify UI added)

- **M4: Full Browser Sync**  
  Browser participates in block propagation and verifies chain tip.

## 7) Testnet Sync Next Steps
- [x] Header cache stored in IndexedDB (per network)
- [x] Snapshot chunk hash verification UI
- [x] P2P bootstrap handshake (WS + signaling + allowlist)
- [x] WS hello‑ack + peer list parsing
- [x] Snapshot verification resume support
- [x] Snapshot retry backoff + progress indicator
- [x] Header cache keyed by chainId (RPC)
- [x] Snapshot progress stored in IndexedDB
- [x] Snapshot download + apply (client)
- [x] WebRTC signaling + data channel wiring (client)
- [x] Bootstrap + signaling WS bridge (simulator)
- [x] Default Playground WS URLs set to local bridge (9050/9051)
- [x] P2P activity feed + peer status table (Playground)
- [x] WebRTC block announce/request/header messaging (Playground)
- [x] Validate received blocks/headers in Playground
- [x] Persist peer cache for follow-up validation
- [x] Peer cache revalidation controls (Playground)
- [x] Snapshot stateRoot check against peer cache
- [x] State proof verification for peer cache (RPC)
- [x] Peer chain store + canonical tip view (Playground)
- [x] Fork-choice + reorg tracking (Playground)
- [x] Receipt proof verification against cached headers (Playground)
- [x] Attestation registry + auto-verify + header signature checks (Playground)
- [x] Canonical chain apply + applied set (Playground)
- [x] Canonical replay + reorg diff (Playground)
- [x] Peer cache pruning + retention config (Playground)
- [x] Orphan handling + retry requests (Playground)

### Local P2P Bridge
- Run `cargo run -p spacekit-simulator --bin p2p_bridge -- --host 0.0.0.0 --bootstrap-port 9050 --signaling-port 9051`
- Set Playground P2P WS URLs to `ws://localhost:9050` and `ws://localhost:9051` (defaulted)

---

## 6) Notes / Decisions

- **Cloudflare** will handle network‑layer protection for the demo site.
- **Gas metering** is the top technical blocker for true production readiness.
- **Security audit + threat model** must be completed before a public mainnet‑style launch.
- **Unified consensus** remains compute‑node‑only; browser VM acts as a light client with proof verification.

---

## 8) What’s Left (Next to Implement)
- Metering stats panel validation (avg/total/last gas)
- Header sync badge verification (Dashboard + Explorer)
- P2P discovery + NAT traversal infra (STUN/TURN servers + credentials)
- Deploy faucet endpoint on AWS compute-node testnet
- Production consensus rules (proposer rotation, finality, validator set)
- Security audit + threat model review
