import test from "node:test";
import assert from "node:assert/strict";
import {
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
  assert.equal(scopeAllowsOperation("contract_call|transfer", "contract_call"), true);
  assert.equal(scopeAllowsOperation("contract_call|transfer", "messaging"), false);
  assert.equal(scopeAllowsOperation("*", "anything"), true);
});

test("SessionHostState create validate revoke", () => {
  const s = new SessionHostState();
  const now = Math.floor(Date.now() / 1000);
  const id = s.create(
    "did:spacekit:owner",
    "did:spacekit:delegate",
    "contract_call",
    now + 3600,
  );
  const idStr = new TextDecoder().decode(id);
  assert.equal(idStr.length, 64);
  assert.match(idStr, /^[0-9a-f]+$/);

  assert.equal(s.validate("did:spacekit:wrong", "did:spacekit:owner", "contract_call"), 0);
  assert.equal(s.validate("did:spacekit:delegate", "did:spacekit:owner", "contract_call"), 1);
  assert.equal(s.validate("did:spacekit:delegate", "did:spacekit:owner", "transfer"), 0);

  assert.equal(s.revoke("did:spacekit:wrong", idStr), false);
  assert.equal(s.revoke("did:spacekit:owner", idStr), true);
  assert.equal(s.validate("did:spacekit:delegate", "did:spacekit:owner", "contract_call"), 0);
});
