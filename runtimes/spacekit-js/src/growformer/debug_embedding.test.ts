import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const WASM_PATH = resolve(__dirname, "../../growformer-pkg/growformer_bg.wasm");
const GLUE_PATH = resolve(__dirname, "../../growformer-pkg/growformer.js");
const NEUROKIT = resolve(__dirname, "../../../../neurokit/growformer");

const PROBE = "Bitcoin surges past $100k as institutional demand grows";

test("debug embedding: crypto bridge vector", async () => {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);
  mod.initSync({ module: wasmBytes });
  mod.growformer_init();

  const brainPath = resolve(NEUROKIT, "agent-data/crypto-analysis/crypto-causal.bin");
  const brainBytes = readFileSync(brainPath);
  mod.growformer_load_brain(new Uint8Array(brainBytes));

  const info = mod.growformer_debug_embedding(PROBE);
  console.log(`\n  WASM crypto embedding: ${info}`);

  mod.growformer_reset_conversation();
  const gen = mod.growformer_generation(PROBE);
  const result = typeof gen === "string" ? JSON.parse(gen) : gen;
  const label = String(result?.text ?? result?.generation ?? "").split("\n")[0].split(" — ")[0].trim();
  console.log(`  WASM crypto label: ${label}`);
});
