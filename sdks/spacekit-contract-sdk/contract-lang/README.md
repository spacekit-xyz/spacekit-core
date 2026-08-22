## SpaceKit.xyz Contract Language (SKCL 💀)

**Version:** v1.0

See `SKCL_V1.md` for the full specification.

This is the initial compiler for a SpaceKit.xyz contract language that outputs
SpaceKit WASM smart contracts using `spacekit-contract-sdk`.

### Syntax (minimal, v1 examples)

```
contract MyToken

storage:
  total_supply: u64
  name: string

functions:
  mint(to: address, amount: u64) -> bool emit Transfer
  balance_of(owner: address) -> u64
  total_supply() -> u64
```

### Build (from this repo root)

```bash
cargo run -p spacekit-contract-lang -- contract-lang/examples/astra_token.scl ../spacekit-compute-node/contracts
```

Or with an explicit manifest:

```bash
cargo run --manifest-path contract-lang/Cargo.toml -- contract-lang/examples/astra_token.scl ../spacekit-compute-node/contracts
```

**Examples:** v1 examples live under `examples/` (e.g. `astra_token.scl`).

This generates:
- `contracts/MyToken/Cargo.toml`
- `contracts/MyToken/src/lib.rs`
- `contracts/MyToken/abi.json`
- `contracts/MyToken/contract_policies.json`
- `contracts/contract_policies.json`
- `contracts/contract_policies.merged.json`

### Notes
This is the first pass of Option 2. It generates compile‑ready stubs, a dispatch
table (opcode‑based), and an `abi.json` for client tooling.

### Optional opcodes
You can pin opcodes with `@opcode`:

```
functions:
  mint(to: address, amount: u64) -> bool @opcode 1
  balance_of(owner: address) -> u64 @opcode 2
```

### Events
Events are recorded in `abi.json` for tooling:

```
events:
  Transfer(from: address, to: address, amount: u64)
```

Functions can emit events (fields must match function arg names):

```
functions:
  mint(to: address, amount: u64) -> bool emit Transfer
```

### DID-gated calls
Require DID verification per function:

```
functions:
  mint(to: address, amount: u64) -> bool emit Transfer require did
```

### Selectors and topics
Generated `abi.json` includes:
- `signature`: Solidity‑style signature string
- `selector`: Keccak‑256 first 4 bytes (hex)
- event `topic`: Keccak‑256 of event signature (hex)

### ABI encoding (production)
- Static types (bool/u64/u128/address) are encoded as 32‑byte words (big‑endian).
- `string` is encoded as dynamic: offset + length + padded bytes.
- `address` expects a 20‑byte hex string (e.g. `0x001122...`).
- Function input decoding currently reads `u64`/`u128` as little‑endian.

### Runtime integration
- Events emit via `emit_event_bytes(signature, payload)` with Keccak topics.
- Storage uses the `spacekit_storage` host module.
- DID gating uses `get_caller_did_string` + `verify_did_string`.

### Generated policy file
The compiler emits a `contract_policies.json` with `require did` selectors/opcodes.
Point the runtime at it:
```
export SPACEKIT_CONTRACT_POLICIES=contract_policies.json
```

Multiple compiles automatically merge into `contract_policies.merged.json` at the output directory.

For Solidity parity, the compiler emits a contract placeholder key:
- `$contract:ContractName` → replace with deployed address.

### Current limitations
- `map<...>` is reserved; events and return types must not use `map`.
