/**
 * SpaceKit SDK - Complete SDK for SpaceKit-JS decentralized applications
 * 
 * @example
 * ```ts
 * // Client-only usage (no React)
 * import { SpacekitClient } from '@spacekit/sdk';
 * 
 * SpacekitClient.init();
 * const did = SpacekitClient.setIdentity('Alice');
 * const balance = SpacekitClient.getBalance();
 * ```
 * 
 * @example
 * ```tsx
 * // React usage
 * import { SpacekitProvider, useSpacekit } from '@spacekit/sdk/react';
 * 
 * function App() {
 *   return (
 *     <SpacekitProvider>
 *       <MyApp />
 *     </SpacekitProvider>
 *   );
 * }
 * ```
 * 
 * @example
 * ```ts
 * // Kyber encryption
 * import { initKyber, generateKyberKeypair, kyberEncrypt, kyberDecrypt } from '@spacekit/sdk/kyber';
 * 
 * await initKyber();
 * const keypair = generateKyberKeypair();
 * const encrypted = kyberEncrypt(data, keypair.publicKey);
 * const decrypted = kyberDecrypt(encrypted, keypair.secretKey);
 * ```
 */

// Core client export
export { SpacekitClient } from './SpacekitClient';
export type {
  PersistedTransaction,
  PersistedReceipt,
  PersistedBlock,
  ExplorerSnapshot,
  KyberKeyPair,
  SyncMessage,
  ClientEventType,
  ClientEvent,
} from './SpacekitClient';

// Crypto utilities
export { safeUUID } from './crypto';

// Kyber encryption exports
export {
  initKyber,
  isKyberAvailable,
  isKyberInitialized,
  generateKyberKeypair,
  encryptWithKyber,
  decryptWithKyber,
  serializeEncryptedData,
  deserializeEncryptedData,
  getKyberKeySizes,
  type KyberKeypair,
  type EncryptedData,
  type KyberAlgorithm,
} from './kyber';

// Download utilities
export {
  detectOS,
  getPrimaryDownload,
  DOWNLOAD_URLS,
  type DetectedOS,
} from './downloads';

// Error classes
export {
  SpacekitError,
  ValidationError,
  NetworkError,
  VmError,
  CryptoError,
  StorageError,
  isSpacekitError,
  isValidationError,
  isNetworkError,
  isVmError,
  isCryptoError,
  isStorageError,
} from './errors';

// Validation utilities
export {
  validateDid,
  validateAmount,
  validateContractId,
  validateHex,
  validateBase64,
  validatePublicKey,
  validateInputBytes,
  validateOptional,
  validateNonce,
  validateTimestamp,
} from './validation';

// Encoding utilities for WASM contract calls
export {
  encodeU16,
  decodeU16,
  encodeU32,
  decodeU32,
  encodeU64,
  decodeU64,
  encodeString,
  decodeString,
  encodeBytes,
  decodeBytes,
  concatBytes,
  toHex,
  fromHex,
  toBase64,
  fromBase64,
  didToAddress,
  bytesEqual,
  safeSlice,
} from './encoding';

// Token adapters
export {
  Erc20Token,
  Erc721Token,
  setDefaultNetwork,
  getDefaultNetwork,
  normalizeDid,
  type DidNetwork,
  type Erc20Config,
  type Erc20Metadata,
  type Erc721Config,
  type Erc721Metadata,
  type NftInfo,
} from './tokens';

// Note: React SDK is available via '@spacekit/sdk/react' to avoid requiring React as a dependency
// for consumers that only need the core SDK functionality.
