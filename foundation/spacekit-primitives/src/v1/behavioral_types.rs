//! Behavioral Cryptography Core Types
//!
//! Fundamental behavioral types for the SWTCH network's behavioral cryptography system.

use alloy_primitives::{Address, U256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// User archetype in the SWTCH network
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

/// Types of services in the SWTCH network
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

/// Interaction style classification for behavioral analysis
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum InteractionStyle {
    Collaborative,
    Independent,
    Supportive,
    Competitive,
    Suspicious,
}

/// Core personality traits for behavioral profiling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalityTraits {
    pub archetype: UserArchetype,
    pub activity_level: u8,
    pub consistency: u8,
    pub collaboration: u8,
    pub innovation: u8,
    pub security_consciousness: u8,
    pub economic_engagement: u8,
    pub cross_chain_preference: u8,
    pub peak_hours: Vec<u8>,
    pub service_preferences: Vec<ServiceType>,
    pub risk_tolerance: u8,
}

/// Behavioral confidence score for identity recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralConfidenceScore {
    pub address: Address,
    pub overall_confidence: f64,
    pub components: ConfidenceComponents,
    pub trend: ConfidenceTrend,
    pub data_points: u32,
    pub calculated_at: DateTime<Utc>,
    pub model_version: String,
}

/// Individual components of behavioral confidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceComponents {
    pub pattern_consistency: f64,
    pub service_predictability: f64,
    pub timing_reliability: f64,
    pub economic_consistency: f64,
    pub cross_chain_consistency: f64,
    pub peer_endorsement: f64,
    pub longevity_bonus: f64,
}

/// Confidence trend over time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfidenceTrend {
    Improving,
    Stable,
    Declining,
    Volatile,
    Insufficient,
}

/// Behavioral fingerprint for identity recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralFingerprint {
    pub fingerprint_id: String,
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Archetype-specific behavioral expectations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeBehavioralExpectations {
    pub archetype: UserArchetype,
    pub expected_activity_range: (f64, f64),
    pub expected_peak_hours: Vec<u8>,
    pub expected_services: Vec<ServiceType>,
    pub expected_interaction_style: InteractionStyle,
    pub expected_consistency_range: (f64, f64),
    pub expected_economic_range: (f64, f64),
    pub description: String,
}

impl UserArchetype {
    pub fn description(&self) -> &'static str {
        match self {
            UserArchetype::BaseUser => "Standard network user with moderate activity",
            UserArchetype::Validator => "Infrastructure operator with 24/7 high availability",
            UserArchetype::Developer => "Software builder with variable but intense patterns",
            UserArchetype::Researcher => "Academic/industry researcher with consistent patterns",
            UserArchetype::Investor => "Economic participant with market-driven activity",
            UserArchetype::Regulator => "Compliance entity with strict operational patterns",
            UserArchetype::Other => "Miscellaneous user with diverse patterns",
        }
    }

    pub fn population_percentage(&self) -> f64 {
        match self {
            UserArchetype::BaseUser => 0.40,
            UserArchetype::Validator => 0.15,
            UserArchetype::Developer => 0.20,
            UserArchetype::Researcher => 0.10,
            UserArchetype::Investor => 0.08,
            UserArchetype::Regulator => 0.02,
            UserArchetype::Other => 0.05,
        }
    }

    pub fn default_expectations(&self) -> ArchetypeBehavioralExpectations {
        match self {
            UserArchetype::BaseUser => ArchetypeBehavioralExpectations {
                archetype: self.clone(),
                expected_activity_range: (4.0, 8.0),
                expected_peak_hours: vec![9, 10, 11, 14, 15, 16, 17],
                expected_services: vec![
                    ServiceType::Compute,
                    ServiceType::Storage,
                    ServiceType::Messaging,
                ],
                expected_interaction_style: InteractionStyle::Collaborative,
                expected_consistency_range: (0.6, 0.8),
                expected_economic_range: (0.3, 0.6),
                description: "Typical network user with moderate, predictable activity".to_string(),
            },
            UserArchetype::Validator => ArchetypeBehavioralExpectations {
                archetype: self.clone(),
                expected_activity_range: (16.0, 30.0),
                expected_peak_hours: (0..24).collect(),
                expected_services: vec![
                    ServiceType::Compute,
                    ServiceType::Identity,
                    ServiceType::CrossChain,
                ],
                expected_interaction_style: InteractionStyle::Independent,
                expected_consistency_range: (0.9, 1.0),
                expected_economic_range: (0.7, 1.0),
                description: "Infrastructure operator with continuous high availability"
                    .to_string(),
            },
            UserArchetype::Developer => ArchetypeBehavioralExpectations {
                archetype: self.clone(),
                expected_activity_range: (9.0, 15.0),
                expected_peak_hours: vec![10, 11, 14, 15, 16, 17, 21, 22, 23],
                expected_services: vec![
                    ServiceType::Compute,
                    ServiceType::Storage,
                    ServiceType::AI,
                ],
                expected_interaction_style: InteractionStyle::Collaborative,
                expected_consistency_range: (0.4, 0.7),
                expected_economic_range: (0.5, 0.8),
                description: "Software developer with variable but intense activity patterns"
                    .to_string(),
            },
            UserArchetype::Researcher => ArchetypeBehavioralExpectations {
                archetype: self.clone(),
                expected_activity_range: (4.0, 8.0),
                expected_peak_hours: vec![9, 10, 11, 12, 13, 14, 15, 16],
                expected_services: vec![
                    ServiceType::Storage,
                    ServiceType::AI,
                    ServiceType::Compute,
                ],
                expected_interaction_style: InteractionStyle::Supportive,
                expected_consistency_range: (0.8, 0.9),
                expected_economic_range: (0.3, 0.6),
                description: "Academic researcher with methodical, consistent patterns".to_string(),
            },
            UserArchetype::Investor => ArchetypeBehavioralExpectations {
                archetype: self.clone(),
                expected_activity_range: (9.0, 15.0),
                expected_peak_hours: vec![6, 7, 8, 9, 14, 15, 16, 20, 21],
                expected_services: vec![ServiceType::CrossChain, ServiceType::Identity],
                expected_interaction_style: InteractionStyle::Competitive,
                expected_consistency_range: (0.4, 0.7),
                expected_economic_range: (0.8, 1.0),
                description: "Economic participant with market-driven activity".to_string(),
            },
            UserArchetype::Regulator => ArchetypeBehavioralExpectations {
                archetype: self.clone(),
                expected_activity_range: (1.0, 3.0),
                expected_peak_hours: vec![9, 10, 11, 12, 13, 14, 15, 16],
                expected_services: vec![ServiceType::Identity, ServiceType::Encryption],
                expected_interaction_style: InteractionStyle::Independent,
                expected_consistency_range: (0.9, 1.0),
                expected_economic_range: (0.2, 0.4),
                description: "Regulatory entity with strict operational patterns".to_string(),
            },
            UserArchetype::Other => ArchetypeBehavioralExpectations {
                archetype: self.clone(),
                expected_activity_range: (1.0, 5.0),
                expected_peak_hours: vec![12, 18],
                expected_services: vec![ServiceType::Messaging, ServiceType::Storage],
                expected_interaction_style: InteractionStyle::Independent,
                expected_consistency_range: (0.2, 0.6),
                expected_economic_range: (0.1, 0.4),
                description: "Miscellaneous user with diverse, unpredictable patterns".to_string(),
            },
        }
    }
}

impl Default for ConfidenceComponents {
    fn default() -> Self {
        Self {
            pattern_consistency: 0.5,
            service_predictability: 0.5,
            timing_reliability: 0.5,
            economic_consistency: 0.5,
            cross_chain_consistency: 0.5,
            peer_endorsement: 0.5,
            longevity_bonus: 0.0,
        }
    }
}

/// Network participation and activity metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkParticipationMetrics {
    /// Total time spent active on network
    pub total_active_time_hours: u64,
    /// Number of different services used
    pub services_utilized_count: usize,
    /// Cross-chain activity frequency
    pub cross_chain_interactions: u64,
    /// Consensus participation rate
    pub consensus_participation_rate: f64,
    /// Average session duration in minutes
    pub avg_session_duration_minutes: f64,
    /// Network reliability score
    pub uptime_percentage: f64,
    /// Geographic distribution diversity
    pub geographic_diversity_score: f64,
}
