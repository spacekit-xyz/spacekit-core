//! KeyMaster — centralized key escrow for storage node server keypairs.
//!
//! Storage nodes must be able to decrypt envelopes stored to their Kyber
//! public key.  Rather than persisting the secret key as plaintext JSON,
//! the KeyMaster holds an encrypted copy and releases it only to
//! authenticated storage nodes over a DID-verified KEM channel.
//!
//! ## Protocol
//!
//! 1. **Register** (`POST /v1/keymaster/register`)
//!    Storage node generates an ephemeral Kyber keypair, sends:
//!    `{ node_did, ephemeral_pk_hex, server_pk_hex?, server_sk_hex? }`
//!    - On first call the node includes the full server keypair; the
//!      KeyMaster stores it encrypted under its own master key.
//!    - On subsequent calls (restarts) the node sends only `node_did` +
//!      `ephemeral_pk_hex`; the KeyMaster looks up the stored keypair,
//!      re-encrypts the secret key to the ephemeral PK, and returns it.
//!
//! 2. **Rotate** (`POST /v1/keymaster/rotate`)
//!    Accepts a new server keypair from the node; replaces the escrowed
//!    copy.  The old key is kept in a `previous_keys` list so envelopes
//!    encrypted to earlier keys can still be decrypted during migration.
//!
//! ## Storage
//!
//! The master escrow is an in-memory `HashMap<String, EscrowEntry>` that
//! is periodically flushed to `{data_dir}/keymaster_escrow.json`,
//! encrypted with AES-256-GCM under a key derived from the compute
//! node's own identity.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;

/// A single escrowed server keypair plus rotation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscrowEntry {
    pub node_did: String,
    pub server_pk_hex: String,
    pub server_sk_hex: String,
    pub algorithm: String,
    pub registered_at: u64,
    pub previous_keys: Vec<PreviousKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviousKey {
    pub server_pk_hex: String,
    pub server_sk_hex: String,
    pub rotated_at: u64,
}

/// In-memory escrow store.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyMasterStore {
    pub entries: HashMap<String, EscrowEntry>,
}

/// The subset of an escrow entry that is safe to return over the API.
///
/// [`EscrowEntry`] contains `server_sk_hex`; serializing it into an HTTP
/// response hands the storage node's secret key to whoever made the request.
/// Handlers must return this type instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicEscrowInfo {
    pub node_did: String,
    pub server_pk_hex: String,
    pub algorithm: String,
    pub registered_at: u64,
    pub previous_key_count: usize,
}

impl From<&EscrowEntry> for PublicEscrowInfo {
    fn from(e: &EscrowEntry) -> Self {
        Self {
            node_did: e.node_did.clone(),
            server_pk_hex: e.server_pk_hex.clone(),
            algorithm: e.algorithm.clone(),
            registered_at: e.registered_at,
            previous_key_count: e.previous_keys.len(),
        }
    }
}

/// Minimum length for the operator escrow secret.
const MIN_ESCROW_SECRET_LEN: usize = 32;

/// Derives the 32-byte AES key protecting the escrow file.
///
/// The identity string alone is **not** secret — a node DID is published in
/// every block and discoverable by any peer, so deriving the key from it made
/// the escrow file decryptable by anyone who obtained a copy. The key is now
/// derived from an operator-held secret, with the identity used only as HKDF
/// salt so two nodes sharing a secret still get distinct keys.
fn derive_master_key(identity: &str) -> Result<[u8; 32], String> {
    let secret = std::env::var("SPACEKIT_KEYMASTER_SECRET").map_err(|_| {
        "SPACEKIT_KEYMASTER_SECRET is not set — refusing to encrypt the key escrow \
         under a publicly derivable key"
            .to_string()
    })?;
    derive_key_from_secret(&secret, identity)
}

/// The derivation itself, separated from environment lookup so it can be
/// tested without mutating global state.
fn derive_key_from_secret(secret: &str, identity: &str) -> Result<[u8; 32], String> {
    if secret.len() < MIN_ESCROW_SECRET_LEN {
        return Err(format!(
            "keymaster secret must be at least {MIN_ESCROW_SECRET_LEN} characters"
        ));
    }

    let hk = hkdf::Hkdf::<Sha256>::new(Some(identity.as_bytes()), secret.as_bytes());
    let mut out = [0u8; 32];
    hk.expand(b"spacekit-keymaster-v2", &mut out)
        .map_err(|e| format!("HKDF expand: {e}"))?;
    Ok(out)
}

fn encrypt_store(store: &KeyMasterStore, identity: &str) -> Result<Vec<u8>, String> {
    let key_bytes = derive_master_key(identity)?;
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).map_err(|e| format!("AES init: {}", e))?;

    let plaintext = serde_json::to_vec(store).map_err(|e| format!("serialize: {}", e))?;

    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_slice())
        .map_err(|e| format!("encrypt: {}", e))?;

    // Format: [12-byte nonce][ciphertext...]
    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

fn decrypt_store(blob: &[u8], identity: &str) -> Result<KeyMasterStore, String> {
    if blob.len() < 12 {
        return Err("escrow blob too short".into());
    }
    let key_bytes = derive_master_key(identity)?;
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).map_err(|e| format!("AES init: {}", e))?;

    let nonce = Nonce::from_slice(&blob[..12]);
    let plaintext = cipher
        .decrypt(nonce, &blob[12..])
        .map_err(|e| format!("decrypt escrow: {}", e))?;

    serde_json::from_slice(&plaintext).map_err(|e| format!("deserialize escrow: {}", e))
}

/// Save the escrow store to disk, encrypted.
pub fn save_escrow(store: &KeyMasterStore, path: &PathBuf, identity: &str) -> Result<(), String> {
    let blob = encrypt_store(store, identity)?;
    std::fs::write(path, blob).map_err(|e| format!("write escrow: {}", e))
}

/// Load the escrow store from disk.
pub fn load_escrow(path: &PathBuf, identity: &str) -> Result<KeyMasterStore, String> {
    let blob = std::fs::read(path).map_err(|e| format!("read escrow: {}", e))?;
    decrypt_store(&blob, identity)
}

/// Register (or retrieve) a server keypair for the given node DID.
///
/// - If `server_sk_hex` is provided (first-time registration), store it.
/// - If not provided (restart recovery), return the existing entry.
pub fn register_or_recover(
    store: &mut KeyMasterStore,
    node_did: &str,
    server_pk_hex: Option<&str>,
    server_sk_hex: Option<&str>,
    algorithm: Option<&str>,
) -> Result<EscrowEntry, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    if let Some(sk) = server_sk_hex {
        let pk = server_pk_hex.ok_or("server_pk_hex required with server_sk_hex")?;
        let algo = algorithm.unwrap_or("Kyber1024");

        if let Some(existing) = store.entries.get(node_did) {
            // Re-registration must not silently rotate. Treating it as a
            // rotation let a caller overwrite an existing node's escrowed key,
            // which both locks the real node out and installs a key the
            // attacker controls. Rotation goes through `rotate_key`.
            if existing.server_pk_hex != pk {
                return Err(format!(
                    "DID {node_did} already has an escrowed key; use the rotate endpoint \
                     to replace it"
                ));
            }
        } else {
            store.entries.insert(
                node_did.to_string(),
                EscrowEntry {
                    node_did: node_did.to_string(),
                    server_pk_hex: pk.to_string(),
                    server_sk_hex: sk.to_string(),
                    algorithm: algo.to_string(),
                    registered_at: now,
                    previous_keys: Vec::new(),
                },
            );
        }
    } else if !store.entries.contains_key(node_did) {
        return Err(format!(
            "No escrowed key for DID {} — first registration must include server_sk_hex",
            node_did
        ));
    }

    store
        .entries
        .get(node_did)
        .cloned()
        .ok_or_else(|| "impossible".into())
}

/// Rotate the server keypair for a node, preserving the old key in history.
pub fn rotate_key(
    store: &mut KeyMasterStore,
    node_did: &str,
    new_pk_hex: &str,
    new_sk_hex: &str,
    algorithm: &str,
) -> Result<(), String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let entry = store
        .entries
        .get_mut(node_did)
        .ok_or_else(|| format!("No escrowed key for DID {}", node_did))?;

    entry.previous_keys.push(PreviousKey {
        server_pk_hex: entry.server_pk_hex.clone(),
        server_sk_hex: entry.server_sk_hex.clone(),
        rotated_at: now,
    });

    entry.server_pk_hex = new_pk_hex.to_string();
    entry.server_sk_hex = new_sk_hex.to_string();
    entry.algorithm = algorithm.to_string();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_recover() {
        let mut store = KeyMasterStore::default();

        let entry = register_or_recover(
            &mut store,
            "did:spacekit:testnet:abc",
            Some("pk_aaa"),
            Some("sk_bbb"),
            Some("Kyber1024"),
        )
        .unwrap();
        assert_eq!(entry.server_sk_hex, "sk_bbb");

        // Recovery without SK
        let entry2 =
            register_or_recover(&mut store, "did:spacekit:testnet:abc", None, None, None).unwrap();
        assert_eq!(entry2.server_sk_hex, "sk_bbb");
    }

    #[test]
    fn rotate_preserves_history() {
        let mut store = KeyMasterStore::default();

        register_or_recover(
            &mut store,
            "did:spacekit:testnet:abc",
            Some("pk1"),
            Some("sk1"),
            Some("Kyber1024"),
        )
        .unwrap();

        rotate_key(
            &mut store,
            "did:spacekit:testnet:abc",
            "pk2",
            "sk2",
            "Kyber1024",
        )
        .unwrap();

        let entry = store.entries.get("did:spacekit:testnet:abc").unwrap();
        assert_eq!(entry.server_pk_hex, "pk2");
        assert_eq!(entry.server_sk_hex, "sk2");
        assert_eq!(entry.previous_keys.len(), 1);
        assert_eq!(entry.previous_keys[0].server_pk_hex, "pk1");
    }

    const TEST_SECRET: &str = "test-operator-secret-that-is-long-enough-32+";

    /// The escrow key is derived from an operator secret, so tests must supply
    /// one just as a deployment does.
    ///
    /// Tests run in parallel in one process, so no test may *unset* this —
    /// the missing-secret path is covered through `derive_key_from_secret`
    /// instead of by mutating the environment.
    fn with_escrow_secret() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| std::env::set_var("SPACEKIT_KEYMASTER_SECRET", TEST_SECRET));
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        with_escrow_secret();
        let mut store = KeyMasterStore::default();
        register_or_recover(&mut store, "did:test", Some("pk"), Some("sk"), None).unwrap();

        let blob = encrypt_store(&store, "my-node-id").unwrap();
        let recovered = decrypt_store(&blob, "my-node-id").unwrap();
        assert_eq!(
            recovered.entries.get("did:test").unwrap().server_sk_hex,
            "sk"
        );
    }

    #[test]
    fn wrong_identity_fails_decrypt() {
        with_escrow_secret();
        let store = KeyMasterStore::default();
        let blob = encrypt_store(&store, "node-a").unwrap();
        assert!(decrypt_store(&blob, "node-b").is_err());
    }

    #[test]
    fn escrow_rejects_a_short_secret() {
        assert!(derive_key_from_secret("too-short", "node-a").is_err());
        assert!(derive_key_from_secret("", "node-a").is_err());
    }

    #[test]
    fn escrow_key_differs_per_identity() {
        // Same operator secret, different node: distinct keys, so one node's
        // escrow file cannot be decrypted with another's derived key.
        assert_ne!(
            derive_key_from_secret(TEST_SECRET, "node-a").unwrap(),
            derive_key_from_secret(TEST_SECRET, "node-b").unwrap()
        );
    }

    #[test]
    fn escrow_key_depends_on_the_operator_secret() {
        // The node DID is public, so the key must not be derivable from it alone.
        assert_ne!(
            derive_key_from_secret(TEST_SECRET, "node-a").unwrap(),
            derive_key_from_secret(&format!("{TEST_SECRET}-other"), "node-a").unwrap()
        );
    }

    #[test]
    fn re_registration_with_a_different_key_is_refused() {
        let mut store = KeyMasterStore::default();
        register_or_recover(&mut store, "did:test", Some("pk1"), Some("sk1"), None).unwrap();

        // Same key is idempotent.
        assert!(
            register_or_recover(&mut store, "did:test", Some("pk1"), Some("sk1"), None).is_ok()
        );

        // A different key must go through rotation, not registration.
        assert!(
            register_or_recover(&mut store, "did:test", Some("pk2"), Some("sk2"), None).is_err()
        );
        assert_eq!(store.entries["did:test"].server_pk_hex, "pk1");
    }

    #[test]
    fn public_info_omits_the_secret_key() {
        let mut store = KeyMasterStore::default();
        let entry =
            register_or_recover(&mut store, "did:test", Some("pk"), Some("sk"), None).unwrap();
        let json = serde_json::to_string(&PublicEscrowInfo::from(&entry)).unwrap();
        assert!(
            !json.contains("sk"),
            "secret key leaked into public info: {json}"
        );
        assert!(json.contains("pk"));
    }
}
