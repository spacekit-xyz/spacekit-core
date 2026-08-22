//! Cross-Server P2P Routing
//!
//! Integrates with spacekit-simulator's cross-network bridge to enable
//! P2P communication between servers in the multi-node architecture.
//!
//! Features:
//! - Connect to remote servers via cross-network bridge
//! - Subscribe to server Gossipsub topics
//! - Route messages between servers
//! - Handle NAT traversal for server connections

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::database::Server;
use uuid::Uuid;

/// Server routing manager for cross-server P2P communication
pub struct ServerRoutingManager {
    /// Active connections to remote servers
    server_connections: Arc<RwLock<HashMap<String, ServerConnection>>>,
    /// Local server ID (if this node is a server)
    local_server_id: Option<String>,
    /// Cross-network bridge (from spacekit-simulator)
    cross_network_bridge: Option<Arc<dyn CrossNetworkBridgeTrait>>,
}

/// Connection to a remote server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConnection {
    pub server_id: String,
    pub server: Server,
    pub connection_id: String,
    pub status: ServerConnectionStatus,
    pub established_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub subscribed_topics: Vec<String>,
    pub connection_metrics: ServerConnectionMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
    Failed,
    Reconnecting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConnectionMetrics {
    pub messages_sent: u64,
    pub messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub avg_latency_ms: f64,
    pub last_error: Option<String>,
}

/// Trait for cross-network bridge operations
/// This allows us to use spacekit-simulator's CrossNetworkBridge
/// without directly depending on it
#[async_trait::async_trait]
pub trait CrossNetworkBridgeTrait: Send + Sync {
    /// Establish connection to remote peer
    async fn establish_connection(&self, endpoint: &str, peer_did: &str) -> Result<String>;

    /// Send message to remote peer
    async fn send_message(&self, connection_id: &str, message: &[u8]) -> Result<()>;

    /// Get connection status
    async fn get_connection_status(&self, connection_id: &str) -> Result<ConnectionStatus>;

    /// Close connection
    async fn close_connection(&self, connection_id: &str) -> Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Failed,
}

impl ServerRoutingManager {
    /// Create new server routing manager
    pub fn new(local_server_id: Option<String>) -> Self {
        Self {
            server_connections: Arc::new(RwLock::new(HashMap::new())),
            local_server_id,
            cross_network_bridge: None,
        }
    }

    /// Set cross-network bridge
    pub fn set_cross_network_bridge(&mut self, bridge: Arc<dyn CrossNetworkBridgeTrait>) {
        self.cross_network_bridge = Some(bridge);
    }

    /// Connect to a remote server
    pub async fn connect_to_server(&self, server: Server) -> Result<String> {
        info!("🔗 Connecting to server: {} ({})", server.name, server.id);

        if let Some(local_id) = &self.local_server_id {
            if local_id == &server.id {
                return Err(anyhow::anyhow!(
                    "Refusing to connect to local server {}",
                    server.id
                ));
            }
        }

        // Check if already connected
        {
            let connections = self.server_connections.read().await;
            if let Some(existing) = connections.get(&server.id) {
                if matches!(existing.status, ServerConnectionStatus::Connected) {
                    info!("Already connected to server: {}", server.id);
                    return Ok(existing.connection_id.clone());
                }
            }
        }

        // Parse server endpoint (multiaddr format: /ip4/127.0.0.1/tcp/7000)
        let endpoint = self.parse_multiaddr(&server.endpoint)?;

        // Get cross-network bridge
        let bridge = self
            .cross_network_bridge
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cross-network bridge not configured"))?;

        // Establish connection via cross-network bridge
        let connection_id = bridge
            .establish_connection(&endpoint, &server.owner_did)
            .await?;

        // Create server connection
        let server_connection = ServerConnection {
            server_id: server.id.clone(),
            server: server.clone(),
            connection_id: connection_id.clone(),
            status: ServerConnectionStatus::Connected,
            established_at: Utc::now(),
            last_activity: Utc::now(),
            subscribed_topics: Vec::new(),
            connection_metrics: ServerConnectionMetrics {
                messages_sent: 0,
                messages_received: 0,
                bytes_sent: 0,
                bytes_received: 0,
                avg_latency_ms: 0.0,
                last_error: None,
            },
        };

        // Store connection
        {
            let mut connections = self.server_connections.write().await;
            connections.insert(server.id.clone(), server_connection);
        }

        info!(
            "✅ Connected to server: {} (connection_id: {})",
            server.name, connection_id
        );

        Ok(connection_id)
    }

    /// Disconnect from a server
    pub async fn disconnect_from_server(&self, server_id: &str) -> Result<()> {
        info!("🔌 Disconnecting from server: {}", server_id);

        let mut connections = self.server_connections.write().await;

        if let Some(connection) = connections.get(server_id) {
            // Close connection via bridge
            if let Some(bridge) = &self.cross_network_bridge {
                if let Err(e) = bridge.close_connection(&connection.connection_id).await {
                    warn!("Failed to close bridge connection: {}", e);
                }
            }

            // Remove from connections
            connections.remove(server_id);
        }

        Ok(())
    }

    /// Send message to server
    pub async fn send_message_to_server(&self, server_id: &str, message: &[u8]) -> Result<()> {
        let connections = self.server_connections.read().await;

        let connection = connections
            .get(server_id)
            .ok_or_else(|| anyhow::anyhow!("Not connected to server: {}", server_id))?;

        if !matches!(connection.status, ServerConnectionStatus::Connected) {
            return Err(anyhow::anyhow!(
                "Server connection not active: {:?}",
                connection.status
            ));
        }

        let bridge = self
            .cross_network_bridge
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cross-network bridge not configured"))?;

        bridge
            .send_message(&connection.connection_id, message)
            .await?;

        // Update metrics
        {
            let mut connections = self.server_connections.write().await;
            if let Some(conn) = connections.get_mut(server_id) {
                conn.connection_metrics.messages_sent += 1;
                conn.connection_metrics.bytes_sent += message.len() as u64;
                conn.last_activity = Utc::now();
            }
        }

        Ok(())
    }

    /// Get all connected servers
    pub async fn get_connected_servers(&self) -> Vec<Server> {
        let connections = self.server_connections.read().await;
        connections
            .values()
            .filter(|conn| matches!(conn.status, ServerConnectionStatus::Connected))
            .map(|conn| conn.server.clone())
            .collect()
    }

    /// Get connection status for a server
    pub async fn get_server_connection_status(
        &self,
        server_id: &str,
    ) -> Option<ServerConnectionStatus> {
        let connections = self.server_connections.read().await;
        connections.get(server_id).map(|conn| conn.status.clone())
    }

    /// Subscribe to server topic (Gossipsub)
    pub async fn subscribe_to_server_topic(&self, server_id: &str, topic: String) -> Result<()> {
        info!(
            "📡 Subscribing to topic '{}' on server: {}",
            topic, server_id
        );

        let mut connections = self.server_connections.write().await;

        let connection = connections
            .get_mut(server_id)
            .ok_or_else(|| anyhow::anyhow!("Not connected to server: {}", server_id))?;

        if !connection.subscribed_topics.contains(&topic) {
            connection.subscribed_topics.push(topic.clone());
        }

        // TODO: Actually subscribe via messaging node Gossipsub
        // This would require integration with spacekit-messaging-node

        Ok(())
    }

    /// Unsubscribe from server topic
    pub async fn unsubscribe_from_server_topic(&self, server_id: &str, topic: &str) -> Result<()> {
        info!(
            "📡 Unsubscribing from topic '{}' on server: {}",
            topic, server_id
        );

        let mut connections = self.server_connections.write().await;

        let connection = connections
            .get_mut(server_id)
            .ok_or_else(|| anyhow::anyhow!("Not connected to server: {}", server_id))?;

        connection.subscribed_topics.retain(|t| t != topic);

        // TODO: Actually unsubscribe via messaging node Gossipsub

        Ok(())
    }

    /// Parse multiaddr format endpoint to connection string
    /// Example: /ip4/127.0.0.1/tcp/7000 -> http://127.0.0.1:7000
    fn parse_multiaddr(&self, multiaddr: &str) -> Result<String> {
        // Simple parser for /ip4/ADDR/tcp/PORT format
        let parts: Vec<&str> = multiaddr.split('/').collect();

        let mut ip = None;
        let mut port = None;

        for (i, part) in parts.iter().enumerate() {
            if part == &"ip4" && i + 1 < parts.len() {
                ip = Some(parts[i + 1]);
            } else if part == &"tcp" && i + 1 < parts.len() {
                port = Some(parts[i + 1]);
            }
        }

        let ip = ip.ok_or_else(|| anyhow::anyhow!("Invalid multiaddr: missing IP"))?;
        let port = port.ok_or_else(|| anyhow::anyhow!("Invalid multiaddr: missing port"))?;

        Ok(format!("http://{}:{}", ip, port))
    }
}

impl Default for ServerRoutingManager {
    fn default() -> Self {
        Self::new(None)
    }
}

/// Simple bridge adapter for when cross-network bridge is not available
/// This allows the server routing to work even without spacekit-simulator
pub struct SimpleBridgeAdapter {
    connections: Arc<RwLock<HashMap<String, String>>>, // connection_id -> endpoint
}

impl SimpleBridgeAdapter {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl CrossNetworkBridgeTrait for SimpleBridgeAdapter {
    async fn establish_connection(&self, endpoint: &str, _peer_did: &str) -> Result<String> {
        let connection_id = Uuid::new_v4().to_string();
        let mut connections = self.connections.write().await;
        connections.insert(connection_id.clone(), endpoint.to_string());
        info!("Simple bridge: Connection established to {}", endpoint);
        Ok(connection_id)
    }

    async fn send_message(&self, connection_id: &str, message: &[u8]) -> Result<()> {
        let connections = self.connections.read().await;
        let endpoint = connections
            .get(connection_id)
            .ok_or_else(|| anyhow::anyhow!("Connection not found: {}", connection_id))?;

        // TODO: Actually send message via HTTP/gRPC to endpoint
        // For now, just log
        debug!(
            "Simple bridge: Would send {} bytes to {}",
            message.len(),
            endpoint
        );
        Ok(())
    }

    async fn get_connection_status(&self, connection_id: &str) -> Result<ConnectionStatus> {
        let connections = self.connections.read().await;
        if connections.contains_key(connection_id) {
            Ok(ConnectionStatus::Connected)
        } else {
            Ok(ConnectionStatus::Disconnected)
        }
    }

    async fn close_connection(&self, connection_id: &str) -> Result<()> {
        let mut connections = self.connections.write().await;
        connections.remove(connection_id);
        Ok(())
    }
}
