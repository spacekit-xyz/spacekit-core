//! # spacekit-unified-consensus
//!
//! Facade for the SpaceKit unified consensus. Wraps a [`CoordinatorHandle`]
//! (production: `ConsensusCoordinator`) with the API surface the documentation
//! references. Standalone BFT in count mode; optional spacetime rotor paths
//! when feature `spacetime` is enabled; reputation via [`ReputationSource`].
//!
//! ## What this crate is
//!
//! - `ReputationWeightedConsensus<C>` — the facade type. Generic over the
//!   coordinator handle so this crate doesn't take a hard dependency on
//!   `spacekit-compute-node`.
//! - `CoordinatorHandle` — the small trait the coordinator implements to
//!   plug in. `ConsensusCoordinator` in `spacekit-compute-node` implements
//!   this trait via a thin adapter (no changes to the coordinator's own API).
//! - `UnifiedConsensusValidator` — facade-side view of a validator,
//!   reconstructed each round from coordinator state + reputation lookups.
//! - `ReputationSource` — trait for plugging in reputation. Default is
//!   `EqualWeightReputation` (1.0 for every validator). Authoritative
//!   sources required for the post-fork weighted-threshold mode.
//! - Spacetime integration (under feature `spacetime`) — bridges to
//!   `spacekit-spacetime-consensus` for rotor aggregation and transition
//!   verification only; other spacetime surfaces use the node coordinator.
//!
//! ## What this crate is NOT
//!
//! - It is not a replacement for `ConsensusCoordinator`. The coordinator
//!   still runs PBFT, P2P, persistence. The facade is a thinner type
//!   above it.
//! - It is not where new consensus logic goes. Logic lives in the
//!   coordinator (PBFT, vote collection), the spacetime crate (rotor/
//!   fingerprint/finality/fraud paths), or `MLReputationEngine` (reputation
//!   computation). The facade exposes those pieces with a unified API.
//!
//! ## Pre-fork vs post-fork
//!
//! Today (pre-fork): the facade defers to the coordinator for the quorum
//! threshold check. Reputation is observable through `ReputationSource`
//! but not authoritative, the coordinator's count-based threshold remains
//! the source of truth.
//!
//! Post-fork: enable `use_weighted_threshold = true` in `FacadeConfig`
//! and supply an authoritative `ReputationSource` (one that derives
//! deterministic per-validator reputation from on-chain state). The
//! facade's threshold check then uses sum-of-reputation-weighted-power
//! instead of vote count.
//!
//! Switching modes is a hard fork, every validator must agree on the
//! same reputation values at every height. See
//! `SPACEKIT_CONSENSUS_UNIFIED.md` §1.4 for the migration plan.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod facade;
pub mod reputation_hook;
pub mod validator;
pub mod voting_power;

#[cfg(feature = "spacetime")]
pub mod spacetime_integration;

#[cfg(test)]
pub(crate) mod tests_support;

pub use facade::{
    CoordinatorHandle, FacadeConfig, FacadeError, ReputationWeightedConsensus, WeightedVotingResult,
};
pub use reputation_hook::{CachedReputationMap, EqualWeightReputation, ReputationSource};
pub use validator::{UnifiedConsensusValidator, ValidatorStatus};

#[cfg(feature = "spacetime")]
pub use spacetime_integration::{BlockSpacetimeData, SpacetimeIntegrationError};

/// Crate version of the facade API. Bumped on breaking changes to the
/// `CoordinatorHandle` trait or the facade's public methods.
pub const FACADE_API_VERSION: u16 = 1;
