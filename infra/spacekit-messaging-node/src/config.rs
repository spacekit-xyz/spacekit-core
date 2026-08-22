//! Configuration structures for the messaging node

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Configuration for the messaging node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingConfig {
    /// Node's decentralized identifier (DID)
    pub node_did: String,
    /// Node's private key for signing
    pub private_key: String,
    /// Listen address for the node
    pub listen_addr: SocketAddr,
    /// Bootstrap peers to connect to
    pub bootstrap_peers: Vec<String>,
    /// Default quantum algorithm for new channels
    pub default_quantum_algorithm: String,
    /// Default cipher suite for encryption
    pub default_cipher_suite: String,
    /// Maximum number of connections
    pub max_connections: u32,
    /// Message retention period in seconds
    pub message_retention_seconds: u64,
    /// Enable peer discovery
    pub enable_peer_discovery: bool,
    /// Network configuration
    pub network: NetworkConfig,
    /// Storage configuration
    pub storage: StorageConfig,
}

/// Network-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Heartbeat interval in seconds
    pub heartbeat_interval: u64,
    /// Connection timeout in seconds
    pub connection_timeout: u64,
    /// Maximum message size in bytes
    pub max_message_size: u64,
    /// Enable network encryption
    pub enable_encryption: bool,
    /// Custom protocol version
    pub protocol_version: String,
}

/// Storage configuration for messages and state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Storage directory path
    pub storage_path: String,
    /// Enable message persistence
    pub enable_persistence: bool,
    /// Load conversation history on demand instead of at startup
    #[serde(default = "default_lazy_load_history")]
    pub lazy_load_history: bool,
    /// Max conversations to keep decrypted history in RAM (LRU eviction)
    #[serde(default = "default_history_cache_conversations")]
    pub history_cache_conversations: usize,
    /// Persist history in redb (`history.redb`) instead of JSONL files
    #[serde(default = "default_use_redb_history")]
    pub use_redb_history: bool,
    /// Maximum storage size in bytes
    pub max_storage_size: u64,
    /// Cleanup interval in seconds
    pub cleanup_interval: u64,
}

impl Default for MessagingConfig {
    fn default() -> Self {
        Self {
            node_did: "did:swtch:messaging:node".to_string(),
            private_key: String::new(),
            listen_addr: "127.0.0.1:8080".parse().unwrap(),
            bootstrap_peers: vec![],
            default_quantum_algorithm: "Kyber1024".to_string(),
            default_cipher_suite: "AES256".to_string(),
            max_connections: 100,
            message_retention_seconds: 86400 * 7, // 7 days
            enable_peer_discovery: true,
            network: NetworkConfig::default(),
            storage: StorageConfig::default(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            heartbeat_interval: 30,
            connection_timeout: 10,
            max_message_size: 1024 * 1024, // 1MB
            enable_encryption: true,
            protocol_version: "swtch/1.0".to_string(),
        }
    }
}

fn default_lazy_load_history() -> bool {
    true
}

fn default_history_cache_conversations() -> usize {
    64
}

fn default_use_redb_history() -> bool {
    true
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            storage_path: "./data/messaging".to_string(),
            enable_persistence: true,
            lazy_load_history: true,
            history_cache_conversations: 64,
            use_redb_history: true,
            max_storage_size: 1024 * 1024 * 1024, // 1GB
            cleanup_interval: 3600,               // 1 hour
        }
    }
}

impl MessagingConfig {
    /// Load configuration from file
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: MessagingConfig = serde_json::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn to_file(&self, path: &str) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Validate the configuration
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.node_did.is_empty() {
            return Err(anyhow::anyhow!("Node DID cannot be empty"));
        }

        if self.private_key.is_empty() {
            return Err(anyhow::anyhow!("Private key cannot be empty"));
        }

        if self.max_connections == 0 {
            return Err(anyhow::anyhow!("Max connections must be greater than 0"));
        }

        if self.network.max_message_size == 0 {
            return Err(anyhow::anyhow!("Max message size must be greater than 0"));
        }

        Ok(())
    }
}
