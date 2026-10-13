/**
 * Descriptor signatures for plugins distributed outside a signed `.spkg`
 * (brains published as marketplace artifacts, contracts).
 *
 * Same envelope as SPKG publisher signatures, with its own domain:
 *
 * ```json
 * { "v": 1, "alg": "ed25519", "did": "did:key:z6Mk…", "public_key": "<64 hex>", "signature": "<128 hex>" }
 * ```
 *
 * Ed25519 over UTF-8 `"SpaceKit plugin descriptor v1\n" + hex(sha256(descriptor bytes))`,
 * where the descriptor bytes are exactly the published `spacekit-plugin.json`.
 * The descriptor pins every file by SHA-256, so the signature covers them.
 *
 * Plugins shipped inside an `.spkg` are covered by the package signature
 * instead; this is not needed for them.
 */

import { ed25519KeyMatchesDid } from "../embed/spkgSignature.js";

export const PLUGIN_SIGNATURE_DOMAIN = "SpaceKit plugin descriptor v1\n";

export interface PluginSignatureResult {
  valid: boolean;
  did: string | null;
  reason?: string;
}

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

/** The exact bytes a publisher signs for a descriptor. */
export async function pluginSigningMessage(descriptorBytes: Uint8Array): Promise<Uint8Array> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", Uint8Array.from(descriptorBytes)));
  return new TextEncoder().encode(PLUGIN_SIGNATURE_DOMAIN + bytesToHex(digest));
}

/**
 * Verify a descriptor signature. `did` is bound to the key only when `valid`.
 * Check `did` against the descriptor's `publisher` (or your allowlist) yourself.
 */
export async function verifyPluginDescriptorSignature(
  descriptorBytes: Uint8Array,
  signatureJson: Uint8Array | string,
): Promise<PluginSignatureResult> {
  let parsed: { v?: unknown; alg?: unknown; did?: unknown; public_key?: unknown; signature?: unknown };
  try {
    parsed = JSON.parse(typeof signatureJson === "string" ? signatureJson : new TextDecoder().decode(signatureJson));
  } catch {
    return { valid: false, did: null, reason: "signature is not JSON" };
  }
  const did = typeof parsed.did === "string" ? parsed.did : null;
  if (parsed.v !== 1) return { valid: false, did, reason: `unsupported version ${String(parsed.v)}` };
  if (typeof parsed.alg !== "string" || parsed.alg.toLowerCase() !== "ed25519") {
    return { valid: false, did, reason: `unsupported algorithm ${String(parsed.alg)}` };
  }
  const pk = typeof parsed.public_key === "string" ? hexToBytes(parsed.public_key) : null;
  const sig = typeof parsed.signature === "string" ? hexToBytes(parsed.signature) : null;
  if (!did || !pk || pk.length !== 32 || !sig || sig.length !== 64) {
    return { valid: false, did, reason: "malformed signature" };
  }
  if (!ed25519KeyMatchesDid(did, pk)) return { valid: false, did, reason: "DID does not match the public key" };
  try {
    const key = await crypto.subtle.importKey("raw", Uint8Array.from(pk), { name: "Ed25519" }, false, ["verify"]);
    const ok = await crypto.subtle.verify(
      { name: "Ed25519" },
      key,
      Uint8Array.from(sig),
      Uint8Array.from(await pluginSigningMessage(descriptorBytes)),
    );
    return ok ? { valid: true, did } : { valid: false, did, reason: "signature does not match" };
  } catch {
    return { valid: false, did, reason: "this environment cannot verify Ed25519" };
  }
}
