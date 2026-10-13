/**
 * Brain plugins: trained model files for an AI engine (Growformer and others).
 *
 * A brain is data, not code. Loading one means: fetch each file the descriptor
 * lists, check its size and SHA-256 against the descriptor, then hand the bytes
 * to the engine in a fixed order (weights first, then rules, guardrails,
 * fragments, topic graph and grounding). Nothing is loaded unless every file
 * verifies.
 *
 * Engines are injected, so this module does not depend on a particular
 * runtime. {@link growformerEngine} adapts the spacekit-js Growformer exports:
 *
 * ```ts
 * import * as spacekit from "@spacekit/spacekit-js";
 * await spacekit.initGrowformerHost();
 * await loadBrainPlugin(descriptor, { engine: growformerEngine(spacekit), fetchFile });
 * ```
 *
 * The browser Growformer runtime holds one brain at a time: loading another
 * replaces it.
 */

import { versionSatisfies, type PluginDescriptor, type PluginFile } from "./descriptor.js";

/** What an engine must offer to load brain plugins. Optional loaders handle sidecar files. */
export interface BrainEngine {
  /** Engine id the descriptor's `brain.engine` must match, e.g. "growformer". */
  readonly name: string;
  /** Engine version, checked against `brain.engine_version` when both are known. */
  readonly version?: string;
  loadBrain(bytes: Uint8Array): void | Promise<void>;
  loadInferenceToml?(toml: string): void | Promise<void>;
  loadGuardrailsJsonl?(jsonl: string): void | Promise<void>;
  loadFragmentsJsonl?(jsonl: string): void | Promise<void>;
  loadTopicGraph?(baseToml: string, overlayToml?: string): void | Promise<void>;
  loadGroundingToml?(toml: string): void | Promise<void>;
}

export type BrainFileFetcher = (file: PluginFile, signal?: AbortSignal) => Promise<Uint8Array>;

export interface BrainLoadProgress {
  phase: "fetch" | "verify" | "load";
  role: string;
  index: number;
  total: number;
}

export interface LoadBrainPluginOptions {
  engine: BrainEngine;
  fetchFile: BrainFileFetcher;
  signal?: AbortSignal;
  onProgress?(progress: BrainLoadProgress): void;
  /** Refuse brains whose files add up to more than this (default 1 GiB). */
  maxTotalBytes?: number;
}

export interface LoadedBrain {
  id: string;
  version: string;
  engine: string;
  /** Roles that were loaded, in order. */
  loaded: string[];
  /** Roles the engine has no loader for and skipped. */
  skipped: string[];
}

export class BrainVerificationError extends Error {
  constructor(message: string, readonly role?: string) {
    super(message);
    this.name = "BrainVerificationError";
  }
}

const DEFAULT_MAX_TOTAL = 1024 * 1024 * 1024;

/** Load order: weights first, then the sidecars that configure them. */
const LOAD_ORDER = ["bin", "inference_toml", "guardrails_jsonl", "fragments_jsonl", "topic_graph", "grounding_toml"];

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", Uint8Array.from(bytes)));
  return Array.from(digest, (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Fetch and verify every file of a brain plugin. Resolves to bytes by role,
 * or throws {@link BrainVerificationError} on the first mismatch.
 */
export async function fetchVerifiedBrainFiles(
  descriptor: PluginDescriptor,
  fetchFile: BrainFileFetcher,
  options: { signal?: AbortSignal; maxTotalBytes?: number; onProgress?(p: BrainLoadProgress): void } = {},
): Promise<Map<string, Uint8Array>> {
  if (descriptor.kind !== "brain") throw new BrainVerificationError(`${descriptor.id} is a ${descriptor.kind} plugin, not a brain`);
  const files = descriptor.files ?? [];
  const total = files.reduce((n, f) => n + f.size, 0);
  const max = options.maxTotalBytes ?? DEFAULT_MAX_TOTAL;
  if (total > max) {
    throw new BrainVerificationError(`brain is ${total} bytes, over the ${max}-byte limit`);
  }
  const out = new Map<string, Uint8Array>();
  for (const [index, file] of files.entries()) {
    options.signal?.throwIfAborted();
    options.onProgress?.({ phase: "fetch", role: file.role, index, total: files.length });
    const bytes = await fetchFile(file, options.signal);
    options.onProgress?.({ phase: "verify", role: file.role, index, total: files.length });
    if (bytes.length !== file.size) {
      throw new BrainVerificationError(`${file.path}: expected ${file.size} bytes, got ${bytes.length}`, file.role);
    }
    const hash = await sha256Hex(bytes);
    if (hash !== file.sha256) {
      throw new BrainVerificationError(`${file.path}: SHA-256 does not match the descriptor`, file.role);
    }
    out.set(file.role, bytes);
  }
  if (!out.has("bin")) throw new BrainVerificationError('brain has no "bin" (weights) file', "bin");
  return out;
}

/** Verify every file, then load the brain into the engine. */
export async function loadBrainPlugin(descriptor: PluginDescriptor, options: LoadBrainPluginOptions): Promise<LoadedBrain> {
  const spec = descriptor.brain;
  if (descriptor.kind !== "brain" || !spec) {
    throw new BrainVerificationError(`${descriptor.id} is not a brain plugin`);
  }
  const { engine } = options;
  if (spec.engine !== engine.name) {
    throw new BrainVerificationError(`${descriptor.name} needs the ${spec.engine} engine, not ${engine.name}`);
  }
  if (spec.engine_version && engine.version && !versionSatisfies(engine.version, spec.engine_version)) {
    throw new BrainVerificationError(
      `${descriptor.name} needs ${spec.engine} ${spec.engine_version}; this runtime has ${engine.version}`,
    );
  }

  const bytes = await fetchVerifiedBrainFiles(descriptor, options.fetchFile, options);
  const text = (role: string) => new TextDecoder("utf-8", { fatal: true }).decode(bytes.get(role)!);
  const loaded: string[] = [];
  const skipped: string[] = [];
  const total = LOAD_ORDER.filter((r) => bytes.has(r)).length;
  let index = 0;
  const step = async (role: string, run: (() => void | Promise<void>) | null) => {
    if (!bytes.has(role)) return;
    options.signal?.throwIfAborted();
    if (!run) {
      skipped.push(role);
      return;
    }
    options.onProgress?.({ phase: "load", role, index: index++, total });
    await run();
    loaded.push(role);
  };

  await step("bin", () => engine.loadBrain(bytes.get("bin")!));
  await step("inference_toml", engine.loadInferenceToml ? () => engine.loadInferenceToml!(text("inference_toml")) : null);
  await step(
    "guardrails_jsonl",
    engine.loadGuardrailsJsonl ? () => engine.loadGuardrailsJsonl!(text("guardrails_jsonl")) : null,
  );
  await step(
    "fragments_jsonl",
    engine.loadFragmentsJsonl ? () => engine.loadFragmentsJsonl!(text("fragments_jsonl")) : null,
  );
  await step(
    "topic_graph",
    engine.loadTopicGraph
      ? () =>
          engine.loadTopicGraph!(
            text("topic_graph"),
            bytes.has("topic_graph_overlay") ? text("topic_graph_overlay") : undefined,
          )
      : null,
  );
  await step("grounding_toml", engine.loadGroundingToml ? () => engine.loadGroundingToml!(text("grounding_toml")) : null);
  for (const role of bytes.keys()) {
    if (!LOAD_ORDER.includes(role) && role !== "topic_graph_overlay") skipped.push(role);
  }
  return { id: descriptor.id, version: descriptor.version, engine: engine.name, loaded, skipped };
}

/** The spacekit-js Growformer functions this adapter uses. */
export interface GrowformerRuntimeExports {
  loadGrowformerBrain(data: Uint8Array): void;
  loadGrowformerInferenceToml(toml: string): void;
  loadGrowformerInferenceGuardrailsJsonl(jsonl: string): void;
  loadGrowformerFragmentsJsonl(jsonl: string): void;
  loadGrowformerTopicGraph(baseToml: string, overlayToml?: string): void;
  loadGrowformerGroundingGraph(toml: string): void;
}

/**
 * Adapt the spacekit-js Growformer runtime (`initGrowformerHost()` first) to
 * {@link BrainEngine}. Pass `name: "growformer-llm"` for brains built for that
 * engine when it is served by a compatible runtime.
 */
export function growformerEngine(
  runtime: GrowformerRuntimeExports,
  options: { name?: string; version?: string } = {},
): BrainEngine {
  return {
    name: options.name ?? "growformer",
    version: options.version,
    loadBrain: (bytes) => runtime.loadGrowformerBrain(bytes),
    loadInferenceToml: (toml) => runtime.loadGrowformerInferenceToml(toml),
    loadGuardrailsJsonl: (jsonl) => runtime.loadGrowformerInferenceGuardrailsJsonl(jsonl),
    loadFragmentsJsonl: (jsonl) => runtime.loadGrowformerFragmentsJsonl(jsonl),
    loadTopicGraph: (base, overlay) => runtime.loadGrowformerTopicGraph(base, overlay),
    loadGroundingToml: (toml) => runtime.loadGrowformerGroundingGraph(toml),
  };
}

/**
 * A fetcher for brain files stored as marketplace artifacts: `path` is the
 * storage file id, served by `{apiBase}/api/files/{id}` or a storage node's
 * file route. Supply your own when files live elsewhere.
 */
export function httpBrainFileFetcher(
  urlFor: (file: PluginFile) => string,
  fetchImpl: typeof fetch = (input, init) => fetch(input, init),
): BrainFileFetcher {
  return async (file, signal) => {
    const res = await fetchImpl(urlFor(file), { signal });
    if (!res.ok) throw new BrainVerificationError(`${file.path}: download failed (${res.status})`, file.role);
    return new Uint8Array(await res.arrayBuffer());
  };
}
