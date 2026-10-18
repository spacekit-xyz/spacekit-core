import { test } from "node:test";
import assert from "node:assert/strict";

import { initGrowformerHost, loadGrowformerBrain, loadGrowformerInferenceToml } from "./runtime.js";
import {
  clearGrowformerContractBrains,
  growformerContractBrainIds,
  growformerContractConverseJson,
  growformerContractGenerationJson,
  growformerContractStatusCode,
  loadGrowformerContractBrain,
  setGrowformerContractArtifacts,
  setGrowformerContractBrainLimit,
  unloadGrowformerContractBrain,
} from "./contract_brains.js";

// A stand-in for the wasm-bindgen module with the instance API. Each instance remembers its
// brain (byte length), rules and topic graph, and replies with them so routing is observable.
// `log` records the order of calls so we can check "brain first, then artifacts".
const fake = `
const inst = new Map([[0, { brain: null, rules: null, topics: null }]]);
let next = 1;
export const log = [];
const get = (id) => { const i = inst.get(id); if (!i) throw new Error("unknown growformer instance " + id); return i; };
const reply = (id, t) => { const i = get(id); return JSON.stringify({ id, brain: i.brain, rules: i.rules, topics: i.topics, said: t }); };
export default async function init() {}
export function growformer_init() {}
export function growformer_ready() { return get(0).brain !== null; }
export function growformer_load_brain(b) { get(0).brain = b.length; }
export function growformer_load_inference_toml(t) { get(0).rules = t; }
export function growformer_reset_conversation() {}
export function growformer_generation(t) { return reply(0, t); }
export function growformer_converse(t) { return reply(0, t); }
export function growformer_engine_info() { return JSON.stringify({ engine: "growformer", version: "0.1.0", brain_formats: [1, 2], compressed_brains: false, multi_instance: true }); }
export function growformer_instance_create() { const id = next++; inst.set(id, { brain: null, rules: null, topics: null }); return id; }
export function growformer_instance_destroy(id) { return inst.delete(id); }
export function growformer_instance_list() { return JSON.stringify([...inst.keys()]); }
export function growformer_load_brain_i(id, b) { if (b.length === 1) throw new Error("bad brain"); log.push("brain:" + id); get(id).brain = b.length; }
export function growformer_load_inference_toml_i(id, t) { log.push("rules:" + id); get(id).rules = t; }
export function growformer_clear_topic_graph_i(id) { get(id).topics = null; }
export function growformer_load_topic_graph_i(id, base) { get(id).topics = base; }
export function growformer_ready_i(id) { return get(id).brain !== null; }
export function growformer_reset_conversation_i(id) { get(id); }
export function growformer_converse_i(id, t) { return reply(id, t); }
export function growformer_generation_i(id, t) { return reply(id, t); }
`;

const say = (contract: string | undefined, text = "hi") =>
  JSON.parse(growformerContractConverseJson(contract, text)) as {
    id: number;
    brain: number | null;
    rules: string | null;
    topics: string | null;
  };

test("each contract that loads a brain gets its own", async () => {
  const url = `data:text/javascript,${encodeURIComponent(fake)}`;
  await initGrowformerHost({ scriptModuleUrl: url });
  const { log } = (await import(url)) as { log: string[] };

  // App-level shared brain and rules (instance 0).
  loadGrowformerBrain(new Uint8Array(3));
  loadGrowformerInferenceToml("[rules]\napp=1");

  // A contract that never loads a brain uses the shared one; so does a call with no contract.
  assert.equal(growformerContractStatusCode("C0"), 1);
  assert.equal(say("C0").id, 0);
  assert.equal(say(undefined).id, 0);

  // Two contracts load different brains and stay apart; the app's rules follow each brain.
  loadGrowformerContractBrain("A", new Uint8Array(5));
  loadGrowformerContractBrain("B", new Uint8Array(7));
  const a = say("A");
  const b = say("B");
  assert.notEqual(a.id, b.id);
  assert.equal(a.brain, 5);
  assert.equal(b.brain, 7);
  assert.equal(a.rules, "[rules]\napp=1");
  assert.equal(say("C0").brain, 3, "shared brain untouched");
  assert.ok(log.indexOf(`brain:${a.id}`) < log.indexOf(`rules:${a.id}`), "brain loads before rules");
  assert.equal(growformerContractGenerationJson("A", "x").includes('"brain":5'), true);

  // Reloading an identical blob keeps the same instance (no re-parse).
  loadGrowformerContractBrain("A", new Uint8Array(5));
  assert.equal(say("A").id, a.id);

  // A per-contract override rebuilds just that contract's instance on its next call.
  setGrowformerContractArtifacts("B", { inferenceToml: "[rules]\nb=1", topicGraphBase: "topics-b" });
  const b2 = say("B");
  assert.notEqual(b2.id, b.id);
  assert.equal(b2.brain, 7);
  assert.equal(b2.rules, "[rules]\nb=1");
  assert.equal(b2.topics, "topics-b");
  assert.equal(say("A").id, a.id, "A not rebuilt");

  // Changing the app's rules reaches contracts that use them, not B's override.
  loadGrowformerInferenceToml("[rules]\napp=2");
  assert.equal(say("A").rules, "[rules]\napp=2");
  assert.equal(say("B").rules, "[rules]\nb=1");

  // Limit 2: loading C frees the least recently used (A, since B was used last).
  setGrowformerContractBrainLimit(2);
  say("B");
  loadGrowformerContractBrain("C", new Uint8Array(9));
  assert.deepEqual(growformerContractBrainIds().sort(), ["B", "C"]);
  assert.equal(say("A").id, 0, "evicted contract falls back to the shared brain");

  // A bad brain leaves the contract without its own brain.
  assert.throws(() => loadGrowformerContractBrain("C", new Uint8Array(1)), /bad brain/);
  assert.equal(say("C").id, 0);

  assert.equal(unloadGrowformerContractBrain("B"), true);
  assert.equal(unloadGrowformerContractBrain("B"), false);
  clearGrowformerContractBrains();
  assert.deepEqual(growformerContractBrainIds(), []);
});
