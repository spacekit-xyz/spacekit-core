# SpaceKit SDK

A React SDK for building decentralized applications with SpaceKit.

## Quick Start

```tsx
import { SpacekitProvider, useSpacekit } from './lib/spacekit-sdk';
import './lib/spacekit-sdk/spacekit-sdk.css';

function App() {
  return (
    <SpacekitProvider>
      <MyApp />
    </SpacekitProvider>
  );
}

function MyApp() {
  const { identity, balance, vm, explorer } = useSpacekit();

  return (
    <div>
      <h1>Hello, {identity.name}!</h1>
      <p>Balance: {balance.formatted} ASTRA</p>
      <p>Chain Height: {explorer.chainHeight}</p>
    </div>
  );
}
```

## Features

### Identity Management

```tsx
import { useIdentity } from './lib/spacekit-sdk';

function Profile() {
  const { did, name, setIdentity } = useIdentity();

  return (
    <div>
      <p>Name: {name}</p>
      <p>DID: {did}</p>
      <button onClick={() => setIdentity('Alice')}>Switch to Alice</button>
    </div>
  );
}
```

### Balance Tracking

```tsx
import { useBalance } from './lib/spacekit-sdk';

function Wallet() {
  const { formatted, microAstra, refresh, deductFee } = useBalance();

  return (
    <div>
      <p>{formatted} ASTRA</p>
      <p>{microAstra} µASTRA</p>
      <button onClick={refresh}>Refresh</button>
    </div>
  );
}
```

### Smart Contract Operations

```tsx
import { useVm } from './lib/spacekit-sdk';

function ContractDemo() {
  const { isReady, deployContract, submitAndMine } = useVm();

  const handleDeploy = async () => {
    const response = await fetch('/wasm/my_contract.wasm');
    const contractId = await deployContract(response, 'my-contract');
    console.log('Deployed:', contractId);
  };

  const handleTransaction = async (contractId: string) => {
    const input = new TextEncoder().encode('hello');
    const result = await submitAndMine(contractId, input, 'Say Hello');
    console.log('Block mined:', result?.block.height);
  };

  return (
    <div>
      <button onClick={handleDeploy}>Deploy</button>
    </div>
  );
}
```

### Block Explorer

```tsx
import { useExplorer } from './lib/spacekit-sdk';

function Explorer() {
  const { blocks, transactions, chainHeight } = useExplorer();

  return (
    <div>
      <p>Height: {chainHeight}</p>
      {blocks.map((block) => (
        <div key={block.blockHash}>
          Block #{block.height} - {block.transactions.length} txs
        </div>
      ))}
    </div>
  );
}
```

### Post-Quantum Encryption

```tsx
import { useKeys } from './lib/spacekit-sdk';

function Encryption() {
  const { hasKeys, generateKeys, encrypt, decrypt } = useKeys();

  const handleEncrypt = async () => {
    if (!hasKeys) await generateKeys();
    
    const data = new TextEncoder().encode('Secret');
    const encrypted = await encrypt(data);
    const decrypted = await decrypt(encrypted);
    console.log(new TextDecoder().decode(decrypted)); // "Secret"
  };

  return <button onClick={handleEncrypt}>Encrypt</button>;
}
```

## Pre-built Components

### SpacekitWallet

```tsx
import { SpacekitWallet } from './lib/spacekit-sdk';

<SpacekitWallet showActions />
<SpacekitWallet compact />
```

### SpacekitExplorer

```tsx
import { SpacekitExplorer } from './lib/spacekit-sdk';

<SpacekitExplorer maxBlocks={10} />
<SpacekitExplorer compact />
```

### SpacekitIdentityCard

```tsx
import { SpacekitIdentityCard } from './lib/spacekit-sdk';

<SpacekitIdentityCard allowSwitch />
<SpacekitIdentityCard compact />
```

## Provider Configuration

```tsx
<SpacekitProvider
  defaultIdentity="Alice"      // Default identity name
  storageMode="indexeddb"      // 'memory' | 'indexeddb'
  autoInitVm={true}            // Auto-initialize VM
  enableMetering={false}       // Gas metering
  gasLimit={1_000_000}         // Gas limit per tx
>
  {children}
</SpacekitProvider>
```

## Event System

```tsx
const { on, emit } = useSpacekit();

// Subscribe to events
useEffect(() => {
  const unsubscribe = on('block-mined', (event) => {
    console.log('New block:', event.data);
  });
  return unsubscribe;
}, [on]);

// Available events:
// - 'identity-change'
// - 'balance-change'
// - 'block-mined'
// - 'transaction-submitted'
// - 'contract-deployed'
// - 'keys-change'
// - 'vm-ready'
// - 'error'
```

## Theming

The SDK uses CSS custom properties for theming:

```css
:root {
  --spacekit-primary: #22d3ee;
  --spacekit-bg: rgba(2, 6, 23, 0.9);
  --spacekit-text: #e2e8f0;
  /* ... see spacekit-sdk.css for all variables */
}
```
