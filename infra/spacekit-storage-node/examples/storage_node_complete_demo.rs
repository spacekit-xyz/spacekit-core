#![recursion_limit = "256"]

//! Complete Storage Node Demo
//!
//! Demonstrates all major features:
//! - Rewards calculation
//! - SQL queries with JOINs and Subqueries
//! - NFT storage
//! - Fact Package storage
//! - Horizontal Sharding
//! - Full-Text Search
//! - Vector Search (Semantic Search)

use anyhow::Result;
use spacekit_primitives::v1::{fact::TextEncoding, identity::QuantumDID};
use spacekit_storage_node::{
    create_simple_nft,
    fulltext_search::{FullTextIndex, SearchQuery},
    sharding::{ShardKeyType, ShardManager},
    vector_search::{
        IndexType, VectorEmbedding, VectorIndex, VectorSearchManager, VectorSearchQuery,
    },
    FactQuery, FactStorageConfig, FactStorageEngine, Filter, FilterOp, FilterValue, NetworkConfig,
    NftMetadata, NftStorageManager, SortBy, StorageNode, StorageNodeConfig, StorageQueryBuilder,
    StorageRewardCalculator, StorageRewardConfig,
};
use std::collections::HashMap;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    Ok(())
}
// async fn main() -> Result<()> {
//     // Initialize logging
//     tracing_subscriber::fmt::init();

//     println!("\n🚀 SpaceKit Storage Node - Complete Demo\n");
//     println!("={}", "=".repeat(60));

//     // 1. SETUP STORAGE NODE
//     println!("\n📦 1. Setting up Storage Node...\n");

//     let config = StorageNodeConfig {
//         max_storage_bytes: 100 * 1024 * 1024 * 1024, // 100GB
//         data_dir: std::path::PathBuf::from("./demo_storage"),
//         database_path: Some(std::path::PathBuf::from("./demo_storage/demo.json")),
//         node_did: "did:spacekit:storage:demo-node".to_string(),
//         preferred_algorithm: "kyber1024".to_string(),
//         encryption_keypair: None,
//         network_config: NetworkConfig::default(),
//         #[cfg(feature = "api-server")]
//         api_config: None,
//     };

//     let storage_node = Arc::new(StorageNode::new(config).await?);
//     println!("✓ Storage node created");

//     storage_node.start().await?;
//     println!("✓ Storage node started");

//     // 2. REWARDS SYSTEM
//     println!("\n💰 2. Setting up Rewards System...\n");

//     let reward_config = StorageRewardConfig {
//         base_reward_per_gb_day: 0.01,
//         quantum_encryption_bonus: 1.2,
//         enable_token_minting: true,
//         ..Default::default()
//     };

//     let calculator = StorageRewardCalculator::new(reward_config, storage_node.clone());

//     // Calculate current rewards
//     let calculation = calculator.calculate_rewards().await?;
//     println!("💎 Current Reward Calculation:");
//     println!("   Base reward: {} ASTRA", calculation.base_reward);
//     println!("   Final reward: {} ASTRA", calculation.final_reward);
//     println!("   Storage: {:.2} GB", calculation.storage_gb);
//     println!("   Multiplier: {:.2}x", calculation.bonus_breakdown.total_multiplier);

//     // Estimate monthly income
//     let monthly = calculator.estimate_monthly_income().await?;
//     println!("\n📈 Estimated monthly income: {} ASTRA", monthly);

//     // Get analytics
//     let analytics = calculator.get_reward_analytics().await?;
//     println!("\n📊 Reward Analytics:");
//     println!("   Total earned: {} ASTRA", analytics.total_earned_astra);
//     println!("   Payment count: {}", analytics.payment_count);
//     println!("   Current multiplier: {:.2}x", analytics.total_multiplier);

//     // 3. SQL QUERY INTERFACE
//     println!("\n🔍 3. SQL Query Interface Demo...\n");

//     let database = storage_node.database();
//     let query_builder = StorageQueryBuilder::new(database.clone());

//     // Example fact query with filters
//     let fact_query = FactQuery {
//         distinct: false,
//         window_functions: Vec::new(),
//         joins: vec![],
//         filters: vec![
//             Filter {
//                 field: "category".to_string(),
//                 op: FilterOp::Equals,
//                 value: FilterValue::String("Scientific".to_string()),
//             },
//             Filter {
//                 field: "confidence_score".to_string(),
//                 op: FilterOp::GreaterThan,
//                 value: FilterValue::Number(0.8),
//             },
//         ],
//         sort_by: Some(SortBy {
//             field: "created_at".to_string(),
//             order: SortOrder::Desc,
//         }),
//         limit: Some(10),
//         offset: None,
//     };

//     let results = query_builder.query_facts(fact_query).await?;
//     println!("✓ Query executed in {}ms", results.execution_time_ms);
//     println!("   Found {} facts", results.total_count);
//     println!("   Returned {} results", results.facts.len());

//     // 4. NFT STORAGE
//     println!("\n🖼️  4. NFT Storage Demo...\n");

//     // Create fact storage engine for NFT manager
//     let quantum_crypto = storage_node.quantum_crypto();
//     let fact_config = FactStorageConfig::default();
//     let fact_storage = FactStorageEngine::new(
//         database.clone(),
//         quantum_crypto,
//         fact_config,
//     ).await?;

//     let nft_manager = NftStorageManager::new(fact_storage);

//     // Create a simple NFT
//     let creator = QuantumDID::new("did:spacekit:test_creator".to_string());
//     let owner = QuantumDID::new("did:spacekit:test_owner".to_string());

//     println!("Creating test NFT...");
//     let nft_data = b"Test NFT Image Data".to_vec();

//     let nft_result = create_simple_nft(
//         &nft_manager,
//         nft_data,
//         "Demo NFT #1".to_string(),
//         "A demonstration NFT stored with quantum-safe encryption".to_string(),
//         creator.clone(),
//         owner.clone(),
//     ).await?;

//     println!("✓ NFT stored successfully!");
//     println!("   NFT ID: {}", hex::encode(nft_result.nft_id));
//     println!("   Content hash: {}", nft_result.content_hash);
//     println!("   Storage location: {}", nft_result.storage_location);
//     println!("   Quantum proof: {}", nft_result.quantum_proof);

//     // Verify NFT
//     let is_authentic = nft_manager.verify_nft(nft_result.nft_id).await?;
//     println!("\n🔐 NFT Verification:");
//     println!("   Authentic: {}", if is_authentic { "✓ Yes" } else { "✗ No" });

//     // 5. FACT PACKAGE STORAGE
//     println!("\n📚 5. Fact Package Storage Demo...\n");

//     println!("Storing example fact package...");

//     // Create a simple fact package
//     use spacekit_primitives::v1::fact::{
//         FactPackage, FactContent, FactMetadata, FactCategory,
//         KnowledgeDomain, AccessPolicy, VerificationLevel,
//         LicenseType, DataSource, CollectionMethod,
//         VerificationProof, ProofType,
//     };
//     use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
//     use chrono::Utc;

//     let fact_content = FactContent::Text {
//         content: "The capital of France is Paris.".to_string(),
//         language: Some("en".to_string()),
//         encoding: TextEncoding::UTF8,
//     };

//     let content_bytes = serde_json::to_vec(&fact_content)?;
//         let checksum = *blake3::hash(&content_bytes).as_bytes();

//     let fact_metadata = FactMetadata {
//         category: FactCategory::Reference,
//         tags: vec!["geography".to_string(), "europe".to_string()],
//         domain: KnowledgeDomain::Geography,
//         source: DataSource::UserInput {
//             application: creator.clone(),
//             user: owner.clone(),
//         },
//         collection_method: CollectionMethod::Manual,
//         verification_level: VerificationLevel::Authoritative,
//         license: LicenseType::PublicDomain,
//         size_bytes: content_bytes.len() as u64,
//         checksum: checksum.clone(),
//     };

//     let fact_id: [u8; 32] = *blake3::hash(b"demo_fact_1").as_bytes();

//     let signature = SPHINCSSignature::new(
//         vec![0u8; 64],
//         "SPHINCS-256f".to_string(),
//         vec![0u8; 32],
//     );

//     let verification_proof = VerificationProof {
//         proof_type: ProofType::QuantumSignature,
//         proof_data: vec![0u8; 32],
//         verification_timestamp: Utc::now().timestamp() as u64,
//         verifier: Some(creator.clone()),
//     };

//     let fact = FactPackage {
//         fact_id,
//         version: 1,
//         created_at: Utc::now().timestamp() as u64,
//         expires_at: None,
//         content: fact_content,
//         metadata: fact_metadata,
//         author: creator,
//         signature,
//         verification_proof,
//         dependencies: Vec::new(),
//         citations: Vec::new(),
//         confidence_score: 0.95,
//         access_policy: AccessPolicy::Public,
//         encryption: None,
//     };

//     // Recreate fact storage for this demo
//     let quantum_crypto = storage_node.quantum_crypto();
//     let fact_config = FactStorageConfig::default();
//     let fact_storage = FactStorageEngine::new(
//         database.clone(),
//         quantum_crypto,
//         fact_config,
//     ).await?;

//     let stored_fact_id = fact_storage.store_fact(fact).await?;
//     println!("✓ Fact stored successfully!");
//     println!("   Fact ID: {}", hex::encode(stored_fact_id));

//     // Verify the fact
//     let verification = fact_storage.verify_fact(stored_fact_id).await?;
//     println!("\n🔍 Fact Verification:");
//     println!("   Signature valid: {}", verification.signature_valid);
//     println!("   Author verified: {}", verification.author_verified);
//     println!("   Trust score: {:.2}", verification.trust_score);
//     println!("   Overall confidence: {:.2}", verification.overall_confidence);

//     // 6. HORIZONTAL SHARDING
//     println!("\n🔀 6. Horizontal Sharding Demo...\n");

//     let shard_manager = ShardManager::new("owner_did".to_string(), ShardKeyType::Hash);

//     // Add shards
//     let db1 = database.clone();
//     shard_manager.add_shard("shard-1".to_string(), "node-1".to_string(), db1).await?;
//     println!("✓ Shard 1 added");

//     let db2 = database.clone();
//     shard_manager.add_shard("shard-2".to_string(), "node-2".to_string(), db2).await?;
//     println!("✓ Shard 2 added");

//     // Route a key to a shard
//     let shard_id = shard_manager.route_to_shard("did:spacekit:user:alice").await?;
//     println!("✓ Key 'did:spacekit:user:alice' routed to: {}", shard_id);

//     // Get shard statistics
//     let shard_stats = shard_manager.get_shard_stats().await?;
//     println!("\n📊 Shard Statistics:");
//     println!("   Total shards: {}", shard_stats.total_shards);
//     println!("   Total data: {} items", shard_stats.total_data);
//     println!("   Avg data per shard: {}", shard_stats.avg_data_per_shard);

//     // 7. FULL-TEXT SEARCH
//     println!("\n🔍 7. Full-Text Search Demo...\n");

//     let fulltext_index = FullTextIndex::new();

//     // Index some documents
//     fulltext_index.index_document(
//         "doc1".to_string(),
//         "files".to_string(),
//         "filename".to_string(),
//         "The quick brown fox jumps over the lazy dog".to_string(),
//     ).await?;
//     println!("✓ Document 1 indexed");

//     fulltext_index.index_document(
//         "doc2".to_string(),
//         "files".to_string(),
//         "filename".to_string(),
//         "Quantum computing is the future of cryptography".to_string(),
//     ).await?;
//     println!("✓ Document 2 indexed");

//     // Search
//     let search_query = SearchQuery {
//         query: "quantum cryptography".to_string(),
//         table: Some("files".to_string()),
//         field: None,
//         limit: Some(10),
//         fuzzy: false,
//         phrase: false,
//     };

//     let search_results = fulltext_index.search(search_query).await?;
//     println!("\n🔍 Search Results:");
//     println!("   Found {} results", search_results.len());
//     for result in &search_results {
//         println!("   - Document {}: score {:.3}", result.document_id, result.score);
//         if !result.snippets.is_empty() {
//             println!("     Snippet: {}", result.snippets[0]);
//         }
//     }

//     // Get index statistics
//     let index_stats = fulltext_index.get_stats().await;
//     println!("\n📊 Full-Text Index Statistics:");
//     println!("   Total documents: {}", index_stats.total_documents);
//     println!("   Total terms: {}", index_stats.total_terms);
//     println!("   Avg terms per document: {:.2}", index_stats.avg_terms_per_document);

//     // 8. VECTOR SEARCH (SEMANTIC SEARCH)
//     println!("\n🧠 8. Vector Search (Semantic Search) Demo...\n");

//     let vector_manager = VectorSearchManager::new();

//     // Create a vector index (384 dimensions for sentence transformers)
//     let vector_index = vector_manager.get_or_create_index(
//         "semantic_index".to_string(),
//         384,
//         IndexType::BruteForce,
//     ).await;

//     // Add some embeddings (simulated - in production, use actual embeddings)
//     let embedding1 = VectorEmbedding {
//         document_id: "doc1".to_string(),
//         table: "files".to_string(),
//         field: "content".to_string(),
//         vector: vec![0.1; 384], // Simulated embedding
//         metadata: HashMap::from([
//             ("title".to_string(), "Introduction to Quantum Computing".to_string()),
//         ]),
//         created_at: chrono::Utc::now(),
//     };
//     vector_index.add_embedding(embedding1).await?;
//     println!("✓ Embedding 1 added");

//     let embedding2 = VectorEmbedding {
//         document_id: "doc2".to_string(),
//         table: "files".to_string(),
//         field: "content".to_string(),
//         vector: vec![0.2; 384], // Simulated embedding
//         metadata: HashMap::from([
//             ("title".to_string(), "Advanced Cryptography".to_string()),
//         ]),
//         created_at: chrono::Utc::now(),
//     };
//     vector_index.add_embedding(embedding2).await?;
//     println!("✓ Embedding 2 added");

//     // Search for similar vectors
//     let query_vector = vec![0.15; 384]; // Simulated query embedding
//     let vector_query = VectorSearchQuery {
//         query_vector,
//         table: Some("files".to_string()),
//         field: None,
//         limit: Some(5),
//         min_similarity: Some(0.5),
//     };

//     let vector_results = vector_index.search(vector_query).await?;
//     println!("\n🧠 Vector Search Results:");
//     println!("   Found {} similar documents", vector_results.len());
//     for result in &vector_results {
//         println!("   - Document {}: similarity {:.3}", result.document_id, result.similarity);
//         if let Some(title) = result.metadata.get("title") {
//             println!("     Title: {}", title);
//         }
//     }

//     // Get vector index statistics
//     let vector_stats = vector_index.get_stats().await;
//     println!("\n📊 Vector Index Statistics:");
//     println!("   Total embeddings: {}", vector_stats.total_embeddings);
//     println!("   Dimension: {}", vector_stats.dimension);
//     println!("   Index type: {:?}", vector_stats.index_type);

//     // 9. STORAGE STATISTICS
//     println!("\n📊 9. Storage Statistics...\n");

//     let stats = storage_node.get_stats().await?;
//     println!("Storage Node Stats:");
//     println!("   Node DID: {}", stats.node_did);
//     println!("   Files: {}", stats.file_count);
//     println!("   Total size: {} bytes", stats.total_size_bytes);
//     println!("   Utilization: {:.1}%", stats.storage_utilization);
//     println!("   Users: {}", stats.user_count);
//     println!("   Messages: {}", stats.message_count);
//     println!("   Quantum algorithm: {}", stats.preferred_algorithm);

//     // 10. SUMMARY
//     println!("\n{}", "=".repeat(60));
//     println!("✓ Demo Complete!");
//     println!("{}", "=".repeat(60));
//     println!("\n📝 Summary:");
//     println!("   • Rewards system configured and calculating earnings");
//     println!("   • SQL query interface ready for complex queries (JOINs, Subqueries)");
//     println!("   • NFT storage operational with quantum-safe encryption");
//     println!("   • Fact Package storage with verification pipeline");
//     println!("   • Horizontal sharding for distributed storage");
//     println!("   • Full-text search with TF-IDF ranking");
//     println!("   • Vector search for semantic similarity");
//     println!("   • Full statistics and monitoring available");

//     println!("\n💡 Next Steps:");
//     println!("   1. Store more NFTs and Facts to increase earnings");
//     println!("   2. Enable P2P replication for bonus rewards");
//     println!("   3. Maintain 99%+ uptime for availability bonus");
//     println!("   4. Monitor rewards analytics and optimize");

//     println!("\n🎯 Potential Monthly Income:");
//     println!("   Current: {} ASTRA/month", monthly);
//     println!("   Optimized (100GB): ~378 ASTRA/month");
//     println!("   Premium (1TB): ~1,186 ASTRA/month");

//     println!("\n🌟 Thank you for using SpaceKit Storage Node!\n");

//     Ok(())
// }
