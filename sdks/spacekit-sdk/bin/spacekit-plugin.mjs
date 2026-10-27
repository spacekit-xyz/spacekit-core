#!/usr/bin/env node
/**
 * spacekit-plugin: create, check, hash and sign SpaceKit plugin descriptors.
 *
 *   spacekit-plugin init <ui|service|contract|brain> [--id acme.thing] [--publisher did:...] [--out spacekit-plugin.json]
 *   spacekit-plugin validate [spacekit-plugin.json]
 *   spacekit-plugin hash <role>=<file> [<role>=<file> ...]   # prints "files" entries
 *   spacekit-plugin sign [spacekit-plugin.json] --key <ed25519-seed-hex-file> [--out spacekit-plugin.sig.json]
 *
 * Spec: sdks/spacekit-apps/SPACEKIT-PLUGIN-SPEC.md
 */
import { createHash, createPrivateKey, createPublicKey, sign as edSign } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { basename } from "node:path";

const here = new URL("../dist/plugins/index.js", import.meta.url);
const plugins = await import(here.href).catch(() => {
  console.error("spacekit-plugin: build the SDK first (npm run build)");
  process.exit(2);
});

const [, , command, ...rest] = process.argv;

function flag(name, fallback) {
  const i = rest.indexOf(`--${name}`);
  if (i === -1) return fallback;
  const value = rest[i + 1];
  rest.splice(i, 2);
  return value;
}

const TEMPLATES = {
  ui: {
    ui: { entry: "index.html", placement: "panel" },
    uses: ["identity", "storage"],
    permissions: [],
    config: { title: { type: "string", label: "Title shown above the panel", default: "Chat", max_length: 60 } },
  },
  service: {
    service: { methods: { lookup: { description: "Look something up", params: { query: { type: "string", label: "Query", required: true } } } } },
    uses: ["storage"],
  },
  contract: {
    contract: { calls: { run: { description: "Run the contract", fee_wei: "0" } } },
    uses: ["compute"],
    files: [],
  },
  brain: {
    brain: {
      engine: "growformer",
      runs_on: ["browser"],
      abilities: ["converse"],
      training: { summary: "Describe what the brain was trained on", data_sources: [], includes_captured_conversations: false },
    },
    uses: ["ai"],
    files: [],
  },
};

function usage(code = 1) {
  console.error(readFileSync(new URL(import.meta.url)).toString().split("\n").slice(2, 10).join("\n").replace(/^ \* ?/gm, ""));
  process.exit(code);
}

function readDescriptor(path = "spacekit-plugin.json") {
  if (!existsSync(path)) {
    console.error(`spacekit-plugin: ${path} not found`);
    process.exit(2);
  }
  return readFileSync(path);
}

const ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function base58(bytes) {
  let n = BigInt(`0x${Buffer.from(bytes).toString("hex") || "0"}`);
  let out = "";
  while (n > 0n) {
    out = ALPHABET[Number(n % 58n)] + out;
    n /= 58n;
  }
  for (const b of bytes) {
    if (b !== 0) break;
    out = `1${out}`;
  }
  return out;
}

switch (command) {
  case "init": {
    const kind = rest.shift();
    if (!plugins.PLUGIN_KINDS.includes(kind)) usage();
    const out = flag("out", "spacekit-plugin.json");
    if (existsSync(out)) {
      console.error(`spacekit-plugin: ${out} already exists`);
      process.exit(2);
    }
    const descriptor = {
      spacekit_plugin: 1,
      id: flag("id", `example.my-${kind}`),
      name: `My ${kind}`,
      version: "0.1.0",
      kind,
      publisher: flag("publisher", "did:key:z6Mk..."),
      summary: "One plain sentence that says what this does for an app.",
      license: "proprietary",
      pricing: { model: "free" },
      ...TEMPLATES[kind],
    };
    writeFileSync(out, `${JSON.stringify(descriptor, null, 2)}\n`);
    console.log(`Wrote ${out}. Fill it in, then run: spacekit-plugin validate ${out}`);
    break;
  }
  case "validate": {
    const path = rest[0] ?? "spacekit-plugin.json";
    const result = plugins.validatePluginDescriptor(readDescriptor(path));
    for (const w of result.warnings) console.log(`warning: ${w}`);
    if (!result.ok) {
      for (const e of result.errors) console.log(`error: ${e}`);
      process.exit(1);
    }
    console.log(`${path} is valid. People installing it will see:\n`);
    for (const line of plugins.describePlugin(result.descriptor)) console.log(`  ${line}`);
    break;
  }
  case "hash": {
    if (!rest.length) usage();
    const files = rest.map((arg) => {
      const eq = arg.indexOf("=");
      if (eq < 1) usage();
      const role = arg.slice(0, eq);
      const file = arg.slice(eq + 1);
      const bytes = readFileSync(file);
      return { path: basename(file), role, sha256: createHash("sha256").update(bytes).digest("hex"), size: statSync(file).size };
    });
    console.log(JSON.stringify({ files }, null, 2));
    break;
  }
  case "sign": {
    const keyFile = flag("key");
    const path = rest[0] ?? "spacekit-plugin.json";
    const out = flag("out", path.replace(/\.json$/, "") + ".sig.json");
    if (!keyFile) usage();
    const bytes = readDescriptor(path);
    const result = plugins.validatePluginDescriptor(bytes);
    if (!result.ok) {
      for (const e of result.errors) console.log(`error: ${e}`);
      console.error("spacekit-plugin: fix the descriptor before signing");
      process.exit(1);
    }
    const seed = Buffer.from(readFileSync(keyFile, "utf8").trim(), "hex");
    if (seed.length !== 32) {
      console.error("spacekit-plugin: --key must hold a 32-byte Ed25519 seed as hex");
      process.exit(2);
    }
    const key = createPrivateKey({
      key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]),
      format: "der",
      type: "pkcs8",
    });
    const pub = Buffer.from(createPublicKey(key).export({ format: "jwk" }).x, "base64url");
    const did = `did:key:z${base58(Buffer.concat([Buffer.from([0xed, 0x01]), pub]))}`;
    if (result.descriptor.publisher !== did) {
      console.log(`warning: publisher is ${result.descriptor.publisher}, but this key is ${did}`);
    }
    const message = await plugins.pluginSigningMessage(new Uint8Array(bytes));
    const signature = edSign(null, Buffer.from(message), key).toString("hex");
    writeFileSync(
      out,
      `${JSON.stringify({ v: 1, alg: "ed25519", did, public_key: pub.toString("hex"), signature }, null, 2)}\n`,
    );
    console.log(`Signed ${path} as ${did} -> ${out}`);
    break;
  }
  default:
    usage(command ? 1 : 0);
}
