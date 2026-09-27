/**
 * SPKG publisher signatures (`signatures/publisher.json`).
 *
 * Mirrors `spkg_signature.rs` in the CLI and storage node:
 *
 * ```json
 * { "v": 1, "alg": "ed25519", "did": "did:key:z6Mk…", "public_key": "<64 hex>", "signature": "<128 hex>" }
 * ```
 *
 * Ed25519 over UTF-8 `"SpaceKit package signature v1\n" + hex(sha256(manifest.json))`.
 * The manifest lists every payload hash and the aggregate checksum, so one
 * signature covers the whole package.
 */

export type PackageSignatureStatus = "valid" | "invalid" | "unsupported";

export interface PackageSignature {
  /** Archive entry name, e.g. `signatures/publisher.json`. */
  entry: string;
  status: PackageSignatureStatus;
  /** The signer's DID (bound to the key only when `status` is "valid"). */
  did: string | null;
  /** Why the signature is not valid. */
  reason?: string;
}

const DOMAIN = "SpaceKit package signature v1\n";
const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

function hexToBytes(hex: string): Uint8Array | null {
  const clean = hex.trim().toLowerCase();
  if (clean.length % 2 !== 0 || !/^[0-9a-f]*$/.test(clean)) return null;
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  return out;
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function base58Decode(input: string): Uint8Array | null {
  const bytes: number[] = [];
  for (const ch of input) {
    let carry = ALPHABET.indexOf(ch);
    if (carry < 0) return null;
    for (let i = bytes.length - 1; i >= 0; i--) {
      carry += bytes[i] * 58;
      bytes[i] = carry & 0xff;
      carry >>= 8;
    }
    while (carry > 0) {
      bytes.unshift(carry & 0xff);
      carry >>= 8;
    }
  }
  let leading = 0;
  while (leading < input.length && input[leading] === "1") leading++;
  return new Uint8Array([...new Array(leading).fill(0), ...bytes]);
}

/**
 * Whether a 32-byte Ed25519 key is the key behind `did`: the W3C
 * `did:key:z6Mk…` form, or kit.space's short form (`did:key:z6Mk` + first 44 hex
 * chars of the key).
 */
export function ed25519KeyMatchesDid(did: string, publicKey: Uint8Array): boolean {
  if (publicKey.length !== 32 || !did.startsWith("did:key:z6Mk")) return false;
  const suffix = did.slice("did:key:z6Mk".length);
  const pkHex = bytesToHex(publicKey);
  if (suffix.length === 44 && /^[0-9a-fA-F]+$/.test(suffix)) return suffix.toLowerCase() === pkHex.slice(0, 44);
  const decoded = base58Decode(did.slice("did:key:z".length));
  return (
    !!decoded &&
    decoded.length === 34 &&
    decoded[0] === 0xed &&
    decoded[1] === 0x01 &&
    decoded.slice(2).every((b, i) => b === publicKey[i])
  );
}

async function signingMessage(manifestJson: Uint8Array): Promise<Uint8Array> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", Uint8Array.from(manifestJson)));
  return new TextEncoder().encode(DOMAIN + bytesToHex(digest));
}

async function ed25519Verify(publicKey: Uint8Array, signature: Uint8Array, message: Uint8Array): Promise<boolean | null> {
  try {
    const key = await crypto.subtle.importKey("raw", Uint8Array.from(publicKey), { name: "Ed25519" }, false, ["verify"]);
    return await crypto.subtle.verify({ name: "Ed25519" }, key, Uint8Array.from(signature), Uint8Array.from(message));
  } catch {
    return null; // WebCrypto without Ed25519
  }
}

/** Check one `signatures/*.json` entry against the raw `manifest.json` bytes. */
export async function verifyPackageSignature(
  entry: string,
  manifestJson: Uint8Array,
  signatureJson: Uint8Array,
): Promise<PackageSignature> {
  let parsed: { v?: unknown; alg?: unknown; did?: unknown; public_key?: unknown; signature?: unknown };
  try {
    parsed = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(signatureJson));
  } catch {
    return { entry, status: "invalid", did: null, reason: "not JSON" };
  }
  const did = typeof parsed.did === "string" ? parsed.did : null;
  if (parsed.v !== 1) return { entry, status: "unsupported", did, reason: `version ${String(parsed.v)}` };
  if (typeof parsed.alg !== "string" || parsed.alg.toLowerCase() !== "ed25519") {
    return { entry, status: "unsupported", did, reason: `algorithm ${String(parsed.alg)}` };
  }
  const pk = typeof parsed.public_key === "string" ? hexToBytes(parsed.public_key) : null;
  const sig = typeof parsed.signature === "string" ? hexToBytes(parsed.signature) : null;
  if (!did || !pk || pk.length !== 32 || !sig || sig.length !== 64) {
    return { entry, status: "invalid", did, reason: "malformed signature entry" };
  }
  if (!ed25519KeyMatchesDid(did, pk)) {
    return { entry, status: "invalid", did, reason: "DID does not match the public key" };
  }
  const ok = await ed25519Verify(pk, sig, await signingMessage(manifestJson));
  if (ok === null) return { entry, status: "unsupported", did, reason: "this browser cannot verify Ed25519" };
  return ok ? { entry, status: "valid", did } : { entry, status: "invalid", did, reason: "signature does not verify" };
}
