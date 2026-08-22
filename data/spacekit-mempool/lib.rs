//! # spacekit-mempool
//!
//! Transaction mempool for the SpaceKit network.
//!
//! The mempool stores **references and metadata** for pending transactions
//! (not transaction bodies — those live in the VM or in content-addressed
//! storage). The mempool's responsibility is ordering, lifecycle, and
//! eviction:
//!
//! - **Ingress.** `submit()` accepts new transaction references.
//! - **Drain.** `drain_for_block()` returns the next set of transactions
//!   to include in a proposed block, respecting per-sender nonce ordering
//!   and byte/count budgets.
//! - **Lifecycle.** Entries transition Pending → InFlight → Finalized as
//!   they progress through consensus. Fraud-proof rollback returns
//!   InFlight entries to Pending via `requeue_block()`.
//! - **Eviction.** Expired entries and pool-full eviction are handled
//!   internally based on configurable thresholds.
//!
//! ## Pluggable strategies
//!
//! - **Priority** (`priority::PriorityStrategy` trait): how the mempool
//!   orders pending entries. Ships with `FeePerByteDescending` (default)
//!   and `ObservationOrder` (FIFO, MEV-resistant).
//! - **Visibility** (feature `visibility`): how the mempool decides what
//!   each viewer can see. Ships with `EverythingPublic`. Encrypted
//!   strategies, if any, ship as separate crates and consume this trait.
//!
//! ## Integration with `spacekit-compute-node`
//!
//! The mempool is `Send + Sync` but **not internally synchronized**. The
//! integration adapter in `spacekit-compute-node` wraps it in an
//! appropriate lock (the same pattern we used for `UnifiedConsensusHost`):
//!
//! ```ignore
//! pub struct ComputeNodeMempool {
//!     inner: Arc<RwLock<Mempool<FeePerByteDescending>>>,
//! }
//! ```
//!
//! On the consensus path:
//!
//! - HTTP `POST /transaction` and P2P tx gossip both call `submit()`.
//! - The propose path calls `drain_for_block()` and `mark_in_flight()`.
//! - The PQ finalize path calls `mark_finalized()` once a block reaches
//!   hard finality.
//! - The fraud-proof acceptance handler in `finality.rs` calls
//!   `requeue_block()` to return reverted transactions to the pool.
//!
//! ## What this crate does NOT include
//!
//! - **Transaction body storage.** The mempool stores `TransactionRef`s
//!   keyed by hash; resolving a hash to its body is the integration
//!   adapter's responsibility.
//! - **Signature verification.** The submitter (HTTP handler, P2P
//!   listener) verifies signatures before calling `submit()`. The mempool
//!   assumes references it receives are signature-verified.
//! - **Fee market dynamics.** Pricing, fee bumping, and replacement-by-
//!   fee policies are outside this crate's scope. The `PriorityStrategy`
//!   trait gives the integration layer a place to implement them.
//! - **P2P transaction gossip.** The mempool exposes ingress and observation
//!   methods; the gossip transport is a separate concern.
//! - **Novel cryptography.** The visibility trait exists for future
//!   integration with encrypted-mempool primitives (Shutter, commit-reveal,
//!   etc.), but no encryption ships in this crate. If/when an encrypted
//!   strategy is added, it ships as a separate crate that implements
//!   `VisibilityStrategy`.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod types;
pub mod priority;
pub mod mempool;

#[cfg(feature = "visibility")]
pub mod visibility;

pub use types::{TransactionRef, TxHash, MempoolEntry, EntryState, MempoolStats};
pub use priority::{PriorityStrategy, FeePerByteDescending, ObservationOrder};
pub use mempool::{Mempool, MempoolConfig, MempoolError};

#[cfg(feature = "visibility")]
pub use visibility::{VisibilityStrategy, EverythingPublic};

/// Crate version of the public API. Bumped on breaking changes to the
/// `Mempool` type or the `PriorityStrategy` trait.
pub const MEMPOOL_API_VERSION: u16 = 1;
