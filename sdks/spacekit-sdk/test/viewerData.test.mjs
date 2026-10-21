// Run after `npm run build`: npm test
import assert from "node:assert/strict";
import { test } from "node:test";
import { createDecipheriv, createHash, hkdfSync, randomBytes } from "node:crypto";

const C = await import("../dist/embed/viewerCrypto.js");
const { ViewerDataStore } = await import("../dist/embed/viewerStore.js");
const { AppDataSdkBridge } = await import("../dist/embed/appDataBridge.js");

const APP = "ab".repeat(32);
const ALICE = "did:key:z6MkAlice000000000000000000000000000000000000";
const BOB = "did:key:z6MkBob00000000000000000000000000000000000000";
const PUBLISHER = "did:spacekit:user:publisher";
const MIB = 1024 * 1024;

// ---------------------------------------------------------------------------
// In-memory storage node with the real node's document rules: documents are
// owner-scoped, app tokens only reach `app_<app>_*`, and with `act_as` a viewer
// may get/put in the publisher's namespace but not list or delete.
// ---------------------------------------------------------------------------
function fakeNode() {
  const docs = new Map(); // `${owner}|${collection}|${id}` -> data
  const tokens = new Map(); // token -> { sub, app, actAs }
  const bodies = [];
  let n = 0;
  const mint = (sub, app, actAs = null) => {
    const t = `sktok1.fake${++n}`;
    tokens.set(t, { sub, app, actAs });
    return `Bearer ${t}`;
  };
  const json = (status, body) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });

  async function fetchImpl(url, init = {}) {
    const method = (init.method ?? "GET").toUpperCase();
    const { pathname } = new URL(url);
    const segs = pathname.split("/").filter(Boolean).map(decodeURIComponent);
    if (segs[0] !== "api" || segs[1] !== "documents") return json(404, { error: "no route" });
    const [, , collection, id] = segs;
    const auth = String(init.headers?.Authorization ?? "").replace(/^Bearer /, "");
    const claims = tokens.get(auth);
    if (!claims) return json(401, { error: "invalid token" });
    if (!collection.startsWith(`app_${claims.app}_`)) return json(403, { error: "out of scope" });
    const op = method === "GET" ? (id === undefined ? "list" : "get") : method === "PUT" ? "put" : "delete";
    let owner = claims.sub;
    if (claims.actAs && claims.actAs !== claims.sub) {
      if (op !== "get" && op !== "put") return json(403, { error: `${op} is owner-only` });
      owner = claims.actAs;
    }
    if (op === "put") {
      const body = String(init.body ?? "");
      bodies.push(body.length);
      if (body.length > MIB) return json(413, { error: "payload too large" });
      docs.set(`${owner}|${collection}|${id}`, JSON.parse(body));
      return json(200, { document: { id } });
    }
    if (op === "get") {
      const k = `${owner}|${collection}|${id}`;
      return docs.has(k) ? json(200, { document: { id, data: docs.get(k) } }) : json(404, { error: "not found" });
    }
    if (op === "delete") {
      docs.delete(`${owner}|${collection}|${id}`);
      return json(200, { deleted: true });
    }
    const out = [];
    for (const [k, data] of docs) {
      const [o, c, i] = k.split("|");
      if (o === owner && c === collection) out.push({ id: i, data });
    }
    return json(200, { documents: out });
  }
  return { docs, bodies, mint, fetchImpl };
}

function memoryLocalStorage() {
  const m = new Map();
  return {
    get length() {
      return m.size;
    },
    key: (i) => [...m.keys()][i] ?? null,
    getItem: (k) => (m.has(k) ? m.get(k) : null),
    setItem: (k, v) => m.set(k, String(v)),
    removeItem: (k) => m.delete(k),
    clear: () => m.clear(),
  };
}

const seeds = { [ALICE]: new Uint8Array(32).fill(1), [BOB]: new Uint8Array(32).fill(2) };

/** A bridge as a host would set it up, for whoever `viewer.did` is. */
function makeBridge(node, viewer, { permissions = [] } = {}) {
  const services = {
    getViewerDid: () => viewer.did,
    getIdentityDid: () => viewer.did,
    handleIdentity: () => null,
    async getAppCredentials(req) {
      if (!viewer.did) return null;
      if (req.namespace === "viewer") return { viewerStorageAuthorization: node.mint(viewer.did, req.appId) };
      return { storageAuthorization: node.mint(viewer.did, req.appId, req.publisherDid) };
    },
    async deriveAppKey(req) {
      const seed = seeds[req.viewerDid];
      return seed ? C.deriveAppDataKey(seed, req) : null;
    },
  };
  const bridge = new AppDataSdkBridge(APP, "https://storage.test", services, () => null);
  bridge.setOwnerDid(PUBLISHER);
  bridge.setManifest({ name: "t", description: "", entry_points: [], permissions, total_size: 0, checksum: "" });
  return bridge;
}

function withFetch(node, fn) {
  return async () => {
    const prevFetch = globalThis.fetch;
    const prevLs = globalThis.localStorage;
    globalThis.fetch = node.fetchImpl;
    globalThis.localStorage = memoryLocalStorage();
    try {
      await fn();
    } finally {
      globalThis.fetch = prevFetch;
      globalThis.localStorage = prevLs;
    }
  };
}

const bytesOf = async (blob) => new Uint8Array(await blob.arrayBuffer());

// ---------------------------------------------------------------------------
// Key derivation and wrap format
// ---------------------------------------------------------------------------

test("crypto: encrypt/decrypt round trip with a derived app key", async () => {
  const kek = await C.deriveAppDataKey(seeds[ALICE], { appId: APP, viewerDid: ALICE });
  const plain = randomBytes(70_000);
  const up = await C.encryptForViewer(kek, new Blob([plain]));
  assert.equal(up.size, plain.length + 16, "GCM adds a 16-byte tag");
  assert.equal(up.contentHash, createHash("sha256").update(await bytesOf(up.ciphertext)).digest("hex"));
  assert.notDeepEqual(await bytesOf(up.ciphertext), new Uint8Array(plain).subarray(0, up.size));
  const back = await C.decryptForViewer(kek, up.ciphertext, up.ivB64, up.wrappedKey);
  assert.deepEqual(await bytesOf(back), new Uint8Array(plain));

  // The same seed on another device derives the same key.
  const again = await C.deriveAppDataKey(new Uint8Array(seeds[ALICE]), { appId: APP.toUpperCase(), viewerDid: ALICE });
  assert.deepEqual(await bytesOf(await C.decryptForViewer(again, up.ciphertext, up.ivB64, up.wrappedKey)), new Uint8Array(plain));
});

test("crypto: wire format is HKDF-SHA256 -> AES-KW over an AES-256-GCM file key", async () => {
  const kek = await C.deriveAppDataKey(seeds[ALICE], { appId: APP, viewerDid: ALICE });
  const plain = Buffer.from("interoperable with any AES-KW implementation");
  const up = await C.encryptForViewer(kek, plain);

  assert.match(up.wrappedKey, /^skw1\./);
  const wrapped = Buffer.from(up.wrappedKey.slice(5), "base64");
  assert.equal(wrapped.length, 40, "AES-KW of a 32-byte key is 40 bytes");
  const iv = Buffer.from(up.ivB64, "base64");
  assert.equal(iv.length, 12);

  // Recompute everything with node:crypto, independently of Web Crypto.
  const kekRaw = Buffer.from(
    hkdfSync("sha256", seeds[ALICE], C.APP_DATA_KEK_SALT, `app:${APP}|did:${ALICE}`, 32),
  );
  const unwrap = createDecipheriv("id-aes256-wrap", kekRaw, Buffer.from("A6A6A6A6A6A6A6A6", "hex"));
  const fileKey = Buffer.concat([unwrap.update(wrapped), unwrap.final()]);
  assert.equal(fileKey.length, 32);
  const ct = Buffer.from(await bytesOf(up.ciphertext));
  const gcm = createDecipheriv("aes-256-gcm", fileKey, iv);
  gcm.setAuthTag(ct.subarray(ct.length - 16));
  const out = Buffer.concat([gcm.update(ct.subarray(0, ct.length - 16)), gcm.final()]);
  assert.equal(out.toString(), plain.toString());
});

test("crypto: another account, another app, or tampering cannot decrypt", async () => {
  const alice = await C.deriveAppDataKey(seeds[ALICE], { appId: APP, viewerDid: ALICE });
  const bob = await C.deriveAppDataKey(seeds[BOB], { appId: APP, viewerDid: BOB });
  const otherApp = await C.deriveAppDataKey(seeds[ALICE], { appId: "cd".repeat(32), viewerDid: ALICE });
  const up = await C.encryptForViewer(alice, new Uint8Array([1, 2, 3]));

  await assert.rejects(C.decryptForViewer(bob, up.ciphertext, up.ivB64, up.wrappedKey), /different account or app/);
  await assert.rejects(C.decryptForViewer(otherApp, up.ciphertext, up.ivB64, up.wrappedKey), /different account or app/);
  const ct = await bytesOf(up.ciphertext);
  ct[0] ^= 1;
  await assert.rejects(C.decryptForViewer(alice, ct, up.ivB64, up.wrappedKey), /Decryption failed/);
  // The desktop's old mock format (the raw key as "wrappedKey") is refused.
  await assert.rejects(C.decryptForViewer(alice, up.ciphertext, up.ivB64, Buffer.alloc(32).toString("base64")), /Unsupported key format/);
  await assert.rejects(C.deriveAppDataKey(new Uint8Array(8), { appId: APP, viewerDid: ALICE }), /at least 16 bytes/);
});

test("crypto: the derived key cannot be exported", async () => {
  const kek = await C.deriveAppDataKey(seeds[ALICE], { appId: APP, viewerDid: ALICE });
  assert.equal(kek.extractable, false);
  await assert.rejects(globalThis.crypto.subtle.exportKey("raw", kek));
});

// ---------------------------------------------------------------------------
// Blob store
// ---------------------------------------------------------------------------

test("blobs: chunked round trip, content address, integrity check, delete", async () => {
  const node = fakeNode();
  const auth = node.mint(ALICE, APP);
  const store = new ViewerDataStore({
    storageOrigin: "https://storage.test/",
    collectionPrefix: `app_${APP}_`,
    authorization: async () => auth,
    fetchImpl: node.fetchImpl,
    chunkBytes: 1000,
  });
  const data = randomBytes(4500);
  const { cid, size } = await store.putBlob(new Blob([data]));
  assert.equal(size, 4500);
  assert.equal(cid, `skb1-${createHash("sha256").update(data).digest("hex")}`);
  const keys = [...node.docs.keys()];
  assert.equal(keys.filter((k) => k.includes("|app_" + APP + "_sk-chunk|")).length, 5);
  assert.ok(keys.every((k) => k.startsWith(`${ALICE}|`)), "stored under the viewer's DID");

  assert.deepEqual(await bytesOf(await store.getBlob(cid)), new Uint8Array(data));

  // A second put of the same bytes writes nothing new.
  const before = node.bodies.length;
  await store.putBlob(data);
  assert.equal(node.bodies.length, before);

  const chunkKey = `${ALICE}|app_${APP}_sk-chunk|${cid}.2`;
  const chunk = node.docs.get(chunkKey);
  node.docs.set(chunkKey, { b64: Buffer.from(Buffer.alloc(1000, 7)).toString("base64") });
  await assert.rejects(store.getBlob(cid), /integrity/);
  node.docs.set(chunkKey, chunk);

  assert.equal(await store.deleteBlob(cid), true);
  assert.equal(node.docs.size, 0);
  await assert.rejects(store.getBlob(cid), /not found/);
  await assert.rejects(store.getBlob("../../etc"), /Not a SpaceKit blob id/);
});

test("blobs: retries a 429 and surfaces other errors", async () => {
  const node = fakeNode();
  const auth = node.mint(ALICE, APP);
  let calls = 0;
  const flaky = async (url, init) => {
    calls++;
    if (calls === 1) return new Response("{}", { status: 429, headers: { "retry-after": "1" } });
    return node.fetchImpl(url, init);
  };
  const delays = [];
  const store = new ViewerDataStore({
    storageOrigin: "https://storage.test",
    collectionPrefix: `app_${APP}_`,
    authorization: async () => auth,
    fetchImpl: flaky,
    retryDelayMs: (attempt, retryAfter) => (delays.push([attempt, retryAfter]), 0),
  });
  await store.putRecord("a", 1);
  assert.deepEqual(delays, [[1, 1]]);
  const denied = new ViewerDataStore({
    storageOrigin: "https://storage.test",
    collectionPrefix: `app_${"cd".repeat(32)}_`,
    authorization: async () => auth,
    fetchImpl: node.fetchImpl,
  });
  await assert.rejects(denied.putRecord("a", 1), /\(403\): out of scope/);
});

// ---------------------------------------------------------------------------
// Bridge: viewer vs publisher namespaces
// ---------------------------------------------------------------------------

{
  const node = fakeNode();
  test(
    "bridge: a 3 MiB encrypted upload lands under the viewer's DID in sub-1 MiB requests and reads back on another device",
    withFetch(node, async () => {
      const viewer = { did: ALICE };
      const bridge = makeBridge(node, viewer, { permissions: [{ Storage: { max_bytes: null } }] });
      await bridge.ensureHydrated();

      const plain = randomBytes(3 * MIB + 123);
      const enc = await bridge.handle("crypto", "encryptUpload", { blob: new Blob([plain]) });
      const { cid, size } = await bridge.handle("storage", "putBlob", { blob: enc.ciphertext });
      assert.equal(size, enc.size);
      await bridge.handle("storage", "putRecord", {
        key: "file:1",
        value: { id: "1", name: "big.bin", cid, iv: enc.ivB64, wrappedKey: enc.wrappedKey },
      });

      assert.ok(node.bodies.every((n) => n < MIB), "every request fits the node's JSON limit");
      assert.ok(node.bodies.length >= 5);
      const owners = new Set([...node.docs.keys()].map((k) => k.split("|")[0]));
      assert.deepEqual([...owners], [ALICE]);
      // Nothing on the node contains the plaintext.
      const needle = Buffer.from(plain.subarray(1000, 1064)).toString("base64").slice(4, 40);
      for (const data of node.docs.values()) assert.ok(!JSON.stringify(data).includes(needle));

      // "Another device": a fresh bridge, empty localStorage, same recovery key.
      globalThis.localStorage = memoryLocalStorage();
      const other = makeBridge(node, { did: ALICE }, { permissions: ["storage"] });
      await other.ensureHydrated();
      assert.deepEqual(await other.handle("storage", "listRecords", { prefix: "file:" }), ["file:1"]);
      const entry = await other.handle("storage", "getRecord", { key: "file:1" });
      const { plaintext } = await other.handle("crypto", "decryptBlob", {
        cid: entry.cid,
        iv: entry.iv,
        wrappedKey: entry.wrappedKey,
      });
      assert.deepEqual(await bytesOf(plaintext), new Uint8Array(plain));

      // Bob sees none of it and cannot decrypt it.
      const bob = makeBridge(node, { did: BOB }, { permissions: ["storage"] });
      await bob.ensureHydrated();
      assert.deepEqual(await bob.handle("storage", "listRecords", { prefix: "" }), []);
      await assert.rejects(bob.handle("storage", "getBlob", { cid }), /not found/);
      await assert.rejects(
        bob.handle("crypto", "decryptBlob", { ciphertext: enc.ciphertext, iv: enc.ivB64, wrappedKey: enc.wrappedKey }),
        /different account/,
      );

      // Delete removes the blob from the node.
      assert.equal(await bridge.handle("storage", "deleteBlob", { cid }), true);
      await bridge.handle("storage", "deleteRecord", { key: "file:1" });
      assert.equal(node.docs.size, 0);
    }),
  );
}

{
  const node = fakeNode();
  test(
    "bridge: documents stay in the publisher's namespace; storage records without the permission stay local",
    withFetch(node, async () => {
      const viewer = { did: ALICE };
      const bridge = makeBridge(node, viewer);
      await bridge.ensureHydrated();

      await bridge.handle("documents", "put", { collection: "board", id: "x", data: { hi: 1 } });
      assert.ok(node.docs.has(`${PUBLISHER}|app_${APP}_board|x`), "documents.* is publisher-owned");
      await assert.rejects(bridge.handle("documents", "list", { collection: "board" }), /Owner access denied/);
      await assert.rejects(bridge.handle("documents", "put", { collection: "sk-kv", id: "x", data: {} }), /reserved/);

      // No `storage` permission: records keep the old per-device behaviour.
      await bridge.handle("storage", "putRecord", { key: "score", value: 42 });
      assert.equal(await bridge.handle("storage", "getRecord", { key: "score" }), 42);
      assert.equal(node.docs.size, 1, "nothing new on the node");
      assert.equal((await bridge.handle("storage", "ready", {})).synced, false);

      // Blobs and crypto still use the viewer's namespace.
      const { cid } = await bridge.handle("storage", "putBlob", { blob: new Blob(["abc"]) });
      assert.ok([...node.docs.keys()].some((k) => k.startsWith(`${ALICE}|app_${APP}_sk-blob|${cid}`)));
    }),
  );
}

{
  const node = fakeNode();
  test(
    "bridge: the localStorage shim stays on the device; signed-out viewers get clear errors",
    withFetch(node, async () => {
      const viewer = { did: ALICE };
      const bridge = makeBridge(node, viewer, { permissions: ["storage"] });
      await bridge.ensureHydrated();
      await bridge.handle("storage", "set", { key: "__ls:theme", value: "dark" });
      await bridge.handle("storage", "set", { key: "notes", value: "synced" });
      assert.equal([...node.docs.keys()].length, 1);
      assert.deepEqual((await bridge.handle("storage", "list", { prefix: "" })).sort(), ["__ls:theme", "notes"]);
      assert.deepEqual(await bridge.handle("storage", "list", { prefix: "__ls:" }), ["__ls:theme"]);

      viewer.did = null;
      await assert.rejects(bridge.handle("storage", "putBlob", { blob: new Blob(["x"]) }), /Sign in to store files/);
      await assert.rejects(bridge.handle("crypto", "encryptUpload", { blob: new Blob(["x"]) }), /Sign in/);
      // Signed out: records fall back to this device and the node data is not visible.
      assert.equal(await bridge.handle("storage", "getRecord", { key: "notes" }), null);

      // Switching accounts never serves the previous account's cached records.
      viewer.did = BOB;
      await bridge.ensureHydrated();
      assert.equal(await bridge.handle("storage", "getRecord", { key: "notes" }), null);
    }),
  );
}
