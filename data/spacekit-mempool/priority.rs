//! Priority strategy: how the mempool orders entries for drain.
//!
//! Default is fee-per-byte descending with FIFO tiebreak. The trait exists
//! so the mempool can swap in fairness-weighted ordering, MEV-resistant
//! ordering (FCFS by observation time), or future encrypted-sort strategies
//! without touching the core mempool type.
//!
//! ## What a priority strategy MUST guarantee
//!
//! 1. **Per-sender nonce order.** A transaction with `(sender=S, nonce=N+1)`
//!    must not be ranked above `(sender=S, nonce=N)`. The mempool's drain
//!    logic relies on this to avoid producing blocks that include nonce
//!    gaps for a sender.
//! 2. **Determinism for equal entries.** Two entries with identical
//!    (fee, size, sender, nonce, observed_at_block) must order
//!    consistently across calls. The FIFO default uses `tx_hash` as the
//!    final tiebreaker, which gives total order on B256 bytes.
//! 3. **Stability under requeue.** Strategies may use `requeue_count` as
//!    a demoting factor, but a requeued transaction must not be ordered
//!    below a transaction it was originally above unless explicitly
//!    intended.

use core::cmp::Ordering;
use crate::types::MempoolEntry;

/// A priority strategy compares two pending entries to decide drain order.
/// Returns `Ordering::Less` if `a` should drain BEFORE `b`.
pub trait PriorityStrategy: Send + Sync {
    fn compare(&self, a: &MempoolEntry, b: &MempoolEntry) -> Ordering;

    /// Optional: if the strategy maintains internal state that must be
    /// reset (e.g., per-sender fairness counters), override this.
    fn reset(&mut self) {}
}

/// Default priority: fee-per-byte descending, then per-sender nonce ascending,
/// then FIFO by `observed_at_block`, then tx_hash byte order for total
/// determinism.
///
/// Trade-offs:
///   - Maximizes block fee revenue at moderate batch sizes
///   - Per-sender nonce ordering is enforced via the secondary key
///   - FIFO third-key means equal-paying transactions drain in submission
///     order, which is operator-fair
#[derive(Debug, Default, Clone, Copy)]
pub struct FeePerByteDescending;

impl PriorityStrategy for FeePerByteDescending {
    fn compare(&self, a: &MempoolEntry, b: &MempoolEntry) -> Ordering {
        // Same-sender always orders by nonce.
        if a.tx_ref.sender == b.tx_ref.sender {
            return a.tx_ref.nonce.cmp(&b.tx_ref.nonce);
        }
        // Different senders: compare fee-per-byte.
        // To avoid division, compare fee_a * size_b vs fee_b * size_a.
        // Reversed because higher fee-per-byte drains FIRST (Less).
        let a_score = a.tx_ref.fee.saturating_mul(alloy_primitives::U256::from(b.tx_ref.size_bytes));
        let b_score = b.tx_ref.fee.saturating_mul(alloy_primitives::U256::from(a.tx_ref.size_bytes));
        match b_score.cmp(&a_score) {
            Ordering::Equal => {
                // FIFO by observation time, then tx_hash for full determinism.
                match a.tx_ref.observed_at_block.cmp(&b.tx_ref.observed_at_block) {
                    Ordering::Equal => a.tx_ref.hash.cmp(&b.tx_ref.hash),
                    other => other,
                }
            }
            other => other,
        }
    }
}

/// FIFO-only priority: drain in observation order regardless of fee. Useful
/// for MEV-resistance and for testnets where fee markets are not yet meaningful.
#[derive(Debug, Default, Clone, Copy)]
pub struct ObservationOrder;

impl PriorityStrategy for ObservationOrder {
    fn compare(&self, a: &MempoolEntry, b: &MempoolEntry) -> Ordering {
        if a.tx_ref.sender == b.tx_ref.sender {
            return a.tx_ref.nonce.cmp(&b.tx_ref.nonce);
        }
        match a.tx_ref.observed_at_block.cmp(&b.tx_ref.observed_at_block) {
            Ordering::Equal => a.tx_ref.hash.cmp(&b.tx_ref.hash),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TransactionRef;
    use alloy_primitives::{B256, U256};

    fn entry(sender_byte: u8, nonce: u64, fee: u64, size: u32, observed: u64, hash_byte: u8) -> MempoolEntry {
        MempoolEntry::new(TransactionRef {
            hash: B256::from([hash_byte; 32]),
            sender: B256::from([sender_byte; 32]),
            nonce,
            fee: U256::from(fee),
            size_bytes: size,
            observed_at_block: observed,
            expires_at_block: None,
        })
    }

    #[test]
    fn same_sender_orders_by_nonce() {
        let p = FeePerByteDescending;
        let a = entry(1, 0, 100, 200, 50, 1);
        let b = entry(1, 1, 1_000_000, 200, 50, 2); // higher fee, higher nonce
        assert_eq!(p.compare(&a, &b), Ordering::Less); // a drains first (lower nonce)
    }

    #[test]
    fn different_senders_order_by_fee_per_byte() {
        let p = FeePerByteDescending;
        let a = entry(1, 0, 1000, 100, 50, 1); // fee/byte = 10
        let b = entry(2, 0, 1000, 200, 50, 2); // fee/byte = 5
        assert_eq!(p.compare(&a, &b), Ordering::Less); // a drains first (higher fee/byte)
    }

    #[test]
    fn equal_fee_per_byte_falls_back_to_observation_order() {
        let p = FeePerByteDescending;
        let a = entry(1, 0, 1000, 100, 50, 1);
        let b = entry(2, 0, 1000, 100, 60, 2);
        assert_eq!(p.compare(&a, &b), Ordering::Less); // earlier observed drains first
    }

    #[test]
    fn equal_in_every_dimension_falls_back_to_hash() {
        let p = FeePerByteDescending;
        let a = entry(1, 0, 1000, 100, 50, 1);
        let b = entry(2, 0, 1000, 100, 50, 2);
        assert_eq!(p.compare(&a, &b), Ordering::Less); // lower hash drains first
    }

    #[test]
    fn observation_order_strategy_ignores_fee() {
        let p = ObservationOrder;
        let a = entry(1, 0, 100, 200, 50, 1);
        let b = entry(2, 0, 1_000_000, 200, 30, 2); // higher fee, but observed later
        assert_eq!(p.compare(&b, &a), Ordering::Less); // b drains first (earlier observed)
    }

    #[test]
    fn observation_order_still_respects_same_sender_nonce() {
        let p = ObservationOrder;
        let a = entry(1, 1, 100, 200, 40, 1); // observed earlier but higher nonce
        let b = entry(1, 0, 100, 200, 50, 2);
        assert_eq!(p.compare(&b, &a), Ordering::Less); // b drains first (lower nonce, same sender)
    }
}
