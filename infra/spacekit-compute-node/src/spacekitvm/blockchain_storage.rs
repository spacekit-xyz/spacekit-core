//! Blockchain Storage using SWTCH Storage Node
//!
//! Provides quantum-safe, distributed blockchain persistence using the existing
//! swtch-storage-node infrastructure.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use spacekit_primitives::v1::crypto::quantum::Algorithm;
use spacekit_storage_node::{QuantumCrypto, StorageNode, StorageNodeConfig};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{SwtchvmAccount, SwtchvmAddress, SwtchvmBlock, SwtchvmState, SwtchvmTransaction};

/// Blockchain Storage Manager using SWTCH Storage Node
pub struct BlockchainStorage {
    storage_node: Arc<StorageNode>,
    quantum_crypto: Arc<QuantumCrypto>,
    block_cache: Arc<RwLock<HashMap<u64, SwtchvmBlock>>>,
    state_cache: Arc<RwLock<SwtchvmState>>,
    config: BlockchainStorageConfig,
    // Blockchain keypair for consistent encryption/decryption
    blockchain_public_key: Vec<u8>,
    blockchain_private_key: Vec<u8>,
}

/// Configuration for blockchain storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainStorageConfig {
    /// Data directory for blockchain storage
    pub data_dir: String,
    /// Quantum encryption algorithm
    pub encryption_algorithm: Algorithm,
    /// Enable distributed storage across P2P network
    pub enable_distributed: bool,
    /// Replication factor for distributed storage
    pub replication_factor: usize,
    /// Cache size for blocks (number of blocks to cache)
    pub block_cache_size: usize,
    /// Batch size for bulk operations
    pub batch_size: usize,
}

impl Default for BlockchainStorageConfig {
    fn default() -> Self {
        Self {
            data_dir: "./blockchain_data".to_string(),
            encryption_algorithm: Algorithm::Kyber1024,
            enable_distributed: true,
            replication_factor: 3,
            block_cache_size: 1000,
            batch_size: 100,
        }
    }
}

/// Storage keys for different blockchain data types
pub struct StorageKeys;

impl StorageKeys {
    pub const GENESIS_BLOCK: &'static str = "genesis_block";
    pub const LATEST_BLOCK_NUMBER: &'static str = "latest_block_number";
    pub const CHAIN_HEAD: &'static str = "chain_head";

    pub fn block_key(block_number: u64) -> String {
        format!("block_{:012}", block_number)
    }

    pub fn block_hash_key(hash: &[u8; 32]) -> String {
        format!("block_hash_{}", hex::encode(hash))
    }

    pub fn transaction_key(tx_hash: &[u8; 32]) -> String {
        format!("transaction_{}", hex::encode(tx_hash))
    }

    pub fn account_key(address: &SwtchvmAddress) -> String {
        format!("account_{}", hex::encode(address.as_bytes()))
    }

    pub fn state_root_key(block_number: u64) -> String {
        format!("state_root_{:012}", block_number)
    }
}

impl BlockchainStorage {
    /// Create a new blockchain storage instance
    pub async fn new(config: BlockchainStorageConfig) -> Result<Self> {
        // Configure storage node for blockchain use
        let storage_config = StorageNodeConfig {
            max_storage_bytes: 1_000_000_000_000, // 1TB for blockchain data
            data_dir: std::path::PathBuf::from(&config.data_dir),
            database_path: Some(std::path::PathBuf::from(format!(
                "{}/blockchain.db",
                config.data_dir
            ))),
            node_did: "did:swtch:blockchain-storage".to_string(),
            preferred_algorithm: format!("{:?}", config.encryption_algorithm),
            encryption_keypair: None, // Will be generated
            network_config: spacekit_storage_node::NetworkConfig {
                listen_port: 0, // Auto-assign available port
                bootstrap_peers: vec![],
                max_connections: 50,
                max_concurrent_operations: Some(10),
                replication_factor: config.replication_factor,
                chunk_size: 1024 * 1024, // 1MB chunks for large blocks
                cache_p2p_chunks_in_memory: false,
            },
            ..Default::default()
        };

        let storage_node = Arc::new(StorageNode::new(storage_config).await?);

        // Initialize quantum crypto with chosen algorithm
        let quantum_crypto = Arc::new(QuantumCrypto::new(
            config.encryption_algorithm.clone(),
            spacekit_primitives::v1::crypto::quantum::CipherSuite::AES256,
        ));

        let block_cache = Arc::new(RwLock::new(HashMap::new()));
        let state_cache = Arc::new(RwLock::new(SwtchvmState::new()));

        // Generate blockchain keypair for consistent encryption/decryption
        let (public_key, private_key) = quantum_crypto
            .generate_keypair(config.encryption_algorithm.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to generate blockchain keypair: {}", e))?;

        Ok(Self {
            storage_node,
            quantum_crypto,
            block_cache,
            state_cache,
            config,
            blockchain_public_key: public_key,
            blockchain_private_key: private_key,
        })
    }

    /// Store a block in the blockchain storage
    pub async fn store_block(&self, block: &SwtchvmBlock) -> Result<()> {
        println!("Storing block {} to quantum-safe storage", block.number);

        // Serialize block
        let block_data = serde_json::to_vec(block)?;

        // Store block by number
        let block_key = StorageKeys::block_key(block.number);
        self.store_data(&block_key, &block_data).await?;

        // Store block by hash for quick lookups
        let hash_key = StorageKeys::block_hash_key(&block.hash);
        let block_ref = serde_json::to_vec(&block.number)?;
        self.store_data(&hash_key, &block_ref).await?;

        // Store individual transactions for indexing
        for (tx_index, tx) in block.transactions.iter().enumerate() {
            let tx_hash = self.calculate_transaction_hash(tx)?;
            let tx_key = StorageKeys::transaction_key(&tx_hash);

            // Store transaction with block reference
            let tx_metadata = TransactionMetadata {
                transaction: tx.clone(),
                block_number: block.number,
                transaction_index: tx_index,
                block_hash: block.hash,
            };
            let tx_metadata_data = serde_json::to_vec(&tx_metadata)?;
            self.store_data(&tx_key, &tx_metadata_data).await?;
        }

        // Update latest block number
        let latest_data = serde_json::to_vec(&block.number)?;
        self.store_data(StorageKeys::LATEST_BLOCK_NUMBER, &latest_data)
            .await?;

        // Update cache
        let mut cache = self.block_cache.write().await;
        cache.insert(block.number, block.clone());

        // Maintain cache size
        if cache.len() > self.config.block_cache_size {
            if let Some(min_key) = cache.keys().min().copied() {
                cache.remove(&min_key);
            }
        }

        println!("✅ Block {} stored successfully", block.number);
        Ok(())
    }

    /// Retrieve a block by number
    pub async fn get_block(&self, block_number: u64) -> Result<Option<SwtchvmBlock>> {
        // Check cache first
        {
            let cache = self.block_cache.read().await;
            if let Some(block) = cache.get(&block_number) {
                return Ok(Some(block.clone()));
            }
        }

        // Load from storage
        let block_key = StorageKeys::block_key(block_number);
        if let Some(block_data) = self.retrieve_data(&block_key).await? {
            let block: SwtchvmBlock = serde_json::from_slice(&block_data)?;

            // Update cache
            let mut cache = self.block_cache.write().await;
            cache.insert(block_number, block.clone());

            Ok(Some(block))
        } else {
            Ok(None)
        }
    }

    /// Retrieve a block by hash
    pub async fn get_block_by_hash(&self, hash: &[u8; 32]) -> Result<Option<SwtchvmBlock>> {
        let hash_key = StorageKeys::block_hash_key(hash);
        if let Some(block_ref_data) = self.retrieve_data(&hash_key).await? {
            let block_number: u64 = serde_json::from_slice(&block_ref_data)?;
            self.get_block(block_number).await
        } else {
            Ok(None)
        }
    }

    /// Get the latest block number
    pub async fn get_latest_block_number(&self) -> Result<Option<u64>> {
        if let Some(data) = self.retrieve_data(StorageKeys::LATEST_BLOCK_NUMBER).await? {
            let block_number: u64 = serde_json::from_slice(&data)?;
            Ok(Some(block_number))
        } else {
            Ok(None)
        }
    }

    /// Store the genesis block
    pub async fn store_genesis_block(&self, genesis_block: &SwtchvmBlock) -> Result<()> {
        println!("Storing genesis block");

        // Store genesis block
        self.store_block(genesis_block).await?;

        // Mark as genesis
        let genesis_data = serde_json::to_vec(genesis_block)?;
        self.store_data(StorageKeys::GENESIS_BLOCK, &genesis_data)
            .await?;

        println!("✅ Genesis block stored");
        Ok(())
    }

    /// Retrieve the genesis block
    pub async fn get_genesis_block(&self) -> Result<Option<SwtchvmBlock>> {
        if let Some(data) = self.retrieve_data(StorageKeys::GENESIS_BLOCK).await? {
            let genesis_block: SwtchvmBlock = serde_json::from_slice(&data)?;
            Ok(Some(genesis_block))
        } else {
            Ok(None)
        }
    }

    /// Store account state
    pub async fn store_account(
        &self,
        address: &SwtchvmAddress,
        account: &SwtchvmAccount,
    ) -> Result<()> {
        let account_key = StorageKeys::account_key(address);
        let account_data = serde_json::to_vec(account)?;
        self.store_data(&account_key, &account_data).await?;

        // Update state cache
        let mut state_cache = self.state_cache.write().await;
        let state_account = state_cache.get_account_mut(address);
        *state_account = account.clone();

        Ok(())
    }

    /// Retrieve account state
    pub async fn get_account(&self, address: &SwtchvmAddress) -> Result<Option<SwtchvmAccount>> {
        // Check state cache first
        {
            let state_cache = self.state_cache.read().await;
            if let Some(account) = state_cache.get_account(address) {
                return Ok(Some(account.clone()));
            }
        }

        // Load from storage
        let account_key = StorageKeys::account_key(address);
        if let Some(account_data) = self.retrieve_data(&account_key).await? {
            let account: SwtchvmAccount = serde_json::from_slice(&account_data)?;

            // Update cache
            let mut state_cache = self.state_cache.write().await;
            let cached_account = state_cache.get_account_mut(address);
            *cached_account = account.clone();

            Ok(Some(account))
        } else {
            Ok(None)
        }
    }

    /// Store complete blockchain state at a specific block
    pub async fn store_state_at_block(
        &self,
        block_number: u64,
        state: &SwtchvmState,
    ) -> Result<()> {
        let state_key = StorageKeys::state_root_key(block_number);
        let state_data = bincode::serialize(state)?;
        self.store_data(&state_key, &state_data).await?;

        // Update state cache
        let mut state_cache = self.state_cache.write().await;
        *state_cache = state.clone();

        Ok(())
    }

    /// Get transaction by hash
    pub async fn get_transaction(&self, tx_hash: &[u8; 32]) -> Result<Option<TransactionMetadata>> {
        let tx_key = StorageKeys::transaction_key(tx_hash);
        if let Some(tx_data) = self.retrieve_data(&tx_key).await? {
            let tx_metadata: TransactionMetadata = serde_json::from_slice(&tx_data)?;
            Ok(Some(tx_metadata))
        } else {
            Ok(None)
        }
    }

    /// Initialize blockchain storage (create genesis if needed)
    pub async fn initialize(&self) -> Result<bool> {
        println!("Initializing blockchain storage...");

        // Check if genesis block exists
        if let Some(_genesis) = self.get_genesis_block().await? {
            println!("✅ Blockchain already initialized");
            return Ok(false);
        }

        println!("🔥 No genesis block found - blockchain needs initialization");
        Ok(true)
    }

    /// Get blockchain statistics
    pub async fn get_stats(&self) -> Result<BlockchainStorageStats> {
        let latest_block_number = self.get_latest_block_number().await?.unwrap_or(0);
        let cache_size = self.block_cache.read().await.len();

        Ok(BlockchainStorageStats {
            latest_block_number,
            total_blocks: latest_block_number + 1,
            cache_size,
            storage_algorithm: format!("{:?}", self.config.encryption_algorithm),
            distributed_storage: self.config.enable_distributed,
            replication_factor: self.config.replication_factor,
        })
    }

    // Private helper methods

    async fn store_data(&self, key: &str, data: &[u8]) -> Result<()> {
        // Use storage node's quantum-safe storage with blockchain keypair
        self.storage_node
            .store_file(
                &format!("blockchain:{}", key),
                data,
                &"did:swtch:blockchain".to_string(),
                &self.blockchain_public_key,
                None,
            )
            .await?;

        println!("📁 Stored blockchain data: {} ({} bytes)", key, data.len());
        Ok(())
    }

    async fn retrieve_data(&self, key: &str) -> Result<Option<Vec<u8>>> {
        // Retrieve using blockchain private key for zero-knowledge decryption
        match self
            .storage_node
            .retrieve_file(
                &format!("blockchain:{}", key),
                &"did:swtch:blockchain".to_string(),
                &self.blockchain_private_key,
            )
            .await
        {
            Ok(Some(data)) => Ok(Some(data)),
            Ok(None) => Ok(None),
            Err(_) => Ok(None), // File doesn't exist
        }
    }

    fn calculate_transaction_hash(&self, tx: &SwtchvmTransaction) -> Result<[u8; 32]> {
        use sha3::{Digest, Keccak256};
        let tx_data = serde_json::to_vec(tx)?;
        let hash = Keccak256::digest(&tx_data);
        let mut hash_array = [0u8; 32];
        hash_array.copy_from_slice(&hash);
        Ok(hash_array)
    }
}

/// Transaction metadata for indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionMetadata {
    pub transaction: SwtchvmTransaction,
    pub block_number: u64,
    pub transaction_index: usize,
    pub block_hash: [u8; 32],
}

/// Blockchain storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainStorageStats {
    pub latest_block_number: u64,
    pub total_blocks: u64,
    pub cache_size: usize,
    pub storage_algorithm: String,
    pub distributed_storage: bool,
    pub replication_factor: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_blockchain_storage_initialization() {
        let temp_dir = tempdir().unwrap();
        let config = BlockchainStorageConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let storage = BlockchainStorage::new(config).await.unwrap();
        let needs_init = storage.initialize().await.unwrap();
        assert!(needs_init); // Should need initialization for new blockchain
    }

    #[tokio::test]
    async fn test_block_storage_and_retrieval() {
        let temp_dir = tempdir().unwrap();
        let config = BlockchainStorageConfig {
            data_dir: temp_dir.path().to_string_lossy().to_string(),
            ..Default::default()
        };

        let storage = BlockchainStorage::new(config).await.unwrap();

        // Create test block
        let test_block = SwtchvmBlock {
            number: 1,
            parent_hash: [0u8; 32],
            hash: [1u8; 32],
            timestamp: 1000,
            gas_limit: 1000000,
            gas_used: 0,
            transactions: vec![],
            state_root: [0u8; 32],
            compute_root: [0u8; 32],
            receipts: vec![],
            verkle_witness: None,
        };

        // Store block
        storage.store_block(&test_block).await.unwrap();

        // Retrieve block
        let retrieved_block = storage.get_block(1).await.unwrap().unwrap();
        assert_eq!(retrieved_block.number, test_block.number);
        assert_eq!(retrieved_block.hash, test_block.hash);
    }
}
