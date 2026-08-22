import { sha256 } from "@noble/hashes/sha256";

/**
 * Status codes returned by tool host imports.
 * Positive values indicate bytes written to the destination buffer.
 */
export const TOOL_STATUS = {
  NOT_CONFIGURED: -1,
  ERROR: -2,
  PENDING: -3,
} as const;

/** Well-known contract return status indicating unfulfilled tool effects. */
export const STATUS_NEEDS_TOOLS = -100;

/** Maximum re-execution rounds to prevent infinite effect loops. */
export const MAX_TOOL_ROUNDS = 4;

export interface ToolEffect {
  toolName: string;
  requestKey: string;
  requestData: Uint8Array;
}

/** SKTCS audit record emitted for every tool invocation (fulfilled or rejected). */
export interface ToolEffectRecord {
  tool_id: string;
  caller_did: string;
  params_hash: string;
  result_hash: string | null;
  cost_charged: string;
  timestamp: number;
  effect_round: number;
  status: "fulfilled" | "rejected" | "pending";
  reason?: string;
}

export interface ToolEffectManager {
  getCachedResult(requestKey: string): Uint8Array | null;
  addPending(effect: ToolEffect): void;
  getPending(): ToolEffect[];
  cacheResult(requestKey: string, result: Uint8Array): void;
  hasPending(): boolean;
  clear(): void;

  /** SKTCS: record an audit entry for a tool invocation. */
  addRecord(record: ToolEffectRecord): void;
  /** SKTCS: retrieve all audit records collected during this execution. */
  getRecords(): ToolEffectRecord[];
  /** SKTCS: clear audit records (called at start of new execution). */
  clearRecords(): void;
}

const encoder = new TextEncoder();

/**
 * Build a deterministic cache key from tool name + raw request bytes.
 * Uses SHA-256 so the key is fixed-length and collision-resistant.
 */
export function toolRequestKey(toolName: string, requestData: Uint8Array): string {
  const prefix = encoder.encode(toolName + ":");
  const combined = new Uint8Array(prefix.length + requestData.length);
  combined.set(prefix);
  combined.set(requestData, prefix.length);
  const hash = sha256(combined);
  return Array.from(hash).map(b => b.toString(16).padStart(2, "0")).join("");
}

export function createToolEffectManager(): ToolEffectManager {
  const cache = new Map<string, Uint8Array>();
  let pending: ToolEffect[] = [];
  let records: ToolEffectRecord[] = [];

  return {
    getCachedResult(requestKey: string): Uint8Array | null {
      return cache.get(requestKey) ?? null;
    },

    addPending(effect: ToolEffect): void {
      if (!cache.has(effect.requestKey)) {
        pending.push(effect);
      }
    },

    getPending(): ToolEffect[] {
      return pending;
    },

    cacheResult(requestKey: string, result: Uint8Array): void {
      cache.set(requestKey, result);
      pending = pending.filter(e => e.requestKey !== requestKey);
    },

    hasPending(): boolean {
      return pending.length > 0;
    },

    clear(): void {
      cache.clear();
      pending = [];
    },

    addRecord(record: ToolEffectRecord): void {
      records.push(record);
    },

    getRecords(): ToolEffectRecord[] {
      return records;
    },

    clearRecords(): void {
      records = [];
    },
  };
}
