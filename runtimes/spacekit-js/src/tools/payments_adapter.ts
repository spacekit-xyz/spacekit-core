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
 * Payment adapter that submits transfer and vault-charge intents
 * to a SpaceKit payment API.
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

  async transfer(to: string, asset: string, amount: bigint): Promise<boolean> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);

    try {
      const res = await fetch(`${this.endpoint}/transfer`, {
        method: "POST",
        headers: this.headers,
        body: JSON.stringify({ to, asset, amount: amount.toString() }),
        signal: controller.signal,
      });
      return res.ok;
    } finally {
      clearTimeout(timer);
    }
  }

  async vaultCharge(amount: string, beneficiary: string): Promise<boolean> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);

    try {
      const res = await fetch(`${this.endpoint}/vault-charge`, {
        method: "POST",
        headers: this.headers,
        body: JSON.stringify({ amount, beneficiary }),
        signal: controller.signal,
      });
      return res.ok;
    } finally {
      clearTimeout(timer);
    }
  }

  async sponsorVaultCharge(
    sponsorDid: string,
    amount: string,
    beneficiaryDid: string,
    operation: string,
  ): Promise<boolean> {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeoutMs);
    try {
      const res = await fetch(`${this.endpoint}/sponsor-vault-charge`, {
        method: "POST",
        headers: this.headers,
        body: JSON.stringify({
          sponsor: sponsorDid,
          amount,
          beneficiary: beneficiaryDid,
          operation,
        }),
        signal: controller.signal,
      });
      return res.ok;
    } catch {
      return false;
    } finally {
      clearTimeout(timer);
    }
  }
}

/**
 * Noop payment adapter for local/dev environments.
 * All operations succeed without performing real transfers.
 */
export class NoopPaymentAdapter implements PaymentAdapter {
  async transfer(_to: string, _asset: string, _amount: bigint): Promise<boolean> {
    return true;
  }
  async vaultCharge(_amount: string, _beneficiary: string): Promise<boolean> {
    return true;
  }
  async sponsorVaultCharge(
    _sponsorDid: string,
    _amount: string,
    _beneficiaryDid: string,
    _operation: string,
  ): Promise<boolean> {
    return true;
  }
}
