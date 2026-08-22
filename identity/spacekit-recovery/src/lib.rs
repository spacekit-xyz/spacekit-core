// SpaceKit Network Recovery: Distributed Confidence Recovery Protocol
// Implementing behavioral cryptography for quantum-resistant identity recovery

pub mod ai;
pub mod simulation;
pub mod behavioral;
pub mod recovery;
pub mod zkp;

// Re-export main behavioral types for easy access
pub use behavioral::{
    BehavioralPatterns, BehavioralPatternAnalyzer, BehavioralFingerprint, 
    BehavioralFingerprintGenerator, ConfidenceScore, ConfidenceScorer,
    StoragePattern, ComputePattern, EconomicPattern, ServiceQualityMetrics,
    MultiChainPattern, ConfidenceFactors, NetworkBehavioralStats,
    PeerEndorsementMatrix, EndorsementRecord, EndorsementType
};

// Re-export recovery types
pub use recovery::{
    RecoveryOrchestrator, RecoveryWorkflowResult, VerificationSummary, 
    RecoverySession, RecoveryPhase, RecoveryStatistics,
    ChallengeResponseProtocol, RecoveryChallenge, ChallengeResponse,
    DistributedVerifier, VerificationResult, NetworkConsensus
};

use spacekit_primitives::v1::identity::Identity;
use std::error::Error;

/// Main interface for the Distributed Confidence Recovery Protocol
pub struct BehavioralRecoverySystem {
    pattern_analyzer: BehavioralPatternAnalyzer,
    fingerprint_generator: BehavioralFingerprintGenerator,
    confidence_scorer: ConfidenceScorer,
    privacy_budget: f64,
}

impl BehavioralRecoverySystem {
    /// Create new behavioral recovery system with privacy parameters
    pub fn new(epsilon: f64, delta: f64) -> Self {
        let confidence_factors = ConfidenceFactors::default();
        
        Self {
            pattern_analyzer: BehavioralPatternAnalyzer::new(epsilon, delta),
            fingerprint_generator: BehavioralFingerprintGenerator::new(
                "Kyber1024".to_string(), 
                epsilon, 
                delta
            ),
            confidence_scorer: ConfidenceScorer::new(epsilon, delta, confidence_factors),
            privacy_budget: epsilon,
        }
    }

    /// Analyze behavioral patterns for an identity
    pub fn analyze_behavioral_patterns(&self, identity: &Identity) -> Result<BehavioralPatterns, Box<dyn Error>> {
        self.pattern_analyzer.analyze_patterns(identity)
    }

    /// Generate behavioral fingerprint from patterns
    pub fn generate_behavioral_fingerprint(
        &self,
        patterns: &BehavioralPatterns,
        identity_did: &str,
    ) -> Result<BehavioralFingerprint, Box<dyn Error>> {
        self.fingerprint_generator.generate_fingerprint(patterns, identity_did)
    }

    /// Compute confidence score for identity recovery
    pub fn compute_confidence_score(
        &self,
        patterns: &BehavioralPatterns,
        peer_endorsements: &PeerEndorsementMatrix,
        identity_did: &str,
    ) -> Result<ConfidenceScore, Box<dyn Error>> {
        self.confidence_scorer.compute_confidence_score(patterns, peer_endorsements, identity_did)
    }

    /// Verify if confidence score meets threshold for recovery
    pub fn verify_recovery_eligibility(&self, confidence_score: &ConfidenceScore) -> Result<bool, Box<dyn Error>> {
        self.confidence_scorer.verify_confidence_threshold(confidence_score)
    }

    /// Generate comprehensive recovery report
    pub fn generate_recovery_report(
        &self,
        patterns: &BehavioralPatterns,
        confidence_score: &ConfidenceScore,
        identity_did: &str,
    ) -> Result<String, Box<dyn Error>> {
        self.confidence_scorer.generate_confidence_report(patterns, confidence_score, identity_did)
    }

    /// Get access to the confidence scorer for advanced operations
    pub fn get_confidence_scorer(&self) -> &ConfidenceScorer {
        &self.confidence_scorer
    }

    /// Decrypt confidence score to get the actual confidence value
    pub fn decrypt_confidence_score(&self, confidence_score: &ConfidenceScore) -> Result<f64, Box<dyn Error>> {
        self.confidence_scorer.decrypt_confidence_score(&confidence_score.encrypted_score)
    }

    /// Complete behavioral recovery workflow
    pub async fn initiate_behavioral_recovery(
        &self,
        claimed_identity: &Identity,
        peer_endorsements: &PeerEndorsementMatrix,
    ) -> Result<RecoveryResult, Box<dyn Error>> {
        // Step 1: Analyze behavioral patterns
        let patterns = self.analyze_behavioral_patterns(claimed_identity)?;
        
        // Step 2: Generate behavioral fingerprint
        let fingerprint = self.generate_behavioral_fingerprint(&patterns, &claimed_identity.did)?;
        
        // Step 3: Compute confidence score
        let confidence_score = self.compute_confidence_score(&patterns, peer_endorsements, &claimed_identity.did)?;
        
        // Step 4: Check if recovery is eligible
        let eligible = self.verify_recovery_eligibility(&confidence_score)?;
        
        // Step 5: Generate comprehensive report
        let report = self.generate_recovery_report(&patterns, &confidence_score, &claimed_identity.did)?;

        Ok(RecoveryResult {
            patterns,
            fingerprint,
            confidence_score,
            eligible,
            report,
        })
    }
}

/// Result of behavioral recovery analysis
#[derive(Debug)]
pub struct RecoveryResult {
    pub patterns: BehavioralPatterns,
    pub fingerprint: BehavioralFingerprint,
    pub confidence_score: ConfidenceScore,
    pub eligible: bool,
    pub report: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ndarray::Array1;

    #[test]
    fn test_behavioral_recovery_system_creation() {
        let recovery_system = BehavioralRecoverySystem::new(1.0, 1e-6);
        assert_eq!(recovery_system.privacy_budget, 1.0);
    }

    #[test]
    fn test_confidence_factors_default() {
        let factors = ConfidenceFactors::default();
        assert_eq!(factors.network_participation_weight, 0.25);
        assert_eq!(factors.peer_endorsement_weight, 0.20);
        assert_eq!(factors.service_quality_weight, 0.20);
        assert_eq!(factors.economic_consistency_weight, 0.15);
        assert_eq!(factors.multi_chain_behavior_weight, 0.10);
        assert_eq!(factors.temporal_weighting, 0.10);
    }

    #[test]
    fn test_peer_endorsement_matrix() {
        let mut matrix = PeerEndorsementMatrix::new();
        matrix.set_total_endorsers(100);
        
        let endorsement = EndorsementRecord {
            endorser_did: "did:swtch:endorser1".to_string(),
            endorsement_strength: 0.9,
            endorsement_type: EndorsementType::StorageReliability,
            timestamp: Utc::now(),
        };
        
        matrix.add_endorsement("did:swtch:target".to_string(), endorsement);
        assert_eq!(matrix.total_endorsers, 100);
        assert!(matrix.endorsements.contains_key("did:swtch:target"));
    }

    #[test]
    fn test_network_behavioral_stats_default() {
        let stats = NetworkBehavioralStats::default();
        assert_eq!(stats.avg_storage_contribution, 10.0);
        assert_eq!(stats.avg_compute_participation, 8.0);
        assert_eq!(stats.avg_economic_consistency, 0.8);
        assert_eq!(stats.network_size, 1000);
    }

    #[tokio::test]
    async fn test_mock_behavioral_patterns() {
        // Create mock identity
        let identity = Identity::new(
            "did:swtch:test123".to_string(),
            "test_user".to_string(),
            "test_password".to_string(),
        );

        // Create recovery system
        let recovery_system = BehavioralRecoverySystem::new(1.0, 1e-6);

        // Create mock peer endorsements
        let mut peer_endorsements = PeerEndorsementMatrix::new();
        peer_endorsements.set_total_endorsers(50);
        
        let endorsement = EndorsementRecord {
            endorser_did: "did:swtch:peer1".to_string(),
            endorsement_strength: 0.85,
            endorsement_type: EndorsementType::ServiceExcellence,
            timestamp: Utc::now(),
        };
        
        peer_endorsements.add_endorsement(identity.did.clone(), endorsement);

        // Test behavioral recovery workflow
        let result = recovery_system.initiate_behavioral_recovery(&identity, &peer_endorsements).await;
        
        // For this test, we expect it to work with mock data
        assert!(result.is_ok());
        let recovery_result = result.unwrap();
        assert!(!recovery_result.report.is_empty());
        assert_eq!(recovery_result.patterns.privacy_budget_used, 1.0);
    }

    #[test]
    fn test_behavioral_pattern_structures() {
        // Test that all pattern structures can be created
        let storage_pattern = StoragePattern {
            avg_daily_storage_gb: 15.5,
            consistency_score: 0.95,
            geographic_preferences: Array1::zeros(10),
            avg_retention_days: 30.0,
            preferred_storage_hours: Array1::zeros(24),
        };

        let compute_pattern = ComputePattern {
            avg_daily_compute_hours: 8.5,
            avg_daily_bandwidth_gb: 50.0,
            availability_pattern: Array1::zeros(24),
            preferred_compute_types: vec!["ML".to_string(), "Storage".to_string()],
            service_quality: 0.92,
        };

        let economic_pattern = EconomicPattern {
            earning_consistency: 0.88,
            avg_stake_duration: 45.0,
            payment_punctuality: 0.96,
            bonding_curve_interactions: 25,
            participation_score: 0.91,
        };

        assert_eq!(storage_pattern.avg_daily_storage_gb, 15.5);
        assert_eq!(compute_pattern.service_quality, 0.92);
        assert_eq!(economic_pattern.participation_score, 0.91);
    }
}
