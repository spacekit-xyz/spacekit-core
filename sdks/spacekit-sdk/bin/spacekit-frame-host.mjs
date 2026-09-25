#!/usr/bin/env node
/**
 * Generate the frame-host page for `isolation: "origin"`.
 *
 *   npx spacekit-frame-host --allow https://kit.space --allow https://www.kit.space \
 *     [--per-app] [--out ./public-apps/spacekit-frame.html]
 *
 * Serve the output at /spacekit-frame.html on your app origin (never on the
 * host origin) with `Content-Security-Policy: frame-ancestors <the same hosts>`.
 * Without --out it prints to stdout. With no --allow it writes a placeholder
 * host that you must replace before deploying.
 */
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const { renderFrameHostHtml } = await import(resolve(here, "../dist/embed/frameBootstrap.js"));

const args = process.argv.slice(2);
const allowed = [];
let out = null;
let perAppHost = false;
for (let i = 0; i < args.length; i++) {
  const a = args[i];
  if (a === "--allow") allowed.push(new URL(args[++i]).origin);
  else if (a === "--out") out = args[++i];
  else if (a === "--per-app") perAppHost = true;
  else if (a === "--help" || a === "-h") {
    console.log("usage: spacekit-frame-host --allow <host-origin> [--allow ...] [--per-app] [--out file]");
    process.exit(0);
  } else {
    console.error(`unknown argument: ${a}`);
    process.exit(1);
  }
}

const html = renderFrameHostHtml({
  allowedParents: allowed.length ? allowed : ["https://your-host.example"],
  perAppHost,
});
if (out) {
  mkdirSync(dirname(resolve(out)), { recursive: true });
  writeFileSync(out, html);
  if (!allowed.length) console.warn(`[spacekit-frame-host] wrote ${out} with a placeholder host; pass --allow before deploying`);
} else {
  process.stdout.write(html);
}
