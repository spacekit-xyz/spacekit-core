use wasm_bindgen::prelude::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use pqcrypto_traits::sign::{DetachedSignature as _, PublicKey as _, SecretKey as _};
use serde::Serialize;

#[derive(Serialize)]
struct KeypairResult {
    #[serde(rename = "publicKeyBase64")]
    public_key_base64: String,
    #[serde(rename = "secretKeyBase64")]
    secret_key_base64: String,
    algorithm: String,
}

#[wasm_bindgen]
pub fn sphincs_keypair(algorithm: &str) -> JsValue {
    let alg = algorithm.trim().to_ascii_lowercase();
    let (pk, sk) = match alg.as_str() {
        "sphincs" | "sphincs+" | "sphincs-256f" => {
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
        _ => {
            use pqcrypto_sphincsplus::sphincssha2256fsimple as alg;
            let (pk, sk) = alg::keypair();
            (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
        }
    };

    let result = KeypairResult {
        public_key_base64: STANDARD.encode(pk),
        secret_key_base64: STANDARD.encode(sk),
        algorithm: algorithm.to_string(),
    };
    serde_wasm_bindgen::to_value(&result).unwrap_or(JsValue::NULL)
}

#[wasm_bindgen]
pub fn sphincs_sign(algorithm: &str, secret_key_base64: &str, message_hex: &str) -> String {
    let alg = algorithm.trim().to_ascii_lowercase();
    let sk_bytes = STANDARD.decode(secret_key_base64).unwrap_or_default();
    let message_bytes = hex::decode(message_hex.trim_start_matches("0x")).unwrap_or_default();
    let sig_bytes = match alg.as_str() {
        "sphincs" | "sphincs+" | "sphincs-256f" => {
            use pqcrypto_sphincsplus::sphincssha2256fsimple as alg;
            let sk = alg::SecretKey::from_bytes(&sk_bytes).unwrap();
            let sig = alg::detached_sign(&message_bytes, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-256s" => {
            use pqcrypto_sphincsplus::sphincssha2256ssimple as alg;
            let sk = alg::SecretKey::from_bytes(&sk_bytes).unwrap();
            let sig = alg::detached_sign(&message_bytes, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-192f" => {
            use pqcrypto_sphincsplus::sphincssha2192fsimple as alg;
            let sk = alg::SecretKey::from_bytes(&sk_bytes).unwrap();
            let sig = alg::detached_sign(&message_bytes, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-192s" => {
            use pqcrypto_sphincsplus::sphincssha2192ssimple as alg;
            let sk = alg::SecretKey::from_bytes(&sk_bytes).unwrap();
            let sig = alg::detached_sign(&message_bytes, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-128f" => {
            use pqcrypto_sphincsplus::sphincssha2128fsimple as alg;
            let sk = alg::SecretKey::from_bytes(&sk_bytes).unwrap();
            let sig = alg::detached_sign(&message_bytes, &sk);
            sig.as_bytes().to_vec()
        }
        "sphincs-128s" => {
            use pqcrypto_sphincsplus::sphincssha2128ssimple as alg;
            let sk = alg::SecretKey::from_bytes(&sk_bytes).unwrap();
            let sig = alg::detached_sign(&message_bytes, &sk);
            sig.as_bytes().to_vec()
        }
        _ => {
            use pqcrypto_sphincsplus::sphincssha2256fsimple as alg;
            let sk = alg::SecretKey::from_bytes(&sk_bytes).unwrap();
            let sig = alg::detached_sign(&message_bytes, &sk);
            sig.as_bytes().to_vec()
        }
    };

    STANDARD.encode(sig_bytes)
}

#[wasm_bindgen]
pub fn sphincs_verify(
    algorithm: &str,
    public_key_base64: &str,
    message_hex: &str,
    signature_base64: &str,
) -> bool {
    let alg = algorithm.trim().to_ascii_lowercase();
    let pk_bytes = STANDARD.decode(public_key_base64).unwrap_or_default();
    let message_bytes = hex::decode(message_hex.trim_start_matches("0x")).unwrap_or_default();
    let sig_bytes = STANDARD.decode(signature_base64).unwrap_or_default();
    match alg.as_str() {
        "sphincs" | "sphincs+" | "sphincs-256f" => {
            use pqcrypto_sphincsplus::sphincssha2256fsimple as alg;
            let pk = alg::PublicKey::from_bytes(&pk_bytes).unwrap();
            let sig = alg::DetachedSignature::from_bytes(&sig_bytes).unwrap();
            alg::verify_detached_signature(&sig, &message_bytes, &pk).is_ok()
        }
        "sphincs-256s" => {
            use pqcrypto_sphincsplus::sphincssha2256ssimple as alg;
            let pk = alg::PublicKey::from_bytes(&pk_bytes).unwrap();
            let sig = alg::DetachedSignature::from_bytes(&sig_bytes).unwrap();
            alg::verify_detached_signature(&sig, &message_bytes, &pk).is_ok()
        }
        "sphincs-192f" => {
            use pqcrypto_sphincsplus::sphincssha2192fsimple as alg;
            let pk = alg::PublicKey::from_bytes(&pk_bytes).unwrap();
            let sig = alg::DetachedSignature::from_bytes(&sig_bytes).unwrap();
            alg::verify_detached_signature(&sig, &message_bytes, &pk).is_ok()
        }
        "sphincs-192s" => {
            use pqcrypto_sphincsplus::sphincssha2192ssimple as alg;
            let pk = alg::PublicKey::from_bytes(&pk_bytes).unwrap();
            let sig = alg::DetachedSignature::from_bytes(&sig_bytes).unwrap();
            alg::verify_detached_signature(&sig, &message_bytes, &pk).is_ok()
        }
        "sphincs-128f" => {
            use pqcrypto_sphincsplus::sphincssha2128fsimple as alg;
            let pk = alg::PublicKey::from_bytes(&pk_bytes).unwrap();
            let sig = alg::DetachedSignature::from_bytes(&sig_bytes).unwrap();
            alg::verify_detached_signature(&sig, &message_bytes, &pk).is_ok()
        }
        "sphincs-128s" => {
            use pqcrypto_sphincsplus::sphincssha2128ssimple as alg;
            let pk = alg::PublicKey::from_bytes(&pk_bytes).unwrap();
            let sig = alg::DetachedSignature::from_bytes(&sig_bytes).unwrap();
            alg::verify_detached_signature(&sig, &message_bytes, &pk).is_ok()
        }
        _ => {
            use pqcrypto_sphincsplus::sphincssha2256fsimple as alg;
            let pk = alg::PublicKey::from_bytes(&pk_bytes).unwrap();
            let sig = alg::DetachedSignature::from_bytes(&sig_bytes).unwrap();
            alg::verify_detached_signature(&sig, &message_bytes, &pk).is_ok()
        }
    }
}
