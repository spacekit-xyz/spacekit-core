//! Standalone messaging node binary
//!
//! Runs a SpaceKit messaging node as a standalone service

use anyhow::Result;
use clap::{Parser, Subcommand};
use spacekit_messaging_node::{MessagingConfig, MessagingNode};
use spacekit_primitives::v1::crypto::quantum::{generate_kem_keypair, Algorithm};
use tokio::signal;
use tracing::info;
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "spacekit-messaging-node")]
#[command(about = "SpaceKit Network Messaging Node - Quantum-resistant P2P messaging")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the messaging node
    Start {
        /// Configuration file path
        #[arg(short, long, default_value = "messaging-config.json")]
        config: String,
        /// Override listen address
        #[arg(short, long)]
        listen: Option<String>,
        /// Override DID
        #[arg(short, long)]
        did: Option<String>,
        /// Override private key (if not present in config)
        #[arg(long)]
        private_key: Option<String>,
        /// Storage root (default: ./data/messaging)
        #[arg(long)]
        storage_path: Option<String>,
        /// Use redb history.redb instead of JSONL files
        #[arg(long, default_value_t = true)]
        use_redb_history: bool,
        /// Lazy-load conversation history on first access
        #[arg(long, default_value_t = true)]
        lazy_load_history: bool,
        /// Max conversations kept in RAM
        #[arg(long, default_value_t = 64)]
        history_cache_conversations: usize,
    },
    /// Generate a default configuration file
    Config {
        /// Output path for configuration file
        #[arg(short, long, default_value = "messaging-config.json")]
        output: String,
    },
    /// Generate a keypair for the node
    Keypair {
        /// Algorithm to use for key generation
        #[arg(short, long, default_value = "Kyber1024")]
        algorithm: String,
        /// Save keys to files
        #[arg(long)]
        save: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start {
            config,
            listen,
            did,
            private_key,
            storage_path,
            use_redb_history,
            lazy_load_history,
            history_cache_conversations,
        } => {
            start_node(
                config,
                listen,
                did,
                private_key,
                storage_path,
                use_redb_history,
                lazy_load_history,
                history_cache_conversations,
            )
            .await
        }
        Commands::Config { output } => generate_config(output).await,
        Commands::Keypair { algorithm, save } => generate_keypair(algorithm, save).await,
    }
}

/// Start the messaging node
async fn start_node(
    config_path: String,
    listen_override: Option<String>,
    did_override: Option<String>,
    private_key_override: Option<String>,
    storage_path: Option<String>,
    use_redb_history: bool,
    lazy_load_history: bool,
    history_cache_conversations: usize,
) -> Result<()> {
    info!("Starting SpaceKit Messaging Node...");

    // Load configuration
    let mut config = if std::path::Path::new(&config_path).exists() {
        MessagingConfig::from_file(&config_path)?
    } else {
        info!("Configuration file not found, using defaults");
        MessagingConfig::default()
    };

    // Apply overrides
    if let Some(listen_addr) = listen_override {
        config.listen_addr = listen_addr.parse()?;
    }

    if let Some(did) = did_override {
        config.node_did = did;
    }
    if let Some(private_key) = private_key_override {
        config.private_key = private_key;
    }
    if let Some(path) = storage_path {
        config.storage.storage_path = path;
    }
    config.storage.use_redb_history = use_redb_history;
    config.storage.lazy_load_history = lazy_load_history;
    config.storage.history_cache_conversations = history_cache_conversations;

    // Validate configuration
    config.validate()?;

    info!("Node DID: {}", config.node_did);
    info!("Listen address: {}", config.listen_addr);

    // Create and start the messaging node
    let node = MessagingNode::new(config).await?;

    // Start the node
    node.start().await?;

    // Create a test group for demonstration
    let _test_group = node
        .create_group(
            "General".to_string(),
            "did:spacekit:test:admin".to_string(),
            Some("Test group for messaging".to_string()),
        )
        .await?;

    info!("Messaging node is running. Press Ctrl+C to stop.");

    // Wait for shutdown signal
    signal::ctrl_c().await?;

    info!("Shutdown signal received, stopping node...");
    node.stop().await?;

    info!("Messaging node stopped successfully");
    Ok(())
}

/// Generate a default configuration file
async fn generate_config(output_path: String) -> Result<()> {
    info!("Generating default configuration...");

    let config = MessagingConfig::default();
    config.to_file(&output_path)?;

    info!("Configuration saved to: {}", output_path);
    println!("✅ Default configuration generated at: {}", output_path);
    println!("📝 Edit the configuration file to customize your node settings");
    println!(
        "🚀 Start your node with: spacekit-messaging-node start --config {}",
        output_path
    );

    Ok(())
}

/// Generate a keypair for the node
async fn generate_keypair(algorithm: String, save: bool) -> Result<()> {
    info!("Generating keypair with algorithm: {}", algorithm);

    let algorithm_enum = match algorithm.as_str() {
        "Kyber512" => Algorithm::Kyber512,
        "Kyber768" => Algorithm::Kyber768,
        "Kyber1024" => Algorithm::Kyber1024,
        "NtruPrimeSntrup761" => Algorithm::NtruPrimeSntrup761,
        "FrodoKem1344Aes" => Algorithm::FrodoKem1344Aes,
        "FrodoKem1344Shake" => Algorithm::FrodoKem1344Shake,
        "ClassicMcEliece348864" => Algorithm::ClassicMcEliece348864,
        "BikeL1" => Algorithm::BikeL1,
        "BikeL3" => Algorithm::BikeL3,
        "BikeL5" => Algorithm::BikeL5,
        _ => Algorithm::Kyber1024,
    };
    let (public_key_raw, private_key_raw) = generate_kem_keypair(algorithm_enum)?;
    let public_key = hex::encode(public_key_raw);
    let private_key = hex::encode(private_key_raw);

    if save {
        std::fs::write("messaging_public_key.txt", &public_key)?;
        std::fs::write("messaging_private_key.txt", &private_key)?;

        println!("✅ Keypair generated and saved:");
        println!("📄 Public key: messaging_public_key.txt");
        println!("🔐 Private key: messaging_private_key.txt");
    } else {
        println!("✅ Generated keypair:");
        println!("🔑 Public key: {}", public_key);
        println!("🔐 Private key: {}", private_key);
    }

    println!("⚠️  Keep your private key secure and never share it!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_generation() {
        let temp_file = "/tmp/test_messaging_config.json";
        generate_config(temp_file.to_string()).await.unwrap();

        // Verify the file was created and can be loaded
        assert!(std::path::Path::new(temp_file).exists());
        let loaded_config = MessagingConfig::from_file(temp_file).unwrap();
        assert!(!loaded_config.node_did.is_empty());

        // Clean up
        std::fs::remove_file(temp_file).ok();
    }

    #[tokio::test]
    async fn test_keypair_generation() {
        // Test keypair generation without saving
        let result = generate_keypair("Kyber1024".to_string(), false).await;
        assert!(result.is_ok());
    }
}
