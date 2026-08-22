# hello-world

Minimal `spacekit_contract!` example: `handle` reads one **wire-encoded UTF-8 string** (`u16` little-endian byte length, then UTF-8 bytes) and returns `Hello, {name}!` as raw UTF-8 bytes (plus an event).

# Prerequisites
Signup for an account at [spacekit.xyz](https://spacekit.xyz) and install the [SpaceKit CLI](https://spacekit.xyz/docs).

Once the account is created, you can install the SpaceKit CLI and start using the hello-world contract. 

# Get Your DID
```bash
% spacekit did list
```
Results:
```bash
📋 Listing DIDs...

📊 DID Overview
━━━━━━━━━━━━━━━━━━━━━━━━
📁 Total DIDs: 1
🔐 Quantum-resistant: 1
👤 Owned by me: 1

🆔 My DIDs:
━━━━━━━━━━━━━━━━━━━━━━━━
  DID: "<YOUR DID>"
  Address: "<YOUR ADDRESS>"
  Algorithm: SPHINCS+ (Quantum-resistant)
  Status: ✅ Active
```

## Fund Account

```bash
% spacekit vm fund --owner-did "<YOUR DID>"
```

## Balance 

```bash
 % spacekit vm balance --owner-did "<YOUR DID>"
```
Results:
```bash
"<YOUR DID>" → balance 99996000
```

## Build Contract

```bash
% cargo build --target wasm32-unknown-unknown --release
```
## Deploying

```bash
% spacekit contract deploy \
--contract hello_world.wasm \
--name HELLOWORLD \
--owner-did "<YOUR DID>"
```

## Call from `spacekit` CLI

Use the reserved function name **`spacekit_handle`** with a JSON array of **one string** (the name to greet). That encodes the wire payload the contract expects.

```bash
spacekit contract call \
  --contract-id 0xdae9f2d2d8009d22f3ec835228bb34cf55fbb359 \
  --function spacekit_handle \
  --args '["World"]' \
  --caller-did did:spacekit:testnet:0x1aa6b39a086e67
```

Empty name (still valid wire):

```bash
spacekit contract call \
  --contract-id <CONTRACT_ID> \
  --function spacekit_handle \
  --args '[]' \
  --caller-did <CALLER_DID>
```

## Multiple / structured arguments

This sample only decodes **one string**. For several values you would extend `handle` to read more fields with `spacekit_contract_sdk::wire` (`read_string`, `read_u64`, …) and build matching payloads (or switch to a single JSON string you parse inside the contract and keep using `spacekit_handle` with one string arg).

For contracts that **parse the JSON envelope** themselves in `handle`, keep using normal `--function` / `--args` as JSON; the node sends `{"function","args"}` bytes in that case.
