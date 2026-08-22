//! SWTCH Network Integration
//!
//! Provides real P2P communication and service discovery for SWTCH compute nodes.
//! Messages are length-prefixed JSON frames over TCP.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio::time::{interval, timeout};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::quantum_security::{QuantumResistantDID, QuantumResistantEncryption};
use crate::spacekitvm::{SwtchvmBlock, SwtchvmNode};

// ─── Wire protocol messages ────────────────────────────────────────────────

/// Typed messages exchanged between compute nodes over TCP.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum P2PMessage {
    /// Initial handshake: sender advertises its node_id and DID.
    Handshake { node_id: String, did: String },
    /// Acknowledge a handshake.
    HandshakeAck { node_id: String, did: String },
    /// Announce a newly produced block.
    BlockAnnounce {
        block_number: u64,
        block_hash: String,
        proposer_did: String,
        state_root: String,
        parent_hash: String,
        timestamp: i64,
    },
    /// Request a range of blocks for catch-up sync.
    BlockRequest { from_block: u64, to_block: u64 },
    /// Response with serialised block data.
    BlockResponse {
        block_number: u64,
        block_json: String,
    },
    /// SwtchVM chain tip advertisement, used to initiate late-join catch-up.
    SwtchvmChainHead {
        chain_id: String,
        block_number: u64,
        block_hash: String,
    },
    /// Complete canonical SwtchVM block. Receivers re-execute it before appending.
    SwtchvmBlockAnnounce {
        chain_id: String,
        block_json: String,
    },
    /// Consensus vote on a proposal.
    ConsensusVote {
        proposal_id: String,
        voter_did: String,
        vote_type: String,
        signature_hex: String,
        round: u64,
        /// JSON-serialized [`spacekit_spacetime_consensus::ConsensusVoteInner`] when PQ policy is enabled.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Dilithium [`ConsensusVoteInner`] JSON (includes `validator_rotor_digest`; replaces separate TransitionWitness gossip).
        pq_vote_json: Option<String>,
    },
    /// Service discovery request / response.
    ServiceAnnounce(ServiceInfo),
}

// ─── Core types ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct NetworkService {
    inner: Arc<RwLock<NetworkServiceInner>>,
    command_tx: mpsc::UnboundedSender<NetworkCommand>,
    /// Subscribe to receive incoming P2P messages from all peers.
    incoming_tx: broadcast::Sender<P2PMessage>,
}

struct NetworkServiceInner {
    config: NetworkConfig,
    node_id: String,
    node_did: String,
    identity: Arc<QuantumResistantDID>,

    peers: HashMap<String, PeerInfo>,
    connected_peers: HashSet<String>,
    /// Write halves of active peer connections, keyed by address.
    peer_writers: HashMap<String, Arc<RwLock<tokio::io::WriteHalf<TcpStream>>>>,

    services: HashMap<String, ServiceInfo>,
    local_services: Vec<ServiceInfo>,

    is_running: bool,
    start_time: SystemTime,

    messages_sent: u64,
    messages_received: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub network_name: String,
    pub listen_address: String,
    pub listen_port: u16,
    pub bootstrap_nodes: Vec<String>,
    pub max_peers: usize,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            network_name: "swtch-compute-network".to_string(),
            listen_address: "127.0.0.1".to_string(),
            listen_port: 9000,
            bootstrap_nodes: vec!["127.0.0.1:9001".to_string()],
            max_peers: 50,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub peer_id: String,
    pub address: String,
    pub capabilities: Vec<String>,
    pub last_seen: DateTime<Utc>,
    pub reputation_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceInfo {
    pub service_id: String,
    pub service_type: String,
    pub did: String,
    pub endpoint: String,
    pub capabilities: Vec<String>,
    pub stake_amount: u64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NetworkStatus {
    pub peer_count: u32,
    pub is_connected: bool,
    pub network_name: String,
    pub uptime: Duration,
    pub total_messages_sent: u64,
    pub total_messages_received: u64,
}

#[derive(Debug, Clone)]
enum NetworkCommand {
    Connect(String),
    RegisterService(ServiceInfo),
    DiscoverServices,
    Broadcast(P2PMessage),
}

// ─── Framing helpers: 4-byte big-endian length prefix + JSON ───────────────

async fn write_frame(writer: &mut (impl AsyncWriteExt + Unpin), msg: &P2PMessage) -> Result<()> {
    let payload = serde_json::to_vec(msg)?;
    let len = (payload.len() as u32).to_be_bytes();
    writer.write_all(&len).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

async fn read_frame(reader: &mut (impl AsyncReadExt + Unpin)) -> Result<Option<P2PMessage>> {
    let mut len_buf = [0u8; 4];
    match reader.read_exact(&mut len_buf).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 16 * 1024 * 1024 {
        return Err(anyhow::anyhow!("frame too large: {} bytes", len));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).await?;
    let msg: P2PMessage = serde_json::from_slice(&buf)?;
    Ok(Some(msg))
}

// ─── NetworkService implementation ──────────────────────────────────────────

impl NetworkService {
    pub async fn new(
        config: NetworkConfig,
        identity: Arc<QuantumResistantDID>,
        _encryption: Arc<QuantumResistantEncryption>,
    ) -> Result<Self> {
        let node_id = format!("node_{}", Uuid::new_v4());
        let node_did = crate::quantum_security::quantum_did_utils::get_did(&identity);
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (incoming_tx, _) = broadcast::channel(4096);

        let inner = Arc::new(RwLock::new(NetworkServiceInner {
            config,
            node_id,
            node_did,
            identity,
            peers: HashMap::new(),
            connected_peers: HashSet::new(),
            peer_writers: HashMap::new(),
            services: HashMap::new(),
            local_services: Vec::new(),
            is_running: false,
            start_time: SystemTime::now(),
            messages_sent: 0,
            messages_received: 0,
        }));

        let service = Self {
            inner: inner.clone(),
            command_tx,
            incoming_tx: incoming_tx.clone(),
        };

        tokio::spawn(Self::handle_commands(
            inner.clone(),
            command_rx,
            incoming_tx,
        ));

        Ok(service)
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting SpaceKit Network Service...");

        let mut inner = self.inner.write().await;
        inner.is_running = true;
        inner.start_time = SystemTime::now();

        let listen_addr = format!(
            "{}:{}",
            inner.config.listen_address, inner.config.listen_port
        );

        let inner_clone = self.inner.clone();
        let incoming_tx = self.incoming_tx.clone();
        tokio::spawn(async move {
            match TcpListener::bind(&listen_addr).await {
                Ok(listener) => {
                    info!("Network service listening on {}", listen_addr);
                    Self::accept_connections(inner_clone, listener, incoming_tx).await;
                }
                Err(e) => error!("Failed to bind P2P listener on {}: {}", listen_addr, e),
            }
        });

        for bootstrap_addr in &inner.config.bootstrap_nodes {
            let _ = self
                .command_tx
                .send(NetworkCommand::Connect(bootstrap_addr.clone()));
        }

        let command_tx_clone = self.command_tx.clone();
        tokio::spawn(async move {
            let mut discovery_interval = interval(Duration::from_secs(60));
            loop {
                discovery_interval.tick().await;
                let _ = command_tx_clone.send(NetworkCommand::DiscoverServices);
            }
        });

        info!("SpaceKit Network Service started successfully");
        Ok(())
    }

    // ── Connection handling ─────────────────────────────────────────────────

    async fn accept_connections(
        inner: Arc<RwLock<NetworkServiceInner>>,
        listener: TcpListener,
        incoming_tx: broadcast::Sender<P2PMessage>,
    ) {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("Incoming connection from {}", addr);
                    let inner_clone = inner.clone();
                    let tx = incoming_tx.clone();
                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_inbound(inner_clone, stream, tx).await {
                            warn!("Inbound connection error from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn handle_inbound(
        inner: Arc<RwLock<NetworkServiceInner>>,
        stream: TcpStream,
        incoming_tx: broadcast::Sender<P2PMessage>,
    ) -> Result<()> {
        let peer_addr = stream.peer_addr()?.to_string();
        let (mut reader, writer) = tokio::io::split(stream);
        let writer = Arc::new(RwLock::new(writer));

        // Send our handshake
        let (our_id, our_did) = {
            let guard = inner.read().await;
            (guard.node_id.clone(), guard.node_did.clone())
        };
        {
            let mut w = writer.write().await;
            write_frame(
                &mut *w,
                &P2PMessage::HandshakeAck {
                    node_id: our_id.clone(),
                    did: our_did,
                },
            )
            .await?;
        }

        // Expect handshake from peer
        let peer_info = match timeout(Duration::from_secs(10), read_frame(&mut reader)).await {
            Ok(Ok(Some(P2PMessage::Handshake { node_id, did }))) => {
                info!("Inbound handshake from {} (DID: {})", node_id, did);
                PeerInfo {
                    peer_id: node_id,
                    address: peer_addr.clone(),
                    capabilities: vec!["compute".to_string()],
                    last_seen: Utc::now(),
                    reputation_score: 0.5,
                }
            }
            _ => {
                warn!("Handshake timeout or invalid from {}", peer_addr);
                return Ok(());
            }
        };

        // Register peer and writer
        {
            let mut guard = inner.write().await;
            guard
                .peers
                .insert(peer_info.peer_id.clone(), peer_info.clone());
            guard.connected_peers.insert(peer_addr.clone());
            guard.peer_writers.insert(peer_addr.clone(), writer);
            guard.messages_received += 1;
        }

        // Read loop: forward incoming messages to the broadcast channel.
        Self::read_loop(inner, &mut reader, incoming_tx, peer_addr).await;
        Ok(())
    }

    async fn handle_outbound(
        inner: Arc<RwLock<NetworkServiceInner>>,
        stream: TcpStream,
        incoming_tx: broadcast::Sender<P2PMessage>,
        address: String,
    ) -> Result<()> {
        let (mut reader, writer) = tokio::io::split(stream);
        let writer = Arc::new(RwLock::new(writer));

        // Send our handshake
        let (our_id, our_did) = {
            let guard = inner.read().await;
            (guard.node_id.clone(), guard.node_did.clone())
        };
        {
            let mut w = writer.write().await;
            write_frame(
                &mut *w,
                &P2PMessage::Handshake {
                    node_id: our_id.clone(),
                    did: our_did,
                },
            )
            .await?;
        }

        // Expect ack
        let peer_info = match timeout(Duration::from_secs(10), read_frame(&mut reader)).await {
            Ok(Ok(Some(P2PMessage::HandshakeAck { node_id, did }))) => {
                info!("Outbound handshake ack from {} (DID: {})", node_id, did);
                PeerInfo {
                    peer_id: node_id,
                    address: address.clone(),
                    capabilities: vec!["compute".to_string()],
                    last_seen: Utc::now(),
                    reputation_score: 0.5,
                }
            }
            _ => {
                warn!("Handshake failed with {}", address);
                return Ok(());
            }
        };

        {
            let mut guard = inner.write().await;
            guard.peers.insert(peer_info.peer_id.clone(), peer_info);
            guard.connected_peers.insert(address.clone());
            guard.peer_writers.insert(address.clone(), writer);
            guard.messages_sent += 1;
        }

        Self::read_loop(inner, &mut reader, incoming_tx, address).await;
        Ok(())
    }

    async fn read_loop(
        inner: Arc<RwLock<NetworkServiceInner>>,
        reader: &mut tokio::io::ReadHalf<TcpStream>,
        incoming_tx: broadcast::Sender<P2PMessage>,
        peer_addr: String,
    ) {
        loop {
            match read_frame(reader).await {
                Ok(Some(msg)) => {
                    debug!(
                        "Received {:?} from {}",
                        std::mem::discriminant(&msg),
                        peer_addr
                    );
                    {
                        let mut guard = inner.write().await;
                        guard.messages_received += 1;
                    }
                    let _ = incoming_tx.send(msg);
                }
                Ok(None) => {
                    info!("Peer {} disconnected", peer_addr);
                    break;
                }
                Err(e) => {
                    warn!("Read error from {}: {}", peer_addr, e);
                    break;
                }
            }
        }

        // Clean up peer on disconnect
        let mut guard = inner.write().await;
        guard.connected_peers.remove(&peer_addr);
        guard.peer_writers.remove(&peer_addr);
    }

    // ── Command handler ─────────────────────────────────────────────────────

    async fn handle_commands(
        inner: Arc<RwLock<NetworkServiceInner>>,
        mut command_rx: mpsc::UnboundedReceiver<NetworkCommand>,
        incoming_tx: broadcast::Sender<P2PMessage>,
    ) {
        while let Some(command) = command_rx.recv().await {
            match command {
                NetworkCommand::Connect(address) => {
                    let inner_c = inner.clone();
                    let tx = incoming_tx.clone();
                    tokio::spawn(async move {
                        Self::connect_to_peer(inner_c, address, tx).await;
                    });
                }
                NetworkCommand::RegisterService(service) => {
                    let mut guard = inner.write().await;
                    guard.local_services.push(service.clone());
                    guard.services.insert(service.service_id.clone(), service);
                    info!("Service registered locally");
                }
                NetworkCommand::DiscoverServices => {
                    Self::discover_services_internal(inner.clone()).await;
                }
                NetworkCommand::Broadcast(msg) => {
                    Self::broadcast_to_peers(inner.clone(), &msg).await;
                }
            }
        }
    }

    async fn connect_to_peer(
        inner: Arc<RwLock<NetworkServiceInner>>,
        address: String,
        incoming_tx: broadcast::Sender<P2PMessage>,
    ) {
        // Skip if already connected
        {
            let guard = inner.read().await;
            if guard.connected_peers.contains(&address) {
                debug!("Already connected to {}, skipping", address);
                return;
            }
        }

        info!("Connecting to peer: {}", address);
        match timeout(Duration::from_secs(10), TcpStream::connect(&address)).await {
            Ok(Ok(stream)) => {
                if let Err(e) =
                    Self::handle_outbound(inner, stream, incoming_tx, address.clone()).await
                {
                    warn!("Outbound connection to {} failed: {}", address, e);
                }
            }
            Ok(Err(e)) => warn!("TCP connect to {} failed: {}", address, e),
            Err(_) => warn!("Connect to {} timed out", address),
        }
    }

    async fn broadcast_to_peers(inner: Arc<RwLock<NetworkServiceInner>>, msg: &P2PMessage) {
        let writers: Vec<(String, Arc<RwLock<tokio::io::WriteHalf<TcpStream>>>)> = {
            let guard = inner.read().await;
            guard
                .peer_writers
                .iter()
                .map(|(addr, w)| (addr.clone(), w.clone()))
                .collect()
        };

        let mut failed = Vec::new();
        for (addr, writer) in &writers {
            let mut w = writer.write().await;
            if let Err(e) = write_frame(&mut *w, msg).await {
                warn!("Failed to send to {}: {}", addr, e);
                failed.push(addr.clone());
            }
        }

        if !failed.is_empty() {
            let mut guard = inner.write().await;
            for addr in failed {
                guard.connected_peers.remove(&addr);
                guard.peer_writers.remove(&addr);
            }
        }

        {
            let mut guard = inner.write().await;
            guard.messages_sent += writers.len() as u64;
        }

        debug!("Broadcast message to {} peers", writers.len());
    }

    async fn discover_services_internal(inner: Arc<RwLock<NetworkServiceInner>>) {
        debug!("Discovering services from peers");
        let guard = inner.read().await;
        let peer_count = guard.connected_peers.len();
        if peer_count > 0 {
            debug!("Querying {} peers for services", peer_count);
        }
    }

    // ── Public API ──────────────────────────────────────────────────────────

    pub async fn register_service(
        &self,
        service_info: ServiceInfo,
        _signature: Vec<u8>,
    ) -> Result<()> {
        self.command_tx
            .send(NetworkCommand::RegisterService(service_info))?;
        Ok(())
    }

    /// Broadcast a typed P2P message to all connected peers.
    pub fn broadcast(&self, msg: P2PMessage) -> Result<()> {
        self.command_tx.send(NetworkCommand::Broadcast(msg))?;
        Ok(())
    }

    /// Subscribe to incoming P2P messages from all peers.
    pub fn subscribe(&self) -> broadcast::Receiver<P2PMessage> {
        self.incoming_tx.subscribe()
    }

    pub async fn get_status(&self) -> Result<NetworkStatus> {
        let guard = self.inner.read().await;
        let uptime = guard.start_time.elapsed().unwrap_or_default();
        Ok(NetworkStatus {
            peer_count: guard.connected_peers.len() as u32,
            is_connected: !guard.connected_peers.is_empty(),
            network_name: guard.config.network_name.clone(),
            uptime,
            total_messages_sent: guard.messages_sent,
            total_messages_received: guard.messages_received,
        })
    }

    pub async fn discover_services(
        &self,
        service_type: Option<String>,
    ) -> Result<Vec<ServiceInfo>> {
        let _ = service_type;
        self.command_tx.send(NetworkCommand::DiscoverServices)?;
        let guard = self.inner.read().await;
        Ok(guard.services.values().cloned().collect())
    }

    pub async fn get_peers(&self) -> Vec<PeerInfo> {
        let guard = self.inner.read().await;
        guard.peers.values().cloned().collect()
    }

    pub async fn peer_count(&self) -> usize {
        let guard = self.inner.read().await;
        guard.connected_peers.len()
    }
}

#[derive(Serialize, Deserialize)]
struct SwtchvmWireBlock {
    chain_id: String,
    block: SwtchvmBlock,
}

/// Bridge a SwtchVM node to the real TCP P2P service.
///
/// Locally mined blocks are announced with full transaction and receipt data. Incoming blocks are
/// accepted only through [`SwtchvmNode::import_block`], which re-executes their transactions.
/// Periodic head advertisements let nodes that connect after mining request missing blocks.
pub fn start_swtchvm_bridge(vm: Arc<SwtchvmNode>, network: NetworkService) {
    let mut mined = vm.subscribe_mined_blocks();
    let mut incoming = network.subscribe();
    tokio::spawn(async move {
        let mut head_interval = interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                local = mined.recv() => match local {
                    Ok(block) => {
                        let wire = SwtchvmWireBlock {
                            chain_id: vm.chain_id().to_string(),
                            block,
                        };
                        match serde_json::to_string(&wire) {
                            Ok(block_json) => {
                                let _ = network.broadcast(P2PMessage::SwtchvmBlockAnnounce {
                                    chain_id: wire.chain_id,
                                    block_json,
                                });
                            }
                            Err(error) => warn!("Failed to serialize mined SwtchVM block: {}", error),
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("SwtchVM P2P bridge lagged by {} locally mined blocks", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                message = incoming.recv() => match message {
                    Ok(P2PMessage::SwtchvmChainHead { chain_id, block_number, block_hash }) => {
                        if chain_id != vm.chain_id() {
                            warn!("Ignoring SwtchVM head for foreign chain {}", chain_id);
                            continue;
                        }
                        let local = vm.get_latest_block();
                        if block_number > local.number {
                            let _ = network.broadcast(P2PMessage::BlockRequest {
                                from_block: local.number + 1,
                                to_block: block_number,
                            });
                        } else if block_number == local.number
                            && block_hash != hex::encode(local.hash)
                        {
                            warn!("Rejecting forked SwtchVM head at height {}", block_number);
                        }
                    }
                    Ok(P2PMessage::SwtchvmBlockAnnounce { chain_id, block_json }) => {
                        match serde_json::from_str::<SwtchvmWireBlock>(&block_json) {
                            Ok(wire) if wire.chain_id == chain_id => {
                                let local_height = vm.get_latest_block().number;
                                if wire.block.number > local_height + 1 {
                                    let _ = network.broadcast(P2PMessage::BlockRequest {
                                        from_block: local_height + 1,
                                        to_block: wire.block.number,
                                    });
                                } else if let Err(error) = vm.import_block(&chain_id, wire.block).await {
                                    warn!("Rejected announced SwtchVM block: {}", error);
                                }
                            }
                            Ok(_) => warn!("Rejected SwtchVM block with inconsistent chain envelope"),
                            Err(error) => warn!("Rejected malformed SwtchVM block: {}", error),
                        }
                    }
                    Ok(P2PMessage::BlockRequest { from_block, to_block }) => {
                        let head = vm.get_latest_block().number;
                        let end = to_block.min(head).min(from_block.saturating_add(255));
                        if from_block <= end {
                            for number in from_block..=end {
                                if let Some(block) = vm.get_block_by_number(number) {
                                    let wire = SwtchvmWireBlock {
                                        chain_id: vm.chain_id().to_string(),
                                        block,
                                    };
                                    if let Ok(block_json) = serde_json::to_string(&wire) {
                                        let _ = network.broadcast(P2PMessage::BlockResponse {
                                            block_number: number,
                                            block_json,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Ok(P2PMessage::BlockResponse { block_number, block_json }) => {
                        match serde_json::from_str::<SwtchvmWireBlock>(&block_json) {
                            Ok(wire) if wire.block.number == block_number => {
                                if let Err(error) = vm.import_block(&wire.chain_id, wire.block).await {
                                    debug!("Ignored SwtchVM catch-up block {}: {}", block_number, error);
                                }
                            }
                            Ok(_) => warn!("Rejected block response with mismatched height"),
                            Err(error) => warn!("Rejected malformed block response: {}", error),
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!("SwtchVM P2P bridge lagged by {} network messages", skipped);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = head_interval.tick() => {
                    let head = vm.get_latest_block();
                    let _ = network.broadcast(P2PMessage::SwtchvmChainHead {
                        chain_id: vm.chain_id().to_string(),
                        block_number: head.number,
                        block_hash: hex::encode(head.hash),
                    });
                }
            }
        }
    });
}

// Convenience constructors for compatibility with existing code
impl NetworkService {
    pub async fn new_simple(name: &str, _endpoint: &str, port: u16) -> Result<Self> {
        let identity = Arc::new(
            crate::quantum_security::quantum_did_utils::new_did(
                "did:spacekit:network:alice",
                "Kyber1024",
            )
            .await?,
        );
        let encryption = Arc::new(
            QuantumResistantEncryption::new("Kyber1024", &["Kyber1024".to_string()]).await?,
        );

        let config = NetworkConfig {
            network_name: name.to_string(),
            listen_address: "127.0.0.1".to_string(),
            listen_port: port,
            ..NetworkConfig::default()
        };

        Self::new(config, identity, encryption).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spacekitvm::{SwtchvmAddress, SwtchvmTransaction, TransactionSignature};

    #[tokio::test]
    async fn test_network_service_creation() {
        let identity = Arc::new(
            crate::quantum_security::quantum_did_utils::new_did("did:spacekit:test", "Kyber1024")
                .await
                .unwrap(),
        );
        let encryption = Arc::new(
            QuantumResistantEncryption::new("Kyber1024", &["Kyber1024".to_string()])
                .await
                .unwrap(),
        );

        let service = NetworkService::new(NetworkConfig::default(), identity, encryption)
            .await
            .unwrap();

        let status = service.get_status().await.unwrap();
        assert_eq!(status.peer_count, 0);
        assert_eq!(status.network_name, "spacekit-compute-network");
    }

    #[tokio::test]
    async fn test_service_registration() {
        let service = NetworkService::new_simple("test-net", "127.0.0.1", 9999)
            .await
            .unwrap();

        let service_info = ServiceInfo {
            service_id: "test-service".to_string(),
            service_type: "compute".to_string(),
            did: "did:spacekit:test".to_string(),
            endpoint: "http://localhost:8080".to_string(),
            capabilities: vec!["wasm".to_string()],
            stake_amount: 1000,
            created_at: Utc::now(),
        };

        let result = service.register_service(service_info, vec![]).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_broadcast_subscribe() {
        let service = NetworkService::new_simple("test-net", "127.0.0.1", 9998)
            .await
            .unwrap();
        let mut rx = service.subscribe();

        // Broadcast with no peers should succeed without error
        let msg = P2PMessage::BlockAnnounce {
            block_number: 1,
            block_hash: "abc".to_string(),
            proposer_did: "did:spacekit:test".to_string(),
            state_root: "root".to_string(),
            parent_hash: "parent".to_string(),
            timestamp: 0,
        };
        assert!(service.broadcast(msg).is_ok());
    }

    #[tokio::test]
    async fn test_message_framing_roundtrip() {
        let msg = P2PMessage::ConsensusVote {
            proposal_id: "p1".to_string(),
            voter_did: "did:spacekit:test".to_string(),
            vote_type: "approve".to_string(),
            signature_hex: "deadbeef".to_string(),
            round: 5,
            pq_vote_json: None,
        };
        let payload = serde_json::to_vec(&msg).unwrap();
        assert!(payload.len() < 16 * 1024 * 1024);
        let decoded: P2PMessage = serde_json::from_slice(&payload).unwrap();
        match decoded {
            P2PMessage::ConsensusVote { round, .. } => assert_eq!(round, 5),
            _ => panic!("wrong variant"),
        }
    }

    fn unused_port() -> u16 {
        std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    async fn test_service(port: u16, bootstrap_nodes: Vec<String>) -> NetworkService {
        let identity = Arc::new(
            crate::quantum_security::quantum_did_utils::new_did(
                &format!("did:spacekit:p2p-test:{port}"),
                "Kyber1024",
            )
            .await
            .unwrap(),
        );
        let encryption = Arc::new(
            QuantumResistantEncryption::new("Kyber1024", &["Kyber1024".to_string()])
                .await
                .unwrap(),
        );
        NetworkService::new(
            NetworkConfig {
                network_name: "swtchvm-p2p-test".into(),
                listen_address: "127.0.0.1".into(),
                listen_port: port,
                bootstrap_nodes,
                max_peers: 8,
            },
            identity,
            encryption,
        )
        .await
        .unwrap()
    }

    fn signed_transaction(
        key: &k256::ecdsa::SigningKey,
        to: Option<SwtchvmAddress>,
        data: Vec<u8>,
        nonce: u64,
    ) -> SwtchvmTransaction {
        use k256::ecdsa::signature::hazmat::PrehashSigner;
        use k256::elliptic_curve::sec1::ToEncodedPoint;
        use sha2::{Digest as _, Sha256};
        use sha3::Keccak256;

        let point = key.verifying_key().to_encoded_point(false);
        let from = {
            let full: [u8; 32] = Keccak256::digest(&point.as_bytes()[1..]).into();
            let mut address = [0u8; 20];
            address.copy_from_slice(&full[12..]);
            SwtchvmAddress::new(address)
        };
        let to_hex = to
            .as_ref()
            .map(|address| hex::encode(address.as_bytes()))
            .unwrap_or_default();
        let canonical = format!(
            "{}|{}|{}|{}|{}",
            hex::encode(from.as_bytes()),
            to_hex,
            0u128,
            nonce,
            hex::encode(&data)
        );
        let prehash: [u8; 32] = Sha256::digest(canonical.as_bytes()).into();
        let (signature, recovery_id): (k256::ecdsa::Signature, k256::ecdsa::RecoveryId) =
            key.sign_prehash(&prehash).unwrap();
        let bytes = signature.to_bytes();
        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&bytes[..32]);
        s.copy_from_slice(&bytes[32..]);
        SwtchvmTransaction {
            from,
            to,
            data,
            gas_limit: 1_000_000,
            gas_price: 1,
            value: 0,
            nonce,
            signature: TransactionSignature {
                v: recovery_id.to_byte() + 27,
                r,
                s,
            },
        }
    }

    async fn wait_for_height(nodes: &[&SwtchvmNode], height: u64) {
        timeout(Duration::from_secs(10), async {
            loop {
                if nodes
                    .iter()
                    .all(|node| node.get_latest_block().number == height)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("chain convergence timeout");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn swtchvm_blocks_converge_and_late_peer_catches_up() {
        let port0 = unused_port();
        let port1 = unused_port();
        let port2 = unused_port();
        let net0 = test_service(port0, vec![]).await;
        net0.start().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let net1 = test_service(port1, vec![format!("127.0.0.1:{port0}")]).await;
        net1.start().await.unwrap();

        let mut vm0 = SwtchvmNode::new(false, false).await.unwrap();
        let mut vm1 = SwtchvmNode::new(false, false).await.unwrap();
        vm0.set_chain_id("4242".into(), 4242);
        vm1.set_chain_id("4242".into(), 4242);
        let vm0 = Arc::new(vm0);
        let vm1 = Arc::new(vm1);
        start_swtchvm_bridge(vm0.clone(), net0.clone());
        start_swtchvm_bridge(vm1.clone(), net1.clone());

        let key = k256::ecdsa::SigningKey::random(&mut rand::thread_rng());
        let deploy = signed_transaction(
            &key,
            None,
            wat::parse_str(
                r#"(module
                    (memory (export "memory") 1)
                    (func (export "main") (param i32 i32) (result i32) i32.const 0)
                )"#,
            )
            .unwrap(),
            0,
        );
        for node in [&vm0, &vm1] {
            node.set_account_balance(&deploy.from, 10_000_000)
                .await
                .unwrap();
        }
        vm0.submit_transaction(deploy.clone()).await.unwrap();
        let deploy_block = vm0.mine_block().await.unwrap();
        wait_for_height(&[vm0.as_ref(), vm1.as_ref()], 1).await;
        assert_eq!(vm0.get_latest_block().hash, vm1.get_latest_block().hash);
        let contract = deploy_block.receipts[0].created_address.unwrap();

        let call = signed_transaction(&key, Some(contract), Vec::new(), 1);
        vm0.submit_transaction(call).await.unwrap();
        vm0.mine_block().await.unwrap();
        wait_for_height(&[vm0.as_ref(), vm1.as_ref()], 2).await;
        assert_eq!(
            vm0.get_latest_block().state_root,
            vm1.get_latest_block().state_root
        );
        assert_eq!(
            vm0.get_account(&contract).await.unwrap().code,
            vm1.get_account(&contract).await.unwrap().code
        );

        let mut vm2 = SwtchvmNode::new(false, false).await.unwrap();
        vm2.set_chain_id("4242".into(), 4242);
        let vm2 = Arc::new(vm2);
        vm2.set_account_balance(&deploy.from, 10_000_000)
            .await
            .unwrap();
        let net2 = test_service(port2, vec![format!("127.0.0.1:{port0}")]).await;
        net2.start().await.unwrap();
        start_swtchvm_bridge(vm2.clone(), net2);
        wait_for_height(&[vm0.as_ref(), vm1.as_ref(), vm2.as_ref()], 2).await;
        assert_eq!(vm0.get_latest_block().hash, vm2.get_latest_block().hash);
        assert_eq!(
            vm0.get_account(&contract).await.unwrap().code,
            vm2.get_account(&contract).await.unwrap().code
        );

        let mut isolated = SwtchvmNode::new(false, false).await.unwrap();
        isolated.set_chain_id("4242".into(), 4242);
        isolated
            .set_account_balance(&deploy.from, 10_000_000)
            .await
            .unwrap();
        isolated
            .import_block("4242", vm0.get_block_by_number(1).unwrap())
            .await
            .unwrap();
        isolated
            .import_block("4242", vm0.get_block_by_number(2).unwrap())
            .await
            .unwrap();
        let mut tampered = isolated.mine_block().await.unwrap();
        tampered.state_root[0] ^= 1;
        assert!(vm2.import_block("4242", tampered).await.is_err());
        assert_eq!(vm2.get_latest_block().number, 2);
    }
}
