#!/usr/bin/env bun
/**
 * SpaceKit Bun Entry Point
 *
 * Starts a SpacekitVm instance with both:
 *  - A Node-compatible HTTP JSON-RPC server (via node:http)
 *  - Optionally a native Bun.serve() server for better performance
 *
 * Usage:
 *   bun dist/entry-bun.js [--port 8747] [--host 127.0.0.1] [--chain-id spacekit-local]
 *
 * Environment variables:
 *   SPACEKIT_PORT       – HTTP port (default 8747)
 *   SPACEKIT_HOST       – Bind address (default 127.0.0.1)
 *   SPACEKIT_CHAIN_ID   – Chain identifier (default spacekit-local)
 *   SPACEKIT_API_KEY    – Optional API key for RPC auth
 *   SPACEKIT_DEV_MODE   – Set to "true" for dev mode (no signature checks)
 *   SPACEKIT_USE_BUN_SERVE – Set to "true" to use Bun.serve() instead of node:http
 */

import { installPolyfills, detectRuntime } from "./platform.js";
import { SpacekitVm } from "./vm/spacekitvm.js";
import { getGenesisPresetForNetwork } from "./vm/genesis.js";
import { createInMemoryStorage } from "./storage.js";
import { startJsonRpcServer } from "./vm/http_rpc_server.js";
import { createJsonRpcHandler, type JsonRpcRequest } from "./vm/json_rpc.js";

/* ───────────────────── CLI arg parsing ───────────────────── */

function parseArgs(): Record<string, string> {
  const args: Record<string, string> = {};
  const argv = (typeof Bun !== "undefined" ? Bun.argv : process.argv).slice(2);
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

/* ───────────────────── Bun.serve()-based server ───────────────────── */

function startBunServer(
  vm: SpacekitVm,
  options: { port: number; host: string; apiKey?: string }
) {
  const handler = createJsonRpcHandler(vm);
  const corsHeaders = {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type, Authorization",
  };

  return Bun.serve({
    port: options.port,
    hostname: options.host,
    async fetch(req: Request) {
      if (req.method === "OPTIONS") {
        return new Response(null, { status: 204, headers: corsHeaders });
      }

      if (req.method !== "POST") {
        return Response.json({ error: "Not found" }, { status: 404, headers: corsHeaders });
      }

      // API key check
      if (options.apiKey) {
        const auth = req.headers.get("authorization");
        const keyHeader = req.headers.get("x-api-key");
        const bearer = auth?.startsWith("Bearer ") ? auth.slice(7) : undefined;
        const provided = keyHeader ?? bearer;
        if (!provided || provided !== options.apiKey) {
          return Response.json({ error: "Invalid API key" }, { status: 401, headers: corsHeaders });
        }
      }

      try {
        const body = await req.json();
        if (Array.isArray(body)) {
          const responses = await Promise.all(
            body.map((item: unknown) => handler(item as JsonRpcRequest))
          );
          return Response.json(responses, { headers: corsHeaders });
        }
        const response = await handler(body as JsonRpcRequest);
        return Response.json(response, { headers: corsHeaders });
      } catch (error) {
        return Response.json(
          {
            jsonrpc: "2.0",
            id: null,
            error: {
              code: -32000,
              message: error instanceof Error ? error.message : "Server error",
            },
          },
          { status: 500, headers: corsHeaders }
        );
      }
    },
  });
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
  const useBunServe = (args["use-bun-serve"] ?? process.env.SPACEKIT_USE_BUN_SERVE) === "true";

  console.log(`[spacekit] Initializing VM (chain: ${chainId}, devMode: ${devMode})...`);

  const storage = createInMemoryStorage();
  const genesisConfig = getGenesisPresetForNetwork(chainId) ?? undefined;
  const vm = new SpacekitVm({
    storage,
    chainId,
    genesisConfig,
    devMode,
  });

  if (useBunServe && typeof Bun !== "undefined") {
    const server = startBunServer(vm, { port, host, apiKey });
    console.log(`[spacekit] Bun.serve() JSON-RPC server listening on http://${server.hostname}:${server.port}`);
  } else {
    startJsonRpcServer(vm, { port, host, apiKey });
    console.log(`[spacekit] JSON-RPC server listening on http://${host}:${port}`);
  }

  console.log("[spacekit] Ready to accept requests.");

  // Graceful shutdown
  const shutdown = () => {
    console.log("\n[spacekit] Shutting down...");
    process.exit(0);
  };

  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}

main().catch((err) => {
  console.error("[spacekit] Fatal error:", err);
  process.exit(1);
});
