//! SpaceKit Behavioral Cryptography Simulation
//! 
//! This simulation implements the behavioral cryptography concepts from the SpaceKit whitepaper,
//! demonstrating how network participation patterns can be used for identity recovery
//! without traditional trustee-based systems.

use anyhow::Result;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use uuid::Uuid;

pub mod archetypes;
pub mod behavioral_engine;
pub mod confidence_recovery;
pub mod simulation;
// pub mod integration;
/// User archetype in the SpaceKit network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum UserArchetype {
    BaseUser,
    Validator,
    Developer,
    Researcher,
    Investor,
    Regulator,
    Other,
}

/// Personality traits that modify behavior (1-100 possible combinations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalityProfile {
    pub archetype: UserArchetype,
    pub activity_level: u8,        // 1-10: How active they are
    pub consistency: u8,           // 1-10: How consistent their patterns are
    pub collaboration: u8,         // 1-10: How much they collaborate
    pub innovation: u8,            // 1-10: How innovative they are
    pub security_consciousness: u8, // 1-10: How security-conscious they are
    pub economic_engagement: u8,   // 1-10: How economically engaged they are
    pub cross_chain_preference: u8, // 1-10: How much they use cross-chain features
    pub peak_hours: Vec<u8>,       // Hours of day they're most active
    pub service_preferences: Vec<ServiceType>,
    pub risk_tolerance: u8,        // 1-10: Risk tolerance for new features
}

/// Types of services in the SpaceKit network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ServiceType {
    Compute,
    Storage,
    Messaging,
    AI,
    Identity,
    CrossChain,
    Encryption,
}

/// Interaction style classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum InteractionStyle {
    Collaborative,
    Independent,
    Supportive,
    Competitive,
    Suspicious,
}

/// Behavioral pattern data structure (based on advanced_network_features.rs)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralPattern {
    pub did: String,
    pub activity_frequency: f64,
    pub peak_activity_hours: Vec<u8>,
    pub service_preferences: Vec<ServiceType>,
    pub interaction_style: InteractionStyle,
    pub anomaly_score: f64,
    pub pattern_stability: f64,
    pub economic_participation: f64,
    pub cross_chain_activity: f64,
    pub security_compliance: f64,
}

/// Activity data for behavioral analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityData {
    pub transactions_per_hour: f64,
    pub hourly_activity: [f64; 24],
    pub service_usage: HashMap<ServiceType, u64>,
    pub success_rate: f64,
    pub quality_score: f64,
    pub uptime_percentage: f64,
    pub avg_response_time: f64,
    pub security_score: f64,
    pub innovation_score: f64,
    pub collaboration_requests: u64,
    pub help_requests: u64,
    pub competitive_actions: u64,
    pub total_requests: u64,
    pub suspicious_patterns: u64,
    pub unusual_timing_score: f64,
    pub rapid_requests: u64,
    pub reputation_manipulation_attempts: u64,
    pub cross_chain_transactions: u64,
    pub economic_transactions: u64,
    pub stake_duration_hours: f64,
    pub governance_participation: u64,
}

/// Detailed reputation score with behavioral analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedReputationScore {
    pub did: String,
    pub overall_score: f64,
    pub components: ReputationComponents,
    pub confidence_level: f64,
    pub trend: ReputationTrend,
    pub last_updated: u64,
    pub prediction_accuracy: f64,
}

/// Reputation score components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationComponents {
    pub service_quality: f64,
    pub reliability: f64,
    pub response_time: f64,
    pub security_compliance: f64,
    pub collaboration_score: f64,
    pub innovation_factor: f64,
    pub fraud_risk: f64,
    pub economic_consistency: f64,
    pub cross_chain_reliability: f64,
}

/// Reputation trend analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReputationTrend {
    Improving,
    Stable,
    Declining,
    Volatile,
}

/// Simulated user in the SpaceKit network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatedUser {
    pub did: String,
    pub personality: PersonalityProfile,
    pub behavioral_pattern: BehavioralPattern,
    pub activity_history: Vec<ActivityData>,
    pub reputation_history: Vec<DetailedReputationScore>,
    pub created_at: u64,
    pub last_active: u64,
    pub confidence_score: f64,
    pub recovery_attempts: u64,
}

/// Behavioral confidence recovery challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryChallenge {
    pub challenge_id: String,
    pub did: String,
    pub challenge_type: ChallengeType,
    pub challenge_data: Vec<u8>,
    pub expected_response: Vec<u8>,
    pub created_at: u64,
    pub expires_at: u64,
    pub difficulty: f64,
}

/// Types of behavioral challenges for identity recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChallengeType {
    ActivityPattern,
    ServicePreference,
    TimingPattern,
    InteractionStyle,
    EconomicBehavior,
    CrossChainPattern,
    ComprehensiveProfile,
}

/// Recovery attempt result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryAttempt {
    pub attempt_id: String,
    pub challenge_id: String,
    pub did: String,
    pub response: Vec<u8>,
    pub success: bool,
    pub confidence_score: f64,
    pub verification_time: u64,
    pub attempted_at: u64,
}

/// Simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    pub num_users: usize,
    pub simulation_days: u64,
    pub confidence_threshold: f64,
    pub recovery_threshold: f64,
    pub enable_fraud_simulation: bool,
    pub fraud_percentage: f64,
    pub personality_diversity: f64,
    pub random_seed: Option<u64>,
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            num_users: 1000,
            simulation_days: 30,
            confidence_threshold: 0.8,
            recovery_threshold: 0.7,
            enable_fraud_simulation: true,
            fraud_percentage: 0.05, // 5% fraud attempts
            personality_diversity: 0.8,
            random_seed: None,
        }
    }
}

/// Simulation results and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResults {
    pub config: SimulationConfig,
    pub users: Vec<SimulatedUser>,
    pub total_recovery_attempts: u64,
    pub successful_recoveries: u64,
    pub failed_recoveries: u64,
    pub fraud_attempts: u64,
    pub fraud_detections: u64,
    pub average_confidence_score: f64,
    pub archetype_performance: HashMap<UserArchetype, ArchetypeMetrics>,
    pub timeline_data: Vec<TimelineSnapshot>,
    pub execution_time_ms: u64,
}

/// Performance metrics by archetype
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeMetrics {
    pub archetype: UserArchetype,
    pub user_count: usize,
    pub average_confidence: f64,
    pub recovery_success_rate: f64,
    pub fraud_detection_rate: f64,
    pub pattern_stability: f64,
    pub economic_participation: f64,
}

/// Timeline snapshot for analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimelineSnapshot {
    pub day: u64,
    pub active_users: usize,
    pub average_confidence: f64,
    pub recovery_attempts: u64,
    pub successful_recoveries: u64,
    pub fraud_attempts: u64,
    pub network_health: f64,
}

/// Utility functions for the simulation
impl PersonalityProfile {
    /// Generate a random personality profile for a given archetype
    pub fn generate_for_archetype(archetype: UserArchetype, rng: &mut StdRng) -> Self {
        let base_traits = Self::get_archetype_base_traits(&archetype);
        
        Self {
            archetype: archetype.clone(),
            activity_level: Self::vary_trait(base_traits.0, rng),
            consistency: Self::vary_trait(base_traits.1, rng),
            collaboration: Self::vary_trait(base_traits.2, rng),
            innovation: Self::vary_trait(base_traits.3, rng),
            security_consciousness: Self::vary_trait(base_traits.4, rng),
            economic_engagement: Self::vary_trait(base_traits.5, rng),
            cross_chain_preference: Self::vary_trait(base_traits.6, rng),
            peak_hours: Self::generate_peak_hours(&archetype, rng),
            service_preferences: Self::generate_service_preferences(&archetype, rng),
            risk_tolerance: Self::vary_trait(base_traits.7, rng),
        }
    }
    
    /// Get base trait values for each archetype
    fn get_archetype_base_traits(archetype: &UserArchetype) -> (u8, u8, u8, u8, u8, u8, u8, u8) {
        match archetype {
            UserArchetype::BaseUser => (5, 6, 5, 4, 5, 4, 3, 5),
            UserArchetype::Validator => (8, 9, 7, 6, 9, 8, 6, 4),
            UserArchetype::Developer => (7, 7, 8, 9, 8, 6, 7, 6),
            UserArchetype::Researcher => (6, 8, 9, 8, 7, 5, 5, 7),
            UserArchetype::Investor => (4, 7, 4, 5, 6, 9, 8, 8),
            UserArchetype::Regulator => (5, 9, 6, 4, 10, 7, 4, 2),
            UserArchetype::Other => (5, 5, 5, 5, 5, 5, 5, 5),
        }
    }
    
    /// Vary a base trait value with some randomness
    fn vary_trait(base: u8, rng: &mut StdRng) -> u8 {
        let variation = rng.gen_range(-2..=2);
        ((base as i8 + variation).max(1).min(10)) as u8
    }
    
    /// Generate peak activity hours based on archetype
    fn generate_peak_hours(archetype: &UserArchetype, rng: &mut StdRng) -> Vec<u8> {
        let base_hours = match archetype {
            UserArchetype::BaseUser => vec![9, 10, 11, 14, 15, 16, 17],
            UserArchetype::Validator => vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23], // 24/7
            UserArchetype::Developer => vec![10, 11, 14, 15, 16, 17, 21, 22, 23],
            UserArchetype::Researcher => vec![9, 10, 11, 12, 13, 14, 15, 16],
            UserArchetype::Investor => vec![6, 7, 8, 9, 14, 15, 16, 20, 21],
            UserArchetype::Regulator => vec![9, 10, 11, 12, 13, 14, 15, 16],
            UserArchetype::Other => vec![rng.gen_range(6..=22), rng.gen_range(6..=22)],
        };
        
        // Add some variation
        let mut hours = base_hours;
        if rng.gen_bool(0.3) {
            hours.push(rng.gen_range(0..24));
        }
        hours.sort();
        hours.dedup();
        hours
    }
    
    /// Generate service preferences based on archetype
    fn generate_service_preferences(archetype: &UserArchetype, rng: &mut StdRng) -> Vec<ServiceType> {
        let base_services = match archetype {
            UserArchetype::BaseUser => vec![ServiceType::Compute, ServiceType::Storage],
            UserArchetype::Validator => vec![ServiceType::Compute, ServiceType::Identity, ServiceType::CrossChain],
            UserArchetype::Developer => vec![ServiceType::Compute, ServiceType::Storage, ServiceType::AI],
            UserArchetype::Researcher => vec![ServiceType::Storage, ServiceType::AI, ServiceType::Compute],
            UserArchetype::Investor => vec![ServiceType::CrossChain, ServiceType::Identity],
            UserArchetype::Regulator => vec![ServiceType::Identity, ServiceType::Encryption],
            UserArchetype::Other => vec![ServiceType::Messaging, ServiceType::Storage],
        };
        
        let mut services = base_services;
        
        // Add random services based on traits
        if rng.gen_bool(0.4) {
            let all_services = vec![
                ServiceType::Compute, ServiceType::Storage, ServiceType::Messaging,
                ServiceType::AI, ServiceType::Identity, ServiceType::CrossChain, ServiceType::Encryption
            ];
            
            let random_service = &all_services[rng.gen_range(0..all_services.len())];
            if !services.contains(random_service) {
                services.push(random_service.clone());
            }
        }
        
        services
    }
}

impl Default for ActivityData {
    fn default() -> Self {
        Self {
            transactions_per_hour: 1.0,
            hourly_activity: [0.0; 24],
            service_usage: HashMap::new(),
            success_rate: 0.95,
            quality_score: 0.8,
            uptime_percentage: 99.0,
            avg_response_time: 1000.0,
            security_score: 0.9,
            innovation_score: 0.5,
            collaboration_requests: 5,
            help_requests: 2,
            competitive_actions: 1,
            total_requests: 100,
            suspicious_patterns: 0,
            unusual_timing_score: 0.1,
            rapid_requests: 10,
            reputation_manipulation_attempts: 0,
            cross_chain_transactions: 10,
            economic_transactions: 50,
            stake_duration_hours: 168.0, // 1 week
            governance_participation: 5,
        }
    }
}

/// Get current timestamp
pub fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Generate a random DID for simulation
pub fn generate_did() -> String {
    format!("did:spacekit:simulation:{}", Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_personality_generation() {
        let mut rng = StdRng::seed_from_u64(12345);
        
        let developer = PersonalityProfile::generate_for_archetype(UserArchetype::Developer, &mut rng);
        assert_eq!(developer.archetype, UserArchetype::Developer);
        assert!(developer.innovation >= 6); // Developers should be innovative
        
        let validator = PersonalityProfile::generate_for_archetype(UserArchetype::Validator, &mut rng);
        assert_eq!(validator.archetype, UserArchetype::Validator);
        assert!(validator.security_consciousness >= 7); // Validators should be security-conscious
    }
    
    #[test]
    fn test_simulation_config() {
        let config = SimulationConfig::default();
        assert_eq!(config.num_users, 1000);
        assert_eq!(config.simulation_days, 30);
        assert_eq!(config.confidence_threshold, 0.8);
    }
}