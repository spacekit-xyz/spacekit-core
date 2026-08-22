//! Fréchet mean (a.k.a. Karcher mean) of rotors on Spin⁺(1,3).
//!
//! Given rotors R₁, …, Rₙ with reputation-derived weights w₁, …, wₙ, the
//! weighted Fréchet mean R* minimizes
//!
//!     Σᵢ wᵢ · d(R*, Rᵢ)²
//!
//! where d is the geodesic distance on Spin⁺(1,3). The minimizer satisfies
//! the first-order condition
//!
//!     Σᵢ wᵢ · log(R*⁻¹ Rᵢ) = 0
//!
//! which gives the iterative update
//!
//!     R* ← R* · exp( (1/W) · Σᵢ wᵢ · log(R*⁻¹ Rᵢ) )      with W = Σ wᵢ
//!
//! For rotors close to the identity (e.g., consensus on small state deltas
//! between adjacent blocks), this converges quadratically in 3–6 iterations.
//!
//! In `collect_weighted_votes`,
//! each validator currently emits a yes/no vote on the proposer's hash. With
//! the spacetime extension, each validator *also* attaches their own computed
//! transition rotor `Rᵢ`. The consensus rotor `R*` is the Fréchet mean of all
//! `Rᵢ` from validators voting YES, weighted by `effective_voting_power`.
//! The block is finalized with `R*`, not the proposer's bare claim — this
//! adds a self-correction layer if the proposer is honest-but-faulty.

use crate::rotor::{Bivector, Rotor};

#[derive(Debug, Clone, Copy)]
pub struct FrechetMeanConfig {
    /// Maximum iterations before bailing out.
    pub max_iters: usize,
    /// Convergence threshold on the size of the tangent update.
    pub tolerance: f64,
    /// If true, fall back to the highest-weighted rotor on non-convergence
    /// rather than returning the partial result. Recommended for production.
    pub safe_fallback: bool,
}

impl Default for FrechetMeanConfig {
    fn default() -> Self {
        Self {
            max_iters: 16,
            tolerance: 1e-9,
            safe_fallback: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AggregationError {
    EmptyInput,
    NonPositiveTotalWeight,
    DidNotConverge,
}

/// Compute the weighted Fréchet mean of rotors. Inputs are `(rotor, weight)`
/// pairs; weights are reputation × stake × performance factors from the
/// existing `UnifiedConsensusValidator::effective_voting_power`.
pub fn aggregate_rotors(
    rotors_with_weights: &[(Rotor, f64)],
    config: &FrechetMeanConfig,
) -> Result<Rotor, AggregationError> {
    if rotors_with_weights.is_empty() {
        return Err(AggregationError::EmptyInput);
    }

    let total_weight: f64 = rotors_with_weights.iter().map(|(_, w)| w).sum();
    if total_weight <= 0.0 {
        return Err(AggregationError::NonPositiveTotalWeight);
    }

    // Initialize with the highest-weighted rotor. Other choices (e.g. the
    // proposer's rotor) are valid; the Fréchet mean is independent of init
    // for sufficiently nearby inputs.
    let mut best_idx = 0usize;
    let mut best_w = f64::NEG_INFINITY;
    for (i, (_, w)) in rotors_with_weights.iter().enumerate() {
        if *w > best_w {
            best_w = *w;
            best_idx = i;
        }
    }
    let mut mean = rotors_with_weights[best_idx].0;

    for _iter in 0..config.max_iters {
        // Sum weighted tangent vectors at the current mean.
        let mean_inv = mean.reverse();
        let mut sum = Bivector::ZERO;
        for (r, w) in rotors_with_weights {
            let rel = mean_inv.compose(r);
            let log_rel = match rel.log() {
                Ok(b) => b,
                Err(_) => continue, // skip pathological inputs; weight effectively zero
            };
            sum = sum.add(&log_rel.scale(*w));
        }
        let step = sum.scale(1.0 / total_weight);

        // Convergence check on step magnitude.
        let step_norm = (step.square_scalar().abs()).sqrt();
        // Apply the update.
        mean = mean.compose(&Rotor::exp(&step));

        if step_norm < config.tolerance {
            return Ok(mean);
        }
    }

    if config.safe_fallback {
        Ok(rotors_with_weights[best_idx].0)
    } else {
        Err(AggregationError::DidNotConverge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_of_identical_rotors_is_that_rotor() {
        let b = Bivector {
            b: [0.0, 0.0, 0.0, 0.3, 0.0, 0.0],
        };
        let r = Rotor::exp(&b);
        let inputs = [(r, 1.0), (r, 2.0), (r, 0.5)];
        let mean = aggregate_rotors(&inputs, &FrechetMeanConfig::default()).unwrap();
        assert!(r.distance(&mean) < 1e-8);
    }

    #[test]
    fn mean_lies_between_inputs() {
        let b1 = Bivector {
            b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
        };
        let b2 = Bivector {
            b: [0.0, 0.0, 0.0, 0.3, 0.0, 0.0],
        };
        let r1 = Rotor::exp(&b1);
        let r2 = Rotor::exp(&b2);
        let mean =
            aggregate_rotors(&[(r1, 1.0), (r2, 1.0)], &FrechetMeanConfig::default()).unwrap();
        // For equal weights, expect the mean to roughly correspond to (b1+b2)/2.
        let b_mid = Bivector {
            b: [0.0, 0.0, 0.0, 0.2, 0.0, 0.0],
        };
        let r_mid = Rotor::exp(&b_mid);
        assert!(
            r_mid.distance(&mean) < 1e-6,
            "distance {}",
            r_mid.distance(&mean)
        );
    }

    #[test]
    fn empty_input_errors() {
        let r: [(Rotor, f64); 0] = [];
        let err = aggregate_rotors(&r, &FrechetMeanConfig::default());
        assert_eq!(err, Err(AggregationError::EmptyInput));
    }
}
