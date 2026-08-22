//! Verkle binding for rotor sequences.
//!
//! Stores `(transition_id → SpacetimeTransition)` in your
//! `QuantumTree<NistSisScheme>`. Light clients fetch any rotor by ID with a
//! SIS-VC multiproof, which is post-quantum sound by the Wee–Wu binding
//! assumption.
//!
//! Key layout:
//!   address = chain_id (20 bytes)         — fixes the per-chain namespace
//!   key     = transition_id_be_bytes      — 32 bytes, big-endian for ordered keys
//!   value   = U256 derived from the digest of the SpacetimeTransition
//!
//! The U256 value is the first 32 bytes of `transition.digest(hash_fn)`. The
//! full transition bytes are stored off-tree in your normal block storage; the
//! Verkle proof binds *which* transition is canonical for a given ID.

use crate::proposal::SpacetimeTransition;
use alloy_primitives::{Address, B256, U256};
use spacekit_quantum_verkle::commitment::{NistSisScheme, QuantumProof, QuantumTree, SisOpening};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VerkleBindingError {
    SetFailed,
    ProofCreationFailed,
    VerificationFailed,
}

/// Wrapper around `QuantumTree` that knows how to handle rotor transitions.
pub struct RotorVerkle {
    pub tree: QuantumTree<NistSisScheme>,
    pub chain_id: Address,
}

impl RotorVerkle {
    pub fn new(chain_id: Address) -> Self {
        Self {
            tree: QuantumTree::<NistSisScheme>::new(),
            chain_id,
        }
    }

    fn key_for(transition_id: u64) -> B256 {
        let mut k = [0u8; 32];
        k[24..32].copy_from_slice(&transition_id.to_be_bytes());
        B256::from(k)
    }

    /// Commit a transition under its `transition_id`. `hash_fn` must be the
    /// same one used elsewhere in your stack (e.g. keccak256).
    pub fn commit<F: Fn(&[u8]) -> [u8; 32]>(
        &mut self,
        transition: &SpacetimeTransition,
        hash_fn: F,
    ) -> Result<(), VerkleBindingError> {
        let digest = transition.digest(hash_fn);
        let value = U256::from_be_bytes::<32>(digest.0);
        self.tree.set(
            &self.chain_id,
            &Self::key_for(transition.transition_id),
            value,
        );
        Ok(())
    }

    /// Produce a Verkle proof that `transition_id` maps to its committed digest.
    /// Returned bytes are the postcard-encoded proof; ready for wire transport.
    pub fn prove<F: Fn(&[u8]) -> [u8; 32]>(
        &self,
        transition_id: u64,
        _hash_fn: F,
    ) -> Result<QuantumProof<SisOpening>, VerkleBindingError> {
        let key = Self::key_for(transition_id);
        self.tree
            .create_proof(&self.chain_id, &key)
            .map_err(|_| VerkleBindingError::ProofCreationFailed)
    }

    /// Verify a Verkle proof for a transition. Light clients run this against
    /// the digest they recomputed from the bytes the validator gossiped.
    pub fn verify<F: Fn(&[u8]) -> [u8; 32]>(
        &self,
        transition: &SpacetimeTransition,
        proof: &QuantumProof<SisOpening>,
        hash_fn: F,
    ) -> bool {
        let digest = transition.digest(hash_fn);
        let value = U256::from_be_bytes::<32>(digest.0);
        let key = Self::key_for(transition.transition_id);
        self.tree.verify_proof(proof, &self.chain_id, &key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal::CausalCoord;
    use crate::rotor::{Bivector, Rotor};

    fn dummy_hash(b: &[u8]) -> [u8; 32] {
        // Test-only non-cryptographic stand-in. In production, pass keccak256.
        let mut h = [0u8; 32];
        for (i, byte) in b.iter().enumerate() {
            h[i % 32] = h[i % 32].wrapping_add(*byte);
        }
        h
    }

    #[test]
    fn commit_and_verify_round_trip() {
        let chain_id = Address::from([1u8; 20]);
        let mut rv = RotorVerkle::new(chain_id);
        let (residual_commitment, residual_norm) =
            SpacetimeTransition::zero_residual_fields(dummy_hash);
        let t = SpacetimeTransition {
            transition_id: 7,
            rotor: Rotor::exp(&Bivector { b: [0.0; 6] }),
            prev_state_hash: B256::ZERO,
            new_state_hash: B256::from([1u8; 32]),
            causal_coord: CausalCoord {
                t: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment,
            residual_norm,
            aux_commit: None,
        };
        rv.commit(&t, dummy_hash).unwrap();
        let proof = rv.prove(t.transition_id, dummy_hash).unwrap();
        assert!(rv.verify(&t, &proof, dummy_hash));
    }
}
