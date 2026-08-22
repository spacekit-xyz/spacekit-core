/**
 * Lazy-loaded Growformer WASM (wasm-bindgen pkg). Used by `spacekit_agent` host imports.
 *
 * Apps must call `initGrowformerHost()` (and usually `growformer_load_brain`) before
 * contracts that import `spacekit_agent` can succeed.
 *
 * **Browser (wasm32-unknown-unknown):** `growformer_bg.wasm` must be built without panicking
 * `std::time::Instant::now` (use `instant` / `web-time` or gate timers). Stock `std` time
 * aborts with `time not implemented on this platform` inside the WASM.
 */

import { recordGrowformerTraffic } from "./capture.js";
import { sha256Hex } from "../vm/hash.js";

export type GrowformerInitInput =
  | RequestInfo
  | URL
  | Response
  | BufferSource
  | WebAssembly.Module
  | undefined;

export const DEFAULT_GROWFORMER_BRAIN_MAX_BYTES = 1024 * 1024 * 1024;

export type GrowformerBrainFetchPhase = "download" | "verify" | "load";

export interface GrowformerBrainFetchProgress {
  phase: GrowformerBrainFetchPhase;
  bytesReceived: number;
  totalBytes?: number;
}

export type GrowformerBrainFetcher = (
  input: RequestInfo | URL,
  init?: RequestInit,
) => Promise<Response>;

export interface GrowformerBrainFetchOptions {
  /** Fetch implementation to use. Defaults to `globalThis.fetch`. */
  fetcher?: GrowformerBrainFetcher;
  signal?: AbortSignal;
  /** Maximum accepted brain size. Defaults to 1 GiB. */
  maxBytes?: number;
  /** Optional, case-insensitive SHA-256 digest encoded as 64 hexadecimal characters. */
  expectedSha256Hex?: string;
  onProgress?: (progress: GrowformerBrainFetchProgress) => void;
}

export interface InitGrowformerHostWithBrainFromUrlOptions
  extends GrowformerBrainFetchOptions {
  moduleOrPath?: GrowformerInitInput | Promise<GrowformerInitInput>;
  /**
   * Vite `?url` import of `growformer-pkg/growformer.js` so the host loads from bundled assets.
   */
  scriptModuleUrl?: string;
}

type GrowformerWasmModule = {
  default: (
    moduleOrPath?: GrowformerInitInput | Promise<GrowformerInitInput> | { module_or_path: GrowformerInitInput | Promise<GrowformerInitInput> }
  ) => Promise<unknown>;
  growformer_init: () => void;
  growformer_load_brain: (data: Uint8Array) => void;
  growformer_ready: () => boolean;
  growformer_brain_info: () => unknown;
  growformer_generation: (text: string) => unknown;
  growformer_converse: (text: string) => unknown;
  growformer_codegen: (text: string) => unknown;
  growformer_reset_conversation: () => void;
  growformer_load_inference_toml: (toml_str: string) => void;
  growformer_load_inference_guardrails_jsonl?: (jsonl: string) => void;
  growformer_load_fragments_jsonl?: (jsonl: string) => number;
  growformer_load_topic_graph?: (base_toml: string, overlay_toml?: string) => void;
  growformer_clear_topic_graph?: () => void;
  growformer_load_grounding_graph?: (toml_str: string) => void;
  growformer_set_agent_state?: (state_json: string) => void;
  growformer_inference_rules_info?: () => unknown;
};

let gf: GrowformerWasmModule | null = null;
let initPromise: Promise<void> | null = null;

/**
 * Fingerprint of the last blob passed to `growformer_load_brain`. Contracts such as
 * `spacekit-growformer-sentiment-analysis` reload from VM storage on **every** `handle` call; without
 * this, each prompt re-parses the full brain in WASM (very slow / memory-heavy on mobile).
 */
let lastBrainCache: { len: number; tag0: number; tag1: number } | null = null;

/** Fingerprint of the last fragments JSONL passed to `growformer_load_fragments_jsonl`. */
let lastFragmentsJsonlCache: string | null = null;

/** Fingerprint of the last guardrails JSONL passed to `growformer_load_inference_guardrails_jsonl`. */
let lastGuardrailsJsonlCache: string | null = null;

/** Fingerprint of the last inference TOML passed to `growformer_load_inference_toml`. */
let lastInferenceTomlCache: string | null = null;

/**
 * Constant-time-ish identity for skip (head/tail/mid samples + length). Full FNV over multi‑MB
 * brains walked the entire blob on every contract `handle` and could freeze mobile WebKit right
 * after "Thinking" before inference even started.
 */
function brainSkipTag(data: Uint8Array): [number, number] {
  const n = data.length;
  let t0 = (n * 0x9e3779b1) >>> 0;
  let t1 = (n ^ 0xa5a5_a5a5) >>> 0;
  const head = Math.min(48, n);
  for (let i = 0; i < head; i++) {
    t0 = (Math.imul(t0, 31) + data[i]) >>> 0;
  }
  const tail = Math.min(48, n);
  for (let i = 0; i < tail; i++) {
    t1 = (Math.imul(t1, 31) + data[n - 1 - i]) >>> 0;
  }
  if (n > 96) {
    const q = n >> 2;
    t0 ^= data[q] | (data[q + (q >> 1)] << 8);
    t1 ^= data[q * 2] | (data[q * 3] << 8);
  }
  return [t0 >>> 0, t1 >>> 0];
}

/** @returns true when bytes were applied to the host (false = skip-cache hit). */
function applyGrowformerBrainBytes(data: Uint8Array): boolean {
  if (!gf) {
    throw new Error("Growformer not initialized; await initGrowformerHost() first");
  }
  const [tag0, tag1] = brainSkipTag(data);
  if (
    lastBrainCache &&
    lastBrainCache.len === data.byteLength &&
    lastBrainCache.tag0 === tag0 &&
    lastBrainCache.tag1 === tag1
  ) {
    return false;
  }
  gf.growformer_load_brain(data);
  lastBrainCache = { len: data.byteLength, tag0, tag1 };
  return true;
}

function defaultGrowformerScriptUrl(): string {
  if (typeof window !== "undefined" && window.location?.origin) {
    return `${window.location.origin}/growformer-pkg/growformer.js`;
  }
  return new URL("../../growformer-pkg/growformer.js", import.meta.url).href;
}

/** True after `initGrowformerHost` finished loading the Growformer wasm-bindgen module (brain optional). */
export function isGrowformerModuleLoaded(): boolean {
  return gf !== null;
}

/**
 * Load Growformer WASM, call `growformer_init`, and optionally load a brain.
 * Safe to call multiple times; subsequent calls only apply `brainBytes` if provided.
 */
export async function initGrowformerHost(options?: {
  /** Passed to wasm-bindgen init (URL, bytes, Module, etc.). Default loads `growformer_bg.wasm` next to the JS glue. */
  moduleOrPath?: GrowformerInitInput | Promise<GrowformerInitInput>;
  brainBytes?: Uint8Array;
  /**
   * Vite `?url` import of `growformer-pkg/growformer.js` so the host loads from bundled assets
   * instead of deprecated `/public/growformer-pkg/`.
   */
  scriptModuleUrl?: string;
}): Promise<void> {
  if (gf) {
    if (options?.brainBytes) {
      applyGrowformerBrainBytes(options.brainBytes);
    }
    return;
  }
  if (initPromise) {
    await initPromise;
    if (options?.brainBytes) {
      if (gf) {
        applyGrowformerBrainBytes(options.brainBytes);
      }
    }
    return;
  }

  initPromise = (async () => {
    lastBrainCache = null;
    const scriptUrl = options?.scriptModuleUrl ?? defaultGrowformerScriptUrl();
    const m = (await import(
      /* @vite-ignore */ scriptUrl
    )) as GrowformerWasmModule;
    await m.default(options?.moduleOrPath as GrowformerInitInput | undefined);
    m.growformer_init();
    gf = m;
    if (options?.brainBytes) {
      applyGrowformerBrainBytes(options.brainBytes);
    }
  })();

  try {
    await initPromise;
  } finally {
    initPromise = null;
  }
}

/** True after WASM is loaded and `growformer_ready()` is true. */
export function isGrowformerHostReady(): boolean {
  try {
    return gf !== null && gf.growformer_ready();
  } catch {
    return false;
  }
}

/** 0 = not ready, 1 = ready (mirrors contract SDK `growformer_status`). */
export function growformerHostStatusCode(): number {
  return isGrowformerHostReady() ? 1 : 0;
}

export function loadGrowformerBrain(data: Uint8Array): void {
  const loaded = applyGrowformerBrainBytes(data);
  // Re-merge domain inference TOML + fragments only after a real brain swap — not on every
  // contract turn (load_brain_from_storage is idempotent but runs each message).
  if (loaded) {
    reapplyGrowformerDomainArtifactsAfterBrainLoad();
  }
}

/** Re-merge cached inference TOML after load_brain (brain plugins can overwrite host rules). */
function reapplyGrowformerInferenceTomlAfterBrainLoad(): void {
  if (!gf || !lastInferenceTomlCache) {
    return;
  }
  gf.growformer_load_inference_toml(lastInferenceTomlCache);
}

/** Re-merge cached fragments JSONL after load_brain (composer stays on the service object). */
function reapplyGrowformerFragmentsAfterBrainLoad(): void {
  if (!gf || !lastFragmentsJsonlCache) {
    return;
  }
  if (!gf.growformer_load_fragments_jsonl) {
    return;
  }
  gf.growformer_load_fragments_jsonl(lastFragmentsJsonlCache);
}

function reapplyGrowformerGuardrailsAfterBrainLoad(): void {
  if (!gf || !lastGuardrailsJsonlCache) {
    return;
  }
  if (!gf.growformer_load_inference_guardrails_jsonl) {
    return;
  }
  gf.growformer_load_inference_guardrails_jsonl(lastGuardrailsJsonlCache);
}

function reapplyGrowformerDomainArtifactsAfterBrainLoad(): void {
  reapplyGrowformerInferenceTomlAfterBrainLoad();
  reapplyGrowformerGuardrailsAfterBrainLoad();
  reapplyGrowformerTopicGraphAfterBrainLoad();
  reapplyGrowformerGroundingGraphAfterBrainLoad();
  reapplyGrowformerFragmentsAfterBrainLoad();
}

/** Drop the host skip-cache so the next `growformer_load_brain_from_storage` re-applies bytes (e.g. after re-seeding VM storage). */
export function clearGrowformerBrainCache(): void {
  lastBrainCache = null;
}

/** Drop cached inference TOML so the next load re-applies rules (e.g. when switching agents). */
export function clearGrowformerInferenceTomlCache(): void {
  lastInferenceTomlCache = null;
}

/** Drop cached guardrails JSONL so the next load re-applies rules (e.g. when switching agents). */
export function clearGrowformerGuardrailsCache(): void {
  lastGuardrailsJsonlCache = null;
}

/** Drop cached fragments JSONL so the next load re-applies the library (e.g. when switching agents). */
export function clearGrowformerFragmentsCache(): void {
  lastFragmentsJsonlCache = null;
}

/** Drop cached topic graph so the next load re-applies routing rules (e.g. when switching agents). */
export function clearGrowformerTopicGraphCache(): void {
  lastTopicGraphCache = null;
  lastTopicGraphBase = null;
  lastTopicGraphOverlay = undefined;
  try {
    gf?.growformer_clear_topic_graph?.();
  } catch {
    /* ignore */
  }
}

/** Drop cached grounding graph so the next load re-applies concept expansion. */
export function clearGrowformerGroundingGraphCache(): void {
  lastGroundingGraphCache = null;
}

/**
 * Load domain-specific inference rules into the Growformer host (e.g. `inference_pets.toml`).
 * Must be called before the first inference for chat-mode brains. Safe to call multiple times;
 * identical TOML content is skipped.
 */
export function loadGrowformerInferenceToml(toml: string): void {
  if (!gf) {
    throw new Error("Growformer not initialized; await initGrowformerHost() first");
  }
  const trimmed = toml.trim();
  if (!trimmed) {
    return;
  }
  if (lastInferenceTomlCache === trimmed) {
    return;
  }
  gf.growformer_load_inference_toml(trimmed);
  lastInferenceTomlCache = trimmed;
  if (lastGuardrailsJsonlCache) {
    reapplyGrowformerGuardrailsAfterBrainLoad();
  }
}

/**
 * Load inference guardrails JSONL into the Growformer host (lattice_misfire / lexical_topic rows).
 * Call after `loadGrowformerInferenceToml`. Safe to call multiple times; identical content is skipped.
 */
export function loadGrowformerInferenceGuardrailsJsonl(jsonl: string): void {
  if (!gf) {
    throw new Error("Growformer not initialized; await initGrowformerHost() first");
  }
  const trimmed = jsonl.trim();
  if (!trimmed) {
    return;
  }
  if (lastGuardrailsJsonlCache === trimmed) {
    return;
  }
  if (!gf.growformer_load_inference_guardrails_jsonl) {
    console.warn(
      "[Growformer] growformer_load_inference_guardrails_jsonl not available in this WASM build",
    );
    return;
  }
  gf.growformer_load_inference_guardrails_jsonl(trimmed);
  lastGuardrailsJsonlCache = trimmed;
}

/**
 * Load a JSONL fragment library into the Growformer host for chat-mode composition.
 * Must be called after `loadGrowformerInferenceToml` when `[fragment_compose]` is enabled.
 * Safe to call multiple times; identical content is skipped.
 */
export function loadGrowformerFragmentsJsonl(jsonl: string): void {
  if (!gf) {
    throw new Error("Growformer not initialized; await initGrowformerHost() first");
  }
  const trimmed = jsonl.trim();
  if (!trimmed) {
    return;
  }
  if (lastFragmentsJsonlCache === trimmed) {
    return;
  }
  if (!gf.growformer_load_fragments_jsonl) {
    console.warn("[Growformer] growformer_load_fragments_jsonl not available in this WASM build");
    return;
  }
  gf.growformer_load_fragments_jsonl(trimmed);
  lastFragmentsJsonlCache = trimmed;
}

/** Fingerprint of the last topic graph loaded. */
let lastTopicGraphCache: string | null = null;
let lastTopicGraphBase: string | null = null;
let lastTopicGraphOverlay: string | undefined;

/**
 * Load knowledge graph TOML into the Growformer topic router.
 * Without this, WASM inference cannot route prompts to the correct topic sub-lattice
 * (e.g., "Hey Luna" → greeting_check_in, "vacuum" → trigger_warning).
 *
 * `baseToml` is the primary knowledge graph; `overlayToml` (optional) is merged on top.
 * Safe to call multiple times; identical content is skipped.
 */
export function loadGrowformerTopicGraph(baseToml: string, overlayToml?: string): void {
  if (!gf) {
    throw new Error("Growformer not initialized; await initGrowformerHost() first");
  }
  if (!gf.growformer_load_topic_graph) {
    return;
  }
  const cacheKey = baseToml + (overlayToml ?? "");
  if (lastTopicGraphCache === cacheKey) {
    return;
  }
  gf.growformer_clear_topic_graph?.();
  gf.growformer_load_topic_graph(baseToml, overlayToml);
  lastTopicGraphCache = cacheKey;
  lastTopicGraphBase = baseToml;
  lastTopicGraphOverlay = overlayToml;
}

/** Fingerprint of the last grounding graph loaded. */
let lastGroundingGraphCache: string | null = null;

/**
 * Load a domain-specific grounding graph (e.g. `pet_world_grounding.toml`)
 * into the Growformer keyword expansion engine.
 *
 * The grounding graph provides typed concept edges that expand BM25 keywords
 * and resolve anchors during inference (e.g. "hungry" → appetite, fullness, feeding).
 * Safe to call multiple times; identical content is skipped.
 */
export function loadGrowformerGroundingGraph(toml: string): void {
  if (!gf) {
    throw new Error("Growformer not initialized; await initGrowformerHost() first");
  }
  const trimmed = toml.trim();
  if (!trimmed || lastGroundingGraphCache === trimmed) {
    return;
  }
  if (!gf.growformer_load_grounding_graph) {
    return;
  }
  gf.growformer_load_grounding_graph(trimmed);
  lastGroundingGraphCache = trimmed;
}

/** Re-merge cached topic graph after load_brain (routing must survive brain plugin reload). */
function reapplyGrowformerTopicGraphAfterBrainLoad(): void {
  if (!gf || !lastTopicGraphBase || !gf.growformer_load_topic_graph) {
    return;
  }
  gf.growformer_clear_topic_graph?.();
  gf.growformer_load_topic_graph(lastTopicGraphBase, lastTopicGraphOverlay);
}

/** Re-merge cached grounding graph after load_brain. */
function reapplyGrowformerGroundingGraphAfterBrainLoad(): void {
  if (!gf || !lastGroundingGraphCache || !gf.growformer_load_grounding_graph) {
    return;
  }
  gf.growformer_load_grounding_graph(lastGroundingGraphCache);
}

/**
 * Set runtime agent state for state-conditioned generation.
 * Accepts a generic state object with arbitrary float dimensions,
 * an optional profile string, turn counter, and idle time.
 *
 * Example:
 * ```ts
 * setGrowformerAgentState({
 *   dimensions: { hunger: 0.4, energy: 0.6, mood: 0.7 },
 *   profile: "cheerful_companion",
 *   turn: 3,
 *   minutes_idle: 12.5,
 * });
 * ```
 */
export function setGrowformerAgentState(state: {
  dimensions?: Record<string, number>;
  profile?: string;
  turn?: number;
  minutes_idle?: number;
}): void {
  if (!gf) {
    throw new Error("Growformer not initialized; await initGrowformerHost() first");
  }
  if (!gf.growformer_set_agent_state) {
    return;
  }
  gf.growformer_set_agent_state(JSON.stringify(state));
}

export function resetGrowformerConversation(): void {
  if (!gf) {
    return;
  }
  try {
    gf.growformer_reset_conversation();
  } catch {
    /* ignore */
  }
}

/**
 * Normalize WASM return to a JSON string. The Rust WASM bindings return
 * `JsValue::from_str(&serde_json::to_string(...))` — already a JSON string.
 * Passing through JSON.stringify would double-encode (wrapping in quotes and
 * escaping inner quotes), wasting contract buffer space and complicating parsing.
 */
function ensureJsonString(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  return JSON.stringify(value);
}

/**
 * @returns JSON string; throws if not ready or Growformer throws
 *
 * **Determinism vs CLI:** Browser runs `growformer_bg.wasm` (wasm32); `./growformer --infer` uses
 * native code. Near-threshold labels (e.g. MIXED vs POSITIVE) can disagree even when the prose tail
 * matches — not a parsing bug in Agent Hub.
 *
 * **Session state:** Reset before *and* after each single-shot call so routing matches a fresh CLI
 * infer and no stray multi-turn state leaks between prompts.
 */
export function growformerHostGenerationJson(prompt: string): string {
  if (!gf || !gf.growformer_ready()) {
    throw new Error("Growformer not ready");
  }
  resetGrowformerConversation();
  try {
    const json = ensureJsonString(gf.growformer_generation(prompt));
    recordGrowformerTraffic(prompt, json);
    return json;
  } finally {
    resetGrowformerConversation();
  }
}

export function growformerHostConverseJson(prompt: string): string {
  if (!gf || !gf.growformer_ready()) {
    throw new Error("Growformer not ready");
  }
  const json = ensureJsonString(gf.growformer_converse(prompt));
  recordGrowformerTraffic(prompt, json);
  return json;
}

export function growformerHostCodegenJson(prompt: string): string {
  if (!gf || !gf.growformer_ready()) {
    throw new Error("Growformer not ready");
  }
  resetGrowformerConversation();
  try {
    const json = ensureJsonString(gf.growformer_codegen(prompt));
    recordGrowformerTraffic(prompt, json);
    return json;
  } finally {
    resetGrowformerConversation();
  }
}

export function growformerHostBrainInfoJson(): string {
  if (!gf || !gf.growformer_ready()) {
    throw new Error("Growformer not ready");
  }
  return ensureJsonString(gf.growformer_brain_info());
}

export function growformerHostInferenceRulesInfoJson(): string {
  if (!gf || !gf.growformer_ready()) {
    throw new Error("Growformer not ready");
  }
  if (!gf.growformer_inference_rules_info) {
    return '{"error":"growformer_inference_rules_info not available in this WASM build"}';
  }
  return ensureJsonString(gf.growformer_inference_rules_info());
}

function validateMaxBytes(maxBytes: number): void {
  if (!Number.isSafeInteger(maxBytes) || maxBytes < 0) {
    throw new RangeError("Growformer brain maxBytes must be a non-negative safe integer");
  }
}

function parseContentLength(value: string | null): number | undefined {
  if (value === null) {
    return undefined;
  }
  const trimmed = value.trim();
  if (!/^\d+$/.test(trimmed)) {
    throw new Error(`Invalid Growformer brain Content-Length: ${value}`);
  }
  const length = Number(trimmed);
  if (!Number.isSafeInteger(length)) {
    throw new Error(`Invalid Growformer brain Content-Length: ${value}`);
  }
  return length;
}

function abortIfRequested(signal: AbortSignal | undefined): void {
  if (!signal?.aborted) {
    return;
  }
  if (typeof signal.throwIfAborted === "function") {
    signal.throwIfAborted();
  }
  throw new DOMException("The operation was aborted", "AbortError");
}

function sizeLimitError(maxBytes: number): RangeError {
  return new RangeError(`Growformer brain exceeds maxBytes (${maxBytes})`);
}

/**
 * Download a Growformer brain into one bounded byte array, optionally verifying its SHA-256.
 */
export async function fetchGrowformerBrainBytes(
  brainUrl: string,
  options: GrowformerBrainFetchOptions = {},
): Promise<Uint8Array> {
  const fetcher = options.fetcher ?? globalThis.fetch;
  if (typeof fetcher !== "function") {
    throw new Error("No fetch implementation is available for the Growformer brain");
  }
  const maxBytes = options.maxBytes ?? DEFAULT_GROWFORMER_BRAIN_MAX_BYTES;
  validateMaxBytes(maxBytes);

  let expectedSha256Hex: string | undefined;
  if (options.expectedSha256Hex !== undefined) {
    expectedSha256Hex = options.expectedSha256Hex.trim().toLowerCase();
    if (!/^[0-9a-f]{64}$/.test(expectedSha256Hex)) {
      throw new Error("expectedSha256Hex must contain exactly 64 hexadecimal characters");
    }
  }

  abortIfRequested(options.signal);
  const response = await fetcher(brainUrl, { signal: options.signal });
  if (!response.ok) {
    throw new Error(`Growformer brain fetch failed: ${brainUrl} (${response.status})`);
  }

  const totalBytes = parseContentLength(response.headers.get("content-length"));
  if (totalBytes !== undefined && totalBytes > maxBytes) {
    throw sizeLimitError(maxBytes);
  }
  options.onProgress?.({ phase: "download", bytesReceived: 0, totalBytes });

  let brainBytes: Uint8Array;
  if (response.body) {
    const reader = response.body.getReader();
    let bytesReceived = 0;
    try {
      if (totalBytes !== undefined) {
        brainBytes = new Uint8Array(totalBytes);
        while (true) {
          abortIfRequested(options.signal);
          const { done, value } = await reader.read();
          abortIfRequested(options.signal);
          if (done) {
            break;
          }
          const nextBytesReceived = bytesReceived + value.byteLength;
          if (nextBytesReceived > maxBytes) {
            throw sizeLimitError(maxBytes);
          }
          if (nextBytesReceived > totalBytes) {
            throw new Error("Growformer brain body exceeds its declared Content-Length");
          }
          brainBytes.set(value, bytesReceived);
          bytesReceived = nextBytesReceived;
          options.onProgress?.({ phase: "download", bytesReceived, totalBytes });
        }
        if (bytesReceived !== totalBytes) {
          throw new Error(
            `Growformer brain body length ${bytesReceived} does not match Content-Length ${totalBytes}`,
          );
        }
      } else {
        const chunks: Uint8Array[] = [];
        while (true) {
          abortIfRequested(options.signal);
          const { done, value } = await reader.read();
          abortIfRequested(options.signal);
          if (done) {
            break;
          }
          const nextBytesReceived = bytesReceived + value.byteLength;
          if (nextBytesReceived > maxBytes) {
            throw sizeLimitError(maxBytes);
          }
          chunks.push(value);
          bytesReceived = nextBytesReceived;
          options.onProgress?.({ phase: "download", bytesReceived });
        }
        brainBytes = new Uint8Array(bytesReceived);
        let offset = 0;
        for (const chunk of chunks) {
          brainBytes.set(chunk, offset);
          offset += chunk.byteLength;
        }
      }
    } catch (error) {
      await reader.cancel(error).catch(() => undefined);
      throw error;
    } finally {
      reader.releaseLock();
    }
  } else {
    abortIfRequested(options.signal);
    const buffer = await response.arrayBuffer();
    abortIfRequested(options.signal);
    if (buffer.byteLength > maxBytes) {
      throw sizeLimitError(maxBytes);
    }
    if (totalBytes !== undefined && buffer.byteLength !== totalBytes) {
      throw new Error(
        `Growformer brain body length ${buffer.byteLength} does not match Content-Length ${totalBytes}`,
      );
    }
    brainBytes = new Uint8Array(buffer);
    options.onProgress?.({
      phase: "download",
      bytesReceived: brainBytes.byteLength,
      totalBytes,
    });
  }

  if (expectedSha256Hex !== undefined) {
    options.onProgress?.({
      phase: "verify",
      bytesReceived: brainBytes.byteLength,
      totalBytes,
    });
    const actualSha256Hex = (await sha256Hex(brainBytes)).toLowerCase();
    if (actualSha256Hex !== expectedSha256Hex) {
      throw new Error(
        `Growformer brain SHA-256 mismatch: expected ${expectedSha256Hex}, got ${actualSha256Hex}`,
      );
    }
  }
  return brainBytes;
}

/**
 * Fetch a `.bin` brain from a URL and load it into the Growformer host.
 * Initializes the Growformer WASM runtime if needed (same as `initGrowformerHost`).
 *
 * **Correlate with contracts:** Contracts that call `growformer_*` from `spacekit_contract_sdk`
 * share one global Growformer runtime. Load the brain that matches the contract you are about to
 * execute (call this after selecting / deploying that agent). Switching agents with different
 * brains should call this again with the new URL — `loadGrowformerBrain` replaces the active brain.
 */
export async function initGrowformerHostWithBrainFromUrl(
  brainUrl: string,
  options: InitGrowformerHostWithBrainFromUrlOptions = {},
): Promise<void> {
  let totalBytes: number | undefined;
  const brainBytes = await fetchGrowformerBrainBytes(brainUrl, {
    ...options,
    onProgress: (progress) => {
      totalBytes = progress.totalBytes;
      options.onProgress?.(progress);
    },
  });
  options.onProgress?.({
    phase: "load",
    bytesReceived: brainBytes.byteLength,
    totalBytes,
  });
  await initGrowformerHost({
    moduleOrPath: options.moduleOrPath,
    scriptModuleUrl: options.scriptModuleUrl,
    brainBytes,
  });
}
