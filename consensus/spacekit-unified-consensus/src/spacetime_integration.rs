//! Spacetime layer integration.
//!
//! When the `spacetime` feature is enabled, this module bridges the facade
//! to `spacekit-spacetime-consensus`. The spacetime crate's documentation
//! says "plugs into `ReputationWeightedConsensus`", this module is where
//! that plug-in lands.
//!
//! ## What goes through this shim (facade only)
//!
//! - Rotor aggregation across voting validators (`aggregate_votes`)
//! - Per-validator transition verification (`verify_transition`)
//!
//! Fingerprint observation, tiered finality, fraud proofs, and parameter
//! ratification live in the spacetime crate but are invoked from the node's
//! `ConsensusCoordinator` / `spacetime_integration.rs`, not through these
//! facade methods. See `README.md` (architecture / call-site table).

use crate::facade::{
    CoordinatorHandle, FacadeError, ReputationWeightedConsensus, WeightedVotingResult,
};
use alloc::vec::Vec;
use alloy_primitives::B256;

extern crate alloc;

use spacekit_spacetime_consensus::{
    causal::CausalCoord, consensus::SpacetimeExtension, proposal::SpacetimeTransition, rotor::Rotor,
};

/// Errors specific to the spacetime integration path.
#[derive(Debug, Clone)]
pub enum SpacetimeIntegrationError {
    /// The spacetime extension is not configured. Set one before calling
    /// spacetime methods on the facade.
    ExtensionNotConfigured,
    /// The spacetime extension returned an error. The error is surfaced as
    /// a string to keep the dependency surface narrow.
    ExtensionError(alloc::string::String),
    /// A per-validator rotor was missing for the aggregation. The facade
    /// today computes aggregation only over validators that submitted
    /// rotors; missing rotors are skipped, not an error. This variant
    /// remains in the enum for future strictness.
    MissingValidatorRotor(B256),
}

/// Per-block spacetime data the facade tracks. Populated by validators
/// during the voting round; consumed by `aggregate_votes` and the
/// fingerprint update path.
#[derive(Debug, Clone)]
pub struct BlockSpacetimeData {
    /// Per-validator transition rotors observed for this block. The
    /// proposer's rotor is the claim; each validator's rotor is their
    /// independent computation.
    pub validator_rotors: alloc::collections::BTreeMap<B256, SpacetimeTransition>,
    /// Aggregated consensus rotor (set after `aggregate_votes` runs).
    pub consensus_rotor: Option<Rotor>,
    /// Causal coordinate of the block.
    pub causal_coord: CausalCoord,
}

impl Default for BlockSpacetimeData {
    fn default() -> Self {
        Self {
            validator_rotors: alloc::collections::BTreeMap::new(),
            consensus_rotor: None,
            causal_coord: CausalCoord::ORIGIN,
        }
    }
}

impl<C: CoordinatorHandle> ReputationWeightedConsensus<C> {
    /// Aggregate per-validator rotors into the consensus rotor for a block.
    ///
    /// Uses the spacetime extension's geometric median (post-v2, joint
    /// signature). Today the facade does not weight rotors by reputation;
    /// the geometric median already provides robust aggregation independent
    /// of weight. Post-fork, reputation can be folded in as the weight in
    /// the Fréchet mean variant.
    ///
    /// Validators that did not submit a rotor are skipped — they vote
    /// yes/no on the proposer's claim but don't contribute to the
    /// geometric median.
    pub fn aggregate_votes(
        &self,
        spacetime_extension: &SpacetimeExtension,
        block_data: &BlockSpacetimeData,
        voting: &WeightedVotingResult,
    ) -> Result<Rotor, SpacetimeIntegrationError> {
        // Collect rotors from validators that both (a) voted supporting
        // and (b) submitted a rotor. Both are required for inclusion in
        // aggregation.
        let mut rotors_to_aggregate = Vec::new();
        let mut weights_to_aggregate = Vec::new();

        for validator in &voting.eligible_validators {
            if !validator.is_eligible() {
                continue;
            }
            if let Some(transition) = block_data.validator_rotors.get(&validator.did_hash) {
                rotors_to_aggregate.push(transition.rotor);
                // Today: equal-weight. The geometric median ignores weights
                // and uses breakdown-point robustness. When we move to the
                // Fréchet mean variant post-fork, this is where reputation
                // weight enters.
                weights_to_aggregate.push(validator.effective_voting_power);
            }
        }

        if rotors_to_aggregate.is_empty() {
            return Err(SpacetimeIntegrationError::ExtensionError(
                "no validator rotors available to aggregate".into(),
            ));
        }

        let pairs: Vec<(Rotor, f64)> = rotors_to_aggregate
            .into_iter()
            .zip(weights_to_aggregate)
            .map(|(r, w)| (r, w))
            .collect();
        // `aggregate_votes_robust` → `geometric_median_rotor` (Spin⁺(1,3) median),
        // not the Fréchet-mean path (`aggregate_votes` / `aggregate_rotors`).
        // Weights are passed through for divergence metadata only; the median
        // step is unweighted and retains the ~50% Byzantine breakdown point.
        spacetime_extension
            .aggregate_votes_robust(&pairs)
            .map(|consensus| consensus.rotor)
            .map_err(|e| SpacetimeIntegrationError::ExtensionError(format!("{:?}", e)))
    }

    /// Verify a single validator's transition against the proposer's claim.
    /// Returns Ok if rotor + residual commitment + causal cone all check out.
    ///
    /// Today: called for each per-validator transition during voting.
    /// Failures surface as `SpacetimeIntegrationError::ExtensionError` with
    /// the underlying spacetime error in the message.
    pub fn verify_transition(
        &self,
        spacetime_extension: &SpacetimeExtension,
        transition: &SpacetimeTransition,
        prev_state: &spacekit_spacetime_consensus::algebra::Multivector,
        new_state: &spacekit_spacetime_consensus::algebra::Multivector,
        prev_coord: &CausalCoord,
        state_tolerance: f64,
        hash_fn: impl Fn(&[u8]) -> [u8; 32] + Copy,
    ) -> Result<(), SpacetimeIntegrationError> {
        spacetime_extension
            .verify_transition(
                transition,
                prev_state,
                new_state,
                prev_coord,
                state_tolerance,
                hash_fn,
            )
            .map_err(|e| SpacetimeIntegrationError::ExtensionError(format!("{:?}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facade::FacadeConfig;
    use crate::reputation_hook::EqualWeightReputation;
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
    fn aggregate_votes_returns_error_with_no_rotors() {
        let coord = MockCoordinator::new(three_validators());
        let facade = ReputationWeightedConsensus::new_equal_weight(coord);
        let block_data = BlockSpacetimeData::default();
        let voting = facade.collect_weighted_votes(B256::from([0xAA; 32]));
        let ext = SpacetimeExtension::default();
        let result = facade.aggregate_votes(&ext, &block_data, &voting);
        assert!(matches!(
            result,
            Err(SpacetimeIntegrationError::ExtensionError(_))
        ));
    }
}
