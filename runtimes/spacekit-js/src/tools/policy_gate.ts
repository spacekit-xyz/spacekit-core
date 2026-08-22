/**
 * SKTCS v0.1 — Policy Gate.
 *
 * Stateless validation and constraint enforcement functions called at the
 * WASM host-import boundary before any tool fulfillment logic.
 */

import type { ToolDef, ParamDef, ConstraintDef } from "./manifest.js";
import type { PaymentAdapter } from "./types.js";

/* ─── SKTCS error codes (returned to WASM as negative i32) ── */

export const SKTCS_ERROR = {
  MISSING_PARAM: -10,
  INVALID_TYPE: -11,
  MAX_BYTES_EXCEEDED: -12,
  OUT_OF_RANGE: -13,
  INVALID_FORMAT: -14,
  MISSING_CALLER_DID: -15,
  RATE_LIMIT_EXCEEDED: -16,
  MAX_EFFECTS_EXCEEDED: -17,
  RECIPIENT_BLOCKED: -18,
  BENEFICIARY_MISMATCH: -19,
  VAULT_CHARGE_FAILED: -20,
  SIZE_LIMIT_EXCEEDED: -21,
} as const;

export type SktcsErrorCode = (typeof SKTCS_ERROR)[keyof typeof SKTCS_ERROR];

export interface PolicyResult {
  rejected: boolean;
  errorCode: SktcsErrorCode | 0;
  reason?: string;
}

const PASS: PolicyResult = { rejected: false, errorCode: 0 };

/* ─── Parameter validation ───────────────────────────────── */

/**
 * Validate actual parameters against the manifest's param definitions.
 * `actualParams` is a bag of named values extracted from guest memory.
 */
export function validateParams(
  toolDef: ToolDef,
  actualParams: Record<string, unknown>,
): PolicyResult {
  for (const [name, def] of Object.entries(toolDef.params)) {
    const value = actualParams[name] ?? def.default;

    if (value === undefined || value === null) {
      if (def.required) {
        return reject(SKTCS_ERROR.MISSING_PARAM, `required param "${name}" missing`);
      }
      continue;
    }

    const typeResult = checkType(name, def, value);
    if (typeResult.rejected) return typeResult;

    if (def.max_bytes !== undefined) {
      const byteLen = byteLength(value);
      if (byteLen > def.max_bytes) {
        return reject(SKTCS_ERROR.MAX_BYTES_EXCEEDED, `param "${name}" exceeds max_bytes (${byteLen} > ${def.max_bytes})`);
      }
    }

    if (def.min !== undefined || def.max !== undefined) {
      const num = typeof value === "number" ? value : typeof value === "bigint" ? Number(value) : NaN;
      if (!isNaN(num)) {
        if (def.min !== undefined && num < def.min) {
          return reject(SKTCS_ERROR.OUT_OF_RANGE, `param "${name}" below min (${num} < ${def.min})`);
        }
        if (def.max !== undefined && num > def.max) {
          return reject(SKTCS_ERROR.OUT_OF_RANGE, `param "${name}" above max (${num} > ${def.max})`);
        }
      }
    }

    if (def.validate && def.validate !== "none") {
      const fmtResult = checkFormat(name, def.validate, value, "");
      if (fmtResult.rejected) return fmtResult;
    }
  }

  return PASS;
}

function checkType(name: string, def: ParamDef, value: unknown): PolicyResult {
  switch (def.type) {
    case "string":
    case "did":
      if (typeof value !== "string") {
        return reject(SKTCS_ERROR.INVALID_TYPE, `param "${name}" expected string, got ${typeof value}`);
      }
      break;
    case "bytes":
      if (!(value instanceof Uint8Array)) {
        return reject(SKTCS_ERROR.INVALID_TYPE, `param "${name}" expected Uint8Array`);
      }
      break;
    case "u32":
    case "u64":
      if (typeof value !== "number" && typeof value !== "bigint") {
        return reject(SKTCS_ERROR.INVALID_TYPE, `param "${name}" expected number/bigint`);
      }
      break;
    case "bool":
      if (typeof value !== "boolean") {
        return reject(SKTCS_ERROR.INVALID_TYPE, `param "${name}" expected boolean`);
      }
      break;
  }
  return PASS;
}

const DID_REGEX = /^did:[a-z0-9]+:/;

function checkFormat(
  name: string,
  mode: string,
  value: unknown,
  callerDid: string,
): PolicyResult {
  const str = String(value);
  switch (mode) {
    case "did_format":
      if (!DID_REGEX.test(str)) {
        return reject(SKTCS_ERROR.INVALID_FORMAT, `param "${name}" is not a valid DID`);
      }
      break;
    case "caller_did_prefix":
      if (!str.startsWith(callerDid)) {
        return reject(SKTCS_ERROR.INVALID_FORMAT, `param "${name}" must start with caller DID`);
      }
      break;
    case "numeric_string":
      if (!/^\d+(\.\d+)?$/.test(str)) {
        return reject(SKTCS_ERROR.INVALID_FORMAT, `param "${name}" is not a numeric string`);
      }
      break;
  }
  return PASS;
}

/* ─── Constraint enforcement ─────────────────────────────── */

export interface ConstraintState {
  effectCounts: Map<string, number>;
  rateState: Map<string, { count: number; windowStart: number }>;
}

export function createConstraintState(): ConstraintState {
  return {
    effectCounts: new Map(),
    rateState: new Map(),
  };
}

/**
 * Check constraints for a tool invocation. Must be called before fulfillment.
 */
export function checkConstraints(
  toolName: string,
  toolDef: ToolDef,
  callerDid: string,
  state: ConstraintState,
  actualParams?: Record<string, unknown>,
): PolicyResult {
  const c = toolDef.constraints;

  if (c.requires_caller_did && (!callerDid || callerDid === "did:spacekit:browser:anonymous")) {
    return reject(SKTCS_ERROR.MISSING_CALLER_DID, "tool requires authenticated caller DID");
  }

  if (c.max_effects_per_execution !== undefined) {
    const current = state.effectCounts.get(toolName) ?? 0;
    if (current >= c.max_effects_per_execution) {
      return reject(SKTCS_ERROR.MAX_EFFECTS_EXCEEDED, `tool "${toolName}" max effects reached (${c.max_effects_per_execution})`);
    }
  }

  if (c.rate_limit) {
    const rateResult = checkRateLimit(toolName, callerDid, c.rate_limit, state);
    if (rateResult.rejected) return rateResult;
  }

  if (c.allowed_recipients && actualParams) {
    const recipient = actualParams["recipient"] ?? actualParams["recipientDid"] ?? actualParams["to"];
    if (recipient && typeof recipient === "string") {
      if (!matchesGlobList(recipient, c.allowed_recipients)) {
        return reject(SKTCS_ERROR.RECIPIENT_BLOCKED, `recipient "${recipient}" not in allowed list`);
      }
    }
  }

  if (c.blocked_recipients && actualParams) {
    const recipient = actualParams["recipient"] ?? actualParams["recipientDid"] ?? actualParams["to"];
    if (recipient && typeof recipient === "string") {
      if (matchesGlobList(recipient, c.blocked_recipients)) {
        return reject(SKTCS_ERROR.RECIPIENT_BLOCKED, `recipient "${recipient}" is blocked`);
      }
    }
  }

  if (c.beneficiary_must_match_caller && actualParams) {
    const beneficiary = actualParams["beneficiary"] ?? actualParams["to"];
    if (beneficiary && typeof beneficiary === "string" && beneficiary !== callerDid) {
      return reject(SKTCS_ERROR.BENEFICIARY_MISMATCH, `beneficiary "${beneficiary}" does not match caller "${callerDid}"`);
    }
  }

  return PASS;
}

function checkRateLimit(
  toolName: string,
  callerDid: string,
  rateLimitStr: string,
  state: ConstraintState,
): PolicyResult {
  const parsed = parseRateLimit(rateLimitStr);
  if (!parsed) return PASS;

  const key = `${callerDid}:${toolName}`;
  const now = Date.now();
  const entry = state.rateState.get(key);

  if (!entry || now - entry.windowStart > parsed.windowMs) {
    state.rateState.set(key, { count: 1, windowStart: now });
    return PASS;
  }

  if (entry.count >= parsed.limit) {
    return reject(SKTCS_ERROR.RATE_LIMIT_EXCEEDED, `tool "${toolName}" rate limit exceeded (${rateLimitStr})`);
  }

  entry.count++;
  return PASS;
}

function parseRateLimit(str: string): { limit: number; windowMs: number } | null {
  const m = str.match(/^(\d+)\/(sec|min|hour)$/);
  if (!m) return null;
  const limit = parseInt(m[1], 10);
  const unit = m[2];
  const windowMs = unit === "sec" ? 1000 : unit === "min" ? 60_000 : 3_600_000;
  return { limit, windowMs };
}

/**
 * Record that a tool effect occurred (for max_effects_per_execution tracking).
 */
export function recordEffect(toolName: string, state: ConstraintState): void {
  state.effectCounts.set(toolName, (state.effectCounts.get(toolName) ?? 0) + 1);
}

/* ─── Vault charging (pay-before-execute) ────────────────── */

export async function chargeVault(
  toolDef: ToolDef,
  callerDid: string,
  paymentAdapter: PaymentAdapter | undefined,
): Promise<PolicyResult> {
  const cost = toolDef.constraints.cost;
  if (!cost || cost === "0") return PASS;

  if (!paymentAdapter) {
    return reject(SKTCS_ERROR.VAULT_CHARGE_FAILED, "no payment adapter configured for vault charge");
  }

  const ok = await paymentAdapter.vaultCharge(cost, callerDid);
  if (!ok) {
    return reject(SKTCS_ERROR.VAULT_CHARGE_FAILED, `vault charge of ${cost} failed for ${callerDid}`);
  }

  return PASS;
}

/* ─── Storage key prefix (DID scoping) ───────────────────── */

/**
 * Prepend `{callerDid}:` to a storage key when the manifest requires it,
 * preventing cross-caller history reference spoofing.
 */
export function applyStorageKeyPrefix(
  key: string,
  callerDid: string,
  constraints: ConstraintDef,
): string {
  if (constraints.storage_key_prefix === "{caller_did}") {
    return `${callerDid}:${key}`;
  }
  if (constraints.storage_key_prefix) {
    return `${constraints.storage_key_prefix}:${key}`;
  }
  return key;
}

/* ─── Size limit check ───────────────────────────────────── */

export function checkSizeLimit(
  toolDef: ToolDef,
  inputBytes: number,
  outputBytes: number,
): PolicyResult {
  const limit = toolDef.constraints.max_input_plus_output_bytes;
  if (limit === undefined) return PASS;
  const total = inputBytes + outputBytes;
  if (total > limit) {
    return reject(SKTCS_ERROR.SIZE_LIMIT_EXCEEDED, `input+output size ${total} exceeds limit ${limit}`);
  }
  return PASS;
}

/* ─── Helpers ────────────────────────────────────────────── */

function reject(code: SktcsErrorCode, reason: string): PolicyResult {
  return { rejected: true, errorCode: code, reason };
}

function byteLength(value: unknown): number {
  if (value instanceof Uint8Array) return value.length;
  if (typeof value === "string") return new TextEncoder().encode(value).length;
  return 0;
}

function matchesGlobList(value: string, patterns: string[]): boolean {
  return patterns.some(pat => globMatch(pat, value));
}

function globMatch(pattern: string, value: string): boolean {
  if (pattern === "*") return true;
  if (!pattern.includes("*")) return pattern === value;
  const regex = new RegExp(
    "^" + pattern.replace(/[.+?^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$",
  );
  return regex.test(value);
}
