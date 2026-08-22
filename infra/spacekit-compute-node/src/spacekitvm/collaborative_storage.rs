//! Collaborative Storage Features for WCVM
//!
//! This module implements advanced collaborative storage features including
//! multi-party file ownership, quantum-safe file sharing, and group access management.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};
use uuid::Uuid;

// Import storage types
use super::storage::{
    AccessControlEntry, FilePermissions, ReputationScore, StorageContractConfig,
    StorageSmartContract,
};

#[cfg(feature = "storage-integration")]
use spacekit_storage_node::database::FileMetadata;

// Import quantum security types
use crate::quantum_security::{Algorithm, QuantumResistantDID, QuantumResistantEncryption};

/// Multi-Party File Ownership
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiPartyFile {
    pub file_id: String,
    pub owners: Vec<FileOwner>,
    pub access_policy: MultiPartyAccessPolicy,
    pub encryption_keys: HashMap<String, EncryptionKeyShare>,
    pub consensus_threshold: u32,
    pub created_at: u64,
    pub last_modified: u64,
}

/// File Owner in Multi-Party System
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOwner {
    pub did: String,
    pub ownership_percentage: f64,
    pub voting_weight: u32,
    pub can_invite_others: bool,
    pub can_modify_policy: bool,
}

/// Multi-Party Access Policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiPartyAccessPolicy {
    pub policy_type: AccessPolicyType,
    pub required_approvals: u32,
    pub approval_timeout_seconds: u64,
    pub allowed_operations: Vec<CollaborativeOperation>,
}

/// Access Policy Types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccessPolicyType {
    Unanimous,      // All owners must approve
    Majority,       // More than 50% must approve
    Threshold(u32), // Specific number must approve
    Weighted(f64),  // Weighted by ownership percentage
}

/// Collaborative Operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollaborativeOperation {
    Read,
    Write,
    Share,
    AddOwner,
    RemoveOwner,
    ChangePolicy,
    Delete,
}

/// Encryption Key Share for Multi-Party Encryption
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionKeyShare {
    pub owner_did: String,
    pub key_share: Vec<u8>,
    pub threshold_index: u32,
    pub verification_hash: Vec<u8>,
}

/// Quantum-Safe Share Link
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumSafeShareLink {
    pub link_id: String,
    pub file_id: String,
    pub creator_did: String,
    pub permissions: ShareLinkPermissions,
    pub encryption_key: Vec<u8>,
    pub expiration_time: Option<u64>,
    pub max_downloads: Option<u32>,
    pub current_downloads: u32,
    pub created_at: u64,
}

/// Share Link Permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareLinkPermissions {
    pub can_read: bool,
    pub can_download: bool,
    pub can_reshare: bool,
    pub requires_authentication: bool,
    pub allowed_dids: Option<Vec<String>>,
}

/// Group Access Management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageGroup {
    pub group_id: String,
    pub group_name: String,
    pub admin_dids: Vec<String>,
    pub member_dids: Vec<String>,
    pub group_permissions: GroupPermissions,
    pub group_policy: GroupPolicy,
    pub created_at: u64,
    pub last_modified: u64,
}

/// Group Permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPermissions {
    pub can_read_all: bool,
    pub can_write_shared: bool,
    pub can_invite_members: bool,
    pub can_create_subgroups: bool,
    pub storage_quota: u64,
}

/// Group Policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupPolicy {
    pub auto_approve_members: bool,
    pub require_admin_approval: bool,
    pub max_members: Option<u32>,
    pub member_permissions: FilePermissions,
}

/// Collaborative Storage Contract
///
/// This contract implements advanced collaborative storage features including
/// multi-party file ownership and quantum-safe sharing mechanisms.
pub struct CollaborativeStorageContract {
    pub config: StorageContractConfig,
    pub multi_party_files: HashMap<String, MultiPartyFile>,
    pub share_links: HashMap<String, QuantumSafeShareLink>,
    pub storage_groups: HashMap<String, StorageGroup>,
    pub pending_approvals: HashMap<String, PendingApproval>,
    pub quantum_crypto: QuantumResistantEncryption,
}

/// Pending Approval for Collaborative Operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub approval_id: String,
    pub file_id: String,
    pub operation: CollaborativeOperation,
    pub requester_did: String,
    pub approvals: Vec<ApprovalVote>,
    pub created_at: u64,
    pub expires_at: u64,
}

/// Approval Vote
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalVote {
    pub voter_did: String,
    pub vote: bool,
    pub weight: u32,
    pub voted_at: u64,
}

impl CollaborativeStorageContract {
    /// Create a new collaborative storage contract
    pub async fn new() -> Result<Self> {
        let quantum_crypto =
            QuantumResistantEncryption::new(&Algorithm::SphincsPlus256128.to_string(), &[]).await?;

        Ok(Self {
            config: StorageContractConfig::default(),
            multi_party_files: HashMap::new(),
            share_links: HashMap::new(),
            storage_groups: HashMap::new(),
            pending_approvals: HashMap::new(),
            quantum_crypto,
        })
    }

    /// Create a multi-party file with shared ownership
    pub async fn multi_party_file_storage(
        &mut self,
        file_data: Vec<u8>,
        owners: Vec<FileOwner>,
        access_policy: MultiPartyAccessPolicy,
    ) -> Result<String> {
        info!("Creating multi-party file with {} owners", owners.len());

        // Validate owners
        if owners.is_empty() {
            return Err(anyhow::anyhow!("At least one owner required"));
        }

        // Validate ownership percentages sum to 100%
        let total_ownership: f64 = owners.iter().map(|o| o.ownership_percentage).sum();
        if (total_ownership - 100.0).abs() > 0.01 {
            return Err(anyhow::anyhow!("Ownership percentages must sum to 100%"));
        }

        let file_id = format!("multiparty_{}", Uuid::new_v4());

        // Generate encryption key shares using threshold cryptography
        let encryption_keys = self.generate_threshold_encryption_keys(&owners).await?;

        // Encrypt the file data
        let encrypted_data = self
            .encrypt_with_threshold_keys(&file_data, &encryption_keys)
            .await?;

        // Store the encrypted file (in production, this would use the storage backend)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let multi_party_file = MultiPartyFile {
            file_id: file_id.clone(),
            owners,
            access_policy,
            encryption_keys,
            consensus_threshold: 2, // Minimum 2 approvals for operations
            created_at: now,
            last_modified: now,
        };

        self.multi_party_files
            .insert(file_id.clone(), multi_party_file);

        info!("Multi-party file created: {}", file_id);
        Ok(file_id)
    }

    /// Create a quantum-safe share link
    pub async fn create_quantum_safe_share_link(
        &mut self,
        file_id: &str,
        creator_did: &str,
        permissions: ShareLinkPermissions,
        expiration_hours: Option<u64>,
        max_downloads: Option<u32>,
    ) -> Result<String> {
        info!("Creating quantum-safe share link for file: {}", file_id);

        // Verify creator has permission to share the file
        if !self.can_share_file(file_id, creator_did).await? {
            return Err(anyhow::anyhow!(
                "Creator does not have permission to share this file"
            ));
        }

        let link_id = format!("share_{}", Uuid::new_v4());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Generate quantum-safe encryption key for the share link
        let encryption_key = self.generate_share_link_key().await?;

        let share_link = QuantumSafeShareLink {
            link_id: link_id.clone(),
            file_id: file_id.to_string(),
            creator_did: creator_did.to_string(),
            permissions,
            encryption_key,
            expiration_time: expiration_hours.map(|h| now + (h * 3600)),
            max_downloads,
            current_downloads: 0,
            created_at: now,
        };

        self.share_links.insert(link_id.clone(), share_link);

        info!("Quantum-safe share link created: {}", link_id);
        Ok(link_id)
    }

    /// Create collaborative storage policy for groups
    pub async fn collaborative_storage_policy(
        &mut self,
        group_name: String,
        admin_dids: Vec<String>,
        group_permissions: GroupPermissions,
        group_policy: GroupPolicy,
    ) -> Result<String> {
        info!(
            "Creating collaborative storage policy for group: {}",
            group_name
        );

        if admin_dids.is_empty() {
            return Err(anyhow::anyhow!("At least one admin required"));
        }

        let group_id = format!("group_{}", Uuid::new_v4());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let storage_group = StorageGroup {
            group_id: group_id.clone(),
            group_name,
            admin_dids,
            member_dids: Vec::new(),
            group_permissions,
            group_policy,
            created_at: now,
            last_modified: now,
        };

        self.storage_groups.insert(group_id.clone(), storage_group);

        info!("Collaborative storage group created: {}", group_id);
        Ok(group_id)
    }

    /// Request approval for a collaborative operation
    pub async fn request_collaborative_approval(
        &mut self,
        file_id: &str,
        operation: CollaborativeOperation,
        requester_did: &str,
    ) -> Result<String> {
        debug!(
            "Requesting approval for {:?} operation on file: {}",
            operation, file_id
        );

        let multi_party_file = self
            .multi_party_files
            .get(file_id)
            .ok_or_else(|| anyhow::anyhow!("Multi-party file not found"))?;

        // Check if requester is an owner
        if !multi_party_file
            .owners
            .iter()
            .any(|o| o.did == requester_did)
        {
            return Err(anyhow::anyhow!("Only owners can request approvals"));
        }

        let approval_id = format!("approval_{}", Uuid::new_v4());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let pending_approval = PendingApproval {
            approval_id: approval_id.clone(),
            file_id: file_id.to_string(),
            operation,
            requester_did: requester_did.to_string(),
            approvals: Vec::new(),
            created_at: now,
            expires_at: now + multi_party_file.access_policy.approval_timeout_seconds,
        };

        self.pending_approvals
            .insert(approval_id.clone(), pending_approval);

        info!("Collaborative approval requested: {}", approval_id);
        Ok(approval_id)
    }

    /// Vote on a pending approval
    pub async fn vote_on_approval(
        &mut self,
        approval_id: &str,
        voter_did: &str,
        vote: bool,
    ) -> Result<bool> {
        debug!("Voting on approval: {} by {}", approval_id, voter_did);

        let pending_approval = self
            .pending_approvals
            .get_mut(approval_id)
            .ok_or_else(|| anyhow::anyhow!("Pending approval not found"))?;

        let multi_party_file = self
            .multi_party_files
            .get(&pending_approval.file_id)
            .ok_or_else(|| anyhow::anyhow!("Multi-party file not found"))?;

        // Check if voter is an owner
        let owner = multi_party_file
            .owners
            .iter()
            .find(|o| o.did == voter_did)
            .ok_or_else(|| anyhow::anyhow!("Only owners can vote"))?;

        // Check if already voted
        if pending_approval
            .approvals
            .iter()
            .any(|a| a.voter_did == voter_did)
        {
            return Err(anyhow::anyhow!("Already voted on this approval"));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Check if expired
        if now > pending_approval.expires_at {
            return Err(anyhow::anyhow!("Approval has expired"));
        }

        let approval_vote = ApprovalVote {
            voter_did: voter_did.to_string(),
            vote,
            weight: owner.voting_weight,
            voted_at: now,
        };

        pending_approval.approvals.push(approval_vote);

        // Check if approval threshold is met
        let approval_met = self.is_approval_threshold_met(approval_id)?;

        if approval_met {
            info!("Approval threshold met for: {}", approval_id);
            // Execute the approved operation
            self.execute_approved_operation(approval_id).await?;
        }

        Ok(approval_met)
    }

    /// Access file through quantum-safe share link
    pub async fn access_shared_file(
        &self,
        link_id: &str,
        accessor_did: Option<&str>,
    ) -> Result<Vec<u8>> {
        debug!("Accessing shared file through link: {}", link_id);

        let share_link = self
            .share_links
            .get(link_id)
            .ok_or_else(|| anyhow::anyhow!("Share link not found"))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Check expiration
        if let Some(expiration) = share_link.expiration_time {
            if now > expiration {
                return Err(anyhow::anyhow!("Share link has expired"));
            }
        }

        // Check download limit
        if let Some(max_downloads) = share_link.max_downloads {
            if share_link.current_downloads >= max_downloads {
                return Err(anyhow::anyhow!("Download limit exceeded"));
            }
        }

        // Check DID permissions
        if share_link.permissions.requires_authentication {
            let accessor_did =
                accessor_did.ok_or_else(|| anyhow::anyhow!("Authentication required"))?;

            if let Some(allowed_dids) = &share_link.permissions.allowed_dids {
                if !allowed_dids.contains(&accessor_did.to_string()) {
                    return Err(anyhow::anyhow!("DID not authorized for this share link"));
                }
            }
        }

        // Retrieve and decrypt file
        let encrypted_file_data = self.retrieve_encrypted_file(&share_link.file_id).await?;
        let decrypted_data = self
            .decrypt_with_share_key(&encrypted_file_data, &share_link.encryption_key)
            .await?;

        // Note: In a real implementation, you would increment download counter here
        // This would require either RefCell/Mutex or returning the incremented counter
        // to be updated by the caller

        info!("File accessed through share link: {}", link_id);
        Ok(decrypted_data)
    }

    /// Increment share link download counter
    pub fn increment_share_link_downloads(&mut self, link_id: &str) -> Result<()> {
        if let Some(share_link) = self.share_links.get_mut(link_id) {
            share_link.current_downloads += 1;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Share link not found"))
        }
    }

    // Private helper methods
    async fn generate_threshold_encryption_keys(
        &self,
        owners: &[FileOwner],
    ) -> Result<HashMap<String, EncryptionKeyShare>> {
        let mut keys = HashMap::new();

        // Generate threshold encryption keys (simplified implementation)
        for (index, owner) in owners.iter().enumerate() {
            let key_share = EncryptionKeyShare {
                owner_did: owner.did.clone(),
                key_share: vec![index as u8; 32], // Placeholder key share
                threshold_index: index as u32,
                verification_hash: vec![0u8; 32], // Placeholder hash
            };
            keys.insert(owner.did.clone(), key_share);
        }

        Ok(keys)
    }

    async fn encrypt_with_threshold_keys(
        &self,
        data: &[u8],
        _keys: &HashMap<String, EncryptionKeyShare>,
    ) -> Result<Vec<u8>> {
        // Placeholder implementation - in production, use threshold cryptography
        Ok(data.to_vec())
    }

    async fn can_share_file(&self, file_id: &str, did: &str) -> Result<bool> {
        if let Some(multi_party_file) = self.multi_party_files.get(file_id) {
            Ok(multi_party_file.owners.iter().any(|o| o.did == did))
        } else {
            // Check regular file permissions (would integrate with other storage contracts)
            Ok(true) // Placeholder
        }
    }

    async fn generate_share_link_key(&self) -> Result<Vec<u8>> {
        // Generate quantum-safe encryption key for share link
        Ok(vec![0u8; 32]) // Placeholder implementation
    }

    async fn retrieve_encrypted_file(&self, _file_id: &str) -> Result<Vec<u8>> {
        // Placeholder implementation
        Ok(vec![0u8; 1024])
    }

    async fn decrypt_with_share_key(&self, data: &[u8], _key: &[u8]) -> Result<Vec<u8>> {
        // Placeholder implementation
        Ok(data.to_vec())
    }

    fn is_approval_threshold_met(&self, approval_id: &str) -> Result<bool> {
        let pending_approval = self
            .pending_approvals
            .get(approval_id)
            .ok_or_else(|| anyhow::anyhow!("Pending approval not found"))?;

        let multi_party_file = self
            .multi_party_files
            .get(&pending_approval.file_id)
            .ok_or_else(|| anyhow::anyhow!("Multi-party file not found"))?;

        let positive_votes: u32 = pending_approval
            .approvals
            .iter()
            .filter(|v| v.vote)
            .map(|v| v.weight)
            .sum();

        let threshold_met = match &multi_party_file.access_policy.policy_type {
            AccessPolicyType::Unanimous => {
                positive_votes
                    == multi_party_file
                        .owners
                        .iter()
                        .map(|o| o.voting_weight)
                        .sum::<u32>()
            }
            AccessPolicyType::Majority => {
                let total_weight: u32 = multi_party_file
                    .owners
                    .iter()
                    .map(|o| o.voting_weight)
                    .sum();
                positive_votes > total_weight / 2
            }
            AccessPolicyType::Threshold(threshold) => positive_votes >= *threshold,
            AccessPolicyType::Weighted(threshold) => {
                let total_ownership: f64 = multi_party_file
                    .owners
                    .iter()
                    .filter(|o| {
                        pending_approval
                            .approvals
                            .iter()
                            .any(|a| a.vote && a.voter_did == o.did)
                    })
                    .map(|o| o.ownership_percentage)
                    .sum();
                total_ownership >= *threshold
            }
        };

        Ok(threshold_met)
    }

    async fn execute_approved_operation(&mut self, approval_id: &str) -> Result<()> {
        let pending_approval = self
            .pending_approvals
            .remove(approval_id)
            .ok_or_else(|| anyhow::anyhow!("Pending approval not found"))?;

        match pending_approval.operation {
            CollaborativeOperation::Read => {
                // Grant read access
                info!(
                    "Approved read access for file: {}",
                    pending_approval.file_id
                );
            }
            CollaborativeOperation::Write => {
                // Grant write access
                info!(
                    "Approved write access for file: {}",
                    pending_approval.file_id
                );
            }
            CollaborativeOperation::Share => {
                // Allow sharing
                info!("Approved sharing for file: {}", pending_approval.file_id);
            }
            CollaborativeOperation::AddOwner => {
                // Add new owner
                info!(
                    "Approved adding owner for file: {}",
                    pending_approval.file_id
                );
            }
            CollaborativeOperation::RemoveOwner => {
                // Remove owner
                info!(
                    "Approved removing owner for file: {}",
                    pending_approval.file_id
                );
            }
            CollaborativeOperation::ChangePolicy => {
                // Change access policy
                info!(
                    "Approved policy change for file: {}",
                    pending_approval.file_id
                );
            }
            CollaborativeOperation::Delete => {
                // Delete file
                info!("Approved deletion for file: {}", pending_approval.file_id);
                self.multi_party_files.remove(&pending_approval.file_id);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_collaborative_storage_creation() {
        let contract = CollaborativeStorageContract::new().await;
        assert!(contract.is_ok());
    }

    #[tokio::test]
    async fn test_multi_party_file_storage() {
        let mut contract = CollaborativeStorageContract::new().await.unwrap();

        let owners = vec![
            FileOwner {
                did: "did:swtch:user1".to_string(),
                ownership_percentage: 60.0,
                voting_weight: 2,
                can_invite_others: true,
                can_modify_policy: true,
            },
            FileOwner {
                did: "did:swtch:user2".to_string(),
                ownership_percentage: 40.0,
                voting_weight: 1,
                can_invite_others: false,
                can_modify_policy: false,
            },
        ];

        let access_policy = MultiPartyAccessPolicy {
            policy_type: AccessPolicyType::Majority,
            required_approvals: 2,
            approval_timeout_seconds: 3600,
            allowed_operations: vec![CollaborativeOperation::Read, CollaborativeOperation::Write],
        };

        let file_data = b"Multi-party collaborative file content".to_vec();

        let file_id = contract
            .multi_party_file_storage(file_data, owners, access_policy)
            .await
            .unwrap();
        assert!(file_id.starts_with("multiparty_"));
        assert!(contract.multi_party_files.contains_key(&file_id));
    }

    #[tokio::test]
    async fn test_quantum_safe_share_link() {
        let mut contract = CollaborativeStorageContract::new().await.unwrap();

        // First create a multi-party file
        let owners = vec![FileOwner {
            did: "did:swtch:owner".to_string(),
            ownership_percentage: 100.0,
            voting_weight: 1,
            can_invite_others: true,
            can_modify_policy: true,
        }];

        let access_policy = MultiPartyAccessPolicy {
            policy_type: AccessPolicyType::Unanimous,
            required_approvals: 1,
            approval_timeout_seconds: 3600,
            allowed_operations: vec![CollaborativeOperation::Share],
        };

        let file_id = contract
            .multi_party_file_storage(b"Test file for sharing".to_vec(), owners, access_policy)
            .await
            .unwrap();

        // Create share link
        let permissions = ShareLinkPermissions {
            can_read: true,
            can_download: true,
            can_reshare: false,
            requires_authentication: false,
            allowed_dids: None,
        };

        let link_id = contract
            .create_quantum_safe_share_link(
                &file_id,
                "did:swtch:owner",
                permissions,
                Some(24), // 24 hours
                Some(10), // Max 10 downloads
            )
            .await
            .unwrap();

        assert!(link_id.starts_with("share_"));
        assert!(contract.share_links.contains_key(&link_id));
    }

    #[tokio::test]
    async fn test_collaborative_storage_policy() {
        let mut contract = CollaborativeStorageContract::new().await.unwrap();

        let admin_dids = vec![
            "did:swtch:admin1".to_string(),
            "did:swtch:admin2".to_string(),
        ];

        let group_permissions = GroupPermissions {
            can_read_all: true,
            can_write_shared: true,
            can_invite_members: false,
            can_create_subgroups: false,
            storage_quota: 10 * 1024 * 1024 * 1024, // 10GB
        };

        let group_policy = GroupPolicy {
            auto_approve_members: false,
            require_admin_approval: true,
            max_members: Some(50),
            member_permissions: FilePermissions::ReadWrite,
        };

        let group_id = contract
            .collaborative_storage_policy(
                "Research Team".to_string(),
                admin_dids,
                group_permissions,
                group_policy,
            )
            .await
            .unwrap();

        assert!(group_id.starts_with("group_"));
        assert!(contract.storage_groups.contains_key(&group_id));
    }

    #[tokio::test]
    async fn test_approval_workflow() {
        let mut contract = CollaborativeStorageContract::new().await.unwrap();

        // Create multi-party file with two owners
        let owners = vec![
            FileOwner {
                did: "did:swtch:user1".to_string(),
                ownership_percentage: 60.0,
                voting_weight: 2,
                can_invite_others: true,
                can_modify_policy: true,
            },
            FileOwner {
                did: "did:swtch:user2".to_string(),
                ownership_percentage: 40.0,
                voting_weight: 1,
                can_invite_others: false,
                can_modify_policy: false,
            },
        ];

        let access_policy = MultiPartyAccessPolicy {
            policy_type: AccessPolicyType::Threshold(3), // Require more than user1's weight (2)
            required_approvals: 2,
            approval_timeout_seconds: 3600,
            allowed_operations: vec![CollaborativeOperation::Write],
        };

        let file_id = contract
            .multi_party_file_storage(b"File requiring approval".to_vec(), owners, access_policy)
            .await
            .unwrap();

        // Request approval
        let approval_id = contract
            .request_collaborative_approval(
                &file_id,
                CollaborativeOperation::Write,
                "did:swtch:user1",
            )
            .await
            .unwrap();

        // Vote on approval
        let vote1 = contract
            .vote_on_approval(&approval_id, "did:swtch:user1", true)
            .await
            .unwrap();
        assert!(!vote1); // Should not be approved yet

        let vote2 = contract
            .vote_on_approval(&approval_id, "did:swtch:user2", true)
            .await
            .unwrap();
        assert!(vote2); // Should be approved now (threshold of 2 met with weights 2+1=3)

        // Approval should be removed after execution
        assert!(!contract.pending_approvals.contains_key(&approval_id));
    }
}
