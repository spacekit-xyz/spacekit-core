//! `ReputationWeightedConsensus`: the facade.
//!
//! Wraps `ConsensusCoordinator` and exposes the API surface the spacetime
//! crate's documentation references. Today the facade delegates to the
//! coordinator for vote tallies and threshold checks; reputation is
//! observable (per `ReputationSource`) but not authoritative.
//!
//! ## Design notes
//!
//! - **The facade does not own validator state.** That lives in
//!   `ConsensusCoordinator`. The facade reads from the coordinator and
//!   computes views (e.g., effective voting power) over those validators.
//! - **Spacetime (optional, feature `spacetime`):** the facade is the entry
//!   for rotor `aggregate_votes` and `verify_transition` only. Fingerprint
//!   updates, tiered finality, fraud proofs, and parameter ratification stay
//!   on `ConsensusCoordinator` + `spacetime_integration.rs` in the node.
//! - **The facade is the single integration point for reputation.** Today
//!   reputation flows in via `ReputationSource`; tomorrow the same trait
//!   provides authoritative weights when the threshold check moves to
//!   weighted voting.
//!
//! ## What this facade is NOT
//!
//! - It is not a replacement for `ConsensusCoordinator`. The coordinator
//!   still runs the PBFT machinery, the P2P layer, the persistence path,
//!   and the actual vote collection. The facade is a thinner type that
//!   sits above all of it.
//! - It is not a place to put new consensus logic. New logic goes in the
//!   coordinator, the spacetime crate, or a sibling module, the facade
//!   exposes those pieces with a unified vocabulary, it does not implement
//!   them.

use crate::reputation_hook::{EqualWeightReputation, ReputationSource};
use crate::validator::{UnifiedConsensusValidator, ValidatorStatus};
use alloc::vec::Vec;
use alloy_primitives::B256;

extern crate alloc;

/// Errors that can occur at the facade level. These are distinct from
/// errors raised inside `ConsensusCoordinator` or the spacetime extension;
/// the facade surfaces those through `CoordinatorError` / `SpacetimeError`
/// variants without re-interpreting them.
#[derive(Debug, Clone, PartialEq)]
pub enum FacadeError {
    /// A validator referenced by DID is not registered with the coordinator.
    UnknownValidator(B256),
    /// The coordinator returned an error. The facade does not re-interpret
    /// these; callers handle them per the coordinator's own error contract.
    CoordinatorError(alloc::string::String),
    /// The spacetime extension returned an error.
    #[cfg(feature = "spacetime")]
    SpacetimeError(alloc::string::String),
    /// Quorum was not reached. Carries the observed and required ratios so
    /// callers can decide whether to retry or escalate.
    QuorumNotReached { observed: f64, required: f64 },
    /// Configuration is invalid (e.g., reputation source marked
    /// authoritative but threshold check is not configured for weighted).
    InvalidConfiguration(alloc::string::String),
}

/// Configuration for the facade. Defaults are equal-weight, count-based
/// threshold — matching `ConsensusCoordinator`'s current behavior.
#[derive(Debug, Clone)]
pub struct FacadeConfig {
    /// Quorum threshold as a fraction of total weight (default 2/3).
    pub quorum_threshold: f64,
    /// If true, the threshold check uses `effective_voting_power` weights.
    /// If false (default), uses count-based: 2/3 of eligible validators.
    ///
    /// Setting `true` requires a `ReputationSource` where
    /// `is_authoritative()` returns true. The facade will return
    /// `InvalidConfiguration` if mismatched.
    pub use_weighted_threshold: bool,
    /// Minimum participation rate (votes-received / eligible-validators)
    /// before threshold is evaluated. Below this, quorum is `NotReached`
    /// even if supporting weight is high.
    pub min_participation_rate: f64,
}

impl Default for FacadeConfig {
    fn default() -> Self {
        Self {
            quorum_threshold: 2.0 / 3.0,
            use_weighted_threshold: false,
            min_participation_rate: 2.0 / 3.0,
        }
    }
}

/// The facade type.
///
/// Construction takes a `ReputationSource` (boxed for object-safety) and a
/// configuration. Holds no validator state directly; queries the
/// coordinator each round.
///
/// Generic over the coordinator type so this crate doesn't take a hard
/// dependency on `spacekit-compute-node`. The coordinator trait is small
/// and lives below.
pub struct ReputationWeightedConsensus<C: CoordinatorHandle> {
    coordinator: C,
    reputation: alloc::boxed::Box<dyn ReputationSource>,
    config: FacadeConfig,
}

/// Minimal trait the facade requires from the underlying coordinator.
/// `ConsensusCoordinator` (in `spacekit-compute-node`) implements this.
///
/// This indirection is what keeps `spacekit-unified-consensus` from
/// depending on the whole compute-node crate. It also makes the facade
/// testable against mock coordinators.
pub trait CoordinatorHandle: Send + Sync {
    /// Enumerate currently-eligible validators by DID hash + stake.
    /// Stake is `u128` to match the coordinator's existing types.
    fn eligible_validators(&self) -> Vec<(B256, u128)>;

    /// Submit a vote on behalf of a validator. Returns whether the vote
    /// was accepted by the coordinator (signature valid, validator known,
    /// not already voted in this round, etc).
    fn submit_vote_raw(
        &mut self,
        validator_did: B256,
        block_hash: B256,
        support: bool,
    ) -> Result<bool, alloc::string::String>;

    /// Count of validators voting `support=true` for a given block in the
    /// current round. Used for count-based quorum.
    fn supporting_vote_count(&self, block_hash: &B256) -> u64;

    /// Total eligible validator count for the current round.
    fn eligible_validator_count(&self) -> u64;

    /// True if the given block has been finalized at the coordinator level
    /// (PBFT 2/3 quorum reached).
    fn is_soft_finalized(&self, block_hash: &B256) -> bool;

    /// DID hashes of validators that voted `support=true` for this block.
    fn supporting_validators(&self, block_hash: &B256) -> Vec<B256>;
}

/// Result of a vote-collection round at the facade level.
#[derive(Debug, Clone)]
pub struct WeightedVotingResult {
    pub block_hash: B256,
    /// Per-validator records observed this round (eligible validators
    /// with their effective_voting_power at round start).
    pub eligible_validators: Vec<UnifiedConsensusValidator>,
    /// Sum of effective_voting_power across all eligible validators.
    pub total_voting_power: f64,
    /// Sum of effective_voting_power across validators that voted `support`.
    pub supporting_power: f64,
    /// Count of supporting votes (used when `use_weighted_threshold = false`).
    pub supporting_count: u64,
    /// Count of eligible validators (denominator for count-based threshold).
    pub eligible_count: u64,
    /// True if the coordinator considers this block soft-finalized.
    pub coordinator_finalized: bool,
}

impl WeightedVotingResult {
    /// Participation rate: supporting_count / eligible_count.
    pub fn participation_rate(&self) -> f64 {
        if self.eligible_count == 0 {
            return 0.0;
        }
        self.supporting_count as f64 / self.eligible_count as f64
    }

    /// Supporting power ratio: supporting_power / total_voting_power.
    pub fn supporting_power_ratio(&self) -> f64 {
        if self.total_voting_power == 0.0 {
            return 0.0;
        }
        self.supporting_power / self.total_voting_power
    }
}

impl<C: CoordinatorHandle> ReputationWeightedConsensus<C> {
    /// Construct with a custom reputation source.
    pub fn new(
        coordinator: C,
        reputation: alloc::boxed::Box<dyn ReputationSource>,
        config: FacadeConfig,
    ) -> Result<Self, FacadeError> {
        if config.use_weighted_threshold && !reputation.is_authoritative() {
            return Err(FacadeError::InvalidConfiguration(
                "use_weighted_threshold=true requires an authoritative ReputationSource".into(),
            ));
        }
        Ok(Self {
            coordinator,
            reputation,
            config,
        })
    }

    /// Construct with equal-weight reputation. Matches `ConsensusCoordinator`'s
    /// current behavior; suitable for testnet and pre-fork deployments.
    pub fn new_equal_weight(coordinator: C) -> Self {
        Self {
            coordinator,
            reputation: alloc::boxed::Box::new(EqualWeightReputation),
            config: FacadeConfig::default(),
        }
    }

    /// Borrow the underlying coordinator. Used when callers need to access
    /// coordinator-specific functionality the facade doesn't expose.
    pub fn coordinator(&self) -> &C {
        &self.coordinator
    }
    pub fn coordinator_mut(&mut self) -> &mut C {
        &mut self.coordinator
    }

    /// Borrow the reputation source.
    pub fn reputation_source(&self) -> &dyn ReputationSource {
        &*self.reputation
    }

    /// Build the per-round validator view by querying the coordinator and
    /// applying reputation.
    pub fn build_validator_view(&self) -> Vec<UnifiedConsensusValidator> {
        self.coordinator
            .eligible_validators()
            .into_iter()
            .map(|(did_hash, stake)| {
                let reputation = self.reputation.reputation_of(&did_hash).unwrap_or(1.0);
                let performance = 1.0; // TODO: pull from per-validator performance tracker
                UnifiedConsensusValidator::new(did_hash, stake, reputation, performance)
            })
            .collect()
    }

    /// Submit a vote through the facade. Delegates to the coordinator.
    /// The facade does not re-validate the vote; that's the coordinator's
    /// responsibility (it has the keys, signatures, replay protection).
    pub fn submit_vote(
        &mut self,
        validator_did: B256,
        block_hash: B256,
        support: bool,
    ) -> Result<bool, FacadeError> {
        self.coordinator
            .submit_vote_raw(validator_did, block_hash, support)
            .map_err(FacadeError::CoordinatorError)
    }

    /// Aggregate the current vote tally for a block. Returns a
    /// `WeightedVotingResult` reflecting both the count-based view (today's
    /// authoritative threshold) and the weighted view (observable today,
    /// authoritative post-fork).
    pub fn collect_weighted_votes(&self, block_hash: B256) -> WeightedVotingResult {
        let validators = self.build_validator_view();
        let total_voting_power: f64 = validators
            .iter()
            .filter(|v| v.is_eligible())
            .map(|v| v.effective_voting_power)
            .sum();
        let supporting_count = self.coordinator.supporting_vote_count(&block_hash);
        let eligible_count = self.coordinator.eligible_validator_count();

        let power_by_did: alloc::collections::BTreeMap<B256, f64> = validators
            .iter()
            .filter(|v| v.is_eligible())
            .map(|v| (v.did_hash, v.effective_voting_power))
            .collect();
        let supporting_power: f64 = self
            .coordinator
            .supporting_validators(&block_hash)
            .iter()
            .filter_map(|did| power_by_did.get(did))
            .sum();

        WeightedVotingResult {
            block_hash,
            eligible_validators: validators,
            total_voting_power,
            supporting_power,
            supporting_count,
            eligible_count,
            coordinator_finalized: self.coordinator.is_soft_finalized(&block_hash),
        }
    }

    /// Has quorum been reached for this block?
    ///
    /// Returns `Ok(())` if quorum is reached per the configured threshold.
    /// Returns `Err(QuorumNotReached { ... })` with observed and required
    /// ratios otherwise.
    pub fn has_consensus(&self, voting: &WeightedVotingResult) -> Result<(), FacadeError> {
        // Always require minimum participation, regardless of threshold mode.
        let participation = voting.participation_rate();
        if participation < self.config.min_participation_rate {
            return Err(FacadeError::QuorumNotReached {
                observed: participation,
                required: self.config.min_participation_rate,
            });
        }

        if self.config.use_weighted_threshold {
            // Authoritative path (post-fork).
            let ratio = voting.supporting_power_ratio();
            if ratio < self.config.quorum_threshold {
                return Err(FacadeError::QuorumNotReached {
                    observed: ratio,
                    required: self.config.quorum_threshold,
                });
            }
            Ok(())
        } else {
            // Count-based path: defer to the coordinator's authoritative answer.
            // The facade's threshold check matches the coordinator's, so this
            // returns Ok if the coordinator considers the block finalized.
            if voting.coordinator_finalized {
                Ok(())
            } else {
                Err(FacadeError::QuorumNotReached {
                    observed: participation,
                    required: self.config.quorum_threshold,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reputation_hook::CachedReputationMap;
    use crate::tests_support::MockCoordinator;
    use alloc::vec;

    fn three_validators() -> Vec<(B256, u128)> {
        vec![
            (B256::from([1; 32]), 1_000_000),
            (B256::from([2; 32]), 1_000_000),
            (B256::from([3; 32]), 1_000_000),
        ]
    }

    #[test]
    fn equal_weight_facade_constructs() {
        let coord = MockCoordinator::new(three_validators());
        let facade = ReputationWeightedConsensus::new_equal_weight(coord);
        let view = facade.build_validator_view();
        assert_eq!(view.len(), 3);
        for v in &view {
            assert_eq!(v.reputation_score, 1.0);
            assert_eq!(v.performance_score, 1.0);
            assert!(v.effective_voting_power > 0.0);
        }
    }

    #[test]
    fn weighted_threshold_requires_authoritative_source() {
        let coord = MockCoordinator::new(three_validators());
        let mut config = FacadeConfig::default();
        config.use_weighted_threshold = true;
        let result = ReputationWeightedConsensus::new(
            coord,
            alloc::boxed::Box::new(EqualWeightReputation),
            config,
        );
        assert!(matches!(result, Err(FacadeError::InvalidConfiguration(_))));
    }

    #[test]
    fn weighted_threshold_accepts_authoritative_source() {
        let coord = MockCoordinator::new(three_validators());
        let mut config = FacadeConfig::default();
        config.use_weighted_threshold = true;
        let result = ReputationWeightedConsensus::new(
            coord,
            alloc::boxed::Box::new(CachedReputationMap::new_authoritative()),
            config,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn vote_submission_passes_through_to_coordinator() {
        let coord = MockCoordinator::new(three_validators());
        let mut facade = ReputationWeightedConsensus::new_equal_weight(coord);
        let block = B256::from([0xAA; 32]);
        let v1 = B256::from([1; 32]);
        let accepted = facade.submit_vote(v1, block, true).unwrap();
        assert!(accepted);
        assert_eq!(facade.coordinator().supporting_vote_count(&block), 1);
    }

    #[test]
    fn consensus_check_defers_to_coordinator_in_count_mode() {
        let mut coord = MockCoordinator::new(three_validators());
        let block = B256::from([0xAA; 32]);
        for did in [
            B256::from([1; 32]),
            B256::from([2; 32]),
            B256::from([3; 32]),
        ] {
            coord.submit_vote_raw(did, block, true).unwrap();
        }
        coord.mark_finalized(block);
        let facade = ReputationWeightedConsensus::new_equal_weight(coord);
        let voting = facade.collect_weighted_votes(block);
        assert!(facade.has_consensus(&voting).is_ok());
    }

    #[test]
    fn consensus_check_fails_below_participation_threshold() {
        let mut coord = MockCoordinator::new(three_validators());
        let block = B256::from([0xAA; 32]);
        coord
            .submit_vote_raw(B256::from([1; 32]), block, true)
            .unwrap();
        let facade = ReputationWeightedConsensus::new_equal_weight(coord);
        let voting = facade.collect_weighted_votes(block);
        let result = facade.has_consensus(&voting);
        assert!(matches!(result, Err(FacadeError::QuorumNotReached { .. })));
    }

    #[test]
    fn supporting_power_sums_actual_supporters_not_count_approximation() {
        let mut rep_map = CachedReputationMap::new_authoritative();
        rep_map.set(B256::from([1; 32]), 1.0);
        rep_map.set(B256::from([2; 32]), 1.0);
        rep_map.set(B256::from([3; 32]), 0.5);

        let mut coord = MockCoordinator::new(three_validators());
        let block = B256::from([0xAA; 32]);
        coord
            .submit_vote_raw(B256::from([3; 32]), block, true)
            .unwrap();

        let facade = ReputationWeightedConsensus::new(
            coord,
            alloc::boxed::Box::new(rep_map),
            FacadeConfig::default(),
        )
        .unwrap();

        let voting = facade.collect_weighted_votes(block);
        let v3 = voting
            .eligible_validators
            .iter()
            .find(|v| v.did_hash == B256::from([3; 32]))
            .unwrap();

        assert!(
            (voting.supporting_power - v3.effective_voting_power).abs() < 1e-6,
            "supporting_power={} v3={}",
            voting.supporting_power,
            v3.effective_voting_power
        );
        let approx = (voting.supporting_count as f64 / voting.eligible_count as f64)
            * voting.total_voting_power;
        assert!(
            (voting.supporting_power - approx).abs() > 1.0,
            "should not use count approximation when reputations differ"
        );
    }

    #[test]
    fn weighted_voting_uses_reputation_in_power_calc() {
        let mut rep_map = CachedReputationMap::new_authoritative();
        rep_map.set(B256::from([1; 32]), 1.0);
        rep_map.set(B256::from([2; 32]), 1.0);
        rep_map.set(B256::from([3; 32]), 0.5);

        let coord = MockCoordinator::new(three_validators());
        let mut config = FacadeConfig::default();
        config.use_weighted_threshold = true;
        let facade =
            ReputationWeightedConsensus::new(coord, alloc::boxed::Box::new(rep_map), config)
                .unwrap();

        let view = facade.build_validator_view();
        let v1_power = view[0].effective_voting_power;
        let v3_power = view[2].effective_voting_power;
        assert!(
            (v3_power / v1_power - 0.5).abs() < 1e-6,
            "v1={} v3={}",
            v1_power,
            v3_power
        );
    }
}
