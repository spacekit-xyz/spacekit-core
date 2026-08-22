/**
 * Type declarations for the optional `fake-indexeddb` package.
 *
 * This package provides a spec-compliant in-memory IndexedDB
 * implementation for Node.js and Bun. It is an optional dependency –
 * only required when running SpaceKit outside a browser.
 */
declare module "fake-indexeddb" {
  export const indexedDB: IDBFactory;
  export const IDBDatabase: typeof globalThis.IDBDatabase;
  export const IDBTransaction: typeof globalThis.IDBTransaction;
  export const IDBRequest: typeof globalThis.IDBRequest;
  export const IDBObjectStore: typeof globalThis.IDBObjectStore;
  export const IDBIndex: typeof globalThis.IDBIndex;
  export const IDBCursor: typeof globalThis.IDBCursor;
  export const IDBCursorWithValue: typeof globalThis.IDBCursorWithValue;
  export const IDBKeyRange: typeof globalThis.IDBKeyRange;
}
