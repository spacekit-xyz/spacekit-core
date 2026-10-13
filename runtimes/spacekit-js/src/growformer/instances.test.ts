import { test } from "node:test";
import assert from "node:assert/strict";

import { initGrowformerHost, loadGrowformerBrain } from "./runtime.js";
import {
  GrowformerInstance,
  growformerEngineInfo,
  isGrowformerMultiInstance,
  listGrowformerInstances,
} from "./instances.js";

// A stand-in for the wasm-bindgen module with the instance API: each instance
// remembers its brain and rules, so isolation is observable.
const fake = `
const inst = new Map([[0, { brain: null, rules: null }]]);
let next = 1;
const get = (id) => { const i = inst.get(id); if (!i) throw new Error("unknown growformer instance " + id); return i; };
export default async function init() {}
export function growformer_init() { inst.set(0, { brain: null, rules: null }); }
export function growformer_ready() { return get(0).brain !== null; }
export function growformer_load_brain(b) { get(0).brain = b.length; }
export function growformer_load_inference_toml(t) { get(0).rules = t; }
export function growformer_converse(t) { const i = get(0); return JSON.stringify({ brain: i.brain, rules: i.rules, said: t }); }
export function growformer_engine_info() { return JSON.stringify({ engine: "growformer", version: "0.1.0", brain_formats: [1, 2], compressed_brains: true, multi_instance: true }); }
export function growformer_instance_create() { const id = next++; inst.set(id, { brain: null, rules: null }); return id; }
export function growformer_instance_destroy(id) { return inst.delete(id); }
export function growformer_instance_list() { return JSON.stringify([...inst.keys()]); }
export function growformer_load_brain_i(id, b) { get(id).brain = b.length; }
export function growformer_load_inference_toml_i(id, t) { get(id).rules = t; }
export function growformer_ready_i(id) { return get(id).brain !== null; }
export function growformer_reset_conversation_i(id) { get(id); }
export function growformer_converse_i(id, t) { const i = get(id); return JSON.stringify({ brain: i.brain, rules: i.rules, said: t }); }
export function growformer_generation_i(id, t) { return growformer_converse_i(id, t); }
`;

test("instances keep separate brains and rules", async () => {
  await initGrowformerHost({ scriptModuleUrl: `data:text/javascript,${encodeURIComponent(fake)}` });
  assert.equal(isGrowformerMultiInstance(), true);
  assert.equal(growformerEngineInfo()?.multi_instance, true);

  loadGrowformerBrain(new Uint8Array(3)); // instance 0, the legacy API
  const a = GrowformerInstance.create();
  const b = GrowformerInstance.create();
  assert.notEqual(a.id, b.id);
  assert.equal(a.ready(), false);
  a.loadBrain(new Uint8Array(5));
  a.loadInferenceToml("[rules]\na=1\n");
  b.loadBrain(new Uint8Array(7));

  assert.deepEqual(JSON.parse(a.converse("hi")), { brain: 5, rules: "[rules]\na=1", said: "hi" });
  assert.deepEqual(JSON.parse(b.generation("yo")), { brain: 7, rules: null, said: "yo" });
  assert.deepEqual(listGrowformerInstances().sort(), [0, a.id, b.id].sort());

  assert.equal(a.destroy(), true);
  assert.equal(a.destroy(), false);
  assert.throws(() => a.converse("x"), /destroyed/);
  assert.deepEqual(listGrowformerInstances().sort(), [0, b.id].sort());
  // Missing exports in older builds give a clear message.
  assert.throws(() => b.codegen("x"), /rebuild growformer-pkg/);
});
