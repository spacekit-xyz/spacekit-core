/**
 * Platform detection and runtime polyfill setup for SpaceKit.
 *
 * Supports three deployment targets:
 *  - Browser (default, no setup needed)
 *  - Node.js (requires `fake-indexeddb` polyfill for IndexedDB)
 *  - Bun (requires `fake-indexeddb` polyfill for IndexedDB)
 */

/* ───────────────────────── Detection ───────────────────────── */

export type SpacekitRuntime = "browser" | "node" | "bun";

/**
 * Detect the current JavaScript runtime.
 */
export function detectRuntime(): SpacekitRuntime {
  // Bun exposes a global `Bun` object
  if (typeof globalThis !== "undefined" && "Bun" in globalThis) {
    return "bun";
  }
  // Node.js exposes `process.versions.node`
  if (
    typeof process !== "undefined" &&
    process.versions != null &&
    typeof process.versions.node === "string"
  ) {
    return "node";
  }
  return "browser";
}

/**
 * Returns true when running outside a browser (Node.js or Bun).
 */
export function isServerRuntime(): boolean {
  const rt = detectRuntime();
  return rt === "node" || rt === "bun";
}

/* ───────────────────────── Polyfills ───────────────────────── */

let polyfillsInstalled = false;

/**
 * Install required polyfills for server-side runtimes.
 *
 * Currently this installs a spec-compliant IndexedDB implementation
 * (`fake-indexeddb`) into the global scope so that all SpaceKit
 * storage adapters work without modification.
 *
 * Safe to call multiple times – subsequent calls are a no-op.
 * Safe to call in the browser – detects the environment and skips.
 */
export async function installPolyfills(): Promise<void> {
  if (polyfillsInstalled) return;

  const runtime = detectRuntime();
  if (runtime === "browser") {
    polyfillsInstalled = true;
    return;
  }

  // Install IndexedDB polyfill for Node.js / Bun
  if (typeof globalThis.indexedDB === "undefined") {
    const useWal =
      typeof process !== "undefined" &&
      process.env?.SPACEKIT_IDB_BACKEND === "wal";

    if (useWal) {
      try {
        const { createIDBFactory } = await import("./idb-node/index.js");
        const basePath =
          typeof process !== "undefined" && process.env?.SPACEKIT_IDB_PATH
            ? process.env.SPACEKIT_IDB_PATH
            : ".spacekit-idb";
        (globalThis as Record<string, unknown>).indexedDB = createIDBFactory(basePath);
      } catch (e) {
        throw new Error(
          `[spacekit] WAL IndexedDB backend failed: ${e instanceof Error ? e.message : String(e)}`
        );
      }
    } else {
      try {
        // fake-indexeddb v6+ exports individual globals
        const fakeIdb = await import("fake-indexeddb");
        const g = globalThis as Record<string, unknown>;
        g.indexedDB = fakeIdb.indexedDB;
        g.IDBDatabase = fakeIdb.IDBDatabase;
        g.IDBTransaction = fakeIdb.IDBTransaction;
        g.IDBRequest = fakeIdb.IDBRequest;
        g.IDBObjectStore = fakeIdb.IDBObjectStore;
        g.IDBIndex = fakeIdb.IDBIndex;
        g.IDBCursor = fakeIdb.IDBCursor;
        g.IDBCursorWithValue = fakeIdb.IDBCursorWithValue;
        g.IDBKeyRange = fakeIdb.IDBKeyRange;
      } catch {
        throw new Error(
          `[spacekit] IndexedDB polyfill not found.\n` +
            `Install it with:\n` +
            `  npm install fake-indexeddb\n` +
            `or:\n` +
            `  bun add fake-indexeddb\n` +
            `Or use persistent WAL backend: SPACEKIT_IDB_BACKEND=wal`
        );
      }
    }
  }

  polyfillsInstalled = true;
}

/**
 * Reset polyfill state (for testing purposes).
 */
export function resetPolyfillState(): void {
  polyfillsInstalled = false;
}
