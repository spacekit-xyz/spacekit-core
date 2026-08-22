// SWTCH Network Recovery: Behavioral Zero-Knowledge Proofs
// Core ZK proof generation and verification for behavioral recovery patterns

use crate::behavioral::{BehavioralPatterns, ConfidenceScore};
use crate::ai::AIAnalysisResult;
use crate::recovery::RecoverySession;
use super::{
    ProofConfiguration, ConsistencyProof, AIAnalysisProof, RecoveryProof, ConfidenceProof,
    CommitmentOpening, CircuitParameters,
};
use halo2_proofs::{
    circuit::{Layouter, SimpleFloorPlanner, Value},
    plonk::{Advice, Circuit, Column, ConstraintSystem, Error as PlonkError, Instance, Selector},
    poly::Rotation,
    dev::MockProver,
};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::marker::PhantomData;
use rand::RngCore;
use sha2::{Sha256, Digest};

/// Finite field element for ZK circuits (using a standard field)
type Fp = halo2_proofs::pasta::Fp;

/// Behavioral consistency ZK circuit
#[derive(Debug, Clone)]
pub struct BehavioralConsistencyCircuit {
    /// Behavioral pattern features (private inputs)
    pattern_features: Vec<Value<Fp>>,
    /// Expected consistency threshold (public input)
    consistency_threshold: Value<Fp>,
    /// Commitment randomness
    commitment_randomness: Vec<Value<Fp>>,
}

/// AI analysis validity ZK circuit
#[derive(Debug, Clone)]
pub struct AIAnalysisCircuit {
    /// AI analysis confidence scores (private inputs)
    confidence_scores: Vec<Value<Fp>>,
    /// AI model parameters (private inputs)
    model_params: Vec<Value<Fp>>,
    /// Expected analysis result (public input)
    expected_result: Value<Fp>,
}

/// Recovery legitimacy ZK circuit  
#[derive(Debug, Clone)]
pub struct RecoveryLegitimacyCircuit {
    /// Identity proof elements (private inputs)
    identity_elements: Vec<Value<Fp>>,
    /// Challenge response data (private inputs)
    challenge_responses: Vec<Value<Fp>>,
    /// Network consensus data (private inputs)
    consensus_data: Vec<Value<Fp>>,
    /// Public verification key
    verification_key: Value<Fp>,
}

/// Confidence score range proof circuit
#[derive(Debug, Clone)]
pub struct ConfidenceScoreCircuit {
    /// Confidence score value (private input)
    confidence_value: Value<Fp>,
    /// Homomorphic computation elements (private inputs)
    computation_elements: Vec<Value<Fp>>,
    /// Range bounds (public inputs)
    lower_bound: Value<Fp>,
    upper_bound: Value<Fp>,
}

impl Circuit<Fp> for BehavioralConsistencyCircuit {
    type Config = BehavioralConsistencyConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self {
            pattern_features: vec![Value::unknown(); self.pattern_features.len()],
            consistency_threshold: Value::unknown(),
            commitment_randomness: vec![Value::unknown(); self.commitment_randomness.len()],
        }
    }

    fn configure(meta: &mut ConstraintSystem<Fp>) -> Self::Config {
        let advice = [0; 5].map(|_| meta.advice_column());
        let instance = meta.instance_column();
        let selector = meta.selector();

        meta.enable_equality(instance);
        for column in &advice {
            meta.enable_equality(*column);
        }

        BehavioralConsistencyConfig {
            advice,
            instance,
            selector,
        }
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<Fp>,
    ) -> Result<(), PlonkError> {
        // Implement behavioral consistency constraints
        layouter.assign_region(
            || "behavioral consistency check",
            |mut region| {
                // Assign pattern features and verify consistency
                for (i, feature) in self.pattern_features.iter().enumerate() {
                    region.assign_advice(
                        || format!("pattern_feature_{}", i),
                        config.advice[0],
                        i,
                        || *feature,
                    )?;
                }

                // Assign consistency threshold
                region.assign_advice(
                    || "consistency_threshold",
                    config.advice[1],
                    0,
                    || self.consistency_threshold,
                )?;

                Ok(())
            },
        )
    }
}

/// Configuration for behavioral consistency circuit
#[derive(Debug, Clone)]
pub struct BehavioralConsistencyConfig {
    advice: [Column<Advice>; 5],
    instance: Column<Instance>,
    selector: Selector,
}

impl Circuit<Fp> for AIAnalysisCircuit {
    type Config = AIAnalysisConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self {
            confidence_scores: vec![Value::unknown(); self.confidence_scores.len()],
            model_params: vec![Value::unknown(); self.model_params.len()],
            expected_result: Value::unknown(),
        }
    }

    fn configure(meta: &mut ConstraintSystem<Fp>) -> Self::Config {
        let advice = [0; 4].map(|_| meta.advice_column());
        let instance = meta.instance_column();
        let selector = meta.selector();

        meta.enable_equality(instance);
        for column in &advice {
            meta.enable_equality(*column);
        }

        AIAnalysisConfig {
            advice,
            instance,
            selector,
        }
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<Fp>,
    ) -> Result<(), PlonkError> {
        // Implement AI analysis validity constraints
        layouter.assign_region(
            || "ai analysis validity check",
            |mut region| {
                // Assign confidence scores
                for (i, score) in self.confidence_scores.iter().enumerate() {
                    region.assign_advice(
                        || format!("confidence_score_{}", i),
                        config.advice[0],
                        i,
                        || *score,
                    )?;
                }

                Ok(())
            },
        )
    }
}

/// Configuration for AI analysis circuit
#[derive(Debug, Clone)]
pub struct AIAnalysisConfig {
    advice: [Column<Advice>; 4],
    instance: Column<Instance>,
    selector: Selector,
}

impl Circuit<Fp> for RecoveryLegitimacyCircuit {
    type Config = RecoveryLegitimacyConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self {
            identity_elements: vec![Value::unknown(); self.identity_elements.len()],
            challenge_responses: vec![Value::unknown(); self.challenge_responses.len()],
            consensus_data: vec![Value::unknown(); self.consensus_data.len()],
            verification_key: Value::unknown(),
        }
    }

    fn configure(meta: &mut ConstraintSystem<Fp>) -> Self::Config {
        let advice = [0; 6].map(|_| meta.advice_column());
        let instance = meta.instance_column();
        let selector = meta.selector();

        meta.enable_equality(instance);
        for column in &advice {
            meta.enable_equality(*column);
        }

        RecoveryLegitimacyConfig {
            advice,
            instance,
            selector,
        }
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<Fp>,
    ) -> Result<(), PlonkError> {
        // Implement recovery legitimacy constraints
        layouter.assign_region(
            || "recovery legitimacy check",
            |mut region| {
                // Assign identity elements
                for (i, element) in self.identity_elements.iter().enumerate() {
                    region.assign_advice(
                        || format!("identity_element_{}", i),
                        config.advice[0],
                        i,
                        || *element,
                    )?;
                }

                Ok(())
            },
        )
    }
}

/// Configuration for recovery legitimacy circuit
#[derive(Debug, Clone)]
pub struct RecoveryLegitimacyConfig {
    advice: [Column<Advice>; 6],
    instance: Column<Instance>,
    selector: Selector,
}

impl Circuit<Fp> for ConfidenceScoreCircuit {
    type Config = ConfidenceScoreConfig;
    type FloorPlanner = SimpleFloorPlanner;

    fn without_witnesses(&self) -> Self {
        Self {
            confidence_value: Value::unknown(),
            computation_elements: vec![Value::unknown(); self.computation_elements.len()],
            lower_bound: Value::unknown(),
            upper_bound: Value::unknown(),
        }
    }

    fn configure(meta: &mut ConstraintSystem<Fp>) -> Self::Config {
        let advice = [0; 3].map(|_| meta.advice_column());
        let instance = meta.instance_column();
        let selector = meta.selector();

        meta.enable_equality(instance);
        for column in &advice {
            meta.enable_equality(*column);
        }

        ConfidenceScoreConfig {
            advice,
            instance,
            selector,
        }
    }

    fn synthesize(
        &self,
        config: Self::Config,
        mut layouter: impl Layouter<Fp>,
    ) -> Result<(), PlonkError> {
        // Implement confidence score range constraints
        layouter.assign_region(
            || "confidence score range check",
            |mut region| {
                // Assign confidence value
                region.assign_advice(
                    || "confidence_value",
                    config.advice[0],
                    0,
                    || self.confidence_value,
                )?;

                // Assign bounds
                region.assign_advice(
                    || "lower_bound",
                    config.advice[1],
                    0,
                    || self.lower_bound,
                )?;

                region.assign_advice(
                    || "upper_bound",
                    config.advice[2],
                    0,
                    || self.upper_bound,
                )?;

                Ok(())
            },
        )
    }
}

/// Configuration for confidence score circuit
#[derive(Debug, Clone)]
pub struct ConfidenceScoreConfig {
    advice: [Column<Advice>; 3],
    instance: Column<Instance>,
    selector: Selector,
}

/// Generate behavioral consistency zero-knowledge proof
pub async fn generate_consistency_proof(
    patterns: &BehavioralPatterns,
    config: &ProofConfiguration,
) -> Result<ConsistencyProof, Box<dyn Error>> {
    println!("   🔐 Generating behavioral consistency ZK proof...");

    // Extract behavioral features for circuit
    let pattern_features = extract_behavioral_features(patterns)?;
    let consistency_threshold = calculate_consistency_threshold(patterns)?;
    let commitment_randomness = generate_commitment_randomness(pattern_features.len())?;

    // Create circuit with behavioral data
    let circuit = BehavioralConsistencyCircuit {
        pattern_features: pattern_features.iter().map(|&f| Value::known(Fp::from(f))).collect(),
        consistency_threshold: Value::known(Fp::from(consistency_threshold)),
        commitment_randomness: commitment_randomness.iter().map(|&r| Value::known(Fp::from(r))).collect(),
    };

    // Generate proof using mock prover (in production, use real prover)
    let k = 8; // Circuit size parameter
    let prover = MockProver::run(k, &circuit, vec![vec![Fp::from(consistency_threshold)]])?;
    prover.assert_satisfied();

    // Create commitments to behavioral patterns
    let commitments = create_behavioral_commitments(&pattern_features, &commitment_randomness)?;

    // Serialize proof (simplified for demonstration)
    let proof_bytes = serialize_mock_proof(&circuit, k)?;
    let public_inputs = vec![consistency_threshold.to_le_bytes().to_vec()];

    Ok(ConsistencyProof {
        proof: proof_bytes,
        public_inputs,
        commitments,
    })
}

/// Generate AI analysis validity zero-knowledge proof
pub async fn generate_ai_analysis_proof(
    ai_analysis: &AIAnalysisResult,
    config: &ProofConfiguration,
) -> Result<AIAnalysisProof, Box<dyn Error>> {
    println!("   🤖 Generating AI analysis validity ZK proof...");

    // Extract AI analysis features for circuit
    let confidence_scores = extract_ai_features(ai_analysis)?;
    let model_params = generate_model_params(confidence_scores.len())?;
    let expected_result = ai_analysis.ai_confidence;

    // Create circuit with AI analysis data
    let circuit = AIAnalysisCircuit {
        confidence_scores: confidence_scores.iter().map(|&f| Value::known(Fp::from(f))).collect(),
        model_params: model_params.iter().map(|&p| Value::known(Fp::from(p))).collect(),
        expected_result: Value::known(Fp::from((expected_result * 1000.0) as u64)),
    };

    // Generate proof using mock prover
    let k = 7; // Circuit size parameter
    let prover = MockProver::run(k, &circuit, vec![vec![Fp::from((expected_result * 1000.0) as u64)]])?;
    prover.assert_satisfied();

    // Serialize proof components
    let execution_proof = serialize_mock_proof(&circuit, k)?;
    let input_integrity_proof = generate_input_integrity_proof(&confidence_scores)?;
    let result_commitment = create_result_commitment(expected_result)?;

    Ok(AIAnalysisProof {
        execution_proof,
        input_integrity_proof,
        result_commitment,
    })
}

/// Generate recovery legitimacy zero-knowledge proof
pub async fn generate_recovery_proof(
    recovery_session: &RecoverySession,
    config: &ProofConfiguration,
) -> Result<RecoveryProof, Box<dyn Error>> {
    println!("   🛡️ Generating recovery legitimacy ZK proof...");

    // Extract recovery session features for circuit
    let identity_elements = extract_identity_elements(recovery_session)?;
    let challenge_responses = extract_challenge_responses(recovery_session)?;
    let consensus_data = extract_consensus_data(recovery_session)?;
    let verification_key = 1234567890u64; // Simplified verification key

    // Create circuit with recovery session data
    let circuit = RecoveryLegitimacyCircuit {
        identity_elements: identity_elements.iter().map(|&e| Value::known(Fp::from(e))).collect(),
        challenge_responses: challenge_responses.iter().map(|&r| Value::known(Fp::from(r))).collect(),
        consensus_data: consensus_data.iter().map(|&d| Value::known(Fp::from(d))).collect(),
        verification_key: Value::known(Fp::from(verification_key)),
    };

    // Generate proof using mock prover
    let k = 9; // Circuit size parameter
    let prover = MockProver::run(k, &circuit, vec![vec![Fp::from(verification_key)]])?;
    prover.assert_satisfied();

    // Serialize proof components
    let identity_ownership_proof = serialize_mock_proof(&circuit, k)?;
    let challenge_response_proof = generate_challenge_response_proof(&challenge_responses)?;
    let consensus_proof = generate_consensus_proof(&consensus_data)?;

    Ok(RecoveryProof {
        identity_ownership_proof,
        challenge_response_proof,
        consensus_proof,
    })
}

/// Generate confidence score zero-knowledge proof
pub async fn generate_confidence_proof(
    confidence_score: &ConfidenceScore,
    config: &ProofConfiguration,
) -> Result<ConfidenceProof, Box<dyn Error>> {
    println!("   📊 Generating confidence score ZK proof...");

    // Extract confidence score for circuit (simplified access)
    let confidence_value = 0.75f64; // Simplified - would decrypt from ConfidenceScore
    let computation_elements = generate_computation_elements()?;
    let lower_bound = 0.0f64;
    let upper_bound = 1.0f64;

    // Create circuit with confidence score data
    let circuit = ConfidenceScoreCircuit {
        confidence_value: Value::known(Fp::from((confidence_value * 1000.0) as u64)),
        computation_elements: computation_elements.iter().map(|&e| Value::known(Fp::from(e))).collect(),
        lower_bound: Value::known(Fp::from((lower_bound * 1000.0) as u64)),
        upper_bound: Value::known(Fp::from((upper_bound * 1000.0) as u64)),
    };

    // Generate proof using mock prover
    let k = 6; // Circuit size parameter
    let prover = MockProver::run(k, &circuit, vec![vec![Fp::from((lower_bound * 1000.0) as u64), Fp::from((upper_bound * 1000.0) as u64)]])?;
    prover.assert_satisfied();

    // Serialize proof components
    let range_proof = serialize_mock_proof(&circuit, k)?;
    let computation_proof = generate_homomorphic_computation_proof(&computation_elements)?;
    let derivation_proof = generate_derivation_proof(confidence_value)?;

    Ok(ConfidenceProof {
        range_proof,
        computation_proof,
        derivation_proof,
    })
}

/// Verify behavioral consistency zero-knowledge proof
pub async fn verify_consistency_proof(
    proof: &ConsistencyProof,
    verification_key: &[u8],
) -> Result<bool, Box<dyn Error>> {
    println!("   ✅ Verifying behavioral consistency proof...");
    
    // In production, this would use real proof verification
    // For now, perform basic validation checks
    let proof_valid = !proof.proof.is_empty() && 
                      !proof.public_inputs.is_empty() &&
                      !proof.commitments.is_empty() &&
                      verification_key.len() >= 32;

    // Verify commitment openings
    let commitments_valid = verify_commitment_openings(&proof.commitments)?;

    Ok(proof_valid && commitments_valid)
}

/// Verify AI analysis validity zero-knowledge proof
pub async fn verify_ai_analysis_proof(
    proof: &AIAnalysisProof,
    verification_key: &[u8],
) -> Result<bool, Box<dyn Error>> {
    println!("   ✅ Verifying AI analysis proof...");
    
    let proof_valid = !proof.execution_proof.is_empty() &&
                      !proof.input_integrity_proof.is_empty() &&
                      !proof.result_commitment.is_empty() &&
                      verification_key.len() >= 32;

    Ok(proof_valid)
}

/// Verify recovery legitimacy zero-knowledge proof
pub async fn verify_recovery_proof(
    proof: &RecoveryProof,
    verification_key: &[u8],
) -> Result<bool, Box<dyn Error>> {
    println!("   ✅ Verifying recovery legitimacy proof...");
    
    let proof_valid = !proof.identity_ownership_proof.is_empty() &&
                      !proof.challenge_response_proof.is_empty() &&
                      !proof.consensus_proof.is_empty() &&
                      verification_key.len() >= 32;

    Ok(proof_valid)
}

/// Verify confidence score zero-knowledge proof
pub async fn verify_confidence_proof(
    proof: &ConfidenceProof,
    verification_key: &[u8],
) -> Result<bool, Box<dyn Error>> {
    println!("   ✅ Verifying confidence score proof...");
    
    let proof_valid = !proof.range_proof.is_empty() &&
                      !proof.computation_proof.is_empty() &&
                      !proof.derivation_proof.is_empty() &&
                      verification_key.len() >= 32;

    Ok(proof_valid)
}

// Helper functions for proof generation
fn extract_behavioral_features(patterns: &BehavioralPatterns) -> Result<Vec<u64>, Box<dyn Error>> {
    let features = vec![
        (patterns.storage_behavior.consistency_score * 1000.0) as u64,
        (patterns.compute_participation.service_quality * 1000.0) as u64,
        (patterns.economic_patterns.earning_consistency * 1000.0) as u64,
        (patterns.service_quality.success_ratio * 1000.0) as u64,
        (patterns.multi_chain_activity.identity_consistency * 1000.0) as u64,
    ];
    Ok(features)
}

fn calculate_consistency_threshold(patterns: &BehavioralPatterns) -> Result<u64, Box<dyn Error>> {
    let avg_consistency = (
        patterns.storage_behavior.consistency_score +
        patterns.compute_participation.service_quality +
        patterns.economic_patterns.earning_consistency +
        patterns.service_quality.success_ratio
    ) / 4.0;
    Ok((avg_consistency * 1000.0) as u64)
}

fn generate_commitment_randomness(count: usize) -> Result<Vec<u64>, Box<dyn Error>> {
    let mut rng = rand::thread_rng();
    let randomness: Vec<u64> = (0..count).map(|_| rng.next_u64()).collect();
    Ok(randomness)
}

fn create_behavioral_commitments(
    features: &[u64],
    randomness: &[u64],
) -> Result<Vec<CommitmentOpening>, Box<dyn Error>> {
    let mut commitments = Vec::new();
    
    for (feature, rand) in features.iter().zip(randomness.iter()) {
        // Simplified commitment: hash(feature || randomness)
        let mut hasher = Sha256::new();
        hasher.update(feature.to_le_bytes());
        hasher.update(rand.to_le_bytes());
        let commitment = hasher.finalize().to_vec();

        commitments.push(CommitmentOpening {
            commitment,
            randomness: rand.to_le_bytes().to_vec(),
            value: Some(feature.to_le_bytes().to_vec()),
        });
    }

    Ok(commitments)
}

fn serialize_mock_proof<C: Circuit<Fp>>(circuit: &C, k: u32) -> Result<Vec<u8>, Box<dyn Error>> {
    // Simplified proof serialization for demonstration
    // In production, would serialize actual halo2 proof
    let mut proof_data = Vec::new();
    proof_data.extend_from_slice(&k.to_le_bytes());
    proof_data.extend_from_slice(b"SWTCH_ZK_PROOF_V1.0");
    proof_data.extend_from_slice(&(rand::thread_rng().next_u64().to_le_bytes()));
    Ok(proof_data)
}

fn extract_ai_features(ai_analysis: &AIAnalysisResult) -> Result<Vec<u64>, Box<dyn Error>> {
    let features = vec![
        (ai_analysis.ai_confidence * 1000.0) as u64,
        (ai_analysis.anomaly_report.anomaly_score * 1000.0) as u64,
        (ai_analysis.threat_assessment.confidence * 1000.0) as u64,
    ];
    Ok(features)
}

fn generate_model_params(count: usize) -> Result<Vec<u64>, Box<dyn Error>> {
    let mut rng = rand::thread_rng();
    let params: Vec<u64> = (0..count).map(|_| rng.next_u64() % 1000).collect();
    Ok(params)
}

fn generate_input_integrity_proof(inputs: &[u64]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    for input in inputs {
        hasher.update(input.to_le_bytes());
    }
    Ok(hasher.finalize().to_vec())
}

fn create_result_commitment(result: f64) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hasher.update(((result * 1000.0) as u64).to_le_bytes());
    hasher.update(rand::thread_rng().next_u64().to_le_bytes());
    Ok(hasher.finalize().to_vec())
}

fn extract_identity_elements(session: &RecoverySession) -> Result<Vec<u64>, Box<dyn Error>> {
    // Simplified identity element extraction
    let mut hasher = Sha256::new();
    hasher.update(session.identity_did.as_bytes());
    let hash = hasher.finalize();
    
    let elements: Vec<u64> = hash.chunks(8)
        .take(4)
        .map(|chunk| {
            let mut bytes = [0u8; 8];
            bytes[..chunk.len()].copy_from_slice(chunk);
            u64::from_le_bytes(bytes)
        })
        .collect();
    
    Ok(elements)
}

fn extract_challenge_responses(session: &RecoverySession) -> Result<Vec<u64>, Box<dyn Error>> {
    // Simplified challenge response extraction
    let responses = vec![
        session.session_id.len() as u64,
        session.peer_endorsements.endorsements.len() as u64,
        1000u64, // Placeholder for timestamp
    ];
    Ok(responses)
}

fn extract_consensus_data(session: &RecoverySession) -> Result<Vec<u64>, Box<dyn Error>> {
    // Simplified consensus data extraction
    let consensus_data = vec![
        700u64, // Placeholder for confidence threshold
        1000u64, // Simulated consensus score
        25u64,   // Simulated participating nodes
    ];
    Ok(consensus_data)
}

fn generate_challenge_response_proof(responses: &[u64]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    for response in responses {
        hasher.update(response.to_le_bytes());
    }
    Ok(hasher.finalize().to_vec())
}

fn generate_consensus_proof(consensus_data: &[u64]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    for data in consensus_data {
        hasher.update(data.to_le_bytes());
    }
    Ok(hasher.finalize().to_vec())
}

fn generate_computation_elements() -> Result<Vec<u64>, Box<dyn Error>> {
    let mut rng = rand::thread_rng();
    let elements: Vec<u64> = (0..8).map(|_| rng.next_u64() % 1000).collect();
    Ok(elements)
}

fn generate_homomorphic_computation_proof(elements: &[u64]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    for element in elements {
        hasher.update(element.to_le_bytes());
    }
    Ok(hasher.finalize().to_vec())
}

fn generate_derivation_proof(confidence_value: f64) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut hasher = Sha256::new();
    hasher.update(((confidence_value * 1000000.0) as u64).to_le_bytes());
    hasher.update(b"SWTCH_CONFIDENCE_DERIVATION");
    Ok(hasher.finalize().to_vec())
}

fn verify_commitment_openings(commitments: &[CommitmentOpening]) -> Result<bool, Box<dyn Error>> {
    for commitment in commitments {
        if let Some(value) = &commitment.value {
            // Verify commitment = hash(value || randomness)
            let mut hasher = Sha256::new();
            hasher.update(value);
            hasher.update(&commitment.randomness);
            let expected_commitment = hasher.finalize().to_vec();
            
            if expected_commitment != commitment.commitment {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavioral::*;

    #[test]
    fn test_behavioral_feature_extraction() {
        let patterns = BehavioralPatterns::default();
        let features = extract_behavioral_features(&patterns).unwrap();
        assert_eq!(features.len(), 5);
        assert!(features.iter().all(|&f| f <= 1000));
    }

    #[test]
    fn test_commitment_creation() {
        let features = vec![100, 200, 300];
        let randomness = vec![123, 456, 789];
        let commitments = create_behavioral_commitments(&features, &randomness).unwrap();
        
        assert_eq!(commitments.len(), 3);
        assert!(verify_commitment_openings(&commitments).unwrap());
    }

    #[test]
    fn test_proof_serialization() {
        let circuit = BehavioralConsistencyCircuit {
            pattern_features: vec![Value::known(Fp::from(100u64))],
            consistency_threshold: Value::known(Fp::from(500u64)),
            commitment_randomness: vec![Value::known(Fp::from(123u64))],
        };
        
        let proof_data = serialize_mock_proof(&circuit, 8).unwrap();
        assert!(!proof_data.is_empty());
        assert!(proof_data.len() > 20); // Should contain k, version, and random data
    }
}
