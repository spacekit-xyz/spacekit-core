/**
 * Adapter interfaces for VM agent tools.
 *
 * Each adapter handles the actual I/O for one tool category.  The host import
 * layer in host.ts is synchronous (WASM constraint); adapters are async and
 * fulfilled by the ToolEffectManager between contract re-executions.
 */

/* ─── Messaging + tool intents (SearchResult JSON shape documented below) ─ */
export interface SearchResult {
  title: string;
  url: string;
  snippet: string | null;
}

/** Messaging topic for `MessagingAdapter.requestResponse` when fulfilling `web_search` tool effects. */
export const SPACEKIT_WEB_SEARCH_TOPIC = "spacekit.tools.web_search.request";

/**
 * SpaceKit Messaging Node adapter for agent tools.
 * Fire-and-forget `send` is used after contract execution (`messaging_send` host).
 * Optional `requestResponse` fulfills effect-queue tools (e.g. `web_search`) by sending
 * a synchronous intent-style request to an operator DID (Messaging Node relays / operator responds).
 */
export interface MessagingAdapter {
  send(recipientDid: string, payload: Uint8Array): Promise<boolean>;
  /**
   * Request/response over messaging (typically `POST …/tool-request` on the Messaging Node).
   * Used to fulfill `web_search` pending effects — the browser never calls search HTTP directly.
   */
  requestResponse?(
    operatorDid: string,
    topic: string,
    payload: Uint8Array,
  ): Promise<Uint8Array>;
}

/* ─── Remote Storage (SpaceTime Storage Node) ────────────── */

export interface RemoteStorageAdapter {
  put(data: Uint8Array): Promise<string>;
  get(ref: string): Promise<Uint8Array | null>;
}

/* ─── Payments (intent-based) ────────────────────────────── */

export interface PaymentEffect {
  type: "transfer" | "vault_charge" | "sponsor_vault_charge";
  to: string;
  asset: string;
  amount: string;
  beneficiary?: string;
  /** Set when `type === "sponsor_vault_charge"` — vault debit is attributed to this sponsor. */
  sponsorDid?: string;
  /** Operation label validated against paymaster policy (e.g. `vault_charge`). */
  operation?: string;
}

export interface PaymentAdapter {
  transfer(to: string, asset: string, amount: bigint): Promise<boolean>;
  vaultCharge(amount: string, beneficiary: string): Promise<boolean>;
  /**
   * Optional: mirror `paymaster_sponsor_charge` to a treasury / payment API after the VM
   * has already enforced in-memory sponsor policy and budget.
   */
  sponsorVaultCharge?(
    sponsorDid: string,
    amount: string,
    beneficiaryDid: string,
    operation: string,
  ): Promise<boolean>;
}

/* ─── Buffered side-effects (fire-and-forget) ────────────── */

export interface BufferedMessage {
  recipientDid: string;
  payload: Uint8Array;
}

export interface BufferedPayment {
  effect: PaymentEffect;
}

export interface ToolSideEffects {
  messages: BufferedMessage[];
  payments: BufferedPayment[];
}

export function createToolSideEffects(): ToolSideEffects {
  return { messages: [], payments: [] };
}
