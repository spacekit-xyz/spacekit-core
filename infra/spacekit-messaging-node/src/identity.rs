//! Self-certifying node identity and signed gateway messages.
//!
//! A messaging node's DID is `did:spacekit:<address>`, where the address is
//! derived from the node's k256 key exactly as on the chain
//! (`keccak256(uncompressed public key)[12..]`). The same key signs:
//!
//! - every gateway message the node publishes on gossip (envelopes, group
//!   announcements, join requests), wrapped as [`SignedMessage`]; receivers
//!   recover the signer's address and accept the message only if it is the
//!   address in the DID the message claims;
//! - so a DID cannot be used by anyone but its key holder, no matter which
//!   peer relays the message, and nothing needs to be trusted on first use.
//!
//! The DID is also the node's chain identity: a channel created by this DID
//! is checked against the entitlement-ledger listing published by the same
//! DID.

use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sha3::Keccak256;

const DOMAIN: &str = "SPACEKIT-MSG-v1";

/// A node's signing key and the DID it certifies.
#[derive(Clone)]
pub struct NodeIdentity {
    key: SigningKey,
    pub did: String,
    pub address: [u8; 20],
}

impl std::fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeIdentity").field("did", &self.did).finish()
    }
}

pub fn address_of(key: &VerifyingKey) -> [u8; 20] {
    let point = key.to_encoded_point(false);
    let hash: [u8; 32] = Keccak256::digest(&point.as_bytes()[1..]).into();
    let mut a = [0u8; 20];
    a.copy_from_slice(&hash[12..]);
    a
}

pub fn did_for(address: &[u8; 20]) -> String {
    format!("did:spacekit:{}", hex::encode(address))
}

/// The address in `did:spacekit:<40 hex>` (any prefix before the last `:`).
pub fn did_address(did: &str) -> Option<[u8; 20]> {
    let suffix = did.trim().rsplit(':').next()?;
    let hex_part = suffix.strip_prefix("0x").unwrap_or(suffix);
    if hex_part.len() != 40 {
        return None;
    }
    hex::decode(hex_part).ok()?.try_into().ok()
}

impl NodeIdentity {
    /// From a 32-byte k256 secret key in hex.
    pub fn from_hex(secret_hex: &str) -> anyhow::Result<Self> {
        let bytes = hex::decode(secret_hex.trim().trim_start_matches("0x"))
            .map_err(|_| anyhow::anyhow!("private_key is not hex"))?;
        let key = SigningKey::from_slice(&bytes)
            .map_err(|_| anyhow::anyhow!("private_key is not a valid k256 secret key"))?;
        let address = address_of(key.verifying_key());
        Ok(Self {
            key,
            did: did_for(&address),
            address,
        })
    }

    pub fn generate() -> Self {
        let key = SigningKey::random(&mut rand::thread_rng());
        let address = address_of(key.verifying_key());
        Self {
            key,
            did: did_for(&address),
            address,
        }
    }

    /// Sign `body` (the JSON of a gateway message).
    pub fn sign(&self, body: String) -> SignedMessage {
        let hash = digest(&body);
        let (sig, recid) = self
            .key
            .sign_prehash_recoverable(&hash)
            .expect("k256 signing a 32-byte digest");
        let mut bytes = sig.to_bytes().to_vec();
        bytes.push(recid.to_byte());
        SignedMessage {
            body,
            signature_hex: hex::encode(bytes),
        }
    }
}

fn digest(body: &str) -> [u8; 32] {
    Sha256::new()
        .chain_update(DOMAIN.as_bytes())
        .chain_update(b"\n")
        .chain_update(body.as_bytes())
        .finalize()
        .into()
}

/// A gateway message and its signer's k256 signature (`r ‖ s ‖ recovery id`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedMessage {
    pub body: String,
    pub signature_hex: String,
}

impl SignedMessage {
    /// The address that signed `body`.
    pub fn signer(&self) -> Option<[u8; 20]> {
        let bytes = hex::decode(&self.signature_hex).ok()?;
        if bytes.len() != 65 {
            return None;
        }
        let sig = Signature::from_slice(&bytes[..64]).ok()?;
        let recid = RecoveryId::from_byte(bytes[64])?;
        let key = VerifyingKey::recover_from_prehash(&digest(&self.body), &sig, recid).ok()?;
        Some(address_of(&key))
    }

    /// True when the signature is by the address in `did`.
    pub fn signed_by(&self, did: &str) -> bool {
        matches!((self.signer(), did_address(did)), (Some(a), Some(b)) if a == b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_recover() {
        let id = NodeIdentity::from_hex(&"11".repeat(32)).unwrap();
        assert!(id.did.starts_with("did:spacekit:"));
        assert_eq!(did_address(&id.did), Some(id.address));
        let m = id.sign("{\"x\":1}".into());
        assert!(m.signed_by(&id.did));
        let other = NodeIdentity::generate();
        assert!(!m.signed_by(&other.did));
        let mut tampered = m.clone();
        tampered.body = "{\"x\":2}".into();
        assert!(!tampered.signed_by(&id.did));
        assert!(!m.signed_by("did:spacekit:user:a"));
        // Same derivation as the chain: key 0x…01 is 0x7e5f…5bdf.
        let one = NodeIdentity::from_hex(&format!("{}1", "0".repeat(63))).unwrap();
        assert_eq!(hex::encode(one.address), "7e5f4552091a69125d5dfcb7b8c2659029395bdf");
    }
}
