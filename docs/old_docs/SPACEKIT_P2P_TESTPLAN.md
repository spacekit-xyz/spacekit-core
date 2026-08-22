# Spacekit P2P Test Plan

## Scope
Validate end-to-end browser P2P bootstrap, signaling, WebRTC data channels, block/header exchange,
cache validation, fork-choice/reorg handling, orphan retries, pruning, and proof verification.

## Environments
- Local dev: `spacekit.xyz-website` + `spacekit-simulator` P2P bridge (ports 9050/9051)
- Optional: mobile client on same LAN for cross-device WebRTC
- Optional: remote bootstrap/signal endpoints for WAN testing

## Prerequisites
- Two browsers (or two devices) with different identities.
- `spacekit-simulator` P2P bridge running:
  - Bootstrap WS: `ws://localhost:9050`
  - Signaling WS: `ws://localhost:9051`
- Playground `/playground` accessible in both browsers.
- P2P tab visible (between AI/LLM and Settings).
- Confirm P2P sub-tabs render (Connection, Validation, State Sync, Attestations, Logs).

## Test Data
- At least 3-5 local blocks mined on one peer.
- At least one tx and receipt in the chain.
- Optional: fork scenario by diverging local chains on two peers.

## 1. Bootstrap WS
- Connect P2P on both peers.
- Verify `hello-ack` and `peer-list` are received.
- Verify peer list includes both DIDs.
- Disconnect/reconnect and confirm list refresh.
- Allowlist: add a DID mismatch and verify connection is rejected.
- Enable `Auto-refresh peer list` and confirm peer list updates without reconnect.

## 2. Signaling WS
- Connect signaling on both peers.
- Verify `signal-ack` logs appear.
- Send a WebRTC offer, verify answer received.
- Validate ICE candidates flow without errors.

## 3. WebRTC Data Channel
- Connect WebRTC from peer A to peer B.
- Verify channel state is `open`.
- Send ping and verify pong log.
- Disconnect and verify state updates to `closed`.
- Configure ICE servers (STUN/TURN) and verify connection still opens.

## 4. Block Announce / Request
- On peer A, announce latest block.
- On peer B, verify request is sent and block is received.
- Verify block is cached and appears in cache list.

## 5. Header Request
- On peer B, request latest header from peer A.
- Verify header is received and cached.
- If header includes signer + signature, verify header is marked invalid on bad signature.
- Verify attestation auto-emits on header receipt and appears in Attestations tab.

## 6. Validation Pipeline (Block/Receipt/Tx)
- Validate block/receipt roots recompute correctly.
- Use `Verify Cached Tx` and ensure txRoot matches.
- Use `Verify Cached Receipt` and ensure receiptRoot matches.
- Request `Peer Tx Proof` and verify against cached header.
- Request `Peer Receipt Proof` and verify against cached header.
- Click `Backfill Proofs` and confirm pending proofs are requested.
- Ensure invalid data shows `invalid` status and errors.
- Enable `Require header signatures` and confirm unsigned headers are rejected.

## 7. Snapshot Root Check
- Provide snapshot URL.
- Run `Verify Snapshot Root`.
- Confirm cache entries at snapshot height are marked valid/invalid.

## 8. State Proof (RPC)
- Provide key hex and (optional) header height.
- Run `Verify State Proof`.
- Ensure status matches expected validity.
- Run `Sample State Root` and confirm valid/invalid counts update.
- Enable `Auto-sample state root` and confirm periodic checks run.
- Enable `Sample on new canonical tip` and confirm a sample fires on new tip.
- Set `Sample Every N Blocks` and confirm sampling triggers after N block delta.
- Set `Proof Coverage Threshold` and enable `Auto-apply snapshot`.
- Verify snapshot auto-applies only after coverage meets threshold.
- Set `Auto-apply Min Delta` to a higher value and confirm it waits for N blocks.
- Enable `Gate manual apply on coverage` and confirm Apply buttons disable when below threshold.
- Apply snapshot twice and confirm second apply uses delta chunks (faster, fewer downloads).

## 9. Receipt Proof (RPC)
- Provide tx id.
- Run `Verify Receipt Proof`.
- Ensure status matches expected validity.

## 10. Fork Choice / Reorg
- Create competing chains from different peers.
- Confirm canonical tip updates to highest chain.
- Trigger reorg and verify reorg count increments.
- Verify `Replay Canonical` logs added/removed blocks.
- Enable finality quorum and confirm canonical selection requires trusted attestations.

## 11. Orphan Handling + Backoff
- Send a block whose parent is missing.
- Confirm it enters Orphan list and parent request is sent.
- Verify retry backoff triggers repeated requests until parent arrives.
- Once parent received, orphan is cleared.

## 12. Cache Persistence
- Reload browser and confirm cached headers/blocks load from localStorage.
- Ensure cache revalidation still functions after reload.

## 13. Pruning
- Set retention depth to small number.
- Run prune and verify older blocks/headers removed.
- Ensure canonical chain blocks remain.

## 14. Apply Canonical to Explorer
- Apply canonical chain to explorer (read-only).
- Validate Explorer data (blocks/txs/receipts) updates.
- Refresh page and ensure explorer state persists.

## 15. Apply Canonical to VM (Experimental)
- Switch to Local VM.
- Click `Apply to VM (Experimental)` and confirm reset dialog.
- Verify VM chain rebuilds and balances update.
- Verify failed txs are reported and do not break flow.
- Click `Inject into VM Store` and confirm prompt.
- Verify block store stats increase and explorer remains unchanged (state not applied).
- Click `Apply Snapshot to VM` and confirm balances + explorer update with snapshot state.

## 16. Error Handling / Resilience
- Disconnect bootstrap/signal servers and verify error UI.
- Simulate invalid JSON messages and confirm logs do not crash.
- Reconnect after failure without page reload.

## 17. Security / Abuse
- Try joining without allowlist when allowlist is enforced.
- Attempt to send oversized messages and confirm safe handling (no crash).
- Verify invalid proofs lower peer score and trigger quarantine.
- Add attestation JSON and toggle trusted state; confirm canonical selection respects `Require attestations`.
- Register signer pubkey and use `Verify Attestations` to mark trusted.
- Verify missing signature on signed header is rejected (signature required when signer present).

## Observability
- Confirm P2P activity log populates.
- Confirm unread badge increments when not in P2P tab.
- Confirm RTC peer table updates (status, channel state, last message).
- Confirm proof coverage percentages update as proofs resolve.
- Confirm apply gating message appears when sampling/attestation/coverage is unmet.
- Confirm attestation policy badges render in Validation tables.
- Confirm auto-connect peers creates offers when enabled.

## Exit Criteria
- All sections pass without crashes.
- P2P cache validation is deterministic across reloads.
- Fork choice and orphan handling behave predictably under reorg stress.