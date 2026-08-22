//! Connection Diagnostics Test
//!
//! Tests the connection diagnostics system by:
//! 1. Starting multiple storage nodes
//! 2. Connecting them via P2P
//! 3. Monitoring connection health
//! 4. Generating connection events
//!
//! ## Usage
//!
//! ```bash
//! # Terminal 1 - Node 1
//! cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 1 --port 9001
//!
//! # Terminal 2 - Node 2 (connects to node 1)
//! cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 2 --port 9002 --bootstrap /ip4/127.0.0.1/tcp/9001/p2p/<peer-id-from-node-1>
//!
//! # Terminal 3 - Node 3 (connects to node 1)
//! cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 3 --port 9003 --bootstrap /ip4/127.0.0.1/tcp/9001/p2p/<peer-id-from-node-1>
//! ```

#![recursion_limit = "256"]

use anyhow::Result;
use spacekit_primitives::v1::identity::QuantumDID;
use spacekit_storage_node::{NetworkConfig, StorageNode, StorageNodeConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug)]
struct Args {
    node_id: u32,
    port: u16,
    bootstrap: Option<String>,
}

impl Args {
    fn from_env() -> Self {
        let mut args = std::env::args().skip(1);
        let mut node_id = 1;
        let mut port = 9001;
        let mut bootstrap = None;

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--node-id" => {
                    if let Some(val) = args.next() {
                        node_id = val.parse().unwrap_or(1);
                    }
                }
                "--port" => {
                    if let Some(val) = args.next() {
                        port = val.parse().unwrap_or(9001);
                    }
                }
                "--bootstrap" => {
                    if let Some(val) = args.next() {
                        bootstrap = Some(val);
                    }
                }
                _ => {}
            }
        }

        Self {
            node_id,
            port,
            bootstrap,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::from_env();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("\n🔍 Connection Diagnostics Test\n");
    println!("Node ID: {}", args.node_id);
    println!("Port: {}", args.port);
    if let Some(bootstrap) = &args.bootstrap {
        println!("Bootstrap: {}", bootstrap);
    }
    println!("{}", "=".repeat(60));

    // Create storage node
    let data_dir = format!("./connection_test_storage_{}", args.node_id);
    let database_path = format!("{}/node_{}.json", data_dir, args.node_id);

    let mut bootstrap_peers = Vec::new();
    if let Some(bootstrap_addr) = &args.bootstrap {
        bootstrap_peers.push(bootstrap_addr.clone());
        println!("✓ Configured bootstrap peer: {}", bootstrap_addr);
    }

    let network_config = NetworkConfig {
        listen_port: args.port,
        bootstrap_peers,
        max_connections: 50,
        replication_factor: 3,
        chunk_size: 1024 * 1024,
        max_concurrent_operations: Some(10),
        cache_p2p_chunks_in_memory: false,
    };

    let config = StorageNodeConfig {
        max_storage_bytes: 10 * 1024 * 1024 * 1024,
        data_dir: std::path::PathBuf::from(&data_dir),
        database_path: Some(std::path::PathBuf::from(&database_path)),
        node_did: format!("did:spacekit:storage:node-{}", args.node_id),
        preferred_algorithm: "kyber1024".to_string(),
        encryption_keypair: None,
        network_config,
        enable_p2p: false,
        enable_real_transactions: false,
        #[cfg(feature = "api-server")]
        api_config: None,
    };

    let storage_node = Arc::new(StorageNode::new(config).await?);
    storage_node.start().await?;

    println!("✓ Storage node started on port {}", args.port);

    // Wait for network to initialize
    sleep(Duration::from_secs(2)).await;

    // Monitor connections
    println!("\n📊 Monitoring Connections...\n");
    println!("Connection events will be logged below:");
    println!("  ✅ = Connection established");
    println!("  ❌ = Connection closed");
    println!("  ⚠️  = KeepAlive timeout detected");
    println!();

    // Periodic connection status reports
    let storage_node_clone = storage_node.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            if let Some(p2p_network) = storage_node_clone.p2p_network() {
                let connected_peers = p2p_network.get_connected_peers().await;
                let stats = p2p_network.get_network_stats().await;

                println!("\n📊 Connection Status Report:");
                println!("  Active connections: {}", connected_peers.len());
                println!("  Stored chunks: {}", stats.stored_chunks);
                println!("  Known DIDs: {}", stats.known_dids);

                if !connected_peers.is_empty() {
                    println!("  Connected peers:");
                    for peer in &connected_peers {
                        println!("    - {}", peer);
                    }
                } else {
                    println!("  ⚠️  No active connections");
                    println!("     This may indicate:");
                    println!("     - KeepAlive timeouts");
                    println!("     - Network connectivity issues");
                    println!("     - Peers not discovered yet");
                }
                println!();
            }
        }
    });

    // Keep running
    println!("✅ Connection diagnostics test running...");
    println!("   Press Ctrl+C to stop\n");

    loop {
        sleep(Duration::from_secs(60)).await;
    }
}
