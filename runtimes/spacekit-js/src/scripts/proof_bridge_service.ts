/**
 * Proof Bridge Service: out-of-process daemon that loads bridge config,
 * creates adapters (Ethereum, Bitcoin, Solana), and polls a SpaceKit storage
 * node or RPC for new bundles/state roots and submits them to each chain.
 *
 * Env:
 *   PROOF_BRIDGE_CONFIG_URL  - fetch config JSON from this URL
 *   PROOF_BRIDGE_CONFIG_PATH - or path to local config JSON file
 *   PROOF_BRIDGE_SOURCE      - "storage" | "rpc" (default: storage)
 *   POLL_INTERVAL_MS         - poll interval (default: 30000)
 *   For storage source:
 *     SPACEKIT_STORAGE_BASE_URL, SPACEKIT_STORAGE_DID,
 *     SPACEKIT_ROLLUP_COLLECTION (default: spacekitvm_rollups)
 *   For rpc source:
 *     SPACEKIT_RPC_URL
 *
 * Run after build: node dist/scripts/proof_bridge_service.js
 * Or: npm run proof-bridge-service
 */

import { StorageNodeAdapter } from "../storage.js";
import {
  loadProofBridgeConfig,
  createAdaptersFromConfig,
  type ProofBridgeConfig,
  type ProofBridgeAdapter,
} from "../vm/proof_bridge.js";

// Register default adapters (Ethereum, Bitcoin, Solana)
import "../vm/adapters/index.js";

function getEnv(name: string, def?: string): string | undefined {
  if (typeof process === "undefined" || !process.env) return def;
  const v = process.env[name];
  return v !== undefined && v !== "" ? v : def;
}

async function loadConfig(): Promise<ProofBridgeConfig> {
  const configUrl = getEnv("PROOF_BRIDGE_CONFIG_URL");
  const configPath = getEnv("PROOF_BRIDGE_CONFIG_PATH");
  if (configUrl) {
    return { source: "url", configUrl };
  }
  if (configPath) {
    const fs = await import("fs");
    const path = await import("path");
    const fullPath = path.resolve(process.cwd(), configPath);
    const raw = fs.readFileSync(fullPath, "utf8");
    const data = JSON.parse(raw) as ProofBridgeConfig;
    return data.source === "url" ? data : { ...data, source: "inline" };
  }
  throw new Error("Set PROOF_BRIDGE_CONFIG_URL or PROOF_BRIDGE_CONFIG_PATH");
}

async function pollStorage(
  adapters: ProofBridgeAdapter[],
  baseUrl: string,
  did: string,
  collection: string,
  seenIds: Set<string>
): Promise<void> {
  const storage = new StorageNodeAdapter({ baseUrl, did, collection });
  const docs = await storage.listDocuments(collection);
  for (const doc of docs) {
    const id = doc?.id;
    const bundle = doc?.data?.bundle;
    if (!id || !bundle) continue;
    if (seenIds.has(id)) continue;
    seenIds.add(id);
    const signed = doc?.data?.bundle?.signature != null;
    const payload = signed
      ? { kind: "signed_bundle" as const, signed: bundle }
      : { kind: "bundle" as const, bundle };
    for (const adapter of adapters) {
      if (!adapter.isReady()) continue;
      try {
        const result = await adapter.submit(payload);
        if (result.success) {
          console.log(`[proof-bridge] ${adapter.chainId} submitted ${id} -> ${result.id ?? "ok"}`);
        } else {
          console.warn(`[proof-bridge] ${adapter.chainId} ${id}: ${result.error}`);
        }
      } catch (e) {
        console.warn(`[proof-bridge] ${adapter.chainId} ${id}:`, e);
      }
    }
  }
}

async function pollRpc(
  adapters: ProofBridgeAdapter[],
  rpcUrl: string,
  lastHeightRef: { current: number }
): Promise<void> {
  const res = await fetch(rpcUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "vm_blocks",
      params: [],
    }),
  });
  if (!res.ok) return;
  const json = await res.json().catch(() => ({}));
  const blocks = json?.result ?? [];
  if (!Array.isArray(blocks) || blocks.length === 0) return;
  const latest = blocks[blocks.length - 1];
  const height = latest?.height ?? 0;
  const stateRoot = latest?.stateRoot;
  const quantumStateRoot = latest?.quantumStateRoot;
  if (height <= lastHeightRef.current || !stateRoot) return;
  lastHeightRef.current = height;
  const payload = { kind: "state_root" as const, blockHeight: height, stateRoot, quantumStateRoot };
  for (const adapter of adapters) {
    if (!adapter.isReady()) continue;
    try {
      const result = await adapter.submit(payload);
      if (result.success) {
        console.log(`[proof-bridge] ${adapter.chainId} state_root ${height} -> ${result.id ?? "ok"}`);
      } else {
        console.warn(`[proof-bridge] ${adapter.chainId} state_root ${height}: ${result.error}`);
      }
    } catch (e) {
      console.warn(`[proof-bridge] ${adapter.chainId} state_root ${height}:`, e);
    }
  }
}

async function main(): Promise<void> {
  const config = await loadConfig();
  const chainConfigs = await loadProofBridgeConfig(config);
  const adapters = await createAdaptersFromConfig(chainConfigs);
  if (adapters.length === 0) {
    console.warn("[proof-bridge] No adapters created. Check config and optional deps (viem, @solana/web3.js).");
  } else {
    console.log(`[proof-bridge] ${adapters.length} adapter(s): ${adapters.map((a) => a.chainId).join(", ")}`);
  }

  const source = getEnv("PROOF_BRIDGE_SOURCE", "storage");
  const pollMs = Math.max(5000, parseInt(getEnv("POLL_INTERVAL_MS", "30000")!, 10));

  if (source === "storage") {
    const baseUrl = getEnv("SPACEKIT_STORAGE_BASE_URL");
    const did = getEnv("SPACEKIT_STORAGE_DID");
    const collection = getEnv("SPACEKIT_ROLLUP_COLLECTION", "spacekitvm_rollups") ?? "spacekitvm_rollups";
    if (!baseUrl || !did) {
      throw new Error("For source=storage set SPACEKIT_STORAGE_BASE_URL and SPACEKIT_STORAGE_DID");
    }
    const seenIds = new Set<string>();
    console.log(`[proof-bridge] Polling storage ${baseUrl} collection=${collection} every ${pollMs}ms`);
    for (;;) {
      await pollStorage(adapters, baseUrl, did, collection, seenIds);
      await new Promise((r) => setTimeout(r, pollMs));
    }
  } else if (source === "rpc") {
    const rpcUrl = getEnv("SPACEKIT_RPC_URL");
    if (!rpcUrl) throw new Error("For source=rpc set SPACEKIT_RPC_URL");
    const lastHeightRef = { current: 0 };
    console.log(`[proof-bridge] Polling RPC ${rpcUrl} every ${pollMs}ms`);
    for (;;) {
      await pollRpc(adapters, rpcUrl, lastHeightRef);
      await new Promise((r) => setTimeout(r, pollMs));
    }
  } else {
    throw new Error("PROOF_BRIDGE_SOURCE must be 'storage' or 'rpc'");
  }
}

main().catch((e) => {
  console.error("[proof-bridge]", e);
  process.exit(1);
});
