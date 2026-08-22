import { sha256Hex } from "./vm/hash.js";
import type { RemoteStorageAdapter } from "./tools/types.js";

const hexTable = Array.from({ length: 256 }, (_, i) =>
  i.toString(16).padStart(2, "0")
);

export function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) {
    out += hexTable[b];
  }
  return out;
}

export interface StorageAdapter {
  get(key: Uint8Array): Uint8Array | undefined;
  set(key: Uint8Array, value: Uint8Array): void;
  getAux?(key: Uint8Array): Uint8Array | undefined;
  setAux?(key: Uint8Array, value: Uint8Array): void;
  entries?(): Array<{ key: Uint8Array; value: Uint8Array }>;
  clear?(): void;
  entriesWithVersion?(): Array<{ key: Uint8Array; value: Uint8Array; version: number; deleted?: boolean; clock?: VectorClock }>;
  setWithVersion?(key: Uint8Array, value: Uint8Array, version: number): void;
  deleteWithVersion?(key: Uint8Array, version: number): void;
  mergeFromRemote?(
    entries: Array<{ key: Uint8Array; value: Uint8Array; version: number; deleted?: boolean; clock?: VectorClock }>,
    strategy?: "lww" | "vector"
  ): void;
}

export function createInMemoryStorage(): StorageAdapter {
  const store = new Map<string, Uint8Array>();
  const auxStore = new Map<string, Uint8Array>();

  return {
    get(key: Uint8Array) {
      return store.get(bytesToHex(key));
    },
    set(key: Uint8Array, value: Uint8Array) {
      store.set(bytesToHex(key), value);
    },
    getAux(key: Uint8Array) {
      return auxStore.get(bytesToHex(key));
    },
    setAux(key: Uint8Array, value: Uint8Array) {
      auxStore.set(bytesToHex(key), value);
    },
    entries() {
      return Array.from(store.entries()).map(([keyHex, value]) => ({
        key: hexToBytes(keyHex),
        value,
      }));
    },
    clear() {
      store.clear();
      auxStore.clear();
    },
  };
}

export interface StorageNodeAdapterOptions {
  baseUrl: string;
  did: string;
  collection?: string;
}

export class StorageNodeAdapter implements StorageAdapter {
  private cache = new Map<string, Uint8Array>();
  private auxCache = new Map<string, Uint8Array>();
  private baseUrl: string;
  private did: string;
  private collection: string;

  constructor(options: StorageNodeAdapterOptions) {
    this.baseUrl = options.baseUrl.replace(/\/$/, "");
    this.did = options.did;
    this.collection = options.collection ?? "spacekitvm";
  }

  get(key: Uint8Array): Uint8Array | undefined {
    return this.cache.get(bytesToHex(key));
  }

  set(key: Uint8Array, value: Uint8Array): void {
    this.cache.set(bytesToHex(key), value);
  }

  getAux(key: Uint8Array): Uint8Array | undefined {
    return this.auxCache.get(bytesToHex(key));
  }

  setAux(key: Uint8Array, value: Uint8Array): void {
    this.auxCache.set(bytesToHex(key), value);
  }

  entries() {
    return Array.from(this.cache.entries()).map(([keyHex, value]) => ({
      key: hexToBytes(keyHex),
      value,
    }));
  }

  clear() {
    this.cache.clear();
    this.auxCache.clear();
  }

  async pull(key: Uint8Array): Promise<Uint8Array | undefined> {
    const keyHex = bytesToHex(key);
    const url = `${this.baseUrl}/api/documents/${encodeURIComponent(
      this.collection
    )}/${encodeURIComponent(keyHex)}`;
    const res = await fetch(url, {
      headers: {
        Authorization: `DID ${this.did}`,
      },
    });
    if (!res.ok) {
      return undefined;
    }
    const json = await res.json();
    const valueHex = json?.document?.data?.value_hex;
    if (typeof valueHex !== "string") {
      return undefined;
    }
    const value = hexToBytes(valueHex);
    this.cache.set(keyHex, value);
    return value;
  }

  async listDocuments(collection = this.collection): Promise<Array<{ id: string; data: any }>> {
    const url = `${this.baseUrl}/api/documents/${encodeURIComponent(collection)}`;
    const res = await fetch(url, {
      headers: {
        Authorization: `DID ${this.did}`,
      },
    });
    if (!res.ok) {
      return [];
    }
    const json = await res.json();
    return Array.isArray(json?.documents) ? json.documents : [];
  }

  async deleteDocument(collection: string, id: string): Promise<boolean> {
    const url = `${this.baseUrl}/api/documents/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`;
    const res = await fetch(url, {
      method: "DELETE",
      headers: {
        Authorization: `DID ${this.did}`,
      },
    });
    return res.ok;
  }

  async pullAllEntries(collection = this.collection): Promise<Array<{ key: Uint8Array; value: Uint8Array; version: number; deleted?: boolean; clock?: VectorClock }>> {
    const docs = await this.listDocuments(collection);
    const entries: Array<{ key: Uint8Array; value: Uint8Array; version: number; deleted?: boolean; clock?: VectorClock }> = [];
    for (const doc of docs) {
      const id = doc?.id;
      const data = doc?.data;
      const valueHex = data?.value_hex;
      const deleted = Boolean(data?.deleted);
      const clock = typeof data?.clock === "object" ? (data.clock as VectorClock) : undefined;
      if (typeof id !== "string" || typeof valueHex !== "string") {
        if (deleted && typeof id === "string") {
          entries.push({
            key: hexToBytes(id),
            value: new Uint8Array(),
            version: typeof data?.version === "number" ? data.version : 0,
            deleted: true,
            clock,
          });
        }
        continue;
      }
      const version = typeof data?.version === "number" ? data.version : 0;
      entries.push({
        key: hexToBytes(id),
        value: hexToBytes(valueHex),
        version,
        deleted,
        clock,
      });
    }
    return entries;
  }

  async pushEntries(
    entries: Array<{ key: Uint8Array; value: Uint8Array; version: number; deleted?: boolean; clock?: VectorClock }>,
    collection = this.collection
  ): Promise<void> {
    await Promise.all(
      entries.map((entry) => {
        const keyHex = bytesToHex(entry.key);
        return this.putDocument(collection, keyHex, {
          value_hex: bytesToHex(entry.value),
          version: entry.version,
          deleted: entry.deleted ?? false,
          clock: entry.clock ?? null,
          updated_at: Date.now(),
        });
      })
    );
  }
  async push(key: Uint8Array): Promise<boolean> {
    const keyHex = bytesToHex(key);
    const value = this.cache.get(keyHex);
    if (!value) {
      return false;
    }
    const url = `${this.baseUrl}/api/documents/${encodeURIComponent(
      this.collection
    )}/${encodeURIComponent(keyHex)}`;
    const res = await fetch(url, {
      method: "PUT",
      headers: {
        Authorization: `DID ${this.did}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify({
        value_hex: bytesToHex(value),
        updated_at: Date.now(),
      }),
    });
    return res.ok;
  }

  async syncAll(): Promise<void> {
    const keys = Array.from(this.cache.keys());
    await Promise.all(keys.map((key) => this.push(hexToBytes(key))));
  }

  async putDocument(collection: string, id: string, body: Record<string, unknown>): Promise<boolean> {
    const url = `${this.baseUrl}/api/documents/${encodeURIComponent(
      collection
    )}/${encodeURIComponent(id)}`;
    const res = await fetch(url, {
      method: "PUT",
      headers: {
        Authorization: `DID ${this.did}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    });
    return res.ok;
  }
}

/**
 * {@link RemoteStorageAdapter} for WASM `remote_storage_put` / `remote_storage_get`: stores **raw bytes**
 * as `value_hex` on **`PUT /api/documents/{collection}/{ref}`** (same wire format as {@link StorageNodeAdapter}).
 *
 * Use this with **spacekit-storage-node** (signatures, facts, P2P) or the **simulator HTTP** stub (in-memory
 * `HashMap` + optional JSON file — no SPHINCS+ / replication). For JSON-only SpaceTime helpers see
 * {@link createSpaceTimeStorageNode} in `./spacetime/storage_remote.js`.
 */
export function createStorageNodeRemoteBlobAdapter(options: {
  baseUrl: string;
  did: string;
  collection?: string;
  /** Prefix for returned ref string, e.g. `blob` → `blob:<sha256_hex>`. */
  refPrefix?: string;
}): RemoteStorageAdapter {
  const baseUrl = options.baseUrl.replace(/\/$/, "");
  const collection = options.collection ?? "spacetime";
  const refPrefix = options.refPrefix ?? "blob";
  const headers = {
    Authorization: `DID ${options.did}`,
    "Content-Type": "application/json",
  } as const;

  return {
    async put(data: Uint8Array): Promise<string> {
      const hash = await sha256Hex(data);
      const ref = `${refPrefix}:${hash}`;
      const url = `${baseUrl}/api/documents/${encodeURIComponent(
        collection
      )}/${encodeURIComponent(ref)}`;
      const res = await fetch(url, {
        method: "PUT",
        headers: { ...headers },
        body: JSON.stringify({
          value_hex: bytesToHex(data),
          updated_at: Date.now(),
        }),
      });
      if (!res.ok) {
        throw new Error(`Storage node remote_storage put failed: ${res.status}`);
      }
      return ref;
    },
    async get(ref: string): Promise<Uint8Array | null> {
      const url = `${baseUrl}/api/documents/${encodeURIComponent(
        collection
      )}/${encodeURIComponent(ref)}`;
      const res = await fetch(url, {
        headers: { Authorization: headers.Authorization },
      });
      if (!res.ok) {
        return null;
      }
      const json = (await res.json()) as {
        document?: { data?: { value_hex?: string } };
      };
      const valueHex = json?.document?.data?.value_hex;
      if (typeof valueHex !== "string") {
        return null;
      }
      return hexToBytes(valueHex);
    },
  };
}

export class IndexedDbStorageAdapter implements StorageAdapter {
  private cache = new Map<string, { value: Uint8Array; version: number; deleted?: boolean; clock?: VectorClock }>();
  private dbName: string;
  private storeName: string;

  constructor(dbName = "spacekitvm", storeName = "kv") {
    this.dbName = dbName;
    this.storeName = storeName;
  }

  get(key: Uint8Array): Uint8Array | undefined {
    return this.cache.get(bytesToHex(key))?.value;
  }

  set(key: Uint8Array, value: Uint8Array): void {
    this.cache.set(bytesToHex(key), { value, version: Date.now(), deleted: false });
  }

  entries() {
    return Array.from(this.cache.entries()).map(([keyHex, entry]) => ({
      key: hexToBytes(keyHex),
      value: entry.value,
    }));
  }

  entriesWithVersion() {
    return Array.from(this.cache.entries()).map(([keyHex, entry]) => ({
      key: hexToBytes(keyHex),
      value: entry.value,
      version: entry.version,
      deleted: entry.deleted ?? false,
      clock: entry.clock,
    }));
  }

  setWithVersion(key: Uint8Array, value: Uint8Array, version: number): void {
    this.cache.set(bytesToHex(key), { value, version, deleted: false, clock: { local: 1 } });
  }

  deleteWithVersion(key: Uint8Array, version: number): void {
    this.cache.set(bytesToHex(key), { value: new Uint8Array(), version, deleted: true, clock: { local: 1 } });
  }

  mergeFromRemote(
    entries: Array<{ key: Uint8Array; value: Uint8Array; version: number; deleted?: boolean; clock?: VectorClock }>,
    strategy: "lww" | "vector" = "lww"
  ): void {
    for (const entry of entries) {
      const keyHex = bytesToHex(entry.key);
      const existing = this.cache.get(keyHex);
      if (strategy === "lww") {
        if (!existing || entry.version >= existing.version) {
          this.cache.set(keyHex, {
            value: entry.value,
            version: entry.version,
            deleted: entry.deleted ?? false,
            clock: entry.clock,
          });
        }
        continue;
      }
      const localClock = existing?.clock ?? {};
      const remoteClock = entry.clock ?? {};
      const comparison = compareVectorClocks(localClock, remoteClock);
      if (comparison === "remote") {
        this.cache.set(keyHex, {
          value: entry.value,
          version: entry.version,
          deleted: entry.deleted ?? false,
          clock: remoteClock,
        });
      } else if (comparison === "concurrent") {
        const conflictKey = `__conflict__:${keyHex}:${Date.now()}`;
        const conflictPayload = JSON.stringify({
          local: { value_hex: bytesToHex(existing?.value ?? new Uint8Array()), version: existing?.version ?? 0, deleted: existing?.deleted ?? false },
          remote: { value_hex: bytesToHex(entry.value), version: entry.version, deleted: entry.deleted ?? false },
        });
        this.cache.set(conflictKey, {
          value: new TextEncoder().encode(conflictPayload),
          version: Date.now(),
          deleted: false,
          clock: { local: 1 },
        });
      }
    }
  }

  clear() {
    this.cache.clear();
  }

  async init(): Promise<void> {
    const db = await this.openDb();
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(this.storeName, "readonly");
      const store = tx.objectStore(this.storeName);
      const request = store.openCursor();
      request.onerror = () => reject(request.error);
      request.onsuccess = () => {
        const cursor = request.result;
        if (!cursor) {
          resolve();
          return;
        }
        const keyHex = cursor.key as string;
        const raw = cursor.value as Uint8Array | { value: Uint8Array; version: number; deleted?: boolean; clock?: VectorClock };
        if (raw instanceof Uint8Array) {
          this.cache.set(keyHex, { value: new Uint8Array(raw), version: 0, deleted: false, clock: { local: 1 } });
        } else {
          this.cache.set(keyHex, {
            value: new Uint8Array(raw.value),
            version: raw.version ?? 0,
            deleted: raw.deleted ?? false,
            clock: raw.clock,
          });
        }
        cursor.continue();
      };
    });
  }

  async syncAll(): Promise<void> {
    const db = await this.openDb();
    const entries = Array.from(this.cache.entries());
    await new Promise<void>((resolve, reject) => {
      const tx = db.transaction(this.storeName, "readwrite");
      const store = tx.objectStore(this.storeName);
      for (const [keyHex, entry] of entries) {
        store.put(
          { value: entry.value, version: entry.version, deleted: entry.deleted ?? false, clock: entry.clock ?? null },
          keyHex
        );
      }
      tx.oncomplete = () => resolve();
      tx.onerror = () => reject(tx.error);
    });
  }

  private openDb(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
      if (typeof indexedDB === "undefined") {
        reject(new Error("indexedDB is not available in this environment"));
        return;
      }
      const request = indexedDB.open(this.dbName, 1);
      request.onerror = () => reject(request.error);
      request.onupgradeneeded = () => {
        const db = request.result;
        if (!db.objectStoreNames.contains(this.storeName)) {
          db.createObjectStore(this.storeName);
        }
      };
      request.onsuccess = () => resolve(request.result);
    });
  }
}

export async function syncWithStorageNode(
  local: IndexedDbStorageAdapter,
  remote: StorageNodeAdapter,
  collection?: string,
  strategy: "lww" | "vector" = "lww"
): Promise<void> {
  const remoteEntries = await remote.pullAllEntries(collection);
  local.mergeFromRemote(remoteEntries, strategy);
  await local.syncAll();
  const localEntries = local.entriesWithVersion?.() ?? [];
  await remote.pushEntries(localEntries, collection);
}

export function hexToBytes(hex: string): Uint8Array {
  const clean = hex.length % 2 === 0 ? hex : `0${hex}`;
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

export type VectorClock = Record<string, number>;

export function compareVectorClocks(
  local: VectorClock,
  remote: VectorClock
): "local" | "remote" | "equal" | "concurrent" {
  let localGreater = false;
  let remoteGreater = false;
  const keys = new Set([...Object.keys(local), ...Object.keys(remote)]);
  for (const key of keys) {
    const lv = local[key] ?? 0;
    const rv = remote[key] ?? 0;
    if (lv > rv) {
      localGreater = true;
    } else if (rv > lv) {
      remoteGreater = true;
    }
  }
  if (localGreater && remoteGreater) {
    return "concurrent";
  }
  if (localGreater) {
    return "local";
  }
  if (remoteGreater) {
    return "remote";
  }
  return "equal";
}
