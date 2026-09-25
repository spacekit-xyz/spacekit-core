/**
 * `<spacekit-app>`: a framework-free way to run a SpaceKit web package.
 *
 * ```html
 * <script type="module">
 *   import { defineSpacekitAppElement } from "@spacekit/sdk/embed/element";
 *   defineSpacekitAppElement({ storageOrigins: ["https://storage.example.com"] });
 * </script>
 * <spacekit-app app-id="3fa0…c1" style="height: 600px"></spacekit-app>
 * ```
 *
 * Attributes: `app-id` (required), `storage-origin` (one origin or a
 * space-separated list, tried in order), `isolation`, `app-origin`,
 * `content-fit`. Anything richer (custom services, trust policy, capability
 * policy) goes in the defaults passed to `defineSpacekitAppElement`, or on the
 * element's `options` property before it is connected.
 */

import { acquireAppDataSdkBridge } from "./appDataBridge.js";
import { defaultTrustedOrigins } from "./capabilities.js";
import { mountSpacekitApp, type IsolationMode, type MountSpacekitAppOptions, type SpacekitAppHandle } from "./host.js";
import { createLocalStorageEmbedHost, type LocalEmbedHostOptions } from "./localIdentityHost.js";
import { createStorageOriginResolver } from "./storageOrigin.js";

export interface SpacekitAppElementDefaults
  extends Partial<Omit<MountSpacekitAppOptions, "appId" | "storageOrigin" | "onStateChange">> {
  /** Storage origins to probe, best first. Overridden by the `storage-origin` attribute. */
  storageOrigins?: string[];
  /** Options for the default localStorage identity host (ignored with custom `services`). */
  hostOptions?: LocalEmbedHostOptions;
}

const STYLE = `
:host{display:block;position:relative;min-height:200px;
  --sk-bg:#0c0f18;--sk-text:#f9fafb;--sk-muted:#9ca3af;--sk-accent:#22d3ee;--sk-accent-text:#080b0f;
  --sk-border:rgba(255,255,255,0.08);--sk-error:#ef4444;--sk-font:system-ui,sans-serif}
.stage{position:absolute;inset:0;display:flex}
.cover{position:absolute;inset:0;display:flex;align-items:center;justify-content:center;padding:24px;
  background:var(--sk-bg);color:var(--sk-text);font:14px/1.5 var(--sk-font)}
.cover[hidden]{display:none}
.panel{max-width:420px;padding:24px;border:1px solid var(--sk-border);border-radius:14px;text-align:center}
.panel h2{margin:0 0 6px;font-size:17px}
.panel p{margin:0 0 14px;color:var(--sk-muted);font-size:12px}
.panel ul{list-style:none;margin:0 0 18px;padding:0;text-align:left}
.panel li{padding:6px 12px;margin-bottom:4px;border:1px solid var(--sk-border);border-radius:8px;font-size:12px}
.actions{display:flex;gap:8px;justify-content:center}
button{font:600 13px var(--sk-font);border-radius:10px;padding:9px 18px;cursor:pointer}
.deny{background:transparent;color:var(--sk-muted);border:1px solid var(--sk-border)}
.allow{background:var(--sk-accent);color:var(--sk-accent-text);border:none}
.error{color:var(--sk-error);font-weight:700;margin-bottom:6px}
`;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string, cls?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (text != null) node.textContent = text;
  if (cls) node.className = cls;
  return node;
}

export function defineSpacekitAppElement(
  defaults: SpacekitAppElementDefaults = {},
  tagName = "spacekit-app",
): CustomElementConstructor {
  const existing = customElements.get(tagName);
  if (existing) return existing;

  class SpacekitAppElement extends HTMLElement {
    static get observedAttributes() {
      return ["app-id", "storage-origin", "isolation", "app-origin", "content-fit"];
    }

    /** Per-element overrides, merged over the defaults. Set before connecting. */
    options: SpacekitAppElementDefaults = {};

    private handle: SpacekitAppHandle | null = null;
    private stage!: HTMLDivElement;
    private cover!: HTMLDivElement;
    private generation = 0;

    connectedCallback() {
      if (!this.shadowRoot) {
        const root = this.attachShadow({ mode: "open" });
        const style = el("style");
        style.textContent = STYLE;
        this.stage = el("div", undefined, "stage");
        this.cover = el("div", undefined, "cover");
        root.append(style, this.stage, this.cover);
      }
      void this.remount();
    }

    disconnectedCallback() {
      this.generation++;
      this.handle?.unmount();
      this.handle = null;
    }

    attributeChangedCallback() {
      if (this.isConnected && this.shadowRoot) void this.remount();
    }

    private showCover(content: Node | null) {
      this.cover.replaceChildren(...(content ? [content] : []));
      this.cover.hidden = !content;
    }

    private async remount() {
      const gen = ++this.generation;
      this.handle?.unmount();
      this.handle = null;
      const appId = this.getAttribute("app-id")?.trim();
      if (!appId) {
        this.showCover(el("div", "Missing app-id"));
        return;
      }
      this.showCover(el("div", "Loading app…"));

      const cfg: SpacekitAppElementDefaults = { ...defaults, ...this.options };
      const attrOrigins = this.getAttribute("storage-origin")?.split(/\s+/).filter(Boolean);
      const candidates = attrOrigins?.length ? attrOrigins : cfg.storageOrigins ?? [];
      if (!candidates.length) {
        this.showCover(el("div", "Missing storage-origin"));
        return;
      }
      const storageOrigin = await createStorageOriginResolver({ candidates, cache: null })(appId);
      if (gen !== this.generation) return;

      const trustedOrigins =
        cfg.capabilities?.trustedOrigins ??
        defaultTrustedOrigins([
          cfg.endpoints?.apiBase,
          cfg.endpoints?.messagingBase,
          cfg.endpoints?.reposApiBase,
          cfg.endpoints?.workspacesApiBase,
        ]);
      let services = cfg.services;
      let acquireBridge = cfg.acquireBridge;
      if (!services) {
        const host = createLocalStorageEmbedHost({
          ...cfg.hostOptions,
          credentialedOrigins: cfg.hostOptions?.credentialedOrigins ?? trustedOrigins,
        });
        services = host.services;
        acquireBridge ??= (id, origin) => acquireAppDataSdkBridge(host.services, host.httpHandler, id, origin);
      }
      if (!acquireBridge) {
        this.showCover(el("div", "spacekit-app: custom services need an acquireBridge"));
        return;
      }

      const isolation = (this.getAttribute("isolation") as IsolationMode | null) ?? cfg.isolation;
      const fit = this.getAttribute("content-fit");
      this.handle = mountSpacekitApp(this.stage, {
        ...cfg,
        appId,
        storageOrigin,
        services,
        acquireBridge,
        isolation,
        appOrigin: this.getAttribute("app-origin") ?? cfg.appOrigin,
        contentFit: fit === "contain" || fit === "fill" ? fit : cfg.contentFit,
        capabilities: { ...cfg.capabilities, trustedOrigins },
        requestPermissions:
          cfg.requestPermissions ??
          ((req) =>
            new Promise<boolean>((resolve) => {
              const panel = el("div", undefined, "panel");
              const list = el("ul");
              for (const p of req.permissions) list.append(el("li", p));
              const deny = el("button", "Don't allow", "deny");
              const allow = el("button", "Allow & launch", "allow");
              deny.onclick = () => resolve(false);
              allow.onclick = () => resolve(true);
              const actions = el("div", undefined, "actions");
              actions.append(deny, allow);
              panel.append(el("h2", req.manifest.name), el("p", "This app asks to:"), list, actions);
              this.showCover(panel);
            })),
        onStateChange: (state) => {
          if (gen !== this.generation) return;
          if (state.status === "running") this.showCover(null);
          else if (state.status === "loading") this.showCover(el("div", "Loading app…"));
          else if (state.status === "error") {
            const panel = el("div", undefined, "panel");
            panel.setAttribute("role", "alert");
            panel.append(el("div", "Failed to load app", "error"), el("p", state.message));
            this.showCover(panel);
          }
          this.dispatchEvent(new CustomEvent("spacekit-state", { detail: state }));
        },
      });
    }
  }

  customElements.define(tagName, SpacekitAppElement);
  return SpacekitAppElement;
}
