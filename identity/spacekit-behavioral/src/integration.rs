//! Integration Module
//! 
//! Provides integration between the behavioral simulation and SpaceKit primitives

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// SpaceKit Primitives
use spacekit_primitives::v1::behavioral_types::{
    UserArchetype as PrimArchetype, ServiceType as PrimServiceType, 
    InteractionStyle as PrimInteractionStyle, PersonalityTraits,
    BehavioralFingerprint, BehavioralConfidenceScore
};
use spacekit_primitives::v1::reputation::{
    ReputationProfile, BehavioralReputationScore
};

// Local simulation types
use crate::{
    UserArchetype, ServiceType, InteractionStyle, PersonalityProfile,
    BehavioralPattern, ActivityData, ReputationScore
};

/// Integration layer between simulation and production primitives
pub struct PrimitivesIntegration {
    /// Mapping from simulation archetypes to primitives archetypes
    archetype_mapping: HashMap<UserArchetype, PrimArchetype>,
    /// Mapping from simulation services to primitives services
    service_mapping: HashMap<ServiceType, PrimServiceType>,
    /// Mapping from simulation interaction styles to primitives styles
    interaction_mapping: HashMap<InteractionStyle, PrimInteractionStyle>,
}

impl PrimitivesIntegration {
    /// Create new integration layer
    pub fn new() -> Self {
        let mut archetype_mapping = HashMap::new();
        archetype_mapping.insert(UserArchetype::BaseUser, PrimArchetype::BaseUser);
        archetype_mapping.insert(UserArchetype::Validator, PrimArchetype::Validator);
        archetype_mapping.insert(UserArchetype::Developer, PrimArchetype::Developer);
        archetype_mapping.insert(UserArchetype::Researcher, PrimArchetype::Researcher);
        archetype_mapping.insert(UserArchetype::Investor, PrimArchetype::Investor);
        archetype_mapping.insert(UserArchetype::Regulator, PrimArchetype::Regulator);
        archetype_mapping.insert(UserArchetype::Other, PrimArchetype::Other);
        
        let mut service_mapping = HashMap::new();
        service_mapping.insert(ServiceType::Compute, PrimServiceType::Compute);
        service_mapping.insert(ServiceType::Storage, PrimServiceType::Storage);
        service_mapping.insert(ServiceType::Messaging, PrimServiceType::Messaging);
        service_mapping.insert(ServiceType::AI, PrimServiceType::AI);
        service_mapping.insert(ServiceType::Identity, PrimServiceType::Identity);
        service_mapping.insert(ServiceType::CrossChain, PrimServiceType::CrossChain);
        service_mapping.insert(ServiceType::Encryption, PrimServiceType::Encryption);
        
        let mut interaction_mapping = HashMap::new();
        interaction_mapping.insert(InteractionStyle::Collaborative, PrimInteractionStyle::Collaborative);
        interaction_mapping.insert(InteractionStyle::Independent, PrimInteractionStyle::Independent);
        interaction_mapping.insert(InteractionStyle::Supportive, PrimInteractionStyle::Supportive);
        interaction_mapping.insert(InteractionStyle::Competitive, PrimInteractionStyle::Competitive);
        interaction_mapping.insert(InteractionStyle::Suspicious, PrimInteractionStyle::Suspicious);
        
        Self {
            archetype_mapping,
            service_mapping,
            interaction_mapping,
        }
    }
    
    /// Convert simulation personality profile to primitives personality traits
    pub fn convert_personality_profile(&self, profile: &PersonalityProfile) -> Result<PersonalityTraits> {
        let prim_archetype = self.archetype_mapping.get(&profile.archetype)
            .ok_or_else(|| anyhow::anyhow!("Unknown archetype: {:?}", profile.archetype))?;
        
        let prim_services: Result<Vec<PrimServiceType>, _> = profile.service_preferences.iter()
            .map(|service| self.service_mapping.get(service)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Unknown service: {:?}", service)))
            .collect();
        
        Ok(PersonalityTraits {
            archetype: prim_archetype.clone(),
            activity_level: profile.activity_level,
            consistency: profile.consistency,
            collaboration: profile.collaboration,
            innovation: profile.innovation,
            security_consciousness: profile.security_consciousness,
            economic_engagement: profile.economic_engagement,
            cross_chain_preference: profile.cross_chain_preference,
            peak_hours: profile.peak_hours.clone(),
            service_preferences: prim_services?,
            risk_tolerance: profile.risk_tolerance,
        })
    }
    
    /// Convert simulation behavioral pattern to primitives behavioral fingerprint
    pub fn convert_behavioral_pattern(&self, pattern: &BehavioralPattern, did: &str) -> Result<BehavioralFingerprint> {
        let prim_services: Result<Vec<PrimServiceType>, _> = pattern.service_preferences.iter()
            .map(|service| self.service_mapping.get(service)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Unknown service: {:?}", service)))
            .collect();
        
        let prim_interaction = self.interaction_mapping.get(&pattern.interaction_style)
            .ok_or_else(|| anyhow::anyhow!("Unknown interaction style: {:?}", pattern.interaction_style))?;
        
        Ok(BehavioralFingerprint {
            fingerprint_id: uuid::Uuid::new_v4().to_string(),
            did: did.to_string(),
            activity_frequency: pattern.activity_frequency,
            peak_activity_hours: pattern.peak_activity_hours.clone(),
            service_preferences: prim_services?,
            interaction_style: prim_interaction.clone(),
            anomaly_score: pattern.anomaly_score,
            pattern_stability: pattern.pattern_stability,
            economic_participation: 0.5, // Would be calculated from economic patterns
            cross_chain_activity: 0.4,   // Would be calculated from cross-chain patterns
            security_compliance: 0.8,    // Would be calculated from security patterns
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
    }
    
    /// Convert simulation reputation score to primitives confidence score
    pub fn convert_reputation_to_confidence(&self, reputation: &ReputationScore) -> Result<BehavioralConfidenceScore> {
        use alloy_primitives::Address;
        
        // Convert string address to Address type (simplified)
        let address = "0x742d35Cc6479C4D1f0C5c5b6F7d4dE7b5D2a1234".parse()
            .map_err(|_| anyhow::anyhow!("Invalid address"))?;
        
        Ok(BehavioralConfidenceScore {
            address,
            overall_confidence: reputation.ml_confidence,
            components: spacekit_primitives::v1::behavioral_types::ConfidenceComponents {
                pattern_consistency: reputation.pattern_consistency,
                service_predictability: 0.7, // Would be calculated
                timing_reliability: 0.8,     // Would be calculated
                economic_consistency: 0.6,   // Would be calculated
                cross_chain_consistency: 0.5, // Would be calculated
                peer_endorsement: 0.75,      // Would be calculated
                longevity_bonus: 0.1,        // Would be calculated
            },
            trend: spacekit_primitives::v1::behavioral_types::ConfidenceTrend::Stable,
            data_points: 100, // Would be tracked
            calculated_at: chrono::Utc::now(),
            model_version: "simulation-v1.0".to_string(),
        })
    }
    
    /// Get archetype mapping
    pub fn get_archetype_mapping(&self) -> &HashMap<UserArchetype, PrimArchetype> {
        &self.archetype_mapping
    }
    
    /// Get service mapping
    pub fn get_service_mapping(&self) -> &HashMap<ServiceType, PrimServiceType> {
        &self.service_mapping
    }
    
    /// Get interaction mapping
    pub fn get_interaction_mapping(&self) -> &HashMap<InteractionStyle, PrimInteractionStyle> {
        &self.interaction_mapping
    }
}

impl Default for PrimitivesIntegration {
    fn default() -> Self {
        Self::new()
    }
}

/// Enhanced simulation results that include primitives types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedSimulationResults {
    /// Original simulation results
    pub original_results: crate::simulation::SimulationResults,
    /// Converted behavioral fingerprints
    pub behavioral_fingerprints: Vec<BehavioralFingerprint>,
    /// Converted confidence scores
    pub confidence_scores: Vec<BehavioralConfidenceScore>,
    /// Integration statistics
    pub integration_stats: IntegrationStatistics,
}

/// Statistics about the integration process
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatistics {
    /// Number of users converted
    pub users_converted: usize,
    /// Number of patterns converted
    pub patterns_converted: usize,
    /// Number of reputation scores converted
    pub reputation_scores_converted: usize,
    /// Conversion success rate
    pub conversion_success_rate: f64,
    /// Average conversion time per user (ms)
    pub avg_conversion_time_ms: u64,
}

/// Integration testing utilities
pub mod testing {
    use super::*;
    use crate::{PersonalityProfile, BehavioralPattern, ReputationScore};
    
    /// Create test personality profile
    pub fn create_test_personality() -> PersonalityProfile {
        PersonalityProfile {
            archetype: UserArchetype::Developer,
            activity_level: 8,
            consistency: 7,
            collaboration: 9,
            innovation: 10,
            security_consciousness: 8,
            economic_engagement: 6,
            cross_chain_preference: 5,
            peak_hours: vec![10, 11, 14, 15, 16, 17, 21, 22],
            service_preferences: vec![ServiceType::Compute, ServiceType::AI],
            risk_tolerance: 7,
        }
    }
    
    /// Create test behavioral pattern
    pub fn create_test_behavioral_pattern() -> BehavioralPattern {
        BehavioralPattern {
            did: "test-user-123".to_string(),
            activity_frequency: 12.5,
            peak_activity_hours: vec![10, 11, 14, 15, 16, 17],
            service_preferences: vec![ServiceType::Compute, ServiceType::Storage],
            interaction_style: InteractionStyle::Collaborative,
            anomaly_score: 0.05,
            pattern_stability: 0.82,
        }
    }
    
    /// Create test reputation score
    pub fn create_test_reputation() -> ReputationScore {
        ReputationScore {
            did: "test-user-123".to_string(),
            overall_score: 0.85,
            ml_confidence: 0.78,
            pattern_consistency: 0.82,
            fraud_risk: 0.03,
            recovery_eligibility: true,
            last_updated: chrono::Utc::now(),
        }
    }
    
    /// Test full integration pipeline
    pub fn test_full_integration() -> Result<()> {
        let integration = PrimitivesIntegration::new();
        
        // Test personality conversion
        let personality = create_test_personality();
        let prim_personality = integration.convert_personality_profile(&personality)?;
        assert_eq!(prim_personality.archetype, PrimArchetype::Developer);
        
        // Test behavioral pattern conversion
        let pattern = create_test_behavioral_pattern();
        let fingerprint = integration.convert_behavioral_pattern(&pattern, "did:spacekit:test")?;
        assert_eq!(fingerprint.did, "did:spacekit:test");
        
        // Test reputation conversion
        let reputation = create_test_reputation();
        let confidence = integration.convert_reputation_to_confidence(&reputation)?;
        assert_eq!(confidence.overall_confidence, reputation.ml_confidence);
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::*;
    
    #[test]
    fn test_primitives_integration_creation() {
        let integration = PrimitivesIntegration::new();
        assert_eq!(integration.archetype_mapping.len(), 7);
        assert_eq!(integration.service_mapping.len(), 7);
        assert_eq!(integration.interaction_mapping.len(), 5);
    }
    
    #[test]
    fn test_personality_conversion() {
        let integration = PrimitivesIntegration::new();
        let personality = testing::create_test_personality();
        let result = integration.convert_personality_profile(&personality);
        assert!(result.is_ok());
        
        let prim_personality = result.unwrap();
        assert_eq!(prim_personality.archetype, PrimArchetype::Developer);
        assert_eq!(prim_personality.activity_level, personality.activity_level);
    }
    
    #[test]
    fn test_behavioral_pattern_conversion() {
        let integration = PrimitivesIntegration::new();
        let pattern = testing::create_test_behavioral_pattern();
        let result = integration.convert_behavioral_pattern(&pattern, "did:spacekit:test");
        assert!(result.is_ok());
        
        let fingerprint = result.unwrap();
        assert_eq!(fingerprint.did, "did:spacekit:test");
        assert_eq!(fingerprint.activity_frequency, pattern.activity_frequency);
    }
    
    #[test]
    fn test_reputation_conversion() {
        let integration = PrimitivesIntegration::new();
        let reputation = testing::create_test_reputation();
        let result = integration.convert_reputation_to_confidence(&reputation);
        assert!(result.is_ok());
        
        let confidence = result.unwrap();
        assert_eq!(confidence.overall_confidence, reputation.ml_confidence);
    }
    
    #[test]
    fn test_full_integration_pipeline() {
        let result = testing::test_full_integration();
        assert!(result.is_ok());
    }
}