/**
 * Calls between an app and the plugins installed in it.
 *
 * An app asks a `service` plugin to do something with
 * `spacekit.plugins.call(pluginId, method, params)`. The call goes to the
 * host, never straight to the plugin's frame. The host checks the method and
 * its parameters against the plugin's descriptor (`service.methods`), sends
 * the request to the plugin frame as an event, and waits for the plugin to
 * answer with `plugins.respond`. Plugins register handlers with
 * `spacekit.plugins.serve(method, handler)`.
 *
 * Every plugin frame (ui or service) can also read its own settings with
 * `spacekit.plugins.config()`. Plugins cannot call other plugins in v1.
 *
 * Wire (over the existing embed protocol, nothing new on the port):
 *   app    → host   call  plugins.call     { plugin, method, params }
 *   host   → plugin event "__plugins:request" { requestId, method, params }
 *   plugin → host   call  plugins.respond  { requestId, result } | { requestId, error }
 */

import { checkConfigValue, type ConfigField, type PluginDescriptor } from "./descriptor.js";

export const PLUGIN_REQUEST_TOPIC = "__plugins:request";
const DEFAULT_TIMEOUT_MS = 30_000;
const MAX_PENDING_PER_PLUGIN = 64;

export interface InstalledPlugin {
  /** The plugin's package / listing app id. */
  pluginAppId: string;
  descriptor: PluginDescriptor;
  /** Resolved settings (see `resolvePluginConfig`). */
  config: Record<string, unknown>;
}

export class PluginCallError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PluginCallError";
  }
}

interface Pending {
  pluginAppId: string;
  resolve(value: unknown): void;
  reject(err: Error): void;
  timer: ReturnType<typeof setTimeout>;
}

/** Check call parameters against a method's declared params. Returns the params to forward. */
export function checkServiceParams(
  descriptor: PluginDescriptor,
  method: string,
  params: Record<string, unknown>,
): Record<string, unknown> {
  const def = descriptor.service?.methods?.[method];
  if (descriptor.kind !== "service" || !def) {
    throw new PluginCallError(`${descriptor.name} has no method "${method}"`);
  }
  const fields: Record<string, ConfigField> = def.params ?? {};
  const problems: string[] = [];
  for (const key of Object.keys(params)) {
    if (!(key in fields)) problems.push(`${key}: not a parameter of ${method}`);
  }
  const out: Record<string, unknown> = {};
  for (const [key, field] of Object.entries(fields)) {
    const value = params[key] !== undefined ? params[key] : field.default;
    if (value === undefined || value === "") {
      if (field.required) problems.push(`${field.label}: required`);
      continue;
    }
    const problem = checkConfigValue(field, value);
    if (problem) problems.push(`${field.label}: ${problem}`);
    else out[key] = value;
  }
  if (problems.length) throw new PluginCallError(`${descriptor.name}.${method}: ${problems.join("; ")}`);
  return out;
}

export class PluginRouter {
  private readonly installed = new Map<string, InstalledPlugin>();
  private readonly frames = new Map<string, (topic: string, msg: unknown) => void>();
  private readonly pending = new Map<string, Pending>();
  private readonly timeoutMs: number;
  private seq = 0;

  constructor(options: { timeoutMs?: number } = {}) {
    this.timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  }

  /** Record a plugin installed in this app. Call before mounting its frame. */
  install(plugin: InstalledPlugin): void {
    this.installed.set(plugin.pluginAppId.toLowerCase(), plugin);
  }

  /** Forget a plugin and fail its outstanding calls. */
  uninstall(pluginAppId: string): void {
    const id = pluginAppId.toLowerCase();
    this.installed.delete(id);
    this.detachFrame(id);
  }

  /** Installed plugins, as the app sees them. */
  list(): Array<{ pluginAppId: string; id: string; name: string; kind: string; version: string; methods: string[] }> {
    return [...this.installed.values()].map((p) => ({
      pluginAppId: p.pluginAppId,
      id: p.descriptor.id,
      name: p.descriptor.name,
      kind: p.descriptor.kind,
      version: p.descriptor.version,
      methods: Object.keys(p.descriptor.service?.methods ?? {}),
    }));
  }

  /** Connect a running plugin frame; `push` delivers events to it. */
  attachFrame(pluginAppId: string, push: (topic: string, msg: unknown) => void): void {
    this.frames.set(pluginAppId.toLowerCase(), push);
  }

  /** Disconnect a plugin frame (unmounted or reloaded) and fail its pending calls. */
  detachFrame(pluginAppId: string): void {
    const id = pluginAppId.toLowerCase();
    this.frames.delete(id);
    for (const [requestId, p] of this.pending) {
      if (p.pluginAppId === id) {
        clearTimeout(p.timer);
        this.pending.delete(requestId);
        p.reject(new PluginCallError("The plugin stopped before it answered"));
      }
    }
  }

  private find(ref: string): InstalledPlugin | undefined {
    const key = ref.trim().toLowerCase();
    return this.installed.get(key) ?? [...this.installed.values()].find((p) => p.descriptor.id === key);
  }

  /** `plugins.*` calls from the host app's frame. */
  async handleAppCall(method: string, params: Record<string, unknown>): Promise<unknown> {
    if (method === "list") return this.list();
    if (method !== "call") throw new PluginCallError(`plugins.${method} is not available to apps`);
    const plugin = this.find(String(params.plugin ?? ""));
    if (!plugin) throw new PluginCallError(`Plugin ${String(params.plugin)} is not installed in this app`);
    const name = String(params.method ?? "");
    const raw = params.params && typeof params.params === "object" ? (params.params as Record<string, unknown>) : {};
    const checked = checkServiceParams(plugin.descriptor, name, raw);
    const id = plugin.pluginAppId.toLowerCase();
    const push = this.frames.get(id);
    if (!push) throw new PluginCallError(`${plugin.descriptor.name} is not running`);
    const outstanding = [...this.pending.values()].filter((p) => p.pluginAppId === id).length;
    if (outstanding >= MAX_PENDING_PER_PLUGIN) throw new PluginCallError(`${plugin.descriptor.name} is busy`);
    const requestId = `r${++this.seq}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(requestId);
        reject(new PluginCallError(`${plugin.descriptor.name}.${name} did not answer in time`));
      }, this.timeoutMs);
      this.pending.set(requestId, { pluginAppId: id, resolve, reject, timer });
      push(PLUGIN_REQUEST_TOPIC, { requestId, method: name, params: checked });
    });
  }

  /** `plugins.*` calls from a plugin's own frame. */
  handlePluginCall(pluginAppId: string, method: string, params: Record<string, unknown>): unknown {
    const id = pluginAppId.toLowerCase();
    const plugin = this.installed.get(id);
    if (!plugin) throw new PluginCallError("This plugin is not installed here");
    if (method === "config") return { ...plugin.config };
    if (method === "respond") {
      const requestId = String(params.requestId ?? "");
      const p = this.pending.get(requestId);
      // Only the plugin a request went to may answer it.
      if (!p || p.pluginAppId !== id) return false;
      clearTimeout(p.timer);
      this.pending.delete(requestId);
      if (typeof params.error === "string" && params.error) p.reject(new PluginCallError(params.error));
      else p.resolve(params.result ?? null);
      return true;
    }
    throw new PluginCallError(`plugins.${method} is not available to plugins`);
  }
}
