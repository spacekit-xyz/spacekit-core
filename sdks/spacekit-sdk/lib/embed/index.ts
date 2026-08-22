export type {
  AppManifest,
  AppPackageJSON,
  ContentRef,
  EmbedEndpoints,
  EmbedHostServices,
  EmbedShimConfig,
  EmbeddedFetchResult,
  EmbeddedHttpHandler,
  HttpBridgeHost,
  LoadedWebPackage,
  MarketplacePurchaseRecord,
  SubscriptionPaymentRequest,
  SubscriptionPaymentResult,
  SubscriptionStatus,
} from "./types.js";

export {
  type EmbeddedSdkBridge,
  type EmbeddedSdkBridgeWithOwner,
  configureBridgeOwner,
  handleSdkCall,
} from "./bridge.js";

export {
  SESSION_EXPIRED_EVENT,
  createEmbeddedHttpHandler,
  handleEmbeddedHttpFetch,
} from "./httpBridge.js";

export {
  AppDataSdkBridge,
  acquireAppDataSdkBridge,
} from "./appDataBridge.js";

export {
  DEFAULT_DID_KEYS,
  DEFAULT_SESSION_TOKEN_KEY,
  createLocalIdentityHost,
  createLocalStorageEmbedHost,
  type LocalEmbedHost,
  type LocalEmbedHostOptions,
  type LocalIdentityHost,
  type LocalIdentityHostOptions,
  type LocalIdentitySnapshot,
} from "./localIdentityHost.js";

export {
  DEFAULT_ORIGIN_CACHE_PREFIX,
  createStorageOriginResolver,
  probeManifest,
  type ManifestProbeResult,
  type StorageOriginResolver,
  type StorageOriginResolverOptions,
} from "./storageOrigin.js";

export {
  loadWebPackage,
  loadWebPackageFromLocal,
  revokeLoadedWebPackage,
  type LoadWebPackageOptions,
} from "./packageLoader.js";

export {
  SPKG_MAX_ENTRIES,
  SPKG_MAX_UNCOMPRESSED_BYTES,
  SPKG_MIMETYPE,
  fetchSpkg,
  loadWebPackageFromSpkg,
  openSpkg,
  parseSpkg,
  type OpenedSpkg,
  type SpkgSource,
} from "./spkg.js";

export { injectSdkBridgeIntoHtml } from "./injectShim.js";
