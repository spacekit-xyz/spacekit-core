/**
 * SpaceKit SDK - Binary Encoding Utilities
 * 
 * These utilities are used for encoding data for WASM contract calls.
 * All multi-byte integers use little-endian encoding to match the WASM ABI.
 * 
 * @example
 * ```ts
 * import { encodeU64, encodeString, concatBytes } from '@spacekit/sdk/encoding';
 * 
 * // Build an ERC-20 mint call
 * const input = concatBytes([
 *   Uint8Array.of(1), // op: mint
 *   encodeString('did:spacekit:demo:alice'),
 *   encodeU64(1000n),
 * ]);
 * 
 * await vm.submitTransaction('my-token', input, callerDid, 0n);
 * ```
 */

/**
 * Encode a 16-bit unsigned integer (little-endian)
 */
export const encodeU16 = (value: number): Uint8Array => {
  const buffer = new ArrayBuffer(2);
  new DataView(buffer).setUint16(0, value, true);
  return new Uint8Array(buffer);
};

/**
 * Decode a 16-bit unsigned integer (little-endian)
 */
export const decodeU16 = (bytes: Uint8Array, offset = 0): number => {
  return new DataView(bytes.buffer, bytes.byteOffset + offset).getUint16(0, true);
};

/**
 * Encode a 32-bit unsigned integer (little-endian)
 */
export const encodeU32 = (value: number): Uint8Array => {
  const buffer = new ArrayBuffer(4);
  new DataView(buffer).setUint32(0, value, true);
  return new Uint8Array(buffer);
};

/**
 * Decode a 32-bit unsigned integer (little-endian)
 */
export const decodeU32 = (bytes: Uint8Array, offset = 0): number => {
  return new DataView(bytes.buffer, bytes.byteOffset + offset).getUint32(0, true);
};

/**
 * Encode a 64-bit unsigned integer (little-endian)
 */
export const encodeU64 = (value: bigint): Uint8Array => {
  const buffer = new ArrayBuffer(8);
  new DataView(buffer).setBigUint64(0, value, true);
  return new Uint8Array(buffer);
};

/**
 * Decode a 64-bit unsigned integer (little-endian)
 */
export const decodeU64 = (bytes: Uint8Array, offset = 0): bigint => {
  return new DataView(bytes.buffer, bytes.byteOffset + offset).getBigUint64(0, true);
};

/**
 * Encode a string with a 16-bit length prefix
 * Format: [u16 length][utf8 bytes]
 */
export const encodeString = (value: string): Uint8Array => {
  const data = new TextEncoder().encode(value);
  const out = new Uint8Array(2 + data.length);
  out.set(encodeU16(data.length), 0);
  out.set(data, 2);
  return out;
};

/**
 * Decode a length-prefixed string (16-bit length prefix)
 * @returns [decoded string, bytes consumed]
 */
export const decodeString = (bytes: Uint8Array, offset = 0): [string, number] => {
  const length = decodeU16(bytes, offset);
  const data = bytes.slice(offset + 2, offset + 2 + length);
  return [new TextDecoder().decode(data), 2 + length];
};

/**
 * Encode a byte array with a 32-bit length prefix
 * Format: [u32 length][bytes]
 */
export const encodeBytes = (value: Uint8Array): Uint8Array => {
  const out = new Uint8Array(4 + value.length);
  out.set(encodeU32(value.length), 0);
  out.set(value, 4);
  return out;
};

/**
 * Decode a length-prefixed byte array (32-bit length prefix)
 * @returns [decoded bytes, bytes consumed]
 */
export const decodeBytes = (bytes: Uint8Array, offset = 0): [Uint8Array, number] => {
  const length = decodeU32(bytes, offset);
  const data = bytes.slice(offset + 4, offset + 4 + length);
  return [data, 4 + length];
};

/**
 * Concatenate multiple Uint8Arrays into one
 */
export const concatBytes = (parts: Uint8Array[]): Uint8Array => {
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let offset = 0;
  for (const part of parts) {
    out.set(part, offset);
    offset += part.length;
  }
  return out;
};

/**
 * Convert bytes to hexadecimal string
 */
export const toHex = (bytes: Uint8Array): string => {
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
};

/**
 * Convert hexadecimal string to bytes
 */
export const fromHex = (hex: string): Uint8Array => {
  const clean = hex.startsWith("0x") ? hex.slice(2) : hex;
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes;
};

/**
 * Convert bytes to base64 string
 * Works in both browser and Node.js environments
 */
export const toBase64 = (bytes: Uint8Array): string => {
  // Browser environment
  if (typeof btoa === "function") {
    return btoa(String.fromCharCode(...bytes));
  }
  // Node.js environment - use dynamic import pattern
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const nodeBuffer = (globalThis as any).Buffer;
  if (nodeBuffer) {
    return nodeBuffer.from(bytes).toString("base64");
  }
  throw new Error("No base64 encoding available in this environment");
};

/**
 * Convert base64 string to bytes
 * Works in both browser and Node.js environments
 */
export const fromBase64 = (base64: string): Uint8Array => {
  // Browser environment
  if (typeof atob === "function") {
    return Uint8Array.from(atob(base64), (c) => c.charCodeAt(0));
  }
  // Node.js environment - use dynamic import pattern
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const nodeBuffer = (globalThis as any).Buffer;
  if (nodeBuffer) {
    return new Uint8Array(nodeBuffer.from(base64, "base64"));
  }
  throw new Error("No base64 decoding available in this environment");
};

/**
 * Derive an Ethereum-style address from a DID using SHA-256
 * Returns the first 20 bytes of the hash as a 0x-prefixed hex string
 */
export const didToAddress = async (did: string): Promise<string> => {
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) {
    throw new Error("WebCrypto not available for address derivation.");
  }
  const data = new TextEncoder().encode(did);
  const digest = await subtle.digest("SHA-256", data);
  const bytes = new Uint8Array(digest).slice(0, 20);
  return `0x${toHex(bytes)}`;
};

/**
 * Compare two Uint8Arrays for equality
 */
export const bytesEqual = (a: Uint8Array, b: Uint8Array): boolean => {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
};

/**
 * Slice bytes with bounds checking
 */
export const safeSlice = (
  bytes: Uint8Array,
  start: number,
  end?: number
): Uint8Array => {
  const actualEnd = end ?? bytes.length;
  if (start < 0 || start > bytes.length || actualEnd > bytes.length) {
    throw new RangeError(`Slice bounds out of range: [${start}, ${actualEnd}] for length ${bytes.length}`);
  }
  return bytes.slice(start, actualEnd);
};
