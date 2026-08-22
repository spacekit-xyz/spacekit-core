//! LayerZero Cross-Chain Bridge Integration for SpaceKit Compute Node
//!
//! Enables cross-chain token transfers, compute task execution, and reward distribution
//! using LayerZero V2 omnichain infrastructure.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};
use uuid::Uuid;

use crate::{ComputeResult, ComputeTask, ExecutionMetrics, TokenMintResult};

// Alloy dependencies for Web3 integration (required for LayerZero bridge)
use alloy_contract::{ContractInstance, Interface};
use alloy_network::Ethereum;
use alloy_network::{TransactionBuilder, TxSigner};
use alloy_primitives::{Address, Bytes, FixedBytes, B256, U256};
use alloy_provider::{Provider as AlloyProvider, ProviderBuilder, RootProvider};
use alloy_rpc_types::{TransactionInput, TransactionReceipt, TransactionRequest};
use alloy_signer::Signer as AlloySigner;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{sol, SolCall, SolValue};
use alloy_transport_http::{Client, Http as HttpTransport};

/// LayerZero endpoint IDs for supported chains
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SupportedChain {
    Ethereum = 30101,
    Arbitrum = 30110,
    Polygon = 30109,
    Avalanche = 30106,
    Optimism = 30111,
    Base = 30184,
    BNBChain = 30102,
}

impl SupportedChain {
    pub fn endpoint_id(&self) -> u32 {
        *self as u32
    }

    pub fn name(&self) -> &'static str {
        match self {
            SupportedChain::Ethereum => "Ethereum",
            SupportedChain::Arbitrum => "Arbitrum",
            SupportedChain::Polygon => "Polygon",
            SupportedChain::Avalanche => "Avalanche",
            SupportedChain::Optimism => "Optimism",
            SupportedChain::Base => "Base",
            SupportedChain::BNBChain => "BNB Chain",
        }
    }

    pub fn from_endpoint_id(eid: u32) -> Option<Self> {
        match eid {
            30101 => Some(SupportedChain::Ethereum),
            30110 => Some(SupportedChain::Arbitrum),
            30109 => Some(SupportedChain::Polygon),
            30106 => Some(SupportedChain::Avalanche),
            30111 => Some(SupportedChain::Optimism),
            30184 => Some(SupportedChain::Base),
            30102 => Some(SupportedChain::BNBChain),
            _ => None,
        }
    }
}

/// Configuration for LayerZero bridge operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerZeroBridgeConfig {
    /// Enable/disable cross-chain bridging
    pub enabled: bool,

    /// SpaceKit network endpoint ID (custom)
    pub spacekit_endpoint_id: u32,

    /// Bridge contract addresses by chain
    pub bridge_contracts: HashMap<SupportedChain, String>,

    /// Supported token mappings (original -> wrapped)
    pub token_mappings: HashMap<SupportedChain, TokenBridgeMapping>,

    /// Gas limits for different operations
    pub gas_limits: BridgeGasLimits,

    /// Bridge fees configuration
    pub bridge_fees: BridgeFeeConfig,

    /// Cross-chain execution settings
    pub cross_chain_execution: CrossChainExecutionConfig,

    /// Use OFT instead of lock-and-mint (migration flag)
    pub use_oft: bool,

    /// OFT contract addresses by chain (if use_oft is true)
    pub oft_contracts: HashMap<SupportedChain, String>,

    /// RPC endpoints for each chain
    pub rpc_endpoints: HashMap<SupportedChain, String>,

    /// Private key for signing transactions (hex string without 0x)
    pub signer_private_key: Option<String>,

    /// When true, bridge operations return synthetic success without RPC or a signer (unit tests only).
    #[serde(default)]
    pub mock_chain_transactions: bool,
}

/// Token bridge mapping for each chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBridgeMapping {
    /// Native ASTRA token address on source chain
    pub astra_token: String,

    /// Wrapped ASTRA token address on destination chain (optional for OFT mode)
    pub wrapped_astra: Option<String>,

    /// USDC token address (for payments)
    pub usdc_token: String,

    /// Other supported tokens
    pub supported_tokens: HashMap<String, String>, // original -> wrapped
}

/// Gas limits for bridge operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeGasLimits {
    /// Gas for token bridging
    pub bridge_token: u64,

    /// Gas for compute task execution
    pub execute_task: u64,

    /// Gas for reward distribution
    pub distribute_reward: u64,

    /// Gas for status updates
    pub status_update: u64,
}

/// Bridge fee configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeFeeConfig {
    /// Base fee percentage (0.0 to 1.0)
    pub base_fee_percentage: f64,

    /// LayerZero message fee buffer percentage
    pub message_fee_buffer: f64,

    /// Minimum bridge amount (in wei)
    #[serde(with = "crate::serde_u128")]
    pub minimum_bridge_amount: u128,

    /// Maximum bridge amount (in wei)
    #[serde(with = "crate::serde_u128")]
    pub maximum_bridge_amount: u128,
}

/// Cross-chain execution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainExecutionConfig {
    /// Enable cross-chain compute task execution
    pub enabled: bool,

    /// Maximum task execution time (seconds)
    pub max_execution_time: u64,

    /// Supported compute runtimes for cross-chain
    pub supported_runtimes: Vec<String>,

    /// Auto-retry failed executions
    pub auto_retry: bool,

    /// Maximum retry attempts
    pub max_retries: u32,
}

/// Cross-chain token transfer request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainTokenTransfer {
    /// Unique transfer ID
    pub transfer_id: String,

    /// Source chain
    pub source_chain: SupportedChain,

    /// Destination chain
    pub destination_chain: SupportedChain,

    /// Token address on source chain
    pub source_token: String,

    /// Token address on destination chain
    pub destination_token: String,

    /// Transfer amount (in wei)
    pub amount: u128,

    /// Recipient address
    pub recipient: String,

    /// Sender DID for authentication
    pub sender_did: String,

    /// Transfer status
    pub status: BridgeStatus,

    /// Transaction hashes
    pub source_tx_hash: Option<String>,
    pub destination_tx_hash: Option<String>,

    /// Timestamps
    pub initiated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Cross-chain compute task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainTaskExecution {
    /// Unique execution ID
    pub execution_id: String,

    /// Original compute task
    pub task: ComputeTask,

    /// Source chain where task was submitted
    pub source_chain: SupportedChain,

    /// Execution chain (where compute actually happens)
    pub execution_chain: SupportedChain,

    /// Reward distribution chain
    pub reward_chain: SupportedChain,

    /// Execution status
    pub status: BridgeStatus,

    /// Execution result (when completed)
    pub result: Option<ComputeResult>,

    /// Cross-chain transaction hashes
    pub task_submission_tx: Option<String>,
    pub execution_confirmation_tx: Option<String>,
    pub reward_distribution_tx: Option<String>,

    /// Timestamps
    pub submitted_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Cross-chain reward distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainReward {
    /// Reward ID
    pub reward_id: String,

    /// Task or service ID
    pub task_id: String,

    /// Provider DID
    pub provider_did: String,

    /// Reward amount (in ASTRA wei)
    pub amount: u128,

    /// Source chain (where task was executed)
    pub source_chain: SupportedChain,

    /// Destination chain (where reward is distributed)
    pub destination_chain: SupportedChain,

    /// Reward type
    pub reward_type: RewardType,

    /// Distribution status
    pub status: BridgeStatus,

    /// Transaction hashes
    pub source_tx_hash: Option<String>,
    pub destination_tx_hash: Option<String>,

    /// Timestamps
    pub created_at: DateTime<Utc>,
    pub distributed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
struct OftSendParams {
    amount: u128,
    dst_eid: u32,
    recipient: String,
}

/// OFT Contract ABI - minimal interface for send operations
const OFT_ABI: &str = r#"[
    {
        "inputs": [
            {
                "components": [
                    {"name": "dstEid", "type": "uint32"},
                    {"name": "to", "type": "bytes32"},
                    {"name": "amountLD", "type": "uint256"},
                    {"name": "minAmountLD", "type": "uint256"},
                    {"name": "extraOptions", "type": "bytes"},
                    {"name": "composeMsg", "type": "bytes"},
                    {"name": "oftCmd", "type": "bytes"}
                ],
                "name": "_sendParam",
                "type": "tuple"
            },
            {
                "components": [
                    {"name": "nativeFee", "type": "uint256"},
                    {"name": "lzTokenFee", "type": "uint256"}
                ],
                "name": "_fee",
                "type": "tuple"
            },
            {"name": "_refundAddress", "type": "address"}
        ],
        "name": "send",
        "outputs": [
            {
                "components": [
                    {"name": "guid", "type": "bytes32"},
                    {"name": "nonce", "type": "uint64"},
                    {"name": "fee", "type": "uint256"}
                ],
                "name": "msgReceipt",
                "type": "tuple"
            },
            {
                "components": [
                    {"name": "amountSentLD", "type": "uint256"},
                    {"name": "amountReceivedLD", "type": "uint256"}
                ],
                "name": "oftReceipt",
                "type": "tuple"
            }
        ],
        "stateMutability": "payable",
        "type": "function"
    },
    {
        "inputs": [
            {
                "components": [
                    {"name": "dstEid", "type": "uint32"},
                    {"name": "to", "type": "bytes32"},
                    {"name": "amountLD", "type": "uint256"},
                    {"name": "minAmountLD", "type": "uint256"},
                    {"name": "extraOptions", "type": "bytes"},
                    {"name": "composeMsg", "type": "bytes"},
                    {"name": "oftCmd", "type": "bytes"}
                ],
                "name": "_sendParam",
                "type": "tuple"
            },
            {"name": "_payInLzToken", "type": "bool"}
        ],
        "name": "quoteSend",
        "outputs": [
            {
                "components": [
                    {"name": "nativeFee", "type": "uint256"},
                    {"name": "lzTokenFee", "type": "uint256"}
                ],
                "name": "msgFee",
                "type": "tuple"
            }
        ],
        "stateMutability": "view",
        "type": "function"
    }
]"#;

/// Type of cross-chain reward
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RewardType {
    ComputeTask,
    VPoSProof,
    StorageService,
    NetworkValidation,
    CrossChainExecution,
}

/// Bridge transaction status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BridgeStatus {
    Pending,
    Submitted,
    Confirmed,
    Executing,
    Completed,
    Failed,
    Cancelled,
}

/// Bridge operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeResult {
    /// Operation success
    pub success: bool,

    /// Source transaction hash
    pub source_tx_hash: String,

    /// Destination transaction hash (if completed)
    pub destination_tx_hash: Option<String>,

    /// LayerZero message GUID
    pub lz_guid: Option<String>,

    /// Gas fees paid
    pub gas_fees: BridgeGasFees,

    /// Error message (if failed)
    pub error_message: Option<String>,

    /// Execution time
    pub execution_time_ms: u64,
}

/// Gas fees breakdown for bridge operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeGasFees {
    /// LayerZero messaging fee
    pub lz_fee: u128,

    /// Source chain gas fee
    pub source_gas_fee: u128,

    /// Destination chain gas fee (estimated)
    pub destination_gas_fee: u128,

    /// Bridge service fee
    pub bridge_service_fee: u128,

    /// Total fees
    pub total_fees: u128,
}

/// Bridge transaction for tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransaction {
    /// Transaction ID
    pub tx_id: String,

    /// Bridge operation type
    pub operation_type: BridgeOperationType,

    /// Source chain
    pub source_chain: SupportedChain,

    /// Destination chain
    pub destination_chain: SupportedChain,

    /// Transaction details
    pub details: BridgeTransactionDetails,

    /// Current status
    pub status: BridgeStatus,

    /// Gas fees
    pub fees: BridgeGasFees,

    /// Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Type of bridge operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BridgeOperationType {
    TokenTransfer,
    TaskExecution,
    RewardDistribution,
    StatusUpdate,
}

/// Bridge transaction details (union type)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BridgeTransactionDetails {
    TokenTransfer(CrossChainTokenTransfer),
    TaskExecution(CrossChainTaskExecution),
    RewardDistribution(CrossChainReward),
}

/// LayerZero bridge events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayerZeroBridgeEvent {
    TokenBridgeInitiated {
        transfer_id: String,
        source_chain: SupportedChain,
        destination_chain: SupportedChain,
        amount: u128,
        recipient: String,
    },
    TokenBridgeCompleted {
        transfer_id: String,
        destination_tx_hash: String,
        amount_received: u128,
    },
    OFTSent {
        transfer_id: String,
        oft_contract: String,
        amount: u128,
        dst_eid: u32,
        tx_hash: String,
    },
    OFTReceived {
        transfer_id: String,
        amount_received: u128,
        recipient: String,
        tx_hash: String,
    },
    CrossChainTaskSubmitted {
        execution_id: String,
        task_id: String,
        source_chain: SupportedChain,
        execution_chain: SupportedChain,
    },
    CrossChainTaskCompleted {
        execution_id: String,
        task_id: String,
        result_hash: String,
    },
    RewardDistributed {
        reward_id: String,
        provider_did: String,
        amount: u128,
        destination_chain: SupportedChain,
    },
    BridgeError {
        transaction_id: String,
        error_message: String,
        failed_at: DateTime<Utc>,
    },
}

/// LayerZero Bridge Manager for cross-chain operations
pub struct LayerZeroBridgeManager {
    /// Bridge configuration
    config: LayerZeroBridgeConfig,

    /// Active bridge transactions
    active_transactions: Arc<RwLock<HashMap<String, BridgeTransaction>>>,

    /// Cross-chain token transfers
    token_transfers: Arc<RwLock<HashMap<String, CrossChainTokenTransfer>>>,

    /// Cross-chain task executions
    task_executions: Arc<RwLock<HashMap<String, CrossChainTaskExecution>>>,

    /// Cross-chain rewards
    rewards: Arc<RwLock<HashMap<String, CrossChainReward>>>,

    /// Event history
    event_history: Arc<RwLock<Vec<LayerZeroBridgeEvent>>>,

    /// Alloy providers cache
    providers: Arc<RwLock<HashMap<SupportedChain, Arc<dyn AlloyProvider + Send + Sync>>>>,

    /// Signer for transactions
    signer: Option<PrivateKeySigner>,
}

impl LayerZeroBridgeManager {
    /// Create new LayerZero bridge manager
    pub fn new(config: LayerZeroBridgeConfig) -> Self {
        // Parse signer if private key is provided
        let signer = config
            .signer_private_key
            .as_ref()
            .and_then(|key| key.parse::<PrivateKeySigner>().ok());

        Self {
            config,
            active_transactions: Arc::new(RwLock::new(HashMap::new())),
            token_transfers: Arc::new(RwLock::new(HashMap::new())),
            task_executions: Arc::new(RwLock::new(HashMap::new())),
            rewards: Arc::new(RwLock::new(HashMap::new())),
            event_history: Arc::new(RwLock::new(Vec::new())),
            providers: Arc::new(RwLock::new(HashMap::new())),
            signer,
        }
    }

    /// Deterministic bridge outcome for unit tests (no RPC, no signer, no on-chain calldata).
    fn synthetic_bridge_result(&self, label: &str, amount: u128) -> BridgeResult {
        let ts = Utc::now().timestamp_millis() as u64;
        let lz_fee = 40_000_000_000_000_000u128;
        let source_gas_fee = 15_000_000_000_000_000u128;
        let destination_gas_fee = 10_000_000_000_000_000u128;
        let bridge_service_fee =
            ((amount as f64) * self.config.bridge_fees.base_fee_percentage).max(1.0) as u128;
        let total_fees = lz_fee + source_gas_fee + destination_gas_fee + bridge_service_fee;
        BridgeResult {
            success: true,
            source_tx_hash: format!("0xmock_{label}_{:016x}", ts),
            destination_tx_hash: Some(format!("0xmock_dst_{label}_{:016x}", ts)),
            lz_guid: Some(format!("0x{:064x}", ts)),
            gas_fees: BridgeGasFees {
                lz_fee,
                source_gas_fee,
                destination_gas_fee,
                bridge_service_fee,
                total_fees,
            },
            error_message: None,
            execution_time_ms: 1,
        }
    }

    /// Initialize bridge connections
    pub async fn initialize(&self) -> Result<()> {
        if !self.config.enabled {
            info!("LayerZero bridge is disabled");
            return Ok(());
        }

        info!("🌉 Initializing LayerZero bridge manager...");

        // Validate configuration
        self.validate_config().await?;

        // Initialize chain connections
        self.initialize_chain_connections().await?;

        // Set up event listeners
        self.setup_event_listeners().await?;

        info!("✅ LayerZero bridge manager initialized successfully");
        Ok(())
    }

    /// Bridge SWTCH tokens to another chain
    pub async fn bridge_swtch_tokens(
        &self,
        source_chain: SupportedChain,
        destination_chain: SupportedChain,
        amount: u128,
        recipient: &str,
        sender_did: &str,
    ) -> Result<BridgeResult> {
        let transfer_id = Uuid::new_v4().to_string();

        info!(
            "🌉 Bridging {} SWTCH tokens from {} to {}",
            amount as f64 / 1e18,
            source_chain.name(),
            destination_chain.name()
        );

        // Create transfer record
        let transfer = CrossChainTokenTransfer {
            transfer_id: transfer_id.clone(),
            source_chain,
            destination_chain,
            source_token: self.get_swtch_token_address(source_chain)?,
            destination_token: self.get_wrapped_swtch_address(destination_chain)?,
            amount,
            recipient: recipient.to_string(),
            sender_did: sender_did.to_string(),
            status: BridgeStatus::Pending,
            source_tx_hash: None,
            destination_tx_hash: None,
            initiated_at: Utc::now(),
            completed_at: None,
        };

        // Store transfer
        self.token_transfers
            .write()
            .await
            .insert(transfer_id.clone(), transfer.clone());

        // Execute bridge transaction
        let result = self.execute_token_bridge(transfer).await?;

        // Emit event
        self.emit_event(LayerZeroBridgeEvent::TokenBridgeInitiated {
            transfer_id,
            source_chain,
            destination_chain,
            amount,
            recipient: recipient.to_string(),
        })
        .await;

        Ok(result)
    }

    /// Execute cross-chain compute task
    pub async fn execute_cross_chain_task(
        &self,
        task: ComputeTask,
        source_chain: SupportedChain,
        execution_chain: SupportedChain,
        reward_chain: SupportedChain,
    ) -> Result<BridgeResult> {
        if !self.config.cross_chain_execution.enabled {
            return Err(anyhow::anyhow!("Cross-chain execution is disabled"));
        }

        let execution_id = Uuid::new_v4().to_string();

        info!(
            "🔄 Executing cross-chain task {} from {} on {}",
            task.id,
            source_chain.name(),
            execution_chain.name()
        );

        // Create execution record
        let execution = CrossChainTaskExecution {
            execution_id: execution_id.clone(),
            task: task.clone(),
            source_chain,
            execution_chain,
            reward_chain,
            status: BridgeStatus::Pending,
            result: None,
            task_submission_tx: None,
            execution_confirmation_tx: None,
            reward_distribution_tx: None,
            submitted_at: Utc::now(),
            executed_at: None,
            completed_at: None,
        };

        // Store execution
        self.task_executions
            .write()
            .await
            .insert(execution_id.clone(), execution.clone());

        // Submit task to execution chain
        let result = self.submit_cross_chain_task(execution).await?;

        // Emit event
        self.emit_event(LayerZeroBridgeEvent::CrossChainTaskSubmitted {
            execution_id,
            task_id: task.id,
            source_chain,
            execution_chain,
        })
        .await;

        Ok(result)
    }

    /// Distribute cross-chain rewards
    pub async fn distribute_cross_chain_reward(
        &self,
        task_id: &str,
        provider_did: &str,
        amount: u128,
        source_chain: SupportedChain,
        destination_chain: SupportedChain,
        reward_type: RewardType,
    ) -> Result<BridgeResult> {
        let reward_id = Uuid::new_v4().to_string();

        info!(
            "💰 Distributing {} SWTCH reward from {} to {}",
            amount as f64 / 1e18,
            source_chain.name(),
            destination_chain.name()
        );

        // Create reward record
        let reward = CrossChainReward {
            reward_id: reward_id.clone(),
            task_id: task_id.to_string(),
            provider_did: provider_did.to_string(),
            amount,
            source_chain,
            destination_chain,
            reward_type,
            status: BridgeStatus::Pending,
            source_tx_hash: None,
            destination_tx_hash: None,
            created_at: Utc::now(),
            distributed_at: None,
        };

        // Store reward
        self.rewards
            .write()
            .await
            .insert(reward_id.clone(), reward.clone());

        // Execute reward distribution
        let result = self.execute_reward_distribution(reward).await?;

        // Emit event
        self.emit_event(LayerZeroBridgeEvent::RewardDistributed {
            reward_id,
            provider_did: provider_did.to_string(),
            amount,
            destination_chain,
        })
        .await;

        Ok(result)
    }

    /// Get bridge transaction status
    pub async fn get_transaction_status(&self, tx_id: &str) -> Option<BridgeStatus> {
        self.active_transactions
            .read()
            .await
            .get(tx_id)
            .map(|tx| tx.status.clone())
    }

    /// Get token transfer status
    pub async fn get_transfer_status(&self, transfer_id: &str) -> Option<CrossChainTokenTransfer> {
        self.token_transfers.read().await.get(transfer_id).cloned()
    }

    /// Get task execution status
    pub async fn get_execution_status(
        &self,
        execution_id: &str,
    ) -> Option<CrossChainTaskExecution> {
        self.task_executions.read().await.get(execution_id).cloned()
    }

    /// Get supported chains
    pub fn get_supported_chains(&self) -> Vec<SupportedChain> {
        self.config.bridge_contracts.keys().cloned().collect()
    }

    /// Get bridge statistics
    pub async fn get_bridge_statistics(&self) -> BridgeStatistics {
        let token_transfers = self.token_transfers.read().await;
        let task_executions = self.task_executions.read().await;
        let rewards = self.rewards.read().await;

        BridgeStatistics {
            total_token_transfers: token_transfers.len() as u64,
            completed_token_transfers: token_transfers
                .values()
                .filter(|t| t.status == BridgeStatus::Completed)
                .count() as u64,
            total_cross_chain_tasks: task_executions.len() as u64,
            completed_cross_chain_tasks: task_executions
                .values()
                .filter(|e| e.status == BridgeStatus::Completed)
                .count() as u64,
            total_rewards_distributed: rewards.len() as u64,
            total_volume_bridged: token_transfers
                .values()
                .filter(|t| t.status == BridgeStatus::Completed)
                .map(|t| t.amount)
                .sum(),
        }
    }

    // Private helper methods

    async fn validate_config(&self) -> Result<()> {
        // Validate bridge contracts are configured
        if self.config.bridge_contracts.is_empty() {
            return Err(anyhow::anyhow!("No bridge contracts configured"));
        }

        // Validate token mappings
        for (chain, mapping) in &self.config.token_mappings {
            if mapping.astra_token.is_empty() {
                return Err(anyhow::anyhow!(
                    "Invalid token mapping for chain (empty astra_token): {:?}",
                    chain
                ));
            }
            if !self.config.use_oft && mapping.wrapped_astra.is_none() {
                return Err(anyhow::anyhow!(
                    "Invalid token mapping for chain (wrapped_astra required in non-OFT mode): {:?}",
                    chain
                ));
            }
        }

        Ok(())
    }

    async fn initialize_chain_connections(&self) -> Result<()> {
        for chain in self.config.bridge_contracts.keys() {
            debug!("Initializing connection to {}", chain.name());
            // In a real implementation, this would establish Web3 connections
            // to each chain's LayerZero endpoint
        }
        Ok(())
    }

    async fn setup_event_listeners(&self) -> Result<()> {
        // In a real implementation, this would set up event listeners
        // for LayerZero message delivery confirmations
        Ok(())
    }

    /// Get or create provider for a chain
    async fn get_provider(
        &self,
        chain: SupportedChain,
    ) -> Result<Arc<dyn AlloyProvider + Send + Sync>> {
        // Check cache first
        {
            let providers = self.providers.read().await;
            if let Some(provider) = providers.get(&chain) {
                return Ok(Arc::clone(provider));
            }
        }

        // Create new provider
        let rpc_url =
            self.config.rpc_endpoints.get(&chain).ok_or_else(|| {
                anyhow::anyhow!("RPC endpoint not configured for chain: {:?}", chain)
            })?;

        let provider =
            ProviderBuilder::new().connect_http(rpc_url.parse().context("Invalid RPC URL")?);

        let provider: Arc<dyn AlloyProvider + Send + Sync> = Arc::new(provider);

        // Cache it
        self.providers
            .write()
            .await
            .insert(chain, Arc::clone(&provider));

        Ok(provider)
    }

    /// Helper to get OFT contract address
    fn get_oft_contract(&self, chain: SupportedChain) -> Result<Address> {
        let addr_str =
            self.config.oft_contracts.get(&chain).ok_or_else(|| {
                anyhow::anyhow!("OFT contract not configured for chain: {:?}", chain)
            })?;
        addr_str.parse().context("Invalid OFT contract address")
    }

    /// Convert recipient string to bytes32 (LayerZero address format)
    fn address_to_bytes32(&self, addr: &str) -> Result<FixedBytes<32>> {
        let addr: Address = addr.parse().context("Invalid recipient address")?;
        let mut bytes = [0u8; 32];
        bytes[12..].copy_from_slice(&addr[..]);
        Ok(FixedBytes(bytes))
    }

    /// Execute OFT send using Alloy
    async fn execute_oft_send(
        &self,
        chain: SupportedChain,
        params: OftSendParams,
    ) -> Result<BridgeResult> {
        let start_time = std::time::Instant::now();

        if self.config.mock_chain_transactions {
            return Ok(self.synthetic_bridge_result("oft", params.amount));
        }

        let oft_contract = self.get_oft_contract(chain)?;
        info!(
            "🚀 Executing OFT send to contract: {:?} on {:?}",
            oft_contract, chain
        );
        debug!(
            "OFT params - amount: {}, dst_eid: {}, recipient: {}",
            params.amount, params.dst_eid, params.recipient
        );

        // Get provider
        let provider = self.get_provider(chain).await?;

        // Get signer
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Signer not configured"))?;

        // Convert recipient to bytes32
        let recipient_bytes = self.address_to_bytes32(&params.recipient)?;

        // Construct SendParam tuple (dstEid, to, amountLD, minAmountLD, extraOptions, composeMsg, oftCmd)
        let send_param = (
            params.dst_eid,
            recipient_bytes,
            U256::from(params.amount),
            U256::from(params.amount), // minAmountLD = amountLD for simplicity
            Bytes::new(),              // extraOptions - empty for default
            Bytes::new(),              // composeMsg - empty for simple transfer
            Bytes::new(),              // oftCmd - empty for simple transfer
        );

        // First, quote the messaging fee
        sol! {
            function quoteSend(
                (uint32,bytes32,uint256,uint256,bytes,bytes,bytes),
                bool
            ) external view returns ((uint256,uint256));

            function send(
                (uint32,bytes32,uint256,uint256,bytes,bytes,bytes),
                (uint256,uint256),
                address
            ) external payable returns ((bytes32,uint64,uint256),(uint256,uint256));
        }

        // Encode quoteSend call
        let quote_input = Bytes::from(
            [
                &quoteSendCall::SELECTOR[..],
                &alloy_sol_types::SolValue::abi_encode(&send_param),
                &alloy_sol_types::SolValue::abi_encode(&false),
            ]
            .concat(),
        );

        let quote_result = provider
            .call(
                TransactionRequest::default()
                    .to(oft_contract)
                    .input(TransactionInput::from(quote_input)),
            )
            .await
            .context("Failed to quote OFT send fee")?;

        // Decode quote result (native_fee, lz_token_fee)
        let native_fee = U256::from_be_slice(&quote_result[..32]);
        let lz_token_fee = U256::from_be_slice(&quote_result[32..64]);

        info!(
            "Quoted fees - native: {}, lz_token: {}",
            native_fee, lz_token_fee
        );

        // Encode send call
        let send_input = Bytes::from(
            [
                &sendCall::SELECTOR[..],
                &alloy_sol_types::SolValue::abi_encode(&send_param),
                &alloy_sol_types::SolValue::abi_encode(&(native_fee, lz_token_fee)),
                &alloy_sol_types::SolValue::abi_encode(&signer.address()),
            ]
            .concat(),
        );

        let tx = TransactionRequest::default()
            .to(oft_contract)
            .value(native_fee)
            .input(TransactionInput::from(send_input))
            .with_from(signer.address());

        // Send transaction (provider handles filling and signing)
        let pending_tx = provider
            .send_transaction(tx)
            .await
            .context("Failed to send transaction")?;

        info!("Transaction sent: {:?}", pending_tx.tx_hash());

        // Wait for confirmation
        let receipt = pending_tx
            .get_receipt()
            .await
            .context("Failed to get transaction receipt")?;

        let source_tx_hash = format!("0x{:x}", receipt.transaction_hash);

        // Extract LayerZero GUID from logs
        let lz_guid = self.extract_lz_guid(&receipt)?;

        // Calculate actual gas fees
        let native_fee_u128 = native_fee.to::<u128>();

        let gas_fees = BridgeGasFees {
            lz_fee: native_fee_u128,
            source_gas_fee: (receipt.gas_used as u128) * (receipt.effective_gas_price as u128),
            destination_gas_fee: native_fee_u128 / 2, // Estimated
            bridge_service_fee: (params.amount as f64 * self.config.bridge_fees.base_fee_percentage)
                as u128,
            total_fees: native_fee_u128
                + ((receipt.gas_used as u128) * (receipt.effective_gas_price as u128)),
        };

        Ok(BridgeResult {
            success: receipt.status(),
            source_tx_hash,
            destination_tx_hash: None, // Will be set when LayerZero delivers message
            lz_guid: Some(lz_guid),
            gas_fees,
            error_message: if !receipt.status() {
                Some("Transaction reverted".to_string())
            } else {
                None
            },
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    /// Extract LayerZero GUID from transaction receipt
    fn extract_lz_guid(&self, receipt: &TransactionReceipt) -> Result<String> {
        // LayerZero emits PacketSent event with GUID as first topic
        // Event signature: PacketSent(bytes encodedPayload, bytes options, address sendLibrary)
        // The GUID is typically in the encoded payload

        for log in receipt.inner.logs().iter() {
            if log.topics().len() >= 2 {
                // The GUID is usually the second topic
                let guid = format!("0x{:x}", log.topics()[1]);
                return Ok(guid);
            }
        }

        // Fallback: generate from tx hash
        Ok(format!("0x{:x}", receipt.transaction_hash))
    }

    /// Execute token bridge using OFT or legacy lock-and-mint
    async fn execute_token_bridge(
        &self,
        transfer: CrossChainTokenTransfer,
    ) -> Result<BridgeResult> {
        let result = if self.config.use_oft {
            // OFT path - use real Web3 calls
            let params = OftSendParams {
                amount: transfer.amount,
                dst_eid: transfer.destination_chain.endpoint_id(),
                recipient: transfer.recipient.clone(),
            };
            self.execute_oft_send(transfer.source_chain, params).await?
        } else {
            // Legacy lock-and-mint path - use real Web3 calls
            info!("Using legacy lock-and-mint bridge");
            self.execute_legacy_bridge(transfer.clone()).await?
        };

        // Update transfer status
        let mut transfers = self.token_transfers.write().await;
        if let Some(transfer_record) = transfers.get_mut(&transfer.transfer_id) {
            transfer_record.status = if result.success {
                BridgeStatus::Completed
            } else {
                BridgeStatus::Failed
            };
            transfer_record.source_tx_hash = Some(result.source_tx_hash.clone());
            transfer_record.destination_tx_hash = result.destination_tx_hash.clone();
            transfer_record.completed_at = Some(Utc::now());
        }

        Ok(result)
    }

    /// Execute legacy lock-and-mint bridge (for backwards compatibility)
    async fn execute_legacy_bridge(
        &self,
        transfer: CrossChainTokenTransfer,
    ) -> Result<BridgeResult> {
        let start_time = std::time::Instant::now();

        if self.config.mock_chain_transactions {
            return Ok(self.synthetic_bridge_result("legacy", transfer.amount));
        }

        let provider = self.get_provider(transfer.source_chain).await?;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Signer not configured"))?;

        // Get bridge contract address
        let bridge_contract: Address = self
            .config
            .bridge_contracts
            .get(&transfer.source_chain)
            .ok_or_else(|| anyhow::anyhow!("Bridge contract not configured"))?
            .parse()?;

        // Legacy bridge typically has: function bridge(address token, uint256 amount, uint32 dstChain, address recipient)
        sol! {
            function bridge(address token, uint256 amount, uint32 dstChain, address recipient) external payable;
        }

        let token_addr: Address = transfer.source_token.parse()?;
        let recipient_addr: Address = transfer.recipient.parse()?;

        let bridge_input = Bytes::from(
            [
                &bridgeCall::SELECTOR[..],
                &alloy_sol_types::SolValue::abi_encode(&token_addr),
                &alloy_sol_types::SolValue::abi_encode(&U256::from(transfer.amount)),
                &alloy_sol_types::SolValue::abi_encode(&transfer.destination_chain.endpoint_id()),
                &alloy_sol_types::SolValue::abi_encode(&recipient_addr),
            ]
            .concat(),
        );

        let tx = TransactionRequest::default()
            .to(bridge_contract)
            .input(TransactionInput::from(bridge_input))
            .value(U256::from(100000000000000000u128))
            .with_from(signer.address());

        let pending_tx = provider.send_transaction(tx).await?;
        let receipt = pending_tx.get_receipt().await?;

        let gas_fees = BridgeGasFees {
            lz_fee: 50000000000000000,
            source_gas_fee: (receipt.gas_used as u128) * (receipt.effective_gas_price as u128),
            destination_gas_fee: 15000000000000000,
            bridge_service_fee: (transfer.amount as f64
                * self.config.bridge_fees.base_fee_percentage)
                as u128,
            total_fees: 65000000000000000
                + ((receipt.gas_used as u128) * (receipt.effective_gas_price as u128)),
        };

        Ok(BridgeResult {
            success: receipt.status(),
            source_tx_hash: format!("0x{:x}", receipt.transaction_hash),
            destination_tx_hash: None,
            lz_guid: self.extract_lz_guid(&receipt).ok(),
            gas_fees,
            error_message: if !receipt.status() {
                Some("Transaction reverted".to_string())
            } else {
                None
            },
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    async fn submit_cross_chain_task(
        &self,
        execution: CrossChainTaskExecution,
    ) -> Result<BridgeResult> {
        info!("🔄 Submitting cross-chain task: {}", execution.task.id);
        let start_time = std::time::Instant::now();

        if self.config.mock_chain_transactions {
            let result = self.synthetic_bridge_result("xchain_task", 0);
            let mut executions = self.task_executions.write().await;
            if let Some(execution_record) = executions.get_mut(&execution.execution_id) {
                execution_record.status = BridgeStatus::Submitted;
                execution_record.task_submission_tx = Some(result.source_tx_hash.clone());
            }
            return Ok(result);
        }

        let provider = self.get_provider(execution.source_chain).await?;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Signer not configured"))?;

        // Get LayerZero endpoint address for source chain
        // In production, this would be configured per chain
        let endpoint_addr: Address = "0x1a44076050125825900e736c501f859c50fE728c".parse()?; // Example endpoint

        // Encode task data
        sol! {
            function send(
                uint32 _dstEid,
                bytes32 _receiver,
                bytes calldata _message,
                address _refundAddress
            ) external payable returns (bytes32 guid);
        }

        let task_data = execution.task.code.clone();
        let receiver = self.address_to_bytes32(&execution.task.owner_did)?;
        let message = Bytes::from(task_data);

        let lz_input = Bytes::from(
            [
                &sendCall::SELECTOR[..],
                &alloy_sol_types::SolValue::abi_encode(&execution.execution_chain.endpoint_id()),
                &alloy_sol_types::SolValue::abi_encode(&receiver),
                &alloy_sol_types::SolValue::abi_encode(&message),
                &alloy_sol_types::SolValue::abi_encode(&signer.address()),
            ]
            .concat(),
        );

        let tx = TransactionRequest::default()
            .to(endpoint_addr)
            .input(TransactionInput::from(lz_input))
            .value(U256::from(150000000000000000u128))
            .with_from(signer.address());

        let pending_tx = provider.send_transaction(tx).await?;
        let receipt = pending_tx.get_receipt().await?;

        let source_tx_hash = format!("0x{:x}", receipt.transaction_hash);
        let lz_guid = self.extract_lz_guid(&receipt)?;

        let gas_fees = BridgeGasFees {
            lz_fee: 75000000000000000,
            source_gas_fee: (receipt.gas_used as u128) * (receipt.effective_gas_price as u128),
            destination_gas_fee: 50000000000000000,
            bridge_service_fee: 10000000000000000,
            total_fees: 135000000000000000
                + ((receipt.gas_used as u128) * (receipt.effective_gas_price as u128)),
        };

        // Update execution status
        let mut executions = self.task_executions.write().await;
        if let Some(execution_record) = executions.get_mut(&execution.execution_id) {
            execution_record.status = BridgeStatus::Submitted;
            execution_record.task_submission_tx = Some(source_tx_hash.clone());
        }

        Ok(BridgeResult {
            success: receipt.status(),
            source_tx_hash,
            destination_tx_hash: None,
            lz_guid: Some(lz_guid),
            gas_fees,
            error_message: if !receipt.status() {
                Some("Transaction reverted".to_string())
            } else {
                None
            },
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    async fn execute_reward_distribution(&self, reward: CrossChainReward) -> Result<BridgeResult> {
        info!(
            "💰 Executing reward distribution: {} SWTCH",
            reward.amount as f64 / 1e18
        );

        let result = if self.config.use_oft {
            // OFT path for rewards
            let oft_contract = self.get_oft_contract(reward.source_chain)?;
            let params = OftSendParams {
                amount: reward.amount,
                dst_eid: reward.destination_chain.endpoint_id(),
                recipient: reward.provider_did.clone(), // In production, convert DID to address
            };

            let oft_result = self.execute_oft_send(reward.source_chain, params).await?;

            // Emit OFT-specific event
            self.emit_event(LayerZeroBridgeEvent::OFTSent {
                transfer_id: reward.reward_id.clone(),
                oft_contract: format!("{:?}", oft_contract),
                amount: reward.amount,
                dst_eid: reward.destination_chain.endpoint_id(),
                tx_hash: oft_result.source_tx_hash.clone(),
            })
            .await;

            oft_result
        } else {
            // Legacy lock-and-mint path - use real Web3
            info!("Using legacy lock-and-mint for rewards");
            self.execute_legacy_reward(reward.clone()).await?
        };

        // Update reward status
        let mut rewards = self.rewards.write().await;
        if let Some(reward_record) = rewards.get_mut(&reward.reward_id) {
            reward_record.status = if result.success {
                BridgeStatus::Completed
            } else {
                BridgeStatus::Failed
            };
            reward_record.source_tx_hash = Some(result.source_tx_hash.clone());
            reward_record.destination_tx_hash = result.destination_tx_hash.clone();
            reward_record.distributed_at = Some(Utc::now());
        }

        Ok(result)
    }

    /// Execute legacy reward distribution
    async fn execute_legacy_reward(&self, reward: CrossChainReward) -> Result<BridgeResult> {
        let start_time = std::time::Instant::now();

        if self.config.mock_chain_transactions {
            return Ok(self.synthetic_bridge_result("reward", reward.amount));
        }

        let provider = self.get_provider(reward.source_chain).await?;
        let signer = self
            .signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Signer not configured"))?;

        // Get reward distributor contract
        let distributor_addr: Address = self
            .config
            .bridge_contracts
            .get(&reward.source_chain)
            .ok_or_else(|| anyhow::anyhow!("Distributor contract not configured"))?
            .parse()?;

        // Reward distributor: function distribute(address recipient, uint256 amount, uint32 dstChain)
        sol! {
            function distribute(address recipient, uint256 amount, uint32 dstChain) external payable;
        }

        let recipient_addr: Address = reward.provider_did.parse()?;

        let distribute_input = Bytes::from(
            [
                &distributeCall::SELECTOR[..],
                &alloy_sol_types::SolValue::abi_encode(&recipient_addr),
                &alloy_sol_types::SolValue::abi_encode(&U256::from(reward.amount)),
                &alloy_sol_types::SolValue::abi_encode(&reward.destination_chain.endpoint_id()),
            ]
            .concat(),
        );

        let tx = TransactionRequest::default()
            .to(distributor_addr)
            .input(TransactionInput::from(distribute_input))
            .value(U256::from(80000000000000000u128))
            .with_from(signer.address());

        let pending_tx = provider.send_transaction(tx).await?;
        let receipt = pending_tx.get_receipt().await?;

        let gas_fees = BridgeGasFees {
            lz_fee: 40000000000000000,
            source_gas_fee: (receipt.gas_used as u128) * (receipt.effective_gas_price as u128),
            destination_gas_fee: 10000000000000000,
            bridge_service_fee: (reward.amount as f64 * 0.001) as u128,
            total_fees: 50000000000000000
                + ((receipt.gas_used as u128) * (receipt.effective_gas_price as u128)),
        };

        Ok(BridgeResult {
            success: receipt.status(),
            source_tx_hash: format!("0x{:x}", receipt.transaction_hash),
            destination_tx_hash: None,
            lz_guid: self.extract_lz_guid(&receipt).ok(),
            gas_fees,
            error_message: if !receipt.status() {
                Some("Transaction reverted".to_string())
            } else {
                None
            },
            execution_time_ms: start_time.elapsed().as_millis() as u64,
        })
    }

    fn get_swtch_token_address(&self, chain: SupportedChain) -> Result<String> {
        self.config
            .token_mappings
            .get(&chain)
            .map(|mapping| mapping.astra_token.clone())
            .ok_or_else(|| anyhow::anyhow!("ASTRA token not configured for chain: {:?}", chain))
    }

    fn get_wrapped_swtch_address(&self, chain: SupportedChain) -> Result<String> {
        self.config
            .token_mappings
            .get(&chain)
            .and_then(|mapping| mapping.wrapped_astra.clone())
            .ok_or_else(|| {
                anyhow::anyhow!("Wrapped ASTRA token not configured for chain: {:?}", chain)
            })
    }

    async fn emit_event(&self, event: LayerZeroBridgeEvent) {
        self.event_history.write().await.push(event);
    }
}

/// Bridge statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStatistics {
    pub total_token_transfers: u64,
    pub completed_token_transfers: u64,
    pub total_cross_chain_tasks: u64,
    pub completed_cross_chain_tasks: u64,
    pub total_rewards_distributed: u64,
    pub total_volume_bridged: u128,
}

impl Default for LayerZeroBridgeConfig {
    fn default() -> Self {
        let mut bridge_contracts = HashMap::new();
        bridge_contracts.insert(
            SupportedChain::Ethereum,
            "0x1234567890abcdef1234567890abcdef12345678".to_string(),
        );
        bridge_contracts.insert(
            SupportedChain::Arbitrum,
            "0xabcdef1234567890abcdef1234567890abcdef12".to_string(),
        );
        bridge_contracts.insert(
            SupportedChain::Polygon,
            "0x567890abcdef1234567890abcdef1234567890ab".to_string(),
        );

        let mut token_mappings = HashMap::new();
        let mut eth_tokens = HashMap::new();
        eth_tokens.insert(
            "USDT".to_string(),
            "0xdAC17F958D2ee523a2206206994597C13D831ec7".to_string(),
        );
        eth_tokens.insert(
            "DAI".to_string(),
            "0x6B175474E89094C44Da98b954EedeAC495271d0F".to_string(),
        );
        token_mappings.insert(
            SupportedChain::Ethereum,
            TokenBridgeMapping {
                astra_token: "0xASTRA_ETH_ADDRESS".to_string(),
                wrapped_astra: Some("0xWASTRA_ETH_ADDRESS".to_string()),
                usdc_token: "0xA0b86a33E6441B8C8A4C6a62Bd2f1b6B5C4D4E5F".to_string(),
                supported_tokens: eth_tokens,
            },
        );
        token_mappings.insert(
            SupportedChain::Arbitrum,
            TokenBridgeMapping {
                astra_token: "0xASTRA_ARB_ADDRESS".to_string(),
                wrapped_astra: Some("0xWASTRA_ARB_ADDRESS".to_string()),
                usdc_token: "0xA0b86991c6218b36c1d19d4a2e9eb0ce3606eb48".to_string(),
                supported_tokens: HashMap::new(),
            },
        );
        token_mappings.insert(
            SupportedChain::Avalanche,
            TokenBridgeMapping {
                astra_token: "0xASTRA_AVAX_ADDRESS".to_string(),
                wrapped_astra: Some("0xWASTRA_AVAX_ADDRESS".to_string()),
                usdc_token: "0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E".to_string(),
                supported_tokens: HashMap::new(),
            },
        );
        token_mappings.insert(
            SupportedChain::Polygon,
            TokenBridgeMapping {
                astra_token: "0xASTRA_POLY_ADDRESS".to_string(),
                wrapped_astra: Some("0xWASTRA_POLY_ADDRESS".to_string()),
                usdc_token: "0x3c499c542cEF5E3811e1192ce70d8cC03e5B963c".to_string(),
                supported_tokens: HashMap::new(),
            },
        );

        let mut oft_contracts = HashMap::new();
        oft_contracts.insert(SupportedChain::Ethereum, "0xOFT_ETH_ADDRESS".to_string());
        oft_contracts.insert(SupportedChain::Arbitrum, "0xOFT_ARB_ADDRESS".to_string());

        let mut rpc_endpoints = HashMap::new();
        rpc_endpoints.insert(
            SupportedChain::Ethereum,
            "https://eth.llamarpc.com".to_string(),
        );
        rpc_endpoints.insert(
            SupportedChain::Arbitrum,
            "https://arb1.arbitrum.io/rpc".to_string(),
        );
        rpc_endpoints.insert(
            SupportedChain::Polygon,
            "https://polygon-rpc.com".to_string(),
        );
        rpc_endpoints.insert(SupportedChain::Base, "https://mainnet.base.org".to_string());

        Self {
            enabled: true,
            spacekit_endpoint_id: 40000, // Custom SpaceKit network endpoint ID
            bridge_contracts,
            token_mappings,
            gas_limits: BridgeGasLimits {
                bridge_token: 200000,
                execute_task: 500000,
                distribute_reward: 150000,
                status_update: 100000,
            },
            bridge_fees: BridgeFeeConfig {
                base_fee_percentage: 0.001,                       // 0.1%
                message_fee_buffer: 0.1,                          // 10% buffer
                minimum_bridge_amount: 1000000000000000000,       // 1 ASTRA
                maximum_bridge_amount: 1000000000000000000000000, // 1M ASTRA
            },
            cross_chain_execution: CrossChainExecutionConfig {
                enabled: true,
                max_execution_time: 3600, // 1 hour
                supported_runtimes: vec![
                    "wasm".to_string(),
                    "gpu".to_string(),
                    "hybrid".to_string(),
                ],
                auto_retry: true,
                max_retries: 3,
            },
            use_oft: false, // Default to false for migration
            oft_contracts,
            rpc_endpoints,
            signer_private_key: None, // Must be provided in production config
            mock_chain_transactions: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge_config_for_unit_tests() -> LayerZeroBridgeConfig {
        let mut c = LayerZeroBridgeConfig::default();
        c.mock_chain_transactions = true;
        c
    }

    #[tokio::test]
    async fn test_bridge_manager_initialization() {
        let config = LayerZeroBridgeConfig::default();
        let bridge_manager = LayerZeroBridgeManager::new(config);

        let result = bridge_manager.initialize().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_swtch_token_bridging() {
        let config = bridge_config_for_unit_tests();
        let bridge_manager = LayerZeroBridgeManager::new(config);
        bridge_manager.initialize().await.unwrap();

        let result = bridge_manager
            .bridge_swtch_tokens(
                SupportedChain::Ethereum,
                SupportedChain::Arbitrum,
                1000000000000000000, // 1 SWTCH
                "0xRecipientAddress",
                "did:swtch:user:test",
            )
            .await;

        assert!(result.is_ok());
        let bridge_result = result.unwrap();
        assert!(bridge_result.success);
        assert!(!bridge_result.source_tx_hash.is_empty());
    }

    #[tokio::test]
    async fn test_cross_chain_task_execution() {
        let config = bridge_config_for_unit_tests();
        let bridge_manager = LayerZeroBridgeManager::new(config);
        bridge_manager.initialize().await.unwrap();

        let task = ComputeTask {
            id: "test_task_001".to_string(),
            name: "Cross-Chain Test Task".to_string(),
            runtime: "wasm".to_string(),
            code: vec![0x00, 0x61, 0x73, 0x6D],
            input_data: vec![0x01, 0x02, 0x03, 0x04],
            status: crate::TaskStatus::Queued,
            created_at: Utc::now(),
            owner_did: "did:swtch:user:test".to_string(),
            estimated_cost: Some(1.0),
            actual_cost: None,
            execution_path: Some("CPU".to_string()),
            result_hash: None,
        };

        let result = bridge_manager
            .execute_cross_chain_task(
                task,
                SupportedChain::Ethereum,
                SupportedChain::Arbitrum,
                SupportedChain::Polygon,
            )
            .await;

        assert!(result.is_ok());
        let bridge_result = result.unwrap();
        assert!(bridge_result.success);
        assert!(!bridge_result.source_tx_hash.is_empty());
    }

    #[tokio::test]
    async fn test_cross_chain_reward_distribution() {
        let config = bridge_config_for_unit_tests();
        let bridge_manager = LayerZeroBridgeManager::new(config);
        bridge_manager.initialize().await.unwrap();

        let result = bridge_manager
            .distribute_cross_chain_reward(
                "test_task_001",
                "did:swtch:provider:test",
                5000000000000000000, // 5 SWTCH
                SupportedChain::Arbitrum,
                SupportedChain::Ethereum,
                RewardType::ComputeTask,
            )
            .await;

        assert!(result.is_ok());
        let bridge_result = result.unwrap();
        assert!(bridge_result.success);
        assert!(bridge_result.destination_tx_hash.is_some());
    }

    #[test]
    fn test_supported_chain_conversion() {
        assert_eq!(SupportedChain::Ethereum.endpoint_id(), 30101);
        assert_eq!(
            SupportedChain::from_endpoint_id(30110),
            Some(SupportedChain::Arbitrum)
        );
        assert_eq!(SupportedChain::from_endpoint_id(99999), None);
    }

    #[tokio::test]
    async fn test_bridge_statistics() {
        let config = bridge_config_for_unit_tests();
        let bridge_manager = LayerZeroBridgeManager::new(config);
        bridge_manager.initialize().await.unwrap();

        // Execute some bridge operations
        bridge_manager
            .bridge_swtch_tokens(
                SupportedChain::Ethereum,
                SupportedChain::Arbitrum,
                1000000000000000000,
                "0xRecipient",
                "did:swtch:user:test",
            )
            .await
            .unwrap();

        let stats = bridge_manager.get_bridge_statistics().await;
        assert_eq!(stats.total_token_transfers, 1);
        assert_eq!(stats.completed_token_transfers, 1);
        assert_eq!(stats.total_volume_bridged, 1000000000000000000);
    }
}
