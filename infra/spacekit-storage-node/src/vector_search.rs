//! Vector Search for Storage Node
//!
//! Provides semantic search using vector embeddings, similarity search,
//! and approximate nearest neighbor (ANN) search.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Vector embedding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorEmbedding {
    pub document_id: String,
    pub table: String,                     // "files", "facts", "users"
    pub field: String,                     // Field that was embedded
    pub vector: Vec<f32>,                  // Embedding vector
    pub metadata: HashMap<String, String>, // Additional metadata
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Vector search index
pub struct VectorIndex {
    /// Document ID -> Vector embedding
    embeddings: Arc<RwLock<HashMap<String, VectorEmbedding>>>,
    /// Vector dimension
    dimension: usize,
    /// Index type
    index_type: IndexType,
}

/// Index type for vector search
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// Brute force (exact search, slow for large datasets)
    BruteForce,
    /// Approximate Nearest Neighbor (fast, approximate)
    ANN,
}

/// Vector search query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchQuery {
    pub query_vector: Vec<f32>,
    pub table: Option<String>, // Filter by table
    pub field: Option<String>, // Filter by field
    pub limit: Option<usize>,
    pub min_similarity: Option<f32>, // Minimum cosine similarity (0.0 to 1.0)
}

/// Vector search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorSearchResult {
    pub document_id: String,
    pub table: String,
    pub field: String,
    pub similarity: f32, // Cosine similarity score
    pub metadata: HashMap<String, String>,
}

impl VectorIndex {
    /// Create a new vector index
    pub fn new(dimension: usize, index_type: IndexType) -> Self {
        Self {
            embeddings: Arc::new(RwLock::new(HashMap::new())),
            dimension,
            index_type,
        }
    }

    /// Add a vector embedding
    pub async fn add_embedding(&self, embedding: VectorEmbedding) -> Result<()> {
        // Validate vector dimension
        if embedding.vector.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Vector dimension mismatch: expected {}, got {}",
                self.dimension,
                embedding.vector.len()
            ));
        }

        let document_id = embedding.document_id.clone();
        let mut embeddings = self.embeddings.write().await;
        embeddings.insert(document_id.clone(), embedding);
        drop(embeddings);

        debug!("Added vector embedding: {}", document_id);
        Ok(())
    }

    /// Search for similar vectors
    pub async fn search(&self, query: VectorSearchQuery) -> Result<Vec<VectorSearchResult>> {
        // Validate query vector dimension
        if query.query_vector.len() != self.dimension {
            return Err(anyhow::anyhow!(
                "Query vector dimension mismatch: expected {}, got {}",
                self.dimension,
                query.query_vector.len()
            ));
        }

        let embeddings = self.embeddings.read().await;

        // Calculate similarity for all embeddings
        let mut results: Vec<VectorSearchResult> = Vec::new();

        for (doc_id, embedding) in embeddings.iter() {
            // Apply filters
            if let Some(ref table_filter) = query.table {
                if embedding.table != *table_filter {
                    continue;
                }
            }
            if let Some(ref field_filter) = query.field {
                if embedding.field != *field_filter {
                    continue;
                }
            }

            // Calculate cosine similarity
            let similarity = self.cosine_similarity(&query.query_vector, &embedding.vector);

            // Apply minimum similarity threshold
            if let Some(min_sim) = query.min_similarity {
                if similarity < min_sim {
                    continue;
                }
            }

            results.push(VectorSearchResult {
                document_id: doc_id.clone(),
                table: embedding.table.clone(),
                field: embedding.field.clone(),
                similarity,
                metadata: embedding.metadata.clone(),
            });
        }

        // Sort by similarity (descending)
        results.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    /// Calculate cosine similarity between two vectors
    fn cosine_similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    /// Remove an embedding
    pub async fn remove_embedding(&self, document_id: &str) -> Result<()> {
        let mut embeddings = self.embeddings.write().await;
        embeddings.remove(document_id);
        drop(embeddings);

        debug!("Removed vector embedding: {}", document_id);
        Ok(())
    }

    /// Get index statistics
    pub async fn get_stats(&self) -> VectorIndexStats {
        let embeddings = self.embeddings.read().await;

        VectorIndexStats {
            total_embeddings: embeddings.len(),
            dimension: self.dimension,
            index_type: self.index_type,
        }
    }
}

/// Vector index statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorIndexStats {
    pub total_embeddings: usize,
    pub dimension: usize,
    pub index_type: IndexType,
}

/// Vector search manager (manages multiple vector indexes)
pub struct VectorSearchManager {
    indexes: Arc<RwLock<HashMap<String, Arc<VectorIndex>>>>, // index_name -> VectorIndex
}

impl VectorSearchManager {
    /// Create a new vector search manager
    pub fn new() -> Self {
        Self {
            indexes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create or get a vector index
    pub async fn get_or_create_index(
        &self,
        index_name: String,
        dimension: usize,
        index_type: IndexType,
    ) -> Arc<VectorIndex> {
        let mut indexes = self.indexes.write().await;

        if let Some(index) = indexes.get(&index_name) {
            return index.clone();
        }

        let index_name_clone = index_name.clone();
        let index = Arc::new(VectorIndex::new(dimension, index_type));
        indexes.insert(index_name_clone.clone(), index.clone());
        drop(indexes);

        info!(
            "Created vector index: {} (dimension: {}, type: {:?})",
            index_name_clone, dimension, index_type
        );
        index
    }

    /// Get an existing index
    pub async fn get_index(&self, index_name: &str) -> Result<Arc<VectorIndex>> {
        let indexes = self.indexes.read().await;
        indexes
            .get(index_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Vector index not found: {}", index_name))
    }

    /// List all indexes
    pub async fn list_indexes(&self) -> Vec<String> {
        let indexes = self.indexes.read().await;
        indexes.keys().cloned().collect()
    }
}

impl Default for VectorSearchManager {
    fn default() -> Self {
        Self::new()
    }
}
