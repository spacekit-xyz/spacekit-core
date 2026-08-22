use wasm_bindgen::prelude::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use pqcrypto_traits::kem::{PublicKey as _, SecretKey as _, SharedSecret as _, Ciphertext as _};
use serde::Serialize;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
    aead::generic_array::GenericArray,
};

/// Result of keypair generation
#[derive(Serialize)]
struct KeypairResult {
    #[serde(rename = "publicKeyBase64")]
    public_key_base64: String,
    #[serde(rename = "secretKeyBase64")]
    secret_key_base64: String,
    algorithm: String,
}

/// Result of encryption (KEM encapsulation + AES-GCM)
#[derive(Serialize)]
struct EncryptResult {
    /// KEM ciphertext (encapsulated key) - base64
    #[serde(rename = "kemCiphertextBase64")]
    kem_ciphertext_base64: String,
    /// AES-GCM nonce - base64
    #[serde(rename = "nonceBase64")]
    nonce_base64: String,
    /// AES-GCM ciphertext - base64
    #[serde(rename = "ciphertextBase64")]
    ciphertext_base64: String,
    algorithm: String,
}

/// Generate a Kyber keypair
/// 
/// # Arguments
/// * `algorithm` - "kyber512", "kyber768", or "kyber1024"
/// 
/// # Returns
/// JSON object with publicKeyBase64, secretKeyBase64, algorithm
#[wasm_bindgen]
pub fn kyber_keypair(algorithm: &str) -> JsValue {
    let alg = algorithm.trim().to_ascii_lowercase();
    let (pk, sk) = match alg.as_str() {
        "kyber512" | "ml-kem-512" => {
            use pqcrypto_kyber::kyber512 as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        "kyber768" | "ml-kem-768" => {
            use pqcrypto_kyber::kyber768 as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
        "kyber1024" | "ml-kem-1024" | "kyber" | _ => {
            use pqcrypto_kyber::kyber1024 as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
    };

    let result = KeypairResult {
        public_key_base64: STANDARD.encode(pk),
        secret_key_base64: STANDARD.encode(sk),
        algorithm: alg,
    };
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

/// Encrypt data using Kyber KEM + AES-256-GCM
/// 
/// # Arguments
/// * `algorithm` - "kyber512", "kyber768", or "kyber1024"
/// * `public_key_base64` - Recipient's public key (base64)
/// * `plaintext_base64` - Data to encrypt (base64)
/// 
/// # Returns
/// JSON object with kemCiphertextBase64, nonceBase64, ciphertextBase64, algorithm
#[wasm_bindgen]
pub fn kyber_encrypt(algorithm: &str, public_key_base64: &str, plaintext_base64: &str) -> JsValue {
    let alg = algorithm.trim().to_ascii_lowercase();
    let pk_bytes = match STANDARD.decode(public_key_base64) {
        Ok(b) => b,
        Err(_) => return JsValue::NULL,
    };
    let plaintext = match STANDARD.decode(plaintext_base64) {
        Ok(b) => b,
        Err(_) => return JsValue::NULL,
    };

    // Encapsulate to get shared secret
    let (kem_ct, shared_secret) = match alg.as_str() {
        "kyber512" | "ml-kem-512" => {
            use pqcrypto_kyber::kyber512 as alg;
            let pk = match alg::PublicKey::from_bytes(&pk_bytes) {
                Ok(pk) => pk,
                Err(_) => return JsValue::NULL,
            };
            let (ss, ct) = alg::encapsulate(&pk);
            (ct.as_bytes().to_vec(), ss.as_bytes().to_vec())
        }
        "kyber768" | "ml-kem-768" => {
            use pqcrypto_kyber::kyber768 as alg;
            let pk = match alg::PublicKey::from_bytes(&pk_bytes) {
                Ok(pk) => pk,
                Err(_) => return JsValue::NULL,
            };
            let (ss, ct) = alg::encapsulate(&pk);
            (ct.as_bytes().to_vec(), ss.as_bytes().to_vec())
        }
        "kyber1024" | "ml-kem-1024" | "kyber" | _ => {
            use pqcrypto_kyber::kyber1024 as alg;
            let pk = match alg::PublicKey::from_bytes(&pk_bytes) {
                Ok(pk) => pk,
                Err(_) => return JsValue::NULL,
            };
            let (ss, ct) = alg::encapsulate(&pk);
            (ct.as_bytes().to_vec(), ss.as_bytes().to_vec())
        }
    };

    // Use first 32 bytes of shared secret as AES-256 key
    let key_bytes: [u8; 32] = shared_secret[..32].try_into().unwrap_or([0u8; 32]);
    let key = GenericArray::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    // Generate random nonce
    let nonce_bytes: [u8; 12] = {
        let mut buf = [0u8; 12];
        getrandom::getrandom(&mut buf).unwrap();
        buf
    };
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Encrypt plaintext
    let ciphertext = match cipher.encrypt(nonce, plaintext.as_ref()) {
        Ok(ct) => ct,
        Err(_) => return JsValue::NULL,
    };

    let result = EncryptResult {
        kem_ciphertext_base64: STANDARD.encode(&kem_ct),
        nonce_base64: STANDARD.encode(&nonce_bytes),
        ciphertext_base64: STANDARD.encode(&ciphertext),
        algorithm: alg,
    };
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

/// Decrypt data using Kyber KEM + AES-256-GCM
/// 
/// # Arguments
/// * `algorithm` - "kyber512", "kyber768", or "kyber1024"
/// * `secret_key_base64` - Recipient's secret key (base64)
/// * `kem_ciphertext_base64` - KEM ciphertext from encryption (base64)
/// * `nonce_base64` - AES-GCM nonce from encryption (base64)
/// * `ciphertext_base64` - AES-GCM ciphertext (base64)
/// 
/// # Returns
/// Decrypted plaintext as base64, or null on error
#[wasm_bindgen]
pub fn kyber_decrypt(
    algorithm: &str,
    secret_key_base64: &str,
    kem_ciphertext_base64: &str,
    nonce_base64: &str,
    ciphertext_base64: &str,
) -> JsValue {
    let alg = algorithm.trim().to_ascii_lowercase();
    
    let sk_bytes = match STANDARD.decode(secret_key_base64) {
        Ok(b) => b,
        Err(_) => return JsValue::NULL,
    };
    let kem_ct_bytes = match STANDARD.decode(kem_ciphertext_base64) {
        Ok(b) => b,
        Err(_) => return JsValue::NULL,
    };
    let nonce_bytes = match STANDARD.decode(nonce_base64) {
        Ok(b) => b,
        Err(_) => return JsValue::NULL,
    };
    let ciphertext = match STANDARD.decode(ciphertext_base64) {
        Ok(b) => b,
        Err(_) => return JsValue::NULL,
    };

    // Decapsulate to recover shared secret
    let shared_secret = match alg.as_str() {
        "kyber512" | "ml-kem-512" => {
            use pqcrypto_kyber::kyber512 as alg;
            let sk = match alg::SecretKey::from_bytes(&sk_bytes) {
                Ok(sk) => sk,
                Err(_) => return JsValue::NULL,
            };
            let ct = match alg::Ciphertext::from_bytes(&kem_ct_bytes) {
                Ok(ct) => ct,
                Err(_) => return JsValue::NULL,
            };
            alg::decapsulate(&ct, &sk).as_bytes().to_vec()
        }
        "kyber768" | "ml-kem-768" => {
            use pqcrypto_kyber::kyber768 as alg;
            let sk = match alg::SecretKey::from_bytes(&sk_bytes) {
                Ok(sk) => sk,
                Err(_) => return JsValue::NULL,
            };
            let ct = match alg::Ciphertext::from_bytes(&kem_ct_bytes) {
                Ok(ct) => ct,
                Err(_) => return JsValue::NULL,
            };
            alg::decapsulate(&ct, &sk).as_bytes().to_vec()
        }
        "kyber1024" | "ml-kem-1024" | "kyber" | _ => {
            use pqcrypto_kyber::kyber1024 as alg;
            let sk = match alg::SecretKey::from_bytes(&sk_bytes) {
                Ok(sk) => sk,
                Err(_) => return JsValue::NULL,
            };
            let ct = match alg::Ciphertext::from_bytes(&kem_ct_bytes) {
                Ok(ct) => ct,
                Err(_) => return JsValue::NULL,
            };
            alg::decapsulate(&ct, &sk).as_bytes().to_vec()
        }
    };

    // Derive AES key from shared secret
    let key_bytes: [u8; 32] = shared_secret[..32].try_into().unwrap_or([0u8; 32]);
    let key = GenericArray::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    // Reconstruct nonce
    if nonce_bytes.len() != 12 {
        return JsValue::NULL;
    }
    let nonce = Nonce::from_slice(&nonce_bytes);

    // Decrypt
    let plaintext = match cipher.decrypt(nonce, ciphertext.as_ref()) {
        Ok(pt) => pt,
        Err(_) => return JsValue::NULL,
    };

    JsValue::from_str(&STANDARD.encode(&plaintext))
}

/// Get public key size for algorithm
#[wasm_bindgen]
pub fn kyber_public_key_size(algorithm: &str) -> u32 {
    let alg = algorithm.trim().to_ascii_lowercase();
    match alg.as_str() {
        "kyber512" | "ml-kem-512" => pqcrypto_kyber::kyber512::public_key_bytes() as u32,
        "kyber768" | "ml-kem-768" => pqcrypto_kyber::kyber768::public_key_bytes() as u32,
        "kyber1024" | "ml-kem-1024" | "kyber" | _ => pqcrypto_kyber::kyber1024::public_key_bytes() as u32,
    }
}

/// Get secret key size for algorithm
#[wasm_bindgen]
pub fn kyber_secret_key_size(algorithm: &str) -> u32 {
    let alg = algorithm.trim().to_ascii_lowercase();
    match alg.as_str() {
        "kyber512" | "ml-kem-512" => pqcrypto_kyber::kyber512::secret_key_bytes() as u32,
        "kyber768" | "ml-kem-768" => pqcrypto_kyber::kyber768::secret_key_bytes() as u32,
        "kyber1024" | "ml-kem-1024" | "kyber" | _ => pqcrypto_kyber::kyber1024::secret_key_bytes() as u32,
    }
}

/// Get ciphertext size for algorithm
#[wasm_bindgen]
pub fn kyber_ciphertext_size(algorithm: &str) -> u32 {
    let alg = algorithm.trim().to_ascii_lowercase();
    match alg.as_str() {
        "kyber512" | "ml-kem-512" => pqcrypto_kyber::kyber512::ciphertext_bytes() as u32,
        "kyber768" | "ml-kem-768" => pqcrypto_kyber::kyber768::ciphertext_bytes() as u32,
        "kyber1024" | "ml-kem-1024" | "kyber" | _ => pqcrypto_kyber::kyber1024::ciphertext_bytes() as u32,
    }
}
