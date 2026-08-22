# SpacekitJS Demo Deployment Checklist

## 1) Build + artifacts
- `npm ci` and `npm run build` in `spacekit-js`.
- Ensure demo bundles are up to date: `npm run demo:browser`.
- Verify `contracts/artifacts/*.wasm` are present and served.
- Confirm service worker cache list matches deployed asset paths.

## 2) Hosting + TLS
- Serve over HTTPS (WASM + storage APIs require secure context).
- Configure correct `Content-Type` for `.wasm` (`application/wasm`).
- Set caching headers for static assets (JS/CSS/WASM/images).

## 3) Compute-node (RPC + WS)
- Deploy `spacekit-compute-node` with:
  - JSON-RPC enabled (`/rpc`) + CORS allowlist.
  - WS endpoint enabled if using live updates.
  - Rate limits and API keys or JWT configured.
- Validate `vm_deploy`, `vm_submit`, `vm_mine`, `vm_blocks` are reachable.

## 4) Storage-node (optional)
- Deploy `spacekit-storage-node` and configure:
  - `/api/documents` access with DID auth.
  - CORS allowlist and API key policy.
- Test `StorageNodeAdapter` sync + WASM fetch from doc IDs.

## 5) Security + wallet hygiene
- Decide if PQ signatures are required or optional.
- Provide user warnings on browser storage risk.
- If using extension wallet, ensure passphrase + key rotation flows are enabled.

## 6) UI defaults
- Confirm default RPC URL, WS URL, and storage-node URL are correct.
- Decide if “Mint + Mine” should be default for NFT flows.
- Verify fee policy visibility and seed balances behavior.

## 7) Observability
- Add error reporting (Sentry or similar).
- Enable compute-node metrics (Prometheus/Grafana).
- Track demo usage events (optional analytics).

## 8) QA checklist
- Chrome + Firefox + Safari smoke tests for WASM/IndexedDB.
- Test local VM + remote compute-node modes.
- Confirm NFT mint/transfer + gallery correctness.
- Confirm signed tx flow (Ed25519 + optional PQ).
- Verify storage sync and refresh behavior.

## 9) Release
- Pin versioned assets or include cache-busting hashes.
- Tag release and publish deployment notes.
- Provide a “Reset demo” playbook for support.
