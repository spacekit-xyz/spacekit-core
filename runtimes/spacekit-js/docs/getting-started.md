# Getting Started

This guide helps you run a local browser VM, load contracts, and interact via RPC.

## Install

### From GitHub
```bash
npm install @spacekit/spacekit-js@github:spacekit-xyz/spacekit-js
```

### From source
```bash
git clone https://github.com/spacekit-xyz/spacekit-js.git
cd spacekit-js
npm install
npm run build
```

### Peer dependencies
```bash
npm install react react-dom
```

## Minimal VM usage
```ts
import { SpacekitVm } from "@spacekit/spacekit-js";

const vm = new SpacekitVm({ maxBlocksInMemory: 100 });
await vm.deployContract(fetch("/contracts/sk_erc20_contract.wasm"), "sk-erc20");
await vm.submitTransaction("sk-erc20", new Uint8Array([4]), "did:spacekit:demo:alice");
const block = await vm.mineBlock();
```

## Contract clients (opt-in)

Contract clients are imported from a separate subpath to keep the core lightweight:
```ts
import { SkErc20Client, SkErc721Client } from "@spacekit/spacekit-js/contracts";
```

## Quantum Verkle state root + proof
```ts
import { SpacekitVm } from "@spacekit/spacekit-js";

const vm = new SpacekitVm({
  quantumVerkle: {
    enabled: true,
    // Optionally override locations if you host WASM assets elsewhere.
    // Defaults resolve relative to the package's dist/wasm assets.
    moduleUrl: "/wasm/quantum_verkle_wasm.js",
    wasmUrl: "/wasm/quantum_verkle_wasm_bg.wasm",
  },
});

await vm.initQuantumVerkle();
const root = await vm.computeQuantumStateRoot();
const proof = await vm.getQuantumStateProof("0x...");
console.log(root, proof);
```

## Auto-mining
```ts
const stop = vm.startAutoMiner({ intervalMs: 2000, onlyIfPending: true });
// later: stop();
```
