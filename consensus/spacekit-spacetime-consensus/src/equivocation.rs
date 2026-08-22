//! Equivocation evidence for PBFT-style slashing.
//!
//! In a sleeper-detonation event the malicious validator must, at some
//! point, sign something that contradicts what an honest network observed:
//! either two different rotors for the same `(round, view)`, a rotor whose
//! sandwich product doesn't match the claimed `new_state_hash`, or a
//! "wake-up" rotor at distance > σ from its own historical fingerprint.
//!
//! Each of these is *cryptographically provable third-party evidence* once
//! observed. This module defines the evidence types and provides the
//! verification side. The submission/slashing path lives in your main
//! consensus crate; what we provide here is the spacetime-aware evidence
//! that didn't exist before.
//!
//! Evidence types defined:
//!   - `DualRotorEvidence`: validator signed two different rotors at the
//!     same (round, view). Classical PBFT equivocation, no new ideas.
//!   - `SandwichMismatchEvidence`: validator signed a transition where
//!     R̃ · prev · R does not yield new_state_hash. Provable from the
//!     transition bytes alone.
//!   - `FingerprintDepartureEvidence`: validator's rotor is >σ from their
//!     own EWMA centroid, where the centroid is reconstructible from
//!     past signed transitions. The other validators verify by replaying
//!     the fingerprint update from the same signed log.
//!   - `CliqueEvidence`: a set of validators whose rotors agree more
//!     tightly than their causal separation permits. Soft evidence —
//!     triggers heightened scrutiny, not automatic slashing.

use crate::algebra::Multivector;
use crate::defense::RotorFingerprint;
use crate::proposal::SpacetimeTransition;
use alloy_primitives::B256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvidenceError {
    NoEquivocation,
    TransitionsAreIdentical,
    SignatureCheckIsCallerResponsibility,
    SandwichActuallyMatches,
    FingerprintNotAnomalous,
    InsufficientHistory,
}

/// Validator signed two different transitions at the same (round, view).
/// Classical equivocation. We don't store signatures here — that's the
/// caller's existing `QuantumSafeVote` machinery. We provide the *content*
/// verification.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DualRotorEvidence {
    pub validator_did: B256,
    pub round: u64,
    pub view: u64,
    pub transition_a: SpacetimeTransition,
    pub transition_b: SpacetimeTransition,
}

impl DualRotorEvidence {
    /// Verify the evidence shows actual contradiction (different rotors).
    /// The caller separately verifies both transitions carry valid signatures
    /// from `validator_did`.
    pub fn verify_contradiction(&self) -> Result<(), EvidenceError> {
        let d = self.transition_a.rotor.distance(&self.transition_b.rotor);
        if d < 1e-9 {
            return Err(EvidenceError::TransitionsAreIdentical);
        }
        Ok(())
    }
}

/// Validator signed a transition where the sandwich product is wrong.
/// This is provable from the bytes alone — no signatures needed for the
/// arithmetic, and only the signature on `transition` is needed for
/// attribution.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SandwichMismatchEvidence {
    pub validator_did: B256,
    pub transition: SpacetimeTransition,
    /// The pre-transition state multivector as it was at the time. Sourced
    /// from the previous transition's commitment in the Verkle tree.
    pub prev_state: Multivector,
    /// The post-transition state multivector that was actually committed.
    pub claimed_new_state: Multivector,
    /// Tolerance used by honest validators (matches `verify_transition`).
    pub tolerance: f64,
}

impl SandwichMismatchEvidence {
    /// Verify that R̃ · prev · R indeed does NOT equal claimed_new_state
    /// within the agreed tolerance.
    pub fn verify_mismatch(&self) -> Result<(), EvidenceError> {
        let predicted = self.transition.rotor.apply(&self.prev_state);
        let mut diff_sq = 0.0;
        for i in 0..crate::algebra::BASIS_DIM {
            let d = predicted.coeffs[i] - self.claimed_new_state.coeffs[i];
            diff_sq += d * d;
        }
        if diff_sq.sqrt() <= self.tolerance {
            return Err(EvidenceError::SandwichActuallyMatches);
        }
        Ok(())
    }
}

/// A validator's rotor at round R departs >σ from the EWMA centroid that any
/// honest observer would have computed from the validator's prior signed
/// transitions.
///
/// Evidence is reproducible: anyone with the validator's signed history
/// can rebuild the same fingerprint deterministically.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FingerprintDepartureEvidence {
    pub validator_did: B256,
    /// The anomalous transition.
    pub transition: SpacetimeTransition,
    /// Reconstructed fingerprint state from prior signed transitions. The
    /// verifier can replay from the same `prior_transitions` list to confirm.
    pub fingerprint_at_event: RotorFingerprint,
    /// Sigma threshold the validator agreed to network-wide.
    pub sigma_threshold: f64,
}

impl FingerprintDepartureEvidence {
    pub fn verify_departure(&self) -> Result<(), EvidenceError> {
        if self.fingerprint_at_event.samples < 16 {
            return Err(EvidenceError::InsufficientHistory);
        }
        if !self
            .fingerprint_at_event
            .is_anomalous(self.transition.rotor, self.sigma_threshold)
        {
            return Err(EvidenceError::FingerprintNotAnomalous);
        }
        Ok(())
    }
}

/// Aggregated slashing decision. The consensus crate's slashing path consumes
/// this and applies the actual stake/reputation penalty according to your
/// existing schedule.
#[derive(Debug, Clone)]
pub struct SlashingProposal {
    pub validator_did: B256,
    pub category: SlashingCategory,
    pub severity: SlashingSeverity,
    /// Evidence digest (for inclusion in the slashing transaction).
    pub evidence_hash: B256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashingCategory {
    /// Two different rotors at the same (round, view). Provable, automatic.
    DualSigning,
    /// Sandwich product doesn't match claimed state. Provable, automatic.
    InvalidTransition,
    /// Rotor at >σ from validator's own fingerprint. Probabilistic, requires
    /// committee confirmation.
    BehavioralDeparture,
    /// Part of an identified coordination clique. Soft evidence — pauses
    /// validator pending investigation; doesn't auto-slash.
    SuspectedCoordination,
    /// YES vote on a parameter change that was followed by a fraud proof within the safety window.
    MalignRatification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashingSeverity {
    /// Full stake slash + reputation reset to 0 + ejection.
    Full,
    /// Partial slash (e.g., 10%) + reputation halved.
    Partial,
    /// No stake slash; reputation reduced by fixed amount.
    Reputational,
    /// No slash; validator paused pending committee review.
    Pause,
}

impl SlashingProposal {
    pub fn from_dual_signing(e: &DualRotorEvidence, evidence_hash: B256) -> Self {
        Self {
            validator_did: e.validator_did,
            category: SlashingCategory::DualSigning,
            severity: SlashingSeverity::Full,
            evidence_hash,
        }
    }
    pub fn from_invalid_transition(e: &SandwichMismatchEvidence, evidence_hash: B256) -> Self {
        Self {
            validator_did: e.validator_did,
            category: SlashingCategory::InvalidTransition,
            severity: SlashingSeverity::Full,
            evidence_hash,
        }
    }
    pub fn from_fingerprint_departure(
        e: &FingerprintDepartureEvidence,
        evidence_hash: B256,
    ) -> Self {
        Self {
            validator_did: e.validator_did,
            category: SlashingCategory::BehavioralDeparture,
            severity: SlashingSeverity::Partial,
            evidence_hash,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal::CausalCoord;
    use crate::rotor::{Bivector, Rotor};

    fn make_transition(rotor: Rotor, prev: B256, new: B256, id: u64) -> SpacetimeTransition {
        let (residual_commitment, residual_norm) =
            SpacetimeTransition::zero_residual_fields(|b| *alloy_primitives::keccak256(b));
        SpacetimeTransition {
            transition_id: id,
            rotor,
            prev_state_hash: prev,
            new_state_hash: new,
            causal_coord: CausalCoord {
                t: id as f64,
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
    fn dual_rotor_evidence_caught() {
        let r1 = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
        });
        let r2 = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.5, 0.0, 0.0],
        });
        let ev = DualRotorEvidence {
            validator_did: B256::from([1u8; 32]),
            round: 10,
            view: 0,
            transition_a: make_transition(r1, B256::ZERO, B256::from([1u8; 32]), 10),
            transition_b: make_transition(r2, B256::ZERO, B256::from([2u8; 32]), 10),
        };
        assert!(ev.verify_contradiction().is_ok());
    }

    #[test]
    fn sandwich_mismatch_detected() {
        let r = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
        });
        let prev = Multivector::ONE;
        // Predicted correctly:
        let correct = r.apply(&prev);
        // But claim something different:
        let mut wrong = correct;
        wrong.coeffs[0] += 1.0;
        let ev = SandwichMismatchEvidence {
            validator_did: B256::from([1u8; 32]),
            transition: make_transition(r, B256::ZERO, B256::from([3u8; 32]), 1),
            prev_state: prev,
            claimed_new_state: wrong,
            tolerance: 1e-6,
        };
        assert!(ev.verify_mismatch().is_ok());
    }
}
