use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(feature = "quantum")]
use spacekit_primitives::v1::crypto::quantum::{verify_sphincs_signature, SPHINCSSignature};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedArchive {
    pub from_height: u64,
    pub to_height: u64,
    pub block_count: u64,
    pub seal_hash: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleSignature {
    pub algorithm: String,
    pub public_key_hex: String,
    pub signature_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPolicy {
    pub allowed_keys: Vec<KeyRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyRule {
    pub public_key_hex: String,
    pub expires_at: Option<u64>,
    pub revoked: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleTxPayload {
    pub block_index: u64,
    pub from: String,
    pub to: Option<String>,
    pub data: String,
    pub value: String,
    pub gas_limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RollupBundle {
    pub bundle_id: String,
    pub from_height: u64,
    pub to_height: u64,
    pub block_count: u64,
    pub block_hashes: Vec<String>,
    pub state_roots: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantum_state_roots: Option<Vec<String>>,
    pub tx_roots: Vec<String>,
    pub receipt_roots: Vec<String>,
    pub sealed_archives: Vec<SealedArchive>,
    pub timestamp: u64,
    pub bundle_hash: String,
    pub signature: Option<BundleSignature>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_payloads: Option<Vec<BundleTxPayload>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReExecutionResult {
    pub block_index: u64,
    pub expected_state_root: String,
    pub computed_state_root: String,
    pub match_ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleVerificationResult {
    pub bundle_id: String,
    pub hash_valid: bool,
    pub signature_valid: bool,
    pub key_allowed: bool,
    pub re_execution_results: Vec<ReExecutionResult>,
    pub all_roots_match: bool,
    pub challenge_window_end: u64,
    pub status: BundleStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BundleStatus {
    Verified,
    Challenged,
    Rejected,
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleValidationResult {
    pub hash_valid: bool,
    pub signature_valid: bool,
    pub key_allowed: bool,
    pub expected_hash: String,
}

/// Domain separator so a bundle hash can never collide with another SpaceKit
/// structure that happens to serialize to the same JSON.
const BUNDLE_HASH_DOMAIN: &[u8] = b"SPACEKIT-ROLLUP-BUNDLE-v2\n";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BundleHashPayload<'a> {
    /// Included so a signature cannot be lifted onto a differently-identified
    /// bundle carrying the same heights and roots.
    bundle_id: &'a str,
    from_height: u64,
    to_height: u64,
    block_count: u64,
    block_hashes: &'a [String],
    state_roots: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    quantum_state_roots: Option<&'a [String]>,
    tx_roots: &'a [String],
    receipt_roots: &'a [String],
    sealed_archives: &'a [SealedArchive],
    /// Included so the signature commits to *which* transactions were bridged.
    /// Without this, the payloads could be swapped wholesale while the
    /// signature stayed valid.
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_payloads: Option<&'a [BundleTxPayload]>,
    timestamp: u64,
}

pub fn compute_bundle_hash(bundle: &RollupBundle) -> Result<String, String> {
    let payload = BundleHashPayload {
        bundle_id: &bundle.bundle_id,
        from_height: bundle.from_height,
        to_height: bundle.to_height,
        block_count: bundle.block_count,
        block_hashes: &bundle.block_hashes,
        state_roots: &bundle.state_roots,
        quantum_state_roots: bundle.quantum_state_roots.as_deref(),
        tx_roots: &bundle.tx_roots,
        receipt_roots: &bundle.receipt_roots,
        sealed_archives: &bundle.sealed_archives,
        tx_payloads: bundle.tx_payloads.as_deref(),
        timestamp: bundle.timestamp,
    };
    let json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    hasher.update(BUNDLE_HASH_DOMAIN);
    hasher.update(json.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

pub fn validate_rollup_bundle(bundle: &RollupBundle) -> Result<BundleValidationResult, String> {
    let expected_hash = compute_bundle_hash(bundle)?;
    let hash_valid = expected_hash == bundle.bundle_hash;

    let mut signature_valid = false;
    let mut key_allowed = false;
    if let Some(signature) = &bundle.signature {
        let msg = hex::decode(&bundle.bundle_hash).map_err(|e| e.to_string())?;
        match signature.algorithm.as_str() {
            "ed25519" => {
                let pub_key_bytes =
                    hex::decode(&signature.public_key_hex).map_err(|e| e.to_string())?;
                if pub_key_bytes.len() != 32 {
                    return Err("Invalid public key length".to_string());
                }
                let sig_bytes =
                    base64::decode(&signature.signature_base64).map_err(|e| e.to_string())?;
                if sig_bytes.len() != 64 {
                    return Err("Invalid signature length".to_string());
                }
                let mut pk = [0u8; 32];
                pk.copy_from_slice(&pub_key_bytes);
                let mut sig = [0u8; 64];
                sig.copy_from_slice(&sig_bytes);
                let verifying_key = VerifyingKey::from_bytes(&pk).map_err(|e| e.to_string())?;
                let signature = Signature::from_bytes(&sig);
                signature_valid = verifying_key.verify(&msg, &signature).is_ok();
                key_allowed = signature_valid;
            }
            "sphincs" | "sphincs+" | "sphincs-128f" | "sphincs-128s" | "sphincs-192f"
            | "sphincs-192s" | "sphincs-256f" | "sphincs-256s" => {
                #[cfg(feature = "quantum")]
                {
                    let pub_key_bytes =
                        hex::decode(&signature.public_key_hex).map_err(|e| e.to_string())?;
                    let sig_bytes =
                        base64::decode(&signature.signature_base64).map_err(|e| e.to_string())?;
                    let sphincs_sig = SPHINCSSignature {
                        signature_bytes: sig_bytes,
                        algorithm: signature.algorithm.clone(),
                        public_key: pub_key_bytes,
                    };
                    signature_valid =
                        verify_sphincs_signature(&msg, &sphincs_sig).map_err(|e| e.to_string())?;
                    key_allowed = signature_valid;
                }
                #[cfg(not(feature = "quantum"))]
                {
                    return Err("Quantum feature not enabled".to_string());
                }
            }
            other => {
                return Err(format!("Unsupported signature algorithm: {}", other));
            }
        }
    }

    Ok(BundleValidationResult {
        hash_valid,
        signature_valid,
        key_allowed,
        expected_hash,
    })
}

pub fn validate_rollup_bundle_with_policy(
    bundle: &RollupBundle,
    policy: &KeyPolicy,
    now: u64,
) -> Result<BundleValidationResult, String> {
    let mut result = validate_rollup_bundle(bundle)?;
    let signature = match &bundle.signature {
        Some(sig) => sig,
        None => {
            result.key_allowed = false;
            return Ok(result);
        }
    };

    let key_rule = policy
        .allowed_keys
        .iter()
        .find(|rule| rule.public_key_hex == signature.public_key_hex);
    if let Some(rule) = key_rule {
        if rule.revoked.unwrap_or(false) {
            result.key_allowed = false;
        } else if let Some(expires_at) = rule.expires_at {
            result.key_allowed = now <= expires_at;
        } else {
            result.key_allowed = true;
        }
    } else {
        result.key_allowed = false;
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MerkleStep {
    pub sibling: String,
    pub position: String,
}

pub fn verify_merkle_proof(leaf: &str, proof: &[MerkleStep], root: &str) -> Result<bool, String> {
    let mut hash = hash_leaf(leaf)?;
    for step in proof {
        if step.position == "left" {
            hash = hash_pair(&step.sibling, &hash)?;
        } else if step.position == "right" {
            hash = hash_pair(&hash, &step.sibling)?;
        } else {
            return Err("Invalid merkle step position".to_string());
        }
    }
    Ok(hash == root)
}

fn hash_leaf(value: &str) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(format!("leaf:{}", value).as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

fn hash_pair(left: &str, right: &str) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(format!("node:{}:{}", left, right).as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

/// Default challenge window: 300 seconds (5 minutes for local/testnet, production would be longer).
pub const DEFAULT_CHALLENGE_WINDOW_SECS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FraudProof {
    pub bundle_id: String,
    pub block_index: u64,
    pub expected_state_root: String,
    pub computed_state_root: String,
    pub challenger_did: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashRecord {
    pub bundle_id: String,
    pub sequencer_key: String,
    pub reason: String,
    pub fraud_proof: FraudProof,
    pub slash_amount: u64,
    pub timestamp: u64,
}
