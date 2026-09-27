import { useEffect, useMemo, useState, type CSSProperties, type FC } from "react";
import {
  acquireAppDataSdkBridge,
  createLocalStorageEmbedHost,
  createStorageOriginResolver,
  defaultTrustedOrigins,
  type AppMountState,
  type CapabilityPolicy,
  type IsolationMode,
  type TrustPolicy,
  type VerifiedFilesLoader,
  type EmbedEndpoints,
  type EmbedHostServices,
  type EmbeddedHttpHandler,
  type EmbeddedSdkBridge,
  type LocalEmbedHostOptions,
  type StorageOriginResolverOptions,
} from "../../embed/index.js";
import EmbedAppLoading from "./EmbedAppLoading.js";
import {
  SpacekitAppFrame,
  type SpacekitAppFrameProps,
  type SpacekitFrameTheme,
  type SpacekitPackageLoader,
} from "./SpacekitAppFrame.js";

export interface SpacekitEmbeddedAppProps {
  appId: string;
  /**
   * Storage origins to try, best first. Ignored when `resolveStorageOrigin` is
   * given. Pass a single origin when the host has only one storage route.
   */
  storageOrigins?: string[] | (() => string[]);
  /** Full control over origin selection (probing, dev proxy rewrites, caching). */
  resolveStorageOrigin?: (appId: string) => Promise<string>;
  /** Normalizes each candidate before probing, e.g. rewriting to a dev proxy. */
  normalizeStorageOrigin?: StorageOriginResolverOptions["normalize"];
  /** Storage origin for the app-data bridge's document reads/writes. */
  bridgeStorageOrigin?: string;
  /** Defaults to a localStorage-backed identity host. */
  services?: EmbedHostServices;
  /** Options for the default identity host. Ignored when `services` is passed. */
  hostOptions?: LocalEmbedHostOptions;
  endpoints?: EmbedEndpoints;
  /** Defaults to the generic app-data bridge, which suits any published app. */
  acquireBridge?: (
    appId: string,
    storageOrigin: string,
    manifestName: string,
  ) => EmbeddedSdkBridge;
  /** @deprecated Ignored; use `loadFiles`. */
  loadPackage?: SpacekitPackageLoader;
  /** Override package loading; must return verified files. */
  loadFiles?: VerifiedFilesLoader;
  /** See `SpacekitAppFrame`. Defaults to `"origin"` with `appOrigin`, else `"opaque"`. */
  isolation?: IsolationMode;
  appOrigin?: string | ((appIdHex: string) => string);
  frameHostPath?: string;
  /**
   * Capability policy. `trustedOrigins` also decides where the default identity
   * host attaches the viewer's credentials; it defaults to this page's origin
   * plus the origins in `endpoints`.
   */
  capabilities?: CapabilityPolicy;
  trustPolicy?: TrustPolicy;
  onStateChange?: (state: AppMountState) => void;
  theme?: Partial<SpacekitFrameTheme>;
  renderPermissions?: SpacekitAppFrameProps["renderPermissions"];
  className?: string;
  fullscreen?: boolean;
  embedded?: boolean;
  active?: boolean;
  contentFit?: "fill" | "contain";
  loadingLabel?: string;
}

const centered: CSSProperties = {
  width: "100%",
  height: "100%",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
};

/**
 * Runs a published SpaceKit app in-page.
 *
 * Wraps {@link SpacekitAppFrame} with the two things every host must supply but
 * few need to customize: choosing a reachable storage origin, and an SDK bridge
 * backed by the visitor's identity.
 */
export const SpacekitEmbeddedApp: FC<SpacekitEmbeddedAppProps> = ({
  appId,
  storageOrigins,
  resolveStorageOrigin,
  normalizeStorageOrigin,
  bridgeStorageOrigin,
  services,
  hostOptions,
  endpoints,
  acquireBridge,
  loadPackage,
  loadFiles,
  isolation,
  appOrigin,
  frameHostPath,
  capabilities,
  trustPolicy,
  onStateChange,
  theme,
  renderPermissions,
  className,
  fullscreen,
  embedded,
  active = true,
  contentFit = "fill",
  loadingLabel,
}) => {
  const [storageOrigin, setStorageOrigin] = useState<string | null>(null);

  const trustedKey = JSON.stringify([
    capabilities?.trustedOrigins ?? null,
    endpoints?.apiBase,
    endpoints?.messagingBase,
    endpoints?.reposApiBase,
    endpoints?.workspacesApiBase,
  ]);
  const trustedOrigins = useMemo(
    () =>
      capabilities?.trustedOrigins ??
      defaultTrustedOrigins([
        endpoints?.apiBase,
        endpoints?.messagingBase,
        endpoints?.reposApiBase,
        endpoints?.workspacesApiBase,
      ]),
    [trustedKey], // eslint-disable-line react-hooks/exhaustive-deps
  );
  const policy = useMemo<CapabilityPolicy>(
    () => ({ ...capabilities, trustedOrigins }),
    [JSON.stringify(capabilities ?? {}), trustedOrigins], // eslint-disable-line react-hooks/exhaustive-deps
  );

  const host = useMemo(
    () =>
      services
        ? null
        : createLocalStorageEmbedHost({
            ...hostOptions,
            credentialedOrigins: hostOptions?.credentialedOrigins ?? trustedOrigins,
          }),
    [services, hostOptions, trustedOrigins],
  );

  const resolvedServices = services ?? host!.services;
  const httpHandler: EmbeddedHttpHandler | null = host?.httpHandler ?? null;

  const resolver = useMemo(() => {
    if (resolveStorageOrigin) return resolveStorageOrigin;
    return createStorageOriginResolver({
      candidates: storageOrigins ?? [],
      normalize: normalizeStorageOrigin,
    });
  }, [resolveStorageOrigin, storageOrigins, normalizeStorageOrigin]);

  useEffect(() => {
    let cancelled = false;
    setStorageOrigin(null);
    void resolver(appId).then((origin) => {
      if (!cancelled) setStorageOrigin(origin);
    });
    return () => {
      cancelled = true;
    };
  }, [appId, resolver]);

  const bridgeFactory = useMemo(() => {
    if (acquireBridge) return acquireBridge;
    // Custom `services` come without an HTTP handler, so there is nothing to
    // give the default bridge; `acquireBridge` becomes required.
    if (!httpHandler) return null;
    return (id: string, origin: string) =>
      acquireAppDataSdkBridge(resolvedServices, httpHandler, id, origin);
  }, [acquireBridge, httpHandler, resolvedServices]);

  if (!bridgeFactory) {
    return (
      <div style={centered}>
        <span style={{ fontSize: 13, color: "#f87171" }}>
          SpacekitEmbeddedApp: pass `acquireBridge` when providing custom `services`.
        </span>
      </div>
    );
  }

  if (!storageOrigin) {
    return (
      <div style={centered}>
        <EmbedAppLoading embedded={embedded} label={loadingLabel} />
      </div>
    );
  }

  return (
    <SpacekitAppFrame
      appId={appId}
      storageOrigin={storageOrigin}
      bridgeStorageOrigin={bridgeStorageOrigin}
      services={resolvedServices}
      endpoints={endpoints}
      fullscreen={fullscreen}
      embedded={embedded}
      active={active}
      contentFit={contentFit}
      acquireBridge={bridgeFactory}
      loadPackage={loadPackage}
      loadFiles={loadFiles}
      isolation={isolation}
      appOrigin={appOrigin}
      frameHostPath={frameHostPath}
      capabilities={policy}
      trustPolicy={trustPolicy}
      onStateChange={onStateChange}
      theme={theme}
      renderPermissions={renderPermissions}
      className={className}
      loadingLabel={loadingLabel}
    />
  );
};

export default SpacekitEmbeddedApp;
