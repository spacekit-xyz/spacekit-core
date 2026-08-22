import test from "node:test";
import assert from "node:assert/strict";
import { parseSpaceTimeCommand } from "./commands.js";

test("parses new-thread with body", () => {
  const input = [
    "spacetime:new-thread",
    "title: Test Thread",
    "",
    "This is the body.",
    "Second line.",
  ].join("\n");

  const result = parseSpaceTimeCommand(input);
  assert.ok(result);
  assert.equal(result?.type, "create_thread");
  if (result?.type !== "create_thread") return;
  assert.equal(result.title, "Test Thread");
  assert.equal(result.text, "This is the body.\nSecond line.");
});

test("parses new-thread without body (uses title)", () => {
  const input = ["spacetime:new-thread", "title: Only Title"].join("\n");
  const result = parseSpaceTimeCommand(input);
  assert.ok(result);
  assert.equal(result?.type, "create_thread");
  if (result?.type !== "create_thread") return;
  assert.equal(result.title, "Only Title");
  assert.equal(result.text, "Only Title");
});

test("parses reply with parent none", () => {
  const input = [
    "spacetime:reply",
    "thread: 7",
    "parent: none",
    "",
    "Top-level reply.",
  ].join("\n");

  const result = parseSpaceTimeCommand(input);
  assert.ok(result);
  assert.equal(result?.type, "reply");
  if (result?.type !== "reply") return;
  assert.equal(result.threadId, 7);
  assert.equal(result.parentPostId, null);
  assert.equal(result.text, "Top-level reply.");
});

test("rejects missing title or body", () => {
  const missingTitle = ["spacetime:new-thread", "no-title-here"].join("\n");
  assert.equal(parseSpaceTimeCommand(missingTitle), null);

  const missingBody = ["spacetime:reply", "thread: 1", "parent: none"].join("\n");
  assert.equal(parseSpaceTimeCommand(missingBody), null);
});
