import test from "node:test";
import assert from "node:assert";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const WASM_PATH = resolve(__dirname, "../../growformer-pkg/growformer_bg.wasm");
const GLUE_PATH = resolve(__dirname, "../../growformer-pkg/growformer.js");
const NEUROKIT = resolve(__dirname, "../../../../neurokit/growformer");

test("brain diagnostics: crypto-causal WASM", async () => {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);
  mod.initSync({ module: wasmBytes });
  mod.growformer_init();

  const brainPath = resolve(NEUROKIT, "agent-data/crypto-analysis/crypto-causal.bin");
  const brainBytes = readFileSync(brainPath);
  mod.growformer_load_brain(new Uint8Array(brainBytes));

  const info = JSON.parse(mod.growformer_brain_info());
  console.log("  brain_info:", JSON.stringify(info, null, 2));

  const rulesInfo = JSON.parse(mod.growformer_inference_rules_info());
  console.log("  rules_info:", JSON.stringify(rulesInfo, null, 2));

  // Run one prompt with WASM, then the same with a fresh load to check determinism
  mod.growformer_reset_conversation();
  const r1 = JSON.parse(mod.growformer_generation("The exploit drained every wallet that interacted with the contract; people lost their entire savings"));
  console.log("  run1:", r1.text.split("\n")[0].split(" — ")[0]);

  mod.growformer_reset_conversation();
  const r2 = JSON.parse(mod.growformer_generation("The exploit drained every wallet that interacted with the contract; people lost their entire savings"));
  console.log("  run2:", r2.text.split("\n")[0].split(" — ")[0]);

  assert.strictEqual(r1.text.split("\n")[0], r2.text.split("\n")[0], "Same prompt should give same result");
});
