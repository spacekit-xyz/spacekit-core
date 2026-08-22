use chrono::{DateTime, Utc};
/// Common Transaction and Execution Results for SWTCH Network
///
/// Provides shared result structures that can be used across different
/// components of the SWTCH network (tokens, compute, storage, etc.)
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Base transaction result that all SWTCH operations should implement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseTransactionResult {
    pub success: bool,
    pub transaction_hash: String,
    pub gas_used: u64,
    pub timestamp: DateTime<Utc>,
    pub error: Option<String>,
}

/// Extended transaction result with network-specific fields
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwtchTransactionResult {
    #[serde(flatten)]
    pub base: BaseTransactionResult,

    /// Block number where transaction was included
    pub block_number: Option<u64>,

    /// Network fees paid
    pub network_fee: u64,

    /// DID of transaction initiator
    pub initiator_did: Option<String>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Execution metrics common across different execution types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseExecutionMetrics {
    pub execution_time_ms: u64,
    pub memory_peak_mb: u64,
    pub compute_units_used: u64,
    pub energy_consumed_kwh: f64,
}

/// Extended execution result for compute operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeExecutionResult {
    #[serde(flatten)]
    pub transaction: SwtchTransactionResult,

    #[serde(flatten)]
    pub metrics: BaseExecutionMetrics,

    /// Result data from execution
    pub result_data: Vec<u8>,

    /// CPU-specific metrics
    pub cpu_time_ms: u64,

    /// GPU-specific metrics (if applicable)
    pub gpu_time_ms: Option<u64>,

    /// Memory operations
    pub memory_operations: u64,

    /// Storage operations
    pub storage_operations: u64,
}

/// Token operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenTransactionResult {
    #[serde(flatten)]
    pub transaction: SwtchTransactionResult,

    /// Amount involved in the transaction
    pub amount: u128,

    /// Token contract address
    pub token_address: String,

    /// Operation type (transfer, approve, stake, etc.)
    pub operation_type: String,

    /// Balances after transaction
    pub balances_updated: HashMap<String, u128>,
}

impl TokenTransactionResult {
    /// Convenience method to access success status
    pub fn success(&self) -> bool {
        self.transaction.base.success
    }

    /// Convenience method to access error message
    pub fn error(&self) -> &Option<String> {
        &self.transaction.base.error
    }
}

/// Storage operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageTransactionResult {
    #[serde(flatten)]
    pub transaction: SwtchTransactionResult,

    /// File or chunk identifier
    pub file_id: String,

    /// Storage operation type
    pub operation_type: StorageOperation,

    /// Bytes stored/retrieved
    pub bytes_processed: u64,

    /// Encryption algorithm used
    pub encryption_algorithm: String,

    /// Replication factor
    pub replication_factor: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageOperation {
    Store,
    Retrieve,
    Delete,
    Replicate,
    Verify,
}

/// Utility functions for creating common results
impl BaseTransactionResult {
    pub fn success(transaction_hash: String, gas_used: u64) -> Self {
        Self {
            success: true,
            transaction_hash,
            gas_used,
            timestamp: Utc::now(),
            error: None,
        }
    }

    pub fn failure(error: String, gas_used: u64) -> Self {
        Self {
            success: false,
            transaction_hash: "".to_string(),
            gas_used,
            timestamp: Utc::now(),
            error: Some(error),
        }
    }
}

impl SwtchTransactionResult {
    pub fn from_base(base: BaseTransactionResult) -> Self {
        Self {
            base,
            block_number: None,
            network_fee: 0,
            initiator_did: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_block(mut self, block_number: u64) -> Self {
        self.block_number = Some(block_number);
        self
    }

    pub fn with_fee(mut self, network_fee: u64) -> Self {
        self.network_fee = network_fee;
        self
    }

    pub fn with_initiator(mut self, did: String) -> Self {
        self.initiator_did = Some(did);
        self
    }
}

/// Helper trait for converting between result types
pub trait IntoSwtchResult {
    fn into_swtch_result(self) -> SwtchTransactionResult;
}

impl IntoSwtchResult for BaseTransactionResult {
    fn into_swtch_result(self) -> SwtchTransactionResult {
        SwtchTransactionResult::from_base(self)
    }
}
