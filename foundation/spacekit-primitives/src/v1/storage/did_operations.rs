//! Storage-Specific DID Operations for SWTCH Network
//!
//! This module provides DID operations specifically designed for storage contracts,
//! including quantum-safe storage verification and identity-based access control.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Storage DID Operations
///
/// This struct provides DID operations specifically for storage contracts,
/// including identity verification and access control.
pub struct StorageDidOperations {
    pub verified_dids: HashMap<String, VerifiedStorageDid>,
    pub storage_credentials: HashMap<String, StorageCredential>,
}

/// Verified Storage DID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedStorageDid {
    pub did: String,
    pub public_key: Vec<u8>,
    pub storage_capacity: u64,
    pub reputation_score: f64,
    pub quantum_safe: bool,
    pub verified_at: u64,
    pub expires_at: Option<u64>,
}

/// Storage Credential
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCredential {
    pub id: String,
    pub holder_did: String,
    pub issuer_did: String,
    pub credential_type: StorageCredentialType,
    pub claims: StorageCredentialClaims,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
    pub quantum_signature: Vec<u8>,
}

/// Storage Credential Type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageCredentialType {
    StorageProvider,
    StorageConsumer,
    StorageAdmin,
    ResearchDataAccess,
    MedicalRecordAccess,
}

/// Storage Credential Claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageCredentialClaims {
    pub storage_capacity: Option<u64>,
    pub max_file_size: Option<u64>,
    pub allowed_algorithms: Vec<String>,
    pub access_levels: Vec<String>,
    pub special_permissions: Vec<String>,
}

/// Storage Access Token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageAccessToken {
    pub token_id: String,
    pub file_id: String,
    pub holder_did: String,
    pub permissions: StorageTokenPermissions,
    pub issued_at: u64,
    pub expires_at: u64,
    pub quantum_signature: Vec<u8>,
}

/// Storage Token Permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageTokenPermissions {
    pub read: bool,
    pub write: bool,
    pub delete: bool,
    pub share: bool,
    pub admin: bool,
}

impl StorageDidOperations {
    /// Create new storage DID operations manager
    pub fn new() -> Self {
        Self {
            verified_dids: HashMap::new(),
            storage_credentials: HashMap::new(),
        }
    }

    /// Verify a DID for storage operations
    pub async fn verify_storage_did(
        &mut self,
        did: &str,
        public_key: Vec<u8>,
        storage_capacity: u64,
        reputation_score: f64,
    ) -> Result<VerifiedStorageDid> {
        // In production, this would verify the DID using quantum-safe cryptography
        let verified_did = VerifiedStorageDid {
            did: did.to_string(),
            public_key,
            storage_capacity,
            reputation_score,
            quantum_safe: true,
            verified_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            expires_at: None,
        };

        self.verified_dids
            .insert(did.to_string(), verified_did.clone());
        Ok(verified_did)
    }

    /// Issue a storage credential
    pub async fn issue_storage_credential(
        &mut self,
        holder_did: &str,
        issuer_did: &str,
        credential_type: StorageCredentialType,
        claims: StorageCredentialClaims,
        expires_in_seconds: Option<u64>,
    ) -> Result<StorageCredential> {
        let credential_id = format!("storage_cred_{}", uuid::Uuid::new_v4());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let credential = StorageCredential {
            id: credential_id.clone(),
            holder_did: holder_did.to_string(),
            issuer_did: issuer_did.to_string(),
            credential_type,
            claims,
            issued_at: now,
            expires_at: expires_in_seconds.map(|exp| now + exp),
            quantum_signature: vec![0u8; 64], // Placeholder for quantum signature
        };

        self.storage_credentials
            .insert(credential_id, credential.clone());
        Ok(credential)
    }

    /// Verify a storage credential
    pub async fn verify_storage_credential(&self, credential: &StorageCredential) -> Result<bool> {
        // Check if credential is expired
        if let Some(expires_at) = credential.expires_at {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            if now > expires_at {
                return Ok(false);
            }
        }

        // Check if holder DID is verified
        if !self.verified_dids.contains_key(&credential.holder_did) {
            return Ok(false);
        }

        // In production, verify quantum signature
        Ok(true)
    }

    /// Create a storage access token
    pub async fn create_storage_access_token(
        &self,
        file_id: &str,
        holder_did: &str,
        permissions: StorageTokenPermissions,
        expires_in_seconds: u64,
    ) -> Result<StorageAccessToken> {
        let token_id = format!("storage_token_{}", uuid::Uuid::new_v4());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let token = StorageAccessToken {
            token_id,
            file_id: file_id.to_string(),
            holder_did: holder_did.to_string(),
            permissions,
            issued_at: now,
            expires_at: now + expires_in_seconds,
            quantum_signature: vec![0u8; 64], // Placeholder for quantum signature
        };

        Ok(token)
    }

    /// Verify a storage access token
    pub async fn verify_storage_access_token(&self, token: &StorageAccessToken) -> Result<bool> {
        // Check if token is expired
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if now > token.expires_at {
            return Ok(false);
        }

        // Check if holder DID is verified
        if !self.verified_dids.contains_key(&token.holder_did) {
            return Ok(false);
        }

        // In production, verify quantum signature
        Ok(true)
    }

    /// Get storage capacity for a DID
    pub fn get_storage_capacity(&self, did: &str) -> Option<u64> {
        self.verified_dids.get(did).map(|v| v.storage_capacity)
    }

    /// Get reputation score for a DID
    pub fn get_reputation_score(&self, did: &str) -> Option<f64> {
        self.verified_dids.get(did).map(|v| v.reputation_score)
    }

    /// Update reputation score for a DID
    pub fn update_reputation_score(&mut self, did: &str, new_score: f64) -> Result<()> {
        if let Some(verified_did) = self.verified_dids.get_mut(did) {
            verified_did.reputation_score = new_score;
            Ok(())
        } else {
            Err(anyhow::anyhow!("DID not found: {}", did))
        }
    }

    /// Check if DID has specific storage credential
    pub fn has_storage_credential(
        &self,
        did: &str,
        credential_type: &StorageCredentialType,
    ) -> bool {
        self.storage_credentials.values().any(|cred| {
            cred.holder_did == did
                && std::mem::discriminant(&cred.credential_type)
                    == std::mem::discriminant(credential_type)
                && cred.expires_at.map_or(true, |exp| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                        <= exp
                })
        })
    }

    /// Get all storage credentials for a DID
    pub fn get_storage_credentials(&self, did: &str) -> Vec<&StorageCredential> {
        self.storage_credentials
            .values()
            .filter(|cred| cred.holder_did == did)
            .collect()
    }
}

impl Default for StorageDidOperations {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for StorageTokenPermissions {
    fn default() -> Self {
        Self {
            read: true,
            write: false,
            delete: false,
            share: false,
            admin: false,
        }
    }
}

impl Default for StorageCredentialClaims {
    fn default() -> Self {
        Self {
            storage_capacity: None,
            max_file_size: None,
            allowed_algorithms: vec!["kyber1024".to_string()],
            access_levels: vec!["basic".to_string()],
            special_permissions: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_storage_did_verification() {
        let mut operations = StorageDidOperations::new();
        let did = "did:swtch:storage:test";
        let public_key = vec![1, 2, 3, 4];
        let storage_capacity = 1024 * 1024 * 1024; // 1GB
        let reputation_score = 0.8;

        let verified = operations
            .verify_storage_did(did, public_key.clone(), storage_capacity, reputation_score)
            .await
            .unwrap();

        assert_eq!(verified.did, did);
        assert_eq!(verified.public_key, public_key);
        assert_eq!(verified.storage_capacity, storage_capacity);
        assert_eq!(verified.reputation_score, reputation_score);
        assert!(verified.quantum_safe);
    }

    #[tokio::test]
    async fn test_storage_credential_issuance() {
        let mut operations = StorageDidOperations::new();
        let holder_did = "did:swtch:holder";
        let issuer_did = "did:swtch:issuer";

        // First verify the holder DID
        operations
            .verify_storage_did(holder_did, vec![1, 2, 3, 4], 1024 * 1024 * 1024, 0.8)
            .await
            .unwrap();

        let credential = operations
            .issue_storage_credential(
                holder_did,
                issuer_did,
                StorageCredentialType::StorageProvider,
                StorageCredentialClaims::default(),
                Some(3600), // 1 hour
            )
            .await
            .unwrap();

        assert_eq!(credential.holder_did, holder_did);
        assert_eq!(credential.issuer_did, issuer_did);
        assert!(matches!(
            credential.credential_type,
            StorageCredentialType::StorageProvider
        ));

        // Verify the credential
        let is_valid = operations
            .verify_storage_credential(&credential)
            .await
            .unwrap();
        assert!(is_valid);
    }

    #[tokio::test]
    async fn test_storage_access_token() {
        let mut operations = StorageDidOperations::new();
        let holder_did = "did:swtch:holder";
        let file_id = "test_file_123";

        // First verify the holder DID
        operations
            .verify_storage_did(holder_did, vec![1, 2, 3, 4], 1024 * 1024 * 1024, 0.8)
            .await
            .unwrap();

        let permissions = StorageTokenPermissions {
            read: true,
            write: true,
            delete: false,
            share: false,
            admin: false,
        };

        let token = operations
            .create_storage_access_token(
                file_id,
                holder_did,
                permissions,
                3600, // 1 hour
            )
            .await
            .unwrap();

        assert_eq!(token.file_id, file_id);
        assert_eq!(token.holder_did, holder_did);
        assert!(token.permissions.read);
        assert!(token.permissions.write);
        assert!(!token.permissions.delete);

        // Verify the token
        let is_valid = operations
            .verify_storage_access_token(&token)
            .await
            .unwrap();
        assert!(is_valid);
    }

    #[test]
    fn test_reputation_management() {
        let mut operations = StorageDidOperations::new();
        let did = "did:swtch:test";

        // Initially no reputation
        assert_eq!(operations.get_reputation_score(did), None);

        // Add verified DID
        let verified_did = VerifiedStorageDid {
            did: did.to_string(),
            public_key: vec![1, 2, 3, 4],
            storage_capacity: 1024 * 1024 * 1024,
            reputation_score: 0.5,
            quantum_safe: true,
            verified_at: 0,
            expires_at: None,
        };
        operations
            .verified_dids
            .insert(did.to_string(), verified_did);

        // Now has reputation
        assert_eq!(operations.get_reputation_score(did), Some(0.5));

        // Update reputation
        operations.update_reputation_score(did, 0.8).unwrap();
        assert_eq!(operations.get_reputation_score(did), Some(0.8));
    }

    #[tokio::test]
    async fn test_credential_type_checking() {
        let mut operations = StorageDidOperations::new();
        let holder_did = "did:swtch:holder";
        let issuer_did = "did:swtch:issuer";

        // First verify the holder DID
        operations
            .verify_storage_did(holder_did, vec![1, 2, 3, 4], 1024 * 1024 * 1024, 0.8)
            .await
            .unwrap();

        // Issue a storage provider credential
        operations
            .issue_storage_credential(
                holder_did,
                issuer_did,
                StorageCredentialType::StorageProvider,
                StorageCredentialClaims::default(),
                Some(3600),
            )
            .await
            .unwrap();

        // Check credential types
        assert!(
            operations.has_storage_credential(holder_did, &StorageCredentialType::StorageProvider)
        );
        assert!(
            !operations.has_storage_credential(holder_did, &StorageCredentialType::StorageConsumer)
        );
        assert!(
            !operations.has_storage_credential(holder_did, &StorageCredentialType::StorageAdmin)
        );

        // Get all credentials
        let credentials = operations.get_storage_credentials(holder_did);
        assert_eq!(credentials.len(), 1);
        assert!(matches!(
            credentials[0].credential_type,
            StorageCredentialType::StorageProvider
        ));
    }
}
