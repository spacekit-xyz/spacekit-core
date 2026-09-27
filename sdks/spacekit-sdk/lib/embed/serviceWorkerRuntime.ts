/**
 * @deprecated Serves apps from the host's own origin, so an app can read the
 * host's storage and session. `mountSpacekitApp` / `SpacekitAppFrame` isolate
 * apps instead and no longer call this; under a cross-origin-isolated host the
 * isolated frame still gets SharedArrayBuffer and WASM threads.
 *
 * Service-worker app runtime (page side).
 *
 * Produces a {@link SpacekitPackageLoader}-compatible loader that serves a
 * verified web package from a real, same-origin, cross-origin-isolated path
 * (`/app-runtime/<token>/…`) instead of a single in-memory `blob:` URL.
 *
 * Flow:
 *   1. Register `/spacekit-app-sw.js` (scope `/app-runtime/`) once, and wait for
 *      it to activate (it calls `clients.claim()`).
 *   2. Fetch + SHA-256-verify the package with {@link loadVerifiedPackageFiles}
 *      (the exact same integrity path as the classic loader).
 *   3. Inject the SDK bridge into the HTML entry (no blob rewriting — assets stay
 *      relative and resolve against the served path), and write every verified
 *      file into the Cache API with `Content-Type`, `Cross-Origin-Embedder-Policy:
 *      require-corp`, and `Cross-Origin-Resource-Policy: same-origin` headers.
 *   4. Return a {@link LoadedWebPackage} whose `blobUrl` is the served URL.
 *
 * The result: the app document is cross-origin isolated (SharedArrayBuffer + WASM
 * threads), its bytes stream and cache across sessions, and `instantiateStreaming`
 * works — the things a `blob:` iframe can never get.
 *
 * If service workers, the Cache API, or a secure context are unavailable, or if
 * anything fails, this transparently falls back to the classic blob loader, so it
 * is always safe to pass as the host's `loadPackage`.
 */

import {
  loadVerifiedPackageFiles,
  loadWebPackage,
  loadWebPackageFromLocal,
  type LoadWebPackageOptions,
  type VerifiedPackageFile,
} from "./packageLoader.js";
import { injectSdkBridgeIntoHtml } from "./injectShim.js";
import type { AppManifest, LoadedWebPackage } from "./types.js";

const DEFAULT_SW_URL = "/spacekit-app-sw.js";
const SCOPE = "/app-runtime/";
const CACHE_PREFIX = "spacekit-app:";
/** How many app mounts to keep cached at once before evicting the oldest. */
const MAX_MOUNTS = 4;

export function isServiceWorkerRuntimeSupported(): boolean {
  return (
    typeof navigator !== "undefined" &&
    "serviceWorker" in navigator &&
    typeof caches !== "undefined" &&
    typeof window !== "undefined" &&
    window.isSecureContext !== false
  );
}

let registration: Promise<ServiceWorkerRegistration> | null = null;

function ensureRegistered(swUrl: string): Promise<ServiceWorkerRegistration> {
  if (!registration) {
    registration = navigator.serviceWorker.register(swUrl, { scope: SCOPE }).catch((err) => {
      registration = null; // allow a later retry
      throw err;
    });
  }
  return registration;
}

/** Resolve once the registration's worker is activated (so it controls the scope). */
async function whenActivated(reg: ServiceWorkerRegistration): Promise<void> {
  if (reg.active) return;
  const sw = reg.installing ?? reg.waiting;
  if (!sw) return;
  await new Promise<void>((resolve) => {
    const onChange = () => {
      if (sw.state === "activated" || sw.state === "redundant") {
        sw.removeEventListener("statechange", onChange);
        resolve();
      }
    };
    sw.addEventListener("statechange", onChange);
    if (sw.state === "activated") {
      sw.removeEventListener("statechange", onChange);
      resolve();
    }
  });
}

const mountOrder: string[] = [];
const lastTokenByApp = new Map<string, string>();

function isolationHeaders(mime: string): HeadersInit {
  return {
    "Content-Type": mime,
    "Cross-Origin-Embedder-Policy": "require-corp",
    "Cross-Origin-Resource-Policy": "same-origin",
    "Cache-Control": "no-cache",
  };
}

async function writeMount(
  token: string,
  appIdHex: string,
  entryPath: string,
  indexHtml: string,
  files: Map<string, VerifiedPackageFile>,
): Promise<void> {
  const cache = await caches.open(CACHE_PREFIX + token);
  const base = `${SCOPE}${token}/`;

  const puts: Promise<void>[] = [];
  for (const [path, file] of files) {
    if (path === entryPath) continue; // replaced by the bridge-injected document below
    // Copy into a fresh Uint8Array so the body type is Uint8Array<ArrayBuffer>
    // (not <ArrayBufferLike>), which BodyInit requires under TS 5.7+ lib types.
    puts.push(
      cache.put(base + path, new Response(new Uint8Array(file.bytes), { headers: isolationHeaders(file.mime) })),
    );
  }
  puts.push(cache.put(base + entryPath, new Response(indexHtml, { headers: isolationHeaders("text/html") })));
  if (entryPath !== "index.html") {
    // Directory-root fallback (`/app-runtime/<token>/`).
    puts.push(cache.put(base + "index.html", new Response(indexHtml, { headers: isolationHeaders("text/html") })));
  }
  await Promise.all(puts);

  // Evict a previous mount of the same app, then cap the total.
  const prev = lastTokenByApp.get(appIdHex);
  if (prev && prev !== token) await caches.delete(CACHE_PREFIX + prev).catch(() => {});
  lastTokenByApp.set(appIdHex, token);

  mountOrder.push(token);
  while (mountOrder.length > MAX_MOUNTS) {
    const old = mountOrder.shift();
    if (old && old !== token) await caches.delete(CACHE_PREFIX + old).catch(() => {});
  }
}

function findHtmlEntryPath(manifest: AppManifest): string | null {
  const eps = manifest.entry_points as Array<Record<string, unknown>>;
  const main = eps.find((ep) => "Html" in ep && (ep.Html as { is_main?: boolean })?.is_main);
  const entry = main ?? eps.find((ep) => "Html" in ep);
  if (entry && "Html" in entry) {
    const path = (entry.Html as { path?: string }).path;
    return typeof path === "string" ? path : null;
  }
  return null;
}

/** Runtime tier an app opts into, deciding how heavily the host invests in it. */
export type AppRuntimeTier = "realtime" | "document";

/** Manifest signals that mark an app as needing the realtime (game) runtime. */
const REALTIME_HINTS = new Set([
  "realtime",
  "real-time",
  "game",
  "games",
  "gaming",
  "webgl",
  "webgpu",
  "wasm-threads",
  "threads",
  "fullscreen",
  "high-fps",
  "60fps",
]);

/**
 * Resolve an app's runtime tier from its manifest.
 *
 * A publisher opts a game into the realtime runtime (isolated origin, streaming,
 * SharedArrayBuffer / WASM threads) with an explicit `runtime`/`profile`/`tier`
 * of `"realtime"`/`"game"`, or a matching permission, keyword, tag, or category.
 * Everything else is a `"document"` app that keeps the lightweight blob path.
 */
export function resolveRuntimeTier(manifest: AppManifest): AppRuntimeTier {
  const m = manifest as unknown as Record<string, unknown>;

  const explicit = String(m.runtime ?? m.profile ?? m.tier ?? "").trim().toLowerCase();
  if (explicit === "realtime" || explicit === "game") return "realtime";
  if (explicit === "document" || explicit === "static") return "document";

  const bag: string[] = [];
  const collect = (value: unknown): void => {
    if (!Array.isArray(value)) return;
    for (const item of value) {
      if (typeof item === "string") bag.push(item.toLowerCase());
      else if (item && typeof item === "object") {
        const tag =
          (item as { type?: unknown }).type ??
          (item as { name?: unknown }).name ??
          (item as { permission?: unknown }).permission;
        if (typeof tag === "string") bag.push(tag.toLowerCase());
      }
    }
  };
  collect(manifest.permissions);
  collect(m.keywords);
  collect(m.tags);
  collect(m.categories);
  if (typeof m.category === "string") bag.push(m.category.toLowerCase());

  return bag.some((s) => REALTIME_HINTS.has(s)) ? "realtime" : "document";
}

export interface ServiceWorkerLoaderConfig {
  /** Path the worker script is served from. Defaults to `/spacekit-app-sw.js`. */
  swUrl?: string;
  /**
   * When true (default), only apps whose manifest declares the realtime/game
   * tier are served through the service worker; every other app keeps the
   * lightweight blob path. Set false to force the SW runtime for every app
   * (useful for testing).
   */
  tierGate?: boolean;
  /** Loader used when the SW runtime is unavailable or fails. Defaults to the blob loader. */
  fallback?: (
    storageOrigin: string,
    appId: string,
    options: LoadWebPackageOptions,
  ) => Promise<LoadedWebPackage>;
}

/**
 * Build a `loadPackage` that serves apps through the service-worker runtime,
 * falling back to the classic blob loader when unsupported.
 */
export function createSpacekitServiceWorkerLoader(config: ServiceWorkerLoaderConfig = {}) {
  const swUrl = config.swUrl ?? DEFAULT_SW_URL;
  const tierGate = config.tierGate ?? true;
  const fallback = config.fallback ?? loadWebPackage;

  return async function serviceWorkerAppLoader(
    storageOrigin: string,
    appId: string,
    options: LoadWebPackageOptions,
  ): Promise<LoadedWebPackage> {
    if (!isServiceWorkerRuntimeSupported()) {
      return fallback(storageOrigin, appId, options);
    }

    try {
      const { pkg, appId: appIdHex, creatorDid, files, integrityErrors } =
        await loadVerifiedPackageFiles(storageOrigin, appId);

      // Tiered runtime: only realtime/game-tier apps get the service-worker path
      // (isolated origin, streaming, threads). Document/basic apps keep the
      // lightweight blob bundle — built here from the bytes we already verified,
      // so there is no second network fetch and no worker is registered for them.
      // `tierGate: false` forces the SW runtime for every app.
      if (tierGate && resolveRuntimeTier(pkg.manifest) !== "realtime") {
        const localFiles: Record<string, Uint8Array> = {};
        for (const [path, file] of files) localFiles[path] = file.bytes;
        return loadWebPackageFromLocal(pkg, localFiles, options);
      }

      const entryPath = findHtmlEntryPath(pkg.manifest);
      if (!entryPath || !files.has(entryPath)) {
        // Nothing servable as a document — let the classic loader render its
        // own error/fallback page for this app.
        return fallback(storageOrigin, appId, options);
      }

      // Register + activate the worker only now that we know this app uses it.
      const reg = await ensureRegistered(swUrl);
      await whenActivated(reg);

      const rawHtml = new TextDecoder().decode(files.get(entryPath)!.bytes);
      const indexHtml = injectSdkBridgeIntoHtml(rawHtml, new Map(), {
        appId: appIdHex,
        parentOrigin: options.parentOrigin,
        endpoints: options.endpoints,
        identityDid: options.identityDid,
        contentFit: options.contentFit,
        sameOriginPassthrough: true,
      });

      const token = `${appIdHex}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
      await writeMount(token, appIdHex, entryPath, indexHtml, files);

      return {
        manifest: pkg.manifest,
        appId: appIdHex,
        creatorDid,
        // The frame renders this as the iframe `src`; a real path, not a blob.
        blobUrl: `${SCOPE}${token}/${entryPath}`,
        assetUrls: new Map(),
        blobAssetUrls: [],
        integrityErrors,
      };
    } catch (err) {
      if (typeof console !== "undefined") {
        console.warn(
          "[spacekit] service-worker app runtime unavailable; using blob loader:",
          err,
        );
      }
      return fallback(storageOrigin, appId, options);
    }
  };
}

/** Ready-to-use loader with default configuration. */
export const spacekitServiceWorkerLoader = createSpacekitServiceWorkerLoader();
