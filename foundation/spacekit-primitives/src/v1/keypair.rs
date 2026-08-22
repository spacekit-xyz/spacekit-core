use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum KeyType {
    Ed25519,
    Secp256k1,
    X25519,
    // Quantum-resistant key types
    Kyber512,
    Kyber768,
    Kyber1024,
    Dilithium2,
    Dilithium3,
    Dilithium5,
    SPHINCS,
    // Add more key types as needed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyPair {
    pub key_id: String,      // Unique identifier for the key pair
    pub key_type: KeyType,   // Type of the key pair
    pub public_key: String,  // Base58 or Hex encoded public key
    pub private_key: String, // Encrypted private key
    pub created_at: DateTime<Utc>,
    pub is_default: bool, // Whether this is the default key pair
}

impl Default for KeyPair {
    fn default() -> Self {
        KeyPair {
            key_id: "".to_string(),
            key_type: KeyType::Ed25519,
            public_key: "".to_string(),
            private_key: "".to_string(),
            created_at: Utc::now(),
            is_default: false,
        }
    }
}

impl KeyPair {
    pub fn new(key_id: String, key_type: KeyType, public_key: String, private_key: String) -> Self {
        KeyPair {
            key_id,
            key_type,
            public_key,
            private_key,
            created_at: Utc::now(),
            is_default: false,
        }
    }
}
