/**
 * Envelope encryption client for zero-knowledge file storage.
 *
 * The storage node never sees plaintext or private keys.
 *
 * ## Browser download flow
 * 1. `requestChallenge(baseUrl, fileId)` → server KEM-encrypts a nonce to owner pubkey.
 * 2. Client decrypts the nonce via `kyber_decrypt` (from `@spacekit/sdk/kyber`).
 * 3. `fetchEnvelope(baseUrl, fileId, challengeId, decryptedNonceHex)` → raw envelope bytes.
 * 4. Client decrypts `encrypted_file_key` from the header via `kyber_decrypt` → file key.
 * 5. `decryptEnvelopeChunks(header, dataSection, fileKey)` → plaintext.
 *
 * ## Browser upload flow
 * 1. Generate random 32-byte file key.
 * 2. KEM-encrypt the file key via `kyber_encrypt` → `{kemCiphertext, nonce, ciphertext}`.
 * 3. `encryptEnvelope(plaintext, encryptedFileKey, kemAlgorithm, fileKey)` → envelope bytes.
 * 4. `uploadEnvelope(baseUrl, ownerDid, ownerPubkeyHex, envelopeBytes)` → fileId.
 */

import { bytesToHex, hexToBytes } from "./storage";
import { gcm } from "@noble/ciphers/aes";
import { sha256 } from "@noble/hashes/sha256";

// ────────────────────── WebCrypto ──────────────────────

/** Resolve the browser WebCrypto SubtleCrypto object, avoiding Node.js polyfill conflicts. */
function getSubtle(): SubtleCrypto | null {
  return (
    globalThis.crypto?.subtle ??
    (typeof window !== "undefined" ? (window as unknown as { crypto?: { subtle?: SubtleCrypto } }).crypto?.subtle : undefined) ??
    null
  );
}

function getWebCrypto(): Crypto {
  const c = globalThis.crypto ?? (typeof window !== "undefined" ? (window as unknown as { crypto?: Crypto }).crypto : undefined);
  if (!c) throw new Error("WebCrypto not available");
  return c;
}

/**
 * AES-256-GCM decrypt using @noble/ciphers (pure JS).
 * Used when crypto.subtle is unavailable (Safari over HTTP / non-secure context).
 */
function nobleAesGcmDecrypt(
  key: Uint8Array,
  nonce: Uint8Array,
  ciphertext: Uint8Array,
): Uint8Array {
  const cipher = gcm(key, nonce);
  return cipher.decrypt(ciphertext);
}

/**
 * AES-256-GCM encrypt using @noble/ciphers (pure JS).
 * Used when crypto.subtle is unavailable.
 */
function nobleAesGcmEncrypt(
  key: Uint8Array,
  nonce: Uint8Array,
  plaintext: Uint8Array,
): Uint8Array {
  const cipher = gcm(key, nonce);
  return cipher.encrypt(plaintext);
}

// ────────────────────── Constants ──────────────────────

const ENVELOPE_VERSION = 1;
const DEFAULT_CHUNK_SIZE = 256 * 1024; // 256 KiB

// ────────────────────── Types ──────────────────────

export interface EncryptedFileKey {
  kem_ciphertext_hex: string;
  nonce_hex: string;
  ciphertext_hex: string;
}

export interface ChunkMeta {
  offset: number;
  encrypted_size: number;
  nonce_hex: string;
}

export interface EnvelopeHeader {
  version: number;
  kem_algorithm: string;
  cipher_suite: string;
  encrypted_file_key: EncryptedFileKey;
  chunk_size: number;
  total_chunks: number;
  total_plaintext_size: number;
  plaintext_hash: string;
  chunks: ChunkMeta[];
}

export interface EncryptedChallenge {
  kem_ciphertext_hex: string;
  nonce_hex: string;
  ciphertext_hex: string;
}

export interface ChallengeResponse {
  success: boolean;
  challenge_id?: string;
  encrypted_challenge?: EncryptedChallenge;
  error?: string;
}

// ────────────────────── Envelope parsing ──────────────────────

export function parseEnvelopeHeader(data: Uint8Array): { header: EnvelopeHeader; headerSize: number } {
  if (data.length < 8) throw new Error("Envelope too short");
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  const len = Number(view.getBigUint64(0, true));
  if (data.length < 8 + len) {
    throw new Error(`Envelope header truncated: need ${8 + len}, got ${data.length}`);
  }
  const headerJson = new TextDecoder().decode(data.subarray(8, 8 + len));
  const header: EnvelopeHeader = JSON.parse(headerJson);
  if (header.version !== ENVELOPE_VERSION) {
    throw new Error(`Unsupported envelope version: ${header.version}`);
  }
  return { header, headerSize: 8 + len };
}

// ────────────────────── Decrypt ──────────────────────

/**
 * Decrypt all chunks of an envelope using a 32-byte file key.
 * The file key is obtained by decrypting `header.encrypted_file_key` via Kyber.
 *
 * Uses WebCrypto (`crypto.subtle`) when available; falls back to
 * `@noble/ciphers` pure-JS AES-GCM for Safari over HTTP (non-secure context).
 */
export async function decryptEnvelopeChunks(
  header: EnvelopeHeader,
  dataSection: Uint8Array,
  fileKey: Uint8Array,
): Promise<Uint8Array> {
  const subtle = getSubtle();

  const parts: Uint8Array[] = [];

  if (subtle) {
    const cryptoKey = await subtle.importKey(
      "raw", toAB(fileKey), { name: "AES-GCM" }, false, ["decrypt"],
    );
    for (const chunk of header.chunks) {
      const start = chunk.offset;
      const end = start + chunk.encrypted_size;
      if (end > dataSection.length) {
        throw new Error(`Chunk out of bounds: end=${end}, data=${dataSection.length}`);
      }
      const ciphertext = dataSection.subarray(start, end);
      const nonce = hexToBytes(chunk.nonce_hex);
      const decrypted = await subtle.decrypt(
        { name: "AES-GCM", iv: toAB(nonce) },
        cryptoKey,
        toAB(ciphertext),
      );
      parts.push(new Uint8Array(decrypted));
    }
  } else {
    for (const chunk of header.chunks) {
      const start = chunk.offset;
      const end = start + chunk.encrypted_size;
      if (end > dataSection.length) {
        throw new Error(`Chunk out of bounds: end=${end}, data=${dataSection.length}`);
      }
      const ciphertext = dataSection.subarray(start, end);
      const nonce = hexToBytes(chunk.nonce_hex);
      parts.push(nobleAesGcmDecrypt(fileKey, nonce, ciphertext));
    }
  }

  return concatUint8Arrays(parts);
}

/**
 * Full envelope decryption: parse header, decrypt file key, decrypt chunks.
 *
 * `decryptFileKeyFn` is called with the `EncryptedFileKey` from the header.
 * In the browser, this wraps `kyber_decrypt` from the SDK.
 */
export async function decryptEnvelope(
  envelopeBytes: Uint8Array,
  decryptFileKeyFn: (efk: EncryptedFileKey) => Promise<Uint8Array>,
): Promise<Uint8Array> {
  const { header, headerSize } = parseEnvelopeHeader(envelopeBytes);
  const dataSection = envelopeBytes.subarray(headerSize);

  const fileKey = await decryptFileKeyFn(header.encrypted_file_key);
  if (fileKey.length !== 32) {
    throw new Error(`Expected 32-byte file key, got ${fileKey.length}`);
  }

  return decryptEnvelopeChunks(header, dataSection, fileKey);
}

/**
 * Decrypt a single chunk. Useful for streaming decryption.
 */
export async function decryptChunk(
  fileKey: Uint8Array,
  encryptedChunk: Uint8Array,
  nonceHex: string,
): Promise<Uint8Array> {
  const nonce = hexToBytes(nonceHex);
  const subtle = getSubtle();
  if (subtle) {
    const cryptoKey = await subtle.importKey(
      "raw", toAB(fileKey), { name: "AES-GCM" }, false, ["decrypt"],
    );
    const decrypted = await subtle.decrypt(
      { name: "AES-GCM", iv: toAB(nonce) },
      cryptoKey,
      toAB(encryptedChunk),
    );
    return new Uint8Array(decrypted);
  }
  return nobleAesGcmDecrypt(fileKey, nonce, encryptedChunk);
}

// ────────────────────── Encrypt ──────────────────────

/**
 * Encrypt plaintext into an envelope blob and return the DEK for later E2E capsules.
 *
 * Retain `fileKey` (32 bytes) so the owner can wrap it to recipients after `OP_GRANT`
 * without asking the storage node to unwrap anything.
 */
export async function encryptEnvelopeWithFileKey(
  plaintext: Uint8Array,
  kemAlgorithm: string,
  encryptFileKeyFn: (fileKey: Uint8Array) => Promise<EncryptedFileKey>,
  chunkSize = DEFAULT_CHUNK_SIZE,
): Promise<{ envelope: Uint8Array; fileKey: Uint8Array }> {
  const wc = getWebCrypto();
  const subtle = getSubtle();

  // Generate random 32-byte file key
  const fileKey = wc.getRandomValues(new Uint8Array(32));

  const encryptedFileKey = await encryptFileKeyFn(fileKey);

  const chunksMeta: ChunkMeta[] = [];
  const chunksData: Uint8Array[] = [];
  let offset = 0;

  if (subtle) {
    const cryptoKey = await subtle.importKey(
      "raw", toAB(fileKey), { name: "AES-GCM" }, false, ["encrypt"],
    );
    for (let i = 0; i < plaintext.length; i += chunkSize) {
      const chunk = plaintext.subarray(i, Math.min(i + chunkSize, plaintext.length));
      const nonce = wc.getRandomValues(new Uint8Array(12));
      const encrypted = await subtle.encrypt(
        { name: "AES-GCM", iv: toAB(nonce) },
        cryptoKey,
        toAB(chunk),
      );
      const encBytes = new Uint8Array(encrypted);
      chunksMeta.push({
        offset,
        encrypted_size: encBytes.length,
        nonce_hex: bytesToHex(nonce),
      });
      offset += encBytes.length;
      chunksData.push(encBytes);
    }
  } else {
    for (let i = 0; i < plaintext.length; i += chunkSize) {
      const chunk = plaintext.subarray(i, Math.min(i + chunkSize, plaintext.length));
      const nonce = wc.getRandomValues(new Uint8Array(12));
      const encBytes = nobleAesGcmEncrypt(fileKey, nonce, chunk);
      chunksMeta.push({
        offset,
        encrypted_size: encBytes.length,
        nonce_hex: bytesToHex(nonce),
      });
      offset += encBytes.length;
      chunksData.push(encBytes);
    }
  }

  let plaintextHash: string;
  if (subtle) {
    plaintextHash = "sha256:" + bytesToHex(new Uint8Array(await subtle.digest("SHA-256", toAB(plaintext))));
  } else {
    plaintextHash = "sha256:" + bytesToHex(sha256(plaintext));
  }

  const header: EnvelopeHeader = {
    version: ENVELOPE_VERSION,
    kem_algorithm: kemAlgorithm,
    cipher_suite: "AES-256-GCM",
    encrypted_file_key: encryptedFileKey,
    chunk_size: chunkSize,
    total_chunks: chunksMeta.length,
    total_plaintext_size: plaintext.length,
    plaintext_hash: plaintextHash,
    chunks: chunksMeta,
  };

  const headerJson = new TextEncoder().encode(JSON.stringify(header));
  const lenBuf = new ArrayBuffer(8);
  new DataView(lenBuf).setBigUint64(0, BigInt(headerJson.length), true);

  const envelope = concatUint8Arrays([
    new Uint8Array(lenBuf),
    headerJson,
    ...chunksData,
  ]);
  return { envelope, fileKey };
}

/**
 * Encrypt plaintext into an envelope blob.
 *
 * `encryptFileKeyFn` is called to KEM-encrypt the random file key.
 * In the browser, this wraps `kyber_encrypt` from the SDK.
 * Prefer {@link encryptEnvelopeWithFileKey} when you need the DEK for E2E grants.
 */
export async function encryptEnvelope(
  plaintext: Uint8Array,
  kemAlgorithm: string,
  encryptFileKeyFn: (fileKey: Uint8Array) => Promise<EncryptedFileKey>,
  chunkSize = DEFAULT_CHUNK_SIZE,
): Promise<Uint8Array> {
  const { envelope } = await encryptEnvelopeWithFileKey(
    plaintext,
    kemAlgorithm,
    encryptFileKeyFn,
    chunkSize,
  );
  return envelope;
}

// ────────────────────── HTTP helpers ──────────────────────

export async function requestChallenge(
  baseUrl: string,
  fileId: string,
): Promise<ChallengeResponse> {
  const url = `${baseUrl.replace(/\/$/, "")}/files/${encodeURIComponent(fileId)}/challenge`;
  const res = await fetch(url);
  return res.json();
}

export async function fetchEnvelope(
  baseUrl: string,
  fileId: string,
  challengeId: string,
  decryptedNonceHex: string,
): Promise<Uint8Array> {
  const url = `${baseUrl.replace(/\/$/, "")}/files/${encodeURIComponent(fileId)}/stream`;
  const res = await fetch(url, {
    headers: {
      "challenge-id": challengeId,
      "challenge-response": decryptedNonceHex,
    },
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Stream failed (${res.status}): ${text}`);
  }
  return new Uint8Array(await res.arrayBuffer());
}

/**
 * Streaming fetch: returns the raw ReadableStream of envelope bytes.
 */
export async function fetchEnvelopeStream(
  baseUrl: string,
  fileId: string,
  challengeId: string,
  decryptedNonceHex: string,
): Promise<ReadableStream<Uint8Array>> {
  const url = `${baseUrl.replace(/\/$/, "")}/files/${encodeURIComponent(fileId)}/stream`;
  const res = await fetch(url, {
    headers: {
      "challenge-id": challengeId,
      "challenge-response": decryptedNonceHex,
    },
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Stream failed (${res.status}): ${text}`);
  }
  if (!res.body) throw new Error("No response body for streaming");
  return res.body;
}

export async function uploadEnvelope(
  baseUrl: string,
  ownerDid: string,
  ownerPublicKeyHex: string,
  envelopeBytes: Uint8Array,
  filename?: string,
  contentType?: string,
): Promise<{ file_id: string; hash: string }> {
  const url = `${baseUrl.replace(/\/$/, "")}/files/envelope-upload`;
  const headers: Record<string, string> = {
    "owner-did": ownerDid,
    "owner-public-key": ownerPublicKeyHex,
  };
  if (filename) headers["filename"] = filename;
  if (contentType) headers["content-type"] = contentType;

  const res = await fetch(url, {
    method: "POST",
    headers,
    body: toAB(envelopeBytes),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Envelope upload failed (${res.status}): ${text}`);
  }
  return res.json();
}

// ────────────────────── Utilities ──────────────────────

/** Extract a clean ArrayBuffer from a Uint8Array (handles subarrays safely). */
function toAB(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

function concatUint8Arrays(arrays: Uint8Array[]): Uint8Array {
  let total = 0;
  for (const a of arrays) total += a.length;
  const out = new Uint8Array(total);
  let offset = 0;
  for (const a of arrays) {
    out.set(a, offset);
    offset += a.length;
  }
  return out;
}
