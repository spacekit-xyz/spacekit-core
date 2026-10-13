/**
 * Install records and license checks, backed by the website API
 * (`/api/apps/:hostAppId/plugins/:pluginAppId`).
 *
 * The API records each install (pinned version and descriptor hash, settings)
 * and, for priced plugins, checks that the app owner holds an active purchase.
 * Hosts call {@link requirePluginLicense} in `trustPolicy` before mounting a
 * plugin, so an unpaid, uninstalled or changed plugin never runs.
 */

export interface PluginInstallRecord {
  hostAppId: string;
  pluginAppId: string;
  pluginId: string;
  version: string;
  descriptorSha256: string;
  kind: string;
  config: Record<string, unknown>;
  ownerDid: string;
  installedAt: string;
  updatedAt: string;
  license: { required: boolean; purchaseId?: string; expiresAt?: string };
  /** Recomputed by the API on every read. */
  licensed: boolean;
  currentVersion?: string;
  currentDescriptorSha256?: string;
  updateAvailable?: boolean;
}

export interface PluginApiOptions {
  apiBase: string;
  fetchImpl?: typeof fetch;
  /** Authorization header value for owner-only calls (install, list, remove). */
  authorization?: () => Promise<string | null> | string | null;
}

function base(apiBase: string): string {
  return apiBase.replace(/\/$/, "");
}

function pluginPath(hostAppId: string, pluginAppId?: string): string {
  const host = encodeURIComponent(hostAppId.trim().toLowerCase());
  return pluginAppId === undefined
    ? `/api/apps/${host}/plugins`
    : `/api/apps/${host}/plugins/${encodeURIComponent(pluginAppId.trim().toLowerCase())}`;
}

async function readError(res: Response): Promise<string> {
  const body = (await res.json().catch(() => ({}))) as { error?: unknown };
  return typeof body.error === "string" ? body.error : `request failed (${res.status})`;
}

/** Error from an install call, with the API's status (402 = payment needed, 409 = version changed). */
export class PluginInstallError extends Error {
  constructor(
    message: string,
    readonly status: number,
    readonly details: Record<string, unknown> = {},
  ) {
    super(message);
    this.name = "PluginInstallError";
  }
}

/** A small client for the website API's plugin install routes. */
export function createPluginInstallClient(options: PluginApiOptions) {
  const doFetch = options.fetchImpl ?? ((input: RequestInfo | URL, init?: RequestInit) => fetch(input, init));
  const auth = async (): Promise<Record<string, string>> => {
    const value = await options.authorization?.();
    return value ? { Authorization: value } : {};
  };
  return {
    /** Public: the install record with live `licensed`, or null when not installed. */
    async get(hostAppId: string, pluginAppId: string): Promise<PluginInstallRecord | null> {
      const res = await doFetch(`${base(options.apiBase)}${pluginPath(hostAppId, pluginAppId)}`);
      if (res.status === 404) return null;
      if (!res.ok) throw new PluginInstallError(await readError(res), res.status);
      return (await res.json()) as PluginInstallRecord;
    },
    /** Owner only: plugins installed in an app. */
    async list(hostAppId: string): Promise<PluginInstallRecord[]> {
      const res = await doFetch(`${base(options.apiBase)}${pluginPath(hostAppId)}`, { headers: await auth() });
      if (!res.ok) throw new PluginInstallError(await readError(res), res.status);
      const body = (await res.json()) as { plugins?: PluginInstallRecord[] };
      return body.plugins ?? [];
    },
    /**
     * Owner only: install or update. `version` and `descriptorSha256` pin what
     * the owner reviewed; the API refuses (409) if the listing changed since.
     */
    async install(
      hostAppId: string,
      pluginAppId: string,
      pin: { version: string; descriptorSha256: string; config?: Record<string, unknown> },
    ): Promise<PluginInstallRecord> {
      const res = await doFetch(`${base(options.apiBase)}${pluginPath(hostAppId, pluginAppId)}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json", ...(await auth()) },
        body: JSON.stringify({ version: pin.version, descriptor_sha256: pin.descriptorSha256, config: pin.config ?? {} }),
      });
      if (!res.ok) {
        const body = (await res.json().catch(() => ({}))) as Record<string, unknown>;
        throw new PluginInstallError(
          typeof body.error === "string" ? body.error : `install failed (${res.status})`,
          res.status,
          body,
        );
      }
      return (await res.json()) as PluginInstallRecord;
    },
    /** Owner only. */
    async remove(hostAppId: string, pluginAppId: string): Promise<boolean> {
      const res = await doFetch(`${base(options.apiBase)}${pluginPath(hostAppId, pluginAppId)}`, {
        method: "DELETE",
        headers: await auth(),
      });
      if (res.status === 404) return false;
      if (!res.ok) throw new PluginInstallError(await readError(res), res.status);
      return true;
    },
  };
}

export type PluginInstallClient = ReturnType<typeof createPluginInstallClient>;

/**
 * Trust policy for mounting a plugin inside `hostAppId`: the plugin must be
 * installed there, licensed (paid, when priced), and unchanged since the owner
 * installed it. Combine with signature checks (`allowPublishers`) as needed.
 *
 * ```ts
 * mountSpacekitApp(el, { appId: pluginAppId, trustPolicy: requirePluginLicense({ hostAppId, client }), … });
 * ```
 */
export function requirePluginLicense(options: {
  hostAppId: string;
  client: Pick<PluginInstallClient, "get">;
  /** Receives the record when the plugin may run (e.g. to feed its settings to a `PluginRouter`). */
  onInstall?: (record: PluginInstallRecord) => void;
}) {
  return async (info: { appId: string }): Promise<true | string> => {
    let record: PluginInstallRecord | null;
    try {
      record = await options.client.get(options.hostAppId, info.appId);
    } catch (err) {
      return `Could not check this plugin's license: ${err instanceof Error ? err.message : String(err)}`;
    }
    if (!record) return "This plugin is not installed in this app";
    if (!record.licensed) return "This app's license for the plugin is missing or has expired";
    if (record.updateAvailable && record.currentDescriptorSha256 && record.currentDescriptorSha256 !== record.descriptorSha256) {
      return "The plugin changed since it was installed. The app owner needs to review and approve the new version";
    }
    options.onInstall?.(record);
    return true;
  };
}
