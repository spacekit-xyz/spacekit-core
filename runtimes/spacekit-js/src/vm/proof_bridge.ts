/**
 * Proof Bridge: adapter interface and config for pushing SpaceKit proofs
 * to external chains (Ethereum, Bitcoin, Solana). See docs/PROOF_BRIDGE_DESIGN.md.
 */

import type { StateProof, QuantumStateProof, TxProof, ReceiptProof } from "./spacekitvm.js";
import type { RollupBundle, SignedRollupBundle } from "./sequencer.js";

/** Payloads that can be submitted to a chain adapter. */
export type ProofBridgePayload =
  | { kind: "state_root"; blockHeight: number; stateRoot: string; quantumStateRoot?: string }
  | { kind: "state_proof"; blockHeight: number; proof: StateProof }
  | { kind: "quantum_state_proof"; blockHeight: number; proof: QuantumStateProof }
  | { kind: "bundle"; bundle: RollupBundle }
  | { kind: "signed_bundle"; signed: SignedRollupBundle }
  | { kind: "tx_proof"; proof: TxProof }
  | { kind: "receipt_proof"; proof: ReceiptProof };

/** Result of a single submit. */
export interface ProofBridgeSubmitResult {
  success: boolean;
  id?: string;
  error?: string;
}

/** Adapter that submits proofs to one target chain. */
export interface ProofBridgeAdapter {
  readonly chainId: string;

  isReady(): boolean;

  submit(payload: ProofBridgePayload): Promise<ProofBridgeSubmitResult>;

  start?(): Promise<void>;
  stop?(): Promise<void>;
}

/** Per-chain config (secrets can be env var names). */
export interface ProofBridgeChainConfig {
  enabled: boolean;
  chainId: string;

  rpcUrl?: string;
  contractAddress?: string;
  privateKeyHex?: string;
  gasLimit?: number;

  network?: "mainnet" | "testnet" | "signet";
  commitMethod?: "op_return" | "taproot" | "indexer_api";
  indexerUrl?: string;

  programId?: string;
  keypairPath?: string;
  commitment?: "processed" | "confirmed" | "finalized";

  payloadKinds?: ProofBridgePayload["kind"][];
}

/** Top-level bridge config; can be loaded from URL or inline. */
export interface ProofBridgeConfig {
  source: "inline" | "url";
  configUrl?: string;
  chains?: Record<string, ProofBridgeChainConfig>;
  refreshIntervalMs?: number;
}

/** Resolve env var references in chain config (e.g. privateKeyHex: "SPACEKIT_ETH_SIGNER_KEY"). */
export function substituteEnvInChainConfig(
  chainConfig: ProofBridgeChainConfig
): ProofBridgeChainConfig {
  const envVarPattern = /^[A-Z_][A-Z0-9_]*$/;
  const resolve = (v: string | undefined): string | undefined => {
    if (v == null) return v;
    if (typeof v !== "string") return v;
    if (envVarPattern.test(v) && typeof process !== "undefined" && process.env?.[v] != null) {
      return process.env[v];
    }
    return v;
  };
  return {
    ...chainConfig,
    privateKeyHex: resolve(chainConfig.privateKeyHex) ?? chainConfig.privateKeyHex,
  };
}

/** Load chain configs from ProofBridgeConfig (inline or fetch from configUrl). Returns enabled chains only. */
export async function loadProofBridgeConfig(
  config: ProofBridgeConfig
): Promise<ProofBridgeChainConfig[]> {
  let raw: Record<string, ProofBridgeChainConfig> | undefined;
  if (config.source === "inline") {
    raw = config.chains;
  } else if (config.source === "url" && config.configUrl) {
    const res = await fetch(config.configUrl);
    if (!res.ok) throw new Error(`Proof bridge config fetch failed: ${res.status} ${res.statusText}`);
    const body = await res.json();
    raw = body.chains ?? body.bridgeConfig?.chains ?? body;
  } else {
    raw = undefined;
  }
  if (!raw || typeof raw !== "object") return [];
  const list = Object.values(raw).filter((c): c is ProofBridgeChainConfig => c?.enabled === true);
  return list.map(substituteEnvInChainConfig);
}

/** Create adapters from loaded chain configs. Requires optional adapter packages or in-repo adapters. */
export async function createAdaptersFromConfig(
  chainConfigs: ProofBridgeChainConfig[],
  options?: { adapterRegistry?: AdapterRegistry }
): Promise<ProofBridgeAdapter[]> {
  const registry = options?.adapterRegistry ?? getDefaultAdapterRegistry();
  const adapters: ProofBridgeAdapter[] = [];
  for (const cc of chainConfigs) {
    const adapter = await registry.create(cc);
    if (adapter) adapters.push(adapter);
  }
  return adapters;
}

/** Registry that creates an adapter from chain config. Plug in custom or optional adapters. */
export type AdapterRegistry = {
  create(config: ProofBridgeChainConfig): Promise<ProofBridgeAdapter | null>;
};

let defaultRegistry: AdapterRegistry | null = null;

/** Set the default adapter registry (used by createAdaptersFromConfig). Used by adapters/index to register Ethereum/Bitcoin/Solana. */
export function setDefaultAdapterRegistry(registry: AdapterRegistry): void {
  defaultRegistry = registry;
}

function getDefaultAdapterRegistry(): AdapterRegistry {
  if (defaultRegistry) return defaultRegistry;
  return {
    async create(_config: ProofBridgeChainConfig): Promise<ProofBridgeAdapter | null> {
      return null;
    },
  };
}
