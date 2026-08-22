/**
 * IndexedDB Block Store for SpacekitVm
 * 
 * Provides persistent storage for blockchain blocks with:
 * - Block storage by height (primary key)
 * - Block header index for fast queries
 * - Hash-to-height mapping for block lookup by hash
 * - LRU cache for in-memory access
 * - Automatic eviction of old blocks from memory (persisted to IndexedDB)
 */

import type { Block, BlockHeader } from "./spacekitvm.js";

/* ───────────────────────── Types ───────────────────────── */

export interface BlockStoreOptions {
  dbName?: string;
  maxBlocksInMemory?: number;
}

export interface BlockStoreStats {
  totalBlocks: number;
  inMemoryBlocks: number;
  persistedBlocks: number;
  latestHeight: number;
  oldestHeight: number;
  dbName: string;
}

/**
 * Serializable block format for IndexedDB storage
 */
interface SerializedBlock {
  height: number;
  prevHash: string;
  blockHash: string;
  stateRoot: string;
  quantumStateRoot?: string;
  txRoot: string;
  receiptRoot: string;
  timestamp: number;
  transactions: Array<{
    id: string;
    contractId: string;
    callerDid: string;
    inputHex: string; // Uint8Array serialized as hex
    value: string; // bigint as string
    timestamp: number;
  }>;
  receipts: Array<{
    txId: string;
    contractId: string;
    status: number;
    resultHex: string; // Uint8Array as hex
    events: Array<{ type: string; dataHex: string }>;
    timestamp: number;
    receiptHash: string;
    gasUsed?: number;
  }>;
  header: BlockHeader;
}

/* ───────────────────────── Helpers ───────────────────────── */

const hexTable = Array.from({ length: 256 }, (_, i) => i.toString(16).padStart(2, "0"));

function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) {
    out += hexTable[b];
  }
  return out;
}

function hexToBytes(hex: string): Uint8Array {
  const clean = hex.length % 2 === 0 ? hex : `0${hex}`;
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function serializeBlock(block: Block): SerializedBlock {
  return {
    height: block.height,
    prevHash: block.prevHash,
    blockHash: block.blockHash,
    stateRoot: block.stateRoot,
    quantumStateRoot: block.quantumStateRoot,
    txRoot: block.txRoot,
    receiptRoot: block.receiptRoot,
    timestamp: block.timestamp,
    transactions: block.transactions.map((tx) => ({
      id: tx.id,
      contractId: tx.contractId,
      callerDid: tx.callerDid,
      inputHex: bytesToHex(tx.input),
      value: tx.value.toString(),
      timestamp: tx.timestamp,
    })),
    receipts: block.receipts.map((r) => ({
      txId: r.txId,
      contractId: r.contractId,
      status: r.status,
      resultHex: bytesToHex(r.result),
      events: r.events.map((e) => ({ type: e.type, dataHex: bytesToHex(e.data) })),
      timestamp: r.timestamp,
      receiptHash: r.receiptHash,
      gasUsed: r.gasUsed,
    })),
    header: block.header,
  };
}

function deserializeBlock(data: SerializedBlock): Block {
  return {
    height: data.height,
    prevHash: data.prevHash,
    blockHash: data.blockHash,
    stateRoot: data.stateRoot,
    quantumStateRoot: data.quantumStateRoot,
    txRoot: data.txRoot,
    receiptRoot: data.receiptRoot,
    timestamp: data.timestamp,
    transactions: data.transactions.map((tx) => ({
      id: tx.id,
      contractId: tx.contractId,
      callerDid: tx.callerDid,
      input: hexToBytes(tx.inputHex),
      value: BigInt(tx.value),
      timestamp: tx.timestamp,
    })),
    receipts: data.receipts.map((r) => ({
      txId: r.txId,
      contractId: r.contractId,
      status: r.status,
      result: hexToBytes(r.resultHex),
      events: r.events.map((e) => ({ type: e.type, data: hexToBytes(e.dataHex) })),
      timestamp: r.timestamp,
      receiptHash: r.receiptHash,
      gasUsed: r.gasUsed,
    })),
    header: data.header,
  };
}

/* ───────────────────────── BlockStore ───────────────────────── */

/**
 * IndexedDB-backed block store with LRU memory cache.
 * 
 * Stores blocks persistently while keeping recent blocks in memory.
 * Automatically evicts old blocks from memory when maxBlocksInMemory is exceeded.
 */
export class IndexedDbBlockStore {
  private static readonly DB_VERSION = 2;
  private static readonly CURRENT_SCHEMA_VERSION = 2;
  private dbName: string;
  private maxBlocksInMemory: number;
  private db: IDBDatabase | null = null;
  private schemaVersion = 1;
  
  // In-memory LRU cache
  private memoryBlocks = new Map<number, Block>(); // height -> block
  private accessOrder: number[] = []; // LRU tracking (oldest first)
  
  // Indexes
  private hashToHeight = new Map<string, number>(); // blockHash -> height
  private latestHeight = 0;
  private oldestPersistedHeight = 0;
  
  // Store names
  private static readonly BLOCKS_STORE = "blocks";
  private static readonly HEADERS_STORE = "headers";
  private static readonly HASH_INDEX_STORE = "hash_index";
  private static readonly META_STORE = "meta";
  
  constructor(options: BlockStoreOptions = {}) {
    this.dbName = options.dbName ?? "spacekit-blocks";
    this.maxBlocksInMemory = options.maxBlocksInMemory ?? 100;
  }
  
  /* ── Initialization ── */
  
  async init(): Promise<void> {
    if (this.db) return;
    
    this.db = await this.openDb();
    await this.loadMeta();
    if (this.schemaVersion < IndexedDbBlockStore.CURRENT_SCHEMA_VERSION) {
      await this.migrateSchema(this.schemaVersion, IndexedDbBlockStore.CURRENT_SCHEMA_VERSION);
    }
    await this.loadRecentBlocks();
  }
  
  private openDb(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
      if (typeof indexedDB === "undefined") {
        reject(new Error("IndexedDB is not available"));
        return;
      }
      
      const request = indexedDB.open(this.dbName, IndexedDbBlockStore.DB_VERSION);
      
      request.onerror = () => reject(request.error);
      
      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result;
        
        // Blocks store: keyed by height
        if (!db.objectStoreNames.contains(IndexedDbBlockStore.BLOCKS_STORE)) {
          db.createObjectStore(IndexedDbBlockStore.BLOCKS_STORE, { keyPath: "height" });
        }
        
        // Headers store: keyed by height (lightweight for queries)
        if (!db.objectStoreNames.contains(IndexedDbBlockStore.HEADERS_STORE)) {
          db.createObjectStore(IndexedDbBlockStore.HEADERS_STORE, { keyPath: "height" });
        }
        
        // Hash index: blockHash -> height
        if (!db.objectStoreNames.contains(IndexedDbBlockStore.HASH_INDEX_STORE)) {
          db.createObjectStore(IndexedDbBlockStore.HASH_INDEX_STORE);
        }
        
        // Metadata store
        if (!db.objectStoreNames.contains(IndexedDbBlockStore.META_STORE)) {
          db.createObjectStore(IndexedDbBlockStore.META_STORE);
        }

        const tx = request.transaction;
        if (tx) {
          const metaStore = tx.objectStore(IndexedDbBlockStore.META_STORE);
          metaStore.put(IndexedDbBlockStore.CURRENT_SCHEMA_VERSION, "schemaVersion");
        }
      };
      
      request.onsuccess = () => resolve(request.result);
    });
  }
  
  private async loadMeta(): Promise<void> {
    if (!this.db) return;
    
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(IndexedDbBlockStore.META_STORE, "readonly");
      const store = tx.objectStore(IndexedDbBlockStore.META_STORE);
      
      const latestReq = store.get("latestHeight");
      const oldestReq = store.get("oldestPersistedHeight");
      const schemaReq = store.get("schemaVersion");
      
      tx.oncomplete = () => {
        this.latestHeight = typeof latestReq.result === "number" ? latestReq.result : 0;
        this.oldestPersistedHeight = typeof oldestReq.result === "number" ? oldestReq.result : 0;
        this.schemaVersion = typeof schemaReq.result === "number" ? schemaReq.result : 1;
        resolve();
      };
      tx.onerror = () => reject(tx.error);
    });
  }

  private async migrateSchema(fromVersion: number, toVersion: number): Promise<void> {
    if (!this.db || fromVersion >= toVersion) return;
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(IndexedDbBlockStore.META_STORE, "readwrite");
      const store = tx.objectStore(IndexedDbBlockStore.META_STORE);
      store.put(toVersion, "schemaVersion");
      tx.oncomplete = () => {
        this.schemaVersion = toVersion;
        resolve();
      };
      tx.onerror = () => reject(tx.error);
    });
  }
  
  private async loadRecentBlocks(): Promise<void> {
    if (!this.db || this.latestHeight === 0) return;
    
    // Load the most recent blocks into memory
    const startHeight = Math.max(1, this.latestHeight - this.maxBlocksInMemory + 1);
    
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction([IndexedDbBlockStore.BLOCKS_STORE, IndexedDbBlockStore.HASH_INDEX_STORE], "readonly");
      const blocksStore = tx.objectStore(IndexedDbBlockStore.BLOCKS_STORE);
      const hashStore = tx.objectStore(IndexedDbBlockStore.HASH_INDEX_STORE);
      
      // Load hash index
      const hashCursor = hashStore.openCursor();
      hashCursor.onsuccess = () => {
        const cursor = hashCursor.result;
        if (cursor) {
          this.hashToHeight.set(cursor.key as string, cursor.value as number);
          cursor.continue();
        }
      };
      
      // Load recent blocks
      const range = IDBKeyRange.lowerBound(startHeight);
      const blockCursor = blocksStore.openCursor(range);
      
      blockCursor.onsuccess = () => {
        const cursor = blockCursor.result;
        if (cursor) {
          const block = deserializeBlock(cursor.value as SerializedBlock);
          this.memoryBlocks.set(block.height, block);
          this.accessOrder.push(block.height);
          cursor.continue();
        }
      };
      
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
  
  /* ── Public API ── */
  
  /**
   * Add a new block to the store.
   * Persists to IndexedDB and manages memory cache.
   */
  async addBlock(block: Block): Promise<void> {
    if (!this.db) {
      throw new Error("BlockStore not initialized. Call init() first.");
    }
    
    // Add to memory cache
    this.memoryBlocks.set(block.height, block);
    this.hashToHeight.set(block.blockHash, block.height);
    this.accessOrder.push(block.height);
    
    // Update latest height
    if (block.height > this.latestHeight) {
      this.latestHeight = block.height;
    }
    
    // Persist to IndexedDB
    await this.persistBlock(block);
    
    // Evict from memory if needed (but keep in IndexedDB)
    await this.evictFromMemory();
  }
  
  private async persistBlock(block: Block): Promise<void> {
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(
        [IndexedDbBlockStore.BLOCKS_STORE, IndexedDbBlockStore.HEADERS_STORE, IndexedDbBlockStore.HASH_INDEX_STORE, IndexedDbBlockStore.META_STORE],
        "readwrite"
      );
      
      const blocksStore = tx.objectStore(IndexedDbBlockStore.BLOCKS_STORE);
      const headersStore = tx.objectStore(IndexedDbBlockStore.HEADERS_STORE);
      const hashStore = tx.objectStore(IndexedDbBlockStore.HASH_INDEX_STORE);
      const metaStore = tx.objectStore(IndexedDbBlockStore.META_STORE);
      
      // Store full block
      blocksStore.put(serializeBlock(block));
      
      // Store header separately for fast queries
      headersStore.put(block.header);
      
      // Update hash index
      hashStore.put(block.height, block.blockHash);
      
      // Update metadata
      metaStore.put(this.latestHeight, "latestHeight");
      if (this.oldestPersistedHeight === 0) {
        this.oldestPersistedHeight = block.height;
        metaStore.put(block.height, "oldestPersistedHeight");
      }
      
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
  
  private async evictFromMemory(): Promise<void> {
    while (this.memoryBlocks.size > this.maxBlocksInMemory) {
      const oldestHeight = this.accessOrder.shift();
      if (oldestHeight !== undefined) {
        this.memoryBlocks.delete(oldestHeight);
      }
    }
  }
  
  /**
   * Get a block by height.
   * First checks memory cache, then IndexedDB.
   */
  async getBlock(height: number): Promise<Block | null> {
    // Check memory cache first
    const cached = this.memoryBlocks.get(height);
    if (cached) {
      // Update LRU
      const idx = this.accessOrder.indexOf(height);
      if (idx !== -1) {
        this.accessOrder.splice(idx, 1);
        this.accessOrder.push(height);
      }
      return cached;
    }
    
    // Load from IndexedDB
    if (!this.db) return null;
    
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(IndexedDbBlockStore.BLOCKS_STORE, "readonly");
      const store = tx.objectStore(IndexedDbBlockStore.BLOCKS_STORE);
      const request = store.get(height);
      
      request.onsuccess = () => {
        if (request.result) {
          const block = deserializeBlock(request.result as SerializedBlock);
          // Add to memory cache (will evict if needed on next add)
          this.memoryBlocks.set(height, block);
          this.accessOrder.push(height);
          resolve(block);
        } else {
          resolve(null);
        }
      };
      request.onerror = () => reject(request.error);
    });
  }
  
  /**
   * Get a block by its hash.
   */
  async getBlockByHash(hash: string): Promise<Block | null> {
    const height = this.hashToHeight.get(hash);
    if (height === undefined) {
      // Try loading from IndexedDB
      if (!this.db) return null;
      
      return new Promise((resolve, reject) => {
        const tx = this.db!.transaction(IndexedDbBlockStore.HASH_INDEX_STORE, "readonly");
        const store = tx.objectStore(IndexedDbBlockStore.HASH_INDEX_STORE);
        const request = store.get(hash);
        
        request.onsuccess = async () => {
          if (typeof request.result === "number") {
            this.hashToHeight.set(hash, request.result);
            resolve(await this.getBlock(request.result));
          } else {
            resolve(null);
          }
        };
        request.onerror = () => reject(request.error);
      });
    }
    
    return this.getBlock(height);
  }
  
  /**
   * Get block header by height (lightweight).
   */
  async getHeader(height: number): Promise<BlockHeader | null> {
    // Check memory first
    const cached = this.memoryBlocks.get(height);
    if (cached) return cached.header;
    
    if (!this.db) return null;
    
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(IndexedDbBlockStore.HEADERS_STORE, "readonly");
      const store = tx.objectStore(IndexedDbBlockStore.HEADERS_STORE);
      const request = store.get(height);
      
      request.onsuccess = () => {
        resolve(request.result as BlockHeader | null);
      };
      request.onerror = () => reject(request.error);
    });
  }
  
  /**
   * Get all blocks currently in memory (for fast iteration).
   */
  getBlocksInMemory(): Block[] {
    return Array.from(this.memoryBlocks.values()).sort((a, b) => a.height - b.height);
  }
  
  /**
   * Get blocks in a height range.
   */
  async getBlockRange(fromHeight: number, toHeight: number): Promise<Block[]> {
    const blocks: Block[] = [];
    
    for (let h = fromHeight; h <= toHeight; h++) {
      const block = await this.getBlock(h);
      if (block) blocks.push(block);
    }
    
    return blocks;
  }
  
  /**
   * Get the latest block.
   */
  async getLatestBlock(): Promise<Block | null> {
    if (this.latestHeight === 0) return null;
    return this.getBlock(this.latestHeight);
  }
  
  /**
   * Get store statistics.
   */
  getStats(): BlockStoreStats {
    return {
      totalBlocks: this.latestHeight,
      inMemoryBlocks: this.memoryBlocks.size,
      persistedBlocks: this.latestHeight - this.oldestPersistedHeight + 1,
      latestHeight: this.latestHeight,
      oldestHeight: this.oldestPersistedHeight,
      dbName: this.dbName,
    };
  }
  
  /**
   * Get the latest block height.
   */
  getLatestHeight(): number {
    return this.latestHeight;
  }
  
  /**
   * Clear all blocks (for testing/reset).
   */
  async clear(): Promise<void> {
    this.memoryBlocks.clear();
    this.hashToHeight.clear();
    this.accessOrder = [];
    this.latestHeight = 0;
    this.oldestPersistedHeight = 0;
    
    if (!this.db) return;
    
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(
        [IndexedDbBlockStore.BLOCKS_STORE, IndexedDbBlockStore.HEADERS_STORE, IndexedDbBlockStore.HASH_INDEX_STORE, IndexedDbBlockStore.META_STORE],
        "readwrite"
      );
      
      tx.objectStore(IndexedDbBlockStore.BLOCKS_STORE).clear();
      tx.objectStore(IndexedDbBlockStore.HEADERS_STORE).clear();
      tx.objectStore(IndexedDbBlockStore.HASH_INDEX_STORE).clear();
      tx.objectStore(IndexedDbBlockStore.META_STORE).clear();
      
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
  
  /**
   * Close the database connection.
   */
  close(): void {
    if (this.db) {
      this.db.close();
      this.db = null;
    }
  }
}

export default IndexedDbBlockStore;
