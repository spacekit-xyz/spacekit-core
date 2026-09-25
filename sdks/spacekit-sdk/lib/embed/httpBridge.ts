import { safeUUID } from "../crypto.js";
import type {
  EmbeddedHttpContext,
  EmbeddedFetchResult,
  EmbeddedHttpHandler,
  HttpBridgeHost,
  SsePushHandler,
} from "./types.js";

export const SESSION_EXPIRED_EVENT = "spacekit:session-expired";

function isBinaryContentType(contentType: string): boolean {
  const ct = contentType.toLowerCase();
  return (
    ct.includes("application/wasm") ||
    ct.includes("application/octet-stream") ||
    ct.startsWith("image/") ||
    ct.startsWith("audio/") ||
    ct.startsWith("video/")
  );
}

function arrayBufferToBase64(buffer: ArrayBuffer): string {
  const bytes = new Uint8Array(buffer);
  const chunkSize = 0x8000;
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i += chunkSize) {
    parts.push(String.fromCharCode(...bytes.subarray(i, i + chunkSize)));
  }
  return btoa(parts.join(""));
}

function withoutAuthHeaders(headers: Record<string, string>): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(headers)) {
    const key = k.toLowerCase();
    if (key === "authorization" || key === "owner-did") continue;
    out[k] = v;
  }
  return out;
}

function resolveUrl(url: string): string {
  try {
    return new URL(url, typeof window !== "undefined" ? window.location.href : undefined).href;
  } catch {
    return url;
  }
}

/**
 * Host credentials go only to origins the host vouches for. Without an explicit
 * `isCredentialedUrl`, that is the host page's own origin.
 */
function isCredentialedUrl(host: HttpBridgeHost, url: string): boolean {
  if (host.isCredentialedUrl) return host.isCredentialedUrl(resolveUrl(url));
  if (typeof window === "undefined") return false;
  try {
    return new URL(url, window.location.href).origin === window.location.origin;
  } catch {
    return false;
  }
}

export async function handleEmbeddedHttpFetch(
  host: HttpBridgeHost,
  params: Record<string, unknown>,
  context?: EmbeddedHttpContext,
): Promise<EmbeddedFetchResult> {
  const url = String(params.url ?? "");
  if (!url) throw new Error("http.fetch requires url");
  const resolvedUrl = resolveUrl(url);

  const init = (params.init ?? {}) as {
    method?: string;
    headers?: Record<string, string>;
    body?: string;
  };

  const trusted = isCredentialedUrl(host, url);
  const appHeaders = { ...(init.headers ?? {}) };
  // On trusted origins, prefer a credential scoped to this app over the viewer's
  // own session; fall back to the session only if the host allows it.
  let headers = appHeaders;
  // `credentialed`: the viewer's own session (headers + cookies) is attached.
  let credentialed = false;
  if (trusted) {
    const scoped = context?.appCredentials ? await context.appCredentials().catch(() => null) : null;
    if (scoped?.apiAuthorization) {
      headers = withoutAuthHeaders(appHeaders);
      headers.Authorization = scoped.apiAuthorization;
    } else if (host.forwardViewerSession !== false) {
      headers = host.mergeFetchHeaders(url, appHeaders);
      credentialed = true;
    }
  }
  const bodyEncoding = headers["X-Body-Encoding"] ?? headers["x-body-encoding"];
  delete headers["X-Body-Encoding"];
  delete headers["x-body-encoding"];

  let fetchBody: BodyInit | undefined;
  if (init.body != null && init.body !== "") {
    if (bodyEncoding === "base64") {
      const raw = atob(init.body);
      const arr = new Uint8Array(raw.length);
      for (let i = 0; i < raw.length; i++) arr[i] = raw.charCodeAt(i);
      fetchBody = arr;
    } else {
      fetchBody = init.body;
    }
  }

  const method = init.method ?? "GET";

  async function doFetch(requestHeaders: Record<string, string>): Promise<Response> {
    return fetch(resolvedUrl, {
      method,
      headers: requestHeaders,
      body: fetchBody,
      credentials: credentialed ? "same-origin" : "omit",
    });
  }

  let res: Response;
  try {
    res = await doFetch(headers);
  } catch (err) {
    const fallback = "Could not reach the server.";
    throw new Error(host.formatFetchError?.(err, fallback) ?? fallback);
  }

  if (
    credentialed &&
    res.status === 401 &&
    host.shouldRetryUnauthorized?.(url, headers) &&
    host.refreshFetchHeaders
  ) {
    const retryHeaders = host.refreshFetchHeaders(url, { ...(init.headers ?? {}) });
    if (retryHeaders.Authorization || retryHeaders.authorization) {
      try {
        res = await doFetch(retryHeaders);
      } catch {
        /* keep first 401 */
      }
    }
  }

  if (credentialed && res.status === 401 && host.getSessionToken() && host.isSessionExpiredError) {
    const errBody = await res.clone().json().catch(() => null);
    if (host.isSessionExpiredError(errBody)) {
      host.onSessionExpired?.("Your session expired. Sign in again to continue.");
    }
  }

  const respHeaders: Record<string, string> = {};
  res.headers.forEach((value, key) => {
    respHeaders[key] = value;
  });

  const contentType = res.headers.get("content-type") ?? "";
  const binary = isBinaryContentType(contentType) || url.includes(".wasm");

  if (binary) {
    const buffer = await res.arrayBuffer();
    return {
      ok: res.ok,
      status: res.status,
      statusText: res.statusText,
      headers: respHeaders,
      body: arrayBufferToBase64(buffer),
      binary: true,
    };
  }

  const body = await res.text();
  return {
    ok: res.ok,
    status: res.status,
    statusText: res.statusText,
    headers: respHeaders,
    body,
  };
}

export function createEmbeddedHttpHandler(host: HttpBridgeHost): EmbeddedHttpHandler {
  const sseStreams = new Map<string, EventSource>();

  return function handleEmbeddedHttp(
    module: string,
    method: string,
    params: Record<string, unknown>,
    push: SsePushHandler,
    context?: EmbeddedHttpContext,
  ): Promise<unknown> | null {
    if (module !== "http") return null;

    if (method === "fetch") {
      return handleEmbeddedHttpFetch(host, params, context);
    }

    if (method === "sseSubscribe") {
      const url = String(params.url ?? "");
      if (!url) throw new Error("http.sseSubscribe requires url");
      const id = safeUUID();
      const es = new EventSource(resolveUrl(url), {
        withCredentials: false,
      });
      sseStreams.set(id, es);
      es.onmessage = (event) => {
        push(`__sse:${id}`, { type: "message", data: event.data });
      };
      es.onerror = () => {
        push(`__sse:${id}`, { type: "error" });
      };
      return Promise.resolve(id);
    }

    if (method === "sseClose") {
      const id = String(params.id ?? "");
      sseStreams.get(id)?.close();
      sseStreams.delete(id);
      return Promise.resolve(true);
    }

    throw new Error(`http.${method} not implemented`);
  };
}
