export interface AppManifest {
  name: string;
  description: string;
  entry_points: Array<Record<string, unknown>>;
  permissions: unknown[];
  total_size: number;
  checksum: string | number[];
}

export interface ContentRef {
  path: string;
  content_type: string | Record<string, unknown>;
  size: number;
  hash: string | number[];
  fact_id: string | number[];
  compression?: string | Record<string, unknown>;
  encrypted?: boolean;
}

export interface AppPackageJSON {
  app_id: string | number[];
  creator_did: string | { did?: string };
  manifest: AppManifest;
  content_refs: ContentRef[];
}

export interface LoadedWebPackage {
  manifest: AppManifest;
  appId: string;
  creatorDid: string;
  blobUrl: string;
  assetUrls: Map<string, string>;
  blobAssetUrls: string[];
  integrityErrors: string[];
}

export interface EmbedEndpoints {
  messagingBase?: string;
  apiBase?: string;
  reposApiBase?: string;
  workspacesApiBase?: string;
  /** Fallback Kyber WASM URL when the package does not ship its own. */
  wasmUrl?: string;
}

export interface SubscriptionPaymentRequest {
  publisherDid: string;
  appId: string;
  amountCents: number;
}

export interface SubscriptionPaymentResult {
  txHash: string;
  payerAddress?: string;
}

export interface MarketplacePurchaseRecord {
  buyerDid: string;
  appId: string;
  txHash?: string;
  payerAddress?: string;
}

/** Payment details the host hands to its subscription service for verification. */
export interface SubscriptionRecordRequest {
  appId: string;
  publisherDid: string;
  amountCents: number;
  periodDays: number;
  txHash: string;
  payerAddress?: string;
}

export interface SubscriptionStatus {
  active: boolean;
  expiresAt: number | null;
  viewerDid: string | null;
  amountCents?: number;
  reason?: string;
  /** True when a server verified the payment behind the record. */
  verified?: boolean;
}

/** What an app's bridge asks the host for when it needs to act for the viewer. */
export interface AppCredentialRequest {
  /** Lower-case hex app id. */
  appId: string;
  /** The app's publisher (`creator_did`), whose namespace holds the app's shared documents. */
  publisherDid: string | null;
  /** Storage origin the bridge talks to. */
  storageOrigin: string;
}

/**
 * Credentials scoped to one app, used instead of the viewer's own session.
 * Values are complete `Authorization` header values.
 */
export interface AppCredentials {
  /**
   * For the app's document calls to the storage node, e.g. an app-scoped node
   * token from `POST /api/auth/delegate` (`"Bearer sktok1.…"`).
   */
  storageAuthorization?: string;
  /**
   * For `http.fetch` to trusted origins (the host API), e.g. a website-api app
   * token from `POST /api/auth/app-token` (`"Bearer skapp1.…"`).
   */
  apiAuthorization?: string;
}

/** Host-provided identity, payments, and auth services for embedded app bridges. */
export interface EmbedHostServices {
  /** Signed-in viewer DID, or null when anonymous. */
  getViewerDid(): string | null;
  /** Identity DID injected into the iframe bootstrap config. */
  getIdentityDid(): string | null;
  handleIdentity(method: string, params: Record<string, unknown>): unknown | Promise<unknown>;
  requestSubscriptionPayment?(
    req: SubscriptionPaymentRequest,
  ): Promise<SubscriptionPaymentResult>;
  recordMarketplacePurchase?(req: MarketplacePurchaseRecord): Promise<void>;
  /**
   * App-scoped credentials for the viewer, or null when the viewer is anonymous
   * or the host cannot mint them. Called often; cache inside.
   */
  getAppCredentials?(req: AppCredentialRequest): Promise<AppCredentials | null>;
  /**
   * Have a trusted service verify the payment and record the subscription
   * (e.g. website-api `POST /api/apps/:appId/subscriptions`). Storage nodes
   * refuse subscription records written by clients, so without this hook
   * `payments.subscribe` fails after payment.
   */
  recordSubscription?(req: SubscriptionRecordRequest): Promise<SubscriptionStatus>;
}

/** Per-call context the bridge passes to the HTTP handler. */
export interface EmbeddedHttpContext {
  appId: string;
  /** App-scoped credentials, when the host provides them. */
  appCredentials?: () => Promise<AppCredentials | null>;
}

export interface HttpBridgeHost {
  /**
   * Whether the host may attach its own credentials (session token, owner DID,
   * cookies) to a request for `url`. Requests that fail this check are sent
   * with only the app's own headers and `credentials: "omit"`. Defaults to
   * same-origin with the host page.
   */
  isCredentialedUrl?(url: string): boolean;
  mergeFetchHeaders(url: string, headers: Record<string, string>): Record<string, string>;
  /**
   * Whether to fall back to the viewer's own session (`mergeFetchHeaders`) on
   * trusted origins when no app-scoped `apiAuthorization` is available.
   * Defaults to true for compatibility; set false once the host API issues app
   * tokens, so apps never carry the viewer's full session.
   */
  forwardViewerSession?: boolean;
  getSessionToken(): string | null;
  /** When true, retry the request once with refreshed auth headers after a 401. */
  shouldRetryUnauthorized?(url: string, headers: Record<string, string>): boolean;
  refreshFetchHeaders?(url: string, headers: Record<string, string>): Record<string, string>;
  isSessionExpiredError?(body: unknown): boolean;
  onSessionExpired?(message: string): void;
  formatFetchError?(err: unknown, fallback: string): string;
}

export interface EmbeddedFetchResult {
  ok: boolean;
  status: number;
  statusText: string;
  headers: Record<string, string>;
  body: string;
  binary?: boolean;
}

export type SsePushHandler = (topic: string, msg: unknown) => void;

export type EmbeddedHttpHandler = (
  module: string,
  method: string,
  params: Record<string, unknown>,
  push: SsePushHandler,
  context?: EmbeddedHttpContext,
) => Promise<unknown> | null;

export interface EmbedShimConfig {
  appId: string;
  parentOrigin: string;
  endpoints: EmbedEndpoints;
  identityDid: string | null;
  kyberWasmBase64?: string;
  contentFit?: "fill" | "contain";
  /**
   * When true, the app's own same-origin `fetch()` calls pass straight through
   * instead of being proxied to the host API. Set by the service-worker runtime,
   * where the app is served from a real same-origin path and its assets must be
   * fetched from that origin (and the SW cache). Defaults to false for the
   * classic blob-bundle path, whose assets are `blob:` URLs.
   */
  sameOriginPassthrough?: boolean;
}
