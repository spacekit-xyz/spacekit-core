//! P2P Networking layer for distributed storage
//!
//! Implements peer-to-peer file sharing with libp2p
//! Enhanced with messaging integration and cross-service DID resolution

use crate::StorageNodeConfig;
use anyhow::Result;
use futures::stream::StreamExt;
use libp2p::{
    gossipsub, identify,
    kad::{store::MemoryStore, Event as KademliaEvent, Mode},
    mdns, noise, ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, info, warn};

/// Discovery mode for P2P network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryMode {
    /// Pure P2P discovery (current implementation)
    Direct,
    /// P2P + messaging node hints
    Hybrid,
    /// Fallback to messaging-only discovery
    MessagingOnly,
}

/// Gossipsub topic for DID document announcements
pub const DID_TOPIC: &str = "spacekit/did/v1";

/// P2P Network events
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    PeerConnected(PeerId),
    PeerDisconnected(PeerId),
    FileChunkRequest {
        chunk_id: String,
        from_peer: PeerId,
    },
    FileChunkResponse {
        chunk_id: String,
        data: Vec<u8>,
        from_peer: PeerId,
    },
    FileAnnouncement {
        file_id: String,
        chunks: Vec<String>,
        from_peer: PeerId,
    },
    NodeDiscovered(PeerId, Multiaddr),
    MessagingNodeDiscovered(PeerId, Multiaddr),
    UserDIDResolved {
        did: String,
        peer_id: PeerId,
        multiaddr: Multiaddr,
    },
    CrossServiceHealthCheck {
        service_type: String,
        peer_id: PeerId,
        healthy: bool,
    },
    DidDocumentReceived {
        did: String,
        document: Vec<u8>,
        from_peer: PeerId,
    },
}

/// DID to peer mapping for cross-service resolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DIDPeerMapping {
    pub did: String,
    #[serde(with = "peer_id_serde")]
    pub peer_id: PeerId,
    #[serde(with = "multiaddr_serde")]
    pub multiaddr: Multiaddr,
    pub service_types: Vec<String>, // ["storage", "messaging", "compute"]
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub reputation_score: f64,
}

// Custom serialization for PeerId
mod peer_id_serde {
    use libp2p::PeerId;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(peer_id: &PeerId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        peer_id.to_string().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<PeerId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

// Custom serialization for Multiaddr
mod multiaddr_serde {
    use libp2p::Multiaddr;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(multiaddr: &Multiaddr, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        multiaddr.to_string().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Multiaddr, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Messaging node client for hybrid discovery
#[derive(Debug, Clone)]
pub struct MessagingNodeClient {
    pub peer_id: PeerId,
    pub multiaddr: Multiaddr,
    pub last_contact: chrono::DateTime<chrono::Utc>,
    pub connection_healthy: bool,
}

/// File chunk information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    pub chunk_id: String,
    pub file_id: String,
    pub chunk_index: usize,
    pub data: Vec<u8>,
    pub hash: String,
    pub encrypted: bool,
}

/// Network behaviour for storage operations
#[derive(NetworkBehaviour)]
pub struct StorageBehaviour {
    kademlia: libp2p::kad::Behaviour<MemoryStore>,
    mdns: mdns::tokio::Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
    gossipsub: gossipsub::Behaviour,
}

/// P2P Network manager with messaging integration
pub struct P2PNetwork {
    swarm: Mutex<Swarm<StorageBehaviour>>,
    event_sender: mpsc::UnboundedSender<NetworkEvent>,
    stored_chunks: RwLock<HashMap<String, FileChunk>>,
    peer_addresses: RwLock<HashMap<PeerId, Vec<Multiaddr>>>,

    // Phase 2: Messaging integration
    messaging_clients: RwLock<HashMap<PeerId, MessagingNodeClient>>,
    discovery_mode: DiscoveryMode,

    // Phase 3: Cross-service DID resolution
    did_peer_mappings: RwLock<HashMap<String, DIDPeerMapping>>,
    service_registry: RwLock<HashMap<PeerId, Vec<String>>>, // peer_id -> [service_types]

    // Bootstrap peers from config
    bootstrap_peers: Vec<String>,
    /// When false, P2P announces availability without retaining chunk bytes in RAM.
    cache_chunks_in_memory: bool,
}

// P2PNetwork is Send + Sync by default since all its fields are Send + Sync

impl P2PNetwork {
    /// Create a new P2P network
    pub async fn new(config: &StorageNodeConfig) -> Result<Arc<Self>> {
        info!("Creating P2P network...");

        let mut swarm = libp2p::SwarmBuilder::with_new_identity()
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|key| {
                let peer_id = key.public().to_peer_id();

                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(5))
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .build()
                    .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

                let gossipsub_behaviour = gossipsub::Behaviour::new(
                    gossipsub::MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                )
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e)))?;

                Ok(StorageBehaviour {
                    kademlia: libp2p::kad::Behaviour::new(peer_id, MemoryStore::new(peer_id)),
                    mdns: mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)?,
                    identify: identify::Behaviour::new(identify::Config::new(
                        "/spacekit-storage/1.0.0".to_string(),
                        key.public(),
                    )),
                    ping: ping::Behaviour::new(
                        ping::Config::new().with_interval(Duration::from_secs(10)),
                    ),
                    gossipsub: gossipsub_behaviour,
                })
            })?
            .with_swarm_config(|cfg| {
                cfg.with_idle_connection_timeout(Duration::from_secs(120)) // 2 min keepalive
            })
            .build();

        // Set Kademlia to server mode
        swarm.behaviour_mut().kademlia.set_mode(Some(Mode::Server));

        // Subscribe to the DID document topic
        let did_topic = gossipsub::IdentTopic::new(DID_TOPIC);
        swarm
            .behaviour_mut()
            .gossipsub
            .subscribe(&did_topic)
            .map_err(|e| anyhow::anyhow!("Failed to subscribe to DID topic: {}", e))?;
        info!("Subscribed to Gossipsub topic: {}", DID_TOPIC);

        // Listen on the configured port
        let listen_addr = format!("/ip4/0.0.0.0/tcp/{}", config.network_config.listen_port);
        swarm.listen_on(listen_addr.parse()?)?;

        let (event_sender, mut event_receiver) = mpsc::unbounded_channel();
        // Drain events — nothing subscribes today; an unbounded channel with no receiver leaks RAM.
        tokio::spawn(async move { while event_receiver.recv().await.is_some() {} });

        info!("P2P network created successfully");
        Ok(Arc::new(Self {
            swarm: Mutex::new(swarm),
            event_sender,
            stored_chunks: RwLock::new(HashMap::new()),
            peer_addresses: RwLock::new(HashMap::new()),
            messaging_clients: RwLock::new(HashMap::new()),
            discovery_mode: DiscoveryMode::Direct,
            did_peer_mappings: RwLock::new(HashMap::new()),
            service_registry: RwLock::new(HashMap::new()),
            bootstrap_peers: config.network_config.bootstrap_peers.clone(),
            cache_chunks_in_memory: config.network_config.cache_p2p_chunks_in_memory,
        }))
    }

    /// Whether full chunk payloads are cached in RAM (default: announce-only).
    pub fn cache_chunks_in_memory(&self) -> bool {
        self.cache_chunks_in_memory
    }

    /// Announce a chunk provider record without retaining bytes in memory.
    pub async fn announce_chunk(&self, chunk_id: &str) -> Result<()> {
        let chunk_key = libp2p::kad::RecordKey::new(&chunk_id);
        let mut swarm = self.swarm.lock().await;
        swarm.behaviour_mut().kademlia.start_providing(chunk_key)?;
        Ok(())
    }

    /// Start the P2P network with enhanced discovery
    pub async fn start(self: Arc<Self>) -> Result<()> {
        info!(
            "Starting P2P network with discovery mode: {:?}",
            self.discovery_mode
        );

        // Bootstrap from messaging nodes if in hybrid/messaging mode
        if matches!(
            self.discovery_mode,
            DiscoveryMode::Hybrid | DiscoveryMode::MessagingOnly
        ) {
            if let Ok(bootstrap_peers) = self.bootstrap_from_messaging_nodes().await {
                info!(
                    "Bootstrapped {} peers from messaging nodes",
                    bootstrap_peers.len()
                );

                // Add bootstrap peers to Kademlia and dial them
                let mut swarm = self.swarm.lock().await;
                for (peer_id, multiaddr) in bootstrap_peers {
                    swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, multiaddr.clone());
                    // Dial the peer to establish connection
                    if !swarm.is_connected(&peer_id) {
                        if let Err(e) = swarm.dial(multiaddr) {
                            debug!("Failed to dial bootstrap peer {}: {}", peer_id, e);
                        }
                    }
                }
                drop(swarm);
            }
        }

        // Handle bootstrap peers from config
        if !self.bootstrap_peers.is_empty() {
            let mut swarm = self.swarm.lock().await;
            for bootstrap_addr_str in &self.bootstrap_peers {
                if let Ok(multiaddr) = bootstrap_addr_str.parse::<Multiaddr>() {
                    // Extract peer ID from multiaddr if present
                    if let Some(peer_id) = multiaddr.iter().last().and_then(|proto| {
                        if let libp2p::multiaddr::Protocol::P2p(hash) = proto {
                            PeerId::from_multihash(hash.into()).ok()
                        } else {
                            None
                        }
                    }) {
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, multiaddr.clone());
                        if !swarm.is_connected(&peer_id) {
                            if let Err(e) = swarm.dial(multiaddr) {
                                debug!("Failed to dial bootstrap peer {}: {}", peer_id, e);
                            } else {
                                info!("Dialing bootstrap peer: {}", peer_id);
                            }
                        }
                    } else {
                        // Try to dial without peer ID (will be resolved during connection)
                        if let Err(e) = swarm.dial(multiaddr.clone()) {
                            debug!(
                                "Failed to dial bootstrap address {}: {}",
                                bootstrap_addr_str, e
                            );
                        }
                    }
                }
            }
            drop(swarm);
        }

        // Start periodic maintenance tasks
        let network_weak = Arc::downgrade(&self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes

            loop {
                interval.tick().await;

                if let Some(network) = network_weak.upgrade() {
                    // Health check messaging nodes
                    network.health_check_messaging_nodes().await;

                    // Cleanup stale DID mappings
                    network.cleanup_stale_mappings().await;

                    debug!("Completed periodic network maintenance");
                } else {
                    // Network has been dropped, exit the task
                    break;
                }
            }
        });

        // Start keepalive task to maintain connections
        let network_keepalive = Arc::downgrade(&self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60)); // Every minute

            loop {
                interval.tick().await;

                if let Some(network) = network_keepalive.upgrade() {
                    // Query Kademlia to keep connections active
                    let swarm = network.swarm.lock().await;
                    let connected_peers: Vec<PeerId> = swarm.connected_peers().copied().collect();
                    drop(swarm);

                    // Perform a random Kademlia query to keep connections alive
                    if !connected_peers.is_empty() {
                        // Use a random peer's ID as a query key to keep the connection active
                        if let Some(peer_id) = connected_peers.first() {
                            let query_key = libp2p::kad::RecordKey::new(&peer_id.to_string());
                            let mut swarm = network.swarm.lock().await;
                            swarm.behaviour_mut().kademlia.get_providers(query_key);
                            drop(swarm);
                            debug!("Keepalive: Querying Kademlia for peer {}", peer_id);
                        }
                    }
                } else {
                    break;
                }
            }
        });

        loop {
            let mut swarm = self.swarm.lock().await;
            tokio::select! {
                event = swarm.select_next_some() => {
                    drop(swarm); // Release the lock before handling the event
                    self.handle_swarm_event(event).await;
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    drop(swarm); // Release the lock
                    // Periodic maintenance
                    debug!("P2P network maintenance tick");
                }
            }
        }
    }

    /// Handle swarm events
    async fn handle_swarm_event(&self, event: SwarmEvent<StorageBehaviourEvent>) {
        match event {
            SwarmEvent::NewListenAddr { address, .. } => {
                info!("Listening on {}", address);
            }
            SwarmEvent::Behaviour(StorageBehaviourEvent::Kademlia(event)) => {
                self.handle_kademlia_event(event).await;
            }
            SwarmEvent::Behaviour(StorageBehaviourEvent::Mdns(event)) => {
                self.handle_mdns_event(event).await;
            }
            SwarmEvent::Behaviour(StorageBehaviourEvent::Identify(event)) => {
                self.handle_identify_event(event).await;
            }
            SwarmEvent::Behaviour(StorageBehaviourEvent::Ping(event)) => {
                self.handle_ping_event(event).await;
            }
            SwarmEvent::Behaviour(StorageBehaviourEvent::Gossipsub(event)) => {
                self.handle_gossipsub_event(event).await;
            }
            SwarmEvent::ConnectionEstablished {
                peer_id, endpoint, ..
            } => {
                info!(
                    "✅ Connection established with peer: {} via {:?}",
                    peer_id, endpoint
                );
                let _ = self.event_sender.send(NetworkEvent::PeerConnected(peer_id));
            }
            SwarmEvent::ConnectionClosed {
                peer_id,
                cause,
                endpoint,
                ..
            } => {
                // Log connection close with detailed cause information
                // This information will be available to the simulator via NetworkEvent
                let cause_str = format!("{:?}", cause);
                warn!(
                    "❌ Connection closed with peer: {} via {:?}\n   Cause: {}",
                    peer_id, endpoint, cause_str
                );

                // If it's a keepalive timeout, provide actionable guidance
                if cause_str.contains("KeepAliveTimeout") || cause_str.contains("keep_alive") {
                    warn!(
                        "⚠️  KeepAlive timeout detected for peer: {}. Consider:\n   - Reducing keepalive interval (currently 60s)\n   - Increasing connection activity\n   - Configuring TCP keepalive",
                        peer_id
                    );
                }

                let _ = self
                    .event_sender
                    .send(NetworkEvent::PeerDisconnected(peer_id));
            }
            SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                warn!(
                    "❌ Outgoing connection error to peer {:?}: {:?}",
                    peer_id, error
                );
            }
            SwarmEvent::IncomingConnectionError {
                local_addr,
                send_back_addr,
                error,
                ..
            } => {
                warn!(
                    "❌ Incoming connection error from {} to {}: {:?}",
                    send_back_addr, local_addr, error
                );
            }
            SwarmEvent::Dialing { peer_id, .. } => {
                debug!("🔄 Dialing peer: {:?}", peer_id);
            }
            SwarmEvent::NewExternalAddrOfPeer {
                peer_id, address, ..
            } => {
                debug!("📍 New external address for peer {}: {}", peer_id, address);
            }
            _ => {}
        }
    }

    /// Handle Kademlia DHT events
    async fn handle_kademlia_event(&self, event: KademliaEvent) {
        match event {
            KademliaEvent::OutboundQueryProgressed { result, .. } => {
                use libp2p::kad::QueryResult;
                match result {
                    QueryResult::GetRecord(Ok(ok)) => {
                        use libp2p::kad::GetRecordOk;
                        match ok {
                            GetRecordOk::FoundRecord(peer_record) => {
                                match serde_json::from_slice::<FileChunk>(&peer_record.record.value)
                                {
                                    Ok(chunk) => {
                                        info!("Received chunk {} from DHT", chunk.chunk_id);
                                        if self.cache_chunks_in_memory {
                                            let mut chunks = self.stored_chunks.write().await;
                                            chunks.insert(chunk.chunk_id.clone(), chunk);
                                        }
                                    }
                                    Err(e) => {
                                        debug!(
                                            "Failed to deserialize DHT record as FileChunk: {}",
                                            e
                                        );
                                    }
                                }
                            }
                            GetRecordOk::FinishedWithNoAdditionalRecord { .. } => {}
                        }
                    }
                    QueryResult::GetRecord(Err(e)) => {
                        debug!("Kademlia GetRecord error: {:?}", e);
                    }
                    QueryResult::PutRecord(Ok(_)) => {
                        debug!("Kademlia PutRecord succeeded");
                    }
                    QueryResult::PutRecord(Err(e)) => {
                        debug!("Kademlia PutRecord error: {:?}", e);
                    }
                    _ => {
                        debug!("Kademlia query progress: {:?}", result);
                    }
                }
            }
            KademliaEvent::RoutingUpdated { peer, .. } => {
                debug!("Routing table updated for peer: {}", peer);
            }
            _ => {}
        }
    }

    /// Handle ping events
    async fn handle_ping_event(&self, event: ping::Event) {
        let ping::Event {
            peer,
            result,
            connection: _,
        } = event;
        match result {
            Ok(_) => {
                debug!("Ping successful with peer: {}", peer);
            }
            Err(e) => {
                debug!("Ping error with peer {}: {:?}", peer, e);
            }
        }
    }

    /// Handle Gossipsub events (DID document broadcasts)
    async fn handle_gossipsub_event(&self, event: gossipsub::Event) {
        match event {
            gossipsub::Event::Message {
                propagation_source,
                message,
                ..
            } => {
                // Parse DID document message: [did_len:u16le][did_bytes][doc_bytes]
                let data = &message.data;
                if data.len() < 2 {
                    return;
                }
                let did_len = u16::from_le_bytes([data[0], data[1]]) as usize;
                if data.len() < 2 + did_len {
                    return;
                }
                let did = match std::str::from_utf8(&data[2..2 + did_len]) {
                    Ok(s) => s.to_string(),
                    Err(_) => return,
                };
                let doc_bytes = data[2 + did_len..].to_vec();

                info!(
                    "Received DID document via Gossipsub: {} ({} bytes) from {}",
                    did,
                    doc_bytes.len(),
                    propagation_source
                );

                // Store locally
                let chunk = FileChunk {
                    chunk_id: format!("did:document:{}", did),
                    file_id: did.clone(),
                    chunk_index: 0,
                    data: doc_bytes.clone(),
                    hash: String::new(),
                    encrypted: false,
                };
                if self.cache_chunks_in_memory {
                    let mut chunks = self.stored_chunks.write().await;
                    chunks.insert(chunk.chunk_id.clone(), chunk);
                }

                let _ = self.event_sender.send(NetworkEvent::DidDocumentReceived {
                    did,
                    document: doc_bytes,
                    from_peer: propagation_source,
                });
            }
            gossipsub::Event::Subscribed { peer_id, topic } => {
                debug!("Peer {} subscribed to topic {}", peer_id, topic);
            }
            gossipsub::Event::Unsubscribed { peer_id, topic } => {
                debug!("Peer {} unsubscribed from topic {}", peer_id, topic);
            }
            _ => {}
        }
    }

    /// Handle identify events with service detection
    async fn handle_identify_event(&self, event: identify::Event) {
        match event {
            identify::Event::Received { peer_id, info, .. } => {
                debug!("Received identify from {}: {:?}", peer_id, info);

                // Add addresses to Kademlia
                for addr in &info.listen_addrs {
                    self.swarm
                        .lock()
                        .await
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                }

                // Store peer addresses
                {
                    let mut peers = self.peer_addresses.write().await;
                    peers.insert(peer_id, info.listen_addrs.clone());
                }

                // Phase 2 & 3: Enhanced service detection
                self.detect_and_register_services(&peer_id, &info).await;
            }
            identify::Event::Sent { peer_id, .. } => {
                debug!("Sent identify to {}", peer_id);
            }
            identify::Event::Error { peer_id, error, .. } => {
                warn!("Identify error with {}: {:?}", peer_id, error);
            }
            _ => {}
        }
    }

    /// Detect and register services from identify info
    async fn detect_and_register_services(&self, peer_id: &PeerId, info: &identify::Info) {
        let mut detected_services = Vec::new();

        // Detect service types from protocol version and agent version
        let protocol = &info.protocol_version;
        let agent = &info.agent_version;

        // Service detection heuristics
        if protocol.contains("spacekit-messaging") || agent.contains("messaging") {
            detected_services.push("messaging".to_string());

            // Register as messaging node if in hybrid mode
            if matches!(self.discovery_mode, DiscoveryMode::Hybrid) {
                if let Some(addr) = info.listen_addrs.first() {
                    let _ = self.register_messaging_node(*peer_id, addr.clone()).await;
                }
            }
        }

        if protocol.contains("spacekit-storage") || agent.contains("storage") {
            detected_services.push("storage".to_string());
        }

        if protocol.contains("spacekit-compute") || agent.contains("compute") {
            detected_services.push("compute".to_string());
        }

        if protocol.contains("spacekit-cortex") || agent.contains("cortex") {
            detected_services.push("cortex".to_string());
        }

        // Register detected services
        for service in &detected_services {
            self.register_service_for_peer(*peer_id, service.clone())
                .await;
        }

        // Try to extract DID from agent version or protocol
        if let Some(did) = self.extract_did_from_info(info) {
            if let Some(addr) = info.listen_addrs.first() {
                let _ = self
                    .register_did_mapping(did, *peer_id, addr.clone(), detected_services.clone())
                    .await;
            }
        }

        if !detected_services.is_empty() {
            info!(
                "Detected services for peer {}: {:?}",
                peer_id, detected_services
            );
        }
    }

    /// Extract DID from identify info
    fn extract_did_from_info(&self, info: &identify::Info) -> Option<String> {
        // Look for DID in agent version string
        if let Some(did_start) = info.agent_version.find("did:spacekit:") {
            if let Some(did_end) = info.agent_version[did_start..].find(' ') {
                return Some(info.agent_version[did_start..did_start + did_end].to_string());
            } else {
                // DID might be at the end of the string
                return Some(info.agent_version[did_start..].to_string());
            }
        }

        // Look for DID in protocol version
        if let Some(did_start) = info.protocol_version.find("did:spacekit:") {
            if let Some(did_end) = info.protocol_version[did_start..].find(' ') {
                return Some(info.protocol_version[did_start..did_start + did_end].to_string());
            }
        }

        None
    }

    /// Handle mDNS events
    async fn handle_mdns_event(&self, event: mdns::Event) {
        match event {
            mdns::Event::Discovered(list) => {
                for (peer_id, multiaddr) in list {
                    info!("Discovered peer via mDNS: {} at {}", peer_id, multiaddr);

                    // Add to Kademlia routing table
                    {
                        let mut swarm = self.swarm.lock().await;
                        swarm
                            .behaviour_mut()
                            .kademlia
                            .add_address(&peer_id, multiaddr.clone());

                        // Actually dial the peer to establish a connection
                        // Only dial if not already connected
                        if !swarm.is_connected(&peer_id) {
                            if let Err(e) = swarm.dial(multiaddr.clone()) {
                                debug!("Failed to dial discovered peer {}: {}", peer_id, e);
                            } else {
                                debug!("Dialing discovered peer: {} at {}", peer_id, multiaddr);
                            }
                        }
                    }

                    let _ = self
                        .event_sender
                        .send(NetworkEvent::NodeDiscovered(peer_id, multiaddr));
                }
            }
            mdns::Event::Expired(list) => {
                for (peer_id, multiaddr) in list {
                    debug!("mDNS entry expired: {} at {}", peer_id, multiaddr);
                }
            }
        }
    }

    /// Store a file chunk in the distributed network.
    ///
    /// When [`Self::cache_chunks_in_memory`] is false (default), only announces
    /// provider records — chunk bytes must be read from local disk instead.
    pub async fn store_chunk(&self, chunk: FileChunk) -> Result<()> {
        info!(
            "Storing chunk: {} for file: {}",
            chunk.chunk_id, chunk.file_id
        );

        let chunk_id = chunk.chunk_id.clone();
        let chunk_key = libp2p::kad::RecordKey::new(&chunk_id);
        let mut swarm = self.swarm.lock().await;

        if self.cache_chunks_in_memory {
            {
                let mut chunks = self.stored_chunks.write().await;
                chunks.insert(chunk_id.clone(), chunk.clone());
            }
            let chunk_bytes = serde_json::to_vec(&chunk)?;
            let record = libp2p::kad::Record {
                key: chunk_key.clone(),
                value: chunk_bytes,
                publisher: None,
                expires: None,
            };
            swarm
                .behaviour_mut()
                .kademlia
                .put_record(record, libp2p::kad::Quorum::One)?;
        }

        swarm.behaviour_mut().kademlia.start_providing(chunk_key)?;

        Ok(())
    }

    /// Retrieve a file chunk from the distributed network
    pub async fn retrieve_chunk(&self, chunk_id: &str) -> Result<Option<FileChunk>> {
        // Check local storage first
        {
            let chunks = self.stored_chunks.read().await;
            if let Some(chunk) = chunks.get(chunk_id) {
                return Ok(Some(chunk.clone()));
            }
        }

        // Try to fetch from Kademlia DHT as a record
        let chunk_key = libp2p::kad::RecordKey::new(&chunk_id);
        self.swarm
            .lock()
            .await
            .behaviour_mut()
            .kademlia
            .get_record(chunk_key);

        // Wait briefly for the DHT lookup (best-effort with timeout)
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(5);
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            // Check if the chunk arrived locally via the event loop
            {
                let chunks = self.stored_chunks.read().await;
                if let Some(chunk) = chunks.get(chunk_id) {
                    return Ok(Some(chunk.clone()));
                }
            }
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }

        debug!(
            "Chunk {} not found locally or in DHT after timeout",
            chunk_id
        );
        Ok(None)
    }

    /// Announce file availability to the network
    pub async fn announce_file(&self, file_id: &str, chunk_ids: Vec<String>) -> Result<()> {
        info!(
            "Announcing file: {} with {} chunks",
            file_id,
            chunk_ids.len()
        );

        // Use Kademlia to announce file availability
        let file_key = libp2p::kad::RecordKey::new(&file_id);
        self.swarm
            .lock()
            .await
            .behaviour_mut()
            .kademlia
            .start_providing(file_key)?;

        Ok(())
    }

    /// Subscribe to a content topic for selective file replication
    pub async fn subscribe_topic(&self, topic: &str) -> Result<()> {
        let topic = gossipsub::IdentTopic::new(topic);
        self.swarm
            .lock()
            .await
            .behaviour_mut()
            .gossipsub
            .subscribe(&topic)
            .map_err(|e| anyhow::anyhow!("Failed to subscribe to topic {}: {}", topic, e))?;
        info!("Subscribed to content topic: {}", topic);
        Ok(())
    }

    /// Unsubscribe from a content topic
    pub async fn unsubscribe_topic(&self, topic: &str) -> Result<()> {
        let topic = gossipsub::IdentTopic::new(topic);
        let ok = self
            .swarm
            .lock()
            .await
            .behaviour_mut()
            .gossipsub
            .unsubscribe(&topic);
        if !ok {
            anyhow::bail!("Failed to unsubscribe from topic (was not subscribed)");
        }
        info!("Unsubscribed from content topic: {}", topic);
        Ok(())
    }

    /// Publish a message to a content topic (e.g., file replication announcements)
    pub async fn publish_to_topic(&self, topic: &str, data: Vec<u8>) -> Result<()> {
        let topic = gossipsub::IdentTopic::new(topic);
        self.swarm
            .lock()
            .await
            .behaviour_mut()
            .gossipsub
            .publish(topic.clone(), data)
            .map_err(|e| anyhow::anyhow!("Failed to publish to topic {}: {}", topic, e))?;
        Ok(())
    }

    /// Subscribe to all standard SpaceKit topics
    pub async fn subscribe_all_topics(&self) -> Result<()> {
        let topics = [
            "spacekit/files/v1",
            "spacekit/messages/v1",
            "spacekit/replication/v1",
            "spacekit/fees/v1",
        ];
        for t in &topics {
            self.subscribe_topic(t).await?;
        }
        Ok(())
    }

    /// Get connected peers
    pub async fn get_connected_peers(&self) -> Vec<PeerId> {
        self.swarm.lock().await.connected_peers().copied().collect()
    }

    /// Count and byte sum of chunks retained in `stored_chunks`.
    pub async fn stored_chunks_memory_estimate(&self) -> (usize, u64) {
        let chunks = self.stored_chunks.read().await;
        let bytes: u64 = chunks.values().map(|c| c.data.len() as u64).sum();
        (chunks.len(), bytes)
    }

    /// Get network statistics
    pub async fn get_network_stats(&self) -> NetworkStats {
        let connected_peers = self.get_connected_peers().await;
        let stored_chunks = self.stored_chunks.read().await.len();
        let messaging_clients = self.messaging_clients.read().await.len();
        let known_dids = self.did_peer_mappings.read().await.len();

        NetworkStats {
            connected_peers: connected_peers.len(),
            stored_chunks,
            peer_list: connected_peers.into_iter().map(|p| p.to_string()).collect(),
            messaging_clients,
            known_dids,
            discovery_mode: self.discovery_mode.clone(),
        }
    }

    // ─── DID Document Replication ──────────────────────────────────────────────

    /// Store a DID document locally and broadcast it to the P2P network via Gossipsub.
    pub async fn store_did_document(&self, did: &str, document: Vec<u8>) -> Result<()> {
        info!(
            "Publishing DID document for {} ({} bytes) via Gossipsub",
            did,
            document.len()
        );

        // Store locally as a chunk
        let chunk = FileChunk {
            chunk_id: format!("did:document:{}", did),
            file_id: did.to_string(),
            chunk_index: 0,
            data: document.clone(),
            hash: String::new(),
            encrypted: false,
        };
        if self.cache_chunks_in_memory {
            let mut chunks = self.stored_chunks.write().await;
            chunks.insert(chunk.chunk_id.clone(), chunk);
        }

        // Announce via Kademlia
        let did_key = libp2p::kad::RecordKey::new(&format!("did:document:{}", did));
        self.swarm
            .lock()
            .await
            .behaviour_mut()
            .kademlia
            .start_providing(did_key)?;

        // Broadcast via Gossipsub
        let topic = gossipsub::IdentTopic::new(DID_TOPIC);
        // Message format: [did_len:u16le][did_bytes][doc_bytes]
        let mut msg = Vec::with_capacity(2 + did.len() + document.len());
        msg.extend_from_slice(&(did.len() as u16).to_le_bytes());
        msg.extend_from_slice(did.as_bytes());
        msg.extend_from_slice(&document);

        self.swarm
            .lock()
            .await
            .behaviour_mut()
            .gossipsub
            .publish(topic, msg)
            .map_err(|e| anyhow::anyhow!("Gossipsub publish failed: {}", e))?;

        info!("DID document for {} published to network", did);
        Ok(())
    }

    /// Resolve a DID document from local storage or the P2P network.
    pub async fn resolve_did_document(&self, did: &str) -> Result<Option<Vec<u8>>> {
        let key = format!("did:document:{}", did);

        // Check local storage first
        {
            let chunks = self.stored_chunks.read().await;
            if let Some(chunk) = chunks.get(&key) {
                return Ok(Some(chunk.data.clone()));
            }
        }

        // Query Kademlia providers
        let did_key = libp2p::kad::RecordKey::new(&key);
        self.swarm
            .lock()
            .await
            .behaviour_mut()
            .kademlia
            .get_providers(did_key);

        // Kademlia provider results come asynchronously via the event loop.
        // Return None for now; the caller can poll or wait for events.
        Ok(None)
    }

    // ─── Phase 2: Messaging Integration Methods ─────────────────────────────

    /// Set discovery mode
    pub async fn set_discovery_mode(&mut self, mode: DiscoveryMode) {
        info!(
            "Changing discovery mode from {:?} to {:?}",
            self.discovery_mode, mode
        );
        self.discovery_mode = mode;
    }

    /// Register a messaging node for hybrid discovery
    pub async fn register_messaging_node(
        &self,
        peer_id: PeerId,
        multiaddr: Multiaddr,
    ) -> Result<()> {
        info!("Registering messaging node: {} at {}", peer_id, multiaddr);

        let client = MessagingNodeClient {
            peer_id,
            multiaddr: multiaddr.clone(),
            last_contact: chrono::Utc::now(),
            connection_healthy: true,
        };

        {
            let mut clients = self.messaging_clients.write().await;
            clients.insert(peer_id, client);
        }

        // Register as messaging service
        self.register_service_for_peer(peer_id, "messaging".to_string())
            .await;

        let _ = self
            .event_sender
            .send(NetworkEvent::MessagingNodeDiscovered(peer_id, multiaddr));
        Ok(())
    }

    /// Get peer list from messaging nodes (bootstrap)
    pub async fn bootstrap_from_messaging_nodes(&self) -> Result<Vec<(PeerId, Multiaddr)>> {
        if !matches!(
            self.discovery_mode,
            DiscoveryMode::Hybrid | DiscoveryMode::MessagingOnly
        ) {
            return Ok(Vec::new());
        }

        info!("Bootstrapping peer discovery from messaging nodes");
        let mut bootstrap_peers = Vec::new();

        let clients = self.messaging_clients.read().await;
        for client in clients.values() {
            if client.connection_healthy {
                // TODO: Implement actual messaging node query
                // For now, add the messaging node itself as a peer
                bootstrap_peers.push((client.peer_id, client.multiaddr.clone()));
                info!("Added messaging node to bootstrap list: {}", client.peer_id);
            }
        }

        Ok(bootstrap_peers)
    }

    /// Health check messaging nodes
    pub async fn health_check_messaging_nodes(&self) {
        let mut clients = self.messaging_clients.write().await;
        let current_time = chrono::Utc::now();

        for (peer_id, client) in clients.iter_mut() {
            let time_since_contact = current_time.signed_duration_since(client.last_contact);
            let healthy = time_since_contact.num_minutes() < 5; // 5 minute timeout

            if client.connection_healthy != healthy {
                client.connection_healthy = healthy;
                let _ = self
                    .event_sender
                    .send(NetworkEvent::CrossServiceHealthCheck {
                        service_type: "messaging".to_string(),
                        peer_id: *peer_id,
                        healthy,
                    });

                if healthy {
                    info!("Messaging node {} is back online", peer_id);
                } else {
                    warn!("Messaging node {} appears offline", peer_id);
                }
            }
        }
    }

    // Phase 3: Cross-Service DID Resolution Methods

    /// Register a DID to peer mapping
    pub async fn register_did_mapping(
        &self,
        did: String,
        peer_id: PeerId,
        multiaddr: Multiaddr,
        service_types: Vec<String>,
    ) -> Result<()> {
        info!(
            "Registering DID mapping: {} -> {} (services: {:?})",
            did, peer_id, service_types
        );

        let mapping = DIDPeerMapping {
            did: did.clone(),
            peer_id,
            multiaddr: multiaddr.clone(),
            service_types: service_types.clone(),
            last_seen: chrono::Utc::now(),
            reputation_score: 1.0, // Default reputation
        };

        {
            let mut mappings = self.did_peer_mappings.write().await;
            mappings.insert(did.clone(), mapping);
        }

        // Update service registry
        {
            let mut registry = self.service_registry.write().await;
            registry.insert(peer_id, service_types);
        }

        let _ = self.event_sender.send(NetworkEvent::UserDIDResolved {
            did,
            peer_id,
            multiaddr,
        });

        Ok(())
    }

    /// Resolve DID to peer information
    pub async fn resolve_did(&self, did: &str) -> Option<DIDPeerMapping> {
        let mappings = self.did_peer_mappings.read().await;
        mappings.get(did).cloned()
    }

    /// Find peers providing a specific service
    pub async fn find_service_providers(
        &self,
        service_type: &str,
    ) -> Vec<(PeerId, Vec<Multiaddr>)> {
        let registry = self.service_registry.read().await;
        let peer_addresses = self.peer_addresses.read().await;

        let mut providers = Vec::new();

        for (peer_id, services) in registry.iter() {
            if services.contains(&service_type.to_string()) {
                if let Some(addresses) = peer_addresses.get(peer_id) {
                    providers.push((*peer_id, addresses.clone()));
                }
            }
        }

        info!(
            "Found {} providers for service: {}",
            providers.len(),
            service_type
        );
        providers
    }

    /// Register a service for a peer
    pub async fn register_service_for_peer(&self, peer_id: PeerId, service_type: String) {
        let mut registry = self.service_registry.write().await;
        registry
            .entry(peer_id)
            .or_insert_with(Vec::new)
            .push(service_type.clone());

        debug!(
            "Registered service '{}' for peer: {}",
            service_type, peer_id
        );
    }

    /// Get all known DIDs
    pub async fn get_known_dids(&self) -> Vec<String> {
        let mappings = self.did_peer_mappings.read().await;
        mappings.keys().cloned().collect()
    }

    /// Update reputation score for a DID
    pub async fn update_did_reputation(&self, did: &str, score_delta: f64) -> Result<()> {
        let mut mappings = self.did_peer_mappings.write().await;

        if let Some(mapping) = mappings.get_mut(did) {
            mapping.reputation_score = (mapping.reputation_score + score_delta).clamp(0.0, 5.0);
            mapping.last_seen = chrono::Utc::now();
            info!(
                "Updated reputation for DID {}: {:.2}",
                did, mapping.reputation_score
            );
            Ok(())
        } else {
            Err(anyhow::anyhow!("DID not found: {}", did))
        }
    }

    /// Cleanup stale DID mappings
    pub async fn cleanup_stale_mappings(&self) {
        let mut mappings = self.did_peer_mappings.write().await;
        let current_time = chrono::Utc::now();
        let stale_threshold = chrono::Duration::hours(24);

        let initial_count = mappings.len();
        mappings.retain(|did, mapping| {
            let age = current_time.signed_duration_since(mapping.last_seen);
            let is_fresh = age < stale_threshold;

            if !is_fresh {
                info!(
                    "Removing stale DID mapping: {} (last seen: {})",
                    did, mapping.last_seen
                );
            }

            is_fresh
        });

        let removed_count = initial_count - mappings.len();
        if removed_count > 0 {
            info!("Cleaned up {} stale DID mappings", removed_count);
        }
    }
}

/// Network statistics
#[derive(Debug, Serialize)]
pub struct NetworkStats {
    pub connected_peers: usize,
    pub stored_chunks: usize,
    pub peer_list: Vec<String>,
    pub messaging_clients: usize,
    pub known_dids: usize,
    pub discovery_mode: DiscoveryMode,
}
