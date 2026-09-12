export { SpacekitAppFrame, type SpacekitAppFrameProps, type SpacekitPackageLoader } from "./SpacekitAppFrame.js";
export { default } from "./SpacekitAppFrame.js";
export { default as EmbedAppLoading, type EmbedAppLoadingProps } from "./EmbedAppLoading.js";
export {
  SpacekitEmbeddedApp,
  type SpacekitEmbeddedAppProps,
} from "./SpacekitEmbeddedApp.js";

export type {
  AppManifest,
  EmbedEndpoints,
  EmbedHostServices,
  EmbeddedSdkBridge,
  LoadedWebPackage,
} from "../../embed/index.js";

// Service-worker app runtime: an opt-in `loadPackage` that serves apps from a
// real, cross-origin-isolated, cache-backed path instead of a `blob:` bundle.
export {
  createSpacekitServiceWorkerLoader,
  spacekitServiceWorkerLoader,
  isServiceWorkerRuntimeSupported,
  resolveRuntimeTier,
  type AppRuntimeTier,
  type ServiceWorkerLoaderConfig,
} from "../../embed/index.js";
