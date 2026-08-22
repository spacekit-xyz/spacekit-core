//! Fact Package Storage Implementation
//!
//! This module provides storage capabilities for SpaceKit Fact Packages, including
//! quantum-safe storage, indexing, compression, and retrieval operations.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

// Import Fact Package primitives
use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
use spacekit_primitives::v1::fact::{
    types::{FactQuery, FactQueryResult, VerificationResult},
    AccessCondition, AccessPolicy, AttributeRequirements, CollectionMethod, ConditionType,
    DataSource, FactCategory, FactContent, FactID, FactMetadata, FactPackage, KnowledgeDomain,
    LicenseType, ProofType, VerificationLevel, VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;

/// Magic prefix for JSON-backed fact content (`FactContent::Json`).
/// Bincode cannot encode `serde_json::Value` (AnyNotSupported on decode).
const FACT_CONTENT_JSON_MAGIC: &[u8] = b"SKFCJ1\0";

#[derive(Serialize, Deserialize)]
struct JsonFactContentPayload {
    data: serde_json::Value,
    schema: Option<String>,
}

/// Encode fact body bytes for CAS storage (JSON facts use magic prefix; others use bincode).
pub(crate) fn encode_fact_content(content: &FactContent) -> Result<Vec<u8>> {
    match content {
        FactContent::Json { data, schema } => {
            let payload = JsonFactContentPayload {
                data: data.clone(),
                schema: schema.clone(),
            };
            let mut out = FACT_CONTENT_JSON_MAGIC.to_vec();
            out.extend(
                serde_json::to_vec(&payload)
                    .map_err(|e| anyhow!("JSON serialization error: {}", e))?,
            );
            Ok(out)
        }
        _ => bincode::serde::encode_to_vec(content, bincode::config::standard())
            .map_err(|e| anyhow!("Serialization error: {}", e)),
    }
}

/// Decode fact body bytes from CAS storage.
pub(crate) fn decode_fact_content(data: &[u8]) -> Result<FactContent> {
    if data.starts_with(FACT_CONTENT_JSON_MAGIC) {
        let payload: JsonFactContentPayload =
            serde_json::from_slice(&data[FACT_CONTENT_JSON_MAGIC.len()..])
                .map_err(|e| anyhow!("JSON deserialization error: {}", e))?;
        return Ok(FactContent::Json {
            data: payload.data,
            schema: payload.schema,
        });
    }

    match bincode::serde::decode_from_slice::<FactContent, _>(data, bincode::config::standard()) {
        Ok((content, _)) => Ok(content),
        Err(bincode_err) if data.first() == Some(&b'{') => {
            let payload: JsonFactContentPayload = serde_json::from_slice(data)
                .map_err(|e| anyhow!("Deserialization error: {} ({})", bincode_err, e))?;
            Ok(FactContent::Json {
                data: payload.data,
                schema: payload.schema,
            })
        }
        Err(e) => Err(anyhow!("Deserialization error: {}", e)),
    }
}

use crate::database::{Database, FactMetadataRecord};
use crate::quantum::QuantumCrypto;

/// Fact storage engine for managing Fact Packages
pub struct FactStorageEngine {
    /// Database connection for fact metadata
    database: Arc<Database>,
    /// Quantum crypto engine for encryption
    quantum_crypto: Arc<QuantumCrypto>,
    /// Storage configuration
    config: FactStorageConfig,
    /// In-memory index for fast queries
    fact_index: Arc<RwLock<FactIndex>>,
    /// Content storage backend
    content_storage: Arc<dyn ContentStorage + Send + Sync>,
    /// Verification cache
    verification_cache: Arc<RwLock<HashMap<FactID, VerificationResult>>>,
    /// Query result cache with TTL
    query_cache: Arc<RwLock<HashMap<QueryCacheKey, QueryCacheEntry>>>,
}

impl std::fmt::Debug for FactStorageEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FactStorageEngine")
            .field("database", &self.database)
            .field("quantum_crypto", &self.quantum_crypto)
            .field("config", &self.config)
            .field("fact_index", &self.fact_index)
            .field("content_storage", &"<ContentStorage>")
            .field("verification_cache", &self.verification_cache)
            .field("query_cache", &self.query_cache)
            .finish()
    }
}

/// Configuration for fact storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactStorageConfig {
    /// Base directory for fact storage
    pub storage_dir: PathBuf,
    /// Maximum fact size in bytes
    pub max_fact_size: u64,
    /// Enable compression for fact content
    pub enable_compression: bool,
    /// Compression algorithm to use
    pub compression_algorithm: CompressionAlgorithm,
    /// Enable content deduplication
    pub enable_deduplication: bool,
    /// Cache size for verification results
    pub verification_cache_size: usize,
    /// Enable automatic indexing
    pub enable_auto_indexing: bool,
    /// Storage tier configuration
    pub storage_tiers: StorageTierConfig,
}

/// Compression algorithms available
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CompressionAlgorithm {
    None,
    Gzip,
    Zstd,
    Lz4,
    Brotli,
}

fn compress_with_algorithm(data: &[u8], algorithm: &CompressionAlgorithm) -> Result<Vec<u8>> {
    match algorithm {
        CompressionAlgorithm::None => Ok(data.to_vec()),
        CompressionAlgorithm::Gzip => spacekit_compressor::BinaryCompressor::new().compress(data),
        CompressionAlgorithm::Zstd => {
            zstd::encode_all(data, 3).map_err(|e| anyhow!("Zstd compression error: {}", e))
        }
        CompressionAlgorithm::Lz4 => Ok(lz4_flex::compress_prepend_size(data)),
        CompressionAlgorithm::Brotli => {
            use brotli::CompressorWriter;
            use std::io::Write;

            let mut writer = CompressorWriter::new(Vec::new(), 4096, 6, 22);
            writer.write_all(data)?;
            Ok(writer.into_inner())
        }
    }
}

fn decompress_with_algorithm(data: &[u8], algorithm: &CompressionAlgorithm) -> Result<Vec<u8>> {
    match algorithm {
        CompressionAlgorithm::None => Ok(data.to_vec()),
        CompressionAlgorithm::Gzip => spacekit_compressor::BinaryCompressor::new().decompress(data),
        CompressionAlgorithm::Zstd => {
            zstd::decode_all(data).map_err(|e| anyhow!("Zstd decompression error: {}", e))
        }
        CompressionAlgorithm::Lz4 => lz4_flex::decompress_size_prepended(data)
            .map_err(|e| anyhow!("Lz4 decompression error: {}", e)),
        CompressionAlgorithm::Brotli => {
            use brotli::Decompressor;
            use std::io::Read;

            let mut decompressor = Decompressor::new(data, 4096);
            let mut decompressed = Vec::new();
            decompressor.read_to_end(&mut decompressed)?;
            Ok(decompressed)
        }
    }
}

/// Storage tier configuration for different fact types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageTierConfig {
    /// Hot storage for frequently accessed facts
    pub hot_storage_dir: PathBuf,
    /// Cold storage for archived facts
    pub cold_storage_dir: PathBuf,
    /// Archive threshold in days
    pub archive_threshold_days: u64,
    /// Maximum hot storage size in bytes
    pub max_hot_storage_bytes: u64,
}

/// In-memory index for fast fact queries
#[derive(Debug, Default)]
pub struct FactIndex {
    /// Map from fact ID to metadata
    pub fact_metadata: HashMap<FactID, IndexedFactMetadata>,
    /// Index by author
    pub by_author: HashMap<QuantumDID, HashSet<FactID>>,
    /// Index by category
    pub by_category: HashMap<String, HashSet<FactID>>,
    /// Index by domain
    pub by_domain: HashMap<String, HashSet<FactID>>,
    /// Index by tags
    pub by_tags: HashMap<String, HashSet<FactID>>,
    /// Dependency graph
    pub dependencies: HashMap<FactID, HashSet<FactID>>,
    /// Reverse dependency graph
    pub dependents: HashMap<FactID, HashSet<FactID>>,
}

/// Metadata stored in the index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFactMetadata {
    pub fact_id: FactID,
    pub version: u32,
    pub author: QuantumDID,
    pub created_at: u64,
    pub content_size: u64,
    pub content_type: String,
    pub category: String,
    pub domain: String,
    pub tags: Vec<String>,
    pub verification_level: String,
    pub confidence_score: f64,
    pub storage_location: StorageLocation,
    pub access_policy_hash: [u8; 32],
}

/// Storage location information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageLocation {
    pub tier: StorageTier,
    pub path: PathBuf,
    pub compressed: bool,
    pub encrypted: bool,
    pub checksum: [u8; 32],
}

/// Storage tiers
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StorageTier {
    Hot,    // Frequently accessed
    Cold,   // Archived
    Frozen, // Long-term storage
}

/// Query cache key for identifying cached queries
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct QueryCacheKey {
    pub requester_hash: [u8; 32], // Hash of the requester DID for privacy
    pub author_hash: Option<[u8; 32]>, // Hash of author filter
    pub category: Option<String>,
    pub domain: Option<String>,
    pub tags: Vec<String>,
    pub sort_by: String, // Serialized sort criteria
    pub pagination_offset: u64,
    pub pagination_limit: u64,
}

/// Query cache entry with TTL
#[derive(Debug, Clone)]
pub struct QueryCacheEntry {
    pub result: FactQueryResult,
    pub cached_at: SystemTime,
    pub ttl: Duration,
}

/// Trait for content storage backends
#[async_trait::async_trait]
pub trait ContentStorage {
    async fn store_content(&self, fact_id: FactID, content: &[u8]) -> Result<StorageLocation>;
    async fn retrieve_content(&self, location: &StorageLocation) -> Result<Vec<u8>>;
    async fn delete_content(&self, location: &StorageLocation) -> Result<()>;
    async fn move_to_tier(
        &self,
        location: &StorageLocation,
        tier: StorageTier,
    ) -> Result<StorageLocation>;
}

/// File-based content storage implementation
#[derive(Debug)]
pub struct FileContentStorage {
    config: StorageTierConfig,
    compression: CompressionAlgorithm,
}

impl FactStorageEngine {
    /// Create a new fact storage engine
    pub async fn new(
        database: Arc<Database>,
        quantum_crypto: Arc<QuantumCrypto>,
        config: FactStorageConfig,
    ) -> Result<Self> {
        // Ensure storage directories exist
        tokio::fs::create_dir_all(&config.storage_dir).await?;
        tokio::fs::create_dir_all(&config.storage_tiers.hot_storage_dir).await?;
        tokio::fs::create_dir_all(&config.storage_tiers.cold_storage_dir).await?;

        // Create content storage backend
        let content_storage = Arc::new(FileContentStorage::new(
            config.storage_tiers.clone(),
            config.compression_algorithm.clone(),
        )?);

        // Initialize fact index
        let fact_index = Arc::new(RwLock::new(FactIndex::default()));

        // Initialize verification cache
        let verification_cache = Arc::new(RwLock::new(HashMap::with_capacity(
            config.verification_cache_size,
        )));

        // Initialize query cache
        let query_cache = Arc::new(RwLock::new(HashMap::with_capacity(1000))); // Default cache size

        let engine = Self {
            database,
            quantum_crypto,
            config,
            fact_index,
            content_storage,
            verification_cache,
            query_cache,
        };

        // Load existing facts into index if auto-indexing is enabled
        if engine.config.enable_auto_indexing {
            engine.rebuild_index().await?;
        }

        Ok(engine)
    }

    fn content_grants_data_dir(&self) -> &std::path::Path {
        self.config
            .storage_dir
            .parent()
            .unwrap_or(&self.config.storage_dir)
    }

    /// Store a new fact package
    pub async fn store_fact(&self, fact: FactPackage) -> Result<FactID> {
        info!("Storing fact package: {:?}", fact.fact_id);

        // Clone fact to avoid partial moves
        let fact_clone = fact.clone();

        // Validate fact size
        let content_size = self.estimate_fact_size(&fact);
        if content_size > self.config.max_fact_size as usize {
            return Err(anyhow!(
                "Fact size {} exceeds maximum {}",
                content_size,
                self.config.max_fact_size
            ));
        }

        // Serialize fact content
        let serialized_content = self.serialize_fact_content(&fact.content)?;

        // Compress if enabled
        let compressed_content = if self.config.enable_compression {
            self.compress_content(&serialized_content)?
        } else {
            serialized_content
        };

        // Encrypt if required by access policy
        let (final_content, is_encrypted) = if self.should_encrypt_fact(&fact) {
            let encrypted_data = self
                .encrypt_fact_content(&compressed_content, &fact)
                .await?;
            (encrypted_data, true)
        } else {
            (compressed_content, false)
        };

        // Store content using storage backend
        let mut storage_location = self
            .content_storage
            .store_content(fact.fact_id, &final_content)
            .await?;

        // Update storage location with encryption status
        storage_location.encrypted = is_encrypted;

        // Create indexed metadata
        let indexed_metadata = IndexedFactMetadata {
            fact_id: fact.fact_id,
            version: fact.version,
            author: fact.author.clone(),
            created_at: fact.created_at,
            content_size: content_size as u64,
            content_type: fact.content.content_type().to_string(),
            category: format!("{:?}", fact.metadata.category),
            domain: format!("{:?}", fact.metadata.domain),
            tags: fact.metadata.tags.clone(),
            verification_level: format!("{:?}", fact.metadata.verification_level),
            confidence_score: fact.confidence_score,
            storage_location,
            access_policy_hash: self.hash_access_policy(&fact.access_policy)?,
        };

        // Update index
        self.update_index(&fact_clone, &indexed_metadata).await?;

        // Store metadata in database
        self.store_fact_metadata(&fact_clone, &indexed_metadata)
            .await?;

        self.persist_access_policy_sidecar(fact.fact_id, &fact.access_policy)
            .await?;

        info!("Successfully stored fact: {:?}", fact.fact_id);
        Ok(fact.fact_id)
    }

    /// Retrieve a fact package by ID
    pub async fn retrieve_fact(&self, fact_id: FactID) -> Result<Option<FactPackage>> {
        debug!("Retrieving fact: {:?}", fact_id);

        // Get metadata from index
        let metadata = {
            let index = self.fact_index.read().await;
            index.fact_metadata.get(&fact_id).cloned()
        };

        let metadata = match metadata {
            Some(meta) => meta,
            None => {
                // Try loading from database if not in index
                match self.load_fact_metadata_from_db(fact_id).await? {
                    Some(meta) => meta,
                    None => return Ok(None),
                }
            }
        };

        // Retrieve content from storage
        let content_bytes = self
            .content_storage
            .retrieve_content(&metadata.storage_location)
            .await?;

        // Decrypt if needed
        let decrypted_content = if metadata.storage_location.encrypted {
            self.decrypt_fact_content(&content_bytes, fact_id).await?
        } else {
            content_bytes
        };

        // Decompress if needed
        let decompressed_content = if metadata.storage_location.compressed {
            self.decompress_content(&decrypted_content)?
        } else {
            decrypted_content
        };

        // Deserialize fact content
        let fact_content = self.deserialize_fact_content(&decompressed_content)?;

        // Reconstruct full fact package from database
        let fact_package = self.reconstruct_fact_package(fact_id, fact_content).await?;

        debug!("Successfully retrieved fact: {:?}", fact_id);
        Ok(Some(fact_package))
    }

    /// Query facts based on criteria
    pub async fn query_facts(&self, query: FactQuery) -> Result<FactQueryResult> {
        debug!("Executing fact query: {:?}", query.requester);

        let start_time = std::time::Instant::now();

        // Generate cache key for this query
        let cache_key = self.generate_query_cache_key(&query)?;

        // Check cache first (with TTL validation)
        let cached_result = self.check_query_cache(&cache_key).await?;
        if let Some(mut cached_result) = cached_result {
            debug!("Query cache hit for requester: {:?}", query.requester);

            // Update query metadata to reflect cache hit
            cached_result.query_metadata.cache_hit = true;
            cached_result.query_metadata.execution_time_ms =
                start_time.elapsed().as_millis() as u64;

            return Ok(cached_result);
        }

        debug!("Query cache miss - executing full query");

        let start_time = std::time::Instant::now();
        let mut matching_facts = HashSet::new();

        // Execute query against index
        {
            let index = self.fact_index.read().await;

            // Apply filters
            if let Some(author) = &query.author {
                if let Some(author_facts) = index.by_author.get(author) {
                    if matching_facts.is_empty() {
                        matching_facts = author_facts.clone();
                    } else {
                        matching_facts =
                            matching_facts.intersection(author_facts).cloned().collect();
                    }
                }
            }

            if let Some(category) = &query.category {
                let category_str = format!("{:?}", category);
                if let Some(category_facts) = index.by_category.get(&category_str) {
                    if matching_facts.is_empty() {
                        matching_facts = category_facts.clone();
                    } else {
                        matching_facts = matching_facts
                            .intersection(category_facts)
                            .cloned()
                            .collect();
                    }
                }
            }

            // Apply tag filters
            for tag in &query.tags {
                if let Some(tag_facts) = index.by_tags.get(tag) {
                    if matching_facts.is_empty() {
                        matching_facts = tag_facts.clone();
                    } else {
                        matching_facts = matching_facts.intersection(tag_facts).cloned().collect();
                    }
                }
            }
        }

        // Convert to fact IDs and apply additional filters
        let mut result_facts = Vec::new();
        for fact_id in matching_facts {
            match self.retrieve_fact(fact_id).await {
                Ok(Some(fact)) => {
                    if self
                        .check_access_permission(&fact, &query.requester)
                        .await?
                    {
                        result_facts.push(fact);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!("Skipping fact {:?} in query: {}", fact_id, e);
                }
            }
        }

        // Apply sorting and pagination
        self.sort_facts(&mut result_facts, &query.sort_by).await?;
        let total_count = result_facts.len();

        let start_idx = query.pagination.offset as usize;
        let end_idx = std::cmp::min(
            start_idx + query.pagination.limit as usize,
            result_facts.len(),
        );

        let paginated_facts = if start_idx < result_facts.len() {
            result_facts[start_idx..end_idx].to_vec()
        } else {
            Vec::new()
        };

        let execution_time = start_time.elapsed().as_millis() as u64;

        let query_result = FactQueryResult {
            facts: paginated_facts,
            total_count,
            query_metadata: spacekit_primitives::v1::fact::types::QueryMetadata {
                execution_time_ms: execution_time,
                filters_applied: self.count_applied_filters(&query),
                cache_hit: false,
            },
        };

        // Cache the result for future queries (with 5 minute TTL)
        self.cache_query_result(&cache_key, &query_result).await?;

        debug!("Query executed and cached in {}ms", execution_time);
        Ok(query_result)
    }

    /// Verify a fact package
    pub async fn verify_fact(&self, fact_id: FactID) -> Result<VerificationResult> {
        // Check verification cache first
        {
            let cache = self.verification_cache.read().await;
            if let Some(cached_result) = cache.get(&fact_id) {
                return Ok(cached_result.clone());
            }
        }

        // Retrieve fact for verification
        let fact = self
            .retrieve_fact(fact_id)
            .await?
            .ok_or_else(|| anyhow!("Fact not found: {:?}", fact_id))?;

        // Perform verification
        let verification_result = self.perform_fact_verification(&fact).await?;

        // Cache result
        {
            let mut cache = self.verification_cache.write().await;
            cache.insert(fact_id, verification_result.clone());
        }

        Ok(verification_result)
    }

    // Private helper methods

    async fn rebuild_index(&self) -> Result<()> {
        info!("Rebuilding fact index from database...");

        // Load all fact metadata from database
        let database = self.database.clone();
        let all_metadata = tokio::task::spawn_blocking(move || database.select_all_fact_metadata())
            .await
            .map_err(|e| anyhow!("Database task join error: {}", e))??;

        info!(
            "Loaded {} fact metadata records from database",
            all_metadata.len()
        );

        // Clear and rebuild the index
        {
            let mut index = self.fact_index.write().await;

            // Clear existing index
            index.fact_metadata.clear();
            index.by_author.clear();
            index.by_category.clear();
            index.by_domain.clear();
            index.by_tags.clear();
            index.dependencies.clear();
            index.dependents.clear();

            // Rebuild index from database records
            for record in all_metadata {
                // Convert database record to indexed metadata
                let indexed_metadata = self.convert_from_db_record(record.clone())?;
                let fact_id = indexed_metadata.fact_id;

                // Add to main metadata map
                index
                    .fact_metadata
                    .insert(fact_id, indexed_metadata.clone());

                // Update author index
                index
                    .by_author
                    .entry(indexed_metadata.author.clone())
                    .or_insert_with(HashSet::new)
                    .insert(fact_id);

                // Update category index
                index
                    .by_category
                    .entry(indexed_metadata.category.clone())
                    .or_insert_with(HashSet::new)
                    .insert(fact_id);

                // Update domain index
                index
                    .by_domain
                    .entry(indexed_metadata.domain.clone())
                    .or_insert_with(HashSet::new)
                    .insert(fact_id);

                // Update tag indices
                for tag in &indexed_metadata.tags {
                    index
                        .by_tags
                        .entry(tag.clone())
                        .or_insert_with(HashSet::new)
                        .insert(fact_id);
                }

                // Update dependency graph
                for dep_str in &record.dependencies {
                    if let Ok(dep_bytes) = hex::decode(dep_str) {
                        if let Ok(dep_id) = dep_bytes.try_into() {
                            index
                                .dependencies
                                .entry(fact_id)
                                .or_insert_with(HashSet::new)
                                .insert(dep_id);

                            index
                                .dependents
                                .entry(dep_id)
                                .or_insert_with(HashSet::new)
                                .insert(fact_id);
                        }
                    }
                }
            }

            info!("Index rebuilt successfully:");
            info!("  - {} facts indexed", index.fact_metadata.len());
            info!("  - {} authors indexed", index.by_author.len());
            info!("  - {} categories indexed", index.by_category.len());
            info!("  - {} domains indexed", index.by_domain.len());
            info!("  - {} tags indexed", index.by_tags.len());
            info!("  - {} dependency relationships", index.dependencies.len());
        }

        Ok(())
    }

    fn estimate_fact_size(&self, fact: &FactPackage) -> usize {
        // Get actual serialized size
        bincode::serde::encode_to_vec(fact, bincode::config::standard())
            .map(|data| data.len())
            .unwrap_or(0)
    }

    fn serialize_fact_content(&self, content: &FactContent) -> Result<Vec<u8>> {
        encode_fact_content(content)
    }

    fn deserialize_fact_content(&self, data: &[u8]) -> Result<FactContent> {
        decode_fact_content(data)
    }

    fn compress_content(&self, data: &[u8]) -> Result<Vec<u8>> {
        compress_with_algorithm(data, &self.config.compression_algorithm)
    }

    fn decompress_content(&self, data: &[u8]) -> Result<Vec<u8>> {
        decompress_with_algorithm(data, &self.config.compression_algorithm)
    }

    fn hash_access_policy(&self, policy: &AccessPolicy) -> Result<[u8; 32]> {
        use sha2::{Digest, Sha256};
        let serialized = bincode::serde::encode_to_vec(policy, bincode::config::standard())?;
        let hash = Sha256::digest(&serialized);
        Ok(hash.into())
    }

    async fn update_index(&self, fact: &FactPackage, metadata: &IndexedFactMetadata) -> Result<()> {
        let mut index = self.fact_index.write().await;

        // Update main metadata map
        index.fact_metadata.insert(fact.fact_id, metadata.clone());

        // Update author index
        index
            .by_author
            .entry(fact.author.clone())
            .or_insert_with(HashSet::new)
            .insert(fact.fact_id);

        // Update category index
        index
            .by_category
            .entry(metadata.category.clone())
            .or_insert_with(HashSet::new)
            .insert(fact.fact_id);

        // Update domain index
        index
            .by_domain
            .entry(metadata.domain.clone())
            .or_insert_with(HashSet::new)
            .insert(fact.fact_id);

        // Update tag indices
        for tag in &metadata.tags {
            index
                .by_tags
                .entry(tag.clone())
                .or_insert_with(HashSet::new)
                .insert(fact.fact_id);
        }

        // Update dependency graph
        for dep in &fact.dependencies {
            index
                .dependencies
                .entry(fact.fact_id)
                .or_insert_with(HashSet::new)
                .insert(*dep);
            index
                .dependents
                .entry(*dep)
                .or_insert_with(HashSet::new)
                .insert(fact.fact_id);
        }

        Ok(())
    }

    async fn store_fact_metadata(
        &self,
        fact: &FactPackage,
        metadata: &IndexedFactMetadata,
    ) -> Result<()> {
        // Convert IndexedFactMetadata to FactMetadataRecord for database storage
        let record = self.convert_to_db_record(fact, metadata)?;

        // Store in database using tokio spawn to avoid blocking
        let database = self.database.clone();
        tokio::task::spawn_blocking(move || database.insert_fact_metadata(&record))
            .await
            .map_err(|e| anyhow!("Database task join error: {}", e))??;

        info!("Fact metadata stored in database: {:?}", metadata.fact_id);
        Ok(())
    }

    async fn load_fact_metadata_from_db(
        &self,
        fact_id: FactID,
    ) -> Result<Option<IndexedFactMetadata>> {
        // Convert FactID to string for database lookup
        let fact_id_str = hex::encode(fact_id);

        // Load from database using tokio spawn to avoid blocking
        let database = self.database.clone();
        let result = tokio::task::spawn_blocking(move || database.get_fact_metadata(&fact_id_str))
            .await
            .map_err(|e| anyhow!("Database task join error: {}", e))??;

        match result {
            Some(record) => {
                let indexed_metadata = self.convert_from_db_record(record)?;
                Ok(Some(indexed_metadata))
            }
            None => Ok(None),
        }
    }

    async fn reconstruct_fact_package(
        &self,
        fact_id: FactID,
        content: FactContent,
    ) -> Result<FactPackage> {
        // Load metadata from database
        let fact_id_str = hex::encode(fact_id);
        let database = self.database.clone();
        let db_record =
            tokio::task::spawn_blocking(move || database.get_fact_metadata(&fact_id_str))
                .await
                .map_err(|e| anyhow!("Database task join error: {}", e))??
                .ok_or_else(|| {
                    anyhow!(
                        "Fact metadata not found in database: {:?}",
                        hex::encode(fact_id)
                    )
                })?;

        // Parse dependencies from hex strings
        let dependencies: Vec<FactID> = db_record
            .dependencies
            .iter()
            .filter_map(|dep_str| {
                hex::decode(dep_str)
                    .ok()
                    .and_then(|bytes| bytes.try_into().ok())
            })
            .collect();

        // Parse author from JSON string
        let author: QuantumDID = serde_json::from_str(&db_record.author)
            .map_err(|e| anyhow!("Failed to parse author QuantumDID: {}", e))?;

        // Create metadata from database record
        let metadata = FactMetadata {
            category: self.parse_fact_category(&db_record.category)?,
            tags: db_record.tags.clone(),
            domain: self.parse_knowledge_domain(&db_record.domain)?,
            source: self.create_default_data_source(&author),
            collection_method: self.parse_collection_method(),
            verification_level: self.parse_verification_level(&db_record.verification_level)?,
            license: self.create_default_license(),
            size_bytes: db_record.content_size,
            checksum: hex::decode(&db_record.checksum)
                .map_err(|e| anyhow!("Failed to decode checksum: {}", e))?
                .try_into()
                .map_err(|_| anyhow!("Invalid checksum length"))?,
        };

        let access_policy = self
            .load_access_policy_sidecar(fact_id)
            .unwrap_or_else(|| Self::access_policy_from_db_record(&db_record));

        // This database schema currently does not persist SPHINCS+ signatures/public keys.
        // Use an empty signature so verification correctly reports signature_valid=false.
        let signature = SPHINCSSignature::new(Vec::new(), "SPHINCS-256f".to_string(), Vec::new());

        let verification_proof = VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: vec![0u8; 32], // Placeholder proof
            verification_timestamp: db_record.created_at.timestamp() as u64,
            verifier: Some(author.clone()),
        };

        // Reconstruct the complete FactPackage
        let fact_package = FactPackage {
            // Core identification
            fact_id,
            version: db_record.version,
            created_at: db_record.created_at.timestamp() as u64,
            expires_at: None, // Not stored in current schema

            // Content
            content,
            metadata,

            // Verification
            author,
            signature,
            verification_proof,

            // Relationships
            dependencies,
            citations: Vec::new(), // Not stored in current schema
            confidence_score: db_record.confidence_score,

            // Access control
            access_policy,
            encryption: None, // Not implemented yet
        };

        debug!(
            "Successfully reconstructed fact package: {:?}",
            hex::encode(fact_id)
        );
        Ok(fact_package)
    }

    async fn check_access_permission(
        &self,
        fact: &FactPackage,
        requester: &QuantumDID,
    ) -> Result<bool> {
        debug!(
            "Checking access permission for fact {:?} by requester {:?}",
            hex::encode(fact.fact_id),
            requester
        );

        if &fact.author == requester {
            return Ok(true);
        }

        // Check if fact has expired
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        if fact.is_expired(current_time) {
            info!("Access denied: Fact has expired");
            return Ok(false);
        }

        // Evaluate access policy
        match &fact.access_policy {
            AccessPolicy::Public => {
                debug!("Public access policy - allowing access");
                Ok(true)
            }

            AccessPolicy::Private(authorized_users) => {
                let is_authorized = authorized_users.contains(requester);
                debug!("Private access policy - authorized: {}", is_authorized);
                Ok(is_authorized)
            }

            AccessPolicy::RoleBased(required_roles) => {
                // Check if requester has required roles
                let user_roles = self.get_user_roles(requester).await?;
                let has_required_role = required_roles.iter().any(|role| user_roles.contains(role));
                debug!(
                    "Role-based access policy - has required role: {}",
                    has_required_role
                );
                Ok(has_required_role)
            }

            AccessPolicy::AttributeBased(requirements) => {
                self.check_attribute_requirements(requester, requirements)
                    .await
            }

            AccessPolicy::Dynamic(policy_id) => {
                // Dynamic policy evaluation (placeholder for external policy engine)
                warn!(
                    "Dynamic policy {} evaluation not yet implemented - denying access",
                    policy_id
                );
                Ok(false)
            }

            AccessPolicy::Conditional(conditions) => {
                self.evaluate_access_conditions(requester, conditions).await
            }
        }
    }

    async fn sort_facts(
        &self,
        facts: &mut Vec<FactPackage>,
        sort_criteria: &spacekit_primitives::v1::fact::types::SortCriteria,
    ) -> Result<()> {
        use spacekit_primitives::v1::fact::types::{SortCriteria, SortOrder};

        debug!("Sorting {} facts by {:?}", facts.len(), sort_criteria);

        match sort_criteria {
            SortCriteria::CreatedAt(order) => match order {
                SortOrder::Ascending => {
                    facts.sort_by(|a, b| a.created_at.cmp(&b.created_at));
                }
                SortOrder::Descending => {
                    facts.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                }
            },

            SortCriteria::Confidence(order) => match order {
                SortOrder::Ascending => {
                    facts.sort_by(|a, b| {
                        a.confidence_score
                            .partial_cmp(&b.confidence_score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
                SortOrder::Descending => {
                    facts.sort_by(|a, b| {
                        b.confidence_score
                            .partial_cmp(&a.confidence_score)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                }
            },

            SortCriteria::AuthorReputation(order) => {
                // Calculate author reputation scores for sorting
                let mut author_scores = HashMap::new();
                for fact in facts.iter() {
                    if !author_scores.contains_key(&fact.author) {
                        let reputation =
                            self.get_user_reputation(&fact.author).await.unwrap_or(0.0);
                        author_scores.insert(fact.author.clone(), reputation);
                    }
                }

                match order {
                    SortOrder::Ascending => {
                        facts.sort_by(|a, b| {
                            let rep_a = author_scores.get(&a.author).unwrap_or(&0.0);
                            let rep_b = author_scores.get(&b.author).unwrap_or(&0.0);
                            rep_a
                                .partial_cmp(rep_b)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                    SortOrder::Descending => {
                        facts.sort_by(|a, b| {
                            let rep_a = author_scores.get(&a.author).unwrap_or(&0.0);
                            let rep_b = author_scores.get(&b.author).unwrap_or(&0.0);
                            rep_b
                                .partial_cmp(rep_a)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                }
            }

            SortCriteria::Relevance(order) => {
                // For relevance, we'll use a combination of confidence score and verification level
                let relevance_score = |fact: &FactPackage| -> f64 {
                    let verification_weight =
                        self.get_verification_level_weight(&fact.metadata.verification_level);
                    let base_score = fact.confidence_score * 0.7 + verification_weight * 0.3;

                    // Boost score for recent facts (recency relevance)
                    let current_time = SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let age_days = (current_time - fact.created_at) / (24 * 60 * 60);
                    let recency_boost = if age_days < 7 {
                        0.1
                    } else if age_days < 30 {
                        0.05
                    } else {
                        0.0
                    };

                    base_score + recency_boost
                };

                match order {
                    SortOrder::Ascending => {
                        facts.sort_by(|a, b| {
                            relevance_score(a)
                                .partial_cmp(&relevance_score(b))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                    SortOrder::Descending => {
                        facts.sort_by(|a, b| {
                            relevance_score(b)
                                .partial_cmp(&relevance_score(a))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                }
            }

            SortCriteria::Usage(order) => {
                // For usage sorting, we'll use a placeholder based on access frequency
                // In production, this would track actual access statistics
                let usage_score = |fact: &FactPackage| -> f64 {
                    // Placeholder: use confidence score as proxy for usage
                    // Higher confidence facts are likely to be accessed more
                    let base_usage = fact.confidence_score;

                    // Add verification level bonus (authoritative facts get more usage)
                    let verification_bonus = match fact.metadata.verification_level {
                        spacekit_primitives::v1::fact::VerificationLevel::Authoritative => 0.2,
                        spacekit_primitives::v1::fact::VerificationLevel::Cryptographic => 0.15,
                        spacekit_primitives::v1::fact::VerificationLevel::Consensus => 0.1,
                        spacekit_primitives::v1::fact::VerificationLevel::PeerReviewed => 0.05,
                        _ => 0.0,
                    };

                    base_usage + verification_bonus
                };

                match order {
                    SortOrder::Ascending => {
                        facts.sort_by(|a, b| {
                            usage_score(a)
                                .partial_cmp(&usage_score(b))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                    SortOrder::Descending => {
                        facts.sort_by(|a, b| {
                            usage_score(b)
                                .partial_cmp(&usage_score(a))
                                .unwrap_or(std::cmp::Ordering::Equal)
                        });
                    }
                }
            }
        }

        debug!("Facts sorted successfully by {:?}", sort_criteria);
        Ok(())
    }

    fn count_applied_filters(&self, query: &FactQuery) -> u32 {
        let mut count = 0;
        if query.author.is_some() {
            count += 1;
        }
        if query.category.is_some() {
            count += 1;
        }
        if !query.tags.is_empty() {
            count += 1;
        }
        if query.domain.is_some() {
            count += 1;
        }
        count
    }

    async fn perform_fact_verification(&self, fact: &FactPackage) -> Result<VerificationResult> {
        info!(
            "Performing quantum-safe verification for fact {:?}",
            hex::encode(fact.fact_id)
        );

        // 1. Verify SPHINCS+ signature
        let signature_valid = self.verify_quantum_signature(fact).await?;
        debug!("Signature verification result: {}", signature_valid);

        // 2. Verify author identity
        let author_verified = self.verify_author_identity(fact).await?;
        debug!("Author verification result: {}", author_verified);

        // 3. Verify content integrity
        let content_integrity = self.verify_content_integrity(fact).await?;
        debug!(
            "Content integrity verification result: {}",
            content_integrity
        );

        // 4. Verify dependencies
        let dependency_verification = self.verify_fact_dependencies(fact).await?;
        debug!(
            "Dependency verification: {}/{} verified",
            dependency_verification.verified_count, dependency_verification.total_count
        );

        // 5. Calculate trust score based on author reputation
        let trust_score = self.calculate_trust_score(fact).await?;
        debug!("Calculated trust score: {}", trust_score);

        // 6. Calculate overall confidence
        let overall_confidence = self
            .calculate_overall_confidence(
                signature_valid,
                author_verified,
                content_integrity,
                &dependency_verification,
                trust_score,
            )
            .await?;

        let verification_result = VerificationResult {
            signature_valid,
            author_verified,
            trust_score,
            dependency_verification,
            overall_confidence,
        };

        info!(
            "Verification complete for fact {:?}: overall_confidence={}",
            hex::encode(fact.fact_id),
            overall_confidence
        );

        Ok(verification_result)
    }

    // Conversion methods between IndexedFactMetadata and FactMetadataRecord

    fn convert_to_db_record(
        &self,
        fact: &FactPackage,
        metadata: &IndexedFactMetadata,
    ) -> Result<FactMetadataRecord> {
        Ok(FactMetadataRecord {
            fact_id: hex::encode(metadata.fact_id),
            version: metadata.version,
            author: serde_json::to_string(&metadata.author)
                .map_err(|e| anyhow!("Failed to serialize author: {}", e))?,
            created_at: chrono::DateTime::from_timestamp(metadata.created_at as i64, 0)
                .unwrap_or_else(|| chrono::Utc::now()),
            content_size: metadata.content_size,
            content_type: metadata.content_type.clone(),
            category: metadata.category.clone(),
            domain: metadata.domain.clone(),
            tags: metadata.tags.clone(),
            verification_level: metadata.verification_level.clone(),
            confidence_score: metadata.confidence_score,
            storage_location_path: metadata.storage_location.path.to_string_lossy().to_string(),
            storage_tier: format!("{:?}", metadata.storage_location.tier),
            compressed: metadata.storage_location.compressed,
            encrypted: metadata.storage_location.encrypted,
            checksum: hex::encode(metadata.storage_location.checksum),
            access_policy_hash: hex::encode(metadata.access_policy_hash),
            access_policy_json: Some(
                serde_json::to_string(&fact.access_policy)
                    .map_err(|e| anyhow!("Failed to serialize access_policy: {}", e))?,
            ),
            dependencies: fact.dependencies.iter().map(|id| hex::encode(id)).collect(),
            last_accessed: None,
        })
    }

    fn access_policy_from_db_record(record: &FactMetadataRecord) -> AccessPolicy {
        if let Some(ref json) = record.access_policy_json {
            if let Ok(policy) = serde_json::from_str::<AccessPolicy>(json) {
                return policy;
            }
        }
        AccessPolicy::Public
    }

    fn access_policy_sidecar_path(&self, fact_id: FactID) -> PathBuf {
        self.config
            .storage_dir
            .join("policies")
            .join(format!("{}.json", hex::encode(fact_id)))
    }

    async fn persist_access_policy_sidecar(
        &self,
        fact_id: FactID,
        policy: &AccessPolicy,
    ) -> Result<()> {
        let path = self.access_policy_sidecar_path(fact_id);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_vec(policy)?;
        tokio::fs::write(&path, json).await?;
        Ok(())
    }

    fn load_access_policy_sidecar(&self, fact_id: FactID) -> Option<AccessPolicy> {
        let path = self.access_policy_sidecar_path(fact_id);
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn convert_from_db_record(&self, record: FactMetadataRecord) -> Result<IndexedFactMetadata> {
        Ok(IndexedFactMetadata {
            fact_id: hex::decode(&record.fact_id)
                .map_err(|e| anyhow!("Failed to decode fact_id: {}", e))?
                .try_into()
                .map_err(|_| anyhow!("Invalid fact_id length"))?,
            version: record.version,
            author: serde_json::from_str(&record.author)
                .map_err(|e| anyhow!("Failed to deserialize author: {}", e))?,
            created_at: record.created_at.timestamp() as u64,
            content_size: record.content_size,
            content_type: record.content_type,
            category: record.category,
            domain: record.domain,
            tags: record.tags,
            verification_level: record.verification_level,
            confidence_score: record.confidence_score,
            storage_location: StorageLocation {
                tier: serde_json::from_str(&format!("\"{}\"", record.storage_tier))
                    .map_err(|e| anyhow!("Failed to parse storage tier: {}", e))?,
                path: PathBuf::from(record.storage_location_path),
                compressed: record.compressed,
                encrypted: record.encrypted,
                checksum: hex::decode(&record.checksum)
                    .map_err(|e| anyhow!("Failed to decode checksum: {}", e))?
                    .try_into()
                    .map_err(|_| anyhow!("Invalid checksum length"))?,
            },
            access_policy_hash: hex::decode(&record.access_policy_hash)
                .map_err(|e| anyhow!("Failed to decode access policy hash: {}", e))?
                .try_into()
                .map_err(|_| anyhow!("Invalid access policy hash length"))?,
        })
    }

    // Helper methods for fact reconstruction

    fn parse_fact_category(&self, category: &str) -> Result<FactCategory> {
        match category {
            "Scientific" => Ok(FactCategory::Scientific),
            "Historical" => Ok(FactCategory::Historical),
            "Statistical" => Ok(FactCategory::Statistical),
            "Legal" => Ok(FactCategory::Legal),
            "Medical" => Ok(FactCategory::Medical),
            "Financial" => Ok(FactCategory::Financial),
            "Technical" => Ok(FactCategory::Technical),
            "Geographic" => Ok(FactCategory::Geographic),
            "Biographical" => Ok(FactCategory::Biographical),
            "Reference" => Ok(FactCategory::Reference),
            "Opinion" => Ok(FactCategory::Opinion),
            "Prediction" => Ok(FactCategory::Prediction),
            "Enterprise" => Ok(FactCategory::Enterprise),
            "UserGenerated" => Ok(FactCategory::UserGenerated),
            _ => Ok(FactCategory::UserGenerated), // Default fallback
        }
    }

    fn parse_knowledge_domain(&self, domain: &str) -> Result<KnowledgeDomain> {
        match domain {
            "Physics" => Ok(KnowledgeDomain::Physics),
            "Chemistry" => Ok(KnowledgeDomain::Chemistry),
            "Biology" => Ok(KnowledgeDomain::Biology),
            "Mathematics" => Ok(KnowledgeDomain::Mathematics),
            "ComputerScience" => Ok(KnowledgeDomain::ComputerScience),
            "Medicine" => Ok(KnowledgeDomain::Medicine),
            "Law" => Ok(KnowledgeDomain::Law),
            "Economics" => Ok(KnowledgeDomain::Economics),
            "History" => Ok(KnowledgeDomain::History),
            "Geography" => Ok(KnowledgeDomain::Geography),
            "Engineering" => Ok(KnowledgeDomain::Engineering),
            "Philosophy" => Ok(KnowledgeDomain::Philosophy),
            other => Ok(KnowledgeDomain::Custom(other.to_string())),
        }
    }

    fn create_default_data_source(&self, author: &QuantumDID) -> DataSource {
        DataSource::UserInput {
            application: author.clone(),
            user: author.clone(),
        }
    }

    fn parse_collection_method(&self) -> CollectionMethod {
        CollectionMethod::Manual // Default for reconstructed facts
    }

    fn parse_verification_level(&self, level: &str) -> Result<VerificationLevel> {
        match level {
            "Unverified" => Ok(VerificationLevel::Unverified),
            "SelfClaimed" => Ok(VerificationLevel::SelfClaimed),
            "PeerReviewed" => Ok(VerificationLevel::PeerReviewed),
            "Consensus" => Ok(VerificationLevel::Consensus),
            "Authoritative" => Ok(VerificationLevel::Authoritative),
            "Cryptographic" => Ok(VerificationLevel::Cryptographic),
            _ => Ok(VerificationLevel::Unverified), // Default fallback
        }
    }

    fn create_default_license(&self) -> LicenseType {
        LicenseType::Proprietary // Default license for reconstructed facts
    }

    fn create_default_access_policy(&self) -> AccessPolicy {
        AccessPolicy::Public // Default access policy for reconstructed facts
    }

    // Access control helper methods

    async fn get_user_roles(&self, user: &QuantumDID) -> Result<HashSet<String>> {
        // TODO: Integrate with user management system
        // For now, return default roles based on user type
        debug!("Getting user roles for: {:?}", user);

        // Placeholder implementation - in production, this would query:
        // - Database for stored user roles
        // - External identity provider
        // - Blockchain-based role registry
        let mut roles = HashSet::new();

        // Default role assignment logic (placeholder)
        roles.insert("user".to_string());

        // Check if user is fact author (higher permissions)
        // This would be more sophisticated in production
        roles.insert("reader".to_string());

        Ok(roles)
    }

    async fn check_attribute_requirements(
        &self,
        requester: &QuantumDID,
        requirements: &AttributeRequirements,
    ) -> Result<bool> {
        debug!("Checking attribute requirements for: {:?}", requester);

        // Check minimum trust score
        if let Some(min_trust) = requirements.minimum_trust_score {
            let user_trust_score = self.get_user_trust_score(requester).await?;
            if user_trust_score < min_trust {
                debug!(
                    "Access denied: Trust score {} below minimum {}",
                    user_trust_score, min_trust
                );
                return Ok(false);
            }
        }

        // Check domain expertise
        if let Some(required_domain) = &requirements.domain_expertise {
            let user_expertise = self.get_user_domain_expertise(requester).await?;
            if !user_expertise.contains(required_domain)
                && !matches!(required_domain, KnowledgeDomain::Custom(_))
            {
                debug!(
                    "Access denied: Missing domain expertise in {:?}",
                    required_domain
                );
                return Ok(false);
            }
        }

        // Check required attributes
        for (attr_name, attr_value) in &requirements.required_attributes {
            let user_attr_value = self.get_user_attribute(requester, attr_name).await?;
            if user_attr_value.as_deref() != Some(attr_value) {
                debug!(
                    "Access denied: Missing required attribute {}={}",
                    attr_name, attr_value
                );
                return Ok(false);
            }
        }

        debug!("All attribute requirements satisfied");
        Ok(true)
    }

    async fn evaluate_access_conditions(
        &self,
        requester: &QuantumDID,
        conditions: &[AccessCondition],
    ) -> Result<bool> {
        debug!(
            "Evaluating {} access conditions for: {:?}",
            conditions.len(),
            requester
        );

        for condition in conditions {
            if self.evaluate_single_condition(requester, condition).await? {
                debug!("Access condition satisfied: {:?}", condition.condition_type);
                return Ok(true);
            }
        }

        debug!("No access condition satisfied");
        Ok(false)
    }

    async fn evaluate_single_condition(
        &self,
        requester: &QuantumDID,
        condition: &AccessCondition,
    ) -> Result<bool> {
        match &condition.condition_type {
            ConditionType::TimeWindow => {
                self.check_time_window_condition(&condition.parameters)
                    .await
            }

            ConditionType::TrustLevel => {
                if condition
                    .parameters
                    .get("subscription_required")
                    .map(|s| s == "true")
                    .unwrap_or(false)
                {
                    let channel = condition
                        .parameters
                        .get("channel_id")
                        .map(String::as_str)
                        .unwrap_or("");
                    let grants = crate::content_grants::ContentGrantStore::from_env_or_data_dir(
                        self.content_grants_data_dir(),
                    );
                    return Ok(grants.has_channel_subscription(requester.as_str(), channel));
                }
                if let Some(min_trust_str) = condition.parameters.get("minimum_trust") {
                    if let Ok(min_trust) = min_trust_str.parse::<f64>() {
                        let user_trust = self.get_user_trust_score(requester).await?;
                        return Ok(user_trust >= min_trust);
                    }
                }
                Ok(false)
            }

            ConditionType::ReputationThreshold => {
                if let Some(min_rep_str) = condition.parameters.get("minimum_reputation") {
                    if let Ok(min_rep) = min_rep_str.parse::<f64>() {
                        let user_rep = self.get_user_reputation(requester).await?;
                        return Ok(user_rep >= min_rep);
                    }
                }
                Ok(false)
            }

            ConditionType::PaymentRequired => {
                let content_id = condition
                    .parameters
                    .get("content_id")
                    .map(String::as_str)
                    .unwrap_or("");
                let grants = crate::content_grants::ContentGrantStore::from_env_or_data_dir(
                    self.content_grants_data_dir(),
                );
                Ok(grants.has_content_grant(requester.as_str(), content_id))
            }

            ConditionType::LocationBased => {
                // Check geographic restrictions (placeholder)
                warn!("Location-based access control not yet implemented");
                Ok(true) // Allow for now
            }

            ConditionType::DeviceType => {
                // Check device type restrictions (placeholder)
                warn!("Device-based access control not yet implemented");
                Ok(true) // Allow for now
            }

            ConditionType::NetworkCondition => {
                // Check network conditions (placeholder)
                warn!("Network-based access control not yet implemented");
                Ok(true) // Allow for now
            }

            ConditionType::MultiFactor => {
                // Check multi-factor authentication (placeholder)
                warn!("Multi-factor access control not yet implemented");
                Ok(false)
            }
        }
    }

    // User attribute lookup methods (placeholders for external systems)

    async fn get_user_trust_score(&self, user: &QuantumDID) -> Result<f64> {
        // TODO: Integrate with reputation system
        debug!("Getting trust score for: {:?}", user);
        Ok(0.8) // Default trust score
    }

    async fn get_user_reputation(&self, user: &QuantumDID) -> Result<f64> {
        // TODO: Integrate with reputation system
        debug!("Getting reputation for: {:?}", user);
        Ok(0.7) // Default reputation
    }

    async fn get_user_domain_expertise(
        &self,
        user: &QuantumDID,
    ) -> Result<HashSet<KnowledgeDomain>> {
        // TODO: Integrate with user profile system
        debug!("Getting domain expertise for: {:?}", user);
        let mut expertise = HashSet::new();
        expertise.insert(KnowledgeDomain::ComputerScience); // Default expertise
        Ok(expertise)
    }

    async fn get_user_attribute(
        &self,
        user: &QuantumDID,
        attribute: &str,
    ) -> Result<Option<String>> {
        // TODO: Integrate with user profile system
        debug!("Getting attribute '{}' for: {:?}", attribute, user);
        Ok(None) // No attributes by default
    }

    async fn check_time_window_condition(
        &self,
        parameters: &HashMap<String, String>,
    ) -> Result<bool> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        // Check if current time is within allowed window
        if let (Some(start_str), Some(end_str)) = (parameters.get("start"), parameters.get("end")) {
            if let (Ok(start), Ok(end)) = (start_str.parse::<u64>(), end_str.parse::<u64>()) {
                return Ok(current_time >= start && current_time <= end);
            }
        }

        // If no valid time window specified, allow access
        Ok(true)
    }

    // Quantum verification helper methods

    async fn verify_quantum_signature(&self, fact: &FactPackage) -> Result<bool> {
        debug!(
            "Verifying SPHINCS+ signature for fact {:?}",
            hex::encode(fact.fact_id)
        );

        // Check if signature is present and valid format
        if fact.signature.signature_bytes.is_empty() {
            warn!("No signature found for fact");
            return Ok(false);
        }

        // Verify signature algorithm is quantum-safe
        if !self.is_quantum_safe_algorithm(&fact.signature.algorithm) {
            warn!(
                "Non-quantum-safe signature algorithm: {}",
                fact.signature.algorithm
            );
            return Ok(false);
        }

        // Bind signature to DID: ensure the signature public key matches the DID registry key.
        let did_str = fact.author.as_ref();
        match self.database.get_global_user(did_str) {
            Ok(Some(user)) => {
                if user.public_key != fact.signature.public_key {
                    warn!(
                        "Signature public key does not match DID registry for {}",
                        did_str
                    );
                    return Ok(false);
                }
            }
            Ok(None) => {
                warn!(
                    "No DID registry entry for {}; cannot verify key binding",
                    did_str
                );
                return Ok(false);
            }
            Err(e) => {
                warn!("Failed to resolve DID {}: {}", did_str, e);
                return Ok(false);
            }
        }

        // Create message to verify (fact content + metadata hash)
        let message_to_verify = self.create_verification_message(fact)?;

        // Verify signature using quantum crypto service
        let verification_result = self
            .quantum_crypto
            .verify_signature(&message_to_verify, &fact.signature, &fact.author)
            .await
            .unwrap_or(false);

        debug!("SPHINCS+ signature verification: {}", verification_result);
        Ok(verification_result)
    }

    async fn verify_author_identity(&self, fact: &FactPackage) -> Result<bool> {
        debug!("Verifying author identity for: {:?}", fact.author);

        // Check if author's DID is valid and active
        let did_valid = self.verify_quantum_did(&fact.author).await?;

        // Check if author has necessary credentials
        let credentials_valid = self
            .verify_author_credentials(&fact.author, &fact.metadata)
            .await?;

        // Check if author is authorized to publish in this domain
        let domain_authorized = self
            .verify_domain_authorization(&fact.author, &fact.metadata.domain)
            .await?;

        let author_verified = did_valid && credentials_valid && domain_authorized;
        debug!(
            "Author verification: DID={}, credentials={}, domain={}, overall={}",
            did_valid, credentials_valid, domain_authorized, author_verified
        );

        Ok(author_verified)
    }

    async fn verify_content_integrity(&self, fact: &FactPackage) -> Result<bool> {
        debug!(
            "Verifying content integrity for fact {:?}",
            hex::encode(fact.fact_id)
        );

        // Verify content hash matches metadata
        let computed_hash = self.compute_content_hash(&fact.content)?;
        let stored_hash = fact.metadata.checksum;

        if computed_hash != stored_hash {
            warn!(
                "Content hash mismatch: computed={}, stored={}",
                hex::encode(computed_hash),
                hex::encode(stored_hash)
            );
            return Ok(false);
        }

        // Verify content structure is valid
        let structure_valid = self.verify_content_structure(&fact.content).await?;

        // Verify content meets size requirements
        let size_valid = fact.metadata.size_bytes <= self.config.max_fact_size;

        let integrity_valid = structure_valid && size_valid;
        debug!(
            "Content integrity: hash_match=true, structure={}, size={}, overall={}",
            structure_valid, size_valid, integrity_valid
        );

        Ok(integrity_valid)
    }

    async fn verify_fact_dependencies(
        &self,
        fact: &FactPackage,
    ) -> Result<spacekit_primitives::v1::fact::types::DependencyVerification> {
        let total_count = fact.dependencies.len();
        let mut verified_count = 0;
        let mut failed_dependencies = Vec::new();

        debug!(
            "Verifying {} dependencies for fact {:?}",
            total_count,
            hex::encode(fact.fact_id)
        );

        for dependency_id in &fact.dependencies {
            match self.verify_single_dependency(*dependency_id).await {
                Ok(true) => {
                    verified_count += 1;
                    debug!(
                        "Dependency {:?} verified successfully",
                        hex::encode(dependency_id)
                    );
                }
                Ok(false) => {
                    debug!(
                        "Dependency {:?} verification failed",
                        hex::encode(dependency_id)
                    );
                    failed_dependencies.push(*dependency_id);
                }
                Err(e) => {
                    warn!(
                        "Error verifying dependency {:?}: {}",
                        hex::encode(dependency_id),
                        e
                    );
                    failed_dependencies.push(*dependency_id);
                }
            }
        }

        let all_dependencies_verified = verified_count == total_count;
        debug!(
            "Dependency verification complete: {}/{} verified",
            verified_count, total_count
        );

        Ok(
            spacekit_primitives::v1::fact::types::DependencyVerification {
                all_dependencies_verified,
                verified_count,
                total_count,
                failed_dependencies,
            },
        )
    }

    async fn calculate_trust_score(&self, fact: &FactPackage) -> Result<f64> {
        debug!(
            "Calculating trust score for fact by author: {:?}",
            fact.author
        );

        // Get author's reputation
        let author_reputation = self.get_user_reputation(&fact.author).await?;

        // Get verification level weight
        let verification_weight =
            self.get_verification_level_weight(&fact.metadata.verification_level);

        // Get domain expertise factor
        let domain_expertise_factor = self
            .get_domain_expertise_factor(&fact.author, &fact.metadata.domain)
            .await?;

        // Calculate base trust score
        let base_trust = (author_reputation * 0.4)
            + (verification_weight * 0.4)
            + (domain_expertise_factor * 0.2);

        // Apply fact age factor (newer facts get slight boost)
        let age_factor = self.calculate_age_factor(fact.created_at);

        // Apply dependency trust factor
        let dependency_factor = self
            .calculate_dependency_trust_factor(&fact.dependencies)
            .await?;

        // Final trust score calculation
        let trust_score = (base_trust * age_factor * dependency_factor)
            .min(1.0)
            .max(0.0);

        debug!(
            "Trust score calculation: base={:.3}, age_factor={:.3}, dep_factor={:.3}, final={:.3}",
            base_trust, age_factor, dependency_factor, trust_score
        );

        Ok(trust_score)
    }

    async fn calculate_overall_confidence(
        &self,
        signature_valid: bool,
        author_verified: bool,
        content_integrity: bool,
        dependency_verification: &spacekit_primitives::v1::fact::types::DependencyVerification,
        trust_score: f64,
    ) -> Result<f64> {
        // Start with base confidence
        let mut confidence = 0.0;

        // Signature verification (30% of confidence)
        if signature_valid {
            confidence += 0.3;
        }

        // Author verification (25% of confidence)
        if author_verified {
            confidence += 0.25;
        }

        // Content integrity (20% of confidence)
        if content_integrity {
            confidence += 0.2;
        }

        // Dependency verification (15% of confidence)
        let dependency_factor = if dependency_verification.total_count == 0 {
            1.0 // No dependencies to verify
        } else {
            dependency_verification.verified_count as f64
                / dependency_verification.total_count as f64
        };
        confidence += 0.15 * dependency_factor;

        // Trust score (10% of confidence)
        confidence += 0.1 * trust_score;

        // Ensure confidence is within bounds
        let overall_confidence = confidence.min(1.0).max(0.0);

        debug!("Overall confidence calculation: sig={}, auth={}, content={}, deps={:.2}, trust={:.2}, final={:.3}", 
               signature_valid, author_verified, content_integrity, dependency_factor, trust_score, overall_confidence);

        Ok(overall_confidence)
    }

    // Verification utility methods

    fn is_quantum_safe_algorithm(&self, algorithm: &str) -> bool {
        matches!(
            algorithm,
            "SPHINCS-128f"
                | "SPHINCS-128s"
                | "SPHINCS-192f"
                | "SPHINCS-192s"
                | "SPHINCS-256f"
                | "SPHINCS-256s"
                | "SPHINCS+"
                | "Dilithium2"
                | "Dilithium3"
                | "Dilithium5"
                | "Falcon-512"
                | "Falcon-1024"
        )
    }

    fn create_verification_message(&self, fact: &FactPackage) -> Result<Vec<u8>> {
        // Create canonical message for signature verification
        let mut message = Vec::new();

        // Add fact ID
        message.extend_from_slice(&fact.fact_id);

        // Add content hash
        message.extend_from_slice(&fact.metadata.checksum);

        // Add author
        let author_bytes = serde_json::to_vec(&fact.author)?;
        message.extend_from_slice(&author_bytes);

        // Add timestamp
        message.extend_from_slice(&fact.created_at.to_le_bytes());

        Ok(message)
    }

    async fn verify_quantum_did(&self, did: &QuantumDID) -> Result<bool> {
        // TODO: Integrate with DID resolution service
        debug!("Verifying QuantumDID: {:?}", did);
        Ok(true) // Placeholder - always valid for now
    }

    async fn verify_author_credentials(
        &self,
        _author: &QuantumDID,
        metadata: &FactMetadata,
    ) -> Result<bool> {
        // TODO: Integrate with credential verification service
        debug!(
            "Verifying author credentials for domain: {:?}",
            metadata.domain
        );
        Ok(true) // Placeholder - always valid for now
    }

    async fn verify_domain_authorization(
        &self,
        author: &QuantumDID,
        domain: &KnowledgeDomain,
    ) -> Result<bool> {
        // TODO: Check if author is authorized to publish in this domain
        debug!(
            "Verifying domain authorization for: {:?} in {:?}",
            author, domain
        );
        Ok(true) // Placeholder - always authorized for now
    }

    fn compute_content_hash(&self, content: &FactContent) -> Result<[u8; 32]> {
        use sha2::{Digest, Sha256};
        let serialized = bincode::serde::encode_to_vec(content, bincode::config::standard())?;
        let hash = Sha256::digest(&serialized);
        Ok(hash.into())
    }

    async fn verify_content_structure(&self, content: &FactContent) -> Result<bool> {
        // Verify content structure is valid for its type
        match content {
            FactContent::Text { content, .. } => Ok(!content.is_empty()),
            FactContent::Numerical { value, .. } => Ok(!value.is_empty()),
            FactContent::Boolean { .. } => Ok(true),
            FactContent::Json { data, .. } => Ok(!data.is_null()),
            FactContent::Binary { data, hash, .. } => {
                // Verify binary data hash
                use sha2::{Digest, Sha256};
                let computed_hash: [u8; 32] = Sha256::digest(data).into();
                Ok(computed_hash == *hash)
            }
            FactContent::Reference { .. } => Ok(true),
            FactContent::Aggregation { source_facts, .. } => Ok(!source_facts.is_empty()),
        }
    }

    async fn verify_single_dependency(&self, dependency_id: FactID) -> Result<bool> {
        // Check if dependency exists and is valid
        match self.retrieve_fact(dependency_id).await? {
            Some(dependency_fact) => {
                // Simplified dependency check to avoid recursion
                // Just verify basic integrity without full verification chain
                let content_hash_valid = {
                    let computed_hash = self.compute_content_hash(&dependency_fact.content)?;
                    computed_hash == dependency_fact.metadata.checksum
                };

                // Verify signature (no recursion; signature check is local to the dependency fact).
                let signature_valid = self
                    .verify_quantum_signature(&dependency_fact)
                    .await
                    .unwrap_or(false);
                let not_expired = !dependency_fact.is_expired(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs(),
                );

                let is_valid = content_hash_valid && signature_valid && not_expired;
                debug!("Dependency {:?} basic validation: hash={}, sig_valid={}, not_expired={}, valid={}", 
                       hex::encode(dependency_id), content_hash_valid, signature_valid, not_expired, is_valid);

                Ok(is_valid)
            }
            None => {
                debug!("Dependency {:?} not found", hex::encode(dependency_id));
                Ok(false)
            }
        }
    }

    fn get_verification_level_weight(&self, level: &VerificationLevel) -> f64 {
        match level {
            VerificationLevel::Unverified => 0.1,
            VerificationLevel::SelfClaimed => 0.3,
            VerificationLevel::PeerReviewed => 0.7,
            VerificationLevel::Consensus => 0.8,
            VerificationLevel::Authoritative => 0.9,
            VerificationLevel::Cryptographic => 1.0,
        }
    }

    async fn get_domain_expertise_factor(
        &self,
        author: &QuantumDID,
        domain: &KnowledgeDomain,
    ) -> Result<f64> {
        let user_expertise = self.get_user_domain_expertise(author).await?;
        if user_expertise.contains(domain) {
            Ok(1.0) // Expert in domain
        } else {
            Ok(0.5) // Not expert in domain
        }
    }

    fn calculate_age_factor(&self, created_at: u64) -> f64 {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let age_days = (current_time - created_at) / (24 * 60 * 60);

        // Age factor: newer facts get slight boost, but old facts aren't penalized much
        if age_days < 30 {
            1.0 // Recent facts
        } else if age_days < 365 {
            0.95 // Older but not ancient
        } else {
            0.9 // Old facts
        }
    }

    async fn calculate_dependency_trust_factor(&self, dependencies: &[FactID]) -> Result<f64> {
        if dependencies.is_empty() {
            return Ok(1.0); // No dependencies to check
        }

        let mut total_trust = 0.0;
        let mut verified_deps = 0;

        for dep_id in dependencies {
            if let Some(dep_fact) = self.retrieve_fact(*dep_id).await? {
                // Use simpler trust calculation to avoid recursion
                let base_trust = if dep_fact.signature.signature_bytes.is_empty() {
                    0.3 // Low trust for unsigned facts
                } else {
                    0.7 // Higher trust for signed facts
                };

                // Apply verification level multiplier
                let verification_multiplier =
                    self.get_verification_level_weight(&dep_fact.metadata.verification_level);
                let dependency_trust = base_trust * verification_multiplier;

                total_trust += dependency_trust;
                verified_deps += 1;

                debug!(
                    "Dependency {:?} trust: base={:.2}, verification={:.2}, final={:.2}",
                    hex::encode(dep_id),
                    base_trust,
                    verification_multiplier,
                    dependency_trust
                );
            }
        }

        if verified_deps == 0 {
            Ok(0.5) // No dependencies could be verified
        } else {
            let avg_trust = total_trust / verified_deps as f64;
            debug!(
                "Dependency trust factor: {:.2} (from {} deps)",
                avg_trust, verified_deps
            );
            Ok(avg_trust)
        }
    }

    // Content encryption helper methods

    fn should_encrypt_fact(&self, fact: &FactPackage) -> bool {
        // Determine if fact should be encrypted based on access policy.
        // Conditional policies (PPV, channel subscribe) are enforced by content_access +
        // grants/on-chain entitlement — not at-rest KEM until publisher keys are wired
        // (get_user_public_key still uses a placeholder; Kyber1024 needs ~1568-byte keys).
        match &fact.access_policy {
            AccessPolicy::Public => false,
            AccessPolicy::Private(_) => true,
            AccessPolicy::RoleBased(_) => true,
            AccessPolicy::AttributeBased(_) => true,
            AccessPolicy::Dynamic(_) => true,
            AccessPolicy::Conditional(_) => false,
        }
    }

    async fn encrypt_fact_content(&self, content: &[u8], fact: &FactPackage) -> Result<Vec<u8>> {
        info!(
            "Encrypting fact content for fact {:?} ({} bytes)",
            hex::encode(fact.fact_id),
            content.len()
        );

        // Generate or derive encryption key for this fact
        let public_key = self.get_encryption_public_key(fact).await?;

        // Encrypt using quantum-safe algorithms
        let encrypted_data = self
            .quantum_crypto
            .encrypt_data(content, &public_key)
            .await?;

        // Serialize the encrypted data structure
        let serialized_encrypted =
            bincode::serde::encode_to_vec(&encrypted_data, bincode::config::standard())?;

        debug!(
            "Content encrypted: original={} bytes, encrypted={} bytes",
            content.len(),
            serialized_encrypted.len()
        );

        Ok(serialized_encrypted)
    }

    async fn decrypt_fact_content(
        &self,
        encrypted_content: &[u8],
        fact_id: FactID,
    ) -> Result<Vec<u8>> {
        info!(
            "Decrypting fact content for fact {:?} ({} bytes)",
            hex::encode(fact_id),
            encrypted_content.len()
        );

        // Deserialize the encrypted data structure
        let encrypted_data: crate::quantum::EncryptedData =
            bincode::serde::decode_from_slice(encrypted_content, bincode::config::standard())
                .map(|(data, _)| data)?;

        // Get private key for decryption
        let private_key = self.get_decryption_private_key(fact_id).await?;

        // Decrypt using quantum crypto service
        let decrypted_content = self
            .quantum_crypto
            .decrypt_data(&encrypted_data, &private_key)
            .await?;

        debug!(
            "Content decrypted: encrypted={} bytes, decrypted={} bytes",
            encrypted_content.len(),
            decrypted_content.len()
        );

        Ok(decrypted_content)
    }

    async fn get_encryption_public_key(&self, fact: &FactPackage) -> Result<Vec<u8>> {
        // TODO: Integrate with key management system
        // For now, generate a deterministic key based on fact ID and author

        debug!(
            "Getting encryption public key for fact {:?} by author {:?}",
            hex::encode(fact.fact_id),
            fact.author
        );

        match &fact.access_policy {
            AccessPolicy::Private(authorized_users) => {
                // For private facts, use the first authorized user's key
                if let Some(first_user) = authorized_users.iter().next() {
                    self.get_user_public_key(first_user).await
                } else {
                    // Fallback to author's key
                    self.get_user_public_key(&fact.author).await
                }
            }
            _ => {
                // For other policies, use author's key
                self.get_user_public_key(&fact.author).await
            }
        }
    }

    async fn get_decryption_private_key(&self, fact_id: FactID) -> Result<Vec<u8>> {
        // TODO: Integrate with key management system
        // For now, generate a deterministic private key

        debug!(
            "Getting decryption private key for fact {:?}",
            hex::encode(fact_id)
        );

        // Generate placeholder private key
        // In production, this would:
        // 1. Look up the fact's encryption metadata
        // 2. Determine which user's key to use
        // 3. Retrieve the private key from secure storage
        // 4. Verify access permissions

        let mut private_key = vec![0u8; 64];
        private_key[..32].copy_from_slice(&fact_id);
        private_key[32..].fill(0x42); // Placeholder pattern

        Ok(private_key)
    }

    async fn get_user_public_key(&self, user: &QuantumDID) -> Result<Vec<u8>> {
        // TODO: Integrate with DID resolution and key management
        debug!("Getting public key for user: {:?}", user);

        // Generate deterministic public key based on user DID
        // In production, this would resolve the DID and get the actual public key
        let user_bytes = serde_json::to_vec(user)?;
        let mut public_key = vec![0u8; 32];

        // Use first 32 bytes of user serialization as placeholder key
        let copy_len = std::cmp::min(32, user_bytes.len());
        public_key[..copy_len].copy_from_slice(&user_bytes[..copy_len]);

        Ok(public_key)
    }

    // Query caching helper methods

    fn generate_query_cache_key(&self, query: &FactQuery) -> Result<QueryCacheKey> {
        use sha2::{Digest, Sha256};

        // Hash the requester DID for privacy
        let requester_bytes = serde_json::to_vec(&query.requester)?;
        let requester_hash: [u8; 32] = Sha256::digest(&requester_bytes).into();

        // Hash the author DID if present
        let author_hash = if let Some(ref author) = query.author {
            let author_bytes = serde_json::to_vec(author)?;
            Some(Sha256::digest(&author_bytes).into())
        } else {
            None
        };

        // Serialize sort criteria for consistent caching
        let sort_by = serde_json::to_string(&query.sort_by)?;

        // Create sorted tags for consistent hashing
        let mut tags = query.tags.clone();
        tags.sort();

        Ok(QueryCacheKey {
            requester_hash,
            author_hash,
            category: query.category.as_ref().map(|c| format!("{:?}", c)),
            domain: query.domain.as_ref().map(|d| format!("{:?}", d)),
            tags,
            sort_by,
            pagination_offset: query.pagination.offset,
            pagination_limit: query.pagination.limit,
        })
    }

    async fn check_query_cache(
        &self,
        cache_key: &QueryCacheKey,
    ) -> Result<Option<FactQueryResult>> {
        let cache = self.query_cache.read().await;

        if let Some(entry) = cache.get(cache_key) {
            // Check if entry is still valid (TTL)
            let elapsed = entry.cached_at.elapsed().unwrap_or(Duration::from_secs(0));
            if elapsed <= entry.ttl {
                debug!("Query cache hit - entry age: {:?}", elapsed);
                return Ok(Some(entry.result.clone()));
            } else {
                debug!(
                    "Query cache entry expired - age: {:?}, TTL: {:?}",
                    elapsed, entry.ttl
                );
            }
        }

        Ok(None)
    }

    async fn cache_query_result(
        &self,
        cache_key: &QueryCacheKey,
        result: &FactQueryResult,
    ) -> Result<()> {
        let mut cache = self.query_cache.write().await;

        // Clean up expired entries if cache is getting large
        if cache.len() > 800 {
            // Clean when 80% full
            self.cleanup_expired_cache_entries(&mut cache).await;
        }

        // Cache the result with 5 minute TTL
        let entry = QueryCacheEntry {
            result: result.clone(),
            cached_at: SystemTime::now(),
            ttl: Duration::from_secs(300), // 5 minutes
        };

        cache.insert(cache_key.clone(), entry);
        debug!("Query result cached - cache size: {}", cache.len());

        Ok(())
    }

    async fn cleanup_expired_cache_entries(
        &self,
        cache: &mut HashMap<QueryCacheKey, QueryCacheEntry>,
    ) {
        let current_time = SystemTime::now();
        let mut expired_keys = Vec::new();

        for (key, entry) in cache.iter() {
            let elapsed = current_time
                .duration_since(entry.cached_at)
                .unwrap_or(Duration::from_secs(0));
            if elapsed > entry.ttl {
                expired_keys.push(key.clone());
            }
        }

        for key in expired_keys {
            cache.remove(&key);
        }

        debug!(
            "Cleaned up expired cache entries - cache size: {}",
            cache.len()
        );
    }
}

impl FileContentStorage {
    pub fn new(config: StorageTierConfig, compression: CompressionAlgorithm) -> Result<Self> {
        Ok(Self {
            config,
            compression,
        })
    }
}

#[async_trait::async_trait]
impl ContentStorage for FileContentStorage {
    async fn store_content(&self, fact_id: FactID, content: &[u8]) -> Result<StorageLocation> {
        let fact_id_hex = hex::encode(fact_id);
        let file_path = self
            .config
            .hot_storage_dir
            .join(format!("{}.dat", fact_id_hex));

        tokio::fs::write(&file_path, content).await?;

        // Calculate checksum
        let checksum = {
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(content);
            hash.into()
        };

        Ok(StorageLocation {
            tier: StorageTier::Hot,
            path: file_path,
            compressed: !matches!(self.compression, CompressionAlgorithm::None),
            encrypted: false, // TODO: Implement encryption
            checksum,
        })
    }

    async fn retrieve_content(&self, location: &StorageLocation) -> Result<Vec<u8>> {
        let content = tokio::fs::read(&location.path).await?;

        // Verify checksum
        {
            use sha2::{Digest, Sha256};
            let hash: [u8; 32] = Sha256::digest(&content).into();
            if hash != location.checksum {
                return Err(anyhow!("Content checksum mismatch"));
            }
        }

        Ok(content)
    }

    async fn delete_content(&self, location: &StorageLocation) -> Result<()> {
        tokio::fs::remove_file(&location.path).await?;
        Ok(())
    }

    async fn move_to_tier(
        &self,
        location: &StorageLocation,
        tier: StorageTier,
    ) -> Result<StorageLocation> {
        let new_dir = match tier {
            StorageTier::Hot => &self.config.hot_storage_dir,
            StorageTier::Cold => &self.config.cold_storage_dir,
            StorageTier::Frozen => &self.config.cold_storage_dir, // Use cold for frozen for now
        };

        let file_name = location
            .path
            .file_name()
            .ok_or_else(|| anyhow!("Invalid file path"))?;
        let new_path = new_dir.join(file_name);

        tokio::fs::rename(&location.path, &new_path).await?;

        let mut new_location = location.clone();
        new_location.tier = tier;
        new_location.path = new_path;

        Ok(new_location)
    }
}

impl Default for FactStorageConfig {
    fn default() -> Self {
        Self {
            storage_dir: PathBuf::from("./fact_storage"),
            max_fact_size: 100 * 1024 * 1024, // 100MB
            enable_compression: true,
            compression_algorithm: CompressionAlgorithm::Gzip,
            enable_deduplication: true,
            verification_cache_size: 10000,
            enable_auto_indexing: true,
            storage_tiers: StorageTierConfig {
                hot_storage_dir: PathBuf::from("./fact_storage/hot"),
                cold_storage_dir: PathBuf::from("./fact_storage/cold"),
                archive_threshold_days: 30,
                max_hot_storage_bytes: 10 * 1024 * 1024 * 1024, // 10GB
            },
        }
    }
}

#[cfg(test)]
mod fact_content_codec_tests {
    use super::*;

    #[test]
    fn json_fact_content_roundtrip() {
        let original = FactContent::Json {
            data: serde_json::json!({
                "schema": "spacekit:licensed_feature:v1",
                "feature_name": "growformer",
                "tiers": [{"name": "free"}]
            }),
            schema: Some("spacekit:licensed_feature:v1".into()),
        };
        let bytes = encode_fact_content(&original).unwrap();
        assert!(bytes.starts_with(FACT_CONTENT_JSON_MAGIC));
        let decoded = decode_fact_content(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn binary_fact_content_still_uses_bincode() {
        let original = FactContent::Binary {
            data: b"hello".to_vec(),
            mime_type: "text/plain".into(),
            hash: [1u8; 32],
        };
        let bytes = encode_fact_content(&original).unwrap();
        assert!(!bytes.starts_with(FACT_CONTENT_JSON_MAGIC));
        assert_eq!(decode_fact_content(&bytes).unwrap(), original);
    }

    #[test]
    fn every_storage_codec_roundtrips() {
        let data = b"SpaceKit fact payload with repeated content. ".repeat(256);
        for algorithm in [
            CompressionAlgorithm::None,
            CompressionAlgorithm::Gzip,
            CompressionAlgorithm::Zstd,
            CompressionAlgorithm::Lz4,
            CompressionAlgorithm::Brotli,
        ] {
            let compressed = compress_with_algorithm(&data, &algorithm).unwrap();
            let decompressed = decompress_with_algorithm(&compressed, &algorithm).unwrap();
            assert_eq!(decompressed, data, "failed codec {algorithm:?}");
            if algorithm != CompressionAlgorithm::None {
                assert!(
                    compressed.len() < data.len(),
                    "codec {algorithm:?} did not compress repeated fixture"
                );
            }
        }
    }

    #[test]
    fn legacy_compressed_records_require_the_configured_algorithm() {
        let data = b"legacy fact payload".repeat(128);
        let gzip = compress_with_algorithm(&data, &CompressionAlgorithm::Gzip).unwrap();
        assert!(decompress_with_algorithm(&gzip, &CompressionAlgorithm::Zstd).is_err());
        assert_eq!(
            decompress_with_algorithm(&gzip, &CompressionAlgorithm::Gzip).unwrap(),
            data
        );
    }
}
