/**
 * The chain is the only ledger.
 *
 * ASTRA exists in one place: the native account balance on the SpaceKit
 * chain (compute nodes), in wei (18 decimals). The browser holds no balance
 * of its own. This module is how spacekit-js reads balances and moves value:
 *
 * - `ChainAccount` is a k256 key and its chain address
 *   (`keccak256(uncompressed public key)[12..]`), with the DID
 *   `did:spacekit:<address hex>`.
 * - `ChainClient` reads balances and nonces, signs and submits transactions
 *   (the node's `SPACEKIT-TX-v2` payload), waits for receipts, and makes
 *   read-only contract calls.
 * - `chainContractCaller` adapts a client + account into the
 *   `SubmitContractCall` used by paid channels and entitlements.
 *
 * Holders of SPHINCS+ DIDs (whose address has no k256 key) spend through
 * `ChainClient.submitPqTransfer` with a SPHINCS+-signed transfer.
 *
 * @module chain/client
 */

import { secp256k1 } from "@noble/curves/secp256k1";
import { keccak_256 } from "@noble/hashes/sha3";
import { sha256 } from "@noble/hashes/sha2";

export const WEI_PER_ASTRA = 10n ** 18n;
export const ASTRA_DECIMALS = 18;

/** Default gas for a contract call; the node refunds what is not used. */
export const DEFAULT_GAS_LIMIT = 2_000_000n;
/** Gas charged for a plain transfer to an address without code. */
export const PLAIN_TRANSFER_GAS = 21_000n;

const hexOf = (bytes: Uint8Array): string =>
  Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");

const bytesOf = (hex: string): Uint8Array => {
  const h = hex.replace(/^0x/i, "");
  if (h.length % 2 !== 0 || /[^0-9a-f]/i.test(h)) throw new Error(`not hex: ${hex}`);
  const out = new Uint8Array(h.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(h.slice(2 * i, 2 * i + 2), 16);
  return out;
};

/** Normalize to lower-case `0x` + 40 hex. */
export function normalizeAddress(address: string): string {
  const h = address.replace(/^0x/i, "").toLowerCase();
  if (h.length !== 40 || /[^0-9a-f]/.test(h)) throw new Error(`not an address: ${address}`);
  return `0x${h}`;
}

/** The address in `did:spacekit:<40 hex>`, or `undefined`. */
export function addressOfDid(did: string): string | undefined {
  const suffix = did.trim().split(":").pop() ?? "";
  try {
    return normalizeAddress(suffix);
  } catch {
    return undefined;
  }
}

/** Parse a decimal ASTRA amount ("1.5") into wei, exactly. */
export function parseAstra(amount: string): bigint {
  const m = amount.trim().match(/^(\d+)(?:\.(\d{0,18}))?$/);
  if (!m) throw new Error(`not an ASTRA amount: ${amount}`);
  return BigInt(m[1]) * WEI_PER_ASTRA + BigInt((m[2] ?? "").padEnd(18, "0") || "0");
}

/** Format wei as a decimal ASTRA string. */
export function formatAstra(wei: bigint): string {
  const whole = wei / WEI_PER_ASTRA;
  const frac = (wei % WEI_PER_ASTRA).toString().padStart(18, "0").replace(/0+$/, "");
  return frac ? `${whole}.${frac}` : whole.toString();
}

export interface UnsignedTransaction {
  from: string;
  /** Contract or recipient; `undefined` deploys `data` as a contract. */
  to?: string;
  data: Uint8Array;
  value: bigint;
  nonce: bigint;
  gasLimit: bigint;
  gasPrice: bigint;
}

/** The node's signing payload (`transaction_signing_payload`, v2). */
export function transactionSigningPayload(chainId: bigint, tx: UnsignedTransaction): string {
  return [
    "SPACEKIT-TX-v2",
    chainId.toString(),
    tx.from.replace(/^0x/i, "").toLowerCase(),
    tx.to ? tx.to.replace(/^0x/i, "").toLowerCase() : "",
    tx.value.toString(),
    tx.nonce.toString(),
    tx.gasLimit.toString(),
    tx.gasPrice.toString(),
    hexOf(tx.data),
  ].join("\n");
}

export interface TransactionSignature {
  v: number;
  r_hex: string;
  s_hex: string;
}

/** A k256 key on the chain. */
export class ChainAccount {
  readonly address: string;
  readonly did: string;

  private constructor(private readonly privateKey: Uint8Array) {
    const pub = secp256k1.getPublicKey(privateKey, false);
    this.address = `0x${hexOf(keccak_256(pub.subarray(1)).subarray(12))}`;
    this.did = `did:spacekit:${this.address.slice(2)}`;
  }

  static fromPrivateKey(privateKeyHex: string): ChainAccount {
    const key = bytesOf(privateKeyHex);
    if (key.length !== 32) throw new Error("private key must be 32 bytes");
    return new ChainAccount(key);
  }

  static generate(): ChainAccount {
    return new ChainAccount(secp256k1.utils.randomPrivateKey());
  }

  privateKeyHex(): string {
    return hexOf(this.privateKey);
  }

  sign(chainId: bigint, tx: UnsignedTransaction): TransactionSignature {
    if (normalizeAddress(tx.from) !== this.address) {
      throw new Error("transaction is not from this account");
    }
    const hash = sha256(new TextEncoder().encode(transactionSigningPayload(chainId, tx)));
    const sig = secp256k1.sign(hash, this.privateKey);
    const bytes = sig.toCompactRawBytes();
    return { v: sig.recovery + 27, r_hex: hexOf(bytes.subarray(0, 32)), s_hex: hexOf(bytes.subarray(32)) };
  }
}

export interface ChainBalance {
  address: string;
  balanceWei: bigint;
  /** Rewards locked during proof of authority, not yet released. */
  lockedWei: bigint;
  nonce: bigint;
}

export interface ChainReceipt {
  tx_hash: string;
  block_number: number;
  success: boolean;
  gas_used: number;
  return_data: number[];
  created_address?: unknown;
}

export class ChainError extends Error {
  constructor(message: string, readonly status?: number) {
    super(message);
    this.name = "ChainError";
  }
}

export interface ChainClientOptions {
  /** Base URL of a compute node, e.g. `http://localhost:8080`. */
  url: string;
  fetch?: typeof fetch;
  /** Receipt polling interval (default 500 ms). */
  pollMs?: number;
}

export interface SendOptions {
  value?: bigint;
  gasLimit?: bigint;
  gasPrice?: bigint;
  /** How long to wait for the receipt (default 60 s). */
  timeoutMs?: number;
}

export class ChainClient {
  readonly url: string;
  private readonly fetchFn: typeof fetch;
  private readonly pollMs: number;
  private chainIdCache?: bigint;

  constructor(options: ChainClientOptions) {
    this.url = options.url.replace(/\/$/, "");
    this.fetchFn = options.fetch ?? ((...a) => fetch(...a));
    this.pollMs = options.pollMs ?? 500;
  }

  private async json<T>(path: string, init?: RequestInit): Promise<T> {
    const res = await this.fetchFn(`${this.url}${path}`, init);
    const text = await res.text();
    if (!res.ok) throw new ChainError(`${init?.method ?? "GET"} ${path}: ${res.status} ${text}`, res.status);
    return JSON.parse(text) as T;
  }

  /** The numeric chain id transactions are signed for (`eth_chainId`). */
  async chainId(): Promise<bigint> {
    if (this.chainIdCache === undefined) {
      const res = await this.json<{ result?: string; error?: unknown }>("/rpc", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_chainId", params: [] }),
      });
      if (!res.result) throw new ChainError("node did not report a chain id");
      this.chainIdCache = BigInt(res.result);
    }
    return this.chainIdCache;
  }

  async getBalance(addressOrDid: string): Promise<ChainBalance> {
    const address = addressOrDid.startsWith("did:")
      ? addressOfDid(addressOrDid)
      : normalizeAddress(addressOrDid);
    if (!address) throw new ChainError(`${addressOrDid} has no chain address`);
    const res = await this.json<{ address: string; balance_wei: string; locked_wei: string; nonce: number }>(
      `/v1/balance/${address}`,
    );
    return {
      address: res.address,
      balanceWei: BigInt(res.balance_wei),
      lockedWei: BigInt(res.locked_wei),
      nonce: BigInt(res.nonce),
    };
  }

  /** Read-only contract call: returns the contract's raw output. */
  async view(contract: string, data: Uint8Array): Promise<Uint8Array> {
    const res = await this.fetchFn(`${this.url}/api/contracts/${normalizeAddress(contract)}/call`, {
      method: "POST",
      headers: { "content-type": "application/octet-stream" },
      body: data.slice().buffer as ArrayBuffer,
    });
    if (!res.ok) throw new ChainError(`view call failed: ${res.status} ${await res.text()}`, res.status);
    return new Uint8Array(await res.arrayBuffer());
  }

  /** Submit a signed transaction; resolves to its hash (`0x…`). */
  async submit(tx: UnsignedTransaction, signature: TransactionSignature): Promise<string> {
    const body = {
      from: normalizeAddress(tx.from),
      to: tx.to ? normalizeAddress(tx.to) : undefined,
      data_hex: hexOf(tx.data),
      gas_limit: tx.gasLimit.toString(),
      gas_price: tx.gasPrice.toString(),
      value: tx.value.toString(),
      nonce: Number(tx.nonce),
      signature,
    };
    const res = await this.json<{ tx_hash: string }>("/transaction", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    return res.tx_hash;
  }

  async receipt(txHash: string): Promise<ChainReceipt | undefined> {
    const res = await this.fetchFn(`${this.url}/receipt/${txHash.replace(/^0x/, "")}`);
    if (res.status === 404) return undefined;
    if (!res.ok) throw new ChainError(`receipt: ${res.status}`, res.status);
    return (await res.json()) as ChainReceipt;
  }

  async waitForReceipt(txHash: string, timeoutMs = 60_000): Promise<ChainReceipt> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const r = await this.receipt(txHash);
      if (r) return r;
      if (Date.now() > deadline) throw new ChainError(`no receipt for ${txHash} within ${timeoutMs} ms`);
      await new Promise((ok) => setTimeout(ok, this.pollMs));
    }
  }

  /**
   * Sign, submit and wait. Throws if the transaction failed (its value was
   * returned; only gas was charged). Resolves to the receipt.
   */
  async send(
    account: ChainAccount,
    to: string | undefined,
    data: Uint8Array,
    options: SendOptions = {},
  ): Promise<ChainReceipt> {
    const [chainId, { nonce }] = await Promise.all([this.chainId(), this.getBalance(account.address)]);
    const tx: UnsignedTransaction = {
      from: account.address,
      to,
      data,
      value: options.value ?? 0n,
      nonce,
      gasLimit: options.gasLimit ?? (data.length === 0 && to ? PLAIN_TRANSFER_GAS : DEFAULT_GAS_LIMIT),
      gasPrice: options.gasPrice ?? 1n,
    };
    const hash = await this.submit(tx, account.sign(chainId, tx));
    const receipt = await this.waitForReceipt(hash, options.timeoutMs);
    if (!receipt.success) {
      throw new ChainError(
        `transaction ${hash} failed: ${new TextDecoder().decode(Uint8Array.from(receipt.return_data ?? []))}`,
      );
    }
    return receipt;
  }

  /** Send native ASTRA to an address. */
  transfer(account: ChainAccount, to: string, amountWei: bigint, options: SendOptions = {}): Promise<ChainReceipt> {
    return this.send(account, normalizeAddress(to), new Uint8Array(), { ...options, value: amountWei });
  }

  /** Submit a SPHINCS+-signed transfer (`{ body_json, signature_hex }`). */
  async submitPqTransfer(signed: { body_json: string; signature_hex: string }): Promise<string> {
    const res = await this.json<{ tx_hash: string }>("/v1/transfer", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(signed),
    });
    return res.tx_hash;
  }
}

/**
 * `SubmitContractCall` for a contract on the chain: submit with value, wait
 * for success, return the contract's output.
 */
export function chainContractCaller(
  client: ChainClient,
  account: ChainAccount,
  contract: string,
  options: Omit<SendOptions, "value"> = {},
): (input: Uint8Array, value: bigint) => Promise<Uint8Array> {
  return async (input, value) => {
    const receipt = await client.send(account, normalizeAddress(contract), input, { ...options, value });
    return Uint8Array.from(receipt.return_data ?? []);
  };
}
