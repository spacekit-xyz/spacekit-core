/**
 * Paid channels: a messaging group whose membership is an on-chain
 * subscription on the entitlement-ledger contract.
 *
 * Publisher:
 *   1. picks a listing id and creates the group (`visibility: "paid"`);
 *   2. submits `OP_CREATE_LISTING` with `file_id` = the group id, so
 *      entitlements for the listing verify against exactly this channel.
 *
 * Subscriber:
 *   1. submits `OP_PURCHASE` (paying the listing price) bound to the hash of
 *      their Kyber public key, and gets an entitlement id;
 *   2. asks to join with that entitlement; the publisher's messaging node
 *      checks it on chain (`OP_VERIFY`, a read-only call to a compute node)
 *      and admits them; it re-checks periodically and removes members whose
 *      subscription expired or was revoked.
 *
 * Contract transactions are submitted by the caller (`submitContractCall`),
 * so the same helpers work with a chain node, a wallet or a local VM.
 *
 * @module messaging/channels
 */

import {
  buildCreateListingInput,
  buildPurchaseInput,
  buildRenewInput,
  buyerPkHashFromPublicKeyHex,
  parsePurchaseResult,
} from "../entitlement.js";
import { bytesToHex } from "../storage.js";
import type { GroupInfo, MessagingClient } from "./client.js";

/** Pricing type byte: one-time purchase. */
export const PRICING_ONE_TIME = 1;
/** Pricing type byte: subscription for `periodSecs`. */
export const PRICING_SUBSCRIPTION = 2;

/**
 * Submit a call to the entitlement-ledger contract and return the contract's
 * output once the transaction succeeded (throw if it failed).
 */
export type SubmitContractCall = (input: Uint8Array, value: bigint) => Promise<Uint8Array>;

export interface CreatePaidChannelOptions {
  client: MessagingClient;
  submitContractCall: SubmitContractCall;
  name: string;
  description?: string;
  /** Price per period (or one-time) in the token's smallest unit. */
  price: bigint;
  /** Token symbol recorded in the listing (default "ASTRA"). */
  token?: string;
  /** Subscription period in seconds (default 30 days); ignored for one-time. */
  periodSecs?: bigint;
  pricingType?: typeof PRICING_ONE_TIME | typeof PRICING_SUBSCRIPTION;
  /** Default: `channel:<random>`. */
  listingId?: string;
}

export interface PaidChannel {
  group: GroupInfo;
  listingId: string;
}

function randomId(): string {
  const bytes = new Uint8Array(12);
  globalThis.crypto.getRandomValues(bytes);
  return bytesToHex(bytes);
}

/** Publisher: create the group, then its listing. */
export async function createPaidChannel(options: CreatePaidChannelOptions): Promise<PaidChannel> {
  const listingId = options.listingId ?? `channel:${randomId()}`;
  const group = await options.client.createGroup({
    name: options.name,
    description: options.description,
    visibility: "paid",
    listingId,
  });
  const pricingType = options.pricingType ?? PRICING_SUBSCRIPTION;
  const input = buildCreateListingInput({
    listingId,
    fileId: group.id,
    price: options.price,
    token: options.token ?? "ASTRA",
    pricingType,
    period: pricingType === PRICING_SUBSCRIPTION ? options.periodSecs ?? 30n * 86_400n : 0n,
  });
  try {
    await options.submitContractCall(input, 0n);
  } catch (error) {
    // Without a listing nobody can subscribe; do not leave the group behind.
    await options.client.deleteGroup(group.id).catch(() => {});
    throw error;
  }
  return { group, listingId };
}

export interface SubscribeToChannelOptions {
  client: MessagingClient;
  submitContractCall: SubmitContractCall;
  /** The channel as listed by `client.listGroups()`. */
  group: GroupInfo;
  /** Listing price (paid with the purchase). */
  price: bigint;
  /** The subscriber's Kyber public key (hex); its hash binds the entitlement. */
  publicKeyHex: string;
  /** Wait for admission (default 30 s); 0 returns right after the request. */
  waitMs?: number;
}

export interface ChannelSubscription {
  entitlementId: string;
  buyerPkHash: string;
  /** The group after admission (`undefined` when `waitMs` is 0 and pending). */
  group?: GroupInfo;
}

/** Subscriber: buy an entitlement for the channel's listing and join. */
export async function subscribeToChannel(
  options: SubscribeToChannelOptions,
): Promise<ChannelSubscription> {
  const { client, group } = options;
  if (group.visibility !== "paid" || !group.listing_id) {
    throw new Error(`${group.id} is not a paid channel`);
  }
  const pkHash = buyerPkHashFromPublicKeyHex(options.publicKeyHex);
  const output = await options.submitContractCall(
    buildPurchaseInput(group.listing_id, pkHash),
    options.price,
  );
  const entitlementId = bytesToHex(parsePurchaseResult(output));
  return joinPaidChannel({
    client,
    group,
    entitlementId,
    buyerPkHash: bytesToHex(pkHash),
    waitMs: options.waitMs,
  });
}

/** Join with an entitlement already held (bought earlier, or granted). */
export async function joinPaidChannel(options: {
  client: MessagingClient;
  group: GroupInfo;
  entitlementId: string;
  buyerPkHash: string;
  waitMs?: number;
}): Promise<ChannelSubscription> {
  const { client, group } = options;
  const waitMs = options.waitMs ?? 30_000;
  const admitted = waitMs > 0 ? client.waitForMembership(group.id, waitMs) : undefined;
  // Avoid an unhandled rejection if the join itself throws first.
  admitted?.catch(() => {});
  const result = await client.joinGroup(group.id, {
    entitlementId: options.entitlementId,
    buyerPkHash: options.buyerPkHash,
  });
  const base = { entitlementId: options.entitlementId, buyerPkHash: options.buyerPkHash };
  if (result.status === "joined") {
    return { ...base, group: await client.getGroup(group.id).catch(() => undefined) };
  }
  return { ...base, group: admitted ? await admitted : undefined };
}

/**
 * Subscriber: extend a subscription by one period (pays the price to the
 * publisher on the chain). Resolves to the new expiry (Unix seconds). The
 * entitlement id stays the same, so channel nodes keep the member.
 */
export async function renewChannelSubscription(options: {
  submitContractCall: SubmitContractCall;
  entitlementId: string;
  price: bigint;
}): Promise<bigint> {
  const id = options.entitlementId.replace(/^0x/, "");
  const bytes = new Uint8Array(id.match(/../g)!.map((h) => parseInt(h, 16)));
  const out = await options.submitContractCall(buildRenewInput(bytes), options.price);
  if (out.length < 9 || out[0] !== 1) throw new Error("unexpected renew result");
  return new DataView(out.buffer, out.byteOffset + 1, 8).getBigUint64(0, true);
}
