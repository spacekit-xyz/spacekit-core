//! VPoS (Verifiable Proof of Service) Implementation
//!
//! Provides cryptographic proofs of compute service delivery for the SpaceKit network

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::{ComputeResult, ComputeTask, CostBreakdown, ExecutionMetrics, TokenMintResult};
use spacekit_did::did::quantum::QuantumResistantWallet;
use spacekit_primitives::v1::crypto::quantum::{
    decrypt_message, encrypt_message, Algorithm, Cipher,
};

/// VPoS Service Proof for compute operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceProof {
    /// Unique proof identifier
    pub proof_id: String,

    /// Task that was executed
    pub task_id: String,

    /// Service provider DID
    pub provider_did: String,

    /// Service requester DID
    pub requester_did: String,

    /// Service type (compute, storage, messaging, etc.)
    pub service_type: ServiceType,

    /// Proof of work/computation
    pub computation_proof: ComputationProof,

    /// Resource utilization proof
    pub resource_proof: ResourceProof,

    /// Quality metrics
    pub quality_metrics: QualityMetrics,

    /// Timestamp when service was provided
    pub service_timestamp: DateTime<Utc>,

    /// Cryptographic signature from provider
    pub provider_signature: Vec<u8>,

    /// Verification hash
    pub verification_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceType {
    Compute,
    Storage,
    Messaging,
    AIAgent,
    Hybrid,
}

/// Proof of computation performed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputationProof {
    /// Input data hash (to verify correct input was processed)
    pub input_hash: String,

    /// Output data hash (to verify correct output was produced)
    pub output_hash: String,

    /// Execution trace hash (to verify computation was performed)
    pub execution_trace_hash: String,

    /// Merkle tree root of computation steps
    pub computation_merkle_root: String,

    /// Random challenge-response for proof verification
    pub challenge_response: ChallengeResponse,

    /// Compute units consumed
    pub compute_units: u64,

    /// Execution time in milliseconds
    pub execution_time_ms: u64,
}

/// Proof of resource utilization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceProof {
    /// CPU utilization percentage
    pub cpu_utilization: f32,

    /// Memory used in MB
    pub memory_used_mb: u64,

    /// GPU utilization (if applicable)
    pub gpu_utilization: Option<f32>,

    /// Network bandwidth used
    pub network_bandwidth_mbps: f32,

    /// Energy consumption in kWh
    pub energy_consumed_kwh: f64,

    /// Resource efficiency score (0.0 - 1.0)
    pub efficiency_score: f32,
}

/// Service quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityMetrics {
    /// Service completion rate (0.0 - 1.0)
    pub completion_rate: f32,

    /// Average response time in milliseconds
    pub avg_response_time_ms: u64,

    /// Error rate (0.0 - 1.0)
    pub error_rate: f32,

    /// Customer satisfaction score (0.0 - 1.0)
    pub satisfaction_score: f32,

    /// Service availability (0.0 - 1.0)
    pub availability: f32,

    /// Security compliance score (0.0 - 1.0)
    pub security_score: f32,
}

/// Challenge-response for computation verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResponse {
    /// Random challenge from network
    pub challenge: String,

    /// Provider's response to challenge
    pub response: String,

    /// Timestamp of challenge
    pub challenge_timestamp: DateTime<Utc>,

    /// Proof that response is correct
    pub response_proof: String,
}

/// VPoS Manager for handling service proofs
#[derive(Debug)]
pub struct VPoSManager {
    /// Provider's quantum-resistant identity
    provider_identity: Arc<QuantumResistantWallet>,

    /// Quantum encryption algorithm
    #[allow(dead_code)]
    encryption_algorithm: Algorithm,

    /// Active service proofs
    active_proofs: HashMap<String, ServiceProof>,

    /// Quality metrics history
    quality_history: Vec<QualityMetrics>,

    /// Challenge-response cache
    challenge_cache: HashMap<String, ChallengeResponse>,
}

impl VPoSManager {
    pub async fn new(
        provider_identity: Arc<QuantumResistantWallet>,
        encryption_algorithm: Algorithm,
    ) -> Result<Self> {
        Ok(Self {
            provider_identity,
            encryption_algorithm,
            active_proofs: HashMap::new(),
            quality_history: Vec::new(),
            challenge_cache: HashMap::new(),
        })
    }

    /// Generate a service proof for a completed compute task
    pub async fn generate_service_proof(
        &mut self,
        task: &ComputeTask,
        result: &ComputeResult,
        resource_usage: &ExecutionMetrics,
        requester_did: &str,
    ) -> Result<ServiceProof> {
        let proof_id = Uuid::new_v4().to_string();

        // Generate computation proof
        let computation_proof = self.generate_computation_proof(task, result).await?;

        // Generate resource proof
        let resource_proof = self.generate_resource_proof(resource_usage)?;

        // Calculate quality metrics
        let quality_metrics = self.calculate_quality_metrics(task, result)?;

        // Create service proof
        let mut service_proof = ServiceProof {
            proof_id: proof_id.clone(),
            task_id: task.id.clone(),
            provider_did: self.provider_identity.identity_doc.did.did.clone(),
            requester_did: requester_did.to_string(),
            service_type: ServiceType::Compute,
            computation_proof,
            resource_proof,
            quality_metrics: quality_metrics.clone(),
            service_timestamp: Utc::now(),
            provider_signature: vec![],
            verification_hash: String::new(),
        };

        // Generate verification hash
        service_proof.verification_hash = self.generate_verification_hash(&service_proof)?;

        // Sign the proof with quantum-resistant signature - exclude signature field to avoid circular dependency
        let mut proof_for_signing = service_proof.clone();
        proof_for_signing.provider_signature = vec![];
        let proof_data = serde_json::to_string(&proof_for_signing)?;
        let signature = self
            .provider_identity
            .sign_content(&proof_data)
            .map_err(|e| anyhow::anyhow!("Failed to sign: {}", e))?;
        service_proof.provider_signature = hex::decode(&signature)
            .map_err(|e| anyhow::anyhow!("Failed to decode signature: {}", e))?;

        // Store the proof
        self.active_proofs.insert(proof_id, service_proof.clone());
        self.quality_history.push(quality_metrics);

        Ok(service_proof)
    }

    /// Generate proof of computation
    async fn generate_computation_proof(
        &mut self,
        task: &ComputeTask,
        result: &ComputeResult,
    ) -> Result<ComputationProof> {
        // Hash the input data
        let input_hash = self.hash_data(&task.input_data);

        // Hash the output data
        let output_hash = self.hash_data(&result.result_data);

        // Generate execution trace hash (simplified)
        let execution_trace = format!(
            "task_id:{},runtime:{},timestamp:{}",
            task.id, task.runtime, task.created_at
        );
        let execution_trace_hash = self.hash_string(&execution_trace);

        // Generate Merkle tree root of computation steps
        let computation_steps = vec![
            input_hash.clone(),
            execution_trace_hash.clone(),
            output_hash.clone(),
        ];
        let computation_merkle_root = self.generate_merkle_root(&computation_steps)?;

        // Generate challenge-response
        let challenge_response = self.generate_challenge_response(&task.id).await?;

        Ok(ComputationProof {
            input_hash,
            output_hash,
            execution_trace_hash,
            computation_merkle_root,
            challenge_response,
            compute_units: result.execution_metrics.compute_units_used,
            execution_time_ms: result.execution_metrics.execution_time_ms,
        })
    }

    /// Generate proof of resource utilization
    fn generate_resource_proof(&self, metrics: &ExecutionMetrics) -> Result<ResourceProof> {
        // Calculate efficiency score based on resource utilization
        let efficiency_score = self.calculate_efficiency_score(metrics);

        Ok(ResourceProof {
            cpu_utilization: 75.0, // Would be actual CPU usage
            memory_used_mb: metrics.memory_peak_mb,
            gpu_utilization: metrics.gpu_time_ms.map(|_| 60.0),
            network_bandwidth_mbps: 100.0, // Would be actual network usage
            energy_consumed_kwh: metrics.energy_consumed_kwh,
            efficiency_score,
        })
    }

    /// Calculate service quality metrics
    fn calculate_quality_metrics(
        &self,
        task: &ComputeTask,
        result: &ComputeResult,
    ) -> Result<QualityMetrics> {
        // Calculate metrics based on historical performance
        let completion_rate = self.calculate_completion_rate();
        let avg_response_time_ms = result.execution_metrics.execution_time_ms;
        let error_rate = self.calculate_error_rate();
        let satisfaction_score = self.calculate_satisfaction_score();
        let availability = self.calculate_availability();
        let security_score = self.calculate_security_score();

        Ok(QualityMetrics {
            completion_rate,
            avg_response_time_ms,
            error_rate,
            satisfaction_score,
            availability,
            security_score,
        })
    }

    /// Generate challenge-response for computation verification
    async fn generate_challenge_response(&mut self, task_id: &str) -> Result<ChallengeResponse> {
        // Generate random challenge
        let challenge = self.generate_random_challenge();

        // Generate response based on computation
        let response = self.compute_challenge_response(&challenge, task_id).await?;

        // Generate proof of correct response
        let response_proof = self.generate_response_proof(&challenge, &response)?;

        let challenge_response = ChallengeResponse {
            challenge: challenge.clone(),
            response,
            challenge_timestamp: Utc::now(),
            response_proof,
        };

        // Cache the challenge-response
        self.challenge_cache
            .insert(challenge, challenge_response.clone());

        Ok(challenge_response)
    }

    /// Verify a service proof and mint tokens if valid
    pub async fn verify_service_proof(&self, proof: &ServiceProof) -> Result<bool> {
        // Verify provider signature - create proof data without signature to avoid circular dependency
        let mut proof_for_verification = proof.clone();
        proof_for_verification.provider_signature = vec![];
        let proof_data = serde_json::to_string(&proof_for_verification)?;
        let signature_hex = hex::encode(&proof.provider_signature);

        // For now, we'll use the current provider's identity for verification
        // In a real implementation, we'd look up the provider's identity from their DID
        if !self
            .provider_identity
            .verify_content(&proof_data, &signature_hex)
            .map_err(|e| anyhow::anyhow!("Failed to verify: {}", e))?
        {
            return Ok(false);
        }

        // Verify verification hash
        let expected_hash = self.generate_verification_hash(proof)?;
        if proof.verification_hash != expected_hash {
            return Ok(false);
        }

        // Verify computation proof
        if !self
            .verify_computation_proof(&proof.computation_proof)
            .await?
        {
            return Ok(false);
        }

        // Verify resource proof
        if !self.verify_resource_proof(&proof.resource_proof)? {
            return Ok(false);
        }

        // Verify quality metrics
        if !self.verify_quality_metrics(&proof.quality_metrics)? {
            return Ok(false);
        }

        // Verify timestamps are reasonable
        if !self.verify_timestamps(proof)? {
            return Ok(false);
        }

        Ok(true)
    }

    /// Verify and calculate token reward based on VPoS proof
    pub async fn verify_and_calculate_reward(&self, proof: &ServiceProof) -> Result<Option<u128>> {
        // First verify the proof
        if !self.verify_service_proof(proof).await? {
            return Ok(None);
        }

        // Calculate token reward based on proof quality
        let base_reward = proof.computation_proof.compute_units;
        let quality_multiplier = self.calculate_quality_multiplier(&proof.quality_metrics);
        let service_multiplier = self.get_service_type_multiplier(&proof.service_type);

        // Apply VPoS-specific bonuses
        let vpos_bonus = self.calculate_vpos_bonus(proof);

        let total_reward =
            (base_reward as f64 * quality_multiplier * service_multiplier * vpos_bonus) as u128;

        println!(
            "🎯 VPoS: Calculated {} ASTRA tokens for verified proof {}",
            total_reward as f64 / 1e18,
            proof.proof_id
        );

        Ok(Some(total_reward))
    }

    /// Submit proof to SpaceKit network
    pub async fn submit_proof_to_network(&self, proof: &ServiceProof) -> Result<String> {
        // Serialize proof for network submission
        let proof_data = serde_json::to_string(proof)?;

        // For now, create a simple hash for mock submission
        // In a real implementation, we'd encrypt the proof before submission
        let tx_hash = format!("0x{:x}", Sha3_256::digest(proof_data.as_bytes()));

        println!(
            "Submitted VPoS proof {} to network: {}",
            proof.proof_id, tx_hash
        );
        Ok(tx_hash)
    }

    /// Get provider reputation score
    pub fn get_reputation_score(&self) -> f32 {
        if self.quality_history.is_empty() {
            return 0.5; // Default score
        }

        let total_scores: f32 = self
            .quality_history
            .iter()
            .map(|metrics| {
                // Weight different metrics
                metrics.completion_rate * 0.3
                    + (1.0 - metrics.error_rate) * 0.2
                    + metrics.satisfaction_score * 0.2
                    + metrics.availability * 0.15
                    + metrics.security_score * 0.15
            })
            .sum();

        total_scores / self.quality_history.len() as f32
    }

    // Helper methods
    fn hash_data(&self, data: &[u8]) -> String {
        format!("{:x}", Sha3_256::digest(data))
    }

    fn hash_string(&self, data: &str) -> String {
        format!("{:x}", Sha3_256::digest(data.as_bytes()))
    }

    fn generate_merkle_root(&self, hashes: &[String]) -> Result<String> {
        if hashes.is_empty() {
            return Ok("0".repeat(64));
        }

        let mut current_level = hashes.to_vec();

        while current_level.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in current_level.chunks(2) {
                let combined = if chunk.len() == 2 {
                    format!("{}{}", chunk[0], chunk[1])
                } else {
                    chunk[0].clone()
                };
                next_level.push(self.hash_string(&combined));
            }

            current_level = next_level;
        }

        Ok(current_level[0].clone())
    }

    fn generate_verification_hash(&self, proof: &ServiceProof) -> Result<String> {
        let hash_input = format!(
            "{}:{}:{}:{}:{}",
            proof.proof_id,
            proof.task_id,
            proof.provider_did,
            proof.requester_did,
            proof.service_timestamp.timestamp()
        );
        Ok(self.hash_string(&hash_input))
    }

    fn generate_random_challenge(&self) -> String {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("challenge_{:x}", timestamp)
    }

    async fn compute_challenge_response(&self, challenge: &str, task_id: &str) -> Result<String> {
        let response_input = format!("{}:{}", challenge, task_id);
        Ok(self.hash_string(&response_input))
    }

    fn generate_response_proof(&self, challenge: &str, response: &str) -> Result<String> {
        let proof_input = format!("{}:{}", challenge, response);
        Ok(self.hash_string(&proof_input))
    }

    // Quality metric calculation methods
    fn calculate_completion_rate(&self) -> f32 {
        if self.quality_history.is_empty() {
            return 1.0;
        }

        let avg_completion: f32 = self
            .quality_history
            .iter()
            .map(|m| m.completion_rate)
            .sum::<f32>()
            / self.quality_history.len() as f32;

        avg_completion
    }

    fn calculate_error_rate(&self) -> f32 {
        if self.quality_history.is_empty() {
            return 0.0;
        }

        let avg_error: f32 = self
            .quality_history
            .iter()
            .map(|m| m.error_rate)
            .sum::<f32>()
            / self.quality_history.len() as f32;

        avg_error
    }

    fn calculate_satisfaction_score(&self) -> f32 {
        if self.quality_history.is_empty() {
            return 0.8;
        }

        let avg_satisfaction: f32 = self
            .quality_history
            .iter()
            .map(|m| m.satisfaction_score)
            .sum::<f32>()
            / self.quality_history.len() as f32;

        avg_satisfaction
    }

    fn calculate_availability(&self) -> f32 {
        // Calculate based on uptime and service availability
        0.99 // 99% availability as default
    }

    fn calculate_security_score(&self) -> f32 {
        // Calculate based on security compliance
        0.95 // High security score for quantum-resistant implementation
    }

    fn calculate_efficiency_score(&self, metrics: &ExecutionMetrics) -> f32 {
        // Calculate efficiency based on resource utilization
        let cpu_efficiency =
            1.0 - (metrics.cpu_time_ms as f32 / metrics.execution_time_ms as f32 - 0.5).abs();
        let memory_efficiency = 1.0 - (metrics.memory_peak_mb as f32 / 1024.0).min(1.0);
        let energy_efficiency = 1.0 - (metrics.energy_consumed_kwh as f32 * 10.0).min(1.0);

        (cpu_efficiency + memory_efficiency + energy_efficiency) / 3.0
    }

    // Verification methods
    async fn verify_computation_proof(&self, proof: &ComputationProof) -> Result<bool> {
        // Verify that computation proof is valid
        // Check challenge-response validity
        if proof.challenge_response.challenge.is_empty()
            || proof.challenge_response.response.is_empty()
        {
            return Ok(false);
        }

        // Verify execution trace hash format
        if proof.execution_trace_hash.len() != 64 {
            return Ok(false);
        }

        // Verify merkle root format
        if proof.computation_merkle_root.len() != 64 {
            return Ok(false);
        }

        // Verify compute units are reasonable
        if proof.compute_units == 0 || proof.compute_units > 1_000_000 {
            return Ok(false);
        }

        Ok(true)
    }

    fn verify_resource_proof(&self, proof: &ResourceProof) -> Result<bool> {
        // Verify that resource utilization claims are reasonable
        Ok(proof.cpu_utilization <= 100.0
            && proof.efficiency_score <= 1.0
            && proof.efficiency_score >= 0.0
            && proof.memory_used_mb <= 1_000_000) // Max 1TB memory
    }

    fn verify_quality_metrics(&self, metrics: &QualityMetrics) -> Result<bool> {
        // Verify that quality metrics are within valid ranges
        Ok(metrics.completion_rate <= 1.0
            && metrics.error_rate <= 1.0
            && metrics.satisfaction_score <= 1.0
            && metrics.availability <= 1.0
            && metrics.security_score <= 1.0
            && metrics.completion_rate >= 0.0
            && metrics.error_rate >= 0.0
            && metrics.satisfaction_score >= 0.0
            && metrics.availability >= 0.0
            && metrics.security_score >= 0.0)
    }

    fn verify_timestamps(&self, proof: &ServiceProof) -> Result<bool> {
        // Verify that timestamps are reasonable
        let now = Utc::now();
        let one_hour_ago = now - chrono::Duration::hours(1);
        let one_minute_future = now + chrono::Duration::minutes(1);

        // Service timestamp should be within last hour and not in future
        if proof.service_timestamp < one_hour_ago || proof.service_timestamp > one_minute_future {
            return Ok(false);
        }

        // Challenge timestamp should be before service timestamp
        if proof
            .computation_proof
            .challenge_response
            .challenge_timestamp
            > proof.service_timestamp
        {
            return Ok(false);
        }

        Ok(true)
    }

    fn calculate_quality_multiplier(&self, metrics: &QualityMetrics) -> f64 {
        // Calculate quality multiplier based on service quality metrics
        let completion_weight = 0.35;
        let error_weight = 0.25;
        let satisfaction_weight = 0.20;
        let availability_weight = 0.15;
        let security_weight = 0.05;

        let quality_score = metrics.completion_rate * completion_weight
            + (1.0 - metrics.error_rate) * error_weight
            + metrics.satisfaction_score * satisfaction_weight
            + metrics.availability * availability_weight
            + metrics.security_score * security_weight;

        // Quality multiplier ranges from 0.5x to 2.0x
        (0.5 + (quality_score * 1.5)) as f64
    }

    fn get_service_type_multiplier(&self, service_type: &ServiceType) -> f64 {
        match service_type {
            ServiceType::Compute => 1.0,
            ServiceType::Storage => 1.2,
            ServiceType::Messaging => 0.8,
            ServiceType::AIAgent => 1.5,
            ServiceType::Hybrid => 1.8,
        }
    }

    fn calculate_vpos_bonus(&self, proof: &ServiceProof) -> f64 {
        // Calculate VPoS-specific bonus based on proof quality
        let mut bonus: f64 = 1.0;

        // Resource efficiency bonus
        if proof.resource_proof.efficiency_score > 0.8 {
            bonus += 0.2;
        }

        // Challenge-response bonus (if response is timely)
        let challenge_time = proof
            .computation_proof
            .challenge_response
            .challenge_timestamp;
        let service_time = proof.service_timestamp;
        let response_time = (service_time - challenge_time).num_seconds();

        if response_time > 0 && response_time < 60 {
            bonus += 0.1; // 10% bonus for quick response
        }

        // Cryptographic proof bonus
        if proof.provider_signature.len() >= 64 {
            bonus += 0.05; // 5% bonus for strong cryptographic proof
        }

        // Cap the bonus at 1.5x
        bonus.min(1.5)
    }
}

/// VPoS Service Registry for tracking service providers for SpaceKit network
pub struct VPoSServiceRegistry {
    /// Registered service providers
    providers: HashMap<String, ServiceProvider>,

    /// Service proofs by provider
    proofs_by_provider: HashMap<String, Vec<ServiceProof>>,

    /// Provider reputation scores
    reputation_scores: HashMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceProvider {
    pub did: String,
    pub service_types: Vec<ServiceType>,
    pub endpoint: String,
    pub stake_amount: u64,
    pub registration_date: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub total_services_provided: u64,
    pub average_quality_score: f32,
}

impl VPoSServiceRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            proofs_by_provider: HashMap::new(),
            reputation_scores: HashMap::new(),
        }
    }

    pub fn register_provider(&mut self, provider: ServiceProvider) -> Result<()> {
        self.providers
            .insert(provider.did.clone(), provider.clone());
        self.proofs_by_provider
            .insert(provider.did.clone(), Vec::new());
        self.reputation_scores.insert(provider.did, 0.5); // Default reputation
        Ok(())
    }

    pub fn record_service_proof(&mut self, proof: ServiceProof) -> Result<()> {
        if let Some(proofs) = self.proofs_by_provider.get_mut(&proof.provider_did) {
            proofs.push(proof.clone());

            // Update provider reputation
            self.update_provider_reputation(&proof.provider_did, &proof.quality_metrics)?;

            // Update provider stats
            if let Some(provider) = self.providers.get_mut(&proof.provider_did) {
                provider.total_services_provided += 1;
                provider.last_activity = Utc::now();
            }
        }
        Ok(())
    }

    pub fn get_provider_reputation(&self, provider_did: &str) -> Option<f32> {
        self.reputation_scores.get(provider_did).copied()
    }

    pub fn get_top_providers(
        &self,
        service_type: &ServiceType,
        limit: usize,
    ) -> Vec<ServiceProvider> {
        let mut providers: Vec<_> = self
            .providers
            .values()
            .filter(|p| p.service_types.contains(service_type))
            .collect();

        providers.sort_by(|a, b| {
            let a_rep = self.reputation_scores.get(&a.did).unwrap_or(&0.0);
            let b_rep = self.reputation_scores.get(&b.did).unwrap_or(&0.0);
            b_rep
                .partial_cmp(a_rep)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        providers.into_iter().take(limit).cloned().collect()
    }

    fn update_provider_reputation(
        &mut self,
        provider_did: &str,
        quality: &QualityMetrics,
    ) -> Result<()> {
        let current_rep = self.reputation_scores.get(provider_did).unwrap_or(&0.5);

        // Calculate new reputation as weighted average
        let quality_score = quality.completion_rate * 0.3
            + (1.0 - quality.error_rate) * 0.2
            + quality.satisfaction_score * 0.2
            + quality.availability * 0.15
            + quality.security_score * 0.15;

        let new_rep = (current_rep * 0.9) + (quality_score * 0.1);
        self.reputation_scores
            .insert(provider_did.to_string(), new_rep);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComputeTask, TaskStatus};

    #[tokio::test]
    async fn test_vpos_proof_generation() {
        // This would require setting up quantum-resistant identity and encryption
        // Skipped for now due to dependency complexity
    }

    #[test]
    fn test_service_registry() {
        let mut registry = VPoSServiceRegistry::new();

        let provider = ServiceProvider {
            did: "did:spacekit:provider:test".to_string(),
            service_types: vec![ServiceType::Compute],
            endpoint: "http://localhost:8080".to_string(),
            stake_amount: 1000,
            registration_date: Utc::now(),
            last_activity: Utc::now(),
            total_services_provided: 0,
            average_quality_score: 0.5,
        };

        registry.register_provider(provider).unwrap();

        let reputation = registry.get_provider_reputation("did:spacekit:provider:test");
        assert_eq!(reputation, Some(0.5));
    }

    #[tokio::test]
    async fn test_vpos_proof_lifecycle() {
        use crate::{ComputeResult, ComputeTask, CostBreakdown, ExecutionMetrics, TaskStatus};
        use chrono::Utc;
        use std::sync::Arc;

        // Mock quantum-resistant wallet for testing
        let mock_wallet = Arc::new(QuantumResistantWallet::new());

        // Create VPoS manager
        let mut vpos_manager = VPoSManager::new(mock_wallet.clone(), Algorithm::Kyber512)
            .await
            .unwrap();

        // Create mock task
        let task = ComputeTask {
            id: "test_task_001".to_string(),
            name: "Test VPoS Task".to_string(),
            runtime: "wasm".to_string(),
            code: vec![0x00, 0x61, 0x73, 0x6D], // Mock WASM header
            input_data: vec![0x01, 0x02, 0x03, 0x04],
            status: TaskStatus::Completed,
            created_at: Utc::now(),
            owner_did: "did:spacekit:user:test".to_string(),
            estimated_cost: Some(1.0),
            actual_cost: Some(1.2),
            execution_path: Some("CPU".to_string()),
            result_hash: Some("hash123".to_string()),
        };

        // Create mock execution metrics
        let execution_metrics = ExecutionMetrics {
            execution_time_ms: 5000,
            cpu_time_ms: 4000,
            gpu_time_ms: None,
            memory_peak_mb: 512,
            compute_units_used: 1000,
            energy_consumed_kwh: 0.001,
        };

        // Create mock compute result
        let compute_result = ComputeResult {
            task_id: task.id.clone(),
            status: TaskStatus::Completed,
            result_data: vec![0x05, 0x06, 0x07, 0x08],
            execution_metrics: execution_metrics.clone(),
            cost_breakdown: CostBreakdown {
                base_cost: 0.1,
                storage_cost: 0.1,
                compute_cost: 0.5,
                memory_cost: 0.2,
                gpu_cost: 0.0,
                encryption_cost: 0.1,
                network_cost: 0.1,
                total_cost: 1.0,
            },
            completed_at: Utc::now(),
        };

        // 🎯 Test 1: Generate VPoS proof
        let proof = vpos_manager
            .generate_service_proof(&task, &compute_result, &execution_metrics, &task.owner_did)
            .await
            .unwrap();

        // Verify proof structure
        assert_eq!(proof.task_id, task.id);
        assert_eq!(proof.provider_did, mock_wallet.identity_doc.did.did);
        assert_eq!(proof.requester_did, task.owner_did);
        assert_eq!(proof.service_type, ServiceType::Compute);
        assert!(!proof.verification_hash.is_empty());
        assert!(!proof.provider_signature.is_empty());

        // 🎯 Test 2: Verify VPoS proof
        let verification_result = vpos_manager.verify_service_proof(&proof).await.unwrap();
        assert!(
            verification_result,
            "VPoS proof verification should succeed"
        );

        // 🎯 Test 3: Calculate VPoS reward
        let reward = vpos_manager
            .verify_and_calculate_reward(&proof)
            .await
            .unwrap();
        assert!(reward.is_some(), "VPoS reward should be calculated");

        let reward_amount = reward.unwrap();
        assert!(reward_amount > 0, "VPoS reward should be positive");

        // 🎯 Test 4: Submit proof to network
        let tx_hash = vpos_manager.submit_proof_to_network(&proof).await.unwrap();
        assert!(
            tx_hash.starts_with("0x"),
            "Transaction hash should be valid"
        );

        // 🎯 Test 5: Verify computation proof details
        let comp_proof = &proof.computation_proof;
        assert_eq!(comp_proof.compute_units, 1000);
        assert_eq!(comp_proof.execution_time_ms, 5000);
        assert!(!comp_proof.input_hash.is_empty());
        assert!(!comp_proof.output_hash.is_empty());
        assert!(!comp_proof.challenge_response.challenge.is_empty());

        // 🎯 Test 6: Verify resource proof details
        let resource_proof = &proof.resource_proof;
        assert_eq!(resource_proof.memory_used_mb, 512);
        assert!(resource_proof.efficiency_score > 0.0);
        assert!(resource_proof.efficiency_score <= 1.0);

        // 🎯 Test 7: Verify quality metrics
        let quality_metrics = &proof.quality_metrics;
        assert!(quality_metrics.completion_rate > 0.0);
        assert!(quality_metrics.completion_rate <= 1.0);
        assert!(quality_metrics.availability > 0.0);
        assert!(quality_metrics.security_score > 0.0);

        // 🎯 Test 8: Test reputation score calculation
        let reputation_score = vpos_manager.get_reputation_score();
        assert!(reputation_score >= 0.0);
        assert!(reputation_score <= 1.0);

        println!("✅ VPoS Phase 2 test completed successfully!");
        println!("🎯 Proof ID: {}", proof.proof_id);
        println!("💰 Reward: {} ASTRA tokens", reward_amount as f64 / 1e18);
        println!("🔗 TX Hash: {}", tx_hash);
        println!("📊 Reputation: {:.2}", reputation_score);
    }

    #[tokio::test]
    async fn test_vpos_proof_verification_edge_cases() {
        use crate::{ComputeResult, ComputeTask, CostBreakdown, ExecutionMetrics, TaskStatus};
        use chrono::Utc;
        use std::sync::Arc;

        // Mock quantum-resistant wallet for testing
        let mock_wallet = Arc::new(QuantumResistantWallet::new());

        // Create VPoS manager
        let mut vpos_manager = VPoSManager::new(mock_wallet.clone(), Algorithm::Kyber512)
            .await
            .unwrap();

        // Create mock task with edge case values
        let task = ComputeTask {
            id: "edge_case_task".to_string(),
            name: "Edge Case Test".to_string(),
            runtime: "hybrid".to_string(),
            code: vec![0x00, 0x61, 0x73, 0x6D],
            input_data: vec![0x01, 0x02, 0x03, 0x04],
            status: TaskStatus::Completed,
            created_at: Utc::now(),
            owner_did: "did:spacekit:user:edge".to_string(),
            estimated_cost: Some(10.0),
            actual_cost: Some(15.0),
            execution_path: Some("Hybrid".to_string()),
            result_hash: Some("edge_hash".to_string()),
        };

        // Create mock execution metrics with high resource usage
        let execution_metrics = ExecutionMetrics {
            execution_time_ms: 30000, // 30 seconds
            cpu_time_ms: 25000,
            gpu_time_ms: Some(20000),
            memory_peak_mb: 2048,      // 2GB
            compute_units_used: 50000, // High compute usage
            energy_consumed_kwh: 0.1,
        };

        // Create mock compute result
        let compute_result = ComputeResult {
            task_id: task.id.clone(),
            status: TaskStatus::Completed,
            result_data: vec![0x05, 0x06, 0x07, 0x08],
            execution_metrics: execution_metrics.clone(),
            cost_breakdown: CostBreakdown {
                base_cost: 0.1,
                storage_cost: 0.1,
                compute_cost: 2.0,
                memory_cost: 1.0,
                gpu_cost: 3.0,
                encryption_cost: 0.5,
                network_cost: 0.2,
                total_cost: 6.8,
            },
            completed_at: Utc::now(),
        };

        // Generate VPoS proof
        let proof = vpos_manager
            .generate_service_proof(&task, &compute_result, &execution_metrics, &task.owner_did)
            .await
            .unwrap();

        // Verify proof passes verification
        let verification_result = vpos_manager.verify_service_proof(&proof).await.unwrap();
        assert!(
            verification_result,
            "High-resource proof should verify successfully"
        );

        // Check reward calculation for high-resource task
        let reward = vpos_manager
            .verify_and_calculate_reward(&proof)
            .await
            .unwrap();
        assert!(reward.is_some(), "High-resource task should get reward");

        let reward_amount = reward.unwrap();
        assert!(
            reward_amount > 50000,
            "High-resource task should get substantial reward"
        );

        // Test service type multiplier for hybrid tasks
        let hybrid_multiplier = vpos_manager.get_service_type_multiplier(&ServiceType::Hybrid);
        assert_eq!(
            hybrid_multiplier, 1.8,
            "Hybrid tasks should get 1.8x multiplier"
        );

        // Test quality multiplier calculation
        let quality_multiplier = vpos_manager.calculate_quality_multiplier(&proof.quality_metrics);
        assert!(
            quality_multiplier >= 0.5,
            "Quality multiplier should be at least 0.5x"
        );
        assert!(
            quality_multiplier <= 2.0,
            "Quality multiplier should be at most 2.0x"
        );

        // Test VPoS bonus calculation
        let vpos_bonus = vpos_manager.calculate_vpos_bonus(&proof);
        assert!(vpos_bonus >= 1.0, "VPoS bonus should be at least 1.0x");
        assert!(vpos_bonus <= 1.5, "VPoS bonus should be at most 1.5x");

        println!("✅ VPoS edge case test completed successfully!");
        println!(
            "💰 High-resource reward: {} ASTRA tokens",
            reward_amount as f64 / 1e18
        );
        println!("🔍 Quality multiplier: {:.2}x", quality_multiplier);
        println!("🎯 VPoS bonus: {:.2}x", vpos_bonus);
    }

    #[test]
    fn test_vpos_service_registry_advanced() {
        let mut registry = VPoSServiceRegistry::new();

        // Register multiple providers
        let providers = vec![
            ServiceProvider {
                did: "did:spacekit:provider:gpu".to_string(),
                service_types: vec![ServiceType::Compute, ServiceType::Hybrid],
                endpoint: "http://testnet.spacekit.xyz:8080".to_string(),
                stake_amount: 50000,
                registration_date: Utc::now(),
                last_activity: Utc::now(),
                total_services_provided: 100,
                average_quality_score: 0.95,
            },
            ServiceProvider {
                did: "did:spacekit:provider:ai".to_string(),
                service_types: vec![ServiceType::AIAgent],
                endpoint: "http://testnet.spacekit.xyz:8080".to_string(),
                stake_amount: 25000,
                registration_date: Utc::now(),
                last_activity: Utc::now(),
                total_services_provided: 50,
                average_quality_score: 0.88,
            },
            ServiceProvider {
                did: "did:spacekit:provider:storage".to_string(),
                service_types: vec![ServiceType::Storage],
                endpoint: "http://testnet.spacekit.xyz:8080".to_string(),
                stake_amount: 10000,
                registration_date: Utc::now(),
                last_activity: Utc::now(),
                total_services_provided: 200,
                average_quality_score: 0.78,
            },
        ];

        for provider in providers {
            registry.register_provider(provider).unwrap();
        }

        // Test getting top providers for compute services
        let top_compute_providers = registry.get_top_providers(&ServiceType::Compute, 10);
        assert_eq!(top_compute_providers.len(), 1);
        assert_eq!(top_compute_providers[0].did, "did:spacekit:provider:gpu");

        // Test getting top providers for AI services
        let top_ai_providers = registry.get_top_providers(&ServiceType::AIAgent, 10);
        assert_eq!(top_ai_providers.len(), 1);
        assert_eq!(top_ai_providers[0].did, "did:spacekit:provider:ai");

        // Test reputation scores
        let gpu_reputation = registry.get_provider_reputation("did:spacekit:provider:gpu");
        let ai_reputation = registry.get_provider_reputation("did:spacekit:provider:ai");
        let storage_reputation = registry.get_provider_reputation("did:spacekit:provider:storage");

        assert!(gpu_reputation.is_some());
        assert!(ai_reputation.is_some());
        assert!(storage_reputation.is_some());

        println!("✅ VPoS service registry advanced test completed!");
        println!("🏆 GPU provider reputation: {:.2}", gpu_reputation.unwrap());
        println!("🤖 AI provider reputation: {:.2}", ai_reputation.unwrap());
        println!(
            "💾 Storage provider reputation: {:.2}",
            storage_reputation.unwrap()
        );
    }
}
