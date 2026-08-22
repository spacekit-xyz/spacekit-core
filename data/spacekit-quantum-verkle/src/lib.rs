#![cfg_attr(not(feature = "std"), no_std)]
//! Quantum-Resistant Verkle Tree Implementation
//!
//! This library provides a Quantum-Resistant Verkle Tree implementation, which is an efficient
//! data structure for storing and verifying key-value pairs.

extern crate alloc;

pub mod commitment;

// Re-export commonly used types
pub use commitment::{
    errors::VerkleError,
    scheme_tree::{QuantumMultiProof, QuantumProof, QuantumRangeProof, QuantumTree},
    schemes::{
        setup_sis_params, HashCommitmentScheme, Kyber1024Params, Kyber512Params, Kyber768Params,
        LatticeCommitmentScheme, LatticeOpening, LatticeParameterSet, NistSisScheme, Sis128B,
        Sis128HB, Sis192B, Sis192HB, SisOpening, SisProfile, SisSecurityLevel,
        WeeWuSisCommitmentScheme, WeeWuSisParams,
    },
};
// Optional: provide convenience functions at the root level
pub fn new_quantum_tree() -> QuantumTree<NistSisScheme> {
    QuantumTree::new()
}
