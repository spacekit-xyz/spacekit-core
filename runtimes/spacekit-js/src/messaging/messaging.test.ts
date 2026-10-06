import { test } from "node:test";
import assert from "node:assert/strict";

import { MessagingClient, SseParser, type GroupInfo, type MessagingEvent } from "./client.js";
import {
  ChannelKeyring,
  kyberKeyWrapper,
  membershipChange,
  openSealedKeyring,
  runChannelKeyManager,
  sealKeyring,
  type KeyWrapper,
} from "./channel_keys.js";
import { createPaidChannel, subscribeToChannel } from "./channels.js";
import { EntitlementOp } from "../entitlement.js";

/**
 * In-memory stand-in for the messaging nodes of several users: the HTTP
 * gateway routes, with group replication and delivery collapsed into one
 * process. Joins to paid channels need an entitlement the test approves.
 */
class FakeGateway {
  groups = new Map<string, GroupInfo>();
  keys = new Map<string, string>();
  validEntitlements = new Set<string>();
  private streams: { did: string; push: (s: string) => void }[] = [];
  private seq = 0;

  emit(event: Record<string, unknown> & { participants: string[] }) {
    const line = `data: ${JSON.stringify(event)}\n\n`;
    for (const s of this.streams) if (event.participants.includes(s.did)) s.push(line);
  }

  private groupEvent(group: GroupInfo, before: string[] = []) {
    this.emit({
      type: "group",
      group_id: group.id,
      group,
      participants: [...new Set([...group.member_dids, ...before, group.creator_did])],
      created_at: "now",
    });
  }

  private update(group: GroupInfo, members: string[]) {
    const before = group.member_dids;
    const next = { ...group, member_dids: members, version: group.version + 1 };
    this.groups.set(group.id, next);
    this.groupEvent(next, before);
    return next;
  }

  removeMember(groupId: string, did: string) {
    const g = this.groups.get(groupId)!;
    return this.update(g, g.member_dids.filter((m) => m !== did));
  }

  fetch: typeof fetch = async (input, init) => {
    const url = new URL(String(input));
    const method = init?.method ?? "GET";
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    const json = (value: unknown, status = 200) =>
      new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });
    const path = url.pathname;

    if (path === "/api/messages/stream") {
      const did = url.searchParams.get("did")!;
      let entry: { did: string; push: (s: string) => void };
      const stream = new ReadableStream<Uint8Array>({
        start: (controller) => {
          entry = {
            did,
            push: (s) => controller.enqueue(new TextEncoder().encode(s)),
          };
          this.streams.push(entry);
          controller.enqueue(new TextEncoder().encode(": keep-alive\n\n"));
          init?.signal?.addEventListener("abort", () => {
            this.streams = this.streams.filter((x) => x !== entry);
            try {
              controller.close();
            } catch {
              /* already closed */
            }
          });
        },
      });
      return new Response(stream, { status: 200, headers: { "content-type": "text/event-stream" } });
    }
    if (path === "/api/messages/envelope" && method === "POST") {
      const sender = body.message.context.did as string;
      const group = body.group_id ? this.groups.get(body.group_id) : undefined;
      if (group && !group.member_dids.includes(sender)) return json({ error: "forbidden" }, 403);
      const participants = group ? group.member_dids : [...(body.recipient_dids ?? []), sender];
      const content =
        body.message.kind === "chat" ? body.message.payload : JSON.stringify(body.message.payload);
      const id = `m${++this.seq}`;
      this.emit({
        type: "message",
        message_id: id,
        conversation_id: id,
        conversation_type: group ? "group" : "direct",
        group_id: body.group_id ?? null,
        sender: { did: sender },
        content,
        created_at: "now",
        participants,
      });
      return json({ status: "ok", conversation_id: id, created_at: "now", message_id: id });
    }
    if (path === "/api/messages/register-key") {
      this.keys.set(body.did, body.publicKey);
      return json({ status: "ok" });
    }
    const key = path.match(/^\/api\/messages\/keys\/(.+)$/);
    if (key) {
      const pk = this.keys.get(decodeURIComponent(key[1]));
      return pk ? json({ did: key[1], publicKey: pk, algorithm: "kyber1024" }) : json({}, 404);
    }
    if (path === "/api/messages/groups" && method === "POST") {
      const id = `grp:${++this.seq}`;
      const group: GroupInfo = {
        id,
        name: body.name,
        creator_did: body.creator_did,
        description: body.description,
        visibility: body.visibility,
        member_dids: [body.creator_did, ...body.member_dids],
        created_at: "now",
        listing_id: body.listing_id,
        version: 1,
      };
      this.groups.set(id, group);
      this.groupEvent(group);
      return json(group);
    }
    if (path === "/api/messages/groups" && method === "GET") {
      return json({ groups: [...this.groups.values()].filter((g) => !g.deleted) });
    }
    const m = path.match(/^\/api\/messages\/groups\/([^/]+)(?:\/(\w+))?$/);
    if (m) {
      const group = this.groups.get(decodeURIComponent(m[1]));
      if (!group) return json({ error: "not found" }, 404);
      switch (m[2]) {
        case undefined:
          return json(group);
        case "join": {
          if (group.visibility === "paid" && !this.validEntitlements.has(body.entitlement_id)) {
            return json({ error: "entitlement is not valid" }, 403);
          }
          // Joins to someone else's group are decided by their node later.
          setTimeout(() => this.update(group, [...group.member_dids, body.did]), 5);
          return json({ status: "pending", group_id: group.id });
        }
        case "delete":
          this.groups.set(group.id, { ...group, deleted: true, member_dids: [] });
          return json({ status: "deleted" });
      }
    }
    return json({ error: `no route ${method} ${path}` }, 404);
  };
}

/** Test KEM: "wraps" a key by XOR with a per-DID pad. Not cryptography. */
function testWrapper(myPk: string): KeyWrapper {
  const pad = (pk: string) => new TextEncoder().encode(pk.padEnd(32, "#").slice(0, 32));
  const xor = (a: Uint8Array, b: Uint8Array) => a.map((x, i) => x ^ b[i]);
  return {
    async wrap(publicKeyHex, key) {
      return { to: publicKeyHex, k: Array.from(xor(key, pad(publicKeyHex))) };
    },
    async unwrap(wrapped) {
      const w = wrapped as { to: string; k: number[] };
      assert.equal(w.to, myPk, "key wrapped for someone else");
      return xor(Uint8Array.from(w.k), pad(myPk));
    },
  };
}

function until<T>(check: () => T | undefined, ms = 2000): Promise<T> {
  return new Promise((resolve, reject) => {
    const start = Date.now();
    const tick = () => {
      const v = check();
      if (v !== undefined) return resolve(v);
      if (Date.now() - start > ms) return reject(new Error("timed out"));
      setTimeout(tick, 5);
    };
    tick();
  });
}

test("SSE parser handles split chunks, comments, multi-line data and CRLF", () => {
  const seen: string[] = [];
  const p = new SseParser((d) => seen.push(d));
  p.push(": keep-alive\n\ndata: {\"a\"");
  p.push(":1}\n\ndata: line1\r\ndata: line2\r\n\r\n");
  p.push("data:x\n");
  assert.deepEqual(seen, ['{"a":1}', "line1\nline2"]);
  p.push("\n");
  assert.deepEqual(seen.at(-1), "x");
});

test("keyring seals per epoch, rejects other groups, survives export", async () => {
  const ring = new ChannelKeyring();
  const wrapper = testWrapper("pk-owner");
  const dist = await ring.rotate("grp:1", [{ did: "did:b", publicKeyHex: "pk-b" }], wrapper);
  assert.equal(dist.epoch, 1);
  const sealed = ring.seal("grp:1", "hello");
  assert.ok(ChannelKeyring.isSealed(sealed));
  assert.equal(ring.open("grp:1", sealed), "hello");
  assert.throws(() => ring.open("grp:2", sealed));

  const member = new ChannelKeyring();
  assert.ok(await member.accept(dist, "did:b", testWrapper("pk-b")));
  assert.equal(member.open("grp:1", sealed), "hello");

  await ring.rotate("grp:1", [], wrapper);
  const newer = ring.seal("grp:1", "after");
  assert.throws(() => member.open("grp:1", newer), /no key/);
  const restored = ChannelKeyring.import(ring.export());
  assert.equal(restored.open("grp:1", newer), "after");
  assert.equal(restored.open("grp:1", sealed), "hello");

  assert.deepEqual(membershipChange(["a", "b"], ["b", "c"]), { added: ["c"], removed: ["a"] });
});

test("client sends, streams and joins; channel keys rotate when a member leaves", async () => {
  const gw = new FakeGateway();
  const owner = new MessagingClient({ baseUrl: "http://node-a", did: "did:owner", fetch: gw.fetch });
  const luna = new MessagingClient({ baseUrl: "http://node-b", did: "did:luna", fetch: gw.fetch });
  const kai = new MessagingClient({ baseUrl: "http://node-c", did: "did:kai", fetch: gw.fetch });
  await luna.registerKey("pk-luna");
  await kai.registerKey("pk-kai");

  const ownerRing = new ChannelKeyring();
  const lunaRing = new ChannelKeyring();
  const kaiRing = new ChannelKeyring();
  const stops = [
    runChannelKeyManager({ client: owner, keyring: ownerRing, wrapper: testWrapper("pk-owner") }),
    runChannelKeyManager({ client: luna, keyring: lunaRing, wrapper: testWrapper("pk-luna") }),
    runChannelKeyManager({ client: kai, keyring: kaiRing, wrapper: testWrapper("pk-kai") }),
  ];
  const lunaInbox: MessagingEvent[] = [];
  stops.push(luna.subscribe((e) => lunaInbox.push(e)));
  await new Promise((r) => setTimeout(r, 20));

  const group = await owner.createGroup({ name: "Garden club" });
  await until(() => (ownerRing.hasKey(group.id) ? true : undefined));

  // Two members join (asynchronously admitted by the owner's node).
  for (const c of [luna, kai]) {
    const res = await c.joinGroup(group.id);
    assert.equal(res.status, "pending");
    const g = await c.waitForMembership(group.id, 2000);
    assert.ok(g.member_dids.includes(c.did));
  }
  await until(() => (lunaRing.hasKey(group.id) && kaiRing.hasKey(group.id) ? true : undefined));
  const epoch1 = ownerRing.currentEpoch(group.id)!;
  assert.equal(lunaRing.currentEpoch(group.id), epoch1);

  await owner.sendChat(ownerRing.seal(group.id, "welcome"), { groupId: group.id });
  const welcome = await until(() =>
    lunaInbox.find((e) => e.type === "message" && ChannelKeyring.isSealed(e.content)),
  );
  assert.equal(welcome.type === "message" && lunaRing.open(group.id, welcome.content), "welcome");

  // Kai leaves: the owner rotates, Luna follows, Kai cannot read new posts.
  gw.removeMember(group.id, "did:kai");
  await until(() => ((ownerRing.currentEpoch(group.id) ?? 0) > epoch1 ? true : undefined));
  const epoch2 = ownerRing.currentEpoch(group.id)!;
  await until(() => (lunaRing.currentEpoch(group.id) === epoch2 ? true : undefined));
  const secret = ownerRing.seal(group.id, "members only");
  assert.equal(lunaRing.open(group.id, secret), "members only");
  assert.equal(kaiRing.currentEpoch(group.id), epoch1);
  assert.throws(() => kaiRing.open(group.id, secret));

  for (const stop of stops) stop();
});

test("paid channel: listing bound to the group, purchase, admission", async () => {
  const gw = new FakeGateway();
  const publisher = new MessagingClient({ baseUrl: "http://a", did: "did:pub", fetch: gw.fetch });
  const fan = new MessagingClient({ baseUrl: "http://b", did: "did:fan", fetch: gw.fetch });
  const calls: { op: number; input: Uint8Array; value: bigint }[] = [];
  const entitlement = new Uint8Array(32).fill(7);
  const submit = async (input: Uint8Array, value: bigint) => {
    calls.push({ op: input[0], input, value });
    if (input[0] === EntitlementOp.PURCHASE) return Uint8Array.from([1, ...entitlement]);
    return Uint8Array.from([1]);
  };

  const { group, listingId } = await createPaidChannel({
    client: publisher,
    submitContractCall: submit,
    name: "Studio notes",
    price: 5n,
  });
  assert.equal(group.visibility, "paid");
  assert.equal(group.listing_id, listingId);
  const listing = calls[0].input;
  assert.equal(listing[0], EntitlementOp.CREATE_LISTING);
  // The listing's file id is the group id.
  const text = new TextDecoder().decode(listing);
  assert.ok(text.includes(listingId) && text.includes(group.id));

  // Without a valid entitlement the node refuses the join.
  const pk = "ab".repeat(32);
  await assert.rejects(
    subscribeToChannel({ client: fan, submitContractCall: submit, group, price: 5n, publicKeyHex: pk, waitMs: 500 }),
    /403/,
  );
  gw.validEntitlements.add(Buffer.from(entitlement).toString("hex"));
  const sub = await subscribeToChannel({
    client: fan,
    submitContractCall: submit,
    group,
    price: 5n,
    publicKeyHex: pk,
    waitMs: 2000,
  });
  assert.equal(sub.entitlementId, "07".repeat(32));
  assert.ok(sub.group?.member_dids.includes("did:fan"));
  const purchase = calls.find((c) => c.op === EntitlementOp.PURCHASE)!;
  assert.equal(purchase.value, 5n);
});

test("kyber wrapper speaks kyber_wasm's API; keyring seals at rest", async () => {
  // A stand-in with kyber_wasm's shapes (base64 in, base64/JSON out).
  const fake = {
    kyber_encrypt: (alg: string, pk: string, pt: string) => ({ kemCiphertextBase64: pk, nonceBase64: "bm9uY2U=", ciphertextBase64: pt, algorithm: alg }),
    kyber_decrypt: (_alg: string, sk: string, kem: string, _n: string, ct: string) => (sk === kem ? ct : null),
  };
  const pkHex = "ab".repeat(16);
  const skB64 = Buffer.from(pkHex, "hex").toString("base64");
  const w = kyberKeyWrapper(fake, { secretKeyBase64: skB64 });
  const key = new Uint8Array(32).fill(7);
  assert.deepEqual(await w.unwrap(await w.wrap(pkHex, key)), key);
  const other = kyberKeyWrapper(fake, { secretKeyBase64: "AAAA" });
  await assert.rejects(other.unwrap(await w.wrap(pkHex, key)));

  const ring = new ChannelKeyring();
  await ring.rotate("grp:9", [], w);
  const storageKey = new Uint8Array(32).fill(3);
  const sealed = sealKeyring(ring, storageKey);
  assert.ok(!sealed.includes("grp:9"), "group ids and keys are not readable at rest");
  const back = openSealedKeyring(sealed, storageKey);
  assert.equal(back.open("grp:9", ring.seal("grp:9", "x")), "x");
  assert.throws(() => openSealedKeyring(sealed, new Uint8Array(32)));
});
