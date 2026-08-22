//! Visibility strategy: who can see which transactions in the mempool.
//!
//! **Today's default is `EverythingPublic`** — every viewer sees every
//! pending transaction. This matches every blockchain in production today
//! and is what testnet will run.
//!
//! The trait exists so that **if** the project later integrates an encrypted
//! mempool (Shutter-style threshold encryption, commit-reveal, or other
//! established primitive), the integration point is here — not scattered
//! through the mempool's drain and observation paths.
//!
//! ## What this trait is NOT
//!
//! - It is not a place to implement novel cryptography. Encryption primitives
//!   should be implemented in dedicated crates (`spacekit-shutter`,
//!   `spacekit-threshold-encryption`, whatever) and *consumed* by visibility
//!   strategies. The trait surface is intentionally narrow so the actual
//!   cryptography stays separate.
//!
//! - It is not a place to gate consensus correctness. Visibility affects
//!   who sees what; it does not affect what gets included in blocks. Drain
//!   logic uses the priority strategy, not the visibility strategy, to
//!   decide order.
//!
//! - It is not committed to any specific encryption scheme. The default
//!   `EverythingPublic` is the only strategy that ships in this crate.
//!   Encrypted strategies, if any, ship as separate crates.

#[cfg(feature = "visibility")]
use crate::types::{MempoolEntry, TxHash};
#[cfg(feature = "visibility")]
use alloy_primitives::B256;

/// Trait for visibility strategies.
///
/// Only compiled under feature `visibility`. The core mempool's `peek` and
/// `query` methods are unconditionally public when this feature is off.
#[cfg(feature = "visibility")]
pub trait VisibilityStrategy: Send + Sync {
    /// True if `viewer` is allowed to see the *existence* of this transaction
    /// (its presence in the pool, its hash, its sender, etc).
    fn entry_visible_to(&self, entry: &MempoolEntry, viewer: &B256) -> bool;

    /// True if `viewer` is allowed to see the *body* of this transaction
    /// (the full payload). Some strategies may grant existence-visibility
    /// to many viewers but body-visibility only to a few (e.g., the sender,
    /// the proposer, or threshold-key holders).
    fn body_visible_to(&self, entry: &MempoolEntry, viewer: &B256) -> bool;

    /// Optional: filter a list of entries down to only those visible to
    /// `viewer`. Default implementation iterates and calls `entry_visible_to`.
    fn filter_visible<'a>(
        &self,
        entries: &'a [MempoolEntry],
        viewer: &B256,
    ) -> alloc::vec::Vec<&'a MempoolEntry> {
        entries.iter().filter(|e| self.entry_visible_to(e, viewer)).collect()
    }
}

#[cfg(feature = "visibility")]
extern crate alloc;

/// Default visibility strategy: every viewer sees every transaction.
/// Matches every blockchain in production today; this is what testnet runs.
#[cfg(feature = "visibility")]
#[derive(Debug, Default, Clone, Copy)]
pub struct EverythingPublic;

#[cfg(feature = "visibility")]
impl VisibilityStrategy for EverythingPublic {
    fn entry_visible_to(&self, _entry: &MempoolEntry, _viewer: &B256) -> bool { true }
    fn body_visible_to(&self, _entry: &MempoolEntry, _viewer: &B256) -> bool { true }
}

#[cfg(all(test, feature = "visibility"))]
mod tests {
    use super::*;
    use crate::types::{MempoolEntry, TransactionRef};
    use alloy_primitives::U256;

    fn sample_entry() -> MempoolEntry {
        MempoolEntry::new(TransactionRef {
            hash: B256::ZERO,
            sender: B256::from([0xAA; 32]),
            nonce: 0,
            fee: U256::from(1000),
            size_bytes: 200,
            observed_at_block: 100,
            expires_at_block: None,
        })
    }

    #[test]
    fn everything_public_shows_to_any_viewer() {
        let s = EverythingPublic;
        let e = sample_entry();
        assert!(s.entry_visible_to(&e, &B256::ZERO));
        assert!(s.entry_visible_to(&e, &B256::from([0xFF; 32])));
        assert!(s.body_visible_to(&e, &B256::ZERO));
    }
}
