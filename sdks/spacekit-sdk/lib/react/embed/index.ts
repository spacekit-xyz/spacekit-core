export {
  SpacekitAppFrame,
  type SpacekitAppFrameProps,
  type SpacekitFrameTheme,
  type SpacekitPackageLoader,
} from "./SpacekitAppFrame.js";
export { default } from "./SpacekitAppFrame.js";
export { default as EmbedAppLoading, type EmbedAppLoadingProps } from "./EmbedAppLoading.js";
export {
  SpacekitEmbeddedApp,
  type SpacekitEmbeddedAppProps,
} from "./SpacekitEmbeddedApp.js";

export type {
  AppManifest,
  AppMountState,
  CapabilityPolicy,
  EmbedEndpoints,
  EmbedHostServices,
  EmbeddedSdkBridge,
  IsolationMode,
  LoadedWebPackage,
  NetworkPolicy,
  PermissionRequest,
  TrustPolicy,
  VerifiedFilesLoader,
} from "../../embed/index.js";

export { allowPublishers, requireSignedPackages, createStorageAuthClient } from "../../embed/index.js";

/** @deprecated See `serviceWorkerRuntime.ts`; isolated frames no longer use it. */
export {
  createSpacekitServiceWorkerLoader,
  spacekitServiceWorkerLoader,
  isServiceWorkerRuntimeSupported,
  resolveRuntimeTier,
  type AppRuntimeTier,
  type ServiceWorkerLoaderConfig,
} from "../../embed/index.js";
