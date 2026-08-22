export interface HeaderCacheEntry<T = unknown> {
  rpcUrl: string;
  chainId?: string;
  latestHeight: number;
  syncedAt: number;
  headers: T[];
}

export class IndexedDbHeaderCache<T = unknown> {
  private dbName: string;
  private storeName: string;
  private db: IDBDatabase | null = null;

  constructor(dbName = "spacekit-header-cache", storeName = "headers") {
    this.dbName = dbName;
    this.storeName = storeName;
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
          db.createObjectStore(this.storeName, { keyPath: "rpcUrl" });
        }
      };
      request.onsuccess = () => resolve(request.result);
    });
  }

  async save(entry: HeaderCacheEntry<T>): Promise<void> {
    await this.init();
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(this.storeName, "readwrite");
      const store = tx.objectStore(this.storeName);
      store.put(entry);
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }

  async load(rpcUrl: string): Promise<HeaderCacheEntry<T> | null> {
    await this.init();
    return new Promise((resolve, reject) => {
      const tx = this.db!.transaction(this.storeName, "readonly");
      const store = tx.objectStore(this.storeName);
      const req = store.get(rpcUrl);
      req.onsuccess = () => resolve((req.result as HeaderCacheEntry<T>) ?? null);
      req.onerror = () => reject(req.error);
    });
  }
}
