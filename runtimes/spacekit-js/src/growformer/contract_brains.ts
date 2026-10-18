/**
 * One Growformer brain per contract.
 *
 * Contracts load their brain with `agent_growformer_load_brain_from_storage`. With a
 * Growformer build that has the instance API ({@link isGrowformerMultiInstance}), each
 * contract that does so gets its own {@link GrowformerInstance}: its own brain, conversation,
 * inference rules, topic graph and grounding graph. Contracts no longer replace each other's
 * brain, and two contracts with different brains can run on the same page.
 *
 * A contract that never loads a brain, a call with no contract id, or an older Growformer
 * build without the instance API all use the shared brain (instance 0, `runtime.ts`), exactly
 * as before.
 *
 * **Domain artifacts.** A contract's instance gets the same inference TOML, topic graph,
 * grounding graph and fragments the app loaded into the shared brain
 * (`loadGrowformerInferenceToml` and friends), applied after the brain like `runtime.ts` does.
 * {@link setGrowformerContractArtifacts} gives one contract its own instead. When the artifacts
 * a contract should use change, its instance is rebuilt from its brain bytes on the next call.
 * Inference guardrails JSONL has no per-instance loader and only reaches the shared brain.
 */

import { GrowformerInstance, isGrowformerMultiInstance } from "./instances.js";
import {
  growformerBrainSkipTag,
  growformerDomainArtifacts,
  growformerHostBrainInfoJson,
  growformerHostCodegenJson,
  growformerHostConverseJson,
  growformerHostGenerationJson,
  growformerHostStatusCode,
  isGrowformerModuleLoaded,
  loadGrowformerBrain,
  resetGrowformerConversation,
} from "./runtime.js";

/** Artifacts for one contract's brain. A field left out uses the app's; `null` means none. */
export interface GrowformerContractArtifacts {
  inferenceToml?: string | null;
  topicGraphBase?: string | null;
  topicGraphOverlay?: string | null;
  groundingGraph?: string | null;
  fragmentsJsonl?: string | null;
}

interface Applied {
  inferenceToml: string | null;
  topicGraphBase: string | null;
  topicGraphOverlay: string | null;
  groundingGraph: string | null;
  fragmentsJsonl: string | null;
}

interface Slot {
  instance: GrowformerInstance;
  bytes: Uint8Array;
  tag: [number, number];
  applied: Applied;
  lastUsed: number;
}

export const DEFAULT_GROWFORMER_CONTRACT_BRAIN_LIMIT = 8;

const slots = new Map<string, Slot>();
const overrides = new Map<string, GrowformerContractArtifacts>();
let limit = DEFAULT_GROWFORMER_CONTRACT_BRAIN_LIMIT;
let clock = 0;

function pick(own: string | null | undefined, app: string | null | undefined): string | null {
  const v = own === undefined ? app : own;
  return v && v.trim() ? v : null;
}

function wanted(contractId: string): Applied {
  const app = growformerDomainArtifacts();
  const own = overrides.get(contractId) ?? {};
  return {
    inferenceToml: pick(own.inferenceToml, app.inferenceToml),
    topicGraphBase: pick(own.topicGraphBase, app.topicGraphBase),
    topicGraphOverlay: pick(own.topicGraphOverlay, app.topicGraphOverlay),
    groundingGraph: pick(own.groundingGraph, app.groundingGraph),
    fragmentsJsonl: pick(own.fragmentsJsonl, app.fragmentsJsonl),
  };
}

function sameApplied(a: Applied, b: Applied): boolean {
  return (
    a.inferenceToml === b.inferenceToml &&
    a.topicGraphBase === b.topicGraphBase &&
    a.topicGraphOverlay === b.topicGraphOverlay &&
    a.groundingGraph === b.groundingGraph &&
    a.fragmentsJsonl === b.fragmentsJsonl
  );
}

/** A fresh instance with `bytes` loaded, then the artifacts (brain first: brain plugins can overwrite rules). */
function build(bytes: Uint8Array, artifacts: Applied): GrowformerInstance {
  const inst = GrowformerInstance.create();
  try {
    inst.loadBrain(bytes);
    if (artifacts.inferenceToml) inst.loadInferenceToml(artifacts.inferenceToml);
    if (artifacts.topicGraphBase) {
      inst.loadTopicGraph(artifacts.topicGraphBase, artifacts.topicGraphOverlay ?? undefined);
    }
    if (artifacts.groundingGraph) inst.loadGroundingGraph(artifacts.groundingGraph);
    if (artifacts.fragmentsJsonl) inst.loadFragmentsJsonl(artifacts.fragmentsJsonl);
    return inst;
  } catch (e) {
    inst.destroy();
    throw e;
  }
}

function drop(contractId: string): void {
  const slot = slots.get(contractId);
  if (!slot) return;
  slots.delete(contractId);
  try {
    slot.instance.destroy();
  } catch {
    /* module gone */
  }
}

/** Free the least recently used contract brain. */
function dropOldest(): void {
  let oldest: string | null = null;
  let at = Infinity;
  for (const [id, s] of slots) {
    if (s.lastUsed < at) {
      at = s.lastUsed;
      oldest = id;
    }
  }
  if (oldest !== null) drop(oldest);
}

/** The contract's own instance, rebuilt first if its artifacts changed; null = use the shared brain. */
function instanceFor(contractId: string | undefined): GrowformerInstance | null {
  if (!contractId) return null;
  const slot = slots.get(contractId);
  if (!slot) return null;
  slot.lastUsed = ++clock;
  const want = wanted(contractId);
  if (!sameApplied(slot.applied, want)) {
    const next = build(slot.bytes, want);
    slot.instance.destroy();
    slot.instance = next;
    slot.applied = want;
  }
  return slot.instance;
}

/** True when contracts get their own brains (instance API present). */
export function growformerContractBrainsEnabled(): boolean {
  return isGrowformerModuleLoaded() && isGrowformerMultiInstance();
}

/** How many contract brains stay loaded; loading one more frees the least recently used. */
export function setGrowformerContractBrainLimit(n: number): void {
  limit = Math.max(1, Math.floor(n));
  while (slots.size > limit) dropOldest();
}

/**
 * Give one contract its own domain artifacts (fields left out use the app's). Pass null to go
 * back to the app's. Takes effect on the contract's next call.
 */
export function setGrowformerContractArtifacts(
  contractId: string,
  artifacts: GrowformerContractArtifacts | null,
): void {
  if (artifacts) overrides.set(contractId, { ...artifacts });
  else overrides.delete(contractId);
}

/**
 * Load `bytes` as `contractId`'s brain. Without a contract id or the instance API this loads the
 * shared brain. An identical blob is not reloaded. Throws when the bytes are not a valid brain;
 * the contract then has no brain of its own.
 */
export function loadGrowformerContractBrain(contractId: string | undefined, bytes: Uint8Array): void {
  if (!contractId || !growformerContractBrainsEnabled()) {
    loadGrowformerBrain(bytes);
    return;
  }
  const tag = growformerBrainSkipTag(bytes);
  const existing = slots.get(contractId);
  if (
    existing &&
    existing.bytes.byteLength === bytes.byteLength &&
    existing.tag[0] === tag[0] &&
    existing.tag[1] === tag[1]
  ) {
    existing.lastUsed = ++clock;
    return;
  }
  drop(contractId);
  const applied = wanted(contractId);
  const instance = build(bytes, applied);
  while (slots.size >= limit) dropOldest();
  slots.set(contractId, { instance, bytes, tag, applied, lastUsed: ++clock });
}

/** Free a contract's brain; it falls back to the shared brain afterwards. */
export function unloadGrowformerContractBrain(contractId: string): boolean {
  const had = slots.has(contractId);
  drop(contractId);
  return had;
}

/** Contracts that currently have their own brain. */
export function growformerContractBrainIds(): string[] {
  return [...slots.keys()];
}

/** Free every contract brain (e.g. when the page switches network or resets the VM). */
export function clearGrowformerContractBrains(): void {
  for (const id of [...slots.keys()]) drop(id);
}

// ── Calls the contract host makes (`host.ts`) ──────────────────────────────────

/** 0 = not ready, 1 = ready, for the brain this contract uses. */
export function growformerContractStatusCode(contractId: string | undefined): number {
  try {
    const inst = instanceFor(contractId);
    if (!inst) return growformerHostStatusCode();
    return inst.ready() ? 1 : 0;
  } catch {
    return 0;
  }
}

/** Single-shot generation from a fresh conversation. Throws when not ready. */
export function growformerContractGenerationJson(contractId: string | undefined, prompt: string): string {
  const inst = instanceFor(contractId);
  return inst ? inst.generation(prompt) : growformerHostGenerationJson(prompt);
}

export function growformerContractConverseJson(contractId: string | undefined, prompt: string): string {
  const inst = instanceFor(contractId);
  return inst ? inst.converse(prompt) : growformerHostConverseJson(prompt);
}

export function growformerContractCodegenJson(contractId: string | undefined, prompt: string): string {
  const inst = instanceFor(contractId);
  return inst ? inst.codegen(prompt) : growformerHostCodegenJson(prompt);
}

export function growformerContractBrainInfoJson(contractId: string | undefined): string {
  const inst = instanceFor(contractId);
  return inst ? inst.brainInfoJson() : growformerHostBrainInfoJson();
}

export function resetGrowformerContractConversation(contractId: string | undefined): void {
  const inst = instanceFor(contractId);
  if (!inst) {
    resetGrowformerConversation();
    return;
  }
  try {
    inst.resetConversation();
  } catch {
    /* ignore */
  }
}
