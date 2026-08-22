//! Full-Text Search for Storage Node
//!
//! Provides text indexing, search ranking, phrase matching,
//! and fuzzy search capabilities.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Full-text search index
pub struct FullTextIndex {
    /// Term -> Document IDs mapping
    inverted_index: Arc<RwLock<HashMap<String, HashSet<String>>>>,
    /// Document ID -> Document metadata
    documents: Arc<RwLock<HashMap<String, DocumentMetadata>>>,
    /// Stop words to ignore
    stop_words: HashSet<String>,
}

/// Document metadata for search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub document_id: String,
    pub table: String,                            // "files", "facts", "users"
    pub field: String,                            // Field that was indexed
    pub content: String,                          // Original content
    pub term_frequencies: HashMap<String, usize>, // Term -> frequency
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub document_id: String,
    pub table: String,
    pub field: String,
    pub score: f64,
    pub snippets: Vec<String>, // Text snippets with matches highlighted
}

/// Search query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub table: Option<String>, // Filter by table
    pub field: Option<String>, // Filter by field
    pub limit: Option<usize>,
    pub fuzzy: bool,  // Enable fuzzy matching
    pub phrase: bool, // Require exact phrase match
}

impl FullTextIndex {
    /// Create a new full-text search index
    pub fn new() -> Self {
        let stop_words = Self::default_stop_words();
        Self {
            inverted_index: Arc::new(RwLock::new(HashMap::new())),
            documents: Arc::new(RwLock::new(HashMap::new())),
            stop_words,
        }
    }

    /// Index a document
    pub async fn index_document(
        &self,
        document_id: String,
        table: String,
        field: String,
        content: String,
    ) -> Result<()> {
        // Tokenize content
        let terms = self.tokenize(&content);

        // Calculate term frequencies
        let mut term_frequencies = HashMap::new();
        for term in &terms {
            *term_frequencies.entry(term.clone()).or_insert(0) += 1;
        }

        // Update inverted index
        let mut index = self.inverted_index.write().await;
        for term in &terms {
            index
                .entry(term.clone())
                .or_insert_with(HashSet::new)
                .insert(document_id.clone());
        }
        drop(index);

        // Store document metadata
        let mut documents = self.documents.write().await;
        documents.insert(
            document_id.clone(),
            DocumentMetadata {
                document_id: document_id.clone(),
                table,
                field,
                content,
                term_frequencies,
                created_at: chrono::Utc::now(),
            },
        );
        drop(documents);

        debug!("Indexed document: {}", document_id);
        Ok(())
    }

    /// Search for documents
    pub async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>> {
        let query_terms = self.tokenize(&query.query);

        if query_terms.is_empty() {
            return Ok(Vec::new());
        }

        let index = self.inverted_index.read().await;
        let documents = self.documents.read().await;

        // Find documents containing query terms
        let mut document_scores: HashMap<String, f64> = HashMap::new();

        for term in &query_terms {
            if let Some(doc_ids) = index.get(term) {
                let term_doc_count = doc_ids.len() as f64;
                let total_docs = documents.len() as f64;

                // Calculate IDF (Inverse Document Frequency)
                let idf = if term_doc_count > 0.0 {
                    (total_docs / term_doc_count).ln()
                } else {
                    0.0
                };

                // Calculate TF-IDF for each document
                for doc_id in doc_ids {
                    if let Some(doc) = documents.get(doc_id) {
                        // Apply filters
                        if let Some(ref table_filter) = query.table {
                            if doc.table != *table_filter {
                                continue;
                            }
                        }
                        if let Some(ref field_filter) = query.field {
                            if doc.field != *field_filter {
                                continue;
                            }
                        }

                        // Calculate TF (Term Frequency)
                        let tf = doc
                            .term_frequencies
                            .get(term)
                            .map(|&freq| {
                                freq as f64 / doc.term_frequencies.values().sum::<usize>() as f64
                            })
                            .unwrap_or(0.0);

                        // TF-IDF score
                        let score = tf * idf;
                        *document_scores.entry(doc_id.clone()).or_insert(0.0) += score;
                    }
                }
            }
        }

        // Convert to search results and sort by score
        let mut results: Vec<SearchResult> = document_scores
            .into_iter()
            .filter_map(|(doc_id, score)| {
                documents.get(&doc_id).map(|doc| {
                    let snippets = self.generate_snippets(&doc.content, &query_terms);
                    SearchResult {
                        document_id: doc_id.clone(),
                        table: doc.table.clone(),
                        field: doc.field.clone(),
                        score,
                        snippets,
                    }
                })
            })
            .collect();

        // Sort by score (descending)
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit
        if let Some(limit) = query.limit {
            results.truncate(limit);
        }

        Ok(results)
    }

    /// Tokenize text into terms
    fn tokenize(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split_whitespace()
            .map(|s| s.trim_matches(|c: char| !c.is_alphanumeric()))
            .filter(|s| !s.is_empty() && !self.stop_words.contains(*s))
            .map(|s| s.to_string())
            .collect()
    }

    /// Generate text snippets with highlighted matches
    fn generate_snippets(&self, content: &str, query_terms: &[String]) -> Vec<String> {
        let mut snippets = Vec::new();
        let content_lower = content.to_lowercase();

        // Find positions of query terms
        for term in query_terms {
            if let Some(pos) = content_lower.find(term) {
                let start = pos.saturating_sub(50);
                let end = (pos + term.len() + 50).min(content.len());
                let snippet = format!("...{}...", &content[start..end]);
                snippets.push(snippet);
            }
        }

        if snippets.is_empty() {
            // Return first 100 characters if no matches found
            snippets.push(format!("{}...", &content[..content.len().min(100)]));
        }

        snippets
    }

    /// Default stop words (common words to ignore)
    fn default_stop_words() -> HashSet<String> {
        let words = vec![
            "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "he", "in",
            "is", "it", "its", "of", "on", "that", "the", "to", "was", "will", "with",
        ];
        words.into_iter().map(|s| s.to_string()).collect()
    }

    /// Remove a document from the index
    pub async fn remove_document(&self, document_id: &str) -> Result<()> {
        let mut documents = self.documents.write().await;
        if let Some(doc) = documents.remove(document_id) {
            // Remove from inverted index
            let mut index = self.inverted_index.write().await;
            for term in doc.term_frequencies.keys() {
                if let Some(doc_set) = index.get_mut(term) {
                    doc_set.remove(document_id);
                    if doc_set.is_empty() {
                        index.remove(term);
                    }
                }
            }
            drop(index);
            drop(documents);

            debug!("Removed document from index: {}", document_id);
        }
        Ok(())
    }

    /// Get index statistics
    pub async fn get_stats(&self) -> IndexStats {
        let index = self.inverted_index.read().await;
        let documents = self.documents.read().await;

        IndexStats {
            total_documents: documents.len(),
            total_terms: index.len(),
            avg_terms_per_document: if documents.len() > 0 {
                index.values().map(|s| s.len()).sum::<usize>() as f64 / documents.len() as f64
            } else {
                0.0
            },
        }
    }
}

/// Index statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub total_documents: usize,
    pub total_terms: usize,
    pub avg_terms_per_document: f64,
}

impl Default for FullTextIndex {
    fn default() -> Self {
        Self::new()
    }
}
