//! Stateless light-client verification.
//!
//! A stateless light client receives:
//!   - `genesis_state_hash`: anchored at first boot from the genesis full node
//!   - `genesis_rotor`: identity by definition
//!   - a list of `SpacetimeTransition`s (200 bytes each)
//!   - the quantum-safe signatures already validated by the existing layer
//!
//! It then verifies the *rotor chain*, that the sequence of rotors is
//! well-formed (each is a valid Spin⁺(1,3) element), that the causal
//! coordinates are monotone in the forward light cone, and that adjacent
//! transitions' state-hash claims chain (new = prev of the next).
//!
//! The client does NOT replay state. The guarantee is: if 2/3+ reputation-
//! weighted validators (verified by the existing signature layer) signed off
//! on this rotor chain, then the post-state hash is canonical.
//!
//! This is the cheap path for **browser VM nodes** and **stateless clients**
//! in your topology. Per-transition cost: constant-time rotor norm check +
//! constant-time causal cone check + signature already done elsewhere.

use crate::proposal::SpacetimeTransition;
use crate::rotor::Rotor;
use crate::SPACETIME_WIRE_VERSION;
use alloc::vec::Vec;
use alloy_primitives::B256;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightClientError {
    EmptyChain,
    InvalidRotorAt(usize),
    StateHashMismatchAt(usize),
    CausalViolationAt(usize),
    WireVersionMismatch,
    NonMonotonicTransitionId,
}

/// A complete rotor-chain proof. Light clients receive this from validator
/// full nodes (e.g. over gossip) and verify it locally.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RotorChainProof {
    pub wire_version: u16,
    /// Hash of the state the chain starts from. Anchored at genesis on first
    /// run; thereafter the client carries its own checkpoint.
    pub anchor_state_hash: B256,
    /// Coordinate the chain starts from (origin for genesis-anchored).
    pub anchor_coord: crate::causal::CausalCoord,
    /// The chain itself.
    pub transitions: Vec<SpacetimeTransition>,
}

impl RotorChainProof {
    /// Compose the entire chain into a single equivalent rotor, given a
    /// well-formed proof. Useful for compact checkpointing — the resulting
    /// rotor represents the cumulative state delta from anchor to tip.
    pub fn composed_rotor(&self) -> Rotor {
        self.transitions
            .iter()
            .fold(Rotor::IDENTITY, |acc, t| acc.compose(&t.rotor))
    }

    /// Tip state hash after the last transition.
    pub fn tip_state_hash(&self) -> B256 {
        self.transitions
            .last()
            .map(|t| t.new_state_hash)
            .unwrap_or(self.anchor_state_hash)
    }
}

/// Verify a rotor chain proof statelessly. Returns `Ok(())` if every
/// transition is internally well-formed AND chains correctly to its
/// neighbors.
pub fn verify_rotor_chain(proof: &RotorChainProof) -> Result<(), LightClientError> {
    if proof.wire_version != SPACETIME_WIRE_VERSION {
        return Err(LightClientError::WireVersionMismatch);
    }
    if proof.transitions.is_empty() {
        return Err(LightClientError::EmptyChain);
    }

    let mut prev_hash = proof.anchor_state_hash;
    let mut prev_coord = proof.anchor_coord;
    let mut prev_id = proof.transitions[0].transition_id.saturating_sub(1);

    for (i, t) in proof.transitions.iter().enumerate() {
        // 1. Rotor well-formedness: norm ≈ 1, even-grade.
        let mv = t.rotor.as_multivector();
        let n2 = mv.norm_squared();
        if (n2 - 1.0).abs() > 1e-4 {
            return Err(LightClientError::InvalidRotorAt(i));
        }

        // 2. Monotonic transition IDs.
        if t.transition_id <= prev_id && !(i == 0 && t.transition_id == 0) {
            return Err(LightClientError::NonMonotonicTransitionId);
        }
        prev_id = t.transition_id;

        // 3. Chain consistency: this transition's prev hash matches the
        //    previous transition's new hash.
        if t.prev_state_hash != prev_hash {
            return Err(LightClientError::StateHashMismatchAt(i));
        }

        // 4. Causal forward cone.
        let dt = t.causal_coord.t - prev_coord.t;
        let dx = t.causal_coord.x - prev_coord.x;
        let dy = t.causal_coord.y - prev_coord.y;
        let dz = t.causal_coord.z - prev_coord.z;
        if dt <= 0.0 {
            return Err(LightClientError::CausalViolationAt(i));
        }
        let interval = dt * dt - dx * dx - dy * dy - dz * dz;
        if interval < -1e-9 {
            return Err(LightClientError::CausalViolationAt(i));
        }

        prev_hash = t.new_state_hash;
        prev_coord = t.causal_coord;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::causal::CausalCoord;
    use crate::rotor::Bivector;

    fn make_proof(n: usize) -> RotorChainProof {
        let mut transitions = Vec::new();
        let mut prev = B256::ZERO;
        for i in 0..n {
            let new = B256::from([(i + 1) as u8; 32]);
            let (residual_commitment, residual_norm) =
                SpacetimeTransition::zero_residual_fields(|b| {
                    let mut out = [0u8; 32];
                    for (j, byte) in b.iter().enumerate() {
                        out[j % 32] = out[j % 32].wrapping_add(byte.wrapping_mul(31));
                    }
                    out
                });
            transitions.push(SpacetimeTransition {
                transition_id: i as u64,
                rotor: Rotor::exp(&Bivector {
                    b: [0.0, 0.0, 0.0, 0.01, 0.0, 0.0],
                }),
                prev_state_hash: prev,
                new_state_hash: new,
                causal_coord: CausalCoord {
                    t: (i + 1) as f64,
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                residual_commitment,
                residual_norm,
                aux_commit: None,
            });
            prev = new;
        }
        RotorChainProof {
            wire_version: SPACETIME_WIRE_VERSION,
            anchor_state_hash: B256::ZERO,
            anchor_coord: CausalCoord::ORIGIN,
            transitions,
        }
    }

    #[test]
    fn well_formed_chain_verifies() {
        let p = make_proof(5);
        assert_eq!(verify_rotor_chain(&p), Ok(()));
    }

    #[test]
    fn empty_chain_fails() {
        let mut p = make_proof(1);
        p.transitions.clear();
        assert_eq!(verify_rotor_chain(&p), Err(LightClientError::EmptyChain));
    }

    #[test]
    fn broken_hash_chain_detected() {
        let mut p = make_proof(3);
        p.transitions[1].prev_state_hash = B256::from([99u8; 32]);
        assert_eq!(
            verify_rotor_chain(&p),
            Err(LightClientError::StateHashMismatchAt(1))
        );
    }

    #[test]
    fn backwards_time_detected() {
        let mut p = make_proof(3);
        p.transitions[2].causal_coord.t = 0.5;
        assert_eq!(
            verify_rotor_chain(&p),
            Err(LightClientError::CausalViolationAt(2))
        );
    }
}
