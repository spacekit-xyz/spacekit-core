/**
 * The frame bootstrap: the only code the host puts inside an app's frame before
 * the app itself.
 *
 * It waits for the host's `spacekit-frame-load` message, turns each verified
 * file into a `blob:` URL owned by the *frame's* origin (a parent-owned blob
 * cannot be loaded by a sandboxed, opaque-origin document), substitutes those
 * URLs into the app HTML, and replaces its own document with the app. The
 * MessagePort that arrived with the load message is left on `window.__skPort`
 * for the injected guest shim to pick up; nothing else crosses the boundary.
 *
 * The same script runs in both isolation modes:
 *   - "opaque": the host builds a bootstrap document with {@link buildOpaqueBootstrapHtml}
 *     and loads it into an `allow-scripts`-only sandbox, so the app has an
 *     opaque origin and cannot reach the host's storage, cookies, or DOM.
 *   - "origin": an operator deploys {@link renderFrameHostHtml}'s output on a
 *     dedicated app origin (for example `https://apps.example.com/spacekit-frame.html`),
 *     so the app gets a real but separate origin with working storage.
 */

import { FRAME_ERROR, FRAME_LOAD, FRAME_READY, SPACEKIT_EMBED_PROTOCOL_VERSION } from "./protocol.js";

export interface FrameBootstrapConfig {
  /** Host origins allowed to load an app into this frame. `"*"` allows any (dev only). */
  allowedParents: string[];
  /**
   * Per-app origins: require this frame's hostname to start with the first 32
   * hex characters of the app id followed by a dot (e.g. `3fa0…c1.apps.example.com`),
   * so one app can never be loaded into another app's origin.
   */
  perAppHost?: boolean;
}

function jsonForScript(value: unknown): string {
  // Escape "<" (no </script> breakout) and the two JS line separators.
  return JSON.stringify(value)
    .replace(/</g, "\\u003c")
    .split(String.fromCharCode(0x2028)).join("\\u2028")
    .split(String.fromCharCode(0x2029)).join("\\u2029");
}

/** Plain-JS bootstrap. `cfgExpr` must evaluate to a {@link FrameBootstrapConfig}. */
function bootstrapScript(cfgExpr: string): string {
  return `(function(){
  "use strict";
  var V=${SPACEKIT_EMBED_PROTOCOL_VERSION};
  var cfg=${cfgExpr}||{allowedParents:[]};
  var parents=Array.isArray(cfg.allowedParents)?cfg.allowedParents:[];
  var loaded=false;
  function fail(msg){
    try{console.error("[spacekit-frame] "+msg);}catch(_e){}
    document.body&&(document.body.textContent="This app could not be loaded: "+msg);
    try{window.parent.postMessage({type:${JSON.stringify(FRAME_ERROR)},v:V,message:String(msg)},"*");}catch(_e){}
  }
  function parentAllowed(origin){
    return parents.indexOf("*")!==-1||parents.indexOf(origin)!==-1;
  }
  window.addEventListener("message",function(e){
    var d=e.data;
    if(loaded||!d||d.type!==${JSON.stringify(FRAME_LOAD)})return;
    if(e.source!==window.parent)return;
    if(d.v!==V){fail("unsupported protocol version "+d.v);return;}
    if(!parentAllowed(e.origin)){fail("host "+e.origin+" is not allowed to load apps here");return;}
    var appId=String(d.appId||"").toLowerCase();
    if(cfg.perAppHost){
      var label=appId.replace(/[^0-9a-f]/g,"").slice(0,32);
      if(!label||location.hostname.toLowerCase().indexOf(label+".")!==0){fail("app "+appId+" does not belong to this origin");return;}
    }
    var port=e.ports&&e.ports[0];
    if(!port){fail("missing bridge port");return;}
    loaded=true;
    var html=String(d.html||"");
    var files=Array.isArray(d.files)?d.files:[];
    for(var i=0;i<files.length;i++){
      var f=files[i];
      if(!f||typeof f.ph!=="string"||!f.ph)continue;
      var url=URL.createObjectURL(new Blob([f.bytes],{type:String(f.mime||"application/octet-stream")}));
      html=html.split(f.ph).join(url);
    }
    port.postMessage({t:"loaded"});
    window.__skPort=port;
    window.__skLocalSeed=d.localSeed||null;
    document.open();
    document.write(html);
    document.close();
  });
  window.parent.postMessage({type:${JSON.stringify(FRAME_READY)},v:V},"*");
})();`;
}

/**
 * Bootstrap document for opaque isolation. Only `hostOrigin` may load an app
 * into it, and the document itself must be loaded into an iframe whose sandbox
 * does NOT include `allow-same-origin`.
 */
export function buildOpaqueBootstrapHtml(hostOrigin: string): string {
  const cfg: FrameBootstrapConfig = { allowedParents: [hostOrigin] };
  return `<!DOCTYPE html><html><head><meta charset="utf-8"><script>${bootstrapScript(jsonForScript(cfg))}</script></head><body></body></html>`;
}

export interface FrameHostHtmlOptions {
  /** Host origins allowed to embed apps, e.g. `["https://kit.space"]`. */
  allowedParents: string[];
  perAppHost?: boolean;
}

/**
 * The static page an operator serves on a dedicated app origin for
 * `isolation: "origin"`. Serve it with
 * `Content-Security-Policy: frame-ancestors <allowed hosts>` as well: the
 * in-page allowlist stops a foreign site from loading apps, the header stops a
 * foreign site from framing the page at all.
 */
export function renderFrameHostHtml(options: FrameHostHtmlOptions): string {
  const cfg: FrameBootstrapConfig = {
    allowedParents: options.allowedParents,
    perAppHost: !!options.perAppHost,
  };
  return `<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="robots" content="noindex">
<title>SpaceKit app frame</title>
<!-- Generated by @spacekit/sdk renderFrameHostHtml(). Edit allowedParents to match your host sites. -->
<script>
${bootstrapScript(jsonForScript(cfg))}
</script>
</head>
<body></body>
</html>
`;
}
