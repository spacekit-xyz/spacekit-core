# SpaceKitJS Technical Whitepaper v1.0
_Multi-runtime WASM VM & host SDK (browser · Node.js · Bun)_

**Version:** 1.0  
**Date:** January 2026  
**Project:** `@spacekit/spacekit-js`  
---

## Executive Summary
SpaceKitJS is a browser‑native virtual machine (VM) and host SDK for executing SpaceKit
WASM smart contracts. It provides deterministic execution, local state, JSON‑RPC + EIP‑1193
compatibility, and optional sync to decentralized infrastructure. It also includes a
light‑client toolkit for header sync, snapshot verification, and proof validation. SpaceKitJS
enables developers to run production‑grade smart contracts entirely in the browser while
maintaining cryptographic integrity, auditability, and interoperability with compute,
storage, and messaging nodes.
Quantum Verkle integration adds a post‑quantum, vector‑commitment‑based state root and
stateless proof verification path anchored to block headers.

---

## 1. Problem Statement
Most smart contract runtimes are server‑first and not optimized for browser execution.
This creates friction for rapid prototyping, client‑side verification, and offline/edge usage.
SpaceKitJS addresses:
- Browser‑first execution of WASM contracts
- Deterministic host ABI across environments
- Local chain state with portable rollups and proofs
- Secure sync to decentralized storage nodes

---

## 2. System Overview
SpaceKitJS is built around:
- **SpacekitVM**: A WASM runtime for contracts
- **Host ABI**: Deterministic WASM imports (`env`, `spacekit_storage`, `spacekit_agent`, `spacekit_tools`, `spacekit_messaging`, `spacekit_remote_storage`, `spacekit_payments`, token/reputation/fact modules, etc.—see §4)
- **Sequencer**: Rollup bundling, export, and proof support
- **JSON‑RPC + EIP‑1193**: Dapp compatibility and extension wallet integration
- **Header Sync Client**: Fetch, validate, and cache block headers
- **Snapshot + Proof Tools**: Verify state snapshots and receipt/state proofs
- **Quantum Verkle**: Post‑quantum state root + proofs via WASM

Key differentiator: **multi-runtime WASM execution** with a **single Host ABI** in `spacekit-js`. The **`spacekit-compute-node`** Rust VM aims for **incremental parity** on overlapping symbols (see `spacekit-compute-node/documentation/VM_PARITY.md` in the monorepo)—full parity across every import module is **not** guaranteed today.

---

## 3. Architecture
### 3.1 Core Components
- **SpacekitVM**: deploy/execute/mine lifecycle, block headers, receipts
- **Host Runtime**: provides deterministic host functions, storage, and logging
- **Storage Adapters**:
  - IndexedDB (local persistence)
  - StorageNodeAdapter (decentralized sync)
- **Header Sync**: RPC header fetch + IndexedDB cache
- **Snapshot Client**: chunked downloads, hash verification, resumable progress
- **P2P Playground Client**: WebRTC + WS signaling for peer exchange and validation
- **Quantum Verkle Bridge**: WASM loader + proof generation/verification

### 3.2 Execution Lifecycle
1. Deploy WASM contract
2. Submit transaction
3. Execute contract
4. Mine block
5. Compute Merkle roots + Quantum Verkle state root
6. Persist receipts, events, proofs
7. (Optional) Export rollup bundle + proofs to compute node

---

## 4. Host ABI and Contract Model

Contracts compile to **`wasm32-unknown-unknown`** and link against the **SpaceKit Host ABI** implemented in **`spacekit-js/src/host.ts`** (`createImports`). The list below is the **JS VM surface**; the Rust compute node registers a **subset** unless noted elsewhere.

### 4.1 Core and storage

| Module | Examples | Notes |
|--------|-----------|--------|
| **`env`** | `storage_*`, `log` / `log_output`, `emit_event`, `get_timestamp`, `msg_value`, `get_balance`, `transfer`, DID helpers, `abort` stub | Shared baseline with compute node where parity work has landed. |
| **`spacekit_storage`** | `storage_save`, `storage_load` | KV overlay used by contracts and tooling. |
| **`metering`** | `usegas` | Gas accounting hooks from metered WASM. |

### 4.2 Tokens, reputation, facts, cross-contract

| Module | Role |
|--------|------|
| **`sk_erc20`**, **`sk_erc721`** | Token/NFT helpers |
| **`spacekit_reputation`** | Scores / thresholds |
| **`spacekit_fact`** | Fact-package existence / hash checks |
| **`spacekit_contract`** | `contract_call` (depth-limited on JS when adapter present; stub on Rust today) |

### 4.3 Agents, tools, messaging, remote storage, payments

Modern agent contracts (e.g. **RouteKit**) use the **`spacekit-contract-sdk`** helpers that import:

| Module | Symbols (representative) |
|--------|---------------------------|
| **`spacekit_agent`** | `agent_growformer_*` (status, load brain from storage, generation, converse, codegen, brain info, reset) — **Growformer** path |
| **`spacekit_tools`** | `web_search` (effect / messaging-backed) |
| **`spacekit_messaging`** | `messaging_send` |
| **`spacekit_remote_storage`** | `remote_storage_put`, `remote_storage_get` |
| **`spacekit_payments`** | `payment_transfer`, `payment_vault_charge` |
| **`spacekit_session`** / **`spacekit_paymaster`** | Session keys + paymaster (ERC-4337–style); optional for many contracts |

These modules require **runtime adapters** on the JS side (`HostOptions`: `messaging`, `payment`, `remoteStorage`, tool operator DID, Growformer host initialization, etc.). Without adapters, host functions return **not-configured** or **pending** codes per `host.ts`.

### 4.4 Legacy / deprecated LLM module

| Module | Notes |
|--------|--------|
| **`spacekit_llm`** | Older `llm_inference` / `llm_status` style surface; **deprecated** in README in favor of **`spacekit_agent`** + Growformer for new work. |

### 4.5 `spacekit-contract-sdk` macros vs the compute node

- **`spacekit_contract!(MyContract)`** and the **`SpacekitContract`** trait live in **`spacekit-contract-sdk`** at **compile time**. They expand to WASM exports (`main`, `get_result`) and glue—not something the compute node “ships” as a library feature.
- **`spacekit-compute-node`** must instead provide matching **`linker.func_wrap(...)`** registrations for every **`wasm_import_module`** the compiled WASM uses.

**Example — `spacekit-standard-library/agents/routekit-agent`:** the crate imports SDK helpers (`growformer_*`, `payment_vault_charge`, `messaging_send`, `remote_storage_*`, `web_search`, etc.). Those resolve at link time to WASM imports under **`spacekit_agent`**, **`spacekit_payments`**, **`spacekit_messaging`**, **`spacekit_remote_storage`**, **`spacekit_tools`**, plus **`env`**. Today the Rust VM implements **`env`** (partial), **`spacekit_storage`**, **`spacekit_crypto`**, **`metering`**, and a **`spacekit_contract.contract_call` stub**—it does **not** generally expose the full **`spacekit_agent` / tools / messaging / remote_storage / payments`** surface RouteKit needs. **RouteKit-class WASM is intended to run in `spacekit-js` with adapters**, not “drop in” on the standalone Rust node without additional host work.

### 4.6 What is left to **run** SpaceKit-JS

Minimal path:

1. **Runtime:** Node.js **≥ 18** or **Bun**, or a modern browser.
2. **Install:** `npm install` in `spacekit-js` (see root `package.json`).
3. **Build:** `npm run build` (TypeScript + asset copy). Optional: `npm run build:growformer-wasm`, `npm run build:quantum-verkle-wasm` when you need those WASM assets locally.
4. **Server polyfills:** On Node/Bun, call **`installPolyfills()`** before IndexedDB-backed code if you rely on persistence (`README.md` — `SPACEKIT_IDB_BACKEND`, `SPACEKIT_IDB_PATH`).
5. **Start VM JSON-RPC (optional):** `npm run start:node` / `start:bun` (wraps build + `dist/entry-node.js` or Bun entry).
6. **Agent contracts:** Configure **`HostOptions`** — messaging adapter, payment adapter, remote storage, **`toolOperatorDid`** + **`effectManager`** resolution path for **`web_search`** / pending effects, and **Growformer** host init + brain assets (`growformer-pkg/`, `initGrowformerHost` patterns in JS docs).

Until those adapters and assets are wired for your deployment, contracts that call **`spacekit_tools`**, **`spacekit_messaging`**, **`remote_storage_*`**, **`payment_*`**, or **Growformer** will observe **stub / not-configured / pending** behavior—not a broken compiler.

ABI versioning for deterministic execution across environments remains **documentation + discipline**; cross-check `host.ts` and `VM_PARITY.md` when changing symbols.

---

## 5. VM API Surface
Core API:
- `deployContract(wasm)`
- `submitTransaction(contractId, input, did)`
- `mineBlock()`

Optional controls:
- Auto‑miner (`startAutoMiner`)
- Rollup bundling (`SpacekitSequencer`)
- Proof generation (`txProof`, `receiptProof`, `stateProof`)
- Quantum Verkle proofs (`computeQuantumStateRoot`, `getQuantumStateProof`)
- Block import (`importBlocks`) for blockstore injection and replay

---

## 6. Storage & Synchronization
SpaceKitJS supports local persistence and remote sync:
- **IndexedDB**: fast local persistence
- **Storage Node Sync**: optional remote archival and sharing
- **Conflict Resolution**: LWW (last‑write‑wins)
- **Header Cache**: IndexedDB header cache keyed by chainId
- **Snapshot Progress**: resumable snapshot verification via IndexedDB

This enables browser‑native chains with portability across devices and nodes.
Quantum Verkle enables stateless verification of key/value proofs against
`BlockHeader.quantumStateRoot` without requiring full state download.

---

## 7. JSON‑RPC + EIP‑1193 Compatibility
SpaceKitJS supports:
- JSON‑RPC for VM operations
  - `vm_deploy`, `vm_submit`, `vm_execute`, `vm_mine`
  - `vm_txProof`, `vm_receiptProof`, `vm_stateProof`
  - `vm_quantumStateRoot`, `vm_quantumStateProof`
- JSON‑RPC for sync
  - `vm_headers`, `vm_header`, `vm_snapshot`, `vm_stateProof`
- EIP‑1193 bridge for wallet integration

This provides compatibility with existing dapp tooling.

---

## 8. Security Model
### 8.1 Signing
- Ed25519 by default
- Post-quantum signing optional (SPHINCS+)

### 8.2 Determinism
Strict ABI + deterministic execution prevents divergence.

### 8.3 Wallet Security
Extension wallet encrypts keys with PBKDF2 + AES‑GCM.

### 8.4 Header Integrity
Headers can include signer metadata and signatures; clients verify signatures when metadata is present.
Quantum Verkle roots are embedded in headers to anchor stateless proofs to authenticated chain state.

---

## 9. AI & LLM Integration

### 9.0 Growformer and `spacekit_agent` (current direction)

Newer agents (**RouteKit**, advanced Growformer flows) call **`spacekit_agent`** imports (`agent_growformer_*`) implemented in `host.ts`, backed by the **Growformer** runtime (`growformer/runtime.ts`, optional `growformer-pkg/` WASM). Initialize brains via host APIs before expecting generation/converse/codegen to return ready status.

The legacy **`spacekit_llm`** module remains for older contracts but is **deprecated** for new development—prefer **`spacekit_agent`** per package README.

### 9.1 Legacy `spacekit_llm` and the reference `spacekit-agent` contract

SpaceKitJS still exposes **`spacekit_llm`** host functions for contract‑bound inference where contracts import that module.
The async/sync bridge uses precompute caching to safely provide LLM results to synchronous WASM calls. LLM integration is optional and can be disabled at build time.

The reference **`spacekit-agent`** contract (in **`spacekit-standard-library`**) uses
`llm_call(prompt, temperature, max_tokens, max_response_len)` for six operations:

| Op | Name        | Description |
|----|-------------|-------------|
| 1  | CHAT        | User message + optional conversation context → Kit response |
| 2  | ANALYZE     | Content → JSON safety/sentiment |
| 3  | SUMMARIZE   | Content → 2–3 sentence summary |
| 4  | CODE_REVIEW | Code → concise review (bugs, security, improvements) |
| 5  | CLASSIFY    | Content + categories → single category label |
| 6  | STATUS      | No LLM; returns host LLM status (not_loaded / ready / loading) |

The UI sends conversation context for CHAT so the model can answer in context; the contract
instructs the model to answer **only** the latest user message.

### 9.2 Internal Prompts (Contract-Side)
Prompts are defined inside the WASM contract so behaviour is auditable and consistent
across UIs. Temperature and max tokens are set per operation.

**CHAT (Kit identity and rules)**  
System prompt (abridged):

- Kit is the SpaceKit on-chain developer agent running in a WASM contract on the SpaceKit VM.
- Recommend **SpaceKit only**: WASM contracts, SpaceKit VM, SKCL, spacekit.xyz, and the SpaceKit ecosystem.
- Do **not** recommend Ethereum, Solidity, EVM, or other blockchains unless the user explicitly asks.
- For development: direct users to SpaceKit (Rust/WASM, spacekit-standard-library, docs).
- Keep answers clear and concise; do not repeat the same phrase.
- **Important:** The conversation payload may include previous turns. Answer **only** the last user message; ignore earlier User/Kit lines.

The contract then appends the conversation (including “Current question (answer only this):” when the UI sends context) and “Kit:” for completion. Parameters: temperature 40 (0.4), max 1024 tokens.

**ANALYZE**  
“Analyze the following content for safety and sentiment. Respond with JSON: {\"safe\": true/false, \"sentiment\": \"positive/negative/neutral\", \"reason\": \"brief explanation\"}”  
Temperature 30, max 128 tokens.

**SUMMARIZE**  
“Summarize the following content in 2-3 sentences.”  
Temperature 50, max 200 tokens.

**CODE_REVIEW**  
“Review the following code for bugs, security issues, and improvements. Be concise and specific.” (Content in fenced block.)  
Temperature 30, max 512 tokens.

**CLASSIFY**  
“Classify the following content into one of these categories: {categories}. Content: {content}. Respond with just the category name.”  
Temperature 20, max 50 tokens.

These prompts are the single source of truth in the contract; the whitepaper summarizes them for transparency and audit.

### 9.3 Future options
- WASM Asyncify
- Oracle pattern for inference requests

---

## 10. Deployment Considerations
### 10.1 Browser Deployment
- HTTPS required
- `application/wasm` content‑type
- Service worker cache strategy for offline use
- WebCrypto required for hashing/proof verification
- Quantum Verkle WASM assets must be served and reachable by `moduleUrl`/`wasmUrl`.
  Defaults resolve relative to the package’s bundled `dist/wasm` assets.

### 10.2 Compute Node Integration

SpaceKitJS can export rollup bundles for compute node validation to support long‑term archival and portability across devices and nodes.

**ABI expectation:** compute-node parity is tracked incrementally—do not assume every **`spacekit-js`** import exists on **`spacekit-compute-node`** without checking **`VM_PARITY.md`**. Agent-heavy WASM should default to the JS VM until Rust registers matching **`spacekit_agent`**, **`spacekit_payments`**, **`spacekit_messaging`**, **`spacekit_remote_storage`**, and **`spacekit_tools`** (or your deployment stubs).

---

## 11. Dependencies on Decentralized Infrastructure
SpaceKit-JS is designed to interoperate with:
- **SpaceKit Storage Node**: Zero‑knowledge, PQ‑encrypted storage and sync
- **SpaceKit Compute Node**: Rollup validation + block finalization
- **SpaceKit Messaging Node**: Event routing + P2P coordination

These dependencies enable decentralized state sync and long‑term archival while maintaining
browser‑native execution. Extension wallet integration is optional and outside the core VM.

---

## 12. Roadmap & Next Steps
- Gas metering tuning + cost model calibration
- Async host functions / Asyncify exploration
- Multi‑node consensus (PoA → PoS) with proposer signatures
- Incremental state sync (delta updates)
- Stateless sync enhancements (Quantum Verkle proof batching, snapshot anchoring)
- Stronger ACL enforcement for storage node
- Security audit + formal threat model

---

## References

- SpaceKitJS README (`spacekit-js/README.md`)
- Developer docs (`spacekit-js/docs/*`)
- **`spacekit-contract-sdk`** (`spacekit-contract-sdk/README.md`, `src/lib.rs`, `src/agent_host.rs`) — WASM import names and `spacekit_contract!` macro
- Rust VM parity inventory (`spacekit-compute-node/documentation/VM_PARITY.md`)
- SpaceKit Storage Node — see that crate’s `README.md` / documentation for storage-node specifics (PDF filenames vary by distribution)

