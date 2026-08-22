//! Subnet Proof System
//!
//! Enables operators to run their own networks (subnets) and submit periodic proofs
//! to the mainnet for validation. Supports both public and private networks with
//! ZK-proof aggregation.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::{secure_multiparty::ZKProofType, vpos::VPoSManager};

/// Network type configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NetworkType {
    /// Public network - anyone can join and validate
    Public,
    /// Private network - only authorized participants
    Private {
        /// List of authorized validator DIDs
        authorized_validators: Vec<String>,
        /// Whether to publish aggregated proofs publicly
        publish_proofs: bool,
    },
    /// Consortium network - semi-private with known validators
    Consortium {
        /// Consortium members
        members: Vec<String>,
        /// Minimum approval threshold
        approval_threshold: f64,
    },
}

/// Network registration on mainnet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetRegistration {
    /// Unique subnet identifier
    pub subnet_id: String,
    /// Subnet operator DID
    pub operator_did: String,
    /// Network type and privacy settings
    pub network_type: NetworkType,
    /// Genesis block hash
    pub genesis_hash: String,
    /// Chain ID for this subnet
    pub chain_id: u64,
    /// Minimum stake required for validators
    pub min_validator_stake: u128,
    /// Proof submission interval (seconds)
    pub proof_submission_interval: u64,
    /// Registration timestamp
    pub registered_at: DateTime<Utc>,
    /// Current status
    pub status: SubnetStatus,
    /// Total value locked in subnet
    pub total_value_locked: u128,
}

/// Subnet operational status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubnetStatus {
    /// Pending approval from mainnet
    Pending,
    /// Active and operational
    Active,
    /// Paused by operator
    Paused,
    /// Suspended by mainnet governance
    Suspended,
    /// Deregistered
    Deregistered,
}

/// Aggregated proof from a subnet to mainnet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubnetProof {
    /// Unique proof identifier
    pub proof_id: String,
    /// Subnet that generated this proof
    pub subnet_id: String,
    /// Block range covered by this proof
    pub block_range: (u64, u64),
    /// Root hash of the block range
    pub state_root: String,
    /// Aggregated transaction count
    pub transaction_count: u64,
    /// Aggregated gas used
    pub total_gas_used: u64,
    /// List of validator signatures
    pub validator_signatures: Vec<ValidatorSignature>,
    /// ZK proof of correct execution
    pub zk_proof: ZKProofData,
    /// Individual service proofs aggregated
    pub aggregated_service_proofs: Vec<String>, // Proof IDs
    /// Proof generation timestamp
    pub generated_at: DateTime<Utc>,
    /// Merkle root of all transactions in range
    pub transaction_merkle_root: String,
}

/// Validator signature on subnet proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    /// Validator DID
    pub validator_did: String,
    /// Quantum-resistant signature
    pub signature: Vec<u8>,
    /// Stake amount at time of signing
    pub stake_amount: u128,
    /// Signature timestamp
    pub signed_at: DateTime<Utc>,
}

/// Zero-knowledge proof data for subnet state transition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZKProofData {
    /// Proof type used
    pub proof_type: ZKProofType,
    /// Compressed proof bytes
    pub proof_bytes: Vec<u8>,
    /// Public inputs to the proof
    pub public_inputs: Vec<String>,
    /// Verification key hash
    pub verification_key_hash: String,
    /// Proof generation time (ms)
    pub generation_time_ms: u64,
}

/// Mainnet verification result for subnet proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofVerificationResult {
    /// Whether proof is valid
    pub is_valid: bool,
    /// Verification timestamp
    pub verified_at: DateTime<Utc>,
    /// Mainnet validator DIDs that verified
    pub verifying_validators: Vec<String>,
    /// Verification confidence score (0.0 - 1.0)
    pub confidence_score: f64,
    /// Any issues detected
    pub issues: Vec<String>,
    /// Mainnet block number where proof was accepted
    pub mainnet_block_number: Option<u64>,
}

/// Mainnet manager for subnet proof verification
pub struct SubnetProofSystem {
    /// Registered subnets
    subnets: Arc<RwLock<HashMap<String, SubnetRegistration>>>,
    /// Pending proofs awaiting verification
    pending_proofs: Arc<RwLock<HashMap<String, SubnetProof>>>,
    /// Verified proofs
    verified_proofs: Arc<RwLock<HashMap<String, (SubnetProof, ProofVerificationResult)>>>,
    /// VPoS manager for service proof verification
    vpos_manager: Arc<VPoSManager>,
    /// Mainnet configuration
    config: SubnetProofConfig,
}

/// Configuration for subnet proof system
#[derive(Debug, Clone)]
pub struct SubnetProofConfig {
    /// Minimum number of validator signatures required
    pub min_validator_signatures: usize,
    /// Minimum stake percentage required for valid proof
    pub min_stake_percentage: f64,
    /// Maximum time between proofs before subnet is suspended (seconds)
    pub max_proof_interval: u64,
    /// Whether to enable ZK proof verification (can be disabled for testing)
    pub enable_zk_verification: bool,
    /// Mainnet DID
    pub mainnet_did: String,
}

impl Default for SubnetProofConfig {
    fn default() -> Self {
        Self {
            min_validator_signatures: 3,
            min_stake_percentage: 0.67, // 67% of stake must sign
            max_proof_interval: 3600,   // 1 hour
            enable_zk_verification: true,
            mainnet_did: "did:spacekit:mainnet".to_string(),
        }
    }
}

impl SubnetProofSystem {
    /// Create a new subnet proof system
    pub fn new(vpos_manager: Arc<VPoSManager>, config: SubnetProofConfig) -> Self {
        Self {
            subnets: Arc::new(RwLock::new(HashMap::new())),
            pending_proofs: Arc::new(RwLock::new(HashMap::new())),
            verified_proofs: Arc::new(RwLock::new(HashMap::new())),
            vpos_manager,
            config,
        }
    }

    /// Register a new subnet on mainnet
    pub async fn register_subnet(
        &self,
        operator_did: String,
        network_type: NetworkType,
        genesis_hash: String,
        min_validator_stake: u128,
        proof_submission_interval: u64,
    ) -> Result<SubnetRegistration> {
        let subnet_id = format!("subnet_{}", Uuid::new_v4());
        let chain_id = self.generate_chain_id().await;

        let registration = SubnetRegistration {
            subnet_id: subnet_id.clone(),
            operator_did: operator_did.clone(),
            network_type: network_type.clone(),
            genesis_hash,
            chain_id,
            min_validator_stake,
            proof_submission_interval,
            registered_at: Utc::now(),
            status: SubnetStatus::Pending,
            total_value_locked: 0,
        };

        let mut subnets = self.subnets.write().await;
        subnets.insert(subnet_id.clone(), registration.clone());

        info!("🌐 Subnet registered: {} by {}", subnet_id, operator_did);
        info!("   Network Type: {:?}", network_type);
        info!("   Chain ID: {}", chain_id);

        Ok(registration)
    }

    /// Activate a subnet (after governance approval)
    pub async fn activate_subnet(&self, subnet_id: &str) -> Result<()> {
        let mut subnets = self.subnets.write().await;
        if let Some(subnet) = subnets.get_mut(subnet_id) {
            subnet.status = SubnetStatus::Active;
            info!("✅ Subnet activated: {}", subnet_id);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Subnet not found: {}", subnet_id))
        }
    }

    /// Submit a proof from subnet to mainnet
    pub async fn submit_subnet_proof(&self, proof: SubnetProof) -> Result<String> {
        // Validate subnet exists and is active
        let subnets = self.subnets.read().await;
        let subnet = subnets
            .get(&proof.subnet_id)
            .ok_or_else(|| anyhow::anyhow!("Subnet not found: {}", proof.subnet_id))?;

        if subnet.status != SubnetStatus::Active {
            return Err(anyhow::anyhow!("Subnet is not active: {:?}", subnet.status));
        }

        // Basic validation
        self.validate_proof_structure(&proof, subnet).await?;

        // Add to pending proofs
        let proof_id = proof.proof_id.clone();
        let mut pending = self.pending_proofs.write().await;
        pending.insert(proof_id.clone(), proof.clone());

        info!(
            "📨 Subnet proof submitted: {} from subnet {}",
            proof_id, proof.subnet_id
        );
        info!(
            "   Block range: {} - {}",
            proof.block_range.0, proof.block_range.1
        );
        info!("   Transactions: {}", proof.transaction_count);
        info!(
            "   Validator signatures: {}",
            proof.validator_signatures.len()
        );

        Ok(proof_id)
    }

    /// Verify a subnet proof on mainnet
    pub async fn verify_subnet_proof(
        &self,
        proof_id: &str,
        verifying_validator_did: &str,
    ) -> Result<ProofVerificationResult> {
        // Get proof from pending
        let pending = self.pending_proofs.read().await;
        let proof = pending
            .get(proof_id)
            .ok_or_else(|| anyhow::anyhow!("Proof not found: {}", proof_id))?
            .clone();

        // Get subnet info
        let subnets = self.subnets.read().await;
        let subnet = subnets
            .get(&proof.subnet_id)
            .ok_or_else(|| anyhow::anyhow!("Subnet not found: {}", proof.subnet_id))?;

        info!(
            "🔍 Verifying subnet proof: {} from {}",
            proof_id, proof.subnet_id
        );

        let mut issues = Vec::new();
        let mut confidence_score = 1.0;

        // 1. Verify validator signatures
        let sig_result = self.verify_validator_signatures(&proof, subnet).await;
        if let Err(e) = sig_result {
            issues.push(format!("Signature verification failed: {}", e));
            confidence_score *= 0.5;
        }

        // 2. Verify stake threshold
        let stake_result = self.verify_stake_threshold(&proof, subnet).await;
        if let Err(e) = stake_result {
            issues.push(format!("Stake threshold not met: {}", e));
            confidence_score *= 0.6;
        }

        // 3. Verify ZK proof
        if self.config.enable_zk_verification {
            let zk_result = self.verify_zk_proof(&proof.zk_proof).await;
            if let Err(e) = zk_result {
                issues.push(format!("ZK proof verification failed: {}", e));
                confidence_score *= 0.3;
            }
        }

        // 4. Verify aggregated service proofs
        let service_proof_result = self.verify_aggregated_service_proofs(&proof).await;
        if let Err(e) = service_proof_result {
            issues.push(format!("Service proof verification failed: {}", e));
            confidence_score *= 0.7;
        }

        // 5. Verify merkle roots
        let merkle_result = self.verify_merkle_roots(&proof).await;
        if let Err(e) = merkle_result {
            issues.push(format!("Merkle root verification failed: {}", e));
            confidence_score *= 0.4;
        }

        let is_valid = confidence_score >= 0.8 && issues.is_empty();

        let result = ProofVerificationResult {
            is_valid,
            verified_at: Utc::now(),
            verifying_validators: vec![verifying_validator_did.to_string()],
            confidence_score,
            issues,
            mainnet_block_number: if is_valid {
                Some(self.get_current_mainnet_block().await)
            } else {
                None
            },
        };

        if is_valid {
            // Move to verified proofs
            drop(pending);
            let mut pending_write = self.pending_proofs.write().await;
            if let Some(verified_proof) = pending_write.remove(proof_id) {
                let mut verified = self.verified_proofs.write().await;
                verified.insert(proof_id.to_string(), (verified_proof, result.clone()));
            }

            info!("✅ Subnet proof verified: {}", proof_id);
            info!("   Confidence: {:.2}%", confidence_score * 100.0);
        } else {
            warn!("❌ Subnet proof verification failed: {}", proof_id);
            warn!("   Issues: {:?}", result.issues);
        }

        Ok(result)
    }

    /// Get subnet registration info
    pub async fn get_subnet(&self, subnet_id: &str) -> Option<SubnetRegistration> {
        let subnets = self.subnets.read().await;
        subnets.get(subnet_id).cloned()
    }

    /// List all registered subnets
    pub async fn list_subnets(&self) -> Vec<SubnetRegistration> {
        let subnets = self.subnets.read().await;
        subnets.values().cloned().collect()
    }

    /// Get verification status for a proof
    pub async fn get_proof_status(&self, proof_id: &str) -> Option<ProofVerificationResult> {
        let verified = self.verified_proofs.read().await;
        verified.get(proof_id).map(|(_, result)| result.clone())
    }

    // Private helper methods

    async fn generate_chain_id(&self) -> u64 {
        // Generate unique chain ID based on current subnet count
        let subnets = self.subnets.read().await;
        1000 + subnets.len() as u64
    }

    async fn validate_proof_structure(
        &self,
        proof: &SubnetProof,
        subnet: &SubnetRegistration,
    ) -> Result<()> {
        // Validate block range
        if proof.block_range.1 <= proof.block_range.0 {
            return Err(anyhow::anyhow!("Invalid block range"));
        }

        // Validate minimum validator signatures
        if proof.validator_signatures.len() < self.config.min_validator_signatures {
            return Err(anyhow::anyhow!(
                "Insufficient validator signatures: {} < {}",
                proof.validator_signatures.len(),
                self.config.min_validator_signatures
            ));
        }

        // Validate network type permissions
        match &subnet.network_type {
            NetworkType::Private {
                authorized_validators,
                ..
            } => {
                for sig in &proof.validator_signatures {
                    if !authorized_validators.contains(&sig.validator_did) {
                        return Err(anyhow::anyhow!(
                            "Unauthorized validator: {}",
                            sig.validator_did
                        ));
                    }
                }
            }
            NetworkType::Consortium { members, .. } => {
                for sig in &proof.validator_signatures {
                    if !members.contains(&sig.validator_did) {
                        return Err(anyhow::anyhow!(
                            "Non-consortium validator: {}",
                            sig.validator_did
                        ));
                    }
                }
            }
            NetworkType::Public => {
                // No restrictions for public networks
            }
        }

        Ok(())
    }

    async fn verify_validator_signatures(
        &self,
        proof: &SubnetProof,
        _subnet: &SubnetRegistration,
    ) -> Result<()> {
        // In production, verify each quantum-resistant signature
        // For now, simplified validation
        for sig in &proof.validator_signatures {
            if sig.signature.is_empty() {
                return Err(anyhow::anyhow!(
                    "Empty signature from {}",
                    sig.validator_did
                ));
            }
            if sig.stake_amount == 0 {
                return Err(anyhow::anyhow!("Zero stake for {}", sig.validator_did));
            }
        }
        Ok(())
    }

    async fn verify_stake_threshold(
        &self,
        proof: &SubnetProof,
        subnet: &SubnetRegistration,
    ) -> Result<()> {
        let total_signing_stake: u128 = proof
            .validator_signatures
            .iter()
            .map(|sig| sig.stake_amount)
            .sum();

        let required_stake =
            (subnet.total_value_locked as f64 * self.config.min_stake_percentage) as u128;

        if total_signing_stake < required_stake {
            return Err(anyhow::anyhow!(
                "Insufficient stake: {} < {} required",
                total_signing_stake,
                required_stake
            ));
        }

        Ok(())
    }

    async fn verify_zk_proof(&self, zk_proof: &ZKProofData) -> Result<()> {
        // In production, verify the actual ZK proof
        // For now, basic validation
        if zk_proof.proof_bytes.is_empty() {
            return Err(anyhow::anyhow!("Empty ZK proof"));
        }

        if zk_proof.public_inputs.is_empty() {
            return Err(anyhow::anyhow!("No public inputs"));
        }

        debug!("ZK proof verified: {} bytes", zk_proof.proof_bytes.len());
        Ok(())
    }

    async fn verify_aggregated_service_proofs(&self, proof: &SubnetProof) -> Result<()> {
        // Verify that aggregated service proofs are valid
        // In production, cross-reference with VPoS records
        if proof.aggregated_service_proofs.is_empty() {
            return Err(anyhow::anyhow!("No service proofs aggregated"));
        }

        debug!(
            "Verified {} aggregated service proofs",
            proof.aggregated_service_proofs.len()
        );
        Ok(())
    }

    async fn verify_merkle_roots(&self, proof: &SubnetProof) -> Result<()> {
        // Verify merkle root consistency
        if proof.transaction_merkle_root.is_empty() {
            return Err(anyhow::anyhow!("Empty transaction merkle root"));
        }

        if proof.state_root.is_empty() {
            return Err(anyhow::anyhow!("Empty state root"));
        }

        Ok(())
    }

    async fn get_current_mainnet_block(&self) -> u64 {
        // In production, get actual mainnet block number
        // For now, return mock value
        12345
    }
}

/// Builder for creating subnet proofs
pub struct SubnetProofBuilder {
    subnet_id: String,
    block_range: (u64, u64),
    transactions: Vec<Vec<u8>>,
    validator_signatures: Vec<ValidatorSignature>,
    service_proofs: Vec<String>,
}

impl SubnetProofBuilder {
    pub fn new(subnet_id: String, block_range: (u64, u64)) -> Self {
        Self {
            subnet_id,
            block_range,
            transactions: Vec::new(),
            validator_signatures: Vec::new(),
            service_proofs: Vec::new(),
        }
    }

    pub fn add_transaction(&mut self, tx: Vec<u8>) -> &mut Self {
        self.transactions.push(tx);
        self
    }

    pub fn add_validator_signature(&mut self, signature: ValidatorSignature) -> &mut Self {
        self.validator_signatures.push(signature);
        self
    }

    pub fn add_service_proof(&mut self, proof_id: String) -> &mut Self {
        self.service_proofs.push(proof_id);
        self
    }

    pub async fn build(&self) -> Result<SubnetProof> {
        // Calculate merkle root of transactions
        let transaction_merkle_root = self.calculate_merkle_root(&self.transactions);

        // Calculate state root
        let state_root = self.calculate_state_root();

        // Generate ZK proof
        let zk_proof = self.generate_zk_proof().await?;

        Ok(SubnetProof {
            proof_id: format!("proof_{}", Uuid::new_v4()),
            subnet_id: self.subnet_id.clone(),
            block_range: self.block_range,
            state_root,
            transaction_count: self.transactions.len() as u64,
            total_gas_used: self.calculate_total_gas(),
            validator_signatures: self.validator_signatures.clone(),
            zk_proof,
            aggregated_service_proofs: self.service_proofs.clone(),
            generated_at: Utc::now(),
            transaction_merkle_root,
        })
    }

    fn calculate_merkle_root(&self, transactions: &[Vec<u8>]) -> String {
        if transactions.is_empty() {
            return String::from(
                "0x0000000000000000000000000000000000000000000000000000000000000000",
            );
        }

        // Simple merkle root calculation
        let mut hasher = Sha3_256::new();
        for tx in transactions {
            hasher.update(tx);
        }
        format!("0x{:x}", hasher.finalize())
    }

    fn calculate_state_root(&self) -> String {
        // Simplified state root calculation
        let mut hasher = Sha3_256::new();
        hasher.update(self.subnet_id.as_bytes());
        hasher.update(&self.block_range.0.to_le_bytes());
        hasher.update(&self.block_range.1.to_le_bytes());
        format!("0x{:x}", hasher.finalize())
    }

    fn calculate_total_gas(&self) -> u64 {
        // Simplified gas calculation
        self.transactions.len() as u64 * 21000
    }

    async fn generate_zk_proof(&self) -> Result<ZKProofData> {
        // Generate ZK proof for state transition
        // In production, use actual ZK proof library
        let proof_bytes = vec![0u8; 256]; // Placeholder

        let public_inputs = vec![
            self.block_range.0.to_string(),
            self.block_range.1.to_string(),
            self.transaction_count().to_string(),
        ];

        Ok(ZKProofData {
            proof_type: ZKProofType::RangeProof,
            proof_bytes,
            public_inputs,
            verification_key_hash: "0xabcd1234".to_string(),
            generation_time_ms: 100,
        })
    }

    fn transaction_count(&self) -> u64 {
        self.transactions.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantum_security::QuantumResistantWallet;
    use spacekit_primitives::v1::crypto::quantum::Algorithm;

    #[tokio::test]
    async fn test_subnet_registration() {
        // Create wallet for VPoS manager
        let wallet = Arc::new(QuantumResistantWallet::new());

        let vpos_manager = Arc::new(VPoSManager::new(wallet, Algorithm::Kyber768).await.unwrap());
        let system = SubnetProofSystem::new(vpos_manager, SubnetProofConfig::default());

        let registration = system
            .register_subnet(
                "did:spacekit:operator1".to_string(),
                NetworkType::Public,
                "0x3E7B10080A1684A3EebC8D9947758a8b91146192".to_string(),
                1000,
                300,
            )
            .await
            .unwrap();

        assert_eq!(registration.status, SubnetStatus::Pending);
        assert!(registration.subnet_id.starts_with("subnet_"));
    }

    #[tokio::test]
    async fn test_subnet_proof_builder() {
        let mut builder = SubnetProofBuilder::new("subnet_test".to_string(), (100, 200));
        builder.add_transaction(vec![1, 2, 3]);
        builder.add_transaction(vec![4, 5, 6]);

        let proof = builder.build().await.unwrap();
        assert_eq!(proof.transaction_count, 2);
        assert_eq!(proof.block_range, (100, 200));
    }
}
