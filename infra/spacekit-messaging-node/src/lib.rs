//! SWTCH Messaging Node Library
//!
//! Provides quantum-resistant P2P messaging services that can be embedded
//! in applications or run as standalone infrastructure nodes.

use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{broadcast, RwLock};
use tracing::info;

// Re-export modules
pub mod access_control; // Access control and permissions
pub mod compression; // Message compression
pub mod config;
pub mod encryption;
pub mod gateway;
pub mod handlers;
pub mod history_store;
pub mod models;
pub mod network;
pub mod network_p2p; // New P2P implementation

// Re-export key types from models
pub use access_control::{
    AccessControlManager, AccessPolicies, AccessStats, Action, NodeType, UserPermissions, UserRole,
    Violation, ViolationSeverity, ViolationType,
};
pub use config::MessagingConfig;
pub use encryption::{EncryptedPayload, GroupEncryptionContext, KeyPair, MessageEncryption};
pub use handlers::{
    ConversationInfo, DirectMessageRequest, DirectMessageType, MessageEvent, MessageHandler,
};
pub use models::{
    ConversationType, DirectConversation, DirectoryEntry, EncryptedMessage, EncryptionConfig,
    FileAccess, FileAccessType, FileStatus, Group, GroupFile, GroupInvitation, GroupMembership,
    InvitationStatus, MemberRole, Message, MessageType, SharedFile, User,
};
pub use network::MessagingNetwork;

// Legacy export for backward compatibility
pub use handlers::GroupMessageHandler;

use spacekit_primitives::v1::crypto::quantum::Algorithm;
use std::collections::HashMap;

use crate::network_p2p::{P2PCommand, P2PMessage, P2PNetworkEvent};
use libp2p::Multiaddr;
use tokio::time::Duration;

/// Core messaging node that provides quantum-resistant group and direct messaging
#[derive(Clone)]
pub struct MessagingNode {
    /// Node configuration
    config: MessagingConfig,
    /// Message handler
    message_handler: Arc<MessageHandler>,
    /// Networking layer
    network: Arc<tokio::sync::Mutex<MessagingNetwork>>,
    /// Message event broadcaster
    message_tx: broadcast::Sender<MessageEvent>,
    /// Directory lookup response broadcaster
    directory_tx: broadcast::Sender<P2PMessage>,
    /// Browser/API envelopes received from other messaging processes
    gateway_tx: broadcast::Sender<serde_json::Value>,
    /// Node status
    status: Arc<RwLock<NodeStatus>>,
    /// Access control manager
    access_control: Arc<access_control::AccessControlManager>,
}

/// Node operational status
#[derive(Debug, Clone)]
pub struct NodeStatus {
    /// Whether the node is currently running
    pub is_running: bool,
    /// Number of active connections
    pub active_connections: u32,
    /// Number of active groups
    pub active_groups: u32,
    /// Number of active direct conversations
    pub active_direct_conversations: u32,
    /// Number of registered users
    pub registered_users: u32,
    /// Messages sent today
    pub messages_sent_today: u64,
    /// Messages received today
    pub messages_received_today: u64,
    /// Direct messages sent today
    pub direct_messages_sent_today: u64,
    /// Direct messages received today
    pub direct_messages_received_today: u64,
    /// Node start time
    pub started_at: chrono::DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: chrono::DateTime<Utc>,
}

/// Messaging node errors
#[derive(Debug, Error)]
pub enum MessagingError {
    #[error("Network error: {0}")]
    Network(#[from] network::NetworkError),
    #[error("Encryption error: {0}")]
    Encryption(String),
    #[error("Group not found: {0}")]
    GroupNotFound(String),
    #[error("User not found: {0}")]
    UserNotFound(String),
    #[error("Conversation not found: {0}")]
    ConversationNotFound(String),
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    #[error("Configuration error: {0}")]
    Configuration(String),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl MessagingNode {
    /// Create a new messaging node with the given configuration
    pub async fn new(config: MessagingConfig) -> Result<Self> {
        let history_dir = std::path::PathBuf::from(&config.storage.storage_path).join("history");
        let history_store = if config.storage.use_redb_history {
            let store = crate::history_store::HistoryStore::open(
                &std::path::Path::new(&config.storage.storage_path).join("history.redb"),
                crate::history_store::HistoryStoreConfig {
                    cache_conversations: config.storage.history_cache_conversations as u64,
                    ..Default::default()
                },
            )?;
            if store.list_conversation_ids()?.is_empty() {
                let migrated = store.migrate_jsonl_dir(&history_dir)?;
                if migrated > 0 {
                    info!(
                        "Migrated {} JSONL history entries into history.redb",
                        migrated
                    );
                }
            }
            Some(store)
        } else {
            None
        };
        let history_writer = history_store
            .as_ref()
            .map(|store| store.spawn_batched_writer());
        let message_handler = Arc::new(MessageHandler::new(
            config.node_did.clone(),
            history_dir,
            config.storage.enable_persistence,
            config.storage.lazy_load_history,
            config.storage.history_cache_conversations,
            config.storage.use_redb_history,
            history_store,
            history_writer,
        ));
        let network = Arc::new(tokio::sync::Mutex::new(
            MessagingNetwork::new(&config).await?,
        ));
        let (message_tx, _) = broadcast::channel(1000);
        let (directory_tx, _) = broadcast::channel(500);
        let (gateway_tx, _) = broadcast::channel(1000);

        // Initialize with public node by default (can be changed)
        let access_control = Arc::new(access_control::AccessControlManager::new(NodeType::Public));

        let status = Arc::new(RwLock::new(NodeStatus {
            is_running: false,
            active_connections: 0,
            active_groups: 0,
            active_direct_conversations: 0,
            registered_users: 0,
            messages_sent_today: 0,
            messages_received_today: 0,
            direct_messages_sent_today: 0,
            direct_messages_received_today: 0,
            started_at: Utc::now(),
            last_activity: chrono::DateTime::<Utc>::default(),
        }));

        Ok(Self {
            config,
            message_handler,
            network,
            message_tx,
            directory_tx,
            gateway_tx,
            status,
            access_control,
        })
    }

    /// Create a new messaging node with specific node type
    pub async fn new_with_type(config: MessagingConfig, node_type: NodeType) -> Result<Self> {
        let history_dir = std::path::PathBuf::from(&config.storage.storage_path).join("history");
        let history_store = if config.storage.use_redb_history {
            let store = crate::history_store::HistoryStore::open(
                &std::path::Path::new(&config.storage.storage_path).join("history.redb"),
                crate::history_store::HistoryStoreConfig {
                    cache_conversations: config.storage.history_cache_conversations as u64,
                    ..Default::default()
                },
            )?;
            if store.list_conversation_ids()?.is_empty() {
                let migrated = store.migrate_jsonl_dir(&history_dir)?;
                if migrated > 0 {
                    info!(
                        "Migrated {} JSONL history entries into history.redb",
                        migrated
                    );
                }
            }
            Some(store)
        } else {
            None
        };
        let history_writer = history_store
            .as_ref()
            .map(|store| store.spawn_batched_writer());
        let message_handler = Arc::new(MessageHandler::new(
            config.node_did.clone(),
            history_dir,
            config.storage.enable_persistence,
            config.storage.lazy_load_history,
            config.storage.history_cache_conversations,
            config.storage.use_redb_history,
            history_store,
            history_writer,
        ));
        let network = Arc::new(tokio::sync::Mutex::new(
            MessagingNetwork::new(&config).await?,
        ));
        let (message_tx, _) = broadcast::channel(1000);
        let (directory_tx, _) = broadcast::channel(500);
        let (gateway_tx, _) = broadcast::channel(1000);
        let access_control = Arc::new(access_control::AccessControlManager::new(node_type));

        let status = Arc::new(RwLock::new(NodeStatus {
            is_running: false,
            active_connections: 0,
            active_groups: 0,
            active_direct_conversations: 0,
            registered_users: 0,
            messages_sent_today: 0,
            messages_received_today: 0,
            direct_messages_sent_today: 0,
            direct_messages_received_today: 0,
            started_at: Utc::now(),
            last_activity: chrono::DateTime::<Utc>::default(),
        }));

        Ok(Self {
            config,
            message_handler,
            network,
            message_tx,
            directory_tx,
            gateway_tx,
            status,
            access_control,
        })
    }

    /// Get access control manager
    pub fn access_control(&self) -> &Arc<access_control::AccessControlManager> {
        &self.access_control
    }

    /// Start the messaging node
    pub async fn start(&self) -> Result<()> {
        {
            let mut status = self.status.write().await;
            status.is_running = true;
            status.started_at = Utc::now();
        }

        // Start the network layer
        {
            let mut network = self.network.lock().await;
            network.start().await?;
        }

        self.message_handler.load_message_history().await?;
        self.start_directory_service().await?;

        tracing::info!("Messaging node started with DID: {}", self.config.node_did);
        Ok(())
    }

    /// Stop the messaging node
    pub async fn stop(&self) -> Result<()> {
        {
            let mut status = self.status.write().await;
            status.is_running = false;
        }

        // Stop the network layer
        {
            let mut network = self.network.lock().await;
            network.stop().await?;
        }

        tracing::info!("Messaging node stopped");
        Ok(())
    }

    /// Register a user in the messaging system
    pub async fn register_user(
        &self,
        did: String,
        username: String,
        public_key: Vec<u8>,
        algorithm: Algorithm,
    ) -> Result<User> {
        // Check if user is banned before allowing registration
        if self.access_control.is_blacklisted(&did).await {
            return Err(anyhow::anyhow!("Cannot register: User is banned").into());
        }

        // For private nodes, check whitelist
        if !self.access_control.has_access(&did).await? {
            return Err(anyhow::anyhow!("Cannot register: Access denied (not whitelisted)").into());
        }

        let user = self
            .message_handler
            .register_user(did.clone(), username, public_key, algorithm)
            .await?;

        // Grant default permissions
        let _ = self
            .access_control
            .grant_permissions(did.clone(), UserRole::Member, None)
            .await;

        // Initialize reputation
        let _ = self.access_control.get_reputation_profile(&did).await?;

        // Update status
        {
            let mut status = self.status.write().await;
            status.registered_users += 1;
            status.last_activity = Utc::now();
        }

        Ok(user)
    }

    /// Get user by DID
    pub async fn get_user_by_did(&self, did: &str) -> Result<Option<User>> {
        self.message_handler.get_user_by_did(did).await
    }

    /// Get encryption service (for P2P encryption)
    pub fn get_encryption_service(&self) -> &encryption::MessageEncryption {
        &self.message_handler.encryption
    }

    /// Get group memberships
    pub async fn get_group_memberships(&self, group_id: &str) -> Result<Vec<GroupMembership>> {
        self.message_handler.get_group_memberships(group_id).await
    }

    /// Get user by ID
    pub async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        self.message_handler.get_user_by_id(user_id).await
    }

    /// Get all registered users
    pub async fn get_all_users(&self) -> Result<Vec<User>> {
        self.message_handler.get_all_users().await
    }

    /// Upload a file to a direct conversation (encrypted for the recipient)
    pub async fn upload_direct_file(
        &self,
        recipient_did: String,
        uploader_id: String,
        filename: String,
        file_data: Vec<u8>,
        _mime_type: String,
    ) -> Result<SharedFile> {
        let recipient = self
            .message_handler
            .get_user_by_did(&recipient_did)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Recipient not found: {}", recipient_did))?;
        let conversation = self
            .message_handler
            .create_or_get_direct_conversation(uploader_id.clone(), recipient.id.clone())
            .await?;

        let shared_file = self
            .message_handler
            .upload_direct_file(
                conversation.id.clone(),
                uploader_id,
                recipient.id,
                filename,
                file_data,
                &self.config.storage.storage_path,
            )
            .await?;

        Ok(shared_file)
    }

    /// Upload a file to a group (encrypted per member, stored as a single bundle)
    pub async fn upload_group_file(
        &self,
        group_id: String,
        uploader_id: String,
        filename: String,
        file_data: Vec<u8>,
        _mime_type: String,
    ) -> Result<SharedFile> {
        self.message_handler
            .upload_group_file(
                group_id,
                uploader_id,
                filename,
                file_data,
                &self.config.storage.storage_path,
            )
            .await
    }

    /// Get all groups a user is a member of
    pub async fn get_groups_for_user(&self, user_id: &str) -> Result<Vec<Group>> {
        self.message_handler.get_groups_for_user(user_id).await
    }

    /// Create a new group
    pub async fn create_group(
        &self,
        name: String,
        creator_id: String,
        description: Option<String>,
    ) -> Result<Group> {
        let group = self
            .message_handler
            .create_group(name, creator_id, description)
            .await?;

        // Update status
        {
            let mut status = self.status.write().await;
            status.active_groups += 1;
            status.last_activity = Utc::now();
        }

        Ok(group)
    }

    /// Send a text message to a group
    pub async fn send_text_message(
        &self,
        group_id: String,
        sender_id: String,
        content: String,
    ) -> Result<Vec<MessageEvent>> {
        // Get sender's DID for reputation update
        let sender_did = {
            let user = self
                .message_handler
                .get_user_by_id(&sender_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Sender not found"))?;
            user.did.clone()
        };

        // Check access control before sending
        if !self.access_control.has_access(&sender_did).await? {
            return Err(
                anyhow::anyhow!("Access denied: User banned or insufficient permissions").into(),
            );
        }

        if !self
            .access_control
            .can_perform(&sender_did, access_control::Action::SendMessage)
            .await?
        {
            return Err(anyhow::anyhow!("Permission denied: Cannot send messages").into());
        }

        // Send message
        let events = self
            .message_handler
            .send_text_message(group_id, sender_id, content)
            .await?;

        // Update status
        {
            let mut status = self.status.write().await;
            status.messages_sent_today += 1;
            status.last_activity = Utc::now();
        }

        // Update reputation (positive - message sent successfully)
        let _ = self.access_control.record_message_sent(&sender_did).await;

        // Broadcast events
        for event in &events {
            let _ = self.message_tx.send(event.clone());
        }

        Ok(events)
    }

    /// Send a direct message to another user
    pub async fn send_direct_message(
        &self,
        sender_id: String,
        recipient_did: String,
        content: String,
    ) -> Result<Vec<MessageEvent>> {
        // Get sender's DID for reputation update
        let sender_did = {
            let user = self
                .message_handler
                .get_user_by_id(&sender_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Sender not found"))?;
            user.did.clone()
        };

        // Check access control before sending
        if !self.access_control.has_access(&sender_did).await? {
            return Err(
                anyhow::anyhow!("Access denied: User banned or insufficient permissions").into(),
            );
        }

        if !self
            .access_control
            .can_perform(&sender_did, access_control::Action::SendMessage)
            .await?
        {
            return Err(anyhow::anyhow!("Permission denied: Cannot send messages").into());
        }

        // Send message
        let events = self
            .message_handler
            .send_direct_message(sender_id, recipient_did.clone(), content)
            .await?;

        // Update status
        {
            let mut status = self.status.write().await;
            status.direct_messages_sent_today += 1;
            status.last_activity = Utc::now();
        }

        // Update reputation (positive - message sent successfully)
        let _ = self.access_control.record_message_sent(&sender_did).await;

        // Broadcast events
        for event in &events {
            let _ = self.message_tx.send(event.clone());
        }

        Ok(events)
    }

    /// Create or get existing direct conversation between two users
    pub async fn create_or_get_direct_conversation(
        &self,
        user_a_id: String,
        user_b_id: String,
    ) -> Result<DirectConversation> {
        let conversation = self
            .message_handler
            .create_or_get_direct_conversation(user_a_id, user_b_id)
            .await?;

        // Update status if this is a new conversation
        {
            let mut status = self.status.write().await;
            status.active_direct_conversations += 1;
            status.last_activity = Utc::now();
        }

        Ok(conversation)
    }

    /// Get direct conversations for a user
    pub async fn get_user_direct_conversations(
        &self,
        user_id: &str,
    ) -> Result<Vec<DirectConversation>> {
        self.message_handler
            .get_user_direct_conversations(user_id)
            .await
    }

    /// Get encrypted messages for a user
    pub async fn get_user_encrypted_messages(
        &self,
        user_id: &str,
    ) -> Result<Vec<EncryptedMessage>> {
        self.message_handler
            .get_user_encrypted_messages(user_id)
            .await
    }

    /// Mark messages as read
    pub async fn mark_messages_read(&self, user_id: &str, message_ids: Vec<String>) -> Result<()> {
        self.message_handler
            .mark_messages_read(user_id, message_ids)
            .await
    }

    /// Get conversation info (group or direct)
    pub async fn get_conversation_info(
        &self,
        conversation_id: &str,
        conversation_type: &ConversationType,
    ) -> Result<ConversationInfo> {
        self.message_handler
            .get_conversation_info(conversation_id, conversation_type)
            .await
    }

    /// Subscribe to message events
    pub fn subscribe_events(&self) -> broadcast::Receiver<MessageEvent> {
        self.message_tx.subscribe()
    }

    /// Subscribe to envelopes delivered by another messaging process.
    pub fn subscribe_gateway_envelopes(&self) -> broadcast::Receiver<serde_json::Value> {
        self.gateway_tx.subscribe()
    }

    /// Publish an API envelope on the signed libp2p gossipsub transport.
    pub async fn publish_gateway_envelope(
        &self,
        message_id: String,
        sender_did: String,
        recipient_dids: Vec<String>,
        payload: serde_json::Value,
    ) -> Result<()> {
        let network = self.network.lock().await;
        network.publish_p2p_message(
            "gateway/envelopes",
            P2PMessage::GatewayEnvelope {
                message_id,
                sender_did,
                recipient_dids,
                payload,
            },
        )
    }

    /// Get current node status
    pub async fn get_status(&self) -> NodeStatus {
        self.status.read().await.clone()
    }

    /// Check if node is running
    pub async fn is_running(&self) -> bool {
        self.status.read().await.is_running
    }

    /// Get node configuration
    pub fn get_config(&self) -> &MessagingConfig {
        &self.config
    }

    /// Get connected peers from the network
    pub async fn get_connected_peers(&self) -> Vec<String> {
        let network = self.network.lock().await;
        network.get_connected_peers()
    }

    pub async fn directory_lookup_remote(
        &self,
        prefix: Option<String>,
        limit: usize,
        timeout: Duration,
        target_peer: Option<String>,
        target_peer_addr: Option<String>,
    ) -> Result<Vec<DirectoryEntry>> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let response_topic = format!("directory/response/{}", request_id);
        let request_topic = match target_peer {
            Some(peer) => format!("directory/lookup/{}", peer),
            None => "directory/lookup".to_string(),
        };

        {
            let network = self.network.lock().await;
            if let Some(addr) = target_peer_addr.clone() {
                let multiaddr = addr
                    .parse::<Multiaddr>()
                    .map_err(|e| anyhow::anyhow!("Invalid peer multiaddr: {}", e))?;
                let _ = network
                    .command_sender()
                    .send(P2PCommand::Dial { address: multiaddr });
            }
            network.subscribe_p2p_topic(&response_topic)?;
            network.publish_p2p_message(
                &request_topic,
                P2PMessage::DirectoryLookupRequest {
                    request_id: request_id.clone(),
                    requester_did: self.config.node_did.clone(),
                    prefix: prefix.clone(),
                    limit,
                },
            )?;
        }

        let mut rx = self.directory_tx.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut merged: HashMap<String, DirectoryEntry> = HashMap::new();

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(P2PMessage::DirectoryLookupResponse {
                    request_id: resp_id,
                    entries,
                    ..
                })) => {
                    if resp_id != request_id {
                        continue;
                    }
                    for entry in entries {
                        merged.insert(entry.did.clone(), entry);
                    }
                }
                Ok(Ok(_)) => {}
                Ok(Err(_)) => break,
                Err(_) => break,
            }
        }

        Ok(merged.into_values().collect())
    }

    pub async fn apply_directory_entries(&self, entries: &[DirectoryEntry]) -> Result<usize> {
        self.message_handler.upsert_directory_entries(entries).await
    }

    pub async fn prune_directory_cache(&self, ttl_seconds: u64) -> Result<usize> {
        self.message_handler
            .prune_directory_cache(ttl_seconds)
            .await
    }

    pub async fn prune_directory_cache_max(&self, max_entries: usize) -> Result<usize> {
        self.message_handler
            .prune_directory_cache_max(max_entries)
            .await
    }

    pub async fn get_shared_file_metadata(&self, file_id: &str) -> Result<Option<SharedFile>> {
        self.message_handler.get_shared_file(file_id).await
    }

    pub async fn get_message_history(&self, did: &str, limit: usize) -> Result<Vec<Message>> {
        let user = self
            .message_handler
            .get_user_by_did(did)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not registered in local directory"))?;
        self.message_handler
            .get_messages_for_user(&user.id, limit)
            .await
    }

    pub async fn get_message_history_decrypted(
        &self,
        did: &str,
        secret_key: &[u8],
        limit: usize,
        conversation_filter: Option<&str>,
    ) -> Result<Vec<Message>> {
        let user = self
            .message_handler
            .get_user_by_did(did)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not registered in local directory"))?;
        self.message_handler
            .get_message_history_for_user(&user.id, secret_key, limit, conversation_filter)
            .await
    }

    pub async fn get_message_by_id_decrypted(
        &self,
        did: &str,
        secret_key: &[u8],
        message_id: &str,
    ) -> Result<Option<Message>> {
        let user = self
            .message_handler
            .get_user_by_did(did)
            .await?
            .ok_or_else(|| anyhow::anyhow!("User not registered in local directory"))?;
        if let Some(entry) = self
            .message_handler
            .get_history_entry_by_message_id(message_id)
            .await?
        {
            if let Some(payload) = entry.payloads.get(&user.id) {
                let algorithm = match payload.algorithm.as_str() {
                    "ECIES" => Algorithm::Kyber1024,
                    other => parse_algorithm_str(other),
                };
                let data = self
                    .message_handler
                    .encryption
                    .decrypt_from_sender(payload, secret_key, algorithm)
                    .await?;
                let message = serde_json::from_slice::<Message>(&data)?;
                return Ok(Some(message));
            }
        }
        Ok(None)
    }

    pub async fn download_file(
        &self,
        file_id: &str,
        recipient_did: &str,
        recipient_private_key: &[u8],
    ) -> Result<Vec<u8>> {
        let file = self
            .message_handler
            .get_shared_file(file_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("File not found: {}", file_id))?;

        let file_bytes = tokio::fs::read(&file.upload_path).await?;

        match &file.conversation_type {
            ConversationType::Direct { .. } => {
                let payload: EncryptedPayload = serde_json::from_slice(&file_bytes)?;
                if payload.kem_ciphertext.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Fallback encrypted files are not supported for decryption"
                    ));
                }
                let algorithm = parse_algorithm_str(&payload.algorithm);
                self.message_handler
                    .encryption
                    .decrypt_from_sender(&payload, recipient_private_key, algorithm)
                    .await
            }
            ConversationType::Group { .. } => {
                #[derive(serde::Deserialize)]
                struct GroupFileBundle {
                    _group_id: String,
                    payloads: HashMap<String, EncryptedPayload>,
                }

                let bundle: GroupFileBundle = serde_json::from_slice(&file_bytes)?;
                let user = self
                    .message_handler
                    .get_user_by_did(recipient_did)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!("Recipient not registered in local directory")
                    })?;

                let payload = bundle
                    .payloads
                    .get(&user.id)
                    .ok_or_else(|| anyhow::anyhow!("No payload found for recipient in bundle"))?;

                if payload.kem_ciphertext.is_empty() {
                    return Err(anyhow::anyhow!(
                        "Fallback encrypted files are not supported for decryption"
                    ));
                }

                let algorithm = parse_algorithm_str(&payload.algorithm);
                self.message_handler
                    .encryption
                    .decrypt_from_sender(payload, recipient_private_key, algorithm)
                    .await
            }
        }
    }

    async fn start_directory_service(&self) -> Result<()> {
        let mut network = self.network.lock().await;
        network.subscribe_p2p_topic("directory/lookup")?;
        network.subscribe_p2p_topic("gateway/envelopes")?;
        let scoped_topic = format!("directory/lookup/{}", self.config.node_did);
        network.subscribe_p2p_topic(&scoped_topic)?;
        let mut event_rx = network
            .take_event_receiver()
            .ok_or_else(|| anyhow::anyhow!("Directory service already started"))?;
        drop(network);

        let message_handler = self.message_handler.clone();
        let command_sender = {
            let network = self.network.lock().await;
            network.command_sender()
        };
        let directory_tx = self.directory_tx.clone();
        let gateway_tx = self.gateway_tx.clone();
        let responder_did = self.config.node_did.clone();
        let status = self.status.clone();

        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                match event {
                    P2PNetworkEvent::PeerConnected { .. } => {
                        status.write().await.active_connections += 1;
                    }
                    P2PNetworkEvent::PeerDisconnected { .. } => {
                        let mut status = status.write().await;
                        status.active_connections = status.active_connections.saturating_sub(1);
                    }
                    P2PNetworkEvent::MessageReceived { message, .. } => match message.clone() {
                        P2PMessage::GatewayEnvelope {
                            sender_did,
                            recipient_dids,
                            payload,
                            ..
                        } => {
                            if sender_did != responder_did
                                && recipient_dids.iter().any(|did| did == &responder_did)
                            {
                                let _ = gateway_tx.send(payload);
                                let mut status = status.write().await;
                                status.messages_received_today += 1;
                                status.direct_messages_received_today += 1;
                                status.last_activity = Utc::now();
                            }
                        }
                        P2PMessage::DirectoryLookupRequest {
                            request_id,
                            prefix,
                            limit,
                            ..
                        } => {
                            let prefix = match prefix {
                                Some(prefix) => prefix,
                                None => {
                                    // Do not respond to unscoped requests
                                    continue;
                                }
                            };
                            let users = message_handler.get_all_users().await.unwrap_or_default();
                            let mut entries = Vec::new();
                            for user in users {
                                if !user.did.starts_with(&prefix) {
                                    continue;
                                }
                                entries.push(DirectoryEntry {
                                    did: user.did,
                                    username: user.username,
                                    encryption_algorithm: user.encryption_algorithm,
                                    public_key: Some(user.public_key),
                                });
                                if entries.len() >= limit {
                                    break;
                                }
                            }

                            let response = P2PMessage::DirectoryLookupResponse {
                                request_id: request_id.clone(),
                                responder_did: responder_did.clone(),
                                entries,
                            };

                            let topic = format!("directory/response/{}", request_id);
                            let _ = command_sender.send(P2PCommand::PublishTopic {
                                topic,
                                message: response,
                            });
                        }
                        P2PMessage::DirectoryLookupResponse { .. } => {
                            let _ = directory_tx.send(message);
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        });

        Ok(())
    }
}

fn parse_algorithm_str(value: &str) -> Algorithm {
    match value {
        "Kyber512" => Algorithm::Kyber512,
        "Kyber768" => Algorithm::Kyber768,
        "Kyber1024" => Algorithm::Kyber1024,
        "NtruPrimeSntrup761" => Algorithm::NtruPrimeSntrup761,
        "FrodoKem1344Aes" => Algorithm::FrodoKem1344Aes,
        "FrodoKem1344Shake" => Algorithm::FrodoKem1344Shake,
        _ => Algorithm::Kyber1024,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_messaging_node_creation() {
        let config = MessagingConfig::default();
        let node = MessagingNode::new(config).await.unwrap();

        assert!(!node.is_running().await);
    }

    #[tokio::test]
    async fn test_user_registration() {
        let config = MessagingConfig::default();
        let node = MessagingNode::new(config).await.unwrap();

        let user = node
            .register_user(
                "did:example:alice".to_string(),
                "Alice".to_string(),
                vec![1, 2, 3, 4], // Mock public key
                Algorithm::Kyber1024,
            )
            .await
            .unwrap();

        assert_eq!(user.username, "Alice");
        assert_eq!(user.did, "did:example:alice");
    }

    #[tokio::test]
    async fn test_group_creation() {
        let config = MessagingConfig::default();
        let node = MessagingNode::new(config).await.unwrap();

        // Register a user first
        let user = node
            .register_user(
                "did:example:alice".to_string(),
                "Alice".to_string(),
                vec![1, 2, 3, 4],
                Algorithm::Kyber1024,
            )
            .await
            .unwrap();

        // Create a group
        let group = node
            .create_group(
                "Test Group".to_string(),
                user.id.clone(),
                Some("A test group".to_string()),
            )
            .await
            .unwrap();

        assert_eq!(group.name, "Test Group");
        assert_eq!(group.creator_id, user.id);
    }

    #[tokio::test]
    async fn test_direct_messaging() {
        let config = MessagingConfig::default();
        let node = MessagingNode::new(config).await.unwrap();

        // Register two users
        let alice = node
            .register_user(
                "did:example:alice".to_string(),
                "Alice".to_string(),
                vec![1, 2, 3, 4],
                Algorithm::Kyber1024,
            )
            .await
            .unwrap();

        let bob = node
            .register_user(
                "did:example:bob".to_string(),
                "Bob".to_string(),
                vec![5, 6, 7, 8],
                Algorithm::Kyber1024,
            )
            .await
            .unwrap();

        // Send a direct message from Alice to Bob
        let events = node
            .send_direct_message(alice.id.clone(), bob.did.clone(), "Hello Bob!".to_string())
            .await
            .unwrap();

        assert!(!events.is_empty());

        // Check that Alice has a direct conversation with Bob
        let conversations = node.get_user_direct_conversations(&alice.id).await.unwrap();
        assert_eq!(conversations.len(), 1);
        assert!(conversations[0].has_participant(&alice.id));
        assert!(conversations[0].has_participant(&bob.id));
    }

    #[tokio::test]
    async fn test_direct_conversation_creation() {
        let config = MessagingConfig::default();
        let node = MessagingNode::new(config).await.unwrap();

        // Register two users
        let alice = node
            .register_user(
                "did:example:alice".to_string(),
                "Alice".to_string(),
                vec![1, 2, 3, 4],
                Algorithm::Kyber1024,
            )
            .await
            .unwrap();

        let bob = node
            .register_user(
                "did:example:bob".to_string(),
                "Bob".to_string(),
                vec![5, 6, 7, 8],
                Algorithm::Kyber1024,
            )
            .await
            .unwrap();

        // Create a direct conversation
        let conversation = node
            .create_or_get_direct_conversation(alice.id.clone(), bob.id.clone())
            .await
            .unwrap();

        assert!(conversation.has_participant(&alice.id));
        assert!(conversation.has_participant(&bob.id));

        // Creating the same conversation again should return the existing one
        let same_conversation = node
            .create_or_get_direct_conversation(bob.id.clone(), alice.id.clone())
            .await
            .unwrap();

        assert_eq!(conversation.id, same_conversation.id);
    }
}
