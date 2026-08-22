//! ML-KEM-768 (FIPS 203) key encapsulation.

#[cfg(feature = "no_std")]
use alloc::vec::Vec;

use anyhow::{anyhow, Result};
use zeroize::Zeroizing;

use crate::v1::secrets_core::types::{KEM_CT_SIZE, KEM_DECAP_SIZE, KEM_PK_SIZE};

use ml_kem::{ml_kem_768::MlKem768, Decapsulate, Encapsulate, Kem, KeyExport, TryKeyInit};

/// Generate a random ML-KEM-768 keypair.
/// Returns (decap_key_seed_64bytes, encap_key_bytes).
pub fn generate_keypair() -> Result<(Zeroizing<[u8; KEM_DECAP_SIZE]>, Vec<u8>)> {
    let (decap, encap) = MlKem768::generate_keypair();
    let seed = decap.to_seed().unwrap();
    let encap_key = encap.to_bytes();
    let encap_bytes = encap_key.as_slice().to_vec();
    Ok((Zeroizing::new(seed.as_slice().try_into()?), encap_bytes))
}

/// Encapsulate a shared secret to the given encapsulation key.
/// Returns (ciphertext, shared_secret_32bytes).
pub fn encapsulate(encap_key_bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let pk: [u8; KEM_PK_SIZE] = encap_key_bytes
        .try_into()
        .map_err(|_| anyhow!("Invalid encap key length: expected {}", KEM_PK_SIZE))?;
    let encap = ml_kem::EncapsulationKey::<MlKem768>::new_from_slice(&pk)
        .map_err(|e| anyhow!("Invalid encap key: {:?}", e))?;
    let (ct, shared) = encap.encapsulate();
    let ct_bytes = ct.as_slice().to_vec();
    let shared_bytes = shared.as_slice().to_vec();
    Ok((ct_bytes, shared_bytes))
}

/// Decapsulate using the 64-byte decapsulation key seed.
pub fn decapsulate(decap_seed: &[u8; KEM_DECAP_SIZE], ciphertext: &[u8]) -> Result<Vec<u8>> {
    let ct: [u8; KEM_CT_SIZE] = ciphertext
        .try_into()
        .map_err(|_| anyhow!("Invalid ciphertext length: expected {}", KEM_CT_SIZE))?;
    let decap = ml_kem::DecapsulationKey::<MlKem768>::from_seed(ml_kem::Seed::from(*decap_seed));
    let shared = decap
        .decapsulate_slice(&ct)
        .map_err(|e| anyhow!("Decapsulation failed: {:?}", e))?;
    Ok(shared.as_slice().to_vec())
}
