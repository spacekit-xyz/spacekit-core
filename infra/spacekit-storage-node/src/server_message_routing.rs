//! Server Message Routing
//!
//! Routes messages between servers using cross-network bridges and Gossipsub topics.
//!
//! Features:
//! - Route messages to remote servers via bridge
//! - Publish messages to server Gossipsub topics
//! - Forward messages received from server topics
//! - Handle message delivery acknowledgments

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::database::Server;

/// Server message router for cross-server message delivery
pub struct ServerMessageRouter {
    /// Active server connections (server_id -> connection info)
    server_connections: Arc<RwLock<HashMap<String, ServerConnection>>>,
    /// Message routing table (server_id -> routing info)
    routing_table: Arc<RwLock<HashMap<String, ServerRoutingInfo>>>,
}

/// Connection information for a server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConnection {
    pub server_id: String,
    pub server: Server,
    pub bridge_connection_id: Option<String>,
    pub subscribed_topics: Vec<String>,
    pub connected_at: DateTime<Utc>,
}

/// Routing information for a server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerRoutingInfo {
    pub server_id: String,
    pub messages_topic: String,
    pub presence_topic: String,
    pub message_count: u64,
    pub last_message_at: Option<DateTime<Utc>>,
}

/// Message to be routed to a server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutedMessage {
    pub message_id: String,
    pub sender_did: String,
    pub recipient_did: Option<String>, // None for broadcast
    pub server_id: String,
    pub content: Vec<u8>, // Encrypted message content
    pub message_type: MessageType,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    DirectMessage,
    GroupMessage,
    PresenceUpdate,
    ServerAnnouncement,
}

impl ServerMessageRouter {
    /// Create new server message router
    pub fn new() -> Self {
        Self {
            server_connections: Arc::new(RwLock::new(HashMap::new())),
            routing_table: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a server connection for message routing
    pub async fn register_server_connection(
        &self,
        server: Server,
        bridge_connection_id: Option<String>,
        subscribed_topics: Vec<String>,
    ) -> Result<()> {
        info!(
            "📡 Registering server connection for routing: {}",
            server.id
        );

        let connection = ServerConnection {
            server_id: server.id.clone(),
            server: server.clone(),
            bridge_connection_id,
            subscribed_topics: subscribed_topics.clone(),
            connected_at: Utc::now(),
        };

        {
            let mut connections = self.server_connections.write().await;
            connections.insert(server.id.clone(), connection);
        }

        // Create routing info
        let routing_info = ServerRoutingInfo {
            server_id: server.id.clone(),
            messages_topic: format!("server:{}:messages", server.id),
            presence_topic: format!("server:{}:presence", server.id),
            message_count: 0,
            last_message_at: None,
        };

        {
            let mut routing = self.routing_table.write().await;
            routing.insert(server.id.clone(), routing_info);
        }

        info!("✅ Server connection registered for routing: {}", server.id);
        Ok(())
    }

    /// Route a message to a server
    pub async fn route_message(&self, message: RoutedMessage) -> Result<()> {
        info!(
            "📨 Routing message {} to server: {}",
            message.message_id, message.server_id
        );

        // Get routing info
        let routing_info = {
            let routing = self.routing_table.read().await;
            routing
                .get(&message.server_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("Server not in routing table: {}", message.server_id)
                })?
                .clone()
        };

        // Get connection info
        let connection = {
            let connections = self.server_connections.read().await;
            connections
                .get(&message.server_id)
                .ok_or_else(|| anyhow::anyhow!("Server not connected: {}", message.server_id))?
                .clone()
        };

        // Route message based on type
        match message.message_type {
            MessageType::DirectMessage | MessageType::GroupMessage => {
                // Publish to server's messages topic via Gossipsub
                // This will be handled by the messaging node's P2P network
                info!(
                    "📤 Publishing message to topic: {}",
                    routing_info.messages_topic
                );

                // TODO: Use messaging node to publish to Gossipsub topic
                // The message will be automatically forwarded to all subscribers
            }
            MessageType::PresenceUpdate => {
                // Publish to server's presence topic
                info!(
                    "📤 Publishing presence update to topic: {}",
                    routing_info.presence_topic
                );

                // TODO: Use messaging node to publish to Gossipsub topic
            }
            MessageType::ServerAnnouncement => {
                // Broadcast to all connected servers
                info!("📢 Broadcasting server announcement");

                // TODO: Broadcast to all server topics
            }
        }

        // Update routing stats
        {
            let mut routing = self.routing_table.write().await;
            if let Some(info) = routing.get_mut(&message.server_id) {
                info.message_count += 1;
                info.last_message_at = Some(Utc::now());
            }
        }

        Ok(())
    }

    /// Handle message received from a server topic
    pub async fn handle_server_message(&self, server_id: String, message: Vec<u8>) -> Result<()> {
        debug!("📥 Received message from server: {}", server_id);

        // Deserialize message
        let routed_message: RoutedMessage = serde_json::from_slice(&message)?;

        // Update routing stats
        {
            let mut routing = self.routing_table.write().await;
            if let Some(info) = routing.get_mut(&server_id) {
                info.message_count += 1;
                info.last_message_at = Some(Utc::now());
            }
        }

        // TODO: Forward message to local messaging handler
        // This would integrate with the messaging node to deliver the message locally

        Ok(())
    }

    /// Get routing statistics for a server
    pub async fn get_routing_stats(&self, server_id: &str) -> Option<ServerRoutingInfo> {
        let routing = self.routing_table.read().await;
        routing.get(server_id).cloned()
    }

    /// Get all connected servers for routing
    pub async fn get_routable_servers(&self) -> Vec<String> {
        let connections = self.server_connections.read().await;
        connections.keys().cloned().collect()
    }
}

impl Default for ServerMessageRouter {
    fn default() -> Self {
        Self::new()
    }
}
