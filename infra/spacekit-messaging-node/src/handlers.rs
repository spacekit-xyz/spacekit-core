//! Message handlers for processing different types of messages

use crate::encryption::{EncryptedPayload, MessageEncryption};
use crate::models::*;
use anyhow::{anyhow, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spacekit_primitives::v1::crypto::quantum::{Algorithm, CipherSuite};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedHistoryEntry {
    pub message_id: String,
    pub conversation_id: String,
    pub created_at: chrono::DateTime<Utc>,
    pub payloads: HashMap<String, EncryptedPayload>,
}

/// Unified message handler that processes both group and direct messaging with encryption
pub struct MessageHandler {
    /// Node DID for message filtering
    #[allow(dead_code)]
    _node_did: String,
    /// Users in the system
    users: Arc<RwLock<HashMap<String, User>>>,
    /// Groups in the system
    groups: Arc<RwLock<HashMap<String, Group>>>,
    /// Direct conversations
    direct_conversations: Arc<RwLock<HashMap<String, DirectConversation>>>,
    /// Group memberships
    memberships: Arc<RwLock<HashMap<String, Vec<GroupMembership>>>>,
    /// Shared files (both group and direct)
    shared_files: Arc<RwLock<HashMap<String, SharedFile>>>,
    /// Encrypted messages by recipient
    encrypted_messages: Arc<RwLock<HashMap<String, Vec<EncryptedMessage>>>>,
    /// Message history by conversation ID
    messages: Arc<RwLock<HashMap<String, Vec<Message>>>>,
    /// Encrypted history entries by conversation ID (LRU cache; disk is source of truth)
    history_entries: Arc<RwLock<HashMap<String, Vec<EncryptedHistoryEntry>>>>,
    /// Conversation IDs discovered on disk (lazy load index)
    known_conversations: Arc<RwLock<HashSet<String>>>,
    /// Conversations currently loaded into `history_entries`
    loaded_conversations: Arc<RwLock<HashSet<String>>>,
    /// Base directory for message history persistence
    history_dir: PathBuf,
    /// When false, skip disk writes and loads
    persistence_enabled: bool,
    /// When true, load history per conversation on demand instead of at startup
    lazy_load_history: bool,
    /// Max conversations kept in RAM before evicting oldest loaded conversation
    history_cache_conversations: usize,
    /// Optional redb-backed durable history (preferred over JSONL)
    history_store: Option<Arc<crate::history_store::HistoryStore>>,
    history_writer: Option<crate::history_store::HistoryStoreWriter>,
    /// When true, append/read via redb instead of JSONL files
    use_redb_history: bool,
    /// Encryption service (public for P2P access)
    pub encryption: MessageEncryption,
    /// Content filter
    #[allow(dead_code)]
    _content_filter: ContentFilter,
}

/// Legacy alias for backward compatibility
pub type GroupMessageHandler = MessageHandler;

/// Events that can be emitted by the message handler
#[derive(Debug, Clone)]
pub enum MessageEvent {
    /// A new message was received (group or direct)
    MessageReceived {
        message: Message,
        conversation_id: String,
        conversation_type: ConversationType,
        sender: User,
    },
    /// A file was uploaded
    FileUploaded {
        file: SharedFile,
        conversation_id: String,
        conversation_type: ConversationType,
        uploader: User,
    },
    /// A user joined a group
    UserJoinedGroup {
        user: User,
        group: Group,
        invited_by: Option<String>,
    },
    /// A user left a group
    UserLeftGroup { user_id: String, group: Group },
    /// A group was created
    GroupCreated { group: Group, creator: User },
    /// A direct conversation was created
    DirectConversationCreated {
        conversation: DirectConversation,
        initiator: User,
        recipient: User,
    },
    /// Group settings were updated
    GroupUpdated { group: Group, updated_by: String },
    /// A file was archived or deleted
    FileStatusChanged {
        file: SharedFile,
        new_status: FileStatus,
        changed_by: String,
    },
    /// Direct message delivery confirmation
    DirectMessageDelivered {
        message_id: String,
        recipient_id: String,
        delivered_at: chrono::DateTime<Utc>,
    },
    /// User is typing in a conversation
    UserTyping {
        user_id: String,
        conversation_id: String,
        conversation_type: ConversationType,
    },
}

/// Direct message request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectMessageRequest {
    pub recipient_did: String,
    pub content: String,
    pub message_type: DirectMessageType,
}

/// Types of direct messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DirectMessageType {
    Text,
    File { filename: String, size: u64 },
    Image { filename: String, size: u64 },
}

impl MessageHandler {
    /// Create a new message handler
    pub fn new(
        node_did: String,
        history_dir: PathBuf,
        persistence_enabled: bool,
        lazy_load_history: bool,
        history_cache_conversations: usize,
        use_redb_history: bool,
        history_store: Option<Arc<crate::history_store::HistoryStore>>,
        history_writer: Option<crate::history_store::HistoryStoreWriter>,
    ) -> Self {
        let encryption = MessageEncryption::new(
            Algorithm::Kyber1024, // Default quantum-resistant algorithm
            CipherSuite::AES256,  // Default cipher suite
        );

        if let Err(e) = std::fs::create_dir_all(&history_dir) {
            warn!(
                "Failed to create history directory {:?}: {}",
                history_dir, e
            );
        }

        Self {
            _node_did: node_did,
            users: Arc::new(RwLock::new(HashMap::new())),
            groups: Arc::new(RwLock::new(HashMap::new())),
            direct_conversations: Arc::new(RwLock::new(HashMap::new())),
            memberships: Arc::new(RwLock::new(HashMap::new())),
            shared_files: Arc::new(RwLock::new(HashMap::new())),
            encrypted_messages: Arc::new(RwLock::new(HashMap::new())),
            messages: Arc::new(RwLock::new(HashMap::new())),
            history_entries: Arc::new(RwLock::new(HashMap::new())),
            known_conversations: Arc::new(RwLock::new(HashSet::new())),
            loaded_conversations: Arc::new(RwLock::new(HashSet::new())),
            encryption,
            _content_filter: ContentFilter::new(true, true),
            history_dir,
            persistence_enabled,
            lazy_load_history,
            history_cache_conversations: history_cache_conversations.max(1),
            history_store,
            history_writer,
            use_redb_history,
        }
    }

    /// Register a user in the system
    pub async fn register_user(
        &self,
        did: String,
        username: String,
        public_key: Vec<u8>,
        algorithm: Algorithm,
    ) -> Result<User> {
        let user = User::new(did, username, public_key, format!("{:?}", algorithm));
        let user_id = user.id.clone();

        let mut users = self.users.write().await;
        users.insert(user_id.clone(), user.clone());

        info!("Registered user: {} (DID: {})", user.username, user.did);
        Ok(user)
    }

    /// Get user by DID
    pub async fn get_user_by_did(&self, did: &str) -> Result<Option<User>> {
        let users = self.users.read().await;
        for user in users.values() {
            if user.did == did {
                return Ok(Some(user.clone()));
            }
        }
        Ok(None)
    }

    /// Get user by ID
    pub async fn get_user_by_id(&self, user_id: &str) -> Result<Option<User>> {
        let users = self.users.read().await;
        Ok(users.get(user_id).cloned())
    }

    /// Get all registered users
    pub async fn get_all_users(&self) -> Result<Vec<User>> {
        let users = self.users.read().await;
        Ok(users.values().cloned().collect())
    }

    /// Upsert directory entries into local user registry (opt-in sync)
    pub async fn upsert_directory_entries(&self, entries: &[DirectoryEntry]) -> Result<usize> {
        let mut users = self.users.write().await;
        let mut updated = 0;

        for entry in entries {
            let public_key = match &entry.public_key {
                Some(key) => key.clone(),
                None => continue,
            };

            if let Some(existing) = users.values_mut().find(|u| u.did == entry.did) {
                existing.username = entry.username.clone();
                existing.public_key = public_key;
                existing.encryption_algorithm = entry.encryption_algorithm.clone();
                existing.last_seen = Some(Utc::now());
                updated += 1;
                continue;
            }

            let mut user = User::new(
                entry.did.clone(),
                entry.username.clone(),
                public_key,
                entry.encryption_algorithm.clone(),
            );
            user.last_seen = Some(Utc::now());
            users.insert(user.id.clone(), user);
            updated += 1;
        }

        Ok(updated)
    }

    pub async fn prune_directory_cache(&self, ttl_seconds: u64) -> Result<usize> {
        let cutoff = Utc::now() - chrono::Duration::seconds(ttl_seconds as i64);
        let mut users = self.users.write().await;
        let before = users.len();

        users.retain(|_, user| {
            let last_seen = user.last_seen.unwrap_or(user.created_at);
            last_seen >= cutoff
        });

        Ok(before.saturating_sub(users.len()))
    }

    pub async fn prune_directory_cache_max(&self, max_entries: usize) -> Result<usize> {
        let mut users = self.users.write().await;
        let before = users.len();
        if before <= max_entries {
            return Ok(0);
        }

        let mut entries: Vec<(String, DateTime<Utc>)> = users
            .iter()
            .map(|(id, user)| {
                let last_seen = user.last_seen.unwrap_or(user.created_at);
                (id.clone(), last_seen)
            })
            .collect();
        entries.sort_by_key(|(_, last_seen)| *last_seen);

        let remove_count = before.saturating_sub(max_entries);
        for (id, _) in entries.into_iter().take(remove_count) {
            users.remove(&id);
        }

        Ok(remove_count)
    }

    pub async fn get_shared_file(&self, file_id: &str) -> Result<Option<SharedFile>> {
        let shared_files = self.shared_files.read().await;
        Ok(shared_files.get(file_id).cloned())
    }

    /// Upload a file to a direct conversation (encrypted for the recipient)
    pub async fn upload_direct_file(
        &self,
        conversation_id: String,
        uploader_id: String,
        recipient_id: String,
        filename: String,
        file_data: Vec<u8>,
        storage_dir: &str,
    ) -> Result<SharedFile> {
        let users = self.users.read().await;
        let recipient = users
            .get(&recipient_id)
            .ok_or_else(|| anyhow!("Recipient not found: {}", recipient_id))?
            .clone();
        drop(users);

        let recipient_algorithm = self.parse_algorithm(&recipient.encryption_algorithm)?;
        let encrypted_payload = self
            .encryption
            .encrypt_for_recipient(&file_data, &recipient.public_key, recipient_algorithm)
            .await?;

        let upload_dir = Path::new(storage_dir)
            .join("uploads")
            .join(&conversation_id);
        tokio::fs::create_dir_all(&upload_dir).await?;

        let upload_path = upload_dir.join(format!("{}.json", Uuid::new_v4()));
        let payload_bytes = serde_json::to_vec(&encrypted_payload)?;
        let encrypted_size = payload_bytes.len() as u64;
        tokio::fs::write(&upload_path, &payload_bytes).await?;

        let file_hash = Sha256::digest(&file_data);
        let shared_file = SharedFile::new_direct(
            conversation_id,
            uploader_id,
            filename,
            file_data.len() as u64,
            encrypted_size,
            "application/octet-stream".to_string(),
            hex::encode(file_hash),
            encrypted_payload.algorithm.clone(),
            upload_path.to_string_lossy().to_string(),
        );

        let mut shared_files = self.shared_files.write().await;
        shared_files.insert(shared_file.id.clone(), shared_file.clone());

        Ok(shared_file)
    }

    /// Upload a file to a group (encrypted per member, stored as a single bundle)
    pub async fn upload_group_file(
        &self,
        group_id: String,
        uploader_id: String,
        filename: String,
        file_data: Vec<u8>,
        storage_dir: &str,
    ) -> Result<SharedFile> {
        let memberships = self.memberships.read().await;
        let group_members = memberships
            .get(&group_id)
            .ok_or_else(|| anyhow!("Group not found: {}", group_id))?;

        let users = self.users.read().await;
        let mut recipient_keys = Vec::new();
        for membership in group_members {
            if let Some(user) = users.get(&membership.user_id) {
                let algorithm = self.parse_algorithm(&user.encryption_algorithm)?;
                recipient_keys.push((user.id.clone(), user.public_key.clone(), algorithm));
            }
        }
        drop(users);

        let encrypted_payloads = self
            .encryption
            .encrypt_for_group(&file_data, &recipient_keys)
            .await?;

        #[derive(Serialize, Deserialize)]
        struct GroupFileBundle {
            group_id: String,
            payloads: HashMap<String, EncryptedPayload>,
        }

        let bundle = GroupFileBundle {
            group_id: group_id.clone(),
            payloads: encrypted_payloads,
        };

        let upload_dir = Path::new(storage_dir).join("uploads").join(&group_id);
        tokio::fs::create_dir_all(&upload_dir).await?;

        let upload_path = upload_dir.join(format!("{}.json", Uuid::new_v4()));
        let payload_bytes = serde_json::to_vec(&bundle)?;
        let encrypted_size = payload_bytes.len() as u64;
        tokio::fs::write(&upload_path, &payload_bytes).await?;

        let file_hash = Sha256::digest(&file_data);
        let shared_file = SharedFile::new_group(
            group_id,
            uploader_id,
            filename,
            file_data.len() as u64,
            encrypted_size,
            "application/octet-stream".to_string(),
            hex::encode(file_hash),
            "group-bundle".to_string(),
            upload_path.to_string_lossy().to_string(),
        );

        let mut shared_files = self.shared_files.write().await;
        shared_files.insert(shared_file.id.clone(), shared_file.clone());

        Ok(shared_file)
    }

    /// Get all groups a user is a member of
    pub async fn get_groups_for_user(&self, user_id: &str) -> Result<Vec<Group>> {
        let memberships = self.memberships.read().await;
        let groups = self.groups.read().await;
        let mut result = Vec::new();

        for (group_id, group_members) in memberships.iter() {
            if group_members.iter().any(|m| m.user_id == user_id) {
                if let Some(group) = groups.get(group_id) {
                    result.push(group.clone());
                }
            }
        }

        Ok(result)
    }

    /// Create a new group
    pub async fn create_group(
        &self,
        name: String,
        creator_id: String,
        description: Option<String>,
    ) -> Result<Group> {
        let group = Group::new(name, creator_id.clone(), description);
        let group_id = group.id.clone();

        // Add creator as a member with Creator role
        let membership = GroupMembership {
            group_id: group_id.clone(),
            user_id: creator_id.clone(),
            role: MemberRole::Creator,
            joined_at: Utc::now(),
            invited_by: None,
        };

        let mut groups = self.groups.write().await;
        let mut memberships = self.memberships.write().await;

        groups.insert(group_id.clone(), group.clone());
        memberships
            .entry(group_id.clone())
            .or_insert_with(Vec::new)
            .push(membership);

        let users = self.users.read().await;
        if let Some(creator) = users.get(&creator_id) {
            info!("Created group '{}' by {}", group.name, creator.username);
        }

        Ok(group)
    }

    /// Create or get existing direct conversation between two users
    pub async fn create_or_get_direct_conversation(
        &self,
        user_a_id: String,
        user_b_id: String,
    ) -> Result<DirectConversation> {
        let conversations = self.direct_conversations.read().await;

        // Check if conversation already exists
        for conversation in conversations.values() {
            if (conversation.participant_a_id == user_a_id
                && conversation.participant_b_id == user_b_id)
                || (conversation.participant_a_id == user_b_id
                    && conversation.participant_b_id == user_a_id)
            {
                return Ok(conversation.clone());
            }
        }

        // Create new conversation
        drop(conversations);
        let conversation = DirectConversation::new(user_a_id, user_b_id);
        let conversation_id = conversation.id.clone();

        let mut conversations = self.direct_conversations.write().await;
        conversations.insert(conversation_id, conversation.clone());

        info!(
            "Created direct conversation between {} and {}",
            conversation.participant_a_id, conversation.participant_b_id
        );
        Ok(conversation)
    }

    /// Send a direct message to another user
    pub async fn send_direct_message(
        &self,
        sender_id: String,
        recipient_did: String,
        content: String,
    ) -> Result<Vec<MessageEvent>> {
        // Find recipient by DID
        let recipient = self
            .get_user_by_did(&recipient_did)
            .await?
            .ok_or_else(|| anyhow!("Recipient not found: {}", recipient_did))?;

        // Create or get conversation
        let conversation = self
            .create_or_get_direct_conversation(sender_id.clone(), recipient.id.clone())
            .await?;

        // Create message
        let message = Message::new_text_direct(conversation.id.clone(), sender_id.clone(), content);

        // Send message
        self.send_message_to_direct_conversation(message, conversation)
            .await
    }

    /// Send a message to a direct conversation
    async fn send_message_to_direct_conversation(
        &self,
        message: Message,
        conversation: DirectConversation,
    ) -> Result<Vec<MessageEvent>> {
        let mut events = Vec::new();

        // Get both participants
        let users = self.users.read().await;
        let sender = users
            .get(&message.sender_id)
            .ok_or_else(|| anyhow!("Sender not found: {}", message.sender_id))?;

        let recipient_id = conversation
            .get_other_participant(&message.sender_id)
            .ok_or_else(|| anyhow!("Could not find other participant"))?;

        let recipient = users
            .get(recipient_id)
            .ok_or_else(|| anyhow!("Recipient not found: {}", recipient_id))?;

        // Encrypt message for recipient
        let message_data = serde_json::to_vec(&message)?;
        let recipient_algorithm = self.parse_algorithm(&recipient.encryption_algorithm)?;

        let encrypted_payload = self
            .encryption
            .encrypt_for_recipient(&message_data, &recipient.public_key, recipient_algorithm)
            .await?;

        let encrypted_payload_clone = encrypted_payload.clone();
        // Store encrypted message
        let encrypted_msg = EncryptedMessage {
            message_id: message.id.clone(),
            recipient_id: recipient.id.clone(),
            encrypted_content: encrypted_payload.data,
            kem_ciphertext: encrypted_payload.kem_ciphertext,
            algorithm: encrypted_payload.algorithm,
            nonce: encrypted_payload.nonce,
            created_at: message.created_at,
        };

        let mut encrypted_messages = self.encrypted_messages.write().await;
        encrypted_messages
            .entry(recipient.id.clone())
            .or_insert_with(Vec::new)
            .push(encrypted_msg);

        // Update conversation last message time
        let mut conversations = self.direct_conversations.write().await;
        if let Some(conv) = conversations.get_mut(&conversation.id) {
            conv.last_message_at = Some(message.created_at);
        }
        drop(conversations);

        self.store_message(&conversation.id, &message).await;
        let history_payloads = {
            let sender_algorithm = self.parse_algorithm(&sender.encryption_algorithm)?;
            let sender_payload = self
                .encryption
                .encrypt_for_recipient(&message_data, &sender.public_key, sender_algorithm)
                .await?;
            let mut payloads = HashMap::new();
            payloads.insert(sender.id.clone(), sender_payload);
            payloads.insert(recipient.id.clone(), encrypted_payload_clone); // Note: Nonce is not cloned
            payloads
        };
        let _ = self
            .append_history_entry(&conversation.id, &message, history_payloads)
            .await;

        // Create events
        events.push(MessageEvent::MessageReceived {
            message: message.clone(),
            conversation_id: conversation.id.clone(),
            conversation_type: ConversationType::Direct {
                conversation_id: conversation.id.clone(),
            },
            sender: sender.clone(),
        });

        events.push(MessageEvent::DirectMessageDelivered {
            message_id: message.id,
            recipient_id: recipient.id.clone(),
            delivered_at: Utc::now(),
        });

        info!(
            "Direct message sent from {} to {}",
            sender.username, recipient.username
        );
        Ok(events)
    }

    /// Send a text message to a group
    pub async fn send_text_message(
        &self,
        group_id: String,
        sender_id: String,
        content: String,
    ) -> Result<Vec<MessageEvent>> {
        let message = Message::new_text_group(group_id.clone(), sender_id.clone(), content);
        self.send_message_to_group(message).await
    }

    /// Send a message to all group members with asymmetric encryption
    async fn send_message_to_group(&self, message: Message) -> Result<Vec<MessageEvent>> {
        let mut events = Vec::new();

        let group_id = match &message.conversation_type {
            ConversationType::Group { group_id } => group_id.clone(),
            _ => return Err(anyhow!("Invalid conversation type for group message")),
        };

        // Get group members
        let memberships = self.memberships.read().await;
        let group_members = memberships
            .get(&group_id)
            .ok_or_else(|| anyhow!("Group not found: {}", group_id))?;

        // Get user public keys
        let users = self.users.read().await;
        let mut recipient_keys = Vec::new();

        for membership in group_members {
            if let Some(user) = users.get(&membership.user_id) {
                let algorithm = self.parse_algorithm(&user.encryption_algorithm)?;
                recipient_keys.push((user.id.clone(), user.public_key.clone(), algorithm));
            }
        }

        // Encrypt message for each recipient
        let message_data = serde_json::to_vec(&message)?;
        let encrypted_payloads = self
            .encryption
            .encrypt_for_group(&message_data, &recipient_keys)
            .await?;
        let encrypted_payloads_clone = encrypted_payloads.clone();
        // Store encrypted messages
        let mut encrypted_messages = self.encrypted_messages.write().await;
        for (user_id, payload) in encrypted_payloads {
            let encrypted_msg = EncryptedMessage {
                message_id: message.id.clone(),
                recipient_id: user_id.clone(),
                encrypted_content: payload.data,
                kem_ciphertext: payload.kem_ciphertext,
                algorithm: payload.algorithm,
                nonce: payload.nonce,
                created_at: message.created_at,
            };

            encrypted_messages
                .entry(user_id)
                .or_insert_with(Vec::new)
                .push(encrypted_msg);
        }
        drop(encrypted_messages);

        self.store_message(&group_id, &message).await;
        let _ = self
            .append_history_entry(&group_id, &message, encrypted_payloads_clone)
            .await;

        // Create message received event
        let groups = self.groups.read().await;
        if let Some(group) = groups.get(&group_id) {
            if let Some(sender) = users.get(&message.sender_id) {
                events.push(MessageEvent::MessageReceived {
                    message,
                    conversation_id: group.id.clone(),
                    conversation_type: ConversationType::Group {
                        group_id: group.id.clone(),
                    },
                    sender: sender.clone(),
                });
            }
        }

        info!("Message sent to {} group members", group_members.len());
        Ok(events)
    }

    async fn store_message(&self, conversation_id: &str, message: &Message) {
        let mut messages = self.messages.write().await;
        messages
            .entry(conversation_id.to_string())
            .or_insert_with(Vec::new)
            .push(message.clone());
    }

    pub async fn load_message_history(&self) -> Result<()> {
        if !self.persistence_enabled {
            return Ok(());
        }
        if self.use_redb_history {
            return self.scan_redb_history_index().await;
        }
        if self.lazy_load_history {
            return self.scan_message_history_index().await;
        }
        self.load_all_message_history().await
    }

    async fn scan_redb_history_index(&self) -> Result<()> {
        let Some(store) = &self.history_store else {
            return Ok(());
        };
        let ids = store.list_conversation_ids()?;
        let mut known = self.known_conversations.write().await;
        known.clear();
        for id in ids {
            known.insert(id);
        }
        info!(
            "Indexed {} conversations in redb history (lazy load)",
            known.len()
        );
        Ok(())
    }

    /// Index conversation IDs on disk without loading message bodies into RAM.
    pub async fn scan_message_history_index(&self) -> Result<()> {
        if !self.history_dir.exists() {
            return Ok(());
        }
        let mut known = self.known_conversations.write().await;
        known.clear();
        let mut entries = tokio::fs::read_dir(&self.history_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                known.insert(stem.to_string());
            }
        }
        info!(
            "Indexed {} conversation history files (lazy load)",
            known.len()
        );
        Ok(())
    }

    async fn load_all_message_history(&self) -> Result<()> {
        if !self.history_dir.exists() {
            return Ok(());
        }

        let mut entries = tokio::fs::read_dir(&self.history_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let conversation_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            self.load_conversation_history(&conversation_id).await?;
        }
        Ok(())
    }

    async fn load_conversation_history(&self, conversation_id: &str) -> Result<()> {
        if self.use_redb_history {
            return self.load_conversation_history_redb(conversation_id).await;
        }
        let path = self.history_file_path(conversation_id);
        if !path.exists() {
            return Ok(());
        }
        let content = tokio::fs::read_to_string(&path).await?;
        let mut loaded = Vec::new();
        for line in content.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<EncryptedHistoryEntry>(line) {
                loaded.push(entry);
            }
        }
        {
            let mut entries = self.history_entries.write().await;
            entries.insert(conversation_id.to_string(), loaded);
        }
        {
            let mut loaded_set = self.loaded_conversations.write().await;
            loaded_set.insert(conversation_id.to_string());
        }
        {
            let mut known = self.known_conversations.write().await;
            known.insert(conversation_id.to_string());
        }
        self.evict_history_cache_if_needed().await;
        Ok(())
    }

    async fn load_conversation_history_redb(&self, conversation_id: &str) -> Result<()> {
        let Some(store) = &self.history_store else {
            return Ok(());
        };
        let mut loaded = Vec::new();
        for bytes in store.load_conversation(conversation_id)? {
            if let Ok(entry) = serde_json::from_slice::<EncryptedHistoryEntry>(&bytes) {
                loaded.push(entry);
            }
        }
        {
            let mut entries = self.history_entries.write().await;
            entries.insert(conversation_id.to_string(), loaded);
        }
        {
            let mut loaded_set = self.loaded_conversations.write().await;
            loaded_set.insert(conversation_id.to_string());
        }
        {
            let mut known = self.known_conversations.write().await;
            known.insert(conversation_id.to_string());
        }
        self.evict_history_cache_if_needed().await;
        Ok(())
    }

    async fn ensure_conversation_loaded(&self, conversation_id: &str) -> Result<()> {
        if !self.persistence_enabled {
            return Ok(());
        }
        {
            let loaded = self.loaded_conversations.read().await;
            if loaded.contains(conversation_id) {
                return Ok(());
            }
        }
        self.load_conversation_history(conversation_id).await
    }

    async fn evict_history_cache_if_needed(&self) {
        let loaded_count = self.loaded_conversations.read().await.len();
        if loaded_count <= self.history_cache_conversations {
            return;
        }
        let to_evict = loaded_count.saturating_sub(self.history_cache_conversations);
        let victims: Vec<String> = {
            let loaded = self.loaded_conversations.read().await;
            loaded.iter().take(to_evict).cloned().collect()
        };
        for conv in victims {
            let mut entries = self.history_entries.write().await;
            entries.remove(&conv);
            let mut loaded = self.loaded_conversations.write().await;
            loaded.remove(&conv);
        }
    }

    async fn find_history_entry_on_disk(
        &self,
        message_id: &str,
    ) -> Result<Option<EncryptedHistoryEntry>> {
        if self.use_redb_history {
            if let Some(store) = &self.history_store {
                if let Some(bytes) = store.find_by_message_id(message_id)? {
                    if let Ok(entry) = serde_json::from_slice(&bytes) {
                        return Ok(Some(entry));
                    }
                }
            }
            return Ok(None);
        }

        let conversations: Vec<String> = {
            let known = self.known_conversations.read().await;
            if !known.is_empty() {
                known.iter().cloned().collect()
            } else {
                drop(known);
                self.scan_message_history_index().await?;
                self.known_conversations
                    .read()
                    .await
                    .iter()
                    .cloned()
                    .collect()
            }
        };
        for conversation_id in conversations {
            let path = self.history_file_path(&conversation_id);
            if !path.exists() {
                continue;
            }
            let content = tokio::fs::read_to_string(&path).await?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(entry) = serde_json::from_str::<EncryptedHistoryEntry>(line) {
                    if entry.message_id == message_id {
                        return Ok(Some(entry));
                    }
                }
            }
        }
        Ok(None)
    }

    async fn append_history_entry(
        &self,
        conversation_id: &str,
        message: &Message,
        payloads: HashMap<String, EncryptedPayload>,
    ) -> Result<()> {
        let entry = EncryptedHistoryEntry {
            message_id: message.id.clone(),
            conversation_id: conversation_id.to_string(),
            created_at: message.created_at,
            payloads,
        };

        {
            let mut entries = self.history_entries.write().await;
            entries
                .entry(conversation_id.to_string())
                .or_insert_with(Vec::new)
                .push(entry.clone());
        }
        {
            let mut known = self.known_conversations.write().await;
            known.insert(conversation_id.to_string());
        }
        {
            let mut loaded = self.loaded_conversations.write().await;
            loaded.insert(conversation_id.to_string());
        }

        if !self.persistence_enabled {
            return Ok(());
        }

        if self.use_redb_history {
            if let Some(writer) = &self.history_writer {
                let line = serde_json::to_vec(&entry)?;
                writer
                    .append(conversation_id, &entry.message_id, line)
                    .await?;
                return Ok(());
            }
            if let Some(store) = &self.history_store {
                let line = serde_json::to_vec(&entry)?;
                store.append(conversation_id, &entry.message_id, &line)?;
            }
            return Ok(());
        }

        let path = self.history_file_path(conversation_id);
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        let line = serde_json::to_string(&entry)?;
        use tokio::io::AsyncWriteExt;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        Ok(())
    }

    fn history_file_path(&self, conversation_id: &str) -> PathBuf {
        self.history_dir.join(format!("{}.jsonl", conversation_id))
    }

    pub async fn get_messages_for_user(&self, user_id: &str, limit: usize) -> Result<Vec<Message>> {
        let mut collected = Vec::new();

        let direct_conversations = self.direct_conversations.read().await;
        let memberships = self.memberships.read().await;
        let messages = self.messages.read().await;

        for (conv_id, conv) in direct_conversations.iter() {
            if conv.participant_a_id == user_id || conv.participant_b_id == user_id {
                if let Some(msgs) = messages.get(conv_id) {
                    collected.extend_from_slice(msgs);
                }
            }
        }

        for (group_id, members) in memberships.iter() {
            if members.iter().any(|m| m.user_id == user_id) {
                if let Some(msgs) = messages.get(group_id) {
                    collected.extend_from_slice(msgs);
                }
            }
        }

        collected.sort_by_key(|msg| msg.created_at);
        collected.reverse();
        collected.truncate(limit);
        Ok(collected)
    }

    pub async fn get_message_history_for_user(
        &self,
        user_id: &str,
        secret_key: &[u8],
        limit: usize,
        conversation_filter: Option<&str>,
    ) -> Result<Vec<Message>> {
        if let Some(filter) = conversation_filter {
            self.ensure_conversation_loaded(filter).await?;
        } else if self.lazy_load_history && self.persistence_enabled {
            self.scan_message_history_index().await?;
            let conversations: Vec<String> = self
                .known_conversations
                .read()
                .await
                .iter()
                .cloned()
                .collect();
            for conv in conversations {
                self.ensure_conversation_loaded(&conv).await?;
            }
        }

        let entries = self.history_entries.read().await;
        let mut messages = Vec::new();

        for (conversation_id, history_entries) in entries.iter() {
            if let Some(filter) = conversation_filter {
                if conversation_id != filter {
                    continue;
                }
            }
            for entry in history_entries {
                if let Some(payload) = entry.payloads.get(user_id) {
                    let algorithm = match payload.algorithm.as_str() {
                        "ECIES" => Algorithm::Kyber1024,
                        other => self.parse_algorithm(other)?,
                    };
                    if let Ok(data) = self
                        .encryption
                        .decrypt_from_sender(payload, secret_key, algorithm)
                        .await
                    {
                        if let Ok(message) = serde_json::from_slice::<Message>(&data) {
                            messages.push(message);
                        }
                    }
                }
            }
        }

        messages.sort_by_key(|msg| msg.created_at);
        messages.reverse();
        messages.truncate(limit);
        Ok(messages)
    }

    pub async fn get_history_entry_by_message_id(
        &self,
        message_id: &str,
    ) -> Result<Option<EncryptedHistoryEntry>> {
        let entries = self.history_entries.read().await;
        for history_entries in entries.values() {
            if let Some(entry) = history_entries
                .iter()
                .find(|entry| entry.message_id == message_id)
            {
                return Ok(Some(entry.clone()));
            }
        }
        drop(entries);

        if self.persistence_enabled {
            return self.find_history_entry_on_disk(message_id).await;
        }
        Ok(None)
    }

    /// Get direct conversations for a user
    pub async fn get_user_direct_conversations(
        &self,
        user_id: &str,
    ) -> Result<Vec<DirectConversation>> {
        let conversations = self.direct_conversations.read().await;
        let mut user_conversations = Vec::new();

        for conversation in conversations.values() {
            if conversation.has_participant(user_id) {
                user_conversations.push(conversation.clone());
            }
        }

        // Sort by last message time
        user_conversations.sort_by(|a, b| {
            let a_time = a.last_message_at.unwrap_or(a.created_at);
            let b_time = b.last_message_at.unwrap_or(b.created_at);
            b_time.cmp(&a_time) // Most recent first
        });

        Ok(user_conversations)
    }

    /// Get encrypted messages for a user
    pub async fn get_user_encrypted_messages(
        &self,
        user_id: &str,
    ) -> Result<Vec<EncryptedMessage>> {
        let encrypted_messages = self.encrypted_messages.read().await;
        Ok(encrypted_messages.get(user_id).cloned().unwrap_or_default())
    }

    /// Mark messages as read
    pub async fn mark_messages_read(&self, user_id: &str, message_ids: Vec<String>) -> Result<()> {
        // Implementation would mark messages as read in a real system
        // For now, we'll just log the action
        info!(
            "User {} marked {} messages as read",
            user_id,
            message_ids.len()
        );
        Ok(())
    }

    /// Get conversation info (group or direct)
    /// Get group memberships
    pub async fn get_group_memberships(&self, group_id: &str) -> Result<Vec<GroupMembership>> {
        let memberships = self.memberships.read().await;
        Ok(memberships.get(group_id).cloned().unwrap_or_default())
    }

    pub async fn get_conversation_info(
        &self,
        _conversation_id: &str,
        conversation_type: &ConversationType,
    ) -> Result<ConversationInfo> {
        match conversation_type {
            ConversationType::Group { group_id } => {
                let groups = self.groups.read().await;
                let group = groups
                    .get(group_id)
                    .ok_or_else(|| anyhow!("Group not found: {}", group_id))?;

                let memberships = self.memberships.read().await;
                let members = memberships.get(group_id).cloned().unwrap_or_default();

                Ok(ConversationInfo::Group {
                    group: group.clone(),
                    member_count: members.len(),
                })
            }
            ConversationType::Direct { conversation_id } => {
                let conversations = self.direct_conversations.read().await;
                let conversation = conversations
                    .get(conversation_id)
                    .ok_or_else(|| anyhow!("Direct conversation not found: {}", conversation_id))?;

                let users = self.users.read().await;
                let participant_a = users.get(&conversation.participant_a_id).cloned();
                let participant_b = users.get(&conversation.participant_b_id).cloned();

                Ok(ConversationInfo::Direct {
                    conversation: conversation.clone(),
                    participant_a,
                    participant_b,
                })
            }
        }
    }

    /// Helper to parse algorithm string
    fn parse_algorithm(&self, algo_str: &str) -> Result<Algorithm> {
        match algo_str {
            "Kyber512" => Ok(Algorithm::Kyber512),
            "Kyber768" => Ok(Algorithm::Kyber768),
            "Kyber1024" => Ok(Algorithm::Kyber1024),
            "Kyber1024" => Ok(Algorithm::Kyber1024),
            "NtruPrimeSntrup761" => Ok(Algorithm::NtruPrimeSntrup761),
            "FrodoKem1344Aes" => Ok(Algorithm::FrodoKem1344Aes),
            "FrodoKem1344Shake" => Ok(Algorithm::FrodoKem1344Shake),
            _ => Ok(Algorithm::Kyber1024), // Default fallback
        }
    }
}

/// Information about a conversation
#[derive(Debug, Clone)]
pub enum ConversationInfo {
    Group {
        group: Group,
        member_count: usize,
    },
    Direct {
        conversation: DirectConversation,
        participant_a: Option<User>,
        participant_b: Option<User>,
    },
}

/// Content filter for message processing
pub struct ContentFilter {
    /// Enable profanity filtering
    enable_profanity_filter: bool,
    /// Enable spam detection
    enable_spam_detection: bool,
}

impl ContentFilter {
    /// Create a new content filter
    pub fn new(enable_profanity_filter: bool, enable_spam_detection: bool) -> Self {
        Self {
            enable_profanity_filter,
            enable_spam_detection,
        }
    }

    /// Apply content filters to a message
    pub async fn filter_message(&self, message: &Message) -> Result<bool> {
        if self.enable_profanity_filter {
            if self.check_profanity(message).await? {
                return Ok(false);
            }
        }

        if self.enable_spam_detection {
            if self.detect_spam(message).await? {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Check for profanity in message content
    async fn check_profanity(&self, message: &Message) -> Result<bool> {
        match &message.content_type {
            MessageType::Text { content } => {
                // Simple profanity check - in reality this would be more sophisticated
                let blocked_words = ["spam", "phishing"]; // Example blocked words
                let content_lower = content.to_lowercase();

                for word in blocked_words {
                    if content_lower.contains(word) {
                        warn!("Profanity detected in message {}: {}", message.id, word);
                        return Ok(true);
                    }
                }
            }
            _ => {} // Other message types don't need profanity filtering
        }

        Ok(false)
    }

    /// Detect spam in message content
    async fn detect_spam(&self, message: &Message) -> Result<bool> {
        // Simple spam detection - check for repeated messages or suspicious patterns
        match &message.content_type {
            MessageType::Text { content } => {
                // Check for suspicious patterns
                if content.len() > 1000 {
                    warn!("Message too long, possible spam: {}", message.id);
                    return Ok(true);
                }

                // Check for excessive capitalization
                let caps_count = content.chars().filter(|c| c.is_uppercase()).count();
                let total_chars = content.chars().count();

                if total_chars > 0 && (caps_count as f64 / total_chars as f64) > 0.7 {
                    warn!("Excessive capitalization detected: {}", message.id);
                    return Ok(true);
                }
            }
            _ => {}
        }

        Ok(false)
    }
}
