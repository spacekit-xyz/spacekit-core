//! # SpaceKit Storage Node
//!
//! Quantum-resistant distributed storage services with enhanced features
//! Migrated and enhanced from spacekit-storage-dep

#![recursion_limit = "512"]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{RwLock, Semaphore};
use tracing::{debug, error, info, warn};

// Enhanced modules (migrated from old implementation)
pub mod artifact_ref_index;
pub mod blob_store;
pub mod database;
pub mod document_blob_store;
pub mod meta_store;
pub mod models;
pub mod storage_migration;

// P2P networking module (new)
#[cfg(feature = "p2p")]
pub mod network;

// Quantum crypto integration
pub mod quantum;

// Envelope encryption: client-side encrypt, server-side opaque storage, streaming download
pub mod envelope;

/// Owner-posted DEK capsules for true E2E entitlement delivery.
pub mod delivery_capsule;

// AWS Secrets Manager integration for key storage
#[cfg(feature = "aws-secrets")]
pub mod aws_secrets;

// Fact Package storage module (new)
pub mod fact_storage;

// App Package storage module (builds on fact_storage)
pub mod app_storage;

// Reward system for storage node operators
pub mod rewards;

// SQL query interface for complex queries
pub mod sql_query;

// ACID transaction management
pub mod transaction;

// Unified storage facade (the seam every read/write goes through; backs the
// transaction-aware/sandbox/idempotency/change-feed code paths).
pub mod storage_facade;

// Per-DID idempotency cache + in-flight tracking + body fingerprinting.
pub mod idempotency;

// Branch-CoW sandbox manager (Phase 1).
pub mod sandbox;

// Blob/fact DID access policy enforcement (ENHANCEMENTS Stream A).
pub mod access_policy;

// Short-lived upload tokens for browser/blob clients (Stream A item 4).
pub mod upload_token;

// Prometheus text metrics from agentic health (Stream F).
pub mod operator_metrics;

// Federation blob manifests and cross-node CAS pull (Phase 3).
pub mod federation;

// Signed workspace export attestations (Phase 3).
pub mod handoff;

// Operator discovery manifest facts (Stream E preview).
pub mod operator_manifest;

// DID-signed workspace migration manifests (see DID-MIGRATION.md).
pub mod migration;

// Local content access grants (pay-per-view / channel subscriptions).
pub mod content_grants;

// DB-backed materialized content installs (post-view, for entitled CLI app runs).
pub mod content_installs;

// Content access evaluation for published FactPackages.
pub mod content_access;

// Entitlement-ledger client for on-chain content grants.
pub mod content_entitlement;

// Payment receipt verification and refund-on-grant-failure.
pub mod content_payment;

// Settlement inbox and pending purchase orchestration.
pub mod content_settlement;

// AppLicenseNFT opcode payloads for per-content license mint / verify.
pub mod content_license;

// astra-escrow hold/release/refund for content purchases.
pub mod content_escrow;

// Library-embedded licensed features (`spacekit:licensed_feature:v1`, GROWFORMER_SPEC §6).
pub mod licensed_feature;

// Repo commit apply path for sandbox transactions (Stream B).
pub mod repo_commit;

// Workspace fact schema and builders (Stream C).
pub mod workspace;

// Append-only change feed with disk ring buffer + SSE fanout (Phase 4).
pub mod change_feed;

// Operator memory diagnostics (`GET /api/agentic/memory`).
pub mod memory_diagnostic;

// In-process MCP server wrapping the facade as agent tools (Phase 5).
//
// The dispatcher is transport-agnostic; the `mcp` Cargo feature pulls in
// optional rmcp/schemars wiring for stdio + SSE transports. The dispatcher
// itself compiles unconditionally so unit tests for the tool catalog and
// idempotency-key derivation run on every build.
pub mod mcp;

// Query planner for optimization
pub mod query_planner;

// High Availability management
pub mod ha;

// Advanced indexing system
pub mod indexes;

// EXPLAIN/ANALYZE for query plans
pub mod explain;

// Horizontal sharding for distributed storage
pub mod sharding;

// Full-text search
pub mod fulltext_search;

// Database migrations for enterprise-grade schema management
pub mod migrations;

// Vector search for semantic similarity
pub mod vector_search;

// NFT storage templates and helpers
pub mod nft_storage;

// NFT collection management
pub mod nft_collection;

// SpaceKitVM integration module (conditional)
#[cfg(feature = "wcvm-integration")]
pub mod spacekitvm;

// Bounded-memory byte streaming for large files (videos, archives, etc.)
#[cfg(feature = "api-server")]
pub mod streaming;

pub mod fact_sidecar;
pub mod stream_mime;

// Cross-server P2P routing
pub mod server_message_routing;
pub mod server_routing;

// Conditional API server support
#[cfg(feature = "api-server")]
pub mod api;

#[cfg(feature = "api-server")]
pub use api::{ApiServer, ServerConfig, ServerKeypair};

#[cfg(feature = "p2p")]
pub use network::{P2PNetwork, StorageBehaviour};

// Re-export key types
pub use database::{
    ContactMessage, Database, DocumentRecord, EncryptedMessage, EncryptedUser, FileMetadata,
    PersistenceConfig, User,
};
pub use models::{ApiError, EncryptedUserResponse, UserResponse};
pub use quantum::{EncryptedData, EncryptionMetadata, QuantumCrypto};

// Re-export fact storage types
pub use fact_storage::{
    CompressionAlgorithm, ContentStorage, FactIndex, FactStorageConfig, FactStorageEngine,
    FileContentStorage, StorageLocation, StorageTier, StorageTierConfig,
};

// Re-export app storage types
pub use app_storage::{
    AppIndex, AppQuery, AppQueryResult, AppSortBy, AppSortOrder, AppStorageEngine, AppStorageStats,
    IndexedAppMetadata,
};

// Re-export reward system types
pub use rewards::{
    BonusMultipliers, RewardAnalytics, RewardCalculation, RewardRecord, StorageRewardCalculator,
    StorageRewardConfig,
};

// Re-export SQL query types
pub use sql_query::{
    AggregateFunction, AggregateQuery, AggregateResult, FactQuery, FactQueryResult, FileQuery,
    FileQueryResult, Filter, FilterOp, FilterValue, SortBy, StorageQueryBuilder, UserQuery,
    UserQueryResult,
};

// Re-export NFT storage types
pub use nft_storage::{
    create_nft_collection, create_simple_nft, NftAttribute, NftCollection as NftCollectionInfo,
    NftMetadata, NftQuery, NftSortCriteria, NftStorageManager, NftStorageResult, NftTransfer,
};

// Re-export NFT collection management types
pub use nft_collection::{
    CollectionAnalytics, CollectionCategory, CollectionProperties, CollectionQuery,
    CollectionSortCriteria, CollectionStats, CollectionUpdate, MintConfig, NftCollection,
    NftCollectionManager, RarityConfig, RarityScore, RoyaltyConfig, RoyaltySplit, SaleData,
    SocialLinks, TokenStandard,
};

// Re-export migration system types
pub use migrations::{
    create_default_migrations, Migration, MigrationManager, MigrationRecord, MigrationStatus,
    CURRENT_SCHEMA_VERSION,
};

// Re-export SpaceKitVM types when feature is enabled
#[cfg(feature = "wcvm-integration")]
pub use spacekitvm::{
    create_spacekitvm_storage_contract, SpacekitvmFilePermissions, SpacekitvmStorageConfig,
    SpacekitvmStorageContract, SpacekitvmStorageFactory, SpacekitvmStorageNode,
    SpacekitvmStorageResult, SpacekitvmStorageStats,
};

/// Storage node configuration (enhanced)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageNodeConfig {
    /// Maximum storage capacity in bytes
    pub max_storage_bytes: u64,
    /// Data directory for file storage
    pub data_dir: PathBuf,
    /// Database path
    pub database_path: Option<PathBuf>,
    /// Node DID for identity
    pub node_did: String,
    /// Quantum-resistant algorithm preference
    pub preferred_algorithm: String,
    /// Encryption key pair
    pub encryption_keypair: Option<(String, String)>, // (public, private)
    /// When false, libp2p is not started (HTTP API only).
    #[serde(default = "default_enable_p2p")]
    pub enable_p2p: bool,
    /// When true, facade commits use real `TransactionManager` apply/revert.
    #[serde(default = "storage_facade::default_enable_real_transactions")]
    pub enable_real_transactions: bool,
    /// Database / blob persistence tuning (redb externalization, caches).
    #[serde(default)]
    pub persistence: crate::database::PersistenceConfig,
    /// Network configuration
    pub network_config: NetworkConfig,
    /// API server configuration (optional)
    #[cfg(feature = "api-server")]
    pub api_config: Option<ServerConfig>,
}

/// Network configuration for P2P connectivity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_port: u16,
    pub bootstrap_peers: Vec<String>,
    pub max_connections: usize,
    pub replication_factor: usize,
    pub chunk_size: usize,
    /// Maximum concurrent storage operations (prevents connection overload)
    pub max_concurrent_operations: Option<usize>,
    /// Retain full chunk bytes in RAM and duplicate them in the Kademlia MemoryStore.
    /// Default `false`: announce chunk availability only (bytes live on disk).
    #[serde(default)]
    pub cache_p2p_chunks_in_memory: bool,
}

fn default_enable_p2p() -> bool {
    true
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_port: 4001,
            bootstrap_peers: Vec::new(),
            max_connections: 50,
            replication_factor: 3,
            chunk_size: 1024 * 1024,             // 1MB chunks
            max_concurrent_operations: Some(10), // Limit to 10 concurrent operations by default
            cache_p2p_chunks_in_memory: false,
        }
    }
}

/// File access control entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessControlEntry {
    pub did: String,
    pub permissions: FilePermissions,
    pub granted_at: chrono::DateTime<chrono::Utc>,
}

/// File permissions enumeration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilePermissions {
    Read,
    Write,
    ReadWrite,
    Admin,
}

/// Storage node with enhanced functionality
pub struct StorageNode {
    config: StorageNodeConfig,
    database: Arc<Database>,
    files: Arc<RwLock<HashMap<String, StoredFile>>>,
    quantum_crypto: Arc<QuantumCrypto>,
    #[cfg(feature = "p2p")]
    p2p_network: Option<Arc<P2PNetwork>>,
    // Server routing for cross-server P2P communication
    server_routing: Arc<server_routing::ServerRoutingManager>,
    // Request concurrency control - limits concurrent operations to prevent overload
    request_semaphore: Arc<Semaphore>,
    // Retry configuration
    max_retries: u32,
    initial_retry_delay_ms: u64,
}

/// Stored file information (enhanced with quantum-resistant features)
#[derive(Debug, Clone)]
pub struct StoredFile {
    pub id: String,
    pub metadata: FileMetadata,
    pub access_control: Vec<AccessControlEntry>,
    pub data_chunks: Vec<String>, // Chunk identifiers for distributed storage
    pub encryption_info: EncryptionInfo,
}

/// Encryption information for files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionInfo {
    pub algorithm: String,
    pub key_derivation: String,
    pub cipher_suite: String,
    pub quantum_resistant: bool,
}

/// Storage node errors
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("File not found: {0}")]
    FileNotFound(String),
    #[error("Storage capacity exceeded")]
    StorageCapacityExceeded,
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Encryption error: {0}")]
    Encryption(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl Default for StorageNodeConfig {
    fn default() -> Self {
        Self {
            max_storage_bytes: 100 * 1024 * 1024 * 1024, // 100GB default
            data_dir: PathBuf::from("./storage_data"),
            database_path: None, // Will use default path
            node_did: String::new(),
            preferred_algorithm: "kyber1024".to_string(),
            encryption_keypair: None,
            enable_p2p: true,
            enable_real_transactions: storage_facade::default_enable_real_transactions(),
            persistence: crate::database::PersistenceConfig::default(),
            network_config: NetworkConfig::default(),
            #[cfg(feature = "api-server")]
            api_config: Some(ServerConfig::default()),
        }
    }
}

impl StorageNode {
    /// Create a new storage node with enhanced features
    pub async fn new(config: StorageNodeConfig) -> Result<Self> {
        // Create data directory
        tokio::fs::create_dir_all(&config.data_dir).await?;

        // Initialize database (using migrated database functionality)
        // IMPORTANT:
        // In production, `spacekit-storage-node` is commonly run under a hardened systemd unit
        // (e.g. `ProtectSystem=strict`) where only the configured data directory is writable.
        // The legacy default DB path (`/var/lib/spacekit/spacekit_storage.json` on Linux) will then
        // fail with "Read-only file system".
        //
        // So when `database_path` is not explicitly provided, default the DB to live alongside
        // the node's `data_dir`.
        let db_path = config
            .database_path
            .clone()
            .unwrap_or_else(|| config.data_dir.join("spacekit_storage.json"));

        let database = Arc::new(Database::with_config(
            db_path.to_str().unwrap(),
            config.persistence.clone(),
        )?);
        database.initialize()?;

        info!("Storage node initialized with database at: {:?}", db_path);

        let files = Arc::new(RwLock::new(HashMap::new()));

        // Initialize quantum crypto
        let pa = config.preferred_algorithm.to_ascii_lowercase();
        let algorithm = match pa.as_str() {
            "kyber512" => spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber512,
            "kyber768" => spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber768,
            "kyber1024" => spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024,
            _ => spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024,
        };
        let quantum_crypto = Arc::new(QuantumCrypto::new(
            algorithm,
            spacekit_primitives::v1::crypto::quantum::CipherSuite::AES256,
        ));

        // Initialize P2P network when enabled (compile-time feature + runtime flag)
        #[cfg(feature = "p2p")]
        let p2p_network = if config.enable_p2p {
            info!(
                "Initializing libp2p on TCP {}",
                config.network_config.listen_port
            );
            Some(P2PNetwork::new(&config).await?)
        } else {
            info!("P2P disabled (HTTP API only)");
            None
        };

        // Initialize request semaphore to limit concurrent operations
        // This prevents connection overload during burst requests (e.g., API warmup)
        let max_concurrent = config
            .network_config
            .max_concurrent_operations
            .unwrap_or(10);
        let request_semaphore = Arc::new(Semaphore::new(max_concurrent));

        info!(
            "Storage node initialized with max {} concurrent operations",
            max_concurrent
        );

        // Initialize server routing manager
        let server_routing = Arc::new(server_routing::ServerRoutingManager::new(Some(
            config.node_did.clone(),
        )));

        // Set up simple bridge adapter (can be replaced with full cross-network bridge)
        let bridge_adapter = Arc::new(server_routing::SimpleBridgeAdapter::new());
        // Note: In production, this would be set via set_cross_network_bridge()
        // with a real CrossNetworkBridge from spacekit-simulator

        Ok(Self {
            config,
            database,
            files,
            quantum_crypto,
            #[cfg(feature = "p2p")]
            p2p_network,
            server_routing,
            request_semaphore,
            max_retries: 3,              // Retry failed operations up to 3 times
            initial_retry_delay_ms: 100, // Start with 100ms delay, exponential backoff
        })
    }

    /// Start the storage node services
    pub async fn start(&self) -> Result<()> {
        info!("Starting SpaceKit Storage Node...");
        info!("Node DID: {}", self.config.node_did);
        info!(
            "Max storage: {} GB",
            self.config.max_storage_bytes / (1024 * 1024 * 1024)
        );
        info!(
            "Preferred quantum algorithm: {}",
            self.config.preferred_algorithm
        );

        if let Err(e) =
            crate::upload_token::persist_upload_token_secret_from_env(&self.config.data_dir)
        {
            warn!(
                "could not persist SPACEKIT_UPLOAD_TOKEN_SECRET to data_dir ({}); \
                 set env before starting the node or write {}/.upload_token_secret",
                e,
                self.config.data_dir.display()
            );
        }
        if let Err(e) = crate::migration::ensure_operator_keypair(&self.config.data_dir) {
            warn!(
                "could not ensure operator SPHINCS keypair ({}): {}",
                e,
                self.config
                    .data_dir
                    .join(".operator_sphincs_keypair")
                    .display()
            );
        }
        if let Ok(s) = std::env::var("SPACEKIT_HANDOFF_SECRET") {
            let t = s.trim();
            if !t.is_empty() {
                let path = self.config.data_dir.join(".handoff_secret");
                let _ = std::fs::write(&path, t.as_bytes());
            }
        }

        // Start P2P network if configured
        #[cfg(feature = "p2p")]
        if let Some(p2p_network) = &self.p2p_network {
            info!(
                "Starting P2P network on port {}",
                self.config.network_config.listen_port
            );
            let network = p2p_network.clone();
            tokio::spawn(async move {
                if let Err(e) = network.start().await {
                    error!("P2P network error: {}", e);
                }
            });
        }

        // Start API server if configured
        #[cfg(feature = "api-server")]
        if let Some(api_config) = &self.config.api_config {
            info!("Starting HTTP API server on port {}", api_config.port);

            let server_config = api_config.clone();
            let database = self.database.clone();
            let data_dir = self.config.data_dir.clone();
            let quantum_crypto = self.quantum_crypto.clone();
            let server_routing = self.server_routing.clone();
            let enable_real_transactions = storage_facade::resolve_enable_real_transactions(
                self.config.enable_real_transactions,
            );
            let operator_did = self.config.node_did.clone();
            let node_config = self.config.clone();
            let files = self.files.clone();
            #[cfg(feature = "p2p")]
            let p2p_network = self.p2p_network.clone();
            let memory_sources = crate::memory_diagnostic::MemoryDiagnosticSources {
                files: Some(files),
                #[cfg(feature = "p2p")]
                p2p: p2p_network,
                enable_p2p: node_config.enable_p2p,
                cache_p2p_chunks_in_memory: node_config.network_config.cache_p2p_chunks_in_memory,
            };

            tokio::spawn(async move {
                // ── Build the Phase 0/1/3/4 storage facade ──
                let upload_token_secret = crate::upload_token::load_signing_secret(Some(&data_dir));
                let blob_fact_auth_mode = crate::access_policy::BlobFactAuthMode::from_env();
                let facade_cfg = crate::storage_facade::FacadeConfig {
                    enable_real_transactions,
                    sandbox_persistence_root: Some(data_dir.join("sandboxes")),
                    cas_data_dir: Some(data_dir.clone()),
                    upload_token_secret,
                    blob_fact_auth_mode,
                    operator_did: Some(operator_did),
                    ..Default::default()
                };
                let facade_data_dir = data_dir.clone();
                let facade = match crate::storage_facade::Facade::new(database.clone(), facade_cfg)
                    .await
                {
                    Ok(f) => {
                        let f = std::sync::Arc::new(f);
                        // Phase 4: enable disk-backed change log so
                        // subscribers can resume across restarts.
                        if let Err(e) = f
                            .change_feed
                            .enable_disk_persistence(facade_data_dir.join("change_log.jsonl"))
                            .await
                        {
                            error!(
                                "change_feed disk persistence init failed ({}); using in-memory ring only",
                                e
                            );
                        }
                        Some(f)
                    }
                    Err(e) => {
                        error!("Failed to build storage facade ({}): legacy routes only", e);
                        None
                    }
                };

                let database_for_memory = database.clone();
                let mut api_server = ApiServer::new_with_file_access_and_routing(
                    database,
                    data_dir,
                    quantum_crypto,
                    server_routing,
                );
                if let Some(f) = facade.clone() {
                    api_server = api_server.with_facade(f);
                    api_server = api_server.with_memory_route_state(
                        crate::api::agentic_routes::AgenticMemoryRouteState {
                            config: node_config.clone(),
                            database: database_for_memory,
                            sources: memory_sources.clone(),
                        },
                    );
                }
                if let Err(e) = api_server.init_server_keypair().await {
                    error!("Failed to initialize server keypair: {}", e);
                }

                // Spawn a background sandbox reaper if the facade is wired.
                if let Some(f) = facade {
                    let f_clone = f.clone();
                    tokio::spawn(async move {
                        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60));
                        loop {
                            tick.tick().await;
                            f_clone.sandboxes.reap().await;
                            let _ = f_clone.transactions.cleanup_expired_transactions().await;
                        }
                    });
                }

                if let Err(e) = api_server.start(server_config).await {
                    error!("API server error: {}", e);
                }
            });
        }

        info!("Storage node started successfully");
        Ok(())
    }

    /// Store a file with quantum-resistant encryption (SECURE - user-controlled)
    ///
    /// The file is encrypted with the provided public key. The storage node does NOT
    /// store the private key and cannot decrypt the file. The user must provide their
    /// private key to decrypt.
    ///
    /// # Arguments
    /// * `filename` - Name of the file
    /// * `data` - Plaintext data to encrypt
    /// * `owner_did` - DID of the file owner
    /// * `owner_public_key` - Public key to encrypt with (hex-encoded)
    /// * `content_type` - Optional MIME type
    ///
    /// # Returns
    /// File ID and public key used (for verification)
    ///
    /// # Concurrency Control
    /// This method uses a semaphore to limit concurrent operations, preventing
    /// connection overload during burst requests (e.g., API warmup).
    pub async fn store_file(
        &self,
        filename: &str,
        data: &[u8],
        owner_did: &str,
        owner_public_key: &[u8],
        content_type: Option<String>,
    ) -> Result<(String, String)> {
        // Acquire permit to limit concurrent operations
        let _permit = self.acquire_operation_permit().await?;

        // Execute with retry logic for transient failures
        self.retry_operation(|| {
            let filename = filename.to_string();
            let data = data.to_vec();
            let owner_did = owner_did.to_string();
            let owner_public_key = owner_public_key.to_vec();
            let content_type = content_type.clone();

            async move {
                self.store_file_internal(
                    &filename,
                    &data,
                    &owner_did,
                    &owner_public_key,
                    content_type,
                )
                .await
            }
        })
        .await
    }

    /// Internal implementation of store_file (without concurrency control)
    async fn store_file_internal(
        &self,
        filename: &str,
        data: &[u8],
        owner_did: &str,
        owner_public_key: &[u8],
        content_type: Option<String>,
    ) -> Result<(String, String)> {
        let file_id = uuid::Uuid::new_v4().to_string();

        // Encrypt data with user's public key (storage node never sees private key)
        let encrypted_data = self
            .quantum_crypto
            .encrypt_data(data, owner_public_key)
            .await?;

        // Calculate file hash for integrity
        let hash = hex::encode(blake3::hash(data).as_bytes());

        // Create encryption info
        let encryption_info = EncryptionInfo {
            algorithm: encrypted_data.metadata.algorithm.clone(),
            key_derivation: encrypted_data.metadata.key_derivation.clone(),
            cipher_suite: encrypted_data.metadata.cipher_suite.clone(),
            quantum_resistant: true,
        };

        // Store public key with metadata (so we know which keypair was used)
        let public_key_hex = hex::encode(owner_public_key);

        // Create file metadata
        let metadata = FileMetadata {
            id: file_id.clone(),
            filename: filename.to_string(),
            size: data.len() as u64,
            hash: hash.clone(),
            owner_did: owner_did.to_string(),
            encryption_algorithm: encryption_info.algorithm.clone(),
            content_type,
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: Some(public_key_hex.clone()),
            sharing_mode: "owner".to_string(), // Single owner, not shared
        };

        // Store metadata in database
        self.database.insert_file_metadata(&metadata)?;

        // Chunk metadata for P2P announcements (bytes persist on disk only).
        let chunk_size = self.config.network_config.chunk_size;
        let mut chunk_hashes = Vec::new();
        let mut chunk_ids = Vec::new();

        for (i, chunk_data) in encrypted_data.data.chunks(chunk_size).enumerate() {
            let chunk_id = format!("{}_{}", file_id, i);
            let chunk_hash = hex::encode(blake3::hash(chunk_data).as_bytes());
            chunk_hashes.push(chunk_hash);
            chunk_ids.push(chunk_id);
        }

        // Create stored file entry
        let stored_file = StoredFile {
            id: file_id.clone(),
            metadata,
            access_control: vec![AccessControlEntry {
                did: owner_did.to_string(),
                permissions: FilePermissions::Admin,
                granted_at: chrono::Utc::now(),
            }],
            data_chunks: chunk_hashes,
            encryption_info,
        };

        // Write full EncryptedData structure to disk (JSON format for easy retrieval)
        // NOTE: Storage node does NOT store private key - user must provide it for decryption
        let data_path = self.config.data_dir.join(&file_id);
        tokio::fs::create_dir_all(&self.config.data_dir).await?;
        let encrypted_data_json = serde_json::to_vec(&encrypted_data)?;
        tokio::fs::write(&data_path, &encrypted_data_json).await?;

        // P2P: announce after disk persist — never retain chunk bytes in RAM by default.
        #[cfg(feature = "p2p")]
        if let Some(p2p) = &self.p2p_network {
            for (i, chunk_id) in chunk_ids.iter().enumerate() {
                if p2p.cache_chunks_in_memory() {
                    let chunk_data = encrypted_data.data.chunks(chunk_size).nth(i).unwrap_or(&[]);
                    let chunk = crate::network::FileChunk {
                        chunk_id: chunk_id.clone(),
                        file_id: file_id.clone(),
                        chunk_index: i,
                        data: chunk_data.to_vec(),
                        hash: stored_file.data_chunks[i].clone(),
                        encrypted: true,
                    };
                    if let Err(e) = p2p.store_chunk(chunk).await {
                        tracing::warn!("Failed to store chunk {} in P2P: {}", chunk_id, e);
                    }
                } else if let Err(e) = p2p.announce_chunk(chunk_id).await {
                    tracing::warn!("Failed to announce chunk {} in P2P: {}", chunk_id, e);
                }
            }
            if let Err(e) = p2p.announce_file(&file_id, chunk_ids).await {
                tracing::warn!("Failed to announce file {} in P2P: {}", file_id, e);
            }
        }

        // Store in memory cache
        {
            let mut files = self.files.write().await;
            files.insert(file_id.clone(), stored_file);
        }

        // Persist owner admin grant to DB
        let owner_grant = crate::database::FileAccessGrant {
            file_id: file_id.clone(),
            grantee_did: owner_did.to_string(),
            granter_did: owner_did.to_string(),
            permissions: "admin".to_string(),
            granted_at: chrono::Utc::now(),
        };
        if let Err(e) = self.database.upsert_file_access_grant(&owner_grant) {
            tracing::error!("Failed to persist owner grant to DB: {}", e);
        }

        #[cfg(feature = "p2p")]
        self.broadcast_file_event(&file_id, owner_did, data.len() as u64)
            .await;

        info!(
            "File stored with user-controlled encryption: {} ({})",
            filename, file_id
        );
        Ok((file_id, public_key_hex))
    }

    /// Broadcast file availability to subscribed peers via gossipsub
    #[cfg(feature = "p2p")]
    async fn broadcast_file_event(&self, file_id: &str, owner_did: &str, size: u64) {
        if let Some(p2p) = &self.p2p_network {
            let event = serde_json::json!({
                "type": "file_available",
                "file_id": file_id,
                "owner_did": owner_did,
                "size": size,
                "timestamp": chrono::Utc::now().to_rfc3339(),
            });
            if let Ok(bytes) = serde_json::to_vec(&event) {
                let _ = p2p.publish_to_topic("spacekit/files/v1", bytes).await;
            }
        }
    }

    /// Retrieve a file (SECURE - requires user's private key)
    ///
    /// The storage node cannot decrypt files - the user must provide their private key.
    /// This ensures zero-knowledge encryption where the storage node never has access
    /// to decrypted data.
    ///
    /// # Arguments
    /// * `file_id` - ID of the file to retrieve
    /// * `requester_did` - DID of the requester (for access control)
    /// * `user_private_key` - REQUIRED: Private key matching the public key used for encryption
    ///
    /// # Returns
    /// Decrypted file content, or None if access denied or key doesn't match
    ///
    /// # Concurrency Control
    /// This method uses a semaphore to limit concurrent operations, preventing
    /// connection overload during burst requests (e.g., API warmup).
    pub async fn retrieve_file(
        &self,
        file_id: &str,
        requester_did: &str,
        user_private_key: &[u8], // REQUIRED - storage node cannot decrypt without it
    ) -> Result<Option<Vec<u8>>> {
        // Acquire permit to limit concurrent operations
        let _permit = self.acquire_operation_permit().await?;

        // Execute with retry logic for transient failures
        self.retry_operation(|| {
            let file_id = file_id.to_string();
            let requester_did = requester_did.to_string();
            let user_private_key = user_private_key.to_vec();

            async move {
                self.retrieve_file_internal(&file_id, &requester_did, &user_private_key)
                    .await
            }
        })
        .await
    }

    /// Internal implementation of retrieve_file (without concurrency control)
    async fn retrieve_file_internal(
        &self,
        file_id: &str,
        requester_did: &str,
        user_private_key: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        // Get file metadata from database
        let metadata = match self.database.get_file_metadata(file_id) {
            Ok(Some(m)) => m,
            Ok(None) => {
                warn!("File not found: {}", file_id);
                return Ok(None);
            }
            Err(e) => {
                error!("Database error retrieving file {}: {}", file_id, e);
                return Err(anyhow::anyhow!("Database error: {}", e));
            }
        };

        // Check if file exists in cache for access control
        let stored_file = {
            let files = self.files.read().await;
            files.get(file_id).cloned()
        };

        // Check access control if file is in cache
        if let Some(file) = &stored_file {
            if !self.check_file_access(file, requester_did) {
                warn!(
                    "Access denied for file {} by DID {}",
                    file_id, requester_did
                );
                return Ok(None);
            }
        } else {
            // File not in cache — check owner OR persisted DB grants
            if metadata.owner_did != requester_did {
                match self.database.has_file_access(file_id, requester_did) {
                    Ok(true) => { /* granted via DB */ }
                    _ => {
                        warn!(
                            "Access denied for file {} by DID {} (not owner, no DB grant)",
                            file_id, requester_did
                        );
                        return Ok(None);
                    }
                }
            }
        }

        // Read encrypted data from disk
        let data_path = self.config.data_dir.join(file_id);
        if !data_path.exists() {
            warn!("File data not found on disk: {:?}", data_path);
            return Ok(None);
        }

        let encrypted_data_json = tokio::fs::read(&data_path).await?;

        // Deserialize EncryptedData structure
        let encrypted_data: crate::quantum::EncryptedData =
            serde_json::from_slice(&encrypted_data_json)
                .map_err(|e| anyhow::anyhow!("Failed to deserialize encrypted data: {}", e))?;

        // Verify the provided private key matches the stored public key
        // This ensures only the correct keypair can decrypt
        // For KEM algorithms (like Kyber), we verify by testing encryption/decryption
        if let Some(stored_public_key_hex) = &metadata.encryption_public_key {
            let stored_public_key = match hex::decode(stored_public_key_hex) {
                Ok(key) => key,
                Err(e) => {
                    warn!("Failed to decode stored public key for file {}: {}. Proceeding with decryption attempt.", file_id, e);
                    // Skip verification if we can't decode the public key - decryption will fail if key is wrong
                    // Return empty vec to skip the verification block
                    Vec::new()
                }
            };

            // Only verify if we successfully decoded the public key
            if !stored_public_key.is_empty() {
                // Parse algorithm from metadata
                let algorithm = match self
                    .quantum_crypto
                    .parse_algorithm(&metadata.encryption_algorithm)
                {
                    Ok(algo) => algo,
                    Err(e) => {
                        warn!(
                            "Failed to parse algorithm for file {}: {}. Using default.",
                            file_id, e
                        );
                        self.quantum_crypto.default_algorithm.clone()
                    }
                };

                // Verify keypair matches before attempting decryption
                // This prevents wasting time on decryption with wrong keys
                match self
                    .quantum_crypto
                    .verify_keypair(&stored_public_key, user_private_key, Some(algorithm))
                    .await
                {
                    Ok(true) => {
                        debug!("Keypair verification successful for file {} - private key matches public key", file_id);
                    }
                    Ok(false) => {
                        error!("Keypair verification failed for file {}: private key does not match public key", file_id);
                        return Err(anyhow::anyhow!("Keypair verification failed: The provided private key does not match the public key used for encryption. Please verify you're using the correct keypair."));
                    }
                    Err(e) => {
                        warn!("Keypair verification error for file {}: {}. Proceeding with decryption attempt.", file_id, e);
                        // Continue - decryption will fail if key is wrong, but we'll try anyway
                    }
                }
            }
        } else {
            warn!("No public key stored for file {} - cannot verify keypair. Proceeding with decryption attempt.", file_id);
        }

        // Decrypt using user's private key (storage node never stores private keys)
        let decrypted_data = match self
            .quantum_crypto
            .decrypt_data(&encrypted_data, user_private_key)
            .await
        {
            Ok(data) => data,
            Err(e) => {
                error!("Decryption failed for file {}: {}. This may indicate the wrong private key was provided.", file_id, e);
                return Err(anyhow::anyhow!("Decryption failed: {}. The provided private key may not match the public key used for encryption.", e));
            }
        };

        // Update last accessed timestamp
        let mut updated_metadata = metadata.clone();
        updated_metadata.last_accessed = Some(chrono::Utc::now());
        self.database.insert_file_metadata(&updated_metadata)?;

        info!(
            "File retrieved and decrypted: {} by {} (zero-knowledge encryption)",
            file_id, requester_did
        );
        Ok(Some(decrypted_data))
    }

    /// Delete a file (enhanced with proper cleanup)
    pub async fn delete_file(&self, file_id: &str, requester_did: &str) -> Result<bool> {
        // First check if file exists in database (even if not in cache)
        let metadata = match self.database.get_file_metadata(file_id) {
            Ok(Some(m)) => m,
            Ok(None) => {
                warn!("File not found in database: {}", file_id);
                return Ok(false);
            }
            Err(e) => {
                error!("Database error checking file {}: {}", file_id, e);
                return Err(anyhow::anyhow!("Database error: {}", e));
            }
        };

        // Check if file exists in cache for access control
        let stored_file = {
            let files = self.files.read().await;
            files.get(file_id).cloned()
        };

        // Check access control - owner always has admin access
        let has_admin = if let Some(file) = &stored_file {
            // Check cache for admin permissions
            file.access_control.iter().any(|ace| {
                ace.did == requester_did && matches!(ace.permissions, FilePermissions::Admin)
            })
        } else {
            // Not in cache, check if requester is the owner
            metadata.owner_did == requester_did
        };

        if !has_admin {
            warn!(
                "Delete access denied for file {} by DID {}",
                file_id, requester_did
            );
            return Ok(false);
        }

        // Delete encrypted data file from disk
        let data_path = self.config.data_dir.join(file_id);
        if data_path.exists() {
            if let Err(e) = tokio::fs::remove_file(&data_path).await {
                warn!(
                    "Failed to delete file data from disk {:?}: {}",
                    data_path, e
                );
                // Continue with deletion even if file doesn't exist on disk
            } else {
                info!("Deleted file data from disk: {:?}", data_path);
            }
        }

        // Delete associated .key file if it exists (legacy storage mode)
        let key_path = self.config.data_dir.join(format!("{}.key", file_id));
        if key_path.exists() {
            if let Err(e) = tokio::fs::remove_file(&key_path).await {
                warn!("Failed to delete key file from disk {:?}: {}", key_path, e);
                // Continue with deletion even if key file doesn't exist
            } else {
                info!("Deleted key file from disk: {:?}", key_path);
            }
        }

        // Remove from database (this persists the deletion)
        self.database.delete_file_metadata(file_id)?;

        // Remove from memory cache
        {
            let mut files = self.files.write().await;
            files.remove(file_id);
        }

        info!(
            "File deleted successfully: {} by {} (metadata, cache, and disk files removed)",
            file_id, requester_did
        );
        Ok(true)
    }

    /// List files for a specific owner (enhanced)
    pub async fn list_files(&self, owner_did: &str) -> Result<Vec<FileMetadata>> {
        self.database.list_files_by_owner(owner_did)
    }

    /// Store simple key-value data (for AI companion conversations)
    /// This is a simpler interface than store_file, optimized for small JSON payloads
    pub async fn store_key_value(
        &self,
        key: &str,
        value: &[u8],
        owner_did: &str,
        owner_public_key: &[u8],
    ) -> Result<()> {
        // Use the file storage mechanism but with a simpler interface
        self.store_file(
            key, // Use key as filename
            value,
            owner_did,
            owner_public_key,
            Some("application/json".to_string()),
        )
        .await?;
        Ok(())
    }

    /// Share a file with another user using their public key (asymmetric encryption)
    ///
    /// Creates a new encrypted copy of the file encrypted with the recipient's public key.
    /// The original file remains encrypted with the owner's key.
    ///
    /// # Arguments
    /// * `file_id` - ID of the file to share
    /// * `owner_did` - DID of the file owner (for authorization)
    /// * `owner_private_key` - Owner's private key to decrypt original file
    /// * `recipient_did` - DID of the recipient
    /// * `recipient_public_key` - Recipient's public key to encrypt the shared copy
    ///
    /// # Returns
    /// New file ID for the shared copy
    pub async fn share_file_with_user(
        &self,
        file_id: &str,
        owner_did: &str,
        owner_private_key: &[u8],
        recipient_did: &str,
        recipient_public_key: &[u8],
    ) -> Result<String> {
        // Retrieve and decrypt the original file
        let decrypted_data = match self
            .retrieve_file(file_id, owner_did, owner_private_key)
            .await?
        {
            Some(data) => data,
            None => return Err(anyhow::anyhow!("File not found or access denied")),
        };

        // Get original metadata
        let original_metadata = self
            .database
            .get_file_metadata(file_id)?
            .ok_or_else(|| anyhow::anyhow!("File metadata not found"))?;

        // Encrypt with recipient's public key
        let shared_file_id = uuid::Uuid::new_v4().to_string();
        let encrypted_data = self
            .quantum_crypto
            .encrypt_data(&decrypted_data, recipient_public_key)
            .await?;

        // Create metadata for shared file
        let shared_metadata = FileMetadata {
            id: shared_file_id.clone(),
            filename: format!("{} (shared)", original_metadata.filename),
            size: decrypted_data.len() as u64,
            hash: original_metadata.hash.clone(),
            owner_did: recipient_did.to_string(),
            encryption_algorithm: encrypted_data.metadata.algorithm.clone(),
            content_type: original_metadata.content_type.clone(),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: Some(hex::encode(recipient_public_key)),
            sharing_mode: "shared".to_string(), // Shared with specific user
        };

        // Store shared file
        self.database.insert_file_metadata(&shared_metadata)?;

        // Write encrypted data
        let data_path = self.config.data_dir.join(&shared_file_id);
        let encrypted_data_json = serde_json::to_vec(&encrypted_data)?;
        tokio::fs::write(&data_path, &encrypted_data_json).await?;

        // Add access control entry to original file
        self.grant_access(file_id, owner_did, recipient_did, FilePermissions::Read)
            .await?;

        info!(
            "File {} shared with {} as {}",
            file_id, recipient_did, shared_file_id
        );
        Ok(shared_file_id)
    }

    /// Share a file with a group using a shared symmetric key
    ///
    /// For group sharing, we use a symmetric key that all group members know.
    /// This is more efficient than encrypting for each member individually.
    ///
    /// # Arguments
    /// * `file_id` - ID of the file to share
    /// * `owner_did` - DID of the file owner
    /// * `owner_private_key` - Owner's private key to decrypt original
    /// * `group_id` - Identifier for the group
    /// * `shared_symmetric_key` - Symmetric key known to all group members
    ///
    /// # Returns
    /// New file ID for the group-shared copy
    pub async fn share_file_with_group(
        &self,
        file_id: &str,
        owner_did: &str,
        owner_private_key: &[u8],
        group_id: &str,
        shared_symmetric_key: &[u8],
    ) -> Result<String> {
        // Retrieve and decrypt the original file
        let decrypted_data = match self
            .retrieve_file(file_id, owner_did, owner_private_key)
            .await?
        {
            Some(data) => data,
            None => return Err(anyhow::anyhow!("File not found or access denied")),
        };

        // Get original metadata
        let original_metadata = self
            .database
            .get_file_metadata(file_id)?
            .ok_or_else(|| anyhow::anyhow!("File metadata not found"))?;

        // Derive an encryption key from the shared group secret + identifiers.
        // We store a per-file random nonce alongside ciphertext for decryption.
        let mut key_material = Vec::new();
        key_material.extend_from_slice(shared_symmetric_key);
        key_material.extend_from_slice(group_id.as_bytes());
        key_material.extend_from_slice(file_id.as_bytes());
        let derived_key = blake3::hash(&key_material);

        #[cfg(feature = "quantum")]
        let encrypted_with_nonce: Vec<u8> = {
            use aes_gcm::{
                aead::{Aead, AeadCore, KeyInit, OsRng},
                Aes256Gcm,
            };

            let cipher = Aes256Gcm::new_from_slice(derived_key.as_bytes())
                .map_err(|_| anyhow::anyhow!("Invalid derived key for AES-256-GCM"))?;

            // AES-GCM nonce is 96-bit / 12 bytes.
            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
            let ciphertext = cipher
                .encrypt(&nonce, decrypted_data.as_ref())
                .map_err(|_| anyhow::anyhow!("AES-256-GCM encryption failed"))?;

            let mut out = nonce.to_vec();
            out.extend_from_slice(&ciphertext);
            out
        };

        #[cfg(not(feature = "quantum"))]
        let encrypted_with_nonce: Vec<u8> = {
            return Err(anyhow::anyhow!(
                "Group sharing requires the \"quantum\" feature (AES-256-GCM)"
            ));
        };

        // Create metadata for group-shared file
        let group_file_id = uuid::Uuid::new_v4().to_string();
        let shared_metadata = FileMetadata {
            id: group_file_id.clone(),
            filename: format!("{} (group: {})", original_metadata.filename, group_id),
            size: decrypted_data.len() as u64,
            hash: original_metadata.hash.clone(),
            owner_did: group_id.to_string(), // Group ID as owner
            encryption_algorithm: "aes-256-gcm-symmetric".to_string(),
            content_type: original_metadata.content_type.clone(),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None, // Symmetric key, no public key
            sharing_mode: format!("group:{}", group_id),
        };

        // Store shared file
        self.database.insert_file_metadata(&shared_metadata)?;

        // Write encrypted data (with nonce prepended)
        let data_path = self.config.data_dir.join(&group_file_id);
        tokio::fs::write(&data_path, &encrypted_with_nonce).await?;

        info!(
            "File {} shared with group {} as {}",
            file_id, group_id, group_file_id
        );
        Ok(group_file_id)
    }

    /// Store pre-encrypted data without additional encryption
    /// Use this when data is already encrypted by the user
    ///
    /// # Concurrency Control
    /// This method uses a semaphore to limit concurrent operations, preventing
    /// connection overload during burst requests (e.g., API warmup).
    pub async fn store_encrypted_data(
        &self,
        key: &str,
        encrypted_data: &[u8],
        owner_did: &str,
    ) -> Result<()> {
        // Acquire permit to limit concurrent operations
        let _permit = self.acquire_operation_permit().await?;

        // Execute with retry logic for transient failures
        self.retry_operation(|| {
            let key = key.to_string();
            let encrypted_data = encrypted_data.to_vec();
            let owner_did = owner_did.to_string();

            async move {
                self.store_encrypted_data_internal(&key, &encrypted_data, &owner_did)
                    .await
            }
        })
        .await
    }

    /// Internal implementation of store_encrypted_data (without concurrency control)
    async fn store_encrypted_data_internal(
        &self,
        key: &str,
        encrypted_data: &[u8],
        owner_did: &str,
    ) -> Result<()> {
        let file_id = uuid::Uuid::new_v4().to_string();

        // Calculate hash for integrity
        let hash = hex::encode(blake3::hash(encrypted_data).as_bytes());

        // Create minimal encryption info (data is already encrypted by user)
        let encryption_info = EncryptionInfo {
            algorithm: "user-encrypted".to_string(),
            key_derivation: "user-managed".to_string(),
            cipher_suite: "user-managed".to_string(),
            quantum_resistant: true,
        };

        // Create file metadata (for pre-encrypted data, public key should be provided)
        let metadata = FileMetadata {
            id: file_id.clone(),
            filename: key.to_string(),
            size: encrypted_data.len() as u64,
            hash: hash.clone(),
            owner_did: owner_did.to_string(),
            encryption_algorithm: encryption_info.algorithm.clone(),
            content_type: Some("application/octet-stream".to_string()),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: None, // Pre-encrypted data - public key not stored
            sharing_mode: "owner".to_string(),
        };

        // Store metadata in database
        self.database.insert_file_metadata(&metadata)?;

        // Create stored file entry
        let stored_file = StoredFile {
            id: file_id.clone(),
            metadata,
            access_control: vec![AccessControlEntry {
                did: owner_did.to_string(),
                permissions: FilePermissions::Admin,
                granted_at: chrono::Utc::now(),
            }],
            data_chunks: vec![],
            encryption_info,
        };

        // Write encrypted data directly to disk (no additional encryption)
        let data_path = self.config.data_dir.join(&file_id);
        tokio::fs::create_dir_all(&self.config.data_dir).await?;
        tokio::fs::write(&data_path, encrypted_data).await?;

        // Store in memory cache (remove old entry with same key first)
        {
            let mut files = self.files.write().await;

            // Remove any existing entry with the same key
            let old_entries: Vec<String> = files
                .iter()
                .filter(|(_, f)| f.metadata.filename == key)
                .map(|(id, _)| id.clone())
                .collect();

            for old_id in &old_entries {
                files.remove(old_id);
                // Also delete old file from disk
                let old_path = self.config.data_dir.join(old_id);
                if old_path.exists() {
                    let _ = tokio::fs::remove_file(&old_path).await;
                }
            }

            files.insert(file_id.clone(), stored_file);
        }

        info!("Pre-encrypted data stored: {} ({})", key, file_id);
        Ok(())
    }

    /// Retrieve simple key-value data (for AI companion conversations)
    pub async fn retrieve_key_value(
        &self,
        key: &str,
        requester_did: &str,
    ) -> Result<Option<Vec<u8>>> {
        // First find the file by its "filename" (which is our key)
        let files = self.files.read().await;

        // Find the file with matching filename
        let stored_file = files.values().find(|f| f.metadata.filename == key).cloned();

        drop(files); // Release lock

        if let Some(file) = stored_file {
            // Check access control
            if !self.check_file_access(&file, requester_did) {
                warn!("Access denied for key {} by DID {}", key, requester_did);
                return Ok(None);
            }

            // Read the actual data from disk.
            // NOTE: This key-value store is for pre-encrypted payloads; return bytes as-is.
            let data_path = self.config.data_dir.join(&file.id);
            if data_path.exists() {
                let encrypted_data = tokio::fs::read(&data_path).await?;
                Ok(Some(encrypted_data))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Grant file access to another DID
    pub async fn grant_access(
        &self,
        file_id: &str,
        granter_did: &str,
        grantee_did: &str,
        permissions: FilePermissions,
    ) -> Result<bool> {
        let mut files = self.files.write().await;

        if let Some(file) = files.get_mut(file_id) {
            // Check if granter has admin access
            let has_admin = file.access_control.iter().any(|ace| {
                ace.did == granter_did && matches!(ace.permissions, FilePermissions::Admin)
            });

            if !has_admin {
                warn!(
                    "Grant access denied for file {} by DID {}",
                    file_id, granter_did
                );
                return Ok(false);
            }

            // Add or update access control entry
            let permissions_clone = permissions.clone();
            if let Some(existing) = file
                .access_control
                .iter_mut()
                .find(|ace| ace.did == grantee_did)
            {
                existing.permissions = permissions;
                existing.granted_at = chrono::Utc::now();
            } else {
                file.access_control.push(AccessControlEntry {
                    did: grantee_did.to_string(),
                    permissions,
                    granted_at: chrono::Utc::now(),
                });
            }

            info!(
                "Access granted for file {} to {} by {}",
                file_id, grantee_did, granter_did
            );

            // Persist to database
            let perm_str = match &permissions_clone {
                FilePermissions::Read => "read",
                FilePermissions::Write => "write",
                FilePermissions::ReadWrite => "readwrite",
                FilePermissions::Admin => "admin",
            };
            let grant = crate::database::FileAccessGrant {
                file_id: file_id.to_string(),
                grantee_did: grantee_did.to_string(),
                granter_did: granter_did.to_string(),
                permissions: perm_str.to_string(),
                granted_at: chrono::Utc::now(),
            };
            if let Err(e) = self.database.upsert_file_access_grant(&grant) {
                tracing::error!("Failed to persist access grant to DB: {}", e);
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Revoke file access from a DID
    pub async fn revoke_access(
        &self,
        file_id: &str,
        revoker_did: &str,
        target_did: &str,
    ) -> Result<bool> {
        let mut files = self.files.write().await;

        if let Some(file) = files.get_mut(file_id) {
            // Check if revoker has admin access
            let has_admin = file.access_control.iter().any(|ace| {
                ace.did == revoker_did && matches!(ace.permissions, FilePermissions::Admin)
            });

            if !has_admin {
                warn!(
                    "Revoke access denied for file {} by DID {}",
                    file_id, revoker_did
                );
                return Ok(false);
            }

            // Prevent removing the last admin to avoid orphaned files
            let admin_count = file
                .access_control
                .iter()
                .filter(|ace| matches!(ace.permissions, FilePermissions::Admin))
                .count();
            let target_is_admin = file.access_control.iter().any(|ace| {
                ace.did == target_did && matches!(ace.permissions, FilePermissions::Admin)
            });
            if target_is_admin && admin_count <= 1 {
                warn!("Cannot revoke last admin for file {}", file_id);
                return Ok(false);
            }

            let original_len = file.access_control.len();
            file.access_control.retain(|ace| ace.did != target_did);
            let revoked = file.access_control.len() != original_len;

            if revoked {
                if let Err(e) = self.database.remove_file_access_grant(file_id, target_did) {
                    tracing::error!("Failed to persist access revoke to DB: {}", e);
                }
                info!(
                    "Access revoked for file {} from {} by {}",
                    file_id, target_did, revoker_did
                );
            }

            Ok(revoked)
        } else {
            Ok(false)
        }
    }

    /// Get storage node statistics (enhanced)
    pub async fn get_stats(&self) -> Result<StorageStats> {
        let files = self.files.read().await;
        let file_count = files.len();
        let total_size: u64 = files.values().map(|f| f.metadata.size).sum();

        // Get database statistics
        let users = self.database.select_all_users()?.len();
        let encrypted_users = self.database.select_all_enc_users()?.len();
        let messages = self.database.select_all_messages()?.len();

        Ok(StorageStats {
            file_count,
            total_size_bytes: total_size,
            max_storage_bytes: self.config.max_storage_bytes,
            storage_utilization: (total_size as f64 / self.config.max_storage_bytes as f64) * 100.0,
            user_count: users,
            encrypted_user_count: encrypted_users,
            message_count: messages,
            node_did: self.config.node_did.clone(),
            preferred_algorithm: self.config.preferred_algorithm.clone(),
        })
    }

    /// Check if a DID has access to a file
    fn check_file_access(&self, file: &StoredFile, requester_did: &str) -> bool {
        if file
            .access_control
            .iter()
            .any(|ace| ace.did == requester_did)
        {
            return true;
        }
        let store = crate::content_grants::ContentGrantStore::new(&self.config.data_dir);
        store.has_keychain_file_access(requester_did, &file.metadata.owner_did, &file.id)
            || self
                .database
                .has_file_access(&file.id, requester_did)
                .unwrap_or(false)
    }

    /// Acquire a permit for a storage operation (prevents connection overload)
    /// This should be called before any storage operation to limit concurrency
    async fn acquire_operation_permit(&self) -> Result<tokio::sync::SemaphorePermit<'_>> {
        self.request_semaphore.acquire().await.map_err(|e| {
            anyhow::anyhow!(
                "Failed to acquire request permit (storage node overloaded): {}",
                e
            )
        })
    }

    /// Execute an operation with retry logic
    /// Used for operations that may fail due to transient connection issues
    async fn retry_operation<F, Fut, T>(&self, mut operation: F) -> Result<T>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let mut last_error = None;
        let mut delay_ms = self.initial_retry_delay_ms;

        for attempt in 0..=self.max_retries {
            match operation().await {
                Ok(result) => {
                    if attempt > 0 {
                        debug!("Operation succeeded after {} retries", attempt);
                    }
                    return Ok(result);
                }
                Err(e) => {
                    // Check if error is retryable (connection errors, timeouts, etc.)
                    let error_str = e.to_string().to_lowercase();
                    let is_retryable = error_str.contains("connection")
                        || error_str.contains("timeout")
                        || error_str.contains("overloaded")
                        || error_str.contains("temporary")
                        || error_str.contains("io error");

                    if !is_retryable {
                        // Non-retryable error (e.g., permission denied, not found)
                        return Err(e);
                    }

                    last_error = Some(e);

                    // Don't retry on last attempt
                    if attempt < self.max_retries {
                        warn!(
                            "Operation failed (attempt {}/{}), retrying in {}ms",
                            attempt + 1,
                            self.max_retries + 1,
                            delay_ms
                        );

                        // Exponential backoff: 100ms, 200ms, 400ms
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        delay_ms *= 2;
                    }
                }
            }
        }

        // All retries exhausted
        error!("Operation failed after {} retries", self.max_retries + 1);
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Operation failed after retries")))
    }

    /// Get database reference for direct access (for API server)
    pub fn database(&self) -> Arc<Database> {
        self.database.clone()
    }

    /// Get configuration reference
    pub fn config(&self) -> &StorageNodeConfig {
        &self.config
    }

    /// Get quantum crypto service
    pub fn quantum_crypto(&self) -> Arc<QuantumCrypto> {
        self.quantum_crypto.clone()
    }

    #[cfg(feature = "p2p")]
    /// Get P2P network service
    pub fn p2p_network(&self) -> Option<Arc<P2PNetwork>> {
        self.p2p_network.clone()
    }
}

/// Storage node statistics (enhanced)
#[derive(Debug, Clone, Serialize)]
pub struct StorageStats {
    pub file_count: usize,
    pub total_size_bytes: u64,
    pub max_storage_bytes: u64,
    pub storage_utilization: f64, // Percentage
    // Database statistics (migrated features)
    pub user_count: usize,
    pub encrypted_user_count: usize,
    pub message_count: usize,
    // Node information
    pub node_did: String,
    pub preferred_algorithm: String,
}

// Tests with enhanced coverage
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_storage_node_creation() {
        let temp_dir = tempdir().unwrap();
        let mut config = StorageNodeConfig::default();
        config.data_dir = temp_dir.path().to_path_buf();
        config.database_path = Some(temp_dir.path().join("test.json"));
        config.node_did = "did:spacekit:test".to_string();

        let node = StorageNode::new(config).await;
        assert!(node.is_ok());
    }

    #[tokio::test]
    async fn test_file_storage_with_enhanced_features() {
        let temp_dir = tempdir().unwrap();
        let mut config = StorageNodeConfig::default();
        config.data_dir = temp_dir.path().to_path_buf();
        config.database_path = Some(temp_dir.path().join("test.json"));
        config.node_did = "did:spacekit:test".to_string();

        let node = StorageNode::new(config).await.unwrap();

        let test_data = b"Hello, quantum-resistant world!";
        let owner_did = "did:spacekit:owner";

        // Generate a test keypair for encryption
        let (public_key, _private_key) = node
            .quantum_crypto
            .generate_keypair(spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024)
            .await
            .unwrap();

        let result = node
            .store_file(
                "test.txt",
                test_data,
                owner_did,
                &public_key,
                Some("text/plain".to_string()),
            )
            .await;
        assert!(result.is_ok());

        let stats = node.get_stats().await.unwrap();
        assert_eq!(stats.file_count, 1);
        assert_eq!(stats.total_size_bytes, test_data.len() as u64);
    }

    #[tokio::test]
    async fn test_access_control() {
        let temp_dir = tempdir().unwrap();
        let mut config = StorageNodeConfig::default();
        config.data_dir = temp_dir.path().to_path_buf();
        config.database_path = Some(temp_dir.path().join("test.json"));
        config.node_did = "did:spacekit:test".to_string();

        let node = StorageNode::new(config).await.unwrap();

        let test_data = b"Private file content";
        let owner_did = "did:spacekit:owner";
        let other_did = "did:spacekit:other";

        // Generate a test keypair for encryption
        let (public_key, _private_key) = node
            .quantum_crypto
            .generate_keypair(spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024)
            .await
            .unwrap();

        let (file_id, _) = node
            .store_file("private.txt", test_data, owner_did, &public_key, None)
            .await
            .unwrap();

        // Grant read access to another DID
        let granted = node
            .grant_access(&file_id, owner_did, other_did, FilePermissions::Read)
            .await
            .unwrap();
        assert!(granted);

        // TODO: Test actual file retrieval with access control
    }

    #[tokio::test]
    async fn test_database_integration() {
        let temp_dir = tempdir().unwrap();
        let mut config = StorageNodeConfig::default();
        config.data_dir = temp_dir.path().to_path_buf();
        config.database_path = Some(temp_dir.path().join("test.json"));

        let node = StorageNode::new(config).await.unwrap();

        // Test database is accessible and initialized
        let stats = node.get_stats().await.unwrap();
        assert_eq!(stats.user_count, 0);
        assert_eq!(stats.message_count, 0);
    }
}
