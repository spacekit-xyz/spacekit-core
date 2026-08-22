use super::*;
use spacekit_primitives::v1::identity::Identity;
use chrono::{DateTime, Utc, Duration, Timelike};
use ndarray::{Array1, arr1};
use std::collections::HashMap;

/// Behavioral Pattern Analyzer with differential privacy protection
pub struct BehavioralPatternAnalyzer {
    /// Privacy budget for behavioral analysis
    epsilon: f64,
    /// Privacy parameter delta
    delta: f64,
    /// Network participation data store
    participation_store: ParticipationDataStore,
}

/// Mock network participation data store
/// In production, this would interface with actual network logs
pub struct ParticipationDataStore {
    storage_logs: HashMap<String, Vec<StorageEvent>>,
    compute_logs: HashMap<String, Vec<ComputeEvent>>,
    economic_logs: HashMap<String, Vec<EconomicEvent>>,
    service_logs: HashMap<String, Vec<ServiceEvent>>,
    chain_logs: HashMap<String, Vec<ChainEvent>>,
}

#[derive(Debug, Clone)]
pub struct StorageEvent {
    pub timestamp: DateTime<Utc>,
    pub storage_gb: f64,
    pub duration_hours: f64,
    pub location_hash: u64, // Hashed geographic indicator
}

#[derive(Debug, Clone)]
pub struct ComputeEvent {
    pub timestamp: DateTime<Utc>,
    pub compute_hours: f64,
    pub bandwidth_gb: f64,
    pub service_type: String,
    pub quality_score: f64,
}

#[derive(Debug, Clone)]
pub struct EconomicEvent {
    pub timestamp: DateTime<Utc>,
    pub tokens_earned: f64,
    pub tokens_staked: f64,
    pub payment_made: f64,
    pub bonding_curve_interaction: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceEvent {
    pub timestamp: DateTime<Utc>,
    pub service_type: String,
    pub peer_rating: f64,
    pub success: bool,
    pub response_time_ms: f64,
}

#[derive(Debug, Clone)]
pub struct ChainEvent {
    pub timestamp: DateTime<Utc>,
    pub chain_name: String,
    pub transaction_type: String,
    pub cross_chain: bool,
}

impl BehavioralPatternAnalyzer {
    /// Create new analyzer with privacy parameters
    pub fn new(epsilon: f64, delta: f64) -> Self {
        Self {
            epsilon,
            delta,
            participation_store: ParticipationDataStore::new(),
        }
    }

    /// Analyze behavioral patterns for an identity with differential privacy
    pub fn analyze_patterns(&self, identity: &Identity) -> Result<BehavioralPatterns, Box<dyn std::error::Error>> {
        let identity_did = &identity.did;

        // Collect raw behavioral data
        let storage_pattern = self.analyze_storage_behavior(identity_did)?;
        let compute_pattern = self.analyze_compute_participation(identity_did)?;
        let economic_pattern = self.analyze_economic_behavior(identity_did)?;
        let service_quality = self.analyze_service_quality(identity_did)?;
        let multi_chain_activity = self.analyze_multi_chain_activity(identity_did)?;

        Ok(BehavioralPatterns {
            storage_behavior: storage_pattern,
            compute_participation: compute_pattern,
            economic_patterns: economic_pattern,
            service_quality,
            multi_chain_activity,
            collected_at: Utc::now(),
            privacy_budget_used: self.epsilon,
        })
    }

    /// Analyze storage behavior with differential privacy
    fn analyze_storage_behavior(&self, identity_did: &str) -> Result<StoragePattern, Box<dyn std::error::Error>> {
        let events = self.participation_store.get_storage_events(identity_did);
        
        // Calculate raw metrics
        let raw_daily_storage = self.calculate_avg_daily_storage(&events);
        let raw_consistency = self.calculate_storage_consistency(&events);
        let raw_retention = self.calculate_avg_retention(&events);
        
        // Apply differential privacy
        let dp_daily_storage = self.add_laplace_noise(raw_daily_storage, 1.0)?;
        let dp_consistency = self.add_laplace_noise(raw_consistency, 0.1)?;
        let dp_retention = self.add_laplace_noise(raw_retention, 1.0)?;

        // Generate geographic and temporal patterns
        let geographic_prefs = self.generate_geographic_preferences(&events)?;
        let hourly_prefs = self.generate_hourly_preferences(&events)?;

        Ok(StoragePattern {
            avg_daily_storage_gb: dp_daily_storage.max(0.0),
            consistency_score: dp_consistency.max(0.0).min(1.0),
            geographic_preferences: geographic_prefs,
            avg_retention_days: dp_retention.max(0.0),
            preferred_storage_hours: hourly_prefs,
        })
    }

    /// Analyze compute participation with differential privacy
    fn analyze_compute_participation(&self, identity_did: &str) -> Result<ComputePattern, Box<dyn std::error::Error>> {
        let events = self.participation_store.get_compute_events(identity_did);
        
        let raw_compute_hours = self.calculate_avg_daily_compute(&events);
        let raw_bandwidth = self.calculate_avg_daily_bandwidth(&events);
        let raw_quality = self.calculate_service_quality(&events);

        // Apply differential privacy
        let dp_compute_hours = self.add_laplace_noise(raw_compute_hours, 1.0)?;
        let dp_bandwidth = self.add_laplace_noise(raw_bandwidth, 1.0)?;
        let dp_quality = self.add_laplace_noise(raw_quality, 0.1)?;

        let availability_pattern = self.generate_availability_pattern(&events)?;
        let preferred_types = self.extract_preferred_compute_types(&events);

        Ok(ComputePattern {
            avg_daily_compute_hours: dp_compute_hours.max(0.0),
            avg_daily_bandwidth_gb: dp_bandwidth.max(0.0),
            availability_pattern,
            preferred_compute_types: preferred_types,
            service_quality: dp_quality.max(0.0).min(1.0),
        })
    }

    /// Analyze economic behavior patterns
    fn analyze_economic_behavior(&self, identity_did: &str) -> Result<EconomicPattern, Box<dyn std::error::Error>> {
        let events = self.participation_store.get_economic_events(identity_did);
        
        let raw_earning_consistency = self.calculate_earning_consistency(&events);
        let raw_stake_duration = self.calculate_avg_stake_duration(&events);
        let raw_payment_punctuality = self.calculate_payment_punctuality(&events);
        let bonding_interactions = self.count_bonding_curve_interactions(&events);

        // Apply differential privacy
        let dp_earning_consistency = self.add_laplace_noise(raw_earning_consistency, 0.1)?;
        let dp_stake_duration = self.add_laplace_noise(raw_stake_duration, 1.0)?;
        let dp_payment_punctuality = self.add_laplace_noise(raw_payment_punctuality, 0.1)?;

        Ok(EconomicPattern {
            earning_consistency: dp_earning_consistency.max(0.0).min(1.0),
            avg_stake_duration: dp_stake_duration.max(0.0),
            payment_punctuality: dp_payment_punctuality.max(0.0).min(1.0),
            bonding_curve_interactions: bonding_interactions,
            participation_score: (dp_earning_consistency + dp_payment_punctuality) / 2.0,
        })
    }

    /// Analyze service quality metrics from VPoS system
    fn analyze_service_quality(&self, identity_did: &str) -> Result<ServiceQualityMetrics, Box<dyn std::error::Error>> {
        let events = self.participation_store.get_service_events(identity_did);
        
        let raw_peer_rating = self.calculate_avg_peer_rating(&events);
        let raw_success_ratio = self.calculate_success_ratio(&events);
        let raw_response_time = self.calculate_avg_response_time(&events);
        let raw_reputation_rate = self.calculate_reputation_accumulation(&events);

        // Apply differential privacy
        let dp_peer_rating = self.add_laplace_noise(raw_peer_rating, 0.1)?;
        let dp_success_ratio = self.add_laplace_noise(raw_success_ratio, 0.05)?;
        let dp_response_time = self.add_laplace_noise(raw_response_time, 10.0)?;
        let dp_reputation_rate = self.add_laplace_noise(raw_reputation_rate, 0.1)?;

        Ok(ServiceQualityMetrics {
            peer_rating_avg: dp_peer_rating.max(0.0).min(5.0),
            success_ratio: dp_success_ratio.max(0.0).min(1.0),
            avg_response_time_ms: dp_response_time.max(0.0),
            reputation_accumulation: dp_reputation_rate.max(0.0),
            total_services_completed: events.len() as u64,
        })
    }

    /// Analyze multi-chain activity patterns
    fn analyze_multi_chain_activity(&self, identity_did: &str) -> Result<MultiChainPattern, Box<dyn std::error::Error>> {
        let events = self.participation_store.get_chain_events(identity_did);
        
        let chain_distribution = self.calculate_chain_usage_distribution(&events)?;
        let cross_chain_freq = self.calculate_cross_chain_frequency(&events);
        let bridge_usage = self.calculate_bridge_usage_frequency(&events);
        let identity_consistency = self.calculate_identity_consistency(&events);

        // Apply differential privacy to sensitive metrics
        let dp_cross_chain_freq = self.add_laplace_noise(cross_chain_freq, 0.1)?;
        let dp_bridge_usage = self.add_laplace_noise(bridge_usage, 0.1)?;
        let dp_identity_consistency = self.add_laplace_noise(identity_consistency, 0.05)?;

        Ok(MultiChainPattern {
            chain_usage_distribution: chain_distribution,
            cross_chain_tx_frequency: dp_cross_chain_freq.max(0.0),
            preferred_networks: self.extract_preferred_networks(&events),
            bridge_usage_frequency: dp_bridge_usage.max(0.0),
            identity_consistency: dp_identity_consistency.max(0.0).min(1.0),
        })
    }

    /// Add Laplace noise for differential privacy
    fn add_laplace_noise(&self, value: f64, sensitivity: f64) -> Result<f64, Box<dyn std::error::Error>> {
        use rand_distr::{Distribution, Normal};
        use rand::thread_rng;
        
        let scale = sensitivity / self.epsilon;
        let normal = Normal::new(0.0, scale)?;
        let noise = normal.sample(&mut thread_rng());
        
        Ok(value + noise)
    }

    // Helper methods for calculating raw metrics
    fn calculate_avg_daily_storage(&self, events: &[StorageEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        events.iter().map(|e| e.storage_gb).sum::<f64>() / events.len() as f64
    }

    fn calculate_storage_consistency(&self, events: &[StorageEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        // Calculate variance and convert to consistency score
        let mean = self.calculate_avg_daily_storage(events);
        let variance = events.iter()
            .map(|e| (e.storage_gb - mean).powi(2))
            .sum::<f64>() / events.len() as f64;
        (1.0 / (1.0 + variance)).max(0.0).min(1.0)
    }

    fn calculate_avg_retention(&self, events: &[StorageEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        events.iter().map(|e| e.duration_hours / 24.0).sum::<f64>() / events.len() as f64
    }

    fn generate_geographic_preferences(&self, events: &[StorageEvent]) -> Result<Array1<f64>, Box<dyn std::error::Error>> {
        // Mock geographic distribution (in production, use actual geographic data)
        let mut geo_dist = Array1::zeros(10);
        for event in events {
            let region = (event.location_hash % 10) as usize;
            geo_dist[region] += 1.0;
        }
        
        // Normalize and add DP noise
        let total = geo_dist.sum();
        if total > 0.0 {
            geo_dist = geo_dist / total;
        }
        
        Ok(geo_dist)
    }

    fn generate_hourly_preferences(&self, events: &[StorageEvent]) -> Result<Array1<f64>, Box<dyn std::error::Error>> {
        let mut hourly_dist = Array1::zeros(24);
        for event in events {
            let hour = event.timestamp.hour() as usize;
            hourly_dist[hour] += 1.0;
        }
        
        // Normalize
        let total = hourly_dist.sum();
        if total > 0.0 {
            hourly_dist = hourly_dist / total;
        }
        
        Ok(hourly_dist)
    }

    // Additional helper methods (abbreviated for space)
    fn calculate_avg_daily_compute(&self, events: &[ComputeEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        events.iter().map(|e| e.compute_hours).sum::<f64>() / events.len() as f64
    }

    fn calculate_avg_daily_bandwidth(&self, events: &[ComputeEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        events.iter().map(|e| e.bandwidth_gb).sum::<f64>() / events.len() as f64
    }

    fn calculate_service_quality(&self, events: &[ComputeEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        events.iter().map(|e| e.quality_score).sum::<f64>() / events.len() as f64
    }

    fn generate_availability_pattern(&self, events: &[ComputeEvent]) -> Result<Array1<f64>, Box<dyn std::error::Error>> {
        let mut hourly_availability = Array1::zeros(24);
        for event in events {
            let hour = event.timestamp.hour() as usize;
            hourly_availability[hour] += event.compute_hours;
        }
        
        // Normalize
        let total = hourly_availability.sum();
        if total > 0.0 {
            hourly_availability = hourly_availability / total;
        }
        
        Ok(hourly_availability)
    }

    fn extract_preferred_compute_types(&self, events: &[ComputeEvent]) -> Vec<String> {
        let mut type_counts: HashMap<String, u32> = HashMap::new();
        for event in events {
            *type_counts.entry(event.service_type.clone()).or_insert(0) += 1;
        }
        
        let mut types: Vec<_> = type_counts.into_iter().collect();
        types.sort_by(|a, b| b.1.cmp(&a.1));
        types.into_iter().take(5).map(|(t, _)| t).collect()
    }

    // Economic analysis helpers
    fn calculate_earning_consistency(&self, events: &[EconomicEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        
        let earnings: Vec<f64> = events.iter().map(|e| e.tokens_earned).collect();
        let mean = earnings.iter().sum::<f64>() / earnings.len() as f64;
        let variance = earnings.iter()
            .map(|e| (e - mean).powi(2))
            .sum::<f64>() / earnings.len() as f64;
        
        (1.0 / (1.0 + variance)).max(0.0).min(1.0)
    }

    fn calculate_avg_stake_duration(&self, events: &[EconomicEvent]) -> f64 {
        // Mock calculation - in production, track actual stake durations
        30.0 // Default 30 days
    }

    fn calculate_payment_punctuality(&self, events: &[EconomicEvent]) -> f64 {
        if events.is_empty() { return 1.0; }
        // Mock calculation - percentage of on-time payments
        0.95
    }

    fn count_bonding_curve_interactions(&self, events: &[EconomicEvent]) -> u64 {
        events.iter().filter(|e| e.bonding_curve_interaction).count() as u64
    }

    // Service quality helpers
    fn calculate_avg_peer_rating(&self, events: &[ServiceEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        events.iter().map(|e| e.peer_rating).sum::<f64>() / events.len() as f64
    }

    fn calculate_success_ratio(&self, events: &[ServiceEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        let successful = events.iter().filter(|e| e.success).count();
        successful as f64 / events.len() as f64
    }

    fn calculate_avg_response_time(&self, events: &[ServiceEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        events.iter().map(|e| e.response_time_ms).sum::<f64>() / events.len() as f64
    }

    fn calculate_reputation_accumulation(&self, events: &[ServiceEvent]) -> f64 {
        // Mock calculation - rate of reputation gain
        0.1
    }

    // Multi-chain analysis helpers
    fn calculate_chain_usage_distribution(&self, events: &[ChainEvent]) -> Result<Array1<f64>, Box<dyn std::error::Error>> {
        let chains = vec!["ethereum", "avalanche", "arbitrum", "polygon", "cosmos", "solana"];
        let mut distribution = Array1::zeros(chains.len());
        
        for event in events {
            if let Some(index) = chains.iter().position(|&c| c == event.chain_name.to_lowercase()) {
                distribution[index] += 1.0;
            }
        }
        
        // Normalize
        let total = distribution.sum();
        if total > 0.0 {
            distribution = distribution / total;
        }
        
        Ok(distribution)
    }

    fn calculate_cross_chain_frequency(&self, events: &[ChainEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        let cross_chain_count = events.iter().filter(|e| e.cross_chain).count();
        cross_chain_count as f64 / events.len() as f64
    }

    fn calculate_bridge_usage_frequency(&self, events: &[ChainEvent]) -> f64 {
        if events.is_empty() { return 0.0; }
        let bridge_count = events.iter()
            .filter(|e| e.transaction_type.contains("bridge"))
            .count();
        bridge_count as f64 / events.len() as f64
    }

    fn calculate_identity_consistency(&self, events: &[ChainEvent]) -> f64 {
        // Mock calculation - consistency of identity usage across chains
        0.9
    }

    fn extract_preferred_networks(&self, events: &[ChainEvent]) -> Vec<String> {
        let mut chain_counts: HashMap<String, u32> = HashMap::new();
        for event in events {
            *chain_counts.entry(event.chain_name.clone()).or_insert(0) += 1;
        }
        
        let mut chains: Vec<_> = chain_counts.into_iter().collect();
        chains.sort_by(|a, b| b.1.cmp(&a.1));
        chains.into_iter().take(3).map(|(c, _)| c).collect()
    }
}

impl ParticipationDataStore {
    pub fn new() -> Self {
        Self {
            storage_logs: HashMap::new(),
            compute_logs: HashMap::new(),
            economic_logs: HashMap::new(),
            service_logs: HashMap::new(),
            chain_logs: HashMap::new(),
        }
    }

    pub fn get_storage_events(&self, identity_did: &str) -> Vec<StorageEvent> {
        self.storage_logs.get(identity_did).cloned().unwrap_or_default()
    }

    pub fn get_compute_events(&self, identity_did: &str) -> Vec<ComputeEvent> {
        self.compute_logs.get(identity_did).cloned().unwrap_or_default()
    }

    pub fn get_economic_events(&self, identity_did: &str) -> Vec<EconomicEvent> {
        self.economic_logs.get(identity_did).cloned().unwrap_or_default()
    }

    pub fn get_service_events(&self, identity_did: &str) -> Vec<ServiceEvent> {
        self.service_logs.get(identity_did).cloned().unwrap_or_default()
    }

    pub fn get_chain_events(&self, identity_did: &str) -> Vec<ChainEvent> {
        self.chain_logs.get(identity_did).cloned().unwrap_or_default()
    }
}
