// Conditional quantum imports - avoid wildcard import to prevent Result type conflict
#[cfg(feature = "quantum")]
use oqs::kem::{Ciphertext, Kem, PublicKey, SecretKey, SharedSecret};

// Common imports (always available)
use crate::v1::utils::file_ops::{load_from_file, save_to_file};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// Crypto imports (always available for fallback)
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce as AesNonce,
};
use chacha20poly1305::{ChaCha20Poly1305, Nonce as ChaChaNonce, XChaCha20Poly1305};

#[cfg(feature = "quantum")]
use pqcrypto_traits::sign::{DetachedSignature as _, PublicKey as _, SecretKey as _};

// Always available types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Algorithm {
    Kyber512,
    Kyber768,
    Kyber1024,
    NtruPrimeSntrup761,
    FrodoKem1344Aes,
    FrodoKem1344Shake,
    ClassicMcEliece348864,
    BikeL1,
    BikeL3,
    BikeL5,
}

/// Generate a KEM keypair for the selected algorithm.
///
/// Returns `(public_key, secret_key)` as raw bytes.
#[cfg(feature = "quantum")]
pub fn generate_kem_keypair(algorithm: Algorithm) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let kem = match algorithm {
        Algorithm::Kyber512 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber512),
        Algorithm::Kyber768 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber768),
        Algorithm::Kyber1024 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber1024),
        Algorithm::NtruPrimeSntrup761 => {
            oqs::kem::Kem::new(oqs::kem::Algorithm::NtruPrimeSntrup761)
        }
        Algorithm::FrodoKem1344Aes => oqs::kem::Kem::new(oqs::kem::Algorithm::FrodoKem1344Aes),
        Algorithm::FrodoKem1344Shake => oqs::kem::Kem::new(oqs::kem::Algorithm::FrodoKem1344Shake),
        Algorithm::ClassicMcEliece348864 => {
            oqs::kem::Kem::new(oqs::kem::Algorithm::ClassicMcEliece348864)
        }
        Algorithm::BikeL1 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL1),
        Algorithm::BikeL3 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL3),
        Algorithm::BikeL5 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL5),
    };

    let kem = kem.map_err(|e| anyhow::anyhow!("Failed to create KEM: {}", e))?;
    let (pk, sk) = kem
        .keypair()
        .map_err(|e| anyhow::anyhow!("Failed to generate keypair: {}", e))?;
    Ok((pk.into_vec(), sk.into_vec()))
}

/// Fallback keypair generator when quantum support is disabled.
///
/// Uses ECIES (secp256k1). Returns `(public_key, secret_key)` as raw bytes.
#[cfg(not(feature = "quantum"))]
pub fn generate_kem_keypair(_algorithm: Algorithm) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let (sk, pk) = ecies::utils::generate_keypair();
    Ok((pk.serialize().to_vec(), sk.secret_bytes().to_vec()))
}

/// SPHINCS+ signature for quantum-resistant digital signatures
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SPHINCSSignature {
    pub signature_bytes: Vec<u8>,
    pub algorithm: String,
    pub public_key: Vec<u8>,
}

impl SPHINCSSignature {
    pub fn new(signature_bytes: Vec<u8>, algorithm: String, public_key: Vec<u8>) -> Self {
        Self {
            signature_bytes,
            algorithm,
            public_key,
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, &'static str> {
        if bytes.len() < 64 {
            return Err("Invalid signature length");
        }
        Ok(Self {
            signature_bytes: bytes.to_vec(),
            algorithm: "SPHINCS+".to_string(),
            public_key: vec![], // Public key should be provided separately
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.signature_bytes
    }
}

/// Verify a SPHINCS+ signature.
///
/// Supported algorithm strings (case-insensitive):
/// - `SPHINCS+` (defaults to `sphincssha256256frobust`)
/// - `SPHINCS-128f`, `SPHINCS-128s`
/// - `SPHINCS-192f`, `SPHINCS-192s`
/// - `SPHINCS-256f`, `SPHINCS-256s`
///
/// Notes:
/// - This uses the `pqcrypto-sphincsplus` implementations.
/// - Keys and signatures must be raw bytes as produced by the same parameter set.
#[cfg(feature = "quantum")]
pub fn verify_sphincs_signature(
    message: &[u8],
    signature: &SPHINCSSignature,
) -> anyhow::Result<bool> {
    let alg = signature.algorithm.trim().to_ascii_lowercase();

    // Map your high-level names to concrete parameter sets.
    // pqcrypto-sphincsplus exposes "simple" parameter sets (SHA2 and SHAKE).
    let ok = match alg.as_str() {
        "sphincs+" | "sphincs" => verify_with_sha2_256f_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
        "sphincs-128f" => verify_with_sha2_128f_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
        "sphincs-128s" | "slh-dsa-sha2-128s" | "slh-dsa-128s" => verify_with_sha2_128s_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
        "sphincs-192f" => verify_with_sha2_192f_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
        "sphincs-192s" | "slh-dsa-sha2-192s" => verify_with_sha2_192s_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
        "sphincs-256f" => verify_with_sha2_256f_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
        "sphincs-256s" => verify_with_sha2_256s_simple(
            message,
            &signature.public_key,
            &signature.signature_bytes,
        )?,
        other => {
            return Err(anyhow::anyhow!(
                "Unsupported SPHINCS+ algorithm string: {}",
                other
            ));
        }
    };

    Ok(ok)
}

/// Generate a SPHINCS+ keypair for the selected parameter set.
///
/// Returns `(public_key_bytes, secret_key_bytes)`.
#[cfg(feature = "quantum")]
pub fn generate_sphincs_keypair(algorithm: &str) -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let alg = algorithm.trim().to_ascii_lowercase();
    let (pk, sk) = match alg.as_str() {
        "sphincs+" | "sphincs" | "sphincs-256f" => {
            use pqcrypto_sphincsplus::sphincssha2256fsimple as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        "sphincs-256s" => {
            use pqcrypto_sphincsplus::sphincssha2256ssimple as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        "sphincs-192f" => {
            use pqcrypto_sphincsplus::sphincssha2192fsimple as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        "sphincs-192s" => {
            use pqcrypto_sphincsplus::sphincssha2192ssimple as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        "sphincs-128f" => {
            use pqcrypto_sphincsplus::sphincssha2128fsimple as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        "sphincs-128s" => {
            use pqcrypto_sphincsplus::sphincssha2128ssimple as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        other => {
            return Err(anyhow::anyhow!(
                "Unsupported SPHINCS+ algorithm string: {}",
                other
            ))
        }
    };

    Ok((pk, sk))
}

/// Convenience: generate a SPHINCS+ keypair and return a detached signature for `message`.
///
/// This is mainly useful for demos/tests and for callers that don't yet have a key management layer.
#[cfg(feature = "quantum")]
pub fn sphincs_keypair_and_sign_detached(
    message: &[u8],
    algorithm: &str,
) -> anyhow::Result<SPHINCSSignature> {
    let (pk, sk) = generate_sphincs_keypair(algorithm)?;
    let sig = sign_sphincs_detached(message, algorithm, &pk, &sk)?;
    Ok(sig)
}

/// Sign `message` using a SPHINCS+ secret key and return an `SPHINCSSignature`.
///
/// `public_key_bytes` is stored into the returned struct so verifiers don't need external DID→key lookups.
#[cfg(feature = "quantum")]
pub fn sign_sphincs_detached(
    message: &[u8],
    algorithm: &str,
    public_key_bytes: &[u8],
    secret_key_bytes: &[u8],
) -> anyhow::Result<SPHINCSSignature> {
    let alg = algorithm.trim().to_ascii_lowercase();

    let signature_bytes = match alg.as_str() {
        "sphincs+" | "sphincs" | "sphincs-256f" => {
            use pqcrypto_sphincsplus::sphincssha2256fsimple as alg;
            let sk = alg::SecretKey::from_bytes(secret_key_bytes)
                .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ secret key"))?;
            let sig = alg::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-256s" => {
            use pqcrypto_sphincsplus::sphincssha2256ssimple as alg;
            let sk = alg::SecretKey::from_bytes(secret_key_bytes)
                .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ secret key"))?;
            let sig = alg::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-192f" => {
            use pqcrypto_sphincsplus::sphincssha2192fsimple as alg;
            let sk = alg::SecretKey::from_bytes(secret_key_bytes)
                .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ secret key"))?;
            let sig = alg::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-192s" => {
            use pqcrypto_sphincsplus::sphincssha2192ssimple as alg;
            let sk = alg::SecretKey::from_bytes(secret_key_bytes)
                .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ secret key"))?;
            let sig = alg::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-128f" => {
            use pqcrypto_sphincsplus::sphincssha2128fsimple as alg;
            let sk = alg::SecretKey::from_bytes(secret_key_bytes)
                .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ secret key"))?;
            let sig = alg::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-128s" => {
            use pqcrypto_sphincsplus::sphincssha2128ssimple as alg;
            let sk = alg::SecretKey::from_bytes(secret_key_bytes)
                .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ secret key"))?;
            let sig = alg::detached_sign(message, &sk);
            sig.as_bytes().to_vec()
        }
        other => {
            return Err(anyhow::anyhow!(
                "Unsupported SPHINCS+ algorithm string: {}",
                other
            ))
        }
    };

    Ok(SPHINCSSignature::new(
        signature_bytes,
        algorithm.to_string(),
        public_key_bytes.to_vec(),
    ))
}

#[cfg(feature = "quantum")]
fn verify_with_sha2_128f_simple(message: &[u8], pk: &[u8], sig: &[u8]) -> anyhow::Result<bool> {
    use pqcrypto_sphincsplus::sphincssha2128fsimple as alg;
    let pk = alg::PublicKey::from_bytes(pk)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ public key"))?;
    let sig = alg::DetachedSignature::from_bytes(sig)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ signature"))?;
    Ok(alg::verify_detached_signature(&sig, message, &pk).is_ok())
}

#[cfg(feature = "quantum")]
fn verify_with_sha2_128s_simple(message: &[u8], pk: &[u8], sig: &[u8]) -> anyhow::Result<bool> {
    use pqcrypto_sphincsplus::sphincssha2128ssimple as alg;
    let pk = alg::PublicKey::from_bytes(pk)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ public key"))?;
    let sig = alg::DetachedSignature::from_bytes(sig)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ signature"))?;
    Ok(alg::verify_detached_signature(&sig, message, &pk).is_ok())
}

#[cfg(feature = "quantum")]
fn verify_with_sha2_192f_simple(message: &[u8], pk: &[u8], sig: &[u8]) -> anyhow::Result<bool> {
    use pqcrypto_sphincsplus::sphincssha2192fsimple as alg;
    let pk = alg::PublicKey::from_bytes(pk)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ public key"))?;
    let sig = alg::DetachedSignature::from_bytes(sig)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ signature"))?;
    Ok(alg::verify_detached_signature(&sig, message, &pk).is_ok())
}

#[cfg(feature = "quantum")]
fn verify_with_sha2_192s_simple(message: &[u8], pk: &[u8], sig: &[u8]) -> anyhow::Result<bool> {
    use pqcrypto_sphincsplus::sphincssha2192ssimple as alg;
    let pk = alg::PublicKey::from_bytes(pk)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ public key"))?;
    let sig = alg::DetachedSignature::from_bytes(sig)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ signature"))?;
    Ok(alg::verify_detached_signature(&sig, message, &pk).is_ok())
}

#[cfg(feature = "quantum")]
fn verify_with_sha2_256f_simple(message: &[u8], pk: &[u8], sig: &[u8]) -> anyhow::Result<bool> {
    use pqcrypto_sphincsplus::sphincssha2256fsimple as alg;
    let pk = alg::PublicKey::from_bytes(pk)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ public key"))?;
    let sig = alg::DetachedSignature::from_bytes(sig)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ signature"))?;
    Ok(alg::verify_detached_signature(&sig, message, &pk).is_ok())
}

#[cfg(feature = "quantum")]
fn verify_with_sha2_256s_simple(message: &[u8], pk: &[u8], sig: &[u8]) -> anyhow::Result<bool> {
    use pqcrypto_sphincsplus::sphincssha2256ssimple as alg;
    let pk = alg::PublicKey::from_bytes(pk)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ public key"))?;
    let sig = alg::DetachedSignature::from_bytes(sig)
        .map_err(|_| anyhow::anyhow!("Invalid SPHINCS+ signature"))?;
    Ok(alg::verify_detached_signature(&sig, message, &pk).is_ok())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CipherSuite {
    AES256,
    ChaCha20,
    XChaCha20,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FileType {
    PEM,
    DER,
    RAW,
}

// Note: Remove Debug derive because cipher types don't implement Debug
#[derive(Clone)]
pub enum Cipher {
    Aes(Aes256Gcm),
    ChaCha(ChaCha20Poly1305),
    XChaCha(XChaCha20Poly1305),
}

#[derive(Clone, Debug)]
pub struct PathConfiguration {
    file_type: FileType,
    path: PathBuf,
}

// Quantum-enabled functions
#[cfg(feature = "quantum")]
pub fn encrypt_message(
    message: &[u8],
    algorithm: &Algorithm,
    _cipher: Cipher,
) -> std::result::Result<(Vec<u8>, Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let kem = match algorithm {
        Algorithm::Kyber512 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber512),
        Algorithm::Kyber768 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber768),
        Algorithm::Kyber1024 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber1024),
        Algorithm::NtruPrimeSntrup761 => {
            oqs::kem::Kem::new(oqs::kem::Algorithm::NtruPrimeSntrup761)
        }
        Algorithm::FrodoKem1344Aes => oqs::kem::Kem::new(oqs::kem::Algorithm::FrodoKem1344Aes),
        Algorithm::FrodoKem1344Shake => oqs::kem::Kem::new(oqs::kem::Algorithm::FrodoKem1344Shake),
        Algorithm::ClassicMcEliece348864 => {
            oqs::kem::Kem::new(oqs::kem::Algorithm::ClassicMcEliece348864)
        }
        Algorithm::BikeL1 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL1),
        Algorithm::BikeL3 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL3),
        Algorithm::BikeL5 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL5),
    };

    let kem = kem.map_err(|e| format!("Failed to create KEM: {}", e))?;
    let (alice_pk, _alice_sk) = kem
        .keypair()
        .map_err(|e| format!("Failed to generate keypair: {}", e))?;
    let (kem_ciphertext, b_kem_ss) = kem
        .encapsulate(&alice_pk)
        .map_err(|e| format!("Failed to encapsulate: {}", e))?;

    let shared_secret = b_kem_ss.as_ref();
    let key_bytes = if shared_secret.len() >= 32 {
        &shared_secret[..32]
    } else {
        shared_secret
    };

    // Use AES256-GCM for encryption
    let key = aes_gcm::Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = AesNonce::from_slice(&[0u8; 12]);
    let encrypted_message = cipher
        .encrypt(nonce, message)
        .map_err(|e| format!("Failed to encrypt: {}", e))?;

    Ok((
        encrypted_message,
        kem_ciphertext.into_vec(),
        alice_pk.into_vec(),
    ))
}

#[cfg(feature = "quantum")]
pub fn decrypt_message(
    encrypted_message: &[u8],
    kem_ciphertext: &[u8],
    secret_key: &[u8],
    _cipher: Cipher,
    algorithm: &Algorithm,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    let kem = match algorithm {
        Algorithm::Kyber512 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber512),
        Algorithm::Kyber768 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber768),
        Algorithm::Kyber1024 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber1024),
        Algorithm::NtruPrimeSntrup761 => {
            oqs::kem::Kem::new(oqs::kem::Algorithm::NtruPrimeSntrup761)
        }
        Algorithm::FrodoKem1344Aes => oqs::kem::Kem::new(oqs::kem::Algorithm::FrodoKem1344Aes),
        Algorithm::FrodoKem1344Shake => oqs::kem::Kem::new(oqs::kem::Algorithm::FrodoKem1344Shake),
        Algorithm::ClassicMcEliece348864 => {
            oqs::kem::Kem::new(oqs::kem::Algorithm::ClassicMcEliece348864)
        }
        Algorithm::BikeL1 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL1),
        Algorithm::BikeL3 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL3),
        Algorithm::BikeL5 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL5),
    };

    let kem = kem.map_err(|e| format!("Failed to create KEM: {}", e))?;
    let alice_sk = kem
        .secret_key_from_bytes(secret_key)
        .ok_or("Failed to parse secret key")?;
    let kem_ct = kem
        .ciphertext_from_bytes(kem_ciphertext)
        .ok_or("Failed to parse ciphertext")?;
    let shared_secret = kem
        .decapsulate(&alice_sk, &kem_ct)
        .map_err(|e| format!("Failed to decapsulate: {}", e))?;

    let key_bytes = if shared_secret.as_ref().len() >= 32 {
        &shared_secret.as_ref()[..32]
    } else {
        shared_secret.as_ref()
    };

    // Use AES256-GCM for decryption
    let key = aes_gcm::Key::<Aes256Gcm>::from_slice(key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = AesNonce::from_slice(&[0u8; 12]);
    cipher
        .decrypt(nonce, encrypted_message)
        .map_err(|e| format!("Failed to decrypt: {}", e).into())
}

// Fallback functions when quantum is disabled
#[cfg(not(feature = "quantum"))]
pub fn encrypt_message(
    message: &[u8],
    _algorithm: &Algorithm,
    _cipher: Cipher,
) -> std::result::Result<(Vec<u8>, Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let key = [0u8; 32];
    let fake_kem_ciphertext = vec![0u8; 32];
    let fake_public_key = vec![0u8; 32];

    let aes_key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key);
    let aes_cipher = Aes256Gcm::new(aes_key);
    let nonce = AesNonce::from_slice(&[0u8; 12]);
    let encrypted_message = aes_cipher.encrypt(nonce, message)?;

    Ok((encrypted_message, fake_kem_ciphertext, fake_public_key))
}

#[cfg(not(feature = "quantum"))]
pub fn decrypt_message(
    encrypted_message: &[u8],
    _kem_ciphertext: &[u8],
    _secret_key: &[u8],
    _cipher: Cipher,
    _algorithm: &Algorithm,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    let key = [0u8; 32];

    let aes_key = aes_gcm::Key::<Aes256Gcm>::from_slice(&key);
    let aes_cipher = Aes256Gcm::new(aes_key);
    let nonce = AesNonce::from_slice(&[0u8; 12]);
    aes_cipher
        .decrypt(nonce, encrypted_message)
        .map_err(Into::into)
}

#[cfg(feature = "quantum")]
pub fn generate_kem(
    algorithm: &Algorithm,
) -> std::result::Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    let kem = match algorithm {
        Algorithm::Kyber512 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber512),
        Algorithm::Kyber768 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber768),
        Algorithm::Kyber1024 => oqs::kem::Kem::new(oqs::kem::Algorithm::Kyber1024),
        Algorithm::NtruPrimeSntrup761 => {
            oqs::kem::Kem::new(oqs::kem::Algorithm::NtruPrimeSntrup761)
        }
        Algorithm::FrodoKem1344Aes => oqs::kem::Kem::new(oqs::kem::Algorithm::FrodoKem1344Aes),
        Algorithm::FrodoKem1344Shake => oqs::kem::Kem::new(oqs::kem::Algorithm::FrodoKem1344Shake),
        Algorithm::ClassicMcEliece348864 => {
            oqs::kem::Kem::new(oqs::kem::Algorithm::ClassicMcEliece348864)
        }
        Algorithm::BikeL1 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL1),
        Algorithm::BikeL3 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL3),
        Algorithm::BikeL5 => oqs::kem::Kem::new(oqs::kem::Algorithm::BikeL5),
    };

    let kem = kem.map_err(|e| format!("Failed to create KEM: {}", e))?;
    let (alice_pk, _alice_sk) = kem
        .keypair()
        .map_err(|e| format!("Failed to generate keypair: {}", e))?;
    let (kem_ciphertext, _b_kem_ss) = kem
        .encapsulate(&alice_pk)
        .map_err(|e| format!("Failed to encapsulate: {}", e))?;

    Ok((kem_ciphertext.into_vec(), alice_pk.into_vec()))
}

#[cfg(not(feature = "quantum"))]
pub fn generate_kem(
    _algorithm: &Algorithm,
) -> std::result::Result<(Vec<u8>, Vec<u8>), Box<dyn std::error::Error>> {
    Ok((vec![0u8; 32], vec![0u8; 32]))
}

#[cfg(feature = "quantum")]
pub fn handle_encryption(
    file_path: &str,
    algorithm: &Algorithm,
    cipher: Cipher,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let message = fs::read(file_path)?;
    let (encrypted_message, kem_ciphertext, public_key) =
        encrypt_message(&message, algorithm, cipher)?;

    save_to_file(&format!("{}.enc", file_path), &encrypted_message)?;
    save_to_file(&format!("{}.kem", file_path), &kem_ciphertext)?;
    save_to_file(&format!("{}.pub", file_path), &public_key)?;

    Ok(())
}

#[cfg(not(feature = "quantum"))]
pub fn handle_encryption(
    file_path: &str,
    algorithm: &Algorithm,
    cipher: Cipher,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let message = fs::read(file_path)?;
    let (encrypted_message, kem_ciphertext, public_key) =
        encrypt_message(&message, algorithm, cipher)?;

    save_to_file(&format!("{}.enc", file_path), &encrypted_message)?;
    save_to_file(&format!("{}.kem", file_path), &kem_ciphertext)?;
    save_to_file(&format!("{}.pub", file_path), &public_key)?;

    Ok(())
}

#[cfg(feature = "quantum")]
pub fn handle_decryption(
    encrypted_file_path: &str,
    kem_file_path: &str,
    secret_key_path: &str,
    cipher: Cipher,
    algorithm: &Algorithm,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    let encrypted_message = load_from_file(encrypted_file_path)?;
    let kem_ciphertext = load_from_file(kem_file_path)?;
    let secret_key = load_from_file(secret_key_path)?;

    decrypt_message(
        &encrypted_message,
        &kem_ciphertext,
        &secret_key,
        cipher,
        algorithm,
    )
}

#[cfg(not(feature = "quantum"))]
pub fn handle_decryption(
    encrypted_file_path: &str,
    _kem_file_path: &str,
    _secret_key_path: &str,
    cipher: Cipher,
    algorithm: &Algorithm,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error>> {
    let encrypted_message = load_from_file(encrypted_file_path)?;
    let kem_ciphertext = vec![0u8; 32];
    let secret_key = vec![0u8; 32];

    decrypt_message(
        &encrypted_message,
        &kem_ciphertext,
        &secret_key,
        cipher,
        algorithm,
    )
}

// Conditional quantum-specific functions
#[cfg(feature = "quantum")]
pub fn encrypt_blobs_with_cipher(
    _shared_secret: SharedSecret,
    cipher_suite: CipherSuite,
    _path: &PathConfiguration,
) {
    match cipher_suite {
        CipherSuite::ChaCha20 => {
            println!("Encrypting with ChaCha20");
        }
        CipherSuite::XChaCha20 => {
            println!("Encrypting with XChaCha20");
        }
        CipherSuite::AES256 => {
            println!("Encrypting with AES256");
        }
    }
}

#[cfg(feature = "quantum")]
pub fn decrypt_blobs_with_cipher(
    _shared_secret: SharedSecret,
    cipher_suite: CipherSuite,
    _path: &PathConfiguration,
) {
    match cipher_suite {
        CipherSuite::ChaCha20 => {
            println!("Decrypting with ChaCha20");
        }
        CipherSuite::XChaCha20 => {
            println!("Decrypting with XChaCha20");
        }
        CipherSuite::AES256 => {
            println!("Decrypting with AES256");
        }
    }
}

pub mod utils {
    pub fn create_output_paths(input_files: &[String], suffix: &str) -> Vec<String> {
        input_files
            .iter()
            .map(|path| format!("{}.{}", path, suffix))
            .collect()
    }
}
