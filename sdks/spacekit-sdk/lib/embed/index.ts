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
  loadVerifiedPackageFiles,
  verifyLocalPackageFiles,
  type LoadWebPackageOptions,
  type VerifiedPackageFile,
  type VerifiedWebPackageFiles,
} from "./packageLoader.js";

/**
 * @deprecated Serves apps from the host origin, which gives them the host's
 * storage and session. Isolated hosts (`mountSpacekitApp`, `SpacekitAppFrame`)
 * no longer use it; kept for hosts that still run `isolation: "unsafe-same-origin"`
 * apps through their own frame code.
 */
export {
  createSpacekitServiceWorkerLoader,
  spacekitServiceWorkerLoader,
  isServiceWorkerRuntimeSupported,
  resolveRuntimeTier,
  type AppRuntimeTier,
  type ServiceWorkerLoaderConfig,
} from "./serviceWorkerRuntime.js";

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

export { decodeContentRef, isDecodableCompression } from "./decompress.js";

// --- Isolated app host (protocol v1) ---
export {
  mountSpacekitApp,
  resolveAppOrigin,
  type AppMountState,
  type AppTrustInfo,
  type IsolationMode,
  type MountSpacekitAppOptions,
  type PermissionRequest,
  type SpacekitAppHandle,
  type TrustPolicy,
  type VerifiedFilesLoader,
} from "./host.js";

export {
  CapabilityError,
  allowPublishers,
  createCapabilityGuard,
  defaultTrustedOrigins,
  parseManifestPermissions,
  requiredCapability,
  type CapabilityGuard,
  type CapabilityGuardOptions,
  type CapabilityPolicy,
  type DeclaredPermissions,
  type NetworkPolicy,
} from "./capabilities.js";

export {
  buildOpaqueBootstrapHtml,
  renderFrameHostHtml,
  type FrameBootstrapConfig,
  type FrameHostHtmlOptions,
} from "./frameBootstrap.js";

export { buildFramePayload, findHtmlEntryPath, type FramePayload, type FramePayloadOptions } from "./framePayload.js";

export {
  FRAME_ERROR,
  FRAME_LOAD,
  FRAME_READY,
  LOCAL_STORAGE_SHIM_PREFIX,
  SPACEKIT_EMBED_PROTOCOL_VERSION,
  type FrameErrorMessage,
  type FrameLoadFile,
  type FrameLoadMessage,
  type FrameReadyMessage,
  type PortCallMessage,
  type PortHostMessage,
} from "./protocol.js";
