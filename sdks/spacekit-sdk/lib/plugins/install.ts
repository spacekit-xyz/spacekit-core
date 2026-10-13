/**
 * Installing a plugin into an app.
 *
 * The rules that keep one author's code safe inside another author's app:
 *
 *   1. **No more than the app has.** A plugin gets the intersection of what it
 *      declares and what the host app was granted. If it needs something the
 *      app lacks, installation is refused with a reason.
 *   2. **The app owner owns the data.** A plugin's documents live in the app
 *      owner's storage namespace, under the plugin's app id and a per-install
 *      scope (`app_<pluginId>_i<hostAppId>_<collection>`). The plugin author
 *      cannot read other installs, and the storage node's app tokens already
 *      confine the plugin to its own prefix.
 *   3. **No charging the app's viewers.** `payments.*` is refused inside a
 *      plugin; plugins are paid by the app owner through their pricing.
 *   4. **Pinned versions.** An install records the exact version and package
 *      hash. An update that asks for new permissions needs a new approval.
 */

import { parseManifestPermissions, type CapabilityPolicy } from "../embed/capabilities.js";
import { formatAstra } from "../astra.js";
import {
  BRAIN_FILE_ROLES,
  checkConfigValue,
  type PluginDescriptor,
} from "./descriptor.js";

/** Lower-case hex, at most 16 characters: the per-install storage scope label. */
export function installScopeLabel(hostAppId: string): string {
  const hex = hostAppId.trim().toLowerCase().replace(/^0x/, "").replace(/[^0-9a-f]/g, "").slice(0, 16);
  if (!hex) throw new Error("hostAppId must be a hex app id");
  return `i${hex}`;
}

export interface PluginInstallScope {
  /** The plugin's own app id (its package id); storage tokens are minted for it. */
  pluginAppId: string;
  /** Whose namespace holds the plugin's data: the host app's owner. */
  ownerDid: string;
  /** Collection infix that separates this install from other installs. */
  collectionScope: string;
}

export function pluginInstallScope(input: {
  pluginAppId: string;
  hostAppId: string;
  hostOwnerDid: string;
}): PluginInstallScope {
  const ownerDid = input.hostOwnerDid.trim();
  if (!ownerDid.startsWith("did:")) throw new Error("hostOwnerDid must be a DID");
  return {
    pluginAppId: input.pluginAppId.trim().toLowerCase(),
    ownerDid,
    collectionScope: installScopeLabel(input.hostAppId),
  };
}

export interface PermissionCheck {
  /** Capabilities the plugin may use. */
  allowed: string[];
  /** Capabilities the plugin declares that the host app does not have. */
  missing: string[];
  /** Network hosts the plugin declares that the host app may not reach. */
  missingHosts: string[];
}

function hostCovered(pattern: string, allowedPatterns: string[]): boolean {
  const p = pattern.toLowerCase();
  return allowedPatterns.some((a) => {
    const q = a.toLowerCase();
    if (q === "*" || q === p) return true;
    if (q.startsWith("*.")) {
      const suffix = q.slice(1);
      const host = p.replace(/^[a-z][a-z0-9+.-]*:\/\//, "").replace(/[:/].*$/, "");
      return host.endsWith(suffix) || p === q;
    }
    return false;
  });
}

/**
 * Compare a plugin's declared permissions with the host app's. Pass the host
 * app's manifest permissions (what its viewers granted).
 */
export function checkPluginPermissions(pluginPermissions: unknown[] | undefined, hostAppPermissions: unknown[]): PermissionCheck {
  const plugin = parseManifestPermissions(pluginPermissions ?? []);
  const host = parseManifestPermissions(hostAppPermissions);
  const allowed: string[] = [];
  const missing: string[] = [];
  for (const cap of plugin.capabilities) {
    (host.capabilities.has(cap) ? allowed : missing).push(cap);
  }
  const missingHosts = plugin.capabilities.has("network")
    ? plugin.networkHosts.filter((h) => !host.capabilities.has("network") || !hostCovered(h, host.networkHosts))
    : [];
  if (missingHosts.length && !missing.includes("network")) missing.push("network");
  return { allowed, missing, missingHosts };
}

/**
 * The capability policy for a plugin frame inside a host app: only what both
 * declared, network limited to declared hosts, payments and the session token
 * always refused.
 */
export function pluginCapabilityPolicy(options: {
  check: PermissionCheck;
  /** The host's own policy (trusted origins and anything it denies). */
  base?: CapabilityPolicy;
}): CapabilityPolicy {
  const deny = new Set([...(options.base?.deny ?? []), "payments", "identity:auth-headers", "identity.authheaders"]);
  for (const cap of options.check.missing) deny.add(cap);
  return {
    ...options.base,
    grant: [],
    deny: [...deny],
    network: "manifest",
    exposeAuthHeaders: false,
    allowIdentityOverride: false,
  };
}

export class PluginConfigError extends Error {
  constructor(readonly errors: string[]) {
    super(`Plugin settings are not valid:\n- ${errors.join("\n- ")}`);
    this.name = "PluginConfigError";
  }
}

/**
 * Apply defaults and check the app owner's settings against the descriptor.
 * Unknown keys are refused so typos surface.
 */
export function resolvePluginConfig(
  descriptor: Pick<PluginDescriptor, "config">,
  values: Record<string, unknown> = {},
): Record<string, unknown> {
  const fields = descriptor.config ?? {};
  const errors: string[] = [];
  const out: Record<string, unknown> = {};
  for (const key of Object.keys(values)) {
    if (!(key in fields)) errors.push(`${key}: this plugin has no such setting`);
  }
  for (const [key, field] of Object.entries(fields)) {
    const value = values[key] !== undefined ? values[key] : field.default;
    if (value === undefined || value === "") {
      if (field.required) errors.push(`${field.label}: required`);
      continue;
    }
    const problem = checkConfigValue(field, value);
    if (problem) errors.push(`${field.label}: ${problem}`);
    else out[key] = value;
  }
  if (errors.length) throw new PluginConfigError(errors);
  return out;
}

const KIND_LABELS: Record<PluginDescriptor["kind"], string> = {
  ui: "Adds a feature you can see in your app",
  service: "Runs behind the scenes for your app",
  contract: "A program on the SpaceKit chain your app can call",
  brain: "A trained AI brain. It is data only: it cannot run code, reach websites or see your users' data",
};

const BLOCK_LABELS: Record<string, string> = {
  identity: "Sign-in and profiles",
  storage: "Saving data",
  messaging: "Messages",
  compute: "On-chain programs",
  payments: "Payments in ASTRA",
  ai: "AI",
};

/** Plain-language lines for the install screen, for people who do not read code. */
export function describePlugin(descriptor: PluginDescriptor): string[] {
  const lines = [`${descriptor.name}: ${descriptor.summary}`, KIND_LABELS[descriptor.kind]];
  if (descriptor.uses?.length) {
    lines.push(`Uses: ${descriptor.uses.map((u) => BLOCK_LABELS[u] ?? u).join(", ")}`);
  }
  const declared = parseManifestPermissions(descriptor.permissions ?? []);
  for (const label of declared.labels) lines.push(`Can: ${label}`);
  const p = descriptor.pricing;
  if (!p || p.model === "free") lines.push("Price: free");
  else if (p.model === "one_time") lines.push(`Price: ${formatAstra(BigInt(p.amount_wei ?? "0"))} ASTRA, once`);
  else lines.push(`Price: ${formatAstra(BigInt(p.amount_wei ?? "0"))} ASTRA every ${p.period_days ?? 30} days`);
  if (descriptor.kind === "brain" && descriptor.brain) {
    lines.push(`Runs on the ${descriptor.brain.engine} engine`);
    const size = (descriptor.files ?? []).reduce((n, f) => n + f.size, 0);
    if (size) {
      lines.push(
        `Download size: ${size >= 1024 * 1024 ? `${(size / (1024 * 1024)).toFixed(1)} MB` : `${Math.max(1, Math.round(size / 1024))} KB`}`,
      );
    }
    if (descriptor.brain.training?.summary) lines.push(`Trained on: ${descriptor.brain.training.summary}`);
    if (descriptor.brain.training?.includes_captured_conversations) {
      lines.push("Trained partly on captured conversations");
    }
    const roles = (descriptor.files ?? []).map((f) => BRAIN_FILE_ROLES[f.role]).filter(Boolean);
    if (roles.length) lines.push(`Includes: ${roles.join(", ")}`);
  }
  if (descriptor.license) lines.push(`License: ${descriptor.license}`);
  return lines;
}
