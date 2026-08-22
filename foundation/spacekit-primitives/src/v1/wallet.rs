//! Quantum-Safe Wallet Implementation for SWTCH Network
//!
//! This module provides a comprehensive wallet system with:
//! - Quantum-resistant key generation (Kyber, SPHINCS+, Dilithium)
//! - DID integration
//! - Blockchain address derivation
//! - Mnemonic phrase support
//! - Password-based encryption

use crate::v1::crypto::EncryptionAlgorithm;
use crate::v1::identity::QuantumDID;
use crate::v1::keypair::{KeyPair, KeyType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Legacy wallet structure (kept for backwards compatibility)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub id: i32,
    pub identity_did: String,
    pub network_id: i32,
    pub public_key: String,
    pub private_key: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Wallet {
    pub fn new(identity_did: String, network_id: i32) -> Self {
        Wallet {
            id: 0,
            identity_did,
            network_id,
            public_key: String::new(),
            private_key: String::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}

/// Quantum-safe wallet with DID and blockchain integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumSafeWallet {
    /// Quantum DID
    pub did: QuantumDID,

    /// Blockchain address (20 bytes, Ethereum-style)
    pub address: [u8; 20],

    /// Quantum-resistant public key
    pub public_key: Vec<u8>,

    /// Encrypted private key (encrypted with user password)
    #[serde(skip_serializing)]
    pub encrypted_private_key: Vec<u8>,

    /// Encryption algorithm used (Kyber1024, Kyber768, NTRUPrime, etc.)
    pub encryption_algorithm: EncryptionAlgorithm,

    /// Key derivation salt
    pub salt: [u8; 32],

    /// Key type used (Kyber1024, Kyber768, etc.)
    pub key_type: KeyType,

    /// Optional mnemonic phrase (BIP39)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mnemonic_encrypted: Option<Vec<u8>>,

    /// Wallet version for future upgrades
    pub version: u8,

    /// Wallet creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,
}

impl QuantumSafeWallet {
    /// Create a new quantum-safe wallet
    pub fn new(
        did: QuantumDID,
        address: [u8; 20],
        public_key: Vec<u8>,
        encrypted_private_key: Vec<u8>,
        encryption_algorithm: EncryptionAlgorithm,
        salt: [u8; 32],
        key_type: KeyType,
    ) -> Self {
        Self {
            did,
            address,
            public_key,
            encrypted_private_key,
            encryption_algorithm,
            salt,
            key_type,
            mnemonic_encrypted: None,
            version: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Get the wallet's DID as a string
    pub fn did_string(&self) -> &str {
        self.did.as_str()
    }

    /// Get the wallet's address as hex string
    pub fn address_hex(&self) -> String {
        format!("0x{}", hex::encode(self.address))
    }

    /// Get the public key as hex string
    pub fn public_key_hex(&self) -> String {
        hex::encode(&self.public_key)
    }

    /// Check if wallet has a mnemonic
    pub fn has_mnemonic(&self) -> bool {
        self.mnemonic_encrypted.is_some()
    }

    /// Set encrypted mnemonic phrase
    pub fn set_mnemonic(&mut self, encrypted_mnemonic: Vec<u8>) {
        self.mnemonic_encrypted = Some(encrypted_mnemonic);
        self.updated_at = Utc::now();
    }

    /// Convert to KeyPair format
    pub fn to_keypair(&self) -> KeyPair {
        KeyPair {
            key_id: self.did.as_str().to_string(),
            key_type: self.key_type.clone(),
            public_key: self.public_key_hex(),
            private_key: hex::encode(&self.encrypted_private_key),
            created_at: self.created_at,
            is_default: false,
        }
    }
}

/// Wallet creation result with backup information
#[derive(Debug, Clone)]
pub struct WalletCreationResult {
    pub wallet: QuantumSafeWallet,
    pub private_key_hex: String,
    pub mnemonic: Option<String>,
}

/// Wallet display information (safe for UI/API)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletInfo {
    pub did: String,
    pub address: String,
    pub key_type: KeyType,
    pub has_mnemonic: bool,
    pub version: u8,
    pub created_at: DateTime<Utc>,
}

impl From<&QuantumSafeWallet> for WalletInfo {
    fn from(wallet: &QuantumSafeWallet) -> Self {
        Self {
            did: wallet.did.as_str().to_string(),
            address: wallet.address_hex(),
            key_type: wallet.key_type.clone(),
            has_mnemonic: wallet.has_mnemonic(),
            version: wallet.version,
            created_at: wallet.created_at,
        }
    }
}

/// Wallet manager for creating and managing quantum-safe wallets
#[derive(Debug, Clone)]
pub struct WalletManager {
    /// Network ID (1 = mainnet, 1337 = testnet, etc.)
    pub network_id: u32,
}

impl WalletManager {
    /// Create a new wallet manager
    pub fn new(network_id: u32) -> Self {
        Self { network_id }
    }

    /// Generate a salt for key derivation
    pub fn generate_salt(&self) -> [u8; 32] {
        use rand::Rng;
        let mut salt = [0u8; 32];
        rand::thread_rng().fill(&mut salt);
        salt
    }

    /// Derive blockchain address from public key (Ethereum-style with Keccak256)
    pub fn derive_address(&self, public_key: &[u8]) -> [u8; 20] {
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        hasher.update(public_key);
        let hash = hasher.finalize();

        let mut address = [0u8; 20];
        address.copy_from_slice(&hash[12..32]);
        address
    }

    /// Encrypt data with password using PBKDF2 + XOR
    pub fn encrypt_with_password(
        &self,
        data: &[u8],
        password: &str,
        salt: &[u8; 32],
    ) -> Result<Vec<u8>, String> {
        use pbkdf2::pbkdf2_hmac;
        use sha2::Sha256;

        let mut derived_key = vec![0u8; data.len()];
        pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, 100_000, &mut derived_key);

        let mut encrypted = data.to_vec();
        for (i, byte) in encrypted.iter_mut().enumerate() {
            *byte ^= derived_key[i % derived_key.len()];
        }

        Ok(encrypted)
    }

    /// Decrypt data with password
    pub fn decrypt_with_password(
        &self,
        encrypted: &[u8],
        password: &str,
        salt: &[u8; 32],
    ) -> Result<Vec<u8>, String> {
        // XOR encryption is symmetric
        self.encrypt_with_password(encrypted, password, salt)
    }
}

impl Default for WalletManager {
    fn default() -> Self {
        Self::new(1337) // Testnet by default
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_creation() {
        let did = QuantumDID::new("did:swtch:user:test".to_string());
        let address = [1u8; 20];
        let public_key = vec![2u8; 32];
        let encrypted_key = vec![3u8; 64];
        let salt = [4u8; 32];

        let wallet = QuantumSafeWallet::new(
            did,
            address,
            public_key,
            encrypted_key,
            EncryptionAlgorithm::Kyber1024,
            salt,
            KeyType::Kyber1024,
        );

        assert_eq!(wallet.did_string(), "did:swtch:user:test");
        assert_eq!(wallet.address.len(), 20);
        assert_eq!(wallet.version, 1);
    }

    #[test]
    fn test_wallet_manager_address_derivation() {
        let manager = WalletManager::new(1337);
        let public_key = b"test_public_key_data_here";
        let address = manager.derive_address(public_key);

        assert_eq!(address.len(), 20);
    }

    #[test]
    fn test_encryption_decryption() {
        let manager = WalletManager::new(1337);
        let data = b"sensitive_private_key_data";
        let password = "secure_password_123";
        let salt = manager.generate_salt();

        let encrypted = manager
            .encrypt_with_password(data, password, &salt)
            .unwrap();
        let decrypted = manager
            .decrypt_with_password(&encrypted, password, &salt)
            .unwrap();

        assert_eq!(data.to_vec(), decrypted);
    }
}
