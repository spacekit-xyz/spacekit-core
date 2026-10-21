/**
 * An app's private data for one viewer, kept on the storage node under the
 * viewer's own DID.
 *
 * Everything goes through the node's document API with an app-scoped token
 * (`POST /api/auth/delegate` without `act_as`), which the node only accepts for
 * `app_<appId>_*` collections in the token holder's namespace. Documents are
 * visible to their owner alone, so nobody else (not the publisher, not other
 * viewers) can list or read them.
 *
 * Collections (after the app prefix `app_<appId>_`, plus the plugin install
 * scope when there is one):
 *   sk-kv     records: one document per key, `{ k, v }`
 *   sk-blob   blob manifests, id = cid: `{ v, size, sha256, chunks, chunkBytes }`
 *   sk-chunk  blob bytes, id = `<cid>.<n>`: `{ b64 }`
 *
 * Blobs are split into chunks because the node caps JSON bodies at 1 MiB;
 * chunks are written first and the manifest last, so a blob exists only once
 * it is complete. Blob ids are content addresses (`skb1-<sha256 hex>`), and
 * reads verify the hash.
 */

import { bytesToBase64, base64ToBytes, sha256Hex } from "./viewerCrypto.js";

export const VIEWER_KV_COLLECTION = "sk-kv";
export const VIEWER_BLOB_COLLECTION = "sk-blob";
export const VIEWER_CHUNK_COLLECTION = "sk-chunk";
/** Collection names the `documents` module may not use, so apps cannot bypass the store. */
export const VIEWER_RESERVED_PREFIX = "sk-";

/** Raw bytes per chunk: base64 makes it 960 KiB, under the node's 1 MiB JSON limit. */
export const DEFAULT_CHUNK_BYTES = 720 * 1024;
const CID_PREFIX = "skb1-";
const MAX_ATTEMPTS = 4;

export interface ViewerStoreOptions {
  /** Storage node origin (or the website API's `/api/storage` proxy). */
  storageOrigin: string;
  /** `app_<appId>_` or `app_<appId>_<installScope>_`. */
  collectionPrefix: string;
  /** Complete `Authorization` value for the viewer's app-scoped token, or null. */
  authorization(): Promise<string | null>;
  fetchImpl?: typeof fetch;
  chunkBytes?: number;
  /** Parallel chunk requests. Default 3. */
  concurrency?: number;
  /** Delay before retry `n` (1-based) after a 429 or 5xx. Default 1s, 2s, 4s. */
  retryDelayMs?(attempt: number, retryAfterSeconds: number | null): number;
}

export interface BlobManifest {
  v: 1;
  size: number;
  sha256: string;
  chunks: number;
  chunkBytes: number;
  createdAt: string;
}

interface StorageDocument {
  id: string;
  data: unknown;
}

function trimSlash(url: string): string {
  return url.replace(/\/+$/, "");
}

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export function isBlobCid(cid: string): boolean {
  return /^skb1-[0-9a-f]{64}$/.test(cid);
}

async function mapLimit<T, R>(items: T[], limit: number, fn: (item: T, i: number) => Promise<R>): Promise<R[]> {
  const out = new Array<R>(items.length);
  let next = 0;
  const workers = Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, async () => {
    while (next < items.length) {
      const i = next++;
      out[i] = await fn(items[i], i);
    }
  });
  await Promise.all(workers);
  return out;
}

export class StorageRequestError extends Error {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message);
    this.name = "StorageRequestError";
  }
}

export class ViewerDataStore {
  private readonly base: string;
  private readonly prefix: string;
  private readonly opts: ViewerStoreOptions;
  private readonly doFetch: typeof fetch;
  private kv: Map<string, unknown> | null = null;
  private hydrating: Promise<void> | null = null;

  constructor(options: ViewerStoreOptions) {
    this.opts = options;
    this.base = trimSlash(options.storageOrigin);
    this.prefix = options.collectionPrefix;
    this.doFetch = options.fetchImpl ?? ((...args: Parameters<typeof fetch>) => fetch(...args));
  }

  collection(name: string): string {
    return `${this.prefix}${name}`;
  }

  private docUrl(collection: string, id?: string): string {
    const c = encodeURIComponent(this.collection(collection));
    return id === undefined
      ? `${this.base}/api/documents/${c}`
      : `${this.base}/api/documents/${c}/${encodeURIComponent(id)}`;
  }

  private async request(method: string, url: string, body?: unknown): Promise<Response> {
    for (let attempt = 1; ; attempt++) {
      const authorization = await this.opts.authorization();
      if (!authorization) throw new StorageRequestError("Sign in to use your SpaceKit storage", 401);
      let res: Response;
      try {
        res = await this.doFetch(url, {
          method,
          headers: {
            Authorization: authorization,
            ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
          },
          body: body !== undefined ? JSON.stringify(body) : undefined,
        });
      } catch (err) {
        if (attempt >= MAX_ATTEMPTS) throw new StorageRequestError(`Could not reach storage: ${String(err)}`, 0);
        await sleep(this.retryDelay(attempt, null));
        continue;
      }
      const retryable = res.status === 429 || res.status >= 500;
      if (!retryable || attempt >= MAX_ATTEMPTS) return res;
      const ra = Number(res.headers.get("retry-after"));
      await sleep(this.retryDelay(attempt, Number.isFinite(ra) && ra > 0 ? ra : null));
    }
  }

  private retryDelay(attempt: number, retryAfter: number | null): number {
    if (this.opts.retryDelayMs) return this.opts.retryDelayMs(attempt, retryAfter);
    return retryAfter !== null ? Math.min(retryAfter, 30) * 1000 : 1000 * 2 ** (attempt - 1);
  }

  private async fail(res: Response, what: string): Promise<never> {
    const text = await res.text().catch(() => "");
    let detail = text;
    try {
      const json = JSON.parse(text) as { error?: unknown };
      if (json?.error) detail = String(json.error);
    } catch {
      /* not JSON */
    }
    throw new StorageRequestError(`${what} failed (${res.status})${detail ? `: ${detail}` : ""}`, res.status);
  }

  private async getDoc(collection: string, id: string): Promise<unknown | undefined> {
    const res = await this.request("GET", this.docUrl(collection, id));
    if (res.status === 404) return undefined;
    if (!res.ok) return this.fail(res, "Storage read");
    const json = (await res.json()) as { document?: StorageDocument };
    return json.document ? json.document.data : undefined;
  }

  private async putDoc(collection: string, id: string, data: unknown): Promise<void> {
    const res = await this.request("PUT", this.docUrl(collection, id), data);
    if (!res.ok) await this.fail(res, "Storage write");
    // Drain the echoed document so the connection can be reused.
    await res.arrayBuffer().catch(() => undefined);
  }

  private async deleteDoc(collection: string, id: string): Promise<void> {
    const res = await this.request("DELETE", this.docUrl(collection, id));
    if (!res.ok && res.status !== 404) await this.fail(res, "Storage delete");
  }

  private async listDocs(collection: string): Promise<StorageDocument[]> {
    const res = await this.request("GET", this.docUrl(collection));
    if (res.status === 404) return [];
    if (!res.ok) return this.fail(res, "Storage list");
    const json = (await res.json()) as { documents?: StorageDocument[] };
    return json.documents ?? [];
  }

  // ---- Records ----

  /** Load every record once (one request); later reads are served from memory. */
  hydrate(force = false): Promise<void> {
    if (force) this.hydrating = null;
    this.hydrating ??= (async () => {
      const docs = await this.listDocs(VIEWER_KV_COLLECTION);
      const next = new Map<string, unknown>();
      for (const doc of docs) {
        const data = doc.data as { k?: unknown; v?: unknown } | null;
        const key = data && typeof data.k === "string" ? data.k : doc.id;
        next.set(key, data && "v" in data ? data.v : null);
      }
      this.kv = next;
    })().catch((err) => {
      this.hydrating = null;
      throw err;
    });
    return this.hydrating;
  }

  private async records(): Promise<Map<string, unknown>> {
    if (!this.kv) await this.hydrate();
    return this.kv!;
  }

  async getRecord(key: string): Promise<unknown> {
    const kv = await this.records();
    return kv.has(key) ? kv.get(key) : null;
  }

  async putRecord(key: string, value: unknown): Promise<void> {
    if (!key) throw new Error("storage: missing key");
    const kv = await this.records();
    await this.putDoc(VIEWER_KV_COLLECTION, key, { k: key, v: value === undefined ? null : value });
    kv.set(key, value === undefined ? null : value);
  }

  async deleteRecord(key: string): Promise<void> {
    const kv = await this.records();
    await this.deleteDoc(VIEWER_KV_COLLECTION, key);
    kv.delete(key);
  }

  async listKeys(prefix = ""): Promise<string[]> {
    const kv = await this.records();
    return [...kv.keys()].filter((k) => k.startsWith(prefix));
  }

  // ---- Blobs ----

  async putBlob(data: Blob | ArrayBuffer | Uint8Array): Promise<{ cid: string; size: number }> {
    const bytes =
      data instanceof Uint8Array ? data : data instanceof ArrayBuffer ? new Uint8Array(data) : new Uint8Array(await data.arrayBuffer());
    const sha256 = await sha256Hex(bytes);
    const cid = `${CID_PREFIX}${sha256}`;
    const existing = (await this.getDoc(VIEWER_BLOB_COLLECTION, cid)) as BlobManifest | undefined;
    if (existing && existing.size === bytes.length) return { cid, size: bytes.length };

    const chunkBytes = this.opts.chunkBytes ?? DEFAULT_CHUNK_BYTES;
    const chunks = Math.max(1, Math.ceil(bytes.length / chunkBytes));
    const indexes = Array.from({ length: chunks }, (_, i) => i);
    await mapLimit(indexes, this.opts.concurrency ?? 3, (i) =>
      this.putDoc(VIEWER_CHUNK_COLLECTION, `${cid}.${i}`, {
        b64: bytesToBase64(bytes.subarray(i * chunkBytes, Math.min(bytes.length, (i + 1) * chunkBytes))),
      }),
    );
    const manifest: BlobManifest = {
      v: 1,
      size: bytes.length,
      sha256,
      chunks,
      chunkBytes,
      createdAt: new Date().toISOString(),
    };
    await this.putDoc(VIEWER_BLOB_COLLECTION, cid, manifest);
    return { cid, size: bytes.length };
  }

  async getBlobManifest(cid: string): Promise<BlobManifest | null> {
    if (!isBlobCid(cid)) throw new Error(`Not a SpaceKit blob id: ${cid}`);
    const m = (await this.getDoc(VIEWER_BLOB_COLLECTION, cid)) as BlobManifest | undefined;
    return m && m.v === 1 ? m : null;
  }

  async getBlob(cid: string): Promise<Blob> {
    const manifest = await this.getBlobManifest(cid);
    if (!manifest) throw new StorageRequestError(`Blob not found: ${cid}`, 404);
    const indexes = Array.from({ length: manifest.chunks }, (_, i) => i);
    const parts = await mapLimit(indexes, this.opts.concurrency ?? 3, async (i) => {
      const doc = (await this.getDoc(VIEWER_CHUNK_COLLECTION, `${cid}.${i}`)) as { b64?: string } | undefined;
      if (!doc || typeof doc.b64 !== "string") throw new StorageRequestError(`Blob ${cid} is missing chunk ${i}`, 404);
      return base64ToBytes(doc.b64);
    });
    const out = new Uint8Array(manifest.size);
    let offset = 0;
    for (const part of parts) {
      if (offset + part.length > out.length) throw new Error(`Blob ${cid} is larger than its manifest`);
      out.set(part, offset);
      offset += part.length;
    }
    if (offset !== manifest.size || (await sha256Hex(out)) !== manifest.sha256) {
      throw new Error(`Blob ${cid} failed its integrity check`);
    }
    return new Blob([out]);
  }

  async deleteBlob(cid: string): Promise<boolean> {
    const manifest = await this.getBlobManifest(cid);
    if (!manifest) return false;
    // Manifest first: a half-deleted blob then reads as absent, not corrupt.
    await this.deleteDoc(VIEWER_BLOB_COLLECTION, cid);
    const indexes = Array.from({ length: manifest.chunks }, (_, i) => i);
    await mapLimit(indexes, this.opts.concurrency ?? 3, (i) => this.deleteDoc(VIEWER_CHUNK_COLLECTION, `${cid}.${i}`));
    return true;
  }
}
