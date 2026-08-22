# SpaceKit.xyz Contract Language (SKCL 💀) v1.0

This is the v1 specification for the SpaceKit.xyz Contract Language (SKCL 💀).

## Goals
- Solidity‑inspired authoring experience
- Deterministic WASM output
- ABI compatibility with Solidity selectors/topics

## Syntax

```
contract ContractName

storage:
  field_name: type

events:
  EventName(arg: type, ...)

functions:
  fn_name(arg: type, ...) -> return_type @opcode N emit EventName require did
```

Notes:
- `@opcode` is optional (autoincrement if omitted).
- `emit EventName` is optional.
- `require did` enforces DID verification at runtime.

## Types
Supported in v1:
- `string`
- `address` (20‑byte hex string, `0x` prefix optional)
- `bool`
- `u64`
- `u128`
- `map<key,value>` (reserved; not supported in events or return types)
- `void`

## ABI Encoding (v1)
- Static types → 32‑byte ABI words (big‑endian)
- `string` → dynamic encoding: offset + length + padded bytes
- Function selector → Keccak‑256 first 4 bytes
- Event topic → Keccak‑256 of event signature
- Function input decoding for `u64`/`u128` currently reads little‑endian

## Events
Events are emitted with:
```
emit_event_bytes(signature, payload)
```
Payloads are ABI‑encoded according to the event signature.

## DID Gating
When `require did` is present:
- Generated code calls `get_caller_did_string`
- Generated code calls `verify_did_string`
- Runtime can enforce via `contract_policies.json`

## Outputs
The compiler generates:
- `src/lib.rs` (WASM contract)
- `Cargo.toml`
- `abi.json`
- `contract_policies.json`
- `contract_policies.merged.json`

## Versioning
This specification applies to `spacekit-contract-lang` **v1.0.0**.
