/**
 * Kyber (ML-KEM) Post-Quantum Encryption Module
 * 
 * Provides:
 * - Keypair generation
 * - Hybrid encryption (Kyber KEM + AES-256-GCM)
 * - Decryption
 * 
 * Uses the wasm-kyber module compiled from spacekit-primitives.
 */

// WASM bindings (static import — blob iframes cannot resolve dynamic `./kyber_wasm-*.js` chunks)
import initKyberWasm, * as kyberWasmBindings from "./wasm/kyber_wasm.js";

interface KyberKeypairResult {
  publicKeyBase64: string;
  secretKeyBase64: string;
  algorithm: string;
}

interface KyberEncryptResult {
  kemCiphertextBase64: string;
  nonceBase64: string;
  ciphertextBase64: string;
  algorithm: string;
}

// WASM module reference
let kyberModule: {
  kyber_keypair: (algorithm: string) => KyberKeypairResult | null;
  kyber_encrypt: (algorithm: string, publicKeyBase64: string, plaintextBase64: string) => KyberEncryptResult | null;
  kyber_decrypt: (algorithm: string, secretKeyBase64: string, kemCiphertextBase64: string, nonceBase64: string, ciphertextBase64: string) => string | null;
  kyber_public_key_size: (algorithm: string) => number;
  kyber_secret_key_size: (algorithm: string) => number;
  kyber_ciphertext_size: (algorithm: string) => number;
} | null = null;

let initPromise: Promise<void> | null = null;

/**
 * Initialize the Kyber WASM module
 * @param wasmInput - Optional WASM source:
 *  - **Uint8Array** — raw module bytes (embedded `.spkg` apps, tests)
 *  - **string URL/path** — defaults to `/wasm/kyber_wasm_bg.wasm` in the browser; in Node, existing
 *    file paths are read from disk (global `fetch(filePath)` does not accept bare POSIX paths).
 */
export async function initKyber(wasmInput?: string | Uint8Array): Promise<void> {
  if (kyberModule) return;
  if (initPromise) return initPromise;

  initPromise = (async () => {
    try {
      console.log("[Kyber] Loading WASM module...");
      console.log("[Kyber] JS bindings loaded, initializing WASM...");

      /** What wasm-bindgen's `default()` accepts: URL string, fetch input, or raw bytes. */
      let initInput: string | Uint8Array;
      if (wasmInput instanceof Uint8Array) {
        initInput = wasmInput;
        console.log("[Kyber] Using inlined WASM bytes");
      } else {
        const resolvedPath = wasmInput ?? "/wasm/kyber_wasm_bg.wasm";
        initInput = resolvedPath;
        if (
          wasmInput &&
          typeof process !== "undefined" &&
          process.versions?.node != null
        ) {
          try {
            const { existsSync, readFileSync } = await import("node:fs");
            if (existsSync(wasmInput)) {
              initInput = new Uint8Array(readFileSync(wasmInput));
              console.log("[Kyber] Using WASM bytes from filesystem (Node)");
            }
          } catch {
            /* ignore — e.g. bundler shim without node:fs */
          }
        }
      }

      await initKyberWasm(initInput);
      kyberModule = kyberWasmBindings;
      console.log("[Kyber] WASM module initialized successfully");
    } catch (error) {
      console.error("[Kyber] Failed to initialize:", error);
      // Reset initPromise so subsequent calls can retry
      initPromise = null;
      throw error;
    }
  })();

  return initPromise;
}

/**
 * Check if Kyber is initialized
 */
export function isKyberInitialized(): boolean {
  return kyberModule !== null;
}

/**
 * Ensure the module is initialized
 */
async function ensureInit(): Promise<void> {
  if (!kyberModule) {
    await initKyber();
  }
  if (!kyberModule) {
    throw new Error("Kyber module not initialized");
  }
}

/**
 * Convert Uint8Array to base64 without stack overflow
 * (avoids String.fromCharCode(...array) which fails on large arrays)
 */
function uint8ArrayToBase64(bytes: Uint8Array): string {
  const CHUNK_SIZE = 0x8000; // 32KB chunks
  const chunks: string[] = [];
  for (let i = 0; i < bytes.length; i += CHUNK_SIZE) {
    const chunk = bytes.subarray(i, i + CHUNK_SIZE);
    chunks.push(String.fromCharCode.apply(null, chunk as unknown as number[]));
  }
  return btoa(chunks.join(""));
}

/**
 * Convert base64 to Uint8Array
 */
function base64ToUint8Array(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/**
 * Supported Kyber algorithms
 */
export type KyberAlgorithm = "kyber512" | "kyber768" | "kyber1024";

/**
 * Kyber keypair
 */
export interface KyberKeypair {
  publicKey: string; // base64
  secretKey: string; // base64
  algorithm: KyberAlgorithm;
}

/**
 * Generate a new Kyber keypair
 */
export async function generateKyberKeypair(algorithm: KyberAlgorithm = "kyber1024"): Promise<KyberKeypair> {
  await ensureInit();
  
  const result = kyberModule!.kyber_keypair(algorithm);
  if (!result) {
    throw new Error("Failed to generate Kyber keypair");
  }
  
  return {
    publicKey: result.publicKeyBase64,
    secretKey: result.secretKeyBase64,
    algorithm: result.algorithm as KyberAlgorithm,
  };
}

/**
 * Encrypted data structure
 */
export interface EncryptedData {
  kemCiphertext: string; // base64 - KEM ciphertext
  nonce: string;         // base64 - AES-GCM nonce
  ciphertext: string;    // base64 - AES-GCM encrypted data
  algorithm: KyberAlgorithm;
}

/**
 * Encrypt data using Kyber KEM + AES-256-GCM
 * 
 * @param publicKey - Recipient's public key (base64)
 * @param data - Data to encrypt (Uint8Array)
 * @param algorithm - Kyber algorithm to use
 */
export async function encryptWithKyber(
  publicKey: string,
  data: Uint8Array,
  algorithm: KyberAlgorithm = "kyber1024"
): Promise<EncryptedData> {
  await ensureInit();
  
  // Convert data to base64 using chunked approach (avoids stack overflow)
  const plaintextBase64 = uint8ArrayToBase64(data);
  
  const result = kyberModule!.kyber_encrypt(algorithm, publicKey, plaintextBase64);
  if (!result) {
    throw new Error("Failed to encrypt with Kyber");
  }
  
  return {
    kemCiphertext: result.kemCiphertextBase64,
    nonce: result.nonceBase64,
    ciphertext: result.ciphertextBase64,
    algorithm: result.algorithm as KyberAlgorithm,
  };
}

/**
 * Decrypt data using Kyber KEM + AES-256-GCM
 * 
 * @param secretKey - Recipient's secret key (base64)
 * @param encrypted - Encrypted data structure
 */
export async function decryptWithKyber(
  secretKey: string,
  encrypted: EncryptedData
): Promise<Uint8Array> {
  await ensureInit();
  
  const plaintextBase64 = kyberModule!.kyber_decrypt(
    encrypted.algorithm,
    secretKey,
    encrypted.kemCiphertext,
    encrypted.nonce,
    encrypted.ciphertext
  );
  
  if (!plaintextBase64) {
    throw new Error("Failed to decrypt with Kyber");
  }
  
  // Convert base64 back to Uint8Array
  return base64ToUint8Array(plaintextBase64);
}

/**
 * Serialize encrypted data to a single base64 string for storage
 */
export function serializeEncryptedData(encrypted: EncryptedData): string {
  const json = JSON.stringify({
    k: encrypted.kemCiphertext,
    n: encrypted.nonce,
    c: encrypted.ciphertext,
    a: encrypted.algorithm,
  });
  return btoa(json);
}

/**
 * Deserialize encrypted data from a base64 string
 */
export function deserializeEncryptedData(serialized: string): EncryptedData {
  const json = atob(serialized);
  const obj = JSON.parse(json) as { k: string; n: string; c: string; a: string };
  return {
    kemCiphertext: obj.k,
    nonce: obj.n,
    ciphertext: obj.c,
    algorithm: obj.a as KyberAlgorithm,
  };
}

/**
 * Check if Kyber module is available
 */
export function isKyberAvailable(): boolean {
  return kyberModule !== null;
}

/**
 * Get key sizes for an algorithm
 */
export async function getKyberKeySizes(algorithm: KyberAlgorithm = "kyber1024"): Promise<{
  publicKeySize: number;
  secretKeySize: number;
  ciphertextSize: number;
}> {
  await ensureInit();
  
  return {
    publicKeySize: kyberModule!.kyber_public_key_size(algorithm),
    secretKeySize: kyberModule!.kyber_secret_key_size(algorithm),
    ciphertextSize: kyberModule!.kyber_ciphertext_size(algorithm),
  };
}
