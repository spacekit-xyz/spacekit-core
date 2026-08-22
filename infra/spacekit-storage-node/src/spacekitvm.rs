//! SpaceKitVM Integration Module for SpaceKit Storage Node
//!
//! This module provides SpaceKitVM-compatible storage functionality, allowing
//! storage contracts to use the SpaceKit Storage Node's quantum-safe storage
//! capabilities.

use anyhow::Result;
use hex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

// Re-export storage node types for SpaceKitVM compatibility
pub use crate::{
    AccessControlEntry, EncryptionInfo, FilePermissions, NetworkConfig, PersistenceConfig,
    StorageError, StorageNode, StorageNodeConfig, StorageStats, StoredFile,
};

// Re-export quantum crypto types
pub use crate::quantum::{EncryptedData, EncryptionMetadata, QuantumCrypto};

// Re-export database types
pub use crate::database::{Database, FileMetadata};

/// SpaceKitVM Storage Contract Interface
///
/// This trait provides a SpaceKitVM-compatible interface for storage operations.
/// It wraps the SpaceKit Storage Node functionality for use in SpaceKitVM contracts.
#[async_trait::async_trait]
pub trait SpacekitvmStorageContract: Send + Sync {
    /// Initialize the storage contract with configuration
    async fn initialize(&mut self, config: SpacekitvmStorageConfig) -> Result<()>;

    /// Store a file with quantum-safe encryption
    async fn store_file(
        &mut self,
        owner_did: &str,
        file_data: Vec<u8>,
        encryption_algorithm: &str,
    ) -> Result<SpacekitvmStorageResult>;

    /// Retrieve a file with access control
    async fn retrieve_file(&self, file_id: &str, requester_did: &str) -> Result<Option<Vec<u8>>>;

    /// Grant access to a file
    async fn grant_access(
        &mut self,
        file_id: &str,
        granter_did: &str,
        grantee_did: &str,
        permissions: SpacekitvmFilePermissions,
    ) -> Result<bool>;

    /// Revoke access to a file
    async fn revoke_access(
        &mut self,
        file_id: &str,
        revoker_did: &str,
        target_did: &str,
    ) -> Result<bool>;

    /// Get storage statistics
    async fn get_stats(&self) -> SpacekitvmStorageStats;
}

/// SpacekitVM Storage Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacekitvmStorageConfig {
    pub max_storage_bytes: u64,
    pub preferred_algorithm: String,
    pub replication_factor: usize,
    pub enable_p2p: bool,
    pub enable_reputation: bool,
    pub data_dir: String,
    pub node_did: String,
    /// Optional default public key (hex) for VM-driven storage
    pub owner_public_key_hex: Option<String>,
    /// Optional default private key (hex) for VM-driven retrieval
    pub user_private_key_hex: Option<String>,
}

impl Default for SpacekitvmStorageConfig {
    fn default() -> Self {
        Self {
            max_storage_bytes: 100 * 1024 * 1024 * 1024, // 100GB
            preferred_algorithm: "kyber1024".to_string(),
            replication_factor: 3,
            enable_p2p: true,
            enable_reputation: true,
            data_dir: "./spacekitvm_storage".to_string(),
            node_did: String::new(),
            owner_public_key_hex: None,
            user_private_key_hex: None,
        }
    }
}

/// SpacekitVM File Permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpacekitvmFilePermissions {
    Read,
    Write,
    ReadWrite,
    Admin,
}

impl From<SpacekitvmFilePermissions> for FilePermissions {
    fn from(spacekitvm_perms: SpacekitvmFilePermissions) -> Self {
        match spacekitvm_perms {
            SpacekitvmFilePermissions::Read => FilePermissions::Read,
            SpacekitvmFilePermissions::Write => FilePermissions::Write,
            SpacekitvmFilePermissions::ReadWrite => FilePermissions::ReadWrite,
            SpacekitvmFilePermissions::Admin => FilePermissions::Admin,
        }
    }
}

/// Spacekitvm Storage Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacekitvmStorageResult {
    pub file_id: String,
    pub chunks_stored: usize,
    pub encryption_algorithm: String,
    pub storage_cost: u64,
    pub reputation_impact: f64,
}

/// Spacekitvm Storage Statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacekitvmStorageStats {
    pub file_count: usize,
    pub total_size_bytes: u64,
    pub storage_utilization: f64,
    pub active_users: usize,
    pub reputation_scores: HashMap<String, f64>,
}

/// Spacekitvm Storage Node Wrapper
///
/// This struct wraps the Spacekitvm Storage Node to provide Spacekitvm-compatible
/// storage functionality for smart contracts.
pub struct SpacekitvmStorageNode {
    storage_node: Arc<StorageNode>,
    config: SpacekitvmStorageConfig,
    reputation_scores: HashMap<String, f64>,
}
impl SpacekitvmStorageNode {
    /// Create a new Spacekitvm storage node
    pub async fn new(config: SpacekitvmStorageConfig) -> Result<Self> {
        info!("Creating Spacekitvm storage node with config: {:?}", config);

        // Convert Spacekitvm config to StorageNodeConfig
        let storage_config = StorageNodeConfig {
            max_storage_bytes: config.max_storage_bytes,
            data_dir: std::path::PathBuf::from(&config.data_dir),
            database_path: None,
            node_did: config.node_did.clone(),
            preferred_algorithm: config.preferred_algorithm.clone(),
            encryption_keypair: None,
            network_config: NetworkConfig {
                listen_port: 4001,
                bootstrap_peers: Vec::new(),
                max_connections: 50,
                max_concurrent_operations: Some(10),
                replication_factor: config.replication_factor,
                chunk_size: 1024 * 1024, // 1MB chunks
                cache_p2p_chunks_in_memory: false,
            },
            enable_p2p: false,
            enable_real_transactions: false,
            persistence: PersistenceConfig::default(),
            #[cfg(feature = "api-server")]
            api_config: None,
        };

        let storage_node = Arc::new(StorageNode::new(storage_config).await?);

        Ok(Self {
            storage_node,
            config,
            reputation_scores: HashMap::new(),
        })
    }

    fn owner_public_key_bytes(&self, owner_did: &str) -> Result<Vec<u8>> {
        let key_hex = self
            .config
            .owner_public_key_hex
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing owner public key for DID {}", owner_did))?;
        hex::decode(key_hex).map_err(|e| anyhow::anyhow!("Invalid owner public key hex: {}", e))
    }

    fn user_private_key_bytes(&self, requester_did: &str) -> Result<Vec<u8>> {
        let key_hex =
            self.config.user_private_key_hex.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Missing user private key for DID {}", requester_did)
            })?;
        hex::decode(key_hex).map_err(|e| anyhow::anyhow!("Invalid user private key hex: {}", e))
    }

    /// Get the underlying storage node
    pub fn storage_node(&self) -> Arc<StorageNode> {
        self.storage_node.clone()
    }

    /// Update reputation score for a DID
    pub fn update_reputation(&mut self, did: &str, score: f64) {
        self.reputation_scores.insert(did.to_string(), score);
    }

    /// Get reputation score for a DID
    pub fn get_reputation(&self, did: &str) -> f64 {
        self.reputation_scores.get(did).copied().unwrap_or(0.0)
    }

    /// Calculate storage cost based on reputation
    pub fn calculate_storage_cost(&self, size: usize, did: &str) -> u64 {
        let base_cost = (size as u64) / (1024 * 1024); // 1 credit per MB

        let reputation = self.get_reputation(did);
        let discount = reputation * 0.1; // Up to 10% discount

        ((base_cost as f64) * (1.0 - discount)) as u64
    }
}

#[async_trait::async_trait]
impl SpacekitvmStorageContract for SpacekitvmStorageNode {
    async fn initialize(&mut self, config: SpacekitvmStorageConfig) -> Result<()> {
        info!("Initializing SpacekitVM storage node with new config");
        self.config = config;
        Ok(())
    }

    async fn store_file(
        &mut self,
        owner_did: &str,
        file_data: Vec<u8>,
        encryption_algorithm: &str,
    ) -> Result<SpacekitvmStorageResult> {
        info!(
            "Storing file via SpacekitVM: {} bytes for DID: {}",
            file_data.len(),
            owner_did
        );

        // Generate a filename for the storage node
        let filename = format!("spacekitvm_file_{}", uuid::Uuid::new_v4());

        // Store file using the storage node
        let owner_public_key = self.owner_public_key_bytes(owner_did)?;
        let (file_id, _public_key_hex) = self
            .storage_node
            .store_file(
                &filename,
                &file_data,
                owner_did,
                &owner_public_key,
                Some("application/octet-stream".to_string()),
            )
            .await?;

        // Calculate storage cost
        let storage_cost = self.calculate_storage_cost(file_data.len(), owner_did);

        // Update reputation
        let reputation_impact = 0.1; // Positive impact for storing
        self.update_reputation(
            owner_did,
            self.get_reputation(owner_did) + reputation_impact,
        );

        Ok(SpacekitvmStorageResult {
            file_id,
            chunks_stored: self.config.replication_factor,
            encryption_algorithm: encryption_algorithm.to_string(),
            storage_cost,
            reputation_impact,
        })
    }

    async fn retrieve_file(&self, file_id: &str, requester_did: &str) -> Result<Option<Vec<u8>>> {
        info!(
            "Retrieving file via SpacekitVM: {} for DID: {}",
            file_id, requester_did
        );

        // Retrieve file using the storage node
        let user_private_key = self.user_private_key_bytes(requester_did)?;
        self.storage_node
            .retrieve_file(file_id, requester_did, &user_private_key)
            .await
    }

    async fn grant_access(
        &mut self,
        file_id: &str,
        granter_did: &str,
        grantee_did: &str,
        permissions: SpacekitvmFilePermissions,
    ) -> Result<bool> {
        info!(
            "Granting access via SpacekitVM: {} to {} by {}",
            file_id, grantee_did, granter_did
        );

        // Grant access using the storage node
        self.storage_node
            .grant_access(file_id, granter_did, grantee_did, permissions.into())
            .await
    }

    async fn revoke_access(
        &mut self,
        file_id: &str,
        revoker_did: &str,
        target_did: &str,
    ) -> Result<bool> {
        info!(
            "Revoking access via SpacekitVM: {} from {} by {}",
            file_id, target_did, revoker_did
        );

        self.storage_node
            .revoke_access(file_id, revoker_did, target_did)
            .await
    }

    async fn get_stats(&self) -> SpacekitvmStorageStats {
        let stats = self
            .storage_node
            .get_stats()
            .await
            .unwrap_or_else(|_| StorageStats {
                file_count: 0,
                total_size_bytes: 0,
                max_storage_bytes: self.config.max_storage_bytes,
                storage_utilization: 0.0,
                user_count: 0,
                encrypted_user_count: 0,
                message_count: 0,
                node_did: self.config.node_did.clone(),
                preferred_algorithm: self.config.preferred_algorithm.clone(),
            });

        SpacekitvmStorageStats {
            file_count: stats.file_count,
            total_size_bytes: stats.total_size_bytes,
            storage_utilization: stats.storage_utilization,
            active_users: self.reputation_scores.len(),
            reputation_scores: self.reputation_scores.clone(),
        }
    }
}

/// Create a SpacekitVM storage contract from storage node configuration
pub async fn create_spacekitvm_storage_contract(
    config: SpacekitvmStorageConfig,
) -> Result<SpacekitvmStorageNode> {
    SpacekitvmStorageNode::new(config).await
}

/// SpacekitVM Storage Contract Factory
///
/// Provides factory methods for creating different types of SpacekitVM storage contracts.
pub struct SpacekitvmStorageFactory;

impl SpacekitvmStorageFactory {
    /// Create a quantum-safe storage contract
    pub async fn create_quantum_safe_storage() -> Result<SpacekitvmStorageNode> {
        let config = SpacekitvmStorageConfig {
            preferred_algorithm: "kyber1024".to_string(),
            ..Default::default()
        };
        create_spacekitvm_storage_contract(config).await
    }

    /// Create a distributed storage contract with P2P
    pub async fn create_distributed_storage() -> Result<SpacekitvmStorageNode> {
        let config = SpacekitvmStorageConfig {
            enable_p2p: true,
            replication_factor: 5,
            ..Default::default()
        };
        create_spacekitvm_storage_contract(config).await
    }

    /// Create a reputation-based storage contract
    pub async fn create_reputation_storage() -> Result<SpacekitvmStorageNode> {
        let config = SpacekitvmStorageConfig {
            enable_reputation: true,
            ..Default::default()
        };
        create_spacekitvm_storage_contract(config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_spacekitvm_storage_node_creation() {
        let config = SpacekitvmStorageConfig {
            data_dir: tempdir().unwrap().path().to_str().unwrap().to_string(),
            ..Default::default()
        };

        let storage_node = SpacekitvmStorageNode::new(config).await;
        assert!(storage_node.is_ok());
    }

    #[tokio::test]
    async fn test_spacekitvm_storage_contract_operations() {
        let config = SpacekitvmStorageConfig {
            data_dir: tempdir().unwrap().path().to_str().unwrap().to_string(),
            ..Default::default()
        };

        let mut contract = SpacekitvmStorageNode::new(config).await.unwrap();

        // Test initialization
        let init_config = SpacekitvmStorageConfig::default();
        assert!(contract.initialize(init_config).await.is_ok());

        // Test file storage
        let test_data = b"Hello, SpacekitVM storage!".to_vec();
        let result = contract
            .store_file("test_did", test_data, "kyber1024")
            .await;
        assert!(result.is_ok());

        // Test stats
        let stats = contract.get_stats().await;
        assert!(stats.file_count > 0);
    }

    #[tokio::test]
    async fn test_storage_factory() {
        let quantum_storage = SpacekitvmStorageFactory::create_quantum_safe_storage().await;
        assert!(quantum_storage.is_ok());

        let distributed_storage = SpacekitvmStorageFactory::create_distributed_storage().await;
        assert!(distributed_storage.is_ok());

        let reputation_storage = SpacekitvmStorageFactory::create_reputation_storage().await;
        assert!(reputation_storage.is_ok());
    }
}
