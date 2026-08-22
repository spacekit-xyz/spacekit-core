//! Confidence Recovery System
//! 
//! Implements the behavioral cryptography identity recovery mechanism
//! from the SWTCH whitepaper - keyless entry through behavioral patterns.

use crate::{
    BehavioralPattern, DetailedReputationScore, RecoveryChallenge, RecoveryAttempt,
    ChallengeType, PersonalityProfile, ServiceType, InteractionStyle,
    get_current_timestamp, generate_did,
};
use anyhow::{Result, anyhow};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Behavioral confidence recovery system
pub struct ConfidenceRecoverySystem {
    /// Historical behavioral patterns for all users
    user_patterns: HashMap<String, Vec<BehavioralPattern>>,
    /// Reputation history for all users
    user_reputations: HashMap<String, Vec<DetailedReputationScore>>,
    /// Active recovery challenges
    active_challenges: HashMap<String, RecoveryChallenge>,
    /// Recovery attempt history
    recovery_history: HashMap<String, Vec<RecoveryAttempt>>,
    /// Configuration thresholds
    config: RecoveryConfig,
}

/// Configuration for recovery system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    pub minimum_confidence_threshold: f64,
    pub minimum_pattern_history: usize,
    pub challenge_difficulty_base: f64,
    pub challenge_timeout_hours: u64,
    pub max_recovery_attempts: u64,
    pub pattern_similarity_threshold: f64,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            minimum_confidence_threshold: 0.7,
            minimum_pattern_history: 10, // Minimum behavioral data points
            challenge_difficulty_base: 0.8,
            challenge_timeout_hours: 24,
            max_recovery_attempts: 3,
            pattern_similarity_threshold: 0.85,
        }
    }
}

/// Recovery challenge data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeData {
    pub expected_peak_hours: Vec<u8>,
    pub expected_services: Vec<ServiceType>,
    pub expected_interaction_style: InteractionStyle,
    pub expected_activity_range: (f64, f64),
    pub expected_economic_pattern: f64,
    pub expected_cross_chain_activity: f64,
}

/// Recovery response data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseData {
    pub claimed_peak_hours: Vec<u8>,
    pub claimed_services: Vec<ServiceType>,
    pub claimed_interaction_style: InteractionStyle,
    pub claimed_activity_level: f64,
    pub claimed_economic_pattern: f64,
    pub claimed_cross_chain_activity: f64,
    pub behavioral_proof: Vec<u8>, // Zero-knowledge proof of behavioral consistency
}

impl ConfidenceRecoverySystem {
    /// Create a new confidence recovery system
    pub fn new() -> Self {
        Self {
            user_patterns: HashMap::new(),
            user_reputations: HashMap::new(),
            active_challenges: HashMap::new(),
            recovery_history: HashMap::new(),
            config: RecoveryConfig::default(),
        }
    }
    
    /// Register user behavioral data
    pub fn register_user_data(
        &mut self,
        did: &str,
        pattern: BehavioralPattern,
        reputation: DetailedReputationScore,
    ) {
        // Store behavioral pattern
        self.user_patterns
            .entry(did.to_string())
            .or_insert_with(Vec::new)
            .push(pattern);
        
        // Store reputation data
        self.user_reputations
            .entry(did.to_string())
            .or_insert_with(Vec::new)
            .push(reputation);
    }
    
    /// Check if user is eligible for behavioral recovery
    pub fn is_recovery_eligible(&self, did: &str) -> bool {
        // Check if user has sufficient behavioral history
        if let Some(patterns) = self.user_patterns.get(did) {
            if patterns.len() < self.config.minimum_pattern_history {
                return false;
            }
            
            // Check if user has sufficient confidence
            if let Some(reputations) = self.user_reputations.get(did) {
                if let Some(latest_reputation) = reputations.last() {
                    return latest_reputation.confidence_level >= self.config.minimum_confidence_threshold;
                }
            }
        }
        
        false
    }
    
    /// Generate a behavioral recovery challenge
    pub fn generate_recovery_challenge(
        &mut self,
        did: &str,
        challenge_type: ChallengeType,
        rng: &mut StdRng,
    ) -> Result<RecoveryChallenge> {
        // Check eligibility
        if !self.is_recovery_eligible(did) {
            return Err(anyhow!("User not eligible for behavioral recovery"));
        }
        
        // Check if user already has too many active challenges
        let active_count = self.active_challenges.values()
            .filter(|c| c.did == did)
            .count();
        
        if active_count >= self.config.max_recovery_attempts as usize {
            return Err(anyhow!("Too many active recovery attempts"));
        }
        
        // Get user's behavioral history
        let patterns = self.user_patterns.get(did)
            .ok_or_else(|| anyhow!("No behavioral patterns found"))?;
        
        let reputations = self.user_reputations.get(did)
            .ok_or_else(|| anyhow!("No reputation history found"))?;
        
        // Generate challenge based on type
        let (challenge_data, expected_response) = match challenge_type {
            ChallengeType::ActivityPattern => self.generate_activity_challenge(patterns, rng)?,
            ChallengeType::ServicePreference => self.generate_service_challenge(patterns, rng)?,
            ChallengeType::TimingPattern => self.generate_timing_challenge(patterns, rng)?,
            ChallengeType::InteractionStyle => self.generate_interaction_challenge(patterns, rng)?,
            ChallengeType::EconomicBehavior => self.generate_economic_challenge(patterns, rng)?,
            ChallengeType::CrossChainPattern => self.generate_cross_chain_challenge(patterns, rng)?,
            ChallengeType::ComprehensiveProfile => self.generate_comprehensive_challenge(patterns, reputations, rng)?,
        };
        
        // Calculate difficulty based on user's historical consistency
        let difficulty = self.calculate_challenge_difficulty(patterns, &challenge_type);
        
        let challenge = RecoveryChallenge {
            challenge_id: generate_did(),
            did: did.to_string(),
            challenge_type,
            challenge_data,
            expected_response,
            created_at: get_current_timestamp(),
            expires_at: get_current_timestamp() + (self.config.challenge_timeout_hours * 3600),
            difficulty,
        };
        
        // Store active challenge
        self.active_challenges.insert(challenge.challenge_id.clone(), challenge.clone());
        
        Ok(challenge)
    }
    
    /// Verify a recovery attempt
    pub fn verify_recovery_attempt(
        &mut self,
        challenge_id: &str,
        response_data: Vec<u8>,
    ) -> Result<RecoveryAttempt> {
        // Get the challenge
        let challenge = self.active_challenges.get(challenge_id)
            .ok_or_else(|| anyhow!("Challenge not found"))?
            .clone();
        
        // Check if challenge has expired
        if get_current_timestamp() > challenge.expires_at {
            return Err(anyhow!("Challenge has expired"));
        }
        
        // Verify the response
        let (success, confidence_score) = self.verify_behavioral_response(&challenge, &response_data)?;
        
        let attempt = RecoveryAttempt {
            attempt_id: generate_did(),
            challenge_id: challenge_id.to_string(),
            did: challenge.did.clone(),
            response: response_data,
            success,
            confidence_score,
            verification_time: get_current_timestamp() - challenge.created_at,
            attempted_at: get_current_timestamp(),
        };
        
        // Store recovery attempt
        self.recovery_history
            .entry(challenge.did.clone())
            .or_insert_with(Vec::new)
            .push(attempt.clone());
        
        // Remove challenge if successful or if max attempts reached
        if success {
            self.active_challenges.remove(challenge_id);
        } else {
            let attempt_count = self.recovery_history.get(&challenge.did)
                .map(|attempts| attempts.len())
                .unwrap_or(0);
            
            if attempt_count >= self.config.max_recovery_attempts as usize {
                self.active_challenges.remove(challenge_id);
            }
        }
        
        Ok(attempt)
    }
    
    /// Generate activity pattern challenge
    fn generate_activity_challenge(
        &self,
        patterns: &[BehavioralPattern],
        rng: &mut StdRng,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        // Analyze historical activity patterns
        let avg_frequency = patterns.iter()
            .map(|p| p.activity_frequency)
            .sum::<f64>() / patterns.len() as f64;
        
        let typical_hours: Vec<u8> = patterns.iter()
            .flat_map(|p| &p.peak_activity_hours)
            .cloned()
            .collect();
        
        // Create challenge data
        let challenge_data = ChallengeData {
            expected_peak_hours: typical_hours.clone(),
            expected_services: patterns.last().unwrap().service_preferences.clone(),
            expected_interaction_style: patterns.last().unwrap().interaction_style.clone(),
            expected_activity_range: (avg_frequency * 0.8, avg_frequency * 1.2),
            expected_economic_pattern: patterns.last().unwrap().economic_participation,
            expected_cross_chain_activity: patterns.last().unwrap().cross_chain_activity,
        };
        
        let challenge_bytes = serde_json::to_vec(&challenge_data)?;
        
        // Expected response (in real implementation, this would be encrypted)
        let expected_response = ResponseData {
            claimed_peak_hours: typical_hours,
            claimed_services: patterns.last().unwrap().service_preferences.clone(),
            claimed_interaction_style: patterns.last().unwrap().interaction_style.clone(),
            claimed_activity_level: avg_frequency,
            claimed_economic_pattern: patterns.last().unwrap().economic_participation,
            claimed_cross_chain_activity: patterns.last().unwrap().cross_chain_activity,
            behavioral_proof: vec![0; 64], // Placeholder for ZK proof
        };
        
        let expected_bytes = serde_json::to_vec(&expected_response)?;
        
        Ok((challenge_bytes, expected_bytes))
    }
    
    /// Generate service preference challenge
    fn generate_service_challenge(
        &self,
        patterns: &[BehavioralPattern],
        _rng: &mut StdRng,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        // Get most common services
        let mut service_counts: HashMap<ServiceType, usize> = HashMap::new();
        for pattern in patterns {
            for service in &pattern.service_preferences {
                *service_counts.entry(service.clone()).or_insert(0) += 1;
            }
        }
        
        let most_common_services: Vec<ServiceType> = service_counts.iter()
            .filter(|(_, &count)| count >= patterns.len() / 2) // Used by at least half the time
            .map(|(service, _)| service.clone())
            .collect();
        
        let challenge_data = format!("Identify your typical service usage pattern from: {:?}", 
                                   vec![ServiceType::Compute, ServiceType::Storage, ServiceType::Messaging, 
                                        ServiceType::AI, ServiceType::Identity, ServiceType::CrossChain, ServiceType::Encryption]);
        
        let expected_response = format!("Services: {:?}", most_common_services);
        
        Ok((challenge_data.as_bytes().to_vec(), expected_response.as_bytes().to_vec()))
    }
    
    /// Generate timing pattern challenge
    fn generate_timing_challenge(
        &self,
        patterns: &[BehavioralPattern],
        _rng: &mut StdRng,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        // Analyze timing patterns
        let mut hour_counts = vec![0; 24];
        for pattern in patterns {
            for &hour in &pattern.peak_activity_hours {
                if (hour as usize) < 24 {
                    hour_counts[hour as usize] += 1;
                }
            }
        }
        
        let peak_hours: Vec<u8> = hour_counts.iter()
            .enumerate()
            .filter(|(_, &count)| count >= patterns.len() / 3) // Peak if used 1/3 of the time
            .map(|(hour, _)| hour as u8)
            .collect();
        
        let challenge_data = "What are your typical peak activity hours (0-23)?";
        let expected_response = format!("Peak hours: {:?}", peak_hours);
        
        Ok((challenge_data.as_bytes().to_vec(), expected_response.as_bytes().to_vec()))
    }
    
    /// Generate interaction style challenge
    fn generate_interaction_challenge(
        &self,
        patterns: &[BehavioralPattern],
        _rng: &mut StdRng,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        // Get most common interaction style
        let mut style_counts: HashMap<InteractionStyle, usize> = HashMap::new();
        for pattern in patterns {
            *style_counts.entry(pattern.interaction_style.clone()).or_insert(0) += 1;
        }
        
        let most_common_style = style_counts.iter()
            .max_by_key(|(_, &count)| count)
            .map(|(style, _)| style.clone())
            .unwrap_or(InteractionStyle::Independent);
        
        let challenge_data = "What is your typical interaction style in the network?";
        let expected_response = format!("Interaction style: {:?}", most_common_style);
        
        Ok((challenge_data.as_bytes().to_vec(), expected_response.as_bytes().to_vec()))
    }
    
    /// Generate economic behavior challenge
    fn generate_economic_challenge(
        &self,
        patterns: &[BehavioralPattern],
        _rng: &mut StdRng,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let avg_economic = patterns.iter()
            .map(|p| p.economic_participation)
            .sum::<f64>() / patterns.len() as f64;
        
        let challenge_data = "What is your typical level of economic participation (0.0-1.0)?";
        let expected_response = format!("Economic participation: {:.2}", avg_economic);
        
        Ok((challenge_data.as_bytes().to_vec(), expected_response.as_bytes().to_vec()))
    }
    
    /// Generate cross-chain pattern challenge
    fn generate_cross_chain_challenge(
        &self,
        patterns: &[BehavioralPattern],
        _rng: &mut StdRng,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        let avg_cross_chain = patterns.iter()
            .map(|p| p.cross_chain_activity)
            .sum::<f64>() / patterns.len() as f64;
        
        let challenge_data = "What is your typical level of cross-chain activity (0.0-1.0)?";
        let expected_response = format!("Cross-chain activity: {:.2}", avg_cross_chain);
        
        Ok((challenge_data.as_bytes().to_vec(), expected_response.as_bytes().to_vec()))
    }
    
    /// Generate comprehensive profile challenge
    fn generate_comprehensive_challenge(
        &self,
        patterns: &[BehavioralPattern],
        reputations: &[DetailedReputationScore],
        rng: &mut StdRng,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        // This combines multiple aspects of behavior
        let latest_pattern = patterns.last().unwrap();
        let latest_reputation = reputations.last().unwrap();
        
        let comprehensive_data = ChallengeData {
            expected_peak_hours: latest_pattern.peak_activity_hours.clone(),
            expected_services: latest_pattern.service_preferences.clone(),
            expected_interaction_style: latest_pattern.interaction_style.clone(),
            expected_activity_range: (latest_pattern.activity_frequency * 0.9, latest_pattern.activity_frequency * 1.1),
            expected_economic_pattern: latest_pattern.economic_participation,
            expected_cross_chain_activity: latest_pattern.cross_chain_activity,
        };
        
        let challenge_bytes = serde_json::to_vec(&comprehensive_data)?;
        
        let expected_response = ResponseData {
            claimed_peak_hours: latest_pattern.peak_activity_hours.clone(),
            claimed_services: latest_pattern.service_preferences.clone(),
            claimed_interaction_style: latest_pattern.interaction_style.clone(),
            claimed_activity_level: latest_pattern.activity_frequency,
            claimed_economic_pattern: latest_pattern.economic_participation,
            claimed_cross_chain_activity: latest_pattern.cross_chain_activity,
            behavioral_proof: vec![1; 64], // Placeholder for comprehensive ZK proof
        };
        
        let expected_bytes = serde_json::to_vec(&expected_response)?;
        
        Ok((challenge_bytes, expected_bytes))
    }
    
    /// Calculate challenge difficulty based on pattern consistency
    fn calculate_challenge_difficulty(&self, patterns: &[BehavioralPattern], challenge_type: &ChallengeType) -> f64 {
        if patterns.is_empty() {
            return 1.0; // Maximum difficulty for no history
        }
        
        // Calculate pattern consistency
        let stability_scores: Vec<f64> = patterns.iter().map(|p| p.pattern_stability).collect();
        let avg_stability = stability_scores.iter().sum::<f64>() / stability_scores.len() as f64;
        
        // Calculate anomaly variance
        let anomaly_scores: Vec<f64> = patterns.iter().map(|p| p.anomaly_score).collect();
        let avg_anomaly = anomaly_scores.iter().sum::<f64>() / anomaly_scores.len() as f64;
        
        // Base difficulty inversely related to stability
        let base_difficulty = self.config.challenge_difficulty_base;
        let stability_factor = 1.0 - avg_stability;
        let anomaly_factor = avg_anomaly;
        
        // Adjust difficulty based on challenge type
        let type_multiplier = match challenge_type {
            ChallengeType::ActivityPattern => 0.8,
            ChallengeType::ServicePreference => 0.7,
            ChallengeType::TimingPattern => 0.9,
            ChallengeType::InteractionStyle => 0.6,
            ChallengeType::EconomicBehavior => 1.0,
            ChallengeType::CrossChainPattern => 1.1,
            ChallengeType::ComprehensiveProfile => 1.2,
        };
        
        (base_difficulty + stability_factor * 0.3 + anomaly_factor * 0.2) * type_multiplier
    }
    
    /// Verify behavioral response
    fn verify_behavioral_response(
        &self,
        challenge: &RecoveryChallenge,
        response_data: &[u8],
    ) -> Result<(bool, f64)> {
        match challenge.challenge_type {
            ChallengeType::ActivityPattern | ChallengeType::ComprehensiveProfile => {
                self.verify_comprehensive_response(challenge, response_data)
            }
            _ => {
                self.verify_simple_response(challenge, response_data)
            }
        }
    }
    
    /// Verify comprehensive behavioral response
    fn verify_comprehensive_response(
        &self,
        challenge: &RecoveryChallenge,
        response_data: &[u8],
    ) -> Result<(bool, f64)> {
        // Parse challenge data
        let challenge_data: ChallengeData = serde_json::from_slice(&challenge.challenge_data)?;
        
        // Parse response data
        let response: ResponseData = serde_json::from_slice(response_data)?;
        
        let mut similarity_score = 0.0;
        let mut component_count = 0.0;
        
        // Verify peak hours (exact match gets full points, partial match gets partial points)
        let hour_similarity = self.calculate_hour_similarity(&challenge_data.expected_peak_hours, &response.claimed_peak_hours);
        similarity_score += hour_similarity * 0.2;
        component_count += 0.2;
        
        // Verify services (exact match gets full points, partial match gets partial points)
        let service_similarity = self.calculate_service_similarity(&challenge_data.expected_services, &response.claimed_services);
        similarity_score += service_similarity * 0.2;
        component_count += 0.2;
        
        // Verify interaction style (binary: match or no match)
        if challenge_data.expected_interaction_style == response.claimed_interaction_style {
            similarity_score += 0.15;
        }
        component_count += 0.15;
        
        // Verify activity level (range check)
        let activity_similarity = if response.claimed_activity_level >= challenge_data.expected_activity_range.0 
            && response.claimed_activity_level <= challenge_data.expected_activity_range.1 {
            1.0
        } else {
            let distance = if response.claimed_activity_level < challenge_data.expected_activity_range.0 {
                challenge_data.expected_activity_range.0 - response.claimed_activity_level
            } else {
                response.claimed_activity_level - challenge_data.expected_activity_range.1
            };
            let max_distance = challenge_data.expected_activity_range.1 * 0.5; // 50% tolerance
            (1.0 - (distance / max_distance)).max(0.0)
        };
        similarity_score += activity_similarity * 0.15;
        component_count += 0.15;
        
        // Verify economic pattern (tolerance-based)
        let economic_diff = (challenge_data.expected_economic_pattern - response.claimed_economic_pattern).abs();
        let economic_similarity = (1.0 - economic_diff.min(0.3) / 0.3).max(0.0); // 30% tolerance
        similarity_score += economic_similarity * 0.15;
        component_count += 0.15;
        
        // Verify cross-chain activity (tolerance-based)
        let cross_chain_diff = (challenge_data.expected_cross_chain_activity - response.claimed_cross_chain_activity).abs();
        let cross_chain_similarity = (1.0 - cross_chain_diff.min(0.3) / 0.3).max(0.0); // 30% tolerance
        similarity_score += cross_chain_similarity * 0.15;
        component_count += 0.15;
        
        // Normalize score
        let final_score = if component_count > 0.0 {
            similarity_score / component_count
        } else {
            0.0
        };
        
        // Success if score meets threshold
        let success = final_score >= self.config.pattern_similarity_threshold;
        
        Ok((success, final_score))
    }
    
    /// Verify simple behavioral response
    fn verify_simple_response(
        &self,
        challenge: &RecoveryChallenge,
        response_data: &[u8],
    ) -> Result<(bool, f64)> {
        // For simple challenges, do string comparison with some tolerance
        let expected_str = String::from_utf8_lossy(&challenge.expected_response);
        let response_str = String::from_utf8_lossy(response_data);
        
        // Calculate similarity using simple string matching
        let similarity = if expected_str.to_lowercase().trim() == response_str.to_lowercase().trim() {
            1.0
        } else if expected_str.to_lowercase().contains(&response_str.to_lowercase()) 
                 || response_str.to_lowercase().contains(&expected_str.to_lowercase()) {
            0.8
        } else {
            0.0
        };
        
        let success = similarity >= self.config.pattern_similarity_threshold;
        
        Ok((success, similarity))
    }
    
    /// Calculate similarity between two sets of hours
    fn calculate_hour_similarity(&self, expected: &[u8], actual: &[u8]) -> f64 {
        if expected.is_empty() && actual.is_empty() {
            return 1.0;
        }
        
        if expected.is_empty() || actual.is_empty() {
            return 0.0;
        }
        
        let intersection: Vec<u8> = expected.iter()
            .filter(|hour| actual.contains(hour))
            .cloned()
            .collect();
        
        let union_size = expected.len() + actual.len() - intersection.len();
        
        if union_size == 0 {
            1.0
        } else {
            intersection.len() as f64 / union_size as f64
        }
    }
    
    /// Calculate similarity between two sets of services
    fn calculate_service_similarity(&self, expected: &[ServiceType], actual: &[ServiceType]) -> f64 {
        if expected.is_empty() && actual.is_empty() {
            return 1.0;
        }
        
        if expected.is_empty() || actual.is_empty() {
            return 0.0;
        }
        
        let intersection: Vec<&ServiceType> = expected.iter()
            .filter(|service| actual.contains(service))
            .collect();
        
        let union_size = expected.len() + actual.len() - intersection.len();
        
        if union_size == 0 {
            1.0
        } else {
            intersection.len() as f64 / union_size as f64
        }
    }
    
    /// Get recovery statistics for a user
    pub fn get_user_recovery_stats(&self, did: &str) -> RecoveryStats {
        let total_attempts = self.recovery_history.get(did)
            .map(|attempts| attempts.len())
            .unwrap_or(0);
        
        let successful_attempts = self.recovery_history.get(did)
            .map(|attempts| attempts.iter().filter(|a| a.success).count())
            .unwrap_or(0);
        
        let average_confidence = if total_attempts > 0 {
            self.recovery_history.get(did)
                .unwrap()
                .iter()
                .map(|a| a.confidence_score)
                .sum::<f64>() / total_attempts as f64
        } else {
            0.0
        };
        
        RecoveryStats {
            total_attempts,
            successful_attempts,
            success_rate: if total_attempts > 0 { 
                successful_attempts as f64 / total_attempts as f64 
            } else { 
                0.0 
            },
            average_confidence,
            is_eligible: self.is_recovery_eligible(did),
        }
    }
    
    /// Get overall system recovery statistics
    pub fn get_system_recovery_stats(&self) -> SystemRecoveryStats {
        let total_users = self.user_patterns.len();
        let eligible_users = self.user_patterns.keys()
            .filter(|did| self.is_recovery_eligible(did))
            .count();
        
        let total_attempts: usize = self.recovery_history.values()
            .map(|attempts| attempts.len())
            .sum();
        
        let successful_attempts: usize = self.recovery_history.values()
            .flat_map(|attempts| attempts.iter())
            .filter(|attempt| attempt.success)
            .count();
        
        let average_confidence = if total_attempts > 0 {
            self.recovery_history.values()
                .flat_map(|attempts| attempts.iter())
                .map(|attempt| attempt.confidence_score)
                .sum::<f64>() / total_attempts as f64
        } else {
            0.0
        };
        
        SystemRecoveryStats {
            total_users,
            eligible_users,
            eligibility_rate: if total_users > 0 { eligible_users as f64 / total_users as f64 } else { 0.0 },
            total_attempts,
            successful_attempts,
            overall_success_rate: if total_attempts > 0 { successful_attempts as f64 / total_attempts as f64 } else { 0.0 },
            average_confidence,
            active_challenges: self.active_challenges.len(),
        }
    }
}

/// User recovery statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryStats {
    pub total_attempts: usize,
    pub successful_attempts: usize,
    pub success_rate: f64,
    pub average_confidence: f64,
    pub is_eligible: bool,
}

/// System-wide recovery statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemRecoveryStats {
    pub total_users: usize,
    pub eligible_users: usize,
    pub eligibility_rate: f64,
    pub total_attempts: usize,
    pub successful_attempts: usize,
    pub overall_success_rate: f64,
    pub average_confidence: f64,
    pub active_challenges: usize,
}

impl Default for ConfidenceRecoverySystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PersonalityProfile, UserArchetype, ServiceType, InteractionStyle};
    
    #[test]
    fn test_recovery_system_creation() {
        let system = ConfidenceRecoverySystem::new();
        assert!(system.user_patterns.is_empty());
        assert!(system.active_challenges.is_empty());
    }
    
    #[test]
    fn test_eligibility_check() {
        let mut system = ConfidenceRecoverySystem::new();
        
        // User should not be eligible initially
        assert!(!system.is_recovery_eligible("test_did"));
        
        // Add insufficient data
        let pattern = BehavioralPattern {
            did: "test_did".to_string(),
            activity_frequency: 5.0,
            peak_activity_hours: vec![9, 10, 11],
            service_preferences: vec![ServiceType::Compute],
            interaction_style: InteractionStyle::Collaborative,
            anomaly_score: 0.1,
            pattern_stability: 0.9,
            economic_participation: 0.8,
            cross_chain_activity: 0.5,
            security_compliance: 0.9,
        };
        
        let reputation = DetailedReputationScore {
            did: "test_did".to_string(),
            overall_score: 0.85,
            components: crate::ReputationComponents {
                service_quality: 0.9,
                reliability: 0.8,
                response_time: 0.7,
                security_compliance: 0.9,
                collaboration_score: 0.6,
                innovation_factor: 0.5,
                fraud_risk: 0.1,
                economic_consistency: 0.8,
                cross_chain_reliability: 0.5,
            },
            confidence_level: 0.75,
            trend: crate::ReputationTrend::Stable,
            last_updated: get_current_timestamp(),
            prediction_accuracy: 0.96,
        };
        
        system.register_user_data("test_did", pattern, reputation);
        
        // Still not eligible with just one data point
        assert!(!system.is_recovery_eligible("test_did"));
    }
}