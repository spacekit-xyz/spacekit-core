/**
 * Pets brain (Luna) inference parity test.
 *
 * Tests both `growformer_generation` (CLI path) and `growformer_converse` (website path)
 * with and without topic graph / inference TOML to isolate what's causing incomplete results
 * on the website.
 *
 * Run: node --test dist/growformer/inference_parity_pets.test.js
 */
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const SPACEKIT_ROOT = "/Users/astor/Projects/2026/spacekit";
const PETS_ROOT = resolve(SPACEKIT_ROOT, "spacekit-projects/pets");
const BRAIN_PATH = resolve(PETS_ROOT, "agent/luna-v2.bin");
const INFERENCE_TOML_PATH = resolve(PETS_ROOT, "data/inference_pets.toml");
const TOPIC_GRAPH_PATH = resolve(PETS_ROOT, "data/knowledge_graph.toml");
const TOPIC_GRAPH_OVERLAY_PATH = resolve(PETS_ROOT, "data/knowledge_graph_pet_overlay.toml");

const SPACEKIT_JS_ROOT = resolve(SPACEKIT_ROOT, "spacekit-js");
const WASM_PATH = resolve(SPACEKIT_JS_ROOT, "growformer-pkg/growformer_bg.wasm");
const GLUE_PATH = resolve(SPACEKIT_JS_ROOT, "growformer-pkg/growformer.js");

const PROMPTS = [
  "Hey Luna",
  "the vacuum is about to start",
  "who's a good kitty",
];

function parseResponse(raw: unknown): { text: string; confidence: number; action_type: string } {
  const obj = typeof raw === "string" ? JSON.parse(raw) : raw;
  return {
    text: obj?.text ?? "",
    confidence: obj?.confidence ?? 0,
    action_type: obj?.action_type ?? "",
  };
}

async function loadGrowformer() {
  const wasmBytes = readFileSync(WASM_PATH);
  const mod = await import(GLUE_PATH);
  mod.initSync({ module: wasmBytes });
  return mod;
}

test("pets brain: growformer_generation (CLI-equivalent path)", async () => {
  const gf = await loadGrowformer();
  const brainBytes = readFileSync(BRAIN_PATH);

  gf.growformer_init();
  gf.growformer_load_brain(new Uint8Array(brainBytes));
  assert.ok(gf.growformer_ready(), "growformer should be ready");

  const info = gf.growformer_brain_info();
  const brainInfo = typeof info === "string" ? JSON.parse(info) : info;
  console.log("  brain:", brainInfo.agent_name, `(${brainInfo.num_groups} group(s))`);
  console.log("  inference_profile:", brainInfo.inference_profile ?? "(none)");

  if (existsSync(INFERENCE_TOML_PATH)) {
    const toml = readFileSync(INFERENCE_TOML_PATH, "utf-8");
    gf.growformer_load_inference_toml(toml);
    console.log("  inference_toml: loaded (%d chars)", toml.length);
  }

  if (existsSync(TOPIC_GRAPH_PATH) && gf.growformer_load_topic_graph) {
    const base = readFileSync(TOPIC_GRAPH_PATH, "utf-8");
    const overlay = existsSync(TOPIC_GRAPH_OVERLAY_PATH)
      ? readFileSync(TOPIC_GRAPH_OVERLAY_PATH, "utf-8")
      : undefined;
    gf.growformer_load_topic_graph(base, overlay);
    console.log("  topic_graph: loaded (base=%d, overlay=%d chars)", base.length, overlay?.length ?? 0);
  }

  console.log("\n  --- growformer_generation (single-shot, like CLI --brain --prompt) ---");
  const seen = new Set<string>();
  for (const prompt of PROMPTS) {
    gf.growformer_reset_conversation();
    const raw = gf.growformer_generation(prompt);
    const r = parseResponse(raw);
    console.log(`  "${prompt}"`);
    console.log(`    → text: "${r.text}"`);
    console.log(`    → conf=${r.confidence.toFixed(4)} action=${r.action_type}`);
    assert.ok(r.text.length > 0, `empty text for "${prompt}"`);
    seen.add(r.text);
  }
  assert.ok(
    seen.size === PROMPTS.length,
    `Expected ${PROMPTS.length} unique responses, got ${seen.size} (duplicate detection)`
  );
});

test("pets brain: growformer_converse (website-equivalent path)", async () => {
  const gf = await loadGrowformer();
  const brainBytes = readFileSync(BRAIN_PATH);

  gf.growformer_load_brain(new Uint8Array(brainBytes));
  assert.ok(gf.growformer_ready(), "growformer should be ready");

  if (existsSync(INFERENCE_TOML_PATH)) {
    gf.growformer_load_inference_toml(readFileSync(INFERENCE_TOML_PATH, "utf-8"));
  }
  if (existsSync(TOPIC_GRAPH_PATH) && gf.growformer_load_topic_graph) {
    const base = readFileSync(TOPIC_GRAPH_PATH, "utf-8");
    const overlay = existsSync(TOPIC_GRAPH_OVERLAY_PATH)
      ? readFileSync(TOPIC_GRAPH_OVERLAY_PATH, "utf-8")
      : undefined;
    gf.growformer_load_topic_graph(base, overlay);
  }

  console.log("\n  --- growformer_converse (multi-turn, like website chat agent) ---");
  const seen = new Set<string>();
  for (const prompt of PROMPTS) {
    const raw = gf.growformer_converse(prompt);
    const r = parseResponse(raw);
    console.log(`  "${prompt}"`);
    console.log(`    → text: "${r.text}"`);
    console.log(`    → conf=${r.confidence.toFixed(4)} action=${r.action_type}`);
    assert.ok(r.text.length > 0, `empty text for "${prompt}"`);
    seen.add(r.text);
  }
  assert.ok(
    seen.size === PROMPTS.length,
    `Expected ${PROMPTS.length} unique responses, got ${seen.size} (duplicate detection)`
  );
});

test("pets brain: WITHOUT topic graph (reproduces website bug)", async () => {
  const gf = await loadGrowformer();
  const brainBytes = readFileSync(BRAIN_PATH);

  gf.growformer_load_brain(new Uint8Array(brainBytes));
  assert.ok(gf.growformer_ready(), "growformer should be ready");

  console.log("\n  --- NO topic graph, NO inference TOML (broken website config) ---");
  const seen = new Set<string>();
  for (const prompt of PROMPTS) {
    gf.growformer_reset_conversation();
    const raw = gf.growformer_generation(prompt);
    const r = parseResponse(raw);
    console.log(`  "${prompt}"`);
    console.log(`    → text: "${r.text}"`);
    console.log(`    → conf=${r.confidence.toFixed(4)} action=${r.action_type}`);
    seen.add(r.text);
  }
  if (seen.size < PROMPTS.length) {
    console.log(
      "\n  ⚠ DUPLICATE RESPONSES DETECTED — topic graph is required for prompt differentiation"
    );
  }
});
