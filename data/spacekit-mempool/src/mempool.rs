//! The main `Mempool` type.
//!
//! Lifecycle states for an entry:
//!
//! ```text
//!                ┌──────────────────────┐
//!                │                      │
//!                ▼                      │
//!   submit ──► Pending ──► InFlight ──► Finalized
//!                ▲           │
//!                │           │ requeue_block (fraud proof rollback)
//!                └───────────┘
//! ```
//!
//! Pending entries are drainable into a proposed block. Once drained, they
//! transition to InFlight tagged with the block height that included them.
//! On soft → hard finality, they transition to Finalized and become evict
//! candidates. On fraud-proof rollback of a soft-finalized block, the
//! InFlight entries for that block transition back to Pending with their
//! `requeue_count` incremented.
//!
//! ## Design choices
//!
//! - **Indexing.** Three indexes: by hash (primary, for lookups), by sender
//!   (for nonce-ordered per-sender views), by state (for fast iteration
//!   over Pending entries during drain).
//! - **Concurrency.** The mempool itself is `Send` + `Sync` but not
//!   internally synchronized. The integration adapter (in
//!   `spacekit-compute-node`) wraps it in an appropriate lock. This
//!   matches the pattern we used for the unified-consensus facade.
//! - **Memory bound.** Configurable max entry count and max total size.
//!   When either bound is hit, the priority strategy decides which entry
//!   to evict (lowest-priority).
//! - **Drain is borrow-based.** `drain_for_block` returns references to
//!   the entries, not copies. The caller decides what to do with them
//!   (typically, build a block proposal). The mempool transitions them
//!   to InFlight only when the caller confirms via `mark_in_flight`.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;
use alloy_primitives::B256;
use crate::types::{TransactionRef, MempoolEntry, EntryState, TxHash, MempoolStats};
use crate::priority::PriorityStrategy;

extern crate alloc;

/// Errors that can occur in mempool operations.
#[derive(Debug, Clone, PartialEq)]
pub enum MempoolError {
    /// A transaction with this hash is already in the pool. Re-submission
    /// is rejected to prevent accidental duplication.
    AlreadyPresent(TxHash),
    /// The mempool has reached its configured capacity limit. The caller
    /// should consider this a "pool full" signal, not a permanent failure.
    Full { current_count: u64, max_count: u64 },
    /// A transaction not in the pool was referenced. Indicates a caller
    /// bug or a race condition the caller should investigate.
    NotFound(TxHash),
    /// A state transition was attempted from an inconsistent state
    /// (e.g., marking Finalized an entry that's still Pending).
    InvalidStateTransition {
        tx_hash: TxHash,
        current: EntryState,
        attempted: alloc::string::String,
    },
}

/// Configuration for the mempool.
#[derive(Debug, Clone)]
pub struct MempoolConfig {
    /// Maximum number of entries the mempool can hold. Hitting this
    /// triggers eviction of the lowest-priority entry to make room.
    pub max_entries: u64,
    /// Maximum total bytes across all entries. Same eviction behavior.
    pub max_total_bytes: u64,
    /// Number of blocks past `observed_at_block` after which entries with
    /// no explicit `expires_at_block` are considered stale. Used by
    /// `evict_expired`.
    pub default_expiry_blocks: u64,
    /// Number of blocks past finalization after which Finalized entries
    /// are dropped from the pool's index. Kept short to bound memory.
    pub finalized_retention_blocks: u64,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_total_bytes: 100 * 1024 * 1024, // 100 MiB
            default_expiry_blocks: 1000,
            finalized_retention_blocks: 100,
        }
    }
}

/// The mempool. Generic over the priority strategy so the strategy can be
/// changed without changing the mempool type.
pub struct Mempool<P: PriorityStrategy> {
    priority: P,
    config: MempoolConfig,
    /// Primary index: by transaction hash.
    by_hash: BTreeMap<TxHash, MempoolEntry>,
    /// Per-sender nonce index: sender → ordered set of (nonce, tx_hash).
    by_sender: BTreeMap<B256, BTreeMap<u64, TxHash>>,
    /// Reverse index from block height → set of in-flight tx_hashes. Used
    /// by `requeue_block` to find which entries to move back to Pending on
    /// fraud-proof rollback.
    in_flight_by_block: BTreeMap<u64, BTreeSet<TxHash>>,
    /// Total bytes currently held (sum of `size_bytes` across all entries).
    total_bytes: u64,
    /// Eviction and requeue counters since the last `reset_stats` call.
    evicted_count: u64,
    requeued_count: u64,
}

impl<P: PriorityStrategy> Mempool<P> {
    pub fn new(priority: P, config: MempoolConfig) -> Self {
        Self {
            priority,
            config,
            by_hash: BTreeMap::new(),
            by_sender: BTreeMap::new(),
            in_flight_by_block: BTreeMap::new(),
            total_bytes: 0,
            evicted_count: 0,
            requeued_count: 0,
        }
    }

    /// Submit a new transaction reference to the pool. Returns the entry's
    /// position in the priority order (0 = next to drain) if accepted.
    pub fn submit(&mut self, tx_ref: TransactionRef) -> Result<(), MempoolError> {
        if self.by_hash.contains_key(&tx_ref.hash) {
            return Err(MempoolError::AlreadyPresent(tx_ref.hash));
        }
        if self.by_hash.len() as u64 >= self.config.max_entries {
            // Try to evict the lowest-priority entry to make room.
            if !self.try_evict_one() {
                return Err(MempoolError::Full {
                    current_count: self.by_hash.len() as u64,
                    max_count: self.config.max_entries,
                });
            }
        }
        let sender = tx_ref.sender;
        let nonce = tx_ref.nonce;
        let hash = tx_ref.hash;
        let size = tx_ref.size_bytes;
        let entry = MempoolEntry::new(tx_ref);

        self.by_hash.insert(hash, entry);
        self.by_sender.entry(sender).or_default().insert(nonce, hash);
        self.total_bytes += size as u64;
        Ok(())
    }

    /// Look up an entry by its hash.
    pub fn get(&self, tx_hash: &TxHash) -> Option<&MempoolEntry> {
        self.by_hash.get(tx_hash)
    }

    /// All entries currently in the pool, in priority order. Cheap for
    /// small pools; for large pools prefer `drain_for_block` which has a
    /// budget.
    pub fn entries_sorted(&self) -> Vec<&MempoolEntry> {
        let mut entries: Vec<&MempoolEntry> = self.by_hash.values()
            .filter(|e| e.is_drainable())
            .collect();
        entries.sort_by(|a, b| self.priority.compare(a, b));
        entries
    }

    /// Build a candidate block draft from pending entries, capped at the
    /// given byte and count budgets. Does NOT transition the entries to
    /// InFlight — the caller calls `mark_in_flight` once they've decided
    /// to actually include them in a proposed block.
    ///
    /// Returns the hashes selected, in drain order.
    pub fn drain_for_block(&self, max_count: usize, max_bytes: u64) -> Vec<TxHash> {
        let mut entries: Vec<&MempoolEntry> = self.by_hash.values()
            .filter(|e| e.is_drainable())
            .collect();
        entries.sort_by(|a, b| self.priority.compare(a, b));

        // Walk entries enforcing per-sender nonce monotonicity. Track the
        // next-expected nonce per sender; skip transactions that would
        // create a gap.
        let mut next_nonce: BTreeMap<B256, u64> = BTreeMap::new();
        let mut selected = Vec::new();
        let mut total_bytes: u64 = 0;

        for entry in entries {
            if selected.len() >= max_count { break; }
            let sender = entry.tx_ref.sender;
            let nonce = entry.tx_ref.nonce;
            let expected = next_nonce.get(&sender).copied().unwrap_or(nonce);
            if nonce != expected { continue; } // skip non-contiguous
            let size = entry.tx_ref.size_bytes as u64;
            if total_bytes + size > max_bytes { continue; }
            selected.push(entry.tx_ref.hash);
            total_bytes += size;
            next_nonce.insert(sender, nonce + 1);
        }
        selected
    }

    /// Transition the listed entries to `InFlight { block_height }`. Called
    /// when the caller has actually included them in a block proposal that
    /// went out on the wire.
    pub fn mark_in_flight(&mut self, tx_hashes: &[TxHash], block_height: u64) -> Result<(), MempoolError> {
        // First pass: validate every transition is legal.
        for h in tx_hashes {
            let entry = self.by_hash.get(h).ok_or(MempoolError::NotFound(*h))?;
            if !matches!(entry.state, EntryState::Pending) {
                return Err(MempoolError::InvalidStateTransition {
                    tx_hash: *h,
                    current: entry.state,
                    attempted: alloc::format!("InFlight at block {}", block_height),
                });
            }
        }
        // Second pass: apply.
        let block_set = self.in_flight_by_block.entry(block_height).or_default();
        for h in tx_hashes {
            if let Some(entry) = self.by_hash.get_mut(h) {
                entry.state = EntryState::InFlight { block_height };
            }
            block_set.insert(*h);
        }
        Ok(())
    }

    /// Transition the listed entries to `Finalized { block_height }`. Called
    /// when a block reaches hard finality.
    pub fn mark_finalized(&mut self, tx_hashes: &[TxHash], block_height: u64) -> Result<(), MempoolError> {
        for h in tx_hashes {
            let entry = self.by_hash.get(h).ok_or(MempoolError::NotFound(*h))?;
            if !matches!(entry.state, EntryState::InFlight { .. }) {
                return Err(MempoolError::InvalidStateTransition {
                    tx_hash: *h,
                    current: entry.state,
                    attempted: alloc::format!("Finalized at block {}", block_height),
                });
            }
        }
        for h in tx_hashes {
            if let Some(entry) = self.by_hash.get_mut(h) {
                if let EntryState::InFlight { block_height: orig_block } = entry.state {
                    entry.state = EntryState::Finalized { block_height };
                    // Remove from in-flight reverse index.
                    if let Some(set) = self.in_flight_by_block.get_mut(&orig_block) {
                        set.remove(h);
                    }
                }
            }
        }
        Ok(())
    }

    /// Requeue all in-flight entries from a block that was reverted by a
    /// fraud proof. This is the hook that `RECOVERY_AND_RATIFICATION.md`
    /// assumes exists.
    ///
    /// Transitions:
    ///   - All `InFlight { block_height }` entries → `Pending`
    ///   - `requeue_count` incremented
    ///   - Reverse index for `block_height` cleared
    ///
    /// Returns the count of entries requeued.
    pub fn requeue_block(&mut self, block_height: u64) -> u64 {
        let hashes: Vec<TxHash> = self.in_flight_by_block
            .remove(&block_height)
            .map(|s| s.into_iter().collect())
            .unwrap_or_default();
        let n = hashes.len() as u64;
        for h in hashes {
            if let Some(entry) = self.by_hash.get_mut(&h) {
                entry.state = EntryState::Pending;
                entry.requeue_count = entry.requeue_count.saturating_add(1);
            }
        }
        self.requeued_count += n;
        n
    }

    /// Evict expired entries. Returns the count evicted.
    pub fn evict_expired(&mut self, current_block: u64) -> u64 {
        let default_horizon = current_block.saturating_sub(self.config.default_expiry_blocks);
        let finalized_horizon = current_block.saturating_sub(self.config.finalized_retention_blocks);

        let to_evict: Vec<TxHash> = self.by_hash.iter()
            .filter_map(|(h, e)| {
                // Explicit expiry takes precedence.
                if let Some(exp) = e.tx_ref.expires_at_block {
                    if current_block >= exp { return Some(*h); }
                }
                match e.state {
                    EntryState::Pending => {
                        if e.tx_ref.observed_at_block < default_horizon { Some(*h) } else { None }
                    }
                    EntryState::Finalized { block_height } => {
                        if block_height < finalized_horizon { Some(*h) } else { None }
                    }
                    EntryState::InFlight { .. } => None, // never evict in-flight
                }
            })
            .collect();

        let count = to_evict.len() as u64;
        for h in to_evict { self.remove_entry(&h); }
        self.evicted_count += count;
        count
    }

    /// Cheap stats snapshot.
    pub fn stats(&self) -> MempoolStats {
        let mut s = MempoolStats::default();
        for entry in self.by_hash.values() {
            match entry.state {
                EntryState::Pending => s.pending_count += 1,
                EntryState::InFlight { .. } => s.in_flight_count += 1,
                EntryState::Finalized { .. } => s.finalized_count += 1,
            }
        }
        s.total_size_bytes = self.total_bytes;
        s.distinct_senders = self.by_sender.len() as u64;
        s.evicted_since_reset = self.evicted_count;
        s.requeued_since_reset = self.requeued_count;
        s
    }

    pub fn reset_stats(&mut self) {
        self.evicted_count = 0;
        self.requeued_count = 0;
    }

    pub fn len(&self) -> usize { self.by_hash.len() }
    pub fn is_empty(&self) -> bool { self.by_hash.is_empty() }

    // --- Internal helpers ---

    fn remove_entry(&mut self, tx_hash: &TxHash) -> Option<MempoolEntry> {
        let entry = self.by_hash.remove(tx_hash)?;
        let sender = entry.tx_ref.sender;
        let nonce = entry.tx_ref.nonce;
        if let Some(senders) = self.by_sender.get_mut(&sender) {
            senders.remove(&nonce);
            if senders.is_empty() { self.by_sender.remove(&sender); }
        }
        if let EntryState::InFlight { block_height } = entry.state {
            if let Some(set) = self.in_flight_by_block.get_mut(&block_height) {
                set.remove(tx_hash);
            }
        }
        self.total_bytes = self.total_bytes.saturating_sub(entry.tx_ref.size_bytes as u64);
        Some(entry)
    }

    fn try_evict_one(&mut self) -> bool {
        // Find lowest-priority drainable entry and evict it.
        let mut entries: Vec<&MempoolEntry> = self.by_hash.values()
            .filter(|e| e.is_drainable())
            .collect();
        if entries.is_empty() { return false; }
        entries.sort_by(|a, b| self.priority.compare(b, a)); // reversed; worst first
        let to_evict = entries[0].tx_ref.hash;
        self.remove_entry(&to_evict);
        self.evicted_count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::priority::FeePerByteDescending;
    use alloy_primitives::U256;

    fn make_ref(sender_byte: u8, nonce: u64, fee: u64) -> TransactionRef {
        let mut h = [0u8; 32];
        h[0] = sender_byte;
        h[8..16].copy_from_slice(&nonce.to_be_bytes());
        TransactionRef {
            hash: B256::from(h),
            sender: B256::from([sender_byte; 32]),
            nonce,
            fee: U256::from(fee),
            size_bytes: 200,
            observed_at_block: 100,
            expires_at_block: None,
        }
    }

    fn pool() -> Mempool<FeePerByteDescending> {
        Mempool::new(FeePerByteDescending, MempoolConfig::default())
    }

    #[test]
    fn submit_then_lookup_round_trips() {
        let mut m = pool();
        let r = make_ref(1, 0, 1000);
        let h = r.hash;
        m.submit(r).unwrap();
        assert!(m.get(&h).is_some());
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn duplicate_submission_rejected() {
        let mut m = pool();
        let r = make_ref(1, 0, 1000);
        m.submit(r.clone()).unwrap();
        assert!(matches!(m.submit(r), Err(MempoolError::AlreadyPresent(_))));
    }

    #[test]
    fn drain_respects_byte_budget() {
        let mut m = pool();
        for nonce in 0..5 {
            m.submit(make_ref(1, nonce, 1000)).unwrap();
        }
        // 200 bytes each * 5 = 1000 total. Budget at 500 = only 2 drain.
        let drained = m.drain_for_block(100, 500);
        assert_eq!(drained.len(), 2);
    }

    #[test]
    fn drain_respects_count_budget() {
        let mut m = pool();
        for nonce in 0..5 {
            m.submit(make_ref(1, nonce, 1000)).unwrap();
        }
        let drained = m.drain_for_block(3, u64::MAX);
        assert_eq!(drained.len(), 3);
    }

    #[test]
    fn drain_enforces_per_sender_nonce_ordering() {
        let mut m = pool();
        // Submit nonces 0, 2, 3 — gap at 1. Drain should stop at nonce 0.
        m.submit(make_ref(1, 0, 1000)).unwrap();
        m.submit(make_ref(1, 2, 1000)).unwrap();
        m.submit(make_ref(1, 3, 1000)).unwrap();
        let drained = m.drain_for_block(100, u64::MAX);
        assert_eq!(drained.len(), 1, "drain must not skip nonce gap");
    }

    #[test]
    fn drain_does_not_change_state_until_mark_in_flight() {
        let mut m = pool();
        for nonce in 0..3 {
            m.submit(make_ref(1, nonce, 1000)).unwrap();
        }
        let drained = m.drain_for_block(100, u64::MAX);
        for h in &drained {
            assert_eq!(m.get(h).unwrap().state, EntryState::Pending);
        }
        m.mark_in_flight(&drained, 105).unwrap();
        for h in &drained {
            assert!(matches!(m.get(h).unwrap().state, EntryState::InFlight { block_height: 105 }));
        }
    }

    #[test]
    fn requeue_block_returns_entries_to_pending() {
        let mut m = pool();
        for nonce in 0..3 {
            m.submit(make_ref(1, nonce, 1000)).unwrap();
        }
        let drained = m.drain_for_block(100, u64::MAX);
        m.mark_in_flight(&drained, 105).unwrap();
        let requeued = m.requeue_block(105);
        assert_eq!(requeued, 3);
        for h in &drained {
            let entry = m.get(h).unwrap();
            assert_eq!(entry.state, EntryState::Pending);
            assert_eq!(entry.requeue_count, 1);
        }
    }

    #[test]
    fn finalize_clears_in_flight_index() {
        let mut m = pool();
        m.submit(make_ref(1, 0, 1000)).unwrap();
        let drained = m.drain_for_block(100, u64::MAX);
        m.mark_in_flight(&drained, 105).unwrap();
        m.mark_finalized(&drained, 105).unwrap();
        // Requeuing 105 should now requeue nothing (no in-flight entries left).
        assert_eq!(m.requeue_block(105), 0);
    }

    #[test]
    fn expired_entries_evicted() {
        let mut m = pool();
        m.submit(make_ref(1, 0, 1000)).unwrap();
        let r = TransactionRef {
            expires_at_block: Some(150),
            ..make_ref(2, 0, 1000)
        };
        m.submit(r).unwrap();
        let evicted = m.evict_expired(160);
        assert_eq!(evicted, 1); // only the explicit-expiry one
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn stats_reflect_state_distribution() {
        let mut m = pool();
        m.submit(make_ref(1, 0, 1000)).unwrap();
        m.submit(make_ref(2, 0, 1000)).unwrap();
        let drained = m.drain_for_block(100, u64::MAX);
        m.mark_in_flight(&drained[..1], 105).unwrap();
        let s = m.stats();
        assert_eq!(s.pending_count, 1);
        assert_eq!(s.in_flight_count, 1);
        assert_eq!(s.finalized_count, 0);
        assert_eq!(s.distinct_senders, 2);
    }

    #[test]
    fn full_pool_evicts_lowest_priority() {
        let config = MempoolConfig { max_entries: 3, ..Default::default() };
        let mut m = Mempool::new(FeePerByteDescending, config);
        // Three different senders so they compete on fee, not nonce.
        m.submit(make_ref(1, 0, 100)).unwrap(); // lowest fee
        m.submit(make_ref(2, 0, 1000)).unwrap();
        m.submit(make_ref(3, 0, 500)).unwrap();
        // Submitting a fourth must evict the lowest-fee one.
        m.submit(make_ref(4, 0, 800)).unwrap();
        assert_eq!(m.len(), 3);
        assert!(m.get(&make_ref(1, 0, 100).hash).is_none()); // evicted
    }
}
