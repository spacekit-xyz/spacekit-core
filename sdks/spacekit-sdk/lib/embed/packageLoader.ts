import { injectSdkBridgeIntoHtml } from "./injectShim.js";
import type { AppPackageJSON, ContentRef, EmbedEndpoints, LoadedWebPackage } from "./types.js";

function parseCreatorDid(value: unknown): string {
  if (typeof value === "string") return value.trim();
  if (value && typeof value === "object" && "did" in value) {
    const did = (value as { did?: unknown }).did;
    return typeof did === "string" ? did.trim() : "";
  }
  return "";
}

function mimeFromPath(path: string): string {
  if (/\.m?[jt]sx?$/i.test(path)) return "application/javascript";
  if (path.endsWith(".css")) return "text/css";
  return "application/octet-stream";
}

function mimeFor(ct: string | Record<string, unknown>): string {
  if (typeof ct === "string") {
    const m: Record<string, string> = {
      Wasm: "application/wasm",
      Html: "text/html",
      Css: "text/css",
      JavaScript: "application/javascript",
      TypeScript: "application/javascript",
      Json: "application/json",
      Markdown: "text/markdown",
    };
    return m[ct] || "application/octet-stream";
  }
  if ("Image" in ct) return `image/${(ct.Image as { format: string }).format}`;
  return "application/octet-stream";
}

function toHex(v: string | number[]): string {
  if (typeof v === "string") return v;
  return v.map((b) => b.toString(16).padStart(2, "0")).join("");
}

async function sha256Hex(data: Uint8Array): Promise<string> {
  if (typeof crypto === "undefined" || !crypto.subtle) {
    throw new Error("SHA-256 verification is unavailable in this environment");
  }
  const hash = await crypto.subtle.digest("SHA-256", Uint8Array.from(data));
  return Array.from(new Uint8Array(hash)).map((b) => b.toString(16).padStart(2, "0")).join("");
}

function contentHashHex(hash: string | number[]): string | null {
  if (typeof hash === "string") {
    const normalized = hash.trim().toLowerCase();
    return /^[0-9a-f]{64}$/.test(normalized) ? normalized : null;
  }
  if (
    hash.length === 32 &&
    hash.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
  ) {
    return hash.map((byte) => byte.toString(16).padStart(2, "0")).join("");
  }
  return null;
}

function uint8ToBase64(bytes: Uint8Array): string {
  const chunkSize = 0x8000;
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i += chunkSize) {
    parts.push(String.fromCharCode(...bytes.subarray(i, i + chunkSize)));
  }
  return btoa(parts.join(""));
}

function findPackagedWasmBase64(
  assetUrls: Map<string, string>,
  wasmBytes: Map<string, Uint8Array>,
): string | undefined {
  for (const path of assetUrls.keys()) {
    if (!path.endsWith(".wasm")) continue;
    const bytes = wasmBytes.get(path);
    if (bytes) return uint8ToBase64(bytes);
  }
  return undefined;
}

async function fetchWithRetry(
  url: string,
  init: RequestInit,
  attempts = 4,
): Promise<Response> {
  let last: Response | null = null;
  for (let i = 0; i < attempts; i++) {
    const res = await fetch(url, init);
    last = res;
    if (res.ok || res.status === 404 || res.status === 401 || res.status === 403) {
      return res;
    }
    // Storage-node rate limit surfaces as 500 "Exceeded" — retrying makes it worse.
    if (res.status === 429) return res;
    if (res.status >= 500) {
      const peek = await res.clone().text().catch(() => "");
      if (/exceeded/i.test(peek) || /rate.?limit/i.test(peek)) return res;
      if (i < attempts - 1) {
        await new Promise((r) => setTimeout(r, 200 * (i + 1)));
        continue;
      }
    }
    return res;
  }
  return last!;
}

async function fetchContentRefBytes(
  storageBase: string,
  factIdHex: string,
  fetchOpts: RequestInit,
): Promise<Uint8Array | null> {
  const streamUrl = `${storageBase}/facts/${encodeURIComponent(factIdHex)}/stream`;
  const streamRes = await fetchWithRetry(streamUrl, fetchOpts);
  if (streamRes.ok) {
    const buf = await streamRes.arrayBuffer();
    return new Uint8Array(buf);
  }
  const metaRes = await fetchWithRetry(
    `${storageBase}/facts/${encodeURIComponent(factIdHex)}`,
    fetchOpts,
  );
  if (!metaRes.ok) return null;
  const factJson = await metaRes.json();
  return extractBinaryFromFact(factJson);
}

function extractBinaryFromFact(fact: Record<string, unknown>): Uint8Array | null {
  const content = fact.content as Record<string, unknown> | undefined;
  if (!content) return null;
  if ("Binary" in content) {
    const bin = (content as { Binary: { data: number[] } }).Binary;
    if (Array.isArray(bin.data)) return new Uint8Array(bin.data);
  }
  return null;
}

function errPage(name: string, msg: string): string {
  return `<!DOCTYPE html><html><head><meta charset="utf-8"><title>${name}</title></head>
<body style="background:#0c0f18;color:#e5e7eb;font-family:'DM Sans',sans-serif;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0">
<div style="text-align:center"><h2>${name}</h2><p style="color:#ef4444">${msg}</p></div></body></html>`;
}

export interface LoadWebPackageOptions {
  parentOrigin: string;
  endpoints: EmbedEndpoints;
  identityDid: string | null;
  contentFit?: "fill" | "contain";
}

export async function loadWebPackage(
  storageBase: string,
  appId: string,
  options: LoadWebPackageOptions,
): Promise<LoadedWebPackage> {
  const fetchOpts: RequestInit = { cache: "no-store" };
  const packageRes = await fetch(
    `${storageBase}/packages/apps/${encodeURIComponent(appId)}`,
    fetchOpts,
  );
  if (packageRes.status === 200) {
    // Dynamic import keeps the strict SPKG loader's local-package integration
    // from creating an unsafe eager packageLoader <-> spkg module cycle.
    const { loadWebPackageFromSpkg } = await import("./spkg.js");
    return loadWebPackageFromSpkg(await packageRes.arrayBuffer(), options, appId);
  }
  if (![404, 405, 501].includes(packageRes.status)) {
    throw new Error(
      `Package fetch failed (${packageRes.status}${packageRes.statusText ? ` ${packageRes.statusText}` : ""})`,
    );
  }

  const manifestRes = await fetchWithRetry(
    `${storageBase}/facts/${encodeURIComponent(appId)}`,
    fetchOpts,
  );
  if (!manifestRes.ok) throw new Error(`App manifest fetch failed (${manifestRes.status})`);
  const manifestFact = await manifestRes.json();

  const factContent = manifestFact.content;
  let pkg: AppPackageJSON;
  if (factContent?.Json?.data) {
    pkg = factContent.Json.data as AppPackageJSON;
  } else if (manifestFact.manifest) {
    pkg = manifestFact as unknown as AppPackageJSON;
  } else {
    throw new Error("Unexpected manifest format — no Json.data or manifest field");
  }

  return assembleWebPackage(pkg, options, async (ref) => {
    const factIdHex = toHex(ref.fact_id);
    if (!factIdHex || factIdHex === "0".repeat(64)) return null;
    return fetchContentRefBytes(storageBase, factIdHex, fetchOpts);
  });
}

/**
 * Build a runnable package from in-memory `.spkg` JSON + file bytes (Desktop Projects preview).
 * `files` maps package-relative paths to raw bytes (Uint8Array or base64 string).
 */
export async function loadWebPackageFromLocal(
  pkg: AppPackageJSON,
  files: Record<string, Uint8Array | string>,
  options: LoadWebPackageOptions,
): Promise<LoadedWebPackage> {
  const decoded = new Map<string, Uint8Array>();
  for (const [path, value] of Object.entries(files)) {
    if (typeof value === "string") {
      const bin = atob(value);
      const bytes = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      decoded.set(path, bytes);
    } else {
      decoded.set(path, value);
    }
  }
  return assembleWebPackage(pkg, options, async (ref) => decoded.get(ref.path) ?? null);
}

async function assembleWebPackage(
  pkg: AppPackageJSON,
  options: LoadWebPackageOptions,
  resolveBytes: (ref: ContentRef) => Promise<Uint8Array | null>,
): Promise<LoadedWebPackage> {
  const appId = pkg.app_id ? toHex(pkg.app_id) : "local-preview";
  const assetUrls = new Map<string, string>();
  const blobAssetUrls: string[] = [];
  const wasmBytes = new Map<string, Uint8Array>();
  const integrityErrors: string[] = [];

  await Promise.all(
    pkg.content_refs.map(async (ref) => {
      const bytes = await resolveBytes(ref);
      if (!bytes) {
        integrityErrors.push(`${ref.path} (missing)`);
        return;
      }

      const expectedHash = contentHashHex(ref.hash);
      if (!expectedHash) {
        integrityErrors.push(`${ref.path} (invalid expected SHA-256)`);
        return;
      }
      const actualHash = await sha256Hex(bytes);
      if (actualHash !== expectedHash) {
        integrityErrors.push(
          `${ref.path} (SHA-256 mismatch: expected ${expectedHash}, got ${actualHash})`,
        );
        return;
      }

      if (ref.path.endsWith(".wasm")) {
        wasmBytes.set(ref.path, bytes);
      }

      let mime = mimeFor(ref.content_type);
      if (mime === "application/octet-stream") mime = mimeFromPath(ref.path);
      if (/\.html?$/i.test(ref.path)) mime = "text/html";
      const blobUrl = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: mime }));
      blobAssetUrls.push(blobUrl);
      assetUrls.set(ref.path, blobUrl);
    }),
  );

  const htmlEntry =
    pkg.manifest.entry_points.find(
      (ep) => "Html" in ep && (ep.Html as { is_main?: boolean }).is_main,
    ) ?? pkg.manifest.entry_points.find((ep) => "Html" in ep);

  const kyberWasmBase64 = findPackagedWasmBase64(assetUrls, wasmBytes);

  let blobUrl: string;
  if (htmlEntry && "Html" in htmlEntry) {
    const htmlPath = (htmlEntry.Html as { path: string }).path;
    const htmlUrl = assetUrls.get(htmlPath);
    if (htmlUrl) {
      const htmlRes = await fetch(htmlUrl);
      if (!htmlRes.ok) {
        integrityErrors.push(`${htmlPath} (fetch failed)`);
        blobUrl = URL.createObjectURL(
          new Blob([errPage(pkg.manifest.name, "HTML entry fetch failed")], { type: "text/html" }),
        );
      } else {
        const htmlText = injectSdkBridgeIntoHtml(await htmlRes.text(), assetUrls, {
          appId,
          parentOrigin: options.parentOrigin,
          endpoints: options.endpoints,
          identityDid: options.identityDid,
          kyberWasmBase64,
          contentFit: options.contentFit,
        });
        blobUrl = URL.createObjectURL(new Blob([htmlText], { type: "text/html" }));
      }
    } else {
      blobUrl = URL.createObjectURL(
        new Blob([errPage(pkg.manifest.name, "HTML entry not in content")], { type: "text/html" }),
      );
    }
  } else {
    blobUrl = URL.createObjectURL(
      new Blob([errPage(pkg.manifest.name, "No HTML entry point")], { type: "text/html" }),
    );
  }

  return {
    manifest: pkg.manifest,
    appId,
    creatorDid: parseCreatorDid(pkg.creator_did),
    blobUrl,
    assetUrls,
    blobAssetUrls,
    integrityErrors,
  };
}

export function revokeLoadedWebPackage(loaded: LoadedWebPackage): void {
  URL.revokeObjectURL(loaded.blobUrl);
  for (const url of loaded.blobAssetUrls) URL.revokeObjectURL(url);
}
