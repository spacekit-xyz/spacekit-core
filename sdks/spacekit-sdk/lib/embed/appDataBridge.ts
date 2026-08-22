import type { EmbeddedSdkBridge } from "./bridge.js";
import type {
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
  amountCents: number;
  periodDays: number;
  txHash?: string;
  updatedAt: string;
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

export class AppDataSdkBridge implements EmbeddedSdkBridge {
  private ownerDid: string | null = null;
  private readonly appId: string;
  private readonly storageOrigin: string;
  private readonly services: EmbedHostServices;
  private readonly httpHandler: EmbeddedHttpHandler;
  private pushToIframe: ((topic: string, msg: unknown) => void) | null = null;

  constructor(
    appId: string,
    storageOrigin: string,
    services: EmbedHostServices,
    httpHandler: EmbeddedHttpHandler,
  ) {
    this.appId = appId.trim().toLowerCase();
    this.storageOrigin = storageOrigin;
    this.services = services;
    this.httpHandler = httpHandler;
  }

  setOwnerDid(did: string): void {
    this.ownerDid = did ? did.trim() : null;
  }

  async ensureHydrated(): Promise<void> {
    this.ensureOwnerDid();
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
    return `app_${this.appId}_${clean}`;
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

  private async ownerFetch(path: string, init?: RequestInit): Promise<Response> {
    const ownerDid = this.ensureOwnerDid();
    return fetch(`${trimSlash(this.storageOrigin)}${path}`, {
      ...init,
      headers: {
        "Content-Type": "application/json",
        Authorization: `DID ${ownerDid}`,
        ...(init?.headers ?? {}),
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
      amountCents: record?.amountCents,
    };
  }

  private async subscribe(params: Record<string, unknown>): Promise<SubscriptionStatus> {
    const viewer = this.services.getViewerDid();
    if (!viewer) throw new Error("Sign in before subscribing");
    const publisherDid = this.ensureOwnerDid();
    const pay = this.services.requestSubscriptionPayment;
    if (!pay) throw new Error("Payments are not configured on this host");

    const amountCents = Math.round(Number(params.amountCents));
    if (!amountCents || amountCents <= 0) throw new Error("subscribe: invalid amountCents");
    const periodDays = Math.max(1, Math.round(Number(params.periodDays) || DEFAULT_PERIOD_DAYS));

    const req: SubscriptionPaymentRequest = {
      publisherDid,
      appId: this.appId,
      amountCents,
    };
    const { txHash, payerAddress } = await pay(req);

    const key = normalizeDid(viewer);
    const existing = (await this.getDocument(SUBSCRIPTIONS_COLLECTION, key))?.data as
      | SubscriptionRecord
      | undefined;
    const base = Math.max(Date.now(), Number(existing?.expiresAt) || 0);
    const record: SubscriptionRecord = {
      buyerDid: viewer,
      expiresAt: base + periodDays * DAY_MS,
      amountCents,
      periodDays,
      txHash,
      updatedAt: new Date().toISOString(),
    };
    await this.putDocument(SUBSCRIPTIONS_COLLECTION, key, record);

    const purchase: MarketplacePurchaseRecord = {
      buyerDid: viewer,
      appId: this.appId,
      txHash,
      payerAddress,
    };
    void this.services.recordMarketplacePurchase?.(purchase).catch(() => {});

    return {
      active: record.expiresAt > Date.now(),
      expiresAt: record.expiresAt,
      viewerDid: viewer,
      amountCents,
    };
  }

  async handle(module: string, method: string, params: Record<string, unknown>): Promise<unknown> {
    const httpResult = this.httpHandler(
      module,
      method,
      params,
      (topic, msg) => this.pushToIframe?.(topic, msg),
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
        return { publisherDid: this.ownerDid, currency: "USD", model: "subscription" };
      }
      if (method === "charge") return this.subscribe(params);
      throw new Error(`Unknown payments method: ${method}`);
    }

    if (module === "documents") {
      const collection = String(params.collection ?? "").trim();
      if (!collection) throw new Error("documents: missing collection");
      if (collection.startsWith(RESERVED_PREFIX)) {
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

    // Generic KV for vibe-coded / Token Wall-style apps. Uses localStorage so any
    // viewer can persist (documents.* requires owner DID auth).
    if (module === "storage") {
      const lsKey = (key: string) => `spacekit:appdata:${this.appId}:${key}`;
      if (method === "ready") return { ready: true, appId: this.appId };
      if (method === "get" || method === "getRecord") {
        const key = String(params.key ?? "");
        try {
          const raw = localStorage.getItem(lsKey(key));
          return raw ? JSON.parse(raw) : null;
        } catch {
          return null;
        }
      }
      if (method === "set" || method === "putRecord") {
        const key = String(params.key ?? "");
        const value = params.value;
        if (value === null || value === undefined) {
          localStorage.removeItem(lsKey(key));
        } else {
          localStorage.setItem(lsKey(key), JSON.stringify(value));
        }
        return method === "putRecord" ? key : true;
      }
      if (method === "list" || method === "listRecords") {
        const prefix = String(params.prefix ?? "");
        const needle = lsKey(prefix);
        const strip = lsKey("");
        const out: string[] = [];
        for (let i = 0; i < localStorage.length; i++) {
          const k = localStorage.key(i)!;
          if (!k.startsWith(needle)) continue;
          out.push(k.slice(strip.length));
        }
        return out;
      }
      if (method === "delete" || method === "deleteRecord") {
        localStorage.removeItem(lsKey(String(params.key ?? "")));
        return true;
      }
    }

    if (module === "messaging" && method === "publish") {
      const topic = String(params.topic ?? "");
      const msg = params.msg;
      this.pushToIframe?.(topic, msg);
      return true;
    }

    throw new Error(`${module}.${method} not bridged`);
  }
}

export function acquireAppDataSdkBridge(
  services: EmbedHostServices,
  httpHandler: EmbeddedHttpHandler,
  appId: string,
  storageOrigin: string,
): AppDataSdkBridge {
  const key = `${appId.trim().toLowerCase()}@${storageOrigin}`;
  const cached = bridgeCache.get(key);
  if (cached) return cached;
  const bridge = new AppDataSdkBridge(appId, storageOrigin, services, httpHandler);
  bridgeCache.set(key, bridge);
  return bridge;
}
