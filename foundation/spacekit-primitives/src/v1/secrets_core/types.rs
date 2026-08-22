//! Types and constants for FIPS 203/204/205 (secrets-core).

#[cfg(feature = "no_std")]
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

// ─── Constants (NIST ML-KEM-768, ML-DSA-65, SLH-DSA) ───────────────────────

pub const KEM_PK_SIZE: usize = 1184;
pub const KEM_CT_SIZE: usize = 1088;
/// ML-KEM 0.3 uses 64-byte Seed for decapsulation key (preferred over expanded form).
pub const KEM_DECAP_SIZE: usize = 64;

pub const MLDSA_PK_SIZE: usize = 1952;
/// ML-DSA 0.1 uses 32-byte Seed for signing key (preferred).
pub const MLDSA_SEED_SIZE: usize = 32;
pub const MLDSA_SIG_SIZE: usize = 3309;

pub const SLH_SHA2_128S_PK_SIZE: usize = 32;
pub const SLH_SHA2_128S_SK_SIZE: usize = 64;
pub const SLH_SHA2_128S_SIG_SIZE: usize = 7856;

pub const SLH_SHA2_192S_PK_SIZE: usize = 48;
pub const SLH_SHA2_192S_SK_SIZE: usize = 96;
pub const SLH_SHA2_192S_SIG_SIZE: usize = 16224;

// ─── Signer variant ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SignerVariant {
    /// Lattice-based, fastest signing (~ms), FIPS 204
    MlDsa65,
    /// Hash-only (SPHINCS+), FIPS 205. Slower (~1s).
    SlhDsaSha2128s,
    /// Hash-only, 192-bit security. Slowest (~4s).
    SlhDsaSha2192s,
}

impl SignerVariant {
    pub fn sig_size(&self) -> usize {
        match self {
            Self::MlDsa65 => MLDSA_SIG_SIZE,
            Self::SlhDsaSha2128s => SLH_SHA2_128S_SIG_SIZE,
            Self::SlhDsaSha2192s => SLH_SHA2_192S_SIG_SIZE,
        }
    }

    pub fn pk_size(&self) -> usize {
        match self {
            Self::MlDsa65 => MLDSA_PK_SIZE,
            Self::SlhDsaSha2128s => SLH_SHA2_128S_PK_SIZE,
            Self::SlhDsaSha2192s => SLH_SHA2_192S_PK_SIZE,
        }
    }

    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::MlDsa65),
            1 => Some(Self::SlhDsaSha2128s),
            2 => Some(Self::SlhDsaSha2192s),
            _ => None,
        }
    }

    pub fn to_u8(&self) -> u8 {
        match self {
            Self::MlDsa65 => 0,
            Self::SlhDsaSha2128s => 1,
            Self::SlhDsaSha2192s => 2,
        }
    }
}

// ─── Key material (full keypair: KEM + signer) ─────────────────────────────

/// SignerVariant holds no secret bytes; we implement Zeroize as no-op for ZeroizeOnDrop on QuantumKeyMaterial.
impl Zeroize for SignerVariant {
    fn zeroize(&mut self) {}
}

#[derive(ZeroizeOnDrop)]
pub struct QuantumKeyMaterial {
    /// ML-KEM-768 decapsulation key (64-byte seed form in ml-kem 0.3)
    pub kem_decap_bytes: Zeroizing<Vec<u8>>,
    pub signer_sk_bytes: Zeroizing<Vec<u8>>,
    pub signer_variant: SignerVariant,
    pub kem_encap_bytes: Vec<u8>,
    pub signer_pk_bytes: Vec<u8>,
}

// ─── Derived key set (wallet or mnemonic derivation) ────────────────────────

#[derive(ZeroizeOnDrop)]
pub struct DerivedKeySet {
    pub kem_decap_bytes: Zeroizing<Vec<u8>>,
    pub kem_encap_bytes: Vec<u8>,
    pub dsa_signing_bytes: Zeroizing<Vec<u8>>,
    pub dsa_verify_bytes: Vec<u8>,
}

/// Message shown to user for wallet-sign derivation (EIP-191 personal_sign).
pub const DERIVE_MESSAGE: &str = "Quantum Secrets Manager: authorize key derivation\n\
     This signature deterministically derives your encryption and signing keys.\n\
     Only sign this on trusted interfaces. Never sign on sites you don't control.";
