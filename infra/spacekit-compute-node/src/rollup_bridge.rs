use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(feature = "quantum")]
use spacekit_primitives::v1::crypto::quantum::{
    verify_slh_dsa_signature, verify_sphincs_signature, SPHINCSSignature,
};

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
            // FIPS-205 SLH-DSA wire strings (browser wasm-did / CLI). Verified
            // with the RustCrypto `slh-dsa` crate — NOT pqcrypto's SPHINCS+ r3,
            // which is a different, non-interoperable scheme despite equal sizes.
            "slh-dsa-sha2-128s" | "slh-dsa-128s" | "slh-dsa-sha2-192s" | "slh-dsa-192s" => {
                #[cfg(feature = "quantum")]
                {
                    let pub_key_bytes =
                        hex::decode(&signature.public_key_hex).map_err(|e| e.to_string())?;
                    let sig_bytes =
                        base64::decode(&signature.signature_base64).map_err(|e| e.to_string())?;
                    signature_valid = verify_slh_dsa_signature(
                        &msg,
                        &signature.algorithm,
                        &pub_key_bytes,
                        &sig_bytes,
                    )
                    .map_err(|e| e.to_string())?;
                    key_allowed = signature_valid;
                }
                #[cfg(not(feature = "quantum"))]
                {
                    return Err("Quantum feature not enabled".to_string());
                }
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

/// The PQ address (`0x` + hex(SHA-256(pubkey)[0..20])) implied by a bundle
/// signature's public key. Matches the frontend `pqAddressFromPublicKey` and
/// `SwtchvmAddress::from_pq_public_key`, so the address a bundle is signed under
/// is exactly the address whose funds it is allowed to move.
pub fn signer_pq_address_hex(signature: &BundleSignature) -> Result<String, String> {
    let pk = hex::decode(&signature.public_key_hex).map_err(|e| e.to_string())?;
    if pk.is_empty() {
        return Err("empty signer public key".to_string());
    }
    let digest = Sha256::digest(&pk);
    Ok(format!("0x{}", hex::encode(&digest[..20])))
}

/// Guardrail for **self-custody** (browser-submitted) settlement: every native
/// transfer in the bundle must spend FROM the signer's own address. A signature
/// authorizes moving the signer's own funds and nothing else, so a browser
/// cannot sign a bundle that drains an address it does not control. `to` may be
/// any address (you can pay anyone). Returns the signer address on success.
///
/// This is what makes the un-operator-gated `/rollup/submit` route safe: the
/// signature *is* the authorization, and it can only authorize the signer's
/// own outflows.
pub fn enforce_self_custody(bundle: &RollupBundle) -> Result<String, String> {
    let signature = bundle
        .signature
        .as_ref()
        .ok_or_else(|| "bundle is unsigned".to_string())?;
    let signer = signer_pq_address_hex(signature)?.to_lowercase();
    if let Some(payloads) = &bundle.tx_payloads {
        for tx in payloads {
            let mut from = tx.from.trim().to_lowercase();
            if !from.starts_with("0x") {
                from = format!("0x{from}");
            }
            if from != signer {
                return Err(format!(
                    "self-custody violation: signer {} cannot spend from {}",
                    signer, tx.from
                ));
            }
        }
    }
    Ok(signer)
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pq_address_matches_spec_vector() {
        // SHA-256("")[0..20] — the canonical PQ address rule (spacekit-did).
        let a = hex::encode(&Sha256::digest(b"")[..20]);
        assert_eq!(a, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4");
    }

    #[test]
    fn self_custody_allows_own_outflow_and_rejects_foreign() {
        let sig = BundleSignature {
            algorithm: "slh-dsa-sha2-128s".into(),
            public_key_hex:
                "8f7a8204730b4f0422b78f3e112e3d551bea6c1f3d7ae6cddab1f15b2d32c019".into(),
            signature_base64: String::new(),
        };
        let signer = signer_pq_address_hex(&sig).unwrap();

        let mk = |from: &str| RollupBundle {
            bundle_id: "b1".into(),
            from_height: 0,
            to_height: 0,
            block_count: 0,
            block_hashes: vec![],
            state_roots: vec![],
            quantum_state_roots: None,
            tx_roots: vec![],
            receipt_roots: vec![],
            sealed_archives: vec![],
            timestamp: 0,
            bundle_hash: String::new(),
            signature: Some(sig.clone()),
            tx_payloads: Some(vec![BundleTxPayload {
                block_index: 0,
                from: from.to_string(),
                to: Some("0x000000000000000000000000000000000000dead".into()),
                data: String::new(),
                value: "100".into(),
                gas_limit: 0,
            }]),
        };

        // Spending from the signer's own address is allowed; any other from is not.
        assert!(enforce_self_custody(&mk(&signer)).is_ok());
        assert!(enforce_self_custody(&mk("0x00000000000000000000000000000000deadbeef")).is_err());
    }

    #[test]
    fn bundle_hash_matches_browser_canonical_vector() {
        // Reference hash computed in kit.space-website (settlement.ts
        // `computeBundleHash`) for this exact transfer bundle. Proves the JS and
        // Rust bundle-hash canonical encodings agree, so a browser-signed
        // bundle's `bundle_hash` is accepted by the node (`hash_valid`).
        let bundle = RollupBundle {
            bundle_id: "xfer-test-v1".into(),
            from_height: 0,
            to_height: 0,
            block_count: 0,
            block_hashes: vec![],
            state_roots: vec![],
            quantum_state_roots: None,
            tx_roots: vec![],
            receipt_roots: vec![],
            sealed_archives: vec![],
            timestamp: 1234567890,
            bundle_hash: String::new(),
            signature: None,
            tx_payloads: Some(vec![BundleTxPayload {
                block_index: 0,
                from: "0x8f7a8204730b4f0422b78f3e112e3d551bea6c1f".into(),
                to: Some("0x000000000000000000000000000000000000dead".into()),
                data: String::new(),
                value: "1000".into(),
                gas_limit: 0,
            }]),
        };
        assert_eq!(
            compute_bundle_hash(&bundle).unwrap(),
            "f8f2c01fb084fd9656a6bd03c1adddfa03508f16cb1bf65e1af23b81ba872a4e"
        );
    }

    /// A pqcrypto SPHINCS+ round-trip stays green under the `sphincs-*` strings.
    #[cfg(feature = "quantum")]
    #[test]
    fn sphincs_128s_roundtrip_verifies() {
        use spacekit_primitives::v1::crypto::quantum::{
            generate_sphincs_keypair, sign_sphincs_detached,
        };
        let (pk, sk) = generate_sphincs_keypair("sphincs-128s").unwrap();
        let msg = b"rollup-bundle-hash";
        let signed = sign_sphincs_detached(msg, "sphincs-128s", &pk, &sk).unwrap();
        let s = SPHINCSSignature {
            signature_bytes: signed.signature_bytes,
            algorithm: "sphincs-128s".into(),
            public_key: pk,
        };
        assert!(verify_sphincs_signature(msg, &s).unwrap());
    }

    // ── SLH-DSA (FIPS-205) browser determinism vector ──────────────────────
    //
    // A real (public key, signature) generated in kit.space-website by
    // `wasm-did`'s SLH-DSA-SHA2-128s bindings (pure-Rust `slh-dsa` / FIPS-205),
    // signing VECTOR_MSG. It MUST verify under this node's
    // `verify_slh_dsa_signature`, proving the browser signer and the node
    // verifier share one parameter set — i.e. a browser-signed settlement bundle
    // is acceptable here. If this fails, the two SLH-DSA impls diverged and
    // settlement signatures would be silently rejected.
    #[cfg(feature = "quantum")]
    const VECTOR_MSG: &[u8] = b"spacekit-slh-dsa-determinism-vector-v1";
    #[cfg(feature = "quantum")]
    const VECTOR_PUB_HEX: &str =
        "8f7a8204730b4f0422b78f3e112e3d551bea6c1f3d7ae6cddab1f15b2d32c019";
    #[cfg(feature = "quantum")]
    const VECTOR_SIG_HEX: &str = include_str!("../tests/vectors/slh_dsa_128s_browser.sig.hex");

    #[cfg(feature = "quantum")]
    #[test]
    fn slh_dsa_browser_wasm_vector_verifies() {
        let pk = hex::decode(VECTOR_PUB_HEX).unwrap();
        let sig = hex::decode(VECTOR_SIG_HEX.trim()).unwrap();
        assert_eq!(pk.len(), 32, "SLH-DSA-SHA2-128s public key is 32 bytes");
        assert_eq!(sig.len(), 7856, "SLH-DSA-SHA2-128s signature is 7856 bytes");
        let ok =
            verify_slh_dsa_signature(VECTOR_MSG, "slh-dsa-sha2-128s", &pk, &sig).unwrap();
        assert!(
            ok,
            "browser wasm-did SLH-DSA-128s signature must verify on the node"
        );
    }

    /// Negative control: the same signature must NOT verify for a different
    /// message — guards against an accidentally-permissive verifier.
    #[cfg(feature = "quantum")]
    #[test]
    fn slh_dsa_browser_wasm_vector_rejects_tampered_message() {
        let pk = hex::decode(VECTOR_PUB_HEX).unwrap();
        let sig = hex::decode(VECTOR_SIG_HEX.trim()).unwrap();
        let ok = verify_slh_dsa_signature(
            b"spacekit-slh-dsa-determinism-vector-v2",
            "slh-dsa-sha2-128s",
            &pk,
            &sig,
        )
        .unwrap();
        assert!(!ok, "signature must not verify for a different message");
    }
}
