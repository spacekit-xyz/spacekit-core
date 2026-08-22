# VM API

The `SpacekitVm` class manages contract deployment, transactions, blocks, proofs, and L2 execution modes.

## Core lifecycle
- `deployContract(wasm, contractId?)`
- `submitTransaction(contractId, input, callerDid, value?, signature?)`
- `executeTransaction(contractId, input, callerDid, value?)`
- `mineBlock()`

## L2 execution modes

### Simulate (read-only)
- `simulateCall(contractId, input, callerDid, value?)` — runs contract against a copy-on-write storage overlay; all writes are discarded, side effects suppressed, no fees charged
- Returns `{ status, result, events, gasUsed }`
- JSON-RPC: `vm_simulate`
- `ContractCallMode: "simulate"` via `createContractCaller`

### Relay (optimistic UX + L1 finalization)
- `setRelayRpcUrl(url)` — configure the L1 compute-node RPC endpoint
- `relayTransaction(contractId, input, callerDid, value?, signature?, options?)` — simulate locally, submit signed intent to L1, return optimistic result + `finalized` Promise
- Returns `{ optimistic, txId, finalized }` where `finalized` resolves to the L1 `Receipt`

### Events
- `vm.on("receipt:diverged", handler)` — fires when L1 result differs from local simulation
- `vm.off("receipt:diverged", handler)` — unsubscribe

### State sync
- `exportStateSnapshot()` — export full KV state as `StateSnapshot`
- JSON-RPC: `vm_snapshot`
- `HeaderSyncClient.syncStateSnapshot(storage, options?)` — pull canonical L1 state into a local storage adapter
- `HeaderSyncClient.pullStorageValue(keyHex, storage)` — fetch a single key from L1

## Block control
- `startAutoMiner({ intervalMs, onlyIfPending })`
- `stopAutoMiner()`

## State and proofs
- `computeStateRoot()` (internal)
- `computeQuantumStateRoot()` (Quantum Verkle root)
- `getQuantumStateProof(keyHex)` (Quantum Verkle proof)
- `verifyComputeNodeQuantumStateProof(proof, header?)` (stateless verification)
- `BlockHeader.quantumStateRoot` (anchor for stateless verification)
- `vm_txProof`, `vm_receiptProof`, `vm_stateProof` via JSON-RPC
- `vm_quantumStateRoot`, `vm_quantumStateProof` via JSON-RPC

## Sequencer
Use `SpacekitSequencer` to bundle blocks and export rollups:
- `mineAndBundle()`
- `flushBundle()`
- `signBundle()`
- `exportBundle()` / `exportSignedBundle()`
