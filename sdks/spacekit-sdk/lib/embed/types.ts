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

export interface SubscriptionStatus {
  active: boolean;
  expiresAt: number | null;
  viewerDid: string | null;
  amountCents?: number;
  reason?: string;
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
}

export interface HttpBridgeHost {
  mergeFetchHeaders(url: string, headers: Record<string, string>): Record<string, string>;
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
) => Promise<unknown> | null;

export interface EmbedShimConfig {
  appId: string;
  parentOrigin: string;
  endpoints: EmbedEndpoints;
  identityDid: string | null;
  kyberWasmBase64?: string;
  contentFit?: "fill" | "contain";
}
