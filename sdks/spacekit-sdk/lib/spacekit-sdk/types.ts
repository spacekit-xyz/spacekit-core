/**
 * SpaceKit SDK Types
 */

/* ───────────────────────── LLM ───────────────────────── */

export type {
  LlmAdapter,
  LlmStatus,
  LlmChatEngine,
  LlmChatMessage,
} from "@spacekit/spacekit-js";

/* ───────────────────────── Identity ───────────────────────── */

export interface Identity {
  /** Decentralized Identifier (DID) */
  did: string;
  /** Display name */
  name: string;
  /** Whether identity is initialized */
  isInitialized: boolean;
}

/* ───────────────────────── Crypto ───────────────────────── */

export interface Crypto {
  /** Generate a safe UUID */
  safeUUID: () => string;
}

/* ───────────────────────── Balance ───────────────────────── */

export interface Balance {
  /** Raw balance in smallest unit (wei-like) */
  raw: bigint;
  /** Formatted balance string (e.g., "1,000,000") */
  formatted: string;
  /** Balance in micro-ASTRA */
  microAstra: string;
  /** Whether balance is loading */
  isLoading: boolean;
}

/* ───────────────────────── Blockchain ───────────────────────── */

export interface Transaction {
  id: string;
  contractId: string;
  callerDid: string;
  input: Uint8Array;
  value: bigint;
  timestamp: number;
  nonce?: number;
}

export interface Receipt {
  txId: string;
  contractId: string;
  status: number;
  result: Uint8Array;
  events: Array<{ type: string; data: Uint8Array }>;
  timestamp: number;
  gasUsed?: number;
  receiptHash: string;
}

export interface Block {
  height: number;
  prevHash: string;
  blockHash: string;
  stateRoot: string;
  txRoot: string;
  receiptRoot: string;
  timestamp: number;
  transactions: Transaction[];
  receipts: Receipt[];
}

export interface ExplorerState {
  /** All blocks (newest first) */
  blocks: Block[];
  /** Recent transactions */
  transactions: Transaction[];
  /** Recent receipts */
  receipts: Receipt[];
  /** Current chain height */
  chainHeight: number;
  /** Total transaction count */
  txCount: number;
  /** Whether explorer is loading */
  isLoading: boolean;
  /** Refresh explorer data */
  refresh: () => void;
}

/* ───────────────────────── VM ───────────────────────── */

export interface DeployedContract {
  id: string;
  name: string;
  deployedAt: number;
}

export interface VmState {
  /** Whether VM is initialized */
  isReady: boolean;
  /** Whether VM is currently processing */
  isProcessing: boolean;
  /** Deployed contracts */
  contracts: DeployedContract[];
  /** Deploy a contract from WASM bytes */
  deployContract: (wasm: ArrayBuffer | Response, name: string) => Promise<string>;
  /** Execute a transaction */
  executeTransaction: (
    contractId: string,
    input: Uint8Array,
    value?: bigint
  ) => Promise<Receipt>;
  /** Submit and mine a transaction in one call */
  submitAndMine: (
    contractId: string,
    input: Uint8Array,
    label?: string,
    value?: bigint
  ) => Promise<{ tx: Transaction; receipt: Receipt; block: Block } | null>;
  /** Initialize or reset the VM */
  initialize: (options?: VmInitOptions) => Promise<void>;
}

export interface VmInitOptions {
  /** Storage mode: 'memory' | 'indexeddb' */
  storageMode?: 'memory' | 'indexeddb';
  /** Enable gas metering */
  enableMetering?: boolean;
  /** Gas limit per transaction */
  gasLimit?: number;
  /** Preserve existing explorer data */
  preserveExplorer?: boolean;
}

/* ───────────────────────── Keys ───────────────────────── */

export interface KyberKeyPair {
  publicKey: string;
  secretKey: string;
  algorithm: string;
  keyId: string;
  createdAt: number;
}

export interface KeysState {
  /** Whether Kyber WASM is ready */
  isReady: boolean;
  /** Current Kyber key pair */
  kyberKeys: KyberKeyPair | null;
  /** Whether keys exist */
  hasKeys: boolean;
  /** Generate new Kyber keys */
  generateKeys: () => Promise<KyberKeyPair>;
  /** Encrypt data with Kyber */
  encrypt: (data: Uint8Array) => Promise<string>;
  /** Decrypt data with Kyber */
  decrypt: (encrypted: string) => Promise<Uint8Array>;
}

/* ───────────────────────── Events ───────────────────────── */

export type SpacekitEventType =
  | 'identity-change'
  | 'balance-change'
  | 'block-mined'
  | 'transaction-submitted'
  | 'contract-deployed'
  | 'keys-change'
  | 'vm-ready'
  | 'error';

export interface SpacekitEvent {
  type: SpacekitEventType;
  data?: unknown;
  timestamp: number;
}

export type SpacekitEventHandler = (event: SpacekitEvent) => void;
