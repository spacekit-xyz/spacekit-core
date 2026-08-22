/**
 * Bitcoin proof-bridge adapter. Supports indexer API (POST commitment to URL).
 * OP_RETURN / Taproot would require optional bitcoinjs-lib.
 */

import type {
  ProofBridgeAdapter,
  ProofBridgeChainConfig,
  ProofBridgePayload,
  ProofBridgeSubmitResult,
} from "../proof_bridge.js";

const CHAIN_ID_PREFIX = "bitcoin";

function createStubAdapter(chainId: string, reason: string): ProofBridgeAdapter {
  return {
    chainId,
    isReady: () => false,
    async submit(): Promise<ProofBridgeSubmitResult> {
      return { success: false, error: reason };
    },
  };
}

export async function createBitcoinAdapter(
  config: ProofBridgeChainConfig
): Promise<ProofBridgeAdapter | null> {
  if (!config.chainId.toLowerCase().startsWith(CHAIN_ID_PREFIX) || !config.enabled) return null;

  if (config.commitMethod === "op_return" || config.commitMethod === "taproot") {
    return createStubAdapter(
      config.chainId,
      "op_return/taproot require optional dependency bitcoinjs-lib"
    );
  }

  const useIndexer = config.commitMethod === "indexer_api" || (config.indexerUrl && config.commitMethod !== "taproot" && config.commitMethod !== "op_return");
  if (useIndexer && config.indexerUrl) {
    const indexerUrl = config.indexerUrl.replace(/\/$/, "");
    const adapter: ProofBridgeAdapter = {
      chainId: config.chainId,
      isReady: () => true,
      async submit(payload: ProofBridgePayload): Promise<ProofBridgeSubmitResult> {
        try {
          let body: { type: string; blockHeight?: number; stateRoot?: string; quantumStateRoot?: string; bundleHash?: string } = { type: payload.kind };
          if (payload.kind === "state_root") {
            body.blockHeight = payload.blockHeight;
            body.stateRoot = payload.stateRoot;
            body.quantumStateRoot = payload.quantumStateRoot;
          } else if (payload.kind === "bundle") {
            body.bundleHash = payload.bundle.bundleHash;
            body.blockHeight = payload.bundle.toHeight;
          } else if (payload.kind === "signed_bundle") {
            body.bundleHash = payload.signed.bundleHash;
            body.blockHeight = payload.signed.toHeight;
          } else {
            return { success: false, error: "unsupported payload kind for indexer_api" };
          }
          const res = await fetch(`${indexerUrl}/commit`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(body),
          });
          if (!res.ok) {
            const text = await res.text();
            return { success: false, error: `${res.status}: ${text}` };
          }
          const data = await res.json().catch(() => ({}));
          const id = data.txId ?? data.id ?? data.txid;
          return { success: true, id };
        } catch (e) {
          const msg = e instanceof Error ? e.message : String(e);
          return { success: false, error: msg };
        }
      },
    };
    return adapter;
  }

  return createStubAdapter(config.chainId, "bitcoin adapter requires commitMethod and indexerUrl or op_return/taproot");
}
