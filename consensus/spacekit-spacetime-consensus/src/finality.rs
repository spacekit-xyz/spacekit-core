//! Tiered finality state machine.
//!
//! Two finality stages:
//!
//!   - **Soft**: 2/3 PBFT reputation-weighted quorum reached. The block is
//!     gossiped and accepted by light clients that opted for fast-path UX.
//!   - **Hard**: soft + `challenge_window` blocks elapsed with no valid
//!     fraud proof. The block is now irreversible. High-value transactions
//!     wait for this stage.
//!
//! The state machine is driven by two events:
//!
//!   1. **Block soft-finalized** at height H → start countdown.
//!   2. **Block age advances** (a new block H' > H finalizes) → if
//!      H' - H ≥ challenge_window and no fraud proof against H, mark H hard.
//!
//! A valid fraud proof during the window triggers `rollback_to_height(H-1)`
//! via the rollback hook the coordinator already exposes. Transactions from
//! the rolled-back block return to the mempool (handled outside this crate).
//!
//! ## Why this isn't just "wait N blocks"
//!
//! Wall-clock waiting doesn't protect against attacks that include the
//! waiting period itself in their plan. The challenge window is measured in
//! *successor blocks* finalized by honest validators, because that
//! requires honest activity to keep advancing. A network that stalls during
//! the window also stalls hard finality, which is the correct safety
//! behavior, if no one's around to challenge, no one's around to commit.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloy_primitives::B256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalityStage {
    /// Block has been soft-finalized (2/3 PBFT quorum) but the challenge
    /// window has not yet elapsed.
    Soft,
    /// Challenge window elapsed without successful fraud proof.
    Hard,
    /// A fraud proof was accepted during the window; this block is reverted.
    Reverted,
}

#[derive(Debug, Clone, Copy)]
pub struct PendingBlock {
    pub height: u64,
    pub block_hash: B256,
    pub soft_finalized_at_height: u64,
    pub stage: FinalityStage,
}

#[derive(Debug, Clone, Copy)]
pub struct TieredFinalityConfig {
    /// Number of successor blocks that must finalize before this block
    /// transitions Soft → Hard. Tune per chain.
    ///
    /// Recommended starting point: 100 blocks. With your 2-3s soft-finality
    /// target, that's a ~3-5 minute hard-finality window.
    pub challenge_window: u64,
    /// Maximum number of pending blocks tracked at once. Bounds memory; if
    /// exceeded, oldest pending block is treated as Hard whether or not the
    /// window has fully elapsed. Should be > challenge_window.
    pub max_pending: usize,
}

impl Default for TieredFinalityConfig {
    fn default() -> Self {
        Self {
            challenge_window: 100,
            max_pending: 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalityError {
    UnknownBlock,
    BlockNotInSoftStage,
    BlockAlreadyReverted,
    RollbackBelowGenesis,
}

#[derive(Debug, Clone)]
pub struct TieredFinality {
    pub config: TieredFinalityConfig,
    pending: BTreeMap<u64, PendingBlock>,
    /// Maximum height that has reached Hard finality. Strictly monotonic.
    pub hard_finalized_height: u64,
    /// Current chain tip height (soft-finalized).
    pub current_height: u64,
}

impl TieredFinality {
    pub fn new(config: TieredFinalityConfig, genesis_height: u64) -> Self {
        Self {
            config,
            pending: BTreeMap::new(),
            hard_finalized_height: genesis_height,
            current_height: genesis_height,
        }
    }

    /// Called when a block reaches soft finality via PBFT.
    ///
    /// Returns the list of heights that transitioned Soft → Hard as a
    /// side effect (because the challenge window for those older blocks
    /// has now elapsed).
    pub fn on_soft_finalize(&mut self, height: u64, block_hash: B256) -> Vec<u64> {
        self.current_height = height.max(self.current_height);
        self.pending.insert(
            height,
            PendingBlock {
                height,
                block_hash,
                soft_finalized_at_height: height,
                stage: FinalityStage::Soft,
            },
        );

        let mut transitioned = Vec::new();
        let mut to_remove = Vec::new();

        for (h, pb) in self.pending.iter_mut() {
            if pb.stage != FinalityStage::Soft {
                continue;
            }
            let age = self
                .current_height
                .saturating_sub(pb.soft_finalized_at_height);
            if age >= self.config.challenge_window {
                pb.stage = FinalityStage::Hard;
                transitioned.push(*h);
                if *h > self.hard_finalized_height {
                    self.hard_finalized_height = *h;
                }
                to_remove.push(*h);
            }
        }
        // Reverted and Hard blocks don't need to stay in pending.
        for h in to_remove {
            self.pending.remove(&h);
        }

        // Cap memory: if we have too many pending blocks, force the oldest
        // to Hard. This shouldn't happen under normal operation but bounds
        // worst-case memory if the challenge window is somehow over-tuned.
        while self.pending.len() > self.config.max_pending {
            if let Some((&oldest, _)) = self.pending.iter().next() {
                self.pending.remove(&oldest);
                transitioned.push(oldest);
                if oldest > self.hard_finalized_height {
                    self.hard_finalized_height = oldest;
                }
            } else {
                break;
            }
        }

        transitioned
    }

    /// Status of a specific block.
    pub fn stage_of(&self, height: u64) -> FinalityStage {
        if height <= self.hard_finalized_height {
            return FinalityStage::Hard;
        }
        self.pending
            .get(&height)
            .map(|pb| pb.stage)
            .unwrap_or(FinalityStage::Hard)
    }

    /// Called when a valid fraud proof is accepted. Marks the target block
    /// and all subsequent soft-finalized blocks as Reverted.
    ///
    /// Returns the list of heights that were rolled back, in descending
    /// order (caller applies rollback hooks tip-first).
    pub fn on_fraud_proof_accepted(
        &mut self,
        target_height: u64,
    ) -> Result<Vec<u64>, FinalityError> {
        if target_height <= self.hard_finalized_height {
            return Err(FinalityError::BlockAlreadyReverted);
        }
        let target = self
            .pending
            .get(&target_height)
            .ok_or(FinalityError::UnknownBlock)?;
        if target.stage != FinalityStage::Soft {
            return Err(FinalityError::BlockNotInSoftStage);
        }

        let mut rolled_back: Vec<u64> = self
            .pending
            .iter()
            .filter(|(h, _)| **h >= target_height)
            .map(|(h, _)| *h)
            .collect();
        rolled_back.sort_by(|a, b| b.cmp(a));

        for h in &rolled_back {
            if let Some(pb) = self.pending.get_mut(h) {
                pb.stage = FinalityStage::Reverted;
            }
        }
        // The new current_height is one below the target.
        self.current_height = target_height.saturating_sub(1);
        // Reverted blocks stay in `pending` briefly for visibility; the
        // coordinator may clean them after applying rollback hooks.
        Ok(rolled_back)
    }

    /// Iterate over still-soft blocks in age order. Useful for proactive
    /// challenge-window monitoring.
    pub fn pending_soft(&self) -> impl Iterator<Item = &PendingBlock> {
        self.pending
            .values()
            .filter(|pb| pb.stage == FinalityStage::Soft)
    }

    /// Drop any reverted blocks from the pending set. Called by the
    /// coordinator after rollback hooks have run.
    pub fn drain_reverted(&mut self) -> Vec<PendingBlock> {
        let reverted: Vec<u64> = self
            .pending
            .iter()
            .filter(|(_, pb)| pb.stage == FinalityStage::Reverted)
            .map(|(h, _)| *h)
            .collect();
        reverted
            .into_iter()
            .filter_map(|h| self.pending.remove(&h))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(b: u8) -> B256 {
        B256::from([b; 32])
    }

    #[test]
    fn block_transitions_soft_to_hard_after_window() {
        let cfg = TieredFinalityConfig {
            challenge_window: 5,
            max_pending: 100,
        };
        let mut tf = TieredFinality::new(cfg, 0);

        tf.on_soft_finalize(1, hash(1));
        assert_eq!(tf.stage_of(1), FinalityStage::Soft);
        // 4 more blocks; not enough.
        for i in 2..=5 {
            tf.on_soft_finalize(i, hash(i as u8));
        }
        assert_eq!(tf.stage_of(1), FinalityStage::Soft);
        // One more: now 5 blocks of distance.
        let transitioned = tf.on_soft_finalize(6, hash(6));
        assert!(transitioned.contains(&1));
        assert_eq!(tf.stage_of(1), FinalityStage::Hard);
    }

    #[test]
    fn fraud_proof_reverts_target_and_successors() {
        let cfg = TieredFinalityConfig {
            challenge_window: 100,
            max_pending: 100,
        };
        let mut tf = TieredFinality::new(cfg, 0);
        for i in 1..=10 {
            tf.on_soft_finalize(i, hash(i as u8));
        }
        let rolled = tf.on_fraud_proof_accepted(5).unwrap();
        // Heights 5..=10 should all be reverted, in descending order.
        assert_eq!(rolled, vec![10, 9, 8, 7, 6, 5]);
        for h in 5..=10 {
            assert_eq!(tf.stage_of(h), FinalityStage::Reverted);
        }
        // Heights 1..=4 unaffected.
        for h in 1..=4 {
            assert_eq!(tf.stage_of(h), FinalityStage::Soft);
        }
        // Current height now points to 4.
        assert_eq!(tf.current_height, 4);
    }

    #[test]
    fn cannot_revert_hard_finalized_block() {
        let cfg = TieredFinalityConfig {
            challenge_window: 2,
            max_pending: 100,
        };
        let mut tf = TieredFinality::new(cfg, 0);
        tf.on_soft_finalize(1, hash(1));
        tf.on_soft_finalize(2, hash(2));
        tf.on_soft_finalize(3, hash(3));
        // height 1 is now Hard.
        assert_eq!(tf.stage_of(1), FinalityStage::Hard);
        let err = tf.on_fraud_proof_accepted(1);
        assert_eq!(err, Err(FinalityError::BlockAlreadyReverted));
    }

    #[test]
    fn drain_reverted_clears_pending() {
        let cfg = TieredFinalityConfig {
            challenge_window: 100,
            max_pending: 100,
        };
        let mut tf = TieredFinality::new(cfg, 0);
        for i in 1..=5 {
            tf.on_soft_finalize(i, hash(i as u8));
        }
        tf.on_fraud_proof_accepted(3).unwrap();
        let drained = tf.drain_reverted();
        assert_eq!(drained.len(), 3); // 3, 4, 5
        assert!(tf.pending_soft().count() == 2); // only 1 and 2 remain
    }

    #[test]
    fn pending_soft_iterates_in_age_order() {
        let cfg = TieredFinalityConfig {
            challenge_window: 100,
            max_pending: 100,
        };
        let mut tf = TieredFinality::new(cfg, 0);
        for i in [3, 1, 2] {
            tf.on_soft_finalize(i, hash(i as u8));
        }
        let heights: Vec<u64> = tf.pending_soft().map(|pb| pb.height).collect();
        assert_eq!(heights, vec![1, 2, 3]);
    }
}
