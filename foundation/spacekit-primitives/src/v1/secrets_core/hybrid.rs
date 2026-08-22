//! Hybrid encryption: ML-KEM-768 + AES-256-GCM.

#[cfg(feature = "no_std")]
use alloc::vec::Vec;

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use anyhow::{anyhow, Result};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::v1::secrets_core::kem;
use crate::v1::secrets_core::types::{KEM_CT_SIZE, KEM_DECAP_SIZE};

/// Encrypt `plaintext` for the holder of `encap_key_bytes`. `context` (e.g. secret ID) binds the ciphertext.
/// Blob layout: [KEM ciphertext (1088) || IV (12) || AES-GCM ciphertext + tag].
pub fn encrypt_for_recipient(
    plaintext: &[u8],
    encap_key_bytes: &[u8],
    context: &[u8],
) -> Result<Vec<u8>> {
    let (kem_ct, shared_secret) = kem::encapsulate(encap_key_bytes)?;
    let aes_key = derive_aes_key(shared_secret.as_slice(), context);
    let cipher = Aes256Gcm::new_from_slice(&aes_key).map_err(|e| anyhow!("AES key init: {}", e))?;
    let mut iv = [0u8; 12];
    getrandom::fill(&mut iv).map_err(|e| anyhow!("getrandom failed: {}", e))?;
    let aes_ct = cipher
        .encrypt(Nonce::from_slice(&iv), plaintext)
        .map_err(|e| anyhow!("AES-GCM encrypt: {}", e))?;

    let mut blob = Vec::with_capacity(KEM_CT_SIZE + 12 + aes_ct.len());
    blob.extend_from_slice(&kem_ct);
    blob.extend_from_slice(&iv);
    blob.extend_from_slice(&aes_ct);
    Ok(blob)
}

/// Decrypt blob using the 64-byte decapsulation key seed.
pub fn decrypt_blob(
    blob: &[u8],
    decap_bytes: &Zeroizing<Vec<u8>>,
    context: &[u8],
) -> Result<Vec<u8>> {
    if blob.len() < KEM_CT_SIZE + 12 + 16 {
        return Err(anyhow!("Blob too short"));
    }
    let decap_arr: [u8; KEM_DECAP_SIZE] = decap_bytes
        .as_slice()
        .try_into()
        .map_err(|_| anyhow!("Invalid decap key length: expected {}", KEM_DECAP_SIZE))?;
    let shared = kem::decapsulate(&decap_arr, &blob[..KEM_CT_SIZE])?;
    let aes_key = derive_aes_key(shared.as_slice(), context);
    let cipher = Aes256Gcm::new_from_slice(&aes_key).map_err(|e| anyhow!("AES key init: {}", e))?;
    cipher
        .decrypt(
            Nonce::from_slice(&blob[KEM_CT_SIZE..KEM_CT_SIZE + 12]),
            &blob[KEM_CT_SIZE + 12..],
        )
        .map_err(|_| anyhow!("AES-GCM decryption failed"))
}

fn derive_aes_key(shared_secret: &[u8], context: &[u8]) -> [u8; 32] {
    let hkdf = Hkdf::<Sha256>::new(Some(context), shared_secret);
    let mut key = [0u8; 32];
    hkdf.expand(b"quantum-secrets-manager-v1", &mut key)
        .expect("HKDF expand");
    key
}
