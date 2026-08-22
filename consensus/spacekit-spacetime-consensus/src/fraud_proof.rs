//! Fraud-proof submission and verification flow.
//!
//! This module is the dispatcher between (a) the evidence types that already
//! exist in `equivocation.rs` and `fingerprint_attestation.rs`, (b) the
//! `TieredFinality` state machine, and (c) the rollback hook the coordinator
//! exposes (`rollback_block_spacetime_side_effects`).
//!
//! A fraud proof targets a specific block height. If verified and accepted
//! inside that block's challenge window, the result is:
//!
//!   1. `TieredFinality::on_fraud_proof_accepted` marks the target and all
//!      successors as Reverted.
//!   2. The coordinator runs rollback hooks tip-first (descending heights).
//!   3. Mempool requeue happens outside this crate.
//!   4. The submitter of the valid fraud proof is eligible for a bounty
//!      from the slashed validator's stake (policy-defined elsewhere).
//!
//! ## Verification cost
//!
//! All fraud-proof verification is local, no signatures to validate beyond
//! what the existing crypto layer does. The arithmetic checks
//! (`verify_mismatch`, `verify_contradiction`, etc.) are microsecond-scale.

use crate::equivocation::{
    DualRotorEvidence, FingerprintDepartureEvidence, SandwichMismatchEvidence, SlashingCategory,
    SlashingProposal, SlashingSeverity,
};
use crate::finality::{FinalityError, FinalityStage, TieredFinality};
use crate::fingerprint_attestation::FingerprintAttestationMismatchEvidence;
use alloc::vec::Vec;
use alloy_primitives::B256;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FraudProof {
    /// Validator signed two different rotors for the same (round, view).
    DualSigning(DualRotorEvidence),
    /// Validator signed a transition whose sandwich product doesn't match
    /// the claimed new state.
    InvalidTransition(SandwichMismatchEvidence),
    /// Validator's rotor at this block exceeded sigma threshold from their
    /// historical fingerprint.
    FingerprintDeparture(FingerprintDepartureEvidence),
    /// Two validators produced different fingerprint roots for the same block.
    /// At least one (possibly both) computed fingerprints incorrectly.
    FingerprintAttestationMismatch(FingerprintAttestationMismatchEvidence),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FraudProofSubmission {
    pub submitter_did_hash: B256,
    pub target_height: u64,
    pub target_block_hash: B256,
    pub proof: FraudProof,
    /// Submitter's signature digest. Verified upstream.
    pub signature_digest: B256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FraudProofError {
    OutsideChallengeWindow,
    InvalidProof,
    TargetBlockNotSoftFinalized,
    AlreadyProcessed,
    StateMachineError(FinalityError),
}

impl From<FinalityError> for FraudProofError {
    fn from(e: FinalityError) -> Self {
        Self::StateMachineError(e)
    }
}

/// Outcome of submitting a fraud proof.
#[derive(Debug, Clone)]
pub struct FraudProofAcceptance {
    pub target_height: u64,
    pub rolled_back_heights: Vec<u64>,
    pub slashing_proposals: Vec<SlashingProposal>,
}

/// Verify a fraud proof's internal consistency. Does NOT check the proof's
/// signatures or whether the block referenced exists — those are caller
/// responsibilities at the network layer.
pub fn verify_fraud_proof(proof: &FraudProof) -> Result<(), FraudProofError> {
    let ok = match proof {
        FraudProof::DualSigning(e) => e.verify_contradiction().is_ok(),
        FraudProof::InvalidTransition(e) => e.verify_mismatch().is_ok(),
        FraudProof::FingerprintDeparture(e) => e.verify_departure().is_ok(),
        FraudProof::FingerprintAttestationMismatch(e) => e.verify_mismatch(),
    };
    if !ok {
        return Err(FraudProofError::InvalidProof);
    }
    Ok(())
}

/// Process a fraud-proof submission: verify, check window, trigger rollback,
/// produce slashing proposals.
///
/// `evidence_hash_fn` is the same domain-tagged hash used elsewhere in the
/// stack; we pass it in to avoid baking a specific algorithm into this crate.
pub fn submit_fraud_proof<F: Fn(&[u8]) -> [u8; 32]>(
    finality: &mut TieredFinality,
    submission: &FraudProofSubmission,
    evidence_hash_fn: F,
) -> Result<FraudProofAcceptance, FraudProofError> {
    // 1. Verify the proof itself.
    verify_fraud_proof(&submission.proof)?;

    // 2. Verify the target is still in the challenge window.
    match finality.stage_of(submission.target_height) {
        FinalityStage::Soft => {}
        FinalityStage::Hard => return Err(FraudProofError::OutsideChallengeWindow),
        FinalityStage::Reverted => return Err(FraudProofError::AlreadyProcessed),
    }

    // 3. Apply state machine transition.
    let rolled_back = finality.on_fraud_proof_accepted(submission.target_height)?;

    // 4. Derive slashing proposals.
    let slashing_proposals = build_slashing_proposals(&submission.proof, evidence_hash_fn);

    Ok(FraudProofAcceptance {
        target_height: submission.target_height,
        rolled_back_heights: rolled_back,
        slashing_proposals,
    })
}

fn build_slashing_proposals<F: Fn(&[u8]) -> [u8; 32]>(
    proof: &FraudProof,
    hash_fn: F,
) -> Vec<SlashingProposal> {
    // For the digest we just hash a minimal canonicalization of the proof
    // variant. The caller's slashing crate may store the full proof
    // separately and use this hash as a key.
    match proof {
        FraudProof::DualSigning(e) => {
            let mut buf = Vec::with_capacity(64);
            buf.extend_from_slice(e.validator_did.as_slice());
            buf.extend_from_slice(&e.round.to_be_bytes());
            buf.extend_from_slice(&e.view.to_be_bytes());
            let h = B256::from(hash_fn(&buf));
            vec![SlashingProposal::from_dual_signing(e, h)]
        }
        FraudProof::InvalidTransition(e) => {
            let mut buf = Vec::with_capacity(64);
            buf.extend_from_slice(e.validator_did.as_slice());
            buf.extend_from_slice(&e.transition.transition_id.to_be_bytes());
            let h = B256::from(hash_fn(&buf));
            vec![SlashingProposal::from_invalid_transition(e, h)]
        }
        FraudProof::FingerprintDeparture(e) => {
            let mut buf = Vec::with_capacity(64);
            buf.extend_from_slice(e.validator_did.as_slice());
            buf.extend_from_slice(&e.transition.transition_id.to_be_bytes());
            let h = B256::from(hash_fn(&buf));
            vec![SlashingProposal::from_fingerprint_departure(e, h)]
        }
        FraudProof::FingerprintAttestationMismatch(e) => {
            // BOTH attesters are candidates for slashing — at least one is wrong.
            let (a, b) = e.slash_candidates();
            let mut buf = Vec::with_capacity(96);
            buf.extend_from_slice(&e.height.to_be_bytes());
            buf.extend_from_slice(e.block_hash.as_slice());
            buf.extend_from_slice(a.as_slice());
            buf.extend_from_slice(b.as_slice());
            let h = B256::from(hash_fn(&buf));
            vec![
                SlashingProposal {
                    validator_did: a,
                    category: SlashingCategory::InvalidTransition,
                    severity: SlashingSeverity::Partial,
                    evidence_hash: h,
                },
                SlashingProposal {
                    validator_did: b,
                    category: SlashingCategory::InvalidTransition,
                    severity: SlashingSeverity::Partial,
                    evidence_hash: h,
                },
            ]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algebra::Multivector;
    use crate::causal::CausalCoord;
    use crate::finality::TieredFinalityConfig;
    use crate::proposal::SpacetimeTransition;
    use crate::rotor::{Bivector, Rotor};

    fn h(b: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, byte) in b.iter().enumerate() {
            out[i % 32] ^= byte.wrapping_mul(31);
        }
        out
    }

    fn dummy_transition(id: u64) -> SpacetimeTransition {
        let (residual_commitment, residual_norm) = SpacetimeTransition::zero_residual_fields(h);
        SpacetimeTransition {
            transition_id: id,
            rotor: Rotor::exp(&Bivector {
                b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
            }),
            prev_state_hash: B256::ZERO,
            new_state_hash: B256::from([id as u8 + 1; 32]),
            causal_coord: CausalCoord {
                t: id as f64 + 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment,
            residual_norm,
            aux_commit: None,
        }
    }

    #[test]
    fn invalid_transition_proof_triggers_rollback() {
        let r = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
        });
        let prev = Multivector::ONE;
        let correct = r.apply(&prev);
        let mut wrong = correct;
        wrong.coeffs[0] += 1.0; // Bad new_state.

        let evidence = SandwichMismatchEvidence {
            validator_did: B256::from([0xAA; 32]),
            transition: dummy_transition(5),
            prev_state: prev,
            claimed_new_state: wrong,
            tolerance: 1e-6,
        };

        let mut tf = TieredFinality::new(
            TieredFinalityConfig {
                challenge_window: 100,
                max_pending: 100,
            },
            0,
        );
        for i in 1..=8 {
            tf.on_soft_finalize(i, B256::from([i as u8; 32]));
        }

        let submission = FraudProofSubmission {
            submitter_did_hash: B256::from([0xCC; 32]),
            target_height: 5,
            target_block_hash: B256::from([5u8; 32]),
            proof: FraudProof::InvalidTransition(evidence),
            signature_digest: B256::from([0xDD; 32]),
        };

        let result = submit_fraud_proof(&mut tf, &submission, h).expect("accept");
        assert_eq!(result.target_height, 5);
        // Heights 5..=8 rolled back, in descending order.
        assert_eq!(result.rolled_back_heights, vec![8, 7, 6, 5]);
        assert_eq!(tf.stage_of(5), FinalityStage::Reverted);
        assert_eq!(result.slashing_proposals.len(), 1);
    }

    #[test]
    fn proof_outside_window_rejected() {
        let r = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
        });
        let prev = Multivector::ONE;
        let mut wrong = r.apply(&prev);
        wrong.coeffs[0] += 1.0;
        let evidence = SandwichMismatchEvidence {
            validator_did: B256::from([0xAA; 32]),
            transition: dummy_transition(2),
            prev_state: prev,
            claimed_new_state: wrong,
            tolerance: 1e-6,
        };
        let mut tf = TieredFinality::new(
            TieredFinalityConfig {
                challenge_window: 3,
                max_pending: 100,
            },
            0,
        );
        // Advance past the challenge window for height 2.
        for i in 1..=10 {
            tf.on_soft_finalize(i, B256::from([i as u8; 32]));
        }
        assert_eq!(tf.stage_of(2), FinalityStage::Hard);

        let submission = FraudProofSubmission {
            submitter_did_hash: B256::from([0xCC; 32]),
            target_height: 2,
            target_block_hash: B256::from([2u8; 32]),
            proof: FraudProof::InvalidTransition(evidence),
            signature_digest: B256::from([0xDD; 32]),
        };
        assert!(matches!(
            submit_fraud_proof(&mut tf, &submission, h),
            Err(FraudProofError::OutsideChallengeWindow)
        ));
    }

    #[test]
    fn bogus_proof_rejected_before_state_change() {
        // A SandwichMismatchEvidence where the math actually MATCHES — should
        // be rejected as InvalidProof, never reaching the state machine.
        let r = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
        });
        let prev = Multivector::ONE;
        let correct = r.apply(&prev);
        let evidence = SandwichMismatchEvidence {
            validator_did: B256::from([0xAA; 32]),
            transition: dummy_transition(5),
            prev_state: prev,
            claimed_new_state: correct, // CORRECT, so no actual fraud.
            tolerance: 1e-6,
        };
        let mut tf = TieredFinality::new(TieredFinalityConfig::default(), 0);
        tf.on_soft_finalize(5, B256::from([5u8; 32]));
        let submission = FraudProofSubmission {
            submitter_did_hash: B256::from([0xCC; 32]),
            target_height: 5,
            target_block_hash: B256::from([5u8; 32]),
            proof: FraudProof::InvalidTransition(evidence),
            signature_digest: B256::from([0xDD; 32]),
        };
        assert!(matches!(
            submit_fraud_proof(&mut tf, &submission, h),
            Err(FraudProofError::InvalidProof)
        ));
        // State unchanged.
        assert_eq!(tf.stage_of(5), FinalityStage::Soft);
    }
}
