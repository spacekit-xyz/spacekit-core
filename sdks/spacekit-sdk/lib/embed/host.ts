/**
 * Framework-agnostic app host.
 *
 * `mountSpacekitApp` fetches and verifies a web package, applies the host's
 * trust policy, asks the viewer to grant declared permissions, and runs the app
 * in an isolated frame connected to the host bridge by a private MessagePort.
 * The React `SpacekitAppFrame` and the `<spacekit-app>` custom element are thin
 * wrappers around it.
 *
 * Isolation modes:
 *   - "opaque" (default): sandbox without `allow-same-origin`. The app runs in
 *     an opaque origin and cannot read the host's storage, cookies, or DOM.
 *     Direct Web Storage is shimmed; IndexedDB, cookies and service workers are
 *     unavailable. Cross-origin isolation (SharedArrayBuffer) is inherited from
 *     the host page when the host is itself cross-origin isolated.
 *   - "origin": the app runs on a separate origin you operate (`appOrigin`),
 *     served by the frame-host page from `renderFrameHostHtml`. Apps get real
 *     storage. Use a per-app origin template to keep apps apart from each other.
 *   - "unsafe-same-origin": legacy behavior. The app runs with the host's
 *     origin and can read everything the host can. Only for code you wrote.
 */

import { configureBridgeOwner, type EmbeddedSdkBridge } from "./bridge.js";
import {
  createCapabilityGuard,
  defaultTrustedOrigins,
  parseManifestPermissions,
  type CapabilityPolicy,
} from "./capabilities.js";
import { buildOpaqueBootstrapHtml } from "./frameBootstrap.js";
import { buildFramePayload } from "./framePayload.js";
import { loadVerifiedPackageFiles, type VerifiedWebPackageFiles } from "./packageLoader.js";
import {
  FRAME_LOAD,
  LOCAL_STORAGE_SHIM_PREFIX,
  SPACEKIT_EMBED_PROTOCOL_VERSION,
  isFrameErrorMessage,
  isFrameReadyMessage,
  isPortCallMessage,
  type FrameLoadMessage,
  type PortHostMessage,
} from "./protocol.js";
import type { AppManifest, EmbedEndpoints, EmbedHostServices } from "./types.js";

export type IsolationMode = "opaque" | "origin" | "unsafe-same-origin";

export interface AppTrustInfo {
  appId: string;
  creatorDid: string;
  manifest: AppManifest;
  storageOrigin: string;
}

/**
 * Decide whether an app may run. Return `true` to allow, `false` or a string
 * (shown as the reason) to refuse.
 */
export type TrustPolicy = (info: AppTrustInfo) => boolean | string | Promise<boolean | string>;

export interface PermissionRequest extends AppTrustInfo {
  /** Human-readable lines describing what the app asks for. */
  permissions: string[];
  /** The raw manifest entries. */
  rawPermissions: unknown[];
}

export type VerifiedFilesLoader = (storageOrigin: string, appId: string) => Promise<VerifiedWebPackageFiles>;

export type AppMountState =
  | { status: "loading" }
  | { status: "permissions"; manifest: AppManifest; permissions: string[] }
  | { status: "running"; manifest: AppManifest }
  | { status: "error"; message: string };

export interface MountSpacekitAppOptions {
  appId: string;
  /** Storage node origin that serves the package. */
  storageOrigin: string;
  /** Storage origin passed to the SDK bridge (defaults to `storageOrigin`). */
  bridgeStorageOrigin?: string;
  services: EmbedHostServices;
  acquireBridge: (appId: string, storageOrigin: string, manifestName: string) => EmbeddedSdkBridge;
  endpoints?: EmbedEndpoints;
  /** Origin reported to the app in its embed config. Defaults to the host page origin. */
  parentOrigin?: string;
  contentFit?: "fill" | "contain";
  /** Defaults to `"origin"` when `appOrigin` is set, otherwise `"opaque"`. */
  isolation?: IsolationMode;
  /**
   * App origin for `isolation: "origin"`. A string origin, a template containing
   * `{app}` (replaced with the first 32 hex chars of the app id, one DNS label),
   * or a function of the app id.
   */
  appOrigin?: string | ((appIdHex: string) => string);
  /** Path of the frame-host page on the app origin. Default `/spacekit-frame.html`. */
  frameHostPath?: string;
  capabilities?: CapabilityPolicy;
  trustPolicy?: TrustPolicy;
  /**
   * Consent UI for declared permissions. Resolve `true` to grant. When absent,
   * apps that declare permissions are refused.
   */
  requestPermissions?: (request: PermissionRequest) => boolean | Promise<boolean>;
  /** Override package loading (e.g. a Desktop encrypted cache). Must return verified files. */
  loadFiles?: VerifiedFilesLoader;
  onStateChange?: (state: AppMountState) => void;
  /**
   * How long to wait for the frame to answer after it is inserted. Covers a
   * frame host that is unreachable or refuses this host. Default 30000 ms.
   */
  readyTimeoutMs?: number;
  /** Extra iframe attributes. */
  frame?: {
    className?: string;
    title?: string;
    style?: Partial<Record<string, string>>;
    /** Permissions-Policy `allow` attribute. */
    allow?: string;
    /** Appended to the frame URL (e.g. the host page's `location.hash`). */
    hash?: string;
  };
}

export interface SpacekitAppHandle {
  readonly state: AppMountState;
  readonly iframe: HTMLIFrameElement | null;
  /** Resolves once the app is running, rejects if it fails to start. */
  readonly ready: Promise<void>;
  unmount(): void;
}

const BASE_SANDBOX = "allow-scripts allow-forms allow-modals allow-pointer-lock allow-downloads";
const DEFAULT_ALLOW = "fullscreen; autoplay; gamepad; cross-origin-isolated";
const DEFAULT_FRAME_HOST_PATH = "/spacekit-frame.html";

function hostOrigin(): string {
  return typeof window !== "undefined" ? window.location.origin : "";
}

/** Resolve an `appOrigin` option to a concrete origin for one app. */
export function resolveAppOrigin(
  appOrigin: NonNullable<MountSpacekitAppOptions["appOrigin"]>,
  appIdHex: string,
): string {
  const label = appIdHex.toLowerCase().replace(/[^0-9a-f]/g, "").slice(0, 32);
  const raw = typeof appOrigin === "function" ? appOrigin(appIdHex) : appOrigin.replace(/\{app\}/g, label);
  const origin = new URL(raw).origin;
  if (origin === "null") throw new Error(`Invalid app origin: ${raw}`);
  return origin;
}

function describeError(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

async function collectLocalSeed(bridge: EmbeddedSdkBridge): Promise<Record<string, string> | null> {
  try {
    const keys = await bridge.handle("storage", "list", { prefix: LOCAL_STORAGE_SHIM_PREFIX });
    if (!Array.isArray(keys)) return null;
    const seed: Record<string, string> = {};
    await Promise.all(
      keys.map(async (k) => {
        const key = String(k);
        if (!key.startsWith(LOCAL_STORAGE_SHIM_PREFIX)) return;
        const value = await bridge.handle("storage", "get", { key });
        if (value != null) seed[key.slice(LOCAL_STORAGE_SHIM_PREFIX.length)] = String(value);
      }),
    );
    return seed;
  } catch {
    return null;
  }
}

/**
 * Mount a SpaceKit web package into `container`.
 */
export function mountSpacekitApp(container: HTMLElement, options: MountSpacekitAppOptions): SpacekitAppHandle {
  const isolation: IsolationMode = options.isolation ?? (options.appOrigin ? "origin" : "opaque");
  let state: AppMountState = { status: "loading" };
  let iframe: HTMLIFrameElement | null = null;
  let port: MessagePort | null = null;
  let bridge: EmbeddedSdkBridge | null = null;
  let bootstrapUrl: string | null = null;
  let onWindowMessage: ((e: MessageEvent) => void) | null = null;
  let readyTimer: ReturnType<typeof setTimeout> | null = null;
  let disposed = false;

  let resolveReady!: () => void;
  let rejectReady!: (err: Error) => void;
  const ready = new Promise<void>((res, rej) => {
    resolveReady = res;
    rejectReady = rej;
  });
  ready.catch(() => {}); // callers may ignore it

  const setState = (next: AppMountState) => {
    if (disposed) return;
    state = next;
    options.onStateChange?.(next);
    if (next.status === "running") resolveReady();
    if (next.status === "error") rejectReady(new Error(next.message));
  };

  const fail = (message: string) => {
    teardownFrame();
    setState({ status: "error", message });
  };

  function teardownFrame() {
    if (readyTimer) clearTimeout(readyTimer);
    readyTimer = null;
    if (onWindowMessage) window.removeEventListener("message", onWindowMessage);
    onWindowMessage = null;
    port?.close();
    port = null;
    iframe?.remove();
    iframe = null;
    if (bootstrapUrl) URL.revokeObjectURL(bootstrapUrl);
    bootstrapUrl = null;
  }

  async function start() {
    const loader = options.loadFiles ?? loadVerifiedPackageFiles;
    const verified = await loader(options.storageOrigin, options.appId);
    if (disposed) return;
    if (verified.integrityErrors.length > 0) {
      fail(`Integrity failed: ${verified.integrityErrors.join(", ")}`);
      return;
    }
    const manifest = verified.pkg.manifest;
    const info: AppTrustInfo = {
      appId: verified.appId,
      creatorDid: verified.creatorDid,
      manifest,
      storageOrigin: options.storageOrigin,
    };

    if (options.trustPolicy) {
      const verdict = await options.trustPolicy(info);
      if (disposed) return;
      if (verdict !== true) {
        fail(typeof verdict === "string" && verdict ? verdict : "This host does not allow this app");
        return;
      }
    }

    const rawPermissions = Array.isArray(manifest.permissions) ? manifest.permissions : [];
    const declared = parseManifestPermissions(rawPermissions);
    if (rawPermissions.length > 0) {
      const labels = declared.labels.length ? declared.labels : rawPermissions.map((p) => String(p));
      setState({ status: "permissions", manifest, permissions: labels });
      const granted = options.requestPermissions
        ? await options.requestPermissions({ ...info, permissions: labels, rawPermissions })
        : false;
      if (disposed) return;
      if (!granted) {
        fail(
          options.requestPermissions
            ? "Permissions were not granted"
            : "This app requests permissions, and this host has no consent prompt",
        );
        return;
      }
    }

    // The bridge exists only once the app is allowed to run.
    const bridgeOrigin = options.bridgeStorageOrigin ?? options.storageOrigin;
    bridge = options.acquireBridge(options.appId, bridgeOrigin, manifest.name);
    configureBridgeOwner(bridge, verified.creatorDid);
    await bridge.ensureHydrated();
    if (disposed) return;

    const policy: CapabilityPolicy = {
      ...options.capabilities,
      trustedOrigins:
        options.capabilities?.trustedOrigins ??
        defaultTrustedOrigins([
          options.endpoints?.apiBase,
          options.endpoints?.messagingBase,
          options.endpoints?.reposApiBase,
          options.endpoints?.workspacesApiBase,
        ]),
    };
    const guard = createCapabilityGuard({ manifestPermissions: rawPermissions, policy });

    const parentOrigin = options.parentOrigin ?? hostOrigin();
    const endpoints: EmbedEndpoints = {
      ...options.endpoints,
      wasmUrl: options.endpoints?.wasmUrl ?? `${hostOrigin()}/wasm/kyber_wasm_bg.wasm`,
    };
    const payload = buildFramePayload(verified, {
      parentOrigin,
      endpoints,
      identityDid: options.services.getIdentityDid(),
      contentFit: options.contentFit,
    });
    const localSeed = isolation === "origin" ? null : await collectLocalSeed(bridge);
    if (disposed) return;

    // Frame URL, sandbox, and the origin the frame's messages must come from.
    let src: string;
    let sandbox = BASE_SANDBOX;
    let expectedOrigin: string;
    let loadTarget: string;
    if (isolation === "origin") {
      if (!options.appOrigin) throw new Error('isolation "origin" requires appOrigin');
      const appOrigin = resolveAppOrigin(options.appOrigin, verified.appId);
      if (appOrigin === hostOrigin()) {
        throw new Error("appOrigin must differ from the host origin, or apps can read host storage");
      }
      src = `${appOrigin}${options.frameHostPath ?? DEFAULT_FRAME_HOST_PATH}`;
      sandbox += " allow-same-origin";
      expectedOrigin = appOrigin;
      loadTarget = appOrigin;
    } else {
      bootstrapUrl = URL.createObjectURL(
        new Blob([buildOpaqueBootstrapHtml(hostOrigin())], { type: "text/html" }),
      );
      src = bootstrapUrl;
      if (isolation === "unsafe-same-origin") {
        console.warn(
          "[spacekit] isolation \"unsafe-same-origin\": the app can read this page's storage and session. Use only for code you trust.",
        );
        sandbox += " allow-same-origin";
        expectedOrigin = hostOrigin();
        loadTarget = hostOrigin();
      } else {
        expectedOrigin = "null";
        loadTarget = "*"; // an opaque origin cannot be named; the target window is checked below
      }
    }
    if (options.frame?.hash) src += options.frame.hash.startsWith("#") ? options.frame.hash : `#${options.frame.hash}`;

    const frame = document.createElement("iframe");
    frame.setAttribute("sandbox", sandbox);
    frame.setAttribute("allow", options.frame?.allow ?? DEFAULT_ALLOW);
    frame.setAttribute("referrerpolicy", "no-referrer");
    frame.title = options.frame?.title ?? manifest.name ?? "SpaceKit app";
    if (options.frame?.className) frame.className = options.frame.className;
    Object.assign(frame.style, { border: "none", width: "100%", height: "100%", display: "block" }, options.frame?.style ?? {});

    const activeBridge = bridge;
    const connect = () => {
      port?.close();
      const channel = new MessageChannel();
      port = channel.port1;
      const hostPort = channel.port1;
      hostPort.onmessage = (e: MessageEvent) => {
        const data = e.data;
        if (data && (data as { t?: unknown }).t === "loaded") {
          if (readyTimer) clearTimeout(readyTimer);
          readyTimer = null;
          if (state.status !== "running") setState({ status: "running", manifest });
          return;
        }
        if (!isPortCallMessage(data)) return;
        const reply = (msg: PortHostMessage) => {
          try {
            hostPort.postMessage(msg);
          } catch {
            /* port closed */
          }
        };
        Promise.resolve()
          .then(() => guard(data.module, data.method, (data.params ?? {}) as Record<string, unknown>))
          .then((params) => activeBridge.handle(data.module, data.method, params))
          .then(
            (result) => reply({ t: "res", id: data.id, result }),
            (err) => reply({ t: "res", id: data.id, error: describeError(err) }),
          );
      };
      activeBridge.setPushHandler((topic, msg) => {
        try {
          hostPort.postMessage({ t: "event", topic, msg } satisfies PortHostMessage);
        } catch {
          /* port closed */
        }
      });
      const load: FrameLoadMessage = {
        type: FRAME_LOAD,
        v: SPACEKIT_EMBED_PROTOCOL_VERSION,
        appId: verified.appId,
        html: payload.html,
        files: payload.files,
        localSeed,
      };
      frame.contentWindow?.postMessage(load, loadTarget, [channel.port2]);
    };

    onWindowMessage = (e: MessageEvent) => {
      if (!frame.contentWindow || e.source !== frame.contentWindow) return;
      if (e.origin !== expectedOrigin) return;
      if (isFrameErrorMessage(e.data)) {
        if (state.status !== "running") fail(`The app frame refused to load this app: ${e.data.message}`);
        return;
      }
      if (!isFrameReadyMessage(e.data)) return;
      // A reload inside the frame re-runs the bootstrap; each run gets a fresh
      // port. The state turns "running" when the frame acknowledges the load.
      connect();
    };
    window.addEventListener("message", onWindowMessage);

    frame.src = src;
    iframe = frame;
    container.appendChild(frame);
    readyTimer = setTimeout(() => {
      readyTimer = null;
      if (state.status !== "running") {
        fail(
          isolation === "origin"
            ? `The app frame at ${src} did not respond. Check that it is deployed and allows this host.`
            : "The app frame did not respond",
        );
      }
    }, options.readyTimeoutMs ?? 30_000);
  }

  start().catch((err) => {
    if (!disposed) fail(describeError(err));
  });
  options.onStateChange?.(state);

  return {
    get state() {
      return state;
    },
    get iframe() {
      return iframe;
    },
    ready,
    unmount() {
      if (disposed) return;
      teardownFrame();
      bridge?.flush();
      bridge = null;
      disposed = true;
    },
  };
}
