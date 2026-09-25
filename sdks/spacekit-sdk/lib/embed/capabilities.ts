/**
 * Capability enforcement for embedded apps.
 *
 * Every call an app makes over the bridge passes through a guard built from the
 * app's manifest `permissions` and the host's {@link CapabilityPolicy}. The
 * guard runs in the host, in front of whatever bridge the host supplies, so a
 * custom bridge gets the same protection as the default one.
 *
 * Baseline calls (app metadata, app-scoped storage and documents, reading the
 * viewer DID, user-confirmed payments, local messaging) need no permission.
 * Everything that reaches beyond the app's own sandbox has to be declared in
 * the manifest and granted by the viewer, or granted by the host policy.
 */

export type NetworkPolicy = "open" | "manifest" | "none";

export interface CapabilityPolicy {
  /** Capabilities granted to every app on this host, declared or not. */
  grant?: string[];
  /**
   * Refused even when declared. Entries are capability names (`"network"`),
   * whole modules (`"payments"`), or single calls (`"messaging.send"`).
   */
  deny?: string[];
  /**
   * Which origins `http.fetch` / `http.sseSubscribe` may reach besides the
   * trusted ones:
   *   - `"open"` (default): any http(s) origin, without host credentials.
   *   - `"manifest"`: only hosts the manifest declares with a `network` permission.
   *   - `"none"`: trusted origins only.
   */
  network?: NetworkPolicy;
  /**
   * Origins the host attaches its credentials to (its own API). Always
   * reachable. Defaults to the host page's origin.
   */
  trustedOrigins?: string[];
  /**
   * Legacy escape hatch: let apps call `identity.authHeaders` and receive the
   * host's session token. Off by default; never enable it for third-party apps.
   */
  exposeAuthHeaders?: boolean;
  /** Let apps replace the signed-in DID through `identity.setState`. Off by default. */
  allowIdentityOverride?: boolean;
}

export interface DeclaredPermissions {
  /** Normalized capability names, e.g. `network`, `identity:write`, `messaging`. */
  capabilities: Set<string>;
  /** Hosts from `network` permissions (`api.example.com`, `*.example.com`, origins, or `*`). */
  networkHosts: string[];
  /** Human-readable lines for the consent prompt. */
  labels: string[];
}

const CAPABILITY_ALIASES: Record<string, string> = {
  internet: "network",
  net: "network",
  http: "network",
  fetch: "network",
  "identity-write": "identity:write",
  identity_write: "identity:write",
  identitywrite: "identity:write",
  "profile:write": "identity:write",
  messages: "messaging",
};

const CAPABILITY_LABELS: Record<string, string> = {
  "identity:read": "Know which account you are signed in with",
  camera: "Use your camera",
  microphone: "Use your microphone",
  geolocation: "Know your location",
  "clipboard:write": "Write to your clipboard",
  notifications: "Show notifications",
  payments: "Ask you to approve payments",
  storage: "Store data on this device",
  network: "Connect to other websites",
  "identity:write": "Change your profile details on this site",
  messaging: "Send and read messages as you",
  "identity:auth-headers": "Use your session token",
  contracts: "Call smart contracts",
  crypto: "Encrypt and decrypt files",
};

function normalizeCapability(raw: string): string {
  const key = raw.trim().toLowerCase();
  return CAPABILITY_ALIASES[key] ?? key;
}

function asStringList(value: unknown): string[] {
  if (typeof value === "string") return value.trim() ? [value.trim()] : [];
  if (Array.isArray(value)) {
    return value.filter((v): v is string => typeof v === "string" && v.trim() !== "").map((v) => v.trim());
  }
  return [];
}

function hostsFrom(value: unknown): string[] {
  if (typeof value === "string" || Array.isArray(value)) return asStringList(value);
  if (value && typeof value === "object") {
    const o = value as Record<string, unknown>;
    return [
      ...asStringList(o.hosts),
      ...asStringList(o.host),
      ...asStringList(o.domains),
      ...asStringList(o.domain),
      ...asStringList(o.origins),
      ...asStringList(o.urls),
      ...asStringList(o.allowed_hosts),
    ];
  }
  return [];
}

/**
 * Parse manifest permissions. Accepts the shapes publishers produce today:
 * `"network:api.example.com"`, `"network"`, `{ type: "network", hosts: [...] }`,
 * and serde-style enums such as `{ "Network": { "hosts": [...] } }`.
 */
export function parseManifestPermissions(permissions: unknown): DeclaredPermissions {
  const capabilities = new Set<string>();
  const networkHosts: string[] = [];
  const labels: string[] = [];

  const add = (capRaw: string, detail: unknown, original: unknown) => {
    let cap = normalizeCapability(capRaw);
    if (!cap) return;
    // Shapes of the Rust `Permission` enum (serde externally tagged).
    const d = detail && typeof detail === "object" ? (detail as Record<string, unknown>) : null;
    if (cap === "identity") cap = d && d.read_only === false ? "identity:write" : "identity:read";
    if (cap === "clipboard") cap = d && d.write ? "clipboard:write" : "clipboard:read";
    if (cap === "wallet") cap = "payments";
    if (cap === "custom" && d && typeof d.name === "string") {
      capabilities.add(`custom:${d.name.toLowerCase()}`);
      labels.push(typeof d.description === "string" && d.description ? d.description : d.name);
      return;
    }
    capabilities.add(cap);
    if (cap === "network") {
      const hosts = hostsFrom(detail);
      networkHosts.push(...(hosts.length ? hosts : ["*"]));
      labels.push(hosts.length ? `Connect to ${hosts.join(", ")}` : CAPABILITY_LABELS.network);
      return;
    }
    labels.push(
      CAPABILITY_LABELS[cap] ?? (typeof original === "string" ? original : JSON.stringify(original)),
    );
  };

  for (const entry of Array.isArray(permissions) ? permissions : []) {
    if (typeof entry === "string") {
      const idx = entry.indexOf(":");
      const head = idx === -1 ? entry : entry.slice(0, idx);
      if (normalizeCapability(head) === "network" && idx !== -1) {
        add("network", entry.slice(idx + 1), entry);
      } else {
        add(entry, null, entry);
      }
      continue;
    }
    if (!entry || typeof entry !== "object") continue;
    const o = entry as Record<string, unknown>;
    const tag = o.type ?? o.name ?? o.permission ?? o.kind;
    if (typeof tag === "string") {
      add(tag, o, entry);
      continue;
    }
    const keys = Object.keys(o);
    if (keys.length === 1) add(keys[0], o[keys[0]], entry);
  }

  return { capabilities, networkHosts, labels };
}

/**
 * Permissions-Policy features to enable on the app frame for declared
 * device permissions (camera, microphone, geolocation, clipboard).
 */
export function permissionPolicyFeatures(declared: DeclaredPermissions): string[] {
  const out: string[] = [];
  if (declared.capabilities.has("camera")) out.push("camera");
  if (declared.capabilities.has("microphone")) out.push("microphone");
  if (declared.capabilities.has("geolocation")) out.push("geolocation");
  if (declared.capabilities.has("clipboard:write")) out.push("clipboard-write");
  return out;
}

/** The capability a call needs, or null for baseline calls. */
export function requiredCapability(module: string, method: string): string | null {
  if (module === "identity" && method === "authHeaders") return "identity:auth-headers";
  if (module === "identity" && method === "setState") return "identity:write";
  if (module === "messaging" && method !== "publish") return "messaging";
  if (module === "contracts") return "contracts";
  if (module === "crypto") return "crypto";
  return null;
}

function hostMatches(pattern: string, url: URL): boolean {
  const p = pattern.trim().toLowerCase();
  if (!p) return false;
  if (p === "*") return true;
  if (/^[a-z][a-z0-9+.-]*:\/\//.test(p)) {
    try {
      return new URL(p).origin === url.origin;
    } catch {
      return false;
    }
  }
  const host = url.hostname.toLowerCase();
  if (p.startsWith("*.")) return host.endsWith(p.slice(1));
  return host === p;
}

export interface CapabilityGuardOptions {
  manifestPermissions: unknown;
  policy?: CapabilityPolicy;
  /** Base for resolving relative app URLs. Defaults to the host page URL. */
  baseUrl?: string;
}

export type CapabilityGuard = (
  module: string,
  method: string,
  params: Record<string, unknown>,
) => Record<string, unknown>;

export class CapabilityError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "CapabilityError";
  }
}

function hostOrigin(): string {
  return typeof window !== "undefined" ? window.location.origin : "";
}

/** Origins a host trusts by default: its own page origin. */
export function defaultTrustedOrigins(extra: Array<string | undefined | null> = []): string[] {
  const out = new Set<string>();
  const own = hostOrigin();
  if (own && own !== "null") out.add(own);
  for (const value of extra) {
    if (!value) continue;
    try {
      out.add(new URL(value, own || undefined).origin);
    } catch {
      /* ignore malformed endpoint */
    }
  }
  return [...out];
}

/**
 * Build the guard for one mounted app. The returned function either returns
 * the (possibly sanitized) params to forward to the bridge, or throws.
 */
export function createCapabilityGuard(options: CapabilityGuardOptions): CapabilityGuard {
  const policy = options.policy ?? {};
  const declared = parseManifestPermissions(options.manifestPermissions);
  const granted = new Set<string>([
    ...declared.capabilities,
    ...(policy.grant ?? []).map(normalizeCapability),
  ]);
  if (policy.exposeAuthHeaders) granted.add("identity:auth-headers");
  const deny = new Set((policy.deny ?? []).map((d) => d.trim().toLowerCase()));
  const networkPolicy: NetworkPolicy = policy.network ?? "open";
  const trusted = new Set(policy.trustedOrigins ?? defaultTrustedOrigins());
  const networkHosts = [
    ...declared.networkHosts,
    ...(granted.has("network") && !declared.capabilities.has("network") ? ["*"] : []),
  ];
  const base = options.baseUrl ?? (typeof window !== "undefined" ? window.location.href : undefined);

  const checkUrl = (raw: unknown, call: string): void => {
    let url: URL;
    try {
      url = new URL(String(raw ?? ""), base);
    } catch {
      throw new CapabilityError(`${call}: invalid URL`);
    }
    if (url.protocol !== "https:" && url.protocol !== "http:") {
      throw new CapabilityError(`${call}: only http(s) URLs are allowed`);
    }
    if (trusted.has(url.origin)) return;
    if (networkPolicy === "open") return;
    if (networkPolicy === "manifest" && networkHosts.some((p) => hostMatches(p, url))) return;
    throw new CapabilityError(
      `${call}: ${url.origin} is not allowed. Declare it with a "network" permission in the app manifest.`,
    );
  };

  return (module, method, params) => {
    const m = module.toLowerCase();
    const full = `${m}.${method.toLowerCase()}`;
    const cap = requiredCapability(module, method);
    if (deny.has(m) || deny.has(full) || (cap && deny.has(cap))) {
      throw new CapabilityError(`${module}.${method} is disabled on this host`);
    }
    if (cap && !granted.has(cap)) {
      throw new CapabilityError(
        `${module}.${method} requires the "${cap}" permission, which this app did not declare or was not granted`,
      );
    }

    if (m === "http" && (method === "fetch" || method === "sseSubscribe")) {
      if (deny.has("network")) throw new CapabilityError(`${module}.${method} is disabled on this host`);
      checkUrl(params.url, `${module}.${method}`);
    }

    if (m === "identity" && method === "setState" && !policy.allowIdentityOverride) {
      const { myDid: _ignored, ...rest } = params ?? {};
      return rest;
    }
    return params ?? {};
  };
}

export interface AllowPublishersOptions {
  /**
   * Require a valid package signature by one of the listed DIDs (default true).
   * With false, a matching `creator_did` claim is enough, which anyone who can
   * upload a package can write, so use it only for trusted storage nodes.
   */
  requireSignature?: boolean;
}

/**
 * Trust-policy helper: only run apps published by `dids`. By default the
 * package must be signed (`signatures/publisher.json`) by one of them.
 */
export function allowPublishers(dids: string[], options: AllowPublishersOptions = {}) {
  const allowed = new Set(dids.map((d) => d.trim().toLowerCase()));
  const requireSignature = options.requireSignature ?? true;
  return (info: { creatorDid: string; signedBy?: string[] }): boolean | string => {
    const signer = (info.signedBy ?? []).find((d) => allowed.has(d.trim().toLowerCase()));
    if (signer) return true;
    if (!requireSignature && allowed.has(info.creatorDid.trim().toLowerCase())) return true;
    if (requireSignature && allowed.has(info.creatorDid.trim().toLowerCase())) {
      return `This package claims publisher ${info.creatorDid} but is not signed by an allowed key`;
    }
    return `Publisher ${info.creatorDid || "(unknown)"} is not on this host's allowlist`;
  };
}

/** Trust-policy helper: require any valid publisher signature. */
export function requireSignedPackages() {
  return (info: { signedBy?: string[] }): boolean | string =>
    (info.signedBy?.length ?? 0) > 0 || "This host only runs signed packages";
}
