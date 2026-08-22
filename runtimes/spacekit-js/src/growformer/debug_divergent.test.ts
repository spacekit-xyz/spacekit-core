import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const WASM_PATH = resolve(__dirname, "../../growformer-pkg/growformer_bg.wasm");
const GLUE_PATH = resolve(__dirname, "../../growformer-pkg/growformer.js");
const NEUROKIT = resolve(__dirname, "../../../../neurokit/growformer");

function extractLabel(raw: unknown): string {
  const result = typeof raw === "string" ? JSON.parse(raw) : raw;
  const text = (result as any)?.text ?? (result as any)?.generation ?? JSON.stringify(result);
  return String(text).split("\n")[0].split(" — ")[0].trim();
}

const DIVERGENT_PROMPTS = [
  "The exploit drained every wallet that interacted with the contract; people lost their entire savings",
  "The founder moved 45,000 ETH to a fresh wallet two hours before the exploit was announced",
  "They stole $50 million from the liquidity pool and vanished — the Discord is nothing but scam victims now",
  "The protocol promised 20% yields but the smart contract had a backdoor the whole time",
  "Wen airdrop? The team keeps teasing and the community is getting restless ngl",
];

test("divergent prompts: crypto WASM vs CLI labels", async () => {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);
  mod.initSync({ module: wasmBytes });
  mod.growformer_init();

  const brainPath = resolve(NEUROKIT, "agent-data/crypto-analysis/crypto-causal.bin");
  const brainBytes = readFileSync(brainPath);
  mod.growformer_load_brain(new Uint8Array(brainBytes));

  for (let i = 0; i < DIVERGENT_PROMPTS.length; i++) {
    mod.growformer_reset_conversation();
    const emb = mod.growformer_debug_embedding(DIVERGENT_PROMPTS[i]);
    const raw = mod.growformer_generation(DIVERGENT_PROMPTS[i]);
    const label = extractLabel(raw);
    console.log(`  [${i+1}] ${label.padEnd(22)} | ${DIVERGENT_PROMPTS[i].slice(0, 70)}`);
    console.log(`      embedding: ${emb}`);
  }
});
