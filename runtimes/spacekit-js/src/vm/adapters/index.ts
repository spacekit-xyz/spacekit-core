/**
 * Proof-bridge adapters for Ethereum, Bitcoin, and Solana.
 * Registers the default adapter registry so createAdaptersFromConfig works.
 */

import {
  setDefaultAdapterRegistry,
  type ProofBridgeAdapter,
  type ProofBridgeChainConfig,
} from "../proof_bridge.js";
import { createEthereumAdapter } from "./ethereum.js";
import { createBitcoinAdapter } from "./bitcoin.js";
import { createSolanaAdapter } from "./solana.js";

async function createAdapter(config: ProofBridgeChainConfig): Promise<ProofBridgeAdapter | null> {
  const id = config.chainId.toLowerCase();
  if (id.startsWith("ethereum")) return createEthereumAdapter(config);
  if (id.startsWith("bitcoin")) return createBitcoinAdapter(config);
  if (id.startsWith("solana")) return createSolanaAdapter(config);
  return null;
}

const registry = {
  async create(config: ProofBridgeChainConfig): Promise<ProofBridgeAdapter | null> {
    return createAdapter(config);
  },
};

/** Register the default Ethereum/Bitcoin/Solana adapter registry. Call once before createAdaptersFromConfig if using adapters. */
export function registerDefaultAdapters(): void {
  setDefaultAdapterRegistry(registry);
}

// Auto-register when this module is loaded so `createAdaptersFromConfig` works for consumers who import from adapters
registerDefaultAdapters();

export { createEthereumAdapter } from "./ethereum.js";
export { createBitcoinAdapter } from "./bitcoin.js";
export { createSolanaAdapter } from "./solana.js";
