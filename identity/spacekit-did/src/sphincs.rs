//! Core SPHINCS+ quantum-resistant cryptographic primitives.
//!
//! This module is `no_std` compatible and provides keygen, sign, and verify
//! operations using the SPHINCS+-SHAKE-256-128s-simple parameter set.

use alloc::string::String;
use alloc::vec::Vec;
use pqcrypto_sphincsplus::sphincsshake256ssimple::{DetachedSignature, PublicKey, SecretKey};
use pqcrypto_traits::sign::{
    DetachedSignature as DetachedSignatureTrait, PublicKey as PublicKeyTrait,
    SecretKey as SecretKeyTrait,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumKeyPair {
    pub public_key: Vec<u8>,
    pub private_key: Vec<u8>,
    pub algorithm: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CryptoError {
    InvalidPrivateKey,
}

pub struct SphincsPlus;

impl SphincsPlus {
    pub fn generate_keypair() -> QuantumKeyPair {
        let (pk, sk) = pqcrypto_sphincsplus::sphincsshake256ssimple::keypair();
        QuantumKeyPair {
            public_key: pk.as_bytes().to_vec(),
            private_key: sk.as_bytes().to_vec(),
            algorithm: String::from("SPHINCS+-SHAKE-256-128s-simple"),
        }
    }

    pub fn sign(private_key: &[u8], message: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let sk = SecretKey::from_bytes(private_key).map_err(|_| CryptoError::InvalidPrivateKey)?;
        let sig = pqcrypto_sphincsplus::sphincsshake256ssimple::detached_sign(message, &sk);
        Ok(sig.as_bytes().to_vec())
    }

    pub fn verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        let pk = match PublicKey::from_bytes(public_key) {
            Ok(k) => k,
            Err(_) => return false,
        };
        let sig = match DetachedSignature::from_bytes(signature) {
            Ok(s) => s,
            Err(_) => return false,
        };
        pqcrypto_sphincsplus::sphincsshake256ssimple::verify_detached_signature(&sig, message, &pk)
            .is_ok()
    }
}
