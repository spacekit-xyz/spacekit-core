//! SWTCHVM (SWTCH WebAssembly Virtual Machine) Module
//!
//! Provides WebAssembly execution with quantum-resistant security integration

#[cfg(feature = "growformer-inference")]
pub mod growformer_host;
pub mod l1_checkpoint;
pub mod swtchvm_node;
pub mod swtchvm_sdk;
pub mod tool_policy;
// pub mod cost;  // Disabled due to wasmtime version issues
#[cfg(feature = "gpu")]
pub mod calculation;
pub mod cost_simple;
#[cfg(feature = "gpu")]
pub mod hybrid_calculation;
// pub mod resource; // Removed - was demo code only
#[cfg(feature = "storage-integration")]
pub mod archive_node;
#[cfg(feature = "storage-integration")]
pub mod blockchain_storage;
pub mod collaborative_storage;
#[cfg(feature = "storage-integration")]
pub mod enhanced_rpc;
pub mod genesis_node;
pub mod specialized_storage;
pub mod storage;

// Re-export key types and functions for easy access
pub use l1_checkpoint::{
    manifest_path_for_snapshot, minimal_l1_manifest_for_proposal, persist_swvm_snapshot,
    read_manifest_optional, tx_batch_verkle_checkpoint_fields, tx_batch_verkle_root_hex,
    unwrap_snapshot_file_bytes, verify_l1_tx_batch_witness_json, verify_manifest_against_loaded,
    wrap_snapshot_payload, zero_hash_hex, L1CheckpointHeader, L1PersistenceConfig,
    SnapshotManifest, TxBatchVerkleWitnessV1, SNAPSHOT_MAGIC, SNAPSHOT_MANIFEST_VERSION,
    SNAPSHOT_WIRE_VERSION, TX_BATCH_VERKLE_ADDRESS, TX_ROOT_SCHEME_QUANTUM_VERKLE_V1,
};
pub use swtchvm_node::{
    FaucetRequestBody, FaucetResponse, SwtchvmAccount, SwtchvmAddress, SwtchvmBlock,
    SwtchvmContext, SwtchvmExecutionResult, SwtchvmGasSchedule, SwtchvmLog, SwtchvmNetworking,
    SwtchvmNode, SwtchvmReceipt, SwtchvmRuntime, SwtchvmState, SwtchvmTransaction,
    TransactionSignature,
};

pub use swtchvm_sdk::{
    AbiEvent, AbiFunction, AbiParameter, ContractAbi, ContractDeployment, SwtchvmClient,
    SwtchvmContract, SwtchvmValue,
};

pub use cost_simple::{
    CostAwareExecutor, CostBreakdown, CostConfig, ExecutionCost, ExecutionMetrics, MeteredStore,
    WasmCostCalculator,
};

#[cfg(feature = "gpu")]
pub use hybrid_calculation::{
    ExecutionPath, ExecutionRecord, HybridComputeManager, HybridExecutionCost, MemoryPattern,
    PerformanceInsights, PerformanceMetrics, PrecisionLevel, WorkloadAnalyzer, WorkloadClassifier,
    WorkloadProfile,
};

// #[cfg(feature = "gpu")]
// pub use calculation::{
//     // Re-export calculation types if they exist
// };

// pub use resource::{
//     // Re-export resource management types if they exist
// };

pub use storage::{
    AccessControlEntry, ComputeAllocation, ComputeRequest, DistributedStorage, EncryptedChunk,
    FilePermissions, ProviderReputation, QuantumSafeStorage, ReputationComputeMarketplace,
    ReputationScore, StorageContractConfig, StorageCost, StorageResult, StorageSmartContract,
    StorageStats, UserReputation,
};

#[cfg(feature = "storage-integration")]
pub use spacekit_storage_node::database::FileMetadata;

#[cfg(feature = "storage-integration")]
pub use blockchain_storage::{
    BlockchainStorage, BlockchainStorageConfig, BlockchainStorageStats, TransactionMetadata,
};

pub use genesis_node::{
    AccountType, ConsensusAlgorithm, ConsensusConfig, GenesisAccount, GenesisConfig, GenesisNode,
    GenesisNodeCli, NetworkConstants,
};

#[cfg(feature = "storage-integration")]
pub use enhanced_rpc::{
    EnhancedRpcServer, JsonRpcError, JsonRpcRequest, JsonRpcResponse, RpcContext, RpcMethodHandler,
    RpcServerConfig,
};

#[cfg(feature = "storage-integration")]
pub use archive_node::{
    ArchiveNode, ArchiveNodeConfig, BalanceHistoryEntry, BlockchainAnalytics, BlockchainIndexer,
    ContractInfo, GasUsageInfo, NetworkStatistics,
};
