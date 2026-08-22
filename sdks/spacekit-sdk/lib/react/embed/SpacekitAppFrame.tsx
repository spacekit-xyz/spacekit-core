import { useCallback, useEffect, useRef, useState, type CSSProperties, type FC } from "react";
import {
  configureBridgeOwner,
  handleSdkCall,
  loadWebPackage,
  revokeLoadedWebPackage,
  type AppManifest,
  type EmbeddedSdkBridge,
  type EmbedEndpoints,
  type EmbedHostServices,
  type LoadWebPackageOptions,
  type LoadedWebPackage,
} from "../../embed/index.js";
import EmbedAppLoading from "./EmbedAppLoading.js";

export type SpacekitPackageLoader = (
  storageOrigin: string,
  appId: string,
  options: LoadWebPackageOptions,
) => Promise<LoadedWebPackage>;

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
  /** Override network fetch (e.g. Desktop encrypted package cache). */
  loadPackage?: SpacekitPackageLoader;
}

type LoadState =
  | { status: "loading" }
  | { status: "permissions"; manifest: AppManifest; pkg: LoadedWebPackage }
  | { status: "running"; manifest: AppManifest; blobUrl: string }
  | { status: "error"; message: string };

function installSdkBridge(
  bridgeRef: React.MutableRefObject<EmbeddedSdkBridge | null>,
  acquireBridge: SpacekitAppFrameProps["acquireBridge"],
  appId: string,
  storageOrigin: string,
  manifestName: string,
  ownerDid: string,
): void {
  bridgeRef.current?.flush();
  const bridge = acquireBridge(appId, storageOrigin, manifestName);
  configureBridgeOwner(bridge, ownerDid);
  bridgeRef.current = bridge;
  void bridge.ensureHydrated();
}

export const SpacekitAppFrame: FC<SpacekitAppFrameProps> = ({
  appId,
  storageOrigin,
  bridgeStorageOrigin,
  services,
  endpoints = {},
  parentOrigin,
  fullscreen,
  embedded,
  active = true,
  contentFit = "fill",
  acquireBridge,
  loadPackage,
}) => {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const pkgRef = useRef<LoadedWebPackage | null>(null);
  const loadedFitRef = useRef<"fill" | "contain" | null>(null);
  const bridgeRef = useRef<EmbeddedSdkBridge | null>(null);
  const resolvedParentOrigin = parentOrigin ?? (typeof window !== "undefined" ? window.location.origin : "");
  const resolvedBridgeOrigin = bridgeStorageOrigin ?? storageOrigin;

  useEffect(() => {
    return () => {
      bridgeRef.current?.flush();
      bridgeRef.current = null;
    };
  }, [appId, resolvedBridgeOrigin]);

  useEffect(() => {
    if (embedded && !active) {
      if (pkgRef.current) {
        revokeLoadedWebPackage(pkgRef.current);
        pkgRef.current = null;
        loadedFitRef.current = null;
      }
      return;
    }
    if (embedded && pkgRef.current && loadedFitRef.current === contentFit) return;

    if (pkgRef.current) {
      revokeLoadedWebPackage(pkgRef.current);
      pkgRef.current = null;
      loadedFitRef.current = null;
    }

    let cancelled = false;
    setState({ status: "loading" });

    const loadOptions: LoadWebPackageOptions = {
      parentOrigin: resolvedParentOrigin,
      endpoints: {
        ...endpoints,
        wasmUrl: endpoints.wasmUrl ?? `${resolvedParentOrigin}/wasm/kyber_wasm_bg.wasm`,
      },
      identityDid: services.getIdentityDid(),
      contentFit,
    };
    const loader = loadPackage ?? loadWebPackage;

    loader(storageOrigin, appId, loadOptions)
      .then((pkg) => {
        if (cancelled) {
          revokeLoadedWebPackage(pkg);
          return;
        }
        pkgRef.current = pkg;
        loadedFitRef.current = contentFit;
        installSdkBridge(
          bridgeRef,
          acquireBridge,
          appId,
          resolvedBridgeOrigin,
          pkg.manifest.name,
          pkg.creatorDid,
        );
        if (pkg.integrityErrors.length > 0) {
          setState({ status: "error", message: `Integrity failed: ${pkg.integrityErrors.join(", ")}` });
          return;
        }
        if (pkg.manifest.permissions.length > 0) {
          setState({ status: "permissions", manifest: pkg.manifest, pkg });
        } else {
          setState({ status: "running", manifest: pkg.manifest, blobUrl: pkg.blobUrl });
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setState({ status: "error", message: err instanceof Error ? err.message : String(err) });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [
    appId,
    storageOrigin,
    resolvedBridgeOrigin,
    resolvedParentOrigin,
    embedded,
    active,
    contentFit,
    services,
    endpoints,
    acquireBridge,
    loadPackage,
  ]);

  useEffect(() => {
    return () => {
      if (pkgRef.current) {
        revokeLoadedWebPackage(pkgRef.current);
        pkgRef.current = null;
      }
    };
  }, [appId, storageOrigin]);

  const handleMessage = useCallback((e: MessageEvent) => {
    if (e.data?.type !== "spacekit-sdk-call") return;
    const iframe = iframeRef.current;
    if (!iframe?.contentWindow || e.source !== iframe.contentWindow) return;
    const bridge = bridgeRef.current;
    if (!bridge) return;
    const { id, module, method, params } = e.data;
    handleSdkCall(bridge, module, method, params)
      .then((result) => {
        iframe.contentWindow?.postMessage({ type: "spacekit-sdk-response", id, result }, "*");
      })
      .catch((err) => {
        iframe.contentWindow?.postMessage(
          { type: "spacekit-sdk-response", id, error: err instanceof Error ? err.message : String(err) },
          "*",
        );
      });
  }, []);

  useEffect(() => {
    const bridge = bridgeRef.current;
    const iframe = iframeRef.current;
    if (!bridge || !iframe) return;
    bridge.setPushHandler((topic, msg) => {
      iframe.contentWindow?.postMessage({ type: "spacekit-sdk-event", topic, msg }, "*");
    });
  }, [state]);

  useEffect(() => {
    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  }, [handleMessage]);

  const notifyIframeResize = useCallback(() => {
    iframeRef.current?.contentWindow?.dispatchEvent(new Event("resize"));
  }, []);

  useEffect(() => {
    if (state.status !== "running") return;
    const iframe = iframeRef.current;
    if (!iframe) return;
    notifyIframeResize();
    const t1 = window.setTimeout(notifyIframeResize, 120);
    const t2 = window.setTimeout(notifyIframeResize, 400);
    const ro =
      typeof ResizeObserver !== "undefined" ? new ResizeObserver(() => notifyIframeResize()) : null;
    ro?.observe(iframe);
    window.addEventListener("resize", notifyIframeResize);
    return () => {
      window.clearTimeout(t1);
      window.clearTimeout(t2);
      ro?.disconnect();
      window.removeEventListener("resize", notifyIframeResize);
    };
  }, [state, notifyIframeResize]);

  const grantPermissions = useCallback(() => {
    if (state.status === "permissions") {
      setState({ status: "running", manifest: state.manifest, blobUrl: state.pkg.blobUrl });
    }
  }, [state]);

  const base: CSSProperties = embedded
    ? {
        width: "100%",
        height: "100%",
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        background: "#0c1020",
      }
    : fullscreen
      ? { width: "100%", height: "100%", display: "flex", flexDirection: "column", minHeight: 0 }
      : { maxWidth: 1200, margin: "0 auto", minHeight: "60vh", display: "flex", flexDirection: "column" };

  const muted = embedded ? "#6b7390" : "#9ca3af";
  const panelBg = "rgba(255,255,255,0.03)";
  const panelBorder = embedded ? "rgba(29,35,60,0.9)" : "rgba(255,255,255,0.08)";
  const accentBtn = embedded
    ? "linear-gradient(180deg, #f3c879, #e0a948)"
    : "linear-gradient(135deg, #67e8f9 0%, #22d3ee 100%)";
  const accentBtnText = embedded ? "#241a05" : "#080b0f";
  const iframeBg = embedded ? "#0c1020" : "#0c0f18";
  const font = embedded ? '"Hanken Grotesk", sans-serif' : "'DM Sans', sans-serif";

  if (state.status === "loading") {
    return (
      <div style={{ ...base, alignItems: "center", justifyContent: "center", padding: embedded ? 24 : 60 }}>
        <EmbedAppLoading embedded={embedded} />
      </div>
    );
  }

  if (state.status === "error") {
    return (
      <div style={{ ...base, alignItems: "center", justifyContent: "center", padding: embedded ? 24 : 60 }}>
        <div
          style={{
            padding: embedded ? "16px 20px" : "20px 28px",
            borderRadius: 12,
            background: "rgba(239,68,68,0.08)",
            border: "1px solid rgba(239,68,68,0.2)",
            textAlign: "center",
            maxWidth: 480,
            fontFamily: font,
          }}
        >
          <div style={{ fontSize: 15, fontWeight: 700, color: "#ef4444", marginBottom: 8 }}>
            Failed to load app
          </div>
          <div style={{ fontSize: 13, color: muted, lineHeight: 1.5 }}>{state.message}</div>
        </div>
      </div>
    );
  }

  if (state.status === "permissions") {
    return (
      <div style={{ ...base, alignItems: "center", justifyContent: "center", padding: embedded ? 24 : 60 }}>
        <div
          style={{
            padding: embedded ? "22px 24px" : "28px 32px",
            borderRadius: 16,
            background: panelBg,
            border: `1px solid ${panelBorder}`,
            maxWidth: 420,
            textAlign: "center",
            fontFamily: font,
          }}
        >
          <div
            style={{
              fontSize: 18,
              fontWeight: 700,
              color: embedded ? "#eef0f7" : "#f9fafb",
              marginBottom: 6,
            }}
          >
            {state.manifest.name}
          </div>
          <div style={{ fontSize: 12, color: muted, marginBottom: 16 }}>This app requests permissions:</div>
          <ul style={{ listStyle: "none", padding: 0, margin: "0 0 20px", textAlign: "left" }}>
            {state.manifest.permissions.map((p, i) => (
              <li
                key={i}
                style={{
                  padding: "6px 12px",
                  marginBottom: 4,
                  borderRadius: 8,
                  background: embedded ? "rgba(116,224,168,0.08)" : "rgba(103,232,249,0.06)",
                  border: embedded ? "1px solid rgba(116,224,168,0.18)" : "1px solid rgba(103,232,249,0.15)",
                  fontSize: 12,
                  color: embedded ? "#eef0f7" : "#e5e7eb",
                }}
              >
                {typeof p === "string" ? p : JSON.stringify(p)}
              </li>
            ))}
          </ul>
          <button
            type="button"
            onClick={grantPermissions}
            style={{
              background: accentBtn,
              color: accentBtnText,
              border: "none",
              borderRadius: 10,
              padding: "10px 28px",
              fontSize: 13,
              fontWeight: 700,
              cursor: "pointer",
              fontFamily: font,
            }}
          >
            Grant & Launch
          </button>
        </div>
      </div>
    );
  }

  return (
    <div style={base}>
      <iframe
        ref={iframeRef}
        src={`${state.blobUrl}${typeof window !== "undefined" ? window.location.hash : ""}`}
        sandbox="allow-scripts allow-same-origin allow-modals"
        style={{
          flex: 1,
          width: "100%",
          minHeight: embedded || fullscreen ? 0 : "60vh",
          border: "none",
          borderRadius: embedded || fullscreen ? 0 : 12,
          background: iframeBg,
        }}
        title={state.manifest.name}
        onLoad={notifyIframeResize}
      />
    </div>
  );
};

export default SpacekitAppFrame;
