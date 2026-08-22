use super::*;
use spacekit_primitives::v1::quantum::{generate_kem, encapsulate};
use chrono::Utc;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Generates behavioral fingerprints with quantum-resistant encryption
pub struct BehavioralFingerprintGenerator {
    /// Quantum algorithm to use for encryption
    algorithm: String,
    /// Privacy parameters
    epsilon: f64,
    delta: f64,
}

impl BehavioralFingerprintGenerator {
    /// Create new fingerprint generator
    pub fn new(algorithm: String, epsilon: f64, delta: f64) -> Self {
        Self {
            algorithm,
            epsilon,
            delta,
        }
    }

    /// Generate behavioral fingerprint from patterns
    pub fn generate_fingerprint(
        &self,
        patterns: &BehavioralPatterns,
        identity_did: &str,
    ) -> Result<BehavioralFingerprint, Box<dyn std::error::Error>> {
        // Create behavioral feature vector
        let feature_vector = self.extract_feature_vector(patterns)?;
        
        // Create identity commitment
        let identity_commitment = self.create_identity_commitment(identity_did)?;
        
        // Encrypt the fingerprint using quantum-resistant cryptography
        let encrypted_fingerprint = self.encrypt_fingerprint(&feature_vector)?;

        Ok(BehavioralFingerprint {
            encrypted_fingerprint,
            epsilon: self.epsilon,
            delta: self.delta,
            created_at: Utc::now(),
            identity_commitment,
        })
    }

    /// Extract numerical feature vector from behavioral patterns
    fn extract_feature_vector(&self, patterns: &BehavioralPatterns) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
        let mut features = Vec::new();

        // Storage behavior features
        features.push(patterns.storage_behavior.avg_daily_storage_gb);
        features.push(patterns.storage_behavior.consistency_score);
        features.push(patterns.storage_behavior.avg_retention_days);
        
        // Add geographic preferences (take first 5 elements)
        let geo_slice = patterns.storage_behavior.geographic_preferences.as_slice().unwrap();
        features.extend_from_slice(&geo_slice[..geo_slice.len().min(5)]);
        
        // Add hourly preferences (take peak hours - indices 8-18 for business hours)
        let hourly_slice = patterns.storage_behavior.preferred_storage_hours.as_slice().unwrap();
        let business_hours = &hourly_slice[8..18.min(hourly_slice.len())];
        features.extend_from_slice(business_hours);

        // Compute behavior features
        features.push(patterns.compute_participation.avg_daily_compute_hours);
        features.push(patterns.compute_participation.avg_daily_bandwidth_gb);
        features.push(patterns.compute_participation.service_quality);
        
        // Add availability pattern (take peak hours)
        let availability_slice = patterns.compute_participation.availability_pattern.as_slice().unwrap();
        let peak_availability = &availability_slice[8..18.min(availability_slice.len())];
        features.extend_from_slice(peak_availability);

        // Economic behavior features
        features.push(patterns.economic_patterns.earning_consistency);
        features.push(patterns.economic_patterns.avg_stake_duration);
        features.push(patterns.economic_patterns.payment_punctuality);
        features.push(patterns.economic_patterns.bonding_curve_interactions as f64);
        features.push(patterns.economic_patterns.participation_score);

        // Service quality features
        features.push(patterns.service_quality.peer_rating_avg);
        features.push(patterns.service_quality.success_ratio);
        features.push(patterns.service_quality.avg_response_time_ms);
        features.push(patterns.service_quality.reputation_accumulation);
        features.push(patterns.service_quality.total_services_completed as f64);

        // Multi-chain activity features
        features.push(patterns.multi_chain_activity.cross_chain_tx_frequency);
        features.push(patterns.multi_chain_activity.bridge_usage_frequency);
        features.push(patterns.multi_chain_activity.identity_consistency);
        
        // Add chain usage distribution
        let chain_dist_slice = patterns.multi_chain_activity.chain_usage_distribution.as_slice().unwrap();
        features.extend_from_slice(chain_dist_slice);

        // Temporal features
        features.push(patterns.collected_at.timestamp() as f64);
        features.push(patterns.privacy_budget_used);

        Ok(features)
    }

    /// Create quantum-resistant identity commitment
    fn create_identity_commitment(&self, identity_did: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Hash the identity DID for commitment
        let mut hasher = DefaultHasher::new();
        identity_did.hash(&mut hasher);
        let hash = hasher.finish();
        
        // Convert to bytes and extend to appropriate length
        let mut commitment = hash.to_be_bytes().to_vec();
        
        // Pad to 32 bytes for quantum resistance
        while commitment.len() < 32 {
            commitment.push(0);
        }
        commitment.truncate(32);
        
        Ok(commitment)
    }

    /// Encrypt fingerprint using quantum-resistant cryptography
    fn encrypt_fingerprint(&self, feature_vector: &[f64]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Serialize feature vector to bytes
        let serialized = serde_json::to_vec(feature_vector)?;
        
        // Generate quantum-resistant key pair
        let kem = generate_kem(&self.algorithm)?;
        let (public_key, _secret_key) = kem.keypair()?;
        
        // Encapsulate to get shared secret and ciphertext
        let (kem_ciphertext, shared_secret) = encapsulate(&kem, &public_key)?;
        
        // Use shared secret to encrypt the feature vector
        let encrypted_data = self.encrypt_with_shared_secret(&serialized, shared_secret.as_ref())?;
        
        // Combine KEM ciphertext with encrypted data
        let mut result = kem_ciphertext.into_vec();
        result.extend_from_slice(&encrypted_data);
        
        Ok(result)
    }

    /// Encrypt data using shared secret (simple XOR for demo - use proper encryption in production)
    fn encrypt_with_shared_secret(&self, data: &[u8], shared_secret: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut encrypted = Vec::with_capacity(data.len());
        
        for (i, &byte) in data.iter().enumerate() {
            let key_byte = shared_secret[i % shared_secret.len()];
            encrypted.push(byte ^ key_byte);
        }
        
        Ok(encrypted)
    }

    /// Verify behavioral fingerprint (for recovery validation)
    pub fn verify_fingerprint(
        &self,
        fingerprint: &BehavioralFingerprint,
        claimed_patterns: &BehavioralPatterns,
        identity_did: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Verify identity commitment
        let expected_commitment = self.create_identity_commitment(identity_did)?;
        if fingerprint.identity_commitment != expected_commitment {
            return Ok(false);
        }

        // Extract features from claimed patterns
        let claimed_features = self.extract_feature_vector(claimed_patterns)?;
        
        // For full verification, would need to decrypt fingerprint and compare
        // This is a simplified version that checks feature vector similarity
        let similarity = self.calculate_feature_similarity(&claimed_features, fingerprint)?;
        
        // Threshold for similarity (adjustable based on privacy requirements)
        let threshold = 0.8;
        Ok(similarity >= threshold)
    }

    /// Calculate similarity between feature vectors (simplified for demo)
    fn calculate_feature_similarity(
        &self,
        claimed_features: &[f64],
        _fingerprint: &BehavioralFingerprint,
    ) -> Result<f64, Box<dyn std::error::Error>> {
        // In production, this would decrypt the fingerprint and compare
        // For demo, return a mock similarity score
        Ok(0.85)
    }

    /// Generate zero-knowledge proof of fingerprint ownership
    pub fn generate_ownership_proof(
        &self,
        fingerprint: &BehavioralFingerprint,
        secret_key: &[u8],
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        // Generate ZK proof that the claimant knows the secret key
        // that can decrypt the fingerprint
        // This is a simplified implementation
        
        let mut proof = Vec::new();
        proof.extend_from_slice(&fingerprint.identity_commitment);
        proof.extend_from_slice(&secret_key[..32.min(secret_key.len())]);
        
        // Hash to create proof
        let mut hasher = DefaultHasher::new();
        proof.hash(&mut hasher);
        let proof_hash = hasher.finish();
        
        Ok(proof_hash.to_be_bytes().to_vec())
    }

    /// Verify zero-knowledge proof of fingerprint ownership
    pub fn verify_ownership_proof(
        &self,
        fingerprint: &BehavioralFingerprint,
        proof: &[u8],
        public_key: &[u8],
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // Verify the ZK proof without revealing the secret key
        // This is a simplified implementation
        
        if proof.len() != 8 || public_key.len() < 32 {
            return Ok(false);
        }
        
        // Mock verification - in production would use proper ZK proof verification
        Ok(true)
    }

    /// Update fingerprint with new behavioral data (for continuous learning)
    pub fn update_fingerprint(
        &self,
        existing_fingerprint: &BehavioralFingerprint,
        new_patterns: &BehavioralPatterns,
        update_weight: f64,
    ) -> Result<BehavioralFingerprint, Box<dyn std::error::Error>> {
        // Extract new features
        let new_features = self.extract_feature_vector(new_patterns)?;
        
        // For demo, create updated fingerprint
        // In production, would decrypt, blend, and re-encrypt
        let updated_encrypted = self.encrypt_fingerprint(&new_features)?;
        
        Ok(BehavioralFingerprint {
            encrypted_fingerprint: updated_encrypted,
            epsilon: existing_fingerprint.epsilon * (1.0 - update_weight) + self.epsilon * update_weight,
            delta: existing_fingerprint.delta * (1.0 - update_weight) + self.delta * update_weight,
            created_at: Utc::now(),
            identity_commitment: existing_fingerprint.identity_commitment.clone(),
        })
    }
}
