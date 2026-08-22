//! Deterministic key derivation from wallet signature or BIP-39 mnemonic.

use anyhow::{anyhow, Result};
use hkdf::Hkdf;
use ml_dsa::signature::Keypair;
use ml_dsa::{KeyGen, MlDsa65};
use ml_kem::{ml_kem_768::MlKem768, FromSeed, KeyExport};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::v1::secrets_core::types::DerivedKeySet;

/// Domain separator for mnemonic-based derivation (distinct from wallet-signature).
/// Only available with the `std` feature (requires bip39).
#[cfg(feature = "std")]
pub const MNEMONIC_DERIVE_SALT: &[u8] = b"spacekit-secrets-mnemonic-v1";

/// Derive ML-KEM-768 + ML-DSA-65 keys from an Ethereum wallet signature.
///
/// * `wallet_signature`: at least 64 bytes (e.g. 65-byte ECDSA signature from personal_sign).
/// * `wallet_address`: 20-byte Ethereum address.
pub fn derive_from_wallet_signature(
    wallet_signature: &[u8],
    wallet_address: &[u8],
) -> Result<DerivedKeySet> {
    anyhow::ensure!(wallet_signature.len() >= 64, "Signature too short");
    anyhow::ensure!(wallet_address.len() == 20, "Address must be 20 bytes");

    let hkdf = Hkdf::<Sha256>::new(Some(wallet_address), wallet_signature);

    let mut kem_seed = Zeroizing::new([0u8; 64]);
    hkdf.expand(b"ml-kem-768-seed-v1", kem_seed.as_mut())
        .map_err(|_| anyhow!("HKDF expand failed for KEM seed"))?;

    let mut dsa_seed = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"ml-dsa-65-seed-v1", dsa_seed.as_mut())
        .map_err(|_| anyhow!("HKDF expand failed for DSA seed"))?;

    let kem_seed_arr: [u8; 64] = kem_seed.as_slice().try_into()?;
    let seed = ml_kem::Seed::from(kem_seed_arr);
    let (decap, encap) = MlKem768::from_seed(&seed);
    let decap_bytes = Zeroizing::new(decap.to_seed().unwrap().as_slice().to_vec());
    let encap_bytes = encap.to_bytes().as_slice().to_vec();

    let sk = MlDsa65::from_seed(&ml_dsa::Seed::from(*dsa_seed));
    let vk = sk.verifying_key();
    let dsa_signing_bytes = Zeroizing::new(dsa_seed.as_ref().to_vec());
    let dsa_verify_bytes = vk.encode().as_slice().to_vec();

    Ok(DerivedKeySet {
        kem_decap_bytes: decap_bytes,
        kem_encap_bytes: encap_bytes,
        dsa_signing_bytes,
        dsa_verify_bytes,
    })
}

/// Derive ML-KEM-768 + ML-DSA-65 keys from a BIP-39 mnemonic (and optional passphrase).
/// Only available with the `std` feature (requires bip39).
#[cfg(feature = "std")]
pub fn derive_from_mnemonic(mnemonic: &str, passphrase: Option<&str>) -> Result<DerivedKeySet> {
    let m =
        bip39::Mnemonic::parse(mnemonic.trim()).map_err(|e| anyhow!("Invalid mnemonic: {}", e))?;
    let pass = passphrase.unwrap_or("");
    let seed: [u8; 64] = m.to_seed(pass);

    let hkdf = Hkdf::<Sha256>::new(Some(MNEMONIC_DERIVE_SALT), &seed);

    let mut kem_seed = Zeroizing::new([0u8; 64]);
    hkdf.expand(b"ml-kem-768-seed-v1", kem_seed.as_mut())
        .map_err(|_| anyhow!("HKDF expand failed for KEM seed"))?;

    let mut dsa_seed = Zeroizing::new([0u8; 32]);
    hkdf.expand(b"ml-dsa-65-seed-v1", dsa_seed.as_mut())
        .map_err(|_| anyhow!("HKDF expand failed for DSA seed"))?;

    let kem_seed_arr: [u8; 64] = kem_seed.as_slice().try_into()?;
    let seed = ml_kem::Seed::from(kem_seed_arr);
    let (decap, encap) = MlKem768::from_seed(&seed);
    let decap_bytes = Zeroizing::new(decap.to_seed().unwrap().as_slice().to_vec());
    let encap_bytes = encap.to_bytes().as_slice().to_vec();

    let sk = MlDsa65::from_seed(&ml_dsa::Seed::from(*dsa_seed));
    let vk = sk.verifying_key();
    let dsa_signing_bytes = Zeroizing::new(dsa_seed.as_ref().to_vec());
    let dsa_verify_bytes = vk.encode().as_slice().to_vec();

    Ok(DerivedKeySet {
        kem_decap_bytes: decap_bytes,
        kem_encap_bytes: encap_bytes,
        dsa_signing_bytes,
        dsa_verify_bytes,
    })
}
