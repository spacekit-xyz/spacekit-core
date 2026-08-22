//! Reputation hook.
//!
//! The facade reads per-validator reputation through this trait. Today the
//! default implementation returns 1.0 for every validator, equal-weight
//! voting matches `ConsensusCoordinator`'s current behavior.
//!
//! When reputation becomes authoritative (post-fork; see `SPACEKIT_CONSENSUS_UNIFIED.md`
//! §1.4), plug in a `ReputationSource` backed by on-chain reputation or
//! `MLReputationEngine`. The facade picks up the new weights with no other
//! changes.
//!
//! ## What "authoritative" means
//!
//! For reputation-weighted voting to be safe at the consensus layer, every
//! validator must agree on every other validator's reputation at every
//! height. Otherwise validators disagree about whether a quorum was reached.
//! Today reputation is observable but not authoritative, different
//! validators may see slightly different values, and that's fine because
//! the quorum check ignores reputation. Once the threshold check switches
//! to use reputation weights, the source must be deterministic from on-chain
//! data.
//!
//! Implementations of `ReputationSource` should document which mode they
//! operate in. The facade itself does not enforce determinism; that's the
//! implementation's responsibility.

use alloy_primitives::B256;

/// Source of per-validator reputation scores.
///
/// Implementations are free to be approximate, cached, or eventually
/// consistent in the **observable** mode. They MUST be deterministic in
/// the **authoritative** mode (where reputation weights affect the
/// quorum threshold).
pub trait ReputationSource: Send + Sync {
    /// Get the reputation score for a validator's DID hash.
    ///
    /// Range: `[0.0, 1.0]`. A score of 1.0 means full effective weight;
    /// 0.0 means the validator's vote is counted but contributes no power.
    /// Values outside the range MUST be clamped by the implementation.
    ///
    /// Returns `None` if the validator is not known to this source. The
    /// facade treats `None` as 1.0 today (equal-weight default) but a
    /// stricter authoritative source can reject the validator entirely.
    fn reputation_of(&self, validator_did: &B256) -> Option<f64>;

    /// True if this source provides authoritative reputation suitable for
    /// quorum-threshold computation. The facade uses this to decide
    /// whether to apply reputation weighting to the threshold check.
    ///
    /// Default: `false` (observable but not authoritative). Implementations
    /// that produce deterministic reputation from on-chain data should
    /// override to `true`.
    fn is_authoritative(&self) -> bool {
        false
    }
}

/// Default reputation source: every known validator has weight 1.0.
/// Matches `ConsensusCoordinator`'s current equal-weight behavior.
///
/// This is the source the facade installs by default. Use it on testnet
/// and any pre-fork deployment.
#[derive(Debug, Default, Clone, Copy)]
pub struct EqualWeightReputation;

impl ReputationSource for EqualWeightReputation {
    fn reputation_of(&self, _validator_did: &B256) -> Option<f64> {
        Some(1.0)
    }
    fn is_authoritative(&self) -> bool {
        false
    }
}

/// In-memory reputation map. Useful for tests and for nodes that maintain
/// a local cached reputation index (observable, not authoritative).
///
/// To use as an authoritative source, the implementation that *populates*
/// this map must derive values deterministically from on-chain state. The
/// map itself does not enforce that.
#[derive(Debug, Default, Clone)]
pub struct CachedReputationMap {
    entries: alloc::collections::BTreeMap<B256, f64>,
    authoritative: bool,
}

extern crate alloc;

impl CachedReputationMap {
    pub fn new() -> Self {
        Self {
            entries: alloc::collections::BTreeMap::new(),
            authoritative: false,
        }
    }

    pub fn new_authoritative() -> Self {
        Self {
            entries: alloc::collections::BTreeMap::new(),
            authoritative: true,
        }
    }

    pub fn set(&mut self, did: B256, reputation: f64) {
        self.entries.insert(did, reputation.clamp(0.0, 1.0));
    }

    pub fn remove(&mut self, did: &B256) {
        self.entries.remove(did);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl ReputationSource for CachedReputationMap {
    fn reputation_of(&self, validator_did: &B256) -> Option<f64> {
        self.entries.get(validator_did).copied()
    }
    fn is_authoritative(&self) -> bool {
        self.authoritative
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_weight_returns_one_for_any_did() {
        let s = EqualWeightReputation;
        assert_eq!(s.reputation_of(&B256::ZERO), Some(1.0));
        assert_eq!(s.reputation_of(&B256::from([0xAB; 32])), Some(1.0));
        assert!(!s.is_authoritative());
    }

    #[test]
    fn cached_map_clamps_out_of_range() {
        let mut m = CachedReputationMap::new();
        m.set(B256::from([1; 32]), 1.5);
        m.set(B256::from([2; 32]), -0.5);
        assert_eq!(m.reputation_of(&B256::from([1; 32])), Some(1.0));
        assert_eq!(m.reputation_of(&B256::from([2; 32])), Some(0.0));
    }

    #[test]
    fn unknown_validator_returns_none() {
        let m = CachedReputationMap::new();
        assert_eq!(m.reputation_of(&B256::ZERO), None);
    }
}
