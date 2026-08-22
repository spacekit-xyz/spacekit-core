/**
 * SpacekitProvider - React Context Provider for SpaceKit SDK
 * 
 * Provides unified access to:
 * - Identity management
 * - Balance tracking  
 * - VM operations
 * - Block explorer
 * - Encryption keys
 */

import { createContext, useContext, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  SpacekitVm,
  IndexedDbStorageAdapter,
  createInMemoryStorage,
} from '@spacekit/spacekit-js';
import { SpacekitClient, type PersistedBlock } from '../SpacekitClient';
import {
  initKyber,
  generateKyberKeypair,
  encryptWithKyber,
  decryptWithKyber,
  serializeEncryptedData,
  deserializeEncryptedData,
  type KyberKeypair,
} from '../kyber';
import type {
  Identity,
  Balance,
  Block,
  Transaction,
  Receipt,
  ExplorerState,
  VmState,
  KeysState,
  VmInitOptions,
  KyberKeyPair,
  SpacekitEvent,
  SpacekitEventHandler,
} from './types';

/* ───────────────────────── Context Value ───────────────────────── */

export interface SpacekitContextValue {
  /** Identity state and actions */
  identity: Identity & {
    setIdentity: (name: string) => string;
  };
  /** Balance state and actions */
  balance: Balance & {
    refresh: () => void;
    deductFee: (fee: bigint | number) => bigint;
  };
  /** Block explorer state */
  explorer: ExplorerState;
  /** VM state and actions */
  vm: VmState;
  /** Encryption keys state and actions */
  keys: KeysState;
  /** Subscribe to SDK events */
  on: (event: string, handler: SpacekitEventHandler) => () => void;
  /** Emit an SDK event */
  emit: (event: SpacekitEvent) => void;
}

const SpacekitContext = createContext<SpacekitContextValue | null>(null);

/* ───────────────────────── Provider Props ───────────────────────── */

export interface SpacekitProviderProps {
  children: React.ReactNode;
  /** Default identity name (defaults to "Alice") */
  defaultIdentity?: string;
  /** Storage mode for VM (defaults to "indexeddb") */
  storageMode?: 'memory' | 'indexeddb';
  /** Auto-initialize VM on mount */
  autoInitVm?: boolean;
  /** Enable gas metering */
  enableMetering?: boolean;
  /** Gas limit per transaction */
  gasLimit?: number;
}

/* ───────────────────────── Helpers ───────────────────────── */

function formatBalance(raw: bigint): string {
  return raw.toString().replace(/\B(?=(\d{3})+(?!\d))/g, ',');
}

function formatMicroAstra(raw: bigint): string {
  const micro = Number(raw) / 1_000_000;
  return micro.toFixed(6);
}

function hydrateBlock(block: PersistedBlock): Block {
  return {
    height: block.height,
    prevHash: block.prevHash,
    blockHash: block.blockHash,
    stateRoot: block.stateRoot,
    txRoot: block.txRoot,
    receiptRoot: block.receiptRoot,
    timestamp: block.timestamp,
    transactions: block.transactions.map((tx) => ({
      id: tx.id,
      contractId: tx.contractId,
      callerDid: tx.callerDid,
      input: new Uint8Array(tx.input),
      value: BigInt(tx.value),
      timestamp: tx.timestamp,
      nonce: tx.nonce,
    })),
    receipts: block.receipts.map((receipt) => ({
      txId: receipt.txId,
      contractId: receipt.contractId,
      status: receipt.status,
      result: new Uint8Array(receipt.result),
      events: receipt.events.map((e) => ({ type: e.type, data: new Uint8Array(e.data) })),
      timestamp: receipt.timestamp,
      gasUsed: receipt.gasUsed,
      receiptHash: receipt.receiptHash,
    })),
  };
}

/* ───────────────────────── Provider Component ───────────────────────── */

export function SpacekitProvider({
  children,
  defaultIdentity = 'Alice',
  storageMode = 'indexeddb',
  autoInitVm = true,
  enableMetering = false,
  gasLimit = 1_000_000,
}: SpacekitProviderProps) {
  // Refs
  const vmRef = useRef<SpacekitVm | null>(null);
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const storageRef = useRef<any>(null);
  const eventHandlersRef = useRef<Map<string, Set<SpacekitEventHandler>>>(new Map());
  const initializingRef = useRef(false);

  // Identity state
  const [identityDid, setIdentityDid] = useState<string>('');
  const [identityName, setIdentityName] = useState<string>('');
  const [identityInitialized, setIdentityInitialized] = useState(false);

  // Balance state
  const [balanceRaw, setBalanceRaw] = useState<bigint>(0n);
  const [balanceLoading, setBalanceLoading] = useState(false);

  // Explorer state
  const [blocks, setBlocks] = useState<Block[]>([]);
  const [explorerLoading, setExplorerLoading] = useState(false);

  // VM state
  const [vmReady, setVmReady] = useState(false);
  const [vmProcessing, setVmProcessing] = useState(false);
  const [deployedContracts, setDeployedContracts] = useState<Array<{ id: string; name: string; deployedAt: number }>>([]);

  // Keys state
  const [kyberReady, setKyberReady] = useState(false);
  const [kyberKeys, setKyberKeys] = useState<KyberKeypair | null>(null);

  /* ── Event System ── */

  const emit = useCallback((event: SpacekitEvent) => {
    const handlers = eventHandlersRef.current.get(event.type);
    if (handlers) {
      handlers.forEach((handler) => handler(event));
    }
    // Also emit to 'all' subscribers
    const allHandlers = eventHandlersRef.current.get('*');
    if (allHandlers) {
      allHandlers.forEach((handler) => handler(event));
    }
  }, []);

  const on = useCallback((eventType: string, handler: SpacekitEventHandler): (() => void) => {
    if (!eventHandlersRef.current.has(eventType)) {
      eventHandlersRef.current.set(eventType, new Set());
    }
    eventHandlersRef.current.get(eventType)!.add(handler);
    return () => {
      eventHandlersRef.current.get(eventType)?.delete(handler);
    };
  }, []);

  /* ── Identity Actions ── */

  const setIdentity = useCallback((name: string): string => {
    const did = SpacekitClient.setIdentity(name);
    setIdentityDid(did);
    setIdentityName(name);
    setIdentityInitialized(true);
    emit({ type: 'identity-change', data: { did, name }, timestamp: Date.now() });
    return did;
  }, [emit]);

  /* ── Balance Actions ── */

  const refreshBalance = useCallback(() => {
    const did = SpacekitClient.getCurrentDid() || identityDid;
    if (!did) return;
    setBalanceLoading(true);
    const balance = SpacekitClient.ensureBalance(did);
    setBalanceRaw(balance);
    setBalanceLoading(false);
  }, [identityDid]);

  const deductFee = useCallback((fee: bigint | number): bigint => {
    const did = SpacekitClient.getCurrentDid() || identityDid;
    if (!did) return 0n;
    const newBalance = SpacekitClient.deductFee(did, fee);
    setBalanceRaw(newBalance);
    emit({ type: 'balance-change', data: { balance: newBalance }, timestamp: Date.now() });
    return newBalance;
  }, [identityDid, emit]);

  /* ── Explorer Actions ── */

  const refreshExplorer = useCallback(() => {
    setExplorerLoading(true);
    const did = SpacekitClient.getCurrentDid() || identityDid;
    const snapshot = SpacekitClient.getExplorerSnapshot(did || undefined);
    if (snapshot?.blocks) {
      const hydrated = snapshot.blocks.map(hydrateBlock);
      setBlocks(hydrated);
    } else {
      setBlocks([]);
    }
    setExplorerLoading(false);
  }, [identityDid]);

  /* ── VM Actions ── */

  const initializeVm = useCallback(async (options: VmInitOptions = {}) => {
    if (initializingRef.current) return;
    initializingRef.current = true;

    try {
      const mode = options.storageMode ?? storageMode;
      const did = SpacekitClient.getCurrentDid() || identityDid || 'default';
      const dbSuffix = did.replace(/[^a-z0-9_-]/gi, '_');
      const dbName = `spacekit-sdk-${dbSuffix}`;

      let storage: IndexedDbStorageAdapter | ReturnType<typeof createInMemoryStorage>;
      if (mode === 'indexeddb') {
        const indexed = new IndexedDbStorageAdapter(dbName, 'kv');
        await indexed.init();
        storage = indexed;
      } else {
        storage = createInMemoryStorage();
      }
      storageRef.current = storage;

      const vm = new SpacekitVm({
        storage,
        enableWasmMetering: options.enableMetering ?? enableMetering,
        gasPolicy: { gasPerByte: 1, gasLimit: options.gasLimit ?? gasLimit },
      });

      vmRef.current = vm;
      setVmReady(true);
      setDeployedContracts([]);

      emit({ type: 'vm-ready', timestamp: Date.now() });

      if (options.preserveExplorer !== false) {
        refreshExplorer();
      }
    } catch (error) {
      emit({ type: 'error', data: { message: 'VM initialization failed', error }, timestamp: Date.now() });
      throw error;
    } finally {
      initializingRef.current = false;
    }
  }, [storageMode, identityDid, enableMetering, gasLimit, emit, refreshExplorer]);

  const deployContract = useCallback(async (wasm: ArrayBuffer | Response, name: string): Promise<string> => {
    if (!vmRef.current) {
      await initializeVm();
    }
    const vm = vmRef.current!;
    setVmProcessing(true);
    try {
      const contract = await vm.deployContract(wasm, name);
      setDeployedContracts((prev) => [...prev, { id: contract.id, name, deployedAt: Date.now() }]);
      emit({ type: 'contract-deployed', data: { contractId: contract.id, name }, timestamp: Date.now() });
      return contract.id;
    } finally {
      setVmProcessing(false);
    }
  }, [initializeVm, emit]);

  const executeTransaction = useCallback(async (
    contractId: string,
    input: Uint8Array,
    value: bigint = 0n
  ): Promise<Receipt> => {
    const vm = vmRef.current;
    if (!vm) throw new Error('VM not initialized. Call initialize() first.');

    const did = SpacekitClient.getCurrentDid() || identityDid;
    if (!did) throw new Error('No identity set. Call setIdentity() first.');

    setVmProcessing(true);
    try {
      const receipt = await vm.executeTransaction(contractId, input, did, value);
      emit({ type: 'transaction-submitted', data: { contractId, receipt }, timestamp: Date.now() });
      return receipt;
    } finally {
      setVmProcessing(false);
    }
  }, [identityDid, emit]);

  const submitAndMine = useCallback(async (
    contractId: string,
    input: Uint8Array,
    _label?: string,
    value: bigint = 0n
  ): Promise<{ tx: Transaction; receipt: Receipt; block: Block } | null> => {
    const vm = vmRef.current;
    if (!vm) throw new Error('VM not initialized. Call initialize() first.');

    const did = SpacekitClient.getCurrentDid() || identityDid;
    if (!did) throw new Error('No identity set. Call setIdentity() first.');

    setVmProcessing(true);
    try {
      const tx = await vm.submitTransaction(contractId, input, did, value);
      const block = await vm.mineBlock();

      if (!block) {
        return null;
      }

      const receipt = block.receipts.find((r) => r.txId === tx.id);
      if (!receipt) {
        return null;
      }

      // Persist to SpacekitClient
      const serialized = SpacekitClient.serializeBlock(block as Parameters<typeof SpacekitClient.serializeBlock>[0]);
      SpacekitClient.addBlock(did, serialized);

      // Deduct fee
      const fee = receipt.gasUsed ?? input.length * 10;
      deductFee(BigInt(fee));

      emit({ type: 'block-mined', data: { block, tx, receipt }, timestamp: Date.now() });
      refreshExplorer();

      return { tx, receipt, block: block as Block };
    } finally {
      setVmProcessing(false);
    }
  }, [identityDid, emit, deductFee, refreshExplorer]);

  /* ── Keys Actions ── */

  const initializeKyber = useCallback(async () => {
    await initKyber();
    setKyberReady(true);

    // Load existing keys
    const stored = SpacekitClient.getKyberKeys();
    if (stored) {
      setKyberKeys(stored as unknown as KyberKeypair);
    }
  }, []);

  const generateKeys = useCallback(async (): Promise<KyberKeyPair> => {
    if (!kyberReady) {
      await initializeKyber();
    }
    const keypair = await generateKyberKeypair('kyber1024');
    const keyPair: KyberKeyPair = {
      publicKey: keypair.publicKey,
      secretKey: keypair.secretKey,
      algorithm: keypair.algorithm,
      keyId: `kyber-${Date.now()}`,
      createdAt: Date.now(),
    };
    SpacekitClient.setKyberKeys(keyPair);
    setKyberKeys(keypair);
    emit({ type: 'keys-change', data: { keyPair }, timestamp: Date.now() });
    return keyPair;
  }, [kyberReady, initializeKyber, emit]);

  const encrypt = useCallback(async (data: Uint8Array): Promise<string> => {
    if (!kyberKeys) throw new Error('No Kyber keys. Call generateKeys() first.');
    const encrypted = await encryptWithKyber(kyberKeys.publicKey, data);
    return serializeEncryptedData(encrypted);
  }, [kyberKeys]);

  const decrypt = useCallback(async (encrypted: string): Promise<Uint8Array> => {
    if (!kyberKeys) throw new Error('No Kyber keys. Call generateKeys() first.');
    const parsed = deserializeEncryptedData(encrypted);
    return decryptWithKyber(kyberKeys.secretKey, parsed);
  }, [kyberKeys]);

  /* ── Initialize on Mount ── */

  useEffect(() => {
    SpacekitClient.init();

    // Load identity
    const did = SpacekitClient.getIdentityDid();
    const name = SpacekitClient.getIdentityName();
    if (did && name) {
      setIdentityDid(did);
      setIdentityName(name);
      setIdentityInitialized(true);
    } else {
      setIdentity(defaultIdentity);
    }

    // Load balance
    refreshBalance();

    // Load explorer
    refreshExplorer();

    // Initialize Kyber
    void initializeKyber();

    // Auto-init VM
    if (autoInitVm) {
      void initializeVm();
    }

    // Subscribe to cross-iframe updates
    const unsubscribe = SpacekitClient.subscribe((msg) => {
      if (msg.type === 'identity' && msg.did) {
        setIdentityDid(msg.did);
        setIdentityName(SpacekitClient.getIdentityName() || '');
      }
      if (msg.type === 'balance') {
        refreshBalance();
      }
      if (msg.type === 'block' || msg.type === 'refresh') {
        refreshExplorer();
      }
    });

    return () => {
      unsubscribe();
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  /* ── Memoized Context Value ── */

  const transactions = useMemo(() => {
    return blocks.flatMap((b) => b.transactions).slice(0, 20);
  }, [blocks]);

  const receipts = useMemo(() => {
    return blocks.flatMap((b) => b.receipts).slice(0, 20);
  }, [blocks]);

  const contextValue = useMemo<SpacekitContextValue>(() => ({
    identity: {
      did: identityDid,
      name: identityName,
      isInitialized: identityInitialized,
      setIdentity,
    },
    balance: {
      raw: balanceRaw,
      formatted: formatBalance(balanceRaw),
      microAstra: formatMicroAstra(balanceRaw),
      isLoading: balanceLoading,
      refresh: refreshBalance,
      deductFee,
    },
    explorer: {
      blocks,
      transactions,
      receipts,
      chainHeight: blocks.length > 0 ? blocks[0].height : 0,
      txCount: transactions.length,
      isLoading: explorerLoading,
      refresh: refreshExplorer,
    },
    vm: {
      isReady: vmReady,
      isProcessing: vmProcessing,
      contracts: deployedContracts,
      deployContract,
      executeTransaction,
      submitAndMine,
      initialize: initializeVm,
    },
    keys: {
      isReady: kyberReady,
      kyberKeys: kyberKeys ? {
        publicKey: kyberKeys.publicKey,
        secretKey: kyberKeys.secretKey,
        algorithm: kyberKeys.algorithm,
        keyId: `kyber-${kyberKeys.algorithm}`,
        createdAt: Date.now(),
      } : null,
      hasKeys: !!kyberKeys,
      generateKeys,
      encrypt,
      decrypt,
    },
    on,
    emit,
  }), [
    identityDid, identityName, identityInitialized, setIdentity,
    balanceRaw, balanceLoading, refreshBalance, deductFee,
    blocks, transactions, receipts, explorerLoading, refreshExplorer,
    vmReady, vmProcessing, deployedContracts, deployContract, executeTransaction, submitAndMine, initializeVm,
    kyberReady, kyberKeys, generateKeys, encrypt, decrypt,
    on, emit,
  ]);

  return (
    <SpacekitContext.Provider value={contextValue}>
      {children}
    </SpacekitContext.Provider>
  );
}

/* ───────────────────────── Hooks ───────────────────────── */

/**
 * Hook to access SpaceKit SDK context
 * @throws Error if used outside SpacekitProvider
 */
export function useSpacekit(): SpacekitContextValue {
  const context = useContext(SpacekitContext);
  if (!context) {
    throw new Error('useSpacekit must be used within a SpacekitProvider');
  }
  return context;
}

/**
 * Hook to optionally access SpaceKit SDK context
 * @returns Context value or null if outside provider
 */
export function useSpacekitOptional(): SpacekitContextValue | null {
  return useContext(SpacekitContext);
}
