//! Basic P2P messaging example
//!
//! This example demonstrates how to use the P2P networking layer directly.
//!
//! Run two instances:
//! ```bash
//! # Terminal 1
//! cargo run --example p2p_basic -- --port 7001
//!
//! # Terminal 2 (after Terminal 1 starts)
//! cargo run --example p2p_basic -- --port 7002 --bootstrap /ip4/127.0.0.1/tcp/7001
//! ```

use libp2p::Multiaddr;
use spacekit_messaging_node::{
    network_p2p::{P2PCommand, P2PMessage, P2PNetwork, P2PNetworkEvent},
    MessagingConfig,
};
use tokio::sync::mpsc;
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Parse command line arguments
    let args: Vec<String> = std::env::args().collect();
    let port: u16 = args
        .get(1)
        .and_then(|s| s.strip_prefix("--port="))
        .and_then(|s| s.parse().ok())
        .unwrap_or(7001);

    let bootstrap_peer = args
        .get(2)
        .and_then(|s| s.strip_prefix("--bootstrap="))
        .map(|s| s.to_string());

    println!("🚀 Starting SpaceKit P2P Messaging Node on port {}", port);

    // Create configuration
    let config = MessagingConfig {
        node_did: format!("did:swtchx:node:{}", port),
        private_key: String::new(),
        listen_addr: format!("0.0.0.0:{}", port).parse().unwrap(),
        bootstrap_peers: bootstrap_peer.into_iter().collect(),
        default_quantum_algorithm: "Kyber768".to_string(),
        default_cipher_suite: "AES256".to_string(),
        max_connections: 100,
        message_retention_seconds: 86400,
        enable_peer_discovery: true,
        network: Default::default(),
        storage: Default::default(),
    };

    // Create channels for communication with P2P network
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let (command_tx, command_rx) = mpsc::unbounded_channel();

    // Create and start P2P network
    let mut p2p_network = P2PNetwork::new(&config, event_tx, command_rx).await?;

    // Listen on configured address
    let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
    p2p_network.listen(listen_addr.clone()).await?;

    println!("📡 Listening on {}", listen_addr);
    println!("🆔 Peer ID: {}", p2p_network.local_peer_id());

    // Subscribe to a test topic
    let topic = "swtchx/test/messages".to_string();
    command_tx.send(P2PCommand::Subscribe {
        topic: topic.clone(),
    })?;
    println!("📢 Subscribed to topic: {}", topic);

    // Spawn network event loop
    tokio::spawn(async move {
        if let Err(e) = p2p_network.run().await {
            eprintln!("Network error: {}", e);
        }
    });

    // Spawn event handler
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                P2PNetworkEvent::PeerConnected { peer_id, addresses } => {
                    println!("✅ Peer connected: {}", peer_id);
                    println!("   Addresses: {:?}", addresses);
                }
                P2PNetworkEvent::PeerDisconnected { peer_id } => {
                    println!("❌ Peer disconnected: {}", peer_id);
                }
                P2PNetworkEvent::PeerDiscovered { peer_id, addresses } => {
                    println!("🔍 Peer discovered (mDNS): {}", peer_id);
                    println!("   Addresses: {:?}", addresses);
                }
                P2PNetworkEvent::MessageReceived { from, message } => {
                    println!("📨 Message from {}: {:?}", from, message);
                }
            }
        }
    });

    // Keep main thread alive and allow sending test messages
    println!("\n💬 P2P Messenger running!");
    println!("   Commands:");
    println!("   - Type a message and press Enter to broadcast");
    println!("   - Press Ctrl+C to quit\n");

    let mut message_counter = 0;
    loop {
        // Read from stdin
        let mut input = String::new();
        if let Ok(_) = std::io::stdin().read_line(&mut input) {
            let input = input.trim();
            if !input.is_empty() {
                message_counter += 1;

                // Create a test message
                let test_message = P2PMessage::Presence {
                    did: format!("did:swtchx:user:node{}", port),
                    username: format!("Node{}", port),
                    status: input.to_string(),
                };

                // Publish to topic
                if let Err(e) = command_tx.send(P2PCommand::PublishTopic {
                    topic: topic.clone(),
                    message: test_message,
                }) {
                    eprintln!("Failed to send command: {}", e);
                }

                println!("📤 Sent message #{}", message_counter);
            }
        }

        // Small delay
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}
