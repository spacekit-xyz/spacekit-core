//! `UnifiedConsensusValidator`: facade-side validator record.
//!
//! Wraps the existing `ValidatorEntry` from `ConsensusCoordinator` and adds
//! the fields the doc's design intent calls for:
//!
//! - `reputation_score`: read through a `ReputationSource` at the facade
//!   level, cached here for the duration of a consensus round
//! - `performance_score`: facade-tracked observability (block proposal
//!   success rate, validation accuracy, availability) — not authoritative
//! - `effective_voting_power`: derived from stake + reputation + performance
//!   via `voting_power::effective_voting_power`
//!
//! ## The relationship to `ValidatorEntry`
//!
//! `ValidatorEntry { did, joined_at }` is the source of truth for who is a
//! validator. This type wraps it with derived attributes. The facade does
//! not own the validator set — `ConsensusCoordinator` does. The facade
//! computes views over that set.

use crate::voting_power::effective_voting_power;
use alloy_primitives::B256;

/// Facade-side validator view. Reconstructed each round from the coordinator's
/// `ValidatorEntry` set plus reputation lookups.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UnifiedConsensusValidator {
    /// DID hash. Matches `ValidatorEntry::did` byte-for-byte.
    pub did_hash: B256,

    /// Stake amount. Read from the staking module at round start. Today
    /// stake is implicit (validator is admitted, has some stake); this
    /// field is plumbing for the post-fork weighted threshold.
    pub stake_amount: u128,

    /// Reputation in [0, 1]. Read through `ReputationSource` at round start.
    /// Defaults to 1.0 when reputation source is `EqualWeightReputation`.
    pub reputation_score: f64,

    /// Performance multiplier in [0, 1]. Aggregates block proposal success
    /// rate, validation accuracy, and availability. Today returns 1.0;
    /// future versions update from per-round telemetry.
    pub performance_score: f64,

    /// Cached effective voting power. Computed from stake/reputation/performance
    /// at construction. Recompute when any input changes.
    pub effective_voting_power: f64,

    /// Validator lifecycle state. Mirrors `ConsensusCoordinator`'s notion of
    /// who can vote in the current round.
    pub status: ValidatorStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ValidatorStatus {
    /// Registered and eligible to vote in the current round.
    Active,
    /// Registered but skipped this round (e.g., known offline).
    Inactive,
    /// Recently admitted; within warm-up window before reputation accrues.
    WarmUp,
    /// Marked for removal at end of round (e.g., persistent slashing).
    PendingRemoval,
}

impl UnifiedConsensusValidator {
    /// Construct from primitives. Computes `effective_voting_power` once.
    pub fn new(
        did_hash: B256,
        stake_amount: u128,
        reputation_score: f64,
        performance_score: f64,
    ) -> Self {
        let r = reputation_score.clamp(0.0, 1.0);
        let p = performance_score.clamp(0.0, 1.0);
        let power = effective_voting_power(stake_amount, r, p);
        Self {
            did_hash,
            stake_amount,
            reputation_score: r,
            performance_score: p,
            effective_voting_power: power,
            status: ValidatorStatus::Active,
        }
    }

    /// Recompute effective voting power. Call after updating any input.
    pub fn recompute_power(&mut self) {
        self.effective_voting_power = effective_voting_power(
            self.stake_amount,
            self.reputation_score,
            self.performance_score,
        );
    }

    /// True if the validator should be counted in the current round.
    pub fn is_eligible(&self) -> bool {
        matches!(
            self.status,
            ValidatorStatus::Active | ValidatorStatus::WarmUp
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_sets_effective_power() {
        let v = UnifiedConsensusValidator::new(B256::from([1; 32]), 1_000_000, 0.8, 0.9);
        assert!(v.effective_voting_power > 0.0);
        assert_eq!(v.reputation_score, 0.8);
        assert_eq!(v.performance_score, 0.9);
    }

    #[test]
    fn equal_weight_validator_matches_equal_weight_power() {
        let v = UnifiedConsensusValidator::new(B256::from([1; 32]), 1_000_000, 1.0, 1.0);
        let expected = crate::voting_power::equal_weight_power(1_000_000);
        assert_eq!(v.effective_voting_power, expected);
    }

    #[test]
    fn recompute_after_reputation_change() {
        let mut v = UnifiedConsensusValidator::new(B256::from([1; 32]), 1_000_000, 1.0, 1.0);
        let original_power = v.effective_voting_power;
        v.reputation_score = 0.5;
        v.recompute_power();
        assert!((v.effective_voting_power / original_power - 0.5).abs() < 1e-9);
    }

    #[test]
    fn inactive_validators_not_eligible() {
        let mut v = UnifiedConsensusValidator::new(B256::from([1; 32]), 1_000_000, 1.0, 1.0);
        assert!(v.is_eligible());
        v.status = ValidatorStatus::Inactive;
        assert!(!v.is_eligible());
        v.status = ValidatorStatus::PendingRemoval;
        assert!(!v.is_eligible());
    }
}
