//! Access Control and Permissions for Messaging Nodes
//!
//! Implements DID-based permissions, reputation scoring, and access lists
//! for both public and private messaging nodes.
//!
//! Integrates with swtchx-primitives reputation system for unified reputation
//! across all SWTCHX services (messaging, compute, storage, etc.)

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

// Import primitives reputation system
use alloy_primitives::{Address, U256};
use spacekit_primitives::v1::behavioral_types::ServiceType;
use spacekit_primitives::v1::reputation::{
    BehavioralReputationScore, ParticipantScore, ReputationAction, ReputationProfile,
    ReputationScore as PrimitiveReputationScore, ServiceParticipation,
};

/// Access control manager for messaging nodes
pub struct AccessControlManager {
    /// Node type (public or private)
    node_type: NodeType,
    /// Whitelist of allowed DIDs (for private nodes)
    whitelist: Arc<RwLock<HashSet<String>>>,
    /// Blacklist of banned DIDs (for all nodes)
    blacklist: Arc<RwLock<HashSet<String>>>,
    /// User permissions
    permissions: Arc<RwLock<HashMap<String, UserPermissions>>>,
    /// Reputation profiles (using swtchx-primitives)
    reputation_profiles: Arc<RwLock<HashMap<String, ReputationProfile>>>,
    /// Access policies
    policies: Arc<RwLock<AccessPolicies>>,
    /// DID to Address mapping (for primitives compatibility)
    did_to_address: Arc<RwLock<HashMap<String, Address>>>,
}

/// Type of messaging node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    /// Public node - anyone can join
    Public,
    /// Private node - whitelist only
    Private,
    /// Invite-only - requires invitation
    InviteOnly,
}

/// User permissions for a messaging node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPermissions {
    pub did: String,
    pub role: UserRole,
    pub can_send_messages: bool,
    pub can_create_groups: bool,
    pub can_invite_users: bool,
    pub can_moderate: bool,
    pub can_admin: bool,
    pub granted_at: DateTime<Utc>,
    pub granted_by: Option<String>, // DID of granter
    pub expires_at: Option<DateTime<Utc>>,
}

/// User roles in the messaging network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum UserRole {
    Banned = 0,
    Guest = 1,
    Member = 2,
    Trusted = 3,
    Moderator = 4,
    Admin = 5,
    Owner = 6,
}

/// Simplified reputation view for messaging context
/// (Wraps swtchx-primitives ReputationProfile)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingReputationView {
    pub did: String,
    pub score: i64, // Normalized from primitives (0-1000 range)
    pub total_messages: u64,
    pub spam_reports: u32,
    pub helpful_votes: u32,
    pub violations: Vec<Violation>,
    pub last_updated: DateTime<Utc>,
    pub behavioral_score: Option<f64>, // From primitives behavioral analysis
    pub service_quality: f64,          // From ServiceParticipation::Messaging
    pub consistency_score: f64,        // From primitives
}

/// Violation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Violation {
    pub violation_type: ViolationType,
    pub description: String,
    pub timestamp: DateTime<Utc>,
    pub reported_by: String, // DID of reporter
    pub severity: ViolationSeverity,
}

/// Types of violations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ViolationType {
    Spam,
    Harassment,
    MaliciousContent,
    RateLimitExceeded,
    UnauthorizedAccess,
    DataBreach,
    PolicyViolation,
}

/// Severity of violations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ViolationSeverity {
    Low,      // Warning
    Medium,   // Temporary restriction
    High,     // Suspension
    Critical, // Permanent ban
}

/// Access policies for the node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessPolicies {
    /// Minimum reputation to join
    pub min_reputation: i64,
    /// Maximum messages per minute
    pub rate_limit_per_minute: u32,
    /// Require invitation for private nodes
    pub require_invitation: bool,
    /// Auto-ban threshold (violations)
    pub auto_ban_threshold: u32,
    /// Reputation decay (points per day of inactivity)
    pub reputation_decay_per_day: i64,
    /// Allow anonymous users (no DID verification)
    pub allow_anonymous: bool,
    /// Require staking (future integration with swtchx-staking)
    pub require_stake: bool,
    pub minimum_stake_amount: u128,
}

impl Default for AccessPolicies {
    fn default() -> Self {
        Self {
            min_reputation: 0,
            rate_limit_per_minute: 60,
            require_invitation: false,
            auto_ban_threshold: 5,
            reputation_decay_per_day: 1,
            allow_anonymous: false,
            require_stake: false,
            minimum_stake_amount: 0,
        }
    }
}

impl AccessControlManager {
    /// Create a new access control manager
    pub fn new(node_type: NodeType) -> Self {
        Self {
            node_type,
            whitelist: Arc::new(RwLock::new(HashSet::new())),
            blacklist: Arc::new(RwLock::new(HashSet::new())),
            permissions: Arc::new(RwLock::new(HashMap::new())),
            reputation_profiles: Arc::new(RwLock::new(HashMap::new())),
            policies: Arc::new(RwLock::new(AccessPolicies::default())),
            did_to_address: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if a DID has access to the node
    pub async fn has_access(&self, did: &str) -> Result<bool> {
        // Check blacklist first (always denied)
        {
            let blacklist = self.blacklist.read().await;
            if blacklist.contains(did) {
                return Ok(false);
            }
        }

        // Check based on node type
        match self.node_type {
            NodeType::Public => {
                // Public nodes: check reputation
                let score = self.get_reputation_score(did).await?;
                let policies = self.policies.read().await;
                Ok(score >= policies.min_reputation)
            }
            NodeType::Private => {
                // Private nodes: must be whitelisted
                let whitelist = self.whitelist.read().await;
                Ok(whitelist.contains(did))
            }
            NodeType::InviteOnly => {
                // Invite-only: check permissions
                let permissions = self.permissions.read().await;
                Ok(permissions.contains_key(did))
            }
        }
    }

    /// Add DID to whitelist
    pub async fn add_to_whitelist(&self, did: String) -> Result<()> {
        let mut whitelist = self.whitelist.write().await;
        whitelist.insert(did.clone());
        println!("✅ Added {} to whitelist", did);
        Ok(())
    }

    /// Remove DID from whitelist
    pub async fn remove_from_whitelist(&self, did: &str) -> Result<()> {
        let mut whitelist = self.whitelist.write().await;
        whitelist.remove(did);
        println!("🚫 Removed {} from whitelist", did);
        Ok(())
    }

    /// Add DID to blacklist (ban)
    pub async fn ban_user(&self, did: String, reason: String) -> Result<()> {
        let mut blacklist = self.blacklist.write().await;
        blacklist.insert(did.clone());
        drop(blacklist); // Release lock before violation record

        // Update reputation directly to avoid recursion
        let mut profiles = self.reputation_profiles.write().await;
        if let Some(profile) = profiles.get_mut(&did) {
            // Set messaging quality to 0 (banned)
            if let Some(messaging) = profile
                .participant_score
                .service_participation
                .iter_mut()
                .find(|s| matches!(s.service_type, ServiceType::Messaging))
            {
                messaging.quality_score = 0.0;
                messaging.success_rate = 0.0;
            }
            profile.participant_score.updated_at = Utc::now();
        }

        println!("🚫 Banned user {} - Reason: {}", did, reason);
        Ok(())
    }

    /// Remove DID from blacklist (unban)
    pub async fn unban_user(&self, did: &str) -> Result<()> {
        let mut blacklist = self.blacklist.write().await;
        blacklist.remove(did);
        println!("✅ Unbanned user {}", did);
        Ok(())
    }

    /// Check if DID is blacklisted
    pub async fn is_blacklisted(&self, did: &str) -> bool {
        let blacklist = self.blacklist.read().await;
        blacklist.contains(did)
    }

    /// Check if DID is whitelisted
    pub async fn is_whitelisted(&self, did: &str) -> bool {
        let whitelist = self.whitelist.read().await;
        whitelist.contains(did)
    }

    /// Grant permissions to a user
    pub async fn grant_permissions(
        &self,
        did: String,
        role: UserRole,
        granted_by: Option<String>,
    ) -> Result<()> {
        let permissions = UserPermissions {
            did: did.clone(),
            role: role.clone(),
            can_send_messages: role >= UserRole::Member,
            can_create_groups: role >= UserRole::Member,
            can_invite_users: role >= UserRole::Trusted,
            can_moderate: role >= UserRole::Moderator,
            can_admin: role >= UserRole::Admin,
            granted_at: Utc::now(),
            granted_by,
            expires_at: None,
        };

        let mut perms = self.permissions.write().await;
        perms.insert(did.clone(), permissions);

        println!("✅ Granted {:?} role to {}", role, did);
        Ok(())
    }

    /// Revoke permissions
    pub async fn revoke_permissions(&self, did: &str) -> Result<()> {
        let mut permissions = self.permissions.write().await;
        permissions.remove(did);
        println!("🚫 Revoked permissions for {}", did);
        Ok(())
    }

    /// Get user permissions
    pub async fn get_permissions(&self, did: &str) -> Option<UserPermissions> {
        let permissions = self.permissions.read().await;
        permissions.get(did).cloned()
    }

    /// Check if user can perform action
    pub async fn can_perform(&self, did: &str, action: Action) -> Result<bool> {
        // Always deny blacklisted users
        if self.is_blacklisted(did).await {
            return Ok(false);
        }

        let permissions = self.permissions.read().await;

        if let Some(perms) = permissions.get(did) {
            // Check if permissions expired
            if let Some(expires_at) = perms.expires_at {
                if Utc::now() > expires_at {
                    return Ok(false);
                }
            }

            // Check specific action permission
            let allowed = match action {
                Action::SendMessage => perms.can_send_messages,
                Action::CreateGroup => perms.can_create_groups,
                Action::InviteUser => perms.can_invite_users,
                Action::Moderate => perms.can_moderate,
                Action::Admin => perms.can_admin,
            };

            Ok(allowed)
        } else {
            // No explicit permissions - check node type
            match self.node_type {
                NodeType::Public => Ok(true), // Public allows basic actions
                NodeType::Private | NodeType::InviteOnly => Ok(false),
            }
        }
    }

    /// Get or create reputation profile (using primitives)
    pub async fn get_or_create_profile(&self, did: &str) -> Result<ReputationProfile> {
        let mut profiles = self.reputation_profiles.write().await;

        if let Some(profile) = profiles.get(did) {
            Ok(profile.clone())
        } else {
            // Create new reputation profile using primitives
            let address = self.did_to_address_internal(did).await;

            let profile = ReputationProfile {
                address,
                participant_score: ParticipantScore {
                    as_consumer: PrimitiveReputationScore {
                        score: U256::from(0),
                        total_actions: U256::from(0),
                        successful_actions: U256::from(0),
                    },
                    as_producer: PrimitiveReputationScore {
                        score: U256::from(0),
                        total_actions: U256::from(0),
                        successful_actions: U256::from(0),
                    },
                    product_scores: Vec::new(),
                    actions: Vec::new(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    behavioral_consistency: None,
                    interaction_style: None,
                    service_participation: vec![ServiceParticipation::new(ServiceType::Messaging)],
                    network_metrics: None,
                },
                eth_escrow_balance: U256::from(0),
                behavioral_score: None,
                archetype_classification: None,
                behavioral_fingerprint: None,
                confidence_score: None,
            };

            profiles.insert(did.to_string(), profile.clone());
            Ok(profile)
        }
    }

    /// Get reputation score as i64 (for backward compatibility)
    pub async fn get_reputation_score(&self, did: &str) -> Result<i64> {
        let profile = self.get_or_create_profile(did).await?;

        // Get messaging service participation
        if let Some(messaging) = profile
            .participant_score
            .service_participation
            .iter()
            .find(|s| matches!(s.service_type, ServiceType::Messaging))
        {
            // Convert quality_score (0.0-1.0) to -1000 to +1000 range
            let base_score = (messaging.quality_score * 1000.0) as i64;

            // Apply consistency bonus
            let consistency_bonus = (messaging.consistency_score * 200.0) as i64;

            Ok(base_score + consistency_bonus - 500) // Center around 0
        } else {
            Ok(0) // Neutral
        }
    }

    /// Get simplified view for messaging (backward compatibility)
    pub async fn get_reputation(&self, did: &str) -> Result<MessagingReputationView> {
        let profile = self.get_or_create_profile(did).await?;
        let score = self.get_reputation_score(did).await?;

        let messaging_service = profile
            .participant_score
            .service_participation
            .iter()
            .find(|s| matches!(s.service_type, ServiceType::Messaging));

        Ok(MessagingReputationView {
            did: did.to_string(),
            score,
            total_messages: messaging_service.map(|s| s.total_actions).unwrap_or(0),
            spam_reports: 0,        // TODO: Track separately
            helpful_votes: 0,       // TODO: Track separately
            violations: Vec::new(), // TODO: Track separately
            last_updated: profile.participant_score.updated_at,
            behavioral_score: profile
                .behavioral_score
                .as_ref()
                .map(|bs| bs.overall_behavioral_score),
            service_quality: messaging_service.map(|s| s.quality_score).unwrap_or(0.5),
            consistency_score: messaging_service
                .map(|s| s.consistency_score)
                .unwrap_or(0.5),
        })
    }

    /// Internal DID to Address conversion
    async fn did_to_address_internal(&self, did: &str) -> Address {
        // Check if we have a mapping
        {
            let mapping = self.did_to_address.read().await;
            if let Some(addr) = mapping.get(did) {
                return *addr;
            }
        }

        // Generate deterministic address from DID
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(did.as_bytes());
        let hash = hasher.finalize();

        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&hash[..20]);
        let address = Address::from(addr_bytes);

        // Store mapping
        let mut mapping = self.did_to_address.write().await;
        mapping.insert(did.to_string(), address);

        address
    }

    /// Update reputation score (using primitives)
    pub async fn update_reputation(&self, did: &str, delta: i64, reason: &str) -> Result<()> {
        let mut profiles = self.reputation_profiles.write().await;

        if let Some(profile) = profiles.get_mut(did) {
            // Update messaging service participation
            if let Some(messaging) = profile
                .participant_score
                .service_participation
                .iter_mut()
                .find(|s| matches!(s.service_type, ServiceType::Messaging))
            {
                // Update quality score based on delta
                let quality_delta = delta as f64 / 1000.0;
                messaging.quality_score = (messaging.quality_score + quality_delta).clamp(0.0, 1.0);
                messaging.last_participation = Utc::now();
                profile.participant_score.updated_at = Utc::now();
            }

            let current_score = self.get_reputation_score(did).await?;

            println!(
                "📊 Updated reputation for {}: {} ({:+} - {})",
                did, current_score, delta, reason
            );

            // Auto-ban if score too low (add to blacklist without recursion)
            if current_score < -100 {
                let did_clone = did.to_string();
                drop(profiles);

                let mut blacklist = self.blacklist.write().await;
                blacklist.insert(did_clone.clone());
                println!("🚫 Auto-banned user {} - Low reputation score", did_clone);
            }
        }

        Ok(())
    }

    /// Record a violation (integrated with primitives)
    pub async fn record_violation(&self, did: &str, violation: Violation) -> Result<()> {
        // Apply reputation penalty based on severity
        let penalty = match violation.severity {
            ViolationSeverity::Low => -10,
            ViolationSeverity::Medium => -50,
            ViolationSeverity::High => -200,
            ViolationSeverity::Critical => -1000,
        };

        // Update reputation using primitives
        let mut profiles = self.reputation_profiles.write().await;

        if let Some(profile) = profiles.get_mut(did) {
            // Update messaging service with failure
            if let Some(messaging) = profile
                .participant_score
                .service_participation
                .iter_mut()
                .find(|s| matches!(s.service_type, ServiceType::Messaging))
            {
                // Record as failed action with poor quality
                messaging.record_action(false, 0.0); // failure, quality = 0
            }

            profile.participant_score.updated_at = Utc::now();

            let violation_count = 1; // TODO: Track violations separately

            println!(
                "⚠️  Violation recorded for {}: {:?} (penalty: {})",
                did, violation.violation_type, penalty
            );

            // Check auto-ban threshold (add to blacklist without recursion)
            let policies = self.policies.read().await;
            if violation.severity == ViolationSeverity::Critical
                || violation_count >= policies.auto_ban_threshold
            {
                let did_clone = did.to_string();
                drop(profiles);
                drop(policies);

                // Add to blacklist directly
                let mut blacklist = self.blacklist.write().await;
                blacklist.insert(did_clone.clone());
                println!("🚫 Auto-banned user {} - Threshold exceeded", did_clone);
            }
        }

        Ok(())
    }

    /// Report spam from a user (integrated with primitives + AI detection)
    pub async fn report_spam(&self, offender_did: &str, reporter_did: &str) -> Result<()> {
        // TODO: Add AI-powered attack detection
        // use swtchx_recovery::ai::AttackDetector;
        // Check if this is a coordinated false report attack
        // For now, proceed with basic spam recording

        // Record spam violation
        self.record_violation(
            offender_did,
            Violation {
                violation_type: ViolationType::Spam,
                description: format!("Reported for spam by {}", reporter_did),
                timestamp: Utc::now(),
                reported_by: reporter_did.to_string(),
                severity: ViolationSeverity::Medium,
            },
        )
        .await?;

        Ok(())
    }

    /// Get reputation profile (full primitives object)
    pub async fn get_reputation_profile(&self, did: &str) -> Result<ReputationProfile> {
        self.get_or_create_profile(did).await
    }

    /// Export reputation for blockchain sync
    pub async fn export_reputation_for_blockchain(&self, did: &str) -> Result<(Address, i64, f64)> {
        let profile = self.get_or_create_profile(did).await?;
        let score = self.get_reputation_score(did).await?;
        let behavioral = profile
            .behavioral_score
            .as_ref()
            .map(|bs| bs.overall_behavioral_score)
            .unwrap_or(0.5);

        Ok((profile.address, score, behavioral))
    }

    /// Increment message count (positive reputation) - Using primitives
    pub async fn record_message_sent(&self, did: &str) -> Result<()> {
        let mut profiles = self.reputation_profiles.write().await;

        let profile = profiles.entry(did.to_string()).or_insert_with(|| {
            // Create default profile if not exists
            let address = Address::ZERO; // Will be set properly on first use
            ReputationProfile {
                address,
                participant_score: ParticipantScore {
                    as_consumer: PrimitiveReputationScore {
                        score: U256::from(0),
                        total_actions: U256::from(0),
                        successful_actions: U256::from(0),
                    },
                    as_producer: PrimitiveReputationScore {
                        score: U256::from(0),
                        total_actions: U256::from(0),
                        successful_actions: U256::from(0),
                    },
                    product_scores: Vec::new(),
                    actions: Vec::new(),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                    behavioral_consistency: None,
                    interaction_style: None,
                    service_participation: vec![ServiceParticipation::new(ServiceType::Messaging)],
                    network_metrics: None,
                },
                eth_escrow_balance: U256::from(0),
                behavioral_score: None,
                archetype_classification: None,
                behavioral_fingerprint: None,
                confidence_score: None,
            }
        });

        // Update messaging service participation using primitives method
        if let Some(messaging) = profile
            .participant_score
            .service_participation
            .iter_mut()
            .find(|s| matches!(s.service_type, ServiceType::Messaging))
        {
            // Use primitives' built-in method!
            messaging.record_action(true, 1.0); // success = true, quality = 1.0

            // This automatically updates:
            // - total_actions
            // - success_rate
            // - quality_score
            // - frequency
            // - consistency_score
        }

        profile.participant_score.updated_at = Utc::now();

        Ok(())
    }

    /// Get all whitelisted DIDs
    pub async fn get_whitelist(&self) -> Vec<String> {
        let whitelist = self.whitelist.read().await;
        whitelist.iter().cloned().collect()
    }

    /// Get all blacklisted DIDs
    pub async fn get_blacklist(&self) -> Vec<String> {
        let blacklist = self.blacklist.read().await;
        blacklist.iter().cloned().collect()
    }

    /// Get access statistics (using primitives)
    pub async fn get_stats(&self) -> AccessStats {
        let whitelist = self.whitelist.read().await;
        let blacklist = self.blacklist.read().await;
        let permissions = self.permissions.read().await;
        let profiles = self.reputation_profiles.read().await;

        // Calculate average reputation from messaging service quality
        let avg_reputation = if !profiles.is_empty() {
            let total: f64 = profiles
                .values()
                .filter_map(|p| {
                    p.participant_score
                        .service_participation
                        .iter()
                        .find(|s| matches!(s.service_type, ServiceType::Messaging))
                        .map(|s| s.quality_score * 1000.0 - 500.0) // Convert to -500 to +500
                })
                .sum();
            total / profiles.len() as f64
        } else {
            0.0
        };

        AccessStats {
            node_type: self.node_type.clone(),
            total_users: permissions.len(),
            whitelisted_users: whitelist.len(),
            blacklisted_users: blacklist.len(),
            average_reputation: avg_reputation,
        }
    }

    /// Update access policies
    pub async fn update_policies(&self, policies: AccessPolicies) -> Result<()> {
        *self.policies.write().await = policies;
        println!("✅ Access policies updated");
        Ok(())
    }

    /// Get current policies
    pub async fn get_policies(&self) -> AccessPolicies {
        self.policies.read().await.clone()
    }

    /// Check rate limiting
    pub async fn check_rate_limit(
        &self,
        did: &str,
        message_count_last_minute: u32,
    ) -> Result<bool> {
        let policies = self.policies.read().await;
        Ok(message_count_last_minute < policies.rate_limit_per_minute)
    }
}

/// Actions that can be performed
#[derive(Debug, Clone)]
pub enum Action {
    SendMessage,
    CreateGroup,
    InviteUser,
    Moderate,
    Admin,
}

/// Access control statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessStats {
    pub node_type: NodeType,
    pub total_users: usize,
    pub whitelisted_users: usize,
    pub blacklisted_users: usize,
    pub average_reputation: f64,
}

impl Default for UserPermissions {
    fn default() -> Self {
        Self {
            did: String::new(),
            role: UserRole::Guest,
            can_send_messages: false,
            can_create_groups: false,
            can_invite_users: false,
            can_moderate: false,
            can_admin: false,
            granted_at: Utc::now(),
            granted_by: None,
            expires_at: None,
        }
    }
}

impl Default for MessagingReputationView {
    fn default() -> Self {
        Self {
            did: String::new(),
            score: 0,
            total_messages: 0,
            spam_reports: 0,
            helpful_votes: 0,
            violations: Vec::new(),
            last_updated: Utc::now(),
            behavioral_score: None,
            service_quality: 0.5,
            consistency_score: 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_public_node_access() {
        let acl = AccessControlManager::new(NodeType::Public);

        // Public nodes allow any DID by default
        assert!(acl.has_access("did:swtch:user:test").await.unwrap());
    }

    #[tokio::test]
    async fn test_private_node_access() {
        let acl = AccessControlManager::new(NodeType::Private);

        // Private nodes deny by default
        assert!(!acl.has_access("did:swtch:user:test").await.unwrap());

        // Add to whitelist
        acl.add_to_whitelist("did:swtch:user:test".to_string())
            .await
            .unwrap();
        assert!(acl.has_access("did:swtch:user:test").await.unwrap());
    }

    #[tokio::test]
    async fn test_blacklist() {
        let acl = AccessControlManager::new(NodeType::Public);

        let did = "did:swtch:user:badactor".to_string();

        // Initially has access
        assert!(acl.has_access(&did).await.unwrap());

        // Ban user
        acl.ban_user(did.clone(), "Spam".to_string()).await.unwrap();

        // Now denied
        assert!(!acl.has_access(&did).await.unwrap());
        assert!(acl.is_blacklisted(&did).await);
    }

    #[tokio::test]
    async fn test_reputation_system() {
        let acl = AccessControlManager::new(NodeType::Public);

        let did = "did:swtch:user:test";

        // Initial reputation is 0
        let rep = acl.get_reputation(did).await.unwrap();
        assert_eq!(rep.score, 0);

        // Update reputation
        acl.update_reputation(did, 50, "Good behavior")
            .await
            .unwrap();

        // Score should be updated (approximately, due to quality score conversion)
        let rep = acl.get_reputation(did).await.unwrap();
        assert!(rep.score > 0); // Should be positive after update
    }

    #[tokio::test]
    async fn test_permissions() {
        let acl = AccessControlManager::new(NodeType::Public);

        let did = "did:swtch:user:test";

        // Grant moderator permissions
        acl.grant_permissions(did.to_string(), UserRole::Moderator, None)
            .await
            .unwrap();

        // Check permissions
        assert!(acl.can_perform(did, Action::SendMessage).await.unwrap());
        assert!(acl.can_perform(did, Action::CreateGroup).await.unwrap());
        assert!(acl.can_perform(did, Action::Moderate).await.unwrap());
        assert!(!acl.can_perform(did, Action::Admin).await.unwrap());
    }
}
