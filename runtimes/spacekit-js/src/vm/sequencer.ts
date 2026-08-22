import { SpacekitVm } from "./spacekitvm.js";
import { sha256Hex, hashString } from "./hash.js";
import type { StorageNodeAdapter } from "../storage.js";
import { bytesToHex, hexToBytes } from "../storage.js";
import type { ProofBridgeAdapter } from "./proof_bridge.js";

export interface BundleTxPayload {
  blockIndex: number;
  from: string;
  to?: string;
  data: string;
  value: string;
  gasLimit: number;
}

export interface RollupBundle {
  bundleId: string;
  fromHeight: number;
  toHeight: number;
  blockCount: number;
  blockHashes: string[];
  stateRoots: string[];
  quantumStateRoots?: string[];
  txRoots: string[];
  receiptRoots: string[];
  sealedArchives: Array<{
    fromHeight: number;
    toHeight: number;
    blockCount: number;
    sealHash: string;
    timestamp: number;
  }>;
  timestamp: number;
  bundleHash: string;
  txPayloads?: BundleTxPayload[];
}

export type BundleStatus = "verified" | "challenged" | "rejected" | "pending";

export interface BundleVerificationResult {
  bundleId: string;
  hashValid: boolean;
  signatureValid: boolean;
  keyAllowed: boolean;
  reExecutionResults: Array<{
    blockIndex: number;
    expectedStateRoot: string;
    computedStateRoot: string;
    matchOk: boolean;
  }>;
  allRootsMatch: boolean;
  challengeWindowEnd: number;
  status: BundleStatus;
}

export interface BundleSignature {
  algorithm: "ed25519";
  publicKeyHex: string;
  signatureBase64: string;
}

export interface SignedRollupBundle extends RollupBundle {
  signature: BundleSignature;
}

export interface SequencerOptions {
  maxBlocksPerBundle?: number;
  onBundle?: (bundle: RollupBundle) => void;
  /** Optional adapters to submit bundles/proofs to other chains (Ethereum, Bitcoin, Solana). */
  proofBridgeAdapters?: ProofBridgeAdapter[];
}

export interface BundleSigningOptions {
  privateKeyHex: string;
}

/**
 * The bundle ID is bound into the bundle hash the L1 verifies, so it must not
 * be guessable: a predictable ID lets an attacker prepare a competing bundle
 * under the same identifier before the real one is published.
 */
function generateBundleId(): string {
  const bytes = new Uint8Array(16);
  const webcrypto = globalThis.crypto;
  if (!webcrypto?.getRandomValues) {
    throw new Error(
      "No cryptographically secure random source available (globalThis.crypto.getRandomValues). " +
        "Refusing to generate a bundle ID from a predictable source.",
    );
  }
  webcrypto.getRandomValues(bytes);
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `bundle_${hex}_${Date.now()}`;
}

export class SpacekitSequencer {
  private vm: SpacekitVm;
  private maxBlocksPerBundle: number;
  private onBundle?: (bundle: RollupBundle) => void;
  private proofBridgeAdapters?: ProofBridgeAdapter[];
  private lastSealedIndex = 0;

  constructor(vm: SpacekitVm, options: SequencerOptions = {}) {
    this.vm = vm;
    this.maxBlocksPerBundle = options.maxBlocksPerBundle ?? 10;
    this.onBundle = options.onBundle;
    this.proofBridgeAdapters = options.proofBridgeAdapters;
  }

  async mineAndBundle(): Promise<RollupBundle | null> {
    const block = await this.vm.mineBlock();
    if (!block) {
      return null;
    }
    const blocks = this.vm.getBlocks();
    if (blocks.length >= this.maxBlocksPerBundle) {
      return this.flushBundle();
    }
    return null;
  }

  async flushBundle(): Promise<RollupBundle> {
    const blocks = this.vm.getBlocks();
    if (blocks.length === 0) {
      throw new Error("No blocks to bundle");
    }
    const fromHeight = blocks[0].height;
    const toHeight = blocks[blocks.length - 1].height;
    const timestamp = Date.now();

    const blockHashes = blocks.map((b) => b.blockHash);
    const stateRoots = blocks.map((b) => b.stateRoot);
    const quantumStateRoots = blocks.every((b) => b.quantumStateRoot)
      ? blocks.map((b) => b.quantumStateRoot as string)
      : undefined;
    const txRoots = blocks.map((b) => b.txRoot);
    const receiptRoots = blocks.map((b) => b.receiptRoot);
    const sealedArchives = this.vm.getSealedArchives().slice(this.lastSealedIndex);
    this.lastSealedIndex = this.vm.getSealedArchives().length;

    const txPayloads: BundleTxPayload[] = [];
    for (const block of blocks) {
      for (const tx of block.transactions ?? []) {
        txPayloads.push({
          blockIndex: block.height,
          from: tx.callerDid ?? "",
          to: tx.contractId,
          data: typeof tx.input === "string" ? tx.input : "",
          value: String(tx.value ?? "0"),
          gasLimit: 0,
        });
      }
    }

    const payload = {
      fromHeight,
      toHeight,
      blockHashes,
      stateRoots,
      quantumStateRoots,
      txRoots,
      receiptRoots,
      sealedArchives,
      timestamp,
    };
    const bundleHash = await sha256Hex(hashString(JSON.stringify(payload)));
    const bundle: RollupBundle = {
      bundleId: generateBundleId(),
      fromHeight,
      toHeight,
      blockCount: blocks.length,
      blockHashes,
      stateRoots,
      quantumStateRoots,
      txRoots,
      receiptRoots,
      sealedArchives,
      timestamp,
      bundleHash,
      txPayloads: txPayloads.length > 0 ? txPayloads : undefined,
    };

    if (this.onBundle) {
      this.onBundle(bundle);
    }
    for (const adapter of this.proofBridgeAdapters ?? []) {
      if (!adapter.isReady()) continue;
      try {
        await adapter.submit({ kind: "bundle", bundle });
      } catch (_e) {
        // Log and continue; caller can add retry or logging
      }
    }
    return bundle;
  }

  async exportBundle(bundle: RollupBundle, storage: StorageNodeAdapter, collection = "spacekitvm_rollups") {
    return storage.putDocument(collection, bundle.bundleId, {
      bundle,
      exported_at: Date.now(),
    });
  }

  async signBundle(bundle: RollupBundle, options: BundleSigningOptions): Promise<SignedRollupBundle> {
    const ed = await import("@noble/ed25519");
    const privateKey = hexToBytes(options.privateKeyHex);
    const publicKey = await ed.getPublicKey(privateKey);
    const signature = await ed.sign(hexToBytes(bundle.bundleHash), privateKey);
    return {
      ...bundle,
      signature: {
        algorithm: "ed25519",
        publicKeyHex: bytesToHex(publicKey),
        signatureBase64: toBase64(signature),
      },
    };
  }

  async exportSignedBundle(
    signedBundle: SignedRollupBundle,
    storage: StorageNodeAdapter,
    collection = "spacekitvm_rollups"
  ) {
    for (const adapter of this.proofBridgeAdapters ?? []) {
      if (!adapter.isReady()) continue;
      try {
        await adapter.submit({ kind: "signed_bundle", signed: signedBundle });
      } catch (_e) {
        // Log and continue
      }
    }
    return storage.putDocument(collection, signedBundle.bundleId, {
      bundle: signedBundle,
      exported_at: Date.now(),
    });
  }

  async submitBundleToL1(
    bundle: RollupBundle | SignedRollupBundle,
    l1Url: string
  ): Promise<BundleVerificationResult> {
    const res = await fetch(`${l1Url.replace(/\/$/, "")}/rollup/validate`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(bundle),
    });
    if (!res.ok) throw new Error(`L1 rejected bundle: ${res.status}`);
    const json = await res.json();
    return json.verification as BundleVerificationResult;
  }

  async queryBundleStatus(
    bundleId: string,
    l1Url: string
  ): Promise<{ status: BundleStatus; challengeWindowEnd: number; fraudProofs: number } | null> {
    const res = await fetch(`${l1Url.replace(/\/$/, "")}/rollup/status/${encodeURIComponent(bundleId)}`);
    if (res.status === 404) return null;
    if (!res.ok) throw new Error(`status query failed: ${res.status}`);
    return res.json();
  }
}

function toBase64(bytes: Uint8Array): string {
  if (typeof btoa !== "undefined") {
    let binary = "";
    for (const b of bytes) {
      binary += String.fromCharCode(b);
    }
    return btoa(binary);
  }
  return Buffer.from(bytes).toString("base64");
}
