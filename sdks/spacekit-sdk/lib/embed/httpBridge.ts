import { safeUUID } from "../crypto.js";
import type {
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

export async function handleEmbeddedHttpFetch(
  host: HttpBridgeHost,
  params: Record<string, unknown>,
): Promise<EmbeddedFetchResult> {
  const url = String(params.url ?? "");
  if (!url) throw new Error("http.fetch requires url");

  const init = (params.init ?? {}) as {
    method?: string;
    headers?: Record<string, string>;
    body?: string;
  };

  const headers = host.mergeFetchHeaders(url, { ...(init.headers ?? {}) });
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
    return fetch(url, {
      method,
      headers: requestHeaders,
      body: fetchBody,
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

  if (res.status === 401 && host.getSessionToken() && host.isSessionExpiredError) {
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
  ): Promise<unknown> | null {
    if (module !== "http") return null;

    if (method === "fetch") {
      return handleEmbeddedHttpFetch(host, params);
    }

    if (method === "sseSubscribe") {
      const url = String(params.url ?? "");
      if (!url) throw new Error("http.sseSubscribe requires url");
      const id = safeUUID();
      const es = new EventSource(url);
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
