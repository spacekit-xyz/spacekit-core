//! FIPS 203/204/205 (secrets-core): ML-KEM-768, ML-DSA-65, SLH-DSA.
//!
//! Enabled by the `secrets-core` feature. Provides key generation,
//! encapsulation/decapsulation, signing/verification, deterministic derivation
//! (wallet signature or mnemonic), and hybrid encryption.

mod derive;
mod hybrid;
mod kem;
mod keygen;
mod rng;
mod signature;
mod types;

pub use derive::derive_from_wallet_signature;
#[cfg(feature = "std")]
pub use derive::{derive_from_mnemonic, MNEMONIC_DERIVE_SALT};
pub use hybrid::{decrypt_blob, encrypt_for_recipient};
pub use kem::{decapsulate, encapsulate, generate_keypair as kem_generate_keypair};
pub use keygen::generate_keypair;
pub use signature::{derive_pubkey_from_signing_key, generate_signer_keypair, sign, verify};
pub use types::{
    DerivedKeySet, QuantumKeyMaterial, SignerVariant, DERIVE_MESSAGE, KEM_CT_SIZE, KEM_DECAP_SIZE,
    KEM_PK_SIZE, MLDSA_PK_SIZE, MLDSA_SEED_SIZE, MLDSA_SIG_SIZE, SLH_SHA2_128S_PK_SIZE,
    SLH_SHA2_128S_SIG_SIZE, SLH_SHA2_128S_SK_SIZE, SLH_SHA2_192S_PK_SIZE, SLH_SHA2_192S_SIG_SIZE,
    SLH_SHA2_192S_SK_SIZE,
};
