export { createHost, createImports, LLM_STATUS } from "./host.js";
export type {
  HostOptions,
  HostContext,
  TokenAdapter,
  NftAdapter,
  ReputationAdapter,
  FactAdapter,
  CompressionAdapter,
  LlmAdapter,
  LlmStatus,
  CapturedLlmRequest,
  Logger,
} from "./host.js";
export {
  registerLlmAdapter,
  unregisterLlmAdapter,
  listLlmAdapters,
  setActiveLlmAdapter,
  getActiveLlmAdapter,
} from "./llm/registry.js";
export { WebLlmAdapter } from "./llm/web_adapter.js";
export type { LlmChatEngine, LlmChatMessage, TRMConfig } from "./llm/web_adapter.js";
export { microgpt_forward, MICROGPT_VOCAB_SIZE } from "./llm/microgpt_forward.js";
export {
  initGrowformerHost,
  initGrowformerHostWithBrainFromUrl,
  fetchGrowformerBrainBytes,
  DEFAULT_GROWFORMER_BRAIN_MAX_BYTES,
  isGrowformerHostReady,
  isGrowformerModuleLoaded,
  growformerHostStatusCode,
  loadGrowformerBrain,
  clearGrowformerBrainCache,
  clearGrowformerInferenceTomlCache,
  clearGrowformerGuardrailsCache,
  clearGrowformerFragmentsCache,
  clearGrowformerTopicGraphCache,
  clearGrowformerGroundingGraphCache,
  loadGrowformerInferenceToml,
  loadGrowformerInferenceGuardrailsJsonl,
  loadGrowformerFragmentsJsonl,
  loadGrowformerTopicGraph,
  loadGrowformerGroundingGraph,
  setGrowformerAgentState,
  resetGrowformerConversation,
  growformerHostGenerationJson,
  growformerHostConverseJson,
  growformerHostCodegenJson,
  growformerHostBrainInfoJson,
} from "./growformer/runtime.js";
export type {
  GrowformerInitInput,
  GrowformerBrainFetchPhase,
  GrowformerBrainFetchProgress,
  GrowformerBrainFetcher,
  GrowformerBrainFetchOptions,
  InitGrowformerHostWithBrainFromUrlOptions,
} from "./growformer/runtime.js";
export {
  configureGrowformerCapture,
  newGrowformerCaptureSession,
  isGrowformerCaptureEnabled,
  setGrowformerCaptureUploader,
  recordGrowformerTraffic,
  flushGrowformerCapture,
  exportGrowformerCaptureJsonl,
  growformerCaptureBufferSize,
  clearGrowformerCaptureBuffer,
} from "./growformer/capture.js";
export type { GrowformerTrafficCapture, GrowformerCaptureUploader } from "./growformer/capture.js";
export { createInMemoryStorage, StorageNodeAdapter, createStorageNodeRemoteBlobAdapter, IndexedDbStorageAdapter, syncWithStorageNode, bytesToHex, hexToBytes } from "./storage.js";
export {
  parseEnvelopeHeader, decryptEnvelope, decryptEnvelopeChunks, decryptChunk,
  encryptEnvelope, encryptEnvelopeWithFileKey,
  requestChallenge, fetchEnvelope, fetchEnvelopeStream, uploadEnvelope,
} from "./envelope.js";
export type {
  EnvelopeHeader, EncryptedFileKey, EncryptedChallenge, ChallengeResponse, ChunkMeta,
} from "./envelope.js";
export {
  EntitlementOp, EntitlementStatus, ENTITLEMENT_EVENT,
  buildCreateListingInput, buildPurchaseInput, buildVerifyInput, parsePurchaseResult,
  buildGrantInput, parseGrantResult, buildRevokeInput,
  buyerPkHashFromPublicKeyHex,
  fetchRewrappedEnvelope, purchaseAndDownload,
  uploadDeliveryCapsule, grantAndPrepareDelivery, downloadWithEntitlement,
} from "./entitlement.js";
export type {
  RewrapOptions, PurchaseAndDownloadOptions,
  UploadDeliveryCapsuleOptions, GrantAndPrepareDeliveryOptions,
  DownloadWithEntitlementOptions,
} from "./entitlement.js";
export { instantiateWasm, callSpacekitMain } from "./runtime.js";
// Contract clients are available via "@spacekit/spacekit-js/contracts"
// e.g. import { SkErc20Client } from "@spacekit/spacekit-js/contracts";
export type {
  SpacekitVmOptions,
  MeteringCostTable,
  Block,
  Receipt,
  Transaction,
  TransactionSignature,
  SealedArchive,
  StateProof,
  StateSnapshot,
  TxProof,
  ReceiptProof,
  BlockHeader,
  FeePolicy,
  GasPolicy,
  PqSignatureVerifier,
  GenesisConfig,
  DidDocument,
  DidResolver,
  SecureBlockHeader,
  QuantumStateProof,
  VerkleWitness,
} from "./vm/spacekitvm.js";
export { VerkleStateManager } from "./vm/verkle_state.js";
export type { AccessRecord } from "./vm/verkle_state.js";
export { SpacekitVm } from "./vm/spacekitvm.js";
export { loadQuantumVerkleWasm } from "./quantum_verkle.js";
export { verifyQuantumVerkleProof } from "./vm/quantum_verkle.js";
export { loadQuantumDidWasm } from "./quantum_did.js";
export type { QuantumDidWasmModule, QuantumDidWasmLoaderOptions, SlhDsaKeypair } from "./quantum_did.js";
export type { QuantumVerkleOptions } from "./vm/quantum_verkle.js";

// Signature verification utilities
export type {
  SignatureAlgorithm,
  SignedMessage,
  SignatureVerifier,
  PqSignatureVerifierFunc,
} from "./vm/signatures.js";
export {
  verifyEd25519,
  signEd25519,
  generateEd25519Keypair,
  createSignatureVerifier,
  createTransactionMessage,
  hashTransactionMessage,
  signTransaction,
  verifyTransactionSignature,
} from "./vm/signatures.js";

// Genesis and DID resolution utilities
export {
  DEFAULT_GENESIS_CONFIG,
  TESTNET_GENESIS_CONFIG,
  TESTNET_FUNDED_ADDRESSES,
  TESTNET_ASTRA_PER_ACCOUNT,
  TESTNET_AUSD_PER_ACCOUNT,
  NETWORK_DECIMAL_PRESETS,
  getGenesisPresetForNetwork,
  computeGenesisHash,
  computeGenesisHashSync,
  getGenesisCanonical,
  isProtectedKey,
  PROTECTED_PREFIXES,
  createDidDocument,
  didDocumentKey,
  createDidResolver,
} from "./vm/genesis.js";
export type {
  NativeCurrencyConfig,
  DidRegistration,
  SupplyState,
} from "./vm/genesis.js";
export type { JsonRpcRequest, JsonRpcResponse } from "./vm/json_rpc.js";
export { createJsonRpcHandler } from "./vm/json_rpc.js";
export type {
  RollupBundle,
  SequencerOptions,
  BundleSignature,
  SignedRollupBundle,
  BundleSigningOptions,
} from "./vm/sequencer.js";
export { SpacekitSequencer } from "./vm/sequencer.js";
export type {
  ProofBridgePayload,
  ProofBridgeAdapter,
  ProofBridgeSubmitResult,
  ProofBridgeChainConfig,
  ProofBridgeConfig,
  AdapterRegistry,
} from "./vm/proof_bridge.js";
export {
  loadProofBridgeConfig,
  createAdaptersFromConfig,
  substituteEnvInChainConfig,
  setDefaultAdapterRegistry,
} from "./vm/proof_bridge.js";
export type { RpcServerOptions } from "./vm/http_rpc_server.js";
export { startJsonRpcServer } from "./vm/http_rpc_server.js";
export type { AutoSyncOptions } from "./vm/autosync.js";
export { VmAutoSync } from "./vm/autosync.js";
export { HOST_ABI_VERSION, HOST_IMPORT_MODULES } from "./vm/abi.js";
export type { MerkleStep } from "./vm/merkle.js";
export { verifyMerkleProof, verifyMerkleProofFromHash } from "./vm/merkle.js";
export type {
  ComputeNodeBlockHeader,
  ComputeNodeTxProof,
  ComputeNodeReceiptProof,
  ComputeNodeStateProof,
} from "./vm/light_client.js";
export {
  verifyComputeNodeTxProof,
  verifyComputeNodeReceiptProof,
  verifyComputeNodeStateProof,
  verifyComputeNodeQuantumStateProof,
} from "./vm/light_client.js";
export type { HeaderSyncClientOptions, HeaderSyncResult } from "./vm/header_sync.js";
export { HeaderSyncClient } from "./vm/header_sync.js";

// Block store for persistent block storage
export type { BlockStoreOptions, BlockStoreStats } from "./vm/blockstore.js";
export { IndexedDbBlockStore } from "./vm/blockstore.js";
export type { HeaderCacheEntry } from "./vm/header_cache.js";
export { IndexedDbHeaderCache } from "./vm/header_cache.js";
export type { SnapshotProgressEntry } from "./vm/snapshot_progress.js";
export { IndexedDbSnapshotProgress } from "./vm/snapshot_progress.js";

export type {
  StateSnapshotDocument,
  StateSnapshotChunk,
  StateSnapshotEntry,
  SnapshotVerifyOptions,
  SnapshotDownloadOptions,
  SnapshotDeltaOptions,
} from "./vm/snapshot_client.js";
export {
  fetchStateSnapshot,
  verifySnapshotAgainstHeader,
  fetchSnapshotChunk,
  verifySnapshotChunks,
  verifySnapshotChunksResumable,
  downloadStateSnapshot,
  downloadStateSnapshotDelta,
} from "./vm/snapshot_client.js";

// Session persistence for VM state recovery
export type {
  ContractDeployment,
  SessionState,
  SessionStoreOptions,
} from "./vm/session.js";
export {
  SessionStore,
  createContractDeployment,
  getWasmFromDeployment,
} from "./vm/session.js";

export type {
  AgentProfile,
  Did,
  Post,
  SpaceTimeConfig,
  SpaceTimeStorage,
  Thread,
} from "./spacetime/types.js";
export { SpaceTimeClient } from "./spacetime/client.js";
export { createSpaceTimeStorage } from "./spacetime/storage.js";
export {
  createSpaceTimeStorageNode,
  createSpaceTimeStorageNodeWithFallback,
} from "./spacetime/storage_remote.js";
export {
  parseSpaceTimeCommand,
  type SpaceTimeAgentAction,
} from "./spacetime/commands.js";
export {
  routeSpaceTimeMessage,
  type ClassifierResult,
} from "./spacetime/router.js";
export type {
  SpacekitMessageKind,
  SpacekitMessageContext,
  SpacekitMessageEnvelope,
} from "./spacetime/message.js";
export { createSpacekitMessage } from "./spacetime/message.js";
export {
  routeSpacekitMessage,
  type SpacekitMessageRouterOptions,
  type SpacekitMessageRouterResult,
} from "./spacetime/message_router.js";
export type {
  SpacekitMessagingAdapter,
  MessageListener,
} from "./spacetime/messaging_adapter.js";
export {
  NoopMessagingAdapter,
  HttpMessagingAdapter,
  LocalMessagingAdapter,
} from "./spacetime/messaging_adapter.js";
export {
  ingestSpacekitTextMessage,
  ingestSpacekitEnvelope,
} from "./spacetime/message_pipeline.js";
export {
  createVmContractCaller,
  JsonContractCodec,
  type ContractCallCodec,
  type ContractCallMode,
  type VmContractCallerOptions,
} from "./vm/contract_caller.js";
export { SpaceTimeJsonCodec, type SpaceTimeMethod } from "./spacetime/codec.js";
export type {
  SpaceTimeThread as StorageContractThread,
  SpaceTimePost as StorageContractPost,
  SpaceTimeAgentProfile as StorageContractAgentProfile,
} from "./spacetime/storage_contract_types.js";
export { createSpaceTimeStorageContractCaller } from "./spacetime/storage_contract.js";

// Platform utilities for multi-runtime support (browser, Node.js, Bun)
export {
  detectRuntime,
  isServerRuntime,
  installPolyfills,
} from "./platform.js";
export type { SpacekitRuntime } from "./platform.js";

// Intent-based payment builder
export {
  IntentBuilder,
  estimateIntentFees,
  buildExecuteContractIntent,
} from "./intent_builder.js";
export type {
  ExecuteContractAction,
  VaultChargeAction,
  TransferAction,
  IntentAction,
  IntentConstraints,
  Intent,
  SignedIntent,
  FeeEstimate,
  IntentSignerFn,
  FeeEstimatorFn,
} from "./intent_builder.js";

// Canonical intent signing payload — must match the node's `intent_auth`
export {
  INTENT_DOMAIN,
  MAX_INTENT_LIFETIME_SECS,
  canonicalJson,
  canonicalIntentPayload,
  canonicalIntentPayloadBytes,
  assertSignableExpiry,
} from "./intent_canonical.js";

// Agent tool infrastructure
export {
  createToolEffectManager,
  toolRequestKey,
  TOOL_STATUS,
  STATUS_NEEDS_TOOLS,
  MAX_TOOL_ROUNDS,
} from "./tools/effect_manager.js";
export type {
  ToolEffect,
  ToolEffectManager,
} from "./tools/effect_manager.js";
export type {
  SearchResult,
  RemoteStorageAdapter,
  PaymentEffect,
  PaymentAdapter,
  MessagingAdapter,
  BufferedMessage,
  BufferedPayment,
  ToolSideEffects,
} from "./tools/types.js";
export { createToolSideEffects, SPACEKIT_WEB_SEARCH_TOPIC } from "./tools/types.js";
export {
  IntentMessagingToolAdapter,
} from "./tools/intent_messaging_adapter.js";
export type { IntentMessagingAdapterOptions } from "./tools/intent_messaging_adapter.js";
export { HttpPaymentAdapter, NoopPaymentAdapter } from "./tools/payments_adapter.js";
export type { PaymentAdapterOptions } from "./tools/payments_adapter.js";