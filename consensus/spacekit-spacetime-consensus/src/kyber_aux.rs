//! Kyber1024 KEM wrapper for sealing Verkle hiding-mode `aux` randomness.
//!
//! Your existing consensus already uses Kyber1024 for key encapsulation.
//! The Verkle crate's hiding profile needs caller-supplied randomness via
//! `set_with_aux`, and that aux must be:
//!   1. Random per commitment.
//!   2. Recoverable by the legitimate revealer.
//!   3. Confidential to everyone else until reveal.
//!
//! Kyber1024 gives us all three at the IND-CCA2 post-quantum security level.
//! The recipe is:
//!   - Proposer holds a Kyber1024 keypair (already part of validator identity).
//!   - For each new transition, proposer generates 32 bytes of aux entropy.
//!   - Proposer encapsulates a fresh secret using the network-wide reveal
//!     key (held by the validator set under threshold), then uses the
//!     shared secret as the AEAD key over the aux entropy.
//!   - The Verkle commitment binds the aux; the ciphertext is what gets
//!     gossiped pre-reveal.
//!
//! This module provides the seal/unseal interface; key management plumbing
//! lives in your existing identity layer.

use alloc::vec::Vec;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum KyberAuxError {
    EncapsulationFailed,
    DecapsulationFailed,
    AeadFailed,
    BadCiphertextLength,
}

/// A sealed aux payload: Kyber1024 ciphertext + AEAD-encrypted aux bytes.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SealedAux {
    /// Kyber1024 KEM ciphertext (1568 bytes).
    pub kem_ct: Vec<u8>,
    /// AEAD-encrypted aux randomness + tag.
    pub aead_ct: Vec<u8>,
    /// AEAD nonce (12 bytes for AES-GCM, 24 for XChaCha20-Poly1305).
    pub nonce: Vec<u8>,
}

/// Trait that abstracts the underlying KEM + AEAD pair. Implemented in your
/// quantum-crypto crate; this module just provides the consensus-layer interface.
pub trait AuxSealer {
    /// Seal `aux` for the holders of the public key bundle. Returns a
    /// `SealedAux` that can be embedded as `aux_commit` evidence.
    fn seal_aux(&self, aux: &[u8; 32], recipient_pk: &[u8]) -> Result<SealedAux, KyberAuxError>;

    /// Unseal `aux` using the corresponding secret key.
    fn unseal_aux(
        &self,
        sealed: &SealedAux,
        recipient_sk: &[u8],
    ) -> Result<[u8; 32], KyberAuxError>;
}

/// Reference structure of the implementation, for documentation. The actual
/// implementation lives in `spacekit-quantum-crypto` so all PQ primitives are
/// in one place.
pub struct Kyber1024AesGcmSealer;

impl Kyber1024AesGcmSealer {
    pub const KYBER_CT_LEN: usize = 1568;
    pub const KYBER_PK_LEN: usize = 1568;
    pub const KYBER_SK_LEN: usize = 3168;
    pub const KYBER_SS_LEN: usize = 32;
}
