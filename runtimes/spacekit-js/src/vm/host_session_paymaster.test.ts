import test from "node:test";
import assert from "node:assert/strict";
import {
  PaymasterHostState,
  SessionHostState,
  didMatchesPattern,
  scopeAllowsOperation,
} from "../host_session_paymaster.js";

test("didMatchesPattern exact and prefix wildcard", () => {
  assert.equal(didMatchesPattern("did:spacekit:alice", "did:spacekit:alice"), true);
  assert.equal(didMatchesPattern("did:spacekit:alice", "did:other:bob"), false);
  assert.equal(didMatchesPattern("did:spacekit:alice", "did:spacekit:*"), true);
  assert.equal(didMatchesPattern("did:spacekit:alice", "*"), true);
});

test("scopeAllowsOperation pipe list and star", () => {
  assert.equal(scopeAllowsOperation("vault_charge|transfer", "vault_charge"), true);
  assert.equal(scopeAllowsOperation("vault_charge|transfer", "messaging"), false);
  assert.equal(scopeAllowsOperation("*", "anything"), true);
});

test("SessionHostState create validate revoke", () => {
  const s = new SessionHostState();
  const now = Math.floor(Date.now() / 1000);
  const id = s.create(
    "did:spacekit:owner",
    "did:spacekit:delegate",
    "vault_charge",
    now + 3600,
  );
  const idStr = new TextDecoder().decode(id);
  assert.equal(idStr.length, 64);
  assert.match(idStr, /^[0-9a-f]+$/);

  assert.equal(s.validate("did:spacekit:wrong", "did:spacekit:owner", "vault_charge"), 0);
  assert.equal(s.validate("did:spacekit:delegate", "did:spacekit:owner", "vault_charge"), 1);
  assert.equal(s.validate("did:spacekit:delegate", "did:spacekit:owner", "transfer"), 0);

  assert.equal(s.revoke("did:spacekit:wrong", idStr), false);
  assert.equal(s.revoke("did:spacekit:owner", idStr), true);
  assert.equal(s.validate("did:spacekit:delegate", "did:spacekit:owner", "vault_charge"), 0);
});

test("PaymasterHostState policy budget and sponsor charge", () => {
  const p = new PaymasterHostState();
  const sponsor = "did:spacekit:sponsor";
  const policy = JSON.stringify({
    allowed_dids: ["did:spacekit:user:*"],
    allowed_ops: ["vault_charge"],
    per_call_max: "500",
    daily_max: "1000",
    budget: "2000",
  });
  p.setPolicy(sponsor, policy);

  assert.equal(p.getBudgetString(sponsor), "2000");

  const ok = p.trySponsorCharge(
    "did:spacekit:user:alice",
    sponsor,
    "100",
    "vault_charge",
  );
  assert.equal(ok, true);
  assert.equal(p.getBudgetString(sponsor), "1900");

  const denied = p.trySponsorCharge(
    "did:spacekit:other:bob",
    sponsor,
    "1",
    "vault_charge",
  );
  assert.equal(denied, false);
});

test("PaymasterHostState rejects charge when allowed_ops empty in policy", () => {
  const p = new PaymasterHostState();
  const sponsor = "did:spacekit:s2";
  p.setPolicy(
    sponsor,
    JSON.stringify({
      allowed_dids: ["did:spacekit:user:bob"],
      allowed_ops: [],
      budget: "99",
    }),
  );
  assert.equal(
    p.trySponsorCharge("did:spacekit:user:bob", sponsor, "1", "vault_charge"),
    false,
  );
});
