import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const BRAIN_PATHS = {
  fintech: resolve(
    __dirname,
    "../../../../neurokit/growformer/agent-data/fintech-analysis/fintech-causal.bin"
  ),
  crypto: resolve(
    __dirname,
    "../../../../neurokit/growformer/agent-data/crypto-analysis/crypto-causal.bin"
  ),
};

const WASM_PATH = resolve(__dirname, "../../growformer-pkg/growformer_bg.wasm");
const GLUE_PATH = resolve(__dirname, "../../growformer-pkg/growformer.js");

async function loadGrowformer() {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);
  mod.initSync({ module: wasmBytes });
  return mod;
}

test("growformer WASM: fintech brain rules parity", async () => {
  const gf = await loadGrowformer();
  const brainBytes = readFileSync(BRAIN_PATHS.fintech);
  gf.growformer_init();
  gf.growformer_load_brain(new Uint8Array(brainBytes));

  assert.ok(gf.growformer_ready(), "growformer should be ready after load_brain");

  const brainInfo = gf.growformer_brain_info();
  console.log("  brain_info:", JSON.stringify(brainInfo));
  assert.ok(brainInfo, "brain_info should return data");

  const rulesInfo = gf.growformer_inference_rules_info();
  console.log("  rules_info:", JSON.stringify(rulesInfo));

  const rules =
    typeof rulesInfo === "string" ? JSON.parse(rulesInfo) : rulesInfo;
  assert.ok(
    rules.headline_lexical_topic > 0,
    `expected headline_lexical_topic > 0, got ${rules.headline_lexical_topic}`
  );
  console.log(
    `  headline_lexical_topic=${rules.headline_lexical_topic}`,
    `lattice_misfire=${rules.lattice_misfire}`,
    `lexical_polarity=${rules.lexical_polarity}`,
    `sarcasm_simple=${rules.sarcasm_simple}`,
    `negative_anchor_tokens=${rules.negative_anchor_tokens}`
  );

  const TEST_PROMPTS = [
    "Honestly the new savings rate is insulting after what they promised last quarter",
    "Crushed earnings, raised guidance, stock is red — make it make sense",
    "The payment processor suffered a data breach exposing 47 million card numbers and has suspended all transactions pending investigation",
  ];

  for (const prompt of TEST_PROMPTS) {
    gf.growformer_reset_conversation();
    const raw = gf.growformer_generation(prompt);
    const result = typeof raw === "string" ? JSON.parse(raw) : raw;
    const text = result?.text ?? result?.generation ?? JSON.stringify(result);
    const firstLine = String(text).split("\n")[0];
    console.log(`  [WASM] "${prompt.slice(0, 60)}…"  →  ${firstLine}`);
  }
});

test("growformer WASM: crypto brain rules parity", async () => {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);

  const gf = mod;
  const brainBytes = readFileSync(BRAIN_PATHS.crypto);
  gf.growformer_load_brain(new Uint8Array(brainBytes));

  const rulesInfo = gf.growformer_inference_rules_info();
  console.log("  crypto rules_info:", JSON.stringify(rulesInfo));

  const CRYPTO_PROMPTS = [
    "ETH fees just hit $0.002, nobody's talking about this",
    "SEC just approved the first spot Ethereum ETF in a unanimous 5-0 vote",
  ];

  for (const prompt of CRYPTO_PROMPTS) {
    gf.growformer_reset_conversation();
    const raw = gf.growformer_generation(prompt);
    const result = typeof raw === "string" ? JSON.parse(raw) : raw;
    const text = result?.text ?? result?.generation ?? JSON.stringify(result);
    const firstLine = String(text).split("\n")[0];
    console.log(`  [WASM] "${prompt.slice(0, 60)}…"  →  ${firstLine}`);
  }
});
