/**
 * Entitlement Protocol client helpers.
 *
 * Flows:
 *   - Purchase: OP_PURCHASE → /rewrap → client decrypt
 *   - Owner approve (E2E): OP_GRANT → owner posts delivery capsule → recipient /rewrap
 *
 * True E2E: envelope is encrypted to the owner PK; storage never unwraps the DEK.
 * After grant, the owner KEM-wraps the DEK to the recipient and PUTs a capsule.
 *
 * @module entitlement
 */

import { bytesToHex, hexToBytes } from "./storage.js";
import { decryptEnvelope } from "./envelope.js";
import { sha256 } from "@noble/hashes/sha2";
import type { SpacekitVm } from "./vm/spacekitvm.js";
import type { EncryptedFileKey } from "./envelope.js";

// ────────────────────── Wire format opcodes ──────────────────────

/** Entitlement ledger contract opcodes. */
export const EntitlementOp = {
  CREATE_LISTING: 0x01,
  PURCHASE: 0x02,
  VERIFY: 0x03,
  REVOKE: 0x04,
  GET_LISTING: 0x05,
  GET_ENTITLEMENT: 0x06,
  /** Publisher-only approve/grant (no payment). */
  GRANT: 0x07,
} as const;

/** Verification status bytes returned by OP_VERIFY. */
export const EntitlementStatus = {
  VALID: 1,
  EXPIRED: 0,
  WRONG_BUYER: 2,
  WRONG_FILE: 3,
  REVOKED: 4,
  WRONG_PK: 5,
} as const;

export const ENTITLEMENT_EVENT = "entitlement:granted";

// ────────────────────── Wire helpers ──────────────────────

/** SHA-256(buyer Kyber public key raw bytes) for OP_PURCHASE / OP_VERIFY binding. */
export function buyerPkHashFromPublicKeyHex(publicKeyHex: string): Uint8Array {
  const pk = hexToBytes(publicKeyHex.replace(/^0x/i, ""));
  return sha256(pk);
}

function encodeString(s: string): Uint8Array {
  const encoded = new TextEncoder().encode(s);
  const buf = new Uint8Array(2 + encoded.length);
  buf[0] = encoded.length & 0xff;
  buf[1] = (encoded.length >> 8) & 0xff;
  buf.set(encoded, 2);
  return buf;
}

function encodeU64LE(v: bigint): Uint8Array {
  const buf = new Uint8Array(8);
  const dv = new DataView(buf.buffer);
  dv.setBigUint64(0, v, true);
  return buf;
}

function concat(...parts: Uint8Array[]): Uint8Array {
  let total = 0;
  for (const p of parts) total += p.length;
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return out;
}

// ────────────────────── Payload builders ──────────────────────

/**
 * Build `OP_CREATE_LISTING` input bytes.
 */
export function buildCreateListingInput(opts: {
  listingId: string;
  fileId: string;
  price: bigint;
  token: string;
  pricingType: number;
  period: bigint;
}): Uint8Array {
  return concat(
    new Uint8Array([EntitlementOp.CREATE_LISTING]),
    encodeString(opts.listingId),
    encodeString(opts.fileId),
    encodeU64LE(opts.price),
    encodeString(opts.token),
    new Uint8Array([opts.pricingType]),
    encodeU64LE(opts.period),
  );
}

/**
 * Build `OP_PURCHASE` input bytes.
 * `buyerPkHash` = SHA-256(buyer Kyber public key raw bytes), 32 bytes.
 */
export function buildPurchaseInput(
  listingId: string,
  buyerPkHash: Uint8Array,
): Uint8Array {
  if (buyerPkHash.length !== 32) {
    throw new Error("buyerPkHash must be 32 bytes");
  }
  return concat(
    new Uint8Array([EntitlementOp.PURCHASE]),
    encodeString(listingId),
    buyerPkHash,
  );
}

/**
 * Build `OP_VERIFY` input bytes.
 */
export function buildVerifyInput(
  entitlementId: Uint8Array,
  buyerDid: string,
  fileId: string,
  buyerPkHash: Uint8Array,
): Uint8Array {
  if (buyerPkHash.length !== 32) {
    throw new Error("buyerPkHash must be 32 bytes");
  }
  return concat(
    new Uint8Array([EntitlementOp.VERIFY]),
    entitlementId,
    encodeString(buyerDid),
    encodeString(fileId),
    buyerPkHash,
  );
}

/**
 * Parse the result of `OP_PURCHASE` — returns the 32-byte entitlement ID.
 */
export function parsePurchaseResult(output: Uint8Array): Uint8Array {
  if (output.length < 33 || output[0] !== 1) {
    throw new Error(
      `Unexpected purchase result (len=${output.length}, first=${output[0]})`,
    );
  }
  return output.slice(1, 33);
}

/**
 * Build `OP_GRANT` input bytes (publisher approves a recipient; no payment).
 * `recipientPkHash` = SHA-256(recipient Kyber public key raw bytes), 32 bytes.
 */
export function buildGrantInput(
  listingId: string,
  recipientDid: string,
  recipientPkHash: Uint8Array,
): Uint8Array {
  if (recipientPkHash.length !== 32) {
    throw new Error("recipientPkHash must be 32 bytes");
  }
  return concat(
    new Uint8Array([EntitlementOp.GRANT]),
    encodeString(listingId),
    encodeString(recipientDid),
    recipientPkHash,
  );
}

/** Parse `OP_GRANT` result — same wire shape as purchase. */
export function parseGrantResult(output: Uint8Array): Uint8Array {
  return parsePurchaseResult(output);
}

/**
 * Build `OP_REVOKE` input bytes.
 */
export function buildRevokeInput(entitlementId: Uint8Array): Uint8Array {
  if (entitlementId.length !== 32) {
    throw new Error("entitlementId must be 32 bytes");
  }
  return concat(new Uint8Array([EntitlementOp.REVOKE]), entitlementId);
}

// ────────────────────── Rewrap fetch ──────────────────────

export interface RewrapOptions {
  storageBaseUrl: string;
  fileId: string;
  entitlementIdHex: string;
  buyerDid: string;
  buyerPublicKeyHex: string;
}

/**
 * POST `/files/{fileId}/rewrap` to the storage node.
 * Returns the re-wrapped envelope bytes (encrypted to buyer's KEM PK).
 */
export async function fetchRewrappedEnvelope(
  opts: RewrapOptions,
): Promise<Uint8Array> {
  const base = opts.storageBaseUrl.replace(/\/$/, "");
  const url = `${base}/files/${encodeURIComponent(opts.fileId)}/rewrap`;

  const res = await fetch(url, {
    method: "POST",
    headers: {
      "entitlement-id": opts.entitlementIdHex,
      "buyer-did": opts.buyerDid,
      "buyer-public-key": opts.buyerPublicKeyHex,
    },
  });

  if (!res.ok) {
    const body = await res.text();
    throw new Error(`Rewrap failed (${res.status}): ${body}`);
  }
  return new Uint8Array(await res.arrayBuffer());
}

// ────────────────────── End-to-end orchestration ──────────────────────

export interface PurchaseAndDownloadOptions {
  /** SpacekitVm instance for executing the purchase transaction. */
  vm: SpacekitVm;
  /** Contract ID (address) of the deployed entitlement-ledger. */
  contractId: string;
  /** Listing ID the buyer wants to purchase. */
  listingId: string;
  /** Buyer's DID (e.g. "did:spacekit:local:abc123..."). */
  buyerDid: string;
  /** Payment amount (in smallest token units). */
  paymentValue: bigint;
  /** File ID on the storage node (from the listing). */
  fileId: string;
  /** Base URL of the storage node (e.g. "https://storage.spacekit.io"). */
  storageBaseUrl: string;
  /** Buyer's Kyber public key (hex). */
  buyerPublicKeyHex: string;
  /**
   * Function to decrypt the `EncryptedFileKey` from the re-wrapped envelope
   * header using the buyer's KEM secret key.  Returns the 32-byte file key.
   */
  decryptFileKey: (efk: EncryptedFileKey) => Promise<Uint8Array>;
}

/**
 * End-to-end: purchase an entitlement, fetch the re-wrapped content,
 * and decrypt it client-side.
 *
 * @returns The decrypted plaintext bytes.
 */
export async function purchaseAndDownload(
  opts: PurchaseAndDownloadOptions,
): Promise<Uint8Array> {
  // 1. Execute OP_PURCHASE on the VM
  const purchaseInput = buildPurchaseInput(
    opts.listingId,
    buyerPkHashFromPublicKeyHex(opts.buyerPublicKeyHex),
  );
  const tx = await opts.vm.submitTransaction(
    opts.contractId,
    purchaseInput,
    opts.buyerDid,
    opts.paymentValue,
  );

  const receipt = opts.vm.getReceipt(tx.id);
  if (!receipt || receipt.status <= 0) {
    throw new Error(
      `Purchase transaction failed: status=${receipt?.status ?? "none"}`,
    );
  }

  const entitlementId = parsePurchaseResult(receipt.result);
  const entitlementIdHex = bytesToHex(entitlementId);

  // 2. Fetch re-wrapped envelope from the storage node
  const envelope = await fetchRewrappedEnvelope({
    storageBaseUrl: opts.storageBaseUrl,
    fileId: opts.fileId,
    entitlementIdHex,
    buyerDid: opts.buyerDid,
    buyerPublicKeyHex: opts.buyerPublicKeyHex,
  });

  // 3. Decrypt the envelope client-side
  const plaintext = await decryptEnvelope(envelope, opts.decryptFileKey);
  return plaintext;
}

// ────────────────────── Owner approve + true E2E capsule ──────────────────────

export interface UploadDeliveryCapsuleOptions {
  storageBaseUrl: string;
  fileId: string;
  entitlementIdHex: string;
  /** Owner DID (must match file metadata owner). Sent as Authorization bearer. */
  ownerDid: string;
  /** DEK wrapped to the recipient's Kyber PK. */
  encryptedFileKey: EncryptedFileKey;
}

/**
 * PUT `/files/{fileId}/delivery-capsule` — owner posts recipient-wrapped DEK.
 */
export async function uploadDeliveryCapsule(
  opts: UploadDeliveryCapsuleOptions,
): Promise<void> {
  const base = opts.storageBaseUrl.replace(/\/$/, "");
  const url = `${base}/files/${encodeURIComponent(opts.fileId)}/delivery-capsule`;
  const res = await fetch(url, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${opts.ownerDid}`,
      "entitlement-id": opts.entitlementIdHex.replace(/^0x/i, ""),
    },
    body: JSON.stringify(opts.encryptedFileKey),
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`Delivery capsule upload failed (${res.status}): ${body}`);
  }
}

export interface GrantAndPrepareDeliveryOptions {
  vm: SpacekitVm;
  contractId: string;
  listingId: string;
  /** Publisher DID (must own the listing). */
  publisherDid: string;
  recipientDid: string;
  recipientPublicKeyHex: string;
  fileId: string;
  storageBaseUrl: string;
  /**
   * 32-byte content DEK (retained at upload, or recovered by decrypting the
   * owner envelope header with the owner Kyber SK).
   */
  fileKey: Uint8Array;
  /**
   * KEM-wrap `fileKey` to the recipient's public key
   * (browser: wrap `kyber_encrypt` from the SDK).
   */
  encryptFileKeyForRecipient: (
    fileKey: Uint8Array,
    recipientPublicKeyHex: string,
  ) => Promise<EncryptedFileKey>;
}

/**
 * Owner approve flow: `OP_GRANT` → wrap DEK to recipient → upload delivery capsule.
 * Recipient then calls `downloadWithEntitlement` (or `fetchRewrappedEnvelope` + decrypt).
 *
 * @returns Hex entitlement id.
 */
export async function grantAndPrepareDelivery(
  opts: GrantAndPrepareDeliveryOptions,
): Promise<string> {
  if (opts.fileKey.length !== 32) {
    throw new Error("fileKey must be 32 bytes");
  }

  const grantInput = buildGrantInput(
    opts.listingId,
    opts.recipientDid,
    buyerPkHashFromPublicKeyHex(opts.recipientPublicKeyHex),
  );
  const tx = await opts.vm.submitTransaction(
    opts.contractId,
    grantInput,
    opts.publisherDid,
  );
  const receipt = opts.vm.getReceipt(tx.id);
  if (!receipt || receipt.status <= 0) {
    throw new Error(
      `Grant transaction failed: status=${receipt?.status ?? "none"}`,
    );
  }

  const entitlementId = parseGrantResult(receipt.result);
  const entitlementIdHex = bytesToHex(entitlementId);

  const capsule = await opts.encryptFileKeyForRecipient(
    opts.fileKey,
    opts.recipientPublicKeyHex,
  );
  await uploadDeliveryCapsule({
    storageBaseUrl: opts.storageBaseUrl,
    fileId: opts.fileId,
    entitlementIdHex,
    ownerDid: opts.publisherDid,
    encryptedFileKey: capsule,
  });

  return entitlementIdHex;
}

export interface DownloadWithEntitlementOptions {
  storageBaseUrl: string;
  fileId: string;
  entitlementIdHex: string;
  buyerDid: string;
  buyerPublicKeyHex: string;
  decryptFileKey: (efk: EncryptedFileKey) => Promise<Uint8Array>;
}

/**
 * Entitled download via `/rewrap` (E2E capsule or server DEK re-wrap) + client decrypt.
 */
export async function downloadWithEntitlement(
  opts: DownloadWithEntitlementOptions,
): Promise<Uint8Array> {
  const envelope = await fetchRewrappedEnvelope({
    storageBaseUrl: opts.storageBaseUrl,
    fileId: opts.fileId,
    entitlementIdHex: opts.entitlementIdHex,
    buyerDid: opts.buyerDid,
    buyerPublicKeyHex: opts.buyerPublicKeyHex,
  });
  return decryptEnvelope(envelope, opts.decryptFileKey);
}
