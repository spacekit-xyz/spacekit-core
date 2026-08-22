use crate::v1::behavioral_types::{
    BehavioralConfidenceScore, BehavioralFingerprint, ConfidenceComponents, ConfidenceTrend,
    InteractionStyle, NetworkParticipationMetrics, ServiceType, UserArchetype,
};
use alloy_primitives::{Address, U256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Enhanced reputation profile with behavioral components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationProfile {
    /// Original fields
    pub address: Address,
    pub participant_score: ParticipantScore,
    pub eth_escrow_balance: U256,

    /// New behavioral fields
    pub behavioral_score: Option<BehavioralReputationScore>,
    pub archetype_classification: Option<UserArchetype>,
    pub behavioral_fingerprint: Option<BehavioralFingerprint>,
    pub confidence_score: Option<BehavioralConfidenceScore>,
}

/// Enhanced participant score with behavioral metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantScore {
    /// Original fields
    pub as_consumer: ReputationScore,
    pub as_producer: ReputationScore,
    pub product_scores: Vec<(String, U256)>, // (product_hash, score)
    pub actions: Vec<(String, ReputationAction)>, // (action_type, action)
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    /// New behavioral fields
    pub behavioral_consistency: Option<f64>,
    pub interaction_style: Option<InteractionStyle>,
    pub service_participation: Vec<ServiceParticipation>,
    pub network_metrics: Option<NetworkParticipationMetrics>,
}

/// Original reputation score structure (unchanged for compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationScore {
    pub score: U256,
    pub total_actions: U256,
    pub successful_actions: U256,
}

/// Original reputation action structure (unchanged for compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationAction {
    pub weight: U256,
    pub last_action_time: U256,
}

/// New behavioral reputation score integrating with existing system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralReputationScore {
    /// Overall behavioral score (0.0-1.0)
    pub overall_behavioral_score: f64,

    /// Behavioral components breakdown
    pub behavioral_components: BehavioralReputationComponents,

    /// Integration with traditional reputation
    pub traditional_score_weight: f64,
    pub behavioral_score_weight: f64,
    pub combined_score: f64,

    /// Confidence and trend
    pub confidence_level: f64,
    pub score_trend: ConfidenceTrend,

    /// Timestamps
    pub calculated_at: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,

    /// Model information
    pub model_version: String,
    pub calculation_method: String,
}

/// Behavioral components of reputation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralReputationComponents {
    /// Service participation consistency (0.0-1.0)
    pub service_consistency: f64,

    /// Pattern reliability over time (0.0-1.0)
    pub pattern_reliability: f64,

    /// Cross-chain behavioral consistency (0.0-1.0)
    pub cross_chain_consistency: f64,

    /// Economic behavior predictability (0.0-1.0)
    pub economic_predictability: f64,

    /// Security compliance score (0.0-1.0)
    pub security_compliance: f64,

    /// Peer interaction quality (0.0-1.0)
    pub peer_interaction_quality: f64,

    /// Fraud risk assessment (0.0-1.0, lower is better)
    pub fraud_risk_score: f64,

    /// Archetype conformity (0.0-1.0)
    pub archetype_conformity: f64,
}

/// Service participation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceParticipation {
    /// Type of service
    pub service_type: ServiceType,

    /// Participation frequency (actions per day)
    pub frequency: f64,

    /// Success rate for this service (0.0-1.0)
    pub success_rate: f64,

    /// Quality score for this service (0.0-1.0)
    pub quality_score: f64,

    /// Total actions performed
    pub total_actions: u64,

    /// First participation timestamp
    pub first_participation: DateTime<Utc>,

    /// Last participation timestamp
    pub last_participation: DateTime<Utc>,

    /// Consistency score (0.0-1.0)
    pub consistency_score: f64,
}

/// Reputation verification challenge for behavioral recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationVerificationChallenge {
    /// Challenge identifier
    pub challenge_id: String,

    /// Associated address
    pub address: Address,

    /// Type of behavioral verification required
    pub verification_type: ReputationVerificationType,

    /// Expected behavioral response
    pub expected_response: Vec<u8>,

    /// Challenge data
    pub challenge_data: Vec<u8>,

    /// Challenge creation time
    pub created_at: DateTime<Utc>,

    /// Challenge expiration time
    pub expires_at: DateTime<Utc>,

    /// Difficulty level (0.0-1.0)
    pub difficulty: f64,
}

/// Types of reputation verification challenges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReputationVerificationType {
    /// Verify historical service usage patterns
    ServicePatternVerification,

    /// Verify economic behavior consistency
    EconomicBehaviorVerification,

    /// Verify peer interaction patterns
    PeerInteractionVerification,

    /// Verify cross-chain activity patterns
    CrossChainPatternVerification,

    /// Verify archetype-specific behaviors
    ArchetypeConformityVerification,

    /// Comprehensive behavioral profile verification
    ComprehensiveBehavioralVerification,
}

/// Reputation recovery session using behavioral patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationRecoverySession {
    /// Session identifier
    pub session_id: String,

    /// Associated address attempting recovery
    pub address: Address,

    /// Recovery challenges issued
    pub challenges: Vec<ReputationVerificationChallenge>,

    /// Challenge responses received
    pub responses: Vec<ReputationVerificationResponse>,

    /// Session status
    pub status: RecoverySessionStatus,

    /// Overall verification score (0.0-1.0)
    pub verification_score: f64,

    /// Session timestamps
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,

    /// Recovery decision
    pub recovery_approved: bool,
    pub approval_confidence: f64,
}

/// Response to reputation verification challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationVerificationResponse {
    /// Response identifier
    pub response_id: String,

    /// Associated challenge
    pub challenge_id: String,

    /// Response data
    pub response_data: Vec<u8>,

    /// Verification result
    pub verification_success: bool,

    /// Confidence in the verification (0.0-1.0)
    pub verification_confidence: f64,

    /// Response timestamp
    pub submitted_at: DateTime<Utc>,

    /// Verification timestamp
    pub verified_at: DateTime<Utc>,
}

/// Status of reputation recovery session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecoverySessionStatus {
    /// Session initiated, waiting for challenges
    Initiated,

    /// Challenges issued, waiting for responses
    ChallengesIssued,

    /// Responses received, under verification
    UnderVerification,

    /// Verification completed successfully
    Verified,

    /// Verification failed
    Failed,

    /// Session expired
    Expired,

    /// Session cancelled
    Cancelled,
}

impl ReputationProfile {
    /// Create a new reputation profile with behavioral components
    pub fn new_with_behavioral(
        address: Address,
        participant_score: ParticipantScore,
        eth_escrow_balance: U256,
        archetype: UserArchetype,
    ) -> Self {
        Self {
            address,
            participant_score,
            eth_escrow_balance,
            behavioral_score: None,
            archetype_classification: Some(archetype),
            behavioral_fingerprint: None,
            confidence_score: None,
        }
    }

    /// Update behavioral components of the reputation
    pub fn update_behavioral_score(&mut self, behavioral_score: BehavioralReputationScore) {
        self.behavioral_score = Some(behavioral_score);
    }

    /// Set behavioral fingerprint
    pub fn set_behavioral_fingerprint(&mut self, fingerprint: BehavioralFingerprint) {
        self.behavioral_fingerprint = Some(fingerprint);
    }

    /// Set confidence score
    pub fn set_confidence_score(&mut self, confidence: BehavioralConfidenceScore) {
        self.confidence_score = Some(confidence);
    }

    /// Get combined reputation score (traditional + behavioral)
    pub fn get_combined_score(&self) -> f64 {
        match &self.behavioral_score {
            Some(behavioral) => behavioral.combined_score,
            None => {
                // Fallback to traditional score only
                let traditional_score = self.participant_score.as_consumer.score.to::<u64>() as f64;
                traditional_score / 1000.0 // Normalize to 0.0-1.0 range
            }
        }
    }

    /// Check if user is eligible for behavioral recovery
    pub fn is_behavioral_recovery_eligible(&self) -> bool {
        if let Some(confidence) = &self.confidence_score {
            confidence.overall_confidence >= 0.7 && confidence.data_points >= 10
        } else {
            false
        }
    }

    /// Get archetype classification with confidence
    pub fn get_archetype_classification(&self) -> Option<(UserArchetype, f64)> {
        if let (Some(archetype), Some(behavioral)) =
            (&self.archetype_classification, &self.behavioral_score)
        {
            Some((
                archetype.clone(),
                behavioral.behavioral_components.archetype_conformity,
            ))
        } else {
            self.archetype_classification
                .as_ref()
                .map(|a| (a.clone(), 0.5))
        }
    }
}

impl Default for BehavioralReputationComponents {
    fn default() -> Self {
        Self {
            service_consistency: 0.5,
            pattern_reliability: 0.5,
            cross_chain_consistency: 0.5,
            economic_predictability: 0.5,
            security_compliance: 0.5,
            peer_interaction_quality: 0.5,
            fraud_risk_score: 0.1,
            archetype_conformity: 0.5,
        }
    }
}

impl ServiceParticipation {
    /// Create new service participation record
    pub fn new(service_type: ServiceType) -> Self {
        let now = Utc::now();
        Self {
            service_type,
            frequency: 0.0,
            success_rate: 1.0,
            quality_score: 0.5,
            total_actions: 0,
            first_participation: now,
            last_participation: now,
            consistency_score: 0.5,
        }
    }

    /// Update participation record with new action
    pub fn record_action(&mut self, success: bool, quality: f64) {
        self.total_actions += 1;
        self.last_participation = Utc::now();

        // Update success rate (exponential moving average)
        let alpha = 0.1;
        if success {
            self.success_rate = self.success_rate * (1.0 - alpha) + alpha;
        } else {
            self.success_rate = self.success_rate * (1.0 - alpha);
        }

        // Update quality score (exponential moving average)
        self.quality_score = self.quality_score * (1.0 - alpha) + quality * alpha;

        // Update frequency (actions per day)
        let days_participating = (Utc::now() - self.first_participation).num_days().max(1) as f64;
        self.frequency = self.total_actions as f64 / days_participating;

        // Update consistency score based on regular participation
        self.update_consistency_score();
    }

    fn update_consistency_score(&mut self) {
        // Simple consistency calculation based on regular participation
        let days_since_last = (Utc::now() - self.last_participation).num_days() as f64;
        let expected_interval = if self.frequency > 0.0 {
            1.0 / self.frequency
        } else {
            7.0
        };

        if days_since_last <= expected_interval * 1.5 {
            self.consistency_score = (self.consistency_score * 0.9 + 0.1).min(1.0);
        } else {
            self.consistency_score = (self.consistency_score * 0.9).max(0.1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_reputation_profile_creation() {
        let address = address!("0x742d35Cc6479C4D1f0C5c5b6F7d4dE7b5D2a1234");
        let participant_score = ParticipantScore {
            as_consumer: ReputationScore {
                score: U256::from(1000),
                total_actions: U256::from(100),
                successful_actions: U256::from(95),
            },
            as_producer: ReputationScore {
                score: U256::from(800),
                total_actions: U256::from(80),
                successful_actions: U256::from(75),
            },
            product_scores: vec![],
            actions: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            behavioral_consistency: None,
            interaction_style: None,
            service_participation: vec![],
            network_metrics: None,
        };

        let profile = ReputationProfile::new_with_behavioral(
            address,
            participant_score,
            U256::from(1000000),
            UserArchetype::Developer,
        );

        assert_eq!(profile.address, address);
        assert_eq!(
            profile.archetype_classification,
            Some(UserArchetype::Developer)
        );
        assert!(!profile.is_behavioral_recovery_eligible()); // No confidence score yet
    }

    #[test]
    fn test_service_participation() {
        let mut participation = ServiceParticipation::new(ServiceType::Compute);

        // Record some successful actions
        participation.record_action(true, 0.8);
        participation.record_action(true, 0.9);
        participation.record_action(false, 0.3);

        assert_eq!(participation.total_actions, 3);
        assert!(participation.success_rate > 0.5);
        assert!(participation.quality_score > 0.5);
    }

    #[test]
    fn test_behavioral_reputation_components() {
        let components = BehavioralReputationComponents::default();

        assert_eq!(components.service_consistency, 0.5);
        assert_eq!(components.fraud_risk_score, 0.1);
        assert_eq!(components.archetype_conformity, 0.5);
    }
}
