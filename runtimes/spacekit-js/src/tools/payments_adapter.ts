import type { PaymentAdapter } from "./types.js";

export interface PaymentAdapterOptions {
  /** Base URL of the payment API endpoint */
  endpoint: string;
  /** Auth token or DID credential */
  authToken?: string;
  /** Extra headers */
  headers?: Record<string, string>;
  /** Request timeout in milliseconds (default: 10000) */
  timeoutMs?: number;
}

/**
 * Payment adapter that forwards ASTRA transfer requests to a payment API,
 * which must settle them as chain transactions.
 */
export class HttpPaymentAdapter implements PaymentAdapter {
  private endpoint: string;
  private headers: Record<string, string>;
  private timeoutMs: number;

  constructor(options: PaymentAdapterOptions) {
    this.endpoint = options.endpoint.replace(/\/$/, "");
    this.headers = {
      "Content-Type": "application/json",
      ...options.headers,
    };
    if (options.authToken) {
      this.headers["Authorization"] = `Bearer ${options.authToken}`;
    }
    this.timeoutMs = options.timeoutMs ?? 10_000;
  }

  async transfer(to: string, asset: "ASTRA", amountWei: bigint): Promise<boolean> {
    if (asset !== "ASTRA") return false;
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);

    try {
      const res = await fetch(`${this.endpoint}/transfer`, {
        method: "POST",
        headers: this.headers,
        body: JSON.stringify({ to, asset: "ASTRA", amount_wei: amountWei.toString() }),
        signal: controller.signal,
      });
      return res.ok;
    } finally {
      clearTimeout(timer);
    }
  }
}

/**
 * Payment adapter for local/dev environments. It moves nothing, so it reports
 * every transfer as failed rather than pretending a payment happened.
 */
export class NoopPaymentAdapter implements PaymentAdapter {
  async transfer(_to: string, _asset: "ASTRA", _amountWei: bigint): Promise<boolean> {
    return false;
  }
}
