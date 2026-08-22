import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const BRAIN_PATH = resolve(
  __dirname,
  "../../../../neurokit/growformer/agent-data/crypto-analysis/crypto-causal.bin"
);
const WASM_PATH = resolve(__dirname, "../../growformer-pkg/growformer_bg.wasm");
const GLUE_PATH = resolve(__dirname, "../../growformer-pkg/growformer.js");

test("growformer WASM: crypto brain ONLY (clean init)", async () => {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);
  mod.initSync({ module: wasmBytes });
  mod.growformer_init();

  const brainBytes = readFileSync(BRAIN_PATH);
  mod.growformer_load_brain(new Uint8Array(brainBytes));

  const rulesInfo = mod.growformer_inference_rules_info();
  console.log("  crypto rules_info:", JSON.stringify(rulesInfo));

  const prompts = [
    "ETH fees just hit $0.002, nobody's talking about this",
    "SEC just approved the first spot Ethereum ETF in a unanimous 5-0 vote",
    "Honestly the new savings rate is insulting after what they promised last quarter",
  ];

  for (const prompt of prompts) {
    mod.growformer_reset_conversation();
    const raw = mod.growformer_generation(prompt);
    const result = typeof raw === "string" ? JSON.parse(raw) : raw;
    const text = result?.text ?? result?.generation ?? JSON.stringify(result);
    const firstLine = String(text).split("\n")[0];
    console.log(`  [WASM-crypto-only] "${prompt.slice(0, 60)}…"  →  ${firstLine}`);
  }
});
