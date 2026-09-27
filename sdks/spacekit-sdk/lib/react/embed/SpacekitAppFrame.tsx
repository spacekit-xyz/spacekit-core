import { useEffect, useMemo, useRef, useState, type CSSProperties, type FC, type ReactNode } from "react";
import {
  mountSpacekitApp,
  type AppMountState,
  type CapabilityPolicy,
  type EmbeddedSdkBridge,
  type EmbedEndpoints,
  type EmbedHostServices,
  type IsolationMode,
  type LoadWebPackageOptions,
  type LoadedWebPackage,
  type PermissionRequest,
  type TrustPolicy,
  type VerifiedFilesLoader,
} from "../../embed/index.js";
import EmbedAppLoading from "./EmbedAppLoading.js";

/** @deprecated Blob-URL loaders cannot be isolated. Use `loadFiles` (verified files) instead. */
export type SpacekitPackageLoader = (
  storageOrigin: string,
  appId: string,
  options: LoadWebPackageOptions,
) => Promise<LoadedWebPackage>;

export interface SpacekitFrameTheme {
  background: string;
  text: string;
  muted: string;
  accent: string;
  accentText: string;
  panelBackground: string;
  panelBorder: string;
  error: string;
  fontFamily: string;
}

function defaultTheme(embedded: boolean | undefined): SpacekitFrameTheme {
  return embedded
    ? {
        background: "#0c1020",
        text: "#eef0f7",
        muted: "#6b7390",
        accent: "linear-gradient(180deg, #f3c879, #e0a948)",
        accentText: "#241a05",
        panelBackground: "rgba(255,255,255,0.03)",
        panelBorder: "rgba(29,35,60,0.9)",
        error: "#ef4444",
        fontFamily: '"Hanken Grotesk", sans-serif',
      }
    : {
        background: "#0c0f18",
        text: "#f9fafb",
        muted: "#9ca3af",
        accent: "linear-gradient(135deg, #67e8f9 0%, #22d3ee 100%)",
        accentText: "#080b0f",
        panelBackground: "rgba(255,255,255,0.03)",
        panelBorder: "rgba(255,255,255,0.08)",
        error: "#ef4444",
        fontFamily: "'DM Sans', sans-serif",
      };
}

export interface SpacekitAppFrameProps {
  appId: string;
  /** Storage node origin used to fetch the `.spkg` manifest and assets. */
  storageOrigin: string;
  /** Storage origin passed to SDK bridges (defaults to `storageOrigin`). */
  bridgeStorageOrigin?: string;
  services: EmbedHostServices;
  endpoints?: EmbedEndpoints;
  parentOrigin?: string;
  fullscreen?: boolean;
  embedded?: boolean;
  active?: boolean;
  contentFit?: "fill" | "contain";
  acquireBridge: (appId: string, storageOrigin: string, manifestName: string) => EmbeddedSdkBridge;
  /**
   * How the app is isolated from this page. Defaults to `"origin"` when
   * `appOrigin` is set, otherwise `"opaque"`. See `mountSpacekitApp`.
   */
  isolation?: IsolationMode;
  /** Dedicated app origin (or `{app}` template / function) for `isolation: "origin"`. */
  appOrigin?: string | ((appIdHex: string) => string);
  frameHostPath?: string;
  /** What apps may do beyond the baseline, and which origins get host credentials. */
  capabilities?: CapabilityPolicy;
  /** Refuse apps before they run (publisher allowlists, org policy). */
  trustPolicy?: TrustPolicy;
  /** Override package loading; must return verified files. */
  loadFiles?: VerifiedFilesLoader;
  /** @deprecated Ignored. Blob-URL loaders cannot be isolated; use `loadFiles`. */
  loadPackage?: SpacekitPackageLoader;
  onStateChange?: (state: AppMountState) => void;
  theme?: Partial<SpacekitFrameTheme>;
  className?: string;
  /** Replace the built-in consent prompt. Call `grant(true | false)`. */
  renderPermissions?: (request: PermissionRequest, grant: (allow: boolean) => void) => ReactNode;
  loadingLabel?: string;
}

let warnedLoadPackage = false;

export const SpacekitAppFrame: FC<SpacekitAppFrameProps> = ({
  appId,
  storageOrigin,
  bridgeStorageOrigin,
  services,
  endpoints,
  parentOrigin,
  fullscreen,
  embedded,
  active = true,
  contentFit = "fill",
  acquireBridge,
  isolation,
  appOrigin,
  frameHostPath,
  capabilities,
  trustPolicy,
  loadFiles,
  loadPackage,
  onStateChange,
  theme: themeOverrides,
  className,
  renderPermissions,
  loadingLabel,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<AppMountState>({ status: "loading" });
  const [consent, setConsent] = useState<{ request: PermissionRequest; resolve: (allow: boolean) => void } | null>(
    null,
  );
  const theme = { ...defaultTheme(embedded), ...themeOverrides };

  if (loadPackage && !warnedLoadPackage) {
    warnedLoadPackage = true;
    console.warn("[spacekit] SpacekitAppFrame `loadPackage` is deprecated and ignored; use `loadFiles`.");
  }

  // Callbacks and plain-data options go through refs/keys so a parent that
  // re-creates them every render does not remount the app.
  const callbacks = useRef({ trustPolicy, loadFiles, onStateChange });
  callbacks.current = { trustPolicy, loadFiles, onStateChange };
  const endpointsKey = JSON.stringify(endpoints ?? {});
  const capabilitiesKey = JSON.stringify(capabilities ?? {});
  const appOriginKey = typeof appOrigin === "function" ? appOrigin : String(appOrigin ?? "");
  const stableEndpoints = useMemo(() => endpoints, [endpointsKey]); // eslint-disable-line react-hooks/exhaustive-deps
  const stableCapabilities = useMemo(() => capabilities, [capabilitiesKey]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (embedded && !active) return;
    const el = containerRef.current;
    if (!el) return;
    setState({ status: "loading" });
    const handle = mountSpacekitApp(el, {
      appId,
      storageOrigin,
      bridgeStorageOrigin,
      services,
      acquireBridge,
      endpoints: stableEndpoints,
      parentOrigin,
      contentFit,
      isolation,
      appOrigin,
      frameHostPath,
      capabilities: stableCapabilities,
      trustPolicy: callbacks.current.trustPolicy ? (info) => callbacks.current.trustPolicy!(info) : undefined,
      loadFiles: callbacks.current.loadFiles
        ? (origin, id) => callbacks.current.loadFiles!(origin, id)
        : undefined,
      requestPermissions: (request) =>
        new Promise<boolean>((resolve) => {
          setConsent({ request, resolve });
        }),
      onStateChange: (next) => {
        setState(next);
        callbacks.current.onStateChange?.(next);
      },
      frame: {
        hash: typeof window !== "undefined" ? window.location.hash : undefined,
        style: {
          borderRadius: embedded || fullscreen ? "0" : "12px",
          background: theme.background,
        },
      },
    });
    return () => {
      handle.unmount();
      setConsent((prev) => {
        prev?.resolve(false);
        return null;
      });
    };
    // theme.background only styles the frame; it is read at mount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    appId,
    storageOrigin,
    bridgeStorageOrigin,
    services,
    acquireBridge,
    stableEndpoints,
    parentOrigin,
    contentFit,
    isolation,
    appOriginKey,
    frameHostPath,
    stableCapabilities,
    embedded,
    active,
    fullscreen,
  ]);

  const grant = (allow: boolean) => {
    consent?.resolve(allow);
    setConsent(null);
  };

  const base: CSSProperties = embedded
    ? { width: "100%", height: "100%", display: "flex", flexDirection: "column", minHeight: 0, background: theme.background }
    : fullscreen
      ? { width: "100%", height: "100%", display: "flex", flexDirection: "column", minHeight: 0 }
      : { maxWidth: 1200, margin: "0 auto", minHeight: "60vh", display: "flex", flexDirection: "column" };

  const overlay: CSSProperties = {
    position: "absolute",
    inset: 0,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    padding: embedded ? 24 : 60,
    background: theme.background,
    fontFamily: theme.fontFamily,
  };

  let cover: ReactNode = null;
  if (state.status === "loading") {
    cover = (
      <div style={overlay}>
        <EmbedAppLoading embedded={embedded} label={loadingLabel} fontFamily={theme.fontFamily} color={theme.muted} />
      </div>
    );
  } else if (state.status === "error") {
    cover = (
      <div style={overlay}>
        <div
          role="alert"
          style={{
            padding: embedded ? "16px 20px" : "20px 28px",
            borderRadius: 12,
            background: "rgba(239,68,68,0.08)",
            border: "1px solid rgba(239,68,68,0.2)",
            textAlign: "center",
            maxWidth: 480,
          }}
        >
          <div style={{ fontSize: 15, fontWeight: 700, color: theme.error, marginBottom: 8 }}>Failed to load app</div>
          <div style={{ fontSize: 13, color: theme.muted, lineHeight: 1.5 }}>{state.message}</div>
        </div>
      </div>
    );
  } else if (state.status === "permissions" && consent) {
    cover = (
      <div style={overlay}>
        {renderPermissions ? (
          renderPermissions(consent.request, grant)
        ) : (
          <div
            style={{
              padding: embedded ? "22px 24px" : "28px 32px",
              borderRadius: 16,
              background: theme.panelBackground,
              border: `1px solid ${theme.panelBorder}`,
              maxWidth: 420,
              textAlign: "center",
            }}
          >
            <div style={{ fontSize: 18, fontWeight: 700, color: theme.text, marginBottom: 6 }}>{state.manifest.name}</div>
            <div style={{ fontSize: 12, color: theme.muted, marginBottom: 16 }}>This app asks to:</div>
            <ul style={{ listStyle: "none", padding: 0, margin: "0 0 20px", textAlign: "left" }}>
              {state.permissions.map((p, i) => (
                <li
                  key={i}
                  style={{
                    padding: "6px 12px",
                    marginBottom: 4,
                    borderRadius: 8,
                    border: `1px solid ${theme.panelBorder}`,
                    fontSize: 12,
                    color: theme.text,
                  }}
                >
                  {p}
                </li>
              ))}
            </ul>
            <div style={{ display: "flex", gap: 8, justifyContent: "center" }}>
              <button
                type="button"
                onClick={() => grant(false)}
                style={{
                  background: "transparent",
                  color: theme.muted,
                  border: `1px solid ${theme.panelBorder}`,
                  borderRadius: 10,
                  padding: "10px 20px",
                  fontSize: 13,
                  fontWeight: 600,
                  cursor: "pointer",
                  fontFamily: theme.fontFamily,
                }}
              >
                Don't allow
              </button>
              <button
                type="button"
                onClick={() => grant(true)}
                style={{
                  background: theme.accent,
                  color: theme.accentText,
                  border: "none",
                  borderRadius: 10,
                  padding: "10px 28px",
                  fontSize: 13,
                  fontWeight: 700,
                  cursor: "pointer",
                  fontFamily: theme.fontFamily,
                }}
              >
                Allow & launch
              </button>
            </div>
          </div>
        )}
      </div>
    );
  }

  return (
    <div className={className} style={base}>
      <div
        style={{
          position: "relative",
          flex: 1,
          minHeight: embedded || fullscreen ? 0 : "60vh",
          display: "flex",
        }}
      >
        <div ref={containerRef} style={{ flex: 1, display: "flex", minHeight: 0 }} />
        {cover}
      </div>
    </div>
  );
};

export default SpacekitAppFrame;
