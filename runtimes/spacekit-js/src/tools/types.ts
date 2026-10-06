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

/**
 * A payment a contract asked for with `payment_transfer`. ASTRA is the only
 * currency: `asset` is always "ASTRA" and `amount` is wei (decimal string).
 */
export interface PaymentEffect {
  type: "transfer";
  to: string;
  asset: "ASTRA";
  amount: string;
}

/**
 * Carries out `payment_transfer` effects after a local execution. ASTRA moves
 * only on the chain, so an implementation must submit a chain transaction
 * (and is responsible for the payer's consent); it must never keep balances.
 */
export interface PaymentAdapter {
  transfer(to: string, asset: "ASTRA", amountWei: bigint): Promise<boolean>;
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
