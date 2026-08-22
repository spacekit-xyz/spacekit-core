/**
 * Cryptographic Signature Verification for SpacekitVm
 * 
 * Supports:
 * - ed25519 (classic, fast)
 * - Dilithium (post-quantum, via external WASM)
 * 
 * Uses @noble/ed25519 for ed25519 verification (audited, browser-compatible)
 */

/* ───────────────────────── Types ───────────────────────── */

export type SignatureAlgorithm = "ed25519" | "dilithium3" | "dilithium5";

export interface SignedMessage {
  /** The message bytes (or hex string) */
  message: Uint8Array | string;
  /** The signature (base64 or Uint8Array) */
  signature: Uint8Array | string;
  /** The public key (hex or Uint8Array) */
  publicKey: Uint8Array | string;
  /** The algorithm used */
  algorithm: SignatureAlgorithm;
}

export interface TransactionSignature {
  /** Signature bytes (base64 encoded) */
  signatureBase64: string;
  /** Public key used for signing (hex encoded) */
  publicKeyHex: string;
  /** Algorithm used (ed25519 or dilithium) */
  algorithm: SignatureAlgorithm;
}

export interface SignatureVerifier {
  /**
   * Verify a signature
   * @returns true if valid, false otherwise
   */
  verify(params: SignedMessage): Promise<boolean>;
  
  /**
   * Get supported algorithms
   */
  supportedAlgorithms(): SignatureAlgorithm[];
}

export interface PqSignatureVerifierFunc {
  (
    messageHex: string,
    signatureBase64: string,
    publicKeyHex: string,
    algorithm?: string
  ): Promise<boolean>;
}

/* ───────────────────────── Helpers ───────────────────────── */

const hexTable = Array.from({ length: 256 }, (_, i) => i.toString(16).padStart(2, "0"));

export function bytesToHex(bytes: Uint8Array): string {
  let out = "";
  for (const b of bytes) {
    out += hexTable[b];
  }
  return out;
}

export function hexToBytes(hex: string): Uint8Array {
  const clean = hex.length % 2 === 0 ? hex : `0${hex}`;
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

export function base64ToBytes(base64: string): Uint8Array {
  if (typeof atob !== "undefined") {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  }
  // Node.js fallback
  return new Uint8Array(Buffer.from(base64, "base64"));
}

export function bytesToBase64(bytes: Uint8Array): string {
  if (typeof btoa !== "undefined") {
    let binary = "";
    for (const b of bytes) {
      binary += String.fromCharCode(b);
    }
    return btoa(binary);
  }
  // Node.js fallback
  return Buffer.from(bytes).toString("base64");
}

function normalizeToBytes(input: Uint8Array | string, encoding: "hex" | "base64" | "utf8" = "hex"): Uint8Array {
  if (input instanceof Uint8Array) return input;
  if (encoding === "hex") return hexToBytes(input);
  if (encoding === "base64") return base64ToBytes(input);
  return new TextEncoder().encode(input);
}

/* ───────────────────────── ed25519 Verification ───────────────────────── */

// Lazy-loaded ed25519 module
let ed25519Module: typeof import("@noble/ed25519") | null = null;
let sha512SyncFn: ((msg: Uint8Array) => Uint8Array) | null = null;
let sha512AsyncFn: ((msg: Uint8Array) => Promise<Uint8Array>) | null = null;

async function getEd25519(): Promise<typeof import("@noble/ed25519")> {
  if (ed25519Module) return ed25519Module;
  
  try {
    // Dynamic import for browser/node compatibility
    ed25519Module = await import("@noble/ed25519");
    if (!sha512SyncFn || !sha512AsyncFn) {
      const { sha512 } = await import("@noble/hashes/sha512");
      sha512SyncFn = (msg: Uint8Array) => sha512(msg);
      sha512AsyncFn = async (msg: Uint8Array) => sha512(msg);
    }
    if (sha512SyncFn) {
      // Ensure noble has SHA-512 configured for browsers
      (ed25519Module.utils as { sha512Sync?: (msg: Uint8Array) => Uint8Array }).sha512Sync = sha512SyncFn;
      (ed25519Module.utils as { sha512Async?: (msg: Uint8Array) => Promise<Uint8Array> }).sha512Async = sha512AsyncFn ?? (async (msg) => sha512SyncFn!(msg));
    }
    return ed25519Module;
  } catch (error) {
    throw new Error("@noble/ed25519 not available. Install with: npm install @noble/ed25519");
  }
}

/**
 * Verify an ed25519 signature
 */
export async function verifyEd25519(
  message: Uint8Array,
  signature: Uint8Array,
  publicKey: Uint8Array
): Promise<boolean> {
  try {
    const ed = await getEd25519();
    return ed.verify(signature, message, publicKey);
  } catch (error) {
    console.error("[signatures] ed25519 verify error:", error);
    return false;
  }
}

/**
 * Sign a message with ed25519 (for testing/dev)
 */
export async function signEd25519(
  message: Uint8Array,
  privateKey: Uint8Array
): Promise<Uint8Array> {
  const ed = await getEd25519();
  return ed.sign(message, privateKey);
}

/**
 * Generate ed25519 keypair (for testing/dev)
 */
export async function generateEd25519Keypair(): Promise<{ publicKey: Uint8Array; privateKey: Uint8Array }> {
  const ed = await getEd25519();
  const privateKey = ed.utils.randomPrivateKey();
  const publicKey = await ed.getPublicKey(privateKey);
  return { publicKey, privateKey };
}

/* ───────────────────────── Combined Verifier ───────────────────────── */

/**
 * Options for creating a signature verifier
 */
export interface SignatureVerifierOptions {
  /** External PQ verifier function (for Dilithium) */
  pqVerifier?: PqSignatureVerifierFunc;
  /** Allow unverified signatures in dev mode */
  devMode?: boolean;
}

/**
 * Create a combined signature verifier supporting both ed25519 and PQ algorithms
 */
export function createSignatureVerifier(options: SignatureVerifierOptions = {}): SignatureVerifier {
  const { pqVerifier, devMode = false } = options;
  
  return {
    async verify(params: SignedMessage): Promise<boolean> {
      const { message, signature, publicKey, algorithm } = params;
      
      // Normalize inputs
      const messageBytes = normalizeToBytes(message, typeof message === "string" ? "hex" : "hex");
      
      if (algorithm === "ed25519") {
        const sigBytes = normalizeToBytes(signature, typeof signature === "string" ? "base64" : "hex");
        const pubBytes = normalizeToBytes(publicKey, typeof publicKey === "string" ? "hex" : "hex");
        return verifyEd25519(messageBytes, sigBytes, pubBytes);
      }
      
      if (algorithm === "dilithium3" || algorithm === "dilithium5") {
        if (!pqVerifier) {
          if (devMode) {
            console.warn("[signatures] PQ verifier not available, skipping verification (dev mode)");
            return true;
          }
          console.error("[signatures] PQ verifier not configured for", algorithm);
          return false;
        }
        
        const messageHex = bytesToHex(messageBytes);
        const sigBase64 = typeof signature === "string" ? signature : bytesToBase64(signature as Uint8Array);
        const pubHex = typeof publicKey === "string" ? publicKey : bytesToHex(publicKey as Uint8Array);
        
        return pqVerifier(messageHex, sigBase64, pubHex, algorithm);
      }
      
      console.error("[signatures] Unknown algorithm:", algorithm);
      return false;
    },
    
    supportedAlgorithms(): SignatureAlgorithm[] {
      const algorithms: SignatureAlgorithm[] = ["ed25519"];
      if (pqVerifier) {
        algorithms.push("dilithium3", "dilithium5");
      }
      return algorithms;
    },
  };
}

/* ───────────────────────── Transaction Signing ───────────────────────── */

/**
 * Create a canonical message for transaction signing
 */
export function createTransactionMessage(tx: {
  contractId: string;
  callerDid: string;
  input: Uint8Array;
  value: bigint;
  nonce?: number;
  timestamp?: number;
}): Uint8Array {
  // Canonical format: contractId|callerDid|inputHex|value|nonce|timestamp
  const inputHex = bytesToHex(tx.input);
  const canonical = `${tx.contractId}|${tx.callerDid}|${inputHex}|${tx.value.toString()}|${tx.nonce ?? 0}|${tx.timestamp ?? Date.now()}`;
  return new TextEncoder().encode(canonical);
}

/**
 * Hash a transaction message for signing
 */
export async function hashTransactionMessage(message: Uint8Array): Promise<Uint8Array> {
  // Create a copy to ensure ArrayBuffer (not SharedArrayBuffer)
  const messageCopy = new Uint8Array(message);
  const hashBuffer = await crypto.subtle.digest("SHA-256", messageCopy);
  return new Uint8Array(hashBuffer);
}

/**
 * Sign a transaction (for wallet/client use)
 */
export async function signTransaction(
  tx: {
    contractId: string;
    callerDid: string;
    input: Uint8Array;
    value: bigint;
    nonce?: number;
    timestamp?: number;
  },
  privateKey: Uint8Array,
  algorithm: SignatureAlgorithm = "ed25519"
): Promise<TransactionSignature> {
  const message = createTransactionMessage(tx);
  const messageHash = await hashTransactionMessage(message);
  
  if (algorithm === "ed25519") {
    const ed = await getEd25519();
    const signature = await ed.sign(messageHash, privateKey);
    const publicKey = await ed.getPublicKey(privateKey);
    
    return {
      signatureBase64: bytesToBase64(signature),
      publicKeyHex: bytesToHex(publicKey),
      algorithm: "ed25519",
    };
  }
  
  throw new Error(`Signing with ${algorithm} not implemented (use external signer)`);
}

/**
 * Verify a transaction signature
 */
export async function verifyTransactionSignature(
  tx: {
    contractId: string;
    callerDid: string;
    input: Uint8Array;
    value: bigint;
    nonce?: number;
    timestamp?: number;
  },
  signature: TransactionSignature,
  verifier: SignatureVerifier
): Promise<boolean> {
  const message = createTransactionMessage(tx);
  const messageHash = await hashTransactionMessage(message);
  
  return verifier.verify({
    message: messageHash,
    signature: signature.signatureBase64,
    publicKey: signature.publicKeyHex,
    algorithm: signature.algorithm,
  });
}

export default {
  verifyEd25519,
  signEd25519,
  generateEd25519Keypair,
  createSignatureVerifier,
  createTransactionMessage,
  hashTransactionMessage,
  signTransaction,
  verifyTransactionSignature,
  bytesToHex,
  hexToBytes,
  base64ToBytes,
  bytesToBase64,
};
