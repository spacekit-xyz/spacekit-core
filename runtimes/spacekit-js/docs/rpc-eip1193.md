# RPC + EIP-1193

SpacekitVM exposes JSON-RPC for VM operations and an EIP-1193 provider bridge
via the browser extension.

## JSON-RPC
Available methods include:
- `vm_deployBatch`, `vm_submitBatch`
- `vm_submitSigned`, `vm_submitSignedBatch`
- `vm_txProof`, `vm_receiptProof`, `vm_stateProof`
- `vm_hostAbi`, `vm_feeEstimate`, `vm_gasEstimate`

HTTP server supports CORS and batching.

## EIP-1193 (extension)
An MV3 extension can inject `window.ethereum` with:
- `eth_requestAccounts`
- `personal_sign`
- `eth_signTypedData_v4`

The extension is optional and not shipped with the npm package. If you need
EIP-1193, build the extension from the repo and wire it to your app bundle.
