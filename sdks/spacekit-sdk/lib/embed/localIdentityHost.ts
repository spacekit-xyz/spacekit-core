/**
 * Browser-localStorage identity host for embedded apps.
 *
 * Every SpaceKit host site keeps the signed-in DID in localStorage, but under
 * different keys and DID methods: spacekit.xyz writes `spacekit.messaging.myDid`
 * with `did:spacekit:user:…`, while kit.space writes `spacekit:identityDid` with
 * `did:key:…`. Both are read here so the same host works on either site, and the
 * DID method is deliberately not validated.
 */

import { createEmbeddedHttpHandler } from "./httpBridge.js";
import type {
  EmbedHostServices,
  EmbeddedHttpHandler,
  HttpBridgeHost,
} from "./types.js";

/** DID keys in priority order; the first non-empty value wins. */
export const DEFAULT_DID_KEYS = ["spacekit.messaging.myDid", "spacekit:identityDid"];
export const DEFAULT_SESSION_TOKEN_KEY = "spacekit:sessionToken";

const DEFAULT_KEYS = {
  displayName: "spacekit.messaging.displayName",
  publicOptIn: "spacekit.messaging.publicOptIn",
  didRegistered: "spacekit.messaging.didRegistered",
};

export interface LocalIdentitySnapshot {
  myDid: string;
  displayName: string;
  publicOptIn: boolean;
  didRegistered: boolean;
}

export interface LocalIdentityHostOptions {
  didKeys?: string[];
  sessionTokenKey?: string;
  displayNameKey?: string;
  publicOptInKey?: string;
  didRegisteredKey?: string;
  /**
   * Placeholder DIDs that must not be treated as a signed-in viewer. Anonymous
   * apps still load; they just see a null viewer DID.
   */
  isEphemeralDid?: (did: string) => boolean;
}

export interface LocalIdentityHost {
  loadDid(): string;
  loadSessionToken(): string;
  authHeaders(): Record<string, string>;
  readSnapshot(): LocalIdentitySnapshot;
  writeSnapshot(partial: Partial<LocalIdentitySnapshot>): LocalIdentitySnapshot;
  handleIdentity(method: string, params: Record<string, unknown>): unknown;
}

function defaultIsEphemeralDid(did: string): boolean {
  return (
    !did ||
    did === "did:spacekit:user:local-dev" ||
    did.includes(":preview:") ||
    did.includes(":local-dev")
  );
}

function readItem(key: string): string {
  try {
    return localStorage.getItem(key) || "";
  } catch {
    return "";
  }
}

function writeItem(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* private mode / quota — identity stays in-memory for this page */
  }
}

export function createLocalIdentityHost(
  options: LocalIdentityHostOptions = {},
): LocalIdentityHost {
  const didKeys = options.didKeys?.length ? options.didKeys : DEFAULT_DID_KEYS;
  const sessionTokenKey = options.sessionTokenKey ?? DEFAULT_SESSION_TOKEN_KEY;
  const displayNameKey = options.displayNameKey ?? DEFAULT_KEYS.displayName;
  const publicOptInKey = options.publicOptInKey ?? DEFAULT_KEYS.publicOptIn;
  const didRegisteredKey = options.didRegisteredKey ?? DEFAULT_KEYS.didRegistered;
  const isEphemeralDid = options.isEphemeralDid ?? defaultIsEphemeralDid;

  function loadDid(): string {
    for (const key of didKeys) {
      const value = readItem(key);
      if (value) return value;
    }
    return "";
  }

  function loadSessionToken(): string {
    return readItem(sessionTokenKey);
  }

  function authHeaders(): Record<string, string> {
    const headers: Record<string, string> = {};
    const did = loadDid();
    if (did && !isEphemeralDid(did)) headers["owner-did"] = did;
    const token = loadSessionToken();
    if (token) headers.Authorization = `Bearer ${token}`;
    return headers;
  }

  function readSnapshot(): LocalIdentitySnapshot {
    return {
      myDid: loadDid(),
      displayName: readItem(displayNameKey),
      publicOptIn: readItem(publicOptInKey) === "1",
      didRegistered: readItem(didRegisteredKey) === "1",
    };
  }

  function writeSnapshot(partial: Partial<LocalIdentitySnapshot>): LocalIdentitySnapshot {
    const next = { ...readSnapshot(), ...partial };
    // Mirror to every configured key so a DID written by the embedded app is
    // visible to the host site regardless of which key it reads.
    for (const key of didKeys) writeItem(key, next.myDid);
    writeItem(displayNameKey, next.displayName);
    writeItem(publicOptInKey, next.publicOptIn ? "1" : "0");
    writeItem(didRegisteredKey, next.didRegistered ? "1" : "0");
    return next;
  }

  function handleIdentity(method: string, params: Record<string, unknown>): unknown {
    if (method === "did") return readSnapshot().myDid;
    if (method === "getState") return readSnapshot();
    if (method === "setState") {
      const next = writeSnapshot(params as Partial<LocalIdentitySnapshot>);
      try {
        window.postMessage({ type: "spacekit-identity-changed", state: next }, "*");
      } catch {
        /* ignore */
      }
      return next;
    }
    if (method === "authHeaders") return authHeaders();
    throw new Error(`identity.${method} not supported`);
  }

  return {
    loadDid,
    loadSessionToken,
    authHeaders,
    readSnapshot,
    writeSnapshot,
    handleIdentity,
  };
}

export interface LocalEmbedHost {
  identity: LocalIdentityHost;
  services: EmbedHostServices;
  httpHandler: EmbeddedHttpHandler;
}

export interface LocalEmbedHostOptions extends LocalIdentityHostOptions {
  /** Extra host services (payments, purchase recording) merged over the defaults. */
  services?: Partial<EmbedHostServices>;
  /** Overrides for the HTTP bridge, e.g. site-specific auth header merging. */
  httpBridge?: Partial<HttpBridgeHost>;
}

/**
 * Builds the `services` + HTTP handler pair that {@link acquireAppDataSdkBridge}
 * and `SpacekitAppFrame` need, backed by localStorage identity.
 */
export function createLocalStorageEmbedHost(
  options: LocalEmbedHostOptions = {},
): LocalEmbedHost {
  const identity = createLocalIdentityHost(options);
  const isEphemeralDid = options.isEphemeralDid ?? defaultIsEphemeralDid;

  const httpBridgeHost: HttpBridgeHost = {
    mergeFetchHeaders(_url, headers) {
      return { ...identity.authHeaders(), ...headers };
    },
    getSessionToken() {
      return identity.loadSessionToken() || null;
    },
    ...options.httpBridge,
  };

  const services: EmbedHostServices = {
    getViewerDid() {
      const did = identity.loadDid();
      return did && !isEphemeralDid(did) ? did : null;
    },
    getIdentityDid() {
      return identity.loadDid();
    },
    handleIdentity: identity.handleIdentity,
    ...options.services,
  };

  return {
    identity,
    services,
    httpHandler: createEmbeddedHttpHandler(httpBridgeHost),
  };
}
