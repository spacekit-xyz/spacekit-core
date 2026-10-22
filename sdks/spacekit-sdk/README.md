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

### Credentials for apps

Apps should never carry the viewer's session. Give the host app-scoped credentials instead:

```ts
import { createStorageAuthClient, createLocalStorageEmbedHost } from "@spacekit/sdk/embed";

// Viewer holds an Ed25519 did:key: sign the storage node's login challenge once,
// then mint per-app tokens that only reach that app's documents.
const storageAuth = createStorageAuthClient({
  storageOrigin: "https://storage.example.com",
  getSigner: async () => (signedIn ? { did, publicKeyHex, sign: (m) => ed25519.signAsync(m, privateKey) } : null),
});
const host = createLocalStorageEmbedHost({
  appCredentials: storageAuth.appCredentials,
  forwardViewerSession: false, // once your API issues app tokens (apiAuthorization)
});
```

A host with its own API adds `apiAuthorization`, for example from the website-api's `POST /api/auth/app-token`.

`storageAuth.appCredentials` also answers `getAppCredentials({ namespace: "viewer" })` with `viewerStorageAuthorization`, a token in the viewer's own namespace. With it, an app's `storage.putBlob` / `getBlob` / `deleteBlob` / `blobInfo` keep the viewer's files on the storage node under the viewer's DID, and `storage.*` records go there too when the manifest declares the `storage` permission (otherwise they stay on the device). To let apps encrypt those files, pass `deriveAppKey`: the host derives a per-app key from the viewer's identity secret, and `crypto.encryptUpload` / `decryptBlob` run in the host, so no key enters the app frame.

```ts
import { deriveAppDataKey } from "@spacekit/sdk/embed";

const host = createLocalStorageEmbedHost({
  appCredentials: storageAuth.appCredentials,
  deriveAppKey: async (req) => (signedIn ? deriveAppDataKey(identitySeed, req) : null),
});
```

For viewers signed in to your API rather than holding a `did:key`, pass `getSession: websiteStorageSession({ apiBase, getBearer })` instead of `getSigner`. Paid apps need two hooks. Prices are in ASTRA, SpaceKit's only currency; apps call `payments.subscribe({ amountAstra: "2.5" })` (or `amountWei`).

- `requestSubscriptionPayment: createAstraPaymentRequester({ confirm, transfer })` shows the viewer the price and payee, then sends the ASTRA transfer on the SpaceKit chain (for example with spacekit-js `ChainClient.transfer`). It never holds keys.
- `recordSubscription: createSubscriptionRecorder({ apiBase, authorization })` has a server verify the transaction on the chain before the subscription is recorded.

### Signed packages

Publishers sign with `spacekit app package … --sign-key <seed-file>`. Hosts check the signature in their trust policy:

```ts
trustPolicy={allowPublishers(["did:key:z6Mk…"])} // must be signed by a listed key
```

## Plugins

A plugin is a feature one author publishes and others add to their apps: a chat panel, a paid newsletter, an agent, a trained brain. `@spacekit/sdk/plugins` validates descriptors (`spacekit-plugin.json`), applies the install rules, and loads brain plugins. The full specification is `sdks/spacekit-apps/SPACEKIT-PLUGIN-SPEC.md`.

```ts
import {
  parsePluginDescriptor, describePlugin, checkPluginPermissions,
  pluginCapabilityPolicy, pluginInstallScope, resolvePluginConfig,
} from "@spacekit/sdk/plugins";

const plugin = parsePluginDescriptor(descriptorJson);
const lines = describePlugin(plugin);                     // plain-language install screen
const check = checkPluginPermissions(plugin.permissions, hostApp.manifest.permissions);
if (check.missing.length) throw new Error(`Needs: ${check.missing.join(", ")}`);
const settings = resolvePluginConfig(plugin, ownerSettings);
const scope = pluginInstallScope({ pluginAppId, hostAppId, hostOwnerDid });

mountSpacekitApp(el, {
  appId: scope.pluginAppId,
  install: { collectionScope: scope.collectionScope, ownerDid: scope.ownerDid },
  capabilities: pluginCapabilityPolicy({ check }),
  acquireBridge: (id, origin, _name, cs) => acquireAppDataSdkBridge(services, http, id, origin, cs),
  /* storageOrigin, services, trustPolicy, requestPermissions … */
});
```

The plugin gets no more than the host app has, its data lives in the app owner's namespace (one scope per install), and it cannot take payments from the app's viewers.

Brains (Growformer and others) are plugins of kind `brain`: data, pinned by SHA-256, loaded only after every file verifies.

```ts
import * as spacekit from "@spacekit/spacekit-js";
import { loadBrainPlugin, growformerEngine, httpBrainFileFetcher } from "@spacekit/sdk/plugins";

await spacekit.initGrowformerHost();
await loadBrainPlugin(brain, {
  engine: growformerEngine(spacekit),
  fetchFile: httpBrainFileFetcher((f) => `${apiBase}/api/files/${f.path}`),
});
```

`spacekit-plugin init | validate | hash | sign` scaffolds, checks, hashes and signs descriptors.

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
| `@spacekit/sdk/plugins` | Plugin descriptors, install rules, brain loading, descriptor signatures |
| `@spacekit/sdk/astra` | ASTRA amounts: `parseAstra`, `formatAstra`, `parseWei` |
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
