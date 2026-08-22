/**
 * WebPackageLoader — fetch an AppPackage manifest from the storage node,
 * resolve ContentRef entries, download files, verify hashes, and produce
 * a blob URL bundle suitable for rendering in a sandboxed iframe.
 *
 * A "WebPackage" is an AppPackage whose primary entry point is Html or Component.
 */

export interface ContentRef {
  path: string;
  content_type: ContentTypeTag;
  size: number;
  /** Hex-encoded SHA-256 */
  hash: string;
  compression: string;
  encrypted: boolean;
  /** Hex-encoded FactID referencing the stored blob */
  fact_id: string;
}

export type ContentTypeTag =
  | "Wasm"
  | "Html"
  | "Css"
  | "JavaScript"
  | "TypeScript"
  | "React"
  | "Json"
  | "Markdown"
  | { Image: { format: string } }
  | { Font: { format: string } }
  | { Audio: { format: string } }
  | { Video: { format: string } }
  | { Binary: { mime_type: string } }
  | { Other: { mime_type: string } };

export interface EntryPointHtml {
  Html: { path: string; is_main: boolean };
}

export interface EntryPointWasm {
  Wasm: { path: string; exports: string[]; memory_pages?: number };
}

export interface EntryPointComponent {
  Component: { path: string; component_name: string; props_schema?: string };
}

export type EntryPoint = EntryPointHtml | EntryPointWasm | EntryPointComponent | Record<string, unknown>;

export interface AppManifest {
  name: string;
  description: string;
  tagline?: string;
  entry_points: EntryPoint[];
  permissions: unknown[];
  content_types: ContentTypeTag[];
  total_size: number;
  checksum: string;
  icon?: string;
  screenshots: string[];
  keywords: string[];
  min_runtime_version?: string;
  platforms: string[];
}

export interface AppPackageJSON {
  app_id: string;
  version: { major: number; minor: number; patch: number };
  created_at: string;
  creator_did: string;
  manifest: AppManifest;
  content_refs: ContentRef[];
  license_type: string;
  access_policy: unknown;
  category: string;
  pricing: unknown;
}

export interface LoadedWebPackage {
  manifest: AppManifest;
  appId: string;
  creatorDid: string;
  /** blob URL for the main HTML entry point with all assets inlined */
  blobUrl: string;
  /** Map of path → blob URL for all loaded assets */
  assetUrls: Map<string, string>;
  /** Paths that failed integrity verification */
  integrityErrors: string[];
}

function mimeForContentType(ct: ContentTypeTag): string {
  if (typeof ct === "string") {
    switch (ct) {
      case "Wasm": return "application/wasm";
      case "Html": return "text/html";
      case "Css": return "text/css";
      case "JavaScript": return "application/javascript";
      case "TypeScript": return "application/typescript";
      case "React": return "text/jsx";
      case "Json": return "application/json";
      case "Markdown": return "text/markdown";
      default: return "application/octet-stream";
    }
  }
  if ("Image" in ct) return `image/${ct.Image.format}`;
  if ("Font" in ct) return `font/${ct.Font.format}`;
  if ("Audio" in ct) return `audio/${ct.Audio.format}`;
  if ("Video" in ct) return `video/${ct.Video.format}`;
  if ("Binary" in ct) return ct.Binary.mime_type;
  if ("Other" in ct) return ct.Other.mime_type;
  return "application/octet-stream";
}

async function sha256Hex(data: ArrayBuffer): Promise<string> {
  const hash = await crypto.subtle.digest("SHA-256", data);
  return Array.from(new Uint8Array(hash))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

export class WebPackageLoader {
  private storageBaseUrl: string;

  constructor(storageBaseUrl: string) {
    this.storageBaseUrl = storageBaseUrl.replace(/\/+$/, "");
  }

  async fetchManifest(appId: string): Promise<AppPackageJSON> {
    const res = await fetch(`${this.storageBaseUrl}/api/apps/${encodeURIComponent(appId)}`);
    if (!res.ok) throw new Error(`Failed to fetch app manifest: ${res.status}`);
    const data = await res.json();
    return data.app || data;
  }

  async fetchContent(factId: string): Promise<ArrayBuffer> {
    const res = await fetch(`${this.storageBaseUrl}/api/facts/${encodeURIComponent(factId)}/content`);
    if (!res.ok) throw new Error(`Failed to fetch content ${factId}: ${res.status}`);
    return res.arrayBuffer();
  }

  async load(appId: string): Promise<LoadedWebPackage> {
    const pkg = await this.fetchManifest(appId);
    const { manifest, content_refs } = pkg;

    const assetUrls = new Map<string, string>();
    const integrityErrors: string[] = [];

    const downloads = await Promise.allSettled(
      content_refs.map(async (ref) => {
        const data = await this.fetchContent(ref.fact_id);
        const hash = await sha256Hex(data);
        if (hash !== ref.hash) {
          integrityErrors.push(ref.path);
        }
        const mime = mimeForContentType(ref.content_type);
        const blob = new Blob([data], { type: mime });
        const url = URL.createObjectURL(blob);
        assetUrls.set(ref.path, url);
        return { path: ref.path, url, data, mime };
      }),
    );

    const htmlEntry = manifest.entry_points.find(
      (ep): ep is EntryPointHtml => "Html" in ep && (ep as EntryPointHtml).Html.is_main,
    ) ?? manifest.entry_points.find(
      (ep): ep is EntryPointHtml => "Html" in ep,
    );

    let blobUrl: string;
    if (htmlEntry) {
      const htmlAssetUrl = assetUrls.get(htmlEntry.Html.path);
      if (htmlAssetUrl) {
        const htmlRes = await fetch(htmlAssetUrl);
        let htmlText = rewriteAssetRefs(await htmlRes.text(), assetUrls);
        htmlText = injectSdkBridge(htmlText, appId);
        const finalBlob = new Blob([htmlText], { type: "text/html" });
        blobUrl = URL.createObjectURL(finalBlob);
      } else {
        blobUrl = URL.createObjectURL(
          new Blob([fallbackHtml(manifest.name, "HTML entry point not found in content")], { type: "text/html" }),
        );
      }
    } else {
      blobUrl = URL.createObjectURL(
        new Blob([fallbackHtml(manifest.name, "No HTML entry point in manifest")], { type: "text/html" }),
      );
    }

    return {
      manifest,
      appId: pkg.app_id,
      creatorDid: pkg.creator_did,
      blobUrl,
      assetUrls,
      integrityErrors,
    };
  }

  static revokeAll(loaded: LoadedWebPackage): void {
    URL.revokeObjectURL(loaded.blobUrl);
    for (const url of loaded.assetUrls.values()) {
      URL.revokeObjectURL(url);
    }
  }
}

function fallbackHtml(name: string, error: string): string {
  return `<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>${name}</title></head>
<body style="background:#0c0f18;color:#e5e7eb;font-family:'DM Sans',sans-serif;display:flex;align-items:center;justify-content:center;min-height:100vh;margin:0">
<div style="text-align:center"><h2>${name}</h2><p style="color:#ef4444">${error}</p></div>
</body></html>`;
}

function normalizeAssetPath(ref: string): string {
  return ref.replace(/^\.\//, "").replace(/^\//, "");
}

function resolveAssetUrl(ref: string, assetUrls: Map<string, string>): string | undefined {
  const norm = normalizeAssetPath(ref);
  if (assetUrls.has(norm)) return assetUrls.get(norm);
  for (const [path, url] of assetUrls) {
    if (path === norm || path.endsWith(`/${norm}`)) return url;
  }
  return undefined;
}

function rewriteAssetRefs(html: string, assetUrls: Map<string, string>): string {
  return html.replace(
    /\b(src|href)\s*=\s*("([^"]+)"|'([^']+)')/gi,
    (match, attr: string, _q: string, dquote?: string, squote?: string) => {
      const ref = dquote ?? squote ?? "";
      if (!ref || /^(https?:|blob:|data:|mailto:|#)/i.test(ref)) return match;
      const blobUrl = resolveAssetUrl(ref, assetUrls);
      return blobUrl ? `${attr}="${blobUrl}"` : match;
    },
  );
}

/**
 * Injects a small SDK bridge script into the HTML that exposes
 * window.spacekit as a postMessage-based API for the host frame.
 */
function injectSdkBridge(html: string, appId: string): string {
  const bridgeScript = `<script>
(function(){
  var pending = {};
  var nextId = 1;
  window.spacekit = {
    appId: ${JSON.stringify(appId)},
    call: function(module, method, params) {
      return new Promise(function(resolve, reject) {
        var id = nextId++;
        pending[id] = { resolve: resolve, reject: reject };
        parent.postMessage({ type: "spacekit-sdk-call", id: id, module: module, method: method, params: params }, "*");
      });
    },
    storage: {
      get: function(key) { return window.spacekit.call("storage", "get", { key: key }); },
      set: function(key, value) { return window.spacekit.call("storage", "set", { key: key, value: value }); },
    },
    messaging: {
      send: function(to, content) { return window.spacekit.call("messaging", "send", { to: to, content: content }); },
      list: function() { return window.spacekit.call("messaging", "list", {}); },
    },
    payments: {
      charge: function(amount, token) { return window.spacekit.call("payments", "charge", { amount: amount, token: token }); },
    },
    identity: {
      did: function() { return window.spacekit.call("identity", "did", {}); },
    },
  };
  window.addEventListener("message", function(e) {
    if (e.data && e.data.type === "spacekit-sdk-response" && pending[e.data.id]) {
      var p = pending[e.data.id];
      delete pending[e.data.id];
      if (e.data.error) p.reject(new Error(e.data.error));
      else p.resolve(e.data.result);
    }
  });
})();
</script>`;

  const headClose = html.indexOf("</head>");
  if (headClose !== -1) {
    return html.slice(0, headClose) + bridgeScript + html.slice(headClose);
  }
  return bridgeScript + html;
}
