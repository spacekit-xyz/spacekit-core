import test from "node:test";
import assert from "node:assert/strict";
import {
  generateEd25519Keypair,
  signEd25519,
  verifyEd25519,
} from "./signatures.js";

// These cover the SHA-512 hook wiring in getEd25519(). @noble/ed25519 moved the
// hook between major versions (v1 `utils`, v2 `etc`, 2.3+ `hashes`); setting only
// one of them leaves signing dead at runtime while still type-checking and
// building cleanly, so the round-trip below is what actually catches it.

test("ed25519 sign and verify round-trip", async () => {
  const { publicKey, privateKey } = await generateEd25519Keypair();
  const message = new TextEncoder().encode("spacekit signing round-trip");

  const signature = await signEd25519(message, privateKey);
  assert.equal(signature.length, 64);
  assert.equal(await verifyEd25519(message, signature, publicKey), true);
});

test("ed25519 verify rejects a tampered message", async () => {
  const { publicKey, privateKey } = await generateEd25519Keypair();
  const message = new TextEncoder().encode("spacekit signing round-trip");
  const signature = await signEd25519(message, privateKey);

  const tampered = new TextEncoder().encode("spacekit signing round-trip!");
  assert.equal(await verifyEd25519(tampered, signature, publicKey), false);
});

test("ed25519 verify rejects a signature from a different key", async () => {
  const a = await generateEd25519Keypair();
  const b = await generateEd25519Keypair();
  const message = new TextEncoder().encode("spacekit signing round-trip");

  const signature = await signEd25519(message, a.privateKey);
  assert.equal(await verifyEd25519(message, signature, b.publicKey), false);
});
