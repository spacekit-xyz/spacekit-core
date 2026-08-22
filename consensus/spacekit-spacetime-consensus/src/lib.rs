//! # SpaceKit Spacetime Consensus
//!
//! A Cl(1,3) (spacetime-algebra / geometric algebra) extension layer for
//! `ReputationWeightedConsensus`. This crate does NOT replace the PBFT-style
//! voting loop, identity verification, or quantum-safe signing in the existing
//! `spacekit-consensus` crate. It adds four orthogonal capabilities:
//!
//! 1. **Rotor-valued transitions.** Every block proposal carries a rotor
//!    `R ∈ Spin(1,3)` representing the state delta. State updates take the
//!    sandwich form `S' = R̃ · S · R`.
//! 2. **Causal-set ordering.** A bivector-induced partial order over events
//!    that respects the light-cone structure. Replaces hand-rolled DAG order
//!    for browser nodes and stateless clients.
//! 3. **Fréchet-mean aggregation.** When validators independently compute a
//!    rotor for the same block, the consensus rotor is the reputation-weighted
//!    geodesic mean on the Spin manifold, not a majority vote on bit-equality.
//! 4. **Verkle binding.** Rotors are committed to `QuantumTree<NistSisScheme>`
//!    at deterministic keys so light clients verify rotor sequences with SIS
//!    multiproofs and no state.
//!
//! ## Integration with `ReputationWeightedConsensus`
//!
//! Wire the extension in at construction time:
//!
//! ```ignore
//! use spacekit_spacetime_consensus::SpacetimeExtension;
//!
//! pub struct ReputationWeightedConsensus {
//!     // ... existing fields ...
//!     pub spacetime: Option<SpacetimeExtension>,
//! }
//! ```
//!
//! Then, in `propose_block`:
//!
//! ```ignore
//! let transition = if let Some(st) = &self.spacetime {
//!     Some(st.compute_transition(&prev_state, &block_data)?)
//! } else {
//!     None
//! };
//! let proposal = QuantumSafeProposal::new_with_transition(
//!     round, view, proposer, block_data, transition,
//! )?;
//! ```
//!
//! In `collect_weighted_votes`, after gathering the votes, ask the extension to
//! aggregate any per-validator transition rotors:
//!
//! ```ignore
//! if let Some(st) = &self.spacetime {
//!     let consensus_rotor = st.aggregate_votes(&voting_result)?;
//!     voting_result.consensus_rotor = Some(consensus_rotor);
//! }
//! ```
//!
//! Existing Y/N tallying and 2/3 threshold logic is untouched.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod agent;
pub mod aggregation;
pub mod algebra;
pub mod causal;
pub mod consensus;
pub mod defense;
pub mod equivocation;
pub mod finality;
pub mod fingerprint_attestation;
pub mod fingerprint_verkle;
pub mod fraud_proof;
pub mod light_client;
pub mod pq_envelope;
pub mod proposal;
pub mod rotor;

#[cfg(feature = "verkle")]
pub mod verkle;
#[cfg(feature = "verkle")]
pub use verkle::RotorVerkle;

#[cfg(feature = "kyber-aux")]
pub mod kyber_aux;

pub use aggregation::{aggregate_rotors, FrechetMeanConfig};
pub use algebra::{Multivector, BASIS_DIM};
pub use causal::{CausalEvent, CausalRelation, CausalSet};
pub use consensus::{ConsensusRotor, SpacetimeError, SpacetimeExtension};
pub use defense::{
    detect_coordination_clique, geometric_median_rotor, CoordinationClique, FingerprintRegistry,
    GeometricMedianConfig, RotorFingerprint, RoundSubmission,
};
pub use equivocation::{
    DualRotorEvidence, FingerprintDepartureEvidence, SandwichMismatchEvidence, SlashingCategory,
    SlashingProposal, SlashingSeverity,
};
#[cfg(feature = "verkle")]
pub use fingerprint_verkle::store::{
    apply_fingerprint_batch, FingerprintStoreSnapshot, FingerprintVerkle,
};
pub use fingerprint_verkle::{
    FingerprintCommitment, FINGERPRINT_NAMESPACE, FINGERPRINT_WIRE_VERSION,
};
pub use light_client::{verify_rotor_chain, RotorChainProof};
pub use pq_envelope::{
    dilithium_sig_digest, tagged_commitment, votes_merkle_root, BlockEnvelope, ConsensusVoteInner,
    ConsensusVoteType, PqEnvelopeError, SignedBlockEnvelope, DOMAIN_BLOCK_ENVELOPE,
    DOMAIN_CONSENSUS_VOTE, DOMAIN_SPACETIME_TRANSITION, DOMAIN_STATE_VERKLE, DOMAIN_TX_VERKLE,
    DOMAIN_VOTES_MERKLE, DOMAIN_VOTE_MERKLE_LEAF, PQ_ENVELOPE_WIRE_VERSION,
};
#[cfg(feature = "pq-signatures")]
pub use pq_envelope::{pq_crypto, verify_quorum_against_envelope};
pub use proposal::{SpacetimeTransition, TransitionWitness};
pub use rotor::{Bivector, Rotor, RotorError};

pub use agent::{
    evaluate_ratification, validator_should_ratify, ActivatedParameterChange, GrowformerInference,
    GrowformerIntent, MalignRatificationEvidence, ParameterChangeProposal, ParameterChangeVote,
    PolicyRegime, RatificationConfig, RatificationError,
};
pub use finality::{
    FinalityError, FinalityStage, PendingBlock, TieredFinality, TieredFinalityConfig,
};
pub use fingerprint_attestation::{
    AttestationError, FingerprintAttestation, FingerprintAttestationCollector,
    FingerprintAttestationMismatchEvidence,
};
pub use fraud_proof::{
    submit_fraud_proof, verify_fraud_proof, FraudProof, FraudProofAcceptance, FraudProofError,
    FraudProofSubmission,
};

/// Crate-wide result type.
pub type Result<T> = core::result::Result<T, SpacetimeError>;

/// Version of the spacetime layer wire format. Bumped on incompatible changes
/// to rotor / proof serialization. Light clients verify this matches the
/// genesis-anchored value before accepting any rotor proof.
pub const SPACETIME_WIRE_VERSION: u16 = 2;
