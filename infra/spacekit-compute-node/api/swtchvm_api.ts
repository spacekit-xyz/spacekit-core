// SWTCHVM Developer API - TypeScript/JavaScript SDK
// Similar to ethers.js but optimized for compute workloads

import { EventEmitter } from 'events';

// Core types
export interface SwtchvmAddress {
  readonly address: string;
  toString(): string;
  equals(other: SwtchvmAddress): boolean;
}

export interface SwtchvmTransaction {
  hash: string;
  from: SwtchvmAddress;
  to?: SwtchvmAddress;
  data: Uint8Array;
  gasLimit: bigint;
  gasPrice: bigint;
  value: bigint;
  nonce: number;
  blockNumber?: number;
  blockHash?: string;
  transactionIndex?: number;
  status?: 'pending' | 'confirmed' | 'failed';
}

export interface SwtchvmExecutionResult {
  success: boolean;
  returnData: Uint8Array;
  gasUsed: bigint;
  computeUnits: bigint;
  memoryUsed: bigint;
  gpuTimeMs?: number;
  logs: SwtchvmLog[];
  error?: string;
}

export interface SwtchvmLog {
  address: SwtchvmAddress;
  topics: string[];
  data: Uint8Array;
  blockNumber: number;
  transactionHash: string;
  logIndex: number;
}

export interface SwtchvmAccount {
  address: SwtchvmAddress;
  balance: bigint;
  nonce: number;
  codeHash?: string;
  storageRoot?: string;
}

export interface SwtchvmBlock {
  number: number;
  hash: string;
  parentHash: string;
  timestamp: number;
  gasLimit: bigint;
  gasUsed: bigint;
  transactions: SwtchvmTransaction[];
  stateRoot: string;
  computeRoot: string;
}

// Provider interface - similar to ethers Provider
export abstract class SwtchvmProvider extends EventEmitter {
  abstract getNetwork(): Promise<SwtchvmNetwork>;
  abstract getBlockNumber(): Promise<number>;
  abstract getBlock(blockNumberOrHash: number | string): Promise<SwtchvmBlock | null>;
  abstract getTransaction(hash: string): Promise<SwtchvmTransaction | null>;
  abstract getTransactionReceipt(hash: string): Promise<SwtchvmTransactionReceipt | null>;
  abstract getAccount(address: SwtchvmAddress): Promise<SwtchvmAccount | null>;
  abstract getBalance(address: SwtchvmAddress): Promise<bigint>;
  abstract getCode(address: SwtchvmAddress): Promise<Uint8Array>;
  abstract getStorage(address: SwtchvmAddress, key: string): Promise<string>;
  abstract estimateGas(transaction: Partial<SwtchvmTransaction>): Promise<bigint>;
  abstract call(transaction: Partial<SwtchvmTransaction>): Promise<Uint8Array>;
  abstract sendTransaction(transaction: SwtchvmTransaction): Promise<string>;
  abstract waitForTransaction(hash: string, confirmations?: number): Promise<SwtchvmTransactionReceipt>;
  
  // Compute-specific methods
  abstract estimateComputeCost(
    code: Uint8Array,
    input: Uint8Array,
    options?: ComputeOptions
  ): Promise<ComputeCostEstimate>;
  
  abstract getGpuInfo(): Promise<GpuInfo[]>;
  abstract getComputeMetrics(): Promise<ComputeMetrics>;
}

export interface SwtchvmNetwork {
  name: string;
  chainId: number;
  rpcUrl: string;
  blockExplorer?: string;
  gpuEnabled: boolean;
  gasPrice: bigint;
}

export interface SwtchvmTransactionReceipt {
  transactionHash: string;
  blockNumber: number;
  blockHash: string;
  transactionIndex: number;
  from: SwtchvmAddress;
  to?: SwtchvmAddress;
  gasUsed: bigint;
  status: number; // 1 for success, 0 for failure
  logs: SwtchvmLog[];
  contractAddress?: SwtchvmAddress;
  executionResult?: SwtchvmExecutionResult;
}

export interface ComputeOptions {
  preferredBackend?: 'cpu' | 'gpu' | 'auto';
  maxMemoryMB?: number;
  timeoutMs?: number;
  precision?: 'float16' | 'float32' | 'float64';
}

export interface ComputeCostEstimate {
  totalCost: bigint;
  breakdown: {
    baseCost: bigint;
    computeCost: bigint;
    memoryCost: bigint;
    gpuCost?: bigint;
    transferCost: bigint;
  };
  estimatedTime: number; // milliseconds
  recommendedBackend: 'cpu' | 'gpu';
}

export interface GpuInfo {
  id: string;
  name: string;
  memoryGB: number;
  computeCapability: string;
  available: boolean;
  costPerSecond: bigint;
}

export interface ComputeMetrics {
  totalExecutions: number;
  totalComputeTime: number;
  averageGasPerExecution: number;
  gpuUtilization: number;
  queueLength: number;
}

// HTTP Provider implementation
export class SwtchvmHttpProvider extends SwtchvmProvider {
  private baseUrl: string;
  private apiKey?: string;

  constructor(url: string, apiKey?: string) {
    super();
    this.baseUrl = url;
    this.apiKey = apiKey;
  }

  async getNetwork(): Promise<SwtchvmNetwork> {
    const response = await this.request('eth_chainId');
    return {
      name: 'SWTCHVM Mainnet',
      chainId: parseInt(response, 16),
      rpcUrl: this.baseUrl,
      gpuEnabled: true,
      gasPrice: BigInt(await this.request('eth_gasPrice')),
    };
  }

  async getBlockNumber(): Promise<number> {
    const response = await this.request('eth_blockNumber');
    return parseInt(response, 16);
  }

  async getBlock(blockNumberOrHash: number | string): Promise<SwtchvmBlock | null> {
    const response = await this.request('eth_getBlockByNumber', [
      typeof blockNumberOrHash === 'number' 
        ? `0x${blockNumberOrHash.toString(16)}` 
        : blockNumberOrHash,
      true
    ]);
    
    if (!response) return null;
    
    return {
      number: parseInt(response.number, 16),
      hash: response.hash,
      parentHash: response.parentHash,
      timestamp: parseInt(response.timestamp, 16),
      gasLimit: BigInt(response.gasLimit),
      gasUsed: BigInt(response.gasUsed),
      transactions: response.transactions.map(this.formatTransaction),
      stateRoot: response.stateRoot,
      computeRoot: response.computeRoot || response.stateRoot,
    };
  }

  async getAccount(address: SwtchvmAddress): Promise<SwtchvmAccount | null> {
    const [balance, nonce, code] = await Promise.all([
      this.getBalance(address),
      this.request('eth_getTransactionCount', [address.toString(), 'latest']),
      this.getCode(address),
    ]);

    return {
      address,
      balance,
      nonce: parseInt(nonce, 16),
      codeHash: code.length > 0 ? this.keccak256(code) : undefined,
    };
  }

  async getBalance(address: SwtchvmAddress): Promise<bigint> {
    const response = await this.request('eth_getBalance', [address.toString(), 'latest']);
    return BigInt(response);
  }

  async getCode(address: SwtchvmAddress): Promise<Uint8Array> {
    const response = await this.request('eth_getCode', [address.toString(), 'latest']);
    return this.hexToBytes(response);
  }

  async estimateComputeCost(
    code: Uint8Array,
    input: Uint8Array,
    options: ComputeOptions = {}
  ): Promise<ComputeCostEstimate> {
    const response = await this.request('swtchvm_estimateComputeCost', [
      this.bytesToHex(code),
      this.bytesToHex(input),
      options,
    ]);

    return {
      totalCost: BigInt(response.totalCost),
      breakdown: {
        baseCost: BigInt(response.breakdown.baseCost),
        computeCost: BigInt(response.breakdown.computeCost),
        memoryCost: BigInt(response.breakdown.memoryCost),
        gpuCost: response.breakdown.gpuCost ? BigInt(response.breakdown.gpuCost) : undefined,
        transferCost: BigInt(response.breakdown.transferCost),
      },
      estimatedTime: response.estimatedTime,
      recommendedBackend: response.recommendedBackend,
    };
  }

  async getGpuInfo(): Promise<GpuInfo[]> {
    const response = await this.request('swtchvm_getGpuInfo');
    return response.map((gpu: any) => ({
      id: gpu.id,
      name: gpu.name,
      memoryGB: gpu.memoryGB,
      computeCapability: gpu.computeCapability,
      available: gpu.available,
      costPerSecond: BigInt(gpu.costPerSecond),
    }));
  }

  async sendTransaction(transaction: SwtchvmTransaction): Promise<string> {
    const serialized = this.serializeTransaction(transaction);
    return await this.request('eth_sendRawTransaction', [serialized]);
  }

  // Helper methods
  private async request(method: string, params: any[] = []): Promise<any> {
    const response = await fetch(this.baseUrl, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        ...(this.apiKey && { 'X-API-Key': this.apiKey }),
      },
      body: JSON.stringify({
        jsonrpc: '2.0',
        method,
        params,
        id: Date.now(),
      }),
    });

    const data = await response.json();
    if (data.error) {
      throw new Error(data.error.message);
    }
    return data.result;
  }

  private formatTransaction(tx: any): SwtchvmTransaction {
    return {
      hash: tx.hash,
      from: new SwtchvmAddressImpl(tx.from),
      to: tx.to ? new SwtchvmAddressImpl(tx.to) : undefined,
      data: this.hexToBytes(tx.input),
      gasLimit: BigInt(tx.gas),
      gasPrice: BigInt(tx.gasPrice),
      value: BigInt(tx.value),
      nonce: parseInt(tx.nonce, 16),
      blockNumber: tx.blockNumber ? parseInt(tx.blockNumber, 16) : undefined,
      blockHash: tx.blockHash || undefined,
    };
  }

  private hexToBytes(hex: string): Uint8Array {
    const cleanHex = hex.startsWith('0x') ? hex.slice(2) : hex;
    const bytes = new Uint8Array(cleanHex.length / 2);
    for (let i = 0; i < cleanHex.length; i += 2) {
      bytes[i / 2] = parseInt(cleanHex.substr(i, 2), 16);
    }
    return bytes;
  }

  private bytesToHex(bytes: Uint8Array): string {
    return '0x' + Array.from(bytes).map(b => b.toString(16).padStart(2, '0')).join('');
  }

  private serializeTransaction(tx: SwtchvmTransaction): string {
    // Simplified transaction serialization
    return this.bytesToHex(new TextEncoder().encode(JSON.stringify(tx)));
  }

  private keccak256(data: Uint8Array): string {
    // Simplified hash - would use proper keccak256
    return '0x' + Array.from(data).map(b => b.toString(16).padStart(2, '0')).join('').slice(0, 64);
  }
}

// Address implementation
class SwtchvmAddressImpl implements SwtchvmAddress {
  constructor(public readonly address: string) {
    if (!this.isValidAddress(address)) {
      throw new Error(`Invalid SWTCHVM address: ${address}`);
    }
  }

  toString(): string {
    return this.address;
  }

  equals(other: SwtchvmAddress): boolean {
    return this.address.toLowerCase() === other.address.toLowerCase();
  }

  private isValidAddress(address: string): boolean {
    return /^0x[a-fA-F0-9]{40}$/.test(address);
  }
}

// Wallet interface for signing transactions
export abstract class SwtchvmWallet {
  abstract readonly address: SwtchvmAddress;
  abstract readonly provider?: SwtchvmProvider;

  abstract signTransaction(transaction: Partial<SwtchvmTransaction>): Promise<string>;
  abstract signMessage(message: string | Uint8Array): Promise<string>;
  
  // Convenience methods
  async getBalance(): Promise<bigint> {
    if (!this.provider) throw new Error('Provider not set');
    return this.provider.getBalance(this.address);
  }

  async getNonce(): Promise<number> {
    if (!this.provider) throw new Error('Provider not set');
    const account = await this.provider.getAccount(this.address);
    return account?.nonce || 0;
  }

  async sendTransaction(transaction: Partial<SwtchvmTransaction>): Promise<SwtchvmTransactionReceipt> {
    if (!this.provider) throw new Error('Provider not set');
    
    const fullTx: SwtchvmTransaction = {
      from: this.address,
      gasLimit: BigInt(100000),
      gasPrice: BigInt(1000000000), // 1 gwei
      value: BigInt(0),
      nonce: await this.getNonce(),
      data: new Uint8Array(),
      ...transaction,
      hash: '', // Will be set after signing
    };

    const signedTx = await this.signTransaction(fullTx);
    const hash = await this.provider.sendTransaction(JSON.parse(signedTx));
    return this.provider.waitForTransaction(hash);
  }
}

// Contract interface - high-level contract interaction
export class SwtchvmContract {
  readonly address: SwtchvmAddress;
  readonly abi: ContractABI;
  readonly provider: SwtchvmProvider;
  readonly signer?: SwtchvmWallet;

  constructor(
    address: string | SwtchvmAddress,
    abi: ContractABI,
    provider: SwtchvmProvider,
    signer?: SwtchvmWallet
  ) {
    this.address = typeof address === 'string' ? new SwtchvmAddressImpl(address) : address;
    this.abi = abi;
    this.provider = provider;
    this.signer = signer;
  }

  // Dynamic method generation based on ABI
  [key: string]: any;

  // Call a read-only function
  async call(functionName: string, args: any[] = []): Promise<any> {
    const func = this.abi.functions[functionName];
    if (!func) throw new Error(`Function ${functionName} not found in ABI`);

    const callData = this.encodeFunction(func, args);
    const result = await this.provider.call({
      to: this.address,
      data: callData,
    });

    return this.decodeResult(func.outputs, result);
  }

  // Send a transaction to a state-changing function
  async send(
    functionName: string,
    args: any[] = [],
    options: Partial<SwtchvmTransaction> = {}
  ): Promise<SwtchvmTransactionReceipt> {
    if (!this.signer) throw new Error('Signer required for sending transactions');

    const func = this.abi.functions[functionName];
    if (!func) throw new Error(`Function ${functionName} not found in ABI`);

    const callData = this.encodeFunction(func, args);
    
    return this.signer.sendTransaction({
      to: this.address,
      data: callData,
      ...options,
    });
  }

  // Estimate gas for a function call
  async estimateGas(functionName: string, args: any[] = []): Promise<bigint> {
    const func = this.abi.functions[functionName];
    if (!func) throw new Error(`Function ${functionName} not found in ABI`);

    const callData = this.encodeFunction(func, args);
    return this.provider.estimateGas({
      to: this.address,
      data: callData,
    });
  }

  // Estimate compute cost for a function call
  async estimateComputeCost(
    functionName: string,
    args: any[] = [],
    options?: ComputeOptions
  ): Promise<ComputeCostEstimate> {
    const func = this.abi.functions[functionName];
    if (!func) throw new Error(`Function ${functionName} not found in ABI`);

    const callData = this.encodeFunction(func, args);
    const code = await this.provider.getCode(this.address);
    
    return this.provider.estimateComputeCost(code, callData, options);
  }

  // Listen for events
  on(eventName: string, listener: (...args: any[]) => void): this {
    // Implementation would set up event filters and polling
    return this;
  }

  private encodeFunction(func: ABIFunction, args: any[]): Uint8Array {
    // Simplified encoding - real implementation would use proper ABI encoding
    const signature = `${func.name}(${func.inputs.map(i => i.type).join(',')})`;
    const selector = this.keccak256(new TextEncoder().encode(signature)).slice(0, 8);
    
    const encoded = new TextEncoder().encode(JSON.stringify(args));
    const result = new Uint8Array(4 + encoded.length);
    result.set(this.hexToBytes(selector), 0);
    result.set(encoded, 4);
    
    return result;
  }

  private decodeResult(outputs: ABIParameter[], data: Uint8Array): any {
    // Simplified decoding
    if (outputs.length === 0) return null;
    if (outputs.length === 1) {
      return JSON.parse(new TextDecoder().decode(data));
    }
    return outputs.map((_, i) => JSON.parse(new TextDecoder().decode(data.slice(i * 32, (i + 1) * 32))));
  }

  private keccak256(data: Uint8Array): string {
    // Simplified hash - would use proper keccak256
    return '0x' + Array.from(data).map(b => b.toString(16).padStart(2, '0')).join('').slice(0, 64);
  }

  private hexToBytes(hex: string): Uint8Array {
    const cleanHex = hex.startsWith('0x') ? hex.slice(2) : hex;
    const bytes = new Uint8Array(cleanHex.length / 2);
    for (let i = 0; i < cleanHex.length; i += 2) {
      bytes[i / 2] = parseInt(cleanHex.substr(i, 2), 16);
    }
    return bytes;
  }
}

// ABI types
export interface ContractABI {
  functions: { [name: string]: ABIFunction };
  events: { [name: string]: ABIEvent };
  constructor?: ABIFunction;
}

export interface ABIFunction {
  name: string;
  type: 'function' | 'constructor';
  inputs: ABIParameter[];
  outputs: ABIParameter[];
  stateMutability: 'pure' | 'view' | 'nonpayable' | 'payable';
  computeIntensive?: boolean; // SWTCHVM-specific
  gpuOptimized?: boolean;     // SWTCHVM-specific
}

export interface ABIEvent {
  name: string;
  type: 'event';
  inputs: ABIParameter[];
  anonymous: boolean;
}

export interface ABIParameter {
  name: string;
  type: string;
  indexed?: boolean;
  components?: ABIParameter[]; // For structs/tuples
}

// Contract factory for deploying new contracts
export class SwtchvmContractFactory {
  readonly abi: ContractABI;
  readonly bytecode: Uint8Array;
  readonly signer: SwtchvmWallet;

  constructor(abi: ContractABI, bytecode: string | Uint8Array, signer: SwtchvmWallet) {
    this.abi = abi;
    this.bytecode = typeof bytecode === 'string' ? this.hexToBytes(bytecode) : bytecode;
    this.signer = signer;
  }

  async deploy(...args: any[]): Promise<SwtchvmContract> {
    const constructor = this.abi.constructor;
    const deployData = constructor 
      ? this.encodeConstructor(constructor, args)
      : this.bytecode;

    const receipt = await this.signer.sendTransaction({
      data: deployData,
      gasLimit: BigInt(3000000), // High gas limit for deployment
    });

    if (!receipt.contractAddress) {
      throw new Error('Contract deployment failed - no contract address returned');
    }

    return new SwtchvmContract(
      receipt.contractAddress,
      this.abi,
      this.signer.provider!,
      this.signer
    );
  }

  async estimateDeploymentCost(...args: any[]): Promise<ComputeCostEstimate> {
    const constructor = this.abi.constructor;
    const deployData = constructor 
      ? this.encodeConstructor(constructor, args)
      : this.bytecode;

    return this.signer.provider!.estimateComputeCost(
      this.bytecode,
      deployData
    );
  }

  private encodeConstructor(constructor: ABIFunction, args: any[]): Uint8Array {
    // Simplified constructor encoding
    return this.bytecode; // Would append encoded constructor args
  }

  private hexToBytes(hex: string): Uint8Array {
    const cleanHex = hex.startsWith('0x') ? hex.slice(2) : hex;
    const bytes = new Uint8Array(cleanHex.length / 2);
    for (let i = 0; i < cleanHex.length; i += 2) {
      bytes[i / 2] = parseInt(cleanHex.substr(i, 2), 16);
    }
    return bytes;
  }
}

// Utility functions
export class SwtchvmUtils {
  static parseEther(ether: string): bigint {
    return BigInt(parseFloat(ether) * 1e18);
  }

  static formatEther(wei: bigint): string {
    return (Number(wei) / 1e18).toString();
  }

  static parseUnits(value: string, decimals: number = 18): bigint {
    return BigInt(parseFloat(value) * Math.pow(10, decimals));
  }

  static formatUnits(value: bigint, decimals: number = 18): string {
    return (Number(value) / Math.pow(10, decimals)).toString();
  }

  static isAddress(address: string): boolean {
    return /^0x[a-fA-F0-9]{40}$/.test(address);
  }

  static getAddress(address: string): SwtchvmAddress {
    return new SwtchvmAddressImpl(address);
  }

  static hexlify(data: Uint8Array): string {
    return '0x' + Array.from(data).map(b => b.toString(16).padStart(2, '0')).join('');
  }

  static arrayify(hex: string): Uint8Array {
    const cleanHex = hex.startsWith('0x') ? hex.slice(2) : hex;
    const bytes = new Uint8Array(cleanHex.length / 2);
    for (let i = 0; i < cleanHex.length; i += 2) {
      bytes[i / 2] = parseInt(cleanHex.substr(i, 2), 16);
    }
    return bytes;
  }
}

// Example usage:
/*
// 1. Connect to provider
const provider = new SwtchvmHttpProvider('https://mainnet.swtchvm.io', 'your-api-key');

// 2. Create wallet
const wallet = new SwtchvmPrivateKeyWallet('0x...', provider);

// 3. Deploy contract
const factory = new SwtchvmContractFactory(contractABI, contractBytecode, wallet);
const contract = await factory.deploy(constructorArg1, constructorArg2);

// 4. Interact with contract
const result = await contract.call('viewFunction', [arg1, arg2]);
const receipt = await contract.send('stateChangingFunction', [arg1, arg2]);

// 5. Estimate costs before execution
const estimate = await contract.estimateComputeCost('expensiveFunction', [largeDataset]);
console.log(`Estimated cost: ${SwtchvmUtils.formatEther(estimate.totalCost)} SWTCHVM`);
console.log(`Recommended backend: ${estimate.recommendedBackend}`);
*/