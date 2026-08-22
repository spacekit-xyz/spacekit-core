//! Horizontal Sharding for Storage Node
//!
//! Provides shard key selection, shard routing, cross-shard queries,
//! and shard rebalancing for distributed storage.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::database::Database;

/// Shard key type
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShardKey {
    /// Hash-based sharding (consistent hashing)
    Hash(String),
    /// Range-based sharding
    Range { min: String, max: String },
    /// List-based sharding (specific values)
    List(Vec<String>),
}

/// Shard configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardConfig {
    pub shard_id: String,
    pub node_id: String,
    pub shard_key_type: ShardKeyType,
    pub shard_key_field: String,
    pub replica_count: usize,
}

/// Shard key type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShardKeyType {
    Hash,
    Range,
    List,
}

/// Shard metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardMetadata {
    pub shard_id: String,
    pub node_id: String,
    pub key_range: Option<(String, String)>, // For range sharding
    pub key_list: Option<Vec<String>>,       // For list sharding
    pub replica_nodes: Vec<String>,
    pub data_count: usize,
    pub last_rebalanced: chrono::DateTime<chrono::Utc>,
}

/// Shard manager
pub struct ShardManager {
    shards: Arc<RwLock<HashMap<String, ShardMetadata>>>,
    databases: Arc<RwLock<HashMap<String, Arc<Database>>>>,
    shard_key_field: String,
    shard_key_type: ShardKeyType,
}

impl ShardManager {
    /// Create a new shard manager
    pub fn new(shard_key_field: String, shard_key_type: ShardKeyType) -> Self {
        Self {
            shards: Arc::new(RwLock::new(HashMap::new())),
            databases: Arc::new(RwLock::new(HashMap::new())),
            shard_key_field,
            shard_key_type,
        }
    }

    /// Add a shard
    pub async fn add_shard(
        &self,
        shard_id: String,
        node_id: String,
        database: Arc<Database>,
    ) -> Result<()> {
        let mut shards = self.shards.write().await;
        let mut databases = self.databases.write().await;

        let metadata = ShardMetadata {
            shard_id: shard_id.clone(),
            node_id: node_id.clone(),
            key_range: None,
            key_list: None,
            replica_nodes: vec![node_id],
            data_count: 0,
            last_rebalanced: chrono::Utc::now(),
        };

        let shard_id_clone = shard_id.clone();
        shards.insert(shard_id_clone.clone(), metadata);
        databases.insert(shard_id_clone.clone(), database);

        info!("Shard added: {}", shard_id_clone);
        Ok(())
    }

    /// Route a key to the appropriate shard
    pub async fn route_to_shard(&self, key: &str) -> Result<String> {
        debug!("Routing key using shard field '{}'", self.shard_key_field);
        match self.shard_key_type {
            ShardKeyType::Hash => {
                // Consistent hashing
                let hash = self.hash_key(key);
                let shards = self.shards.read().await;
                let shard_count = shards.len();
                if shard_count == 0 {
                    return Err(anyhow::anyhow!("No shards available"));
                }
                let shard_index = (hash % shard_count as u64) as usize;
                let shard_ids: Vec<_> = shards.keys().collect();
                Ok(shard_ids[shard_index].clone())
            }
            ShardKeyType::Range => {
                // Range-based routing
                let shards = self.shards.read().await;
                for (shard_id, metadata) in shards.iter() {
                    if let Some((min, max)) = &metadata.key_range {
                        if key >= min.as_str() && key <= max.as_str() {
                            return Ok(shard_id.clone());
                        }
                    }
                }
                // Default to first shard if no range matches
                shards
                    .keys()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No shards available"))
                    .map(|s| s.clone())
            }
            ShardKeyType::List => {
                // List-based routing
                let shards = self.shards.read().await;
                for (shard_id, metadata) in shards.iter() {
                    if let Some(list) = &metadata.key_list {
                        if list.contains(&key.to_string()) {
                            return Ok(shard_id.clone());
                        }
                    }
                }
                // Default to first shard if no list matches
                shards
                    .keys()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No shards available"))
                    .map(|s| s.clone())
            }
        }
    }

    /// Get database for a shard
    pub async fn get_shard_database(&self, shard_id: &str) -> Result<Arc<Database>> {
        let databases = self.databases.read().await;
        databases
            .get(shard_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Shard not found: {}", shard_id))
    }

    /// Execute a cross-shard query
    pub async fn execute_cross_shard_query<F, T>(&self, query_fn: F) -> Result<Vec<T>>
    where
        F: Fn(
            Arc<Database>,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<T>>> + Send>>,
        T: Send + 'static,
    {
        let databases = self.databases.read().await;
        let mut results = Vec::new();

        // Execute query on all shards in parallel
        let mut futures = Vec::new();
        for db in databases.values() {
            futures.push(query_fn(db.clone()));
        }

        // Wait for all queries to complete
        for future in futures {
            match future.await {
                Ok(shard_results) => results.extend(shard_results),
                Err(e) => {
                    warn!("Cross-shard query failed on one shard: {}", e);
                }
            }
        }

        Ok(results)
    }

    /// Rebalance shards (move data between shards for load balancing)
    pub async fn rebalance_shards(&self) -> Result<()> {
        let shards = self.shards.read().await;
        let mut shard_loads: Vec<_> = shards
            .values()
            .map(|s| (s.shard_id.clone(), s.data_count))
            .collect();

        // Sort by load (descending)
        shard_loads.sort_by(|a, b| b.1.cmp(&a.1));

        let total_load: usize = shard_loads.iter().map(|(_, load)| load).sum();
        let avg_load = if shard_loads.len() > 0 {
            total_load / shard_loads.len()
        } else {
            0
        };

        // Identify overloaded and underloaded shards
        let overloaded: Vec<_> = shard_loads
            .iter()
            .filter(|(_, load)| *load > avg_load * 2)
            .collect();

        let underloaded: Vec<_> = shard_loads
            .iter()
            .filter(|(_, load)| *load < avg_load / 2)
            .collect();

        if !overloaded.is_empty() && !underloaded.is_empty() {
            info!(
                "Rebalancing: {} overloaded shards, {} underloaded shards",
                overloaded.len(),
                underloaded.len()
            );
            // TODO: Implement actual data migration
        }

        Ok(())
    }

    /// Hash a key for consistent hashing
    fn hash_key(&self, key: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    /// Get shard statistics
    pub async fn get_shard_stats(&self) -> Result<ShardStats> {
        let shards = self.shards.read().await;
        let total_shards = shards.len();
        let total_data: usize = shards.values().map(|s| s.data_count).sum();
        let avg_data_per_shard = if total_shards > 0 {
            total_data / total_shards
        } else {
            0
        };

        Ok(ShardStats {
            total_shards,
            total_data,
            avg_data_per_shard,
            shard_details: shards.values().cloned().collect(),
        })
    }
}

/// Shard statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardStats {
    pub total_shards: usize,
    pub total_data: usize,
    pub avg_data_per_shard: usize,
    pub shard_details: Vec<ShardMetadata>,
}
