//! Intent and SignedIntent types plus signature verification (ed25519, secp256k1/EVM, quantum).
//! Matches SpaceKit Intent Protocol v0.2; relay validates then forwards.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Minimal Intent shape for relay validation. Full schema in SPACEKIT-INTENT-PROTOCOL-SPEC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent_id: String,
    pub version: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub chain: String,
    pub constraints: serde_json::Value,
    pub actions: serde_json::Value,
    pub nonce: String,
    pub expiry: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// SignedIntent as received by the relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedIntent {
    pub intent: Intent,
    /// Hex-encoded signature over intent.intent_id (as bytes).
    pub signature: String,
    pub sig_type: String,
}

/// Validation result for the relay.
pub struct Validation {
    pub ok: bool,
    pub error_code: Option<&'static str>,
    pub error_message: Option<String>,
}

const MIN_EXPIRY_SECS: i64 = 30;
const SUPPORTED_VERSION: &str = "1.0";

/// Validate intent: schema, version, expiry. Does not verify signature.
pub fn validate_intent(intent: &Intent, now_secs: i64) -> Validation {
    if intent.version != SUPPORTED_VERSION {
        return Validation {
            ok: false,
            error_code: Some("SCHEMA_INVALID"),
            error_message: Some(format!("unsupported version {}", intent.version)),
        };
    }
    if intent.intent_id.is_empty() {
        return Validation {
            ok: false,
            error_code: Some("SCHEMA_INVALID"),
            error_message: Some("intent_id is required".to_string()),
        };
    }
    if intent.actor.is_empty() {
        return Validation {
            ok: false,
            error_code: Some("SCHEMA_INVALID"),
            error_message: Some("actor is required".to_string()),
        };
    }
    if intent.expiry <= now_secs + MIN_EXPIRY_SECS {
        return Validation {
            ok: false,
            error_code: Some("EXPIRY_EXCEEDED"),
            error_message: Some("intent expiry too soon or already expired".to_string()),
        };
    }
    Validation {
        ok: true,
        error_code: None,
        error_message: None,
    }
}

/// Verify Ed25519 signature: signature (hex) over intent_id (hex decoded to bytes) with public key (actor as 64-char hex = 32-byte pubkey).
pub fn verify_ed25519(
    intent_id_hex: &str,
    signature_hex: &str,
    actor_hex: &str,
) -> Result<(), String> {
    let intent_id_bytes =
        hex::decode(intent_id_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    if intent_id_bytes.len() != 32 {
        return Err("intent_id must be 32 bytes (SHA-256)".to_string());
    }
    let sig_bytes =
        hex::decode(signature_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    if sig_bytes.len() != 64 {
        return Err("ed25519 signature must be 64 bytes".to_string());
    }
    let pk_bytes = hex::decode(actor_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    if pk_bytes.len() != 32 {
        return Err("ed25519 actor (public key) must be 32 bytes".to_string());
    }

    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let pk = VerifyingKey::from_bytes(
        pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "invalid pubkey")?,
    )
    .map_err(|_| "invalid ed25519 pubkey".to_string())?;
    let sig = Signature::from_bytes(
        sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "invalid signature")?,
    );
    pk.verify(&intent_id_bytes, &sig)
        .map_err(|_| "SIG_INVALID: signature verification failed".to_string())?;
    Ok(())
}

/// EIP-191 prefix for personal_sign. Message hashed is: prefix + len(decimal) + message.
const EIP191_PREFIX: &[u8] = b"\x19Ethereum Signed Message:\n";

/// Build keccak256 hash of EIP-191 personal_sign message (prefix + len + payload).
fn eip191_hash(message: &[u8]) -> [u8; 32] {
    use sha3::{Digest, Keccak256};
    let len_str = message.len().to_string();
    let mut prefixed = Vec::with_capacity(EIP191_PREFIX.len() + len_str.len() + message.len());
    prefixed.extend_from_slice(EIP191_PREFIX);
    prefixed.extend_from_slice(len_str.as_bytes());
    prefixed.extend_from_slice(message);
    *Keccak256::digest(&prefixed).as_ref()
}

/// Verify EIP-191 `personal_sign` over a **UTF-8 string** (e.g. Agent Hub charge message). Matches viem `verifyMessage`.
pub fn verify_eip191_utf8_message(
    message: &str,
    signature_hex: &str,
    actor_hex: &str,
) -> Result<(), String> {
    let digest = eip191_hash(message.as_bytes());
    recover_and_match_eth_address(&digest, signature_hex, actor_hex)
}

/// Shared recovery: keccak digest → address, compare to actor.
fn recover_and_match_eth_address(
    digest: &[u8; 32],
    signature_hex: &str,
    actor_hex: &str,
) -> Result<(), String> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    use sha3::Digest;

    let sig_bytes =
        hex::decode(signature_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    if sig_bytes.len() != 65 {
        return Err("secp256k1 signature must be 65 bytes (r,s,v)".to_string());
    }
    let actor_bytes = hex::decode(actor_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    if actor_bytes.len() != 20 {
        return Err("actor must be 20 bytes (Ethereum address) for secp256k1".to_string());
    }

    let v = sig_bytes[64];
    let recid_byte = v.checked_sub(27).ok_or("invalid v (must be 27 or 28)")?;
    if recid_byte > 1 {
        return Err("invalid recovery id (v must be 27 or 28)".to_string());
    }
    let recovery_id =
        RecoveryId::try_from(recid_byte).map_err(|_| "invalid recovery id".to_string())?;
    let sig = Signature::try_from(sig_bytes[..64].as_ref())
        .map_err(|e| format!("invalid signature: {:?}", e))?;
    let vk = VerifyingKey::recover_from_prehash(digest, &sig, recovery_id)
        .map_err(|_| "SIG_INVALID: secp256k1 recovery failed".to_string())?;

    let uncompressed = vk.to_encoded_point(false);
    let pubkey_bytes = uncompressed.as_bytes();
    let addr_hash = sha3::Keccak256::digest(pubkey_bytes);
    let recovered_addr: [u8; 20] = addr_hash[12..32]
        .try_into()
        .map_err(|_| "SIG_INVALID: address derivation failed".to_string())?;
    if recovered_addr.as_slice() != actor_bytes.as_slice() {
        return Err("SIG_INVALID: recovered address does not match actor".to_string());
    }
    Ok(())
}

/// Verify secp256k1 (EVM) signature: EIP-191 personal_sign over intent_id bytes, then ecrecover; actor is 20-byte address (hex).
fn verify_secp256k1(
    intent_id_hex: &str,
    signature_hex: &str,
    actor_hex: &str,
) -> Result<(), String> {
    let intent_id_bytes =
        hex::decode(intent_id_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    if intent_id_bytes.len() != 32 {
        return Err("intent_id must be 32 bytes for secp256k1 (EIP-191)".to_string());
    }
    let digest = eip191_hash(&intent_id_bytes);
    recover_and_match_eth_address(&digest, signature_hex, actor_hex)
}

/// Verify quantum (ML-DSA-65 or SLH-DSA) signature using spacekit-primitives.
fn verify_quantum(
    intent_id_hex: &str,
    signature_hex: &str,
    actor_hex: &str,
    variant: spacekit_primitives::secrets_core::SignerVariant,
) -> Result<(), String> {
    use spacekit_primitives::secrets_core::verify;
    let message = hex::decode(intent_id_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    let sig_bytes =
        hex::decode(signature_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    let pk_bytes = hex::decode(actor_hex.trim_start_matches("0x")).map_err(|e| e.to_string())?;
    verify(&pk_bytes, &message, &sig_bytes, &variant).map_err(|e| format!("SIG_INVALID: {}", e))?;
    Ok(())
}

/// Verify signature based on sig_type. Supports ed25519, secp256k1 (stub), and quantum (mldsa65, slh_dsa_sha2_128s, slh_dsa_sha2_192s).
pub fn verify_signature(
    intent_id_hex: &str,
    signature_hex: &str,
    actor: &str,
    sig_type: &str,
) -> Result<(), String> {
    use spacekit_primitives::secrets_core::SignerVariant;
    match sig_type {
        "ed25519" => verify_ed25519(intent_id_hex, signature_hex, actor),
        "secp256k1" => verify_secp256k1(intent_id_hex, signature_hex, actor),
        "mldsa65" => verify_quantum(intent_id_hex, signature_hex, actor, SignerVariant::MlDsa65),
        "slh_dsa_sha2_128s" => {
            verify_quantum(intent_id_hex, signature_hex, actor, SignerVariant::SlhDsaSha2128s)
        }
        "slh_dsa_sha2_192s" => {
            verify_quantum(intent_id_hex, signature_hex, actor, SignerVariant::SlhDsaSha2192s)
        }
        _ => Err(format!(
            "sig_type must be ed25519, secp256k1, mldsa65, slh_dsa_sha2_128s, or slh_dsa_sha2_192s; got {}",
            sig_type
        )),
    }
}

/// Current Unix time in seconds (for expiry check).
pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
