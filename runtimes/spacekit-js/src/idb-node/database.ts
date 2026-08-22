/**
 * IDBDatabase: stores, transactions, createObjectStore.
 */

import type { WalBackend } from "./wal-backend.js";
import { IDBTransactionImpl } from "./transaction.js";
import { FakeIDBObjectStore } from "./object-store.js";

export interface StoreSchema {
  name: string;
  keyPath: string | string[] | null;
  autoIncrement: boolean;
}

export class FakeIDBDatabase {
  private stores = new Map<string, StoreSchema>();

  constructor(
    public readonly name: string,
    public readonly version: number,
    private backend: WalBackend
  ) {}

  createObjectStore(name: string, options?: { keyPath?: string | string[] | null; autoIncrement?: boolean }): FakeIDBObjectStore {
    if (this.stores.has(name)) throw new DOMException("Object store already exists", "ConstraintError");
    const keyPath = options?.keyPath ?? null;
    const autoIncrement = options?.autoIncrement ?? false;
    this.stores.set(name, { name, keyPath, autoIncrement });
    return new FakeIDBObjectStore(this.backend, name, keyPath, autoIncrement, () => null);
  }

  transaction(storeNames: string | string[], mode: IDBTransactionMode): IDBTransaction {
    const names = typeof storeNames === "string" ? [storeNames] : storeNames;
    for (const n of names) {
      if (!this.stores.has(n)) throw new DOMException(`Object store '${n}' not found`, "NotFoundError");
    }
    const impl = new IDBTransactionImpl(mode, this.backend, (ops) => {
      for (const op of ops) {
        if (op.op === "put" && op.key !== undefined && op.value !== undefined) {
          this.backend.put(op.store, op.key, op.value);
        } else if (op.op === "del" && op.key !== undefined) {
          this.backend.delete(op.store, op.key);
        } else if (op.op === "clear") {
          this.backend.clear(op.store);
        }
      }
    });
    const self = this;
    const wrapper = {
      objectStore(name: string): FakeIDBObjectStore {
        if (!names.includes(name)) throw new DOMException(`Object store '${name}' not in transaction`, "NotFoundError");
        const schema = self.stores.get(name)!;
        return new FakeIDBObjectStore(self.backend, name, schema.keyPath, schema.autoIncrement, () => wrapper as unknown as IDBTransactionImpl);
      },
      get mode() {
        return impl.mode;
      },
      get complete() {
        return !impl.abortedFlag;
      },
      oncomplete: null as ((ev: Event) => void) | null,
      onerror: null as ((ev: Event) => void) | null,
      enqueuePut: impl.enqueuePut.bind(impl),
      enqueueDelete: impl.enqueueDelete.bind(impl),
      enqueueClear: impl.enqueueClear.bind(impl),
      addRequest: impl.addRequest.bind(impl),
      commit() {
        impl.commit();
        if (wrapper.oncomplete) wrapper.oncomplete(new Event("complete"));
      },
    };
    return wrapper as unknown as IDBTransaction;
  }

  get objectStoreNames(): DOMStringList {
    const list = Array.from(this.stores.keys());
    return {
      contains: (name: string) => list.includes(name),
      item: (i: number) => list[i] ?? null,
      get length() {
        return list.length;
      },
    } as DOMStringList;
  }

  /** For factory: load schema when opening without upgrade. */
  setStoresFromSchema(schema: StoreSchema[]): void {
    for (const s of schema) {
      this.stores.set(s.name, s);
    }
  }

  /** For factory: persist schema after upgrade. */
  getStoresSchema(): StoreSchema[] {
    return Array.from(this.stores.values());
  }

  close(): void {
    // no-op for this backend
  }
}
