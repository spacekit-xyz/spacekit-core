//! Behavioral Analysis Engine
//! 
//! Implements the ML-based behavioral analysis from the SWTCH whitepaper,
//! calculating reputation scores and detecting fraud patterns.

use crate::{
    ActivityData, BehavioralPattern, DetailedReputationScore, PersonalityProfile,
    ReputationComponents, ReputationTrend, ServiceType, InteractionStyle, UserArchetype,
    get_current_timestamp,
};
use anyhow::Result;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Behavioral analysis engine for calculating reputation and detecting patterns
pub struct BehavioralEngine {
    /// Historical patterns for comparison
    historical_patterns: HashMap<String, Vec<BehavioralPattern>>,
    /// Reputation history for trend analysis
    reputation_history: HashMap<String, Vec<DetailedReputationScore>>,
    /// ML model weights (simplified for simulation)
    model_weights: ModelWeights,
    /// Fraud detection thresholds
    fraud_thresholds: FraudThresholds,
}

/// ML model weights for reputation calculation
#[derive(Debug, Clone)]
struct ModelWeights {
    service_quality: f64,
    reliability: f64,
    response_time: f64,
    security_compliance: f64,
    collaboration: f64,
    innovation: f64,
    economic_consistency: f64,
    cross_chain_reliability: f64,
    pattern_stability: f64,
}

/// Fraud detection thresholds
#[derive(Debug, Clone)]
struct FraudThresholds {
    anomaly_score: f64,
    pattern_instability: f64,
    rapid_requests: u64,
    suspicious_patterns: u64,
    reputation_manipulation: u64,
}

impl Default for ModelWeights {
    fn default() -> Self {
        Self {
            service_quality: 0.20,
            reliability: 0.18,
            response_time: 0.12,
            security_compliance: 0.15,
            collaboration: 0.10,
            innovation: 0.08,
            economic_consistency: 0.10,
            cross_chain_reliability: 0.05,
            pattern_stability: 0.02,
        }
    }
}

impl Default for FraudThresholds {
    fn default() -> Self {
        Self {
            anomaly_score: 0.8,
            pattern_instability: 0.3,
            rapid_requests: 100,
            suspicious_patterns: 5,
            reputation_manipulation: 1,
        }
    }
}

impl BehavioralEngine {
    /// Create a new behavioral analysis engine
    pub fn new() -> Self {
        Self {
            historical_patterns: HashMap::new(),
            reputation_history: HashMap::new(),
            model_weights: ModelWeights::default(),
            fraud_thresholds: FraudThresholds::default(),
        }
    }
    
    /// Generate activity data based on personality profile and time
    pub fn generate_activity_data(
        &self,
        personality: &PersonalityProfile,
        day: u64,
        hour: u8,
        rng: &mut StdRng,
    ) -> ActivityData {
        let mut activity = ActivityData::default();
        
        // Calculate base activity level
        let base_activity = self.calculate_base_activity(personality, hour);
        activity.transactions_per_hour = base_activity;
        
        // Generate hourly activity pattern
        activity.hourly_activity = self.generate_hourly_pattern(personality);
        
        // Generate service usage based on preferences
        activity.service_usage = self.generate_service_usage(personality, base_activity, rng);
        
        // Calculate personality-based metrics
        activity.success_rate = self.calculate_success_rate(personality, rng);
        activity.quality_score = self.calculate_quality_score(personality, rng);
        activity.uptime_percentage = self.calculate_uptime(personality, rng);
        activity.avg_response_time = self.calculate_response_time(personality, rng);
        activity.security_score = self.calculate_security_score(personality, rng);
        activity.innovation_score = self.calculate_innovation_score(personality, rng);
        
        // Calculate interaction metrics
        let interaction_metrics = self.calculate_interaction_metrics(personality, base_activity, rng);
        activity.collaboration_requests = interaction_metrics.0;
        activity.help_requests = interaction_metrics.1;
        activity.competitive_actions = interaction_metrics.2;
        activity.total_requests = interaction_metrics.3;
        
        // Calculate economic and cross-chain metrics
        activity.cross_chain_transactions = self.calculate_cross_chain_activity(personality, base_activity, rng);
        activity.economic_transactions = self.calculate_economic_activity(personality, base_activity, rng);
        activity.stake_duration_hours = self.calculate_stake_duration(personality, rng);
        activity.governance_participation = self.calculate_governance_participation(personality, rng);
        
        // Calculate fraud indicators (most users should have very low values)
        if rng.gen_bool(0.95) { // 95% of users are legitimate
            activity.suspicious_patterns = 0;
            activity.unusual_timing_score = rng.gen_range(0.0..0.2);
            activity.reputation_manipulation_attempts = 0;
            activity.rapid_requests = rng.gen_range(0..20);
        } else { // 5% might show some suspicious patterns
            activity.suspicious_patterns = rng.gen_range(1..3);
            activity.unusual_timing_score = rng.gen_range(0.3..0.8);
            activity.reputation_manipulation_attempts = rng.gen_range(0..2);
            activity.rapid_requests = rng.gen_range(50..150);
        }
        
        activity
    }
    
    /// Calculate behavioral pattern from activity data
    pub fn calculate_behavioral_pattern(
        &mut self,
        did: &str,
        personality: &PersonalityProfile,
        activity: &ActivityData,
    ) -> Result<BehavioralPattern> {
        // Extract peak hours from activity pattern
        let peak_hours = self.extract_peak_hours(&activity.hourly_activity);
        
        // Determine interaction style
        let interaction_style = self.classify_interaction_style(activity, personality);
        
        // Calculate anomaly score
        let anomaly_score = self.calculate_anomaly_score(activity);
        
        // Calculate pattern stability
        let pattern_stability = self.calculate_pattern_stability(did, activity);
        
        // Calculate economic participation
        let economic_participation = self.calculate_economic_participation_score(activity);
        
        // Calculate cross-chain activity score
        let cross_chain_activity = self.calculate_cross_chain_activity_score(activity);
        
        // Calculate security compliance
        let security_compliance = activity.security_score * 
            (1.0 - anomaly_score * 0.2);
        
        let pattern = BehavioralPattern {
            did: did.to_string(),
            activity_frequency: activity.transactions_per_hour,
            peak_activity_hours: peak_hours,
            service_preferences: personality.service_preferences.clone(),
            interaction_style,
            anomaly_score,
            pattern_stability,
            economic_participation,
            cross_chain_activity,
            security_compliance,
        };
        
        // Store in history
        self.historical_patterns
            .entry(did.to_string())
            .or_insert_with(Vec::new)
            .push(pattern.clone());
        
        Ok(pattern)
    }
    
    /// Calculate comprehensive reputation score
    pub fn calculate_reputation_score(
        &mut self,
        did: &str,
        personality: &PersonalityProfile,
        activity: &ActivityData,
        pattern: &BehavioralPattern,
    ) -> Result<DetailedReputationScore> {
        // Calculate reputation components
        let components = self.calculate_reputation_components(activity, pattern);
        
        // Calculate weighted overall score
        let overall_score = self.calculate_weighted_score(&components);
        
        // Determine trend
        let trend = self.calculate_reputation_trend(did, overall_score);
        
        // Calculate confidence level
        let confidence_level = self.calculate_confidence_level(activity, pattern);
        
        let reputation = DetailedReputationScore {
            did: did.to_string(),
            overall_score,
            components,
            confidence_level,
            trend,
            last_updated: get_current_timestamp(),
            prediction_accuracy: 0.96, // Simulated ML model accuracy
        };
        
        // Store in history
        self.reputation_history
            .entry(did.to_string())
            .or_insert_with(Vec::new)
            .push(reputation.clone());
        
        Ok(reputation)
    }
    
    /// Calculate base activity level for a given hour
    fn calculate_base_activity(&self, personality: &PersonalityProfile, hour: u8) -> f64 {
        let base_rate = personality.activity_level as f64;
        
        // Apply peak hour multiplier
        let peak_multiplier = if personality.peak_hours.contains(&hour) {
            2.0
        } else {
            0.3
        };
        
        base_rate * peak_multiplier
    }
    
    /// Generate 24-hour activity pattern
    fn generate_hourly_pattern(&self, personality: &PersonalityProfile) -> [f64; 24] {
        let mut pattern = [0.0; 24];
        
        for hour in 0..24 {
            if personality.peak_hours.contains(&(hour as u8)) {
                pattern[hour] = personality.activity_level as f64 * 2.0;
            } else {
                pattern[hour] = personality.activity_level as f64 * 0.3;
            }
        }
        
        pattern
    }
    
    /// Generate service usage based on preferences
    fn generate_service_usage(
        &self,
        personality: &PersonalityProfile,
        base_activity: f64,
        rng: &mut StdRng,
    ) -> HashMap<ServiceType, u64> {
        let mut usage = HashMap::new();
        
        for service in &personality.service_preferences {
            let base_usage = (base_activity * 10.0) as u64;
            let variation = rng.gen_range(0.8..1.2);
            usage.insert(service.clone(), (base_usage as f64 * variation) as u64);
        }
        
        // Add some usage for non-preferred services
        let all_services = vec![
            ServiceType::Compute, ServiceType::Storage, ServiceType::Messaging,
            ServiceType::AI, ServiceType::Identity, ServiceType::CrossChain, ServiceType::Encryption
        ];
        
        for service in all_services {
            if !usage.contains_key(&service) && rng.gen_bool(0.3) {
                let low_usage = rng.gen_range(1..5);
                usage.insert(service, low_usage);
            }
        }
        
        usage
    }
    
    /// Calculate success rate based on personality
    fn calculate_success_rate(&self, personality: &PersonalityProfile, rng: &mut StdRng) -> f64 {
        let base_rate = match personality.archetype {
            UserArchetype::Validator => 0.98,
            UserArchetype::Developer => 0.95,
            UserArchetype::Researcher => 0.96,
            UserArchetype::Investor => 0.94,
            UserArchetype::Regulator => 0.97,
            UserArchetype::BaseUser | UserArchetype::Other => 0.92,
        };
        
        // Consistency is a small modifier around the archetype's base rate, not a
        // multiplier. Perfect consistency (10) leaves the base rate intact; lower
        // consistency trims it only slightly. The previous `consistency / 10.0`
        // factor (0.3–0.6 for most archetypes) drove every profile below the 0.7
        // clamp, so the base rate never showed through and success_rate was pinned
        // to exactly 0.7 for everyone — masking the archetype entirely.
        let consistency_factor = 0.90 + (personality.consistency as f64 / 100.0);
        let variation = rng.gen_range(-0.05..0.03);

        (base_rate * consistency_factor + variation).max(0.7).min(1.0)
    }
    
    /// Calculate quality score based on personality
    fn calculate_quality_score(&self, personality: &PersonalityProfile, rng: &mut StdRng) -> f64 {
        let base_quality = (personality.activity_level + personality.consistency) as f64 / 20.0;
        let innovation_bonus = personality.innovation as f64 / 100.0;
        let variation = rng.gen_range(-0.1..0.1);
        
        (base_quality + innovation_bonus + variation).max(0.5).min(1.0)
    }
    
    /// Calculate uptime based on personality
    fn calculate_uptime(&self, personality: &PersonalityProfile, rng: &mut StdRng) -> f64 {
        let base_uptime = match personality.archetype {
            UserArchetype::Validator => 99.5,
            UserArchetype::Developer => 95.0,
            UserArchetype::Researcher => 90.0,
            UserArchetype::BaseUser | UserArchetype::Other => 85.0,
            UserArchetype::Investor => 80.0,
            UserArchetype::Regulator => 95.0,
        };
        
        let consistency_bonus = personality.consistency as f64;
        let variation = rng.gen_range(-5.0..2.0);
        
        (base_uptime + consistency_bonus + variation).max(50.0).min(100.0)
    }
    
    /// Calculate response time based on personality
    fn calculate_response_time(&self, personality: &PersonalityProfile, rng: &mut StdRng) -> f64 {
        let base_time = match personality.archetype {
            UserArchetype::Validator => 200.0,
            UserArchetype::Developer => 800.0,
            UserArchetype::Researcher => 1500.0,
            UserArchetype::BaseUser | UserArchetype::Other => 2000.0,
            UserArchetype::Investor => 3000.0,
            UserArchetype::Regulator => 1000.0,
        };
        
        let efficiency_factor = (11 - personality.activity_level) as f64 / 10.0;
        let variation = rng.gen_range(0.8..1.5);
        
        base_time * efficiency_factor * variation
    }
    
    /// Calculate security score based on personality
    fn calculate_security_score(&self, personality: &PersonalityProfile, rng: &mut StdRng) -> f64 {
        let base_score = personality.security_consciousness as f64 / 10.0;
        let archetype_bonus = match personality.archetype {
            UserArchetype::Validator | UserArchetype::Regulator => 0.1,
            UserArchetype::Developer => 0.05,
            _ => 0.0,
        };
        let variation = rng.gen_range(-0.05..0.05);
        
        (base_score + archetype_bonus + variation).max(0.3).min(1.0)
    }
    
    /// Calculate innovation score based on personality
    fn calculate_innovation_score(&self, personality: &PersonalityProfile, rng: &mut StdRng) -> f64 {
        let base_score = personality.innovation as f64 / 10.0;
        let variation = rng.gen_range(-0.1..0.1);
        
        (base_score + variation).max(0.0).min(1.0)
    }
    
    /// Calculate interaction metrics
    fn calculate_interaction_metrics(
        &self,
        personality: &PersonalityProfile,
        base_activity: f64,
        rng: &mut StdRng,
    ) -> (u64, u64, u64, u64) {
        let total_requests = (base_activity * 50.0) as u64;
        
        let collaboration_ratio = personality.collaboration as f64 / 10.0;
        let collaboration_requests = (total_requests as f64 * collaboration_ratio * 0.3) as u64;
        
        let help_ratio = match personality.archetype {
            UserArchetype::Researcher | UserArchetype::Developer => 0.1,
            UserArchetype::BaseUser => 0.05,
            _ => 0.02,
        };
        let help_requests = (total_requests as f64 * help_ratio) as u64;
        
        let competitive_ratio = match personality.archetype {
            UserArchetype::Investor => 0.2,
            UserArchetype::Developer => 0.1,
            _ => 0.05,
        };
        let competitive_actions = (total_requests as f64 * competitive_ratio) as u64;
        
        (collaboration_requests, help_requests, competitive_actions, total_requests)
    }
    
    /// Calculate cross-chain activity
    fn calculate_cross_chain_activity(
        &self,
        personality: &PersonalityProfile,
        base_activity: f64,
        rng: &mut StdRng,
    ) -> u64 {
        let preference_factor = personality.cross_chain_preference as f64 / 10.0;
        let archetype_multiplier = match personality.archetype {
            UserArchetype::Investor => 3.0,
            UserArchetype::Validator => 2.0,
            UserArchetype::Developer => 1.5,
            _ => 1.0,
        };
        
        let base_transactions = base_activity * preference_factor * archetype_multiplier;
        let variation = rng.gen_range(0.8..1.2);
        
        (base_transactions * variation) as u64
    }
    
    /// Calculate economic activity
    fn calculate_economic_activity(
        &self,
        personality: &PersonalityProfile,
        base_activity: f64,
        rng: &mut StdRng,
    ) -> u64 {
        let engagement_factor = personality.economic_engagement as f64 / 10.0;
        let archetype_multiplier = match personality.archetype {
            UserArchetype::Investor => 5.0,
            UserArchetype::Validator => 3.0,
            UserArchetype::Developer => 2.0,
            UserArchetype::Researcher => 1.5,
            _ => 1.0,
        };
        
        let base_transactions = base_activity * engagement_factor * archetype_multiplier * 10.0;
        let variation = rng.gen_range(0.7..1.3);
        
        (base_transactions * variation) as u64
    }
    
    /// Calculate stake duration
    fn calculate_stake_duration(&self, personality: &PersonalityProfile, rng: &mut StdRng) -> f64 {
        let base_duration = match personality.archetype {
            UserArchetype::Validator => 720.0, // 1 month
            UserArchetype::Investor => 2160.0, // 3 months
            UserArchetype::Developer => 168.0, // 1 week
            UserArchetype::Researcher => 336.0, // 2 weeks
            _ => 72.0, // 3 days
        };
        
        let consistency_factor = personality.consistency as f64 / 10.0;
        let variation = rng.gen_range(0.5..2.0);
        
        base_duration * consistency_factor * variation
    }
    
    /// Calculate governance participation
    fn calculate_governance_participation(&self, personality: &PersonalityProfile, rng: &mut StdRng) -> u64 {
        let base_participation = match personality.archetype {
            UserArchetype::Validator => 20,
            UserArchetype::Regulator => 15,
            UserArchetype::Developer => 10,
            UserArchetype::Researcher => 8,
            UserArchetype::Investor => 5,
            _ => 2,
        };
        
        let engagement_factor = personality.economic_engagement as f64 / 10.0;
        let variation = rng.gen_range(0.5..1.5);
        
        (base_participation as f64 * engagement_factor * variation) as u64
    }
    
    /// Extract peak hours from activity pattern
    fn extract_peak_hours(&self, hourly_activity: &[f64; 24]) -> Vec<u8> {
        let avg_activity = hourly_activity.iter().sum::<f64>() / 24.0;
        let threshold = avg_activity * 1.5;
        
        hourly_activity
            .iter()
            .enumerate()
            .filter(|(_, &activity)| activity > threshold)
            .map(|(hour, _)| hour as u8)
            .collect()
    }
    
    /// Classify interaction style
    fn classify_interaction_style(&self, activity: &ActivityData, personality: &PersonalityProfile) -> InteractionStyle {
        let collaboration_ratio = activity.collaboration_requests as f64 / (activity.total_requests as f64 + 1.0);
        let help_ratio = activity.help_requests as f64 / (activity.total_requests as f64 + 1.0);
        let competitive_ratio = activity.competitive_actions as f64 / (activity.total_requests as f64 + 1.0);
        
        if activity.suspicious_patterns > 3 || activity.reputation_manipulation_attempts > 0 {
            InteractionStyle::Suspicious
        } else if collaboration_ratio > 0.2 || personality.collaboration >= 8 {
            InteractionStyle::Collaborative
        } else if help_ratio > 0.1 || personality.archetype == UserArchetype::Researcher {
            InteractionStyle::Supportive
        } else if competitive_ratio > 0.15 || personality.archetype == UserArchetype::Investor {
            InteractionStyle::Competitive
        } else {
            InteractionStyle::Independent
        }
    }
    
    /// Calculate anomaly score
    fn calculate_anomaly_score(&self, activity: &ActivityData) -> f64 {
        let mut score = 0.0;
        
        // Unusual timing patterns
        if activity.unusual_timing_score > 0.5 {
            score += 0.3;
        }
        
        // Suspicious patterns
        score += (activity.suspicious_patterns as f64) * 0.1;
        
        // Rapid requests
        if activity.rapid_requests > self.fraud_thresholds.rapid_requests {
            score += 0.2;
        }
        
        // Reputation manipulation
        if activity.reputation_manipulation_attempts > 0 {
            score += 0.4;
        }
        
        // Low success rate
        if activity.success_rate < 0.7 {
            score += 0.2;
        }
        
        score.min(1.0)
    }
    
    /// Calculate pattern stability
    fn calculate_pattern_stability(&self, did: &str, activity: &ActivityData) -> f64 {
        if let Some(patterns) = self.historical_patterns.get(did) {
            if patterns.is_empty() {
                return 0.5; // Default for new users
            }
            
            let last_pattern = &patterns[patterns.len() - 1];
            let mut stability = 1.0;
            
            // Compare activity frequency
            let freq_diff = (activity.transactions_per_hour - last_pattern.activity_frequency).abs();
            stability -= freq_diff * 0.05;
            
            // Compare anomaly scores
            let anomaly_diff = (self.calculate_anomaly_score(activity) - last_pattern.anomaly_score).abs();
            stability -= anomaly_diff * 0.2;
            
            stability.max(0.0)
        } else {
            0.5 // Default for new users
        }
    }
    
    /// Calculate economic participation score
    fn calculate_economic_participation_score(&self, activity: &ActivityData) -> f64 {
        let stake_factor = (activity.stake_duration_hours / 720.0).min(1.0); // Normalize to 1 month
        let transaction_factor = (activity.economic_transactions as f64 / 100.0).min(1.0);
        let governance_factor = (activity.governance_participation as f64 / 20.0).min(1.0);
        
        (stake_factor + transaction_factor + governance_factor) / 3.0
    }
    
    /// Calculate cross-chain activity score
    fn calculate_cross_chain_activity_score(&self, activity: &ActivityData) -> f64 {
        (activity.cross_chain_transactions as f64 / 50.0).min(1.0)
    }
    
    /// Calculate reputation components
    fn calculate_reputation_components(&self, activity: &ActivityData, pattern: &BehavioralPattern) -> ReputationComponents {
        ReputationComponents {
            service_quality: (activity.success_rate * 0.4 + activity.quality_score * 0.6) * 
                            (1.0 - pattern.anomaly_score * 0.2),
            reliability: (activity.uptime_percentage / 100.0 * 0.6 + pattern.pattern_stability * 0.4) * 
                        (1.0 - pattern.anomaly_score * 0.1),
            response_time: (1.0 - (activity.avg_response_time / 10000.0)).max(0.0),
            security_compliance: pattern.security_compliance,
            collaboration_score: (activity.collaboration_requests as f64) / (activity.total_requests as f64 + 1.0),
            innovation_factor: activity.innovation_score,
            fraud_risk: pattern.anomaly_score,
            economic_consistency: pattern.economic_participation,
            cross_chain_reliability: pattern.cross_chain_activity,
        }
    }
    
    /// Calculate weighted overall score
    fn calculate_weighted_score(&self, components: &ReputationComponents) -> f64 {
        let weights = &self.model_weights;
        
        let score = components.service_quality * weights.service_quality +
                   components.reliability * weights.reliability +
                   components.response_time * weights.response_time +
                   components.security_compliance * weights.security_compliance +
                   components.collaboration_score * weights.collaboration +
                   components.innovation_factor * weights.innovation +
                   components.economic_consistency * weights.economic_consistency +
                   components.cross_chain_reliability * weights.cross_chain_reliability -
                   components.fraud_risk * 0.05; // Negative weight for fraud risk
        
        score.max(0.0).min(1.0)
    }
    
    /// Calculate reputation trend
    fn calculate_reputation_trend(&self, did: &str, current_score: f64) -> ReputationTrend {
        if let Some(history) = self.reputation_history.get(did) {
            if history.is_empty() {
                return ReputationTrend::Stable;
            }
            
            let previous_score = history[history.len() - 1].overall_score;
            let diff = current_score - previous_score;
            
            if diff > 0.05 {
                ReputationTrend::Improving
            } else if diff < -0.05 {
                ReputationTrend::Declining
            } else if diff.abs() > 0.02 {
                ReputationTrend::Volatile
            } else {
                ReputationTrend::Stable
            }
        } else {
            ReputationTrend::Stable
        }
    }
    
    /// Calculate confidence level
    fn calculate_confidence_level(&self, activity: &ActivityData, pattern: &BehavioralPattern) -> f64 {
        let mut confidence = 0.5; // Base confidence
        
        // More data points increase confidence
        confidence += (activity.total_requests as f64 / 1000.0).min(0.3);
        
        // Stable patterns increase confidence
        confidence += pattern.pattern_stability * 0.2;
        
        // Less anomalies increase confidence
        confidence += (1.0 - pattern.anomaly_score) * 0.2;
        
        // Longer participation increases confidence
        confidence += (activity.stake_duration_hours / 2160.0).min(0.1); // Normalize to 3 months
        
        confidence.min(1.0)
    }
}

impl Default for BehavioralEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PersonalityProfile, UserArchetype};
    
    #[test]
    fn test_behavioral_engine_creation() {
        let engine = BehavioralEngine::new();
        assert!(engine.historical_patterns.is_empty());
        assert!(engine.reputation_history.is_empty());
    }
    
    #[test]
    fn test_activity_generation() {
        let engine = BehavioralEngine::new();
        let mut rng = StdRng::seed_from_u64(12345);
        
        let personality = PersonalityProfile::generate_for_archetype(UserArchetype::Developer, &mut rng);
        let activity = engine.generate_activity_data(&personality, 1, 10, &mut rng);
        
        assert!(activity.transactions_per_hour > 0.0);
        // Developer base rate is 0.95, trimmed only slightly by consistency +
        // variation, so it lands well above 0.8 and at/under 1.0 — a real bound,
        // not the old `> 0.65` that merely tested the 0.7 clamp floor.
        assert!(activity.success_rate > 0.8);
        assert!(activity.success_rate <= 1.0);
        assert!(!activity.service_usage.is_empty());
    }
    
    #[test]
    fn test_reputation_calculation() {
        let mut engine = BehavioralEngine::new();
        let mut rng = StdRng::seed_from_u64(12345);
        
        let personality = PersonalityProfile::generate_for_archetype(UserArchetype::Validator, &mut rng);
        let activity = engine.generate_activity_data(&personality, 1, 10, &mut rng);
        let pattern = engine.calculate_behavioral_pattern("test_did", &personality, &activity).unwrap();
        let reputation = engine.calculate_reputation_score("test_did", &personality, &activity, &pattern).unwrap();
        
        assert!(reputation.overall_score >= 0.0);
        assert!(reputation.overall_score <= 1.0);
        assert!(reputation.confidence_level > 0.0);
    }
}