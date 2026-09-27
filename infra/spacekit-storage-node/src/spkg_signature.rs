//! Publisher signatures for SPKG archives.
//!
//! An SPKG may carry `signatures/publisher.json`:
//!
//! ```json
//! { "v": 1, "alg": "ed25519", "did": "did:key:z6Mk…", "public_key": "<64 hex>", "signature": "<128 hex>" }
//! ```
//!
//! The signature is Ed25519 over the UTF-8 bytes of
//! `"SpaceKit package signature v1\n" + hex(sha256(manifest.json))`.
//! `manifest.json` carries every payload hash and the aggregate checksum, so the
//! signature covers the whole package. `did` must be the `did:key` of
//! `public_key`; whether that DID is an acceptable publisher is the host's call
//! (it is usually the package's `creator_did`, or listed in a trust policy).
//!
//! The same file is kept in the CLI (`tools/spacekit-cli/src/spkg_signature.rs`)
//! and the storage node (`infra/spacekit-storage-node/src/spkg_signature.rs`),
//! and mirrored in the web SDK (`sdks/spacekit-sdk/lib/embed/spkgSignature.ts`).

use anyhow::{anyhow, bail, Context, Result};
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const SIGNATURE_ENTRY: &str = "signatures/publisher.json";
const DOMAIN: &str = "SpaceKit package signature v1\n";
const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublisherSignature {
    pub v: u8,
    pub alg: String,
    pub did: String,
    pub public_key: String,
    pub signature: String,
}

/// Bytes the publisher signs for a given `manifest.json`.
pub fn signing_message(manifest_json: &[u8]) -> Vec<u8> {
    let digest = Sha256::digest(manifest_json);
    format!("{DOMAIN}{}", hex::encode(digest)).into_bytes()
}

fn base58_encode(input: &[u8]) -> String {
    let mut digits: Vec<u8> = vec![0];
    for &byte in input {
        let mut carry = byte as u32;
        for d in digits.iter_mut() {
            carry += (*d as u32) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut out: String = input.iter().take_while(|&&b| b == 0).map(|_| '1').collect();
    out.extend(digits.iter().rev().map(|&d| ALPHABET[d as usize] as char));
    out
}

fn base58_decode(input: &str) -> Option<Vec<u8>> {
    let mut bytes: Vec<u8> = Vec::new();
    for c in input.bytes() {
        let mut carry = ALPHABET.iter().position(|&a| a == c)? as u32;
        for b in bytes.iter_mut().rev() {
            carry += (*b as u32) * 58;
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    let leading = input.bytes().take_while(|&c| c == b'1').count();
    let mut out = vec![0u8; leading];
    out.extend(bytes);
    Some(out)
}

/// W3C `did:key` for an Ed25519 public key.
pub fn did_key_for(public_key: &[u8; 32]) -> String {
    let mut data = Vec::with_capacity(34);
    data.extend_from_slice(&[0xed, 0x01]);
    data.extend_from_slice(public_key);
    format!("did:key:z{}", base58_encode(&data))
}

/// Whether `public_key` is the key behind `did` (W3C multibase form, or the
/// kit.space short form `did:key:z6Mk` + first 44 hex chars of the key).
pub fn ed25519_key_matches_did(did: &str, public_key: &[u8]) -> bool {
    if public_key.len() != 32 {
        return false;
    }
    let Some(suffix) = did.strip_prefix("did:key:z6Mk") else {
        return false;
    };
    let pk_hex = hex::encode(public_key);
    if suffix.len() == 44 && suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
        return suffix.eq_ignore_ascii_case(&pk_hex[..44]);
    }
    match base58_decode(&did["did:key:z".len()..]) {
        Some(d) => d.len() == 34 && d[0] == 0xed && d[1] == 0x01 && d[2..] == *public_key,
        None => false,
    }
}

/// Sign a package manifest. The DID is the key's W3C `did:key`.
pub fn sign(manifest_json: &[u8], key: &SigningKey) -> PublisherSignature {
    let public_key = key.verifying_key().to_bytes();
    let signature = key.sign(&signing_message(manifest_json));
    PublisherSignature {
        v: 1,
        alg: "ed25519".into(),
        did: did_key_for(&public_key),
        public_key: hex::encode(public_key),
        signature: hex::encode(signature.to_bytes()),
    }
}

/// Verify a signature entry against `manifest.json`. Returns the signer DID.
/// Errors on anything that is not a valid v1 Ed25519 signature bound to its DID.
pub fn verify(manifest_json: &[u8], entry: &[u8]) -> Result<String> {
    let sig: PublisherSignature =
        serde_json::from_slice(entry).context("signature entry is not valid JSON")?;
    if sig.v != 1 {
        bail!("unsupported signature version {}", sig.v);
    }
    if !sig.alg.eq_ignore_ascii_case("ed25519") {
        bail!("unsupported signature algorithm {}", sig.alg);
    }
    let pk = hex::decode(sig.public_key.trim()).context("public_key hex")?;
    if !ed25519_key_matches_did(&sig.did, &pk) {
        bail!("signature DID {} does not match its public key", sig.did);
    }
    let pk: [u8; 32] = pk.try_into().map_err(|_| anyhow!("public key must be 32 bytes"))?;
    let raw = hex::decode(sig.signature.trim()).context("signature hex")?;
    let raw: [u8; 64] = raw.try_into().map_err(|_| anyhow!("signature must be 64 bytes"))?;
    VerifyingKey::from_bytes(&pk)
        .map_err(|_| anyhow!("invalid public key"))?
        .verify_strict(&signing_message(manifest_json), &ed25519_dalek::Signature::from_bytes(&raw))
        .map_err(|_| anyhow!("package signature does not verify"))?;
    Ok(sig.did)
}

/// Load an Ed25519 signing key from a file holding the 32-byte seed as hex
/// (the kit.space recovery-key format; spaces and dashes are ignored).
pub fn load_signing_key(path: &std::path::Path) -> Result<SigningKey> {
    let raw = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let bytes = hex::decode(&cleaned).context("signing key must be hex")?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("signing key must be a 32-byte Ed25519 seed (64 hex chars)"))?;
    Ok(SigningKey::from_bytes(&seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_verify_and_tamper() {
        let key = SigningKey::from_bytes(&[5u8; 32]);
        let manifest = br#"{"manifest":{"checksum":"00"}}"#;
        let sig = sign(manifest, &key);
        assert!(sig.did.starts_with("did:key:z6Mk"));
        let entry = serde_json::to_vec(&sig).unwrap();
        assert_eq!(verify(manifest, &entry).unwrap(), sig.did);
        // Different manifest
        assert!(verify(br#"{"manifest":{"checksum":"01"}}"#, &entry).is_err());
        // Claimed DID of someone else
        let mut other = sig.clone();
        other.did = did_key_for(&SigningKey::from_bytes(&[6u8; 32]).verifying_key().to_bytes());
        assert!(verify(manifest, &serde_json::to_vec(&other).unwrap()).is_err());
        // kit.space short DID form binds too
        let pk = key.verifying_key().to_bytes();
        let short = format!("did:key:z6Mk{}", &hex::encode(pk)[..44]);
        assert!(ed25519_key_matches_did(&short, &pk));
        let mut s2 = sig.clone();
        s2.did = short.clone();
        assert_eq!(verify(manifest, &serde_json::to_vec(&s2).unwrap()).unwrap(), short);
        // Swapped key + resigned by attacker still names attacker's DID
        let attacker = SigningKey::from_bytes(&[9u8; 32]);
        let forged = sign(manifest, &attacker);
        assert_ne!(verify(manifest, &serde_json::to_vec(&forged).unwrap()).unwrap(), sig.did);
    }
}
