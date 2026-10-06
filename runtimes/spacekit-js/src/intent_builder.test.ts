import test from "node:test";
import assert from "node:assert/strict";
import { IntentBuilder, estimateIntentFees } from "./intent_builder.js";

const ASTRA = 10n ** 18n;

test("intents carry ASTRA wei only", () => {
  const intent = new IntentBuilder("did:spacekit:alice")
    .executeContract("did:contract:xyz", "00", { valueWei: 2n * ASTRA })
    .transferAstra("did:spacekit:bob", 10_000n)
    .maxValueWei(3n * ASTRA)
    .build();
  assert.equal(intent.constraints.max_value_wei, (3n * ASTRA).toString());
  const json = JSON.stringify(intent);
  assert.ok(!/usd|vault/i.test(json), json);

  const fees = estimateIntentFees(intent.actions);
  // 2 ASTRA attached + 10_000 wei transfer + 25 bps network fee (25 wei).
  assert.equal(fees.total_wei, 2n * ASTRA + 10_025n);
});

test("negative amounts are refused", () => {
  assert.throws(() => new IntentBuilder("did:spacekit:alice").transferAstra("did:spacekit:bob", -1n));
});
