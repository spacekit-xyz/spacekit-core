# SpaceKit Plugins v1

A plugin is a finished feature that one author publishes and other authors add to their apps: a chat panel, a paid newsletter, a support agent, a trained brain. This document defines what a plugin is, how it is described, how an app installs one safely, and how plugin authors get paid.

Implementation: `@spacekit/sdk/plugins` (descriptor validation, install rules, brain loading, signatures), `mountSpacekitApp({ install })` in `@spacekit/sdk/embed`, and the `spacekit-plugin` command.

---

## 1. Building blocks and plugins

SpaceKit has two layers. Keeping them separate is the point of this spec.

| Layer | What it is | Who provides it | Optional? |
|---|---|---|---|
| **Building blocks** | Identity, storage, messaging, compute (contracts), payments (ASTRA), AI engines | SpaceKit (the network and SDKs) | No. Every app has them. |
| **Plugins** | Features built from the blocks: group chat, file sharing with expiring links, paid access, a storefront, a booking calendar, an AI agent, a brain | Anyone | Yes. Apps pick them. |

Messaging, storage and compute are therefore not plugins. They are what plugins are made of. An app never "installs storage"; it installs "file sharing", which uses storage.

The building blocks already have one surface inside an app (`window.spacekit`, see `SPACEKIT-EMBED-PROTOCOL.md`): `identity`, `storage`, `documents`, `messaging`, `payments`, `http`. Plugins use the same surface, with tighter limits (§5).

## 2. Kinds

| Kind | What ships | Runs where | Example |
|---|---|---|---|
| `ui` | A web package (`.spkg`) with an HTML entry | An isolated frame inside the host app | Group chat panel |
| `service` | A web package with no visible UI; other code calls its methods | An isolated frame, hidden | Translation, moderation |
| `contract` | A deployed contract (or its WASM) | The SpaceKit chain | Escrow, raffle, tipping |
| `brain` | Trained model files for an AI engine | Wherever the engine runs | A Growformer brain for pet care |

A plugin can depend on others (`requires`). An agent is usually a `contract` or `service` that requires a `brain`.

## 3. The descriptor (`spacekit-plugin.json`)

```json
{
  "spacekit_plugin": 1,
  "id": "acme.group-chat",
  "name": "Group chat",
  "version": "0.3.1",
  "kind": "ui",
  "publisher": "did:key:z6Mk…",
  "summary": "A chat panel for your community.",
  "license": "proprietary",
  "uses": ["messaging", "identity"],
  "permissions": ["messaging"],
  "ui": { "entry": "index.html", "placement": "panel" },
  "config": {
    "title": { "type": "string", "label": "Title", "default": "Chat", "max_length": 20 },
    "max_members": { "type": "number", "label": "Max members", "min": 2, "max": 500, "required": true }
  },
  "pricing": { "model": "subscription", "amount_wei": "2000000000000000000", "period_days": 30 }
}
```

| Field | Required | Meaning |
|---|---|---|
| `spacekit_plugin` | yes | `1` |
| `id` | yes | Lower-case dotted segments, `publisher.thing`. Stable across versions. |
| `name`, `summary` | yes | What people see. `summary` is one plain sentence (≤ 200 chars). |
| `version` | yes | Semantic version. |
| `kind` | yes | `ui`, `service`, `contract` or `brain`. |
| `publisher` | yes | Publisher DID. A claim; trust comes from the signature (§7). |
| `license`, `homepage`, `description` | no | `homepage` must be https. |
| `uses` | no | Building blocks used: `identity`, `storage`, `messaging`, `compute`, `payments`, `ai`. Shown on the install screen. |
| `permissions` | no | Same shapes as app manifest permissions. Not allowed for brains. |
| `config` | no | Settings the app owner fills in (§4). |
| `pricing` | no | What the app owner pays (§6). Absent means free. |
| `requires` | no | `[{ "id", "version" }]`, version exact or `^`/`~` range. |
| `files` | kind-dependent | `[{ "path", "role", "sha256", "size" }]`, every file pinned by hash. |
| `ui` / `service` / `contract` / `brain` | for that kind only | Kind-specific section. |

Kind sections:

- `ui`: `entry` (required), `placement` (`inline`, `panel`, `page`), `min_height`.
- `service`: `methods`, each with a `description` and optional `params` (same field types as `config`).
- `contract`: `addresses` by network and/or a `files` entry with role `wasm`; `calls` with a `description` and optional `fee_wei`.
- `brain`: see §8.

`spacekit-plugin validate` and `validatePluginDescriptor()` report every problem at once, in plain language.

## 4. Settings for non-technical owners

`config` fields drive a generated settings form, so an app owner configures a plugin without code. Types: `string`, `text`, `number`, `boolean`, `select` (with `options`), `url`, `did`, `astra` (an amount in wei), `color` (`#rrggbb`). Each field has a `label` people read, and optional `help`, `default`, `required`, `min`, `max`, `max_length`.

`resolvePluginConfig(descriptor, values)` applies defaults, rejects unknown keys (so typos surface) and returns messages that name the field's label.

Settings are not secrets. A plugin that needs an API key should get it from its own backend, not from `config`.

## 5. Installing a plugin

Installing records `{ plugin id, exact version, package or descriptor hash, settings }` in the host app. Four rules apply.

### 5.1 No more than the app has

A plugin gets the intersection of what it declares and what the host app was granted. `checkPluginPermissions(plugin.permissions, hostApp.permissions)` returns `allowed`, `missing` and `missingHosts`. If anything is missing, the host refuses the install and says why ("This plugin needs to send messages, which your app does not do"). `pluginCapabilityPolicy({ check, base })` builds the frame's capability policy: only the shared capabilities, `network: "manifest"`, and the refusals in 5.3.

Validation also refuses outright:

- `identity:auth-headers` (the viewer's session token);
- `payments` (see 5.3).

### 5.2 The app owner owns the data

A plugin's documents live in the host app owner's storage namespace, under the plugin's app id and a per-install scope:

```
app_<pluginAppId>_i<first 16 hex of hostAppId>_<collection>
```

- **Isolation between installs:** one install cannot read another install's data, even for the same plugin.
- **Author access:** the plugin author cannot read any install's data. The data is not in the author's namespace.
- **Enforcement:** the storage node's app tokens (`POST /api/auth/delegate { app_id: <pluginAppId>, act_as: <host owner> }`) already confine the plugin to `app_<pluginAppId>_*`, and `list`/`delete` stay with the owner. No storage node change is needed.

Hosts mount a plugin with:

```ts
const scope = pluginInstallScope({ pluginAppId, hostAppId, hostOwnerDid });
mountSpacekitApp(el, {
  appId: scope.pluginAppId,
  install: { collectionScope: scope.collectionScope, ownerDid: scope.ownerDid },
  capabilities: pluginCapabilityPolicy({ check, base: hostPolicy }),
  acquireBridge: (id, origin, _name, collectionScope) =>
    acquireAppDataSdkBridge(services, http, id, origin, collectionScope),
  // …the usual storageOrigin, services, trustPolicy, requestPermissions
});
```

### 5.3 No charging the app's viewers

Inside a plugin frame, `payments.*` and `identity.authHeaders` are refused. The app owner has the relationship with their viewers. Plugins are paid by the app owner (§6), and contracts can charge per call with `collect_fee` when the caller attaches the fee.

### 5.4 Pinned versions

An install pins the exact version and hash. Updates are offered, never applied silently. If an update declares new permissions, it needs a new approval from the app owner.

## 6. Pricing and payment

Prices are in ASTRA, SpaceKit's only currency, as wei strings (`amount_wei`). USD fields are rejected.

| `model` | Meaning |
|---|---|
| `free` | No charge. |
| `one_time` | The app owner pays once per install. |
| `subscription` | The app owner pays every `period_days` (default 30). |

- **Who receives it:** payment goes to the publisher through the `astra-entitlement-ledger` (a listing per plugin version line), or directly to `pay_to`. SpaceKit takes no cut.
- **Checking payment:** hosts check the install's entitlement with a read-only ledger call (or the website API's verified purchase record) in their `trustPolicy` before mounting a paid plugin.
- **Per-call fees:** a `contract` plugin may also charge per call (`calls.*.fee_wei`, enforced on chain by `collect_fee`).
- **Wording:** plugin revenue is payment for software. Do not describe plugins, brains or their sales as investments, yields or returns.

## 7. Distribution and trust

| Kind | Distributed as | Integrity |
|---|---|---|
| `ui`, `service` | `.spkg` with `spacekit-plugin.json` inside | Package publisher signature (`signatures/publisher.json`, see the embed protocol §10) covers the descriptor and every file. |
| `brain`, `contract` | Descriptor plus files (for example marketplace listing artifacts) | Descriptor signature `spacekit-plugin.sig.json`: Ed25519 over `"SpaceKit plugin descriptor v1\n" + hex(sha256(descriptor bytes))`. The descriptor pins every file by SHA-256. |

- **Signature envelope:** same as SPKG signatures: `{ "v": 1, "alg": "ed25519", "did", "public_key", "signature" }`.
- **Verifying:** `verifyPluginDescriptorSignature()` checks the signature. Hosts then compare the signer with `publisher` or an allowlist (`allowPublishers`).
- **Signing:** `spacekit-plugin sign --key <seed-hex-file>` writes the envelope.
- **Marketplace:** listings mirror the descriptor so the marketplace can show `summary`, `uses`, permissions, price and, for brains, training provenance before anyone downloads anything.

## 8. Brains

Brains for Growformer and Growformer-LLM are plugins of kind `brain`. They are a different kind from code plugins for one reason: **a brain is data, not code.**

- **What a brain cannot do:** it cannot call the network, read storage or act as anyone. It cannot declare permissions, and its `uses` can only be `["ai"]`.
- **What the risks are:** integrity (the files are what the author published), size, licensing, and what the brain was trained on. The brain descriptor covers each of these.
- **Composition:** it separates the people who train brains from the people who build agents and apps with them.

```json
{
  "spacekit_plugin": 1,
  "id": "acme.pets-brain",
  "name": "Pets brain",
  "version": "1.2.0",
  "kind": "brain",
  "publisher": "did:key:z6Mk…",
  "summary": "Answers questions about pet care.",
  "license": "CC-BY-4.0",
  "uses": ["ai"],
  "brain": {
    "engine": "growformer",
    "engine_version": "^0.1.0",
    "runs_on": ["browser"],
    "abilities": ["converse"],
    "base": { "id": "spacekit.growformer-base", "version": "1.0.0" },
    "training": {
      "summary": "Public pet-care guides",
      "data_sources": ["…"],
      "includes_captured_conversations": false
    }
  },
  "files": [
    { "path": "pets.bin", "role": "bin", "sha256": "…", "size": 41943040 },
    { "path": "inference.toml", "role": "inference_toml", "sha256": "…", "size": 812 },
    { "path": "guardrails.jsonl", "role": "guardrails_jsonl", "sha256": "…", "size": 2048 },
    { "path": "topics.toml", "role": "topic_graph", "sha256": "…", "size": 4096 }
  ],
  "pricing": { "model": "one_time", "amount_wei": "5000000000000000000" }
}
```

- **`engine` and `engine_version`:**
  - `engine` names the runtime that executes the brain, for example `growformer` or `growformer-llm`.
  - Brains are not interchangeable between engines. A loader refuses a brain whose engine or version range does not match.
  - `runs_on` tells hosts whether it fits in a browser, needs Node, or runs on a compute node.
- **Files and roles:** every brain ships exactly one weights file, whose role depends on the engine (`bin` for `growformer` and `growformer-llm`, `system1` for `growformer-system1`). The other roles are:
  - the roles marketplace listings already use: `inference_toml`, `guardrails_jsonl`, `fragments_jsonl`, `topic_graph` (plus optional `topic_graph_overlay`), `grounding_toml`, `companion_ui`;
  - growformer-llm and System 1 project files: `label_model` (`agent/label-model.json`), `system1` (`*.s1.json.gz`), `lm_checkpoint` (a chat model checkpoint) and `tokenizer`.

  Unknown roles are allowed with a warning. Engines skip roles they do not use.
- **Open-weight base models:**
  - A `growformer-system1` brain can run on a third-party open-weight model (GGUF; Qwen3, Qwen2 or Llama), which is gigabytes and has its own license.
  - Do not put the model in `files`. Declare it as `brain.base_model: { name, arch, sha256, size, license, source }`.
  - The install screen then shows the extra download and the model's license.
- **Lineage:** `base` records which brain this one was trained or tuned from, so a fine-tuned brain credits and depends on its base.
- **Provenance:** `training.summary` and `data_sources` are shown on the install screen.
  - `includes_captured_conversations` must be true when the brain was trained on conversations collected through Growformer capture.
  - Hosts should not offer such brains for sensitive uses without telling people.
- **Loading:**
  - `loadBrainPlugin(descriptor, { engine, fetchFile })` downloads every file and checks each file's size and SHA-256.
  - Only when all files verify does it load them, weights first, then rules, guardrails, fragments, topic graph and grounding.
  - A tampered file means nothing is loaded.
  - `growformerEngine(spacekitJs)` adapts the spacekit-js Growformer runtime. Other engines implement the small `BrainEngine` interface.
- **Several brains per page:** Growformer builds with the instance API hold many brains, each with its own conversation, rules, topic graph and grounding. Create one `GrowformerInstance` per brain (spacekit-js) and load it with `growformerInstanceEngine`. Older builds hold one brain; loading another replaces it.
- **Engines:**
  - `growformer`: GWFBRPKG brain packages, format 1 or 2, optionally gzip-wrapped as GWFCMPKG. It runs in browsers (WASM) and natively.
  - `growformer-llm`: domain language models that use a Growformer brain as memory. Native only today.
  - `growformer-system1`: calibrated typed decisions (`*.s1.json[.gz]`), a separate format.

  `growformer_engine_info()` reports the engine name, version and the brain formats a build reads. The crate versions today are `growformer` 0.1.0, `growformer-llm` 0.2.0 and `growformer-system1` 0.1.0, so `engine_version` ranges are `^0.1.0` (or `^0.2.0` for growformer-llm).
- **Terminology:** Growformer's own "brain plugins" (`[train].brain_plugins_toml`, `BrainPluginsManifest`) are inference settings embedded in the `.bin` (format 2). They are part of a brain's weights file, not SpaceKit plugins, and need no separate descriptor. To avoid confusion in Growformer's docs, consider renaming them to "brain settings".
- **How several brains share one runtime:**
  - Growformer keeps inference rules, the topic graph and the domain grounding graph process-wide.
  - Each instance (`growformer/src/brain_instances.rs`) saves its own copies, together with its brain, and makes them active before every call. Switching is by reference, with no re-parsing.
  - Id 0 is the single-brain API, which keeps working unchanged.

### 8.1 Where brains stand today

Models already appear in the marketplace as `app_listings` whose `artifacts[]` pair a `bin` brain with sidecars (kit.space reads them in `models/registry.ts`). Those listings have no hashes or engine fields, so they cannot be verified or matched to an engine. Migration:

1. **Publish descriptors:** when publishing a brain, the CLI writes a `spacekit-plugin.json` with hashes from `spacekit-plugin hash bin=… inference_toml=…`, signs it, and stores the descriptor and signature with the listing.
2. **Badge verified brains:** kit.space shows "verified" for listings with a valid descriptor signature and loads brains only through `loadBrainPlugin`.
3. **Retire unpinned listings:** listings without descriptors stay visible but are marked unverified until republished.

## 9. Making it easy for people who do not code

Plugins become easy only when something assembles them. The pieces, in order:

1. **Settings forms:** generated from `config` (§4).
2. **Plain install screens:** `describePlugin()` produces the lines: what it does, what it uses, what it can do, price, and for brains, engine, size and training.
3. **Templates:** a template is a list of plugin ids, versions and settings ("community", "shop", "creator page", "AI helper"). A template has no code of its own, so it reuses everything above.
4. **The app spec:**
   - `app.spacekit.yaml` (SPACEKITAPP-SPEC) gains `plugins:`, a list of `{ id, version, config }` the generator installs.
   - A plugin's `service.methods` can be referenced as capabilities, and `ui` plugins as widgets.
   - This lets a builder (or an AI assistant) compose an app from plugins declaratively.

## 10. Developer workflow

```bash
spacekit-plugin init ui --id acme.group-chat --publisher did:key:z6Mk…
spacekit-plugin validate                      # errors, warnings, and the install screen text
spacekit-plugin hash bin=pets.bin inference_toml=inference.toml   # brains: files entries
spacekit-plugin sign --key publisher.seed     # brains/contracts: descriptor signature
spacekit app package … --sign-key publisher.seed   # ui/service: signed .spkg with the descriptor inside
```

The Rust CLI has the same commands plus brain and listing support:

```bash
spacekit plugin brain-descriptor pets.bin --inference-toml inference.toml --topic-graph topics.toml \
  --publisher did:key:z6Mk… --price-astra 5 --license CC-BY-4.0 --training-summary "Public pet-care guides"
spacekit plugin sign --key publisher.seed
spacekit plugin verify
spacekit plugin publish --app-id <listing id>              # attach descriptor + signature to the listing
spacekit storage deploy … --publish --plugin-key publisher.seed   # publish a brain with a signed descriptor in one step
```

`brain-descriptor` reads the brain package header (name, version, description, base brain) so the descriptor matches the brain.

For a Growformer project, point it at the manifest instead:

```bash
spacekit plugin brain-descriptor --project crypto-sentiment-analysis.gf.toml --publisher did:key:z6Mk… \
  --training-summary "Labelled crypto and DeFi posts"
```

It takes the brain (`[infer].brain` or `[train].brain_output`), `[inference]` rules, guardrails, topic graph (plus `knowledge_graph_sentiment_overlay.toml` when it sits next to the graph), grounding, and the project name and description from the manifest. Explicit flags win. With `--engine growformer-llm`, it also pins `agent/label-model.json` and the `[system1]` brain.

## 11. Calls between an app and its plugins

An app calls a `service` plugin through the host, never frame to frame:

```text
app    → host    plugins.call    { plugin, method, params }
host   → plugin  event "__plugins:request" { requestId, method, params }
plugin → host    plugins.respond { requestId, result } | { requestId, error }
```

- **Checked by the host:** `PluginRouter` (`@spacekit/sdk/plugins`) checks that the plugin is installed in this app, that `method` is in `service.methods`, and that `params` match the declared fields (types, required, no extras).
- **Answers:** only the plugin a request went to can answer it. Calls time out after 30 seconds and fail if the plugin frame stops.
- **In the guest:**
  - Apps use `spacekit.plugins.call(id, method, params)` and `spacekit.plugins.list()`.
  - Service plugins register handlers with `spacekit.plugins.serve(method, handler)`.
  - Any plugin reads its settings with `spacekit.plugins.config()`.
- **v1 limit:** plugins cannot call other plugins.
- **Host wiring:** hosts pass the same router to `mountSpacekitApp({ plugins: { router } })` for the app and for each plugin (plugins also get `install`).

## 12. Website API

Listings carry the plugin as `plugin: { descriptor, signature }`, attached by `spacekit plugin publish`.

**Listing status.** When serving a listing, the API adds `plugin.status`: `verified`, `signer`, `publisher_matches_listing`, `descriptor_sha256`, `id`, `version`, `kind` and `reason`. The status is computed on every read; a stored document cannot set it.

**Install routes.** All are under `/api/apps/:app_id/plugins`. Records are stored in the `plugin_installs` collection.

| Route | Who | What |
|---|---|---|
| `PUT …/:plugin_app_id` `{ version, descriptor_sha256, config? }` | Host app owner (full session) | Installs or updates. It is refused with 409 if the listing changed since the owner reviewed it, and with 402 `{ price_wei }` if the plugin is priced and the owner has no active purchase. Unverified plugins are refused unless `PLUGINS_ALLOW_UNVERIFIED=true` (development). |
| `GET …` | Host app owner | Lists the installs. |
| `GET …/:plugin_app_id` | Anyone | Returns the record plus a live `licensed` check, `currentVersion`, `currentDescriptorSha256` and `updateAvailable`. |
| `DELETE …/:plugin_app_id` | Host app owner | Uninstalls. |

**SDK helpers.**

- `createPluginInstallClient({ apiBase, authorization })` wraps these routes.
- `requirePluginLicense({ hostAppId, client })` is a `trustPolicy` that refuses to mount a plugin that is not installed, not licensed, or changed since install.

## 13. Status

Done:

- the descriptor format, validator and signatures;
- install rules (permission intersection, per-install data owned by the app owner, no payments inside plugins);
- brain verification and loading;
- plugin calls (§11);
- the website API (§12) and license checks;
- the `spacekit-plugin` (Node) and `spacekit plugin` (Rust CLI) commands, including `brain-descriptor`, which reads a Growformer brain's own header, and signed brain publishing (`spacekit storage deploy … --publish --plugin-key`);
- kit.space plugin pages:
  - trust badges;
  - an install screen with a generated settings form;
  - "Check files";
  - loading a brain in the browser when `VITE_GROWFORMER_PKG_URL` is set;
- a multi-instance Growformer WASM runtime (`growformer_instance_*` and `*_i` exports), with `GrowformerInstance` in spacekit-js and `growformerInstanceEngine` in the SDK.

Next:

1. **Templates and `plugins:` in the app spec.**
2. **Install-time checks in the API:** check settings against the descriptor's `config` fields, and check the permission intersection against the host app's manifest.
3. **Entitlement-ledger licenses:** today the API checks marketplace purchases; it should also accept entitlement-ledger licenses via a read-only `verify` call.
4. **Growformer engine details:**
   - Guardrails JSONL loads only in native Growformer; the browser build has no loader yet.
   - `growformer-llm` has no WASM build and runs on Node or a compute node only, so its brains should say `runs_on: ["node"]`.
   - `growformer-system1` has a WASM build (`--features wasm`) but no spacekit-js loader yet. Add a `BrainEngine` for it so its brains can load in browsers.
5. **Rebuild `growformer-pkg`:** the multi-instance exports need a fresh `wasm-pack` build of growformer with the `wasm-bindgen` feature. Until then, pages fall back to one brain at a time.
