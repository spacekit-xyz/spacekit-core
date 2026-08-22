//! The `SpacetimeExtension` — optional reference extension for
//! [`ReputationWeightedConsensus`]. Hold one alongside the BFT coordinator;
//! the consensus loop is **augmented** (not replaced) with rotor-valued
//! transitions and causal-set ordering. PBFT quorum remains authoritative;
//! prefer `aggregate_votes_robust` (geometric median) at commit time.

use crate::aggregation::{aggregate_rotors, AggregationError, FrechetMeanConfig};
use crate::algebra::Multivector;
use crate::causal::{CausalCoord, CausalEvent, CausalSet};
use crate::defense::{
    geometric_median_rotor, DefenseError, FingerprintRegistry, GeometricMedianConfig,
};
use crate::proposal::{SpacetimeTransition, TransitionWitness};
use crate::rotor::{Bivector, Rotor, RotorError};
use alloc::vec::Vec;
use alloy_primitives::B256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpacetimeError {
    InvalidRotor(RotorError),
    AggregationFailed(AggregationError),
    TransitionMismatch,
    CausalViolation,
    SerializationError,
    UnsupportedWireVersion,
    RobustAggregationFailed(DefenseError),
}

impl From<RotorError> for SpacetimeError {
    fn from(e: RotorError) -> Self {
        Self::InvalidRotor(e)
    }
}
impl From<AggregationError> for SpacetimeError {
    fn from(e: AggregationError) -> Self {
        Self::AggregationFailed(e)
    }
}

/// The aggregated rotor that finalizes a block. Returned by
/// `SpacetimeExtension::aggregate_votes` and stored alongside the
/// `FinalizedBlock`.
#[derive(Debug, Clone, Copy)]
pub struct ConsensusRotor {
    pub rotor: Rotor,
    pub contributing_validators: u32,
    pub total_weight: f64,
    pub max_divergence: f64,
}

/// The extension struct. Hold one of these inside your `ReputationWeightedConsensus`.
pub struct SpacetimeExtension {
    pub origin: CausalCoord,
    pub causal_set: CausalSet,
    pub mean_config: FrechetMeanConfig,
    pub divergence_threshold: f64,
    /// Per-validator behavioral fingerprints (v2 joint rotor + residual_norm).
    pub fingerprints: FingerprintRegistry,
    /// Weiszfeld geometric median (Byzantine threshold > 1/3).
    pub median_config: GeometricMedianConfig,
}

impl Default for SpacetimeExtension {
    fn default() -> Self {
        Self {
            origin: CausalCoord::ORIGIN,
            causal_set: CausalSet::new(),
            mean_config: FrechetMeanConfig::default(),
            divergence_threshold: 0.5,
            fingerprints: FingerprintRegistry::new(0.95),
            median_config: GeometricMedianConfig::default(),
        }
    }
}

impl SpacetimeExtension {
    pub fn new(origin: CausalCoord) -> Self {
        Self {
            origin,
            ..Self::default()
        }
    }

    pub fn compute_transition<F: Fn(&[u8]) -> [u8; 32]>(
        &self,
        transition_id: u64,
        prev_state: &Multivector,
        new_state: &Multivector,
        prev_state_hash: B256,
        new_state_hash: B256,
        proposer_coord: CausalCoord,
        aux_commit: Option<B256>,
        hash_fn: F,
    ) -> Result<SpacetimeTransition, SpacetimeError> {
        let rotor = self.fit_rotor(prev_state, new_state)?;
        let rotor_part = rotor.apply(prev_state);
        let residual = *new_state - rotor_part;
        let residual_commitment = SpacetimeTransition::commit_residual(&residual, &hash_fn);
        let residual_norm = SpacetimeTransition::compute_residual_norm(&residual);
        Ok(SpacetimeTransition {
            transition_id,
            rotor,
            prev_state_hash,
            new_state_hash,
            causal_coord: proposer_coord,
            residual_commitment,
            residual_norm,
            aux_commit,
        })
    }

    pub fn verify_transition<F: Fn(&[u8]) -> [u8; 32]>(
        &self,
        transition: &SpacetimeTransition,
        prev_state: &Multivector,
        new_state: &Multivector,
        prev_coord: &CausalCoord,
        state_tolerance: f64,
        hash_fn: F,
    ) -> Result<(), SpacetimeError> {
        let _ = Rotor::from_multivector(*transition.rotor.as_multivector())?;

        let rotor_part = transition.rotor.apply(prev_state);
        let computed_residual = *new_state - rotor_part;
        let computed_commit = SpacetimeTransition::commit_residual(&computed_residual, &hash_fn);
        if computed_commit != transition.residual_commitment {
            return Err(SpacetimeError::TransitionMismatch);
        }
        let computed_norm = SpacetimeTransition::compute_residual_norm(&computed_residual);
        if (computed_norm - transition.residual_norm).abs() > state_tolerance {
            return Err(SpacetimeError::TransitionMismatch);
        }
        let reconstructed = rotor_part + computed_residual;
        let mut diff_sq = 0.0;
        for i in 0..crate::algebra::BASIS_DIM {
            let d = reconstructed.coeffs[i] - new_state.coeffs[i];
            diff_sq += d * d;
        }
        #[cfg(feature = "std")]
        let diff = diff_sq.sqrt();
        #[cfg(not(feature = "std"))]
        let diff = libm::sqrt(diff_sq);
        if diff > state_tolerance {
            return Err(SpacetimeError::TransitionMismatch);
        }

        let dt = transition.causal_coord.t - prev_coord.t;
        let dx = transition.causal_coord.x - prev_coord.x;
        let dy = transition.causal_coord.y - prev_coord.y;
        let dz = transition.causal_coord.z - prev_coord.z;
        if dt <= 0.0 {
            return Err(SpacetimeError::CausalViolation);
        }
        let interval = dt * dt - dx * dx - dy * dy - dz * dz;
        if interval < -1e-9 {
            return Err(SpacetimeError::CausalViolation);
        }

        Ok(())
    }

    pub fn aggregate_votes(
        &self,
        validator_rotors: &[(Rotor, f64)],
    ) -> Result<ConsensusRotor, SpacetimeError> {
        let mean = aggregate_rotors(validator_rotors, &self.mean_config)?;
        Self::consensus_rotor_from_aggregate(mean, validator_rotors)
    }

    /// Byzantine-resilient aggregation (breakdown point 1/2). Prefer this at commit time.
    pub fn aggregate_votes_robust(
        &self,
        validator_rotors: &[(Rotor, f64)],
    ) -> Result<ConsensusRotor, SpacetimeError> {
        let median = geometric_median_rotor(validator_rotors, &self.median_config)
            .map_err(SpacetimeError::RobustAggregationFailed)?;
        Self::consensus_rotor_from_aggregate(median, validator_rotors)
    }

    /// Observe a v2 transition in the in-memory fingerprint registry.
    pub fn observe_transition(&mut self, did_hash: B256, transition: &SpacetimeTransition) -> f64 {
        self.fingerprints
            .observe_joint(did_hash, transition.rotor, transition.residual_norm)
    }

    fn consensus_rotor_from_aggregate(
        rotor: Rotor,
        validator_rotors: &[(Rotor, f64)],
    ) -> Result<ConsensusRotor, SpacetimeError> {
        let mut max_div = 0.0;
        for (r, _) in validator_rotors {
            let d = rotor.distance(r);
            if d > max_div {
                max_div = d;
            }
        }
        let total_weight: f64 = validator_rotors.iter().map(|(_, w)| w).sum();
        Ok(ConsensusRotor {
            rotor,
            contributing_validators: validator_rotors.len() as u32,
            total_weight,
            max_divergence: max_div,
        })
    }

    pub fn record_event(&mut self, content: B256, coord: CausalCoord) {
        self.causal_set.push(CausalEvent { content, coord });
    }

    pub fn find_divergent_validators(
        &self,
        consensus: &ConsensusRotor,
        validator_rotors: &[(Rotor, f64)],
    ) -> Vec<usize> {
        validator_rotors
            .iter()
            .enumerate()
            .filter(|(_, (r, _))| consensus.rotor.distance(r) > self.divergence_threshold)
            .map(|(i, _)| i)
            .collect()
    }

    fn fit_rotor(&self, prev: &Multivector, new: &Multivector) -> Result<Rotor, SpacetimeError> {
        let prev_rev = prev.reverse();
        let prev_norm_sq = (*prev * prev_rev).coeffs[0];
        if prev_norm_sq.abs() < 1e-12 {
            return Ok(Rotor::IDENTITY);
        }
        let delta = (*new - *prev) * (prev_rev * (1.0 / prev_norm_sq));
        let mut b = Bivector::ZERO;
        for i in 0..6 {
            b.b[i] = delta.coeffs[5 + i];
        }
        Ok(Rotor::exp(&b.scale(0.5)))
    }
}

pub fn extract_validator_rotors(
    witnesses: &[TransitionWitness],
    voting_powers: &[(B256, f64)],
) -> Vec<(Rotor, f64)> {
    witnesses
        .iter()
        .filter_map(|w| {
            let power = voting_powers
                .iter()
                .find(|(h, _)| *h == w.proposal_hash)
                .map(|(_, p)| *p)?;
            Some((w.transition.rotor, power))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(b: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, byte) in b.iter().enumerate() {
            out[i % 32] = out[i % 32].wrapping_add(byte.wrapping_mul(31));
        }
        out
    }

    #[test]
    fn verify_passes_for_identity_transition() {
        let ext = SpacetimeExtension::default();
        let prev = Multivector::ONE;
        let new = Multivector::ONE;
        let zero = Multivector::ZERO;
        let t = SpacetimeTransition {
            transition_id: 1,
            rotor: Rotor::IDENTITY,
            prev_state_hash: B256::ZERO,
            new_state_hash: B256::ZERO,
            causal_coord: CausalCoord {
                t: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment: SpacetimeTransition::commit_residual(&zero, h),
            residual_norm: 0.0,
            aux_commit: None,
        };
        let res = ext.verify_transition(&t, &prev, &new, &CausalCoord::ORIGIN, 1e-6, h);
        assert!(res.is_ok(), "expected ok, got {:?}", res);
    }

    #[test]
    fn causal_violation_detected() {
        let ext = SpacetimeExtension::default();
        let prev = Multivector::ONE;
        let new = Multivector::ONE;
        let zero = Multivector::ZERO;
        let t = SpacetimeTransition {
            transition_id: 1,
            rotor: Rotor::IDENTITY,
            prev_state_hash: B256::ZERO,
            new_state_hash: B256::ZERO,
            causal_coord: CausalCoord {
                t: -1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment: SpacetimeTransition::commit_residual(&zero, h),
            residual_norm: 0.0,
            aux_commit: None,
        };
        let res = ext.verify_transition(&t, &prev, &new, &CausalCoord::ORIGIN, 1e-9, h);
        assert_eq!(res, Err(SpacetimeError::CausalViolation));
    }

    #[test]
    fn forged_residual_commitment_detected() {
        let ext = SpacetimeExtension::default();
        let prev = Multivector::ONE;
        let new = Multivector::ONE;
        let t = SpacetimeTransition {
            transition_id: 1,
            rotor: Rotor::IDENTITY,
            prev_state_hash: B256::ZERO,
            new_state_hash: B256::ZERO,
            causal_coord: CausalCoord {
                t: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment: B256::from([0xDE; 32]),
            residual_norm: 0.0,
            aux_commit: None,
        };
        let res = ext.verify_transition(&t, &prev, &new, &CausalCoord::ORIGIN, 1e-9, h);
        assert_eq!(res, Err(SpacetimeError::TransitionMismatch));
    }
}
