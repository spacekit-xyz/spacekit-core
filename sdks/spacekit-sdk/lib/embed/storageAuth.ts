/**
 * Client for the storage node's signed-login and delegation endpoints.
 *
 * A host whose viewer holds an Ed25519 `did:key` (kit.space, desktop wallets)
 * logs in once by signing a node-issued challenge, then mints short-lived
 * app-scoped tokens for each embedded app. Apps never see the viewer's key or
 * session; the node only accepts their token for that app's collections.
 *
 * ```ts
 * const auth = createStorageAuthClient({
 *   storageOrigin: "https://kit.space/api/storage",
 *   getSigner: async () => ({ did, publicKeyHex, sign: (msg) => ed.signAsync(msg, privateKey) }),
 * });
 * const host = createLocalStorageEmbedHost({ appCredentials: auth.appCredentials });
 * ```
 */

import type {
  AppCredentialRequest,
  AppCredentials,
  SubscriptionRecordRequest,
  SubscriptionStatus,
} from "./types.js";

export type StorageSignatureAlgorithm = "ed25519" | "slh-dsa-sha2-128s" | "slh-dsa-sha2-192s";

export interface StorageSigner {
  did: string;
  /** Hex public key (Ed25519: 32 bytes; SLH-DSA-SHA2-128s: 32; -192s: 48). */
  publicKeyHex: string;
  /** Defaults to "ed25519". SLH-DSA is FIPS 205 (kit.space quantum identities). */
  algorithm?: StorageSignatureAlgorithm;
  /** Signature over the exact message bytes. */
  sign(message: Uint8Array): Promise<Uint8Array>;
}

/** A node session obtained some other way (e.g. from the website API). */
export interface StorageSession {
  token: string;
  did: string;
  expiresAt: number;
}

export interface StorageAuthClientOptions {
  /** Base the node's `/api/auth/*` routes live under (a node origin or a proxy prefix). */
  storageOrigin: string;
  /**
   * The signed-in viewer's signer, or null when nobody is signed in (apps then
   * run without storage credentials). Called when a new session is needed.
   */
  getSigner?(): Promise<StorageSigner | null>;
  /**
   * Alternative to `getSigner` for viewers whose DID has no key the node can
   * check (e.g. `did:spacekit:user:*` accounts signed in to the website API):
   * return a node session minted by a trusted service. See
   * `websiteStorageSession`.
   */
  getSession?(): Promise<StorageSession | null>;
  fetchImpl?: typeof fetch;
}

interface CachedToken {
  authorization: string;
  expiresAt: number;
}

export interface StorageAuthClient {
  /** `Bearer <session token>` for the viewer, or null when not signed in. */
  sessionAuthorization(): Promise<string | null>;
  /** `Bearer <app token>` for one app, acting in `actAs`'s namespace when given. */
  appAuthorization(appId: string, actAs?: string | null): Promise<string | null>;
  /** Provider for `createLocalStorageEmbedHost({ appCredentials })`. */
  appCredentials(req: AppCredentialRequest): Promise<AppCredentials | null>;
  /** Forget cached tokens (call on sign-out or identity change). */
  clear(): void;
}

const REFRESH_MARGIN_S = 60;

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function trimSlash(url: string): string {
  return url.replace(/\/+$/, "");
}

export function createStorageAuthClient(options: StorageAuthClientOptions): StorageAuthClient {
  const base = trimSlash(options.storageOrigin);
  const doFetch = options.fetchImpl ?? ((...args: Parameters<typeof fetch>) => fetch(...args));
  let session: (CachedToken & { did: string }) | null = null;
  let sessionInflight: Promise<string | null> | null = null;
  const appTokens = new Map<string, CachedToken>();
  const appInflight = new Map<string, Promise<string | null>>();

  const fresh = (t: CachedToken | null | undefined): t is CachedToken =>
    !!t && t.expiresAt - REFRESH_MARGIN_S > Date.now() / 1000;

  async function postJson(path: string, body: unknown, authorization?: string): Promise<Record<string, unknown>> {
    const res = await doFetch(`${base}${path}`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(authorization ? { Authorization: authorization } : {}),
      },
      body: JSON.stringify(body),
    });
    const json = (await res.json().catch(() => ({}))) as Record<string, unknown>;
    if (!res.ok) throw new Error(String(json.error ?? `storage auth ${path} failed (${res.status})`));
    return json;
  }

  async function login(): Promise<string | null> {
    if (options.getSession) {
      const s = await options.getSession();
      if (!s) return null;
      session = { did: s.did, authorization: `Bearer ${s.token}`, expiresAt: s.expiresAt };
      appTokens.clear();
      return session.authorization;
    }
    const signer = options.getSigner ? await options.getSigner() : null;
    if (!signer) return null;
    if (fresh(session) && session.did === signer.did) return session.authorization;
    const ch = await postJson("/api/auth/challenge", { did: signer.did });
    const message = String(ch.message ?? "");
    const challenge = String(ch.challenge ?? "");
    if (!message || !challenge) throw new Error("storage node returned no challenge");
    const signature = await signer.sign(new TextEncoder().encode(message));
    const issued = await postJson("/api/auth/session", {
      did: signer.did,
      challenge,
      algorithm: signer.algorithm ?? "ed25519",
      public_key_hex: signer.publicKeyHex,
      signature_hex: toHex(signature),
    });
    session = {
      did: signer.did,
      authorization: `Bearer ${String(issued.token)}`,
      expiresAt: Number(issued.expires_at) || 0,
    };
    appTokens.clear();
    return session.authorization;
  }

  async function sessionAuthorization(): Promise<string | null> {
    if (fresh(session)) return session.authorization;
    sessionInflight ??= login().finally(() => {
      sessionInflight = null;
    });
    return sessionInflight;
  }

  async function appAuthorization(appId: string, actAs?: string | null): Promise<string | null> {
    const key = `${appId.toLowerCase()}|${actAs ?? ""}`;
    const cached = appTokens.get(key);
    if (fresh(cached)) return cached.authorization;
    let inflight = appInflight.get(key);
    if (!inflight) {
      inflight = (async () => {
        const parent = await sessionAuthorization();
        if (!parent) return null;
        const issued = await postJson(
          "/api/auth/delegate",
          { app_id: appId.toLowerCase(), ...(actAs ? { act_as: actAs } : {}) },
          parent,
        );
        const token: CachedToken = {
          authorization: `Bearer ${String(issued.token)}`,
          expiresAt: Number(issued.expires_at) || 0,
        };
        appTokens.set(key, token);
        return token.authorization;
      })().finally(() => appInflight.delete(key));
      appInflight.set(key, inflight);
    }
    return inflight;
  }

  return {
    sessionAuthorization,
    appAuthorization,
    async appCredentials(req) {
      try {
        const storageAuthorization = await appAuthorization(req.appId, req.publisherDid);
        return storageAuthorization ? { storageAuthorization } : null;
      } catch (err) {
        console.warn("[spacekit] could not get app-scoped storage credentials:", err);
        return null;
      }
    },
    clear() {
      session = null;
      appTokens.clear();
    },
  };
}

/**
 * `getSession` for viewers signed in to the website API (passkey / magic link):
 * trades their bearer session for a storage-node session via
 * `POST {apiBase}/api/auth/storage-token`.
 */
export function websiteStorageSession(options: {
  apiBase: string;
  /** The viewer's website-API bearer token, or null when signed out. */
  getBearer(): string | null | Promise<string | null>;
  fetchImpl?: typeof fetch;
}): () => Promise<StorageSession | null> {
  const doFetch = options.fetchImpl ?? ((...args: Parameters<typeof fetch>) => fetch(...args));
  return async () => {
    const bearer = await options.getBearer();
    if (!bearer) return null;
    const res = await doFetch(`${trimSlash(options.apiBase)}/api/auth/storage-token`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${bearer}` },
      body: "{}",
    });
    if (!res.ok) return null;
    const json = (await res.json()) as { token?: string; did?: string; expires_at?: number };
    if (!json.token || !json.did) return null;
    return { token: json.token, did: json.did, expiresAt: Number(json.expires_at) || 0 };
  };
}

/**
 * `recordSubscription` backed by the website API
 * (`POST {apiBase}/api/apps/:appId/subscriptions`), which verifies the payment
 * on-chain and writes the record as a trusted service.
 */
export function createSubscriptionRecorder(options: {
  apiBase: string;
  /** Authorization for the viewer, e.g. `storageAuth.sessionAuthorization`. */
  authorization(): Promise<string | null>;
  fetchImpl?: typeof fetch;
}): (req: SubscriptionRecordRequest) => Promise<SubscriptionStatus> {
  const doFetch = options.fetchImpl ?? ((...args: Parameters<typeof fetch>) => fetch(...args));
  return async (req) => {
    const authorization = await options.authorization();
    if (!authorization) throw new Error("Sign in before subscribing");
    const res = await doFetch(
      `${trimSlash(options.apiBase)}/api/apps/${encodeURIComponent(req.appId)}/subscriptions`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: authorization },
        body: JSON.stringify({
          publisher_did: req.publisherDid,
          amount_cents: req.amountCents,
          period_days: req.periodDays,
          tx_hash: req.txHash,
          payer_address: req.payerAddress,
        }),
      },
    );
    const json = (await res.json().catch(() => ({}))) as Record<string, unknown>;
    if (!res.ok) throw new Error(String(json.error ?? `subscription failed (${res.status})`));
    return json as unknown as SubscriptionStatus;
  };
}
