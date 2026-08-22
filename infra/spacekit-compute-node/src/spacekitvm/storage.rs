//! WCVM Storage Contract Framework
//!
//! Provides quantum-safe, DID-native storage contracts for the WCVM ecosystem.
//! This module implements the storage layer described in WCVM_STORAGE.md.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// Re-export WCVM types
use super::{
    SwtchvmAddress, SwtchvmContext, SwtchvmExecutionResult, SwtchvmLog, SwtchvmState, SwtchvmValue,
};

// Re-export quantum security types
use crate::quantum_security::{Algorithm, QuantumResistantDID, QuantumResistantEncryption};

// Re-export storage node types (when available)
#[cfg(feature = "storage-integration")]
use spacekit_storage_node::{QuantumCrypto, StorageNode, StorageNodeConfig};

/// Storage Smart Contract Base Trait
///
/// All storage contracts must implement this trait to be compatible with WCVM.
/// This provides a unified interface for quantum-safe storage operations.
#[async_trait]
pub trait StorageSmartContract: Send + Sync {
    /// Initialize the storage contract
    async fn initialize(&mut self, config: StorageContractConfig) -> Result<()>;

    /// Store a file with quantum-safe encryption
    async fn store_file(
        &mut self,
        owner_did: &str,
        file_data: Vec<u8>,
        encryption_algorithm: Algorithm,
    ) -> Result<StorageResult>;

    /// Retrieve a file with access control
    async fn retrieve_file(&self, file_id: &str, requester_did: &str) -> Result<Option<Vec<u8>>>;

    /// Grant access to a file
    async fn grant_access(
        &mut self,
        file_id: &str,
        granter_did: &str,
        grantee_did: &str,
        permissions: FilePermissions,
    ) -> Result<bool>;

    /// Revoke access to a file
    async fn revoke_access(
        &mut self,
        file_id: &str,
        revoker_did: &str,
        target_did: &str,
    ) -> Result<bool>;

    /// Get storage statistics
    async fn get_stats(&self) -> StorageStats;
}

/// Storage Contract Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageContractConfig {
    pub max_storage_bytes: u64,
    pub preferred_algorithm: Algorithm,
    pub replication_factor: usize,
    pub enable_p2p: bool,
    pub enable_reputation: bool,
}

impl Default for StorageContractConfig {
    fn default() -> Self {
        Self {
            max_storage_bytes: 100 * 1024 * 1024 * 1024, // 100GB
            preferred_algorithm: Algorithm::SphincsPlus256128,
            replication_factor: 3,
            enable_p2p: true,
            enable_reputation: true,
        }
    }
}

/// File Permissions for Access Control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilePermissions {
    Read,
    Write,
    ReadWrite,
    Admin,
}

/// Storage Operation Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageResult {
    pub file_id: String,
    pub chunks_stored: usize,
    pub encryption_algorithm: String,
    pub storage_cost: u64,
    pub reputation_impact: f64,
}

/// Storage Statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub file_count: usize,
    pub total_size_bytes: u64,
    pub storage_utilization: f64,
    pub active_users: usize,
    pub reputation_scores: HashMap<String, f64>,
}

/// File Metadata for Storage Contracts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwtchvmFileMetadata {
    pub file_id: String,
    pub owner_did: String,
    pub size: usize,
    pub encryption_algorithm: String,
    pub created_at: u64,
    pub access_control: Vec<AccessControlEntry>,
    pub chunks: Vec<String>,
}

/// Access Control Entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessControlEntry {
    pub did: String,
    pub permissions: FilePermissions,
    pub granted_at: u64,
    pub expires_at: Option<u64>,
}

/// Reputation Score for Storage Providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationScore {
    pub did: String,
    pub score: f64,
    pub storage_contribution: u64,
    pub service_quality: f64,
    pub last_updated: u64,
}

impl ReputationScore {
    pub fn new(did: String) -> Self {
        Self {
            did,
            score: 0.0,
            storage_contribution: 0,
            service_quality: 0.0,
            last_updated: 0,
        }
    }

    pub fn update_with_service_quality(&mut self, quality: f64) {
        self.service_quality = quality;
        self.score = (self.score + quality) / 2.0;
        self.last_updated = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }
}

/// Quantum-Safe Storage Contract
///
/// Implements quantum-resistant file storage with multiple algorithm support.
/// This is the first quantum-safe storage contract implementation.
#[derive(Debug)]
pub struct QuantumSafeStorage {
    config: StorageContractConfig,
    files: HashMap<String, SwtchvmFileMetadata>,
    quantum_crypto: QuantumResistantEncryption,
    reputation_scores: HashMap<String, ReputationScore>,
    storage_used: u64,
}

impl QuantumSafeStorage {
    pub async fn new() -> Self {
        Self {
            config: StorageContractConfig::default(),
            files: HashMap::new(),
            quantum_crypto: QuantumResistantEncryption::new("SphincsPlus256128", &[])
                .await
                .unwrap(),
            reputation_scores: HashMap::new(),
            storage_used: 0,
        }
    }

    /// Store file with quantum encryption
    pub async fn store_quantum_safe_file(
        &mut self,
        owner_did: &str,
        file_data: Vec<u8>,
        encryption_algorithm: Algorithm,
    ) -> Result<StorageResult> {
        info!(
            "Storing file with quantum encryption: {} bytes",
            file_data.len()
        );

        // Check storage capacity
        if self.storage_used + file_data.len() as u64 > self.config.max_storage_bytes {
            return Err(anyhow::anyhow!("Storage capacity exceeded"));
        }

        // Generate file ID
        let file_id = self.generate_file_id(owner_did, &file_data);

        // Encrypt file data with quantum algorithm
        let encrypted_data = self
            .quantum_crypto
            .encrypt(&file_data, &self.get_identity_for_did(owner_did).await?)
            .await?;

        // Create file metadata
        let metadata = SwtchvmFileMetadata {
            file_id: file_id.clone(),
            owner_did: owner_did.to_string(),
            size: file_data.len(),
            encryption_algorithm: format!("{:?}", encryption_algorithm),
            created_at: self.get_current_timestamp(),
            access_control: vec![AccessControlEntry {
                did: owner_did.to_string(),
                permissions: FilePermissions::Admin,
                granted_at: self.get_current_timestamp(),
                expires_at: None,
            }],
            chunks: self.chunk_file(&encrypted_data),
        };

        // Store file
        self.files.insert(file_id.clone(), metadata);
        self.storage_used += file_data.len() as u64;

        // Update reputation
        self.update_reputation(owner_did, 0.1)?;

        Ok(StorageResult {
            file_id,
            chunks_stored: self.config.replication_factor,
            encryption_algorithm: format!("{:?}", encryption_algorithm),
            storage_cost: self.calculate_storage_cost(file_data.len()),
            reputation_impact: 0.1,
        })
    }

    /// Retrieve file with quantum decryption
    pub async fn retrieve_quantum_safe_file(
        &self,
        file_id: &str,
        requester_did: &str,
    ) -> Result<Option<Vec<u8>>> {
        debug!("Retrieving file: {} for DID: {}", file_id, requester_did);

        let metadata = self
            .files
            .get(file_id)
            .ok_or_else(|| anyhow::anyhow!("File not found"))?;

        // Check access permissions
        if !self.has_access(metadata, requester_did) {
            return Err(anyhow::anyhow!("Access denied"));
        }

        // Retrieve and decrypt file
        let encrypted_data = self.retrieve_file_chunks(&metadata.chunks)?;
        let decrypted_data = self
            .quantum_crypto
            .decrypt(
                &encrypted_data,
                &self.get_identity_for_did(requester_did).await?,
            )
            .await?;

        Ok(Some(decrypted_data))
    }

    // Private helper methods
    fn generate_file_id(&self, owner_did: &str, data: &[u8]) -> String {
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        hasher.update(owner_did.as_bytes());
        hasher.update(data);
        hasher.update(self.get_current_timestamp().to_be_bytes());
        format!("file_{}", hex::encode(hasher.finalize().as_slice()))
    }

    async fn get_identity_for_did(&self, did: &str) -> Result<QuantumResistantDID> {
        // In production, this would resolve the DID to get the actual identity
        // For now, create a placeholder identity
        crate::quantum_security::quantum_did_utils::from_did(did).await
    }

    fn get_current_timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn chunk_file(&self, data: &[u8]) -> Vec<String> {
        let chunk_size = 1024 * 1024; // 1MB chunks
        let mut chunks = Vec::new();

        for (i, chunk) in data.chunks(chunk_size).enumerate() {
            let chunk_id = format!(
                "chunk_{}_{}",
                i,
                hex::encode(sha3::Sha3_256::digest(chunk).as_slice())
            );
            chunks.push(chunk_id);
        }

        chunks
    }

    fn retrieve_file_chunks(&self, chunk_ids: &[String]) -> Result<Vec<u8>> {
        // In production, this would retrieve chunks from P2P network
        // For now, return placeholder data
        Ok(vec![0u8; 1024])
    }

    fn has_access(&self, metadata: &SwtchvmFileMetadata, requester_did: &str) -> bool {
        metadata.access_control.iter().any(|entry| {
            entry.did == requester_did
                && (entry.expires_at.is_none()
                    || entry.expires_at.unwrap() > self.get_current_timestamp())
        })
    }

    fn calculate_storage_cost(&self, size: usize) -> u64 {
        // Base cost: 1 credit per MB
        let base_cost = (size as u64) / (1024 * 1024);

        // Apply reputation discount
        let reputation = self
            .reputation_scores
            .get("default")
            .map(|r| r.score)
            .unwrap_or(0.0);
        let discount = reputation * 0.1; // Up to 10% discount

        ((base_cost as f64) * (1.0 - discount)) as u64
    }

    fn update_reputation(&mut self, did: &str, delta: f64) -> Result<()> {
        let reputation = self
            .reputation_scores
            .entry(did.to_string())
            .or_insert_with(|| ReputationScore::new(did.to_string()));

        reputation.update_with_service_quality(delta);
        Ok(())
    }
}

#[async_trait]
impl StorageSmartContract for QuantumSafeStorage {
    async fn initialize(&mut self, config: StorageContractConfig) -> Result<()> {
        self.config = config;
        info!(
            "QuantumSafeStorage initialized with config: {:?}",
            self.config
        );
        Ok(())
    }

    async fn store_file(
        &mut self,
        owner_did: &str,
        file_data: Vec<u8>,
        encryption_algorithm: Algorithm,
    ) -> Result<StorageResult> {
        self.store_quantum_safe_file(owner_did, file_data, encryption_algorithm)
            .await
    }

    async fn retrieve_file(&self, file_id: &str, requester_did: &str) -> Result<Option<Vec<u8>>> {
        self.retrieve_quantum_safe_file(file_id, requester_did)
            .await
    }

    async fn grant_access(
        &mut self,
        file_id: &str,
        granter_did: &str,
        grantee_did: &str,
        permissions: FilePermissions,
    ) -> Result<bool> {
        // Check access before getting mutable reference
        let has_access = self
            .files
            .get(file_id)
            .map(|metadata| self.has_access(metadata, granter_did))
            .unwrap_or(false);

        if !has_access {
            return Err(anyhow::anyhow!("Insufficient permissions"));
        }

        let now = self.get_current_timestamp();
        let metadata = self
            .files
            .get_mut(file_id)
            .ok_or_else(|| anyhow::anyhow!("File not found"))?;

        metadata.access_control.push(AccessControlEntry {
            did: grantee_did.to_string(),
            permissions,
            granted_at: now,
            expires_at: None,
        });

        Ok(true)
    }

    async fn revoke_access(
        &mut self,
        file_id: &str,
        revoker_did: &str,
        target_did: &str,
    ) -> Result<bool> {
        // Check access before getting mutable reference
        let has_access = self
            .files
            .get(file_id)
            .map(|metadata| self.has_access(metadata, revoker_did))
            .unwrap_or(false);

        if !has_access {
            return Err(anyhow::anyhow!("Insufficient permissions"));
        }

        let metadata = self
            .files
            .get_mut(file_id)
            .ok_or_else(|| anyhow::anyhow!("File not found"))?;

        // Remove access control entry
        metadata
            .access_control
            .retain(|entry| entry.did != target_did);

        Ok(true)
    }

    async fn get_stats(&self) -> StorageStats {
        StorageStats {
            file_count: self.files.len(),
            total_size_bytes: self.storage_used,
            storage_utilization: (self.storage_used as f64 / self.config.max_storage_bytes as f64)
                * 100.0,
            active_users: self.reputation_scores.len(),
            reputation_scores: self
                .reputation_scores
                .iter()
                .map(|(did, score)| (did.clone(), score.score))
                .collect(),
        }
    }
}

/// Distributed Storage Contract
///
/// Implements P2P distributed storage with DID-based access control and reputation.
/// This contract integrates with the SWTCH Storage Node for actual file distribution.
pub struct DistributedStorage {
    config: StorageContractConfig,
    files: HashMap<String, SwtchvmFileMetadata>,
    did_permissions: HashMap<String, StoragePermissions>,
    reputation_scores: HashMap<String, ReputationScore>,
    #[cfg(feature = "storage-integration")]
    storage_node: Option<Arc<StorageNode>>,
}

/// Storage Permissions for DIDs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePermissions {
    pub can_store: bool,
    pub can_retrieve: bool,
    pub can_share: bool,
    pub max_storage_bytes: u64,
    pub reputation_required: f64,
}

impl Default for StoragePermissions {
    fn default() -> Self {
        Self {
            can_store: true,
            can_retrieve: true,
            can_share: false,
            max_storage_bytes: 1024 * 1024 * 1024, // 1GB
            reputation_required: 0.0,
        }
    }
}

impl DistributedStorage {
    pub fn new() -> Self {
        Self {
            config: StorageContractConfig::default(),
            files: HashMap::new(),
            did_permissions: HashMap::new(),
            reputation_scores: HashMap::new(),
            #[cfg(feature = "storage-integration")]
            storage_node: None,
        }
    }

    /// Store file with P2P distribution
    pub fn store_quantum_safe_file(
        &mut self,
        owner_did: &str,
        file_data: Vec<u8>,
        encryption_algorithm: Algorithm,
    ) -> Result<StorageResult> {
        info!(
            "Storing file with P2P distribution: {} bytes",
            file_data.len()
        );

        // Check permissions
        let default_permissions = StoragePermissions::default();
        let permissions = self
            .did_permissions
            .get(owner_did)
            .unwrap_or(&default_permissions);

        if !permissions.can_store {
            return Err(anyhow::anyhow!("Storage permission denied"));
        }

        // Check storage limits
        if file_data.len() as u64 > permissions.max_storage_bytes {
            return Err(anyhow::anyhow!("Storage limit exceeded"));
        }

        // Check reputation requirements
        let reputation = self
            .reputation_scores
            .get(owner_did)
            .map(|r| r.score)
            .unwrap_or(0.0);

        if reputation < permissions.reputation_required {
            return Err(anyhow::anyhow!("Insufficient reputation"));
        }

        // Generate file ID
        let file_id = self.generate_file_id(owner_did, &file_data);

        // Encrypt and distribute file
        let encrypted_chunks =
            self.quantum_encrypt_and_chunk(file_data.clone(), encryption_algorithm.clone())?;
        let chunks_stored = self.distribute_chunks_p2p(&encrypted_chunks)?;

        // Create file metadata
        let metadata = SwtchvmFileMetadata {
            file_id: file_id.clone(),
            owner_did: owner_did.to_string(),
            size: file_data.len(),
            encryption_algorithm: format!("{:?}", encryption_algorithm),
            created_at: self.get_current_timestamp(),
            access_control: vec![AccessControlEntry {
                did: owner_did.to_string(),
                permissions: FilePermissions::Admin,
                granted_at: self.get_current_timestamp(),
                expires_at: None,
            }],
            chunks: encrypted_chunks
                .iter()
                .map(|c| c.chunk_id.clone())
                .collect(),
        };

        // Store metadata
        self.files.insert(file_id.clone(), metadata);

        // Update reputation
        self.update_storage_provider_reputation(owner_did, 0.1)?;

        Ok(StorageResult {
            file_id,
            chunks_stored,
            encryption_algorithm: format!("{:?}", encryption_algorithm),
            storage_cost: self.calculate_reputation_based_cost(owner_did, file_data.len()),
            reputation_impact: 0.1,
        })
    }

    /// Retrieve file with P2P gathering
    pub fn retrieve_quantum_safe_file(
        &self,
        file_id: &str,
        requester_did: &str,
    ) -> Result<Option<Vec<u8>>> {
        debug!("Retrieving file with P2P gathering: {}", file_id);

        let metadata = self
            .files
            .get(file_id)
            .ok_or_else(|| anyhow::anyhow!("File not found"))?;

        // Check access permissions
        if !self.has_access(metadata, requester_did) {
            return Err(anyhow::anyhow!("Access denied"));
        }

        // Gather chunks from P2P network
        let encrypted_chunks = self.gather_chunks_from_p2p(&metadata.chunks)?;

        // Decrypt and reassemble file
        let decrypted_data = self.quantum_decrypt_and_reassemble(encrypted_chunks)?;

        Ok(Some(decrypted_data))
    }

    // Private helper methods
    fn generate_file_id(&self, owner_did: &str, data: &[u8]) -> String {
        use sha3::{Digest, Sha3_256};
        let mut hasher = Sha3_256::new();
        hasher.update(owner_did.as_bytes());
        hasher.update(data);
        hasher.update(self.get_current_timestamp().to_be_bytes());
        format!("distributed_file_{}", hex::encode(hasher.finalize()))
    }

    fn quantum_encrypt_and_chunk(
        &self,
        data: Vec<u8>,
        algorithm: Algorithm,
    ) -> Result<Vec<EncryptedChunk>> {
        // In production, this would use the SWTCH Storage Node's quantum crypto
        let chunk_size = 1024 * 1024; // 1MB chunks
        let mut chunks = Vec::new();

        for (i, chunk) in data.chunks(chunk_size).enumerate() {
            let chunk_id = format!(
                "chunk_{}_{}",
                i,
                hex::encode(sha3::Sha3_256::digest(chunk).as_slice())
            );
            chunks.push(EncryptedChunk {
                chunk_id,
                data: chunk.to_vec(), // In production, this would be encrypted
                algorithm: format!("{:?}", algorithm),
            });
        }

        Ok(chunks)
    }

    fn distribute_chunks_p2p(&self, chunks: &[EncryptedChunk]) -> Result<usize> {
        // In production, this would use the SWTCH Storage Node's P2P network
        info!("Distributing {} chunks via P2P network", chunks.len());
        Ok(chunks.len() * self.config.replication_factor)
    }

    fn gather_chunks_from_p2p(&self, chunk_ids: &[String]) -> Result<Vec<EncryptedChunk>> {
        // In production, this would gather chunks from P2P network
        info!("Gathering {} chunks from P2P network", chunk_ids.len());

        let mut chunks = Vec::new();
        for chunk_id in chunk_ids {
            chunks.push(EncryptedChunk {
                chunk_id: chunk_id.clone(),
                data: vec![0u8; 1024], // Placeholder data
                algorithm: "SphincsPlus256128".to_string(),
            });
        }

        Ok(chunks)
    }

    fn quantum_decrypt_and_reassemble(&self, chunks: Vec<EncryptedChunk>) -> Result<Vec<u8>> {
        // In production, this would decrypt and reassemble the file
        let mut reassembled = Vec::new();
        for chunk in chunks {
            reassembled.extend_from_slice(&chunk.data);
        }
        Ok(reassembled)
    }

    fn has_access(&self, metadata: &SwtchvmFileMetadata, requester_did: &str) -> bool {
        metadata.access_control.iter().any(|entry| {
            entry.did == requester_did
                && (entry.expires_at.is_none()
                    || entry.expires_at.unwrap() > self.get_current_timestamp())
        })
    }

    fn calculate_reputation_based_cost(&self, did: &str, size: usize) -> u64 {
        let base_cost = (size as u64) / (1024 * 1024); // 1 credit per MB

        let reputation = self
            .reputation_scores
            .get(did)
            .map(|r| r.score)
            .unwrap_or(0.0);

        // High reputation = lower costs (up to 50% discount)
        let discount = reputation * 0.5;
        ((base_cost as f64) * (1.0 - discount)) as u64
    }

    fn update_storage_provider_reputation(
        &mut self,
        provider_did: &str,
        service_quality: f32,
    ) -> Result<()> {
        let reputation = self
            .reputation_scores
            .entry(provider_did.to_string())
            .or_insert_with(|| ReputationScore::new(provider_did.to_string()));

        reputation.update_with_service_quality(service_quality as f64);
        Ok(())
    }

    fn get_current_timestamp(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

#[async_trait]
impl StorageSmartContract for DistributedStorage {
    async fn initialize(&mut self, config: StorageContractConfig) -> Result<()> {
        self.config = config.clone();
        info!("DistributedStorage initialized with config: {:?}", config);
        Ok(())
    }

    async fn store_file(
        &mut self,
        owner_did: &str,
        file_data: Vec<u8>,
        encryption_algorithm: Algorithm,
    ) -> Result<StorageResult> {
        Ok(self.store_quantum_safe_file(owner_did, file_data, encryption_algorithm)?)
    }

    async fn retrieve_file(&self, file_id: &str, requester_did: &str) -> Result<Option<Vec<u8>>> {
        Ok(self.retrieve_quantum_safe_file(file_id, requester_did)?)
    }

    async fn grant_access(
        &mut self,
        file_id: &str,
        granter_did: &str,
        grantee_did: &str,
        permissions: FilePermissions,
    ) -> Result<bool> {
        // Check access before getting mutable reference
        let has_access = self
            .files
            .get(file_id)
            .map(|metadata| self.has_access(metadata, granter_did))
            .unwrap_or(false);

        if !has_access {
            return Err(anyhow::anyhow!("Insufficient permissions"));
        }

        let now = self.get_current_timestamp();
        let metadata = self
            .files
            .get_mut(file_id)
            .ok_or_else(|| anyhow::anyhow!("File not found"))?;

        metadata.access_control.push(AccessControlEntry {
            did: grantee_did.to_string(),
            permissions,
            granted_at: now,
            expires_at: None,
        });

        Ok(true)
    }

    async fn revoke_access(
        &mut self,
        file_id: &str,
        revoker_did: &str,
        target_did: &str,
    ) -> Result<bool> {
        // Check access before getting mutable reference
        let has_access = self
            .files
            .get(file_id)
            .map(|metadata| self.has_access(metadata, revoker_did))
            .unwrap_or(false);

        if !has_access {
            return Err(anyhow::anyhow!("Insufficient permissions"));
        }

        let metadata = self
            .files
            .get_mut(file_id)
            .ok_or_else(|| anyhow::anyhow!("File not found"))?;

        // Remove access control entry
        metadata
            .access_control
            .retain(|entry| entry.did != target_did);

        Ok(true)
    }

    async fn get_stats(&self) -> StorageStats {
        StorageStats {
            file_count: self.files.len(),
            total_size_bytes: self.files.values().map(|f| f.size as u64).sum(),
            storage_utilization: 0.0, // Would calculate from actual storage
            active_users: self.reputation_scores.len(),
            reputation_scores: self
                .reputation_scores
                .iter()
                .map(|(did, score)| (did.clone(), score.score))
                .collect(),
        }
    }
}

/// Encrypted Chunk for P2P Distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedChunk {
    pub chunk_id: String,
    pub data: Vec<u8>,
    pub algorithm: String,
}

/// Reputation Compute Marketplace Contract
///
/// Implements reputation-based storage economics with merit-based pricing.
/// This contract manages storage costs based on user reputation and service quality.
pub struct ReputationComputeMarketplace {
    config: StorageContractConfig,
    provider_reputations: HashMap<String, ProviderReputation>,
    user_reputations: HashMap<String, UserReputation>,
    storage_contracts: HashMap<String, Box<dyn StorageSmartContract + Send + Sync>>,
}

/// Provider Reputation for Storage Services
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderReputation {
    pub did: String,
    pub score: f64,
    pub storage_capacity: u64,
    pub service_quality: f64,
    pub uptime: f64,
    pub response_time: f64,
}

/// User Reputation for Storage Consumers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserReputation {
    pub did: String,
    pub score: f64,
    pub storage_usage: u64,
    pub payment_history: f64,
    pub access_patterns: f64,
}

impl ReputationComputeMarketplace {
    pub fn new() -> Self {
        Self {
            config: StorageContractConfig::default(),
            provider_reputations: HashMap::new(),
            user_reputations: HashMap::new(),
            storage_contracts: HashMap::new(),
        }
    }

    /// Request compute with reputation-based allocation
    pub fn request_compute(
        &mut self,
        user_did: &str,
        compute_request: ComputeRequest,
    ) -> Result<ComputeAllocation> {
        // Verify user identity
        self.verify_user_identity(user_did)?;

        // Check user's reputation score
        let default_reputation = UserReputation::new(user_did.to_string());
        let user_reputation = self
            .user_reputations
            .get(user_did)
            .unwrap_or(&default_reputation);

        // Allocate resources based on reputation
        let allocation = if user_reputation.score > 0.8 {
            // High reputation = premium allocation
            self.allocate_premium_compute(compute_request)
        } else if user_reputation.score > 0.5 {
            // Medium reputation = standard allocation
            self.allocate_standard_compute(compute_request)
        } else {
            // Low reputation = limited allocation
            self.allocate_limited_compute(compute_request)
        };

        Ok(allocation)
    }

    /// Calculate reputation-based pricing
    pub fn calculate_reputation_based_pricing(
        &self,
        requester_did: &str,
        file_size: u64,
    ) -> StorageCost {
        let reputation = self.get_did_reputation(requester_did);
        let base_cost = file_size * BASE_STORAGE_RATE;

        // High reputation = lower costs
        let discount = reputation.calculate_discount();
        StorageCost {
            total: (base_cost as f64 * (1.0 - discount)) as u64,
            base_cost,
            reputation_discount: discount,
            final_cost: (base_cost as f64 * (1.0 - discount)) as u64,
        }
    }

    // Private helper methods
    fn verify_user_identity(&self, did: &str) -> Result<()> {
        // In production, this would verify the DID
        Ok(())
    }

    fn get_did_reputation(&self, did: &str) -> ReputationScore {
        self.user_reputations
            .get(did)
            .map(|r| ReputationScore {
                did: did.to_string(),
                score: r.score,
                storage_contribution: r.storage_usage,
                service_quality: r.payment_history,
                last_updated: 0,
            })
            .unwrap_or_else(|| ReputationScore::new(did.to_string()))
    }

    fn allocate_premium_compute(&self, request: ComputeRequest) -> ComputeAllocation {
        ComputeAllocation {
            gpu_cores: request.gpu_cores * 2,
            memory_mb: request.memory_mb * 2,
            priority: "high".to_string(),
            cost_multiplier: 1.5,
        }
    }

    fn allocate_standard_compute(&self, request: ComputeRequest) -> ComputeAllocation {
        ComputeAllocation {
            gpu_cores: request.gpu_cores,
            memory_mb: request.memory_mb,
            priority: "normal".to_string(),
            cost_multiplier: 1.0,
        }
    }

    fn allocate_limited_compute(&self, request: ComputeRequest) -> ComputeAllocation {
        ComputeAllocation {
            gpu_cores: request.gpu_cores / 2,
            memory_mb: request.memory_mb / 2,
            priority: "low".to_string(),
            cost_multiplier: 0.8,
        }
    }
}

#[async_trait]
impl StorageSmartContract for ReputationComputeMarketplace {
    async fn initialize(&mut self, config: StorageContractConfig) -> Result<()> {
        self.config = config.clone();
        info!(
            "ReputationComputeMarketplace initialized with config: {:?}",
            config
        );
        Ok(())
    }

    async fn store_file(
        &mut self,
        owner_did: &str,
        file_data: Vec<u8>,
        encryption_algorithm: Algorithm,
    ) -> Result<StorageResult> {
        // Delegate to appropriate storage contract based on reputation
        let contract = self.get_best_storage_contract(owner_did)?;
        contract
            .store_file(owner_did, file_data, encryption_algorithm)
            .await
    }

    async fn retrieve_file(&self, file_id: &str, requester_did: &str) -> Result<Option<Vec<u8>>> {
        // Find the contract that has this file
        for contract in self.storage_contracts.values() {
            if let Ok(Some(data)) = contract.retrieve_file(file_id, requester_did).await {
                return Ok(Some(data));
            }
        }
        Ok(None)
    }

    async fn grant_access(
        &mut self,
        file_id: &str,
        granter_did: &str,
        grantee_did: &str,
        permissions: FilePermissions,
    ) -> Result<bool> {
        // Delegate to appropriate storage contract
        let contract = self.get_best_storage_contract(granter_did)?;
        contract
            .grant_access(file_id, granter_did, grantee_did, permissions)
            .await
    }

    async fn revoke_access(
        &mut self,
        file_id: &str,
        revoker_did: &str,
        target_did: &str,
    ) -> Result<bool> {
        // Delegate to appropriate storage contract
        let contract = self.get_best_storage_contract(revoker_did)?;
        contract
            .revoke_access(file_id, revoker_did, target_did)
            .await
    }

    async fn get_stats(&self) -> StorageStats {
        let mut total_file_count = 0;
        let mut total_size_bytes = 0;

        for contract in self.storage_contracts.values() {
            let stats = contract.get_stats().await;
            total_file_count += stats.file_count;
            total_size_bytes += stats.total_size_bytes;
        }

        StorageStats {
            file_count: total_file_count,
            total_size_bytes,
            storage_utilization: 0.0,
            active_users: self.user_reputations.len(),
            reputation_scores: self
                .user_reputations
                .iter()
                .map(|(did, r)| (did.clone(), r.score))
                .collect(),
        }
    }
}

impl ReputationComputeMarketplace {
    fn get_best_storage_contract(
        &mut self,
        did: &str,
    ) -> Result<&mut (dyn StorageSmartContract + Send + Sync)> {
        // In production, this would select the best contract based on reputation and availability
        self.storage_contracts
            .values_mut()
            .next()
            .map(|c| c.as_mut() as &mut (dyn StorageSmartContract + Send + Sync))
            .ok_or_else(|| anyhow::anyhow!("No storage contracts available"))
    }
}

// Supporting types for the marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeRequest {
    pub gpu_cores: u32,
    pub memory_mb: u32,
    pub duration_seconds: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeAllocation {
    pub gpu_cores: u32,
    pub memory_mb: u32,
    pub priority: String,
    pub cost_multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCost {
    pub total: u64,
    pub base_cost: u64,
    pub reputation_discount: f64,
    pub final_cost: u64,
}

const BASE_STORAGE_RATE: u64 = 1; // 1 credit per byte

impl UserReputation {
    pub fn new(did: String) -> Self {
        Self {
            did,
            score: 0.0,
            storage_usage: 0,
            payment_history: 0.0,
            access_patterns: 0.0,
        }
    }
}

impl ReputationScore {
    pub fn calculate_discount(&self) -> f64 {
        // Higher reputation = higher discount (up to 50%)
        (self.score * 0.5).min(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_quantum_safe_storage_creation() {
        let mut storage = QuantumSafeStorage::new().await;
        let config = StorageContractConfig::default();
        assert!(storage.initialize(config).await.is_ok());
    }

    #[tokio::test]
    async fn test_distributed_storage_creation() {
        let mut storage = DistributedStorage::new();
        let config = StorageContractConfig::default();
        assert!(StorageSmartContract::initialize(&mut storage, config)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_reputation_marketplace_creation() {
        let mut marketplace = ReputationComputeMarketplace::new();
        let config = StorageContractConfig::default();
        assert!(StorageSmartContract::initialize(&mut marketplace, config)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_storage_contract_trait() {
        let mut storage = QuantumSafeStorage::new().await;
        let config = StorageContractConfig::default();
        StorageSmartContract::initialize(&mut storage, config)
            .await
            .unwrap();

        let test_data = b"Hello, quantum world!".to_vec();
        let result = StorageSmartContract::store_file(
            &mut storage,
            "did:test:user",
            test_data,
            Algorithm::SphincsPlus256128,
        )
        .await;
        assert!(result.is_ok());
    }
}
