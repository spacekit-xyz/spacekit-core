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

test("crypto demo prompts: WASM labels", async () => {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);
  mod.initSync({ module: wasmBytes });
  mod.growformer_init();

  const brainPath = resolve(NEUROKIT, "agent-data/crypto-analysis/crypto-causal.bin");
  const brainBytes = readFileSync(brainPath);
  mod.growformer_load_brain(new Uint8Array(brainBytes));

  const promptsFile = readFileSync(
    resolve(NEUROKIT, "data/crypto/crypto-prompts-demo.txt"),
    "utf-8"
  );
  const prompts = promptsFile
    .split("\n")
    .map((l: string) => l.trim())
    .filter((l: string) => l.length > 0 && !l.startsWith("#"));

  console.log(`  Running ${prompts.length} crypto demo prompts on WASM…\n`);
  for (let i = 0; i < prompts.length; i++) {
    mod.growformer_reset_conversation();
    const raw = mod.growformer_generation(prompts[i]);
    const label = extractLabel(raw);
    console.log(`  [${String(i + 1).padStart(2)}] ${label.padEnd(22)} | ${prompts[i].slice(0, 80)}`);
  }
});

test("fintech demo prompts: WASM labels", async () => {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);

  mod.growformer_init();
  const brainPath = resolve(NEUROKIT, "agent-data/fintech-analysis/fintech-causal.bin");
  const brainBytes = readFileSync(brainPath);
  mod.growformer_load_brain(new Uint8Array(brainBytes));

  const promptsFile = readFileSync(
    resolve(NEUROKIT, "data/fintech/fintech-prompts-demo.txt"),
    "utf-8"
  );
  const prompts = promptsFile
    .split("\n")
    .map((l: string) => l.trim())
    .filter((l: string) => l.length > 0 && !l.startsWith("#"));

  console.log(`  Running ${prompts.length} fintech demo prompts on WASM…\n`);
  for (let i = 0; i < prompts.length; i++) {
    mod.growformer_reset_conversation();
    const raw = mod.growformer_generation(prompts[i]);
    const label = extractLabel(raw);
    console.log(`  [${String(i + 1).padStart(2)}] ${label.padEnd(22)} | ${prompts[i].slice(0, 80)}`);
  }
});
