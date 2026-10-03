/**
 * In-process session keys for the WASM import `spacekit_session` (see
 * `spacekit-contract-sdk` `agent_host.rs`).
 *
 * Sponsored payments are not here: they are the `spacekit-paymaster`
 * contract, which holds native ASTRA on the chain.
 */

const MAX_SCOPE_LEN = 512;

type SessionRow = {
  ownerDid: string;
  delegateDid: string;
  scopeRaw: string;
  expiresAt: number;
  revoked: boolean;
};

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
