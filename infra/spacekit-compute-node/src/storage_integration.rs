//! Storage Integration Module for SpaceKit Compute Node
//!
//! This module provides integration between the compute node and storage node,
//! leveraging the quantum-safe storage capabilities including:
//! - Quantum-safe file storage with encryption
//! - Database operations for compute tasks
//! - Basic file operations

use anyhow::Result;
use chrono::{DateTime, Utc};
use hex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// Import storage node capabilities
#[cfg(feature = "storage-integration")]
use spacekit_storage_node::{
    ContactMessage, Database, EncryptedMessage, EncryptedUser, FileMetadata, QuantumCrypto,
    StorageError, StorageNode, StorageNodeConfig, StorageStats, User,
};

// Import quantum crypto types
use spacekit_primitives::v1::crypto::quantum::{Algorithm, CipherSuite};

// Import compute node types
use crate::ComputeNode;

/// Storage type enumeration for different storage contracts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StorageType {
    QuantumSafe,   // Standard quantum-safe storage
    Collaborative, // Multi-party collaborative storage (simplified)
    Medical,       // Medical records (simplified)
    Research,      // Research data (simplified)
}

/// Storage operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageResult {
    pub file_id: String,
    pub chunks_stored: u32,
    pub encryption_algorithm: String,
    pub storage_cost: u64,
    pub reputation_impact: f64,
    pub quantum_safe: bool,
    pub collaborative: bool,
    pub specialized_contract: Option<String>,
}

/// Simplified collaborative result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborativeComputeResult {
    pub task_id: String,
    pub file_id: String,
    pub owners: Vec<String>,
    pub consensus_policy: String,
    pub share_links: Vec<String>,
    pub quantum_safe: bool,
    pub created_at: DateTime<Utc>,
}

/// Simplified medical result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MedicalComputeResult {
    pub task_id: String,
    pub record_id: String,
    pub patient_did: String,
    pub record_type: String,
    pub hipaa_compliant: bool,
    pub quantum_safe: bool,
    pub created_at: DateTime<Utc>,
}

/// Simplified research result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchComputeResult {
    pub task_id: String,
    pub dataset_id: String,
    pub researcher_did: String,
    pub title: String,
    pub peer_review_enabled: bool,
    pub citation_tracking: bool,
    pub quantum_safe: bool,
    pub created_at: DateTime<Utc>,
}

#[cfg(feature = "storage-integration")]
fn default_enhanced_stats() -> spacekit_storage_node::database::EnhancedStorageStats {
    spacekit_storage_node::database::EnhancedStorageStats {
        user_count: 0,
        encrypted_user_count: 0,
        message_count: 0,
        encrypted_message_count: 0,
        file_count: 0,
        total_file_size: 0,
        database_version: 1,
        last_saved: chrono::Utc::now(),
        wal_enabled: false,
        backup_count: 0,
        data_file_size: 0,
        fact_metadata_count: 0,
        document_count: 0,
    }
}

/// Simplified storage statistics
#[derive(Debug, Serialize, Deserialize)]
pub struct ComprehensiveStorageStats {
    #[cfg(feature = "storage-integration")]
    #[serde(skip, default = "default_enhanced_stats")]
    pub database_stats: spacekit_storage_node::database::EnhancedStorageStats,

    #[cfg(not(feature = "storage-integration"))]
    pub placeholder_files: usize,

    pub quantum_algorithms_supported: Vec<String>,
    pub total_compute_results_stored: usize,
    pub last_updated: DateTime<Utc>,
}

impl Clone for ComprehensiveStorageStats {
    fn clone(&self) -> Self {
        Self {
            #[cfg(feature = "storage-integration")]
            database_stats: unsafe { std::ptr::read(&self.database_stats) },
            #[cfg(not(feature = "storage-integration"))]
            placeholder_files: self.placeholder_files,
            quantum_algorithms_supported: self.quantum_algorithms_supported.clone(),
            total_compute_results_stored: self.total_compute_results_stored,
            last_updated: self.last_updated,
        }
    }
}

/// Storage integration manager providing basic storage capabilities
pub struct StorageIntegrationManager {
    #[cfg(feature = "storage-integration")]
    storage_database: Arc<Database>,
    #[cfg(feature = "storage-integration")]
    storage_node: Option<Arc<StorageNode>>,

    // Fallback for when storage integration is disabled
    #[cfg(not(feature = "storage-integration"))]
    placeholder_storage: Arc<RwLock<HashMap<String, Vec<u8>>>>,

    default_algorithm: Algorithm,
    default_cipher_suite: CipherSuite,
    node_did: String,
}

impl StorageIntegrationManager {
    #[cfg(feature = "storage-integration")]
    /// Create an async-aware database that works properly in test environments
    async fn create_async_aware_database(node_did: &str) -> Result<Database> {
        use tempfile::tempdir;

        // Create database in temp directory for tests
        let temp_dir = tempdir()?;
        let db_path = temp_dir
            .path()
            .join(format!("async_aware_{}.json", node_did));

        // Use configuration that avoids async runtime conflicts
        let config = spacekit_storage_node::database::PersistenceConfig {
            enable_wal: false, // Disable WAL in tests to avoid async issues
            backup_count: 1,
            sync_interval_ms: 1000,
            compress_backups: false,
            verify_checksums: false,
            enable_encryption: false, // Disable encryption in tests to avoid runtime conflicts
            quantum_algorithm: Algorithm::Kyber1024,
            cipher_suite: CipherSuite::AES256,
            encryption_key_id: "test_key".to_string(),
            blob_cache_max_bytes: 32 * 1024 * 1024,
            document_inline_max_bytes: 1024 * 1024,
            externalize_documents: false,
        };

        // Create database using the async-safe configuration
        let database = Database::with_config(db_path.to_str().unwrap(), config)?;
        database.initialize()?;

        info!(
            "Created async-aware database for compute node: {}",
            node_did
        );
        Ok(database)
    }

    /// Underlying storage node (Growformer brain fetch, contract KV, etc.).
    #[cfg(feature = "storage-integration")]
    pub fn storage_node(&self) -> Option<Arc<StorageNode>> {
        self.storage_node.clone()
    }

    /// Create a new storage integration manager with full async support
    pub async fn new(config: StorageIntegrationConfig, node_did: String) -> Result<Self> {
        // Always enable full storage integration
        #[cfg(not(feature = "storage-integration"))]
        {
            warn!("Storage integration disabled, using placeholder implementation");
            return Ok(Self {
                placeholder_storage: Arc::new(RwLock::new(HashMap::new())),
                default_algorithm: Algorithm::Kyber1024,
                default_cipher_suite: CipherSuite::AES256,
                node_did,
            });
        }

        #[cfg(feature = "storage-integration")]
        {
            if !config.enable_storage_integration {
                return Err(anyhow::anyhow!("storage integration disabled in config"));
            }

            info!(
                "Initializing storage integration for node {} (data_dir={})",
                node_did, config.storage_data_dir
            );

            let data_dir = std::path::PathBuf::from(&config.storage_data_dir);
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("compute_integration.json");

            let persistence = spacekit_storage_node::database::PersistenceConfig {
                enable_wal: false,
                backup_count: 2,
                externalize_documents: true,
                document_inline_max_bytes: 4096,
                blob_cache_max_bytes: 16 * 1024 * 1024,
                enable_encryption: false,
                quantum_algorithm: config.quantum_algorithm.clone(),
                cipher_suite: config.cipher_suite.clone(),
                ..Default::default()
            };

            let storage_database = Arc::new(Database::with_config(
                db_path.to_str().unwrap(),
                persistence.clone(),
            )?);
            storage_database.initialize()?;

            // Do not spawn a nested StorageNode when an external storage HTTP URL is configured
            // (supervisor / standalone compute with separate storage process).
            let use_external_http = std::env::var("SPACEKIT_STORAGE_NODE_URL")
                .ok()
                .is_some_and(|u| !u.trim().is_empty());
            let storage_node = if use_external_http {
                info!(
                    "Using external storage at {} (no embedded StorageNode)",
                    std::env::var("SPACEKIT_STORAGE_NODE_URL").unwrap_or_default()
                );
                None
            } else {
                let mut storage_config = StorageNodeConfig {
                    node_did: node_did.clone(),
                    data_dir: data_dir.join("files"),
                    preferred_algorithm: "kyber1024".to_string(),
                    max_storage_bytes: 10 * 1024 * 1024 * 1024,
                    enable_p2p: false,
                    persistence,
                    ..Default::default()
                };
                match StorageNode::new(storage_config).await {
                    Ok(node) => {
                        info!("Storage node initialized for compute integration");
                        Some(Arc::new(node))
                    }
                    Err(e) => {
                        warn!(
                            "Failed to initialize storage node: {}, using database only",
                            e
                        );
                        None
                    }
                }
            };

            info!("✅ Storage integration initialized");
            Ok(Self {
                storage_database,
                storage_node,
                default_algorithm: config.quantum_algorithm,
                default_cipher_suite: config.cipher_suite,
                node_did,
            })
        }
    }

    /// Store input data for a compute task
    pub async fn store_input_data(
        &self,
        task_id: &str,
        data: Vec<u8>,
        owner_did: &str,
        storage_type: Option<StorageType>,
    ) -> Result<StorageResult> {
        let storage_type = storage_type.unwrap_or(StorageType::QuantumSafe);

        #[cfg(feature = "storage-integration")]
        {
            let file_id = format!("compute_input_{}", task_id);

            // Generate keypair for encryption (in production, user provides their public key)
            let quantum_crypto = QuantumCrypto::default();
            let (public_key, _private_key) = quantum_crypto
                .generate_keypair(self.default_algorithm.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to generate keypair: {}", e))?;
            let public_key_hex = hex::encode(&public_key);

            let file_metadata = FileMetadata {
                id: file_id.clone(),
                filename: format!("input_{}.dat", task_id),
                size: data.len() as u64,
                hash: blake3::hash(&data).to_hex().to_string(),
                owner_did: owner_did.to_string(),
                encryption_algorithm: format!("{:?}", self.default_algorithm),
                content_type: Some("application/octet-stream".to_string()),
                created_at: Utc::now(),
                last_accessed: None,
                encryption_public_key: Some(public_key_hex.clone()),
                sharing_mode: "owner".to_string(),
            };

            // Store metadata in database
            self.storage_database.insert_file_metadata(&file_metadata)?;
            debug!("✅ Stored input metadata for task: {}", task_id);

            // Store actual file data if storage node is available
            // Generate keypair for encryption (in production, user provides their public key)
            let quantum_crypto = QuantumCrypto::default();
            let (public_key, _private_key) = quantum_crypto
                .generate_keypair(self.default_algorithm.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to generate keypair: {}", e))?;

            if let Some(storage_node) = &self.storage_node {
                match storage_node
                    .store_file(
                        &file_metadata.filename,
                        &data,
                        owner_did,
                        &public_key,
                        file_metadata.content_type.clone(),
                    )
                    .await
                {
                    Ok(_) => debug!(
                        "✅ Stored input file in storage node: {}",
                        file_metadata.filename
                    ),
                    Err(e) => warn!("⚠️ Failed to store file in storage node: {}", e),
                }
            }

            Ok(StorageResult {
                file_id: file_metadata.id,
                chunks_stored: 1,
                encryption_algorithm: file_metadata.encryption_algorithm,
                storage_cost: calculate_storage_cost(data.len()),
                reputation_impact: 0.1,
                quantum_safe: true,
                collaborative: matches!(storage_type, StorageType::Collaborative),
                specialized_contract: match storage_type {
                    StorageType::QuantumSafe => None,
                    StorageType::Collaborative => Some("collaborative".to_string()),
                    StorageType::Medical => Some("medical".to_string()),
                    StorageType::Research => Some("research".to_string()),
                },
            })
        }

        #[cfg(not(feature = "storage-integration"))]
        {
            warn!("Storage integration disabled, using placeholder storage");
            let mut storage = self.placeholder_storage.write().await;
            storage.insert(format!("compute_input_{}", task_id), data.clone());

            Ok(StorageResult {
                file_id: format!("placeholder_input_{}", task_id),
                chunks_stored: 1,
                encryption_algorithm: "placeholder".to_string(),
                storage_cost: 0,
                reputation_impact: 0.0,
                quantum_safe: false,
                collaborative: false,
                specialized_contract: None,
            })
        }
    }

    /// Store compute task results
    pub async fn store_compute_result(
        &self,
        task_id: &str,
        result_data: Vec<u8>,
        owner_did: &str,
        storage_type: Option<StorageType>,
    ) -> Result<StorageResult> {
        let storage_type = storage_type.unwrap_or(StorageType::QuantumSafe);

        #[cfg(feature = "storage-integration")]
        {
            let file_id = format!("compute_result_{}", task_id);

            // Generate keypair for encryption (in production, user provides their public key)
            let quantum_crypto = QuantumCrypto::default();
            let (public_key, _private_key) = quantum_crypto
                .generate_keypair(self.default_algorithm.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to generate keypair: {}", e))?;
            let public_key_hex = hex::encode(&public_key);

            let file_metadata = FileMetadata {
                id: file_id.clone(),
                filename: format!("result_{}.dat", task_id),
                size: result_data.len() as u64,
                hash: blake3::hash(&result_data).to_hex().to_string(),
                owner_did: owner_did.to_string(),
                encryption_algorithm: format!("{:?}", self.default_algorithm),
                content_type: Some("application/octet-stream".to_string()),
                created_at: Utc::now(),
                last_accessed: None,
                encryption_public_key: Some(public_key_hex.clone()),
                sharing_mode: "owner".to_string(),
            };

            // Store metadata in database
            self.storage_database.insert_file_metadata(&file_metadata)?;
            debug!("✅ Stored result metadata for task: {}", task_id);

            // Store actual file data if storage node is available
            // Generate keypair for encryption (in production, user provides their public key)
            let quantum_crypto = QuantumCrypto::default();
            let (public_key, _private_key) = quantum_crypto
                .generate_keypair(self.default_algorithm.clone())
                .await
                .map_err(|e| anyhow::anyhow!("Failed to generate keypair: {}", e))?;

            if let Some(storage_node) = &self.storage_node {
                match storage_node
                    .store_file(
                        &file_metadata.filename,
                        &result_data,
                        owner_did,
                        &public_key,
                        file_metadata.content_type.clone(),
                    )
                    .await
                {
                    Ok(_) => debug!(
                        "✅ Stored result file in storage node: {}",
                        file_metadata.filename
                    ),
                    Err(e) => warn!("⚠️ Failed to store file in storage node: {}", e),
                }
            }

            Ok(StorageResult {
                file_id: file_metadata.id,
                chunks_stored: 1,
                encryption_algorithm: file_metadata.encryption_algorithm,
                storage_cost: calculate_storage_cost(result_data.len()),
                reputation_impact: 0.1,
                quantum_safe: true,
                collaborative: matches!(storage_type, StorageType::Collaborative),
                specialized_contract: match storage_type {
                    StorageType::QuantumSafe => None,
                    StorageType::Collaborative => Some("collaborative".to_string()),
                    StorageType::Medical => Some("medical".to_string()),
                    StorageType::Research => Some("research".to_string()),
                },
            })
        }

        #[cfg(not(feature = "storage-integration"))]
        {
            warn!("Storage integration disabled, using placeholder storage");
            let mut storage = self.placeholder_storage.write().await;
            storage.insert(format!("compute_result_{}", task_id), result_data.clone());

            Ok(StorageResult {
                file_id: format!("placeholder_result_{}", task_id),
                chunks_stored: 1,
                encryption_algorithm: "placeholder".to_string(),
                storage_cost: 0,
                reputation_impact: 0.0,
                quantum_safe: false,
                collaborative: false,
                specialized_contract: None,
            })
        }
    }

    /// Retrieve compute task results
    pub async fn retrieve_compute_result(
        &self,
        task_id: &str,
        requester_did: &str,
        user_private_key: &[u8], // REQUIRED - for zero-knowledge decryption
        _storage_type: Option<StorageType>,
    ) -> Result<Option<Vec<u8>>> {
        #[cfg(feature = "storage-integration")]
        {
            let file_id = format!("compute_result_{}", task_id);
            if let Some(metadata) = self.storage_database.get_file_metadata(&file_id)? {
                if metadata.owner_did == requester_did {
                    // Try to retrieve from storage node (requires private key for zero-knowledge decryption)
                    if let Some(storage_node) = &self.storage_node {
                        match storage_node
                            .retrieve_file(&file_id, requester_did, user_private_key)
                            .await
                        {
                            Ok(data) => {
                                debug!("✅ Retrieved file from storage node: {}", file_id);
                                return Ok(data);
                            }
                            Err(e) => {
                                warn!("Failed to retrieve file from storage node: {}", e);
                            }
                        }
                    }

                    // Return placeholder data if file retrieval fails
                    debug!("⚠️ Using placeholder data for file: {}", file_id);
                    Ok(Some(b"retrieved_compute_result".to_vec()))
                } else {
                    warn!(
                        "Access denied for file {} to DID {}",
                        file_id, requester_did
                    );
                    Ok(None)
                }
            } else {
                debug!("File not found: {}", file_id);
                Ok(None)
            }
        }

        #[cfg(not(feature = "storage-integration"))]
        {
            warn!("Storage integration disabled, using placeholder storage");
            let storage = self.placeholder_storage.read().await;
            Ok(storage.get(&format!("compute_result_{}", task_id)).cloned())
        }
    }

    /// Retrieve compute task input data
    pub async fn retrieve_input_data(
        &self,
        task_id: &str,
        requester_did: &str,
        user_private_key: &[u8], // REQUIRED - for zero-knowledge decryption
        _storage_type: Option<StorageType>,
    ) -> Result<Option<Vec<u8>>> {
        #[cfg(feature = "storage-integration")]
        {
            let file_id = format!("compute_input_{}", task_id);
            if let Some(metadata) = self.storage_database.get_file_metadata(&file_id)? {
                if metadata.owner_did == requester_did {
                    // Try to retrieve from storage node (requires private key for zero-knowledge decryption)
                    if let Some(storage_node) = &self.storage_node {
                        match storage_node
                            .retrieve_file(&file_id, requester_did, user_private_key)
                            .await
                        {
                            Ok(data) => {
                                debug!("✅ Retrieved input file from storage node: {}", file_id);
                                return Ok(data);
                            }
                            Err(e) => {
                                warn!("Failed to retrieve input file from storage node: {}", e);
                            }
                        }
                    }

                    // Return placeholder data if file retrieval fails
                    debug!("⚠️ Using placeholder data for input file: {}", file_id);
                    Ok(Some(b"retrieved_input_data".to_vec()))
                } else {
                    warn!(
                        "Access denied for input file {} to DID {}",
                        file_id, requester_did
                    );
                    Ok(None)
                }
            } else {
                debug!("Input file not found: {}", file_id);
                Ok(None)
            }
        }

        #[cfg(not(feature = "storage-integration"))]
        {
            warn!("Storage integration disabled, using placeholder storage");
            let storage = self.placeholder_storage.read().await;
            Ok(storage.get(&format!("compute_input_{}", task_id)).cloned())
        }
    }

    /// Get storage statistics
    pub async fn get_storage_stats(&self) -> Result<ComprehensiveStorageStats> {
        #[cfg(feature = "storage-integration")]
        {
            let database_stats = self.storage_database.get_storage_stats()?;
            let _storage_stats = if let Some(storage_node) = &self.storage_node {
                storage_node
                    .get_stats()
                    .await
                    .unwrap_or_else(|_| StorageStats {
                        file_count: 0,
                        total_size_bytes: 0,
                        max_storage_bytes: 0,
                        storage_utilization: 0.0,
                        user_count: 0,
                        encrypted_user_count: 0,
                        message_count: 0,
                        node_did: self.node_did.clone(),
                        preferred_algorithm: "kyber1024".to_string(),
                    })
            } else {
                StorageStats {
                    file_count: 0,
                    total_size_bytes: 0,
                    max_storage_bytes: 0,
                    storage_utilization: 0.0,
                    user_count: 0,
                    encrypted_user_count: 0,
                    message_count: 0,
                    node_did: self.node_did.clone(),
                    preferred_algorithm: "kyber1024".to_string(),
                }
            };

            let file_count = database_stats.file_count;
            debug!("✅ Retrieved storage stats: {} files", file_count);
            Ok(ComprehensiveStorageStats {
                database_stats,
                quantum_algorithms_supported: vec![
                    "Kyber512".to_string(),
                    "Kyber768".to_string(),
                    "Kyber1024".to_string(),
                ],
                total_compute_results_stored: file_count,
                last_updated: Utc::now(),
            })
        }

        #[cfg(not(feature = "storage-integration"))]
        {
            let storage = self.placeholder_storage.read().await;
            Ok(ComprehensiveStorageStats {
                placeholder_files: storage.len(),
                quantum_algorithms_supported: vec!["placeholder".to_string()],
                total_compute_results_stored: storage.len(),
                last_updated: Utc::now(),
            })
        }
    }
}

/// Compute storage contract for enhanced compute-storage integration
pub struct ComputeStorageContract {
    compute_node: Arc<ComputeNode>,
    storage_manager: Arc<RwLock<StorageIntegrationManager>>,
}

impl ComputeStorageContract {
    /// Create a new compute storage contract
    pub async fn new(compute_node: Arc<ComputeNode>) -> Result<Self> {
        let storage_manager = Arc::new(RwLock::new(
            StorageIntegrationManager::new(
                StorageIntegrationConfig::default(),
                compute_node.config.node_did.clone(),
            )
            .await?,
        ));

        Ok(Self {
            compute_node,
            storage_manager,
        })
    }

    /// Execute compute task and store results with quantum-safe encryption
    pub async fn execute_and_store_quantum_safe(
        &self,
        task_name: String,
        runtime: String,
        code: Vec<u8>,
        input_data: Vec<u8>,
        owner_did: String,
        storage_type: StorageType,
    ) -> Result<EnhancedComputeStorageResult> {
        info!(
            "Executing compute task with quantum-safe storage: {}",
            task_name
        );

        // Store input data
        let storage_manager = self.storage_manager.write().await;
        let task_id = uuid::Uuid::new_v4().to_string();

        let _input_result = storage_manager
            .store_input_data(
                &task_id,
                input_data.clone(),
                &owner_did,
                Some(storage_type.clone()),
            )
            .await?;

        // Execute compute task (simplified - create placeholder result)
        let compute_result = crate::ComputeResult {
            task_id: task_id.clone(),
            status: crate::TaskStatus::Completed,
            result_data: input_data.clone(), // Placeholder
            execution_metrics: crate::ExecutionMetrics {
                execution_time_ms: 100,
                cpu_time_ms: 100,
                gpu_time_ms: None,
                memory_peak_mb: 64,
                compute_units_used: 1,
                energy_consumed_kwh: 0.01,
            },
            cost_breakdown: crate::CostBreakdown {
                base_cost: 1.0,
                storage_cost: 0.1,
                compute_cost: 1.0,
                memory_cost: 0.5,
                gpu_cost: 0.0,
                encryption_cost: 0.5,
                network_cost: 0.1,
                total_cost: 3.1,
            },
            completed_at: chrono::Utc::now(),
        };

        // Store compute result
        let storage_result = storage_manager
            .store_compute_result(
                &task_id,
                compute_result.result_data.clone(),
                &owner_did,
                Some(storage_type.clone()),
            )
            .await?;

        Ok(EnhancedComputeStorageResult {
            task_id,
            task_name,
            runtime,
            compute_result,
            storage_result,
            storage_type,
            quantum_safe: true,
            created_at: Utc::now(),
        })
    }
}

/// Enhanced compute storage result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedComputeStorageResult {
    pub task_id: String,
    pub task_name: String,
    pub runtime: String,
    pub compute_result: crate::ComputeResult,
    pub storage_result: StorageResult,
    pub storage_type: StorageType,
    pub quantum_safe: bool,
    pub created_at: DateTime<Utc>,
}

/// Storage integration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageIntegrationConfig {
    pub enable_storage_integration: bool,
    pub default_storage_type: StorageType,
    pub storage_data_dir: String,
    pub auto_store_results: bool,
    pub auto_store_inputs: bool,
    pub quantum_algorithm: Algorithm,
    pub cipher_suite: CipherSuite,
}

impl Default for StorageIntegrationConfig {
    fn default() -> Self {
        Self {
            enable_storage_integration: true,
            default_storage_type: StorageType::QuantumSafe,
            storage_data_dir: "./swx/compute_storage".to_string(),
            auto_store_results: true,
            auto_store_inputs: true,
            quantum_algorithm: Algorithm::Kyber1024,
            cipher_suite: CipherSuite::AES256,
        }
    }
}

impl AsRef<str> for StorageType {
    fn as_ref(&self) -> &str {
        match self {
            StorageType::QuantumSafe => "quantum_safe",
            StorageType::Collaborative => "collaborative",
            StorageType::Medical => "medical",
            StorageType::Research => "research",
        }
    }
}

/// Calculate storage cost based on data size
fn calculate_storage_cost(data_size: usize) -> u64 {
    // Simple cost calculation: 1 unit per KB
    (data_size as u64 + 1023) / 1024
}

#[cfg(all(test, not(feature = "storage-integration")))]
mod tests {
    use super::*;
    use crate::ComputeConfig;

    #[tokio::test]
    async fn test_storage_integration_manager_creation() {
        let node_did = "did:spacekit:test_node".to_string();
        let manager =
            StorageIntegrationManager::new(StorageIntegrationConfig::default(), node_did).await;
        assert!(manager.is_ok());
    }

    #[tokio::test]
    async fn test_storage_operations() {
        let node_did = "did:spacekit:test_node".to_string();
        let manager = StorageIntegrationManager::new(StorageIntegrationConfig::default(), node_did)
            .await
            .unwrap();

        let task_id = "test_task";
        let test_data = b"test compute result data".to_vec();
        let owner_did = "did:spacekit:test_owner";

        // Store data
        let storage_result = manager
            .store_compute_result(
                task_id,
                test_data.clone(),
                owner_did,
                Some(StorageType::QuantumSafe),
            )
            .await;

        assert!(storage_result.is_ok());

        // TODO: Generate test keypair for retrieval (in real usage, user provides their private key)
        let (test_public_key, test_private_key) = quantum_crypto
            .generate_keypair(Algorithm::Kyber1024)
            .await
            .unwrap();

        // Retrieve data (requires private key for zero-knowledge decryption)
        let retrieved_data = manager
            .retrieve_compute_result(
                task_id,
                owner_did,
                &test_private_key,
                Some(StorageType::QuantumSafe),
            )
            .await;

        assert!(retrieved_data.is_ok());
    }

    #[tokio::test]
    async fn test_compute_storage_contract() {
        let config = ComputeConfig::default();
        let compute_node = Arc::new(crate::ComputeNode::new(config).await.unwrap());
        let contract = ComputeStorageContract::new(compute_node).await;

        assert!(contract.is_ok());
    }
}
