# spacekit-anchor

Tamper-proof **content-hash timestamps** for Cairn notes, Quay files, and other apps.

The contract never sees plaintext — only a 64-char hex SHA-256 digest plus an opaque `note_id`.

## Build

```bash
cd spacekit-standard-library
cargo build -p spacekit-anchor --target wasm32-unknown-unknown --release
# artifact: target/wasm32-unknown-unknown/release/spacekit_anchor.wasm
```

Deploy with `spacekit contract deploy` and set `VITE_SPACEKIT_ANCHOR_CONTRACT_ID` in the website `.env`.

## Wire format (little-endian)

| Op | Opcode | Request body (after opcode) | Response |
|----|--------|----------------------------|----------|
| HEALTH | `0x10` | — | `{"status":"ok","agent":"spacekit-anchor","version":1}` |
| ANCHOR | `0x01` | `[note_id_len u16][note_id utf8][hash_len u16][hash_hex utf8]` | JSON anchor record |
| VERIFY | `0x02` | `[note_id_len u16][note_id utf8]` | Stored JSON or `{"ok":false,"error":"not_found"}` |

### ANCHOR response

```json
{"ok":true,"note_id":"abc","content_hash":"<64 hex>","timestamp":1700000000,"caller":"did:spacekit:…"}
```

Charges vault tier **50** (atomic units) per anchor via `payment_vault_charge`.

## Client example (TypeScript)

See `spacekit.xyz-website/src/utils/spacekitAnchorWire.ts` for `encodeAnchorCall` / `executeAnchorContract`.

```ts
const input = encodeAnchorCall(noteId, contentHashHex);
// POST compute node JSON-RPC: vm_execute { contractId, callerDid, inputBase64 }
```
