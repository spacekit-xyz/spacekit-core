# @spacekit/sdk

> React integration, token adapters, and encryption for SpaceKit-JS decentralized applications.

## Installation

### From GitHub
```bash
npm install @spacekit/sdk@github:spacekit-xyz/spacekit-sdk
```

### Peer Dependencies

This package requires `@spacekit/spacekit-js` and React in the host app:
```bash
npm install @spacekit/spacekit-js@github:spacekit-xyz/spacekit-js react react-dom
```

## Quick Start

### Client-Only Usage (No React)

```typescript
import { SpacekitClient } from '@spacekit/sdk';

// Initialize the singleton client
SpacekitClient.init();

// Set identity
const did = SpacekitClient.setIdentity('Alice');
console.log('Identity:', did); // did:spacekit:demo:alice

// Check balance
const balance = SpacekitClient.getBalance();
console.log('Balance:', balance);
```

### React Integration

```tsx
import { SpacekitProvider, useSpacekit } from '@spacekit/sdk/react';

function App() {
  return (
    <SpacekitProvider>
      <Wallet />
    </SpacekitProvider>
  );
}

function Wallet() {
  const { identity, balance, vm, ready } = useSpacekit();
  
  if (!ready) return <div>Loading...</div>;
  
  return (
    <div>
      <p>Identity: {identity?.did}</p>
      <p>Balance: {balance} ASTRA</p>
    </div>
  );
}
```

### Token Adapters

```typescript
import { Erc20Token, Erc721Token, setDefaultNetwork } from '@spacekit/sdk/tokens';

// Set network for DID expansion
setDefaultNetwork('testnet'); // or 'demo', 'mainnet'

// Deploy and use ERC-20 token
const token = await Erc20Token.deploy(vm, { name: 'SpaceUSD', symbol: 'SUSD' });
await token.mint('alice', 1000n);
await token.transfer('alice', 'bob', 500n);

const balance = await token.balanceOf('alice'); // 500n
```

### Kyber Encryption

```typescript
import { initKyber, generateKyberKeypair, encryptWithKyber, decryptWithKyber } from '@spacekit/sdk/kyber';

// Initialize WASM module
await initKyber();

// Generate keypair
const keypair = await generateKyberKeypair('kyber1024');

// Encrypt
const encrypted = await encryptWithKyber(
  new TextEncoder().encode('secret message'),
  keypair.publicKey
);

// Decrypt
const decrypted = await decryptWithKyber(encrypted, keypair.secretKey);
```

### Encoding Utilities

```typescript
import { encodeU64, encodeString, concatBytes, toHex } from '@spacekit/sdk/encoding';

// Build contract call input
const input = concatBytes([
  Uint8Array.of(1), // operation code
  encodeString('did:spacekit:demo:alice'),
  encodeU64(1000n),
]);

// Convert to hex for display
console.log('Input:', toHex(input));
```

## Hosting SpaceKit apps

Run published web packages (`.spkg`) inside your own site or network. Apps run in an isolated frame. They can't read your page's storage, cookies, session or DOM, and they reach the viewer's identity only through a permission-checked bridge. The wire protocol is specified in [`SPACEKIT-EMBED-PROTOCOL.md`](../spacekit-apps/SPACEKIT-EMBED-PROTOCOL.md).

### React

```tsx
import { SpacekitEmbeddedApp, allowPublishers } from "@spacekit/sdk/react/embed";

<SpacekitEmbeddedApp
  appId={appId}
  storageOrigins={["https://storage.example.com"]}
  fullscreen
  // Optional hardening for private deployments:
  capabilities={{ network: "manifest", trustedOrigins: ["https://api.example.com"] }}
  trustPolicy={allowPublishers(["did:key:z6Mk…"])}
  // Optional: real storage per app on a dedicated origin (see the protocol spec, section 8)
  // appOrigin="https://{app}.apps.example-usercontent.com"
/>
```

### Any framework (custom element)

```html
<script type="module">
  import { defineSpacekitAppElement } from "@spacekit/sdk/embed/element";
  defineSpacekitAppElement({ storageOrigins: ["https://storage.example.com"] });
</script>
<spacekit-app app-id="3fa0…c1" style="height: 640px"></spacekit-app>
```

### Plain JavaScript

```ts
import { mountSpacekitApp, createLocalStorageEmbedHost, acquireAppDataSdkBridge } from "@spacekit/sdk/embed";

const host = createLocalStorageEmbedHost();
const handle = mountSpacekitApp(document.getElementById("app")!, {
  appId,
  storageOrigin: "https://storage.example.com",
  services: host.services,
  acquireBridge: (id, origin) => acquireAppDataSdkBridge(host.services, host.httpHandler, id, origin),
  requestPermissions: async ({ manifest, permissions }) => confirm(`${manifest.name} asks to:\n${permissions.join("\n")}`),
});
// later: handle.unmount()
```

### Inside an app

```ts
import { getSpacekit } from "@spacekit/sdk/guest";
const sk = getSpacekit();
await sk.storage.set("highScore", 1200);
```

### Isolation modes

| `isolation` | What the app gets | Infrastructure |
|---|---|---|
| `"opaque"` (default) | Opaque origin. `localStorage` is shimmed onto the bridge. No IndexedDB or cookies. | None |
| `"origin"` | Its own real origin (optionally one per app), with full storage | An app origin serving `spacekit-frame.html`. Generate it with `npx spacekit-frame-host --allow https://your.site` |
| `"unsafe-same-origin"` | Your page's origin and everything on it | For your own code only |

Games keep `SharedArrayBuffer` / WASM threads in every mode, as long as the host page is cross-origin isolated (COOP `same-origin` + COEP `require-corp`).

### Upgrading from the pre-v1 frame

- `loadPackage` is ignored (blob-URL loaders cannot be isolated). Pass `loadFiles`, which returns verified files, for example with `verifyLocalPackageFiles(pkg, files)` for a local or encrypted cache.
- `createSpacekitServiceWorkerLoader` is deprecated. It served apps from the host origin.
- Apps no longer receive the session token (`identity.authHeaders`) or overwrite the signed-in DID. `http.fetch` attaches host credentials only to trusted origins: your page origin plus `capabilities.trustedOrigins` / `endpoints`.
- The React frame takes `theme`, `className` and `renderPermissions` for your own look and consent UI.

## Exports

| Module | Description |
|--------|-------------|
| `@spacekit/sdk` | Main entry - SpacekitClient, tokens, kyber, encoding |
| `@spacekit/sdk/client` | SpacekitClient singleton |
| `@spacekit/sdk/tokens` | ERC-20 and ERC-721 token adapters |
| `@spacekit/sdk/kyber` | Kyber post-quantum encryption |
| `@spacekit/sdk/encoding` | Binary encoding utilities |
| `@spacekit/sdk/react` | React hooks and provider |
| `@spacekit/sdk/embed` | Framework-agnostic app host (`mountSpacekitApp`), package loading and verification, capability policy |
| `@spacekit/sdk/embed/element` | `<spacekit-app>` custom element |
| `@spacekit/sdk/react/embed` | `SpacekitEmbeddedApp` / `SpacekitAppFrame` React components |
| `@spacekit/sdk/guest` | Types and accessor for `window.spacekit`, for use inside apps |
| `@spacekit/sdk/frame-host/spacekit-frame.html` | Frame-host page template for `isolation: "origin"` (placeholder host; regenerate with `spacekit-frame-host`) |
| `@spacekit/sdk/styles` | Default CSS styles |

## API Reference

### SpacekitClient

| Method | Description |
|--------|-------------|
| `init()` | Initialize the singleton |
| `setIdentity(name)` | Create/set identity, returns DID |
| `getCurrentDid()` | Get current identity DID |
| `getBalance(did?)` | Get ASTRA balance |
| `setBalance(did, amount)` | Set balance |
| `addBlock(did, block)` | Add block to explorer |
| `getExplorerSnapshot(did)` | Get explorer data |
| `subscribe(callback)` | Subscribe to events |

### Erc20Token

| Method | Description |
|--------|-------------|
| `deploy(vm, config)` | Deploy new token |
| `mint(to, amount)` | Mint tokens |
| `transfer(from, to, amount)` | Transfer tokens |
| `balanceOf(did)` | Get balance |
| `totalSupply()` | Get total supply |
| `metadata()` | Get token metadata |

### Erc721Token

| Method | Description |
|--------|-------------|
| `deploy(vm, config)` | Deploy new NFT collection |
| `mint(to, uri)` | Mint NFT, returns token ID |
| `transfer(from, to, id)` | Transfer NFT |
| `ownerOf(id)` | Get owner |
| `tokenUri(id)` | Get token URI |
| `getInfo(id)` | Get full NFT info |

## Requirements

- Node.js 18+
- React 18+ (only for the React entry points; `embed`, `embed/element` and `guest` have no React dependency)
- Modern browser with WebAssembly support

## License

MIT
