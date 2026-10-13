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
