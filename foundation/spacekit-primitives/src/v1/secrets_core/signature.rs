//! ML-DSA-65 and SLH-DSA (FIPS 204/205) sign and verify.

#[cfg(feature = "no_std")]
use alloc::vec::Vec;

use anyhow::{anyhow, Result};
use zeroize::Zeroizing;

use crate::v1::secrets_core::types::{
    SignerVariant, MLDSA_PK_SIZE, MLDSA_SEED_SIZE, SLH_SHA2_128S_PK_SIZE, SLH_SHA2_192S_PK_SIZE,
};

use ml_dsa::{KeyGen, MlDsa65};
use slh_dsa::{
    signature::Keypair, Sha2_128s, Sha2_192s, SigningKey as SlhSigningKey,
    VerifyingKey as SlhVerifyingKey,
};

use super::rng::OsRng;

/// Generate a random signer keypair for the given variant.
/// Returns (sk_bytes, pk_bytes).
pub fn generate_signer_keypair(variant: &SignerVariant) -> Result<(Zeroizing<Vec<u8>>, Vec<u8>)> {
    match variant {
        SignerVariant::MlDsa65 => {
            use ml_dsa::signature::Keypair;
            let sk = MlDsa65::key_gen(&mut OsRng);
            let seed = sk.to_seed();
            let sk_bytes = Zeroizing::new(seed.as_slice().to_vec());
            let vk = sk.verifying_key();
            let pk_bytes = vk.encode().as_slice().to_vec();
            Ok((sk_bytes, pk_bytes))
        }
        SignerVariant::SlhDsaSha2128s => {
            let sk = SlhSigningKey::<Sha2_128s>::new(&mut OsRng);
            let vk = sk.verifying_key();
            let sk_bytes = Zeroizing::new(sk.to_bytes().as_slice().to_vec());
            let pk_bytes = vk.to_bytes().as_slice().to_vec();
            Ok((sk_bytes, pk_bytes))
        }
        SignerVariant::SlhDsaSha2192s => {
            let sk = SlhSigningKey::<Sha2_192s>::new(&mut OsRng);
            let vk = sk.verifying_key();
            let sk_bytes = Zeroizing::new(sk.to_bytes().as_slice().to_vec());
            let pk_bytes = vk.to_bytes().as_slice().to_vec();
            Ok((sk_bytes, pk_bytes))
        }
    }
}

/// Sign `message` with the given signing key bytes and variant.
pub fn sign(sk_bytes: &[u8], message: &[u8], variant: &SignerVariant) -> Result<Vec<u8>> {
    match variant {
        SignerVariant::MlDsa65 => {
            use ml_dsa::signature::Signer;
            let seed: [u8; MLDSA_SEED_SIZE] = sk_bytes.try_into().map_err(|_| {
                anyhow!(
                    "Invalid ML-DSA key length: expected {} (seed)",
                    MLDSA_SEED_SIZE
                )
            })?;
            let sk = MlDsa65::from_seed(&ml_dsa::Seed::from(seed));
            let sig = sk.sign(message);
            Ok(sig.encode().as_slice().to_vec())
        }
        SignerVariant::SlhDsaSha2128s => {
            use slh_dsa::signature::Signer;
            let sk = SlhSigningKey::<Sha2_128s>::try_from(sk_bytes)
                .map_err(|e| anyhow!("Invalid SLH-DSA-128s key: {:?}", e))?;
            let sig = sk.sign(message);
            Ok(sig.to_bytes().as_slice().to_vec())
        }
        SignerVariant::SlhDsaSha2192s => {
            use slh_dsa::signature::Signer;
            let sk = SlhSigningKey::<Sha2_192s>::try_from(sk_bytes)
                .map_err(|e| anyhow!("Invalid SLH-DSA-192s key: {:?}", e))?;
            let sig = sk.sign(message);
            Ok(sig.to_bytes().as_slice().to_vec())
        }
    }
}

/// Verify signature over `message` with the given public key and variant.
pub fn verify(
    pk_bytes: &[u8],
    message: &[u8],
    sig_bytes: &[u8],
    variant: &SignerVariant,
) -> Result<()> {
    use ml_dsa::signature::Verifier as MlVerifier;

    match variant {
        SignerVariant::MlDsa65 => {
            let pk_arr: [u8; MLDSA_PK_SIZE] = pk_bytes
                .try_into()
                .map_err(|_| anyhow!("Invalid ML-DSA pubkey length"))?;
            let enc = ml_dsa::EncodedVerifyingKey::<MlDsa65>::from(pk_arr);
            let vk = ml_dsa::VerifyingKey::<MlDsa65>::decode(&enc);
            let sig = ml_dsa::Signature::<MlDsa65>::try_from(sig_bytes)
                .map_err(|e| anyhow!("Invalid ML-DSA signature: {:?}", e))?;
            vk.verify(message, &sig)
                .map_err(|_| anyhow!("ML-DSA-65 signature verification failed"))
        }
        SignerVariant::SlhDsaSha2128s => {
            let pk_arr: [u8; SLH_SHA2_128S_PK_SIZE] = pk_bytes
                .try_into()
                .map_err(|_| anyhow!("Invalid SLH-DSA-128s pubkey length"))?;
            let vk = SlhVerifyingKey::<Sha2_128s>::try_from(pk_arr.as_ref())
                .map_err(|e| anyhow!("Invalid SLH-DSA-128s pubkey: {:?}", e))?;
            let sig = slh_dsa::Signature::<Sha2_128s>::try_from(sig_bytes)
                .map_err(|e| anyhow!("Invalid SLH-DSA-128s signature: {:?}", e))?;
            vk.verify(message, &sig)
                .map_err(|_| anyhow!("SLH-DSA-SHA2-128s verification failed"))
        }
        SignerVariant::SlhDsaSha2192s => {
            let pk_arr: [u8; SLH_SHA2_192S_PK_SIZE] = pk_bytes
                .try_into()
                .map_err(|_| anyhow!("Invalid SLH-DSA-192s pubkey length"))?;
            let vk = SlhVerifyingKey::<Sha2_192s>::try_from(pk_arr.as_ref())
                .map_err(|e| anyhow!("Invalid SLH-DSA-192s pubkey: {:?}", e))?;
            let sig = slh_dsa::Signature::<Sha2_192s>::try_from(sig_bytes)
                .map_err(|e| anyhow!("Invalid SLH-DSA-192s signature: {:?}", e))?;
            vk.verify(message, &sig)
                .map_err(|_| anyhow!("SLH-DSA-SHA2-192s verification failed"))
        }
    }
}

/// Derive public key bytes from signing key bytes (for the given variant).
pub fn derive_pubkey_from_signing_key(
    sk: &Zeroizing<Vec<u8>>,
    variant: &SignerVariant,
) -> Result<Vec<u8>> {
    match variant {
        SignerVariant::MlDsa65 => {
            use ml_dsa::signature::Keypair;
            let seed: [u8; MLDSA_SEED_SIZE] = sk
                .as_slice()
                .try_into()
                .map_err(|_| anyhow!("Invalid ML-DSA key length"))?;
            let sk_inner = MlDsa65::from_seed(&ml_dsa::Seed::from(seed));
            Ok(sk_inner.verifying_key().encode().as_slice().to_vec())
        }
        SignerVariant::SlhDsaSha2128s => {
            let sk_inner = SlhSigningKey::<Sha2_128s>::try_from(sk.as_slice())
                .map_err(|e| anyhow!("Invalid SLH-DSA-128s key: {:?}", e))?;
            Ok(sk_inner.verifying_key().to_bytes().as_slice().to_vec())
        }
        SignerVariant::SlhDsaSha2192s => {
            let sk_inner = SlhSigningKey::<Sha2_192s>::try_from(sk.as_slice())
                .map_err(|e| anyhow!("Invalid SLH-DSA-192s key: {:?}", e))?;
            Ok(sk_inner.verifying_key().to_bytes().as_slice().to_vec())
        }
    }
}
