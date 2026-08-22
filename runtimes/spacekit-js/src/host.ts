import { MemoryView, LinearMemoryAllocator } from "./memory.js";
import { StorageAdapter, createInMemoryStorage, bytesToHex as storageBytesToHex } from "./storage.js";
import { sha256 } from "@noble/hashes/sha256";
import { enforceStorageProtection } from "./vm/genesis.js";
import { microgpt_forward } from "./llm/microgpt_forward.js";
import {
  growformerHostBrainInfoJson,
  growformerHostCodegenJson,
  growformerHostConverseJson,
  growformerHostGenerationJson,
  growformerHostStatusCode,
  isGrowformerHostReady,
  isGrowformerModuleLoaded,
  loadGrowformerBrain,
  resetGrowformerConversation,
} from "./growformer/runtime.js";
import {
  type ToolEffectManager,
  type ToolEffectRecord,
  createToolEffectManager,
  toolRequestKey,
  TOOL_STATUS,
} from "./tools/effect_manager.js";
import {
  type RemoteStorageAdapter,
  type PaymentAdapter,
  type MessagingAdapter,
  type ToolSideEffects,
  createToolSideEffects,
} from "./tools/types.js";
import type { ToolManifest, ToolDef } from "./tools/manifest.js";
import {
  validateParams,
  checkConstraints,
  checkSizeLimit,
  applyStorageKeyPrefix,
  recordEffect,
  SKTCS_ERROR,
  type ConstraintState,
  createConstraintState,
} from "./tools/policy_gate.js";
import { applySanitize } from "./tools/sanitize.js";
import { PaymasterHostState, SessionHostState } from "./host_session_paymaster.js";

export interface TokenAdapter {
  balanceOf(did: string): bigint;
  transfer(from: string, to: string, amount: bigint): boolean;
  totalSupply(): bigint;
}

export interface NftAdapter {
  mint(contractId: string, owner: string): bigint;
  ownerOf(contractId: string, tokenId: bigint): string | null;
  transfer(contractId: string, tokenId: bigint, to: string): boolean;
}

export interface ReputationAdapter {
  getScore(did: string, repType: number): bigint;
  checkThreshold(did: string, repType: number, threshold: bigint): boolean;
  getOverall(did: string): bigint;
  getBreakdown(did: string): bigint[];
}

export interface FactAdapter {
  exists(packageId: string): boolean;
  verifyHash(packageId: string, hash: string): boolean;
}

export interface ContractCallResult {
  status: number;
  result: Uint8Array;
}

export interface ContractCallAdapter {
  call(
    contractId: string,
    input: Uint8Array,
    callerDid: string,
    value?: bigint
  ): ContractCallResult;
}

export interface CompressionAdapter {
  compress(data: Uint8Array, mode: string): Uint8Array;
  decompress(data: Uint8Array, mode: string): Uint8Array;
}

/**
 * LLM status codes used by host functions
 */
export const LLM_STATUS = {
  NOT_LOADED: 0,
  READY: 1,
  LOADING: 2,
} as const;

export type LlmStatus = typeof LLM_STATUS[keyof typeof LLM_STATUS];

/**
 * Captured LLM request from contract execution (for two-phase inference)
 */
export interface CapturedLlmRequest {
  prompt: string;
  maxTokens: number;
  temperature: number;
}

/**
 * LLM Adapter interface for integrating language models with smart contracts.
 *
 * The infer method is synchronous because WASM host functions cannot be async.
 * Implementations should pre-load models and cache responses.
 *
 * Two-phase execution pattern (when using setCaptureMode):
 * 1. Contract runs in "capture mode" – llm_call records the prompt and returns a
 *    placeholder response (e.g. empty string) to the contract.
 * 2. Host runs async inference with the captured prompt.
 * 3. Contract runs again – llm_call returns the cached inference result.
 */
export interface LlmAdapter {
  /**
   * Run inference on a prompt. Must be synchronous.
   * @param prompt - The input prompt
   * @param maxTokens - Maximum tokens to generate  
   * @param temperature - Sampling temperature (0-100, e.g., 70 = 0.7)
   * @returns The generated text, or empty string on failure
   */
  infer(prompt: string, maxTokens: number, temperature: number): string;
  
  /**
   * Get the current LLM status
   * @returns 0 = not loaded, 1 = ready, 2 = loading
   */
  getStatus(): LlmStatus;
  
  /**
   * Enable/disable capture mode for two-phase execution.
   * In capture mode, infer() records the request but returns empty string.
   */
  setCaptureMode?(enabled: boolean): void;
  
  /**
   * Get the captured LLM request from the last capture-mode execution.
   */
  getCapturedRequest?(): CapturedLlmRequest | null;
  
  /**
   * Clear any captured request.
   */
  clearCapturedRequest?(): void;
}

export interface Logger {
  log(message: string): void;
}

export interface HostOptions {
  storage?: StorageAdapter;
  token?: TokenAdapter;
  nft?: NftAdapter;
  reputation?: ReputationAdapter;
  fact?: FactAdapter;
  compression?: CompressionAdapter;
  llm?: LlmAdapter;
  logger?: Logger;
  callerDid?: string;
  contractCall?: ContractCallAdapter;

  // Agent tool adapters
  /** Operator DID — web_search intents are routed here via MessagingAdapter.requestResponse. */
  toolOperatorDid?: string;
  remoteStorage?: RemoteStorageAdapter;
  payment?: PaymentAdapter;
  messaging?: MessagingAdapter;

  /**
   * Synchronous SPHINCS+ verifier backing the `spacekit_crypto.sphincs_verify`
   * host function.
   *
   * It must be synchronous because WASM host calls cannot await. Supply a
   * WASM-backed implementation (for example the pqcrypto build). When absent,
   * `sphincs_verify` reports "verifier unavailable" rather than pretending the
   * signature was checked.
   */
  sphincsVerifier?: SphincsVerifier;
}

/** Synchronous SPHINCS+ verification. Returns true only for a valid signature. */
export interface SphincsVerifier {
  (publicKey: Uint8Array, message: Uint8Array, signature: Uint8Array): boolean;
}

/** `sphincs_verify` return codes. */
export const SPHINCS_VERIFY_VALID = 1;
export const SPHINCS_VERIFY_INVALID = 0;
/** No verifier is configured — the signature was *not* checked. */
export const SPHINCS_VERIFY_UNAVAILABLE = -1;
/** Arguments could not be read from guest memory. */
export const SPHINCS_VERIFY_BAD_ARGS = -2;

export interface HostContext {
  setMemory(memory: WebAssembly.Memory): void;
  readBytes(ptr: number, len: number): Uint8Array;
  writeBytes(ptr: number, data: Uint8Array): void;
  readString(ptr: number, len: number): string;
  writeString(ptr: number, value: string): number;
  alloc(size: number): number;
  events: Array<{ type: string; data: Uint8Array }>;
  callerDid: string;
  msgValue: bigint;
  gasUsed: number;
  gasLimit: number;
  setGasLimit(limit: number): void;
  consumeGas(amount: number): void;
  contractId?: string;
  contractCall?: ContractCallAdapter;
}

function toBigInt(value: number | bigint): bigint {
  return typeof value === "bigint" ? value : BigInt(value);
}

function toNumber(value: number | bigint): number {
  return typeof value === "bigint" ? Number(value) : value;
}

class InMemoryTokenAdapter implements TokenAdapter {
  private balances = new Map<string, bigint>();
  private supply = 0n;

  balanceOf(did: string): bigint {
    return this.balances.get(did) ?? 0n;
  }

  transfer(from: string, to: string, amount: bigint): boolean {
    if (amount <= 0n) {
      return false;
    }
    const fromBal = this.balanceOf(from);
    if (fromBal < amount) {
      return false;
    }
    this.balances.set(from, fromBal - amount);
    this.balances.set(to, this.balanceOf(to) + amount);
    return true;
  }

  totalSupply(): bigint {
    return this.supply;
  }

  mint(to: string, amount: bigint) {
    if (amount <= 0n) {
      return;
    }
    this.supply += amount;
    this.balances.set(to, this.balanceOf(to) + amount);
  }
}

class InMemoryNftAdapter implements NftAdapter {
  private counters = new Map<string, bigint>();
  private owners = new Map<string, Map<bigint, string>>();

  mint(contractId: string, owner: string): bigint {
    const next = (this.counters.get(contractId) ?? 0n) + 1n;
    this.counters.set(contractId, next);
    let registry = this.owners.get(contractId);
    if (!registry) {
      registry = new Map<bigint, string>();
      this.owners.set(contractId, registry);
    }
    registry.set(next, owner);
    return next;
  }

  ownerOf(contractId: string, tokenId: bigint): string | null {
    return this.owners.get(contractId)?.get(tokenId) ?? null;
  }

  transfer(contractId: string, tokenId: bigint, to: string): boolean {
    const registry = this.owners.get(contractId);
    if (!registry || !registry.has(tokenId)) {
      return false;
    }
    registry.set(tokenId, to);
    return true;
  }
}

class InMemoryReputationAdapter implements ReputationAdapter {
  private scores = new Map<string, Map<number, bigint>>();

  getScore(did: string, repType: number): bigint {
    return this.scores.get(did)?.get(repType) ?? 0n;
  }

  checkThreshold(did: string, repType: number, threshold: bigint): boolean {
    return this.getScore(did, repType) >= threshold;
  }

  getOverall(did: string): bigint {
    const perType = this.scores.get(did);
    if (!perType || perType.size === 0) {
      return 0n;
    }
    let total = 0n;
    for (const score of perType.values()) {
      total += score;
    }
    return total / BigInt(perType.size);
  }

  getBreakdown(did: string): bigint[] {
    const perType = this.scores.get(did);
    const result = Array.from({ length: 6 }, () => 0n);
    if (!perType) {
      return result;
    }
    for (const [type, score] of perType.entries()) {
      if (type >= 0 && type < result.length) {
        result[type] = score;
      }
    }
    return result;
  }
}

class InMemoryFactAdapter implements FactAdapter {
  private entries = new Map<string, string>();

  register(packageId: string, hash: string) {
    this.entries.set(packageId, hash);
  }

  exists(packageId: string): boolean {
    return this.entries.has(packageId);
  }

  verifyHash(packageId: string, hash: string): boolean {
    return this.entries.get(packageId) === hash;
  }
}

class NoopCompressionAdapter implements CompressionAdapter {
  compress(data: Uint8Array): Uint8Array {
    return data;
  }

  decompress(data: Uint8Array): Uint8Array {
    return data;
  }
}

class NoopLlmAdapter implements LlmAdapter {
  infer(prompt: string, _maxTokens: number, _temperature: number): string {
    return `[LLM not configured] Prompt: ${prompt.slice(0, 50)}...`;
  }
  
  getStatus(): LlmStatus {
    return LLM_STATUS.NOT_LOADED;
  }
}

class ConsoleLogger implements Logger {
  log(message: string) {
    console.log(message);
  }
}

class HostContextImpl implements HostContext {
  private memoryView: MemoryView;
  private allocator: LinearMemoryAllocator;

  storage: StorageAdapter;
  token: TokenAdapter;
  nft: NftAdapter;
  reputation: ReputationAdapter;
  fact: FactAdapter;
  compression: CompressionAdapter;
  llm: LlmAdapter;
  logger: Logger;
  callerDid: string;
  contractId?: string;
  contractCall?: ContractCallAdapter;
  msgValue: bigint;
  events: Array<{ type: string; data: Uint8Array }> = [];
  gasUsed = 0;
  gasLimit = 0;

  // Agent tool adapters and effect infrastructure
  toolOperatorDid?: string;
  remoteStorage?: RemoteStorageAdapter;
  payment?: PaymentAdapter;
  messaging?: MessagingAdapter;
  effectManager: ToolEffectManager;
  sideEffects: ToolSideEffects;
  sessionHost: SessionHostState;
  paymasterHost: PaymasterHostState;

  // SKTCS policy gate
  manifest?: ToolManifest;
  constraintState: ConstraintState;
  devMode: boolean;

  /** Backs `spacekit_crypto.sphincs_verify`; absent means "cannot verify". */
  sphincsVerifier?: SphincsVerifier;

  private llmResponse = "";

  constructor(options: HostOptions) {
    const memory = new WebAssembly.Memory({ initial: 2 });
    this.memoryView = new MemoryView(memory);
    this.allocator = new LinearMemoryAllocator(memory);
    this.storage = options.storage ?? createInMemoryStorage();
    this.token = options.token ?? new InMemoryTokenAdapter();
    this.nft = options.nft ?? new InMemoryNftAdapter();
    this.reputation = options.reputation ?? new InMemoryReputationAdapter();
    this.fact = options.fact ?? new InMemoryFactAdapter();
    this.compression = options.compression ?? new NoopCompressionAdapter();
    this.llm = options.llm ?? new NoopLlmAdapter();
    this.logger = options.logger ?? new ConsoleLogger();
    this.callerDid = options.callerDid ?? "did:spacekit:browser:anonymous";
    this.sphincsVerifier = options.sphincsVerifier;
    this.contractCall = options.contractCall;
    this.msgValue = 0n;
    this.events = [];

    // Agent tools
    this.toolOperatorDid = options.toolOperatorDid;
    this.remoteStorage = options.remoteStorage;
    this.payment = options.payment;
    this.messaging = options.messaging;
    this.effectManager = createToolEffectManager();
    this.sideEffects = createToolSideEffects();
    this.constraintState = createConstraintState();
    this.devMode = false;
    this.sessionHost = new SessionHostState();
    this.paymasterHost = new PaymasterHostState();
  }

  setMemory(memory: WebAssembly.Memory) {
    this.memoryView.setMemory(memory);
    this.allocator.setMemory(memory);
  }

  readBytes(ptr: number, len: number): Uint8Array {
    return this.memoryView.readBytes(ptr, len);
  }

  writeBytes(ptr: number, data: Uint8Array): void {
    this.memoryView.writeBytes(ptr, data);
  }

  readString(ptr: number, len: number): string {
    return this.memoryView.readString(ptr, len);
  }

  writeString(ptr: number, value: string): number {
    return this.memoryView.writeString(ptr, value);
  }

  alloc(size: number): number {
    return this.allocator.alloc(size);
  }

  setGasLimit(limit: number): void {
    this.gasLimit = limit;
    this.gasUsed = 0;
  }

  consumeGas(amount: number): void {
    if (!Number.isFinite(amount) || amount <= 0) {
      return;
    }
    this.gasUsed += amount;
    if (this.gasLimit > 0 && this.gasUsed > this.gasLimit) {
      throw new Error("Out of gas");
    }
  }

  setLlmResponse(value: string) {
    this.llmResponse = value;
  }

  getLlmResponse(): string {
    return this.llmResponse;
  }
}

function readKey(ctx: HostContextImpl, ptr: number, len: number): Uint8Array {
  return ctx.readBytes(ptr, len);
}

function readDid(ctx: HostContextImpl, ptr: number, len: number): string {
  return ctx.readString(ptr, len);
}

function readHash(ctx: HostContextImpl, ptr: number, len: number): string {
  return ctx.readString(ptr, len);
}

/**
 * SKTCS policy gate — run validation + constraints for a tool invocation.
 * Returns a negative error code when rejected, or 0 when the call is allowed.
 * When devMode is active, violations are logged as warnings instead of rejecting.
 */
function policyGate(
  ctx: HostContextImpl,
  toolName: string,
  params: Record<string, unknown>,
  effectRound: number,
): number {
  const toolDef = ctx.manifest?.tools[toolName];
  if (!toolDef) return 0; // no manifest or tool not declared — legacy passthrough

  const paramResult = validateParams(toolDef, params);
  if (paramResult.rejected) {
    emitToolRecord(ctx, toolName, effectRound, "rejected", paramResult.reason);
    if (ctx.devMode) {
      console.warn(`[SKTCS devMode] ${paramResult.reason}`);
      return 0;
    }
    return paramResult.errorCode;
  }

  const constraintResult = checkConstraints(
    toolName, toolDef, ctx.callerDid, ctx.constraintState, params,
  );
  if (constraintResult.rejected) {
    emitToolRecord(ctx, toolName, effectRound, "rejected", constraintResult.reason);
    if (ctx.devMode) {
      console.warn(`[SKTCS devMode] ${constraintResult.reason}`);
      return 0;
    }
    return constraintResult.errorCode;
  }

  recordEffect(toolName, ctx.constraintState);
  return 0;
}

/**
 * Tools whose results are attacker-influenced (web pages, data written by
 * another party) and end up in an agent's prompt context. Their output is
 * sanitized even when the manifest says nothing, so a contract cannot opt out
 * of the protection by omitting a declaration.
 */
const EXTERNAL_CONTENT_TOOLS = new Set(["web_search", "remote_storage_get"]);

/**
 * Write a tool result into guest memory, applying the sanitize mode the
 * manifest declares for that tool's `result` param.
 *
 * `applySanitize` existed but was never called, so `sanitize: "prompt_fence"`
 * declarations in manifests had no effect and raw external content reached the
 * guest verbatim.
 *
 * Returns the number of bytes written.
 */
function writeSanitizedToolResult(
  ctx: HostContextImpl,
  toolName: string,
  result: Uint8Array,
  destPtr: number,
  maxLen: number,
): number {
  const declared = ctx.manifest?.tools[toolName]?.params?.result?.sanitize;
  const mode = declared ?? (EXTERNAL_CONTENT_TOOLS.has(toolName) ? "strip_control_chars" : undefined);

  let payload = result;
  if (mode) {
    try {
      const text = new TextDecoder("utf-8", { fatal: false }).decode(result);
      // The fence tag is derived from the content hash so the content author
      // cannot predict it and close the fence early.
      const hashPrefix = Array.from(sha256(result), (b) => b.toString(16).padStart(2, "0")).join("");
      payload = new TextEncoder().encode(applySanitize(text, mode, hashPrefix));
    } catch (e) {
      console.error(`[SpacekitVM] failed to sanitize ${toolName} result:`, e);
      return TOOL_STATUS.ERROR;
    }
  }

  const n = Math.min(payload.length, maxLen);
  ctx.writeBytes(destPtr, payload.subarray(0, n));
  return n;
}

function emitToolRecord(
  ctx: HostContextImpl,
  toolId: string,
  effectRound: number,
  status: ToolEffectRecord["status"],
  reason?: string,
): void {
  ctx.effectManager.addRecord({
    tool_id: toolId,
    caller_did: ctx.callerDid,
    params_hash: "",
    result_hash: null,
    cost_charged: "0",
    timestamp: Date.now(),
    effect_round: effectRound,
    status,
    reason,
  });
}

function toContractDid(contractId: string | undefined, fallbackDid: string): string {
  if (!contractId) {
    return fallbackDid;
  }
  if (contractId.startsWith("did:")) {
    return contractId;
  }
  return `did:spacekit:contract:${contractId}`;
}

function getTimestamp(): bigint {
  return BigInt(Math.floor(Date.now() / 1000));
}

export function createHost(options: HostOptions = {}) {
  const ctx = new HostContextImpl(options);
  const imports = createImports(ctx);

  return {
    context: ctx,
    imports,
    bindInstance(instance: WebAssembly.Instance) {
      const memory = instance.exports.memory as WebAssembly.Memory | undefined;
      if (!memory) {
        throw new Error("WASM instance does not export memory");
      }
      ctx.setMemory(memory);
    },
  };
}

export function createImports(ctx: HostContextImpl): WebAssembly.Imports {
  const storageRead = (
    keyPtr: number,
    keyLen: number,
    outputPtr?: number,
    maxLen?: number
  ): number => {
    const key = readKey(ctx, keyPtr, keyLen);
    const value = ctx.storage.get(key);
    if (!value) {
      return -1;
    }
    if (outputPtr === undefined || maxLen === undefined) {
      return value.length;
    }
    const len = Math.min(value.length, maxLen);
    ctx.writeBytes(outputPtr, value.subarray(0, len));
    return len;
  };

  const storageLoad = (
    keyPtr: number,
    keyLen: number,
    outputPtr: number,
    maxLen: number
  ): number => {
    const key = readKey(ctx, keyPtr, keyLen);
    const value = ctx.storage.get(key);
    if (!value) {
      return 0;
    }
    const len = Math.min(value.length, maxLen);
    ctx.writeBytes(outputPtr, value.subarray(0, len));
    return len;
  };

  const storageWrite = (keyPtr: number, keyLen: number, valuePtr: number, valueLen: number): number => {
    const key = readKey(ctx, keyPtr, keyLen);
    if (!ctx.devMode && ctx.contractId) {
      const keyStr = new TextDecoder().decode(key);
      try {
        enforceStorageProtection(keyStr, ctx.contractId);
      } catch (e) {
        console.warn(`[SpaceKit] ${(e as Error).message}`);
        return -1;
      }
    }
    const value = ctx.readBytes(valuePtr, valueLen);
    ctx.storage.set(key, value);
    return valueLen;
  };

  const tokenTransfer = (
    fromPtr: number,
    fromLen: number,
    toPtr: number,
    toLen: number,
    amount: bigint
  ): number => {
    const from = readDid(ctx, fromPtr, fromLen);
    const to = readDid(ctx, toPtr, toLen);
    return ctx.token.transfer(from, to, amount) ? 1 : 0;
  };

  const tokenBalance = (didPtr: number, didLen: number): bigint => {
    return ctx.token.balanceOf(readDid(ctx, didPtr, didLen));
  };

  const nftMint = (
    contractIdPtr: number,
    contractIdLen: number,
    ownerPtr: number,
    ownerLen: number
  ): bigint => {
    const contractId = readDid(ctx, contractIdPtr, contractIdLen);
    const owner = readDid(ctx, ownerPtr, ownerLen);
    return ctx.nft.mint(contractId, owner);
  };

  const nftOwnerOf = (
    contractIdPtr: number,
    contractIdLen: number,
    tokenId: bigint,
    outputPtr: number,
    maxLen: number
  ): number => {
    const contractId = readDid(ctx, contractIdPtr, contractIdLen);
    const owner = ctx.nft.ownerOf(contractId, tokenId);
    if (!owner) {
      return -1;
    }
    const written = ctx.writeString(outputPtr, owner);
    return Math.min(written, maxLen);
  };

  const nftTransfer = (
    contractIdPtr: number,
    contractIdLen: number,
    tokenId: bigint,
    toPtr: number,
    toLen: number
  ): number => {
    const contractId = readDid(ctx, contractIdPtr, contractIdLen);
    const to = readDid(ctx, toPtr, toLen);
    return ctx.nft.transfer(contractId, tokenId, to) ? 1 : 0;
  };

  const reputationGetScore = (didPtr: number, didLen: number, repType: number): bigint => {
    return ctx.reputation.getScore(readDid(ctx, didPtr, didLen), repType);
  };

  const reputationCheckThreshold = (
    didPtr: number,
    didLen: number,
    repType: number,
    threshold: bigint
  ): number => {
    const did = readDid(ctx, didPtr, didLen);
    return ctx.reputation.checkThreshold(did, repType, threshold) ? 1 : 0;
  };

  const reputationGetOverall = (didPtr: number, didLen: number): bigint => {
    return ctx.reputation.getOverall(readDid(ctx, didPtr, didLen));
  };

  const reputationGetBreakdown = (didPtr: number, didLen: number, outputPtr: number) => {
    const did = readDid(ctx, didPtr, didLen);
    const scores = ctx.reputation.getBreakdown(did);
    const out = new BigInt64Array(scores.length);
    for (let i = 0; i < scores.length; i += 1) {
      out[i] = scores[i];
    }
    ctx.writeBytes(outputPtr, new Uint8Array(out.buffer));
  };

  const factExists = (packageIdPtr: number, packageIdLen: number): number => {
    const packageId = readDid(ctx, packageIdPtr, packageIdLen);
    return ctx.fact.exists(packageId) ? 1 : 0;
  };

  const factVerifyHash = (
    packageIdPtr: number,
    packageIdLen: number,
    hashPtr: number,
    hashLen: number
  ): number => {
    const packageId = readDid(ctx, packageIdPtr, packageIdLen);
    const hash = readHash(ctx, hashPtr, hashLen);
    return ctx.fact.verifyHash(packageId, hash) ? 1 : 0;
  };

  const pythonCompress = (
    inputPtr: number,
    inputLen: number,
    modePtr: number,
    modeLen: number,
    outputPtr: number,
    outputMaxLen: number
  ): number => {
    const input = ctx.readBytes(inputPtr, inputLen);
    const mode = ctx.readString(modePtr, modeLen);
    const output = ctx.compression.compress(input, mode);
    const len = Math.min(output.length, outputMaxLen);
    ctx.writeBytes(outputPtr, output.subarray(0, len));
    return len;
  };

  const pythonDecompress = (
    inputPtr: number,
    inputLen: number,
    modePtr: number,
    modeLen: number,
    outputPtr: number,
    outputMaxLen: number
  ): number => {
    const input = ctx.readBytes(inputPtr, inputLen);
    const mode = ctx.readString(modePtr, modeLen);
    const output = ctx.compression.decompress(input, mode);
    const len = Math.min(output.length, outputMaxLen);
    ctx.writeBytes(outputPtr, output.subarray(0, len));
    return len;
  };

  /**
   * LLM inference host function
   * Signature matches SDK: llm_inference(prompt_ptr, prompt_len, dest_ptr, max_len, temperature, max_tokens) -> i32
   * Returns: >0 = bytes written, -1 = LLM not ready, -2 = inference error
   */
  const llmInference = (
    promptPtr: number,
    promptLen: number,
    destPtr: number,
    maxLen: number,
    temperature: number, // temperature * 100 (e.g., 70 = 0.7)
    maxTokens: number
  ): number => {
    // Check if LLM is ready
    const status = ctx.llm.getStatus();
    if (status !== 1) { // LLM_STATUS.READY
      return -1; // Not ready
    }
    
    const prompt = ctx.readString(promptPtr, promptLen);
    
    try {
      const response = ctx.llm.infer(prompt, maxTokens, temperature);
      if (!response || response.length === 0) {
        return -2; // Inference error
      }
      
      // Write response directly to dest buffer
      const encoder = new TextEncoder();
      const responseBytes = encoder.encode(response);
      const bytesToWrite = Math.min(responseBytes.length, maxLen);
      ctx.writeBytes(destPtr, responseBytes.subarray(0, bytesToWrite));
      
      return bytesToWrite;
    } catch (e) {
      console.error("[SpacekitVM] LLM inference error:", e);
      return -2;
    }
  };

  /**
   * Get LLM status: 0 = not loaded, 1 = ready, 2 = loading
   */
  const llmStatus = (): number => {
    return ctx.llm.getStatus();
  };

  /**
   * Micro-GPT forward primitive: write logits to out_ptr (VOCAB_SIZE f32s).
   * Signature: microgpt_forward(token_id: u32, pos_id: u32, out_ptr: u32) -> void
   */
  const microgptForward = (tokenId: number, posId: number, outPtr: number): void => {
    const logits = microgpt_forward(tokenId, posId);
    const bytes = new Uint8Array(logits.buffer, logits.byteOffset, logits.byteLength);
    ctx.writeBytes(outPtr, bytes);
  };

  /**
   * Growformer / spacekit_agent: write UTF-8 JSON into contract memory.
   * Returns: >0 = bytes written, -1 = not ready, -2 = error
   */
  const writeAgentJson = (destPtr: number, maxLen: number, json: string): number => {
    try {
      const bytes = new TextEncoder().encode(json);
      if (bytes.length > maxLen) {
        console.warn(
          `[SpacekitVM] writeAgentJson: response truncated (${bytes.length} bytes > ${maxLen} buffer). Increase MAX_RESPONSE_LEN in the contract.`
        );
      }
      const n = Math.min(bytes.length, maxLen);
      ctx.writeBytes(destPtr, bytes.subarray(0, n));
      return n;
    } catch {
      return -2;
    }
  };

  const agentGrowformerStatus = (): number => {
    return growformerHostStatusCode();
  };

  /**
   * Load Growformer brain bytes from VM storage (same key space as `storage_load` / `storage_get`).
   * Returns bytes loaded (>0), or -2 error, -3 missing key, -4 Growformer module not initialized.
   */
  const agentGrowformerLoadBrainFromStorage = (keyPtr: number, keyLen: number): number => {
    if (!isGrowformerModuleLoaded()) {
      return -4;
    }
    try {
      const key = ctx.readBytes(keyPtr, keyLen);
      const value = ctx.storage.get(key);
      if (!value || value.length === 0) {
        return -3;
      }
      loadGrowformerBrain(value);
      return value.length;
    } catch (e) {
      console.error("[SpacekitVM] agent_growformer_load_brain_from_storage:", e);
      return -2;
    }
  };

  const agentGrowformerGeneration = (
    promptPtr: number,
    promptLen: number,
    destPtr: number,
    maxLen: number
  ): number => {
    if (!isGrowformerHostReady()) {
      return -1;
    }
    try {
      const prompt = ctx.readString(promptPtr, promptLen);
      const json = growformerHostGenerationJson(prompt);
      return writeAgentJson(destPtr, maxLen, json);
    } catch (e) {
      console.error("[SpacekitVM] agent_growformer_generation:", e);
      return -2;
    }
  };

  const agentGrowformerConverse = (
    promptPtr: number,
    promptLen: number,
    destPtr: number,
    maxLen: number
  ): number => {
    if (!isGrowformerHostReady()) {
      return -1;
    }
    try {
      const prompt = ctx.readString(promptPtr, promptLen);
      const json = growformerHostConverseJson(prompt);
      return writeAgentJson(destPtr, maxLen, json);
    } catch (e) {
      console.error("[SpacekitVM] agent_growformer_converse:", e);
      return -2;
    }
  };

  const agentGrowformerCodegen = (
    promptPtr: number,
    promptLen: number,
    destPtr: number,
    maxLen: number
  ): number => {
    if (!isGrowformerHostReady()) {
      return -1;
    }
    try {
      const prompt = ctx.readString(promptPtr, promptLen);
      const json = growformerHostCodegenJson(prompt);
      return writeAgentJson(destPtr, maxLen, json);
    } catch (e) {
      console.error("[SpacekitVM] agent_growformer_codegen:", e);
      return -2;
    }
  };

  const agentGrowformerBrainInfo = (destPtr: number, maxLen: number): number => {
    if (!isGrowformerHostReady()) {
      return -1;
    }
    try {
      const json = growformerHostBrainInfoJson();
      return writeAgentJson(destPtr, maxLen, json);
    } catch (e) {
      console.error("[SpacekitVM] agent_growformer_brain_info:", e);
      return -2;
    }
  };

  const agentGrowformerResetConversation = (): void => {
    resetGrowformerConversation();
  };

  const useGas = (amount: number) => {
    ctx.consumeGas(amount);
  };

  // AssemblyScript-compiled WASM may import env.abort (e.g. for assertions). Provide a stub.
  const envAbort = (message?: number, fileName?: number, line?: number, column?: number): void => {
    console.warn("[SpaceKit] contract abort", { message, fileName, line, column });
  };

  const baseEnv = {
    abort: envAbort,
    storage_read: storageRead,
    storage_write: storageWrite,
    storage_save: storageWrite,
    storage_load: storageLoad,
    get_caller_did: (outputPtr: number, maxLen: number): number => {
      const len = ctx.writeString(outputPtr, ctx.callerDid);
      return Math.min(len, maxLen);
    },
    verify_did: (didPtr: number, didLen: number): number => {
      return readDid(ctx, didPtr, didLen).length > 0 ? 1 : 0;
    },
    // Alias both names so WASM targeting either symbol links (Rust VM registers `log` + `log_output`).
    log: (ptr: number, len: number) => {
      ctx.logger.log(ctx.readString(ptr, len));
    },
    log_output: (ptr: number, len: number) => {
      ctx.logger.log(ctx.readString(ptr, len));
    },
    emit_event: (typePtr: number, typeLen: number, dataPtr: number, dataLen: number) => {
      const type = ctx.readString(typePtr, typeLen);
      const data = ctx.readBytes(dataPtr, dataLen);
      ctx.events.push({ type, data });
    },
    msg_value: () => {
      return ctx.msgValue;
    },
    get_balance: (addressPtr: number): bigint => {
      const addrBytes = ctx.readBytes(addressPtr, 20);
      const hex = Array.from(addrBytes).map(b => b.toString(16).padStart(2, "0")).join("");
      const did = `did:spacekit:address:${hex}`;
      return ctx.token.balanceOf(did);
    },
    transfer: (toPtr: number, amount: bigint): number => {
      if (amount <= 0n) return 1;
      const toBytes = ctx.readBytes(toPtr, 20);
      const toHex = Array.from(toBytes).map(b => b.toString(16).padStart(2, "0")).join("");
      const toDid = `did:spacekit:address:${toHex}`;
      const fromDid = toContractDid(ctx.contractId, ctx.callerDid);
      return ctx.token.transfer(fromDid, toDid, amount) ? 0 : 1;
    },
    get_timestamp: () => {
      return getTimestamp();
    },
    reputation_get_score: reputationGetScore,
    reputation_check_threshold: reputationCheckThreshold,
    reputation_get_overall: reputationGetOverall,
    reputation_get_breakdown: reputationGetBreakdown,
    python_compress: pythonCompress,
    python_decompress: pythonDecompress,
  };

  return {
    metering: {
      usegas: useGas,
    },
    env: baseEnv,
    spacekit_storage: {
      storage_save: storageWrite,
      storage_load: storageLoad,
    },
    spacekit_contract: {
      contract_call: (
        contractIdPtr: number,
        contractIdLen: number,
        inputPtr: number,
        inputLen: number,
        outputPtr: number,
        maxLen: number
      ): number => {
        if (!ctx.contractCall) {
          return -1;
        }
        try {
          const contractId = ctx.readString(contractIdPtr, contractIdLen);
          const input = ctx.readBytes(inputPtr, inputLen);
          const callerDid = toContractDid(ctx.contractId, ctx.callerDid);
          const { status, result } = ctx.contractCall.call(
            contractId,
            input,
            callerDid,
            0n
          );
          if (status <= 0) {
            return status;
          }
          const len = Math.min(result.length, maxLen);
          ctx.writeBytes(outputPtr, result.subarray(0, len));
          return len;
        } catch (err) {
          console.error("[SpacekitVM] contract_call error:", err);
          return -2;
        }
      },
    },
    sk_erc20: {
      token_transfer: tokenTransfer,
      token_balance: tokenBalance,
    },
    sk_erc721: {
      nft_mint: nftMint,
      nft_owner_of: nftOwnerOf,
      nft_transfer: nftTransfer,
    },
    spacekit_reputation: {
      reputation_get_score: reputationGetScore,
      reputation_check_threshold: reputationCheckThreshold,
      reputation_get_overall: reputationGetOverall,
      reputation_get_breakdown: reputationGetBreakdown,
    },
    spacekit_fact: {
      fact_package_exists: factExists,
      fact_verify_hash: factVerifyHash,
    },
    spacekit_agent: {
      /**
       * Growformer brain runtime (wasm-bindgen pkg in `growformer-pkg/`).
       *
       * **Not tied to a specific contract deployment:** the VM loads many contract WASMs, but the
       * Growformer host is a single global instance. Initialize with `initGrowformerHost()`. Brains
       * can be loaded via `agent_growformer_load_brain_from_storage` (reads VM `storage` keys seeded
       * at deploy) or `loadGrowformerBrain` / `initGrowformerHostWithBrainFromUrl` from the app.
       * If the wrong brain is loaded, or none, imports return -1 / errors.
       *
       * `agent_growformer_status`: 0 = not ready, 1 = ready (`growformer_ready()` after init + brain).
       */
      agent_growformer_status: agentGrowformerStatus,
      agent_growformer_load_brain_from_storage: agentGrowformerLoadBrainFromStorage,
      /** JSON result written to dest; returns byte length or -1 / -2. */
      agent_growformer_generation: agentGrowformerGeneration,
      agent_growformer_converse: agentGrowformerConverse,
      agent_growformer_codegen: agentGrowformerCodegen,
      agent_growformer_brain_info: agentGrowformerBrainInfo,
      agent_growformer_reset_conversation: agentGrowformerResetConversation,
    },
    spacekit_llm: {
      llm_inference: llmInference,
      llm_status: llmStatus,
    },
    spacekit_microgpt: {
      microgpt_forward: microgptForward,
    },
    spacekit_crypto: {
      /**
       * Verify a SPHINCS+ signature.
       *
       * Returns 1 for a valid signature, 0 for an invalid one, and a negative
       * code when verification could not be performed. The distinction
       * matters: this previously returned 0 unconditionally without a
       * verifier, which a contract following the usual "0 means OK" convention
       * would read as a successful verification.
       */
      sphincs_verify: (
        pkPtr: number, pkLen: number,
        msgPtr: number, msgLen: number,
        sigPtr: number, sigLen: number,
      ): number => {
        const verifier = ctx.sphincsVerifier;
        if (!verifier) {
          console.error(
            "[spacekit_crypto] sphincs_verify called but no sphincsVerifier is configured; " +
              "the signature was NOT verified",
          );
          return SPHINCS_VERIFY_UNAVAILABLE;
        }
        try {
          const publicKey = ctx.readBytes(pkPtr, pkLen);
          const message = ctx.readBytes(msgPtr, msgLen);
          const signature = ctx.readBytes(sigPtr, sigLen);
          return verifier(publicKey, message, signature)
            ? SPHINCS_VERIFY_VALID
            : SPHINCS_VERIFY_INVALID;
        } catch {
          return SPHINCS_VERIFY_BAD_ARGS;
        }
      },
      sha256: (dataPtr: number, dataLen: number, outPtr: number): number => {
        try {
          const input = ctx.readBytes(dataPtr, dataLen);
          const hash = sha256(input);
          ctx.writeBytes(outPtr, hash);
          return 32;
        } catch {
          return -1;
        }
      },
    },

    /* ─── Agent Tool Modules ─────────────────────────────── */

    spacekit_tools: {
      /**
       * Web search: query_ptr/query_len = UTF-8 query string,
       * max_results = cap on results, dest_ptr/max_len = output buffer.
       * Returns bytes written (>0), -1 not configured, -2 error, -3 PENDING.
       */
      web_search: (
        queryPtr: number,
        queryLen: number,
        maxResults: number,
        destPtr: number,
        maxLen: number,
      ): number => {
        const op = ctx.toolOperatorDid;
        if (!op || typeof ctx.messaging?.requestResponse !== "function") {
          return TOOL_STATUS.NOT_CONFIGURED;
        }
        try {
          const query = ctx.readString(queryPtr, queryLen);

          const gate = policyGate(ctx, "web_search", { query, maxResults }, 0);
          if (gate !== 0) return gate;

          const reqBytes = new TextEncoder().encode(
            JSON.stringify({ query, maxResults }),
          );
          const key = toolRequestKey("web_search", reqBytes);
          const cached = ctx.effectManager.getCachedResult(key);
          if (cached) {
            emitToolRecord(ctx, "web_search", 0, "fulfilled");
            return writeSanitizedToolResult(ctx, "web_search", cached, destPtr, maxLen);
          }
          ctx.effectManager.addPending({
            toolName: "web_search",
            requestKey: key,
            requestData: reqBytes,
          });
          emitToolRecord(ctx, "web_search", 0, "pending");
          return TOOL_STATUS.PENDING;
        } catch (e) {
          console.error("[SpacekitVM] web_search error:", e);
          return TOOL_STATUS.ERROR;
        }
      },

    },

    spacekit_messaging: {
      /**
       * Fire-and-forget message send. Buffered and flushed after contract
       * execution completes. recipient_ptr/len = UTF-8 DID,
       * payload_ptr/len = raw payload bytes.
       * Returns 1 on success, -1 not configured, -2 error.
       */
      messaging_send: (
        recipientPtr: number,
        recipientLen: number,
        payloadPtr: number,
        payloadLen: number,
      ): number => {
        if (!ctx.messaging) return TOOL_STATUS.NOT_CONFIGURED;
        try {
          const recipientDid = ctx.readString(recipientPtr, recipientLen);
          const payload = ctx.readBytes(payloadPtr, payloadLen);

          const gate = policyGate(ctx, "messaging_send", { recipientDid, payloadLen: payload.length }, 0);
          if (gate !== 0) return gate;

          ctx.sideEffects.messages.push({
            recipientDid,
            payload: payload.slice(),
          });
          emitToolRecord(ctx, "messaging_send", 0, "fulfilled");
          return 1;
        } catch (e) {
          console.error("[SpacekitVM] messaging_send error:", e);
          return TOOL_STATUS.ERROR;
        }
      },
    },

    spacekit_remote_storage: {
      /**
       * Store data on the SpaceTime Storage Node.
       * data_ptr/data_len = bytes to store, ref_dest/ref_max = buffer for
       * the returned content-addressed ref string.
       * Returns bytes written (>0), -1 not configured, -2 error, -3 PENDING.
       */
      remote_storage_put: (
        dataPtr: number,
        dataLen: number,
        refDest: number,
        refMax: number,
      ): number => {
        if (!ctx.remoteStorage) return TOOL_STATUS.NOT_CONFIGURED;
        try {
          const data = ctx.readBytes(dataPtr, dataLen);

          const gate = policyGate(ctx, "remote_storage_put", { dataLen: data.length }, 0);
          if (gate !== 0) return gate;

          const key = toolRequestKey("remote_storage_put", data);
          const cached = ctx.effectManager.getCachedResult(key);
          if (cached) {
            emitToolRecord(ctx, "remote_storage_put", 0, "fulfilled");
            const n = Math.min(cached.length, refMax);
            ctx.writeBytes(refDest, cached.subarray(0, n));
            return n;
          }
          ctx.effectManager.addPending({
            toolName: "remote_storage_put",
            requestKey: key,
            requestData: data.slice(),
          });
          emitToolRecord(ctx, "remote_storage_put", 0, "pending");
          return TOOL_STATUS.PENDING;
        } catch (e) {
          console.error("[SpacekitVM] remote_storage_put error:", e);
          return TOOL_STATUS.ERROR;
        }
      },

      /**
       * Retrieve data from the SpaceTime Storage Node by ref.
       * ref_ptr/ref_len = UTF-8 ref string, dest/max = output buffer.
       * Returns bytes written (>0), -1 not configured, -2 error, -3 PENDING.
       */
      remote_storage_get: (
        refPtr: number,
        refLen: number,
        destPtr: number,
        maxLen: number,
      ): number => {
        if (!ctx.remoteStorage) return TOOL_STATUS.NOT_CONFIGURED;
        try {
          let ref = ctx.readString(refPtr, refLen);

          const toolDef = ctx.manifest?.tools["remote_storage_get"];
          if (toolDef) {
            ref = applyStorageKeyPrefix(ref, ctx.callerDid, toolDef.constraints);
          }

          const gate = policyGate(ctx, "remote_storage_get", { ref }, 0);
          if (gate !== 0) return gate;

          const reqBytes = new TextEncoder().encode(ref);
          const key = toolRequestKey("remote_storage_get", reqBytes);
          const cached = ctx.effectManager.getCachedResult(key);
          if (cached) {
            emitToolRecord(ctx, "remote_storage_get", 0, "fulfilled");
            return writeSanitizedToolResult(ctx, "remote_storage_get", cached, destPtr, maxLen);
          }
          ctx.effectManager.addPending({
            toolName: "remote_storage_get",
            requestKey: key,
            requestData: reqBytes,
          });
          emitToolRecord(ctx, "remote_storage_get", 0, "pending");
          return TOOL_STATUS.PENDING;
        } catch (e) {
          console.error("[SpacekitVM] remote_storage_get error:", e);
          return TOOL_STATUS.ERROR;
        }
      },
    },

    spacekit_payments: {
      /**
       * Fire-and-forget transfer. Buffered and flushed after execution.
       * Returns 1 on success, -1 not configured, -2 error.
       */
      payment_transfer: (
        toPtr: number,
        toLen: number,
        assetPtr: number,
        assetLen: number,
        amount: bigint,
      ): number => {
        if (!ctx.payment) return TOOL_STATUS.NOT_CONFIGURED;
        try {
          const to = ctx.readString(toPtr, toLen);
          const asset = ctx.readString(assetPtr, assetLen);

          const gate = policyGate(ctx, "payment_transfer", { to, asset, amount }, 0);
          if (gate !== 0) return gate;

          ctx.sideEffects.payments.push({
            effect: { type: "transfer", to, asset, amount: amount.toString() },
          });
          emitToolRecord(ctx, "payment_transfer", 0, "fulfilled");
          return 1;
        } catch (e) {
          console.error("[SpacekitVM] payment_transfer error:", e);
          return TOOL_STATUS.ERROR;
        }
      },

      /**
       * Fire-and-forget vault charge. Buffered and flushed after execution.
       * Returns 1 on success, -1 not configured, -2 error.
       */
      payment_vault_charge: (
        amountPtr: number,
        amountLen: number,
        beneficiaryPtr: number,
        beneficiaryLen: number,
      ): number => {
        if (!ctx.payment) return TOOL_STATUS.NOT_CONFIGURED;
        try {
          const amount = ctx.readString(amountPtr, amountLen);
          const beneficiary = ctx.readString(beneficiaryPtr, beneficiaryLen);

          const gate = policyGate(ctx, "payment_vault_charge", { amount, beneficiary }, 0);
          if (gate !== 0) return gate;

          ctx.sideEffects.payments.push({
            effect: {
              type: "vault_charge",
              to: beneficiary,
              asset: "ausd",
              amount,
              beneficiary,
            },
          });
          emitToolRecord(ctx, "payment_vault_charge", 0, "fulfilled");
          return 1;
        } catch (e) {
          console.error("[SpacekitVM] payment_vault_charge error:", e);
          return TOOL_STATUS.ERROR;
        }
      },
    },

    spacekit_session: {
      /**
       * Owner = ctx.callerDid. Writes new session id (64-char hex) UTF-8 to dest.
       * Returns bytes written, or negative error code.
       */
      session_create: (
        delegatePtr: number,
        delegateLen: number,
        scopePtr: number,
        scopeLen: number,
        expiresAt: bigint,
        destPtr: number,
        destMax: number,
      ): number => {
        try {
          const ownerDid = ctx.callerDid;
          const delegateDid = ctx.readString(delegatePtr, delegateLen);
          const scopeRaw = ctx.readString(scopePtr, scopeLen);
          const expNs =
            typeof expiresAt === "bigint" ? expiresAt : BigInt(expiresAt);
          if (expNs < 0n || expNs > BigInt(Number.MAX_SAFE_INTEGER)) {
            return TOOL_STATUS.ERROR;
          }
          const idBytes = ctx.sessionHost.create(
            ownerDid,
            delegateDid,
            scopeRaw,
            Number(expNs),
          );
          if (idBytes.length > destMax) {
            return TOOL_STATUS.ERROR;
          }
          ctx.writeBytes(destPtr, idBytes);
          return idBytes.length;
        } catch (e) {
          console.error("[SpacekitVM] session_create error:", e);
          return TOOL_STATUS.ERROR;
        }
      },

      /** Caller = delegate. Returns 1 valid, 0 invalid/expired, negative on error. */
      session_validate: (
        ownerPtr: number,
        ownerLen: number,
        operationPtr: number,
        operationLen: number,
      ): number => {
        try {
          const ownerDid = ctx.readString(ownerPtr, ownerLen);
          const operation = ctx.readString(operationPtr, operationLen);
          return ctx.sessionHost.validate(ctx.callerDid, ownerDid, operation);
        } catch (e) {
          console.error("[SpacekitVM] session_validate error:", e);
          return TOOL_STATUS.ERROR;
        }
      },

      /** Owner = ctx.callerDid. Returns 1 ok, negative on error. */
      session_revoke: (sessionIdPtr: number, sessionIdLen: number): number => {
        try {
          const sessionId = ctx.readString(sessionIdPtr, sessionIdLen);
          return ctx.sessionHost.revoke(ctx.callerDid, sessionId)
            ? 1
            : TOOL_STATUS.ERROR;
        } catch (e) {
          console.error("[SpacekitVM] session_revoke error:", e);
          return TOOL_STATUS.ERROR;
        }
      },
    },

    spacekit_paymaster: {
      /** Sponsor = ctx.callerDid. Policy JSON per SDK `paymaster_set_policy`. */
      paymaster_set_policy: (policyPtr: number, policyLen: number): number => {
        try {
          const sponsorDid = ctx.callerDid;
          const json = ctx.readString(policyPtr, policyLen);
          ctx.paymasterHost.setPolicy(sponsorDid, json);
          return 1;
        } catch (e) {
          console.error("[SpacekitVM] paymaster_set_policy error:", e);
          return TOOL_STATUS.ERROR;
        }
      },

      /**
       * Validates in-memory sponsor policy + budget, decrements budget, buffers
       * optional `PaymentAdapter.sponsorVaultCharge` flush.
       */
      paymaster_sponsor_charge: (
        sponsorPtr: number,
        sponsorLen: number,
        amountPtr: number,
        amountLen: number,
        operationPtr: number,
        operationLen: number,
      ): number => {
        try {
          const sponsorDid = ctx.readString(sponsorPtr, sponsorLen);
          const amount = ctx.readString(amountPtr, amountLen);
          const operation = ctx.readString(operationPtr, operationLen);
          const ok = ctx.paymasterHost.trySponsorCharge(
            ctx.callerDid,
            sponsorDid,
            amount,
            operation,
          );
          if (!ok) {
            return TOOL_STATUS.ERROR;
          }
          ctx.sideEffects.payments.push({
            effect: {
              type: "sponsor_vault_charge",
              to: sponsorDid,
              asset: "ausd",
              amount,
              beneficiary: ctx.callerDid,
              sponsorDid,
              operation,
            },
          });
          return 1;
        } catch (e) {
          console.error("[SpacekitVM] paymaster_sponsor_charge error:", e);
          return TOOL_STATUS.ERROR;
        }
      },

      /** Writes UTF-8 decimal budget string for sponsor (not necessarily caller). */
      paymaster_budget: (
        sponsorPtr: number,
        sponsorLen: number,
        destPtr: number,
        destMax: number,
      ): number => {
        try {
          const sponsorDid = ctx.readString(sponsorPtr, sponsorLen);
          const s = ctx.paymasterHost.getBudgetString(sponsorDid);
          const enc = new TextEncoder().encode(s);
          if (enc.length > destMax) {
            return TOOL_STATUS.ERROR;
          }
          ctx.writeBytes(destPtr, enc);
          return enc.length;
        } catch (e) {
          console.error("[SpacekitVM] paymaster_budget error:", e);
          return TOOL_STATUS.ERROR;
        }
      },
    },

  };
}

export { HostContextImpl };
