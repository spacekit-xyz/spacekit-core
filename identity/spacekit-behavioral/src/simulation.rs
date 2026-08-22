//! Main Simulation Engine
//! 
//! Orchestrates the behavioral cryptography simulation with multiple user archetypes,
//! behavioral pattern evolution, and recovery testing over time.

use crate::{
    UserArchetype, PersonalityProfile, SimulatedUser, BehavioralPattern, 
    SimulationConfig, SimulationResults, ArchetypeMetrics, TimelineSnapshot,
    ChallengeType, get_current_timestamp, generate_did,
    behavioral_engine::BehavioralEngine,
    confidence_recovery::{ConfidenceRecoverySystem, ResponseData},
};
use anyhow::Result;
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use std::collections::HashMap;
use tracing::{info, debug, warn};

/// Main simulation orchestrator
pub struct BehavioralSimulation {
    /// Simulation configuration
    config: SimulationConfig,
    /// Random number generator
    rng: StdRng,
    /// Behavioral analysis engine
    behavioral_engine: BehavioralEngine,
    /// Recovery system
    recovery_system: ConfidenceRecoverySystem,
    /// Simulated users
    users: Vec<SimulatedUser>,
    /// Timeline tracking
    timeline: Vec<TimelineSnapshot>,
    /// Fraud simulation users (for testing detection)
    fraud_users: Vec<String>,
}

impl BehavioralSimulation {
    /// Create a new simulation
    pub fn new(config: SimulationConfig) -> Self {
        let seed = config.random_seed.unwrap_or_else(|| {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
        });
        
        info!("🚀 Initializing SWTCH Behavioral Cryptography Simulation");
        info!("📊 Configuration: {} users, {} days, seed: {}", 
              config.num_users, config.simulation_days, seed);
        
        Self {
            config,
            rng: StdRng::seed_from_u64(seed),
            behavioral_engine: BehavioralEngine::new(),
            recovery_system: ConfidenceRecoverySystem::new(),
            users: Vec::new(),
            timeline: Vec::new(),
            fraud_users: Vec::new(),
        }
    }
    
    pub fn get_user_count(&self) -> usize {
        self.users.len()
    }

    /// Run the complete simulation
    pub async fn run_simulation(&mut self) -> Result<SimulationResults> {
        let start_time = std::time::Instant::now();
        
        info!("🎭 Generating user population with diverse archetypes...");
        self.generate_user_population()?;
        
        info!("⏳ Simulating {} days of network behavior...", self.config.simulation_days);
        self.simulate_behavioral_evolution().await?;
        
        info!("🔐 Testing behavioral recovery mechanisms...");
        self.test_recovery_mechanisms().await?;
        
        let execution_time = start_time.elapsed().as_millis() as u64;
        
        info!("✅ Simulation completed in {}ms", execution_time);
        self.generate_results(execution_time)
    }
    
    /// Generate diverse user population
    fn generate_user_population(&mut self) -> Result<()> {
        let archetypes = vec![
            UserArchetype::BaseUser,
            UserArchetype::Validator,
            UserArchetype::Developer,
            UserArchetype::Researcher,
            UserArchetype::Investor,
            UserArchetype::Regulator,
            UserArchetype::Other,
        ];
        
        // Calculate distribution
        let base_users = (self.config.num_users as f64 * 0.40) as usize; // 40% base users
        let validators = (self.config.num_users as f64 * 0.15) as usize;  // 15% validators
        let developers = (self.config.num_users as f64 * 0.20) as usize;  // 20% developers
        let researchers = (self.config.num_users as f64 * 0.10) as usize; // 10% researchers
        let investors = (self.config.num_users as f64 * 0.08) as usize;   // 8% investors
        let regulators = (self.config.num_users as f64 * 0.02) as usize;  // 2% regulators
        let others = self.config.num_users - (base_users + validators + developers + researchers + investors + regulators);
        
        let distributions = vec![
            (UserArchetype::BaseUser, base_users),
            (UserArchetype::Validator, validators),
            (UserArchetype::Developer, developers),
            (UserArchetype::Researcher, researchers),
            (UserArchetype::Investor, investors),
            (UserArchetype::Regulator, regulators),
            (UserArchetype::Other, others),
        ];
        
        for (archetype, count) in distributions {
            for _ in 0..count {
                let user = self.create_simulated_user(archetype.clone())?;
                self.users.push(user);
            }
        }
        
        // Mark some users for fraud simulation
        if self.config.enable_fraud_simulation {
            let fraud_count = (self.config.num_users as f64 * self.config.fraud_percentage) as usize;
            for _ in 0..fraud_count {
                let user_index = self.rng.gen_range(0..self.users.len());
                self.fraud_users.push(self.users[user_index].did.clone());
            }
            info!("🚨 Marked {} users for fraud simulation", fraud_count);
        }
        
        info!("👥 Generated {} users across {} archetypes", self.users.len(), archetypes.len());
        Ok(())
    }
    
    /// Create a simulated user
    fn create_simulated_user(&mut self, archetype: UserArchetype) -> Result<SimulatedUser> {
        let did = generate_did();
        let personality = PersonalityProfile::generate_for_archetype(archetype, &mut self.rng);
        
        // Generate initial behavioral pattern
        let initial_activity = self.behavioral_engine.generate_activity_data(&personality, 0, 10, &mut self.rng);
        let behavioral_pattern = self.behavioral_engine.calculate_behavioral_pattern(&did, &personality, &initial_activity)?;
        let initial_reputation = self.behavioral_engine.calculate_reputation_score(&did, &personality, &initial_activity, &behavioral_pattern)?;
        
        let user = SimulatedUser {
            did: did.clone(),
            personality,
            behavioral_pattern,
            activity_history: vec![initial_activity],
            reputation_history: vec![initial_reputation.clone()],
            created_at: get_current_timestamp(),
            last_active: get_current_timestamp(),
            confidence_score: initial_reputation.confidence_level,
            recovery_attempts: 0,
        };
        
        Ok(user)
    }
    
    /// Simulate behavioral evolution over time
    async fn simulate_behavioral_evolution(&mut self) -> Result<()> {
        for day in 0..self.config.simulation_days {
            debug!("📅 Simulating day {}", day + 1);
            
            let mut daily_stats = TimelineSnapshot {
                day: day + 1,
                active_users: 0,
                average_confidence: 0.0,
                recovery_attempts: 0,
                successful_recoveries: 0,
                fraud_attempts: 0,
                network_health: 0.0,
            };
            
            // Simulate each user's daily activity
            for i in 0..self.users.len() {
                // Generate multiple hours of activity for this day
                let hours_active = self.rng.gen_range(1..=8); // 1-8 hours of activity per day
                
                for _ in 0..hours_active {
                    let hour = *self.users[i].personality.peak_hours.get(
                        self.rng.gen_range(0..self.users[i].personality.peak_hours.len())
                    ).unwrap_or(&12);
                    
                    // Generate activity data
                    let mut activity = self.behavioral_engine.generate_activity_data(&self.users[i].personality, day, hour, &mut self.rng);
                    
                    // Apply fraud patterns if this is a fraud user
                    if self.fraud_users.contains(&self.users[i].did) {
                        self.apply_fraud_patterns(&mut activity, day);
                    }
                    
                    // Calculate behavioral pattern
                    let pattern = self.behavioral_engine.calculate_behavioral_pattern(&self.users[i].did, &self.users[i].personality, &activity)?;
                    
                    // Calculate reputation
                    let reputation = self.behavioral_engine.calculate_reputation_score(&self.users[i].did, &self.users[i].personality, &activity, &pattern)?;
                    
                    // Update user data
                    self.users[i].activity_history.push(activity);
                    self.users[i].reputation_history.push(reputation.clone());
                    self.users[i].behavioral_pattern = pattern.clone();
                    self.users[i].confidence_score = reputation.confidence_level;
                    self.users[i].last_active = get_current_timestamp();
                    
                    // Register with recovery system
                    self.recovery_system.register_user_data(&self.users[i].did, pattern, reputation);
                    
                    daily_stats.active_users += 1;
                }
            }
            
            // Calculate daily statistics
            let total_confidence: f64 = self.users.iter().map(|u| u.confidence_score).sum();
            daily_stats.average_confidence = total_confidence / self.users.len() as f64;
            
            // Calculate network health (average reputation)
            let total_reputation: f64 = self.users.iter()
                .filter_map(|u| u.reputation_history.last())
                .map(|r| r.overall_score)
                .sum();
            daily_stats.network_health = total_reputation / self.users.len() as f64;
            
            self.timeline.push(daily_stats);
            
            // Periodic recovery testing (every 7 days)
            if day % 7 == 6 {
                self.run_periodic_recovery_tests().await?;
            }
        }
        
        info!("📈 Behavioral evolution simulation completed");
        Ok(())
    }
    
    /// Apply fraud patterns to activity data
    fn apply_fraud_patterns(&mut self, activity: &mut crate::ActivityData, day: u64) {
        // Gradually introduce fraudulent behavior
        let fraud_intensity = (day as f64 / self.config.simulation_days as f64).min(1.0);
        
        // Increase suspicious patterns
        activity.suspicious_patterns += self.rng.gen_range(1..=3);
        
        // Add unusual timing
        activity.unusual_timing_score += fraud_intensity * 0.3;
        
        // Add rapid requests
        activity.rapid_requests += (fraud_intensity * 100.0) as u64;
        
        // Occasionally add reputation manipulation attempts
        if self.rng.gen_bool(fraud_intensity * 0.1) {
            activity.reputation_manipulation_attempts += 1;
        }
        
        // Degrade service quality
        activity.success_rate *= 1.0 - fraud_intensity * 0.2;
        activity.quality_score *= 1.0 - fraud_intensity * 0.15;
    }
    
    /// Run periodic recovery tests
    async fn run_periodic_recovery_tests(&mut self) -> Result<()> {
        debug!("🔄 Running periodic recovery tests");
        
        // Test recovery for a subset of eligible users
        let eligible_users: Vec<String> = self.users.iter()
            .filter(|u| self.recovery_system.is_recovery_eligible(&u.did))
            .map(|u| u.did.clone())
            .collect();
        
        let test_count = (eligible_users.len() / 10).max(1); // Test 10% of eligible users
        
        for _ in 0..test_count {
            if let Some(did) = eligible_users.get(self.rng.gen_range(0..eligible_users.len())) {
                self.test_user_recovery(did).await?;
            }
        }
        
        Ok(())
    }
    
    /// Test recovery mechanisms
    async fn test_recovery_mechanisms(&mut self) -> Result<()> {
        info!("🔍 Testing recovery mechanisms for eligible users");
        
        let challenge_types = vec![
            ChallengeType::ActivityPattern,
            ChallengeType::ServicePreference,
            ChallengeType::TimingPattern,
            ChallengeType::InteractionStyle,
            ChallengeType::EconomicBehavior,
            ChallengeType::CrossChainPattern,
            ChallengeType::ComprehensiveProfile,
        ];
        
        let eligible_users: Vec<String> = self.users.iter()
            .filter(|u| self.recovery_system.is_recovery_eligible(&u.did))
            .map(|u| u.did.clone())
            .collect();
        
        info!("📊 Found {} eligible users for recovery testing", eligible_users.len());
        
        // Test each challenge type with different users
        for challenge_type in challenge_types {
            let test_users = eligible_users.iter()
                .take(eligible_users.len().min(20)) // Test up to 20 users per challenge type
                .collect::<Vec<_>>();
            
            for did in test_users {
                match self.recovery_system.generate_recovery_challenge(did, challenge_type.clone(), &mut self.rng) {
                    Ok(challenge) => {
                        // Simulate user response (simplified - in real implementation would be interactive)
                        let response = self.simulate_user_response(&challenge, did)?;
                        
                        match self.recovery_system.verify_recovery_attempt(&challenge.challenge_id, response) {
                            Ok(attempt) => {
                                // Update user statistics
                                if let Some(user) = self.users.iter_mut().find(|u| u.did == *did) {
                                    user.recovery_attempts += 1;
                                }
                                
                                debug!("Recovery attempt for {}: success={}, confidence={:.3}", 
                                      did, attempt.success, attempt.confidence_score);
                            }
                            Err(e) => {
                                warn!("Recovery verification failed for {}: {}", did, e);
                            }
                        }
                    }
                    Err(e) => {
                        debug!("Challenge generation failed for {}: {}", did, e);
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Test recovery for a specific user
    async fn test_user_recovery(&mut self, did: &str) -> Result<()> {
        let challenge_types = vec![
            ChallengeType::ActivityPattern,
            ChallengeType::ComprehensiveProfile,
        ];
        
        let challenge_type = challenge_types[self.rng.gen_range(0..challenge_types.len())].clone();
        
        match self.recovery_system.generate_recovery_challenge(did, challenge_type, &mut self.rng) {
            Ok(challenge) => {
                let response = self.simulate_user_response(&challenge, did)?;
                
                match self.recovery_system.verify_recovery_attempt(&challenge.challenge_id, response) {
                    Ok(attempt) => {
                        if let Some(user) = self.users.iter_mut().find(|u| u.did == did) {
                            user.recovery_attempts += 1;
                        }
                        
                        // Update timeline
                        if let Some(latest_snapshot) = self.timeline.last_mut() {
                            latest_snapshot.recovery_attempts += 1;
                            if attempt.success {
                                latest_snapshot.successful_recoveries += 1;
                            }
                            if self.fraud_users.contains(&did.to_string()) {
                                latest_snapshot.fraud_attempts += 1;
                            }
                        }
                    }
                    Err(_) => {}
                }
            }
            Err(_) => {}
        }
        
        Ok(())
    }
    
    /// Simulate user response to recovery challenge
    fn simulate_user_response(&mut self, challenge: &crate::RecoveryChallenge, did: &str) -> Result<Vec<u8>> {
        // Find the user
        let user = self.users.iter()
            .find(|u| u.did == did)
            .ok_or_else(|| anyhow::anyhow!("User not found"))?;
        
        match challenge.challenge_type {
            ChallengeType::ActivityPattern | ChallengeType::ComprehensiveProfile => {
                // Create a comprehensive response
                let latest_pattern = &user.behavioral_pattern;
                
                let response = ResponseData {
                    claimed_peak_hours: latest_pattern.peak_activity_hours.clone(),
                    claimed_services: latest_pattern.service_preferences.clone(),
                    claimed_interaction_style: latest_pattern.interaction_style.clone(),
                    claimed_activity_level: latest_pattern.activity_frequency,
                    claimed_economic_pattern: latest_pattern.economic_participation,
                    claimed_cross_chain_activity: latest_pattern.cross_chain_activity,
                    behavioral_proof: vec![1; 64],
                };
                
                // Add some noise for fraud users or random variation
                if self.fraud_users.contains(&did.to_string()) || self.rng.gen_bool(0.1) {
                    // Simulate incorrect or partially correct responses
                    let mut noisy_response = response;
                    
                    if self.rng.gen_bool(0.3) {
                        // Wrong peak hours
                        noisy_response.claimed_peak_hours = vec![self.rng.gen_range(0..24)];
                    }
                    
                    if self.rng.gen_bool(0.2) {
                        // Wrong interaction style
                        noisy_response.claimed_interaction_style = crate::InteractionStyle::Suspicious;
                    }
                    
                    Ok(serde_json::to_vec(&noisy_response)?)
                } else {
                    Ok(serde_json::to_vec(&response)?)
                }
            }
            _ => {
                // For simple challenges, create simple string responses
                let response = match challenge.challenge_type {
                    ChallengeType::ServicePreference => {
                        format!("Services: {:?}", user.behavioral_pattern.service_preferences)
                    }
                    ChallengeType::TimingPattern => {
                        format!("Peak hours: {:?}", user.behavioral_pattern.peak_activity_hours)
                    }
                    ChallengeType::InteractionStyle => {
                        format!("Interaction style: {:?}", user.behavioral_pattern.interaction_style)
                    }
                    ChallengeType::EconomicBehavior => {
                        format!("Economic participation: {:.2}", user.behavioral_pattern.economic_participation)
                    }
                    ChallengeType::CrossChainPattern => {
                        format!("Cross-chain activity: {:.2}", user.behavioral_pattern.cross_chain_activity)
                    }
                    _ => "Default response".to_string(),
                };
                
                // Add noise for fraud users
                if self.fraud_users.contains(&did.to_string()) && self.rng.gen_bool(0.5) {
                    Ok(format!("Wrong: {}", self.rng.gen_range(0..1000)).into_bytes())
                } else {
                    Ok(response.into_bytes())
                }
            }
        }
    }
    
    /// Generate simulation results
    fn generate_results(&self, execution_time_ms: u64) -> Result<SimulationResults> {
        info!("📊 Generating simulation results...");
        
        // Calculate overall statistics
        let recovery_stats = self.recovery_system.get_system_recovery_stats();
        
        // Calculate archetype performance
        let mut archetype_performance = HashMap::new();
        
        for archetype in [
            UserArchetype::BaseUser,
            UserArchetype::Validator,
            UserArchetype::Developer,
            UserArchetype::Researcher,
            UserArchetype::Investor,
            UserArchetype::Regulator,
            UserArchetype::Other,
        ] {
            let archetype_users: Vec<&SimulatedUser> = self.users.iter()
                .filter(|u| u.personality.archetype == archetype)
                .collect();
            
            if !archetype_users.is_empty() {
                let avg_confidence = archetype_users.iter()
                    .map(|u| u.confidence_score)
                    .sum::<f64>() / archetype_users.len() as f64;
                
                let recovery_attempts: u64 = archetype_users.iter()
                    .map(|u| u.recovery_attempts)
                    .sum();
                
                let successful_recoveries: u64 = archetype_users.iter()
                    .map(|u| {
                        self.recovery_system.get_user_recovery_stats(&u.did).successful_attempts as u64
                    })
                    .sum();
                
                let recovery_success_rate = if recovery_attempts > 0 {
                    successful_recoveries as f64 / recovery_attempts as f64
                } else {
                    0.0
                };
                
                let fraud_users_in_archetype = archetype_users.iter()
                    .filter(|u| self.fraud_users.contains(&u.did))
                    .count();
                
                let fraud_detection_rate = if fraud_users_in_archetype > 0 {
                    // Simplified fraud detection rate based on low confidence scores
                    archetype_users.iter()
                        .filter(|u| self.fraud_users.contains(&u.did))
                        .filter(|u| u.confidence_score < 0.5) // Low confidence indicates detection
                        .count() as f64 / fraud_users_in_archetype as f64
                } else {
                    1.0
                };
                
                let avg_pattern_stability = archetype_users.iter()
                    .map(|u| u.behavioral_pattern.pattern_stability)
                    .sum::<f64>() / archetype_users.len() as f64;
                
                let avg_economic_participation = archetype_users.iter()
                    .map(|u| u.behavioral_pattern.economic_participation)
                    .sum::<f64>() / archetype_users.len() as f64;
                
                let metrics = ArchetypeMetrics {
                    archetype: archetype.clone(),
                    user_count: archetype_users.len(),
                    average_confidence: avg_confidence,
                    recovery_success_rate,
                    fraud_detection_rate,
                    pattern_stability: avg_pattern_stability,
                    economic_participation: avg_economic_participation,
                };
                
                archetype_performance.insert(archetype, metrics);
            }
        }
        
        let results = SimulationResults {
            config: self.config.clone(),
            users: self.users.clone(),
            total_recovery_attempts: recovery_stats.total_attempts as u64,
            successful_recoveries: recovery_stats.successful_attempts as u64,
            failed_recoveries: (recovery_stats.total_attempts - recovery_stats.successful_attempts) as u64,
            fraud_attempts: self.fraud_users.len() as u64,
            fraud_detections: self.fraud_users.iter()
                .filter(|did| {
                    self.users.iter()
                        .find(|u| u.did == **did)
                        .map(|u| u.confidence_score < 0.5)
                        .unwrap_or(false)
                })
                .count() as u64,
            average_confidence_score: recovery_stats.average_confidence,
            archetype_performance,
            timeline_data: self.timeline.clone(),
            execution_time_ms,
        };
        
        info!("✅ Results generated successfully");
        info!("📈 Average confidence: {:.3}", results.average_confidence_score);
        info!("🎯 Recovery success rate: {:.1}%", 
              if results.total_recovery_attempts > 0 {
                  results.successful_recoveries as f64 / results.total_recovery_attempts as f64 * 100.0
              } else {
                  0.0
              });
        info!("🛡️ Fraud detection rate: {:.1}%", 
              if results.fraud_attempts > 0 {
                  results.fraud_detections as f64 / results.fraud_attempts as f64 * 100.0
              } else {
                  0.0
              });
        
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_simulation_creation() {
        let config = SimulationConfig {
            num_users: 10,
            simulation_days: 3,
            ..Default::default()
        };
        
        let simulation = BehavioralSimulation::new(config);
        assert_eq!(simulation.users.len(), 0);
        assert_eq!(simulation.timeline.len(), 0);
    }
    
    #[tokio::test]
    async fn test_user_generation() {
        let config = SimulationConfig {
            num_users: 10,
            simulation_days: 1,
            ..Default::default()
        };
        
        let mut simulation = BehavioralSimulation::new(config);
        simulation.generate_user_population().unwrap();
        
        assert_eq!(simulation.users.len(), 10);
        
        // Check archetype distribution
        let archetypes: Vec<UserArchetype> = simulation.users.iter()
            .map(|u| u.personality.archetype.clone())
            .collect();
        
        assert!(archetypes.contains(&UserArchetype::BaseUser));
        assert!(archetypes.contains(&UserArchetype::Developer));
    }
    
    #[tokio::test]
    async fn test_small_simulation() {
        let config = SimulationConfig {
            num_users: 5,
            simulation_days: 2,
            enable_fraud_simulation: false,
            ..Default::default()
        };
        
        let mut simulation = BehavioralSimulation::new(config);
        let results = simulation.run_simulation().await.unwrap();
        
        assert_eq!(results.users.len(), 5);
        assert_eq!(results.timeline_data.len(), 2);
        assert!(results.execution_time_ms > 0);
    }
}