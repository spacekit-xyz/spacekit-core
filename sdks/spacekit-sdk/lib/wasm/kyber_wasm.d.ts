/* tslint:disable */
/* eslint-disable */

/**
 * Get ciphertext size for algorithm
 */
export function kyber_ciphertext_size(algorithm: string): number;

/**
 * Decrypt data using Kyber KEM + AES-256-GCM
 *
 * # Arguments
 * * `algorithm` - "kyber512", "kyber768", or "kyber1024"
 * * `secret_key_base64` - Recipient's secret key (base64)
 * * `kem_ciphertext_base64` - KEM ciphertext from encryption (base64)
 * * `nonce_base64` - AES-GCM nonce from encryption (base64)
 * * `ciphertext_base64` - AES-GCM ciphertext (base64)
 *
 * # Returns
 * Decrypted plaintext as base64, or null on error
 */
export function kyber_decrypt(algorithm: string, secret_key_base64: string, kem_ciphertext_base64: string, nonce_base64: string, ciphertext_base64: string): any;

/**
 * Encrypt data using Kyber KEM + AES-256-GCM
 *
 * # Arguments
 * * `algorithm` - "kyber512", "kyber768", or "kyber1024"
 * * `public_key_base64` - Recipient's public key (base64)
 * * `plaintext_base64` - Data to encrypt (base64)
 *
 * # Returns
 * JSON object with kemCiphertextBase64, nonceBase64, ciphertextBase64, algorithm
 */
export function kyber_encrypt(algorithm: string, public_key_base64: string, plaintext_base64: string): any;

/**
 * Generate a Kyber keypair
 *
 * # Arguments
 * * `algorithm` - "kyber512", "kyber768", or "kyber1024"
 *
 * # Returns
 * JSON object with publicKeyBase64, secretKeyBase64, algorithm
 */
export function kyber_keypair(algorithm: string): any;

/**
 * Get public key size for algorithm
 */
export function kyber_public_key_size(algorithm: string): number;

/**
 * Get secret key size for algorithm
 */
export function kyber_secret_key_size(algorithm: string): number;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly kyber_keypair: (a: number, b: number) => any;
    readonly kyber_encrypt: (a: number, b: number, c: number, d: number, e: number, f: number) => any;
    readonly kyber_decrypt: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number, j: number) => any;
    readonly kyber_public_key_size: (a: number, b: number) => number;
    readonly kyber_secret_key_size: (a: number, b: number) => number;
    readonly kyber_ciphertext_size: (a: number, b: number) => number;
    readonly PQCRYPTO_RUST_randombytes: (a: number, b: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
