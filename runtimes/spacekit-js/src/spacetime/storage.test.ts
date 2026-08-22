import test from "node:test";
import assert from "node:assert/strict";
import { createInMemoryStorage } from "../storage.js";
import { createSpaceTimeStorage } from "./storage.js";

test("stores and retrieves blobs by ref", async () => {
  const adapter = createInMemoryStorage();
  const storage = createSpaceTimeStorage(adapter, "spacetime-test");
  const ref = await storage.putBlob({ text: "hello" });
  const blob = await storage.getBlob(ref);
  assert.equal(blob.text, "hello");
});
