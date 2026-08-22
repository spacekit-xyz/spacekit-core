#!/usr/bin/env node
/**
 * SpaceKit Node.js Entry Point
 *
 * Starts a SpacekitVm instance with an HTTP JSON-RPC server.
 *
 * Usage:
 *   node dist/entry-node.js [--port 8747] [--host 127.0.0.1] [--chain-id spacekit-local]
 *
 * Environment variables:
 *   SPACEKIT_PORT       – HTTP port (default 8747)
 *   SPACEKIT_HOST       – Bind address (default 127.0.0.1)
 *   SPACEKIT_CHAIN_ID   – Chain identifier (default spacekit-local)
 *   SPACEKIT_API_KEY    – Optional API key for RPC auth
 *   SPACEKIT_DEV_MODE   – Set to "true" for dev mode (no signature checks)
 */

import { installPolyfills, detectRuntime } from "./platform.js";
import { SpacekitVm } from "./vm/spacekitvm.js";
import { getGenesisPresetForNetwork } from "./vm/genesis.js";
import { createInMemoryStorage } from "./storage.js";
import { startJsonRpcServer } from "./vm/http_rpc_server.js";

/* ───────────────────── CLI arg parsing ───────────────────── */

function parseArgs(): Record<string, string> {
  const args: Record<string, string> = {};
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg.startsWith("--")) {
      const key = arg.slice(2);
      const next = argv[i + 1];
      if (next && !next.startsWith("--")) {
        args[key] = next;
        i++;
      } else {
        args[key] = "true";
      }
    }
  }
  return args;
}

/* ───────────────────── Bootstrap ───────────────────── */

async function main(): Promise<void> {
  const runtime = detectRuntime();
  console.log(`[spacekit] Runtime detected: ${runtime}`);
  console.log("[spacekit] Installing polyfills...");

  await installPolyfills();

  const args = parseArgs();
  const port = parseInt(args["port"] ?? process.env.SPACEKIT_PORT ?? "8747", 10);
  const host = args["host"] ?? process.env.SPACEKIT_HOST ?? "127.0.0.1";
  const chainId = args["chain-id"] ?? process.env.SPACEKIT_CHAIN_ID ?? "spacekit-local";
  const apiKey = args["api-key"] ?? process.env.SPACEKIT_API_KEY;
  const devMode = (args["dev-mode"] ?? process.env.SPACEKIT_DEV_MODE) === "true";

  console.log(`[spacekit] Initializing VM (chain: ${chainId}, devMode: ${devMode})...`);

  const storage = createInMemoryStorage();
  const genesisConfig = getGenesisPresetForNetwork(chainId) ?? undefined;
  const vm = new SpacekitVm({
    storage,
    chainId,
    genesisConfig,
    devMode,
  });

  const server = startJsonRpcServer(vm, {
    port,
    host,
    apiKey,
  });

  console.log(`[spacekit] JSON-RPC server listening on http://${host}:${port}`);
  console.log("[spacekit] Ready to accept requests.");

  // Graceful shutdown
  const shutdown = () => {
    console.log("\n[spacekit] Shutting down...");
    server.close(() => {
      console.log("[spacekit] Server closed.");
      process.exit(0);
    });
  };

  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

main().catch((err) => {
  console.error("[spacekit] Fatal error:", err);
  process.exit(1);
});
