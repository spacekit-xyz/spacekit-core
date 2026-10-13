/**
 * Several Growformer brains on one page.
 *
 * The functions in `runtime.ts` drive one brain (the wasm runtime's instance 0).
 * A Growformer build with the instance API (`growformer_instance_create` and
 * the `*_i` exports, spacekit-ai `growformer/src/wasm.rs`) can hold more:
 * each {@link GrowformerInstance} has its own brain, conversation, inference
 * rules, topic graph and grounding graph.
 *
 * ```ts
 * await initGrowformerHost();
 * const support = GrowformerInstance.create();
 * support.loadBrain(supportBrainBytes);
 * const reply = JSON.parse(support.converse("How do I reset my password?"));
 * support.destroy();
 * ```
 *
 * Older Growformer builds have no instance API: {@link isGrowformerMultiInstance}
 * returns false and `GrowformerInstance.create()` throws.
 */

import { recordGrowformerTraffic } from "./capture.js";
import { loadedGrowformerModule } from "./runtime.js";

export interface GrowformerEngineInfo {
  engine: string;
  version: string;
  /** Brain package format versions this build reads. */
  brain_formats: number[];
  /** Whether gzip-wrapped (`GWFCMPKG`) brains load. */
  compressed_brains: boolean;
  multi_instance: boolean;
}

type Fn = (...args: unknown[]) => unknown;

function mod(): Record<string, unknown> {
  const m = loadedGrowformerModule();
  if (!m) throw new Error("Growformer not initialized; await initGrowformerHost() first");
  return m;
}

function fn(name: string): Fn {
  const f = mod()[name];
  if (typeof f !== "function") {
    throw new Error(`This Growformer build has no ${name}; rebuild growformer-pkg with the instance API`);
  }
  return f as Fn;
}

function asJson(value: unknown): string {
  return typeof value === "string" ? value : JSON.stringify(value);
}

/**
 * What the loaded Growformer build reports about itself, or null when it is
 * not loaded or predates `growformer_engine_info`.
 */
export function growformerEngineInfo(): GrowformerEngineInfo | null {
  const m = loadedGrowformerModule();
  const f = m?.growformer_engine_info;
  if (typeof f !== "function") return null;
  try {
    const raw = (f as Fn)();
    return JSON.parse(asJson(raw)) as GrowformerEngineInfo;
  } catch {
    return null;
  }
}

/** True when the loaded Growformer build can hold more than one brain. */
export function isGrowformerMultiInstance(): boolean {
  const m = loadedGrowformerModule();
  return !!m && typeof m.growformer_instance_create === "function";
}

/** Ids of the live instances (instance 0 is the one `runtime.ts` uses). */
export function listGrowformerInstances(): number[] {
  return JSON.parse(asJson(fn("growformer_instance_list")())) as number[];
}

export class GrowformerInstance {
  private destroyed = false;

  private constructor(readonly id: number) {}

  /** Create an empty instance. Load a brain into it before asking it anything. */
  static create(): GrowformerInstance {
    const id = Number(fn("growformer_instance_create")());
    if (!Number.isInteger(id) || id <= 0) throw new Error(`Growformer returned an invalid instance id ${id}`);
    return new GrowformerInstance(id);
  }

  private call(name: string, ...args: unknown[]): unknown {
    if (this.destroyed) throw new Error(`Growformer instance ${this.id} was destroyed`);
    return fn(`${name}_i`)(this.id, ...args);
  }

  /** Load a brain package (GWFBRPKG, or GWFCMPKG when the build supports it). */
  loadBrain(bytes: Uint8Array): void {
    this.call("growformer_load_brain", bytes);
  }

  loadInferenceToml(toml: string): void {
    const trimmed = toml.trim();
    if (trimmed) this.call("growformer_load_inference_toml", trimmed);
  }

  /** Returns how many fragments were loaded. */
  loadFragmentsJsonl(jsonl: string): number {
    const trimmed = jsonl.trim();
    return trimmed ? Number(this.call("growformer_load_fragments_jsonl", trimmed)) : 0;
  }

  loadTopicGraph(baseToml: string, overlayToml?: string): void {
    this.call("growformer_clear_topic_graph");
    this.call("growformer_load_topic_graph", baseToml, overlayToml);
  }

  loadGroundingGraph(toml: string): void {
    const trimmed = toml.trim();
    if (trimmed) this.call("growformer_load_grounding_graph", trimmed);
  }

  setAgentState(state: Record<string, unknown>): void {
    this.call("growformer_set_agent_state", JSON.stringify(state));
  }

  ready(): boolean {
    return Boolean(this.call("growformer_ready"));
  }

  /** Brain metadata as JSON text. */
  brainInfoJson(): string {
    return asJson(this.call("growformer_brain_info"));
  }

  resetConversation(): void {
    this.call("growformer_reset_conversation");
  }

  /** Multi-turn chat; returns the engine's JSON reply. */
  converse(prompt: string): string {
    const json = asJson(this.call("growformer_converse", prompt));
    recordGrowformerTraffic(prompt, json);
    return json;
  }

  /** Single-shot generation from a fresh conversation. */
  generation(prompt: string): string {
    this.resetConversation();
    try {
      const json = asJson(this.call("growformer_generation", prompt));
      recordGrowformerTraffic(prompt, json);
      return json;
    } finally {
      this.resetConversation();
    }
  }

  codegen(prompt: string): string {
    this.resetConversation();
    try {
      return asJson(this.call("growformer_codegen", prompt));
    } finally {
      this.resetConversation();
    }
  }

  /** Free the brain. The instance cannot be used afterwards. */
  destroy(): boolean {
    if (this.destroyed) return false;
    this.destroyed = true;
    return Boolean(fn("growformer_instance_destroy")(this.id));
  }
}
