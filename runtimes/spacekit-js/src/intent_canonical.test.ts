import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import {
  canonicalJson,
  canonicalIntentPayload,
  assertSignableExpiry,
  INTENT_DOMAIN,
  MAX_INTENT_LIFETIME_SECS,
} from "./intent_canonical.js";
import type { Intent } from "./intent_builder.js";

const sha256Hex = (s: string) => createHash("sha256").update(s, "utf8").digest("hex");

test("canonicalJson sorts object keys", () => {
  assert.equal(canonicalJson({ b: 1, a: 2, c: 3 }), '{"a":2,"b":1,"c":3}');
  assert.equal(canonicalJson({ z: { y: 1, x: 2 } }), '{"z":{"x":2,"y":1}}');
});

test("canonicalJson preserves array order", () => {
  assert.equal(canonicalJson([3, 1, 2]), "[3,1,2]");
});

test("canonicalJson drops undefined fields, matching serde's omitted None", () => {
  assert.equal(canonicalJson({ a: 1, b: undefined }), '{"a":1}');
});

test("canonicalJson escapes control characters", () => {
  assert.equal(canonicalJson("a\nb"), '"a\\nb"');
  assert.equal(canonicalJson("a\tb"), '"a\\tb"');
});

/**
 * The node rejects any signature over a payload it would not itself construct,
 * so this vector must stay identical to `cross_language_canonical_vector` in
 * `spacekit-compute-node/src/intent_auth.rs`. If you change one, change both.
 */
test("canonical payload matches the node's cross-language vector", async () => {
  const intent = {
    version: "1.0",
    intent_id: "0123456789abcdef0123456789abcdef",
    actor: "did:spacekit:testnet:alice",
    chain: "spacekit:testnet",
    nonce: "1",
    expiry: 1700000000,
    actions: [{ type: "transfer", to: "did:bob", amount: "500" }],
    constraints: { max_fee_astra: "100" },
  } as unknown as Intent;

  const lines = (await canonicalIntentPayload(intent)).split("\n");

  assert.equal(lines[0], INTENT_DOMAIN);
  assert.equal(lines[1], "1.0");
  assert.equal(lines[2], "0123456789abcdef0123456789abcdef");
  assert.equal(lines[3], "did:spacekit:testnet:alice");
  assert.equal(lines[4], "", "absent `agent` must serialize as an empty line");
  assert.equal(lines[5], "spacekit:testnet");
  assert.equal(lines[6], "1");
  assert.equal(lines[7], "1700000000");
  assert.equal(
    lines[8],
    sha256Hex('[{"amount":"500","to":"did:bob","type":"transfer"}]'),
  );
  assert.equal(lines[9], sha256Hex('{"max_fee_astra":"100"}'));
  assert.equal(lines.length, 10);
});

test("payload changes when any execution-affecting field changes", async () => {
  const base = {
    version: "1.0",
    intent_id: "a",
    actor: "did:a",
    chain: "c",
    nonce: "1",
    expiry: 100,
    actions: [],
    constraints: {},
  } as unknown as Intent;

  const baseline = await canonicalIntentPayload(base);

  const mutations: Array<Partial<Record<keyof Intent, unknown>>> = [
    { actor: "did:mallory" },
    { chain: "other" },
    { nonce: "2" },
    { expiry: 101 },
    { actions: [{ type: "transfer", to: "did:mallory", amount: "1" }] },
    { constraints: { max_fee_astra: "0" } },
  ];

  for (const patch of mutations) {
    const mutated = { ...base, ...patch } as Intent;
    assert.notEqual(
      await canonicalIntentPayload(mutated),
      baseline,
      `mutating ${Object.keys(patch)[0]} must change the signing payload`,
    );
  }
});

test("assertSignableExpiry rejects expired and over-long intents", () => {
  const now = 1_000_000;
  const at = (expiry: number) => ({ expiry }) as Intent;

  assert.throws(() => assertSignableExpiry(at(now), now), /expired/);
  assert.throws(() => assertSignableExpiry(at(now - 1), now), /expired/);
  assert.throws(
    () => assertSignableExpiry(at(now + MAX_INTENT_LIFETIME_SECS + 1), now),
    /exceeds/,
  );
  assert.throws(() => assertSignableExpiry({} as Intent, now), /Unix timestamp/);

  assertSignableExpiry(at(now + 1), now);
  assertSignableExpiry(at(now + MAX_INTENT_LIFETIME_SECS), now);
});
