/**
 * SpaceKit plugin descriptors (`spacekit-plugin.json`).
 *
 * A plugin is a finished feature one author publishes and other authors add to
 * their apps: a chat panel, a paid newsletter, a support agent, a trained
 * brain. The descriptor says what the plugin is, what it needs, what it costs
 * and how an app owner configures it. The full specification is
 * `sdks/spacekit-apps/SPACEKIT-PLUGIN-SPEC.md`.
 *
 * Kinds:
 *   - `ui`       a web package mounted inside the host app (an isolated frame)
 *   - `service`  a web package with no visible UI that other code calls
 *   - `contract` an on-chain contract (WASM) apps call
 *   - `brain`    trained model files for an AI engine: data, not code
 */

import { parseManifestPermissions } from "../embed/capabilities.js";
import { parseWei } from "../astra.js";

export const PLUGIN_DESCRIPTOR_VERSION = 1;
export const PLUGIN_DESCRIPTOR_FILE = "spacekit-plugin.json";

export type PluginKind = "ui" | "service" | "contract" | "brain";
export const PLUGIN_KINDS: readonly PluginKind[] = ["ui", "service", "contract", "brain"];

/** The network building blocks a plugin is built on. */
export type BuildingBlock = "identity" | "storage" | "messaging" | "compute" | "payments" | "ai";
export const BUILDING_BLOCKS: readonly BuildingBlock[] = [
  "identity",
  "storage",
  "messaging",
  "compute",
  "payments",
  "ai",
];

export type ConfigFieldType =
  | "string"
  | "text"
  | "number"
  | "boolean"
  | "select"
  | "url"
  | "did"
  | "astra"
  | "color";

/** One setting the app owner fills in. Hosts render a form from these. */
export interface ConfigField {
  type: ConfigFieldType;
  label: string;
  help?: string;
  required?: boolean;
  default?: unknown;
  /** `select` choices. */
  options?: string[];
  min?: number;
  max?: number;
  max_length?: number;
}

export type PricingModel = "free" | "one_time" | "subscription";

/**
 * What an app owner pays to use the plugin, in ASTRA. Payment goes to the
 * publisher through the entitlement ledger; SpaceKit takes no cut.
 */
export interface PluginPricing {
  model: PricingModel;
  /** ASTRA wei, decimal string. Required unless `model` is "free". */
  amount_wei?: string;
  /** Subscription length; default 30. */
  period_days?: number;
  /** Payout address; defaults to the publisher's. */
  pay_to?: string;
}

/** A file the plugin ships, pinned by hash. */
export interface PluginFile {
  /** Path inside the package, or a storage file id for listing artifacts. */
  path: string;
  role: string;
  /** SHA-256 of the file, 64 hex characters. */
  sha256: string;
  size: number;
}

export interface PluginDependency {
  id: string;
  /** Exact version or a caret/tilde range ("^1.2.0"). */
  version: string;
}

export interface UiSpec {
  /** HTML entry inside the package. */
  entry: string;
  /** Where hosts are expected to show it. */
  placement?: "inline" | "panel" | "page";
  min_height?: number;
}

export interface ServiceMethod {
  description: string;
  params?: Record<string, ConfigField>;
}

export interface ServiceSpec {
  methods: Record<string, ServiceMethod>;
}

export interface ContractSpec {
  /** Deployed addresses by network ("testnet", "mainnet"). */
  addresses?: Record<string, string>;
  /** Entry points apps may call, with plain-language descriptions. */
  calls?: Record<string, { description: string; fee_wei?: string }>;
}

export type BrainAbility = "generation" | "converse" | "codegen";

export interface BrainSpec {
  /** AI engine that runs the brain, e.g. "growformer" or "growformer-llm". */
  engine: string;
  /** Engine versions the brain was built for (caret/tilde range or exact). */
  engine_version?: string;
  /** Where the brain can run. */
  runs_on?: Array<"browser" | "node" | "compute-node">;
  abilities?: BrainAbility[];
  /** The brain this one was trained or tuned from. */
  base?: PluginDependency;
  /**
   * An open-weight model the brain needs but does not ship (e.g. a GGUF for a
   * growformer-system1 brain). It is downloaded separately, under its own license.
   */
  base_model?: BaseModel;
  /** Provenance, shown to people choosing a brain. */
  training?: {
    summary?: string;
    data_sources?: string[];
    /** True when user conversations were captured for training. */
    includes_captured_conversations?: boolean;
  };
}

/** A third-party open-weight model, pinned by hash. */
export interface BaseModel {
  /** File or model name, e.g. "Qwen3-0.6B-Q8_0.gguf". */
  name: string;
  /** Architecture the engine needs to know, e.g. "qwen3". */
  arch?: string;
  sha256: string;
  size: number;
  /** The model's own license (SPDX id or name), shown before download. */
  license: string;
  /** Where to get it (https). */
  source?: string;
}

export interface PluginDescriptor {
  spacekit_plugin: 1;
  /** Lower-case id, unique per publisher: "acme.group-chat". */
  id: string;
  name: string;
  /** Semantic version, "1.2.0". */
  version: string;
  kind: PluginKind;
  /** Publisher DID. Trust comes from the signature, not this field. */
  publisher: string;
  /** One sentence for non-technical people. */
  summary: string;
  description?: string;
  /** SPDX identifier or "proprietary". */
  license?: string;
  homepage?: string;
  /** Building blocks it uses, for discovery and the consent screen. */
  uses?: BuildingBlock[];
  /** Same shapes as app manifest `permissions`. Not allowed for brains. */
  permissions?: unknown[];
  config?: Record<string, ConfigField>;
  pricing?: PluginPricing;
  requires?: PluginDependency[];
  files?: PluginFile[];
  ui?: UiSpec;
  service?: ServiceSpec;
  contract?: ContractSpec;
  brain?: BrainSpec;
}

/**
 * Known brain file roles. The first eight are the roles marketplace listings
 * use; the rest come from growformer-llm and growformer-system1 projects.
 */
export const BRAIN_FILE_ROLES: Record<string, string> = {
  bin: "Brain",
  inference_toml: "Inference rules",
  guardrails_jsonl: "Guardrails",
  fragments_jsonl: "Fragments",
  topic_graph: "Topic graph",
  topic_graph_overlay: "Topic graph overlay",
  grounding_toml: "Grounding",
  companion_ui: "Companion UI",
  /** growformer-llm label classifier (`label-model.json`). */
  label_model: "Label model",
  /** growformer-system1 decision brain (`*.s1.json.gz`). */
  system1: "System 1 decisions",
  /** growformer-llm chat language model checkpoint. */
  lm_checkpoint: "Chat model",
  /** Tokenizer for the chat model. */
  tokenizer: "Tokenizer",
};

/**
 * The file that holds a brain's weights, by engine. Engines not listed use "bin".
 * A brain must ship exactly one.
 */
export const ENGINE_WEIGHTS_ROLE: Record<string, string> = {
  growformer: "bin",
  "growformer-llm": "bin",
  "growformer-system1": "system1",
};

/** The weights role for an engine. */
export function weightsRole(engine: string | undefined): string {
  return (engine && ENGINE_WEIGHTS_ROLE[engine]) || "bin";
}

export class PluginDescriptorError extends Error {
  constructor(readonly errors: string[]) {
    super(`Invalid plugin descriptor:\n- ${errors.join("\n- ")}`);
    this.name = "PluginDescriptorError";
  }
}

export type PluginValidation =
  | { ok: true; descriptor: PluginDescriptor; warnings: string[] }
  | { ok: false; errors: string[]; warnings: string[] };

const ID_RE = /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?){1,4}$/;
const SEMVER_RE = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/;
const RANGE_RE = /^[\^~]?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?$/;
const SHA256_RE = /^[0-9a-f]{64}$/;
const ADDRESS_RE = /^0x[0-9a-f]{40}$/;
const CONFIG_KEY_RE = /^[a-z][a-z0-9_]{0,63}$/;
const FIELD_TYPES: readonly ConfigFieldType[] = [
  "string",
  "text",
  "number",
  "boolean",
  "select",
  "url",
  "did",
  "astra",
  "color",
];

function isObject(v: unknown): v is Record<string, unknown> {
  return !!v && typeof v === "object" && !Array.isArray(v);
}

function str(v: unknown): string | undefined {
  return typeof v === "string" && v.trim() ? v.trim() : undefined;
}

/** Check a config value against its field. Returns an error message or null. */
export function checkConfigValue(field: ConfigField, value: unknown): string | null {
  switch (field.type) {
    case "string":
    case "text":
    case "url":
    case "did":
    case "color": {
      if (typeof value !== "string") return "must be text";
      if (field.max_length !== undefined && value.length > field.max_length) {
        return `must be at most ${field.max_length} characters`;
      }
      if (field.type === "url" && value && !/^https?:\/\/[^\s]+$/.test(value)) return "must be an http(s) URL";
      if (field.type === "did" && value && !/^did:[a-z0-9]+:[^\s]+$/.test(value)) return "must be a DID";
      if (field.type === "color" && value && !/^#[0-9a-fA-F]{6}$/.test(value)) return "must be a #rrggbb color";
      return null;
    }
    case "number": {
      if (typeof value !== "number" || !Number.isFinite(value)) return "must be a number";
      if (field.min !== undefined && value < field.min) return `must be at least ${field.min}`;
      if (field.max !== undefined && value > field.max) return `must be at most ${field.max}`;
      return null;
    }
    case "boolean":
      return typeof value === "boolean" ? null : "must be true or false";
    case "select":
      return typeof value === "string" && (field.options ?? []).includes(value)
        ? null
        : `must be one of ${(field.options ?? []).join(", ")}`;
    case "astra":
      return parseWei(value) !== null ? null : "must be an ASTRA amount in wei (integer string)";
  }
}

function validateConfig(raw: unknown, errors: string[], where: string): Record<string, ConfigField> | undefined {
  if (raw === undefined) return undefined;
  if (!isObject(raw)) {
    errors.push(`${where} must be an object of fields`);
    return undefined;
  }
  const out: Record<string, ConfigField> = {};
  for (const [key, def] of Object.entries(raw)) {
    const at = `${where}.${key}`;
    if (!CONFIG_KEY_RE.test(key)) errors.push(`${at}: keys are lower_snake_case`);
    if (!isObject(def)) {
      errors.push(`${at} must be an object`);
      continue;
    }
    const type = def.type as ConfigFieldType;
    if (!FIELD_TYPES.includes(type)) {
      errors.push(`${at}.type must be one of ${FIELD_TYPES.join(", ")}`);
      continue;
    }
    const label = str(def.label);
    if (!label) errors.push(`${at}.label is required (people see it)`);
    const field: ConfigField = { type, label: label ?? key };
    if (str(def.help)) field.help = str(def.help);
    if (def.required !== undefined) field.required = def.required === true;
    if (type === "select") {
      const options = Array.isArray(def.options) ? def.options.filter((o): o is string => typeof o === "string") : [];
      if (options.length === 0) errors.push(`${at}.options must list the choices`);
      field.options = options;
    }
    for (const k of ["min", "max", "max_length"] as const) {
      if (def[k] !== undefined) {
        if (typeof def[k] !== "number") errors.push(`${at}.${k} must be a number`);
        else field[k] = def[k] as number;
      }
    }
    if (def.default !== undefined) {
      const problem = checkConfigValue(field, def.default);
      if (problem) errors.push(`${at}.default ${problem}`);
      field.default = def.default;
    }
    out[key] = field;
  }
  return out;
}

function validateDependency(raw: unknown, at: string, errors: string[]): PluginDependency | undefined {
  if (!isObject(raw)) {
    errors.push(`${at} must be { id, version }`);
    return undefined;
  }
  const id = str(raw.id);
  const version = str(raw.version);
  if (!id || !ID_RE.test(id)) errors.push(`${at}.id is not a plugin id`);
  if (!version || !RANGE_RE.test(version)) errors.push(`${at}.version must be a version or ^/~ range`);
  return id && version ? { id, version } : undefined;
}

function validateFiles(raw: unknown, errors: string[]): PluginFile[] | undefined {
  if (raw === undefined) return undefined;
  if (!Array.isArray(raw)) {
    errors.push("files must be an array");
    return undefined;
  }
  const out: PluginFile[] = [];
  const paths = new Set<string>();
  raw.forEach((f, i) => {
    const at = `files[${i}]`;
    if (!isObject(f)) {
      errors.push(`${at} must be an object`);
      return;
    }
    const path = str(f.path);
    const role = str(f.role);
    const sha256 = typeof f.sha256 === "string" ? f.sha256.toLowerCase() : "";
    const size = f.size;
    if (!path) errors.push(`${at}.path is required`);
    else if (path.startsWith("/") || path.split("/").includes("..")) errors.push(`${at}.path must be relative`);
    else if (paths.has(path)) errors.push(`${at}.path ${path} is listed twice`);
    if (!role) errors.push(`${at}.role is required`);
    if (!SHA256_RE.test(sha256)) errors.push(`${at}.sha256 must be 64 hex characters`);
    if (typeof size !== "number" || !Number.isSafeInteger(size) || size < 0) errors.push(`${at}.size must be a byte count`);
    if (path && role && SHA256_RE.test(sha256) && typeof size === "number") {
      paths.add(path);
      out.push({ path, role, sha256, size });
    }
  });
  return out;
}

function validatePricing(raw: unknown, errors: string[]): PluginPricing | undefined {
  if (raw === undefined) return undefined;
  if (!isObject(raw)) {
    errors.push("pricing must be an object");
    return undefined;
  }
  if (raw.amount_cents !== undefined || raw.amount_usd !== undefined || raw.amount_ausd !== undefined) {
    errors.push("pricing is in ASTRA: use amount_wei, not USD fields");
  }
  const model = raw.model as PricingModel;
  if (!["free", "one_time", "subscription"].includes(model)) {
    errors.push('pricing.model must be "free", "one_time" or "subscription"');
    return undefined;
  }
  const pricing: PluginPricing = { model };
  if (model !== "free") {
    const wei = parseWei(raw.amount_wei);
    if (wei === null || wei === 0n) errors.push("pricing.amount_wei must be a positive integer string (ASTRA wei)");
    else pricing.amount_wei = wei.toString();
  }
  if (raw.period_days !== undefined) {
    if (model !== "subscription") errors.push("pricing.period_days only applies to subscriptions");
    else if (typeof raw.period_days !== "number" || !Number.isInteger(raw.period_days) || raw.period_days < 1) {
      errors.push("pricing.period_days must be a whole number of days");
    } else pricing.period_days = raw.period_days;
  }
  if (raw.pay_to !== undefined) {
    const payTo = typeof raw.pay_to === "string" ? raw.pay_to.toLowerCase() : "";
    if (!ADDRESS_RE.test(payTo)) errors.push("pricing.pay_to must be a 0x chain address");
    else pricing.pay_to = payTo;
  }
  return pricing;
}

/** Validate a descriptor (parsed JSON or a JSON string), collecting every problem. */
export function validatePluginDescriptor(input: unknown): PluginValidation {
  const errors: string[] = [];
  const warnings: string[] = [];
  let raw: unknown = input;
  if (typeof input === "string" || input instanceof Uint8Array) {
    try {
      raw = JSON.parse(typeof input === "string" ? input : new TextDecoder().decode(input));
    } catch {
      return { ok: false, errors: ["not valid JSON"], warnings };
    }
  }
  if (!isObject(raw)) return { ok: false, errors: ["descriptor must be a JSON object"], warnings };

  if (raw.spacekit_plugin !== PLUGIN_DESCRIPTOR_VERSION) {
    errors.push(`spacekit_plugin must be ${PLUGIN_DESCRIPTOR_VERSION}`);
  }
  const id = str(raw.id);
  if (!id || !ID_RE.test(id)) {
    errors.push('id must be lower-case dotted segments, e.g. "acme.group-chat"');
  }
  const name = str(raw.name);
  if (!name) errors.push("name is required");
  else if (name.length > 80) errors.push("name must be at most 80 characters");
  const version = str(raw.version);
  if (!version || !SEMVER_RE.test(version)) errors.push('version must be semantic, e.g. "1.0.0"');
  const kind = raw.kind as PluginKind;
  if (!PLUGIN_KINDS.includes(kind)) errors.push(`kind must be one of ${PLUGIN_KINDS.join(", ")}`);
  const publisher = str(raw.publisher);
  if (!publisher || !/^did:[a-z0-9]+:[^\s]+$/.test(publisher)) errors.push("publisher must be the publisher's DID");
  const summary = str(raw.summary);
  if (!summary) errors.push("summary is required: one plain sentence for people choosing plugins");
  else if (summary.length > 200) errors.push("summary must be at most 200 characters");

  const uses = Array.isArray(raw.uses) ? raw.uses : raw.uses === undefined ? undefined : null;
  if (uses === null) errors.push("uses must be an array");
  for (const u of uses ?? []) {
    if (!BUILDING_BLOCKS.includes(u as BuildingBlock)) {
      errors.push(`uses: unknown building block ${JSON.stringify(u)} (known: ${BUILDING_BLOCKS.join(", ")})`);
    }
  }

  let permissions: unknown[] | undefined;
  if (raw.permissions !== undefined) {
    if (!Array.isArray(raw.permissions)) errors.push("permissions must be an array");
    else permissions = raw.permissions;
  }

  const config = validateConfig(raw.config, errors, "config");
  const pricing = validatePricing(raw.pricing, errors);
  const files = validateFiles(raw.files, errors);

  let requires: PluginDependency[] | undefined;
  if (raw.requires !== undefined) {
    if (!Array.isArray(raw.requires)) errors.push("requires must be an array");
    else {
      requires = raw.requires
        .map((d, i) => validateDependency(d, `requires[${i}]`, errors))
        .filter((d): d is PluginDependency => !!d);
      if (id && requires.some((d) => d.id === id)) errors.push("a plugin cannot require itself");
    }
  }

  const descriptor: PluginDescriptor = {
    spacekit_plugin: 1,
    id: id ?? "",
    name: name ?? "",
    version: version ?? "",
    kind,
    publisher: publisher ?? "",
    summary: summary ?? "",
  };
  if (str(raw.description)) descriptor.description = str(raw.description);
  if (str(raw.license)) descriptor.license = str(raw.license);
  else warnings.push("license is not set; people cannot tell how they may use the plugin");
  if (str(raw.homepage)) {
    if (!/^https:\/\//.test(str(raw.homepage)!)) errors.push("homepage must be an https URL");
    descriptor.homepage = str(raw.homepage);
  }
  if (uses) descriptor.uses = uses as BuildingBlock[];
  if (permissions) descriptor.permissions = permissions;
  if (config) descriptor.config = config;
  if (pricing) descriptor.pricing = pricing;
  if (requires?.length) descriptor.requires = requires;
  if (files) descriptor.files = files;

  // Kind-specific sections.
  const sections = { ui: raw.ui, service: raw.service, contract: raw.contract, brain: raw.brain };
  for (const [section, value] of Object.entries(sections)) {
    if (value !== undefined && section !== kind) errors.push(`${section} is only allowed for kind "${section}"`);
  }

  if (kind === "ui") {
    const ui = raw.ui;
    const entry = isObject(ui) ? str(ui.entry) : undefined;
    if (!entry) errors.push("ui.entry (the HTML entry file) is required");
    else {
      descriptor.ui = { entry };
      const placement = isObject(ui) ? ui.placement : undefined;
      if (placement !== undefined) {
        if (!["inline", "panel", "page"].includes(placement as string)) {
          errors.push('ui.placement must be "inline", "panel" or "page"');
        } else descriptor.ui.placement = placement as UiSpec["placement"];
      }
      if (isObject(ui) && typeof ui.min_height === "number") descriptor.ui.min_height = ui.min_height;
    }
  }

  if (kind === "service") {
    const methods = isObject(raw.service) && isObject(raw.service.methods) ? raw.service.methods : null;
    if (!methods || Object.keys(methods).length === 0) errors.push("service.methods must list at least one method");
    else {
      const out: Record<string, ServiceMethod> = {};
      for (const [m, def] of Object.entries(methods)) {
        if (!/^[a-z][A-Za-z0-9_]{0,63}$/.test(m)) errors.push(`service.methods.${m}: method names are camelCase`);
        const description = isObject(def) ? str(def.description) : undefined;
        if (!description) errors.push(`service.methods.${m}.description is required`);
        const params = isObject(def) ? validateConfig(def.params, errors, `service.methods.${m}.params`) : undefined;
        out[m] = { description: description ?? "", ...(params ? { params } : {}) };
      }
      descriptor.service = { methods: out };
    }
  }

  if (kind === "contract") {
    const c = isObject(raw.contract) ? raw.contract : {};
    const spec: ContractSpec = {};
    if (c.addresses !== undefined) {
      if (!isObject(c.addresses)) errors.push("contract.addresses must map network names to addresses");
      else {
        spec.addresses = {};
        for (const [net, addr] of Object.entries(c.addresses)) {
          const a = typeof addr === "string" ? addr.toLowerCase() : "";
          if (!ADDRESS_RE.test(a)) errors.push(`contract.addresses.${net} must be a 0x chain address`);
          else spec.addresses[net] = a;
        }
      }
    }
    if (c.calls !== undefined) {
      if (!isObject(c.calls)) errors.push("contract.calls must be an object");
      else {
        spec.calls = {};
        for (const [call, def] of Object.entries(c.calls)) {
          const description = isObject(def) ? str(def.description) : undefined;
          if (!description) errors.push(`contract.calls.${call}.description is required`);
          const fee = isObject(def) && def.fee_wei !== undefined ? parseWei(def.fee_wei) : undefined;
          if (fee === null) errors.push(`contract.calls.${call}.fee_wei must be ASTRA wei`);
          spec.calls[call] = { description: description ?? "", ...(fee != null ? { fee_wei: fee.toString() } : {}) };
        }
      }
    }
    const hasWasm = (files ?? []).some((f) => f.role === "wasm");
    if (!spec.addresses && !hasWasm) {
      errors.push('a contract plugin needs contract.addresses or a files entry with role "wasm"');
    }
    descriptor.contract = spec;
  }

  if (kind === "brain") {
    const b = raw.brain;
    if (!isObject(b)) errors.push("brain is required for kind brain");
    else {
      const engine = str(b.engine);
      if (!engine || !/^[a-z0-9][a-z0-9-]{0,63}$/.test(engine)) {
        errors.push('brain.engine names the AI engine, e.g. "growformer"');
      }
      const spec: BrainSpec = { engine: engine ?? "" };
      if (b.engine_version !== undefined) {
        if (typeof b.engine_version !== "string" || !RANGE_RE.test(b.engine_version)) {
          errors.push("brain.engine_version must be a version or ^/~ range");
        } else spec.engine_version = b.engine_version;
      }
      if (b.runs_on !== undefined) {
        const allowed = ["browser", "node", "compute-node"];
        const list = Array.isArray(b.runs_on) ? b.runs_on : [];
        if (!list.length || list.some((r) => !allowed.includes(r as string))) {
          errors.push(`brain.runs_on must list ${allowed.join(", ")}`);
        } else spec.runs_on = list as BrainSpec["runs_on"];
      }
      if (b.abilities !== undefined) {
        const allowed = ["generation", "converse", "codegen"];
        const list = Array.isArray(b.abilities) ? b.abilities : [];
        if (list.some((a) => !allowed.includes(a as string))) {
          errors.push(`brain.abilities must be from ${allowed.join(", ")}`);
        } else spec.abilities = list as BrainAbility[];
      }
      if (b.base !== undefined) spec.base = validateDependency(b.base, "brain.base", errors);
      if (b.base_model !== undefined) {
        const m = b.base_model;
        if (!isObject(m)) errors.push("brain.base_model must be an object");
        else {
          const name = str(m.name);
          const sha = typeof m.sha256 === "string" ? m.sha256.toLowerCase() : "";
          const license = str(m.license);
          if (!name) errors.push("brain.base_model.name is required");
          if (!SHA256_RE.test(sha)) errors.push("brain.base_model.sha256 must be 64 hex characters");
          if (typeof m.size !== "number" || !Number.isSafeInteger(m.size) || m.size <= 0) {
            errors.push("brain.base_model.size must be a byte count");
          }
          if (!license) errors.push("brain.base_model.license is required: people must see the model's own license");
          const source = str(m.source);
          if (source && !/^https:\/\//.test(source)) errors.push("brain.base_model.source must be an https URL");
          if (name && SHA256_RE.test(sha) && typeof m.size === "number" && license) {
            spec.base_model = { name, sha256: sha, size: m.size, license };
            if (str(m.arch)) spec.base_model.arch = str(m.arch);
            if (source) spec.base_model.source = source;
          }
        }
      }
      if (b.training !== undefined) {
        if (!isObject(b.training)) errors.push("brain.training must be an object");
        else {
          spec.training = {};
          if (str(b.training.summary)) spec.training.summary = str(b.training.summary);
          if (Array.isArray(b.training.data_sources)) {
            spec.training.data_sources = b.training.data_sources.filter((s): s is string => typeof s === "string");
          }
          if (b.training.includes_captured_conversations !== undefined) {
            spec.training.includes_captured_conversations = b.training.includes_captured_conversations === true;
          }
        }
      } else {
        warnings.push("brain.training is not set; people cannot see where the brain's knowledge came from");
      }
      descriptor.brain = spec;
    }
    if (permissions?.length) errors.push("brains are data: they cannot declare permissions");
    if (descriptor.uses?.some((u) => u !== "ai")) errors.push('brains can only list "ai" in uses');
    const weights = weightsRole(descriptor.brain?.engine);
    const brainFiles = (files ?? []).filter((f) => f.role === weights);
    if (brainFiles.length !== 1) {
      errors.push(`a brain needs exactly one files entry with role "${weights}" (the weights)`);
    }
    for (const f of files ?? []) {
      if (!(f.role in BRAIN_FILE_ROLES)) {
        warnings.push(`files: role "${f.role}" is not a known brain role; engines will ignore it`);
      }
    }
    for (const role of Object.keys(BRAIN_FILE_ROLES)) {
      if ((files ?? []).filter((f) => f.role === role).length > 1) errors.push(`files: role "${role}" is listed twice`);
    }
  }

  if (permissions?.length) {
    const declared = parseManifestPermissions(permissions);
    if (declared.capabilities.has("identity:auth-headers")) {
      errors.push("plugins can never ask for the viewer's session token (identity:auth-headers)");
    }
    if (declared.capabilities.has("payments")) {
      errors.push("plugins cannot take payments from the host app's viewers; charge app owners through pricing");
    }
    if (declared.networkHosts.includes("*")) {
      warnings.push("network access to any website: list the hosts the plugin really needs");
    }
  }

  return errors.length ? { ok: false, errors, warnings } : { ok: true, descriptor, warnings };
}

/** Parse and validate; throws {@link PluginDescriptorError} listing every problem. */
export function parsePluginDescriptor(input: unknown): PluginDescriptor {
  const result = validatePluginDescriptor(input);
  if ("errors" in result) throw new PluginDescriptorError(result.errors);
  return result.descriptor;
}

/** True when `version` satisfies `range` ("1.2.3", "^1.2.0", "~1.2.0"). Pre-releases match exactly only. */
export function versionSatisfies(version: string, range: string): boolean {
  const v = SEMVER_RE.exec(version);
  const r = RANGE_RE.exec(range);
  if (!v || !r) return false;
  const [vMaj, vMin, vPat] = [Number(v[1]), Number(v[2]), Number(v[3])];
  const [rMaj, rMin, rPat] = [Number(r[1]), Number(r[2]), Number(r[3])];
  const op = range[0];
  if (version.includes("-") || range.includes("-")) return version === range.replace(/^[\^~]/, "");
  const geq = vMaj > rMaj || (vMaj === rMaj && (vMin > rMin || (vMin === rMin && vPat >= rPat)));
  if (op === "^") {
    if (!geq) return false;
    if (rMaj > 0) return vMaj === rMaj;
    if (rMin > 0) return vMaj === 0 && vMin === rMin;
    return vMaj === 0 && vMin === 0 && vPat === rPat;
  }
  if (op === "~") return geq && vMaj === rMaj && vMin === rMin;
  return vMaj === rMaj && vMin === rMin && vPat === rPat;
}
