/**
 * Picks which storage origin serves a given app's manifest.
 *
 * A host site may be able to reach several storage origins (a local node, a
 * website-api proxy, production) and an app's `.spkg` only exists on some of
 * them. This probes `GET /facts/{appId}` across the candidates and caches the
 * winner per app, so only the first launch pays for probing.
 *
 * Callers own the candidate list, since deriving it from env vars and dev
 * proxies is site-specific.
 */

export const DEFAULT_ORIGIN_CACHE_PREFIX = "spacekit:app-storage:";

/**
 * `rate_limited` counts as usable: older storage-node builds cap `GET /facts`
 * per IP, and probing every candidate would burn the budget the real load needs.
 */
export type ManifestProbeResult = "ok" | "missing" | "rate_limited" | "error";

export interface StorageOriginResolverOptions {
  /** Ordered candidates; the first is the fallback when every probe fails. */
  candidates: string[] | (() => string[]);
  /** Maps a raw origin to a fetchable one (dev proxy rewrites, etc.). */
  normalize?: (origin: string) => string;
  /** Where the resolved origin is cached. Defaults to `sessionStorage`. */
  cache?: Pick<Storage, "getItem" | "setItem" | "removeItem"> | null;
  cacheKeyPrefix?: string;
  fetchImpl?: typeof fetch;
  /**
   * Whether a probe result is good enough to use that origin. Defaults to
   * accepting `ok` and `rate_limited`. Widen it to keep a preferred origin on
   * transient errors, e.g. `index === 0 ? probe !== "missing" : probe === "ok"`.
   */
  accept?: (probe: ManifestProbeResult, origin: string, index: number) => boolean;
  /** Origin used when every probe fails. Defaults to the first candidate. */
  fallbackOrigin?: string | (() => string);
}

export interface StorageOriginResolver {
  (appId: string): Promise<string>;
  remember(appId: string, origin: string): void;
  forget(appId: string): void;
  cached(appId: string): string | null;
}

function trimSlash(url: string): string {
  return url.replace(/\/$/, "");
}

function defaultCache(): StorageOriginResolverOptions["cache"] {
  try {
    return typeof sessionStorage !== "undefined" ? sessionStorage : null;
  } catch {
    return null;
  }
}

export async function probeManifest(
  storageBase: string,
  appId: string,
  fetchImpl: typeof fetch = fetch,
): Promise<ManifestProbeResult> {
  try {
    const res = await fetchImpl(`${trimSlash(storageBase)}/facts/${encodeURIComponent(appId)}`, {
      method: "GET",
      cache: "no-store",
    });
    if (res.ok) return "ok";
    if (res.status === 404) return "missing";
    if (res.status === 429) return "rate_limited";
    if (res.status >= 500) {
      const peek = await res
        .clone()
        .text()
        .catch(() => "");
      if (/exceeded/i.test(peek) || /rate.?limit/i.test(peek)) return "rate_limited";
      return "error";
    }
    return "error";
  } catch {
    return "error";
  }
}

export function createStorageOriginResolver(
  options: StorageOriginResolverOptions,
): StorageOriginResolver {
  const {
    candidates,
    normalize = (origin: string) => trimSlash(origin),
    cacheKeyPrefix = DEFAULT_ORIGIN_CACHE_PREFIX,
    fetchImpl = fetch,
    accept = (probe: ManifestProbeResult) => probe === "ok" || probe === "rate_limited",
  } = options;
  const cache = options.cache === undefined ? defaultCache() : options.cache;

  const cacheKey = (appId: string) => `${cacheKeyPrefix}${appId}`;

  function cached(appId: string): string | null {
    try {
      return cache?.getItem(cacheKey(appId)) ?? null;
    } catch {
      return null;
    }
  }

  function remember(appId: string, origin: string): void {
    try {
      cache?.setItem(cacheKey(appId), normalize(origin));
    } catch {
      /* ignore */
    }
  }

  function forget(appId: string): void {
    try {
      cache?.removeItem(cacheKey(appId));
    } catch {
      /* ignore */
    }
  }

  const resolve = async (appId: string): Promise<string> => {
    const list = (typeof candidates === "function" ? candidates() : candidates)
      .map((origin) => normalize(origin))
      .filter((origin, index, all) => origin && all.indexOf(origin) === index);
    const configuredFallback =
      typeof options.fallbackOrigin === "function"
        ? options.fallbackOrigin()
        : options.fallbackOrigin;
    const fallback = configuredFallback ? normalize(configuredFallback) : (list[0] ?? "");

    const remembered = cached(appId);
    if (remembered) {
      const normalized = normalize(remembered);
      const probe = await probeManifest(normalized, appId, fetchImpl);
      if (probe === "ok" || probe === "rate_limited") {
        if (normalized !== remembered) remember(appId, normalized);
        return normalized;
      }
      forget(appId);
    }

    for (let index = 0; index < list.length; index++) {
      const origin = list[index];
      const probe = await probeManifest(origin, appId, fetchImpl);
      if (accept(probe, origin, index)) {
        remember(appId, origin);
        return origin;
      }
    }

    return fallback;
  };

  return Object.assign(resolve, { remember, forget, cached });
}
