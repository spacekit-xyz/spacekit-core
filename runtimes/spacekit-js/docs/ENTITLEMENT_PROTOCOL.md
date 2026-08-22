# Entitlement Protocol

> Owner-approve or purchase → entitlement → storage delivery (server DEK re-wrap **or** true E2E capsule).

## Overview

```
Publisher creates listing (OP_CREATE_LISTING)
        │
        ├─ Paid: Buyer OP_PURCHASE(listing, buyer_pk_hash)
        │
        └─ Private share: Publisher OP_GRANT(listing, recipient_did, recipient_pk_hash)
                │
                └─ (E2E) Publisher wraps DEK → PUT /files/{id}/delivery-capsule
        │
        ▼
Recipient POST /files/{id}/rewrap
  + entitlement-id, buyer-did, buyer-public-key
        │
Storage ──OP_VERIFY──▶ Compute Node
        │
        ├─ E2E capsule present → stream [header+capsule EFK] ++ ciphertext
        │                       (storage never sees DEK / plaintext)
        └─ else server-wrapped blob → header DEK re-wrap / chunked stream
        │
        ▼
Recipient: decryptEnvelope(envelope, buyer_sk)
```

## Entitlement Contract

Deployed at `spacekit-standard-library/marketplace/astra-entitlement-ledger`.

### Opcodes

| Op | Code | Input | Output |
|----|------|-------|--------|
| `CREATE_LISTING` | `0x01` | `[op][listing_id:str][file_id:str][price:u64le][token:str][pricing_type:u8][period:u64le]` | `[1]` |
| `PURCHASE` | `0x02` | `[op][listing_id:str][buyer_pk_hash:32]` (+ `msg_value >= price`) | `[1][entitlement_id:32]` |
| `VERIFY` | `0x03` | `[op][entitlement_id:32][buyer_did:str][file_id:str][buyer_pk_hash:32]` | `[1][status:u8]` |
| `REVOKE` | `0x04` | `[op][entitlement_id:32]` | `[1]` |
| `GET_LISTING` | `0x05` | `[op][listing_id:str]` | `[1][listing_record]` |
| `GET_ENTITLEMENT` | `0x06` | `[op][entitlement_id:32]` | `[1][entitlement_record]` |
| `GRANT` | `0x07` | `[op][listing_id:str][recipient_did:str][buyer_pk_hash:32]` | `[1][entitlement_id:32]` |

String encoding: `[len:u16le][utf8_bytes]`.

`buyer_pk_hash` = `SHA-256(raw Kyber public key bytes)` — must be non-zero at purchase/grant.

### Pricing types

| Byte | Meaning |
|------|---------|
| `1` | One-time (`expires_at = u64::MAX`) |
| `2` | Subscription (`expires_at = now + period`) |

### Verify status codes

| Byte | Meaning |
|------|---------|
| `1` | Valid |
| `0` | Expired |
| `2` | Buyer DID mismatch |
| `3` | File ID mismatch |
| `4` | Revoked |
| `5` | Buyer public key mismatch |

### Events

- `entitlement:granted` — payload: `[entitlement_id:32][buyer_did_utf8][0x00][listing_id_utf8]`
- `entitlement:revoked` — payload: `[entitlement_id:32]`

## True E2E delivery

1. Owner encrypts with `encryptEnvelopeWithFileKey` to **owner** Kyber PK and `uploadEnvelope`.
2. Retain the 32-byte `fileKey` (or recover later by decrypting the owner envelope header).
3. `OP_GRANT` (or `OP_PURCHASE`) binds recipient DID + `buyer_pk_hash`.
4. Owner calls `grantAndPrepareDelivery` / `uploadDeliveryCapsule` — DEK KEM-wrapped to recipient.
5. Recipient `downloadWithEntitlement` → `/rewrap` streams capsule header + ciphertext.

Storage never holds the owner or recipient secret keys and never unwraps the DEK on the E2E path.

## Storage endpoints

### `POST /files/{file_id}/rewrap`

| Header | Required | Description |
|--------|----------|-------------|
| `entitlement-id` | Yes | 32-byte entitlement ID (hex) |
| `buyer-did` | Yes | Recipient DID |
| `buyer-public-key` | Yes | Recipient Kyber1024 PK (hex/base64) |

### `PUT /files/{file_id}/delivery-capsule`

| Header / body | Required | Description |
|---------------|----------|-------------|
| `Authorization: Bearer <owner-did>` | Yes | Must match file owner |
| `entitlement-id` | Yes | Entitlement from grant/purchase |
| JSON body | Yes | `EncryptedFileKey` (`kem_ciphertext_hex`, `nonce_hex`, `ciphertext_hex`) |

## spacekit-js

```typescript
import {
  encryptEnvelopeWithFileKey,
  uploadEnvelope,
  buildCreateListingInput,
  grantAndPrepareDelivery,
  downloadWithEntitlement,
  purchaseAndDownload,
} from "@spacekit/spacekit-js";

// Owner: encrypt once (retain DEK)
const { envelope, fileKey } = await encryptEnvelopeWithFileKey(
  plaintext,
  "Kyber1024",
  (fk) => kyberEncryptFileKey(fk, ownerPublicKey),
);
const { file_id } = await uploadEnvelope(storageUrl, ownerDid, ownerPkHex, envelope);

// Listing (price 0 for private share, or paid)
await vm.submitTransaction(contractId, buildCreateListingInput({
  listingId: "share-v1",
  fileId: file_id,
  price: 0n,
  token: "ASTRA",
  pricingType: 1,
  period: 0n,
}), ownerDid);

// Approve recipient + post E2E capsule
const entitlementIdHex = await grantAndPrepareDelivery({
  vm,
  contractId,
  listingId: "share-v1",
  publisherDid: ownerDid,
  recipientDid,
  recipientPublicKeyHex: recipientPkHex,
  fileId: file_id,
  storageBaseUrl: storageUrl,
  fileKey,
  encryptFileKeyForRecipient: (fk, pkHex) => kyberEncryptFileKey(fk, pkHex),
});

// Recipient download
const pt = await downloadWithEntitlement({
  storageBaseUrl: storageUrl,
  fileId: file_id,
  entitlementIdHex,
  buyerDid: recipientDid,
  buyerPublicKeyHex: recipientPkHex,
  decryptFileKey: (efk) => kyberDecryptFileKey(efk, recipientSecretKey),
});
```

## Trust boundaries

| Party | Holds | Sees plaintext? |
|-------|--------|-----------------|
| Storage (E2E path) | Ciphertext + recipient capsule EFK | **No** |
| Storage (server-wrapped path) | Server Kyber SK wrapping on-disk DEK | Can unwrap DEK to re-wrap |
| Owner | Owner Kyber SK + optional retained DEK | Yes (their content) |
| Recipient | Recipient Kyber SK | Yes after capsule/rewrap |
| Compute / entitlement ledger | Grant records | No content keys |
