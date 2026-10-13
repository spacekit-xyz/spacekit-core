// Run after `npm run build`: npm test
import assert from "node:assert/strict";
import { test } from "node:test";
import { createHash, generateKeyPairSync, sign as edSign } from "node:crypto";

const P = await import("../dist/plugins/index.js");
const A = await import("../dist/astra.js");
const Pay = await import("../dist/embed/payments.js");
const { AppDataSdkBridge, acquireAppDataSdkBridge } = await import("../dist/embed/appDataBridge.js");

const sha = (b) => createHash("sha256").update(b).digest("hex");
const enc = (s) => new TextEncoder().encode(s);

const brainBin = new Uint8Array([1, 2, 3, 4, 5]);
const rules = enc('[rules]\nmax_tokens = 64\n');
const topic = enc('[topics]\na = "b"\n');
const overlay = enc('[topics]\nc = "d"\n');
const extra = enc("whatever");

function brainDescriptor(overrides = {}) {
  return {
    spacekit_plugin: 1,
    id: "acme.pets-brain",
    name: "Pets brain",
    version: "1.2.0",
    kind: "brain",
    publisher: "did:key:z6MkExample",
    summary: "Answers questions about pet care.",
    license: "CC-BY-4.0",
    uses: ["ai"],
    pricing: { model: "one_time", amount_wei: "1500000000000000000" },
    brain: {
      engine: "growformer",
      engine_version: "^0.4.0",
      runs_on: ["browser"],
      abilities: ["converse"],
      training: { summary: "Public pet-care guides", data_sources: ["guides"], includes_captured_conversations: false },
    },
    files: [
      { path: "pets.bin", role: "bin", sha256: sha(brainBin), size: brainBin.length },
      { path: "inference.toml", role: "inference_toml", sha256: sha(rules), size: rules.length },
      { path: "topics.toml", role: "topic_graph", sha256: sha(topic), size: topic.length },
      { path: "overlay.toml", role: "topic_graph_overlay", sha256: sha(overlay), size: overlay.length },
      { path: "notes.txt", role: "notes", sha256: sha(extra), size: extra.length },
    ],
    ...overrides,
  };
}

test("astra amounts", () => {
  assert.equal(A.parseAstra("1.5"), 1500000000000000000n);
  assert.equal(A.parseAstra("0.000000000000000001"), 1n);
  assert.equal(A.parseAstra("1.0000000000000000001"), null);
  assert.equal(A.parseAstra("-1"), null);
  assert.equal(A.parseWei("12"), 12n);
  assert.equal(A.parseWei(1.5), null);
  assert.equal(A.formatAstra(1500000000000000000n), "1.5");
  assert.equal(A.formatAstra("2000000000000000000"), "2");
  assert.equal(A.formatAstra(1n, 6), "<0.000001");
  assert.equal(A.priceFromParams({ amountAstra: "2" }, "x"), 2000000000000000000n);
  assert.equal(A.priceFromParams({ amountWei: "7" }, "x"), 7n);
  assert.throws(() => A.priceFromParams({ amountCents: 500 }, "x"), /in ASTRA/);
  assert.throws(() => A.priceFromParams({ amountAstra: "0" }, "x"), /invalid amount/);
});

test("brain descriptor validates and describes", () => {
  const r = P.validatePluginDescriptor(JSON.stringify(brainDescriptor()));
  assert.equal(r.ok, true, JSON.stringify(r.errors));
  assert.ok(r.warnings.some((w) => w.includes('"notes"')));
  const lines = P.describePlugin(r.descriptor);
  assert.ok(lines.includes("Price: 1.5 ASTRA, once"));
  assert.ok(lines.some((l) => l.startsWith("A trained AI brain")));
  assert.ok(lines.some((l) => l.includes("growformer")));
});

test("descriptor errors are collected and explained", () => {
  const bad = brainDescriptor({
    id: "Bad Id",
    version: "1",
    permissions: ["messaging"],
    pricing: { model: "one_time", amount_cents: 500 },
    uses: ["ai", "messaging"],
    files: [{ path: "../x.bin", role: "bin", sha256: "nothex", size: -1 }],
    ui: { entry: "index.html" },
  });
  const r = P.validatePluginDescriptor(bad);
  assert.equal(r.ok, false);
  const all = r.errors.join("\n");
  for (const needle of ["id must be", "version must be semantic", "cannot declare permissions", "USD", "only list \"ai\"", "must be relative", "sha256", "size", "ui is only allowed", 'role "bin"']) {
    assert.ok(all.includes(needle), `missing error about ${needle}:\n${all}`);
  }
  assert.throws(() => P.parsePluginDescriptor(bad), P.PluginDescriptorError);
});

test("ui plugin: permissions rules, config and install policy", () => {
  const ui = {
    spacekit_plugin: 1,
    id: "acme.group-chat",
    name: "Group chat",
    version: "0.3.1",
    kind: "ui",
    publisher: "did:key:z6MkAuthor",
    summary: "A chat panel for your community.",
    uses: ["messaging", "identity"],
    permissions: ["messaging", "network:api.acme.com"],
    ui: { entry: "index.html", placement: "panel" },
    config: {
      title: { type: "string", label: "Title", default: "Chat", max_length: 20 },
      theme: { type: "select", label: "Theme", options: ["light", "dark"], default: "dark" },
      tip: { type: "astra", label: "Suggested tip" },
      max_members: { type: "number", label: "Max members", min: 2, max: 500, required: true },
    },
    pricing: { model: "subscription", amount_wei: "1000", period_days: 30 },
  };
  const d = P.parsePluginDescriptor(ui);
  assert.deepEqual(P.resolvePluginConfig(d, { max_members: 50 }), { title: "Chat", theme: "dark", max_members: 50 });
  assert.throws(() => P.resolvePluginConfig(d, {}), /Max members: required/);
  assert.throws(() => P.resolvePluginConfig(d, { max_members: 1 }), /at least 2/);
  assert.throws(() => P.resolvePluginConfig(d, { max_members: 5, colour: "x" }), /no such setting/);
  assert.throws(() => P.resolvePluginConfig(d, { max_members: 5, tip: "1.5" }), /wei/);

  assert.equal(P.validatePluginDescriptor({ ...ui, permissions: ["payments"] }).ok, false);
  assert.equal(P.validatePluginDescriptor({ ...ui, permissions: [{ Identity: { read_only: false } }, "identity:auth-headers"] }).ok, false);

  const hostApp = ["messaging", { type: "network", hosts: ["*.acme.com"] }];
  const ok = P.checkPluginPermissions(d.permissions, hostApp);
  assert.deepEqual(ok.missing, []);
  const lacking = P.checkPluginPermissions(d.permissions, ["network:api.other.com"]);
  assert.deepEqual(lacking.missing.sort(), ["messaging", "network"]);
  assert.deepEqual(lacking.missingHosts, ["api.acme.com"]);
  const policy = P.pluginCapabilityPolicy({ check: lacking, base: { deny: ["crypto"], trustedOrigins: ["https://host.app"] } });
  assert.equal(policy.network, "manifest");
  for (const d2 of ["crypto", "payments", "messaging", "identity:auth-headers"]) assert.ok(policy.deny.includes(d2));
  assert.deepEqual(policy.trustedOrigins, ["https://host.app"]);
});

test("install scope keeps installs apart and owned by the app owner", async () => {
  const scope = P.pluginInstallScope({ pluginAppId: "ABCDEF", hostAppId: "0x1234abcd5678ef901234", hostOwnerDid: "did:spacekit:00000000000000000000000000000000000000aa" });
  assert.equal(scope.collectionScope, "i1234abcd5678ef90");
  assert.equal(scope.pluginAppId, "abcdef");
  assert.throws(() => P.pluginInstallScope({ pluginAppId: "a", hostAppId: "zz", hostOwnerDid: "did:x:y" }), /hex/);

  const calls = [];
  globalThis.fetch = async (url, init) => {
    calls.push({ url: String(url), init });
    return new Response(JSON.stringify({ document: { id: "1", data: { n: 1 } } }), { status: 200 });
  };
  const services = {
    getViewerDid: () => "did:key:viewer",
    getIdentityDid: () => null,
    handleIdentity: () => null,
    getAppCredentials: async (req) => ({ storageAuthorization: `Bearer tok-for-${req.appId}-${req.publisherDid}` }),
  };
  const http = () => null;
  const a = acquireAppDataSdkBridge(services, http, "abcdef", "https://store", scope.collectionScope);
  const b = acquireAppDataSdkBridge(services, http, "abcdef", "https://store", "i99");
  const plain = acquireAppDataSdkBridge(services, http, "abcdef", "https://store");
  assert.notEqual(a, b);
  assert.notEqual(a, plain);
  a.setOwnerDid(scope.ownerDid);
  plain.setOwnerDid("did:key:author");
  await a.handle("documents", "get", { collection: "msgs", id: "1" });
  await plain.handle("documents", "get", { collection: "msgs", id: "1" });
  assert.equal(calls[0].url, "https://store/api/documents/app_abcdef_i1234abcd5678ef90_msgs/1");
  assert.equal(calls[0].init.headers.Authorization, `Bearer tok-for-abcdef-${scope.ownerDid}`);
  assert.equal(calls[1].url, "https://store/api/documents/app_abcdef_msgs/1");
  assert.throws(() => new AppDataSdkBridge("ab", "https://s", services, http, "../x"), /install scope/);
});

test("subscriptions are paid and recorded in ASTRA", async () => {
  const recorded = [];
  const prompts = [];
  const transfers = [];
  const services = {
    getViewerDid: () => "did:key:viewer",
    getIdentityDid: () => null,
    handleIdentity: () => null,
    requestSubscriptionPayment: Pay.createAstraPaymentRequester({
      confirm: (p) => (prompts.push(p), true),
      transfer: async (to, wei) => (transfers.push([to, wei]), "0xabc"),
    }),
    recordSubscription: async (req) => (recorded.push(req), { active: true, expiresAt: 1, viewerDid: "did:key:viewer", amountWei: req.amountWei, asset: "ASTRA", verified: true }),
  };
  const bridge = new AppDataSdkBridge("feed01", "https://store", services, () => null);
  bridge.setOwnerDid("did:spacekit:testnet:00000000000000000000000000000000000000AB");
  const status = await bridge.handle("payments", "subscribe", { amountAstra: "2.5", periodDays: 7 });
  assert.equal(status.verified, true);
  assert.deepEqual(transfers, [["0x00000000000000000000000000000000000000ab", 2500000000000000000n]]);
  assert.equal(prompts[0].amountAstra, "2.5");
  assert.deepEqual(recorded[0], { appId: "feed01", publisherDid: "did:spacekit:testnet:00000000000000000000000000000000000000AB", amountWei: "2500000000000000000", periodDays: 7, txHash: "0xabc" });
  await assert.rejects(bridge.handle("payments", "subscribe", { amountCents: 499 }), /in ASTRA/);
  const cfg = await bridge.handle("payments", "config", {});
  assert.equal(cfg.currency, "ASTRA");

  const noAddress = new AppDataSdkBridge("feed02", "https://store", services, () => null);
  noAddress.setOwnerDid("did:key:z6MkNoChainAddress");
  await assert.rejects(noAddress.handle("payments", "subscribe", { amountWei: "5" }), /no ASTRA payout address/);
  const declined = Pay.createAstraPaymentRequester({ confirm: () => false, transfer: async () => "0x" });
  await assert.rejects(declined({ publisherDid: "did:spacekit:00000000000000000000000000000000000000ab", appId: "x", amountWei: "1", asset: "ASTRA" }), /cancelled/);
});

test("subscription recorder sends amount_wei", async () => {
  const { createSubscriptionRecorder } = await import("../dist/embed/storageAuth.js");
  let body;
  const rec = createSubscriptionRecorder({
    apiBase: "https://api.example/",
    authorization: async () => "Bearer s",
    fetchImpl: async (url, init) => {
      body = JSON.parse(init.body);
      assert.equal(url, "https://api.example/api/apps/feed01/subscriptions");
      return new Response(JSON.stringify({ active: true }), { status: 200 });
    },
  });
  await rec({ appId: "feed01", publisherDid: "did:key:p", amountWei: "10", periodDays: 30, txHash: "0x1" });
  assert.deepEqual(body, { publisher_did: "did:key:p", amount_wei: "10", period_days: 30, tx_hash: "0x1" });
});

test("brain plugin loads in order after verification", async () => {
  const d = P.parsePluginDescriptor(brainDescriptor());
  const bytes = { "pets.bin": brainBin, "inference.toml": rules, "topics.toml": topic, "overlay.toml": overlay, "notes.txt": extra };
  const fetchFile = async (f) => bytes[f.path];
  const log = [];
  const runtime = {
    loadGrowformerBrain: (b) => log.push(["brain", b.length]),
    loadGrowformerInferenceToml: (t) => log.push(["rules", t]),
    loadGrowformerInferenceGuardrailsJsonl: () => log.push(["guardrails"]),
    loadGrowformerFragmentsJsonl: () => log.push(["fragments"]),
    loadGrowformerTopicGraph: (b, o) => log.push(["topics", b, o]),
    loadGrowformerGroundingGraph: () => log.push(["grounding"]),
  };
  const progress = [];
  const res = await P.loadBrainPlugin(d, { engine: P.growformerEngine(runtime, { version: "0.4.2" }), fetchFile, onProgress: (p) => progress.push(p.phase) });
  assert.deepEqual(res.loaded, ["bin", "inference_toml", "topic_graph"]);
  assert.deepEqual(res.skipped, ["notes"]);
  assert.deepEqual(log.map((l) => l[0]), ["brain", "rules", "topics"]);
  assert.equal(log[2][2], new TextDecoder().decode(overlay));
  assert.ok(progress.includes("verify") && progress.includes("load"));

  // Tampered file: nothing is loaded.
  log.length = 0;
  const tampered = { ...bytes, "inference.toml": enc('[rules]\nmax_tokens = 99\n') };
  await assert.rejects(P.loadBrainPlugin(d, { engine: P.growformerEngine(runtime), fetchFile: async (f) => tampered[f.path] }), /size|SHA-256/);
  assert.equal(log.length, 0);
  // Wrong engine or version.
  await assert.rejects(P.loadBrainPlugin(d, { engine: P.growformerEngine(runtime, { name: "growformer-llm" }), fetchFile }), /needs the growformer engine/);
  await assert.rejects(P.loadBrainPlugin(d, { engine: P.growformerEngine(runtime, { version: "0.5.0" }), fetchFile }), /\^0\.4\.0/);
  await assert.rejects(P.loadBrainPlugin(d, { engine: P.growformerEngine(runtime), fetchFile, maxTotalBytes: 3 }), /limit/);
});

test("versions", () => {
  assert.ok(P.versionSatisfies("1.4.0", "^1.2.0"));
  assert.ok(!P.versionSatisfies("2.0.0", "^1.2.0"));
  assert.ok(P.versionSatisfies("0.4.9", "^0.4.0"));
  assert.ok(!P.versionSatisfies("0.5.0", "^0.4.0"));
  assert.ok(P.versionSatisfies("1.2.9", "~1.2.0"));
  assert.ok(!P.versionSatisfies("1.3.0", "~1.2.0"));
  assert.ok(P.versionSatisfies("1.2.0", "1.2.0"));
  assert.ok(!P.versionSatisfies("1.2.0-beta", "^1.0.0"));
});

test("descriptor signatures", async () => {
  const { publicKey, privateKey } = generateKeyPairSync("ed25519");
  const pub = Buffer.from(publicKey.export({ format: "jwk" }).x, "base64url");
  const did = `did:key:z6Mk${pub.toString("hex").slice(0, 44)}`; // kit.space short form
  const bytes = enc(JSON.stringify(brainDescriptor({ publisher: did })));
  const msg = await P.pluginSigningMessage(bytes);
  const sig = edSign(null, Buffer.from(msg), privateKey).toString("hex");
  const env = JSON.stringify({ v: 1, alg: "ed25519", did, public_key: pub.toString("hex"), signature: sig });
  assert.deepEqual(await P.verifyPluginDescriptorSignature(bytes, env), { valid: true, did });
  const other = enc(JSON.stringify(brainDescriptor({ publisher: did, version: "9.9.9" })));
  assert.equal((await P.verifyPluginDescriptorSignature(other, env)).valid, false);
  const wrongDid = JSON.stringify({ v: 1, alg: "ed25519", did: "did:key:z6Mkdeadbeef", public_key: pub.toString("hex"), signature: sig });
  assert.match((await P.verifyPluginDescriptorSignature(bytes, wrongDid)).reason, /DID/);
});

test("plugin router: app calls a service plugin through the host", async () => {
  const svc = P.parsePluginDescriptor({
    spacekit_plugin: 1, id: "acme.translate", name: "Translate", version: "1.0.0", kind: "service",
    publisher: "did:key:z6MkA", summary: "Translates text.",
    service: { methods: { translate: { description: "Translate text", params: {
      text: { type: "string", label: "Text", required: true, max_length: 100 },
      to: { type: "select", label: "Language", options: ["en", "es"], default: "en" },
    } } } },
  });
  const router = new P.PluginRouter({ timeoutMs: 50 });
  router.install({ pluginAppId: "ABC123", descriptor: svc, config: { tone: "formal" } });
  assert.deepEqual(router.list().map((p) => [p.id, p.methods]), [["acme.translate", ["translate"]]]);
  await assert.rejects(router.handleAppCall("call", { plugin: "acme.translate", method: "translate", params: { text: "hola" } }), /not running/);

  // The plugin frame answers requests it receives as events.
  router.attachFrame("abc123", (topic, msg) => {
    assert.equal(topic, P.PLUGIN_REQUEST_TOPIC);
    queueMicrotask(() => router.handlePluginCall("abc123", "respond", { requestId: msg.requestId, result: `${msg.params.to}:${msg.params.text}` }));
  });
  assert.equal(await router.handleAppCall("call", { plugin: "acme.translate", method: "translate", params: { text: "hola" } }), "en:hola");
  assert.equal(await router.handleAppCall("call", { plugin: "abc123", method: "translate", params: { text: "hi", to: "es" } }), "es:hi");
  await assert.rejects(router.handleAppCall("call", { plugin: "acme.translate", method: "delete", params: {} }), /no method/);
  await assert.rejects(router.handleAppCall("call", { plugin: "acme.translate", method: "translate", params: { text: "x", to: "fr" } }), /one of/);
  await assert.rejects(router.handleAppCall("call", { plugin: "acme.translate", method: "translate", params: { extra: 1, text: "x" } }), /not a parameter/);
  await assert.rejects(router.handleAppCall("call", { plugin: "nope", method: "translate", params: {} }), /not installed/);
  assert.deepEqual(router.handlePluginCall("abc123", "config", {}), { tone: "formal" });
  assert.throws(() => router.handlePluginCall("abc123", "call", {}), /not available to plugins/);
  assert.throws(() => router.handlePluginCall("other", "config", {}), /not installed/);

  // Errors from the plugin, answers from the wrong plugin, timeouts, detach.
  router.attachFrame("abc123", (_t, msg) => queueMicrotask(() => router.handlePluginCall("abc123", "respond", { requestId: msg.requestId, error: "quota exceeded" })));
  await assert.rejects(router.handleAppCall("call", { plugin: "abc123", method: "translate", params: { text: "a" } }), /quota exceeded/);
  router.install({ pluginAppId: "def456", descriptor: { ...svc, id: "acme.other" }, config: {} });
  let lastId;
  router.attachFrame("abc123", (_t, msg) => { lastId = msg.requestId; });
  const pending = router.handleAppCall("call", { plugin: "abc123", method: "translate", params: { text: "a" } });
  assert.equal(router.handlePluginCall("def456", "respond", { requestId: lastId, result: "forged" }), false);
  await assert.rejects(pending, /did not answer in time/);
  const pending2 = router.handleAppCall("call", { plugin: "abc123", method: "translate", params: { text: "a" } });
  router.detachFrame("abc123");
  await assert.rejects(pending2, /stopped/);
});

test("requirePluginLicense", async () => {
  const rec = (over) => ({ hostAppId: "h", pluginAppId: "p", pluginId: "acme.x", version: "1.0.0", descriptorSha256: "aa", kind: "ui", config: { a: 1 }, ownerDid: "did:x:o", installedAt: "", updatedAt: "", license: { required: true }, licensed: true, currentDescriptorSha256: "aa", updateAvailable: false, ...over });
  let seen;
  const policy = (r) => P.requirePluginLicense({ hostAppId: "h", client: { get: async () => r }, onInstall: (x) => { seen = x; } });
  assert.equal(await policy(rec({}))({ appId: "p" }), true);
  assert.deepEqual(seen.config, { a: 1 });
  assert.match(await policy(null)({ appId: "p" }), /not installed/);
  assert.match(await policy(rec({ licensed: false }))({ appId: "p" }), /license/);
  assert.match(await policy(rec({ updateAvailable: true, currentDescriptorSha256: "bb" }))({ appId: "p" }), /changed since/);
  const failing = P.requirePluginLicense({ hostAppId: "h", client: { get: async () => { throw new Error("offline"); } } });
  assert.match(await failing({ appId: "p" }), /offline/);

  const calls = [];
  const client = P.createPluginInstallClient({
    apiBase: "https://api.example/",
    authorization: () => "Bearer s",
    fetchImpl: async (url, init = {}) => {
      calls.push([String(url), init.method ?? "GET", init.headers?.Authorization, init.body]);
      if (String(url).endsWith("/missing")) return new Response("{}", { status: 404 });
      if (init.method === "PUT") return new Response(JSON.stringify({ error: "pay first", price_wei: "5" }), { status: 402 });
      return new Response(JSON.stringify(rec({})), { status: 200 });
    },
  });
  assert.equal((await client.get("H", "P")).licensed, true);
  assert.equal(await client.get("h", "missing"), null);
  await assert.rejects(client.install("h", "p", { version: "1.0.0", descriptorSha256: "aa" }), (e) => e.status === 402 && e.details.price_wei === "5");
  assert.deepEqual(calls[0].slice(0, 2), ["https://api.example/api/apps/h/plugins/p", "GET"]);
  assert.equal(calls[2][2], "Bearer s");
  assert.deepEqual(JSON.parse(calls[2][3]), { version: "1.0.0", descriptor_sha256: "aa", config: {} });
});

test("brain plugins load into separate Growformer instances", async () => {
  const d = P.parsePluginDescriptor(brainDescriptor());
  const bytes = { "pets.bin": brainBin, "inference.toml": rules, "topics.toml": topic, "overlay.toml": overlay, "notes.txt": extra };
  const make = () => {
    const state = { brain: 0, rules: null, topics: null };
    return { state, inst: {
      loadBrain: (b) => { state.brain = b.length; },
      loadInferenceToml: (t) => { state.rules = t; },
      loadFragmentsJsonl: () => 0,
      loadTopicGraph: (b, o) => { state.topics = [b, o]; },
      loadGroundingGraph: () => {},
    } };
  };
  const a = make();
  const b = make();
  await P.loadBrainPlugin(d, { engine: P.growformerInstanceEngine(a.inst, { engine: "growformer", version: "0.4.1" }), fetchFile: async (f) => bytes[f.path] });
  assert.equal(a.state.brain, brainBin.length);
  assert.equal(b.state.brain, 0);
  assert.ok(a.state.rules.includes("max_tokens"));
  await assert.rejects(P.loadBrainPlugin(d, { engine: P.growformerInstanceEngine(b.inst, { engine: "growformer", version: "0.1.0" }), fetchFile: async (f) => bytes[f.path] }), /\^0\.4\.0/);
  // The default-runtime adapter takes the version from growformerEngineInfo and never loads guardrails.
  const eng = P.growformerEngine({ loadGrowformerBrain() {}, loadGrowformerInferenceToml() {}, loadGrowformerFragmentsJsonl() {}, loadGrowformerTopicGraph() {}, loadGrowformerGroundingGraph() {}, growformerEngineInfo: () => ({ engine: "growformer", version: "0.4.3" }) });
  assert.equal(eng.version, "0.4.3");
  assert.equal(eng.loadGuardrailsJsonl, undefined);
});

test("growformer-system1 brains, extra roles and open base models", async () => {
  const s1 = enc("system1-heads");
  const d = brainDescriptor({
    id: "acme.support-decisions",
    brain: {
      engine: "growformer-system1",
      runs_on: ["browser", "node"],
      training: { summary: "Labelled support tickets" },
      base_model: { name: "Qwen3-0.6B-Q8_0.gguf", arch: "qwen3", sha256: "a".repeat(64), size: 639 * 1024 * 1024, license: "Apache-2.0", source: "https://huggingface.co/Qwen" },
    },
    files: [{ path: "support.s1.json.gz", role: "system1", sha256: sha(s1), size: s1.length }],
  });
  const r = P.validatePluginDescriptor(d);
  assert.equal(r.ok, true, JSON.stringify(r.errors));
  assert.equal(P.weightsRole("growformer-system1"), "system1");
  assert.ok(P.describePlugin(r.descriptor).some((l) => l.includes("Qwen3-0.6B-Q8_0.gguf") && l.includes("Apache-2.0")));
  // A system1 brain without its weights, or a base model without a license, is refused.
  assert.match(P.validatePluginDescriptor({ ...d, files: [] }).errors.join(), /role "system1"/);
  const noLicense = structuredClone(d);
  delete noLicense.brain.base_model.license;
  assert.match(P.validatePluginDescriptor(noLicense).errors.join(), /license/);
  // The loader hands the system1 weights to the engine.
  let got;
  const res = await P.loadBrainPlugin(r.descriptor, {
    engine: { name: "growformer-system1", loadBrain: (b) => { got = b.length; } },
    fetchFile: async () => s1,
  });
  assert.equal(got, s1.length);
  assert.deepEqual(res.loaded, ["system1"]);
  // growformer-llm roles are known (no warnings).
  const llm = brainDescriptor({
    brain: { engine: "growformer-llm", runs_on: ["node"], training: { summary: "x" } },
    files: [
      { path: "b.bin", role: "bin", sha256: sha(brainBin), size: brainBin.length },
      { path: "label-model.json", role: "label_model", sha256: "b".repeat(64), size: 10 },
      { path: "chat.json", role: "lm_checkpoint", sha256: "c".repeat(64), size: 10 },
      { path: "chat.tok", role: "tokenizer", sha256: "d".repeat(64), size: 10 },
    ],
  });
  const lr = P.validatePluginDescriptor(llm);
  assert.equal(lr.ok, true);
  assert.equal(lr.warnings.filter((w) => w.includes("not a known brain role")).length, 0);
});
