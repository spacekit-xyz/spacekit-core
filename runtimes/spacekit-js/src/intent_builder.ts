/**
 * Intent Builder for SpaceKit Intent-Based Payments
 *
 * Composable helpers for building signed intents with contract execution and
 * ASTRA transfer actions. ASTRA is SpaceKit's only currency: every amount is
 * wei (18 decimals) as a decimal string, so no precision is lost.
 *
 * Usage:
 *   const intent = new IntentBuilder("did:alice", "spacekit:mainnet")
 *     .executeContract("did:contract:xyz", inputHex, { valueWei: parseAstra("2") })
 *     .transferAstra("did:bob", parseAstra("0.5"))
 *     .maxValueWei(parseAstra("3"))
 *     .build();
 */

import { assertSignableExpiry, canonicalIntentPayload } from "./intent_canonical.js";

/* ─── Types ─────────────────────────────────────────────── */

export interface ExecuteContractAction {
  type: "execute_contract";
  contract_id: string;
  input: string;
  /** ASTRA wei attached as `msg_value` (decimal string). */
  value_astra?: string;
  /** Maximum fee in ASTRA wei (decimal string). */
  max_fee_astra?: string;
}

export interface TransferAction {
  type: "transfer";
  asset: string;
  to: string;
  /** ASTRA wei (decimal string). */
  amount: string;
}

export type IntentAction = ExecuteContractAction | TransferAction;

export interface IntentConstraints {
  /** Most ASTRA wei the intent may move (decimal string). */
  max_value_wei?: string;
  [key: string]: unknown;
}

export interface Intent {
  intent_id: string;
  version: string;
  actor: string;
  agent?: string;
  chain: string;
  constraints: IntentConstraints;
  actions: IntentAction[];
  nonce: string;
  expiry: number;
  meta?: Record<string, unknown>;
}

export interface SignedIntent {
  intent: Intent;
  signature: string;
  sig_type: string;
}

/** Fees and value of an intent, all in ASTRA wei. */
export interface FeeEstimate {
  total_wei: bigint;
  breakdown: {
    action_type: string;
    label: string;
    amount_wei: bigint;
  }[];
}

/**
 * Signs the canonical intent payload.
 *
 * The argument is the full signing payload from
 * {@link canonicalIntentPayload}, not the intent ID. Signing only the ID left
 * every economically meaningful field — actions, amounts, beneficiaries,
 * expiry — outside the signature and therefore rewritable in transit.
 */
export interface IntentSignerFn {
  (payload: string): Promise<{ signature: string; sig_type: string }>;
}

export interface FeeEstimatorFn {
  (actions: IntentAction[]): Promise<FeeEstimate>;
}

function weiString(v: bigint | string): string {
  const b = typeof v === "bigint" ? v : BigInt(v.trim());
  if (b < 0n) throw new Error("ASTRA amounts cannot be negative");
  return b.toString();
}

/* ─── Builder ───────────────────────────────────────────── */

export class IntentBuilder {
  private actor: string;
  private chain: string;
  private agent?: string;
  private actions: IntentAction[] = [];
  private constraints: IntentConstraints = {};
  private meta: Record<string, unknown> = {};
  private expirySeconds: number = 300; // default: 5 minutes
  private nonceOverride?: string;

  constructor(actor: string, chain: string = "spacekit:mainnet") {
    this.actor = actor;
    this.chain = chain;
  }

  /** Set an agent DID for delegated execution. */
  delegateTo(agentDid: string): this {
    this.agent = agentDid;
    return this;
  }

  /** Add a contract execution action. */
  executeContract(
    contractId: string,
    inputHex: string,
    opts?: {
      /** ASTRA wei to attach. */
      valueWei?: bigint | string;
      /** Maximum fee in ASTRA wei. */
      maxFeeWei?: bigint | string;
    },
  ): this {
    const action: ExecuteContractAction = {
      type: "execute_contract",
      contract_id: contractId,
      input: inputHex,
    };
    if (opts?.valueWei !== undefined) action.value_astra = weiString(opts.valueWei);
    if (opts?.maxFeeWei !== undefined) action.max_fee_astra = weiString(opts.maxFeeWei);
    this.actions.push(action);
    return this;
  }

  /** Add a native ASTRA transfer action (`amountWei` in wei). */
  transferAstra(to: string, amountWei: bigint | string): this {
    this.actions.push({
      type: "transfer",
      asset: "spacekit:mainnet:native",
      to,
      amount: weiString(amountWei),
    });
    return this;
  }

  /** Most ASTRA wei the intent may move (attached value plus transfers). */
  maxValueWei(value: bigint | string): this {
    this.constraints.max_value_wei = weiString(value);
    return this;
  }

  /** Set a custom constraint. */
  constraint(key: string, value: unknown): this {
    this.constraints[key] = value;
    return this;
  }

  /** Set intent expiry (default: 300 seconds from now). */
  expiry(seconds: number): this {
    this.expirySeconds = seconds;
    return this;
  }

  /** Override the nonce (default: timestamp-based). */
  nonce(nonce: string): this {
    this.nonceOverride = nonce;
    return this;
  }

  /** Add metadata to the intent. */
  addMeta(key: string, value: unknown): this {
    this.meta[key] = value;
    return this;
  }

  /** Build the unsigned intent. */
  build(): Intent {
    const now = Math.floor(Date.now() / 1000);
    return {
      intent_id: generateIntentId(),
      version: "1.0",
      actor: this.actor,
      agent: this.agent,
      chain: this.chain,
      constraints: this.constraints,
      actions: this.actions,
      nonce: this.nonceOverride ?? now.toString(),
      expiry: now + this.expirySeconds,
      meta: Object.keys(this.meta).length > 0 ? this.meta : undefined,
    };
  }

  /**
   * Build and sign the intent.
   *
   * The signature covers the canonical payload over every field, so the
   * network can reject an intent whose contents were altered after signing.
   */
  async buildAndSign(signer: IntentSignerFn): Promise<SignedIntent> {
    const intent = this.build();
    assertSignableExpiry(intent);
    const payload = await canonicalIntentPayload(intent);
    const { signature, sig_type } = await signer(payload);
    return { intent, signature, sig_type };
  }
}

/* ─── Fee Estimation ────────────────────────────────────── */

const DEFAULT_NETWORK_FEE_BPS = 25n;

/**
 * Estimate what an intent costs, in ASTRA wei: attached contract value plus
 * the fee cap, and transfers plus the network fee (as spacekit-payments'
 * FeeRouter computes it).
 */
export function estimateIntentFees(
  actions: IntentAction[],
  opts?: { networkFeeBps?: number },
): FeeEstimate {
  const feeBps = BigInt(opts?.networkFeeBps ?? Number(DEFAULT_NETWORK_FEE_BPS));
  const breakdown: FeeEstimate["breakdown"] = [];
  let total = 0n;

  for (const action of actions) {
    switch (action.type) {
      case "execute_contract": {
        const amount = BigInt(action.value_astra ?? "0") + BigInt(action.max_fee_astra ?? "0");
        breakdown.push({
          action_type: "execute_contract",
          label: `Execute ${action.contract_id}`,
          amount_wei: amount,
        });
        total += amount;
        break;
      }
      case "transfer": {
        const amount = BigInt(action.amount);
        const fee = (amount * feeBps) / 10_000n;
        breakdown.push({
          action_type: "transfer",
          label: `Transfer ${action.amount} wei → ${action.to}`,
          amount_wei: amount + fee,
        });
        total += amount + fee;
        break;
      }
    }
  }

  return { total_wei: total, breakdown };
}

/* ─── Convenience: build + estimate in one step ─────────── */

/**
 * High-level helper: build an execute-contract intent, estimate its cost in
 * ASTRA, and sign it. The intent's `max_value_wei` is the attached value.
 */
export async function buildExecuteContractIntent(opts: {
  actor: string;
  contractId: string;
  inputHex: string;
  chain?: string;
  /** ASTRA wei to attach. */
  valueWei?: bigint | string;
  /** Maximum fee in ASTRA wei. */
  maxFeeWei?: bigint | string;
  agent?: string;
  signer: IntentSignerFn;
}): Promise<{ signed: SignedIntent; fees: FeeEstimate }> {
  const builder = new IntentBuilder(opts.actor, opts.chain);

  if (opts.agent) builder.delegateTo(opts.agent);

  builder.executeContract(opts.contractId, opts.inputHex, {
    valueWei: opts.valueWei,
    maxFeeWei: opts.maxFeeWei,
  });
  builder.maxValueWei(opts.valueWei ?? 0n);

  const intent = builder.build();
  const fees = estimateIntentFees(intent.actions);
  assertSignableExpiry(intent);
  const { signature, sig_type } = await opts.signer(await canonicalIntentPayload(intent));
  const signed: SignedIntent = { intent, signature, sig_type };

  return { signed, fees };
}

/* ─── Utilities ─────────────────────────────────────────── */

/**
 * Generate a 128-bit intent ID from a CSPRNG.
 *
 * There is deliberately no `Math.random()` fallback: it is seeded predictably
 * in several JS runtimes, and a guessable intent ID lets an attacker
 * front-run or collide with a pending intent. If no CSPRNG is available we
 * fail rather than silently downgrade.
 */
function generateIntentId(): string {
  const bytes = new Uint8Array(16);
  const webcrypto = globalThis.crypto;
  if (!webcrypto?.getRandomValues) {
    throw new Error(
      "No cryptographically secure random source available (globalThis.crypto.getRandomValues). " +
        "Refusing to generate an intent ID from a predictable source.",
    );
  }
  webcrypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
