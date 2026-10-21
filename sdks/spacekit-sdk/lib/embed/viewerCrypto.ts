/**
 * Per-viewer file encryption for embedded apps.
 *
 * Each file gets a fresh AES-256-GCM key. That key is wrapped (AES-KW,
 * RFC 3394) with a key-encryption key the host derives from the viewer's
 * identity secret, separately for every app:
 *
 *   KEK = HKDF-SHA256(ikm = identity secret,
 *                     salt = "spacekit/app-data-kek/v1",
 *                     info = "app:<appId>|did:<did>")  →  AES-KW-256
 *
 * The identity secret is the Ed25519 seed behind the viewer's `did:key` (the
 * recovery key), so the same person can decrypt on any device where they sign
 * in with it, and nobody else can. The KEK is non-extractable and stays in the
 * host; the app frame only ever sees ciphertext, the IV and the wrapped key.
 *
 * Wire format (strings an app stores next to the blob):
 *   ivB64       base64 of the 12-byte GCM IV
 *   wrappedKey  "skw1." + base64 of the 40-byte AES-KW output
 */

export const APP_DATA_KEK_SALT = "spacekit/app-data-kek/v1";
export const WRAPPED_KEY_PREFIX = "skw1.";

/** What a host derives a key for. */
export interface AppKeyRequest {
  /** Lower-case hex app id. */
  appId: string;
  /** The signed-in viewer's DID. */
  viewerDid: string;
}

export interface EncryptedUpload {
  ciphertext: Blob;
  ivB64: string;
  wrappedKey: string;
  /** SHA-256 (hex) of the ciphertext. */
  contentHash: string;
  /** Ciphertext size in bytes. */
  size: number;
}

function subtle(): SubtleCrypto {
  const s = globalThis.crypto?.subtle;
  if (!s) throw new Error("Web Crypto is unavailable on this host (needs https:// or localhost)");
  return s;
}

function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  if (bytes.byteOffset === 0 && bytes.byteLength === bytes.buffer.byteLength && bytes.buffer instanceof ArrayBuffer) {
    return bytes.buffer;
  }
  // A copy through the base constructor: `slice()` on a subclass such as
  // Node's Buffer returns a view of the whole backing pool.
  return new Uint8Array(bytes).buffer as ArrayBuffer;
}

export function bytesToBase64(bytes: Uint8Array): string {
  const chunk = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

export function base64ToBytes(b64: string): Uint8Array {
  const raw = atob(b64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

export function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) out += b.toString(16).padStart(2, "0");
  return out;
}

export async function sha256Hex(data: ArrayBuffer | Uint8Array): Promise<string> {
  const buf = data instanceof Uint8Array ? toArrayBuffer(data) : data;
  return bytesToHex(new Uint8Array(await subtle().digest("SHA-256", buf)));
}

/** The HKDF `info` for one viewer and app. */
export function appKeyInfo(req: AppKeyRequest): string {
  return `app:${req.appId.trim().toLowerCase()}|did:${req.viewerDid.trim()}`;
}

/**
 * Derive the viewer's key-encryption key for one app. Hosts call this inside
 * their `deriveAppKey` service with the identity secret they hold, so the SDK
 * bridge never receives the secret itself.
 */
export async function deriveAppDataKey(secret: Uint8Array, req: AppKeyRequest): Promise<CryptoKey> {
  if (!(secret instanceof Uint8Array) || secret.length < 16) {
    throw new Error("deriveAppDataKey: identity secret must be at least 16 bytes");
  }
  if (!req.appId || !req.viewerDid) throw new Error("deriveAppDataKey: appId and viewerDid are required");
  const enc = new TextEncoder();
  const ikm = await subtle().importKey("raw", toArrayBuffer(secret), "HKDF", false, ["deriveKey"]);
  return subtle().deriveKey(
    {
      name: "HKDF",
      hash: "SHA-256",
      salt: enc.encode(APP_DATA_KEK_SALT),
      info: enc.encode(appKeyInfo(req)),
    },
    ikm,
    { name: "AES-KW", length: 256 },
    false,
    ["wrapKey", "unwrapKey"],
  );
}

async function toBytes(data: Blob | ArrayBuffer | Uint8Array): Promise<Uint8Array> {
  if (data instanceof Uint8Array) return data;
  if (data instanceof ArrayBuffer) return new Uint8Array(data);
  return new Uint8Array(await data.arrayBuffer());
}

/** Encrypt one file under a fresh key, wrapped with `kek`. */
export async function encryptForViewer(
  kek: CryptoKey,
  data: Blob | ArrayBuffer | Uint8Array,
): Promise<EncryptedUpload> {
  const s = subtle();
  const plain = await toBytes(data);
  const fileKey = await s.generateKey({ name: "AES-GCM", length: 256 }, true, ["encrypt", "decrypt"]);
  const iv = globalThis.crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(await s.encrypt({ name: "AES-GCM", iv }, fileKey, toArrayBuffer(plain)));
  const wrapped = new Uint8Array(await s.wrapKey("raw", fileKey, kek, { name: "AES-KW" }));
  return {
    ciphertext: new Blob([ct]),
    ivB64: bytesToBase64(iv),
    wrappedKey: WRAPPED_KEY_PREFIX + bytesToBase64(wrapped),
    contentHash: await sha256Hex(ct),
    size: ct.byteLength,
  };
}

/** Unwrap a file key produced by {@link encryptForViewer}. */
export async function unwrapFileKey(kek: CryptoKey, wrappedKey: string): Promise<CryptoKey> {
  if (typeof wrappedKey !== "string" || !wrappedKey.startsWith(WRAPPED_KEY_PREFIX)) {
    throw new Error("Unsupported key format: this file was not encrypted by the SpaceKit bridge");
  }
  let wrapped: Uint8Array;
  try {
    wrapped = base64ToBytes(wrappedKey.slice(WRAPPED_KEY_PREFIX.length));
  } catch {
    throw new Error("Malformed wrapped key");
  }
  if (wrapped.length !== 40) throw new Error("Malformed wrapped key");
  try {
    return await subtle().unwrapKey(
      "raw",
      toArrayBuffer(wrapped),
      kek,
      { name: "AES-KW" },
      { name: "AES-GCM", length: 256 },
      false,
      ["decrypt"],
    );
  } catch {
    // AES-KW's integrity check fails for any other KEK.
    throw new Error("This file was encrypted for a different account or app");
  }
}

/** Decrypt a ciphertext produced by {@link encryptForViewer}. */
export async function decryptForViewer(
  kek: CryptoKey,
  ciphertext: Blob | ArrayBuffer | Uint8Array,
  ivB64: string,
  wrappedKey: string,
): Promise<Blob> {
  const fileKey = await unwrapFileKey(kek, wrappedKey);
  const iv = base64ToBytes(String(ivB64 ?? ""));
  if (iv.length !== 12) throw new Error("Malformed IV");
  const ct = await toBytes(ciphertext);
  let plain: ArrayBuffer;
  try {
    plain = await subtle().decrypt({ name: "AES-GCM", iv: toArrayBuffer(iv) }, fileKey, toArrayBuffer(ct));
  } catch {
    throw new Error("Decryption failed: the file is corrupted or does not match its key");
  }
  return new Blob([plain]);
}
