# Proof Bridge: Pushing SpaceKit Proofs to Other Blockchains

This document outlines a design for an **adapter system** (and optional **separate service**) that takes proofs from the SpaceKit VM or its JSON-RPC API and submits them to external chains: **Ethereum**, **Bitcoin**, and **Solana**. Config can be loaded from a network URL or from local env/files.

**VM decimals and chain presets:** The VM’s native currency decimals (and symbol) are set via **genesis config** (`GenesisConfig.nativeCurrency` in `src/vm/genesis.ts`), not by the proof bridge. To align the browser-VM with a given network (BTC=8, ETH/SOL/ASTRA=18/9/18), use `getGenesisPresetForNetwork(chainId)` or `NETWORK_DECIMAL_PRESETS` when constructing the VM; entry-node and entry-bun use this when `SPACEKIT_CHAIN_ID` matches a known network. This is compatible with proof-bridge: you can run a VM with e.g. `chainId: "ethereum"` and decimals 18, and attach proof-bridge adapters to push to Ethereum L1.

**Examples:** See [docs/examples/PROOF_BRIDGE_EXAMPLES.md](examples/PROOF_BRIDGE_EXAMPLES.md) for example config JSON, env file, and code snippets. Example files: `docs/examples/proof-bridge-config.example.json`, `docs/examples/proof-bridge-service.env.example`.

**Current status:** Implemented — adapter interface and config in `src/vm/proof_bridge.ts`; config loader `loadProofBridgeConfig`, env substitution `substituteEnvInChainConfig`, and `createAdaptersFromConfig` in the same file; sequencer hook in `flushBundle()` and `exportSignedBundle()`; Ethereum adapter in `src/vm/adapters/ethereum.ts` (optional `viem`); Bitcoin adapter in `src/vm/adapters/bitcoin.ts` (indexer API; op_return/taproot stubbed); Solana adapter in `src/vm/adapters/solana.ts` (optional `@solana/web3.js`); Proof Bridge Service in `src/scripts/proof_bridge_service.ts` (run via `npm run proof-bridge-service`). Use `@spacekit/spacekit-js/adapters` to register default adapters.

---

## Goals

- **Produce once, publish elsewhere**: SpaceKit VM (or compute-node) produces Verkle/state proofs and rollup bundles; the same data can be published to one or more external chains for attestation, dispute resolution, or light-client bridges.
- **Pluggable per chain**: Each target chain has an adapter that knows how to encode and submit payloads (state root + proof, bundle hash, etc.) to that chain’s contracts or APIs.
- **Config from network or local**: Bridge behavior is driven by config that can be fetched from a URL (e.g. a SpaceKit network endpoint) or from local config (env vars, JSON file).

---

## Artifacts to Bridge

| Artifact | Description | Typical use on L1 |
|----------|-------------|-------------------|
| **State root** | Merkle or Quantum Verkle root for a block | Commit in contract / attestation |
| **State proof** | `StateProof` or `QuantumStateProof` (key → value + proof) | Verify and optionally store key/value on L1 |
| **Bundle summary** | `RollupBundle` (or signed): heights, block hashes, state roots, bundle hash | Rollup commitment / batch submission |
| **Tx / receipt proof** | `TxProof`, `ReceiptProof` | Prove inclusion for withdrawals or disputes |

Adapters can choose to submit only what the target chain supports (e.g. state root + bundle hash for Ethereum, OP_RETURN commitment for Bitcoin).

---

## Adapter Interface

A single interface allows the VM or sequencer to push to multiple chains without knowing chain-specific details. Implemented in `src/vm/proof_bridge.ts`.

```ts
// types for proof-bridge (could live in src/vm/proof_bridge.ts or a separate package)

export type ProofBridgePayload =
  | { kind: "state_root"; blockHeight: number; stateRoot: string; quantumStateRoot?: string }
  | { kind: "state_proof"; blockHeight: number; proof: StateProof }
  | { kind: "quantum_state_proof"; blockHeight: number; proof: QuantumStateProof }
  | { kind: "bundle"; bundle: RollupBundle }
  | { kind: "signed_bundle"; signed: SignedRollupBundle }
  | { kind: "tx_proof"; proof: TxProof }
  | { kind: "receipt_proof"; proof: ReceiptProof };

export interface ProofBridgeAdapter {
  readonly chainId: string; // "ethereum" | "bitcoin" | "solana" | custom

  /** Whether this adapter is configured and ready. */
  isReady(): boolean;

  /** Submit a payload. Returns tx id / commitment id or throws. */
  submit(payload: ProofBridgePayload): Promise<{ success: boolean; id?: string; error?: string }>;

  /** Optional: subscribe to VM/sequencer events instead of push. */
  start?(): Promise<void>;
  stop?(): Promise<void>;
}
```

- **`chainId`**: Identifies the target chain (and possibly network: e.g. `ethereum:mainnet`, `solana:devnet`).
- **`isReady()`**: So the orchestrator can skip disabled or misconfigured adapters.
- **`submit(payload)`**: Adapter encodes the payload for the chain and submits (contract call, transaction, API). Returns a minimal result so callers can log or retry.
- **`start`/`stop`**: Optional for adapters that run their own loop (e.g. polling RPC) instead of being pushed to.

---

## Config Model

Config drives which chains are enabled and how each adapter is built (RPC, contract addresses, keys, etc.).

```ts
export interface ProofBridgeChainConfig {
  enabled: boolean;
  chainId: string;

  // Ethereum
  rpcUrl?: string;
  contractAddress?: string;
  privateKeyHex?: string;        // or env var name, e.g. "SPACEKIT_ETH_SIGNER_KEY"
  gasLimit?: number;

  // Bitcoin (example)
  network?: "mainnet" | "testnet" | "signet";
  commitMethod?: "op_return" | "taproot" | "indexer_api";
  indexerUrl?: string;

  // Solana
  rpcUrl?: string;
  programId?: string;
  keypairPath?: string;          // or env var
  commitment?: "processed" | "confirmed" | "finalized";

  // Optional: only submit certain payload kinds
  payloadKinds?: ProofBridgePayload["kind"][];
}

export interface ProofBridgeConfig {
  /** Source of config: "inline" | "url" */
  source: "inline" | "url";
  /** If source === "url", fetch from this URL (JSON). */
  configUrl?: string;
  /** If source === "inline", use this. */
  chains?: Record<string, ProofBridgeChainConfig>;
  /** Poll interval (ms) when source === "url". */
  refreshIntervalMs?: number;
}
```

- **Loading from network**: When `source === "url"`, the client fetches `configUrl` (e.g. `https://api.spacekit.xyz/bridge-config` or a SpaceKit storage document). Response is JSON in the shape of `{ chains: { ... } }`. Optional `refreshIntervalMs` to refetch periodically.
- **Loading locally**: `source === "inline"` and `chains` provided from env, a local JSON file, or a build-time default. Env vars can hold secrets (e.g. `process.env.SPACEKIT_ETH_SIGNER_KEY`) and be referenced by name in config so keys are not in the fetched JSON.

---

## Integration Points

### 1. Sequencer hook (in-process)

Extend the sequencer so that after `flushBundle()` (and optionally `signBundle()`), it can push the bundle (and optionally derived proofs) to a list of adapters:

```ts
// SequencerOptions extended
interface SequencerOptions {
  maxBlocksPerBundle?: number;
  onBundle?: (bundle: RollupBundle) => void;
  proofBridgeAdapters?: ProofBridgeAdapter[];
}

// In flushBundle() or a new method (e.g. flushAndBridge()):
for (const adapter of this.proofBridgeAdapters ?? []) {
  if (!adapter.isReady()) continue;
  try {
    await adapter.submit({ kind: "bundle", bundle });
  } catch (e) { /* log, optionally retry */ }
}
```

Same idea can apply to signed bundles: pass `{ kind: "signed_bundle", signed }` after `signBundle()`.

### 2. VM / RPC event or callback (in-process)

For state-root or state-proof submissions on each block (or on demand), the VM could accept an optional list of adapters and, after each `mineBlock()`, call:

```ts
adapter.submit({ kind: "state_root", blockHeight, stateRoot, quantumStateRoot });
```

Alternatively, a small **ProofBridgeOrchestrator** in spacekit-js holds the adapters and subscribes to VM/sequencer events (if we add an event emitter) or is called explicitly by the app after mine/flush.

### 3. Separate service (out-of-process)

A **Proof Bridge Service** (separate Node/Bun process or container):

- **Input**: Reads from SpaceKit JSON-RPC (e.g. `vm_blocks`, `vm_quantumStateRoot`, or a dedicated `vm_bridgePayloads`), or from a SpaceKit storage node (e.g. rollup collection), or from a queue (Redis, etc.).
- **Config**: Loads from URL or file (same `ProofBridgeConfig` shape).
- **Logic**: Instantiates one adapter per chain, maps incoming proofs/bundles to `ProofBridgePayload`, and calls `adapter.submit(payload)`.
- **Benefit**: No change to VM/sequencer core; same adapters can be reused; keys and chain dependencies stay in one service.

In all cases, adapters share the same interface; only the **orchestration** (who calls `submit`) differs (in-process vs service).

---

## Per-Chain Adapter Sketches

### Ethereum

- **Payload**: State root (and optionally bundle hash) committed to a contract (e.g. `commitStateRoot(blockNumber, stateRootHash)`). State proof could be verified in-contract (if proof format is verifiable in Solidity) or verified off-chain and only a result stored.
- **Config**: `rpcUrl`, `contractAddress`, `privateKeyHex` (or env ref), `gasLimit`.
- **Implementation**: Use `ethers` or `viem` to call the contract. Adapter implements `ProofBridgeAdapter` and encodes payloads into the contract’s ABI.

### Bitcoin

- **Payload**: Commitment in a transaction: e.g. OP_RETURN (state root or bundle hash), or a Taproot script path that commits to the same. Alternatively, post to an indexer/API that stores commitments.
- **Config**: `network`, `commitMethod` (`op_return` | `taproot` | `indexer_api`), `indexerUrl` (if API), signing key or wallet.
- **Implementation**: Use a small Bitcoin lib (e.g. `bitcoinjs-lib`, or a REST client for an indexer). Adapter builds the tx or HTTP request and returns the tx id.

### Solana

- **Payload**: State root / bundle hash (and optionally proof) stored in an account or passed to a program. Could be a simple "commitment" account per block or a single account that gets updated.
- **Config**: `rpcUrl`, `programId`, keypair (path or env), `commitment`.
- **Implementation**: Use `@solana/web3.js` (and optionally `@coral-xyz/anchor`) to build and send the transaction. Adapter encodes payload into the program’s instruction format.

---

## Config Loading from Network

- **Endpoint**: e.g. `GET https://api.spacekit.xyz/v1/bridge-config` or a SpaceKit document URL. Returns JSON: `ProofBridgeConfig` (with `source: "inline"` and `chains` populated) or a wrapper `{ bridgeConfig: ProofBridgeConfig }`.
- **Auth**: If needed, use a bearer token or DID-signed request; keep secrets (e.g. private keys) out of the fetched config and use env var names instead.
- **Refresh**: If `refreshIntervalMs` is set, the client or service refetches and updates adapter config (or recreates adapters). Handles chain list or RPC URL changes without restart.

---

## Suggested Implementation Order

1. ~~**Types and interface**: Add `ProofBridgePayload`, `ProofBridgeAdapter`, and `ProofBridgeConfig` (e.g. in `src/vm/proof_bridge.ts` or a new `proof-bridge` package).~~ **Done** — see `src/vm/proof_bridge.ts`.
2. ~~**Config loader**: `loadProofBridgeConfig`, `substituteEnvInChainConfig`, and `createAdaptersFromConfig` (with optional adapter registry).~~ **Done** — `src/vm/proof_bridge.ts`.
3. ~~**Ethereum adapter**: Uses `viem` (optional dep), `commitStateRoot` / `commitBundle` contract calls.~~ **Done** — `src/vm/adapters/ethereum.ts`.
4. ~~**Sequencer integration**: Add `proofBridgeAdapters?: ProofBridgeAdapter[]` to sequencer options and call `adapter.submit({ kind: "bundle", bundle })` (and signed variant) after flush/sign.~~ **Done** — sequencer calls adapters in `flushBundle()` and `exportSignedBundle()`.
5. ~~**Bitcoin and Solana adapters**: Bitcoin indexer API in `bitcoin.ts`; op_return/taproot stubbed. Solana in `solana.ts` (optional `@solana/web3.js`).~~ **Done** — `src/vm/adapters/bitcoin.ts`, `src/vm/adapters/solana.ts`.
6. ~~**Optional Proof Bridge Service**: Loads config (URL or file), creates adapters, polls storage or RPC, submits to chains.~~ **Done** — `src/scripts/proof_bridge_service.ts`; run `npm run proof-bridge-service`.

This keeps the VM and sequencer agnostic of target chains while giving a clear path to "push proofs to Ethereum, Bitcoin, Solana" either in-process or via a separate service, with config from network or local.
