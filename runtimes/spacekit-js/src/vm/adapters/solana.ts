/**
 * Solana proof-bridge adapter. Optional dependency: @solana/web3.js.
 * Submits state root / bundle hash to a program account or instruction.
 */

import type {
  ProofBridgeAdapter,
  ProofBridgeChainConfig,
  ProofBridgePayload,
  ProofBridgeSubmitResult,
} from "../proof_bridge.js";

const CHAIN_ID_PREFIX = "solana";

function createStubAdapter(chainId: string, reason: string): ProofBridgeAdapter {
  return {
    chainId,
    isReady: () => false,
    async submit(): Promise<ProofBridgeSubmitResult> {
      return { success: false, error: reason };
    },
  };
}

export async function createSolanaAdapter(
  config: ProofBridgeChainConfig
): Promise<ProofBridgeAdapter | null> {
  if (!config.chainId.toLowerCase().startsWith(CHAIN_ID_PREFIX) || !config.enabled) return null;
  if (!config.rpcUrl || !config.programId) return null;

  let solana: Record<string, unknown>;
  try {
    solana = await import("@solana/web3.js") as Record<string, unknown>;
  } catch {
    return createStubAdapter(config.chainId, "@solana/web3.js not installed");
  }

  const Connection = solana.Connection as new (url: string, opts?: { commitment?: string }) => {
    sendTransaction: (tx: unknown, signers: unknown[], opts?: unknown) => Promise<string>;
    getLatestBlockhash: (c?: string) => Promise<{ blockhash: string; lastValidBlockHeight: number }>;
    confirmTransaction: (args: unknown) => Promise<void>;
  };
  const Keypair = solana.Keypair as { fromSecretKey: (bytes: Uint8Array) => unknown };
  const PublicKey = solana.PublicKey as new (s: string) => unknown;
  const Transaction = solana.Transaction as new () => { add: (ix: unknown) => unknown };
  const TransactionInstruction = solana.TransactionInstruction as new (opts: { keys: unknown[]; programId: unknown; data: Buffer }) => unknown;

  const connection = new Connection(config.rpcUrl, {
    commitment: config.commitment ?? "confirmed",
  });

  let keypair: unknown = null;
  if (config.privateKeyHex) {
    const hex = config.privateKeyHex.startsWith("0x") ? config.privateKeyHex.slice(2) : config.privateKeyHex;
    keypair = Keypair.fromSecretKey(new Uint8Array(Buffer.from(hex, "hex")));
  } else if (config.keypairPath && typeof process !== "undefined" && process.env) {
    const fs = await import("fs").catch(() => null);
    const path = await import("path").catch(() => null);
    if (fs?.readFileSync && path?.resolve) {
      try {
        const keypairPath = path.resolve(process.cwd(), config.keypairPath);
        const secret = JSON.parse(fs.readFileSync(keypairPath, "utf8")) as number[];
        keypair = Keypair.fromSecretKey(Uint8Array.from(secret));
      } catch {
        // ignore
      }
    }
  }

  const programId = new PublicKey(config.programId);

  const adapter: ProofBridgeAdapter = {
    chainId: config.chainId,
    isReady: () => keypair != null,
    async submit(payload: ProofBridgePayload): Promise<ProofBridgeSubmitResult> {
      if (!keypair) return { success: false, error: "no keypair configured" };
      try {
        const data = new Uint8Array(128);
        const enc = new TextEncoder();
        if (payload.kind === "state_root") {
          data.set(enc.encode("state_root"), 0);
          new DataView(data.buffer).setUint32(32, payload.blockHeight, true);
          const stateRootHex = payload.stateRoot.startsWith("0x") ? payload.stateRoot.slice(2) : payload.stateRoot;
          const stateRootBytes = Buffer.from(stateRootHex.padStart(64, "0").slice(0, 64), "hex");
          data.set(stateRootBytes, 36);
        } else if (payload.kind === "bundle" || payload.kind === "signed_bundle") {
          const bundleHash = payload.kind === "bundle" ? payload.bundle.bundleHash : payload.signed.bundleHash;
          data.set(enc.encode("bundle"), 0);
          const hashHex = bundleHash.startsWith("0x") ? bundleHash.slice(2) : bundleHash;
          const hashBytes = Buffer.from(hashHex.padStart(64, "0").slice(0, 64), "hex");
          data.set(hashBytes, 32);
        } else {
          return { success: false, error: "unsupported payload kind" };
        }

        const ix = new TransactionInstruction({
          keys: [{ pubkey: programId, isSigner: false, isWritable: true }],
          programId,
          data: Buffer.from(data),
        });
        const tx = new Transaction().add(ix);
        const sig = await connection.sendTransaction(tx, [keypair], {
          skipPreflight: false,
          preflightCommitment: config.commitment ?? "confirmed",
        });
        const latest = await connection.getLatestBlockhash(config.commitment ?? "confirmed");
        await connection.confirmTransaction({ signature: sig, ...latest, commitment: config.commitment ?? "confirmed" });
        return { success: true, id: sig };
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        return { success: false, error: msg };
      }
    },
  };

  return adapter;
}
