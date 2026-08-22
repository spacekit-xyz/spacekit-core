/**
 * In-process session keys + paymaster state for WASM imports
 * `spacekit_session` / `spacekit_paymaster` (see `spacekit-contract-sdk` `agent_host.rs`).
 */

const MAX_POLICY_JSON_BYTES = 32_768;
const MAX_SCOPE_LEN = 512;

export type PaymasterPolicyJson = {
  allowed_dids?: string[];
  allowed_ops?: string[];
  per_call_max?: string;
  daily_max?: string;
  expires_at?: number;
  /** Initial sponsor budget (decimal string, same convention as vault_charge amounts). */
  budget?: string;
};

type SessionRow = {
  ownerDid: string;
  delegateDid: string;
  scopeRaw: string;
  expiresAt: number;
  revoked: boolean;
};

function utcDayKey(): string {
  return new Date().toISOString().slice(0, 10);
}

function parsePositiveIntString(s: string): bigint {
  const t = s.trim();
  if (!/^\d+$/.test(t)) {
    throw new Error("invalid_int_string");
  }
  return BigInt(t);
}

export function scopeAllowsOperation(scopeRaw: string, operation: string): boolean {
  const parts = scopeRaw
    .split("|")
    .map((p) => p.trim())
    .filter(Boolean);
  if (parts.includes("*")) {
    return true;
  }
  return parts.includes(operation);
}

export function didMatchesPattern(caller: string, pattern: string): boolean {
  const p = pattern.trim();
  if (p === "*") {
    return true;
  }
  if (p.endsWith("*") && p.length > 1) {
    return caller.startsWith(p.slice(0, -1));
  }
  return caller === p;
}

export function randomSessionIdHex(): string {
  const b = new Uint8Array(32);
  globalThis.crypto.getRandomValues(b);
  let hex = "";
  for (let i = 0; i < b.length; i += 1) {
    hex += b[i]!.toString(16).padStart(2, "0");
  }
  return hex;
}

export class SessionHostState {
  private sessions = new Map<string, SessionRow>();

  create(
    ownerDid: string,
    delegateDid: string,
    scopeRaw: string,
    expiresAtSec: number,
  ): Uint8Array {
    if (!delegateDid || scopeRaw.length > MAX_SCOPE_LEN) {
      throw new Error("session_create_invalid");
    }
    const now = Math.floor(Date.now() / 1000);
    if (expiresAtSec <= now) {
      throw new Error("session_create_expired");
    }
    const id = randomSessionIdHex();
    this.sessions.set(id, {
      ownerDid,
      delegateDid,
      scopeRaw,
      expiresAt: expiresAtSec,
      revoked: false,
    });
    return new TextEncoder().encode(id);
  }

  revoke(ownerDid: string, sessionId: string): boolean {
    const row = this.sessions.get(sessionId);
    if (!row || row.ownerDid !== ownerDid) {
      return false;
    }
    row.revoked = true;
    return true;
  }

  /** 1 = valid, 0 = invalid / expired, throws on bad args */
  validate(callerDid: string, ownerDid: string, operation: string): number {
    const now = Math.floor(Date.now() / 1000);
    for (const row of this.sessions.values()) {
      if (row.revoked) {
        continue;
      }
      if (row.ownerDid !== ownerDid || row.delegateDid !== callerDid) {
        continue;
      }
      if (row.expiresAt < now) {
        continue;
      }
      if (scopeAllowsOperation(row.scopeRaw, operation)) {
        return 1;
      }
    }
    return 0;
  }
}

type SponsorLedger = {
  policy: PaymasterPolicyJson;
  budget: bigint;
  dailySpent: bigint;
  dailyKey: string;
};

export class PaymasterHostState {
  private ledgers = new Map<string, SponsorLedger>();

  setPolicy(sponsorDid: string, jsonUtf8: string): void {
    if (jsonUtf8.length > MAX_POLICY_JSON_BYTES) {
      throw new Error("policy_too_large");
    }
    const parsed = JSON.parse(jsonUtf8) as PaymasterPolicyJson;
    const existing = this.ledgers.get(sponsorDid);
    let nextBudget = existing?.budget ?? 0n;
    if (parsed.budget !== undefined && parsed.budget !== null) {
      nextBudget = parsePositiveIntString(String(parsed.budget));
    }
    const day = utcDayKey();
    const dailySpent =
      existing && existing.dailyKey === day ? existing.dailySpent : 0n;
    this.ledgers.set(sponsorDid, {
      policy: parsed,
      budget: nextBudget,
      dailySpent,
      dailyKey: day,
    });
  }

  getBudgetString(sponsorDid: string): string {
    const L = this.ledgers.get(sponsorDid);
    if (!L) {
      return "0";
    }
    this.rollDaily(L);
    return L.budget.toString();
  }

  private rollDaily(L: SponsorLedger): void {
    const d = utcDayKey();
    if (L.dailyKey !== d) {
      L.dailyKey = d;
      L.dailySpent = 0n;
    }
  }

  /**
   * Validates sponsor policy + budget, decrements in-memory budget, returns true if allowed.
   * Caller should enqueue network flush separately.
   */
  trySponsorCharge(
    callerDid: string,
    sponsorDid: string,
    amountStr: string,
    operation: string,
  ): boolean {
    const L = this.ledgers.get(sponsorDid);
    if (!L) {
      return false;
    }
    this.rollDaily(L);

    const pol = L.policy;
    if (typeof pol.expires_at === "number" && pol.expires_at < Math.floor(Date.now() / 1000)) {
      return false;
    }
    const allowedDids = pol.allowed_dids ?? [];
    if (allowedDids.length === 0) {
      return false;
    }
    if (!allowedDids.some((pat) => didMatchesPattern(callerDid, pat))) {
      return false;
    }
    const allowedOps = pol.allowed_ops ?? [];
    if (allowedOps.length === 0) {
      return false;
    }
    if (!allowedOps.includes(operation)) {
      return false;
    }

    const amount = parsePositiveIntString(amountStr);
    if (amount <= 0n) {
      return false;
    }
    if (pol.per_call_max) {
      const max = parsePositiveIntString(pol.per_call_max);
      if (amount > max) {
        return false;
      }
    }
    if (pol.daily_max) {
      const dm = parsePositiveIntString(pol.daily_max);
      if (L.dailySpent + amount > dm) {
        return false;
      }
    }
    if (amount > L.budget) {
      return false;
    }

    L.budget -= amount;
    L.dailySpent += amount;
    return true;
  }
}
