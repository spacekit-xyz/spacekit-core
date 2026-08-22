//! Advanced Indexing System for Storage Node
//!
//! Provides B-tree, Hash, and composite indexes for query optimization.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Index type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    /// B-tree index (sorted, supports range queries)
    BTree,
    /// Hash index (equality lookups only)
    Hash,
    /// Composite index (multiple columns)
    Composite,
}

/// Index definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDefinition {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>, // Single column for B-tree/Hash, multiple for composite
    pub index_type: IndexType,
    pub unique: bool,
}

/// B-tree index implementation
#[derive(Debug, Clone)]
pub struct BTreeIndex {
    pub name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
    index: Arc<RwLock<BTreeMap<String, Vec<String>>>>, // value -> [row_ids]
}

/// Hash index implementation
#[derive(Debug, Clone)]
pub struct HashIndex {
    pub name: String,
    pub table: String,
    pub column: String,
    pub unique: bool,
    index: Arc<RwLock<HashMap<String, Vec<String>>>>, // value -> [row_ids]
}

/// Composite index implementation
#[derive(Debug, Clone)]
pub struct CompositeIndex {
    pub name: String,
    pub table: String,
    pub columns: Vec<String>,
    pub unique: bool,
    index: Arc<RwLock<BTreeMap<Vec<String>, Vec<String>>>>, // [values] -> [row_ids]
}

/// Index manager
pub struct IndexManager {
    btree_indexes: Arc<RwLock<HashMap<String, BTreeIndex>>>,
    hash_indexes: Arc<RwLock<HashMap<String, HashIndex>>>,
    composite_indexes: Arc<RwLock<HashMap<String, CompositeIndex>>>,
}

impl IndexManager {
    /// Create a new index manager
    pub fn new() -> Self {
        Self {
            btree_indexes: Arc::new(RwLock::new(HashMap::new())),
            hash_indexes: Arc::new(RwLock::new(HashMap::new())),
            composite_indexes: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new index
    pub async fn create_index(&self, definition: IndexDefinition) -> Result<()> {
        match definition.index_type {
            IndexType::BTree => {
                if definition.columns.len() != 1 {
                    return Err(anyhow::anyhow!("B-tree index requires exactly one column"));
                }
                let name = definition.name.clone();
                let table = definition.table.clone();
                let column = definition.columns[0].clone();
                let index = BTreeIndex {
                    name: name.clone(),
                    table: table.clone(),
                    column: column.clone(),
                    unique: definition.unique,
                    index: Arc::new(RwLock::new(BTreeMap::new())),
                };
                let mut indexes = self.btree_indexes.write().await;
                indexes.insert(name.clone(), index);
                info!("Created B-tree index: {} on {}.{}", name, table, column);
            }
            IndexType::Hash => {
                if definition.columns.len() != 1 {
                    return Err(anyhow::anyhow!("Hash index requires exactly one column"));
                }
                let name = definition.name.clone();
                let table = definition.table.clone();
                let column = definition.columns[0].clone();
                let index = HashIndex {
                    name: name.clone(),
                    table: table.clone(),
                    column: column.clone(),
                    unique: definition.unique,
                    index: Arc::new(RwLock::new(HashMap::new())),
                };
                let mut indexes = self.hash_indexes.write().await;
                indexes.insert(name.clone(), index);
                info!("Created Hash index: {} on {}.{}", name, table, column);
            }
            IndexType::Composite => {
                if definition.columns.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Composite index requires at least one column"
                    ));
                }
                let name = definition.name.clone();
                let table = definition.table.clone();
                let columns = definition.columns.clone();
                let index = CompositeIndex {
                    name: name.clone(),
                    table: table.clone(),
                    columns: columns.clone(),
                    unique: definition.unique,
                    index: Arc::new(RwLock::new(BTreeMap::new())),
                };
                let mut indexes = self.composite_indexes.write().await;
                indexes.insert(name.clone(), index);
                info!(
                    "Created Composite index: {} on {}.{:?}",
                    name, table, columns
                );
            }
        }
        Ok(())
    }

    /// Drop an index
    pub async fn drop_index(&self, index_name: &str) -> Result<()> {
        let mut btree = self.btree_indexes.write().await;
        if btree.remove(index_name).is_some() {
            info!("Dropped B-tree index: {}", index_name);
            return Ok(());
        }
        drop(btree);

        let mut hash = self.hash_indexes.write().await;
        if hash.remove(index_name).is_some() {
            info!("Dropped Hash index: {}", index_name);
            return Ok(());
        }
        drop(hash);

        let mut composite = self.composite_indexes.write().await;
        if composite.remove(index_name).is_some() {
            info!("Dropped Composite index: {}", index_name);
            return Ok(());
        }

        Err(anyhow::anyhow!("Index not found: {}", index_name))
    }

    /// Insert a value into a B-tree index
    pub async fn btree_insert(
        &self,
        index_name: &str,
        value: String,
        row_id: String,
    ) -> Result<()> {
        let indexes = self.btree_indexes.read().await;
        let index = indexes
            .get(index_name)
            .ok_or_else(|| anyhow::anyhow!("B-tree index not found: {}", index_name))?;

        let mut idx = index.index.write().await;
        idx.entry(value).or_insert_with(Vec::new).push(row_id);
        Ok(())
    }

    /// Lookup values in a B-tree index (range query)
    pub async fn btree_lookup_range(
        &self,
        index_name: &str,
        min: Option<&str>,
        max: Option<&str>,
    ) -> Result<Vec<String>> {
        let indexes = self.btree_indexes.read().await;
        let index = indexes
            .get(index_name)
            .ok_or_else(|| anyhow::anyhow!("B-tree index not found: {}", index_name))?;

        let idx = index.index.read().await;
        let mut results = Vec::new();

        // Handle range bounds
        let range = match (min, max) {
            (Some(min_val), Some(max_val)) => idx.range(min_val.to_string()..=max_val.to_string()),
            (Some(min_val), None) => idx.range(min_val.to_string()..),
            (None, Some(max_val)) => idx.range(..=max_val.to_string()),
            (None, None) => idx.range::<String, _>(..),
        };

        for (_key, row_ids) in range {
            results.extend(row_ids.clone());
        }

        Ok(results)
    }

    /// Insert a value into a Hash index
    pub async fn hash_insert(&self, index_name: &str, value: String, row_id: String) -> Result<()> {
        let indexes = self.hash_indexes.read().await;
        let index = indexes
            .get(index_name)
            .ok_or_else(|| anyhow::anyhow!("Hash index not found: {}", index_name))?;

        let mut idx = index.index.write().await;
        idx.entry(value).or_insert_with(Vec::new).push(row_id);
        Ok(())
    }

    /// Lookup a value in a Hash index (equality only)
    pub async fn hash_lookup(&self, index_name: &str, value: &str) -> Result<Vec<String>> {
        let indexes = self.hash_indexes.read().await;
        let index = indexes
            .get(index_name)
            .ok_or_else(|| anyhow::anyhow!("Hash index not found: {}", index_name))?;

        let idx = index.index.read().await;
        Ok(idx.get(value).cloned().unwrap_or_default())
    }

    /// Insert values into a Composite index
    pub async fn composite_insert(
        &self,
        index_name: &str,
        values: Vec<String>,
        row_id: String,
    ) -> Result<()> {
        let indexes = self.composite_indexes.read().await;
        let index = indexes
            .get(index_name)
            .ok_or_else(|| anyhow::anyhow!("Composite index not found: {}", index_name))?;

        if values.len() != index.columns.len() {
            return Err(anyhow::anyhow!("Value count doesn't match column count"));
        }

        let mut idx = index.index.write().await;
        idx.entry(values).or_insert_with(Vec::new).push(row_id);
        Ok(())
    }

    /// Lookup values in a Composite index
    pub async fn composite_lookup(
        &self,
        index_name: &str,
        values: &[String],
    ) -> Result<Vec<String>> {
        let indexes = self.composite_indexes.read().await;
        let index = indexes
            .get(index_name)
            .ok_or_else(|| anyhow::anyhow!("Composite index not found: {}", index_name))?;

        if values.len() != index.columns.len() {
            return Err(anyhow::anyhow!("Value count doesn't match column count"));
        }

        let idx = index.index.read().await;
        Ok(idx.get(values).cloned().unwrap_or_default())
    }

    /// Find the best index for a query
    pub async fn find_best_index(
        &self,
        table: &str,
        column: &str,
        op: &crate::sql_query::FilterOp,
    ) -> Option<String> {
        // Check B-tree indexes (good for range queries)
        {
            let indexes = self.btree_indexes.read().await;
            for (name, index) in indexes.iter() {
                if index.table == table && index.column == column {
                    match op {
                        crate::sql_query::FilterOp::Equals
                        | crate::sql_query::FilterOp::GreaterThan
                        | crate::sql_query::FilterOp::LessThan
                        | crate::sql_query::FilterOp::GreaterThanOrEqual
                        | crate::sql_query::FilterOp::LessThanOrEqual => {
                            return Some(name.clone());
                        }
                        _ => {}
                    }
                }
            }
        }

        // Check Hash indexes (good for equality only)
        {
            let indexes = self.hash_indexes.read().await;
            for (name, index) in indexes.iter() {
                if index.table == table && index.column == column {
                    if matches!(op, crate::sql_query::FilterOp::Equals) {
                        return Some(name.clone());
                    }
                }
            }
        }

        None
    }

    /// Get index statistics
    pub async fn get_index_stats(&self, index_name: &str) -> Result<IndexStats> {
        // Check B-tree
        {
            let indexes = self.btree_indexes.read().await;
            if let Some(index) = indexes.get(index_name) {
                let idx = index.index.read().await;
                return Ok(IndexStats {
                    name: index_name.to_string(),
                    index_type: IndexType::BTree,
                    entry_count: idx.len(),
                    unique: index.unique,
                });
            }
        }

        // Check Hash
        {
            let indexes = self.hash_indexes.read().await;
            if let Some(index) = indexes.get(index_name) {
                let idx = index.index.read().await;
                return Ok(IndexStats {
                    name: index_name.to_string(),
                    index_type: IndexType::Hash,
                    entry_count: idx.len(),
                    unique: index.unique,
                });
            }
        }

        // Check Composite
        {
            let indexes = self.composite_indexes.read().await;
            if let Some(index) = indexes.get(index_name) {
                let idx = index.index.read().await;
                return Ok(IndexStats {
                    name: index_name.to_string(),
                    index_type: IndexType::Composite,
                    entry_count: idx.len(),
                    unique: index.unique,
                });
            }
        }

        Err(anyhow::anyhow!("Index not found: {}", index_name))
    }
}

/// Index statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub name: String,
    pub index_type: IndexType,
    pub entry_count: usize,
    pub unique: bool,
}

impl Default for IndexManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_btree_index() {
        let manager = IndexManager::new();

        let definition = IndexDefinition {
            name: "idx_owner_did".to_string(),
            table: "files".to_string(),
            columns: vec!["owner_did".to_string()],
            index_type: IndexType::BTree,
            unique: false,
        };

        manager.create_index(definition).await.unwrap();

        let stats = manager.get_index_stats("idx_owner_did").await.unwrap();
        assert_eq!(stats.index_type, IndexType::BTree);
        assert_eq!(stats.name, "idx_owner_did");
    }

    #[tokio::test]
    async fn test_create_hash_index() {
        let manager = IndexManager::new();

        let definition = IndexDefinition {
            name: "idx_email".to_string(),
            table: "users".to_string(),
            columns: vec!["email".to_string()],
            index_type: IndexType::Hash,
            unique: true,
        };

        manager.create_index(definition).await.unwrap();

        let stats = manager.get_index_stats("idx_email").await.unwrap();
        assert_eq!(stats.index_type, IndexType::Hash);
        assert_eq!(stats.unique, true);
    }

    #[tokio::test]
    async fn test_btree_insert_lookup() {
        let manager = IndexManager::new();

        let definition = IndexDefinition {
            name: "idx_test".to_string(),
            table: "files".to_string(),
            columns: vec!["owner_did".to_string()],
            index_type: IndexType::BTree,
            unique: false,
        };

        manager.create_index(definition).await.unwrap();

        manager
            .btree_insert(
                "idx_test",
                "did:spacekit:user:alice".to_string(),
                "file1".to_string(),
            )
            .await
            .unwrap();
        manager
            .btree_insert(
                "idx_test",
                "did:spacekit:user:alice".to_string(),
                "file2".to_string(),
            )
            .await
            .unwrap();

        let results = manager
            .btree_lookup_range(
                "idx_test",
                Some("did:spacekit:user:alice"),
                Some("did:spacekit:user:alice"),
            )
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_hash_insert_lookup() {
        let manager = IndexManager::new();

        let definition = IndexDefinition {
            name: "idx_email".to_string(),
            table: "users".to_string(),
            columns: vec!["email".to_string()],
            index_type: IndexType::Hash,
            unique: false,
        };

        manager.create_index(definition).await.unwrap();

        manager
            .hash_insert(
                "idx_email",
                "alice@example.com".to_string(),
                "user1".to_string(),
            )
            .await
            .unwrap();

        let results = manager
            .hash_lookup("idx_email", "alice@example.com")
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], "user1");
    }
}
