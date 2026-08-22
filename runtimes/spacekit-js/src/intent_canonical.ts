/**
 * Canonical intent signing payload.
 *
 * An intent authorizes payments and contract execution, so the signature must
 * cover the whole intent. The previous scheme signed only `intent_id` — a
 * random 16-byte value — which meant a relay, or anyone who observed an intent
 * in flight, could keep the signature and rewrite the actions, amounts,
 * beneficiaries, and expiry.
 *
 * This module must produce byte-identical output to the Rust implementation in
 * `spacekit-compute-node/src/intent_auth.rs`. The shared test vector in both
 * files pins that down; change them together.
 */

import type { Intent } from "./intent_builder.js";

export const INTENT_DOMAIN = "SPACEKIT-INTENT-v1";

/** Longest an intent may remain valid, in seconds. Mirrors the node's limit. */
export const MAX_INTENT_LIFETIME_SECS = 3600;

/**
 * Deterministic JSON encoding: object keys sorted, no insignificant whitespace.
 *
 * `JSON.stringify` preserves insertion order, so two structurally identical
 * intents built in a different order would otherwise hash differently.
 */
export function canonicalJson(value: unknown): string {
  if (value === null || value === undefined) return "null";

  if (Array.isArray(value)) {
    return `[${value.map(canonicalJson).join(",")}]`;
  }

  switch (typeof value) {
    case "boolean":
      return value ? "true" : "false";
    case "number":
      if (!Number.isFinite(value)) {
        throw new Error(`cannot canonicalize non-finite number: ${value}`);
      }
      return JSON.stringify(value);
    case "string":
      return JSON.stringify(value);
    case "object": {
      const entries = Object.entries(value as Record<string, unknown>)
        // `undefined` has no JSON representation; dropping it here matches
        // serde, which omits `None` fields entirely.
        .filter(([, v]) => v !== undefined)
        // Default sort compares UTF-16 code units, which is what the Rust side
        // deliberately matches.
        .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
      return `{${entries
        .map(([k, v]) => `${JSON.stringify(k)}:${canonicalJson(v)}`)
        .join(",")}}`;
    }
    default:
      throw new Error(`cannot canonicalize value of type ${typeof value}`);
  }
}

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

async function sha256Hex(input: string): Promise<string> {
  const data = new TextEncoder().encode(input);
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) {
    throw new Error(
      "WebCrypto SubtleCrypto is unavailable; cannot compute the intent signing payload",
    );
  }
  return toHex(new Uint8Array(await subtle.digest("SHA-256", data)));
}

/**
 * Build the exact bytes an actor must sign for `intent`.
 *
 * Every field that affects execution is bound: changing the actor, chain,
 * nonce, expiry, any action, or any constraint produces a different payload
 * and invalidates the signature.
 */
export async function canonicalIntentPayload(intent: Intent): Promise<string> {
  const [actionsHash, constraintsHash] = await Promise.all([
    sha256Hex(canonicalJson(intent.actions ?? null)),
    sha256Hex(canonicalJson(intent.constraints ?? null)),
  ]);

  return [
    INTENT_DOMAIN,
    intent.version ?? "",
    intent.intent_id ?? "",
    intent.actor ?? "",
    intent.agent ?? "",
    intent.chain ?? "",
    intent.nonce ?? "",
    String(intent.expiry ?? 0),
    actionsHash,
    constraintsHash,
  ].join("\n");
}

/** Bytes form of {@link canonicalIntentPayload}, for signers that take bytes. */
export async function canonicalIntentPayloadBytes(intent: Intent): Promise<Uint8Array> {
  return new TextEncoder().encode(await canonicalIntentPayload(intent));
}

/**
 * Reject an intent whose expiry the node would refuse, so the failure surfaces
 * at signing time rather than as an opaque 401 later.
 */
export function assertSignableExpiry(intent: Intent, nowSecs = Math.floor(Date.now() / 1000)): void {
  if (typeof intent.expiry !== "number" || !Number.isFinite(intent.expiry)) {
    throw new Error("intent.expiry must be a Unix timestamp in seconds");
  }
  if (intent.expiry <= nowSecs) {
    throw new Error(`intent already expired at ${intent.expiry} (now ${nowSecs})`);
  }
  if (intent.expiry - nowSecs > MAX_INTENT_LIFETIME_SECS) {
    throw new Error(
      `intent expiry is ${intent.expiry - nowSecs}s away, which exceeds the ` +
        `${MAX_INTENT_LIFETIME_SECS}s maximum the network accepts`,
    );
  }
}
