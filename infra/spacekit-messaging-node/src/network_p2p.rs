//! P2P Networking implementation using libp2p
//!
//! This module provides the actual peer-to-peer networking layer for SWTCHX Messenger
//! using libp2p for transport, discovery, and message routing.

use anyhow::Result;
use libp2p::multiaddr::Protocol;
use libp2p::{
    futures::StreamExt,
    gossipsub, identity, kad, mdns, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, SwarmBuilder,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::{DirectoryEntry, MessagingConfig};

/// P2P Network events that can be emitted
#[derive(Debug, Clone)]
pub enum P2PNetworkEvent {
    /// New peer discovered and connected
    PeerConnected {
        peer_id: String,
        addresses: Vec<String>,
    },
    /// Peer disconnected
    PeerDisconnected { peer_id: String },
    /// Message received from the network
    MessageReceived { from: String, message: P2PMessage },
    /// Peer discovered via mDNS
    PeerDiscovered {
        peer_id: String,
        addresses: Vec<String>,
    },
}

/// Messages that can be sent over the P2P network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum P2PMessage {
    /// Browser/API envelope propagated over signed gossipsub transport.
    GatewayEnvelope {
        message_id: String,
        sender_did: String,
        recipient_dids: Vec<String>,
        payload: serde_json::Value,
    },
    /// Direct encrypted message
    DirectMessage {
        message_id: String,
        sender_did: String,
        recipient_did: String,
        encrypted_payload: Vec<u8>,
        kem_ciphertext: Vec<u8>,
        nonce: Vec<u8>,
        timestamp: u64,
    },
    /// Group message
    GroupMessage {
        message_id: String,
        group_id: String,
        sender_did: String,
        encrypted_payloads: HashMap<String, Vec<u8>>, // recipient_did -> encrypted_content
        timestamp: u64,
    },
    /// Presence announcement
    Presence {
        did: String,
        username: String,
        status: String,
    },
    /// Scoped directory lookup request
    DirectoryLookupRequest {
        request_id: String,
        requester_did: String,
        prefix: Option<String>,
        limit: usize,
    },
    /// Scoped directory lookup response
    DirectoryLookupResponse {
        request_id: String,
        responder_did: String,
        entries: Vec<DirectoryEntry>,
    },
    /// Message delivery acknowledgment
    MessageAck {
        message_id: String,
        recipient_did: String,
    },
    /// Opt-in directory sync payload
    DirectorySync {
        source_did: String,
        entries: Vec<DirectoryEntry>,
    },
}

/// Custom network behavior combining multiple libp2p protocols
#[derive(NetworkBehaviour)]
pub struct MessagingBehaviour {
    /// Gossipsub for pub/sub messaging
    gossipsub: gossipsub::Behaviour,
    /// mDNS for local network peer discovery
    mdns: mdns::tokio::Behaviour,
    /// Kademlia DHT for peer routing and discovery
    kad: kad::Behaviour<kad::store::MemoryStore>,
    /// Identify protocol for peer information
    identify: libp2p::identify::Behaviour,
    /// Ping for connection keepalive
    ping: libp2p::ping::Behaviour,
}

/// P2P Network implementation
pub struct P2PNetwork {
    /// libp2p swarm
    swarm: libp2p::Swarm<MessagingBehaviour>,
    /// Local peer ID
    local_peer_id: PeerId,
    /// Event sender
    event_tx: mpsc::UnboundedSender<P2PNetworkEvent>,
    /// Command receiver
    command_rx: mpsc::UnboundedReceiver<P2PCommand>,
    /// Connected peers
    peers: HashMap<PeerId, PeerInfo>,
    /// Subscribed gossipsub topics
    subscribed_topics: Vec<gossipsub::IdentTopic>,
}

/// Commands that can be sent to the P2P network
#[derive(Debug)]
pub enum P2PCommand {
    /// Send a message to specific peer
    SendDirect {
        peer_id: PeerId,
        message: P2PMessage,
    },
    /// Publish message to gossipsub topic
    PublishTopic { topic: String, message: P2PMessage },
    /// Subscribe to a gossipsub topic
    Subscribe { topic: String },
    /// Unsubscribe from a topic
    Unsubscribe { topic: String },
    /// Connect to a peer at specific address
    Dial { address: Multiaddr },
    /// Add peer to DHT
    AddPeer {
        peer_id: PeerId,
        addresses: Vec<Multiaddr>,
    },
}

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub addresses: Vec<Multiaddr>,
    pub protocols: Vec<String>,
    pub agent_version: Option<String>,
    pub last_seen: std::time::Instant,
}

impl P2PNetwork {
    /// Create a new P2P network instance
    pub async fn new(
        config: &MessagingConfig,
        event_tx: mpsc::UnboundedSender<P2PNetworkEvent>,
        command_rx: mpsc::UnboundedReceiver<P2PCommand>,
    ) -> Result<Self> {
        // Generate or load identity
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());

        info!("Local peer ID: {}", local_peer_id);

        // Configure Gossipsub
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .message_id_fn(|msg| {
                // Custom message ID to prevent duplicates
                let mut hasher = DefaultHasher::new();
                msg.data.hash(&mut hasher);
                gossipsub::MessageId::from(hasher.finish().to_string())
            })
            .build()
            .expect("Valid gossipsub config");

        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )
        .unwrap();

        // Configure mDNS for local discovery
        let mdns = mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)?;

        // Configure Kademlia DHT
        let kad_store = kad::store::MemoryStore::new(local_peer_id);
        let mut kad = kad::Behaviour::new(local_peer_id, kad_store);

        // Add bootstrap peers to DHT
        for peer_addr in &config.bootstrap_peers {
            if let Ok(addr) = peer_addr.parse::<Multiaddr>() {
                let mut peer_id_opt = None;
                for protocol in addr.iter() {
                    if let Protocol::P2p(peer_id) = protocol {
                        peer_id_opt = Some(peer_id);
                        break;
                    }
                }

                if let Some(peer_id) = peer_id_opt {
                    kad.add_address(&peer_id, addr.clone());
                    debug!("Adding bootstrap peer with peer ID: {}", peer_id);
                } else {
                    debug!("Adding bootstrap peer without peer ID: {}", addr);
                }
            }
        }

        // Configure Identify protocol
        let identify = libp2p::identify::Behaviour::new(
            libp2p::identify::Config::new(
                "/swtchx/messenger/1.0.0".to_string(),
                local_key.public(),
            )
            .with_agent_version(format!("swtchx-messenger/{}", env!("CARGO_PKG_VERSION"))),
        );

        // Configure Ping
        let ping = libp2p::ping::Behaviour::new(
            libp2p::ping::Config::new().with_interval(Duration::from_secs(30)),
        );

        // Create the behavior
        let behaviour = MessagingBehaviour {
            gossipsub,
            mdns,
            kad,
            identify,
            ping,
        };

        // Build the swarm with our existing identity
        let swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|_key| behaviour)?
            .with_swarm_config(|cfg| {
                cfg.with_idle_connection_timeout(Duration::from_secs(120)) // 2 min keepalive
            })
            .build();

        let mut network = Self {
            swarm,
            local_peer_id,
            event_tx,
            command_rx,
            peers: HashMap::new(),
            subscribed_topics: Vec::new(),
        };

        // Dial bootstrap peers
        for peer_addr in &config.bootstrap_peers {
            if let Ok(addr) = peer_addr.parse::<Multiaddr>() {
                if let Err(e) = network.swarm.dial(addr.clone()) {
                    warn!("Failed to dial bootstrap peer {}: {}", addr, e);
                } else {
                    info!("Dialing bootstrap peer: {}", addr);
                }
            }
        }

        Ok(network)
    }

    /// Start listening on configured address
    pub async fn listen(&mut self, addr: Multiaddr) -> Result<()> {
        self.swarm.listen_on(addr)?;
        info!("Listening for connections");
        Ok(())
    }

    /// Main event loop
    pub async fn run(mut self) -> Result<()> {
        info!("Starting P2P network event loop");

        loop {
            tokio::select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => {
                    self.handle_swarm_event(event).await;
                }

                // Handle commands
                Some(command) = self.command_rx.recv() => {
                    self.handle_command(command).await;
                }
            }
        }
    }

    /// Handle swarm events
    async fn handle_swarm_event(&mut self, event: SwarmEvent<MessagingBehaviourEvent>) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
            }

            SwarmEvent::Behaviour(MessagingBehaviourEvent::Mdns(mdns_event)) => {
                self.handle_mdns_event(mdns_event).await;
            }

            SwarmEvent::Behaviour(MessagingBehaviourEvent::Gossipsub(gossip_event)) => {
                self.handle_gossipsub_event(gossip_event).await;
            }

            SwarmEvent::Behaviour(MessagingBehaviourEvent::Identify(identify_event)) => {
                self.handle_identify_event(identify_event).await;
            }

            SwarmEvent::Behaviour(MessagingBehaviourEvent::Kad(kad_event)) => {
                self.handle_kad_event(kad_event).await;
            }

            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                info!("Connection established with {}", peer_id);

                // Add peer to gossipsub mesh explicitly
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .add_explicit_peer(&peer_id);
                info!("Added peer {} to gossipsub mesh", peer_id);

                let _ = self.event_tx.send(P2PNetworkEvent::PeerConnected {
                    peer_id: peer_id.to_string(),
                    addresses: vec![endpoint.get_remote_address().to_string()],
                });
            }

            SwarmEvent::ConnectionClosed { peer_id, .. } => {
                info!("Connection closed with {}", peer_id);

                // Remove from gossipsub mesh
                self.swarm
                    .behaviour_mut()
                    .gossipsub
                    .remove_explicit_peer(&peer_id);

                self.peers.remove(&peer_id);
                let _ = self.event_tx.send(P2PNetworkEvent::PeerDisconnected {
                    peer_id: peer_id.to_string(),
                });
            }

            _ => {}
        }
    }

    /// Handle mDNS events (local network discovery)
    async fn handle_mdns_event(&mut self, event: mdns::Event) {
        match event {
            mdns::Event::Discovered(list) => {
                for (peer_id, multiaddr) in list {
                    // Skip if it's ourselves
                    if peer_id == self.local_peer_id {
                        continue;
                    }

                    info!("Discovered peer via mDNS: {} at {}", peer_id, multiaddr);

                    // Add to DHT first
                    self.swarm
                        .behaviour_mut()
                        .kad
                        .add_address(&peer_id, multiaddr.clone());

                    // Dial the discovered peer only if not already connected
                    if !self.peers.contains_key(&peer_id) {
                        if let Err(e) = self.swarm.dial(multiaddr.clone()) {
                            warn!("Failed to dial discovered peer: {}", e);
                        } else {
                            info!("Dialing discovered peer: {}", peer_id);
                        }
                    }

                    let _ = self.event_tx.send(P2PNetworkEvent::PeerDiscovered {
                        peer_id: peer_id.to_string(),
                        addresses: vec![multiaddr.to_string()],
                    });
                }
            }
            mdns::Event::Expired(list) => {
                for (peer_id, _) in list {
                    debug!("mDNS peer expired: {}", peer_id);
                }
            }
        }
    }

    /// Handle gossipsub events (pub/sub messaging)
    async fn handle_gossipsub_event(&mut self, event: gossipsub::Event) {
        match event {
            gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            } => {
                debug!(
                    "Received gossipsub message from {}: {:?}",
                    propagation_source, message_id
                );

                // Deserialize the message
                if let Ok(p2p_message) = serde_json::from_slice::<P2PMessage>(&message.data) {
                    let _ = self.event_tx.send(P2PNetworkEvent::MessageReceived {
                        from: propagation_source.to_string(),
                        message: p2p_message,
                    });
                }
            }
            gossipsub::Event::Subscribed { peer_id, topic } => {
                debug!("Peer {} subscribed to topic: {}", peer_id, topic);
            }
            gossipsub::Event::Unsubscribed { peer_id, topic } => {
                debug!("Peer {} unsubscribed from topic: {}", peer_id, topic);
            }
            _ => {}
        }
    }

    /// Handle identify events (peer information exchange)
    async fn handle_identify_event(&mut self, event: libp2p::identify::Event) {
        match event {
            libp2p::identify::Event::Received { peer_id, info } => {
                debug!("Received identify from {}: {:?}", peer_id, info);

                // Store peer information
                self.peers.insert(
                    peer_id,
                    PeerInfo {
                        peer_id,
                        addresses: info.listen_addrs.clone(),
                        protocols: info.protocols.iter().map(|p| p.to_string()).collect(),
                        agent_version: Some(info.agent_version),
                        last_seen: std::time::Instant::now(),
                    },
                );

                // Add peer to DHT
                for addr in info.listen_addrs {
                    self.swarm.behaviour_mut().kad.add_address(&peer_id, addr);
                }
            }
            _ => {}
        }
    }

    /// Handle Kademlia DHT events
    async fn handle_kad_event(&mut self, event: kad::Event) {
        match event {
            kad::Event::RoutingUpdated { peer, .. } => {
                debug!("DHT routing updated for peer: {}", peer);
            }
            _ => {}
        }
    }

    /// Handle commands sent to the network
    async fn handle_command(&mut self, command: P2PCommand) {
        match command {
            P2PCommand::PublishTopic { topic, message } => {
                let topic = gossipsub::IdentTopic::new(topic);

                if let Ok(data) = serde_json::to_vec(&message) {
                    if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, data) {
                        error!("Failed to publish to topic: {}", e);
                    }
                }
            }

            P2PCommand::Subscribe { topic } => {
                let topic = gossipsub::IdentTopic::new(topic.clone());

                if let Err(e) = self.swarm.behaviour_mut().gossipsub.subscribe(&topic) {
                    error!("Failed to subscribe to topic: {}", e);
                } else {
                    let cloned_topic = topic.clone();
                    self.subscribed_topics.push(cloned_topic.clone());
                    info!("Subscribed to topic: {}", cloned_topic.hash().as_str());
                }
            }

            P2PCommand::Unsubscribe { topic } => {
                let topic = gossipsub::IdentTopic::new(topic);

                if let Err(e) = self.swarm.behaviour_mut().gossipsub.unsubscribe(&topic) {
                    error!("Failed to unsubscribe from topic: {}", e);
                } else {
                    self.subscribed_topics
                        .retain(|t| t.hash().as_str() != topic.hash().as_str());
                    info!("Unsubscribed from topic: {}", topic);
                }
            }

            P2PCommand::Dial { address } => {
                if let Err(e) = self.swarm.dial(address.clone()) {
                    error!("Failed to dial {}: {}", address, e);
                } else {
                    info!("Dialing {}", address);
                }
            }

            P2PCommand::AddPeer { peer_id, addresses } => {
                for addr in addresses {
                    self.swarm.behaviour_mut().kad.add_address(&peer_id, addr);
                }
            }

            P2PCommand::SendDirect { .. } => {
                // TODO: Implement direct messaging via request-response protocol
                warn!("Direct messaging not yet implemented");
            }
        }
    }

    /// Get local peer ID
    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }

    /// Get connected peers
    pub fn connected_peers(&self) -> Vec<PeerInfo> {
        self.peers.values().cloned().collect()
    }
}
