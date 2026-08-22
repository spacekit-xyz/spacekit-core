/**
 * IDBObjectStore implementation over WalBackend + transaction.
 */

import type { WalBackend } from "./wal-backend.js";
import type { ScanRange } from "./wal-backend.js";
import type { IDBTransactionImpl } from "./transaction.js";
import { FakeIDBRequest } from "./request.js";
import { FakeIDBCursorWithValue } from "./cursor.js";
import type { IDBCursorDirection } from "./cursor.js";

export interface StoreParams {
  name: string;
  keyPath: string | string[] | null;
  autoIncrement: boolean;
}

export class FakeIDBObjectStore {
  constructor(
    private backend: WalBackend,
    public readonly name: string,
    public readonly keyPath: string | string[] | null,
    public readonly autoIncrement: boolean,
    private getTransaction: () => IDBTransactionImpl | null
  ) {}

  private resolveKey(key: IDBValidKey | undefined, value?: unknown): IDBValidKey {
    if (key !== undefined) return key;
    if (this.keyPath !== null && value !== undefined && typeof value === "object" && value !== null) {
      const path = Array.isArray(this.keyPath) ? this.keyPath[0] : this.keyPath;
      const k = (value as Record<string, unknown>)[path];
      if (k !== undefined) return k as IDBValidKey;
    }
    throw new DOMException("Key is required", "DataError");
  }

  put(value: unknown, key?: IDBValidKey): FakeIDBRequest<IDBValidKey> {
    const req = new FakeIDBRequest<IDBValidKey>();
    const resolvedKey = this.resolveKey(key, value);
    const tx = this.getTransaction();
    if (tx) {
      tx.enqueuePut(this.name, resolvedKey, value);
      tx.addRequest(req, resolvedKey);
      queueMicrotask(() => tx.commit());
    } else {
      this.backend.put(this.name, resolvedKey, value);
      req._resolve(resolvedKey);
    }
    return req;
  }

  add(value: unknown, key?: IDBValidKey): FakeIDBRequest<IDBValidKey> {
    const resolvedKey = this.resolveKey(key, value);
    const existing = this.backend.get(this.name, resolvedKey);
    if (existing !== undefined) {
      const req = new FakeIDBRequest<IDBValidKey>();
      req._reject(new DOMException("Key already exists", "ConstraintError"));
      return req;
    }
    return this.put(value, key);
  }

  get(key: IDBValidKey): FakeIDBRequest<unknown> {
    const req = new FakeIDBRequest<unknown>();
    const value = this.backend.get(this.name, key);
    queueMicrotask(() => req._resolve(value as never));
    return req;
  }

  delete(key: IDBValidKey): FakeIDBRequest<undefined> {
    const req = new FakeIDBRequest<undefined>();
    const tx = this.getTransaction();
    if (tx) {
      tx.enqueueDelete(this.name, key);
      tx.addRequest(req, undefined);
      queueMicrotask(() => tx.commit());
    } else {
      this.backend.delete(this.name, key);
      req._resolve(undefined);
    }
    return req;
  }

  clear(): FakeIDBRequest<undefined> {
    const req = new FakeIDBRequest<undefined>();
    const tx = this.getTransaction();
    if (tx) {
      tx.enqueueClear(this.name);
      tx.addRequest(req, undefined);
      queueMicrotask(() => tx.commit());
    } else {
      this.backend.clear(this.name);
      req._resolve(undefined);
    }
    return req;
  }

  openCursor(
    range?: IDBKeyRange | null,
    direction?: IDBCursorDirection
  ): FakeIDBRequest<IDBCursorWithValue | null> {
    const req = new FakeIDBRequest<IDBCursorWithValue | null>();
    const scanRange = rangeToScanRange(range);
    const dir = direction ?? "next";
    const source = { name: this.name, keyPath: this.keyPath, autoIncrement: this.autoIncrement };
    const cursor = new FakeIDBCursorWithValue(this.name, source, this.backend, scanRange, dir, req);
    queueMicrotask(() => {
      if (cursor.finished) req._resolve(null);
      else req._resolve(cursor as unknown as IDBCursorWithValue);
    });
    return req;
  }
}

function rangeToScanRange(range?: IDBKeyRange | null): ScanRange {
  if (!range) return {};
  const r = range as { lower?: IDBValidKey; upper?: IDBValidKey; lowerOpen?: boolean; upperOpen?: boolean };
  return {
    lower: r.lower,
    upper: r.upper,
    lowerOpen: r.lowerOpen,
    upperOpen: r.upperOpen,
  };
}
