/**
 * Intent Builder for SpaceKit Intent-Based Payments
 *
 * Composable helpers for building signed intents that include
 * contract execution, vault charges, and transfer actions.
 *
 * Usage:
 *   const intent = new IntentBuilder("did:alice", "spacekit:mainnet")
 *     .vaultCharge("2.00", "did:contract:xyz")
 *     .executeContract("did:contract:xyz", inputHex, { maxFeeUsdc: "3.00" })
 *     .maxNotionalUsd(5.0)
 *     .build();
 */

import { assertSignableExpiry, canonicalIntentPayload } from "./intent_canonical.js";

/* ─── Types ─────────────────────────────────────────────── */

export interface ExecuteContractAction {
  type: "execute_contract";
  contract_id: string;
  input: string;
  value_astra?: string;
  max_fee_usdc?: string;
  max_fee_astra?: string;
}

export interface VaultChargeAction {
  type: "vault_charge";
  amount_ausd: string;
  beneficiary: string;
}

export interface TransferAction {
  type: "transfer";
  asset: string;
  to: string;
  amount: string;
}

export type IntentAction = ExecuteContractAction | VaultChargeAction | TransferAction;

export interface IntentConstraints {
  max_notional_usd?: number;
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

export interface FeeEstimate {
  total_ausd: number;
  total_astra: number;
  breakdown: {
    action_type: string;
    label: string;
    amount_ausd: number;
    amount_astra: number;
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

  /** Add a vault charge action to deduct aUSD before execution. */
  vaultCharge(amountAusd: string, beneficiary: string): this {
    this.actions.push({
      type: "vault_charge",
      amount_ausd: amountAusd,
      beneficiary,
    });
    return this;
  }

  /** Add a contract execution action. */
  executeContract(
    contractId: string,
    inputHex: string,
    opts?: {
      valueAstra?: string;
      maxFeeUsdc?: string;
      maxFeeAstra?: string;
    },
  ): this {
    const action: ExecuteContractAction = {
      type: "execute_contract",
      contract_id: contractId,
      input: inputHex,
    };
    if (opts?.valueAstra) action.value_astra = opts.valueAstra;
    if (opts?.maxFeeUsdc) action.max_fee_usdc = opts.maxFeeUsdc;
    if (opts?.maxFeeAstra) action.max_fee_astra = opts.maxFeeAstra;
    this.actions.push(action);
    return this;
  }

  /** Add a native ASTRA transfer action. */
  transferAstra(to: string, amount: string): this {
    this.actions.push({
      type: "transfer",
      asset: "spacekit:mainnet:native",
      to,
      amount,
    });
    return this;
  }

  /** Set the max notional USD constraint. */
  maxNotionalUsd(value: number): this {
    this.constraints.max_notional_usd = value;
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

const DEFAULT_USDC_TO_ASTRA_RATE = 1_000_000;
const DEFAULT_NETWORK_FEE_BPS = 25;

/**
 * Estimate fees for a set of intent actions.
 * Uses the same conversion logic as spacekit-payments FeeRouter.
 */
export function estimateIntentFees(
  actions: IntentAction[],
  opts?: { usdcToAstraRate?: number; networkFeeBps?: number },
): FeeEstimate {
  const rate = opts?.usdcToAstraRate ?? DEFAULT_USDC_TO_ASTRA_RATE;
  const feeBps = opts?.networkFeeBps ?? DEFAULT_NETWORK_FEE_BPS;
  const breakdown: FeeEstimate["breakdown"] = [];
  let totalAusd = 0;
  let totalAstra = 0;

  for (const action of actions) {
    switch (action.type) {
      case "vault_charge": {
        const amount = parseFloat(action.amount_ausd) || 0;
        const fee = (amount * feeBps) / 10_000;
        const netAstra = Math.floor((amount - fee) * rate);
        breakdown.push({
          action_type: "vault_charge",
          label: `Charge ${action.amount_ausd} aUSD → ${action.beneficiary}`,
          amount_ausd: amount,
          amount_astra: netAstra,
        });
        totalAusd += amount;
        totalAstra += netAstra;
        break;
      }
      case "execute_contract": {
        const value = parseInt(action.value_astra ?? "0", 10) || 0;
        const maxUsdc = parseFloat(action.max_fee_usdc ?? "0") || 0;
        const maxAstra = parseInt(action.max_fee_astra ?? "0", 10) || 0;
        breakdown.push({
          action_type: "execute_contract",
          label: `Execute ${action.contract_id}`,
          amount_ausd: maxUsdc,
          amount_astra: value + maxAstra,
        });
        totalAusd += maxUsdc;
        totalAstra += value + maxAstra;
        break;
      }
      case "transfer": {
        const amount = parseInt(action.amount, 10) || 0;
        const fee = Math.floor((amount * feeBps) / 10_000);
        breakdown.push({
          action_type: "transfer",
          label: `Transfer ${action.amount} → ${action.to}`,
          amount_ausd: 0,
          amount_astra: amount + fee,
        });
        totalAstra += amount + fee;
        break;
      }
    }
  }

  return { total_ausd: totalAusd, total_astra: totalAstra, breakdown };
}

/* ─── Convenience: build + estimate in one step ─────────── */

/**
 * High-level helper: build an execute-contract intent with automatic vault
 * charge, fee estimation, and signing.
 */
export async function buildExecuteContractIntent(opts: {
  actor: string;
  contractId: string;
  inputHex: string;
  chain?: string;
  valueAstra?: string;
  maxFeeUsdc?: string;
  agent?: string;
  signer: IntentSignerFn;
  usdcToAstraRate?: number;
}): Promise<{ signed: SignedIntent; fees: FeeEstimate }> {
  const builder = new IntentBuilder(opts.actor, opts.chain);

  if (opts.agent) builder.delegateTo(opts.agent);

  if (opts.maxFeeUsdc) {
    builder.vaultCharge(opts.maxFeeUsdc, opts.contractId);
    builder.maxNotionalUsd(parseFloat(opts.maxFeeUsdc));
  }

  builder.executeContract(opts.contractId, opts.inputHex, {
    valueAstra: opts.valueAstra,
    maxFeeUsdc: opts.maxFeeUsdc,
  });

  const intent = builder.build();
  const fees = estimateIntentFees(intent.actions, {
    usdcToAstraRate: opts.usdcToAstraRate,
  });
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
