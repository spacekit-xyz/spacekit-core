//! High Availability (HA) Management for Storage Node
//!
//! Provides leader election, automatic failover, health monitoring,
//! and split-brain prevention for distributed storage nodes.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{interval, Interval};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Node role in the cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeRole {
    /// Leader node (handles writes)
    Leader,
    /// Follower node (replicates from leader)
    Follower,
    /// Candidate (participating in election)
    Candidate,
    /// Standalone (not in cluster)
    Standalone,
}

/// Node health status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

/// Node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: String,
    pub role: NodeRole,
    pub health: HealthStatus,
    pub last_heartbeat: DateTime<Utc>,
    pub address: String,
    pub port: u16,
}

/// Cluster state
#[derive(Debug, Clone)]
pub struct ClusterState {
    pub nodes: HashMap<String, NodeInfo>,
    pub leader_id: Option<String>,
    pub term: u64, // Raft-style term for election
    pub last_election: DateTime<Utc>,
}

/// High Availability Manager
pub struct HAManager {
    node_id: String,
    role: Arc<RwLock<NodeRole>>,
    cluster_state: Arc<RwLock<ClusterState>>,
    health_status: Arc<RwLock<HealthStatus>>,
    heartbeat_interval: Duration,
    election_timeout: Duration,
    health_check_interval: Duration,
}

impl HAManager {
    /// Create a new HA manager
    pub fn new(
        node_id: Option<String>,
        heartbeat_interval_secs: u64,
        election_timeout_secs: u64,
        health_check_interval_secs: u64,
    ) -> Self {
        let node_id = node_id.unwrap_or_else(|| Uuid::new_v4().to_string());

        Self {
            node_id: node_id.clone(),
            role: Arc::new(RwLock::new(NodeRole::Standalone)),
            cluster_state: Arc::new(RwLock::new(ClusterState {
                nodes: HashMap::new(),
                leader_id: None,
                term: 0,
                last_election: Utc::now(),
            })),
            health_status: Arc::new(RwLock::new(HealthStatus::Unknown)),
            heartbeat_interval: Duration::from_secs(heartbeat_interval_secs),
            election_timeout: Duration::from_secs(election_timeout_secs),
            health_check_interval: Duration::from_secs(health_check_interval_secs),
        }
    }

    /// Start HA management
    pub async fn start(&self) -> Result<()> {
        info!("Starting HA manager for node {}", self.node_id);

        // Initialize this node
        self.register_node().await?;

        // Start background tasks
        let health_checker = self.clone();
        tokio::spawn(async move {
            health_checker.health_check_loop().await;
        });

        let heartbeat_sender = self.clone();
        tokio::spawn(async move {
            heartbeat_sender.heartbeat_loop().await;
        });

        let election_monitor = self.clone();
        tokio::spawn(async move {
            election_monitor.election_monitor_loop().await;
        });

        Ok(())
    }

    /// Register this node in the cluster
    async fn register_node(&self) -> Result<()> {
        let mut cluster = self.cluster_state.write().await;
        let node_info = NodeInfo {
            node_id: self.node_id.clone(),
            role: *self.role.read().await,
            health: self.health_status.read().await.clone(),
            last_heartbeat: Utc::now(),
            address: "127.0.0.1".to_string(), // TODO: Get actual address
            port: 8080,                       // TODO: Get actual port
        };

        cluster.nodes.insert(self.node_id.clone(), node_info);
        info!("Node {} registered in cluster", self.node_id);
        Ok(())
    }

    /// Health check loop
    async fn health_check_loop(&self) {
        let mut interval = interval(self.health_check_interval);

        loop {
            interval.tick().await;

            let health = self.check_health().await;
            {
                let mut status = self.health_status.write().await;
                *status = health.clone();
            }

            // Update cluster state
            {
                let mut cluster = self.cluster_state.write().await;
                if let Some(node) = cluster.nodes.get_mut(&self.node_id) {
                    node.health = health.clone();
                    node.last_heartbeat = Utc::now();
                }
            }

            debug!("Node {} health check: {:?}", self.node_id, health);
        }
    }

    /// Check node health
    async fn check_health(&self) -> HealthStatus {
        // TODO: Implement actual health checks
        // - Database connectivity
        // - Disk space
        // - Memory usage
        // - Network connectivity

        // For now, always return healthy
        HealthStatus::Healthy
    }

    /// Heartbeat loop (only for leader)
    async fn heartbeat_loop(&self) {
        let mut interval = interval(self.heartbeat_interval);

        loop {
            interval.tick().await;

            let role = *self.role.read().await;
            if role == NodeRole::Leader {
                self.send_heartbeat().await;
            }
        }
    }

    /// Send heartbeat to followers
    async fn send_heartbeat(&self) {
        let followers: Vec<_> = {
            let cluster = self.cluster_state.read().await;
            cluster
                .nodes
                .values()
                .filter(|node| node.role == NodeRole::Follower)
                .cloned()
                .collect()
        };

        // TODO: Send actual heartbeat messages via network
        debug!(
            "Leader {} sending heartbeat to {} followers",
            self.node_id,
            followers.len()
        );
    }

    /// Election monitor loop
    async fn election_monitor_loop(&self) {
        let mut interval = interval(self.election_timeout);

        loop {
            interval.tick().await;

            let cluster = self.cluster_state.read().await;
            let has_leader = cluster.leader_id.is_some();
            let leader_last_seen = cluster.last_election;
            drop(cluster);

            // Check if leader is still alive
            let elapsed = Utc::now() - leader_last_seen;
            if has_leader && elapsed.num_seconds() > self.election_timeout.as_secs() as i64 {
                warn!("Leader appears to be down, starting election");
                self.start_election().await;
            }
        }
    }

    /// Start leader election
    async fn start_election(&self) {
        let mut cluster = self.cluster_state.write().await;
        cluster.term += 1;
        cluster.last_election = Utc::now();

        // Set this node as candidate
        let mut role = self.role.write().await;
        *role = NodeRole::Candidate;
        drop(role);

        // TODO: Request votes from other nodes
        // For now, assume this node wins if it's the only one

        if cluster.nodes.len() == 1 {
            // Single node cluster - become leader
            *self.role.write().await = NodeRole::Leader;
            cluster.leader_id = Some(self.node_id.clone());
            info!(
                "Node {} elected as leader (single node cluster)",
                self.node_id
            );
        } else {
            // Multi-node cluster - need to request votes
            // TODO: Implement Raft-style voting
            warn!("Multi-node election not yet implemented");
        }

        drop(cluster);
    }

    /// Get current node role
    pub async fn get_role(&self) -> NodeRole {
        *self.role.read().await
    }

    /// Check if this node is the leader
    pub async fn is_leader(&self) -> bool {
        *self.role.read().await == NodeRole::Leader
    }

    /// Get cluster state
    pub async fn get_cluster_state(&self) -> ClusterState {
        self.cluster_state.read().await.clone()
    }

    /// Get node health
    pub async fn get_health(&self) -> HealthStatus {
        self.health_status.read().await.clone()
    }

    /// Add a node to the cluster
    pub async fn add_node(&self, node_info: NodeInfo) -> Result<()> {
        let mut cluster = self.cluster_state.write().await;
        cluster.nodes.insert(node_info.node_id.clone(), node_info);
        info!("Node added to cluster");
        Ok(())
    }

    /// Remove a node from the cluster
    pub async fn remove_node(&self, node_id: &str) -> Result<()> {
        let mut cluster = self.cluster_state.write().await;

        // If removing the leader, trigger election
        if cluster.leader_id.as_ref() == Some(&node_id.to_string()) {
            cluster.leader_id = None;
            drop(cluster);
            self.start_election().await;
        } else {
            cluster.nodes.remove(node_id);
            drop(cluster);
        }

        info!("Node {} removed from cluster", node_id);
        Ok(())
    }

    /// Handle failover (promote follower to leader)
    pub async fn handle_failover(&self) -> Result<()> {
        let mut cluster = self.cluster_state.write().await;

        if cluster.leader_id.is_none() {
            // No leader - start election
            drop(cluster);
            self.start_election().await;
        } else {
            // Check if leader is actually down
            if let Some(leader_id) = &cluster.leader_id {
                if let Some(leader) = cluster.nodes.get(leader_id) {
                    let elapsed = Utc::now() - leader.last_heartbeat;
                    if elapsed.num_seconds() > self.election_timeout.as_secs() as i64 {
                        // Leader is down
                        warn!("Leader {} is down, initiating failover", leader_id);
                        cluster.leader_id = None;
                        drop(cluster);
                        self.start_election().await;
                    }
                }
            }
        }

        Ok(())
    }

    /// Prevent split-brain by ensuring only one leader
    pub async fn prevent_split_brain(&self) -> Result<()> {
        let cluster = self.cluster_state.read().await;

        // Count nodes claiming to be leader
        let leader_count = cluster
            .nodes
            .values()
            .filter(|node| node.role == NodeRole::Leader)
            .count();

        if leader_count > 1 {
            error!(
                "Split-brain detected! {} nodes claim to be leader",
                leader_count
            );
            // Resolve by electing the node with highest term
            // TODO: Implement proper split-brain resolution
        }

        Ok(())
    }
}

impl Clone for HAManager {
    fn clone(&self) -> Self {
        Self {
            node_id: self.node_id.clone(),
            role: self.role.clone(),
            cluster_state: self.cluster_state.clone(),
            health_status: self.health_status.clone(),
            heartbeat_interval: self.heartbeat_interval,
            election_timeout: self.election_timeout,
            health_check_interval: self.health_check_interval,
        }
    }
}
