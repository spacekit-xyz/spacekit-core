pub mod pattern_analyzer;
pub mod fingerprint;
pub mod confidence_scorer;

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use ndarray::Array1;

/// Core behavioral patterns as defined in the whitepaper
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralPatterns {
    /// Storage contribution patterns, storage duration consistency, geographic distribution
    pub storage_behavior: StoragePattern,
    /// CPU/bandwidth contribution schedules, preferred computation types, service quality metrics
    pub compute_participation: ComputePattern,
    /// Token earning consistency, stake duration, service fee payment patterns
    pub economic_patterns: EconomicPattern,
    /// Peer ratings from VPoS system, successful transaction ratios, response time consistency
    pub service_quality: ServiceQualityMetrics,
    /// Cross-chain interaction patterns, preferred networks, transaction timing
    pub multi_chain_activity: MultiChainPattern,
    /// Timestamp when patterns were collected
    pub collected_at: DateTime<Utc>,
    /// Privacy budget used for this collection
    pub privacy_budget_used: f64,
}

/// Storage behavior patterns - file sharing patterns, storage duration consistency
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct StoragePattern {
    /// Average daily storage contribution in GB
    pub avg_daily_storage_gb: f64,
    /// Storage consistency score (0.0 to 1.0)
    pub consistency_score: f64,
    /// Geographic distribution preferences (encoded as vector)
    pub geographic_preferences: Array1<f64>,
    /// Storage duration patterns (average file retention in days)
    pub avg_retention_days: f64,
    /// Preferred storage times (hourly distribution)
    pub preferred_storage_hours: Array1<f64>,
}

/// Compute participation patterns - CPU/bandwidth contribution schedules
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ComputePattern {
    /// Average daily compute contribution in CPU hours
    pub avg_daily_compute_hours: f64,
    /// Bandwidth contribution in GB/day
    pub avg_daily_bandwidth_gb: f64,
    /// Availability pattern (24-hour distribution)
    pub availability_pattern: Array1<f64>,
    /// Preferred computation types (ML, storage, messaging, etc.)
    pub preferred_compute_types: Vec<String>,
    /// Service quality score (0.0 to 1.0)
    pub service_quality: f64,
}

/// Economic behavior patterns - token earning, staking, fee payments
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct EconomicPattern {
    /// Token earning consistency over time
    pub earning_consistency: f64,
    /// Average stake duration in days
    pub avg_stake_duration: f64,
    /// Fee payment punctuality score (0.0 to 1.0)
    pub payment_punctuality: f64,
    /// Bonding curve interaction frequency
    pub bonding_curve_interactions: u64,
    /// Economic participation score
    pub participation_score: f64,
}

/// Service quality metrics from VPoS system
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ServiceQualityMetrics {
    /// Peer rating average (0.0 to 5.0)
    pub peer_rating_avg: f64,
    /// Successful transaction ratio (0.0 to 1.0)
    pub success_ratio: f64,
    /// Average response time in milliseconds
    pub avg_response_time_ms: f64,
    /// Reputation score accumulation rate
    pub reputation_accumulation: f64,
    /// Total completed services
    pub total_services_completed: u64,
}

/// Multi-chain activity patterns across supported blockchains
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct MultiChainPattern {
    /// Chain usage distribution (Ethereum, Avalanche, Arbitrum, Polygon, Cosmos, Solana)
    pub chain_usage_distribution: Array1<f64>,
    /// Cross-chain transaction frequency
    pub cross_chain_tx_frequency: f64,
    /// Preferred networks for different activities
    pub preferred_networks: Vec<String>,
    /// Bridge usage patterns
    pub bridge_usage_frequency: f64,
    /// Multi-chain identity consistency score
    pub identity_consistency: f64,
}

/// Encrypted behavioral fingerprint for privacy-preserving verification
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct BehavioralFingerprint {
    /// Homomorphically encrypted fingerprint data
    pub encrypted_fingerprint: Vec<u8>,
    /// Differential privacy parameters used
    pub epsilon: f64,
    pub delta: f64,
    /// Timestamp of fingerprint creation
    pub created_at: DateTime<Utc>,
    /// Identity commitment (quantum-resistant)
    pub identity_commitment: Vec<u8>,
}

/// Confidence score computed using homomorphic encryption
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceScore {
    /// Encrypted confidence value
    pub encrypted_score: Vec<u8>,
    /// Confidence threshold required for recovery
    pub threshold: f64,
    /// Contributing factors breakdown
    pub factor_weights: ConfidenceFactors,
    /// Timestamp of calculation
    pub calculated_at: DateTime<Utc>,
}

/// Breakdown of confidence score factors as per whitepaper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidenceFactors {
    /// Network participation weight
    pub network_participation_weight: f64,
    /// Peer endorsement weight  
    pub peer_endorsement_weight: f64,
    /// Service quality weight
    pub service_quality_weight: f64,
    /// Economic consistency weight
    pub economic_consistency_weight: f64,
    /// Multi-chain behavior weight
    pub multi_chain_behavior_weight: f64,
    /// Temporal weighting factor
    pub temporal_weighting: f64,
}

impl BehavioralPatterns {
    /// Create properly initialized behavioral patterns for testing
    pub fn new_for_archetype(archetype: &spacekit_primitives::v1::behavioral_types::UserArchetype) -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        
        let expectations = archetype.default_expectations();
        let activity_level = (expectations.expected_activity_range.0 + expectations.expected_activity_range.1) / 2.0;
        
        // Create 24-hour activity pattern
        let mut hourly_pattern = vec![0.0; 24];
        for hour in &expectations.expected_peak_hours {
            if *hour < 24 {
                hourly_pattern[*hour as usize] = 0.8 + rng.r#gen::<f64>() * 0.2; // 0.8-1.0 for peak hours
            }
        }
        
        // Create 6-region geographic distribution (normalized)
        let geo_prefs: Vec<f64> = (0..6).map(|_| rng.r#gen::<f64>()).collect();
        let geo_sum: f64 = geo_prefs.iter().sum();
        let geo_normalized: Vec<f64> = geo_prefs.iter().map(|x| x / geo_sum).collect();
        
        // Create 6-chain distribution for multi-chain (normalized)
        let chain_dist: Vec<f64> = (0..6).map(|_| rng.r#gen::<f64>()).collect();
        let chain_sum: f64 = chain_dist.iter().sum();
        let chain_normalized: Vec<f64> = chain_dist.iter().map(|x| x / chain_sum).collect();
        
        Self {
            storage_behavior: StoragePattern {
                avg_daily_storage_gb: activity_level * 10.0,
                consistency_score: expectations.expected_consistency_range.1,
                geographic_preferences: Array1::from_vec(geo_normalized),
                avg_retention_days: 30.0,
                preferred_storage_hours: Array1::from_vec(hourly_pattern.clone()),
            },
            compute_participation: ComputePattern {
                avg_daily_compute_hours: activity_level * 2.0,
                avg_daily_bandwidth_gb: activity_level * 5.0,
                availability_pattern: Array1::from_vec(hourly_pattern.clone()),
                preferred_compute_types: vec!["ML".to_string(), "Storage".to_string()],
                service_quality: expectations.expected_consistency_range.1,
            },
            economic_patterns: EconomicPattern {
                earning_consistency: expectations.expected_consistency_range.1,
                avg_stake_duration: expectations.expected_economic_range.1 * 100.0,
                payment_punctuality: expectations.expected_consistency_range.1,
                bonding_curve_interactions: (activity_level * 10.0) as u64,
                participation_score: expectations.expected_economic_range.1,
            },
            service_quality: ServiceQualityMetrics {
                peer_rating_avg: 4.0 + rng.r#gen::<f64>(),
                success_ratio: 0.85 + rng.r#gen::<f64>() * 0.10,
                avg_response_time_ms: 100.0 + rng.r#gen::<f64>() * 200.0,
                reputation_accumulation: expectations.expected_consistency_range.1 * 10.0,
                total_services_completed: (activity_level * 100.0) as u64,
            },
            multi_chain_activity: MultiChainPattern {
                chain_usage_distribution: Array1::from_vec(chain_normalized),
                cross_chain_tx_frequency: activity_level,
                preferred_networks: vec!["Ethereum".to_string(), "Avalanche".to_string()],
                bridge_usage_frequency: activity_level * 0.5,
                identity_consistency: expectations.expected_consistency_range.1,
            },
            collected_at: Utc::now(),
            privacy_budget_used: 0.0,
        }
    }
}

// Re-export main types
pub use pattern_analyzer::BehavioralPatternAnalyzer;
pub use fingerprint::BehavioralFingerprintGenerator;
pub use confidence_scorer::{ConfidenceScorer, NetworkBehavioralStats, PeerEndorsementMatrix, EndorsementRecord, EndorsementType};
