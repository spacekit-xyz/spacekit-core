use crate::v1::keypair::{KeyPair, KeyType};
use alloy_primitives::Address;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Quantum-resistant Decentralized Identifier
/// A unique identifier for entities in the SWTCH Network that supports quantum-safe operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct QuantumDID {
    /// The DID string in format: "did:swtchx:identifier" (or legacy "did:swtch:identifier")
    pub did: String,
}

impl QuantumDID {
    /// Create a new QuantumDID from a DID string
    pub fn new(did: String) -> Self {
        Self { did }
    }

    /// Create a QuantumDID from an address
    pub fn from_address(address: &Address) -> Self {
        Self {
            did: format!("did:swtchx:{:?}", address),
        }
    }

    /// Get the DID string
    pub fn as_str(&self) -> &str {
        &self.did
    }

    /// Convert to bytes for hashing and cryptographic operations
    pub fn to_bytes(&self) -> Vec<u8> {
        self.did.as_bytes().to_vec()
    }

    /// Parse a QuantumDID from a string, validating format
    pub fn parse(did_str: &str) -> Result<Self, String> {
        if did_str.starts_with("did:spacekit:")
            || did_str.starts_with("did:swtchx:")
            || did_str.starts_with("did:swtch:")
        {
            Ok(Self {
                did: did_str.to_string(),
            })
        } else {
            Err("Invalid DID format: must start with 'did:spacekit:', 'did:swtchx:', or 'did:swtch:'".to_string())
        }
    }
}

impl std::fmt::Display for QuantumDID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.did)
    }
}

impl AsRef<str> for QuantumDID {
    fn as_ref(&self) -> &str {
        &self.did
    }
}

impl From<String> for QuantumDID {
    fn from(did: String) -> Self {
        Self::new(did)
    }
}

impl From<&str> for QuantumDID {
    fn from(did: &str) -> Self {
        Self::new(did.to_string())
    }
}

/// DIDIdentity represents a user's identity on the network with blockchain-specific details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DIDIdentity {
    pub address: Address,
    pub owner: Address,
    pub claims_contract: Address,
    pub did_document: String,
}

/// IdentityProfile represents a specific profile within an Identity
/// It can be published to the network or used locally in the wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityProfile {
    pub name: String,
    pub did_identity: Option<DIDIdentity>, // Optional because profile may not be published yet
    pub key_pairs: Vec<KeyPair>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Identity is the main struct that represents a user's identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub did: String,                    // Main DID identifier
    pub username: String,               // User's chosen username
    pub master_password: String,        // Master password for the identity
    pub default_profile: bool,          // Whether this is the default identity
    pub profiles: Vec<IdentityProfile>, // Associated profiles
    pub authenticated: bool,            // Authentication status
    pub key_pairs: Vec<KeyPair>,        // Key pairs associated with main identity
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Identity {
    pub fn new(did: String, username: String, master_password: String) -> Self {
        Identity {
            did,
            username,
            master_password,
            authenticated: false,
            default_profile: false,
            profiles: vec![],
            key_pairs: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Generate a QuantumDID from this identity
    pub fn to_quantum_did(&self) -> QuantumDID {
        QuantumDID::new(self.did.clone())
    }

    /// Create an Identity from a QuantumDID
    pub fn from_quantum_did(
        quantum_did: QuantumDID,
        username: String,
        master_password: String,
    ) -> Self {
        Self::new(quantum_did.did, username, master_password)
    }

    // Add a new profile to this identity
    pub fn add_profile(&mut self, name: String) -> &IdentityProfile {
        let profile = IdentityProfile {
            name,
            did_identity: None,
            key_pairs: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.profiles.push(profile);
        self.updated_at = Utc::now();
        self.profiles.last().unwrap()
    }

    // Publish a profile to the network by adding DIDIdentity
    pub fn publish_profile(&mut self, profile_name: &str, did_identity: DIDIdentity) -> bool {
        if let Some(profile) = self.profiles.iter_mut().find(|p| p.name == profile_name) {
            profile.did_identity = Some(did_identity);
            profile.updated_at = Utc::now();
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    // Get a profile by name
    pub fn get_profile(&self, name: &str) -> Option<&IdentityProfile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    // Get a mutable profile by name
    pub fn get_profile_mut(&mut self, name: &str) -> Option<&mut IdentityProfile> {
        self.profiles.iter_mut().find(|p| p.name == name)
    }

    pub fn get_did(&self) -> String {
        self.did.clone()
    }

    pub fn get_username(&self) -> String {
        self.username.clone()
    }

    pub fn get_master_password(&self) -> String {
        self.master_password.clone()
    }

    pub fn get_authenticated(&self) -> bool {
        self.authenticated
    }

    pub fn get_profiles(&self) -> Vec<IdentityProfile> {
        self.profiles.clone()
    }

    pub fn get_default_profile(&self) -> bool {
        self.default_profile
    }

    pub fn set_default_profile(&mut self, default_profile: bool) {
        self.default_profile = default_profile;
        self.updated_at = Utc::now();
    }

    pub fn add_key_pair(
        &mut self,
        key_type: KeyType,
        public_key: String,
        private_key: String,
    ) -> KeyPair {
        let key_pair = KeyPair {
            key_id: Uuid::new_v4().to_string(), // Use UUID for unique key IDs
            key_type,
            public_key,
            private_key,
            created_at: Utc::now(),
            is_default: self.key_pairs.is_empty(), // First key becomes default
        };
        self.key_pairs.push(key_pair.clone());
        self.updated_at = Utc::now();
        key_pair
    }

    pub fn set_default_key(&mut self, key_id: &str) -> bool {
        if let Some(_index) = self.key_pairs.iter().position(|k| k.key_id == key_id) {
            for key_pair in &mut self.key_pairs {
                key_pair.is_default = key_pair.key_id == key_id;
            }
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    pub fn get_default_key(&self) -> Option<&KeyPair> {
        self.key_pairs.iter().find(|k| k.is_default)
    }

    pub fn remove_key(&mut self, key_id: &str) -> bool {
        if let Some(index) = self.key_pairs.iter().position(|k| k.key_id == key_id) {
            let removed_key = self.key_pairs.remove(index);
            if removed_key.is_default && !self.key_pairs.is_empty() {
                self.key_pairs[0].is_default = true;
            }
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    pub fn get_created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    pub fn get_updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    pub fn login(&mut self, username: String, password: String) -> bool {
        if self.username == username && self.master_password == password {
            self.authenticated = true;
            self.updated_at = Utc::now();
            true
        } else {
            false
        }
    }

    pub fn logout(&mut self) {
        self.authenticated = false;
        self.updated_at = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let identity = Identity::new(
            "did:swtchx:0xDF10698b56d85C754d201e8Ad79a6bCD3A6AA750".to_string(),
            "madmax".to_string(),
            "thunderdome".to_string(),
        );
        assert_eq!(
            identity.get_did(),
            "did:swtchx:0xDF10698b56d85C754d201e8Ad79a6bCD3A6AA750".to_string()
        );
        assert_eq!(identity.get_username(), "madmax".to_string());
        assert_eq!(identity.get_master_password(), "thunderdome".to_string());
        assert_eq!(identity.get_authenticated(), false);
        assert_eq!(identity.get_default_profile(), false);
    }

    #[test]
    fn test_login() {
        let mut identity = Identity::new(
            "did:swtchx:0xDF10698b56d85C754d201e8Ad79a6bCD3A6AA750".to_string(),
            "test".to_string(),
            "test".to_string(),
        );
        assert_eq!(identity.login("test".to_string(), "test".to_string()), true);
        assert_eq!(identity.get_authenticated(), true);
    }

    #[test]
    fn test_logout() {
        let mut identity = Identity::new(
            "did:swtchx:0xDF10698b56d85C754d201e8Ad79a6bCD3A6AA750".to_string(),
            "test".to_string(),
            "test".to_string(),
        );
        identity.login("test".to_string(), "test".to_string());
        identity.logout();
        assert_eq!(identity.get_authenticated(), false);
    }

    #[test]
    fn test_quantum_did_creation() {
        let did_string = "did:swtchx:0x1234567890abcdef";
        let quantum_did = QuantumDID::new(did_string.to_string());
        assert_eq!(quantum_did.as_str(), did_string);
        assert_eq!(quantum_did.to_string(), did_string);
    }

    #[test]
    fn test_quantum_did_parse() {
        let valid_did_x = "did:swtchx:0x1234567890abcdef";
        let valid_did = "did:swtch:0x1234567890abcdef";
        let invalid_did = "invalid:did:format";

        assert!(QuantumDID::parse(valid_did_x).is_ok());
        assert!(QuantumDID::parse(valid_did).is_ok());
        assert!(QuantumDID::parse(invalid_did).is_err());
    }

    #[test]
    fn test_quantum_did_from_address() {
        let address = Address::from([0u8; 20]);
        let quantum_did = QuantumDID::from_address(&address);
        assert!(quantum_did.as_str().starts_with("did:swtchx:"));
    }

    #[test]
    fn test_identity_quantum_did_integration() {
        let quantum_did = QuantumDID::new("did:swtchx:test123".to_string());
        let identity = Identity::from_quantum_did(
            quantum_did.clone(),
            "testuser".to_string(),
            "password".to_string(),
        );

        assert_eq!(identity.get_did(), quantum_did.as_str());
        assert_eq!(identity.to_quantum_did(), quantum_did);
    }

    #[test]
    fn test_quantum_did_hash_equality() {
        let did1 = QuantumDID::new("did:swtchx:test".to_string());
        let did2 = QuantumDID::new("did:swtchx:test".to_string());
        let did3 = QuantumDID::new("did:swtchx:different".to_string());

        assert_eq!(did1, did2);
        assert_ne!(did1, did3);

        // Test that they can be used in HashSet
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(did1.clone());
        assert!(set.contains(&did2));
        assert!(!set.contains(&did3));
    }
}

// Add implementation for IdentityProfile
impl IdentityProfile {
    pub fn new(name: String) -> Self {
        IdentityProfile {
            name,
            did_identity: None,
            key_pairs: vec![],
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    pub fn add_key_pair(
        &mut self,
        key_type: KeyType,
        public_key: String,
        private_key: String,
    ) -> KeyPair {
        let key_pair = KeyPair {
            key_id: Uuid::new_v4().to_string(),
            key_type,
            public_key,
            private_key,
            created_at: Utc::now(),
            is_default: self.key_pairs.is_empty(),
        };
        self.key_pairs.push(key_pair.clone());
        self.updated_at = Utc::now();
        key_pair
    }

    pub fn is_published(&self) -> bool {
        self.did_identity.is_some()
    }
}
