/**
 * Input Validation Utilities
 * 
 * Provides validation functions for DIDs, balances, contract IDs,
 * and other inputs to ensure type safety and prevent common errors.
 */

import { ValidationError } from './errors';

/**
 * DID format patterns
 */
const DID_PATTERN = /^did:spacekit:[a-z0-9-]+:[a-zA-Z0-9_-]+$/;
const SHORT_NAME_PATTERN = /^[a-zA-Z][a-zA-Z0-9_-]{2,31}$/;
const HEX_PATTERN = /^[0-9a-fA-F]+$/;
const BASE64_PATTERN = /^[A-Za-z0-9+/]*={0,2}$/;

/**
 * Validate a DID string
 * 
 * Valid formats:
 * - Full: did:spacekit:local:alice
 * - Short: alice (must start with letter, 3-32 chars)
 */
export function validateDid(did: unknown, fieldName = 'did'): string {
  if (typeof did !== 'string') {
    throw ValidationError.invalidDid(String(did));
  }
  
  const trimmed = did.trim();
  if (!trimmed) {
    throw ValidationError.missingField(fieldName);
  }
  
  // Full DID format
  if (trimmed.startsWith('did:')) {
    if (!DID_PATTERN.test(trimmed)) {
      throw ValidationError.invalidDid(trimmed);
    }
    return trimmed;
  }
  
  // Short name format
  if (!SHORT_NAME_PATTERN.test(trimmed)) {
    throw new ValidationError(
      `Invalid DID short name: "${trimmed}"`,
      {
        field: fieldName,
        value: trimmed,
        expected: '3-32 alphanumeric characters starting with a letter',
      }
    );
  }
  
  return trimmed;
}

/**
 * Validate and normalize a balance amount
 * 
 * Accepts: number, bigint, or string representation
 * Returns: bigint
 */
export function validateAmount(amount: unknown, fieldName = 'amount'): bigint {
  if (amount === undefined || amount === null) {
    throw ValidationError.missingField(fieldName);
  }
  
  let value: bigint;
  
  if (typeof amount === 'bigint') {
    value = amount;
  } else if (typeof amount === 'number') {
    if (!Number.isFinite(amount) || !Number.isInteger(amount)) {
      throw ValidationError.invalidAmount(amount);
    }
    value = BigInt(amount);
  } else if (typeof amount === 'string') {
    const trimmed = amount.trim();
    if (!/^-?\d+$/.test(trimmed)) {
      throw ValidationError.invalidAmount(amount);
    }
    try {
      value = BigInt(trimmed);
    } catch {
      throw ValidationError.invalidAmount(amount);
    }
  } else {
    throw ValidationError.invalidAmount(amount);
  }
  
  if (value < 0n) {
    throw new ValidationError(
      `Amount cannot be negative: ${value}`,
      { field: fieldName, value: value.toString() }
    );
  }
  
  return value;
}

/**
 * Validate a contract ID
 */
export function validateContractId(contractId: unknown, fieldName = 'contractId'): string {
  if (typeof contractId !== 'string') {
    throw ValidationError.invalidContractId(String(contractId));
  }
  
  const trimmed = contractId.trim();
  if (!trimmed) {
    throw ValidationError.missingField(fieldName);
  }
  
  // Contract IDs should be non-empty alphanumeric with underscores/hyphens
  if (!/^[a-zA-Z][a-zA-Z0-9_-]*$/.test(trimmed)) {
    throw ValidationError.invalidContractId(trimmed);
  }
  
  if (trimmed.length > 64) {
    throw new ValidationError(
      `Contract ID too long: ${trimmed.length} chars (max 64)`,
      { field: fieldName, value: trimmed }
    );
  }
  
  return trimmed;
}

/**
 * Validate a hex string
 */
export function validateHex(hex: unknown, fieldName = 'hex'): string {
  if (typeof hex !== 'string') {
    throw new ValidationError(
      `Expected hex string, got ${typeof hex}`,
      { field: fieldName, value: hex }
    );
  }
  
  const normalized = hex.startsWith('0x') ? hex.slice(2) : hex;
  
  if (!normalized) {
    throw ValidationError.missingField(fieldName);
  }
  
  if (!HEX_PATTERN.test(normalized)) {
    throw new ValidationError(
      'Invalid hex string',
      { field: fieldName, expected: 'hexadecimal characters (0-9, a-f)' }
    );
  }
  
  return normalized;
}

/**
 * Validate a base64 string
 */
export function validateBase64(base64: unknown, fieldName = 'base64'): string {
  if (typeof base64 !== 'string') {
    throw new ValidationError(
      `Expected base64 string, got ${typeof base64}`,
      { field: fieldName, value: base64 }
    );
  }
  
  const trimmed = base64.trim();
  
  if (!trimmed) {
    throw ValidationError.missingField(fieldName);
  }
  
  if (!BASE64_PATTERN.test(trimmed)) {
    throw new ValidationError(
      'Invalid base64 string',
      { field: fieldName, expected: 'valid base64 encoding' }
    );
  }
  
  return trimmed;
}

/**
 * Validate a public key (hex encoded)
 */
export function validatePublicKey(publicKey: unknown, fieldName = 'publicKey'): string {
  const hex = validateHex(publicKey, fieldName);
  
  // Ed25519 public keys are 32 bytes = 64 hex chars
  // Kyber public keys are larger
  if (hex.length < 64) {
    throw new ValidationError(
      `Public key too short: ${hex.length / 2} bytes (min 32)`,
      { field: fieldName }
    );
  }
  
  return hex;
}

/**
 * Validate input bytes for a transaction
 */
export function validateInputBytes(
  input: unknown,
  fieldName = 'input',
  maxBytes = 1024 * 1024 // 1MB default max
): Uint8Array {
  if (input instanceof Uint8Array) {
    if (input.length > maxBytes) {
      throw new ValidationError(
        `Input too large: ${input.length} bytes (max ${maxBytes})`,
        { field: fieldName, details: { size: input.length, max: maxBytes } }
      );
    }
    return input;
  }
  
  if (typeof input === 'string') {
    // Assume hex or base64 encoding
    let bytes: Uint8Array;
    try {
      const hex = validateHex(input, fieldName);
      bytes = new Uint8Array(hex.match(/.{2}/g)!.map(b => parseInt(b, 16)));
    } catch {
      // Try base64
      try {
        const b64 = validateBase64(input, fieldName);
        bytes = Uint8Array.from(atob(b64), c => c.charCodeAt(0));
      } catch {
        throw new ValidationError(
          'Input must be Uint8Array, hex string, or base64 string',
          { field: fieldName }
        );
      }
    }
    
    if (bytes.length > maxBytes) {
      throw new ValidationError(
        `Input too large: ${bytes.length} bytes (max ${maxBytes})`,
        { field: fieldName, details: { size: bytes.length, max: maxBytes } }
      );
    }
    
    return bytes;
  }
  
  throw new ValidationError(
    `Invalid input type: ${typeof input}`,
    { field: fieldName, expected: 'Uint8Array, hex string, or base64 string' }
  );
}

/**
 * Validate an optional field, returning undefined if empty
 */
export function validateOptional<T>(
  value: unknown,
  validator: (v: unknown) => T,
  defaultValue?: T
): T | undefined {
  if (value === undefined || value === null || value === '') {
    return defaultValue;
  }
  return validator(value);
}

/**
 * Validate a nonce (transaction sequence number)
 */
export function validateNonce(nonce: unknown, fieldName = 'nonce'): number {
  if (typeof nonce !== 'number') {
    throw new ValidationError(
      `Nonce must be a number, got ${typeof nonce}`,
      { field: fieldName }
    );
  }
  
  if (!Number.isInteger(nonce) || nonce < 0) {
    throw new ValidationError(
      `Nonce must be a non-negative integer: ${nonce}`,
      { field: fieldName, value: nonce }
    );
  }
  
  return nonce;
}

/**
 * Validate a timestamp
 */
export function validateTimestamp(timestamp: unknown, fieldName = 'timestamp'): number {
  if (typeof timestamp !== 'number') {
    throw new ValidationError(
      `Timestamp must be a number, got ${typeof timestamp}`,
      { field: fieldName }
    );
  }
  
  if (!Number.isInteger(timestamp) || timestamp < 0) {
    throw new ValidationError(
      `Timestamp must be a non-negative integer: ${timestamp}`,
      { field: fieldName, value: timestamp }
    );
  }
  
  // Sanity check: should be a reasonable Unix timestamp (after 2020)
  const year2020 = 1577836800000; // Jan 1, 2020
  const year2100 = 4102444800000; // Jan 1, 2100
  
  if (timestamp < year2020 || timestamp > year2100) {
    throw new ValidationError(
      `Timestamp out of range: ${timestamp}`,
      { field: fieldName, value: timestamp, expected: 'Unix timestamp between 2020 and 2100' }
    );
  }
  
  return timestamp;
}
