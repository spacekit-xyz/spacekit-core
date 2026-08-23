//! Core mempool types.
//!
//! Designed so the mempool stores **references and metadata** for transactions,
//! not the transaction body itself. The body lives wherever the VM keeps it
//! (or in a separate content-addressed blob store, or in-memory for the
//! lifetime of the entry). The mempool's job is ordering, eviction, and
//! lifecycle — not transaction storage.
//!
//! ## Why references-not-bodies
//!
//! Three reasons:
//!
//! 1. **Memory bound.** A mempool that stores full transaction bodies grows
//!    proportionally to total pending value. References + metadata grow
//!    proportionally to count, which is much smaller.
//! 2. **Serialization independence.** Transaction encoding may change
//!    (different VM versions, different signature schemes, different
//!    block-data layouts). Storing references means mempool versioning is
//!    independent of those changes.
//! 3. **Encryption-strategy independence.** When the visibility-strategy
//!    trait lands (feature `visibility`), the body may not be readable by
//!    the mempool layer at all. References are always readable; bodies
//!    may be encrypted.

use alloc::vec::Vec;
use alloy_primitives::{B256, U256};

extern crate alloc;

/// Stable identifier for a transaction. Computed by hashing the transaction's
/// canonical serialization. The mempool indexes entries by this hash.
pub type TxHash = B256;

/// A reference to a pending transaction. The mempool stores these; the
/// full transaction body is resolved through a `TransactionStore` when
/// needed (typically at propose time).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransactionRef {
    /// Stable identifier (hash of canonical serialization).
    pub hash: TxHash,
    /// Submitter's DID hash. Used for sybil-tracking, per-sender ordering
    /// (nonces), and visibility checks. Not the transaction's signature
    /// — that's verified by the VM before submission.
    pub sender: B256,
    /// Sender's nonce for this transaction. Used to enforce per-sender
    /// ordering during drain — a nonce N+1 tx must drain after nonce N.
    pub nonce: u64,
    /// Fee offered, in the network's fee unit. Used by the priority
    /// strategy to order transactions.
    pub fee: U256,
    /// Size of the transaction body in bytes. Used by the priority
    /// strategy and by block-builder logic that has byte budgets.
    pub size_bytes: u32,
    /// Time the transaction was first observed by this node. Used for
    /// FIFO tiebreaking and for expiry policies.
    pub observed_at_block: u64,
    /// Optional expiry deadline. If the transaction has not been included
    /// by this block height, the mempool evicts it without notifying the
    /// sender.
    pub expires_at_block: Option<u64>,
}

/// A mempool entry: the transaction reference plus mempool-internal state.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MempoolEntry {
    pub tx_ref: TransactionRef,
    pub state: EntryState,
    /// Number of times this entry has been requeued (e.g., due to fraud
    /// proof rollback). Helps the priority strategy demote repeatedly-
    /// reverted transactions.
    pub requeue_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum EntryState {
    /// Entered the pool, eligible for inclusion in a proposed block.
    Pending,
    /// Drained into a proposed block, awaiting soft finality. Not eligible
    /// for re-drain (would double-spend), but kept in the index in case
    /// the block is reverted by a fraud proof.
    InFlight { block_height: u64 },
    /// Block containing this transaction reached hard finality. The
    /// mempool drops these on the next eviction cycle.
    Finalized { block_height: u64 },
}

impl MempoolEntry {
    pub fn new(tx_ref: TransactionRef) -> Self {
        Self { tx_ref, state: EntryState::Pending, requeue_count: 0 }
    }

    /// True if this entry is eligible for inclusion in a new proposed block.
    pub fn is_drainable(&self) -> bool {
        matches!(self.state, EntryState::Pending)
    }
}

/// Summary statistics for monitoring and observability. Cheap to compute
/// at any time; useful for the runbook scenarios that watch for mempool
/// turbulence.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MempoolStats {
    pub pending_count: u64,
    pub in_flight_count: u64,
    pub finalized_count: u64,
    pub total_size_bytes: u64,
    /// Number of distinct senders with at least one pending tx.
    pub distinct_senders: u64,
    /// Number of entries evicted since the last reset.
    pub evicted_since_reset: u64,
    /// Number of entries requeued (from rollback) since the last reset.
    pub requeued_since_reset: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ref(nonce: u64, fee: u64) -> TransactionRef {
        TransactionRef {
            hash: B256::from([nonce as u8; 32]),
            sender: B256::from([0xAA; 32]),
            nonce,
            fee: U256::from(fee),
            size_bytes: 200,
            observed_at_block: 100,
            expires_at_block: Some(200),
        }
    }

    #[test]
    fn new_entry_is_pending_and_drainable() {
        let e = MempoolEntry::new(sample_ref(0, 1000));
        assert_eq!(e.state, EntryState::Pending);
        assert!(e.is_drainable());
        assert_eq!(e.requeue_count, 0);
    }

    #[test]
    fn in_flight_entries_are_not_drainable() {
        let mut e = MempoolEntry::new(sample_ref(0, 1000));
        e.state = EntryState::InFlight { block_height: 105 };
        assert!(!e.is_drainable());
    }

    #[test]
    fn finalized_entries_are_not_drainable() {
        let mut e = MempoolEntry::new(sample_ref(0, 1000));
        e.state = EntryState::Finalized { block_height: 105 };
        assert!(!e.is_drainable());
    }
}
