/**
 * Session Persistence for SpacekitVm
 * 
 * Stores and recovers:
 * - Contract deployments (WASM bytes + metadata)
 * - Chain state (height, state root)
 * - User nonces
 */

/* ───────────────────────── Types ───────────────────────── */

export interface ContractDeployment {
  id: string;
  name: string;
  wasmHash: string;
  /** WASM bytes stored as base64 */
  wasmBase64: string;
  abiVersion: string;
  deployedAt: number;
}

export interface SessionState {
  /** Chain ID */
  chainId: string;
  /** Active identity DID (optional) */
  identityDid?: string;
  /** Latest block height */
  latestHeight: number;
  /** Latest block hash */
  latestBlockHash: string;
  /** State root from latest block */
  stateRoot: string;
  /** Quantum Verkle state root from latest block (optional) */
  quantumStateRoot?: string;
  /** Deployed contracts */
  contracts: ContractDeployment[];
  /** Nonces by DID */
  nonces: Record<string, number>;
  /** Session timestamp */
  timestamp: number;
  /** Session version for migration */
  version: number;
}

export interface SessionStoreOptions {
  dbName?: string;
  storeName?: string;
}

/* ───────────────────────── Helpers ───────────────────────── */

function bytesToBase64(bytes: Uint8Array): string {
  if (typeof btoa !== "undefined") {
    let binary = "";
    for (const b of bytes) {
      binary += String.fromCharCode(b);
    }
    return btoa(binary);
  }
  return Buffer.from(bytes).toString("base64");
}

function base64ToBytes(base64: string): Uint8Array {
  if (typeof atob !== "undefined") {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }
  return new Uint8Array(Buffer.from(base64, "base64"));
}

/* ───────────────────────── Session Store ───────────────────────── */

const LEGACY_VERSION = 1;
const CURRENT_VERSION = 2;

export class SessionStore {
  private dbName: string;
  private storeName: string;
  private db: IDBDatabase | null = null;
  
  constructor(options: SessionStoreOptions = {}) {
    this.dbName = options.dbName ?? "spacekit-session";
    this.storeName = options.storeName ?? "session";
  }
  
  async init(): Promise<void> {
    if (this.db) return;
    this.db = await this.openDb();
  }
  
  private openDb(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
      if (typeof indexedDB === "undefined") {
        reject(new Error("IndexedDB not available"));
        return;
      }
      
      const request = indexedDB.open(this.dbName, 1);
      
      request.onerror = () => reject(request.error);
      
      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result;
        if (!db.objectStoreNames.contains(this.storeName)) {
          db.createObjectStore(this.storeName);
        }
      };
      
      request.onsuccess = () => resolve(request.result);
    });
  }
  
  /**
   * Save session state
   */
  async saveSession(state: SessionState): Promise<void> {
    if (!this.db) await this.init();
    
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(this.storeName, "readwrite");
      const store = tx.objectStore(this.storeName);
      
      // Store with version for migration support
      store.put({ ...state, version: CURRENT_VERSION }, "current");
      
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
  
  /**
   * Load session state
   */
  async loadSession(): Promise<SessionState | null> {
    if (!this.db) await this.init();
    
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(this.storeName, "readonly");
      const store = tx.objectStore(this.storeName);
      const request = store.get("current");
      
      request.onsuccess = () => {
        const state = request.result as SessionState | undefined;
        if (state) {
          const version = typeof state.version === "number" ? state.version : LEGACY_VERSION;
          if (version <= CURRENT_VERSION) {
            resolve({ ...state, version: CURRENT_VERSION });
            return;
          }
        }
        resolve(null);
      };
      request.onerror = () => reject(request.error);
    });
  }
  
  /**
   * Clear session
   */
  async clearSession(): Promise<void> {
    if (!this.db) await this.init();
    
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(this.storeName, "readwrite");
      const store = tx.objectStore(this.storeName);
      store.delete("current");
      
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }
  
  /**
   * Add a contract deployment to the session
   */
  async addContract(contract: ContractDeployment): Promise<void> {
    const session = await this.loadSession();
    if (session) {
      // Replace existing contract with same ID
      session.contracts = session.contracts.filter(c => c.id !== contract.id);
      session.contracts.push(contract);
      await this.saveSession(session);
    }
  }
  
  /**
   * Get WASM bytes for a contract
   */
  async getContractWasm(contractId: string): Promise<Uint8Array | null> {
    const session = await this.loadSession();
    if (!session) return null;
    
    const contract = session.contracts.find(c => c.id === contractId);
    if (!contract) return null;
    
    return base64ToBytes(contract.wasmBase64);
  }
  
  /**
   * Check if a session exists
   */
  async hasSession(): Promise<boolean> {
    const session = await this.loadSession();
    return session !== null && session.contracts.length > 0;
  }
  
  close(): void {
    if (this.db) {
      this.db.close();
      this.db = null;
    }
  }
}

/**
 * Create a contract deployment record
 */
export function createContractDeployment(
  id: string,
  name: string,
  wasmBytes: Uint8Array,
  wasmHash: string,
  abiVersion: string
): ContractDeployment {
  return {
    id,
    name,
    wasmHash,
    wasmBase64: bytesToBase64(wasmBytes),
    abiVersion,
    deployedAt: Date.now(),
  };
}

/**
 * Extract WASM bytes from a deployment record
 */
export function getWasmFromDeployment(deployment: ContractDeployment): Uint8Array {
  return base64ToBytes(deployment.wasmBase64);
}

export default SessionStore;
