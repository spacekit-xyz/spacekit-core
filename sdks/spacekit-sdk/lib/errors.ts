/**
 * SpaceKit SDK Error Classes
 * 
 * Provides typed error classes for better error handling and debugging.
 * 
 * @example
 * ```ts
 * import { SpacekitError, ValidationError, NetworkError } from '@spacekit/sdk';
 * 
 * try {
 *   await token.mint('invalid-did', 100n);
 * } catch (error) {
 *   if (error instanceof ValidationError) {
 *     console.log('Invalid input:', error.field, error.message);
 *   } else if (error instanceof NetworkError) {
 *     console.log('Network issue:', error.statusCode, error.message);
 *   }
 * }
 * ```
 */

/**
 * Base error class for all SpaceKit SDK errors
 */
export class SpacekitError extends Error {
  /** Error code for programmatic handling */
  readonly code: string;
  /** Additional context/details */
  readonly details?: Record<string, unknown>;
  /** Timestamp when error occurred */
  readonly timestamp: number;

  constructor(message: string, code: string, details?: Record<string, unknown>) {
    super(message);
    this.name = 'SpacekitError';
    this.code = code;
    this.details = details;
    this.timestamp = Date.now();
    
    // Maintains proper stack trace for where error was thrown (V8 engines)
    const ErrorWithCapture = Error as typeof Error & { 
      captureStackTrace?: (target: object, constructor?: Function) => void 
    };
    if (ErrorWithCapture.captureStackTrace) {
      ErrorWithCapture.captureStackTrace(this, SpacekitError);
    }
  }

  toJSON(): Record<string, unknown> {
    return {
      name: this.name,
      message: this.message,
      code: this.code,
      details: this.details,
      timestamp: this.timestamp,
      stack: this.stack,
    };
  }
}

/**
 * Error thrown when input validation fails
 */
export class ValidationError extends SpacekitError {
  /** The field that failed validation */
  readonly field?: string;
  /** The invalid value (if safe to include) */
  readonly value?: unknown;
  /** Expected format or constraint */
  readonly expected?: string;

  constructor(
    message: string,
    options?: {
      field?: string;
      value?: unknown;
      expected?: string;
      details?: Record<string, unknown>;
    }
  ) {
    super(message, 'VALIDATION_ERROR', options?.details);
    this.name = 'ValidationError';
    this.field = options?.field;
    this.value = options?.value;
    this.expected = options?.expected;
  }

  static invalidDid(did: string): ValidationError {
    return new ValidationError(
      `Invalid DID format: "${did}"`,
      {
        field: 'did',
        value: did,
        expected: 'did:spacekit:<network>:<name> or short name',
      }
    );
  }

  static invalidAmount(amount: unknown): ValidationError {
    return new ValidationError(
      `Invalid amount: ${amount}`,
      {
        field: 'amount',
        value: amount,
        expected: 'positive bigint or number',
      }
    );
  }

  static missingField(field: string): ValidationError {
    return new ValidationError(
      `Missing required field: ${field}`,
      { field, expected: 'non-empty value' }
    );
  }

  static invalidContractId(contractId: string): ValidationError {
    return new ValidationError(
      `Invalid contract ID: "${contractId}"`,
      {
        field: 'contractId',
        value: contractId,
        expected: 'non-empty string identifier',
      }
    );
  }
}

/**
 * Error thrown when a network request fails
 */
export class NetworkError extends SpacekitError {
  /** HTTP status code (if applicable) */
  readonly statusCode?: number;
  /** Request URL */
  readonly url?: string;
  /** Whether the error is retryable */
  readonly retryable: boolean;

  constructor(
    message: string,
    options?: {
      statusCode?: number;
      url?: string;
      retryable?: boolean;
      details?: Record<string, unknown>;
    }
  ) {
    super(message, 'NETWORK_ERROR', options?.details);
    this.name = 'NetworkError';
    this.statusCode = options?.statusCode;
    this.url = options?.url;
    this.retryable = options?.retryable ?? false;
  }

  static timeout(url: string, timeoutMs: number): NetworkError {
    return new NetworkError(
      `Request timed out after ${timeoutMs}ms`,
      { url, retryable: true, details: { timeoutMs } }
    );
  }

  static connectionFailed(url: string, reason?: string): NetworkError {
    return new NetworkError(
      `Connection failed${reason ? `: ${reason}` : ''}`,
      { url, retryable: true }
    );
  }

  static httpError(url: string, statusCode: number, statusText?: string): NetworkError {
    const retryable = statusCode >= 500 || statusCode === 429;
    return new NetworkError(
      `HTTP ${statusCode}${statusText ? ` ${statusText}` : ''}`,
      { url, statusCode, retryable }
    );
  }
}

/**
 * Error thrown when VM operations fail
 */
export class VmError extends SpacekitError {
  /** Contract ID involved */
  readonly contractId?: string;
  /** Transaction ID */
  readonly txId?: string;
  /** Receipt status code */
  readonly status?: number;

  constructor(
    message: string,
    options?: {
      contractId?: string;
      txId?: string;
      status?: number;
      details?: Record<string, unknown>;
    }
  ) {
    super(message, 'VM_ERROR', options?.details);
    this.name = 'VmError';
    this.contractId = options?.contractId;
    this.txId = options?.txId;
    this.status = options?.status;
  }

  static notInitialized(): VmError {
    return new VmError('VM not initialized. Call initialize() first.');
  }

  static contractNotFound(contractId: string): VmError {
    return new VmError(
      `Contract not found: ${contractId}`,
      { contractId }
    );
  }

  static transactionFailed(txId: string, status: number, reason?: string): VmError {
    return new VmError(
      `Transaction failed${reason ? `: ${reason}` : ''}`,
      { txId, status }
    );
  }

  static insufficientBalance(did: string, required: bigint, available: bigint): VmError {
    return new VmError(
      `Insufficient balance for ${did}`,
      { details: { did, required: required.toString(), available: available.toString() } }
    );
  }
}

/**
 * Error thrown when encryption/decryption fails
 */
export class CryptoError extends SpacekitError {
  /** Algorithm involved */
  readonly algorithm?: string;
  /** Operation that failed */
  readonly operation?: 'encrypt' | 'decrypt' | 'sign' | 'verify' | 'keygen';

  constructor(
    message: string,
    options?: {
      algorithm?: string;
      operation?: 'encrypt' | 'decrypt' | 'sign' | 'verify' | 'keygen';
      details?: Record<string, unknown>;
    }
  ) {
    super(message, 'CRYPTO_ERROR', options?.details);
    this.name = 'CryptoError';
    this.algorithm = options?.algorithm;
    this.operation = options?.operation;
  }

  static kyberNotInitialized(): CryptoError {
    return new CryptoError(
      'Kyber not initialized. Call initKyber() first.',
      { algorithm: 'kyber', operation: 'encrypt' }
    );
  }

  static decryptionFailed(reason?: string): CryptoError {
    return new CryptoError(
      `Decryption failed${reason ? `: ${reason}` : ''}`,
      { operation: 'decrypt' }
    );
  }

  static invalidKey(keyType: 'public' | 'private' | 'secret'): CryptoError {
    return new CryptoError(
      `Invalid ${keyType} key`,
      { details: { keyType } }
    );
  }
}

/**
 * Error thrown when storage operations fail
 */
export class StorageError extends SpacekitError {
  /** Storage key involved */
  readonly key?: string;
  /** Operation that failed */
  readonly operation?: 'get' | 'set' | 'delete' | 'sync';

  constructor(
    message: string,
    options?: {
      key?: string;
      operation?: 'get' | 'set' | 'delete' | 'sync';
      details?: Record<string, unknown>;
    }
  ) {
    super(message, 'STORAGE_ERROR', options?.details);
    this.name = 'StorageError';
    this.key = options?.key;
    this.operation = options?.operation;
  }

  static quotaExceeded(): StorageError {
    return new StorageError(
      'Storage quota exceeded',
      { operation: 'set' }
    );
  }

  static notFound(key: string): StorageError {
    return new StorageError(
      `Key not found: ${key}`,
      { key, operation: 'get' }
    );
  }

  static indexedDbUnavailable(): StorageError {
    return new StorageError(
      'IndexedDB not available in this environment',
      { operation: 'sync' }
    );
  }
}

/**
 * Type guard to check if an error is a SpacekitError
 */
export function isSpacekitError(error: unknown): error is SpacekitError {
  return error instanceof SpacekitError;
}

/**
 * Type guard to check if an error is a ValidationError
 */
export function isValidationError(error: unknown): error is ValidationError {
  return error instanceof ValidationError;
}

/**
 * Type guard to check if an error is a NetworkError
 */
export function isNetworkError(error: unknown): error is NetworkError {
  return error instanceof NetworkError;
}

/**
 * Type guard to check if an error is a VmError
 */
export function isVmError(error: unknown): error is VmError {
  return error instanceof VmError;
}

/**
 * Type guard to check if an error is a CryptoError
 */
export function isCryptoError(error: unknown): error is CryptoError {
  return error instanceof CryptoError;
}

/**
 * Type guard to check if an error is a StorageError
 */
export function isStorageError(error: unknown): error is StorageError {
  return error instanceof StorageError;
}
