/**
 * SpaceKit SDK - Token Adapters
 * 
 * Simple, high-level APIs for ERC-20 and ERC-721 tokens on SpaceKit.
 * 
 * @example
 * ```typescript
 * import { Erc20Token, Erc721Token } from '@spacekit/sdk/tokens';
 * 
 * // Deploy and use ERC-20 token
 * const token = await Erc20Token.deploy(vm, { name: "SpaceUSD", symbol: "SUSD" });
 * await token.mint("alice", 1000n);  // Short names auto-expand to DIDs
 * await token.transfer("alice", "bob", 500n);
 * const balance = await token.balanceOf("alice"); // 500n
 * 
 * // Deploy and use ERC-721 NFT
 * const nft = await Erc721Token.deploy(vm, { name: "SpaceNFT", symbol: "SNFT" });
 * const tokenId = await nft.mint("alice", "ipfs://metadata/1");
 * await nft.transfer("alice", "bob", tokenId);
 * const owner = await nft.ownerOf(tokenId); // "did:spacekit:demo:bob"
 * ```
 */

import type { SpacekitVm } from '@spacekit/spacekit-js';
import { NetworkError, VmError } from './errors';

/* ───────────────────────── Constants ───────────────────────── */

const ERC20_OPS = {
  mint: 1,
  transfer: 2,
  balance: 3,
  totalSupply: 4,
  metadata: 5,
} as const;

const ERC721_OPS = {
  mint: 1,
  transfer: 2,
  ownerOf: 3,
  tokenUri: 4,
  totalSupply: 5,
  metadata: 6,
} as const;

const DEFAULT_ERC20_WASM = '/wasm/astra_erc20.wasm';
const DEFAULT_ERC721_WASM = '/wasm/astra_erc721.wasm';

/* ───────────────────────── Helpers ───────────────────────── */

function concat(parts: Uint8Array[]): Uint8Array {
  const total = parts.reduce((sum, p) => sum + p.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
}

function encodeU16(value: number): Uint8Array {
  const buffer = new ArrayBuffer(2);
  new DataView(buffer).setUint16(0, value, true);
  return new Uint8Array(buffer);
}

function encodeU64(value: bigint): Uint8Array {
  const buffer = new ArrayBuffer(8);
  new DataView(buffer).setBigUint64(0, value, true);
  return new Uint8Array(buffer);
}

function encodeString(value: string): Uint8Array {
  const data = new TextEncoder().encode(value);
  return concat([encodeU16(data.length), data]);
}

function decodeU64(bytes: Uint8Array): bigint {
  if (bytes.length < 8) return 0n;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return view.getBigUint64(0, true);
}

function decodeString(bytes: Uint8Array, offset: number): { value: string; next: number } {
  if (offset + 2 > bytes.length) return { value: '', next: offset };
  const view = new DataView(bytes.buffer, bytes.byteOffset + offset, bytes.byteLength - offset);
  const len = view.getUint16(0, true);
  const start = offset + 2;
  const end = start + len;
  if (end > bytes.length) return { value: '', next: offset };
  const slice = bytes.slice(start, end);
  const value = new TextDecoder().decode(slice);
  return { value, next: end };
}

/**
 * DID network prefix for auto-expanding short names
 * Can be: 'demo', 'testnet', 'mainnet', or a custom string
 */
export type DidNetwork = 'demo' | 'testnet' | 'mainnet' | string;

/** Global default network for DID expansion */
let defaultNetwork: DidNetwork = 'demo';

/**
 * Set the default network for DID expansion
 * @example
 * setDefaultNetwork('testnet');
 * normalizeDid("alice") => "did:spacekit:testnet:alice"
 */
export function setDefaultNetwork(network: DidNetwork): void {
  defaultNetwork = network;
}

/**
 * Get the current default network
 */
export function getDefaultNetwork(): DidNetwork {
  return defaultNetwork;
}

/**
 * Normalize a DID - accepts short names or full DIDs
 * @param didOrName Short name (e.g., "alice") or full DID
 * @param network Optional network override (defaults to global setting)
 * @example
 * normalizeDid("alice") => "did:spacekit:demo:alice"
 * normalizeDid("alice", "testnet") => "did:spacekit:testnet:alice"
 * normalizeDid("did:spacekit:testnet:bob") => "did:spacekit:testnet:bob"
 */
export function normalizeDid(didOrName: string, network?: DidNetwork): string {
  if (didOrName.startsWith('did:')) {
    return didOrName;
  }
  const slug = didOrName.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '');
  return `did:spacekit:${network || defaultNetwork}:${slug}`;
}

/* ───────────────────────── Types ───────────────────────── */

export interface Erc20Config {
  /** Token name (e.g., "SpaceUSD") */
  name?: string;
  /** Token symbol (e.g., "SUSD") */
  symbol?: string;
  /** Contract ID for the deployed contract */
  contractId?: string;
  /** URL to the WASM file (defaults to built-in) */
  wasmUrl?: string;
  /** Network for DID expansion (defaults to global setting) */
  network?: DidNetwork;
}

export interface Erc20Metadata {
  version: number;
  name: string;
  symbol: string;
  decimals: number;
}

export interface Erc721Config {
  /** NFT collection name */
  name?: string;
  /** NFT collection symbol */
  symbol?: string;
  /** Contract ID for the deployed contract */
  contractId?: string;
  /** URL to the WASM file (defaults to built-in) */
  wasmUrl?: string;
  /** Network for DID expansion (defaults to global setting) */
  network?: DidNetwork;
}

export interface Erc721Metadata {
  version: number;
  name: string;
  symbol: string;
}

export interface NftInfo {
  tokenId: bigint;
  owner: string;
  uri: string;
}

/* ───────────────────────── ERC-20 Token ───────────────────────── */

/**
 * High-level ERC-20 token adapter for SpaceKit
 * 
 * @example
 * ```typescript
 * // Deploy a new token
 * const token = await Erc20Token.deploy(vm, { name: "SpaceUSD", symbol: "SUSD" });
 * 
 * // Or connect to existing contract
 * const token = new Erc20Token(vm, "my-erc20-contract");
 * 
 * // Use the token
 * await token.mint("alice", 1000n);
 * await token.transfer("alice", "bob", 500n);
 * console.log(token.balanceOf("alice")); // 500n
 * ```
 */
export class Erc20Token {
  private vm: SpacekitVm;
  private _contractId: string;
  private callerDid: string;
  private network?: DidNetwork;

  constructor(vm: SpacekitVm, contractId: string, callerDid?: string, network?: DidNetwork) {
    this.vm = vm;
    this._contractId = contractId;
    this.network = network;
    this.callerDid = callerDid || normalizeDid('alice', network);
  }

  /** Contract ID for this token */
  get contractId(): string {
    return this._contractId;
  }

  /**
   * Deploy a new ERC-20 token contract
   */
  static async deploy(
    vm: SpacekitVm,
    config: Erc20Config = {},
    callerDid?: string
  ): Promise<Erc20Token> {
    const wasmUrl = config.wasmUrl || DEFAULT_ERC20_WASM;
    const contractId = config.contractId || `erc20-${Date.now()}`;
    
    let response: Response;
    try {
      response = await fetch(wasmUrl);
    } catch (e) {
      throw NetworkError.connectionFailed(wasmUrl, e instanceof Error ? e.message : undefined);
    }
    
    if (!response.ok) {
      throw NetworkError.httpError(wasmUrl, response.status, response.statusText);
    }
    
    const contract = await vm.deployContract(response, contractId);
    return new Erc20Token(vm, contract.id, callerDid, config.network);
  }

  /**
   * Set the caller DID for transactions
   */
  setCaller(did: string): this {
    this.callerDid = normalizeDid(did, this.network);
    return this;
  }

  /**
   * Set the network for DID expansion
   */
  setNetwork(network: DidNetwork): this {
    this.network = network;
    return this;
  }

  /**
   * Mint tokens to an address
   * @param to Recipient DID or short name (e.g., "alice")
   * @param amount Amount to mint
   */
  async mint(to: string, amount: bigint): Promise<boolean> {
    const input = concat([
      Uint8Array.of(ERC20_OPS.mint),
      encodeString(normalizeDid(to, this.network)),
      encodeU64(amount),
    ]);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    return receipt.status === 0;
  }

  /**
   * Transfer tokens between addresses
   * @param from Sender DID or short name
   * @param to Recipient DID or short name
   * @param amount Amount to transfer
   */
  async transfer(from: string, to: string, amount: bigint): Promise<boolean> {
    const input = concat([
      Uint8Array.of(ERC20_OPS.transfer),
      encodeString(normalizeDid(from, this.network)),
      encodeString(normalizeDid(to, this.network)),
      encodeU64(amount),
    ]);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    return receipt.status === 0;
  }

  /**
   * Get balance of an address
   * @param did Address DID or short name
   */
  async balanceOf(did: string): Promise<bigint> {
    const input = concat([
      Uint8Array.of(ERC20_OPS.balance),
      encodeString(normalizeDid(did, this.network)),
    ]);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    if (receipt.status !== 0 || receipt.result.length < 8) return 0n;
    return decodeU64(receipt.result);
  }

  /**
   * Get total supply of the token
   */
  async totalSupply(): Promise<bigint> {
    const input = Uint8Array.of(ERC20_OPS.totalSupply);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    if (receipt.status !== 0 || receipt.result.length < 8) return 0n;
    return decodeU64(receipt.result);
  }

  /**
   * Get token metadata
   */
  async metadata(): Promise<Erc20Metadata> {
    const input = Uint8Array.of(ERC20_OPS.metadata);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    if (receipt.status !== 0 || receipt.result.length < 1) {
      return { version: 0, name: '', symbol: '', decimals: 0 };
    }
    const result = receipt.result;
    let offset = 0;
    const version = result[offset];
    offset += 1;
    const nameDecoded = decodeString(result, offset);
    offset = nameDecoded.next;
    const symbolDecoded = decodeString(result, offset);
    offset = symbolDecoded.next;
    const decimals = result[offset] ?? 0;
    return {
      version,
      name: nameDecoded.value,
      symbol: symbolDecoded.value,
      decimals,
    };
  }
}

/* ───────────────────────── ERC-721 NFT ───────────────────────── */

/**
 * High-level ERC-721 NFT adapter for SpaceKit
 * 
 * @example
 * ```typescript
 * // Deploy a new NFT collection
 * const nft = await Erc721Token.deploy(vm, { name: "SpaceNFT", symbol: "SNFT" });
 * 
 * // Or connect to existing contract
 * const nft = new Erc721Token(vm, "my-nft-contract");
 * 
 * // Use the NFT
 * const tokenId = await nft.mint("alice", "ipfs://metadata/1");
 * await nft.transfer("alice", "bob", tokenId);
 * console.log(nft.ownerOf(tokenId)); // "did:spacekit:demo:bob"
 * ```
 */
export class Erc721Token {
  private vm: SpacekitVm;
  private _contractId: string;
  private callerDid: string;
  private network?: DidNetwork;

  constructor(vm: SpacekitVm, contractId: string, callerDid?: string, network?: DidNetwork) {
    this.vm = vm;
    this._contractId = contractId;
    this.network = network;
    this.callerDid = callerDid || normalizeDid('alice', network);
  }

  /** Contract ID for this NFT collection */
  get contractId(): string {
    return this._contractId;
  }

  /**
   * Deploy a new ERC-721 NFT contract
   */
  static async deploy(
    vm: SpacekitVm,
    config: Erc721Config = {},
    callerDid?: string
  ): Promise<Erc721Token> {
    const wasmUrl = config.wasmUrl || DEFAULT_ERC721_WASM;
    const contractId = config.contractId || `erc721-${Date.now()}`;
    
    let response: Response;
    try {
      response = await fetch(wasmUrl);
    } catch (e) {
      throw NetworkError.connectionFailed(wasmUrl, e instanceof Error ? e.message : undefined);
    }
    
    if (!response.ok) {
      throw NetworkError.httpError(wasmUrl, response.status, response.statusText);
    }
    
    const contract = await vm.deployContract(response, contractId);
    return new Erc721Token(vm, contract.id, callerDid, config.network);
  }

  /**
   * Set the caller DID for transactions
   */
  setCaller(did: string): this {
    this.callerDid = normalizeDid(did, this.network);
    return this;
  }

  /**
   * Set the network for DID expansion
   */
  setNetwork(network: DidNetwork): this {
    this.network = network;
    return this;
  }

  /**
   * Mint a new NFT
   * @param to Recipient DID or short name
   * @param uri Token metadata URI (e.g., "ipfs://...")
   * @returns The minted token ID
   */
  async mint(to: string, uri: string): Promise<bigint> {
    const input = concat([
      Uint8Array.of(ERC721_OPS.mint),
      encodeString(normalizeDid(to, this.network)),
      encodeString(uri),
    ]);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    if (receipt.status !== 0 || receipt.result.length < 8) {
      throw new Error('NFT mint failed');
    }
    return decodeU64(receipt.result);
  }

  /**
   * Transfer an NFT between addresses
   * @param from Current owner DID or short name
   * @param to New owner DID or short name
   * @param tokenId Token ID to transfer
   */
  async transfer(from: string, to: string, tokenId: bigint): Promise<boolean> {
    const input = concat([
      Uint8Array.of(ERC721_OPS.transfer),
      encodeString(normalizeDid(from, this.network)),
      encodeString(normalizeDid(to, this.network)),
      encodeU64(tokenId),
    ]);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    return receipt.status === 0;
  }

  /**
   * Get the owner of an NFT
   * @param tokenId Token ID
   */
  async ownerOf(tokenId: bigint): Promise<string> {
    const input = concat([
      Uint8Array.of(ERC721_OPS.ownerOf),
      encodeU64(tokenId),
    ]);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    if (receipt.status !== 0 || receipt.result.length < 2) return '';
    const { value } = decodeString(receipt.result, 0);
    return value;
  }

  /**
   * Get the URI of an NFT
   * @param tokenId Token ID
   */
  async tokenUri(tokenId: bigint): Promise<string> {
    const input = concat([
      Uint8Array.of(ERC721_OPS.tokenUri),
      encodeU64(tokenId),
    ]);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    if (receipt.status !== 0 || receipt.result.length < 2) return '';
    const { value } = decodeString(receipt.result, 0);
    return value;
  }

  /**
   * Get total supply (number of minted NFTs)
   */
  async totalSupply(): Promise<bigint> {
    const input = Uint8Array.of(ERC721_OPS.totalSupply);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    if (receipt.status !== 0 || receipt.result.length < 8) return 0n;
    return decodeU64(receipt.result);
  }

  /**
   * Get NFT collection metadata
   */
  async metadata(): Promise<Erc721Metadata> {
    const input = Uint8Array.of(ERC721_OPS.metadata);
    const receipt = await this.vm.executeTransaction(
      this._contractId,
      input,
      this.callerDid,
      0n
    );
    if (receipt.status !== 0 || receipt.result.length < 1) {
      return { version: 0, name: '', symbol: '' };
    }
    const result = receipt.result;
    let offset = 0;
    const version = result[offset];
    offset += 1;
    const nameDecoded = decodeString(result, offset);
    offset = nameDecoded.next;
    const symbolDecoded = decodeString(result, offset);
    return {
      version,
      name: nameDecoded.value,
      symbol: symbolDecoded.value,
    };
  }

  /**
   * Get info about an NFT
   * @param tokenId Token ID
   */
  async getInfo(tokenId: bigint): Promise<NftInfo> {
    const [owner, uri] = await Promise.all([
      this.ownerOf(tokenId),
      this.tokenUri(tokenId),
    ]);
    return { tokenId, owner, uri };
  }
}
