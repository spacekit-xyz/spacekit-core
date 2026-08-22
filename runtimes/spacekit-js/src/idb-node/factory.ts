/**
 * IDBFactory: open and deleteDatabase (pure TS WAL backend).
 */

import { readFileSync, writeFileSync, existsSync, unlinkSync, mkdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { WalBackend } from "./wal-backend.js";
import { FakeIDBDatabase } from "./database.js";
import { FakeIDBOpenDBRequest } from "./request.js";

const DEFAULT_BASE_PATH = join(typeof process !== "undefined" ? process.cwd() : ".", ".spacekit-idb");

function readVersionFile(dbPath: string): number {
  const vPath = dbPath + ".version";
  if (!existsSync(vPath)) return 0;
  try {
    return parseInt(readFileSync(vPath, "utf8"), 10) || 0;
  } catch {
    return 0;
  }
}

function writeVersionFile(dbPath: string, version: number): void {
  const dir = dirname(dbPath);
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
  writeFileSync(dbPath + ".version", String(version));
}

function readSchemaFile(dbPath: string): Array<{ name: string; keyPath: string | string[] | null; autoIncrement: boolean }> {
  const sPath = dbPath + ".schema.json";
  if (!existsSync(sPath)) return [];
  try {
    const json = readFileSync(sPath, "utf8");
    const arr = JSON.parse(json);
    return Array.isArray(arr) ? arr : [];
  } catch {
    return [];
  }
}

function writeSchemaFile(dbPath: string, schema: Array<{ name: string; keyPath: string | string[] | null; autoIncrement: boolean }>): void {
  const dir = dirname(dbPath);
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
  writeFileSync(dbPath + ".schema.json", JSON.stringify(schema));
}

export function createIDBFactory(basePath: string = DEFAULT_BASE_PATH): IDBFactory {
  return {
    open(name: string, version?: number): IDBOpenDBRequest {
      const req = new FakeIDBOpenDBRequest();
      const requestedVersion = version ?? 1;
      const dbPath = join(basePath, name);
      const storedVersion = readVersionFile(dbPath);
      const backend = new WalBackend(dbPath);
      const db = new FakeIDBDatabase(name, requestedVersion, backend);

      if (requestedVersion > storedVersion) {
        queueMicrotask(() => {
          try {
            if (req.onupgradeneeded) {
              const ev = new Event("upgradeneeded") as IDBVersionChangeEvent;
              Object.defineProperty(ev, "newVersion", { value: requestedVersion, writable: false });
              Object.defineProperty(ev, "oldVersion", { value: storedVersion, writable: false });
              req.onupgradeneeded(ev);
            }
            writeVersionFile(dbPath, requestedVersion);
            writeSchemaFile(dbPath, db.getStoresSchema());
          } catch (e) {
            req._reject(e instanceof Error ? e : new Error(String(e)));
            return;
          }
          req._resolve(db as unknown as IDBDatabase);
        });
      } else {
        const schema = readSchemaFile(dbPath);
        if (schema.length > 0) db.setStoresFromSchema(schema);
        queueMicrotask(() => req._resolve(db as unknown as IDBDatabase));
      }
      return req as unknown as IDBOpenDBRequest;
    },

    deleteDatabase(name: string): IDBOpenDBRequest {
      const req = new FakeIDBOpenDBRequest();
      const dbPath = join(basePath, name);
      queueMicrotask(() => {
        try {
          const walPath = dbPath.endsWith(".wal") ? dbPath : dbPath + ".wal";
          if (existsSync(walPath)) unlinkSync(walPath);
          if (existsSync(dbPath + ".version")) unlinkSync(dbPath + ".version");
          if (existsSync(dbPath + ".schema.json")) unlinkSync(dbPath + ".schema.json");
          req._resolve(undefined as unknown as IDBDatabase);
        } catch (e) {
          req._reject(e instanceof Error ? e : new Error(String(e)));
        }
      });
      return req as unknown as IDBOpenDBRequest;
    },
  } as unknown as IDBFactory;
}
