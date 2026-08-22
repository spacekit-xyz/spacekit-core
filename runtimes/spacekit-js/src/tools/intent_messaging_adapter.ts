import type { MessagingAdapter } from "./types.js";
import type { SpacekitMessageEnvelope } from "../spacetime/message.js";
import { bytesToHex } from "../storage.js";

export interface IntentMessagingAdapterOptions {
  /** Messaging Node base URL (no trailing slash). */
  baseUrl: string;
  /** Extra headers on every POST. */
  headers?: Record<string, string>;
  /**
   * Path for synchronous tool intents (effect-queue fulfillment).
   * Messaging Node SHOULD implement POST with JSON body and return `{ result_utf8?: string }` or `{ error?: string }`.
   */
  toolRequestPath?: string;
  /** Path for envelope send (defaults to `/api/messages/envelope`). */
  envelopePath?: string;
  /** Caller DID echoed in intents (optional metadata). */
  callerDid?: string;
  timeoutMs?: number;
}

function joinUrl(base: string, path: string): string {
  const b = base.replace(/\/$/, "");
  const p = path.startsWith("/") ? path : `/${path}`;
  return `${b}${p}`;
}

/**
 * Adapter that routes tool completion (including `web_search`) through the Messaging Node
 * using an intent-shaped JSON payload — not direct browser → search-provider HTTP.
 *
 * Compatible with RouteKit-style intent relays: POST body carries `recipient_did`, `topic`,
 * and opaque `payload_utf8`; response carries `result_utf8`.
 */
export class IntentMessagingToolAdapter implements MessagingAdapter {
  private readonly baseUrl: string;
  private readonly headers: Record<string, string>;
  private readonly toolRequestPath: string;
  private readonly envelopePath: string;
  private readonly callerDid: string;
  private readonly timeoutMs: number;

  constructor(options: IntentMessagingAdapterOptions) {
    this.baseUrl = options.baseUrl.replace(/\/$/, "");
    this.headers = { "Content-Type": "application/json", ...options.headers };
    this.toolRequestPath = options.toolRequestPath ?? "/api/messages/tool-request";
    this.envelopePath = options.envelopePath ?? "/api/messages/envelope";
    this.callerDid = options.callerDid ?? "did:spacekit:vm:anonymous";
    this.timeoutMs = options.timeoutMs ?? 60_000;
  }

  async send(recipientDid: string, payload: Uint8Array): Promise<boolean> {
    const url = joinUrl(this.baseUrl, this.envelopePath);
    const message: SpacekitMessageEnvelope<Record<string, unknown>> = {
      kind: "spacetime",
      payload: {
        recipient_did: recipientDid,
        payload_hex: bytesToHex(payload),
      },
      context: {
        did: this.callerDid,
        timestamp: Date.now(),
        source: "spacekit-vm",
      },
    };
    const body = {
      message,
      conversation_type: "direct",
      recipient_did: recipientDid,
    };
    try {
      const ctrl = AbortSignal.timeout(this.timeoutMs);
      const res = await fetch(url, { method: "POST", headers: this.headers, body: JSON.stringify(body), signal: ctrl });
      return res.ok;
    } catch {
      return false;
    }
  }

  async requestResponse(
    operatorDid: string,
    topic: string,
    payload: Uint8Array,
  ): Promise<Uint8Array> {
    const url = joinUrl(this.baseUrl, this.toolRequestPath);
    const payload_utf8 = new TextDecoder().decode(payload);
    const ctrl = AbortSignal.timeout(this.timeoutMs);
    const res = await fetch(url, {
      method: "POST",
      headers: this.headers,
      body: JSON.stringify({
        recipient_did: operatorDid,
        topic,
        caller_did: this.callerDid,
        payload_utf8,
      }),
      signal: ctrl,
    });
    const text = await res.text();
    if (!res.ok) {
      throw new Error(`tool-request HTTP ${res.status}: ${text.slice(0, 200)}`);
    }
    let parsed: { result_utf8?: string; error?: string };
    try {
      parsed = JSON.parse(text);
    } catch {
      throw new Error(`tool-request: non-JSON body: ${text.slice(0, 120)}`);
    }
    if (parsed.error) {
      throw new Error(parsed.error);
    }
    if (typeof parsed.result_utf8 !== "string") {
      throw new Error("tool-request: missing result_utf8");
    }
    return new TextEncoder().encode(parsed.result_utf8);
  }
}
