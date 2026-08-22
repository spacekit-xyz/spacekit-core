//! Core implementation of the Quantum-Resistant Verkle Tree commitment scheme
//!
//! This module contains the implementation of:
//! - Quantum-Resistant Verkle Tree data structure
//! - Commitment schemes
//! - Proof generation and verification
//! - Error handling
//! - Optimized multiproof implementation (std-only)

pub mod errors;
#[cfg(feature = "std")]
pub mod multiproof;
pub mod scheme_tree;
pub mod schemes;

// Re-exports
pub use errors::VerkleError;
#[cfg(feature = "std")]
pub use multiproof::sha3::Sha3_256QuantumTree;
pub use scheme_tree::{QuantumMultiProof, QuantumProof, QuantumRangeProof, QuantumTree};
pub use schemes::{
    setup_sis_params, CommitmentScheme, HashCommitmentScheme, Kyber1024Params, Kyber512Params,
    Kyber768Params, LatticeCommitmentScheme, LatticeOpening, LatticeParameterSet, NistSisScheme,
    Sis128B, Sis128HB, Sis192B, Sis192HB, SisOpening, SisProfile, SisSecurityLevel,
    WeeWuSisCommitmentScheme, WeeWuSisParams,
};
