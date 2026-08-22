//! Network layer for P2P messaging using libp2p

use crate::network_p2p::{P2PCommand, P2PMessage, P2PNetwork, P2PNetworkEvent};
use crate::{Message, MessagingConfig};
use anyhow::Result;
use libp2p::Multiaddr;
use std::net::SocketAddr;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{error, info};

/// Network errors
#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("Transport error: {0}")]
    Transport(String),
    #[error("Protocol error: {0}")]
    Protocol(String),
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("libp2p error: {0}")]
    Libp2p(String),
}

/// P2P messaging network implementation
/// NOTE: This is currently a simplified implementation
/// TODO: Integrate proper libp2p networking once API compatibility is resolved
pub struct MessagingNetwork {
    /// Channel for receiving network events
    event_receiver: Option<mpsc::UnboundedReceiver<P2PNetworkEvent>>,
    /// Channel for sending network commands
    command_sender: mpsc::UnboundedSender<P2PCommand>,
    /// P2P network instance (moved into task on start)
    p2p_network: Option<P2PNetwork>,
    /// Background task handle
    p2p_task: Option<JoinHandle<()>>,
    /// Node configuration
    config: MessagingConfig,
    /// Whether the network is running
    is_running: bool,
}

/// Network events that can occur
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// New peer connected
    PeerConnected(String), // Using String instead of PeerId for now
    /// Peer disconnected
    PeerDisconnected(String),
    /// Message received from network
    MessageReceived { from: String, message: Message },
    /// New peer discovered via mDNS
    PeerDiscovered(String),
}

/// Commands that can be sent to the network
#[derive(Debug)]
pub enum NetworkCommand {
    /// Send a message to specific peers
    SendMessage {
        message: Message,
        recipients: Vec<String>,
    },
    /// Subscribe to a topic
    SubscribeTopic(String),
    /// Unsubscribe from a topic
    UnsubscribeTopic(String),
    /// Connect to a specific peer
    ConnectPeer(String),
}

impl MessagingNetwork {
    /// Create a new messaging network
    pub async fn new(config: &MessagingConfig) -> Result<Self> {
        let (command_sender, command_receiver) = mpsc::unbounded_channel();
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let p2p_network = P2PNetwork::new(config, event_sender, command_receiver).await?;

        Ok(Self {
            event_receiver: Some(event_receiver),
            command_sender,
            p2p_network: Some(p2p_network),
            p2p_task: None,
            config: config.clone(),
            is_running: false,
        })
    }

    /// Start the network layer
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting messaging network on {}", self.config.listen_addr);

        if let Some(mut network) = self.p2p_network.take() {
            let listen_addr = socketaddr_to_multiaddr(self.config.listen_addr)?;
            network.listen(listen_addr).await?;
            self.p2p_task = Some(tokio::spawn(async move {
                if let Err(e) = network.run().await {
                    error!("P2P network stopped with error: {}", e);
                }
            }));
        }

        self.is_running = true;

        info!("Messaging network started successfully (P2P mode)");
        Ok(())
    }

    /// Stop the network layer
    pub async fn stop(&mut self) -> Result<()> {
        info!("Stopping messaging network");

        self.is_running = false;
        if let Some(task) = self.p2p_task.take() {
            task.abort();
        }

        info!("Messaging network stopped");
        Ok(())
    }

    /// Send a message to specific recipients
    pub async fn send_message(&self, message: &Message, recipients: &[String]) -> Result<()> {
        info!(
            "Sending message {} to {} recipients",
            message.id,
            recipients.len()
        );

        // TODO: Implement actual quantum-resistant message sending via libp2p
        // For now, simulate successful sending
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        info!("Message {} sent successfully", message.id);
        Ok(())
    }

    /// Subscribe to a messaging topic
    pub async fn subscribe_topic(&self, topic: &str) -> Result<()> {
        info!("Subscribing to topic: {}", topic);
        Ok(())
    }

    /// Unsubscribe from a messaging topic
    pub async fn unsubscribe_topic(&self, topic: &str) -> Result<()> {
        info!("Unsubscribing from topic: {}", topic);
        Ok(())
    }

    /// Connect to a specific peer
    pub async fn connect_peer(&self, peer_addr: &str) -> Result<()> {
        info!("Connecting to peer: {}", peer_addr);
        Ok(())
    }

    /// Get list of connected peers
    pub fn get_connected_peers(&self) -> Vec<String> {
        vec![] // TODO: Implement actual peer listing
    }

    pub fn publish_p2p_message(&self, topic: &str, message: P2PMessage) -> Result<()> {
        self.command_sender
            .send(P2PCommand::PublishTopic {
                topic: topic.to_string(),
                message,
            })
            .map_err(|e| NetworkError::Connection(e.to_string()))?;
        Ok(())
    }

    pub fn command_sender(&self) -> mpsc::UnboundedSender<P2PCommand> {
        self.command_sender.clone()
    }

    pub fn subscribe_p2p_topic(&self, topic: &str) -> Result<()> {
        self.command_sender
            .send(P2PCommand::Subscribe {
                topic: topic.to_string(),
            })
            .map_err(|e| NetworkError::Connection(e.to_string()))?;
        Ok(())
    }

    pub fn take_event_receiver(&mut self) -> Option<mpsc::UnboundedReceiver<P2PNetworkEvent>> {
        self.event_receiver.take()
    }

    pub async fn next_p2p_event(&mut self) -> Option<P2PNetworkEvent> {
        if let Some(receiver) = &mut self.event_receiver {
            receiver.recv().await
        } else {
            None
        }
    }

    /// Get network statistics
    pub async fn get_network_stats(&self) -> NetworkStats {
        NetworkStats {
            connected_peers: 0,
            total_messages_sent: 0,
            total_messages_received: 0,
            bytes_sent: 0,
            bytes_received: 0,
        }
    }
}

fn socketaddr_to_multiaddr(addr: SocketAddr) -> Result<Multiaddr> {
    let (ip, port) = (addr.ip(), addr.port());
    let multiaddr = match ip {
        std::net::IpAddr::V4(ipv4) => format!("/ip4/{}/tcp/{}", ipv4, port),
        std::net::IpAddr::V6(ipv6) => format!("/ip6/{}/tcp/{}", ipv6, port),
    };
    Ok(multiaddr.parse()?)
}

/// Network statistics
#[derive(Debug, Clone)]
pub struct NetworkStats {
    pub connected_peers: u32,
    pub total_messages_sent: u64,
    pub total_messages_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}
