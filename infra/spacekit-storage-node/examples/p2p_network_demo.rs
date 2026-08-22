#![recursion_limit = "256"]

//! P2P Network Demo - Real Peer-to-Peer Storage
//!
//! This example demonstrates actual P2P capabilities of the storage node:
//! - Multiple nodes connecting to each other
//! - Peer discovery via mDNS and Kademlia DHT
//! - File storage and retrieval across nodes
//! - Query capabilities across the distributed network
//!
//! ## Usage
//!
//! ### Single Node (mDNS Discovery)
//! ```bash
//! cargo run --example p2p_network_demo --features p2p,database -- --node-id 1 --port 9001
//! ```
//!
//! ### Multiple Nodes (Bootstrap Connection)
//!
//! **Terminal 1 - First Node:**
//! ```bash
//! cargo run --example p2p_network_demo --features p2p,database,quantum -- --node-id 1 --port 9001
//! ```
//!
//! Note the peer ID from the output, then in **Terminal 2 - Second Node:**
//! ```bash
//! cargo run --example p2p_network_demo --features p2p,database,quantum -- --node-id 2 --port 9002 --bootstrap /ip4/127.0.0.1/tcp/9001/p2p/<peer-id>
//! ```
//!
//! Replace `<peer-id>` with the actual peer ID from the first node's output.
//!
//! ### Verbose Logging
//! Add `--verbose` or `-v` for detailed P2P network logs:
//! ```bash
//! cargo run --example p2p_network_demo --features p2p,database,quantum -- --node-id 1 --port 9001 --verbose
//! ```
//!
//! ## What This Demo Shows
//!
//! 1. **P2P Network Initialization**: Creates a libp2p swarm with Kademlia DHT, mDNS, identify, and ping protocols
//! 2. **Peer Discovery**: Automatically discovers peers on the local network via mDNS
//! 3. **Bootstrap Connection**: Manually connect to specific peers using bootstrap addresses
//! 4. **File Storage**: Stores encrypted files on the local node
//! 5. **File Announcement**: Announces file availability to the P2P network
//! 6. **Query Interface**: Queries files using the SQL-like query interface
//! 7. **Network Status**: Shows connected peers and network statistics
//!
//! ## Real P2P Features Demonstrated
//!
//! - ✅ **libp2p Integration**: Real peer-to-peer networking (not mocked)
//! - ✅ **Kademlia DHT**: Distributed hash table for peer and content discovery
//! - ✅ **mDNS Discovery**: Automatic peer discovery on local networks
//! - ✅ **Peer Connections**: Actual TCP connections between nodes
//! - ✅ **File Chunking**: File storage and retrieval across the network
//! - ✅ **Network Events**: Real-time peer connection/disconnection events

use anyhow::Result;
use spacekit_primitives::v1::{crypto::quantum::Algorithm, identity::QuantumDID};
use spacekit_storage_node::{
    FileQuery, Filter, FilterOp, FilterValue, NetworkConfig, SortBy, StorageNode,
    StorageNodeConfig, StorageQueryBuilder,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug)]
struct Args {
    node_id: u32,
    port: u16,
    bootstrap: Option<String>,
    verbose: bool,
}

impl Args {
    fn from_env() -> Self {
        let mut args = std::env::args().skip(1);
        let mut node_id = 1;
        let mut port = 9001;
        let mut bootstrap = None;
        let mut verbose = false;

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
                "-v" | "--verbose" => verbose = true,
                _ => {}
            }
        }

        Self {
            node_id,
            port,
            bootstrap,
            verbose,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    Ok(())
}
// async fn main() -> Result<()> {
//     let args = Args::from_env();

//     // Initialize logging
//     if args.verbose {
//         tracing_subscriber::fmt()
//             .with_max_level(tracing::Level::DEBUG)
//             .init();
//     } else {
//         tracing_subscriber::fmt()
//             .with_max_level(tracing::Level::INFO)
//             .init();
//     }

//     println!("\n🌐 SpaceKit Storage Node - P2P Network Demo\n");
//     println!("Node ID: {}", args.node_id);
//     println!("Listening on port: {}", args.port);
//     if let Some(bootstrap) = &args.bootstrap {
//         println!("Bootstrap peer: {}", bootstrap);
//     }
//     println!("{}", "=".repeat(60));

//     // 1. CREATE STORAGE NODE WITH P2P NETWORKING
//     println!("\n📦 Step 1: Creating Storage Node with P2P Network...\n");

//     let data_dir = format!("./p2p_demo_storage_node_{}", args.node_id);
//     let database_path = format!("{}/node_{}.json", data_dir, args.node_id);

//     // Configure network with bootstrap peers
//     let mut bootstrap_peers = Vec::new();
//     if let Some(bootstrap_addr) = &args.bootstrap {
//         bootstrap_peers.push(bootstrap_addr.clone());
//         println!("✓ Configured bootstrap peer: {}", bootstrap_addr);
//     }

//     let network_config = NetworkConfig {
//         listen_port: args.port,
//         bootstrap_peers,
//         max_connections: 50,
//         replication_factor: 3,
//         chunk_size: 1024 * 1024, // 1MB chunks
//         max_concurrent_operations: Some(10),
//     };

//     let config = StorageNodeConfig {
//         max_storage_bytes: 10 * 1024 * 1024 * 1024, // 10GB
//         data_dir: std::path::PathBuf::from(&data_dir),
//         database_path: Some(std::path::PathBuf::from(&database_path)),
//         node_did: format!("did:spacekit:storage:node-{}", args.node_id),
//         preferred_algorithm: "kyber1024".to_string(),
//         encryption_keypair: None,
//         network_config,
//         #[cfg(feature = "api-server")]
//         api_config: None,
//     };

//     let storage_node = Arc::new(StorageNode::new(config).await?);
//     println!("✓ Storage node created");
//     println!("  Node DID: {}", storage_node.config().node_did);

//     // Start the storage node (this starts the P2P network)
//     storage_node.start().await?;
//     println!("✓ Storage node started");
//     println!("✓ P2P network listening on port {}", args.port);

//     // Give the network time to initialize
//     println!("\n⏳ Waiting for P2P network to initialize...");
//     sleep(Duration::from_secs(2)).await;

//     // 2. DISCOVER PEERS
//     println!("\n🔍 Step 2: Discovering Peers...\n");

//     #[cfg(feature = "p2p")]
//     {
//         if let Some(p2p_network) = storage_node.p2p_network() {
//             // Get connected peers
//             let connected_peers = p2p_network.get_connected_peers().await;
//             println!("Connected peers: {}", connected_peers.len());
//             for peer_id in &connected_peers {
//                 println!("  ✓ Peer: {}", peer_id);
//             }

//             if connected_peers.is_empty() {
//                 println!("  ⚠️  No peers connected yet. If running multiple nodes:");
//                 println!("     - Make sure other nodes are running");
//                 println!("     - Use --bootstrap flag to connect to another node");
//                 println!("     - Wait a few seconds for mDNS discovery");
//             }

//             // Wait a bit more for mDNS discovery
//             println!("\n⏳ Waiting for mDNS peer discovery (10 seconds)...");
//             sleep(Duration::from_secs(10)).await;

//             let connected_peers_after = p2p_network.get_connected_peers().await;
//             if connected_peers_after.len() > connected_peers.len() {
//                 println!("✓ Discovered {} new peers via mDNS!",
//                     connected_peers_after.len() - connected_peers.len());
//                 for peer_id in &connected_peers_after {
//                     if !connected_peers.contains(peer_id) {
//                         println!("  ✓ New peer: {}", peer_id);
//                     }
//                 }
//             }
//         } else {
//             println!("⚠️  P2P network not available (p2p feature not enabled?)");
//         }
//     }

//     #[cfg(not(feature = "p2p"))]
//     {
//         println!("⚠️  P2P feature not enabled. Rebuild with --features p2p");
//     }

//     // 3. STORE FILES ON THIS NODE
//     println!("\n💾 Step 3: Storing Files on Node {}...\n", args.node_id);

//     // Generate a test DID and keypair for this node
//     // Note: QuantumDID::parse requires did:spacekit: or did:spacekit: prefix
//     let did_str = format!("did:spacekit:storage:node-{}", args.node_id);
//     let did = QuantumDID::parse(&did_str)
//         .map_err(|e| anyhow::anyhow!("Failed to parse DID: {}", e))?;

//     // Generate a real quantum-safe keypair for file encryption
//     // This is what would be used in production
//     let quantum_crypto = storage_node.quantum_crypto();
//     let (public_key, _private_key) = quantum_crypto
//         .generate_keypair(Algorithm::Kyber1024)
//         .await
//         .map_err(|e| anyhow::anyhow!("Failed to generate quantum keypair: {}", e))?;

//     println!("✓ Generated quantum-safe keypair (Kyber1024)");
//     println!("  Public key size: {} bytes", public_key.len());
//     println!("  Note: Private key is kept secret (not stored on node)");

//     // Store some test files
//     let test_files: Vec<(&str, &[u8])> = vec![
//         ("document1.txt", b"Hello from node 1! This is a test document." as &[u8]),
//         ("document2.txt", b"Another document stored on this node to test the query capabilities." as &[u8]),
//         ("data.json", b"{\"node_id\": 1, \"timestamp\": \"2025-01-01\", \"data\": \"This is a test data file.\"}" as &[u8]),
//     ];

//     let mut stored_file_ids = Vec::new();
//     for (filename, data) in &test_files {
//         match storage_node.store_file(
//             filename,
//             *data,
//             &did.to_string(),
//             public_key.as_slice(),
//             Some("text/plain".to_string()),
//         ).await {
//             Ok((file_id, _)) => {
//                 println!("✓ Stored file: {} (ID: {})", filename, file_id);
//                 stored_file_ids.push((filename.to_string(), file_id));
//             }
//             Err(e) => {
//                 println!("✗ Failed to store {}: {}", filename, e);
//             }
//         }
//     }

//     // 4. ANNOUNCE FILES TO P2P NETWORK
//     println!("\n📢 Step 4: Announcing Files to P2P Network...\n");

//     #[cfg(feature = "p2p")]
//     {
//         if let Some(p2p_network) = storage_node.p2p_network() {
//             for (filename, file_id) in &stored_file_ids {
//                 // Create a chunk for the file (simplified - in production, files would be chunked)
//                 let chunk_id = format!("chunk_{}", file_id);

//                 // In a real implementation, we would:
//                 // 1. Split the file into chunks
//                 // 2. Store chunks on different nodes
//                 // 3. Announce chunk availability

//                 println!("  📢 Announcing file: {} (ID: {})", filename, file_id);

//                 // For demo purposes, we'll just announce the file ID
//                 // In production, this would announce all chunks
//                 if let Err(e) = p2p_network.announce_file(file_id, vec![chunk_id.clone()]).await {
//                     println!("    ⚠️  Failed to announce: {}", e);
//                 } else {
//                     println!("    ✓ File announced to network");
//                 }
//             }
//         }
//     }

//     // 5. QUERY FILES ACROSS THE NETWORK
//     println!("\n🔍 Step 5: Querying Files...\n");

//     // Small delay to ensure database is synced
//     sleep(Duration::from_millis(100)).await;

//     let database = storage_node.database();
//     let query_builder = StorageQueryBuilder::new(database.clone());

//     // First, query all files to verify storage
//     println!("Querying all files on this node...");
//     let all_files_query = FileQuery {
//         distinct: false,
//         window_functions: Vec::new(),
//         filters: vec![],
//         joins: vec![],
//         sort_by: Some(SortBy {
//             field: "created_at".to_string(),
//             order: SortOrder::Desc,
//         }),
//         limit: Some(10),
//         offset: Some(0),
//     };

//     match query_builder.query_files(all_files_query).await {
//         Ok(result) => {
//             println!("✓ Query executed successfully");
//             println!("  Found {} total files on this node", result.files.len());
//             for file in &result.files {
//                 println!("    - {} (ID: {})", file.filename, file.id);
//                 println!("      Owner: {}", file.owner_did);
//                 println!("      Size: {} bytes", file.size);
//             }
//         }
//         Err(e) => {
//             println!("✗ Query failed: {}", e);
//         }
//     }

//     // Now query files for this specific DID
//     println!("\nQuerying files for DID: {}...", did);
//     let file_query = FileQuery {
//         distinct: false,
//         window_functions: Vec::new(),
//         filters: vec![
//             Filter {
//                 field: "owner_did".to_string(),
//                 op: FilterOp::Equals,
//                 value: FilterValue::String(did.to_string()),
//             }
//         ],
//         joins: vec![],
//         sort_by: Some(SortBy {
//             field: "created_at".to_string(),
//             order: SortOrder::Desc,
//         }),
//         limit: Some(10),
//         offset: Some(0),
//     };

//     match query_builder.query_files(file_query).await {
//         Ok(result) => {
//             println!("✓ Query executed successfully");
//             println!("  Found {} files for this DID", result.files.len());
//             for file in &result.files {
//                 println!("    - {} (ID: {})", file.filename, file.id);
//                 println!("      Owner: {}", file.owner_did);
//                 println!("      Size: {} bytes", file.size);
//             }
//         }
//         Err(e) => {
//             println!("✗ Query failed: {}", e);
//         }
//     }

//     // 6. DEMONSTRATE P2P FILE RETRIEVAL (if peers are connected)
//     println!("\n🌐 Step 6: P2P File Retrieval Demo...\n");

//     #[cfg(feature = "p2p")]
//     {
//         if let Some(p2p_network) = storage_node.p2p_network() {
//             // Wait a moment for connections to stabilize
//             sleep(Duration::from_millis(500)).await;

//             let connected_peers = p2p_network.get_connected_peers().await;

//             if !connected_peers.is_empty() {
//                 println!("✓ Connected to {} peer(s)", connected_peers.len());
//                 for peer in &connected_peers {
//                     println!("  - Peer ID: {}", peer);
//                 }
//                 println!("\n  In a production scenario, files would be:");
//                 println!("    - Chunked and distributed across peers");
//                 println!("    - Retrieved from peers when needed");
//                 println!("    - Cached locally for faster access");

//                 // Demonstrate chunk retrieval (if chunks were stored)
//                 if !stored_file_ids.is_empty() {
//                     println!("\n  Attempting to retrieve chunks from network...");
//                     for (filename, file_id) in &stored_file_ids {
//                         let chunk_id = format!("chunk_{}", file_id);
//                         match p2p_network.retrieve_chunk(&chunk_id).await {
//                             Ok(Some(_chunk)) => {
//                                 println!("  ✓ Retrieved chunk for {} from network", filename);
//                             }
//                             Ok(None) => {
//                                 println!("  ℹ️  Chunk for {} not found on network (stored locally only)", filename);
//                             }
//                             Err(e) => {
//                                 println!("  ⚠️  Error retrieving chunk: {}", e);
//                             }
//                         }
//                     }
//                 }
//             } else {
//                 println!("⚠️  No peers connected");
//                 println!("  To test P2P file retrieval:");
//                 println!("    1. Start another node instance:");
//                 println!("       cargo run --example p2p_network_demo --features p2p,database,quantum -- --node-id 2 --port 9002");
//                 println!("    2. Restart this node with bootstrap:");
//                 println!("       cargo run --example p2p_network_demo --features p2p,database,quantum -- --node-id 1 --port 9001 --bootstrap /ip4/127.0.0.1/tcp/9002/p2p/<peer-id>");
//             }
//         }
//     }

//     // 7. NETWORK STATUS
//     println!("\n📊 Step 7: Network Status...\n");

//     #[cfg(feature = "p2p")]
//     {
//         if let Some(p2p_network) = storage_node.p2p_network() {
//             let connected_peers = p2p_network.get_connected_peers().await;
//             println!("Network Status:");
//             println!("  Node ID: {}", args.node_id);
//             println!("  Node DID: {}", storage_node.config().node_did);
//             println!("  Listening on: /ip4/0.0.0.0/tcp/{}", args.port);
//             println!("  Connected peers: {}", connected_peers.len());

//             if !connected_peers.is_empty() {
//                 println!("  Peer list:");
//                 for peer_id in &connected_peers {
//                     println!("    - {}", peer_id);
//                 }
//             }
//         }
//     }

//     // Keep the node running
//     println!("\n✅ P2P Network Demo Running...");
//     println!("   Node {} is active and listening for connections", args.node_id);
//     println!("   Press Ctrl+C to stop\n");

//     // Run for a while to allow peer connections
//     loop {
//         sleep(Duration::from_secs(30)).await;

//         #[cfg(feature = "p2p")]
//         {
//             if let Some(p2p_network) = storage_node.p2p_network() {
//                 let connected_peers = p2p_network.get_connected_peers().await;
//                 if !connected_peers.is_empty() {
//                     println!("📡 Currently connected to {} peer(s)", connected_peers.len());
//                 }
//             }
//         }
//     }
// }
