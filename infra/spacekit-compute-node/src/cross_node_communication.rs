//! Cross-Node Communication Module for SWTCH Compute Node
//!
//! This module provides communication capabilities between compute nodes and storage nodes,
//! including service discovery, health monitoring, load balancing, and failover mechanisms.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, warn};

// Import storage node types (using crate re-exports)
// use crate::{SwtchvmStorageNode, SwtchvmStorageConfig};

/// Storage Node Information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageNodeInfo {
    pub node_id: String,
    pub did: String,
    pub endpoint: String,
    pub capacity: u64,
    pub used_storage: u64,
    pub reputation_score: f64,
    pub last_seen: u64,
    pub status: NodeStatus,
    pub supported_algorithms: Vec<String>,
}

/// Node Status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodeStatus {
    Online,
    Offline,
    Degraded,
    Maintenance,
}

/// Health Check Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub node_id: String,
    pub status: NodeStatus,
    pub response_time_ms: u64,
    pub capacity: u64,
    pub used_storage: u64,
    pub reputation_score: f64,
    pub checked_at: u64,
}

/// Load Balancing Strategy
#[derive(Debug, Clone)]
pub enum LoadBalancingStrategy {
    RoundRobin,
    LeastUsed,
    ByReputation,
    ByProximity,
    Hybrid,
}

/// Cross-Node Communication Manager
///
/// This manager handles all communication between compute nodes and storage nodes,
/// providing service discovery, health monitoring, and load balancing.
pub struct CrossNodeCommunicationManager {
    pub storage_nodes: Arc<RwLock<HashMap<String, StorageNodeInfo>>>,
    pub health_check_interval: Duration,
    pub load_balancing_strategy: LoadBalancingStrategy,
    pub max_retry_attempts: usize,
    pub connection_timeout: Duration,
}

impl CrossNodeCommunicationManager {
    /// Create a new cross-node communication manager
    pub fn new(
        health_check_interval: Duration,
        load_balancing_strategy: LoadBalancingStrategy,
    ) -> Self {
        Self {
            storage_nodes: Arc::new(RwLock::new(HashMap::new())),
            health_check_interval,
            load_balancing_strategy,
            max_retry_attempts: 3,
            connection_timeout: Duration::from_secs(30),
        }
    }

    /// Discover storage nodes in the network
    pub async fn discover_storage_nodes(&self) -> Result<Vec<StorageNodeInfo>> {
        info!("Discovering storage nodes in the network");

        // In production, this would use P2P discovery mechanisms
        // For now, we'll return a mock list of storage nodes
        let mut discovered_nodes = Vec::new();

        // Mock storage nodes for demonstration
        for i in 1..=3 {
            let node = StorageNodeInfo {
                node_id: format!("storage_node_{}", i),
                did: format!("did:swtch:storage:node{}", i),
                endpoint: format!("http://storage-node-{}.swtch.network:4001", i),
                capacity: 100 * 1024 * 1024 * 1024,    // 100GB
                used_storage: 30 * 1024 * 1024 * 1024, // 30GB used
                reputation_score: 0.8 + (i as f64 * 0.05),
                last_seen: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                status: NodeStatus::Online,
                supported_algorithms: vec![
                    "kyber1024".to_string(),
                    "kyber768".to_string(),
                    "aes256".to_string(),
                ],
            };
            discovered_nodes.push(node);
        }

        // Register discovered nodes
        let mut nodes = self.storage_nodes.write().await;
        for node in &discovered_nodes {
            nodes.insert(node.node_id.clone(), node.clone());
        }

        info!("Discovered {} storage nodes", discovered_nodes.len());
        Ok(discovered_nodes)
    }

    /// Register a storage node
    pub async fn register_storage_node(&self, node_info: StorageNodeInfo) -> Result<()> {
        info!("Registering storage node: {}", node_info.node_id);

        let mut nodes = self.storage_nodes.write().await;
        nodes.insert(node_info.node_id.clone(), node_info);

        Ok(())
    }

    /// Perform health check on a storage node
    pub async fn health_check_node(&self, node_id: &str) -> Result<HealthCheckResult> {
        debug!("Performing health check on node: {}", node_id);

        let nodes = self.storage_nodes.read().await;
        let node = nodes
            .get(node_id)
            .ok_or_else(|| anyhow::anyhow!("Storage node not found: {}", node_id))?;

        let start_time = SystemTime::now();

        // In production, this would perform actual health checks
        // For now, we'll simulate a health check
        let health_check_success = self.simulate_health_check(&node.endpoint).await?;

        let response_time = start_time.elapsed().unwrap().as_millis() as u64;

        let status = if health_check_success {
            if response_time < 1000 {
                NodeStatus::Online
            } else {
                NodeStatus::Degraded
            }
        } else {
            NodeStatus::Offline
        };

        let result = HealthCheckResult {
            node_id: node_id.to_string(),
            status: status.clone(),
            response_time_ms: response_time,
            capacity: node.capacity,
            used_storage: node.used_storage,
            reputation_score: node.reputation_score,
            checked_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Update node status
        drop(nodes);
        let mut nodes = self.storage_nodes.write().await;
        if let Some(mut node_info) = nodes.get_mut(node_id) {
            node_info.status = status;
            node_info.last_seen = result.checked_at;
        }

        Ok(result)
    }

    /// Perform health checks on all registered storage nodes
    pub async fn health_check_all_nodes(&self) -> Result<Vec<HealthCheckResult>> {
        info!("Performing health checks on all storage nodes");

        let node_ids: Vec<String> = {
            let nodes = self.storage_nodes.read().await;
            nodes.keys().cloned().collect()
        };

        let mut results = Vec::new();
        for node_id in node_ids {
            match self.health_check_node(&node_id).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    warn!("Health check failed for node {}: {}", node_id, e);
                }
            }
        }

        Ok(results)
    }

    /// Start periodic health monitoring
    pub async fn start_health_monitoring(&self) -> Result<()> {
        info!(
            "Starting periodic health monitoring with interval: {:?}",
            self.health_check_interval
        );

        let manager = self.clone();
        tokio::spawn(async move {
            let mut interval = interval(manager.health_check_interval);

            loop {
                interval.tick().await;

                if let Err(e) = manager.health_check_all_nodes().await {
                    error!("Health monitoring error: {}", e);
                }
            }
        });

        Ok(())
    }

    /// Select the best storage node for a given task using load balancing
    pub async fn select_storage_node(&self, required_capacity: u64) -> Result<String> {
        debug!(
            "Selecting storage node with required capacity: {} bytes",
            required_capacity
        );

        let nodes = self.storage_nodes.read().await;
        let available_nodes: Vec<&StorageNodeInfo> = nodes
            .values()
            .filter(|node| {
                matches!(node.status, NodeStatus::Online | NodeStatus::Degraded)
                    && (node.capacity - node.used_storage) >= required_capacity
            })
            .collect();

        if available_nodes.is_empty() {
            return Err(anyhow::anyhow!(
                "No available storage nodes with sufficient capacity"
            ));
        }

        let selected_node = match self.load_balancing_strategy {
            LoadBalancingStrategy::RoundRobin => {
                // Simple round-robin selection
                &available_nodes[0]
            }
            LoadBalancingStrategy::LeastUsed => {
                // Select node with least used storage
                available_nodes
                    .iter()
                    .min_by(|a, b| a.used_storage.cmp(&b.used_storage))
                    .unwrap()
            }
            LoadBalancingStrategy::ByReputation => {
                // Select node with highest reputation
                available_nodes
                    .iter()
                    .max_by(|a, b| a.reputation_score.partial_cmp(&b.reputation_score).unwrap())
                    .unwrap()
            }
            LoadBalancingStrategy::ByProximity => {
                // For now, just select first available (proximity would require geo data)
                &available_nodes[0]
            }
            LoadBalancingStrategy::Hybrid => {
                // Combine reputation and least used
                available_nodes
                    .iter()
                    .max_by(|a, b| {
                        let score_a = a.reputation_score * 0.7
                            + (1.0 - (a.used_storage as f64 / a.capacity as f64)) * 0.3;
                        let score_b = b.reputation_score * 0.7
                            + (1.0 - (b.used_storage as f64 / b.capacity as f64)) * 0.3;
                        score_a.partial_cmp(&score_b).unwrap()
                    })
                    .unwrap()
            }
        };

        info!(
            "Selected storage node: {} (reputation: {:.2}, utilization: {:.1}%)",
            selected_node.node_id,
            selected_node.reputation_score,
            (selected_node.used_storage as f64 / selected_node.capacity as f64) * 100.0
        );

        Ok(selected_node.node_id.clone())
    }

    /// Implement failover mechanism
    pub async fn handle_node_failure(&self, failed_node_id: &str) -> Result<String> {
        warn!("Handling failure for storage node: {}", failed_node_id);

        // Mark the node as offline
        {
            let mut nodes = self.storage_nodes.write().await;
            if let Some(node) = nodes.get_mut(failed_node_id) {
                node.status = NodeStatus::Offline;
            }
        }

        // Select an alternative node
        let alternative_node = self.select_storage_node(0).await?;

        info!(
            "Failover completed: {} -> {}",
            failed_node_id, alternative_node
        );
        Ok(alternative_node)
    }

    /// Get storage node statistics
    pub async fn get_storage_statistics(&self) -> Result<StorageNetworkStats> {
        let nodes = self.storage_nodes.read().await;

        let total_nodes = nodes.len();
        let online_nodes = nodes
            .values()
            .filter(|n| matches!(n.status, NodeStatus::Online))
            .count();
        let total_capacity = nodes.values().map(|n| n.capacity).sum();
        let total_used = nodes.values().map(|n| n.used_storage).sum();
        let average_reputation = if total_nodes > 0 {
            nodes.values().map(|n| n.reputation_score).sum::<f64>() / total_nodes as f64
        } else {
            0.0
        };

        Ok(StorageNetworkStats {
            total_nodes,
            online_nodes,
            total_capacity,
            total_used,
            utilization: if total_capacity > 0 {
                (total_used as f64 / total_capacity as f64) * 100.0
            } else {
                0.0
            },
            average_reputation,
        })
    }

    /// Create a storage node connection
    pub async fn create_storage_connection(&self, node_id: &str) -> Result<String> {
        debug!("Creating storage connection to node: {}", node_id);

        let nodes = self.storage_nodes.read().await;
        let _node = nodes
            .get(node_id)
            .ok_or_else(|| anyhow::anyhow!("Storage node not found: {}", node_id))?;

        // Create the storage node connection (placeholder implementation)
        let connection_id = format!("connection_to_{}", node_id);

        info!("Created storage connection to node: {}", node_id);
        Ok(connection_id)
    }

    // Private helper methods
    async fn simulate_health_check(&self, endpoint: &str) -> Result<bool> {
        // Simulate network latency and potential failures
        sleep(Duration::from_millis(50)).await;

        // 95% success rate for simulation
        Ok(rand::random::<f64>() < 0.95)
    }
}

impl Clone for CrossNodeCommunicationManager {
    fn clone(&self) -> Self {
        Self {
            storage_nodes: self.storage_nodes.clone(),
            health_check_interval: self.health_check_interval,
            load_balancing_strategy: self.load_balancing_strategy.clone(),
            max_retry_attempts: self.max_retry_attempts,
            connection_timeout: self.connection_timeout,
        }
    }
}

/// Storage Network Statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageNetworkStats {
    pub total_nodes: usize,
    pub online_nodes: usize,
    pub total_capacity: u64,
    pub total_used: u64,
    pub utilization: f64,
    pub average_reputation: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_cross_node_manager_creation() {
        let manager = CrossNodeCommunicationManager::new(
            Duration::from_secs(30),
            LoadBalancingStrategy::Hybrid,
        );

        assert_eq!(manager.health_check_interval, Duration::from_secs(30));
        assert!(matches!(
            manager.load_balancing_strategy,
            LoadBalancingStrategy::Hybrid
        ));
    }

    #[tokio::test]
    async fn test_storage_node_discovery() {
        let manager = CrossNodeCommunicationManager::new(
            Duration::from_secs(30),
            LoadBalancingStrategy::RoundRobin,
        );

        let discovered = manager.discover_storage_nodes().await.unwrap();
        assert!(!discovered.is_empty());

        let nodes = manager.storage_nodes.read().await;
        assert_eq!(nodes.len(), discovered.len());
    }

    #[tokio::test]
    async fn test_storage_node_registration() {
        let manager = CrossNodeCommunicationManager::new(
            Duration::from_secs(30),
            LoadBalancingStrategy::RoundRobin,
        );

        let node_info = StorageNodeInfo {
            node_id: "test_node".to_string(),
            did: "did:swtch:test".to_string(),
            endpoint: "http://test.example.com".to_string(),
            capacity: 1024 * 1024 * 1024,
            used_storage: 0,
            reputation_score: 0.9,
            last_seen: 0,
            status: NodeStatus::Online,
            supported_algorithms: vec!["kyber1024".to_string()],
        };

        manager.register_storage_node(node_info).await.unwrap();

        let nodes = manager.storage_nodes.read().await;
        assert!(nodes.contains_key("test_node"));
    }

    #[tokio::test]
    async fn test_health_check() {
        let manager = CrossNodeCommunicationManager::new(
            Duration::from_secs(30),
            LoadBalancingStrategy::RoundRobin,
        );

        // Register a test node
        let node_info = StorageNodeInfo {
            node_id: "health_test_node".to_string(),
            did: "did:swtch:test".to_string(),
            endpoint: "http://test.example.com".to_string(),
            capacity: 1024 * 1024 * 1024,
            used_storage: 0,
            reputation_score: 0.9,
            last_seen: 0,
            status: NodeStatus::Online,
            supported_algorithms: vec!["kyber1024".to_string()],
        };

        manager.register_storage_node(node_info).await.unwrap();

        // Perform health check
        let result = manager.health_check_node("health_test_node").await.unwrap();
        assert_eq!(result.node_id, "health_test_node");
        assert!(matches!(
            result.status,
            NodeStatus::Online | NodeStatus::Degraded
        ));
    }

    #[tokio::test]
    async fn test_load_balancing_strategies() {
        let manager = CrossNodeCommunicationManager::new(
            Duration::from_secs(30),
            LoadBalancingStrategy::ByReputation,
        );

        // Register nodes with different characteristics
        for i in 1..=3 {
            let node_info = StorageNodeInfo {
                node_id: format!("load_test_node_{}", i),
                did: format!("did:swtch:test{}", i),
                endpoint: format!("http://test{}.example.com", i),
                capacity: 1024 * 1024 * 1024,
                used_storage: i * 100 * 1024 * 1024, // Different usage levels
                reputation_score: 0.5 + (i as f64 * 0.1), // Different reputation scores
                last_seen: 0,
                status: NodeStatus::Online,
                supported_algorithms: vec!["kyber1024".to_string()],
            };
            manager.register_storage_node(node_info).await.unwrap();
        }

        let selected = manager
            .select_storage_node(100 * 1024 * 1024)
            .await
            .unwrap();
        assert!(selected.starts_with("load_test_node_"));
    }

    #[tokio::test]
    async fn test_failover_mechanism() {
        let manager = CrossNodeCommunicationManager::new(
            Duration::from_secs(30),
            LoadBalancingStrategy::RoundRobin,
        );

        // Register multiple nodes
        for i in 1..=3 {
            let node_info = StorageNodeInfo {
                node_id: format!("failover_test_node_{}", i),
                did: format!("did:swtch:test{}", i),
                endpoint: format!("http://test{}.example.com", i),
                capacity: 1024 * 1024 * 1024,
                used_storage: 0,
                reputation_score: 0.8,
                last_seen: 0,
                status: NodeStatus::Online,
                supported_algorithms: vec!["kyber1024".to_string()],
            };
            manager.register_storage_node(node_info).await.unwrap();
        }

        // Simulate node failure
        let alternative = manager
            .handle_node_failure("failover_test_node_1")
            .await
            .unwrap();
        assert_ne!(alternative, "failover_test_node_1");

        // Check that the failed node is marked as offline
        let nodes = manager.storage_nodes.read().await;
        let failed_node = nodes.get("failover_test_node_1").unwrap();
        assert!(matches!(failed_node.status, NodeStatus::Offline));
    }

    #[tokio::test]
    async fn test_storage_statistics() {
        let manager = CrossNodeCommunicationManager::new(
            Duration::from_secs(30),
            LoadBalancingStrategy::RoundRobin,
        );

        // Register nodes
        for i in 1..=3 {
            let node_info = StorageNodeInfo {
                node_id: format!("stats_test_node_{}", i),
                did: format!("did:swtch:test{}", i),
                endpoint: format!("http://test{}.example.com", i),
                capacity: 1024 * 1024 * 1024,
                used_storage: 500 * 1024 * 1024,
                reputation_score: 0.8,
                last_seen: 0,
                status: NodeStatus::Online,
                supported_algorithms: vec!["kyber1024".to_string()],
            };
            manager.register_storage_node(node_info).await.unwrap();
        }

        let stats = manager.get_storage_statistics().await.unwrap();
        assert_eq!(stats.total_nodes, 3);
        assert_eq!(stats.online_nodes, 3);
        assert!(stats.utilization > 0.0);
        // Use approximate equality for floating point comparison
        assert!((stats.average_reputation - 0.8).abs() < 0.001);
    }
}
