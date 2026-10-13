/**
 * Host-side ASTRA payments for embedded apps.
 *
 * `payments.subscribe` in an app asks the host for a payment. The host shows
 * the viewer what they are paying, sends an ASTRA transfer on the SpaceKit
 * chain from the viewer's account, and returns the transaction hash. A trusted
 * service (`recordSubscription`) then verifies that transaction on the chain.
 *
 * `createAstraPaymentRequester` wires those steps together. The host supplies
 * the consent prompt and the transfer (for example spacekit-js
 * `ChainClient.transfer` with the viewer's `ChainAccount`); this module never
 * holds keys.
 */

import { formatAstra } from "../astra.js";
import type { SubscriptionPaymentRequest, SubscriptionPaymentResult } from "./types.js";

const SPACEKIT_DID = /^did:spacekit:(?:[a-z0-9-]+:)?(?:0x)?([0-9a-fA-F]{40})$/;

/** The chain address inside a `did:spacekit:[network:]<40 hex>` DID, or null. */
export function chainAddressOfDid(did: string): string | null {
  const m = SPACEKIT_DID.exec(did.trim());
  return m ? `0x${m[1].toLowerCase()}` : null;
}

export interface AstraPaymentPrompt extends SubscriptionPaymentRequest {
  /** Where the ASTRA goes. */
  payTo: string;
  /** The amount for display, e.g. "1.5". */
  amountAstra: string;
}

export interface AstraPaymentRequesterOptions {
  /**
   * Ask the viewer to approve. Resolve false to cancel. Show at least the app,
   * `amountAstra` ASTRA and `payTo`.
   */
  confirm(prompt: AstraPaymentPrompt): boolean | Promise<boolean>;
  /** Send `amountWei` ASTRA to `to` from the viewer's account; resolve the tx hash. */
  transfer(to: string, amountWei: bigint): Promise<string>;
  /**
   * The publisher's payout address. Defaults to the address in a
   * `did:spacekit:` DID. Publishers with other DIDs register a payout address
   * with the marketplace; look it up here.
   */
  resolvePayee?(publisherDid: string, appId: string): string | null | Promise<string | null>;
}

/** Build an `EmbedHostServices.requestSubscriptionPayment` hook that pays in ASTRA. */
export function createAstraPaymentRequester(
  options: AstraPaymentRequesterOptions,
): (req: SubscriptionPaymentRequest) => Promise<SubscriptionPaymentResult> {
  return async (req) => {
    if (req.asset !== "ASTRA") throw new Error(`Unsupported asset ${String(req.asset)}`);
    const amountWei = BigInt(req.amountWei);
    if (amountWei <= 0n) throw new Error("Payment amount must be positive");
    const payTo =
      (await options.resolvePayee?.(req.publisherDid, req.appId)) ?? chainAddressOfDid(req.publisherDid);
    if (!payTo) {
      throw new Error("This publisher has no ASTRA payout address, so the app cannot take payments yet");
    }
    const approved = await options.confirm({ ...req, payTo, amountAstra: formatAstra(amountWei) });
    if (!approved) throw new Error("Payment cancelled");
    const txHash = await options.transfer(payTo, amountWei);
    return { txHash };
  };
}
