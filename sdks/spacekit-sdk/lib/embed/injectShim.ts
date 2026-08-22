import type { EmbedShimConfig } from "./types.js";

function normalizeAssetPath(ref: string): string {
  return ref.replace(/^\.\//, "").replace(/^\//, "");
}

function uint8ToBase64(bytes: Uint8Array): string {
  const chunkSize = 0x8000;
  const parts: string[] = [];
  for (let i = 0; i < bytes.length; i += chunkSize) {
    parts.push(String.fromCharCode(...bytes.subarray(i, i + chunkSize)));
  }
  return btoa(parts.join(""));
}

function findPackagedWasmBase64(assetUrls: Map<string, string>, wasmBytes: Map<string, Uint8Array>): string | undefined {
  for (const path of assetUrls.keys()) {
    if (!path.endsWith(".wasm")) continue;
    const bytes = wasmBytes.get(path);
    if (bytes) return uint8ToBase64(bytes);
  }
  return undefined;
}

function packagedWasmBlobUrl(assetUrls: Map<string, string>): string | undefined {
  for (const [path, url] of assetUrls) {
    if (path.endsWith(".wasm")) return url;
  }
  return undefined;
}

function resolveAssetUrl(ref: string, assetUrls: Map<string, string>): string | undefined {
  const norm = normalizeAssetPath(ref);
  if (assetUrls.has(norm)) return assetUrls.get(norm);
  for (const [path, url] of assetUrls) {
    if (path === norm || path.endsWith(`/${norm}`)) return url;
  }
  return undefined;
}

function factStreamUrl(storageBase: string, factIdHex: string): string {
  return `${storageBase}/facts/${encodeURIComponent(factIdHex)}/stream`;
}

/** Rewrite src/href in packaged HTML so assets resolve to storage stream URLs (not the host origin). */
function rewriteAssetRefs(html: string, assetUrls: Map<string, string>): string {
  return html.replace(
    /\b(src|href)\s*=\s*("([^"]+)"|'([^']+)')/gi,
    (match, attr: string, _q: string, dquote?: string, squote?: string) => {
      const ref = dquote ?? squote ?? "";
      if (!ref || /^(https?:|blob:|data:|mailto:|#)/i.test(ref)) return match;
      // Vite absolute `/src/...` → package-relative `src/...`
      const normalized = normalizeAssetPath(ref);
      const blobUrl = resolveAssetUrl(normalized, assetUrls) ?? resolveAssetUrl(ref, assetUrls);
      return blobUrl ? `${attr}="${blobUrl}"` : match;
    },
  );
}

/** Import map so blob:-origin module scripts can resolve sibling chunks (e.g. kyber_wasm-*.js). */
function buildImportMapScript(assetUrls: Map<string, string>): string {
  const imports: Record<string, string> = {};
  for (const [path, url] of assetUrls) {
    if (!path.endsWith(".js")) continue;
    const norm = normalizeAssetPath(path);
    const base = norm.split("/").pop() ?? norm;
    imports[`./${base}`] = url;
    imports[`./${norm}`] = url;
    if (norm.startsWith("assets/")) {
      imports[`./assets/${base}`] = url;
    }
  }
  if (Object.keys(imports).length === 0) return "";
  return `<script type="importmap">${JSON.stringify({ imports })}</script>`;
}

/** Inject before the first module script — import maps must precede module graph loads. */
function injectBeforeModuleScripts(html: string, fragments: string): string {
  const mod = html.search(/<script\s+type=["']module["']/i);
  if (mod === -1) {
    const headClose = html.indexOf("</head>");
    if (headClose !== -1) return html.slice(0, headClose) + fragments + html.slice(headClose);
    return fragments + html;
  }
  return html.slice(0, mod) + fragments + html.slice(mod);
}

function buildEmbedContainStyle(): string {
  return `<style id="sk-embed-contain">
html,body{height:100%!important;margin:0!important;overflow:hidden!important}
body{display:flex!important;align-items:center!important;justify-content:center!important;background:#000!important}
#cabinet,#game-root,.game-frame,.arcade-cabinet{
  width:min(100vw,calc(100vh * 400 / 640),560px)!important;
  height:min(100vh,calc(100vw * 640 / 400),896px)!important;
  max-width:100%!important;
  max-height:100%!important;
  aspect-ratio:400/640!important;
  margin:0 auto!important;
  flex-shrink:0!important;
}
#cabinet::before{display:none!important}
</style>`;
}

function buildMobileAudioUnlockPatch(): string {
  return `<script id="sk-mobile-audio-unlock">(function(){
  var AC=window.AudioContext||window.webkitAudioContext;
  if(!AC||window.__skAudioPatched)return;
  window.__skAudioPatched=true;
  var Orig=AC;
  function Patched(){
    var ctx=new Orig();
    var unlock=function(){try{if(ctx.state==="suspended")ctx.resume();}catch(e){}};
    document.addEventListener("touchstart",unlock,{once:true,passive:true});
    document.addEventListener("touchend",unlock,{once:true,passive:true});
    document.addEventListener("pointerdown",unlock,{once:true,passive:true});
    document.addEventListener("click",unlock,{once:true});
    return ctx;
  }
  Patched.prototype=Orig.prototype;
  window.AudioContext=window.webkitAudioContext=Patched;
})();</script>`;
}

export function injectSdkBridgeIntoHtml(
  html: string,
  assetUrls: Map<string, string>,
  config: EmbedShimConfig,
): string {
  html = rewriteAssetRefs(html, assetUrls);
  const { appId, parentOrigin, endpoints, identityDid, kyberWasmBase64, contentFit } = config;
  const packagedWasm = packagedWasmBlobUrl(assetUrls);
  const embedConfig = {
    parentOrigin,
    wasmUrl: packagedWasm ?? endpoints.wasmUrl ?? `${parentOrigin}/wasm/kyber_wasm_bg.wasm`,
    ...(kyberWasmBase64 ? { kyberWasmBase64 } : {}),
    messagingBase: endpoints.messagingBase ?? "",
    apiBase: endpoints.apiBase ?? "",
    reposApiBase: endpoints.reposApiBase ?? "",
    identityDid: identityDid ?? "",
    workspacesApiBase: endpoints.workspacesApiBase ?? "",
    assetUrls: Object.fromEntries(assetUrls),
  };
  const embedApiBaseJson = JSON.stringify((endpoints.apiBase ?? "").trim().replace(/\/$/, ""));
  const heightFix = `<style>html,body,#root{height:100%;margin:0;overflow:hidden;background:#0c1020;color:#eef0f7;touch-action:manipulation;-webkit-text-size-adjust:100%}</style>`;
  const containFix = contentFit === "contain" ? buildEmbedContainStyle() : "";
  const audioUnlock = buildMobileAudioUnlockPatch();
  const script = `<script>
(function(){
  var pending={},nextId=1;
  window.__SPACEKIT_EMBED__=${JSON.stringify(embedConfig)};
  var embedApiBase=${embedApiBaseJson};
  function skRewriteEmbedUrl(url){
    if(!embedApiBase)return String(url);
    var u=String(url);
    var prod="https://api.spacekit.xyz";
    if(u.indexOf(prod)===0)u=embedApiBase+u.slice(prod.length);
    u=u.replace(/\\/api\\/storage\\/api\\/documents\\//g,"/api/documents/");
    return u;
  }
  window.spacekit={
    appId:${JSON.stringify(appId)},
    call:function(mod,method,params){
      return new Promise(function(res,rej){
        var id=nextId++;
        pending[id]={resolve:res,reject:rej};
        parent.postMessage({type:"spacekit-sdk-call",id:id,module:mod,method:method,params:params},"*");
      });
    },
    http:{
      fetch:function(url,init){
        return Promise.resolve().then(function(){
          var headers={};
          if(init&&init.headers){
            if(init.headers instanceof Headers)init.headers.forEach(function(v,k){headers[k]=v});
            else if(Array.isArray(init.headers))init.headers.forEach(function(p){headers[p[0]]=p[1]});
            else headers=Object.assign({},init.headers);
          }
          var bodyPromise=Promise.resolve(undefined);
          if(init&&init.body!=null){
            if(typeof Blob!=="undefined"&&(init.body instanceof Blob||init.body instanceof ArrayBuffer)){
              bodyPromise=Promise.resolve(init.body instanceof Blob?init.body.arrayBuffer():init.body).then(function(buf){
                var bytes=new Uint8Array(buf);
                var chunk=0x8000;
                var parts=[];
                for(var i=0;i<bytes.length;i+=chunk){
                  parts.push(String.fromCharCode.apply(null,bytes.subarray(i,i+chunk)));
                }
                headers["X-Body-Encoding"]="base64";
                return btoa(parts.join(""));
              });
            }else{
              bodyPromise=Promise.resolve(String(init.body));
            }
          }
          return bodyPromise.then(function(body){
            return window.spacekit.call("http","fetch",{url:String(url),init:{method:init&&init.method,headers:headers,body:body}});
          });
        }).then(function(r){
          if(r.binary){
            var raw=atob(r.body);
            var arr=new Uint8Array(raw.length);
            for(var i=0;i<raw.length;i++)arr[i]=raw.charCodeAt(i);
            return new Response(arr,{status:r.status,statusText:r.statusText,headers:r.headers});
          }
          return new Response(r.body,{status:r.status,statusText:r.statusText,headers:r.headers});
        });
      },
      sseSubscribe:function(url){return window.spacekit.call("http","sseSubscribe",{url:String(url)})},
      sseClose:function(id){return window.spacekit.call("http","sseClose",{id:id})},
    },
    storage:{
      get:function(k){return window.spacekit.call("storage","get",{key:k})},
      set:function(k,v){return window.spacekit.call("storage","set",{key:k,value:v})},
      list:function(p){return window.spacekit.call("storage","list",{prefix:p||""})},
      delete:function(k){return window.spacekit.call("storage","delete",{key:k})},
      ready:function(){return window.spacekit.call("storage","ready",{})},
      putBlob:function(blob){return window.spacekit.call("storage","putBlob",{blob:blob})},
      getBlob:function(cid){return window.spacekit.call("storage","getBlob",{cid:cid})},
      putRecord:function(k,v){return window.spacekit.call("storage","putRecord",{key:k,value:v})},
      getRecord:function(k){return window.spacekit.call("storage","getRecord",{key:k})},
      listRecords:function(p){return window.spacekit.call("storage","listRecords",{prefix:p||""})},
      deleteRecord:function(k){return window.spacekit.call("storage","deleteRecord",{key:k})},
    },
    messaging:{
      publish:function(topic,msg){return window.spacekit.call("messaging","publish",{topic:topic,msg:msg})},
      send:function(to,c){return window.spacekit.call("messaging","send",{to:to,content:c})},
      list:function(){return window.spacekit.call("messaging","list",{})},
      subscribe:function(topic,cb){
        if(!window.__skTopicSubs)window.__skTopicSubs={};
        if(!window.__skTopicSubs[topic])window.__skTopicSubs[topic]=[];
        var id=Math.random().toString(36).slice(2);
        window.__skTopicSubs[topic].push({id:id,cb:cb});
        return function(){window.__skTopicSubs[topic]=(window.__skTopicSubs[topic]||[]).filter(function(x){return x.id!==id})};
      },
    },
    contracts:{
      anchor:function(id,contentHash){return window.spacekit.call("contracts","anchor",{noteId:id,fileId:id,contentHash:contentHash})},
      verify:function(id){return window.spacekit.call("contracts","verify",{noteId:id,fileId:id})},
      createShare:function(input){return window.spacekit.call("contracts","createShare",{input:input})},
      revokeShare:function(shareId){return window.spacekit.call("contracts","revokeShare",{shareId:shareId})},
      /** Generic facet invoke (Token Wall et al.): method name + positional args. */
      invoke:function(method,args){return window.spacekit.call("contracts","invoke",{method:method,args:args||[]})},
      call:function(method,args){return window.spacekit.call("contracts","call",{method:method,args:args||[]})},
      status:function(){return window.spacekit.call("contracts","status",{})},
    },
    payments:{
      status:function(){return window.spacekit.call("payments","status",{})},
      subscribe:function(o){return window.spacekit.call("payments","subscribe",o||{})},
      config:function(){return window.spacekit.call("payments","config",{})},
      charge:function(a,t){return window.spacekit.call("payments","charge",{amount:a,token:t})},
    },
    identity:{
      did:function(){return window.spacekit.call("identity","did",{})},
      getState:function(){return window.spacekit.call("identity","getState",{})},
      setState:function(s){return window.spacekit.call("identity","setState",s||{})},
      authHeaders:function(){return window.spacekit.call("identity","authHeaders",{})},
    },
    crypto:{
      encryptUpload:function(blob,ownerPubkey){return window.spacekit.call("crypto","encryptUpload",{blob:blob,ownerPubkey:ownerPubkey||"demo-owner"})},
      decryptBlob:function(cid,iv,wrappedKey){return window.spacekit.call("crypto","decryptBlob",{cid:cid,iv:iv,wrappedKey:wrappedKey})},
    },
    app:{
      ready:function(){return window.spacekit.call("app","ready",{})},
      isOwner:function(){return window.spacekit.call("app","isOwner",{})},
      ownerDid:function(){return window.spacekit.call("app","ownerDid",{})},
    },
    documents:{
      get:function(collection,id){return window.spacekit.call("documents","get",{collection:collection,id:id})},
      put:function(collection,id,data){return window.spacekit.call("documents","put",{collection:collection,id:id,data:data})},
      list:function(collection){return window.spacekit.call("documents","list",{collection:collection})},
      delete:function(collection,id){return window.spacekit.call("documents","delete",{collection:collection,id:id})},
    },
  };
  window.addEventListener("message",function(e){
    if(e.data&&e.data.type==="spacekit-sdk-response"&&pending[e.data.id]){
      var p=pending[e.data.id];delete pending[e.data.id];
      if(e.data.error)p.reject(new Error(e.data.error));else p.resolve(e.data.result);
    }
    if(e.data&&e.data.type==="spacekit-sdk-event"){
      if(window.__skTopicSubs){
        var subs=window.__skTopicSubs[e.data.topic];
        if(subs)subs.forEach(function(s){s.cb(e.data.msg)});
      }
      if(window.__skSseStreams&&e.data.topic&&String(e.data.topic).indexOf("__sse:")===0){
        var stream=window.__skSseStreams[e.data.topic.slice(6)];
        if(stream){
          var msg=e.data.msg||{};
          if(msg.type==="message"&&stream.onmessage)stream.onmessage({data:msg.data});
          if(msg.type==="error"&&stream.onerror)stream.onerror({});
        }
      }
    }
  });
  var origFetch=window.fetch.bind(window);
  window.fetch=function(input,init){
    var url=typeof input==="string"?input:(input&&input.url?input.url:String(input));
    url=skRewriteEmbedUrl(url);
    try{
      var parsed=new URL(url,window.location.href);
      if(parsed.protocol!=="http:"&&parsed.protocol!=="https:")return origFetch(input,init);
      if(parsed.pathname.endsWith(".wasm"))return origFetch(input,init);
    }catch(_e){return origFetch(input,init);}
    return window.spacekit.http.fetch(url,init||{});
  };
  var OrigES=window.EventSource;
  window.EventSource=function(url,opts){
    if(!window.spacekit||!window.spacekit.http)return new OrigES(url,opts);
    var self=this;
    this.onmessage=null;this.onerror=null;this.onopen=null;this.readyState=0;this.withCredentials=!!(opts&&opts.withCredentials);
    this.close=function(){
      self.readyState=2;
      if(self.__streamId)window.spacekit.http.sseClose(self.__streamId);
      self.__streamId=null;
    };
    if(!window.__skSseStreams)window.__skSseStreams={};
    window.spacekit.http.sseSubscribe(skRewriteEmbedUrl(String(url))).then(function(id){
      self.__streamId=id;
      window.__skSseStreams[id]=self;
      self.readyState=1;
      if(self.onopen)self.onopen({});
    }).catch(function(){self.readyState=2;if(self.onerror)self.onerror({});});
    this.addEventListener=function(type,cb){
      if(type==="message")self.onmessage=cb;
      if(type==="error")self.onerror=cb;
      if(type==="open")self.onopen=cb;
    };
    this.removeEventListener=function(){};
    return this;
  };
})();
</script>`;
  const viewportMeta = /<meta[^>]+name=["']viewport["']/i.test(html)
    ? ""
    : '<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">';
  const i = html.indexOf("</head>");
  const headInjection = viewportMeta + buildImportMapScript(assetUrls) + heightFix + containFix + audioUnlock + script;
  if (i !== -1) {
    return injectBeforeModuleScripts(html, headInjection);
  }
  return headInjection + html;
}

