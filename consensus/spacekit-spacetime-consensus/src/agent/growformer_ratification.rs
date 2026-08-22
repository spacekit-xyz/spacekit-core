//! Growformer parameter ratification.
//!
//! Closes the disarmament risk: an attacker who compromises the Growformer
//! optimizer cannot silently push security-critical parameters (sigma_threshold,
//! divergence_threshold, challenge_window, etc.) into a weakened state.
//! Instead, Growformer *proposes*; validators *ratify* via PBFT vote.
//!
//! ## Wire format mirrors the training data
//!
//! `ParameterChangeProposal` is shaped like a row in your training JSONL
//! (`semantic_intent`, `action_target`, `policy_regime`, `expected_response`).
//! A Growformer inference can be cast directly into a proposal payload
//! without translation. This means the same TOML grounding files
//! (e.g. fintech sentiment) provide the vocabulary for consensus tuning if
//! you choose to extend them; the consensus-tuning domain TOML lives
//! separately but uses the identical schema.
//!
//! ## Ratification flow
//!
//! 1. Growformer emits a `GrowformerInference` covering recent consensus
//!    metrics. The intent classifies "tighten", "loosen", "no_change", or
//!    "alert".
//! 2. A proposer wraps the inference into a `ParameterChangeProposal` and
//!    broadcasts it.
//! 3. Validators independently re-run their own Growformer over the same
//!    metric input. If their inference agrees, they vote YES. If it
//!    disagrees, they vote NO.
//! 4. Standard PBFT 2/3 threshold applies. On YES quorum, the change is
//!    queued for activation at `block_proposal_height + activation_delay`.
//! 5. The delay window is the security margin: if a YES voter is later
//!    shown to have ratified a change that caused a security breach
//!    within `safety_window` blocks of activation, the YES vote becomes
//!    slashable evidence.
//!
//! ## Why activation delay matters
//!
//! Without a delay, a ratified change applies immediately, and any in-flight
//! attack relying on the old parameter completes before observers can react.
//! The activation delay gives the fraud-proof pipeline time to reverse the
//! change if it was poisoned. Recommended: activation_delay ≥ challenge_window.

use alloc::string::String;
use alloc::vec::Vec;
use alloy_primitives::B256;

/// Which parameter is being changed. String-typed for forward compatibility
/// — new parameters can be added without recompiling validators. Validators
/// reject proposals targeting unknown parameters.
pub type ParameterPath = String;

/// Policy regime aligns with the training-data `policy_regime` field.
/// `Default` is normal operation; `Secure` is heightened-threat mode that
/// rejects loosening proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PolicyRegime {
    Default,
    Secure,
    /// "Permissive" — allows looser thresholds during low-threat windows.
    /// Only enterable by supermajority vote, not by Growformer alone.
    Permissive,
}

/// Growformer-derived classification of "what should happen to this parameter."
/// Maps to `semantic_intent` in your training JSONL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GrowformerIntent {
    /// Tighten the parameter (e.g. lower sigma_threshold, lower
    /// divergence_threshold). Reduces false negatives, may increase false
    /// positives.
    Tighten,
    /// Loosen the parameter. Reduces false positives. Inherently riskier;
    /// in `Secure` regime, loosening proposals require unanimous validator
    /// agreement.
    Loosen,
    /// No change recommended.
    NoChange,
    /// Strong anomalous-pattern alert; usually paired with a tighten
    /// proposal on a security threshold, or an escalation to incident
    /// response (outside this crate).
    Alert,
}

/// One inference from Growformer over a window of consensus metrics.
/// `task_id` and `expected_response` mirror the training-data shape so a
/// production Growformer can emit these directly.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GrowformerInference {
    pub task_id: String,
    /// Domain — typically `"consensus_tuning"`.
    pub domain: String,
    pub semantic_intent: GrowformerIntent,
    /// Which parameter; e.g. `"spacetime.divergence_threshold"`.
    pub action_target: ParameterPath,
    pub policy_regime: PolicyRegime,
    /// Human-readable reasoning. Mirrors training-data `expected_response`.
    pub expected_response: String,
    /// Hash of the consensus-metrics window fed into Growformer. Validators
    /// recompute their own inference over the same window to verify.
    pub metrics_window_hash: B256,
    /// Growformer's confidence in [0, 1]. Proposals below a configurable
    /// minimum confidence are auto-rejected by validators.
    pub confidence: f64,
}

/// A proposal to change a consensus parameter, wrapping a Growformer
/// inference plus the concrete numeric change.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParameterChangeProposal {
    /// Unique proposal ID — typically a hash of (inference, current value, height).
    pub proposal_id: B256,
    /// Proposing validator's DID hash.
    pub proposer_did_hash: B256,
    /// Block height at which this proposal was created.
    pub proposed_at_height: u64,
    /// The Growformer inference backing this proposal.
    pub inference: GrowformerInference,
    /// Current value, encoded as little-endian f64 bytes. Parameter-specific
    /// types are decoded by the consensus crate.
    pub current_value: [u8; 8],
    /// Proposed new value, same encoding.
    pub proposed_value: [u8; 8],
    /// Activation delay in blocks. Must be ≥ `min_activation_delay`.
    pub activation_delay: u64,
}

impl ParameterChangeProposal {
    pub fn current_f64(&self) -> f64 {
        f64::from_le_bytes(self.current_value)
    }
    pub fn proposed_f64(&self) -> f64 {
        f64::from_le_bytes(self.proposed_value)
    }
}

/// A vote on a parameter-change proposal. The validator's commitment that
/// their independent Growformer inference agreed with the proposer's,
/// AND that the change is safe under the validator's view of the network.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParameterChangeVote {
    pub proposal_id: B256,
    pub voter_did_hash: B256,
    pub vote: bool, // true = YES, false = NO
    /// Validator's own metrics-window hash. If this differs from the
    /// proposal's, validators saw different metric inputs — which is itself
    /// suspicious and bias YES voters toward NO.
    pub voter_metrics_window_hash: B256,
    pub signature_digest: B256,
}

/// Configuration governing ratification.
#[derive(Debug, Clone, Copy)]
pub struct RatificationConfig {
    /// Minimum confidence for a Growformer inference to be considered.
    /// Proposals below this are rejected by validators.
    pub min_confidence: f64,
    /// Minimum activation delay in blocks. Should be ≥ challenge_window
    /// from the finality config.
    pub min_activation_delay: u64,
    /// Quorum threshold (0.0–1.0). PBFT 2/3 default.
    pub quorum_threshold: f64,
    /// Stricter quorum for loosening proposals in Secure regime.
    pub secure_loosen_threshold: f64,
    /// Bounds on each parameter. Proposals whose proposed_value falls
    /// outside the bound for the target parameter are auto-rejected. The
    /// parameter-to-bound mapping is stored in the consensus crate; we
    /// just carry the global min/max here as a safety floor/ceiling.
    pub global_min: f64,
    pub global_max: f64,
}

impl Default for RatificationConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.7,
            min_activation_delay: 100,
            quorum_threshold: 0.67,
            secure_loosen_threshold: 0.95,
            global_min: f64::MIN,
            global_max: f64::MAX,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RatificationError {
    ConfidenceTooLow,
    ActivationDelayTooShort,
    ValueOutOfBounds,
    UnknownParameter,
    LooseningRejectedInSecureRegime,
    QuorumNotReached,
    ProposalNotFound,
    AlreadyVoted,
}

/// Validator-side check before voting YES on a proposal. The consensus crate
/// calls this with its own re-derived Growformer inference; if any check
/// fails, the validator votes NO (or abstains).
pub fn validator_should_ratify(
    proposal: &ParameterChangeProposal,
    own_inference: &GrowformerInference,
    current_regime: PolicyRegime,
    config: &RatificationConfig,
) -> Result<(), RatificationError> {
    if proposal.inference.confidence < config.min_confidence {
        return Err(RatificationError::ConfidenceTooLow);
    }
    if proposal.activation_delay < config.min_activation_delay {
        return Err(RatificationError::ActivationDelayTooShort);
    }
    let proposed = proposal.proposed_f64();
    if proposed < config.global_min || proposed > config.global_max {
        return Err(RatificationError::ValueOutOfBounds);
    }
    // Loosening in Secure regime is blocked at the validator level.
    if current_regime == PolicyRegime::Secure
        && proposal.inference.semantic_intent == GrowformerIntent::Loosen
    {
        return Err(RatificationError::LooseningRejectedInSecureRegime);
    }
    // Validator's own inference must agree with proposer's, AND must be
    // computed over the same metrics window — otherwise vote NO.
    if own_inference.semantic_intent != proposal.inference.semantic_intent {
        return Err(RatificationError::ConfidenceTooLow); // intent disagreement
    }
    if own_inference.metrics_window_hash != proposal.inference.metrics_window_hash {
        // Different metrics windows: either upstream gossip is broken or
        // someone is feeding the proposer fabricated metrics. Either way,
        // refuse.
        return Err(RatificationError::ConfidenceTooLow);
    }
    Ok(())
}

/// Aggregate votes for a proposal and decide ratification outcome.
///
/// `voting_powers` provides reputation-weighted power per voter. The same
/// power table used by `ReputationWeightedConsensus`.
pub fn evaluate_ratification(
    proposal: &ParameterChangeProposal,
    votes: &[ParameterChangeVote],
    voting_powers: &[(B256, f64)],
    current_regime: PolicyRegime,
    config: &RatificationConfig,
) -> Result<f64, RatificationError> {
    let total_power: f64 = voting_powers.iter().map(|(_, p)| p).sum();
    if total_power <= 0.0 {
        return Err(RatificationError::QuorumNotReached);
    }

    let mut yes_power = 0.0;
    let mut no_power = 0.0;
    for v in votes {
        let p = voting_powers
            .iter()
            .find(|(did, _)| *did == v.voter_did_hash)
            .map(|(_, p)| *p)
            .unwrap_or(0.0);
        if v.vote {
            yes_power += p;
        } else {
            no_power += p;
        }
    }

    let yes_ratio = yes_power / total_power;
    let no_ratio = no_power / total_power;
    let threshold = match (current_regime, proposal.inference.semantic_intent) {
        (PolicyRegime::Secure, GrowformerIntent::Loosen) => config.secure_loosen_threshold,
        _ => config.quorum_threshold,
    };
    if yes_ratio < threshold {
        return Err(RatificationError::QuorumNotReached);
    }
    // Explicit NO weight must not meet or exceed YES among voters who cast a ballot.
    let cast_power = yes_power + no_power;
    if cast_power > 0.0 && yes_power <= no_power {
        return Err(RatificationError::QuorumNotReached);
    }
    // Reject if opposing weight alone exceeds (1 - threshold) of total reputation.
    if no_ratio > 1.0 - threshold {
        return Err(RatificationError::QuorumNotReached);
    }
    Ok(yes_ratio)
}

/// Evidence that a YES vote ratified a parameter change which, within the
/// safety window after activation, was followed by a successful attack
/// exploiting the weakened threshold. Slashable.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MalignRatificationEvidence {
    pub proposal_id: B256,
    pub bad_voter_did_hash: B256,
    /// The YES vote being indicted.
    pub vote: ParameterChangeVote,
    /// Height at which the parameter change activated.
    pub activated_at_height: u64,
    /// Height at which a fraud proof exploiting the weakened parameter was
    /// accepted. Must be within `safety_window` of `activated_at_height`.
    pub attack_height: u64,
    /// The fraud-proof submission digest, linking back to the recovery event.
    pub fraud_proof_digest: B256,
}

impl MalignRatificationEvidence {
    /// Verify that the attack indeed fell within the safety window after
    /// the parameter activated.
    pub fn verify(&self, safety_window: u64) -> bool {
        if self.attack_height < self.activated_at_height {
            return false;
        }
        let gap = self.attack_height - self.activated_at_height;
        gap <= safety_window
    }

    /// Slashable proposal for the YES voter who ratified a change later exploited.
    pub fn to_slashing_proposal(&self) -> crate::equivocation::SlashingProposal {
        use crate::equivocation::{SlashingCategory, SlashingProposal, SlashingSeverity};
        SlashingProposal {
            validator_did: self.bad_voter_did_hash,
            category: SlashingCategory::MalignRatification,
            severity: SlashingSeverity::Partial,
            evidence_hash: self.fraud_proof_digest,
        }
    }
}

/// A parameter change that reached quorum and activated on-chain.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ActivatedParameterChange {
    pub proposal: ParameterChangeProposal,
    pub activated_at_height: u64,
    pub yes_votes: Vec<ParameterChangeVote>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_inference(intent: GrowformerIntent, conf: f64) -> GrowformerInference {
        GrowformerInference {
            task_id: String::from("test_001"),
            domain: String::from("consensus_tuning"),
            semantic_intent: intent,
            action_target: String::from("spacetime.divergence_threshold"),
            policy_regime: PolicyRegime::Default,
            expected_response: String::from("test"),
            metrics_window_hash: B256::from([0xAA; 32]),
            confidence: conf,
        }
    }

    fn make_proposal(intent: GrowformerIntent, conf: f64, delay: u64) -> ParameterChangeProposal {
        ParameterChangeProposal {
            proposal_id: B256::from([0x11; 32]),
            proposer_did_hash: B256::from([0x22; 32]),
            proposed_at_height: 100,
            inference: make_inference(intent, conf),
            current_value: 0.5f64.to_le_bytes(),
            proposed_value: 0.4f64.to_le_bytes(),
            activation_delay: delay,
        }
    }

    #[test]
    fn low_confidence_proposal_rejected() {
        let p = make_proposal(GrowformerIntent::Tighten, 0.5, 100);
        let own = make_inference(GrowformerIntent::Tighten, 0.95);
        let r = validator_should_ratify(
            &p,
            &own,
            PolicyRegime::Default,
            &RatificationConfig::default(),
        );
        assert_eq!(r, Err(RatificationError::ConfidenceTooLow));
    }

    #[test]
    fn short_delay_rejected() {
        let p = make_proposal(GrowformerIntent::Tighten, 0.9, 5);
        let own = make_inference(GrowformerIntent::Tighten, 0.9);
        let r = validator_should_ratify(
            &p,
            &own,
            PolicyRegime::Default,
            &RatificationConfig::default(),
        );
        assert_eq!(r, Err(RatificationError::ActivationDelayTooShort));
    }

    #[test]
    fn loosen_blocked_in_secure_regime() {
        let p = make_proposal(GrowformerIntent::Loosen, 0.9, 200);
        let own = make_inference(GrowformerIntent::Loosen, 0.9);
        let r = validator_should_ratify(
            &p,
            &own,
            PolicyRegime::Secure,
            &RatificationConfig::default(),
        );
        assert_eq!(r, Err(RatificationError::LooseningRejectedInSecureRegime));
    }

    #[test]
    fn metrics_window_mismatch_rejected() {
        let p = make_proposal(GrowformerIntent::Tighten, 0.9, 200);
        let mut own = make_inference(GrowformerIntent::Tighten, 0.9);
        own.metrics_window_hash = B256::from([0xBB; 32]); // different window
        let r = validator_should_ratify(
            &p,
            &own,
            PolicyRegime::Default,
            &RatificationConfig::default(),
        );
        assert_eq!(r, Err(RatificationError::ConfidenceTooLow));
    }

    #[test]
    fn intent_disagreement_rejected() {
        let p = make_proposal(GrowformerIntent::Tighten, 0.9, 200);
        let own = make_inference(GrowformerIntent::Loosen, 0.9);
        let r = validator_should_ratify(
            &p,
            &own,
            PolicyRegime::Default,
            &RatificationConfig::default(),
        );
        assert_eq!(r, Err(RatificationError::ConfidenceTooLow));
    }

    #[test]
    fn quorum_reached_with_two_thirds() {
        let p = make_proposal(GrowformerIntent::Tighten, 0.9, 200);
        let voters: Vec<B256> = (1..=9).map(|i| B256::from([i; 32])).collect();
        let voting_powers: Vec<(B256, f64)> = voters.iter().map(|v| (*v, 1.0)).collect();

        // 7/9 YES → 77.7% > 67%
        let votes: Vec<ParameterChangeVote> = voters
            .iter()
            .enumerate()
            .map(|(i, v)| ParameterChangeVote {
                proposal_id: p.proposal_id,
                voter_did_hash: *v,
                vote: i < 7,
                voter_metrics_window_hash: B256::from([0xAA; 32]),
                signature_digest: B256::ZERO,
            })
            .collect();

        let ratio = evaluate_ratification(
            &p,
            &votes,
            &voting_powers,
            PolicyRegime::Default,
            &RatificationConfig::default(),
        )
        .unwrap();
        assert!(ratio >= 0.67);
    }

    #[test]
    fn explicit_no_majority_blocks_even_at_threshold() {
        let mut cfg = RatificationConfig::default();
        cfg.quorum_threshold = 0.5;
        let p = make_proposal(GrowformerIntent::Tighten, 0.9, 200);
        let voters: Vec<B256> = (1..=10).map(|i| B256::from([i; 32])).collect();
        let voting_powers: Vec<(B256, f64)> = voters.iter().map(|v| (*v, 1.0)).collect();
        // 5 YES / 5 NO → yes_ratio = 0.5 meets threshold, but NO ties YES.
        let votes: Vec<ParameterChangeVote> = voters
            .iter()
            .enumerate()
            .map(|(i, v)| ParameterChangeVote {
                proposal_id: p.proposal_id,
                voter_did_hash: *v,
                vote: i < 5,
                voter_metrics_window_hash: B256::from([0xAA; 32]),
                signature_digest: B256::ZERO,
            })
            .collect();
        assert_eq!(
            evaluate_ratification(&p, &votes, &voting_powers, PolicyRegime::Default, &cfg),
            Err(RatificationError::QuorumNotReached)
        );
    }

    #[test]
    fn loosen_in_secure_needs_supermajority() {
        let p = make_proposal(GrowformerIntent::Loosen, 0.9, 200);
        let voters: Vec<B256> = (1..=10).map(|i| B256::from([i; 32])).collect();
        let voting_powers: Vec<(B256, f64)> = voters.iter().map(|v| (*v, 1.0)).collect();

        // 8/10 YES → 80% > 67% normal threshold, but < 95% secure-loosen threshold
        let votes: Vec<ParameterChangeVote> = voters
            .iter()
            .enumerate()
            .map(|(i, v)| ParameterChangeVote {
                proposal_id: p.proposal_id,
                voter_did_hash: *v,
                vote: i < 8,
                voter_metrics_window_hash: B256::from([0xAA; 32]),
                signature_digest: B256::ZERO,
            })
            .collect();

        // In Default regime: passes.
        assert!(evaluate_ratification(
            &p,
            &votes,
            &voting_powers,
            PolicyRegime::Default,
            &RatificationConfig::default()
        )
        .is_ok());
        // In Secure regime: fails (need 95% for loosening).
        assert_eq!(
            evaluate_ratification(
                &p,
                &votes,
                &voting_powers,
                PolicyRegime::Secure,
                &RatificationConfig::default()
            ),
            Err(RatificationError::QuorumNotReached)
        );
    }

    #[test]
    fn malign_ratification_within_window_is_slashable() {
        let ev = MalignRatificationEvidence {
            proposal_id: B256::from([1; 32]),
            bad_voter_did_hash: B256::from([2; 32]),
            vote: ParameterChangeVote {
                proposal_id: B256::from([1; 32]),
                voter_did_hash: B256::from([2; 32]),
                vote: true,
                voter_metrics_window_hash: B256::ZERO,
                signature_digest: B256::ZERO,
            },
            activated_at_height: 1000,
            attack_height: 1050,
            fraud_proof_digest: B256::from([3; 32]),
        };
        assert!(ev.verify(100)); // 1050 - 1000 = 50 <= 100
        assert!(!ev.verify(40)); // 50 > 40
                                 // Attack before activation: bogus evidence.
        let mut bogus = ev.clone();
        bogus.attack_height = 999;
        assert!(!bogus.verify(100));
    }
}
