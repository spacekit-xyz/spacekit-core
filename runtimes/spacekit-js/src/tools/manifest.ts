/**
 * SKTCS v0.1 — Tool Manifest parser and types.
 *
 * A tool manifest is a JSON document embedded in a WASM custom section named
 * `spacekit:tools`. It declares which host-provided tools a contract uses,
 * their parameter schemas, and constraint bounds the VM enforces at the
 * host-import boundary.
 */

/* ─── Types ──────────────────────────────────────────────── */

export type ParamType = "string" | "bytes" | "u32" | "u64" | "did" | "bool";
export type SanitizeMode = "strip_control_chars" | "prompt_fence" | "none";
export type ValidateMode = "did_format" | "caller_did_prefix" | "numeric_string" | "none";
export type ExecutionPattern = "effect_queue" | "fire_and_forget" | "synchronous";

export interface ParamDef {
  type: ParamType;
  max_bytes?: number;
  min?: number;
  max?: number;
  required?: boolean;
  default?: unknown;
  sanitize?: SanitizeMode;
  validate?: ValidateMode;
}

export interface ConstraintDef {
  cost?: string;
  cost_unit?: string;
  rate_limit?: string;
  max_effects_per_execution?: number;
  requires_caller_did?: boolean;
  storage_key_prefix?: string;
  allowed_recipients?: string[];
  blocked_recipients?: string[];
  beneficiary_must_match_caller?: boolean;
  max_input_plus_output_bytes?: number;
}

export interface ToolDef {
  module: string;
  function: string;
  pattern: ExecutionPattern;
  params: Record<string, ParamDef>;
  constraints: ConstraintDef;
  version?: number;
}

export interface ToolManifest {
  version: string;
  contract_id: string;
  tools: Record<string, ToolDef>;
}

/* ─── Custom-section extraction ──────────────────────────── */

const CUSTOM_SECTION_NAME = "spacekit:tools";

/**
 * Extract and parse the SKTCS tool manifest from a compiled WASM module.
 * Returns `null` when the module has no `spacekit:tools` custom section
 * (legacy / pre-SKTCS contract).
 */
export function parseManifestFromModule(
  module: WebAssembly.Module,
): ToolManifest | null {
  const sections = WebAssembly.Module.customSections(module, CUSTOM_SECTION_NAME);
  if (sections.length === 0) return null;

  const bytes = new Uint8Array(sections[0]);
  const json = new TextDecoder().decode(bytes);
  return validateManifestJson(json);
}

/**
 * Parse and validate a manifest JSON string. Throws on schema violations.
 */
export function validateManifestJson(json: string): ToolManifest {
  const raw = JSON.parse(json);

  if (typeof raw.version !== "string") {
    throw new ManifestError("manifest missing `version` string");
  }
  if (typeof raw.contract_id !== "string") {
    throw new ManifestError("manifest missing `contract_id` string");
  }
  if (typeof raw.tools !== "object" || raw.tools === null) {
    throw new ManifestError("manifest missing `tools` object");
  }

  const tools: Record<string, ToolDef> = {};
  for (const [name, def] of Object.entries(raw.tools)) {
    tools[name] = validateToolDef(name, def as Record<string, unknown>);
  }

  return {
    version: raw.version,
    contract_id: raw.contract_id,
    tools,
  };
}

/* ─── Validation helpers ─────────────────────────────────── */

const VALID_PATTERNS: Set<string> = new Set(["effect_queue", "fire_and_forget", "synchronous"]);
const VALID_PARAM_TYPES: Set<string> = new Set(["string", "bytes", "u32", "u64", "did", "bool"]);
const VALID_SANITIZE: Set<string> = new Set(["strip_control_chars", "prompt_fence", "none"]);
const VALID_VALIDATE: Set<string> = new Set(["did_format", "caller_did_prefix", "numeric_string", "none"]);

function validateToolDef(name: string, raw: Record<string, unknown>): ToolDef {
  if (typeof raw.module !== "string") {
    throw new ManifestError(`tool ${name}: missing \`module\` string`);
  }
  if (typeof raw.function !== "string") {
    throw new ManifestError(`tool ${name}: missing \`function\` string`);
  }
  if (!VALID_PATTERNS.has(raw.pattern as string)) {
    throw new ManifestError(`tool ${name}: invalid pattern "${raw.pattern}"`);
  }

  const params: Record<string, ParamDef> = {};
  if (raw.params && typeof raw.params === "object") {
    for (const [pName, pDef] of Object.entries(raw.params as Record<string, unknown>)) {
      params[pName] = validateParamDef(name, pName, pDef as Record<string, unknown>);
    }
  }

  const constraints = validateConstraintDef(
    name,
    (raw.constraints as Record<string, unknown>) ?? {},
  );

  return {
    module: raw.module as string,
    function: raw.function as string,
    pattern: raw.pattern as ExecutionPattern,
    params,
    constraints,
    version: typeof raw.version === "number" ? raw.version : undefined,
  };
}

function validateParamDef(
  toolName: string,
  paramName: string,
  raw: Record<string, unknown>,
): ParamDef {
  if (!VALID_PARAM_TYPES.has(raw.type as string)) {
    throw new ManifestError(`tool ${toolName}.${paramName}: invalid type "${raw.type}"`);
  }

  const def: ParamDef = { type: raw.type as ParamType };

  if (raw.max_bytes !== undefined) {
    if (typeof raw.max_bytes !== "number" || raw.max_bytes < 0) {
      throw new ManifestError(`tool ${toolName}.${paramName}: max_bytes must be non-negative number`);
    }
    def.max_bytes = raw.max_bytes;
  }
  if (raw.min !== undefined) {
    if (typeof raw.min !== "number") {
      throw new ManifestError(`tool ${toolName}.${paramName}: min must be a number`);
    }
    def.min = raw.min;
  }
  if (raw.max !== undefined) {
    if (typeof raw.max !== "number") {
      throw new ManifestError(`tool ${toolName}.${paramName}: max must be a number`);
    }
    def.max = raw.max;
  }
  if (raw.required !== undefined) def.required = !!raw.required;
  if (raw.default !== undefined) def.default = raw.default;

  if (raw.sanitize !== undefined) {
    if (!VALID_SANITIZE.has(raw.sanitize as string)) {
      throw new ManifestError(`tool ${toolName}.${paramName}: invalid sanitize "${raw.sanitize}"`);
    }
    def.sanitize = raw.sanitize as SanitizeMode;
  }
  if (raw.validate !== undefined) {
    if (!VALID_VALIDATE.has(raw.validate as string)) {
      throw new ManifestError(`tool ${toolName}.${paramName}: invalid validate "${raw.validate}"`);
    }
    def.validate = raw.validate as ValidateMode;
  }

  return def;
}

function validateConstraintDef(
  toolName: string,
  raw: Record<string, unknown>,
): ConstraintDef {
  const c: ConstraintDef = {};

  if (raw.cost !== undefined) {
    if (typeof raw.cost !== "string") {
      throw new ManifestError(`tool ${toolName}: cost must be a string`);
    }
    c.cost = raw.cost;
  }
  if (raw.cost_unit !== undefined) c.cost_unit = String(raw.cost_unit);
  if (raw.rate_limit !== undefined) c.rate_limit = String(raw.rate_limit);
  if (raw.max_effects_per_execution !== undefined) {
    c.max_effects_per_execution = Number(raw.max_effects_per_execution);
  }
  if (raw.requires_caller_did !== undefined) {
    c.requires_caller_did = !!raw.requires_caller_did;
  }
  if (raw.storage_key_prefix !== undefined) {
    c.storage_key_prefix = String(raw.storage_key_prefix);
  }
  if (raw.allowed_recipients !== undefined) {
    if (!Array.isArray(raw.allowed_recipients)) {
      throw new ManifestError(`tool ${toolName}: allowed_recipients must be array`);
    }
    c.allowed_recipients = raw.allowed_recipients.map(String);
  }
  if (raw.blocked_recipients !== undefined) {
    if (!Array.isArray(raw.blocked_recipients)) {
      throw new ManifestError(`tool ${toolName}: blocked_recipients must be array`);
    }
    c.blocked_recipients = raw.blocked_recipients.map(String);
  }
  if (raw.beneficiary_must_match_caller !== undefined) {
    c.beneficiary_must_match_caller = !!raw.beneficiary_must_match_caller;
  }
  if (raw.max_input_plus_output_bytes !== undefined) {
    c.max_input_plus_output_bytes = Number(raw.max_input_plus_output_bytes);
  }

  return c;
}

/* ─── Error class ────────────────────────────────────────── */

export class ManifestError extends Error {
  constructor(message: string) {
    super(`SKTCS manifest: ${message}`);
    this.name = "ManifestError";
  }
}
