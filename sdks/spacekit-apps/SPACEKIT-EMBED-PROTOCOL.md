# SpaceKit Embed Protocol v1

How a host page runs a SpaceKit web package (`.spkg`) in an isolated frame, and how the app talks to the host. The reference implementation is `@spacekit/sdk/embed` (`mountSpacekitApp`), wrapped by `@spacekit/sdk/react/embed` (`SpacekitAppFrame`, `SpacekitEmbeddedApp`) and `@spacekit/sdk/embed/element` (`<spacekit-app>`). Apps can import types for the guest API from `@spacekit/sdk/guest`.

Status: v1, September 2026. It replaces the unversioned `spacekit-sdk-call` window-message bridge.

---

## 1. Roles

| Role | What it is |
|---|---|
| **Host** | The page embedding an app: kit.space, an org portal, a desktop shell. It owns the viewer's identity and session. |
| **Frame** | The iframe the app runs in. It first runs the *bootstrap*, then the app replaces the bootstrap. |
| **App** | The publisher's HTML/JS/WASM from the package, plus the guest shim the host injects (`window.spacekit`). |
| **Bridge** | The host-side object that answers the app's calls (`EmbeddedSdkBridge`). The default is `AppDataSdkBridge`. |
| **Guard** | The host-side capability check that runs in front of the bridge on every call. |

The security goal: **an app can do nothing as the viewer except through the bridge, and the guard decides what the bridge will do.** In particular, an app must never read the host's storage, cookies, session token or DOM.

## 2. Isolation modes

| Mode | Frame origin | Sandbox | Use when |
|---|---|---|---|
| `opaque` (default) | Opaque (`null`) | `allow-scripts allow-forms allow-modals allow-pointer-lock allow-downloads` | Any third-party app. Works with no extra infrastructure. |
| `origin` | A separate origin you run, e.g. `https://{app}.apps.example.com` | the same, plus `allow-same-origin` (safe because the origin is not the host's) | Apps that need real `localStorage`, IndexedDB or cookies. Orgs deploying privately. |
| `unsafe-same-origin` | The host's origin | the same, plus `allow-same-origin` | Only code you wrote yourself. The app can read everything the host can. The host logs a warning. |

What each mode gives the app:

| | opaque | origin | unsafe-same-origin |
|---|---|---|---|
| Host storage, cookies, DOM | blocked | blocked | **exposed** |
| `window.spacekit` bridge | yes | yes | yes |
| `localStorage` / `sessionStorage` | shimmed (see §7.3) | native, per app origin | native, **shared with host** |
| IndexedDB, cookies, service workers | unavailable | native | native |
| `SharedArrayBuffer` / WASM threads | yes, if the host is cross-origin isolated | yes, if the host and the frame host both send COOP/COEP | yes, if the host is cross-origin isolated |

The package's bytes never become `blob:` URLs owned by the host. A sandboxed, opaque-origin document cannot load a parent-owned `blob:` URL, and a host-owned URL would put the app back on the host's origin. Instead, the frame builds its own URLs (§4).

## 3. Loading sequence

1. **Fetch and verify.** The host fetches the package (`GET {storage}/packages/apps/{appId}` as `.spkg`, falling back to `GET {storage}/facts/{appId}` plus per-file streams). It checks every file's SHA-256 against the manifest and, for `.spkg`, the manifest checksum. Any mismatch stops the load.
2. **Trust policy.** If the host has a `trustPolicy`, it is called with `{ appId, creatorDid, manifest, storageOrigin, signatures, signedBy }` (§10). Anything other than `true` stops the load; a string is shown as the reason. `allowPublishers([...dids])` is a ready-made allowlist.
3. **Consent.** If `manifest.permissions` is non-empty, the host shows the viewer what the app asks for (§6) and waits. If the viewer refuses, or the host has no consent UI, the load stops.
4. **Bridge.** Only now does the host create the bridge and give it the owner DID (`creator_did`).
5. **Frame.** The host inserts the iframe for the chosen mode and runs the handshake (§4).

## 4. Handshake

All window messages carry `v: 1`. Messages with another version are ignored.

```
frame → host   window.postMessage   { type: "spacekit-frame-ready", v: 1 }
host  → frame  window.postMessage   { type: "spacekit-frame-load",  v: 1, appId, html, files, localSeed }
                                    + transfer: [MessagePort]
frame → host   port                 { t: "loaded" }                         (accepted)
frame → host   window.postMessage   { type: "spacekit-frame-error", v: 1, message }   (refused)
```

**Host checks on `spacekit-frame-ready`:** `event.source` must be the frame's `contentWindow`. `event.origin` must be `"null"` (opaque), the resolved app origin (origin), or the host origin (unsafe). Each accepted `ready` gets a new `MessageChannel`. That is how a reload inside the frame reconnects. The host sends `spacekit-frame-load` to the resolved app origin, or to `"*"` for an opaque frame, which cannot be named. The target is still the specific `contentWindow` that was checked.

**`spacekit-frame-load` fields:**

| Field | Meaning |
|---|---|
| `appId` | Hex app id. |
| `html` | The entry HTML after the host injected the guest shim. Every asset reference is replaced by a placeholder `sk-asset-<96-bit nonce>-<n>`. |
| `files` | `[{ ph, mime, bytes: ArrayBuffer }]`, one per verified file. |
| `localSeed` | `{ key: value }` for the `localStorage` shim, or `null`. |

**Frame checks (bootstrap):** the message comes from `window.parent`; `v === 1`; `event.origin` is in the frame's `allowedParents`. With `perAppHost`, the frame's hostname must also start with `<first 32 hex chars of appId>.`. On success the bootstrap:

1. creates a `blob:` URL in its own origin for each file,
2. replaces every placeholder in `html` with that URL,
3. posts `{ t: "loaded" }` on the port and leaves the port on `window.__skPort`,
4. replaces its document with `document.open(); document.write(html); document.close()`.

The guest shim reads `__skPort` and deletes it before any app script runs.

The host marks the app `running` when `loaded` arrives. It fails the mount on `spacekit-frame-error`, or when neither message arrives within `readyTimeoutMs` (default 30 s).

## 5. Calls

After the handshake, all traffic uses the private port. Nothing is broadcast.

```
app  → host   { t: "call",  id: number, module: string, method: string, params: object }
host → app    { t: "res",   id, result }  |  { t: "res", id, error: string }
host → app    { t: "event", topic: string, msg: any }
```

The guard receives each call first. It either rejects it, which becomes a `res` with `error`, or passes it on with sanitized params. Then the bridge's `handle(module, method, params)` runs.

### 5.1 Modules

| Call | Permission needed | Notes |
|---|---|---|
| `app.ready` / `isOwner` / `ownerDid` | none | |
| `storage.get` / `set` / `list` / `delete` | none | Key/value storage scoped to the app id. Default bridge: host `localStorage` under `spacekit:appdata:<appId>:`. |
| `documents.get` / `put` | none | Scoped to `app_<appId>_<collection>` on the storage node. |
| `documents.list` / `delete` | none | Only when the viewer is the app owner. |
| `identity.did` / `getState` | none | Reveals the viewer DID to the app. |
| `identity.setState` | `identity:write` | `myDid` is always removed unless the host sets `allowIdentityOverride`. |
| `identity.authHeaders` | `identity:auth-headers` | Hands over the session token. Refused unless the host sets `exposeAuthHeaders`, a legacy option. |
| `payments.status` / `config` / `subscribe` / `charge` | none | The host's payment UI asks the viewer to confirm. |
| `messaging.publish` / `subscribe` | none | Local to the frame (loopback). |
| `messaging.send` / `list` | `messaging` | |
| `http.fetch` / `sseSubscribe` / `sseClose` | see §6.2 | |
| `contracts.*` | `contracts` | Not implemented by the default bridge. |
| `crypto.*` | `crypto` | Not implemented by the default bridge. |

A host can refuse any capability, module or single call with `capabilities.deny` (for example `["payments", "messaging.send"]`), and grant capabilities to every app with `capabilities.grant`.

## 6. Permissions and network access

### 6.1 Declaring permissions

In the package manifest, `permissions` is an array. The host accepts these shapes:

```json
[
  "messaging",
  "identity:write",
  "network:api.example.com",
  { "type": "network", "hosts": ["*.example.org", "https://cdn.example.net"] },
  { "Network": { "allowed_hosts": ["tiles.example.com"] } },
  { "Identity": { "read_only": false } },
  "Camera"
]
```

The last three are what `spacekit app package --permission network:tiles.example.com --permission identity:write --permission camera` writes (the Rust `Permission` enum). `Camera`, `Microphone`, `Geolocation` and `Clipboard { write: true }` also turn on the matching iframe `allow` feature once the viewer grants them.

`network` with no hosts means any host. The consent prompt lists one plain-language line per entry, such as "Connect to api.example.com" or "Send and read messages as you".

### 6.2 Network policy

`http.fetch` and `http.sseSubscribe` URLs must be `http:` or `https:`. Relative URLs resolve against the host page. The guest shim routes the app's own `fetch()` and `EventSource` through these calls.

| `capabilities.network` | Allowed origins |
|---|---|
| `"open"` (default) | Any |
| `"manifest"` | Trusted origins, plus hosts the manifest declares with `network` |
| `"none"` | Trusted origins only |

Orgs running third-party apps should use `"manifest"`. The default is `"open"` only because existing packages do not declare `network` yet.

### 6.3 Credentials

Apps never receive the viewer's key or session. Credentials are attached host-side, and only where they belong.

**App-scoped credentials (preferred).** A host implements `EmbedHostServices.getAppCredentials({ appId, publisherDid, storageOrigin })`, or passes `appCredentials` to `createLocalStorageEmbedHost`. It returns up to two `Authorization` values:
- `storageAuthorization`: an app token from the storage node (`POST /api/auth/delegate`). The node accepts it only for `/api/documents/app_<appId>_*`. When it acts in the publisher's namespace, only `get` and `put` work; `list` and `delete` are the owner's. The bridge uses it for `documents.*`.
- `apiAuthorization`: an app token from the host API (website-api `POST /api/auth/app-token`, one hour, app documents only). `http.fetch` to trusted origins sends it instead of the session, without cookies.

`createStorageAuthClient` (`@spacekit/sdk/embed`) implements the storage side for hosts whose viewer has an Ed25519 `did:key`. It signs the node's login challenge and caches the session and per-app tokens.

**Fallback.** Without app credentials, the host attaches the viewer's own session to **trusted origins** only: its page origin plus `capabilities.trustedOrigins` / `credentialedOrigins`, and in the React and element wrappers the origins in `endpoints`. That means the session token, the `owner-did` header and same-origin cookies. Set `forwardViewerSession: false` to stop this. The documents bridge falls back to a bare `DID <publisher>` header, which storage nodes in strict mode reject.

Other origins always get only the headers the app set, with `credentials: "omit"`.

## 7. Guest environment

### 7.1 `window.spacekit`

The injected shim provides `appId`, `call()` and the typed modules in §5.1. `@spacekit/sdk/guest` exports the `SpacekitGuestApi` type plus `getSpacekit()` / `isSpacekitHosted()`.

### 7.2 Proxied fetch

The shim patches `fetch` and `EventSource` so absolute http(s) URLs go through `http.fetch` / `http.sseSubscribe`. `.wasm` URLs and the app's own `blob:` assets are fetched directly.

### 7.3 Storage shims (opaque mode)

If the frame's origin has no Web Storage, the shim installs in-memory `localStorage` and `sessionStorage`. `localStorage` writes persist through `storage.set` under the key prefix `__ls:`. The host sends them back in `localSeed` on the next load, so they survive reloads. Values are stored as strings. IndexedDB, cookies and service workers are not available in opaque mode. Apps that need them should run in `origin` mode.

## 8. Deploying an app origin (`isolation: "origin"`)

1. **Choose the origin.** Use a dedicated host that is not a subdomain your host site's cookies are scoped to. `apps.example-usercontent.com` is better than `apps.example.com`. For one origin per app, use wildcard DNS and TLS for `*.apps.example-usercontent.com`, set `appOrigin: "https://{app}.apps.example-usercontent.com"`, and generate the frame host with `--per-app`. `{app}` is the first 32 hex characters of the app id, which fits in one DNS label.
2. **Generate the frame host.**
   ```bash
   npx spacekit-frame-host --allow https://kit.space --allow https://www.kit.space --per-app \
     --out /var/www/spacekit-apps/spacekit-frame.html
   ```
   Or call `renderFrameHostHtml({ allowedParents, perAppHost })` from your own build.
3. **Serve it with these headers:**
   ```nginx
   location = /spacekit-frame.html {
       add_header Content-Security-Policy "frame-ancestors https://kit.space https://www.kit.space" always;
       add_header Cross-Origin-Embedder-Policy require-corp always;   # needed if the host is cross-origin isolated
       add_header Cross-Origin-Opener-Policy same-origin always;
       add_header Cross-Origin-Resource-Policy cross-origin always;
       add_header Cache-Control "no-cache" always;
       add_header X-Content-Type-Options nosniff always;
   }
   location / { return 404; }
   ```
   `frame-ancestors` stops foreign sites from framing the page. The in-page allowlist stops them from loading apps into it even where the header is missing.
4. **Point the host at it:** `<SpacekitEmbeddedApp appOrigin="https://{app}.apps.example-usercontent.com" … />`. The mount refuses an `appOrigin` equal to the host origin.

## 9. Private deployments

An org running SpaceKit apps on its own network typically sets:

```ts
mountSpacekitApp(el, {
  appId,
  storageOrigin: "https://storage.corp.internal",
  services,                // its SSO-backed EmbedHostServices
  acquireBridge,
  appOrigin: "https://{app}.apps.corp-usercontent.internal",
  trustPolicy: allowPublishers(["did:key:z6Mk…internal-publisher"]),
  capabilities: {
    network: "manifest",
    trustedOrigins: ["https://portal.corp.internal", "https://api.corp.internal"],
    deny: ["payments"],
  },
  requestPermissions: showConsentDialog,
});
```

`EmbedHostServices` is the seam for the org's identity. `getViewerDid`, `getIdentityDid` and `handleIdentity` can be backed by any SSO, and the default localStorage host is only one implementation.

## 10. Publisher signatures

An `.spkg` may carry `signatures/publisher.json`:

```json
{ "v": 1, "alg": "ed25519", "did": "did:key:z6Mk…", "public_key": "<64 hex>", "signature": "<128 hex>" }
```

The signature is Ed25519 over the UTF-8 bytes of `"SpaceKit package signature v1\n" + hex(sha256(manifest.json))`. The manifest lists every payload hash and the aggregate checksum, so the signature covers the whole package. `did` must be the `did:key` of `public_key`.

- **Signing:** `spacekit app package … --sign-key <seed-hex-file>`.
- **Storage node:** rejects uploads carrying an invalid signature. With `SPACEKIT_REQUIRE_SIGNED_PACKAGES=true`, it also rejects unsigned ones.
- **SDK:** `openSpkg` throws on an invalid signature. Trust policies receive `signatures` and `signedBy`, the DIDs whose signatures verified. `allowPublishers([dids])` requires a signature by a listed DID by default. `requireSignedPackages()` accepts any valid signature.
- **Legacy uploads:** packages loaded through the legacy facts path are unsigned (`signedBy: []`).

`creator_did` in the manifest is a claim, not proof. Base trust decisions on `signedBy`.

## 11. Known gaps

- **Subscription records are client-asserted.** `payments.subscribe` writes the subscription record through the bridge after the host's payment UI returns a transaction hash. The node does not check that payment. Verify it server-side (the website-api already verifies marketplace payments) before relying on subscriptions for paid access.
- **Non-Ed25519 identities.** Storage login supports Ed25519 `did:key` only. SLH-DSA and `did:spacekit:*` SPHINCS+ users authenticate through a backend holding the storage secret, or keep bare-DID auth until their client migrates.
- **Asset URLs.** Asset paths are rewritten only in the entry HTML. Relative `url()` references in CSS and relative ES-module imports between chunks do not resolve from `blob:` URLs. This is the same as before v1.

## 12. Compatibility

- The guest shim is injected by the host, so packages built for the old bridge need no rebuild. They behave differently only if they call `identity.authHeaders`, write `myDid`, or depend on IndexedDB, cookies or service workers in opaque mode.
- Removed from the old bridge: replies broadcast with `postMessage("*")`, `identity.authHeaders` by default, app-controlled `myDid` writes, and credentials sent to any URL.
- `SpacekitAppFrame`'s `loadPackage` prop is ignored because blob-URL loaders cannot be isolated; use `loadFiles` with `verifyLocalPackageFiles`. `createSpacekitServiceWorkerLoader` is deprecated, since it served apps from the host origin.
