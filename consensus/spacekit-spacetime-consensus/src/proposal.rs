//! Spacetime transition data attached to consensus proposals.
//!
//! A `SpacetimeTransition` is the rotor-valued representation of a block's
//! state delta PLUS a commitment to the residual (the part the rotor doesn't
//! capture). It is included as a side-car to your existing
//! `QuantumSafeProposal` and `QuantumSafeVote` without altering the wire
//! format of those types, the side-car is hashed into the proposal's
//! `block_hash` so the existing Dilithium + SPHINCS+ signatures cover it.

use crate::algebra::Multivector;
use crate::causal::CausalCoord;
use crate::rotor::Rotor;
use alloy_primitives::B256;

/// A claimed state transition expressed as a Spin⁺(1,3) rotor plus the
/// residual commitment for non-Lorentz-shaped state changes.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SpacetimeTransition {
    pub transition_id: u64,
    pub rotor: Rotor,
    pub prev_state_hash: B256,
    pub new_state_hash: B256,
    pub causal_coord: CausalCoord,
    /// Domain-tagged hash of the residual multivector Δ.
    pub residual_commitment: B256,
    /// L2 norm of the residual multivector.
    pub residual_norm: f64,
    pub aux_commit: Option<B256>,
}

impl SpacetimeTransition {
    pub const SERIALIZED_SIZE: usize = 240;
    pub const SERIALIZED_SIZE_NO_AUX: usize = 208;

    pub const RESIDUAL_DOMAIN: &[u8] = b"spacekit-spacetime-residual-v2";

    pub fn commit_residual<F: Fn(&[u8]) -> [u8; 32]>(residual: &Multivector, hash_fn: F) -> B256 {
        let bytes = residual.to_bytes();
        let mut buf = alloc::vec::Vec::with_capacity(Self::RESIDUAL_DOMAIN.len() + 1 + 128);
        buf.extend_from_slice(Self::RESIDUAL_DOMAIN);
        buf.push(0x1f);
        buf.extend_from_slice(&bytes);
        B256::from(hash_fn(&buf))
    }

    pub fn compute_residual_norm(residual: &Multivector) -> f64 {
        let mut s = 0.0;
        for c in residual.coeffs.iter() {
            s += c * c;
        }
        #[cfg(feature = "std")]
        {
            s.sqrt()
        }
        #[cfg(not(feature = "std"))]
        {
            libm::sqrt(s)
        }
    }

    /// Residual fields for Δ = 0 (rotor-only / test transitions).
    pub fn zero_residual_fields<F: Fn(&[u8]) -> [u8; 32]>(hash_fn: F) -> (B256, f64) {
        let zero = Multivector::ZERO;
        (Self::commit_residual(&zero, hash_fn), 0.0)
    }

    pub fn to_bytes(&self) -> [u8; Self::SERIALIZED_SIZE] {
        let mut out = [0u8; Self::SERIALIZED_SIZE];
        let mut o = 0;
        out[o..o + 8].copy_from_slice(&self.transition_id.to_le_bytes());
        o += 8;
        let mv = self.rotor.as_multivector();
        for &i in &[0usize, 5, 6, 7, 8, 9, 10, 15] {
            out[o..o + 8].copy_from_slice(&mv.coeffs[i].to_le_bytes());
            o += 8;
        }
        out[o..o + 32].copy_from_slice(self.prev_state_hash.as_slice());
        o += 32;
        out[o..o + 32].copy_from_slice(self.new_state_hash.as_slice());
        o += 32;
        out[o..o + 8].copy_from_slice(&self.causal_coord.t.to_le_bytes());
        o += 8;
        out[o..o + 8].copy_from_slice(&self.causal_coord.x.to_le_bytes());
        o += 8;
        out[o..o + 8].copy_from_slice(&self.causal_coord.y.to_le_bytes());
        o += 8;
        out[o..o + 8].copy_from_slice(&self.causal_coord.z.to_le_bytes());
        o += 8;
        out[o..o + 32].copy_from_slice(self.residual_commitment.as_slice());
        o += 32;
        out[o..o + 8].copy_from_slice(&self.residual_norm.to_le_bytes());
        o += 8;
        if let Some(aux) = &self.aux_commit {
            out[o..o + 32].copy_from_slice(aux.as_slice());
        }
        out
    }

    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < Self::SERIALIZED_SIZE_NO_AUX {
            return None;
        }
        let mut o = 0;
        let transition_id = u64::from_le_bytes(b[o..o + 8].try_into().ok()?);
        o += 8;
        let mut mv = Multivector::ZERO;
        for &i in &[0usize, 5, 6, 7, 8, 9, 10, 15] {
            mv.coeffs[i] = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
            o += 8;
        }
        let rotor = Rotor::renormalize(mv).ok()?;
        let prev_state_hash = B256::from_slice(&b[o..o + 32]);
        o += 32;
        let new_state_hash = B256::from_slice(&b[o..o + 32]);
        o += 32;
        let t = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
        o += 8;
        let x = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
        o += 8;
        let y = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
        o += 8;
        let z = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
        o += 8;
        let residual_commitment = B256::from_slice(&b[o..o + 32]);
        o += 32;
        let residual_norm = f64::from_le_bytes(b[o..o + 8].try_into().ok()?);
        o += 8;
        let aux_commit = if b.len() >= Self::SERIALIZED_SIZE {
            Some(B256::from_slice(&b[o..o + 32]))
        } else {
            None
        };
        Some(Self {
            transition_id,
            rotor,
            prev_state_hash,
            new_state_hash,
            causal_coord: CausalCoord { t, x, y, z },
            residual_commitment,
            residual_norm,
            aux_commit,
        })
    }

    pub fn digest<F: Fn(&[u8]) -> [u8; 32]>(&self, hash_fn: F) -> B256 {
        const DOMAIN: &[u8] = b"spacekit-spacetime-transition-v2";
        let mut buf = alloc::vec::Vec::with_capacity(DOMAIN.len() + 1 + Self::SERIALIZED_SIZE);
        buf.extend_from_slice(DOMAIN);
        buf.push(0x1f);
        let serialized = self.to_bytes();
        let len = if self.aux_commit.is_some() {
            Self::SERIALIZED_SIZE
        } else {
            Self::SERIALIZED_SIZE_NO_AUX
        };
        buf.extend_from_slice(&serialized[..len]);
        B256::from(hash_fn(&buf))
    }

    pub fn joint_signature(&self) -> (f64, f64) {
        let rotor_magnitude = match self.rotor.log() {
            Ok(b) => {
                #[cfg(feature = "std")]
                {
                    b.square_scalar().abs().sqrt()
                }
                #[cfg(not(feature = "std"))]
                {
                    libm::sqrt(b.square_scalar().abs())
                }
            }
            Err(_) => 0.0,
        };
        (rotor_magnitude, self.residual_norm)
    }
}

/// A witness binding a `SpacetimeTransition` to a `QuantumSafeProposal`.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TransitionWitness {
    pub transition: SpacetimeTransition,
    pub proposal_hash: B256,
    pub post_state_witness: B256,
}

impl TransitionWitness {
    pub fn rotor_digest<F: Fn(&[u8]) -> [u8; 32]>(&self, hash_fn: F) -> B256 {
        self.transition.digest(hash_fn)
    }

    pub fn from_parts(
        transition: SpacetimeTransition,
        proposal_hash: B256,
        post_state_witness: B256,
    ) -> Self {
        Self {
            transition,
            proposal_hash,
            post_state_witness,
        }
    }

    #[cfg(feature = "pq-signatures")]
    pub fn from_vote(
        vote: &crate::pq_envelope::ConsensusVoteInner,
        transition: &SpacetimeTransition,
        hash_fn: impl Fn(&[u8]) -> [u8; 32],
    ) -> Option<Self> {
        let digest = transition.digest(&hash_fn);
        if digest != vote.validator_rotor_digest {
            return None;
        }
        Some(Self {
            transition: *transition,
            proposal_hash: vote.proposal_hash,
            post_state_witness: transition.new_state_hash,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotor::Bivector;

    fn h(b: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, byte) in b.iter().enumerate() {
            out[i % 32] = out[i % 32].wrapping_add(byte.wrapping_mul(31));
        }
        out
    }

    #[test]
    fn round_trip_v2_with_aux() {
        let r = Rotor::exp(&Bivector {
            b: [0.1, 0.0, 0.0, 0.2, 0.0, 0.0],
        });
        let mut residual = Multivector::ZERO;
        residual.coeffs[1] = 0.05;
        let t = SpacetimeTransition {
            transition_id: 42,
            rotor: r,
            prev_state_hash: B256::from([1u8; 32]),
            new_state_hash: B256::from([2u8; 32]),
            causal_coord: CausalCoord {
                t: 1.0,
                x: 0.1,
                y: 0.2,
                z: 0.3,
            },
            residual_commitment: SpacetimeTransition::commit_residual(&residual, h),
            residual_norm: SpacetimeTransition::compute_residual_norm(&residual),
            aux_commit: Some(B256::from([3u8; 32])),
        };
        let bytes = t.to_bytes();
        let parsed = SpacetimeTransition::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.transition_id, t.transition_id);
        assert_eq!(parsed.residual_commitment, t.residual_commitment);
        assert!((parsed.residual_norm - t.residual_norm).abs() < 1e-12);
        assert!(r.distance(&parsed.rotor) < 1e-9);
    }

    #[test]
    fn round_trip_v2_no_aux() {
        let r = Rotor::IDENTITY;
        let (rc, rn) = SpacetimeTransition::zero_residual_fields(h);
        let t = SpacetimeTransition {
            transition_id: 0,
            rotor: r,
            prev_state_hash: B256::ZERO,
            new_state_hash: B256::ZERO,
            causal_coord: CausalCoord::ORIGIN,
            residual_commitment: rc,
            residual_norm: rn,
            aux_commit: None,
        };
        let bytes = t.to_bytes();
        let parsed =
            SpacetimeTransition::from_bytes(&bytes[..SpacetimeTransition::SERIALIZED_SIZE_NO_AUX])
                .unwrap();
        assert!(parsed.aux_commit.is_none());
    }

    #[test]
    fn residual_commitment_distinguishes_residuals() {
        let mut residual_a = Multivector::ZERO;
        residual_a.coeffs[1] = 0.01;
        let mut residual_b = Multivector::ZERO;
        residual_b.coeffs[1] = 0.05;

        let commit_a = SpacetimeTransition::commit_residual(&residual_a, h);
        let commit_b = SpacetimeTransition::commit_residual(&residual_b, h);
        assert_ne!(commit_a, commit_b);

        let norm_a = SpacetimeTransition::compute_residual_norm(&residual_a);
        let norm_b = SpacetimeTransition::compute_residual_norm(&residual_b);
        assert!(norm_b > norm_a);
    }

    #[test]
    fn joint_signature_catches_residual_swap_attack() {
        let r = Rotor::exp(&Bivector {
            b: [0.0, 0.0, 0.0, 0.1, 0.0, 0.0],
        });
        let t_innocent = SpacetimeTransition {
            transition_id: 0,
            rotor: r,
            prev_state_hash: B256::ZERO,
            new_state_hash: B256::ZERO,
            causal_coord: CausalCoord {
                t: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment: B256::ZERO,
            residual_norm: 0.01,
            aux_commit: None,
        };
        let mut t_attack = t_innocent;
        t_attack.residual_norm = 5.0;
        t_attack.residual_commitment = B256::from([0xFF; 32]);

        let sig_innocent = t_innocent.joint_signature();
        let sig_attack = t_attack.joint_signature();
        assert_eq!(sig_innocent.0, sig_attack.0);
        assert!(sig_attack.1 > sig_innocent.1 * 100.0);
    }
}
