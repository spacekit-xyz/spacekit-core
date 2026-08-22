use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// User identity in the messaging system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub did: String, // Decentralized Identifier
    pub username: String,
    pub public_key: Vec<u8>, // Quantum-resistant public key
    pub encryption_algorithm: String,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
}

/// Directory entry for user discovery (privacy-preserving)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub did: String,
    pub username: String,
    pub encryption_algorithm: String,
    pub public_key: Option<Vec<u8>>,
}

/// Group in the messaging system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub creator_id: String, // User ID of the group creator
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_private: bool,
    pub max_members: Option<usize>,
}

/// Direct conversation between two users
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectConversation {
    pub id: String,
    pub participant_a_id: String,
    pub participant_b_id: String,
    pub created_at: DateTime<Utc>,
    pub last_message_at: Option<DateTime<Utc>>,
    pub is_active: bool,
}

/// Group membership information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupMembership {
    pub group_id: String,
    pub user_id: String,
    pub role: MemberRole,
    pub joined_at: DateTime<Utc>,
    pub invited_by: Option<String>,
}

/// Member roles in a group
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MemberRole {
    Creator,
    Admin,
    Member,
}

/// Message in the system (supports both group and direct messages)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub conversation_type: ConversationType,
    pub sender_id: String,
    pub content_type: MessageType,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub reply_to: Option<String>, // Message ID being replied to
}

/// Type of conversation (group or direct)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConversationType {
    Group { group_id: String },
    Direct { conversation_id: String },
}

/// Types of message content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    Text {
        content: String,
    },
    File {
        file_id: String,
        filename: String,
        size: u64,
    },
    Image {
        file_id: String,
        filename: String,
        size: u64,
        width: Option<u32>,
        height: Option<u32>,
    },
    System {
        content: String,
    }, // System messages like "User joined"
}

/// Encrypted message for individual recipients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedMessage {
    pub message_id: String,
    pub recipient_id: String,
    pub encrypted_content: Vec<u8>, // Asymmetrically encrypted for this specific recipient
    pub kem_ciphertext: Vec<u8>,    // Key encapsulation ciphertext
    pub algorithm: String,          // Quantum-resistant algorithm used
    pub nonce: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

/// File stored in the system (now supports both group and direct files)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFile {
    pub id: String,
    pub conversation_type: ConversationType,
    pub uploader_id: String,
    pub filename: String,
    pub original_size: u64,
    pub encrypted_size: u64,
    pub mime_type: String,
    pub file_hash: String, // Hash of original file for integrity
    pub encryption_algorithm: String,
    pub upload_path: String, // Path where encrypted file is stored
    pub status: FileStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Legacy GroupFile for backward compatibility
pub type GroupFile = SharedFile;

/// Status of files in the system
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileStatus {
    Active,
    Archived,
    Deleted,
}

/// File access record for auditing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAccess {
    pub file_id: String,
    pub user_id: String,
    pub access_type: FileAccessType,
    pub accessed_at: DateTime<Utc>,
    pub ip_address: Option<String>,
}

/// Types of file access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileAccessType {
    Download,
    View,
    Share,
}

/// Group invitation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupInvitation {
    pub id: String,
    pub group_id: String,
    pub inviter_id: String,
    pub invitee_did: String, // DID of person being invited
    pub message: Option<String>,
    pub status: InvitationStatus,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub responded_at: Option<DateTime<Utc>>,
}

/// Status of group invitations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Rejected,
    Expired,
}

/// Configuration for quantum-resistant encryption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    pub default_algorithm: String,
    pub supported_algorithms: Vec<String>,
    pub key_rotation_interval: Option<chrono::Duration>,
}

impl User {
    pub fn new(did: String, username: String, public_key: Vec<u8>, algorithm: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            did,
            username,
            public_key,
            encryption_algorithm: algorithm,
            created_at: Utc::now(),
            last_seen: None,
        }
    }
}

impl Group {
    pub fn new(name: String, creator_id: String, description: Option<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            description,
            creator_id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            is_private: false,
            max_members: None,
        }
    }
}

impl DirectConversation {
    pub fn new(participant_a_id: String, participant_b_id: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            participant_a_id,
            participant_b_id,
            created_at: Utc::now(),
            last_message_at: None,
            is_active: true,
        }
    }

    /// Check if a user is a participant in this conversation
    pub fn has_participant(&self, user_id: &str) -> bool {
        self.participant_a_id == user_id || self.participant_b_id == user_id
    }

    /// Get the other participant's ID
    pub fn get_other_participant(&self, user_id: &str) -> Option<&String> {
        if self.participant_a_id == user_id {
            Some(&self.participant_b_id)
        } else if self.participant_b_id == user_id {
            Some(&self.participant_a_id)
        } else {
            None
        }
    }
}

impl Message {
    pub fn new_text_group(group_id: String, sender_id: String, content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_type: ConversationType::Group { group_id },
            sender_id,
            content_type: MessageType::Text { content },
            created_at: Utc::now(),
            updated_at: None,
            reply_to: None,
        }
    }

    pub fn new_text_direct(conversation_id: String, sender_id: String, content: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_type: ConversationType::Direct { conversation_id },
            sender_id,
            content_type: MessageType::Text { content },
            created_at: Utc::now(),
            updated_at: None,
            reply_to: None,
        }
    }

    pub fn new_file_group(
        group_id: String,
        sender_id: String,
        file_id: String,
        filename: String,
        size: u64,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_type: ConversationType::Group { group_id },
            sender_id,
            content_type: MessageType::File {
                file_id,
                filename,
                size,
            },
            created_at: Utc::now(),
            updated_at: None,
            reply_to: None,
        }
    }

    pub fn new_file_direct(
        conversation_id: String,
        sender_id: String,
        file_id: String,
        filename: String,
        size: u64,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_type: ConversationType::Direct { conversation_id },
            sender_id,
            content_type: MessageType::File {
                file_id,
                filename,
                size,
            },
            created_at: Utc::now(),
            updated_at: None,
            reply_to: None,
        }
    }
}

impl SharedFile {
    pub fn new_group(
        group_id: String,
        uploader_id: String,
        filename: String,
        original_size: u64,
        encrypted_size: u64,
        mime_type: String,
        file_hash: String,
        encryption_algorithm: String,
        upload_path: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_type: ConversationType::Group { group_id },
            uploader_id,
            filename,
            original_size,
            encrypted_size,
            mime_type,
            file_hash,
            encryption_algorithm,
            upload_path,
            status: FileStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            expires_at: None,
        }
    }

    pub fn new_direct(
        conversation_id: String,
        uploader_id: String,
        filename: String,
        original_size: u64,
        encrypted_size: u64,
        mime_type: String,
        file_hash: String,
        encryption_algorithm: String,
        upload_path: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            conversation_type: ConversationType::Direct { conversation_id },
            uploader_id,
            filename,
            original_size,
            encrypted_size,
            mime_type,
            file_hash,
            encryption_algorithm,
            upload_path,
            status: FileStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            expires_at: None,
        }
    }
}
