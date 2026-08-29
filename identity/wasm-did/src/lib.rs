//! WASM bindings for SpaceKit quantum DID signatures.
//!
//! Uses SLH-DSA (FIPS-205 / SPHINCS+) via pure-Rust `slh-dsa` crate,
//! matching the parameter sets in `spacekit-primitives/secrets_core`.

use wasm_bindgen::prelude::*;
use serde::Serialize;
use slh_dsa::{Sha2_128s, Sha2_192s, SigningKey, VerifyingKey};
use signature::{Signer, Verifier, Keypair};
use rand_core::{Infallible, TryCryptoRng, TryRng};

struct OsRng;

impl TryRng for OsRng {
    type Error = Infallible;
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        getrandom::fill(dest).expect("getrandom failed");
        Ok(())
    }
    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        let mut buf = [0u8; 4];
        getrandom::fill(&mut buf).expect("getrandom failed");
        Ok(u32::from_le_bytes(buf))
    }
    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let mut buf = [0u8; 8];
        getrandom::fill(&mut buf).expect("getrandom failed");
        Ok(u64::from_le_bytes(buf))
    }
}

impl TryCryptoRng for OsRng {}

#[derive(Serialize)]
struct KeypairResult {
    #[serde(rename = "publicKey")]
    public_key: Vec<u8>,
    #[serde(rename = "privateKey")]
    private_key: Vec<u8>,
    algorithm: String,
    #[serde(rename = "publicKeySize")]
    public_key_size: usize,
    #[serde(rename = "signatureSize")]
    signature_size: usize,
}

// ─── SLH-DSA-SHA2-128s (32-byte pk, 64-byte sk, 7856-byte sig) ─────────

#[wasm_bindgen(js_name = "slhDsa128sKeypair")]
pub fn slh_dsa_128s_keypair() -> JsValue {
    let sk = SigningKey::<Sha2_128s>::new(&mut OsRng);
    let vk = sk.verifying_key();
    let result = KeypairResult {
        public_key: vk.to_bytes().as_slice().to_vec(),
        private_key: sk.to_bytes().as_slice().to_vec(),
        algorithm: "SLH-DSA-SHA2-128s".into(),
        public_key_size: 32,
        signature_size: 7856,
    };
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen(js_name = "slhDsa128sSign")]
pub fn slh_dsa_128s_sign(private_key: &[u8], message: &[u8]) -> Result<Vec<u8>, JsError> {
    let sk = SigningKey::<Sha2_128s>::try_from(private_key)
        .map_err(|e| JsError::new(&format!("invalid SLH-DSA-128s private key: {e:?}")))?;
    let sig = sk.sign(message);
    Ok(sig.to_bytes().as_slice().to_vec())
}

/// Recover the 32-byte public key from a 64-byte SLH-DSA-128s private key.
/// The SLH-DSA secret key embeds the public seed + root, so this is a pure
/// derivation — used to restore a quantum identity from its recovery key.
#[wasm_bindgen(js_name = "slhDsa128sPublicKey")]
pub fn slh_dsa_128s_public_key(private_key: &[u8]) -> Result<Vec<u8>, JsError> {
    let sk = SigningKey::<Sha2_128s>::try_from(private_key)
        .map_err(|e| JsError::new(&format!("invalid SLH-DSA-128s private key: {e:?}")))?;
    Ok(sk.verifying_key().to_bytes().as_slice().to_vec())
}

#[wasm_bindgen(js_name = "slhDsa128sVerify")]
pub fn slh_dsa_128s_verify(
    public_key: &[u8],
    message: &[u8],
    sig_bytes: &[u8],
) -> Result<bool, JsError> {
    let vk = VerifyingKey::<Sha2_128s>::try_from(public_key)
        .map_err(|e| JsError::new(&format!("invalid SLH-DSA-128s public key: {e:?}")))?;
    let sig = slh_dsa::Signature::<Sha2_128s>::try_from(sig_bytes)
        .map_err(|e| JsError::new(&format!("invalid signature: {e:?}")))?;
    Ok(vk.verify(message, &sig).is_ok())
}

// ─── SLH-DSA-SHA2-192s (48-byte pk, 96-byte sk, 16224-byte sig) ────────

#[wasm_bindgen(js_name = "slhDsa192sKeypair")]
pub fn slh_dsa_192s_keypair() -> JsValue {
    let sk = SigningKey::<Sha2_192s>::new(&mut OsRng);
    let vk = sk.verifying_key();
    let result = KeypairResult {
        public_key: vk.to_bytes().as_slice().to_vec(),
        private_key: sk.to_bytes().as_slice().to_vec(),
        algorithm: "SLH-DSA-SHA2-192s".into(),
        public_key_size: 48,
        signature_size: 16224,
    };
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen(js_name = "slhDsa192sSign")]
pub fn slh_dsa_192s_sign(private_key: &[u8], message: &[u8]) -> Result<Vec<u8>, JsError> {
    let sk = SigningKey::<Sha2_192s>::try_from(private_key)
        .map_err(|e| JsError::new(&format!("invalid SLH-DSA-192s private key: {e:?}")))?;
    let sig = sk.sign(message);
    Ok(sig.to_bytes().as_slice().to_vec())
}

/// Recover the 48-byte public key from a 96-byte SLH-DSA-192s private key.
#[wasm_bindgen(js_name = "slhDsa192sPublicKey")]
pub fn slh_dsa_192s_public_key(private_key: &[u8]) -> Result<Vec<u8>, JsError> {
    let sk = SigningKey::<Sha2_192s>::try_from(private_key)
        .map_err(|e| JsError::new(&format!("invalid SLH-DSA-192s private key: {e:?}")))?;
    Ok(sk.verifying_key().to_bytes().as_slice().to_vec())
}

#[wasm_bindgen(js_name = "slhDsa192sVerify")]
pub fn slh_dsa_192s_verify(
    public_key: &[u8],
    message: &[u8],
    sig_bytes: &[u8],
) -> Result<bool, JsError> {
    let vk = VerifyingKey::<Sha2_192s>::try_from(public_key)
        .map_err(|e| JsError::new(&format!("invalid SLH-DSA-192s public key: {e:?}")))?;
    let sig = slh_dsa::Signature::<Sha2_192s>::try_from(sig_bytes)
        .map_err(|e| JsError::new(&format!("invalid signature: {e:?}")))?;
    Ok(vk.verify(message, &sig).is_ok())
}
