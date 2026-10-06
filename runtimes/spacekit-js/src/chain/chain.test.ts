import { test } from "node:test";
import assert from "node:assert/strict";
import { secp256k1 } from "@noble/curves/secp256k1";
import { sha256 } from "@noble/hashes/sha2";

import {
  ChainAccount,
  ChainClient,
  addressOfDid,
  formatAstra,
  parseAstra,
  transactionSigningPayload,
  WEI_PER_ASTRA,
} from "./client.js";
import { guardLocalCurrency, LocalCurrencyError } from "./currency_guard.js";
import { createInMemoryStorage } from "../storage.js";
import { SpacekitVm } from "../vm/spacekitvm.js";

test("accounts derive Ethereum-style addresses and sign the node's v2 payload", () => {
  const account = ChainAccount.fromPrivateKey("0".repeat(63) + "1");
  assert.equal(account.address, "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf");
  assert.equal(account.did, "did:spacekit:7e5f4552091a69125d5dfcb7b8c2659029395bdf");
  assert.equal(addressOfDid(account.did), account.address);

  const tx = {
    from: account.address,
    to: "0x00000000000000000000000000000000000000AA",
    data: Uint8Array.from([1, 2]),
    value: 5n * WEI_PER_ASTRA,
    nonce: 3n,
    gasLimit: 21_000n,
    gasPrice: 1n,
  };
  const payload = transactionSigningPayload(1337n, tx);
  assert.equal(
    payload,
    "SPACEKIT-TX-v2\n1337\n7e5f4552091a69125d5dfcb7b8c2659029395bdf\n" +
      "00000000000000000000000000000000000000aa\n5000000000000000000\n3\n21000\n1\n0102",
  );
  const sig = account.sign(1337n, tx);
  const hash = sha256(new TextEncoder().encode(payload));
  const recovered = new secp256k1.Signature(BigInt("0x" + sig.r_hex), BigInt("0x" + sig.s_hex))
    .addRecoveryBit(sig.v - 27)
    .recoverPublicKey(hash)
    .toRawBytes(false);
  assert.deepEqual(recovered, secp256k1.getPublicKey(account.privateKeyHex(), false));
  assert.throws(() => account.sign(1337n, { ...tx, from: "0x" + "11".repeat(20) }));
});

test("ASTRA amounts are exact in wei", () => {
  assert.equal(parseAstra("1.5"), 1_500_000_000_000_000_000n);
  assert.equal(parseAstra("0.000000000000000001"), 1n);
  assert.equal(formatAstra(1_500_000_000_000_000_000n), "1.5");
  assert.equal(formatAstra(2n * WEI_PER_ASTRA), "2");
  assert.throws(() => parseAstra("1.0000000000000000001"));
});

test("client: balance as exact bigint, sign + submit + receipt, failures throw", async () => {
  const account = ChainAccount.generate();
  const seen: { path: string; body?: any }[] = [];
  const fakeFetch: typeof fetch = async (input, init) => {
    const url = new URL(String(input));
    const body = init?.body && typeof init.body === "string" ? JSON.parse(init.body) : undefined;
    seen.push({ path: url.pathname, body });
    const json = (v: unknown, status = 200) => new Response(JSON.stringify(v), { status });
    if (url.pathname === "/rpc") return json({ jsonrpc: "2.0", id: 1, result: "0x539" });
    if (url.pathname.startsWith("/v1/balance/"))
      return json({ address: account.address, balance_wei: "123456789012345678901234567", locked_wei: "0", nonce: 4 });
    if (url.pathname === "/transaction") return json({ tx_hash: body.value === "1" ? "0xbad" : "0xabc" }, 202);
    if (url.pathname === "/receipt/abc") return json({ tx_hash: "abc", success: true, return_data: [1, 2], gas_used: 1, block_number: 1 });
    if (url.pathname === "/receipt/bad")
      return json({ tx_hash: "bad", success: false, return_data: [...new TextEncoder().encode("nope")], gas_used: 1, block_number: 1 });
    return json({}, 404);
  };
  const client = new ChainClient({ url: "http://node", fetch: fakeFetch, pollMs: 1 });
  const bal = await client.getBalance(account.did);
  assert.equal(bal.balanceWei, 123456789012345678901234567n);
  assert.equal(bal.nonce, 4n);

  const receipt = await client.transfer(account, "0x" + "22".repeat(20), 7n);
  assert.deepEqual(receipt.return_data, [1, 2]);
  const sent = seen.find((s) => s.path === "/transaction")!.body;
  assert.equal(sent.nonce, 4);
  assert.equal(sent.value, "7");
  assert.equal(sent.gas_limit, "21000");
  assert.equal(sent.data_hex, "");
  await assert.rejects(client.transfer(account, "0x" + "22".repeat(20), 1n), /failed: nope/);
});

test("chain mode: the VM holds no currency and refuses value", async () => {
  const storage = createInMemoryStorage();
  const enc = new TextEncoder();
  // A browser profile from before keeps a local balance; it is ignored.
  storage.set(enc.encode("native:astra:balance:did:x"), new Uint8Array(8).fill(1));
  const guarded = guardLocalCurrency(storage);
  assert.equal(guarded.get(enc.encode("native:astra:balance:did:x")), undefined);
  assert.throws(() => guarded.set(enc.encode("native:astra:balance:did:x"), new Uint8Array(8)), LocalCurrencyError);
  assert.throws(() => guarded.set(enc.encode("astra:erc20:balance:did:x"), new Uint8Array(8)), LocalCurrencyError);
  guarded.set(enc.encode("app:state"), Uint8Array.from([1]));
  assert.deepEqual(guarded.get(enc.encode("app:state")), Uint8Array.from([1]));
  assert.ok(guarded.entries!().every((e) => !new TextDecoder().decode(e.key).startsWith("native:")));

  const vm = new SpacekitVm({ storage: createInMemoryStorage(), devMode: true, quantumVerkle: { enabled: false } as any });
  assert.equal(vm.currency, "chain");
  await assert.rejects(vm.submitTransaction("c", new Uint8Array(), "did:x", 1n), LocalCurrencyError);
  await assert.rejects(vm.executeTransaction("c", new Uint8Array(), "did:x", 1n), LocalCurrencyError);
  assert.equal(vm.exportStateSnapshot().entries.some((e) => Buffer.from(e.keyHex, "hex").toString().startsWith("native:")), false);

  const dev = new SpacekitVm({ storage: createInMemoryStorage(), devMode: true, currency: "local-dev", quantumVerkle: { enabled: false } as any });
  assert.equal(dev.exportStateSnapshot().entries.some((e) => Buffer.from(e.keyHex, "hex").toString().startsWith("native:")), true);
});
