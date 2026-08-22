//! Standalone SpaceKit Storage Node Binary
//!

#![recursion_limit = "512"]

use clap::{Parser, Subcommand};
use spacekit_storage_node::{ServerConfig, StorageNode, StorageNodeConfig};
use std::path::PathBuf;
use tokio::signal;
use tracing::{error, info};

#[derive(Parser)]
#[command(name = "spacekit-storage-node")]
#[command(about = "SpaceKit Network Storage Node - Quantum-resistant distributed storage")]
#[command(version = "1.0.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the storage node
    Start {
        /// Configuration file path
        #[arg(short, long)]
        config: Option<PathBuf>,

        /// Node DID (Decentralized Identifier)
        #[arg(long)]
        did: Option<String>,

        /// Data directory
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Maximum storage in GB
        #[arg(long, default_value = "100")]
        max_storage_gb: u64,

        /// Preferred quantum algorithm
        #[arg(long, default_value = "kyber1024")]
        algorithm: String,

        /// API server port
        #[arg(long, default_value = "3030")]
        port: u16,

        /// Public key for encryption (hex format)
        #[arg(long)]
        public_key: Option<String>,

        /// Enable API server
        #[arg(long, default_value = "true")]
        enable_api: bool,

        /// Libp2p TCP listen port (ignored when --disable-p2p).
        #[arg(long, default_value = "4001")]
        p2p_port: u16,

        /// Do not start libp2p (HTTP API only; no TCP 4001 listeners).
        #[arg(long, action = clap::ArgAction::SetTrue)]
        disable_p2p: bool,

        /// Externalize document payloads > inline threshold into redb (default: true).
        #[arg(long, default_value_t = true)]
        externalize_documents: bool,

        /// Keep document JSON inline when serialized size is at or below this many bytes.
        #[arg(long, default_value_t = 4096)]
        document_inline_max_bytes: usize,

        /// Max bytes in the redb blob read cache (default 32 MiB).
        #[arg(long, default_value_t = 32 * 1024 * 1024)]
        blob_cache_max_bytes: u64,
    },

    /// Run one-time migrations (JSON documents → meta.redb, docstore → blobs.redb).
    Migrate {
        #[arg(short, long, default_value = "./storage_data")]
        data_dir: PathBuf,
    },

    /// Stream-ingest large file(s) as CAS blobs under data_dir/blobs (for videos etc.).
    Ingest {
        /// File or directory to ingest (e.g. /Users/astor/Projects/2024/video)
        #[arg(short, long)]
        path: PathBuf,
        #[arg(short, long, default_value = "./storage_data")]
        data_dir: PathBuf,
    },

    /// Generate a new quantum-resistant keypair
    GenerateKeys {
        /// Output directory for keys
        #[arg(short, long, default_value = "./keys")]
        output: PathBuf,

        /// Quantum algorithm to use
        #[arg(short, long, default_value = "kyber1024")]
        algorithm: String,
    },

    /// Show storage node status
    Status {
        /// API server URL
        #[arg(long, default_value = "http://localhost:3030")]
        url: String,
    },

    /// Run the in-process MCP server (Phase 5).
    ///
    /// Reads JSON-RPC 2.0 requests from stdin and writes responses to stdout
    /// — the standard MCP stdio transport. Wires the agentic Facade so
    /// `tx_*`, `sandbox_*`, and `graph_traverse` tools are callable. Use
    /// `--data-dir` to point at an existing storage node's data directory.
    Mcp {
        /// Data directory for the underlying storage node.
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Node DID (Decentralized Identifier).
        #[arg(long)]
        did: Option<String>,

        /// Enable real transactional apply (default on; use --enable-real-transactions=false to opt out).
        #[arg(long, default_value_t = true)]
        enable_real_transactions: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            config,
            did,
            data_dir,
            max_storage_gb,
            algorithm,
            port,
            public_key,
            enable_api,
            p2p_port,
            disable_p2p,
            externalize_documents,
            document_inline_max_bytes,
            blob_cache_max_bytes,
        } => {
            start_node(
                config,
                did,
                data_dir,
                max_storage_gb,
                algorithm,
                port,
                public_key,
                enable_api,
                p2p_port,
                disable_p2p,
                externalize_documents,
                document_inline_max_bytes,
                blob_cache_max_bytes,
            )
            .await?;
        }
        Commands::GenerateKeys { output, algorithm } => {
            generate_keys(&output, &algorithm).await?;
        }
        Commands::Status { url } => {
            show_status(&url).await?;
        }
        Commands::Mcp {
            data_dir,
            did,
            enable_real_transactions,
        } => {
            run_mcp(data_dir, did, enable_real_transactions).await?;
        }
        Commands::Migrate { data_dir } => {
            run_migrate(&data_dir)?;
        }
        Commands::Ingest { path, data_dir } => {
            run_ingest(&path, &data_dir)?;
        }
    }

    Ok(())
}

async fn run_mcp(
    data_dir: Option<PathBuf>,
    did: Option<String>,
    enable_real_transactions: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting SpaceKit Storage Node MCP server (stdio transport)…");

    let mut config = StorageNodeConfig::default();
    if let Some(d) = did {
        config.node_did = d;
    } else {
        config.node_did = format!("did:spacekit:mcp:{}", uuid::Uuid::new_v4());
    }
    if let Some(dir) = data_dir {
        config.data_dir = dir;
    }
    config.enable_p2p = false;
    config.api_config = None;
    config.enable_real_transactions = enable_real_transactions;

    let node = StorageNode::new(config).await?;
    let database = node.database();
    let real_apply = spacekit_storage_node::storage_facade::resolve_enable_real_transactions(
        enable_real_transactions,
    );
    let facade_cfg = spacekit_storage_node::storage_facade::FacadeConfig {
        enable_real_transactions: real_apply,
        sandbox_persistence_root: Some(node.config().data_dir.join("sandboxes")),
        ..Default::default()
    };
    let facade = std::sync::Arc::new(
        spacekit_storage_node::storage_facade::Facade::new(database, facade_cfg).await?,
    );
    let server = spacekit_storage_node::mcp::McpServer::new(facade);
    spacekit_storage_node::mcp::run_stdio(server).await?;
    Ok(())
}

async fn start_node(
    _config_path: Option<PathBuf>,
    did: Option<String>,
    data_dir: Option<PathBuf>,
    max_storage_gb: u64,
    algorithm: String,
    port: u16,
    public_key: Option<String>,
    enable_api: bool,
    p2p_port: u16,
    disable_p2p: bool,
    externalize_documents: bool,
    document_inline_max_bytes: usize,
    blob_cache_max_bytes: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Starting SpaceKit Storage Node...");

    // Create configuration
    let mut config = StorageNodeConfig::default();

    if let Some(did) = did {
        config.node_did = did;
    } else {
        config.node_did = format!("did:spacekit:storage:{}", uuid::Uuid::new_v4());
    }

    if let Some(data_dir) = data_dir {
        config.data_dir = data_dir;
    }

    config.max_storage_bytes = max_storage_gb * 1024 * 1024 * 1024;
    config.preferred_algorithm = algorithm;
    config.network_config.listen_port = p2p_port;
    config.enable_p2p = !disable_p2p;
    config.persistence.externalize_documents = externalize_documents;
    config.persistence.document_inline_max_bytes = document_inline_max_bytes;
    config.persistence.blob_cache_max_bytes = blob_cache_max_bytes;

    // Configure API server
    #[cfg(feature = "api-server")]
    if enable_api {
        let mut api = ServerConfig::default();
        api.port = port;
        api.public_key = public_key.unwrap_or_default();
        api.enable_cors = true;
        config.api_config = Some(api);
    }

    // Create and start the storage node
    let node = StorageNode::new(config).await?;

    info!("Node DID: {}", node.config().node_did);
    info!("Data directory: {:?}", node.config().data_dir);
    info!("Max storage: {} GB", max_storage_gb);
    info!("Quantum algorithm: {}", node.config().preferred_algorithm);

    #[cfg(feature = "api-server")]
    if enable_api {
        info!("API server will start on port {}", port);
    }
    #[cfg(feature = "p2p")]
    if node.config().enable_p2p {
        info!("Libp2p will listen on TCP port {}", p2p_port);
    }

    // Start the node
    node.start().await?;

    // Wait for shutdown signal
    info!("Storage node is running. Press Ctrl+C to stop...");
    signal::ctrl_c().await?;

    info!("Shutting down storage node...");
    Ok(())
}

async fn generate_keys(
    output_dir: &PathBuf,
    algorithm: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    info!(
        "Generating quantum-resistant keypair with algorithm: {}",
        algorithm
    );

    // Create output directory
    tokio::fs::create_dir_all(output_dir).await?;

    // Parse algorithm string to Algorithm enum
    let algorithm_enum = match algorithm.to_lowercase().as_str() {
        "kyber512" => spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber512,
        "kyber768" => spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber768,
        "kyber1024" => spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024,
        "ntru" => spacekit_primitives::v1::crypto::quantum::Algorithm::NtruPrimeSntrup761,
        "frodokem1344aes" => spacekit_primitives::v1::crypto::quantum::Algorithm::FrodoKem1344Aes,
        "frodokem1344shake" => {
            spacekit_primitives::v1::crypto::quantum::Algorithm::FrodoKem1344Shake
        }
        "classicmceliece348864" => {
            spacekit_primitives::v1::crypto::quantum::Algorithm::ClassicMcEliece348864
        }
        "bikel1" => spacekit_primitives::v1::crypto::quantum::Algorithm::BikeL1,
        "bikel3" => spacekit_primitives::v1::crypto::quantum::Algorithm::BikeL3,
        "bikel5" => spacekit_primitives::v1::crypto::quantum::Algorithm::BikeL5,
        _ => {
            error!(
                "Unsupported algorithm: {}. Using default Kyber1024",
                algorithm
            );
            spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024
        }
    };

    // Create quantum crypto service
    let quantum_crypto = spacekit_storage_node::QuantumCrypto::new(
        algorithm_enum.clone(),
        spacekit_primitives::v1::crypto::quantum::CipherSuite::AES256,
    );

    // Generate actual quantum-resistant keypair
    let (public_key_bytes, private_key_bytes) =
        quantum_crypto.generate_keypair(algorithm_enum).await?;

    // Convert to hex strings
    let public_key = hex::encode(&public_key_bytes);
    let private_key = hex::encode(&private_key_bytes);

    let public_key_path = output_dir.join("public_key.hex");
    let private_key_path = output_dir.join("private_key.hex");

    tokio::fs::write(&public_key_path, &public_key).await?;
    tokio::fs::write(&private_key_path, &private_key).await?;

    info!("Keys generated:");
    info!("  Public key: {:?}", public_key_path);
    info!("  Private key: {:?}", private_key_path);
    info!("  Algorithm: {}", algorithm);
    info!("  Public key size: {} bytes", public_key_bytes.len());
    info!("  Private key size: {} bytes", private_key_bytes.len());

    println!("✅ Quantum-resistant keypair generated successfully!");
    println!("📂 Output directory: {:?}", output_dir);
    println!("🔑 Public key file: {:?}", public_key_path);
    println!("🔐 Private key file: {:?}", private_key_path);
    println!("⚡ Algorithm: {}", algorithm);
    println!(
        "📏 Key sizes: {} bytes (public), {} bytes (private)",
        public_key_bytes.len(),
        private_key_bytes.len()
    );

    Ok(())
}

fn run_migrate(data_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    info!("Running storage migrations in {:?}", data_dir);
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir.join("spacekit_storage.json");
    let db = spacekit_storage_node::database::Database::with_config(
        db_path.to_str().unwrap(),
        spacekit_storage_node::database::PersistenceConfig::default(),
    )?;
    db.initialize()?;
    println!(
        "✅ Migration complete — meta.redb + blobs.redb in {:?}",
        data_dir
    );
    Ok(())
}

fn run_ingest(source: &PathBuf, data_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(data_dir)?;
    let files: Vec<PathBuf> = if source.is_dir() {
        let mut out = Vec::new();
        let mut stack = vec![source.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
        out
    } else {
        vec![source.clone()]
    };
    if files.is_empty() {
        println!("No files found under {:?}", source);
        return Ok(());
    }
    let reports =
        spacekit_storage_node::storage_migration::ingest_files_as_cas_blobs(data_dir, &files)?;
    for (file, report) in files.iter().zip(reports.iter()) {
        let action = if report.cache_hit {
            "unchanged (cache hit)"
        } else if report.skipped {
            "deduplicated"
        } else {
            "ingested"
        };
        println!(
            "{} {} ({} bytes) → blake3:{}",
            action,
            file.display(),
            report.bytes,
            report.hash
        );
    }
    Ok(())
}

async fn show_status(url: &str) -> Result<(), Box<dyn std::error::Error>> {
    info!("Checking storage node status at: {}", url);

    // TODO: Implement actual status check via HTTP API
    let response = reqwest::get(url).await?;
    let body = response.text().await?;
    println!("Body: {}", body);
    // For now, just show a placeholder

    println!("🔍 SpaceKit Storage Node Status");
    println!("📡 URL: {}", url);
    println!("❌ Status: Not implemented yet");
    println!("💡 Use the API endpoints directly for now:");
    println!("   GET {}/did - Get node DID", url);
    println!("   GET {}/service/all_users - List users", url);
    println!("   GET {}/service/all_messages - List messages", url);

    println!("Body: {}", body);

    Ok(())
}
