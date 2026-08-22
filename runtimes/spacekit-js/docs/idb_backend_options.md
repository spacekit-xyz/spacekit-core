# IndexedDB backend options (Node.js / Bun)

This doc outlines how to replace the current **in-memory** IndexedDB polyfill (`fake-indexeddb`) with persistent backends, and how the **spacekit-storage-node** (Rust) fits in.

## Current state

- **Browser**: Native IndexedDB.
- **Node.js / Bun**: `fake-indexeddb` (in-memory) by default, or **pure TS WAL backend** when opted in (see below).

## Pure TypeScript WAL backend (implemented)

A full IndexedDB-compatible implementation lives in **`src/idb-node/`** with no SQLite and no native addons:

1. **Key encoding** (`key-codec.ts`): Encodes keys so IndexedDB ordering (number &lt; Date &lt; string &lt; binary &lt; array) is preserved as lexicographic byte order. Enables range queries and cursors without a query engine.
2. **Storage backend** (`wal-backend.ts`): Per-store sorted in-memory arrays + append-only WAL file. Replay on open; optional `compact()` to rewrite the WAL.
3. **Transaction** (`transaction.ts`): Buffered puts/deletes/clears; commit applies to backend and resolves requests.
4. **API layer**: `request.ts`, `key-range.ts`, `cursor.ts`, `object-store.ts`, `database.ts`, `factory.ts` implement IDBFactory, IDBDatabase, IDBObjectStore, IDBCursor, etc.

**Enable**: Set `SPACEKIT_IDB_BACKEND=wal` (and optionally `SPACEKIT_IDB_PATH=/path/to/dir`; default `.spacekit-idb`) before calling `installPolyfills()`. Data is stored as `<path>/<dbName>.wal`, `<path>/<dbName>.version`, and `<path>/<dbName>.schema.json`.

## Option A: Create new portable IndexedDB implementation that can be used with SQLite-backed IndexedDB (TypeScript)

**Goal**: Full IndexedDB API in TS, without SQLite, so existing code (blockstore, session, storage adapter, etc.) works unchanged with **real persistence** on disk.

- **Node**: `better-sqlite3` (optional dependency).
- **Bun**: `bun:sqlite` (built-in).
- **Required surface**: `IDBFactory`, `IDBDatabase`, `IDBObjectStore`, `IDBTransaction`, **`IDBCursor`**, **`IDBKeyRange`**, request/event simulation. Key encoding (number, string, binary) must preserve IndexedDB ordering.
- **Placement**: New package or `src/idb-sqlite/` in spacekit-js; install as optional backend when user wants persistence (e.g. `SPACEKIT_IDB_BACKEND=sqlite`).

This is **independent** of the Rust storage-node. It gives local, single-process, file-based persistence for VM state, blocks, and session.

## spacekit-storage-node (Rust): what it is and what it is not

- **What it is**:
  - A **library** (`spacekit_storage_node`) + **standalone binary** (`spacekit-storage-node`).
  - Persistent storage: custom **JSON + WAL + encrypted backups** (no SQLite by default; optional `rusqlite` feature for analytics).
  - HTTP API: `/api/documents/{collection}/{id}`, list, query; file upload/download; DID auth; quantum crypto.
  - P2P (libp2p), fact storage, NFT, etc.
- **What it is not**:
  - It does **not** implement the IndexedDB API. Its persistence is document/collection/key-value shaped, not IDB object stores/cursors/key ranges.
  - It does **not** target WASM: tokio, `std::fs`, libp2p, OQS, etc. make it a native/server crate. WASM would require a separate, slim crate.

So we **don’t** “replace the polyfill with the storage-node” in the sense of implementing IDB in Rust. We **use** the storage-node in two ways: **embedded binary** and (optionally) **future native addon**.

## Using the storage-node as an embedded binary

- **Idea**: Ship the `spacekit-storage-node` binary with the app; start it as a **subprocess** from Node/Bun (e.g. from a launcher or from spacekit-js).
- **How spacekit-js talks to it**: Use the existing **StorageNodeAdapter** (and sync/export flows) with `baseUrl: "http://127.0.0.1:3030"` (or the port the binary uses). No change to the IDB layer.
- **Result**:
  - **Local IDB**: Still the polyfill (in-memory) or, once implemented, the SQLite-backed IDB (Option A) for VM state, blockstore, session.
  - **Durable / sync**: Documents and sync go to the embedded storage-node over HTTP (quantum-safe, P2P, etc.).
- **Persistence**: The Rust node’s data dir (e.g. `./storage_data`) holds the durable state; the JS process’s IDB (polyfill or SQLite) holds local VM state and can sync to that node.

So “embedded” here means **same deployable unit** (app + binary), not “IDB implemented in Rust”.

## Possible future: native addon (Rust → Node)

- **Idea**: A **Rust crate** (new or under `spacekit-storage-node`) that exposes a **minimal IDB-like or KV API** to Node via **napi-rs** (or Neon):
  - e.g. `open(db_name, path)`, `get(store, key)`, `put(store, key, value)`, `delete(store, key)`, `cursor_open(store, range)`, `cursor_next(cursor)`, etc.
- **Backend**: Reuse the storage-node’s **Database** (JSON + WAL) or a slim key-value layer on top of it. No HTTP.
- **TypeScript**: The same IDB polyfill (IDBFactory, IDBDatabase, …) would call this addon instead of SQLite when e.g. `SPACEKIT_IDB_BACKEND=native` and the addon is installed.
- **Benefit**: Single process, no subprocess, same persistence and crypto as the storage-node. **Cost**: New Rust API, build/ABI for Node, and maintaining the addon.

## WASM

- The **current** storage-node does **not** compile to WASM (no `wasm32` target, heavy deps).
- A **WASM-based** persistent store would be a **new, slim** Rust crate:
  - Target `wasm32-unknown-unknown` or `wasm32-wasi`.
  - Expose a small API (e.g. get/put/delete/cursor) that JS can call via wasm-bindgen.
  - Persistence: in-memory only, or WASI file I/O if targeting `wasm32-wasi` (e.g. in Node/Bun).
- That could be used from browser or Node/Bun as a **third** backend (memory, SQLite, WASM), but it’s a separate project from the existing storage-node.

## Recommended order

1. **Short term**: Implement **Option A** (SQLite-backed IDB in TypeScript) with cursor + key range + key encoding. Keeps spacekit-js self-contained, no Rust build, works on Node and Bun with real local persistence.
2. **Embedded binary**: Use the **existing** storage-node binary as a subprocess and the **existing** StorageNodeAdapter for sync and remote-style storage; keep IDB (polyfill or SQLite) for local VM state.
3. **Later** (if needed): **Native addon** that backs the same IDB polyfill with the Rust Database for a single-process, “one stack” story.
4. **WASM**: Only if you need the same persistence code path in browser and server; then design a small dedicated crate, not the full storage-node.

## Summary

| Backend              | Persistence     | Where        | When to use                          |
|----------------------|-----------------|-------------|--------------------------------------|
| fake-indexeddb       | In-memory       | Node/Bun    | Default, tests, ephemeral             |
| SQLite (Option A)    | File (SQLite)   | Node/Bun TS | Local persistent VM/block/session     |
| Storage-node (HTTP)  | Rust node dir   | Any         | Sync, documents, quantum-safe, P2P   |
| Native addon (future)| Rust DB (WAL)   | Node        | Single process, same stack as node   |
| WASM (future)        | Memory / WASI   | Browser/Node| Shared persistence logic in WASM      |

The storage-node is the right place for **embedded binary** and, if you want one-process persistence with the same Rust stack, for a **native IDB-like addon**. It does **not** replace the need for a proper **SQLite-backed IDB implementation in TypeScript** (Option A) for the existing IDB API surface and cursor/key-range semantics.
