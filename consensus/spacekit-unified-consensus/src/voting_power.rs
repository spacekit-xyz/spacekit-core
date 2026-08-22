//! Effective voting power calculation.
//!
//! Combines stake and reputation into a single `effective_voting_power`
//! float per validator. Today this is consulted but does NOT affect the
//! quorum threshold (which remains count-based at `ConsensusCoordinator`'s
//! 2/3 of registered validators).
//!
//! When the threshold check moves to reputation-weighted (post-fork), this
//! function's output is the weight that goes into the sum.
//!
//! ## The calculation
//!
//! `effective_voting_power = sqrt(stake) * reputation * performance_multiplier`
//!
//! - `sqrt(stake)` — square root reduces whale influence; doubling stake
//!   gives ~41% more weight, not 100% more.
//! - `reputation` — \[0, 1\] from the `ReputationSource`. Defaults to 1.0
//!   for `EqualWeightReputation`.
//! - `performance_multiplier` — captured separately because performance
//!   is observable in real-time but reputation lags. Today equals 1.0;
//!   the field exists so the post-fork weighting has a place to land.
//!
//! All three factors are independent; the formula multiplies rather than
//! adds so weight degrades gracefully when any one factor is low.

#[cfg(not(feature = "std"))]
use libm;
#[cfg(feature = "std")]
use std::f64;

/// Compute effective voting power from primitives.
///
/// Today this is observable. After the reputation-fork, this becomes
/// authoritative — the value here goes into the quorum threshold sum.
pub fn effective_voting_power(stake: u128, reputation: f64, performance: f64) -> f64 {
    let base = sqrt_u128(stake);
    let r = reputation.clamp(0.0, 1.0);
    let p = performance.clamp(0.0, 1.0);
    base * r * p
}

/// Convenience: equal-weight stake-only power. Used when no reputation
/// source is configured. Matches the testnet equal-weight model.
pub fn equal_weight_power(stake: u128) -> f64 {
    sqrt_u128(stake)
}

fn sqrt_u128(x: u128) -> f64 {
    let f = x as f64;
    #[cfg(feature = "std")]
    {
        f.sqrt()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::sqrt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_stake_equal_reputation_produces_equal_power() {
        let a = effective_voting_power(1_000_000, 1.0, 1.0);
        let b = effective_voting_power(1_000_000, 1.0, 1.0);
        assert_eq!(a, b);
    }

    #[test]
    fn doubling_stake_does_not_double_power() {
        let single = effective_voting_power(1_000_000, 1.0, 1.0);
        let double = effective_voting_power(2_000_000, 1.0, 1.0);
        // sqrt(2) ≈ 1.414, so doubling stake gives ~41% more power
        let ratio = double / single;
        assert!(ratio < 1.5, "ratio was {}", ratio);
        assert!(ratio > 1.4, "ratio was {}", ratio);
    }

    #[test]
    fn low_reputation_attenuates_power() {
        let full = effective_voting_power(1_000_000, 1.0, 1.0);
        let half = effective_voting_power(1_000_000, 0.5, 1.0);
        assert!((half / full - 0.5).abs() < 1e-9);
    }

    #[test]
    fn zero_reputation_zeroes_power() {
        assert_eq!(effective_voting_power(1_000_000, 0.0, 1.0), 0.0);
    }

    #[test]
    fn reputation_clamps_to_unit_interval() {
        let over = effective_voting_power(1_000_000, 1.5, 1.0);
        let exact = effective_voting_power(1_000_000, 1.0, 1.0);
        assert_eq!(over, exact);
    }

    #[test]
    fn equal_weight_power_matches_full_calc_at_unit_reputation() {
        let eq = equal_weight_power(2_500_000);
        let full = effective_voting_power(2_500_000, 1.0, 1.0);
        assert_eq!(eq, full);
    }
}
