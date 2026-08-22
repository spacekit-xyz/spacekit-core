/**
 * Pure TypeScript IndexedDB implementation (WAL + sorted in-memory).
 * Node/Bun only. Use createIDBFactory() and assign to globalThis.indexedDB
 * when SPACEKIT_IDB_BACKEND=wal.
 */

export { createIDBFactory } from "./factory.js";
export { WalBackend } from "./wal-backend.js";
export { encodeKey, decodeKey, compareEncodedKeys, KeyType } from "./key-codec.js";
export type { ScanRange } from "./wal-backend.js";
