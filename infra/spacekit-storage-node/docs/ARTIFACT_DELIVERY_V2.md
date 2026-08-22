# Artifact delivery v2 — encrypt once, wrap key per user

## Problem

`/files/{id}/stream` and `/files/{id}/rewrap` previously:

1. Read the full on-disk envelope (~127MB)
2. Server-decrypt all AES chunks → plaintext
3. Build a brand-new envelope encrypted to the buyer/requester key

Peak RAM scales as `plaintext × concurrent_requests`. Access control was coupled to bulk re-encryption.

## On-disk format (unchanged)

Deploy already stores a **single** PQ envelope:

- **DEK** (32-byte file key) in the header, KEM-wrapped to the storage server key
- **Ciphertext** in fixed-size AES-GCM chunks after the header

No blob migration is required for existing brains.

## Fast path (v2)

1. If nested (double-wrapped), **peel outer layers on disk** chunk-by-chunk (O(chunk) RAM), atomically replace the blob with a single-layer envelope
2. Read header only from disk (bounded, ~few KB)
3. Server decapsulates **DEK only** (32 bytes)
4. Re-KEM DEK to requester/buyer Kyber PK (`pqcrypto-kyber1024`, browser-compatible)
5. Stream: `[new header] ++ file_stream(data_section from offset header_size)`

Memory per stream ≈ **64 KiB** (chunk size), not file size.

If header-only re-wrap is unavailable, fall back to **chunked** decrypt→re-encrypt streaming (still O(chunk)).

The legacy full-file decrypt/re-encrypt path is **fail-closed** above 4 MiB (`MAX_LEGACY_FULL_BUFFER_BYTES`) so large artifacts cannot OOM the node.

## Endpoints

| Route | Auth | v2 behavior |
|-------|------|-------------|
| `GET /files/{id}/stream` | Challenge-response | Peel nested → header re-wrap → chunked stream |
| `POST /files/{id}/rewrap` | Entitlement + buyer PK | After `OP_VERIFY`: **E2E capsule** (preferred) or server DEK re-wrap |
| `PUT /files/{id}/delivery-capsule` | Owner DID | Store recipient-wrapped DEK for true E2E `/rewrap` |
| `GET /files/{id}/admin-stream` | DID (trusted) | Peel nested → chunked plaintext stream |

Large-file routes refuse the legacy full-buffer path above 4 MiB.

### True E2E (owner-wrapped DEK)

1. Upload via `envelope-upload` encrypted to **owner** Kyber PK (storage cannot unwrap).
2. Publisher `OP_GRANT` (or buyer `OP_PURCHASE`) binds recipient DID + `buyer_pk_hash`.
3. Owner `PUT /delivery-capsule` with DEK KEM-wrapped to recipient.
4. `/rewrap` streams `[header with capsule EFK] ++ ciphertext` — no server DEK unwrap.

## Entitlement binding (security)

**OP_PURCHASE** input now includes `buyer_pk_hash` (32 bytes) = `SHA-256(buyer Kyber public key raw bytes)`.

**OP_VERIFY** input appends the same hash; status `5` = wrong PK.

Storage `/rewrap` sends the hash in the verify payload. Entitlements with a non-zero stored hash reject requests where `SHA-256(buyer-public-key) ≠ stored hash`.

Legacy entitlements (all-zero hash) skip PK check until re-purchased.

## Client updates

- `spacekit-js`: `buildPurchaseInput(listingId, buyerPkHash)`, `buyerPkHashFromPublicKeyHex()`
- CLI `content pay` / `content purchase`: binds CLI identity public key at purchase

## Audit checklist (CertiK)

- [ ] DEK never leaves wrap layer on delivery path (server only decapsulates 32-byte file key)
- [ ] Ciphertext chunks are immutable across recipients
- [ ] `buyer_pk_hash` binding at purchase and verify
- [ ] Legacy entitlement backward compatibility
- [ ] Nested envelope fallback does not weaken single-layer security

## Not in scope (optional follow-ups)

- `OP_CONSUME` for single-use delivery policy
- Content-addressed blob dedup (`blob/{blake3}`)
- Protected tier (compute-side inference, brain never to browser)
