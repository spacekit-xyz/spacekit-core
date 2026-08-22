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

## Exports

| Module | Description |
|--------|-------------|
| `@spacekit/sdk` | Main entry - SpacekitClient, tokens, kyber, encoding |
| `@spacekit/sdk/client` | SpacekitClient singleton |
| `@spacekit/sdk/tokens` | ERC-20 and ERC-721 token adapters |
| `@spacekit/sdk/kyber` | Kyber post-quantum encryption |
| `@spacekit/sdk/encoding` | Binary encoding utilities |
| `@spacekit/sdk/react` | React hooks and provider |
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
- React 18+ (for React integration)
- Modern browser with WebAssembly support

## License

MIT
