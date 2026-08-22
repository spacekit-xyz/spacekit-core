use chrono::{DateTime, Utc};
use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;

pub use crate::sphincs::{QuantumKeyPair, SphincsPlus};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecentralizedIdentifier {
    pub did: String,
    pub method: String,
    pub identifier: String,
}

impl fmt::Display for DecentralizedIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.did)
    }
}

impl AsRef<str> for DecentralizedIdentifier {
    fn as_ref(&self) -> &str {
        &self.did
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableCredential {
    pub id: String,
    pub issuer: String,
    pub subject: String,
    pub credential_type: String,
    pub claims: HashMap<String, String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityDocument {
    pub did: DecentralizedIdentifier,
    pub public_keys: Vec<QuantumKeyPair>,
    pub authentication: Vec<String>,
    pub assertion_method: Vec<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

#[derive(Debug)]
pub struct QuantumResistantWallet {
    pub address: String,
    pub key_pairs: Vec<QuantumKeyPair>,
    pub identity_doc: IdentityDocument,
    pub credentials: Vec<VerifiableCredential>,
}

impl Default for QuantumResistantWallet {
    fn default() -> Self {
        Self::new()
    }
}

impl QuantumResistantWallet {
    pub fn new() -> Self {
        let key_pair = SphincsPlus::generate_keypair();
        let address = Self::derive_address(&key_pair.public_key);
        let did = DecentralizedIdentifier {
            did: format!("did:spacekit:testnet:{}", &address[0..16]),
            method: "spacekit:testnet".to_string(),
            identifier: address.clone(),
        };

        let identity_doc = IdentityDocument {
            did: did.clone(),
            public_keys: vec![key_pair.clone()],
            authentication: vec![format!("{}#key-1", did.did)],
            assertion_method: vec![format!("{}#key-1", did.did)],
            created: Utc::now(),
            updated: Utc::now(),
        };

        Self {
            address,
            key_pairs: vec![key_pair],
            identity_doc,
            credentials: Vec::new(),
        }
    }

    fn derive_address(public_key: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(public_key);
        let hash = hasher.finalize();
        format!("0x{}", hex::encode(&hash[0..20]))
    }

    pub fn sign_content(&self, content: &str) -> Result<String, &'static str> {
        if self.key_pairs.is_empty() {
            return Err("No key pairs available");
        }

        let message = content.as_bytes();
        let signature = SphincsPlus::sign(&self.key_pairs[0].private_key, message)
            .map_err(|_| "Invalid private key")?;
        Ok(hex::encode(signature))
    }

    pub fn verify_content(&self, content: &str, signature_hex: &str) -> Result<bool, &'static str> {
        if self.key_pairs.is_empty() {
            return Err("No key pairs available");
        }

        let signature = hex::decode(signature_hex).map_err(|_| "Invalid signature format")?;
        let message = content.as_bytes();
        let is_valid = SphincsPlus::verify(&self.key_pairs[0].public_key, message, &signature);
        Ok(is_valid)
    }

    pub fn issue_credential(
        &self,
        subject_did: &str,
        credential_type: &str,
        claims: HashMap<String, String>,
        expires_in_days: Option<i64>,
    ) -> Result<VerifiableCredential, &'static str> {
        let id = format!("vc:{}", uuid::Uuid::new_v4());
        let expires_at = expires_in_days.map(|days| Utc::now() + chrono::Duration::days(days));

        let credential = VerifiableCredential {
            id: id.clone(),
            issuer: self.identity_doc.did.did.clone(),
            subject: subject_did.to_string(),
            credential_type: credential_type.to_string(),
            claims,
            issued_at: Utc::now(),
            expires_at,
            signature: String::new(), // Will be filled after signing
        };

        // Create the content to sign
        let content = serde_json::to_string(&credential).map_err(|_| "Serialization error")?;
        let signature = self.sign_content(&content)?;

        let mut signed_credential = credential;
        signed_credential.signature = signature;

        Ok(signed_credential)
    }

    pub fn verify_credential(
        &self,
        credential: &VerifiableCredential,
    ) -> Result<bool, &'static str> {
        // Create a copy without signature for verification
        let mut credential_copy = credential.clone();
        credential_copy.signature = String::new();

        let content = serde_json::to_string(&credential_copy).map_err(|_| "Serialization error")?;

        // In a real implementation, you would look up the issuer's public key
        self.verify_content(&content, &credential.signature)
    }

    pub fn add_credential(&mut self, credential: VerifiableCredential) {
        self.credentials.push(credential);
    }

    pub fn get_credentials_by_type(&self, credential_type: &str) -> Vec<&VerifiableCredential> {
        self.credentials
            .iter()
            .filter(|cred| cred.credential_type == credential_type)
            .collect()
    }

    /// Apply a DID string from external configuration (e.g. SpaceKit CLI `config.toml`).
    ///
    /// The in-wallet SPHINCS+ signing keys are unchanged; only the DID document metadata is updated.
    /// Callers should load CLI KEM material separately when Kyber encryption parity is required.
    pub fn apply_config_did(&mut self, did_str: &str) -> Result<(), &'static str> {
        let rest = did_str
            .strip_prefix("did:")
            .ok_or("DID must start with did:")?;
        let (method, identifier) = rest
            .split_once(':')
            .ok_or("DID must include method and identifier (e.g. did:spacekit:user:...)")?;
        let did = DecentralizedIdentifier {
            did: did_str.to_string(),
            method: method.to_string(),
            identifier: identifier.to_string(),
        };
        self.identity_doc.did = did.clone();
        self.identity_doc.authentication = vec![format!("{}#key-1", did.did)];
        self.identity_doc.assertion_method = vec![format!("{}#key-1", did.did)];
        self.identity_doc.updated = Utc::now();
        Ok(())
    }

    pub fn rotate_keys(&mut self) -> Result<(), &'static str> {
        let new_key_pair = SphincsPlus::generate_keypair();
        self.key_pairs.insert(0, new_key_pair);
        self.identity_doc.updated = Utc::now();

        // Keep only the latest 3 key pairs for practical purposes
        if self.key_pairs.len() > 3 {
            self.key_pairs.truncate(3);
        }

        Ok(())
    }

    pub fn export_identity_document(&self) -> Result<String, &'static str> {
        serde_json::to_string_pretty(&self.identity_doc).map_err(|_| "Serialization error")
    }

    pub fn present_proof(&self, credential_types: &[&str]) -> Vec<&VerifiableCredential> {
        self.credentials
            .iter()
            .filter(|cred| credential_types.contains(&cred.credential_type.as_str()))
            .filter(|cred| {
                // Check if credential is still valid
                match cred.expires_at {
                    Some(expiry) => expiry > Utc::now(),
                    None => true,
                }
            })
            .collect()
    }
}

impl fmt::Display for QuantumResistantWallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Quantum-Resistant Wallet\nDID: {}\nAddress: {}\nKeys: {}\nCredentials: {}",
            self.identity_doc.did.did,
            self.address,
            self.key_pairs.len(),
            self.credentials.len()
        )
    }
}

// Example usage and tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_config_did_updates_document() {
        let mut wallet = QuantumResistantWallet::new();
        let before = wallet.identity_doc.did.did.clone();
        wallet
            .apply_config_did("did:spacekit:user:test-uuid")
            .expect("valid DID");
        assert_eq!(wallet.identity_doc.did.did, "did:spacekit:user:test-uuid");
        assert_eq!(wallet.identity_doc.did.method, "spacekit");
        assert_eq!(wallet.identity_doc.did.identifier, "user:test-uuid");
        assert_ne!(before, wallet.identity_doc.did.did);
    }

    #[test]
    fn test_wallet_creation() {
        let wallet = QuantumResistantWallet::new();
        assert!(!wallet.address.is_empty());
        assert_eq!(wallet.key_pairs.len(), 1);
        assert!(wallet
            .identity_doc
            .did
            .as_ref()
            .starts_with("did:spacekit:testnet:"));
    }

    #[test]
    fn test_sign_and_verify() {
        let wallet = QuantumResistantWallet::new();
        let content = "Hello, quantum-resistant world!";

        let signature = wallet.sign_content(content).unwrap();
        let is_valid = wallet.verify_content(content, &signature).unwrap();

        assert!(is_valid);

        // Test with wrong content
        let is_invalid = wallet
            .verify_content("Different content", &signature)
            .unwrap();
        assert!(!is_invalid);
    }

    #[test]
    fn test_credential_lifecycle() {
        let mut issuer = QuantumResistantWallet::new();
        let subject = QuantumResistantWallet::new();

        let mut claims = HashMap::new();
        claims.insert("name".to_string(), "Alice Smith".to_string());
        claims.insert("degree".to_string(), "Computer Science".to_string());

        let credential = issuer
            .issue_credential(
                &subject.identity_doc.did.did,
                "EducationCredential",
                claims,
                Some(365), // 1 year validity
            )
            .unwrap();

        let is_valid = issuer.verify_credential(&credential).unwrap();
        assert!(is_valid);

        issuer.add_credential(credential);
        assert_eq!(issuer.credentials.len(), 1);
    }

    #[test]
    fn test_key_rotation() {
        let mut wallet = QuantumResistantWallet::new();
        assert_eq!(wallet.key_pairs.len(), 1);

        let original_key = wallet.key_pairs[0].public_key.clone();

        wallet.rotate_keys().unwrap();
        assert_eq!(wallet.key_pairs.len(), 2);
        assert_ne!(wallet.key_pairs[0].public_key, original_key);

        // Test signing with new key still works
        let content = "Test after rotation";
        let signature = wallet.sign_content(content).unwrap();
        let is_valid = wallet.verify_content(content, &signature).unwrap();
        assert!(is_valid);
    }

    #[test]
    fn test_key_rotation_limit() {
        let mut wallet = QuantumResistantWallet::new();

        // Rotate keys 5 times, should only keep latest 3
        for _ in 0..5 {
            wallet.rotate_keys().unwrap();
        }

        assert_eq!(wallet.key_pairs.len(), 3);
    }

    #[test]
    fn test_address_derivation_consistency() {
        let wallet1 = QuantumResistantWallet::new();
        let wallet2 = QuantumResistantWallet::new();

        // Different wallets should have different addresses
        assert_ne!(wallet1.address, wallet2.address);

        // Same public key should always give same address
        let addr1 = QuantumResistantWallet::derive_address(&wallet1.key_pairs[0].public_key);
        let addr2 = QuantumResistantWallet::derive_address(&wallet1.key_pairs[0].public_key);
        assert_eq!(addr1, addr2);
        assert_eq!(addr1, wallet1.address);
    }

    #[test]
    fn test_cross_wallet_verification() {
        let wallet1 = QuantumResistantWallet::new();
        let wallet2 = QuantumResistantWallet::new();

        let content = "Cross-wallet test message";
        let signature = wallet1.sign_content(content).unwrap();

        // wallet2 should be able to verify wallet1's signature using wallet1's public key
        let is_valid = SphincsPlus::verify(
            &wallet1.key_pairs[0].public_key,
            content.as_bytes(),
            &hex::decode(&signature).unwrap(),
        );
        assert!(is_valid);

        // But wallet1's signature should not verify with wallet2's key
        let is_invalid = SphincsPlus::verify(
            &wallet2.key_pairs[0].public_key,
            content.as_bytes(),
            &hex::decode(&signature).unwrap(),
        );
        assert!(!is_invalid);
    }

    #[test]
    fn test_credential_management() {
        let mut wallet = QuantumResistantWallet::new();
        let subject_did = "did:spacekit:testnet:test123";

        // Create multiple credentials of different types
        let mut edu_claims = HashMap::new();
        edu_claims.insert("degree".to_string(), "PhD".to_string());

        let mut id_claims = HashMap::new();
        id_claims.insert("name".to_string(), "Alice".to_string());

        let edu_cred = wallet
            .issue_credential(subject_did, "EducationCredential", edu_claims, Some(365))
            .unwrap();
        let id_cred = wallet
            .issue_credential(subject_did, "IdentityCredential", id_claims, None)
            .unwrap();

        wallet.add_credential(edu_cred);
        wallet.add_credential(id_cred);

        // Test filtering by type
        let edu_creds = wallet.get_credentials_by_type("EducationCredential");
        assert_eq!(edu_creds.len(), 1);

        let id_creds = wallet.get_credentials_by_type("IdentityCredential");
        assert_eq!(id_creds.len(), 1);

        let nonexistent = wallet.get_credentials_by_type("NonexistentType");
        assert_eq!(nonexistent.len(), 0);
    }

    #[test]
    fn test_proof_presentation() {
        let mut wallet = QuantumResistantWallet::new();
        let subject_did = "did:spacekit:testnet:test123";

        // Create credentials
        let mut edu_claims = HashMap::new();
        edu_claims.insert("degree".to_string(), "PhD".to_string());

        let mut work_claims = HashMap::new();
        work_claims.insert("company".to_string(), "TechCorp".to_string());

        let edu_cred = wallet
            .issue_credential(subject_did, "EducationCredential", edu_claims, Some(365))
            .unwrap();
        let work_cred = wallet
            .issue_credential(subject_did, "WorkCredential", work_claims, Some(30))
            .unwrap();

        wallet.add_credential(edu_cred);
        wallet.add_credential(work_cred);

        // Test proof presentation
        let proof = wallet.present_proof(&["EducationCredential", "WorkCredential"]);
        assert_eq!(proof.len(), 2);

        let edu_proof = wallet.present_proof(&["EducationCredential"]);
        assert_eq!(edu_proof.len(), 1);

        let empty_proof = wallet.present_proof(&["NonexistentType"]);
        assert_eq!(empty_proof.len(), 0);
    }

    #[test]
    fn test_identity_document_export() {
        let wallet = QuantumResistantWallet::new();

        let doc_json = wallet.export_identity_document().unwrap();
        assert!(doc_json.contains("did:spacekit:testnet:"));
        assert!(doc_json.contains("public_keys"));
        assert!(doc_json.contains("authentication"));

        // Verify it's valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&doc_json).unwrap();
        assert!(parsed["did"]["did"]
            .as_str()
            .unwrap()
            .starts_with("did:spacekit:testnet:"));
    }

    #[test]
    fn test_invalid_signature_verification() {
        let wallet = QuantumResistantWallet::new();
        let content = "Test message";

        // Test with completely invalid signature
        let is_valid = wallet.verify_content(content, "invalid_hex_signature");
        assert!(is_valid.is_err());

        // Test with valid hex but wrong signature
        let wrong_signature = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let is_valid = wallet.verify_content(content, wrong_signature).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn test_error_handling() {
        let wallet = QuantumResistantWallet::new();

        // Test credential verification with tampered content
        let subject_did = "did:spacekit:testnet:test123";
        let mut claims = HashMap::new();
        claims.insert("name".to_string(), "Alice".to_string());

        let mut credential = wallet
            .issue_credential(subject_did, "TestCredential", claims, Some(365))
            .unwrap();

        // Tamper with the credential
        credential
            .claims
            .insert("name".to_string(), "Bob".to_string());

        let is_valid = wallet.verify_credential(&credential).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn test_sphincs_key_generation_deterministic() {
        // Test that different key generations produce different keys
        let keypair1 = SphincsPlus::generate_keypair();
        let keypair2 = SphincsPlus::generate_keypair();

        assert_ne!(keypair1.public_key, keypair2.public_key);
        assert_ne!(keypair1.private_key, keypair2.private_key);
        assert_eq!(keypair1.algorithm, "SPHINCS+-SHAKE-256-128s-simple");
        assert_eq!(keypair2.algorithm, "SPHINCS+-SHAKE-256-128s-simple");
    }
}
