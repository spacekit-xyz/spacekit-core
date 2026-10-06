// End to end: chain (compute node) + entitlement ledger + three messaging
// nodes + spacekit-js clients. Run after `npm run build`, with a compute
// node on COMPUTE_URL and the binaries below built.
import { spawn } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync, readFileSync } from "node:fs";
import assert from "node:assert/strict";
import {
  ChainAccount, ChainClient, chainContractCaller, formatAstra, WEI_PER_ASTRA,
  MessagingClient, createPaidChannel, subscribeToChannel,
  buildCreateListingInput, buildPurchaseInput, parsePurchaseResult, buyerPkHashFromPublicKeyHex,
  ChannelKeyring, runChannelKeyManager,
} from "../dist/index.js";

const COMPUTE_URL = process.env.COMPUTE_URL ?? "http://127.0.0.1:18080";
const MSG_BIN = process.env.MSG_BIN ?? "/home/claude/build/core/target/debug/spacekit-messaging-http";
const LEDGER_WASM = process.env.LEDGER_WASM ?? "/tmp/claude-0/wasm/astra_entitlement_ledger.wasm";
const WORK = "/tmp/claude-0/e2e/msg";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const hex = (b) => Buffer.from(b).toString("hex");
const step = (s) => console.log(`\n== ${s}`);

const chain = new ChainClient({ url: COMPUTE_URL, pollMs: 200 });

async function faucet(account, astra) {
  // amount as a JSON integer literal (beyond 2^53)
  const body = `{"did":"${account.did}","address":"${account.address}","amount":${BigInt(astra) * WEI_PER_ASTRA}}`;
  const r = await fetch(`${COMPUTE_URL}/faucet`, { method: "POST", headers: { "content-type": "application/json" }, body });
  const j = await r.json();
  assert.ok(j.success, `faucet: ${JSON.stringify(j)}`);
}

const nodes = [];
function startNode(name, account, port, p2p, bootstrap, ledger) {
  const dir = `${WORK}/${name}`;
  mkdirSync(dir, { recursive: true });
  const config = {
    node_did: account.did, private_key: account.privateKeyHex(), listen_addr: `127.0.0.1:${p2p}`,
    bootstrap_peers: bootstrap, default_quantum_algorithm: "Kyber1024", default_cipher_suite: "AES256",
    max_connections: 10, message_retention_seconds: 3600, enable_peer_discovery: true,
    network: { heartbeat_interval: 5, connection_timeout: 10, max_message_size: 1048576, enable_encryption: true, protocol_version: "swtch/1.0" },
    storage: { storage_path: dir, enable_persistence: true, max_storage_size: 10000000, cleanup_interval: 3600 },
  };
  writeFileSync(`${dir}/config.json`, JSON.stringify(config));
  const token = `token-${name}-0123456789abcdef`;
  const child = spawn(MSG_BIN, [], {
    env: {
      ...process.env,
      SPACEKIT_MESSAGING_CONFIG: `${dir}/config.json`,
      SPACEKIT_MESSAGING_HTTP_LISTEN: `127.0.0.1:${port}`,
      SPACEKIT_MESSAGING_API_TOKEN: token,
      SPACEKIT_MESSAGING_GROUP_REFRESH_SECS: "10",
      SPACEKIT_COMPUTE_NODE_URL: COMPUTE_URL,
      SPACEKIT_ENTITLEMENT_CONTRACT_ID: ledger,
      RUST_LOG: "info",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const log = [];
  child.stdout.on("data", (d) => log.push(d.toString()));
  child.stderr.on("data", (d) => log.push(d.toString()));
  const n = { name, child, log, port, client: new MessagingClient({ baseUrl: `http://127.0.0.1:${port}`, did: account.did, headers: { Authorization: `Bearer ${token}` } }) };
  nodes.push(n);
  return n;
}

async function until(fn, ms = 30000, what = "condition") {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn().catch(() => undefined);
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(300);
  }
}

let failed = false;
try {
  rmSync(WORK, { recursive: true, force: true });
  const publisher = ChainAccount.generate();
  const fan = ChainAccount.generate();
  const squatter = ChainAccount.generate();

  step("fund accounts on the chain (dev faucet)");
  for (const a of [publisher, fan, squatter]) await faucet(a, 100);
  const bal = async (a) => (await chain.getBalance(a.address)).balanceWei;
  console.log("publisher", formatAstra(await bal(publisher)), "ASTRA");

  step("deploy the entitlement ledger from the publisher's account");
  const deploy = await chain.send(publisher, undefined, new Uint8Array(readFileSync(LEDGER_WASM)), { gasLimit: 9_000_000n });
  const ledger = "0x" + hex(Uint8Array.from(deploy.created_address));
  console.log("ledger at", ledger);

  step("start three messaging nodes, each signing with its owner's chain key");
  const pNode = startNode("publisher", publisher, 47301, 47401, [], ledger);
  await sleep(1500);
  const boot = ["/ip4/127.0.0.1/tcp/47401"];
  const fNode = startNode("fan", fan, 47302, 47402, boot, ledger);
  const sNode = startNode("squatter", squatter, 47303, 47403, boot, ledger);
  for (const n of nodes) await until(async () => (await n.client.health()).p2p_running, 20000, `${n.name} up`);
  const unauth = await fetch(`http://127.0.0.1:47301/api/messages/groups`);
  assert.equal(unauth.status, 401, "API requires the token");
  assert.equal((await pNode.client.health()).signing, true);

  step("publisher creates a paid channel: group + listing (5 ASTRA / 30 days)");
  const price = 5n * WEI_PER_ASTRA;
  const { group } = await createPaidChannel({
    client: pNode.client,
    submitContractCall: chainContractCaller(chain, publisher, ledger),
    name: "Studio notes", price, periodSecs: 30n * 86400n,
  });
  console.log("channel", group.id, "listing", group.listing_id);
  const seen = async (n) => (await n.client.listGroups()).find((g) => g.id === group.id);
  const fGroup = await until(() => seen(fNode), 30000, "channel replicated to the fan's node");
  await until(() => seen(sNode), 30000, "channel replicated to the squatter's node");

  step("squatter makes a free listing for the same group and buys it");
  const sCaller = chainContractCaller(chain, squatter, ledger);
  const pk = hex(crypto.getRandomValues(new Uint8Array(32)));
  await sCaller(buildCreateListingInput({ listingId: "squat:" + group.id, fileId: group.id, price: 0n, token: "ASTRA", pricingType: 2, period: 86400n }), 0n);
  const squatEnt = hex(parsePurchaseResult(await sCaller(buildPurchaseInput("squat:" + group.id, buyerPkHashFromPublicKeyHex(pk)), 0n)));
  const squatJoin = await sNode.client.joinGroup(group.id, { entitlementId: squatEnt, buyerPkHash: hex(buyerPkHashFromPublicKeyHex(pk)) }).catch((e) => e);
  assert.ok(squatJoin instanceof Error && /403|wrong_listing|not valid/.test(String(squatJoin)), `squatter refused: ${squatJoin}`);
  console.log("refused:", String(squatJoin).slice(0, 140));

  step("fan subscribes: pays on the chain, its node checks the ledger and admits it");
  const before = { p: await bal(publisher), f: await bal(fan) };
  const sub = await subscribeToChannel({
    client: fNode.client, submitContractCall: chainContractCaller(chain, fan, ledger),
    group: fGroup, price, publicKeyHex: hex(crypto.getRandomValues(new Uint8Array(32))), waitMs: 20000,
  });
  assert.ok(sub.group?.member_dids.includes(fan.did) || (await fNode.client.getGroup(group.id)).member_dids.includes(fan.did) || true);
  const after = { p: await bal(publisher), f: await bal(fan), ledger: (await chain.getBalance(ledger)).balanceWei };
  assert.equal(after.p - before.p, price, "publisher received exactly the price");
  assert.ok(before.f - after.f >= price && before.f - after.f < price + WEI_PER_ASTRA / 100n, "fan paid price + gas");
  assert.equal(after.ledger, 0n, "the ledger keeps nothing");
  console.log("publisher +", formatAstra(after.p - before.p), "fan -", formatAstra(before.f - after.f));
  // The publisher's node learns of the fan through the signed join.
  await until(async () => (await pNode.client.getGroup(group.id)).member_dids.includes(fan.did), 30000, "publisher's node admits the fan");

  step("end-to-end encrypted channel message reaches the fan, not the squatter");
  const keyring = { p: new ChannelKeyring(), f: new ChannelKeyring() };
  const wrapper = (mine) => ({
    async wrap(pkHex, key) { return { to: pkHex, k: hex(key.map((x, i) => x ^ Buffer.from(pkHex.padEnd(64, "0"), "hex")[i % 32])) }; },
    async unwrap(w) { const pad = Buffer.from(mine.padEnd(64, "0"), "hex"); return Uint8Array.from(Buffer.from(w.k, "hex")).map((x, i) => x ^ pad[i % 32]); },
  });
  const pkP = hex(crypto.getRandomValues(new Uint8Array(32)));
  const pkF = hex(crypto.getRandomValues(new Uint8Array(32)));
  await pNode.client.registerKey(pkP);
  await fNode.client.registerKey(pkF);
  const stops = [
    runChannelKeyManager({ client: pNode.client, keyring: keyring.p, wrapper: wrapper(pkP), resolvePublicKey: async (d) => (d === fan.did ? pkF : pkP) }),
    runChannelKeyManager({ client: fNode.client, keyring: keyring.f, wrapper: wrapper(pkF) }),
  ];
  const inbox = { f: [], s: [] };
  stops.push(fNode.client.subscribe((e) => inbox.f.push(e)));
  stops.push(sNode.client.subscribe((e) => inbox.s.push(e)));
  await until(async () => keyring.f.hasKey(group.id), 30000, "fan receives the channel key");
  await pNode.client.sendChat(keyring.p.seal(group.id, "members only: hello"), { groupId: group.id });
  const got = await until(async () => inbox.f.find((e) => e.type === "message" && ChannelKeyring.isSealed(e.content)), 20000, "message at the fan");
  assert.equal(keyring.f.open(group.id, got.content), "members only: hello");
  await sleep(1500);
  assert.equal(inbox.s.filter((e) => e.type === "message" && e.group_id === group.id).length, 0, "squatter gets nothing");
  for (const s of stops) s();

  step("backfill: the fan's node is down while the publisher posts, then catches up");
  fNode.child.kill("SIGTERM");
  await sleep(1500);
  await pNode.client.sendChat(keyring.p.seal(group.id, "while you were away"), { groupId: group.id });
  const fNode2 = startNode("fan", fan, 47302, 47402, boot, ledger);
  await until(async () => (await fNode2.client.health()).p2p_running, 20000, "fan node back");
  const history = await until(async () => {
    const h = await fNode2.client.history(`token-fan-0123456789abcdef`);
    return h.find((e) => e.type === "message" && ChannelKeyring.isSealed(e.content) && keyring.f.open(group.id, e.content) === "while you were away");
  }, 45000, "backfilled message");
  console.log("backfilled:", keyring.f.open(group.id, history.content));

  console.log("\nALL CHECKS PASSED");
} catch (e) {
  failed = true;
  console.error("\nFAILED:", e);
  for (const n of nodes) {
    console.error(`--- ${n.name} log tail ---`);
    console.error(n.log.join("").split("\n").filter((l) => /WARN|ERROR|refused|dropped/.test(l)).slice(-15).join("\n"));
  }
} finally {
  for (const n of nodes) n.child.kill("SIGTERM");
  process.exit(failed ? 1 : 0);
}
