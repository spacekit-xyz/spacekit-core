/**
 * SpacekitClient - Full SDK for SpaceKit-JS
 *
 * Provides a standard API for:
 * - Identity (DID) management with reactive updates
 * - Native ASTRA balance tracking
 * - Block/transaction snapshot persistence
 * - Kyber encryption key persistence
 * - Cross-iframe synchronization via BroadcastChannel
 * - Event system for state changes
 */

/* ───────────────────────── Types ───────────────────────── */

export interface PersistedTransaction {
  id: string;
  contractId: string;
  callerDid: string;
  input: number[];
  value: string;
  timestamp: number;
  nonce?: number;
}

export interface PersistedReceipt {
  txId: string;
  contractId: string;
  status: number;
  result: number[];
  events: Array<{ type: string; data: number[] }>;
  timestamp: number;
  gasUsed?: number;
  receiptHash: string;
}

export interface PersistedBlock {
  height: number;
  prevHash: string;
  blockHash: string;
  stateRoot: string;
  txRoot: string;
  receiptRoot: string;
  timestamp: number;
  transactions: PersistedTransaction[];
  receipts: PersistedReceipt[];
}

export interface ExplorerSnapshot {
  blocks: PersistedBlock[];
  updatedAt: number;
}

export interface KyberKeyPair {
  publicKey: string;
  secretKey: string;  // Kyber uses secretKey, not privateKey
  algorithm?: string;
  keyId: string;
  createdAt: number;
}

export interface SyncMessage {
  type: "identity" | "balance" | "block" | "tx" | "refresh" | "keys";
  did?: string;
  balance?: string;
  block?: PersistedBlock;
  timestamp: number;
}

export type ClientEventType = 
  | "identity-change" 
  | "balance-change" 
  | "block-added" 
  | "keys-change" 
  | "sync";

export interface ClientEvent {
  type: ClientEventType;
  did: string;
  data?: unknown;
}

/* ───────────────────────── Constants ───────────────────────── */

const CHANNEL_NAME = "spacekit:client:sync";
const STORAGE_KEYS = {
  identityDid: "spacekit:identityDid",
  identityName: "spacekit:identityName",
  explorerPrefix: "spacekit:playground:explorer:",
  balancePrefix: "spacekit:playground:nativeBalance:",
  kyberKeysPrefix: "spacekit:kyber:keys:", // Global keys, not per-DID
};

const DEFAULT_BALANCES: Record<string, bigint> = {
  alice: 5_000_000n,
  bob: 2_000_000n,
  treasury: 10_000_000n,
};

const DEFAULT_NEW_USER_BALANCE = 1_000_000n;

/* ───────────────────────── Client Class ───────────────────────── */

class SpacekitClientImpl {
  private static readonly MAX_EXPLORER_BLOCKS = 200;
  private static readonly MAX_EXPLORER_BYTES = 4096;
  private channel: BroadcastChannel | null = null;
  private syncListeners: Set<(msg: SyncMessage) => void> = new Set();
  private eventListeners: Map<ClientEventType, Set<(event: ClientEvent) => void>> = new Map();
  private initialized = false;
  private currentDid: string | null = null;

  /* ── Initialization ── */

  init(): void {
    if (this.initialized) return;
    this.initialized = true;

    if (typeof BroadcastChannel !== "undefined") {
      this.channel = new BroadcastChannel(CHANNEL_NAME);
      this.channel.onmessage = (event: MessageEvent<SyncMessage>) => {
        this.syncListeners.forEach((listener) => listener(event.data));
        this.handleSyncMessage(event.data);
      };
    }

    // Load current identity from storage
    this.currentDid = localStorage.getItem(STORAGE_KEYS.identityDid);
    if (!this.currentDid) {
      this.setIdentity("Alice");
    } else {
      this.ensureBalance(this.currentDid);
    }
  }

  private handleSyncMessage(msg: SyncMessage): void {
    switch (msg.type) {
      case "identity":
        if (msg.did) {
          this.currentDid = msg.did;
          this.emit("identity-change", msg.did);
        }
        break;
      case "balance":
        if (msg.did) {
          this.emit("balance-change", msg.did, { balance: msg.balance });
        }
        break;
      case "block":
        if (msg.did && msg.block) {
          this.emit("block-added", msg.did, { block: msg.block });
        }
        break;
      case "keys":
        if (msg.did) {
          this.emit("keys-change", msg.did);
        }
        break;
      case "refresh":
        this.emit("sync", msg.did ?? this.currentDid ?? "");
        break;
    }
  }

  destroy(): void {
    if (this.channel) {
      this.channel.close();
      this.channel = null;
    }
    this.syncListeners.clear();
    this.eventListeners.clear();
    this.initialized = false;
    this.currentDid = null;
  }

  /* ── Event System ── */

  on(event: ClientEventType, callback: (event: ClientEvent) => void): () => void {
    if (!this.eventListeners.has(event)) {
      this.eventListeners.set(event, new Set());
    }
    this.eventListeners.get(event)!.add(callback);
    return () => this.eventListeners.get(event)?.delete(callback);
  }

  private emit(type: ClientEventType, did: string, data?: unknown): void {
    const listeners = this.eventListeners.get(type);
    if (listeners) {
      const event: ClientEvent = { type, did, data };
      listeners.forEach((callback) => callback(event));
    }
  }

  /* ── Broadcast ── */

  private broadcast(msg: SyncMessage): void {
    if (this.channel) {
      this.channel.postMessage(msg);
    }
  }

  subscribe(callback: (msg: SyncMessage) => void): () => void {
    this.syncListeners.add(callback);
    return () => this.syncListeners.delete(callback);
  }

  /* ── Identity ── */

  private toSlug(name: string): string {
    return name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "_")
      .replace(/^_+|_+$/g, "") || "user";
  }

  getCurrentDid(): string | null {
    // Always refresh from storage in case another tab changed it
    this.currentDid = localStorage.getItem(STORAGE_KEYS.identityDid);
    return this.currentDid;
  }

  getIdentityDid(): string | null {
    return this.getCurrentDid();
  }

  getIdentityName(): string | null {
    return localStorage.getItem(STORAGE_KEYS.identityName);
  }

  setIdentity(name: string): string {
    const slug = this.toSlug(name);
    const did = `did:spacekit:demo:${slug}`;
    localStorage.setItem(STORAGE_KEYS.identityDid, did);
    localStorage.setItem(STORAGE_KEYS.identityName, name);
    this.currentDid = did;
    this.ensureBalance(did);
    this.broadcast({ type: "identity", did, timestamp: Date.now() });
    this.emit("identity-change", did);
    return did;
  }

  /**
   * Get DID for operations - uses provided DID or falls back to current
   */
  resolveDid(did?: string): string {
    const resolved = did || this.getCurrentDid();
    if (!resolved) {
      throw new Error("No DID available. Call setIdentity first.");
    }
    return resolved;
  }

  /**
   * Get the current DID, guaranteeing a non-null value.
   * If no identity is set, returns a default demo DID.
   * This is the recommended way to get the DID in components.
   */
  requireDid(): string {
    const did = this.getCurrentDid();
    if (did) return did;
    // If no identity, set default and return it
    return this.setIdentity("Alice");
  }

  /**
   * Generate a DID-scoped database name for IndexedDB isolation.
   * This ensures each user has their own isolated database.
   * 
   * @param prefix - The storage prefix (e.g., "video", "files", "llm-chat")
   * @param did - Optional DID override, defaults to current identity
   * @returns A normalized database name like "spacekitvm-video-did-spacekit-demo-alice"
   * 
   * @example
   * const dbName = SpacekitClient.getScopedDbName("video");
   * // Returns: "spacekitvm-video-did-spacekit-demo-alice"
   */
  getScopedDbName(prefix: string, did?: string): string {
    const resolvedDid = did || this.requireDid();
    const normalizedDid = resolvedDid
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "");
    return `spacekitvm-${prefix}-${normalizedDid}`;
  }

  /**
   * Generate a DID-scoped collection name for remote storage.
   * 
   * @param prefix - The collection prefix (e.g., "spacekitvm")
   * @param did - Optional DID override, defaults to current identity
   * @returns A collection name like "spacekitvm:did:spacekit:demo:alice"
   */
  getScopedCollection(prefix: string, did?: string): string {
    const resolvedDid = did || this.requireDid();
    return `${prefix}:${resolvedDid}`;
  }

  /**
   * Check if the provided DID matches the current identity.
   * Useful for detecting identity changes in components.
   */
  isCurrentIdentity(did: string): boolean {
    return did === this.getCurrentDid();
  }

  /* ── Balance ── */

  private getBalanceKey(did: string): string {
    return `${STORAGE_KEYS.balancePrefix}${did}`;
  }

  private getDefaultBalance(did: string): bigint {
    const suffix = did.split(":").pop() ?? "";
    return DEFAULT_BALANCES[suffix] ?? DEFAULT_NEW_USER_BALANCE;
  }

  ensureBalance(did?: string): bigint {
    const resolvedDid = this.resolveDid(did);
    const key = this.getBalanceKey(resolvedDid);
    const raw = localStorage.getItem(key);
    if (raw !== null) {
      try {
        const value = BigInt(raw);
        if (value > 0n) {
          return value;
        }
      } catch (e) {
        // Invalid stored balance, will seed below
        if (typeof console !== 'undefined' && console.debug) {
          console.debug('[SpacekitClient] Invalid stored balance, reseeding:', e);
        }
      }
    }
    const seed = this.getDefaultBalance(resolvedDid);
    if (seed > 0n) {
      localStorage.setItem(key, seed.toString());
      this.broadcast({ type: "balance", did: resolvedDid, balance: seed.toString(), timestamp: Date.now() });
    }
    return seed;
  }

  getBalance(did?: string): bigint {
    const resolvedDid = this.resolveDid(did);
    const key = this.getBalanceKey(resolvedDid);
    const raw = localStorage.getItem(key);
    if (raw !== null) {
      try {
        const value = BigInt(raw);
        if (value === 0n) {
          return this.ensureBalance(resolvedDid);
        }
        return value;
      } catch (e) {
        // Invalid stored balance for shared pool
        if (typeof console !== 'undefined' && console.debug) {
          console.debug('[SpacekitClient] Invalid shared balance:', e);
        }
      }
    }
    return this.ensureBalance(resolvedDid);
  }

  setBalance(did: string | undefined, balance: bigint): void {
    const resolvedDid = this.resolveDid(did);
    const key = this.getBalanceKey(resolvedDid);
    localStorage.setItem(key, balance.toString());
    this.broadcast({ type: "balance", did: resolvedDid, balance: balance.toString(), timestamp: Date.now() });
    this.emit("balance-change", resolvedDid, { balance: balance.toString() });
  }

  deductFee(did: string | undefined, fee: bigint | number): bigint {
    const resolvedDid = this.resolveDid(did);
    const current = this.getBalance(resolvedDid);
    const feeAmount = typeof fee === "bigint" ? fee : BigInt(Math.max(0, Math.floor(fee)));
    const next = current > feeAmount ? current - feeAmount : 0n;
    this.setBalance(resolvedDid, next);
    return next;
  }

  /* ── Kyber Keys (Global, not per-DID) ── */

  private getKyberKeysKey(): string {
    // Keys are global, stored once for all DIDs
    return `${STORAGE_KEYS.kyberKeysPrefix}global`;
  }

  getKyberKeys(): KyberKeyPair | null {
    const key = this.getKyberKeysKey();
    const raw = localStorage.getItem(key);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as KyberKeyPair;
    } catch (e) {
      // Corrupted stored keys
      if (typeof console !== 'undefined' && console.warn) {
        console.warn('[SpacekitClient] Invalid stored Kyber keys, clearing:', e);
      }
      localStorage.removeItem(key);
      return null;
    }
  }

  setKyberKeys(keys: KyberKeyPair): void {
    const key = this.getKyberKeysKey();
    localStorage.setItem(key, JSON.stringify(keys));
    this.broadcast({ type: "keys", did: this.currentDid ?? "", timestamp: Date.now() });
    this.emit("keys-change", this.currentDid ?? "");
  }

  hasKyberKeys(): boolean {
    return this.getKyberKeys() !== null;
  }

  /* ── Explorer Snapshot ── */

  private getExplorerKey(did: string): string {
    return `${STORAGE_KEYS.explorerPrefix}${did || "default"}`;
  }

  getExplorerSnapshot(did?: string): ExplorerSnapshot | null {
    const resolvedDid = this.resolveDid(did);
    const key = this.getExplorerKey(resolvedDid);
    const raw = localStorage.getItem(key);
    if (!raw) return null;
    try {
      return JSON.parse(raw) as ExplorerSnapshot;
    } catch (e) {
      // Corrupted explorer data
      if (typeof console !== 'undefined' && console.warn) {
        console.warn('[SpacekitClient] Invalid stored explorer snapshot, clearing:', e);
      }
      localStorage.removeItem(key);
      return null;
    }
  }

  setExplorerSnapshot(did: string | undefined, snapshot: ExplorerSnapshot): void {
    const resolvedDid = this.resolveDid(did);
    const key = this.getExplorerKey(resolvedDid);
    localStorage.setItem(key, JSON.stringify(snapshot));
  }

  getNextExplorerHeight(did?: string): { nextHeight: number; prevHash: string; latestBlock: PersistedBlock | null } {
    const resolvedDid = this.resolveDid(did);
    const snapshot = this.getExplorerSnapshot(resolvedDid);
    const latestBlock = snapshot?.blocks?.[0] ?? null;
    const key = `spacekit:explorer:height:${resolvedDid}`;
    const stored = Number(localStorage.getItem(key) || 0);
    const base = Math.max(latestBlock?.height ?? 0, stored);
    const nextHeight = base + 1;
    localStorage.setItem(key, String(nextHeight));
    const prevHash = latestBlock?.blockHash ?? (nextHeight === 1 ? "genesis" : `block_${nextHeight - 1}`);
    return { nextHeight, prevHash, latestBlock };
  }

  addBlock(did: string | undefined, block: PersistedBlock): void {
    const resolvedDid = this.resolveDid(did);
    const snapshot = this.getExplorerSnapshot(resolvedDid) ?? { blocks: [], updatedAt: 0 };
    let normalized = this.normalizeExplorerBlock(block);
    const latestBlock = snapshot.blocks[0] ?? null;
    const latestHeight = latestBlock?.height ?? 0;
    const existingByHash = snapshot.blocks.findIndex((b) => b.blockHash === normalized.blockHash);
    const existingByTx = snapshot.blocks.findIndex((b) =>
      b.transactions.some((tx) => normalized.transactions.some((n) => n.id === tx.id))
    );
    if (existingByHash >= 0) {
      snapshot.blocks[existingByHash] = normalized;
      snapshot.updatedAt = Date.now();
      this.persistExplorerSnapshot(resolvedDid, snapshot);
      this.broadcast({ type: "block", did: resolvedDid, block, timestamp: Date.now() });
      this.emit("block-added", resolvedDid, { block });
      return;
    }
    if (existingByTx >= 0) {
      snapshot.blocks[existingByTx] = normalized;
      snapshot.updatedAt = Date.now();
      this.persistExplorerSnapshot(resolvedDid, snapshot);
      this.broadcast({ type: "block", did: resolvedDid, block, timestamp: Date.now() });
      this.emit("block-added", resolvedDid, { block });
      return;
    }
    if (normalized.height <= latestHeight) {
      const nextHeight = latestHeight + 1;
      const prevHash = latestBlock?.blockHash ?? normalized.prevHash;
      const nonce = `${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
      normalized = {
        ...normalized,
        height: nextHeight,
        prevHash,
        blockHash: `block_${nextHeight}_${nonce}`,
      };
    }
    snapshot.blocks.push(normalized);
    snapshot.blocks.sort((a, b) => b.height - a.height);
    if (snapshot.blocks.length > SpacekitClientImpl.MAX_EXPLORER_BLOCKS) {
      snapshot.blocks = snapshot.blocks.slice(0, SpacekitClientImpl.MAX_EXPLORER_BLOCKS);
    }
    snapshot.updatedAt = Date.now();
    this.persistExplorerSnapshot(resolvedDid, snapshot);
    this.broadcast({ type: "block", did: resolvedDid, block, timestamp: Date.now() });
    this.emit("block-added", resolvedDid, { block });
  }

  private normalizeExplorerBlock(block: PersistedBlock): PersistedBlock {
    const limit = SpacekitClientImpl.MAX_EXPLORER_BYTES;
    return {
      ...block,
      transactions: block.transactions.map((tx) => ({
        ...tx,
        input: tx.input.length > limit ? tx.input.slice(0, limit) : tx.input,
      })),
      receipts: block.receipts.map((receipt) => ({
        ...receipt,
        result: receipt.result.length > limit ? receipt.result.slice(0, limit) : receipt.result,
        events: receipt.events.map((event) => ({
          ...event,
          data: event.data.length > limit ? event.data.slice(0, limit) : event.data,
        })),
      })),
    };
  }

  private persistExplorerSnapshot(resolvedDid: string, snapshot: ExplorerSnapshot): void {
    const key = this.getExplorerKey(resolvedDid);
    try {
      localStorage.setItem(key, JSON.stringify(snapshot));
    } catch (error) {
      if (!this.isQuotaExceeded(error)) {
        throw error;
      }
      // Aggressively trim and retry to recover from quota issues.
      const trimmed = {
        ...snapshot,
        blocks: snapshot.blocks.slice(0, Math.max(20, Math.floor(snapshot.blocks.length / 2))),
        updatedAt: Date.now(),
      };
      try {
        localStorage.setItem(key, JSON.stringify(trimmed));
      } catch (retryError) {
        if (!this.isQuotaExceeded(retryError)) {
          throw retryError;
        }
        // Last resort: keep only the latest 10 blocks.
        const minimal = {
          ...trimmed,
          blocks: trimmed.blocks.slice(0, 10),
          updatedAt: Date.now(),
        };
        localStorage.setItem(key, JSON.stringify(minimal));
      }
    }
  }

  private isQuotaExceeded(error: unknown): boolean {
    if (!error || typeof error !== "object") return false;
    const name = (error as { name?: string }).name;
    return name === "QuotaExceededError";
  }

  clearExplorer(did?: string): void {
    const resolvedDid = this.resolveDid(did);
    const key = this.getExplorerKey(resolvedDid);
    localStorage.removeItem(key);
    this.broadcast({ type: "refresh", did: resolvedDid, timestamp: Date.now() });
  }

  /* ── Block Serialization Helpers ── */

  serializeBlock(block: {
    height: number;
    prevHash: string;
    blockHash: string;
    stateRoot: string;
    txRoot: string;
    receiptRoot: string;
    timestamp: number;
    transactions: Array<{
      id: string;
      contractId: string;
      callerDid: string;
      input: Uint8Array;
      value: bigint;
      timestamp: number;
      nonce?: number;
    }>;
    receipts: Array<{
      txId: string;
      contractId: string;
      status: number;
      result: Uint8Array;
      events: Array<{ type: string; data: Uint8Array }>;
      timestamp: number;
      gasUsed?: number;
      receiptHash: string;
    }>;
  }): PersistedBlock {
    return {
      height: block.height,
      prevHash: block.prevHash,
      blockHash: block.blockHash,
      stateRoot: block.stateRoot,
      txRoot: block.txRoot,
      receiptRoot: block.receiptRoot,
      timestamp: block.timestamp,
      transactions: block.transactions.map((tx) => ({
        id: tx.id,
        contractId: tx.contractId,
        callerDid: tx.callerDid,
        input: Array.from(tx.input),
        value: tx.value.toString(),
        timestamp: tx.timestamp,
        nonce: tx.nonce,
      })),
      receipts: block.receipts.map((receipt) => ({
        txId: receipt.txId,
        contractId: receipt.contractId,
        status: receipt.status,
        result: Array.from(receipt.result),
        events: receipt.events.map((event) => ({
          type: event.type,
          data: Array.from(event.data),
        })),
        timestamp: receipt.timestamp,
        gasUsed: receipt.gasUsed,
        receiptHash: receipt.receiptHash,
      })),
    };
  }

  hydrateBlock(block: PersistedBlock): {
    height: number;
    prevHash: string;
    blockHash: string;
    stateRoot: string;
    txRoot: string;
    receiptRoot: string;
    timestamp: number;
    transactions: Array<{
      id: string;
      contractId: string;
      callerDid: string;
      input: Uint8Array;
      value: bigint;
      timestamp: number;
      nonce?: number;
    }>;
    receipts: Array<{
      txId: string;
      contractId: string;
      status: number;
      result: Uint8Array;
      events: Array<{ type: string; data: Uint8Array }>;
      timestamp: number;
      gasUsed?: number;
      receiptHash: string;
    }>;
  } {
    return {
      height: block.height,
      prevHash: block.prevHash,
      blockHash: block.blockHash,
      stateRoot: block.stateRoot,
      txRoot: block.txRoot,
      receiptRoot: block.receiptRoot,
      timestamp: block.timestamp,
      transactions: block.transactions.map((tx) => ({
        id: tx.id,
        contractId: tx.contractId,
        callerDid: tx.callerDid,
        input: new Uint8Array(tx.input),
        value: BigInt(tx.value),
        timestamp: tx.timestamp,
        nonce: tx.nonce,
      })),
      receipts: block.receipts.map((receipt) => ({
        txId: receipt.txId,
        contractId: receipt.contractId,
        status: receipt.status,
        result: new Uint8Array(receipt.result),
        events: receipt.events.map((event) => ({
          type: event.type,
          data: new Uint8Array(event.data),
        })),
        timestamp: receipt.timestamp,
        gasUsed: receipt.gasUsed,
        receiptHash: receipt.receiptHash,
      })),
    };
  }

  /* ── Refresh Signal ── */

  requestRefresh(did?: string): void {
    const resolvedDid = did || this.currentDid || "";
    this.broadcast({ type: "refresh", did: resolvedDid, timestamp: Date.now() });
  }

  /* ── Utility ── */

  isInitialized(): boolean {
    return this.initialized;
  }
}

/* ───────────────────────── Singleton Export ───────────────────────── */

export const SpacekitClient = new SpacekitClientImpl();
