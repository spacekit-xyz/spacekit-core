/**
 * Pure TypeScript storage backend: sorted in-memory store + append-only WAL.
 * Replay WAL on open; compaction rewrites WAL to a snapshot.
 * Node/Bun only (uses node:fs).
 */

import { Buffer } from "node:buffer";
import { readFileSync, writeFileSync, appendFileSync, renameSync, existsSync, mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { encodeKey, decodeKey } from "./key-codec.js";

export interface ScanRange {
  lower?: IDBValidKey;
  upper?: IDBValidKey;
  lowerOpen?: boolean;
  upperOpen?: boolean;
}

interface WalEntry {
  op: "put" | "del";
  store: string;
  key: string;
  value?: string;
}

export class WalBackend {
  /** store name -> sorted array of [encodedKeyHex, valueJson] */
  private data = new Map<string, Array<[string, string]>>();
  private walPath: string;

  constructor(dbPath: string) {
    this.walPath = dbPath.endsWith(".wal") ? dbPath : dbPath + ".wal";
    const dir = dirname(this.walPath);
    if (!existsSync(dir)) {
      mkdirSync(dir, { recursive: true });
    }
    this.replay();
  }

  private replay(): void {
    if (!existsSync(this.walPath)) return;
    const content = readFileSync(this.walPath, "utf8");
    const lines = content.split("\n").filter(Boolean);
    for (const line of lines) {
      try {
        const entry: WalEntry = JSON.parse(line);
        this.applyEntry(entry);
      } catch {
        // skip malformed lines
      }
    }
  }

  private applyEntry(entry: WalEntry): void {
    if (!this.data.has(entry.store)) this.data.set(entry.store, []);
    const pairs = this.data.get(entry.store)!;
    const idx = this.binarySearch(pairs, entry.key);

    if (entry.op === "put") {
      const value = entry.value ?? "";
      if (idx < pairs.length && pairs[idx]![0] === entry.key) {
        pairs[idx]![1] = value;
      } else {
        pairs.splice(idx, 0, [entry.key, value]);
      }
    } else if (entry.op === "del") {
      if (idx < pairs.length && pairs[idx]![0] === entry.key) {
        pairs.splice(idx, 1);
      }
    }
  }

  private binarySearch(pairs: Array<[string, string]>, key: string): number {
    let lo = 0;
    let hi = pairs.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (pairs[mid]![0]! < key) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  private keyToHex(key: IDBValidKey): string {
    return encodeKey(key).toString("hex");
  }

  put(store: string, key: IDBValidKey, value: unknown): void {
    const encoded = this.keyToHex(key);
    const entry: WalEntry = { op: "put", store, key: encoded, value: JSON.stringify(value) };
    appendFileSync(this.walPath, JSON.stringify(entry) + "\n");
    this.applyEntry(entry);
  }

  get(store: string, key: IDBValidKey): unknown | undefined {
    const encoded = this.keyToHex(key);
    const pairs = this.data.get(store) ?? [];
    const idx = this.binarySearch(pairs, encoded);
    if (idx < pairs.length && pairs[idx]![0] === encoded) {
      try {
        return JSON.parse(pairs[idx]![1]);
      } catch {
        return undefined;
      }
    }
    return undefined;
  }

  delete(store: string, key: IDBValidKey): void {
    const encoded = this.keyToHex(key);
    const entry: WalEntry = { op: "del", store, key: encoded };
    appendFileSync(this.walPath, JSON.stringify(entry) + "\n");
    this.applyEntry(entry);
  }

  clear(store: string): void {
    const pairs = this.data.get(store);
    if (!pairs || pairs.length === 0) return;
    this.data.set(store, []);
    for (const [key] of pairs) {
      const entry: WalEntry = { op: "del", store, key };
      appendFileSync(this.walPath, JSON.stringify(entry) + "\n");
    }
  }

  /**
   * Scan a store within an optional range, for cursors and key ranges.
   */
  scan(
    store: string,
    range: ScanRange,
    direction: "next" | "prev" = "next"
  ): Array<{ key: IDBValidKey; value: unknown }> {
    const pairs = this.data.get(store) ?? [];
    const lowerEnc = range.lower !== undefined ? this.keyToHex(range.lower) : null;
    const upperEnc = range.upper !== undefined ? this.keyToHex(range.upper) : null;

    const filtered = pairs.filter(([k]) => {
      if (lowerEnc !== null) {
        if (range.lowerOpen ? k <= lowerEnc : k < lowerEnc) return false;
      }
      if (upperEnc !== null) {
        if (range.upperOpen ? k >= upperEnc : k > upperEnc) return false;
      }
      return true;
    });

    const results = direction === "prev" ? filtered.slice().reverse() : filtered;

    return results.map(([k, v]) => {
      let value: unknown;
      try {
        value = JSON.parse(v);
      } catch {
        value = undefined;
      }
      return {
        key: decodeKey(Buffer.from(k, "hex")),
        value,
      };
    });
  }

  listStores(): string[] {
    return Array.from(this.data.keys());
  }

  /**
   * Compact WAL: rewrite as a single snapshot and truncate.
   */
  compact(): void {
    const snapshot: WalEntry[] = [];
    for (const [store, pairs] of this.data) {
      for (const [key, value] of pairs) {
        snapshot.push({ op: "put", store, key, value });
      }
    }
    const tmp = this.walPath + ".tmp";
    writeFileSync(tmp, snapshot.map((e) => JSON.stringify(e)).join("\n") + "\n");
    renameSync(tmp, this.walPath);
  }
}
