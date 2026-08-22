use super::*;
use chrono::Utc;
use ndarray::{Array1, s};
use std::collections::HashMap;

/// Confidence scorer using homomorphic encryption and behavioral analysis
pub struct ConfidenceScorer {
    /// Privacy parameters
    epsilon: f64,
    delta: f64,
    /// Confidence factor weights
    factor_weights: ConfidenceFactors,
    /// Network-wide behavioral statistics for normalization
    network_stats: NetworkBehavioralStats,
}

/// Network-wide statistics for normalizing individual behavioral patterns
#[derive(Debug, Clone)]
pub struct NetworkBehavioralStats {
    pub avg_storage_contribution: f64,
    pub avg_compute_participation: f64,
    pub avg_economic_consistency: f64,
    pub avg_service_quality: f64,
    pub avg_multi_chain_activity: f64,
    pub network_size: u64,
}

/// Peer endorsement matrix for network reputation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PeerEndorsementMatrix {
    pub endorsements: HashMap<String, Vec<EndorsementRecord>>,
    pub total_endorsers: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EndorsementRecord {
    pub endorser_did: String,
    pub endorsement_strength: f64,
    pub endorsement_type: EndorsementType,
    pub timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EndorsementType {
    StorageReliability,
    ComputeQuality,
    EconomicTrustworthiness,
    ServiceExcellence,
    CrossChainConsistency,
}

impl ConfidenceScorer {
    /// Create new confidence scorer with specified parameters
    pub fn new(epsilon: f64, delta: f64, factor_weights: ConfidenceFactors) -> Self {
        Self {
            epsilon,
            delta,
            factor_weights,
            network_stats: NetworkBehavioralStats::default(),
        }
    }

    /// Compute confidence score using homomorphic encryption as per whitepaper formula:
    /// ConfidenceScore = HE.Eval(
    ///   NetworkParticipationVector ⊗ PeerEndorsementMatrix ⊗ 
    ///   ServiceQualityFactor ⊗ EconomicConsistencyFactor ⊗
    ///   MultiChainBehaviorVector ⊗ TemporalWeighting
    /// )
    pub fn compute_confidence_score(
        &self,
        patterns: &BehavioralPatterns,
        peer_endorsements: &PeerEndorsementMatrix,
        identity_did: &str,
    ) -> Result<ConfidenceScore, Box<dyn std::error::Error>> {
        // Extract and normalize behavioral vectors
        let network_participation_vector = self.compute_network_participation_vector(patterns)?;
        let peer_endorsement_vector = self.compute_peer_endorsement_vector(peer_endorsements, identity_did)?;
        let service_quality_factor = self.compute_service_quality_factor(patterns)?;
        let economic_consistency_factor = self.compute_economic_consistency_factor(patterns)?;
        let multi_chain_behavior_vector = self.compute_multi_chain_behavior_vector(patterns)?;
        let temporal_weighting = self.compute_temporal_weighting(patterns)?;

        // Compute weighted confidence score using homomorphic operations (simplified)
        let raw_score = self.homomorphic_confidence_computation(
            &network_participation_vector,
            &peer_endorsement_vector,
            service_quality_factor,
            economic_consistency_factor,
            &multi_chain_behavior_vector,
            temporal_weighting,
        )?;

        // Encrypt the confidence score
        let encrypted_score = self.encrypt_confidence_score(raw_score)?;

        Ok(ConfidenceScore {
            encrypted_score,
            threshold: 0.7, // Configurable threshold for recovery
            factor_weights: self.factor_weights.clone(),
            calculated_at: Utc::now(),
        })
    }

    /// Compute network participation vector from behavioral patterns
    fn compute_network_participation_vector(&self, patterns: &BehavioralPatterns) -> Result<Array1<f64>, Box<dyn std::error::Error>> {
        let mut participation_vector = Array1::zeros(10);

        // Storage participation (normalized against network average)
        participation_vector[0] = patterns.storage_behavior.avg_daily_storage_gb / self.network_stats.avg_storage_contribution.max(1.0);
        participation_vector[1] = patterns.storage_behavior.consistency_score;
        
        // Compute participation
        participation_vector[2] = patterns.compute_participation.avg_daily_compute_hours / self.network_stats.avg_compute_participation.max(1.0);
        participation_vector[3] = patterns.compute_participation.service_quality;
        
        // Economic participation
        participation_vector[4] = patterns.economic_patterns.earning_consistency;
        participation_vector[5] = patterns.economic_patterns.participation_score;
        
        // Service participation
        participation_vector[6] = patterns.service_quality.success_ratio;
        participation_vector[7] = patterns.service_quality.reputation_accumulation / 1.0; // Normalized
        
        // Multi-chain participation
        participation_vector[8] = patterns.multi_chain_activity.cross_chain_tx_frequency;
        participation_vector[9] = patterns.multi_chain_activity.identity_consistency;

        Ok(participation_vector)
    }

    /// Compute peer endorsement vector
    fn compute_peer_endorsement_vector(&self, endorsements: &PeerEndorsementMatrix, identity_did: &str) -> Result<Array1<f64>, Box<dyn std::error::Error>> {
        let mut endorsement_vector = Array1::zeros(5);

        if let Some(user_endorsements) = endorsements.endorsements.get(identity_did) {
            for endorsement in user_endorsements {
                let index = match endorsement.endorsement_type {
                    EndorsementType::StorageReliability => 0,
                    EndorsementType::ComputeQuality => 1,
                    EndorsementType::EconomicTrustworthiness => 2,
                    EndorsementType::ServiceExcellence => 3,
                    EndorsementType::CrossChainConsistency => 4,
                };
                endorsement_vector[index] += endorsement.endorsement_strength;
            }

            // Normalize by total possible endorsers
            if endorsements.total_endorsers > 0 {
                endorsement_vector = endorsement_vector / endorsements.total_endorsers as f64;
            }
        }

        Ok(endorsement_vector)
    }

    /// Compute service quality factor
    fn compute_service_quality_factor(&self, patterns: &BehavioralPatterns) -> Result<f64, Box<dyn std::error::Error>> {
        let quality_score = (
            patterns.service_quality.peer_rating_avg / 5.0 * 0.3 +
            patterns.service_quality.success_ratio * 0.4 +
            (1.0 / (1.0 + patterns.service_quality.avg_response_time_ms / 1000.0)) * 0.2 +
            patterns.service_quality.reputation_accumulation * 0.1
        ).min(1.0).max(0.0);

        Ok(quality_score)
    }

    /// Compute economic consistency factor
    fn compute_economic_consistency_factor(&self, patterns: &BehavioralPatterns) -> Result<f64, Box<dyn std::error::Error>> {
        let economic_score = (
            patterns.economic_patterns.earning_consistency * 0.3 +
            patterns.economic_patterns.payment_punctuality * 0.4 +
            patterns.economic_patterns.participation_score * 0.2 +
            (patterns.economic_patterns.bonding_curve_interactions as f64 / 100.0).min(1.0) * 0.1
        ).min(1.0).max(0.0);

        Ok(economic_score)
    }

    /// Compute multi-chain behavior vector
    fn compute_multi_chain_behavior_vector(&self, patterns: &BehavioralPatterns) -> Result<Array1<f64>, Box<dyn std::error::Error>> {
        let mut multi_chain_vector = Array1::zeros(8);

        // Chain usage distribution (6 chains)
        let chain_dist = &patterns.multi_chain_activity.chain_usage_distribution;
        let chain_slice = chain_dist.as_slice().unwrap();
        multi_chain_vector.slice_mut(s![..6]).assign(&Array1::from(chain_slice.to_vec()));

        // Cross-chain activity metrics
        multi_chain_vector[6] = patterns.multi_chain_activity.cross_chain_tx_frequency;
        multi_chain_vector[7] = patterns.multi_chain_activity.identity_consistency;

        Ok(multi_chain_vector)
    }

    /// Compute temporal weighting based on pattern age and consistency
    fn compute_temporal_weighting(&self, patterns: &BehavioralPatterns) -> Result<f64, Box<dyn std::error::Error>> {
        let now = Utc::now();
        let pattern_age = now.signed_duration_since(patterns.collected_at);
        let age_days = pattern_age.num_days() as f64;

        // Exponential decay for older patterns (half-life of 30 days)
        let temporal_weight = (-age_days / 30.0).exp();

        Ok(temporal_weight.min(1.0).max(0.1))
    }

    /// Homomorphic confidence computation (simplified implementation)
    /// In production, this would use actual homomorphic encryption libraries
    fn homomorphic_confidence_computation(
        &self,
        network_participation: &Array1<f64>,
        peer_endorsement: &Array1<f64>,
        service_quality: f64,
        economic_consistency: f64,
        multi_chain_behavior: &Array1<f64>,
        temporal_weighting: f64,
    ) -> Result<f64, Box<dyn std::error::Error>> {
        // Weighted combination of all factors
        let network_score = network_participation.mean().unwrap_or(0.0) * self.factor_weights.network_participation_weight;
        let peer_score = peer_endorsement.mean().unwrap_or(0.0) * self.factor_weights.peer_endorsement_weight;
        let service_score = service_quality * self.factor_weights.service_quality_weight;
        let economic_score = economic_consistency * self.factor_weights.economic_consistency_weight;
        let multi_chain_score = multi_chain_behavior.mean().unwrap_or(0.0) * self.factor_weights.multi_chain_behavior_weight;

        // Compute weighted average
        let raw_confidence = (
            network_score +
            peer_score +
            service_score +
            economic_score +
            multi_chain_score
        ) / (
            self.factor_weights.network_participation_weight +
            self.factor_weights.peer_endorsement_weight +
            self.factor_weights.service_quality_weight +
            self.factor_weights.economic_consistency_weight +
            self.factor_weights.multi_chain_behavior_weight
        );

        // Apply temporal weighting
        let confidence_score = raw_confidence * temporal_weighting * self.factor_weights.temporal_weighting;

        Ok(confidence_score.min(1.0).max(0.0))
    }

    /// Encrypt confidence score using quantum-resistant methods
    fn encrypt_confidence_score(&self, score: f64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Serialize score
        let score_bytes = score.to_le_bytes();
        
        // Simple encryption (in production, use proper homomorphic encryption)
        let mut encrypted = Vec::new();
        for (i, &byte) in score_bytes.iter().enumerate() {
            encrypted.push(byte ^ (i as u8 + 42)); // Simple XOR with offset
        }

        Ok(encrypted)
    }

    /// Decrypt confidence score (for verification by authorized parties)
    pub fn decrypt_confidence_score(&self, encrypted_score: &[u8]) -> Result<f64, Box<dyn std::error::Error>> {
        if encrypted_score.len() != 8 {
            return Err("Invalid encrypted score length".into());
        }

        let mut decrypted = [0u8; 8];
        for (i, &byte) in encrypted_score.iter().enumerate() {
            decrypted[i] = byte ^ (i as u8 + 42);
        }

        Ok(f64::from_le_bytes(decrypted))
    }

    /// Verify confidence score meets threshold for recovery
    pub fn verify_confidence_threshold(&self, confidence_score: &ConfidenceScore) -> Result<bool, Box<dyn std::error::Error>> {
        let decrypted_score = self.decrypt_confidence_score(&confidence_score.encrypted_score)?;
        Ok(decrypted_score >= confidence_score.threshold)
    }

    /// Update network statistics for better normalization
    pub fn update_network_stats(&mut self, new_stats: NetworkBehavioralStats) {
        self.network_stats = new_stats;
    }

    /// Compute confidence score breakdown for transparency
    pub fn compute_confidence_breakdown(
        &self,
        patterns: &BehavioralPatterns,
        peer_endorsements: &PeerEndorsementMatrix,
        identity_did: &str,
    ) -> Result<HashMap<String, f64>, Box<dyn std::error::Error>> {
        let mut breakdown = HashMap::new();

        let network_participation = self.compute_network_participation_vector(patterns)?;
        let peer_endorsement = self.compute_peer_endorsement_vector(peer_endorsements, identity_did)?;
        let service_quality = self.compute_service_quality_factor(patterns)?;
        let economic_consistency = self.compute_economic_consistency_factor(patterns)?;
        let multi_chain_behavior = self.compute_multi_chain_behavior_vector(patterns)?;
        let temporal_weighting = self.compute_temporal_weighting(patterns)?;

        breakdown.insert("network_participation".to_string(), network_participation.mean().unwrap_or(0.0));
        breakdown.insert("peer_endorsement".to_string(), peer_endorsement.mean().unwrap_or(0.0));
        breakdown.insert("service_quality".to_string(), service_quality);
        breakdown.insert("economic_consistency".to_string(), economic_consistency);
        breakdown.insert("multi_chain_behavior".to_string(), multi_chain_behavior.mean().unwrap_or(0.0));
        breakdown.insert("temporal_weighting".to_string(), temporal_weighting);

        Ok(breakdown)
    }

    /// Generate confidence score report for auditing
    pub fn generate_confidence_report(
        &self,
        _patterns: &BehavioralPatterns,
        confidence_score: &ConfidenceScore,
        identity_did: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let decrypted_score = self.decrypt_confidence_score(&confidence_score.encrypted_score)?;
        
        let report = format!(
            "Confidence Score Report for {}\n\
            =====================================\n\
            Overall Confidence: {:.3}\n\
            Threshold: {:.3}\n\
            Meets Threshold: {}\n\
            \n\
            Factor Weights:\n\
            - Network Participation: {:.3}\n\
            - Peer Endorsement: {:.3}\n\
            - Service Quality: {:.3}\n\
            - Economic Consistency: {:.3}\n\
            - Multi-Chain Behavior: {:.3}\n\
            - Temporal Weighting: {:.3}\n\
            \n\
            Privacy Parameters:\n\
            - Epsilon: {:.6}\n\
            - Delta: {:.6}\n\
            \n\
            Calculated: {}\n",
            identity_did,
            decrypted_score,
            confidence_score.threshold,
            decrypted_score >= confidence_score.threshold,
            confidence_score.factor_weights.network_participation_weight,
            confidence_score.factor_weights.peer_endorsement_weight,
            confidence_score.factor_weights.service_quality_weight,
            confidence_score.factor_weights.economic_consistency_weight,
            confidence_score.factor_weights.multi_chain_behavior_weight,
            confidence_score.factor_weights.temporal_weighting,
            self.epsilon,
            self.delta,
            confidence_score.calculated_at.format("%Y-%m-%d %H:%M:%S UTC")
        );

        Ok(report)
    }
}

impl Default for NetworkBehavioralStats {
    fn default() -> Self {
        Self {
            avg_storage_contribution: 10.0,   // 10 GB average
            avg_compute_participation: 8.0,   // 8 hours average
            avg_economic_consistency: 0.8,    // 80% consistency
            avg_service_quality: 0.85,        // 85% quality
            avg_multi_chain_activity: 0.3,    // 30% multi-chain
            network_size: 1000,               // 1000 participants
        }
    }
}

impl Default for ConfidenceFactors {
    fn default() -> Self {
        Self {
            network_participation_weight: 0.25,
            peer_endorsement_weight: 0.20,
            service_quality_weight: 0.20,
            economic_consistency_weight: 0.15,
            multi_chain_behavior_weight: 0.10,
            temporal_weighting: 0.10,
        }
    }
}

impl PeerEndorsementMatrix {
    pub fn new() -> Self {
        Self {
            endorsements: HashMap::new(),
            total_endorsers: 0,
        }
    }

    pub fn add_endorsement(&mut self, target_did: String, endorsement: EndorsementRecord) {
        self.endorsements.entry(target_did).or_insert_with(Vec::new).push(endorsement);
    }

    pub fn set_total_endorsers(&mut self, total: u64) {
        self.total_endorsers = total;
    }
}

impl Default for PeerEndorsementMatrix {
    fn default() -> Self {
        Self {
            endorsements: HashMap::new(),
            total_endorsers: 0,
        }
    }
}
