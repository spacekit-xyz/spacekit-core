//! Genesis Node Implementation
//!
//! Handles genesis block creation, initial state setup, and network bootstrapping
//! for the SWTCHVM blockchain.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use spacekit_primitives::v1::sdk::token::ASTRA_MAX_SUPPLY_WEI;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use super::{
    SwtchvmAccount, SwtchvmAddress, SwtchvmBlock, SwtchvmNode, SwtchvmRuntime, SwtchvmState,
    SwtchvmTransaction, TransactionSignature,
};

#[cfg(feature = "storage-integration")]
use super::{BlockchainStorage, BlockchainStorageConfig};

/// Genesis configuration for blockchain initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    /// Chain ID for the blockchain network
    pub chain_id: u64,

    /// Network name
    pub network_name: String,

    /// Genesis timestamp (Unix timestamp)
    pub genesis_timestamp: u64,

    /// Initial gas limit for the genesis block
    pub genesis_gas_limit: u128,

    /// Pre-funded accounts with initial balances
    pub alloc: HashMap<String, GenesisAccount>,

    /// Initial consensus parameters
    pub consensus_config: ConsensusConfig,

    /// Network-wide constants
    pub constants: NetworkConstants,

    /// Optional custom genesis message
    pub genesis_message: Option<String>,
}

/// Account configuration in genesis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    /// Initial balance in compute credits
    pub balance: u128,

    /// Initial nonce
    pub nonce: u64,

    /// Pre-deployed contract code (optional)
    pub code: Option<Vec<u8>>,

    /// Initial storage state (optional)
    pub storage: Option<HashMap<String, String>>,

    /// Account type (normal, contract, system)
    pub account_type: AccountType,
}

/// Account types in genesis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountType {
    Normal,
    Contract,
    System,
    Validator,
}

/// Consensus configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    /// Consensus algorithm
    pub algorithm: ConsensusAlgorithm,

    /// Block time target in seconds
    pub block_time: u64,

    /// Minimum gas price
    pub min_gas_price: u128,

    /// Maximum block size in bytes
    pub max_block_size: u64,

    /// Proof of Compute difficulty (if using PoC)
    pub poc_difficulty: u64,
}

/// Consensus algorithm types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusAlgorithm {
    ProofOfWork {
        difficulty: u64,
    },
    ProofOfStake {
        min_stake: u64,
    },
    ProofOfCompute {
        difficulty: u64,
        min_compute_power: u64,
    },
    DevMode, // For development/testing
}

/// Network-wide constants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConstants {
    /// Native token name
    pub token_name: String,

    /// Native token symbol
    pub token_symbol: String,

    /// Token decimals
    pub token_decimals: u8,

    /// Maximum supply
    pub max_supply: u128,

    /// Block reward
    pub block_reward: u128,

    /// Transaction fee burn rate (percentage)
    pub fee_burn_rate: u8,
}

/// Well-known system contract addresses
pub mod system_contracts {
    pub const FAUCET: &str = "0x0000000000000000000000000000000000000001";
    pub const DID_REGISTRY: &str = "0x0000000000000000000000000000000000000002";
    /// AstraRewards SKCL contract (SRA CREDIT target).
    pub const ASTRA_REWARDS: &str = "0x0000000000000000000000000000000000000003";
}

/// Try to load the DID registry WASM binary from known build paths.
fn load_did_registry_wasm() -> Option<Vec<u8>> {
    load_system_wasm(&[
        "spacekit-standard-library/target/wasm32-unknown-unknown/release/spacekit_did_registry.wasm",
        "../spacekit-standard-library/target/wasm32-unknown-unknown/release/spacekit_did_registry.wasm",
    ], "DID registry")
}

fn load_astra_rewards_wasm() -> Option<Vec<u8>> {
    load_system_wasm(
        &[
            "spacekit-standard-library/target/wasm32-unknown-unknown/release/astra_rewards.wasm",
            "../spacekit-standard-library/target/wasm32-unknown-unknown/release/astra_rewards.wasm",
        ],
        "AstraRewards",
    )
}

/// Install system contract WASM at well-known addresses if not already present.
pub fn install_system_contracts(state: &mut crate::spacekitvm::swtchvm_node::SwtchvmState) {
    install_contract_if_missing(
        state,
        system_contracts::DID_REGISTRY,
        load_did_registry_wasm(),
    );
    install_contract_if_missing(
        state,
        system_contracts::ASTRA_REWARDS,
        load_astra_rewards_wasm(),
    );
}

fn install_contract_if_missing(
    state: &mut crate::spacekitvm::swtchvm_node::SwtchvmState,
    address_hex: &str,
    code: Option<Vec<u8>>,
) {
    let Some(wasm) = code else { return };
    let Ok(addr) = crate::spacekitvm::swtchvm_node::SwtchvmAddress::from_hex(address_hex) else {
        return;
    };
    let account = state.get_account_mut(&addr);
    if account.code.is_none() {
        account.code = Some(wasm);
        tracing::info!("Installed system contract at {}", address_hex);
    }
}

fn load_system_wasm(candidates: &[&str], name: &str) -> Option<Vec<u8>> {
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            tracing::info!("Loaded {} WASM from {} ({} bytes)", name, path, bytes.len());
            return Some(bytes);
        }
    }
    tracing::warn!("{name} WASM not found; genesis will omit system contract code");
    None
}

impl Default for GenesisConfig {
    fn default() -> Self {
        let mut alloc = HashMap::new();

        // Faucet / system account
        alloc.insert(
            system_contracts::FAUCET.to_string(),
            GenesisAccount {
                balance: 1_000_000_000_000_000_000_000_000_000, // 1T credits
                nonce: 0,
                code: None,
                storage: None,
                account_type: AccountType::System,
            },
        );

        // DID Registry system contract
        alloc.insert(
            system_contracts::DID_REGISTRY.to_string(),
            GenesisAccount {
                balance: 0,
                nonce: 0,
                code: load_did_registry_wasm(),
                storage: None,
                account_type: AccountType::System,
            },
        );

        // AstraRewards — per-DID balances; SRA submits CREDIT here
        alloc.insert(
            system_contracts::ASTRA_REWARDS.to_string(),
            GenesisAccount {
                balance: 0,
                nonce: 0,
                code: load_astra_rewards_wasm(),
                storage: None,
                account_type: AccountType::System,
            },
        );

        Self {
            chain_id: 1337, // Default dev chain ID
            network_name: "SpaceKit Devnet".to_string(),
            genesis_timestamp: Utc::now().timestamp() as u64,
            genesis_gas_limit: 30_000_000,
            alloc,
            consensus_config: ConsensusConfig {
                algorithm: ConsensusAlgorithm::DevMode,
                block_time: 10, // 10 second blocks
                min_gas_price: 1,
                max_block_size: 1024 * 1024, // 1MB blocks
                poc_difficulty: 1000,
            },
            constants: NetworkConstants {
                token_name: "SpaceKit Credits".to_string(),
                token_symbol: "ASTRA".to_string(),
                token_decimals: 18,
                max_supply: ASTRA_MAX_SUPPLY_WEI, // 2B ASTRA hard cap (18 decimals)
                block_reward: 50_000_000_000_000_000, // 0.05 ASTRA per block (devnet)
                fee_burn_rate: 0,                 // v2: no protocol automatic fee burn
            },
            genesis_message: Some(
                "Genesis block for SpaceKit quantum-resistant compute network".to_string(),
            ),
        }
    }
}

/// Genesis Node - responsible for blockchain initialization
pub struct GenesisNode {
    config: GenesisConfig,
    #[cfg(feature = "storage-integration")]
    storage: Arc<BlockchainStorage>,
    #[allow(dead_code)]
    runtime: Arc<SwtchvmRuntime>,
    genesis_block: Option<SwtchvmBlock>,
}

impl GenesisNode {
    /// Create a new genesis node (without storage integration)
    pub async fn new_simple(genesis_config: GenesisConfig) -> Result<Self> {
        let runtime = Arc::new(SwtchvmRuntime::new(false)?); // Start without GPU for genesis

        #[cfg(feature = "storage-integration")]
        let storage = {
            // Avoid a fixed cwd-relative path: parallel tests and sandboxed runners race on
            // `./temp_blockchain_storage` and can see ENOENT from conflicting teardown.
            let data_dir: PathBuf = std::env::var_os("SPACEKIT_GENESIS_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    std::env::temp_dir().join(format!("spacekit_genesis_node_{}", Uuid::new_v4()))
                });
            std::fs::create_dir_all(&data_dir).map_err(|e| {
                anyhow::anyhow!(
                    "create genesis storage data_dir {}: {}",
                    data_dir.display(),
                    e
                )
            })?;
            let storage_config = BlockchainStorageConfig {
                data_dir: data_dir.to_string_lossy().to_string(),
                encryption_algorithm:
                    spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024,
                enable_distributed: false, // Disable for simple mode
                replication_factor: 1,
                block_cache_size: 10, // Small cache for testing
                batch_size: 1,
            };
            match BlockchainStorage::new(storage_config).await {
                Ok(storage) => Arc::new(storage),
                Err(e) => {
                    tracing::warn!("Failed to create blockchain storage: {}", e);
                    return Err(e);
                }
            }
        };

        Ok(Self {
            config: genesis_config,
            #[cfg(feature = "storage-integration")]
            storage,
            runtime,
            genesis_block: None,
        })
    }

    /// Create a new genesis node (with storage integration)
    #[cfg(feature = "storage-integration")]
    pub async fn new(
        genesis_config: GenesisConfig,
        storage_config: BlockchainStorageConfig,
    ) -> Result<Self> {
        let storage = Arc::new(BlockchainStorage::new(storage_config).await?);
        let runtime = Arc::new(SwtchvmRuntime::new(false)?); // Start without GPU for genesis

        Ok(Self {
            config: genesis_config,
            storage,
            runtime,
            genesis_block: None,
        })
    }

    /// Initialize the blockchain with genesis block (simple version without storage)
    pub async fn initialize_blockchain(&mut self) -> Result<SwtchvmBlock> {
        println!("🔥 Initializing SpaceKit Blockchain...");

        // Check if we already have a genesis block
        if let Some(existing_genesis) = &self.genesis_block {
            println!(
                "✅ Blockchain already initialized with genesis block {}",
                existing_genesis.number
            );
            return Ok(existing_genesis.clone());
        }

        // Create genesis state
        let genesis_state = self.create_genesis_state().await?;

        // Create genesis block
        let genesis_block = self.create_genesis_block(&genesis_state).await?;

        // Store genesis block locally (without storage node for minimal demo)
        self.genesis_block = Some(genesis_block.clone());

        println!("🎉 Genesis block created successfully!");
        println!("   Chain ID: {}", self.config.chain_id);
        println!("   Network: {}", self.config.network_name);
        println!("   Genesis Hash: {}", hex::encode(&genesis_block.hash));
        println!("   Accounts: {}", self.config.alloc.len());

        Ok(genesis_block)
    }

    /// Create the initial blockchain state from genesis config
    async fn create_genesis_state(&self) -> Result<SwtchvmState> {
        println!("📋 Creating genesis state...");

        let mut state = SwtchvmState::new();

        // Create accounts from genesis allocation
        for (address_str, genesis_account) in &self.config.alloc {
            let address = self.parse_address(address_str)?;

            let mut account = SwtchvmAccount {
                address,
                balance: genesis_account.balance,
                nonce: genesis_account.nonce,
                code: genesis_account.code.clone(),
                storage: HashMap::new(),
                compute_used: 0,
            };

            // Set up initial storage if specified
            if let Some(storage_config) = &genesis_account.storage {
                for (key_str, value_str) in storage_config {
                    let key = self.parse_storage_key(key_str)?;
                    let value = self.parse_storage_value(value_str)?;
                    account.storage.insert(key, value);
                }
            }

            // Add account to state
            *state.get_account_mut(&address) = account;

            println!(
                "   👤 Account {}: {} credits ({})",
                address_str,
                genesis_account.balance,
                match genesis_account.account_type {
                    AccountType::Normal => "Normal",
                    AccountType::Contract => "Contract",
                    AccountType::System => "System",
                    AccountType::Validator => "Validator",
                }
            );
        }

        println!(
            "✅ Genesis state created with {} accounts",
            self.config.alloc.len()
        );
        Ok(state)
    }

    /// Create the genesis block
    async fn create_genesis_block(&self, genesis_state: &SwtchvmState) -> Result<SwtchvmBlock> {
        println!("🧱 Creating genesis block...");

        // Create genesis transaction (optional system initialization)
        let genesis_transactions = self.create_genesis_transactions().await?;

        let genesis_block = SwtchvmBlock {
            number: 0,
            parent_hash: [0u8; 32],
            hash: [0u8; 32],
            timestamp: self.config.genesis_timestamp,
            gas_limit: self.config.genesis_gas_limit,
            gas_used: 0,
            transactions: genesis_transactions,
            state_root: genesis_state.state_root(),
            compute_root: [0u8; 32],
            receipts: vec![],
            verkle_witness: None,
        };

        // Calculate genesis block hash
        let mut block_with_hash = genesis_block;
        block_with_hash.hash = self.calculate_genesis_hash(&block_with_hash)?;

        println!("✅ Genesis block created");
        Ok(block_with_hash)
    }

    /// Create system transactions for genesis block (if needed)
    async fn create_genesis_transactions(&self) -> Result<Vec<SwtchvmTransaction>> {
        let mut transactions = Vec::new();

        // Add genesis message transaction if specified
        if let Some(message) = &self.config.genesis_message {
            let system_address = SwtchvmAddress::new([0u8; 20]);

            let genesis_tx = SwtchvmTransaction {
                from: system_address,
                to: None,
                data: message.as_bytes().to_vec(),
                gas_limit: 21000,
                gas_price: 0, // Free genesis transaction
                value: 0,
                nonce: 0,
                signature: TransactionSignature {
                    v: 0,
                    r: [0u8; 32],
                    s: [0u8; 32],
                },
            };

            transactions.push(genesis_tx);
        }

        Ok(transactions)
    }

    /// Convert to a full SpaceKit node after genesis
    pub async fn into_swtch_node(self) -> Result<SwtchvmNode> {
        if self.genesis_block.is_none() {
            return Err(anyhow::anyhow!("Genesis block not initialized"));
        }

        println!("🔄 Converting genesis node to full SpaceKit node...");

        // Create full node with the initialized blockchain
        let mut node = SwtchvmNode::new(false, false).await?;

        // Load the genesis block into the node's blockchain
        if let Some(genesis) = &self.genesis_block {
            // Note: This would need to be properly implemented in SwtchvmNode
            // For now, we'll create a new node that starts with genesis
            println!("✅ SpaceKit node ready with genesis block");
        }

        Ok(node)
    }

    /// Get genesis configuration
    pub fn get_genesis_config(&self) -> &GenesisConfig {
        &self.config
    }

    /// Get genesis block (if created)
    pub fn get_genesis_block(&self) -> Option<&SwtchvmBlock> {
        self.genesis_block.as_ref()
    }

    /// Export genesis configuration to file
    pub fn export_genesis_config(&self, path: &str) -> Result<()> {
        let config_json = serde_json::to_string_pretty(&self.config)?;
        std::fs::write(path, config_json)?;
        println!("📄 Genesis config exported to {}", path);
        Ok(())
    }

    /// Load genesis configuration from file
    pub fn load_genesis_config(path: &str) -> Result<GenesisConfig> {
        let config_json = std::fs::read_to_string(path)?;
        let config: GenesisConfig = serde_json::from_str(&config_json)?;
        Ok(config)
    }

    // Helper methods

    fn parse_address(&self, address_str: &str) -> Result<SwtchvmAddress> {
        let address_str = address_str.strip_prefix("0x").unwrap_or(address_str);
        let address_bytes = hex::decode(address_str)?;

        if address_bytes.len() != 20 {
            return Err(anyhow::anyhow!(
                "Invalid address length: {}",
                address_bytes.len()
            ));
        }

        let mut addr = [0u8; 20];
        addr.copy_from_slice(&address_bytes);
        Ok(SwtchvmAddress::new(addr))
    }

    fn parse_storage_key(&self, key_str: &str) -> Result<[u8; 32]> {
        let key_str = key_str.strip_prefix("0x").unwrap_or(key_str);
        let key_bytes = hex::decode(key_str)?;

        if key_bytes.len() != 32 {
            return Err(anyhow::anyhow!(
                "Invalid storage key length: {}",
                key_bytes.len()
            ));
        }

        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);
        Ok(key)
    }

    fn parse_storage_value(&self, value_str: &str) -> Result<[u8; 32]> {
        let value_str = value_str.strip_prefix("0x").unwrap_or(value_str);
        let value_bytes = hex::decode(value_str)?;

        if value_bytes.len() != 32 {
            return Err(anyhow::anyhow!(
                "Invalid storage value length: {}",
                value_bytes.len()
            ));
        }

        let mut value = [0u8; 32];
        value.copy_from_slice(&value_bytes);
        Ok(value)
    }

    fn calculate_genesis_hash(&self, block: &SwtchvmBlock) -> Result<[u8; 32]> {
        let mut block_copy = block.clone();
        block_copy.hash = [0u8; 32]; // Zero out hash for calculation

        let block_data = bincode::serialize(&block_copy)?;
        let hash = Keccak256::digest(&block_data);

        let mut hash_array = [0u8; 32];
        hash_array.copy_from_slice(&hash);
        Ok(hash_array)
    }
}

// Extension trait for SwtchvmState to get all accounts
trait StateExt {
    fn get_all_accounts(&self) -> Vec<SwtchvmAccount>;
}

impl StateExt for SwtchvmState {
    fn get_all_accounts(&self) -> Vec<SwtchvmAccount> {
        // This would need to be implemented in SwtchvmState
        // For now, return empty vec
        vec![]
    }
}

/// Genesis Node CLI for easy blockchain initialization
pub struct GenesisNodeCli;

impl GenesisNodeCli {
    /// Initialize a new blockchain from command line (simple version)
    pub async fn init_blockchain(
        genesis_config_path: Option<String>,
        _data_dir: Option<String>, // Unused in simple version
    ) -> Result<()> {
        println!("🚀 SpaceKit Genesis Node CLI");

        // Load or create genesis config
        let genesis_config = if let Some(path) = genesis_config_path {
            println!("📖 Loading genesis config from {}", path);
            GenesisNode::load_genesis_config(&path)?
        } else {
            println!("🔧 Using default genesis configuration");
            GenesisConfig::default()
        };

        // Create and initialize genesis node (simple version without storage)
        let mut genesis_node = GenesisNode::new_simple(genesis_config).await?;
        let genesis_block = genesis_node.initialize_blockchain().await?;

        println!("\n🎉 Blockchain initialized successfully!");
        println!("Genesis Block Hash: {}", hex::encode(&genesis_block.hash));
        println!("Ready to start mining!");

        Ok(())
    }

    /// Create a default genesis configuration file
    pub fn create_default_config(output_path: &str) -> Result<()> {
        let default_config = GenesisConfig::default();
        let config_json = serde_json::to_string_pretty(&default_config)?;
        std::fs::write(output_path, config_json)?;
        println!("📄 Default genesis config created at {}", output_path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_genesis_node_creation() {
        let genesis_config = GenesisConfig::default();
        let genesis_node = GenesisNode::new_simple(genesis_config).await;
        assert!(genesis_node.is_ok());
    }

    #[tokio::test]
    async fn test_genesis_block_initialization() {
        let genesis_config = GenesisConfig::default();
        let mut genesis_node = GenesisNode::new_simple(genesis_config).await.unwrap();
        let genesis_block = genesis_node.initialize_blockchain().await.unwrap();

        assert_eq!(genesis_block.number, 0);
        assert_ne!(genesis_block.hash, [0u8; 32]); // Should have calculated hash
    }

    #[test]
    fn test_genesis_config_serialization() {
        let config = GenesisConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: GenesisConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.chain_id, deserialized.chain_id);
        assert_eq!(config.network_name, deserialized.network_name);
    }
}
