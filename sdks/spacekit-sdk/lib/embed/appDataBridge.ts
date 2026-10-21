import { priceFromParams } from "../astra.js";
import type { EmbeddedSdkBridge } from "./bridge.js";
import { parseManifestPermissions } from "./capabilities.js";
import { LOCAL_STORAGE_SHIM_PREFIX } from "./protocol.js";
import { decryptForViewer, encryptForViewer } from "./viewerCrypto.js";
import { VIEWER_RESERVED_PREFIX, ViewerDataStore } from "./viewerStore.js";
import type {
  AppCredentials,
  AppManifest,
  EmbeddedHttpHandler,
  EmbedHostServices,
  MarketplacePurchaseRecord,
  SubscriptionPaymentRequest,
  SubscriptionStatus,
} from "./types.js";

const bridgeCache = new Map<string, AppDataSdkBridge>();

const RESERVED_PREFIX = "__";
const SUBSCRIPTIONS_COLLECTION = "__subscriptions";
const DEFAULT_PERIOD_DAYS = 30;
const DAY_MS = 24 * 60 * 60 * 1000;

interface SubscriptionRecord {
  buyerDid: string;
  expiresAt: number;
  /** ASTRA wei, decimal string. */
  amountWei?: string;
  periodDays: number;
  txHash?: string;
  updatedAt: string;
  verified?: boolean;
}

function trimSlash(url: string): string {
  return url.replace(/\/$/, "");
}

function normalizeDid(did: string): string {
  return did.trim().toLowerCase();
}

interface StorageDocument<T = unknown> {
  id: string;
  data: T;
  collection?: string;
  owner_did?: string;
  created_at?: string;
  updated_at?: string;
}

function asBlob(value: unknown, what: string): Blob {
  if (value instanceof Blob) return value;
  if (value instanceof ArrayBuffer) return new Blob([value]);
  if (value instanceof Uint8Array) return new Blob([value.slice()]);
  throw new Error(`${what} requires a Blob`);
}

/**
 * Generic bridge for any app, with two data namespaces:
 *
 * - `documents.*` lives in the app **publisher's** namespace (shared app data:
 *   any viewer may get/put, only the publisher lists or deletes).
 * - `storage.*` is the **viewer's** private data for this app. Blobs
 *   (`putBlob`/`getBlob`/`deleteBlob`) always go to the storage node under the
 *   viewer's DID; records go there too when the manifest declares the `storage`
 *   permission, and otherwise stay in this device's localStorage as before. The
 *   app's `localStorage` shim (`__ls:` keys) always stays on the device.
 *   Viewer mode needs the host to return `viewerStorageAuthorization` from
 *   `getAppCredentials({ namespace: "viewer" })`.
 * - `crypto.encryptUpload` / `decryptBlob` encrypt with a per-file key wrapped
 *   by the host's `deriveAppKey` (see `viewerCrypto.ts`). The key-encryption
 *   key never enters the app frame.
 */
export class AppDataSdkBridge implements EmbeddedSdkBridge {
  private ownerDid: string | null = null;
  /** Manifest declares `storage`: records sync to the viewer's node namespace. */
  private syncRecords = false;
  private readonly viewerStores = new Map<string, ViewerDataStore>();
  private readonly appKeys = new Map<string, Promise<CryptoKey | null>>();
  private readonly appId: string;
  private readonly storageOrigin: string;
  private readonly services: EmbedHostServices;
  private readonly httpHandler: EmbeddedHttpHandler;
  /** Per-install infix for plugins (`i<hex>`), so installs never share data. */
  private readonly collectionScope: string | null;
  private pushToIframe: ((topic: string, msg: unknown) => void) | null = null;
  private legacyWarned = false;

  constructor(
    appId: string,
    storageOrigin: string,
    services: EmbedHostServices,
    httpHandler: EmbeddedHttpHandler,
    collectionScope?: string,
  ) {
    this.appId = appId.trim().toLowerCase();
    if (collectionScope !== undefined && !/^i[0-9a-f]{1,16}$/.test(collectionScope)) {
      throw new Error("collectionScope must be an install scope label (i + up to 16 hex)");
    }
    this.collectionScope = collectionScope ?? null;
    this.storageOrigin = storageOrigin;
    this.services = services;
    this.httpHandler = httpHandler;
  }

  setOwnerDid(did: string): void {
    this.ownerDid = did ? did.trim() : null;
  }

  setManifest(manifest: AppManifest): void {
    this.syncRecords = parseManifestPermissions(manifest?.permissions ?? []).capabilities.has("storage");
  }

  async ensureHydrated(): Promise<void> {
    this.ensureOwnerDid();
    // Refresh the viewer's records on every mount, so changes made on another
    // device show up when the app is reopened.
    const store = await this.recordStore();
    if (store) {
      await store.hydrate(true).catch((err) => {
        console.warn("[spacekit] could not load this app's records from storage:", err);
      });
    }
  }

  flush(): void {}

  setPushHandler(handler: (topic: string, msg: unknown) => void): void {
    this.pushToIframe = handler;
  }

  private ensureOwnerDid(): string {
    if (this.ownerDid) return this.ownerDid;
    throw new Error("App owner DID not configured (package missing creator_did)");
  }

  private scopedCollection(collection: string): string {
    const clean = collection.trim().replace(/[^a-zA-Z0-9_-]/g, "_") || "default";
    return this.collectionScope
      ? `app_${this.appId}_${this.collectionScope}_${clean}`
      : `app_${this.appId}_${clean}`;
  }

  private isOwnerViewer(): boolean {
    const viewer = this.services.getViewerDid();
    return Boolean(viewer && this.ownerDid && normalizeDid(viewer) === normalizeDid(this.ownerDid));
  }

  private assertOwner(): void {
    if (!this.isOwnerViewer()) {
      throw new Error("Owner access denied — sign in as the app owner");
    }
  }

  /** App-scoped credentials from the host, if it issues them. */
  private appCredentials(): Promise<AppCredentials | null> {
    const get = this.services.getAppCredentials;
    if (!get) return Promise.resolve(null);
    return get
      .call(this.services, {
        appId: this.appId,
        publisherDid: this.ownerDid,
        storageOrigin: this.storageOrigin,
      })
      .catch(() => null);
  }

  /** App-scoped token for the viewer's own namespace, or null. */
  private async viewerAuthorization(viewerKey: string): Promise<string | null> {
    const viewer = this.services.getViewerDid();
    if (!viewer || normalizeDid(viewer) !== viewerKey) return null; // signed out or switched
    const get = this.services.getAppCredentials;
    if (!get) return null;
    const creds = await get
      .call(this.services, {
        appId: this.appId,
        publisherDid: this.ownerDid,
        storageOrigin: this.storageOrigin,
        namespace: "viewer",
      })
      .catch(() => null);
    return creds?.viewerStorageAuthorization ?? null;
  }

  /** The signed-in viewer's store for this app (no network yet), or null when anonymous. */
  private viewerStore(): ViewerDataStore | null {
    const viewer = this.services.getViewerDid();
    if (!viewer) return null;
    const key = normalizeDid(viewer);
    let store = this.viewerStores.get(key);
    if (!store) {
      store = new ViewerDataStore({
        storageOrigin: this.storageOrigin,
        collectionPrefix: this.collectionScope
          ? `app_${this.appId}_${this.collectionScope}_`
          : `app_${this.appId}_`,
        authorization: () => this.viewerAuthorization(key),
      });
      this.viewerStores.set(key, store);
    }
    return store;
  }

  /** The viewer's store when the host can authorize it, else null. */
  private async usableViewerStore(): Promise<ViewerDataStore | null> {
    const viewer = this.services.getViewerDid();
    const store = this.viewerStore();
    if (!viewer || !store) return null;
    return (await this.viewerAuthorization(normalizeDid(viewer))) ? store : null;
  }

  /** Where `storage.*` records live for this call: the node, or null for this device. */
  private async recordStore(): Promise<ViewerDataStore | null> {
    return this.syncRecords ? this.usableViewerStore() : null;
  }

  private async blobStore(): Promise<ViewerDataStore> {
    const store = await this.usableViewerStore();
    if (store) return store;
    throw new Error(
      this.services.getViewerDid()
        ? "This host cannot store files for this app (no viewer storage credentials)"
        : "Sign in to store files",
    );
  }

  private appKey(): Promise<CryptoKey> {
    const viewer = this.services.getViewerDid();
    if (!viewer) return Promise.reject(new Error("Sign in to encrypt and decrypt files"));
    const derive = this.services.deriveAppKey;
    if (!derive) return Promise.reject(new Error("This host cannot encrypt files (no key service)"));
    const key = normalizeDid(viewer);
    let pending = this.appKeys.get(key);
    if (!pending) {
      pending = derive.call(this.services, { appId: this.appId, viewerDid: viewer.trim() }).catch((err) => {
        this.appKeys.delete(key);
        throw err;
      });
      this.appKeys.set(key, pending);
    }
    return pending.then((k) => {
      if (!k) {
        this.appKeys.delete(key);
        throw new Error("This host holds no key for the signed-in account");
      }
      return k;
    });
  }

  private async ownerFetch(path: string, init?: RequestInit): Promise<Response> {
    const ownerDid = this.ensureOwnerDid();
    const creds = await this.appCredentials();
    let authorization = creds?.storageAuthorization;
    if (!authorization) {
      // Legacy: a bare claim of the publisher DID. Storage nodes in strict
      // mode reject it; hosts should implement getAppCredentials.
      if (!this.legacyWarned) {
        this.legacyWarned = true;
        console.warn(
          "[spacekit] no app-scoped storage credentials from the host; falling back to a bare DID header",
        );
      }
      authorization = `DID ${ownerDid}`;
    }
    return fetch(`${trimSlash(this.storageOrigin)}${path}`, {
      ...init,
      headers: {
        "Content-Type": "application/json",
        ...(init?.headers ?? {}),
        Authorization: authorization,
      },
    });
  }

  private docPath(collection: string, id: string): string {
    return `/api/documents/${encodeURIComponent(collection)}/${encodeURIComponent(id)}`;
  }

  private collectionPath(collection: string): string {
    return `/api/documents/${encodeURIComponent(collection)}`;
  }

  private async getDocument(collection: string, id: string): Promise<StorageDocument | null> {
    const res = await this.ownerFetch(this.docPath(this.scopedCollection(collection), id));
    if (res.status === 404) return null;
    if (!res.ok) return null;
    const json = (await res.json()) as { document?: StorageDocument };
    return json.document ?? null;
  }

  private async putDocument(collection: string, id: string, data: unknown): Promise<StorageDocument> {
    const res = await this.ownerFetch(this.docPath(this.scopedCollection(collection), id), {
      method: "PUT",
      body: JSON.stringify(data),
    });
    if (!res.ok) {
      const body = await res.text().catch(() => "");
      throw new Error(body || `Failed to write document (${res.status})`);
    }
    return { id, data };
  }

  private async listDocuments(collection: string): Promise<StorageDocument[]> {
    const res = await this.ownerFetch(this.collectionPath(this.scopedCollection(collection)));
    if (!res.ok) throw new Error(`Failed to list documents (${res.status})`);
    const json = (await res.json()) as { documents?: StorageDocument[] };
    return json.documents ?? [];
  }

  private async deleteDocument(collection: string, id: string): Promise<void> {
    const res = await this.ownerFetch(this.docPath(this.scopedCollection(collection), id), {
      method: "DELETE",
    });
    if (!res.ok && res.status !== 404) {
      throw new Error(`Failed to delete document (${res.status})`);
    }
  }

  private async subscriptionStatus(): Promise<SubscriptionStatus> {
    const viewer = this.services.getViewerDid();
    if (!viewer) {
      return { active: false, expiresAt: null, viewerDid: null, reason: "not-signed-in" };
    }
    const doc = await this.getDocument(SUBSCRIPTIONS_COLLECTION, normalizeDid(viewer));
    const record = doc?.data as SubscriptionRecord | undefined;
    const expiresAt = Number(record?.expiresAt) || 0;
    return {
      active: expiresAt > Date.now(),
      expiresAt: expiresAt || null,
      viewerDid: viewer,
      amountWei: record?.amountWei,
      asset: "ASTRA",
      verified: record?.verified === true,
    };
  }

  private async subscribe(params: Record<string, unknown>): Promise<SubscriptionStatus> {
    const viewer = this.services.getViewerDid();
    if (!viewer) throw new Error("Sign in before subscribing");
    const publisherDid = this.ensureOwnerDid();
    const pay = this.services.requestSubscriptionPayment;
    if (!pay) throw new Error("Payments are not configured on this host");

    const amountWei = priceFromParams(params, "payments.subscribe").toString();
    const periodDays = Math.max(1, Math.round(Number(params.periodDays) || DEFAULT_PERIOD_DAYS));

    const req: SubscriptionPaymentRequest = {
      publisherDid,
      appId: this.appId,
      amountWei,
      asset: "ASTRA",
    };
    const record = this.services.recordSubscription;
    if (!record) {
      // Checked before paying: without a verifying service the record could not be stored.
      throw new Error("This host cannot record verified subscriptions");
    }
    const { txHash } = await pay(req);

    // A trusted service verifies the ASTRA payment on the chain and writes the
    // record; storage nodes refuse subscription records written by clients.
    const status = await record.call(this.services, {
      appId: this.appId,
      publisherDid,
      amountWei,
      periodDays,
      txHash,
    });

    const purchase: MarketplacePurchaseRecord = {
      buyerDid: viewer,
      appId: this.appId,
      txHash,
    };
    void this.services.recordMarketplacePurchase?.(purchase).catch(() => {});

    return status;
  }

  async handle(module: string, method: string, params: Record<string, unknown>): Promise<unknown> {
    const httpResult = this.httpHandler(
      module,
      method,
      params,
      (topic, msg) => this.pushToIframe?.(topic, msg),
      { appId: this.appId, appCredentials: () => this.appCredentials() },
    );
    if (httpResult) return httpResult;

    if (module === "identity") {
      if (method === "did") return this.services.getViewerDid();
      return this.services.handleIdentity(method, params);
    }

    if (module === "app") {
      if (method === "ready") return { ready: true, appId: this.appId };
      if (method === "isOwner") return this.isOwnerViewer();
      if (method === "ownerDid") return this.ownerDid;
    }

    if (module === "payments") {
      if (method === "status") return this.subscriptionStatus();
      if (method === "subscribe") return this.subscribe(params);
      if (method === "config") {
        return { publisherDid: this.ownerDid, currency: "ASTRA", decimals: 18, model: "subscription" };
      }
      if (method === "charge") return this.subscribe(params);
      throw new Error(`Unknown payments method: ${method}`);
    }

    if (module === "documents") {
      const collection = String(params.collection ?? "").trim();
      if (!collection) throw new Error("documents: missing collection");
      if (collection.startsWith(RESERVED_PREFIX) || collection.startsWith(VIEWER_RESERVED_PREFIX)) {
        throw new Error(`documents: collection "${collection}" is reserved`);
      }

      if (method === "get") {
        const doc = await this.getDocument(collection, String(params.id ?? ""));
        return doc ? doc.data : null;
      }
      if (method === "put") {
        const id = String(params.id ?? "").trim();
        if (!id) throw new Error("documents.put: missing id");
        await this.putDocument(collection, id, params.data ?? {});
        return { id };
      }
      if (method === "list") {
        this.assertOwner();
        const docs = await this.listDocuments(collection);
        return docs.map((d) => ({ id: d.id, data: d.data, updatedAt: d.updated_at }));
      }
      if (method === "delete") {
        this.assertOwner();
        await this.deleteDocument(collection, String(params.id ?? ""));
        return true;
      }
      throw new Error(`Unknown documents method: ${method}`);
    }

    if (module === "storage") return this.handleStorage(method, params);
    if (module === "crypto") return this.handleCrypto(method, params);

    if (module === "messaging" && method === "publish") {
      const topic = String(params.topic ?? "");
      const msg = params.msg;
      this.pushToIframe?.(topic, msg);
      return true;
    }

    throw new Error(`${module}.${method} not bridged`);
  }

  // ---- storage.* (viewer namespace) ----

  private localKey(key: string): string {
    const scope = this.collectionScope ? `${this.collectionScope}:` : "";
    return `spacekit:appdata:${this.appId}:${scope}${key}`;
  }

  private localGet(key: string): unknown {
    try {
      const raw = localStorage.getItem(this.localKey(key));
      return raw ? JSON.parse(raw) : null;
    } catch {
      return null;
    }
  }

  private localSet(key: string, value: unknown): void {
    if (value === null || value === undefined) localStorage.removeItem(this.localKey(key));
    else localStorage.setItem(this.localKey(key), JSON.stringify(value));
  }

  private localList(prefix: string): string[] {
    const needle = this.localKey(prefix);
    const strip = this.localKey("").length;
    const out: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const k = localStorage.key(i);
      if (k?.startsWith(needle)) out.push(k.slice(strip));
    }
    return out;
  }

  private async handleStorage(method: string, params: Record<string, unknown>): Promise<unknown> {
    const key = String(params.key ?? "");
    // The app's localStorage shim is a per-device cache; it never syncs.
    const local = key.startsWith(LOCAL_STORAGE_SHIM_PREFIX);

    if (method === "ready") {
      const store = await this.recordStore();
      return { ready: true, appId: this.appId, synced: Boolean(store) };
    }
    if (method === "get" || method === "getRecord") {
      const store = local ? null : await this.recordStore();
      return store ? store.getRecord(key) : this.localGet(key);
    }
    if (method === "set" || method === "putRecord") {
      const value = params.value;
      const store = local ? null : await this.recordStore();
      if (!store) this.localSet(key, value);
      else if (value === null || value === undefined) await store.deleteRecord(key);
      else await store.putRecord(key, value);
      return method === "putRecord" ? key : true;
    }
    if (method === "delete" || method === "deleteRecord") {
      const store = local ? null : await this.recordStore();
      if (store) await store.deleteRecord(key);
      else this.localSet(key, null);
      return true;
    }
    if (method === "list" || method === "listRecords") {
      const prefix = String(params.prefix ?? "");
      const store = await this.recordStore();
      if (!store) return this.localList(prefix);
      const shim = prefix.startsWith(LOCAL_STORAGE_SHIM_PREFIX) || LOCAL_STORAGE_SHIM_PREFIX.startsWith(prefix)
        ? this.localList(prefix).filter((k) => k.startsWith(LOCAL_STORAGE_SHIM_PREFIX))
        : [];
      return [...(await store.listKeys(prefix)), ...shim];
    }
    if (method === "putBlob") {
      const blob = asBlob(params.blob, "storage.putBlob");
      return (await this.blobStore()).putBlob(blob);
    }
    if (method === "getBlob") {
      return (await this.blobStore()).getBlob(String(params.cid ?? ""));
    }
    if (method === "deleteBlob") {
      return (await this.blobStore()).deleteBlob(String(params.cid ?? ""));
    }
    if (method === "blobInfo") {
      const m = await (await this.blobStore()).getBlobManifest(String(params.cid ?? ""));
      return m ? { cid: String(params.cid), size: m.size, createdAt: m.createdAt } : null;
    }
    throw new Error(`storage.${method} not bridged`);
  }

  // ---- crypto.* (keys stay in the host) ----

  private async handleCrypto(method: string, params: Record<string, unknown>): Promise<unknown> {
    if (method === "encryptUpload") {
      const blob = asBlob(params.blob, "crypto.encryptUpload");
      return encryptForViewer(await this.appKey(), blob);
    }
    if (method === "decryptBlob") {
      const kek = await this.appKey();
      const ciphertext =
        params.ciphertext !== undefined && params.ciphertext !== null
          ? asBlob(params.ciphertext, "crypto.decryptBlob")
          : await (await this.blobStore()).getBlob(String(params.cid ?? ""));
      const plaintext = await decryptForViewer(kek, ciphertext, String(params.iv ?? ""), String(params.wrappedKey ?? ""));
      return { plaintext };
    }
    throw new Error(`crypto.${method} not bridged`);
  }
}

export function acquireAppDataSdkBridge(
  services: EmbedHostServices,
  httpHandler: EmbeddedHttpHandler,
  appId: string,
  storageOrigin: string,
  /** Plugin installs: the install scope label from `pluginInstallScope`. */
  collectionScope?: string,
): AppDataSdkBridge {
  const key = `${appId.trim().toLowerCase()}${collectionScope ? `/${collectionScope}` : ""}@${storageOrigin}`;
  const cached = bridgeCache.get(key);
  if (cached) return cached;
  const bridge = new AppDataSdkBridge(appId, storageOrigin, services, httpHandler, collectionScope);
  bridgeCache.set(key, bridge);
  return bridge;
}
