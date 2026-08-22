//! Dual-Key Wallet System
//!
//! Supports both Ethereum ECDSA keys (for MetaMask compatibility)
//! and quantum-safe keys (for SWTCH security).
//!
//! Users can:
//! - Use MetaMask with ECDSA keys
//! - Link ECDSA address to quantum-safe DID
//! - Transactions signed with ECDSA are internally converted to quantum-safe format

use crate::v1::identity::QuantumDID;
use alloy_primitives::Address;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

/// Dual-key wallet containing both ECDSA and quantum-safe keys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualKeyWallet {
    /// Quantum-safe DID (primary identity)
    pub did: QuantumDID,

    /// Quantum-safe public key (Kyber/Dilithium)
    pub quantum_public_key: Vec<u8>,

    /// Ethereum-compatible address (derived from ECDSA public key)
    pub ethereum_address: String, // 0x... format

    /// ECDSA public key (for MetaMask compatibility)
    pub ecdsa_public_key: Vec<u8>,

    /// Linkage proof (signature proving both keys belong to same owner)
    pub linkage_proof: Option<Vec<u8>>,
}

impl DualKeyWallet {
    /// Create a new dual-key wallet from both key types
    pub fn new(
        did: QuantumDID,
        quantum_public_key: Vec<u8>,
        ecdsa_public_key: Vec<u8>,
    ) -> Result<Self> {
        let ethereum_address = Self::public_key_to_address(&ecdsa_public_key)?;

        Ok(Self {
            did,
            quantum_public_key,
            ethereum_address,
            ecdsa_public_key,
            linkage_proof: None,
        })
    }

    /// Convert ECDSA public key to Ethereum address
    /// Ethereum address = last 20 bytes of Keccak256(public_key)
    pub fn public_key_to_address(public_key: &[u8]) -> Result<String> {
        // For uncompressed ECDSA public key (65 bytes), skip the first byte (0x04)
        let key_bytes = if public_key.len() == 65 && public_key[0] == 0x04 {
            &public_key[1..]
        } else {
            public_key
        };

        // Hash with Keccak256
        let mut hasher = Keccak256::new();
        hasher.update(key_bytes);
        let hash = hasher.finalize();

        // Take last 20 bytes and format as 0x...
        let address_bytes = &hash[12..];
        Ok(format!("0x{}", hex::encode(address_bytes)))
    }

    /// Generate linkage proof (signature of Ethereum address using quantum key)
    pub fn generate_linkage_proof(&mut self, quantum_private_key: &[u8]) -> Result<()> {
        // TODO: Sign ethereum_address with quantum private key
        // This proves ownership of both keys
        let _ = quantum_private_key;
        self.linkage_proof = Some(vec![0u8; 64]); // Placeholder
        Ok(())
    }

    /// Verify linkage proof
    pub fn verify_linkage_proof(&self) -> Result<bool> {
        // TODO: Verify signature
        Ok(self.linkage_proof.is_some())
    }
}

/// Address mapping for DID <-> Ethereum address lookups
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressMapping {
    pub did: String,
    pub ethereum_address: String,
    pub quantum_address: Vec<u8>,
    pub created_at: u64,
}

/// Registry for managing address mappings
pub struct DualKeyRegistry {
    // In production, this would be stored in swtch-storage-node
    mappings: std::collections::HashMap<String, AddressMapping>,
}

impl DualKeyRegistry {
    pub fn new() -> Self {
        Self {
            mappings: std::collections::HashMap::new(),
        }
    }

    /// Register a dual-key wallet
    pub fn register(&mut self, wallet: &DualKeyWallet) -> Result<()> {
        let mapping = AddressMapping {
            did: wallet.did.to_string(),
            ethereum_address: wallet.ethereum_address.clone(),
            quantum_address: wallet.quantum_public_key.clone(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs(),
        };

        // Index by both DID and Ethereum address
        self.mappings
            .insert(wallet.did.to_string(), mapping.clone());
        self.mappings
            .insert(wallet.ethereum_address.clone(), mapping);

        Ok(())
    }

    /// Lookup DID by Ethereum address
    pub fn get_did_by_ethereum_address(&self, eth_address: &str) -> Option<String> {
        self.mappings.get(eth_address).map(|m| m.did.clone())
    }

    /// Lookup Ethereum address by DID
    pub fn get_ethereum_address_by_did(&self, did: &str) -> Option<String> {
        self.mappings.get(did).map(|m| m.ethereum_address.clone())
    }
}

impl Default for DualKeyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_key_to_address() {
        // Example uncompressed ECDSA public key (65 bytes starting with 0x04)
        let public_key = vec![0x04; 65];
        let address = DualKeyWallet::public_key_to_address(&public_key).unwrap();
        assert!(address.starts_with("0x"));
        assert_eq!(address.len(), 42); // 0x + 40 hex chars
    }

    #[test]
    fn test_dual_key_registry() {
        let mut registry = DualKeyRegistry::new();

        let did = QuantumDID::from_address(&Address::from([0u8; 20]));
        let wallet = DualKeyWallet::new(did.clone(), vec![1, 2, 3], vec![0x04; 65]).unwrap();

        registry.register(&wallet).unwrap();

        // Lookup by Ethereum address
        let found_did = registry.get_did_by_ethereum_address(&wallet.ethereum_address);
        assert_eq!(found_did, Some(did.to_string()));

        // Lookup by DID
        let found_address = registry.get_ethereum_address_by_did(&did.to_string());
        assert_eq!(found_address, Some(wallet.ethereum_address));
    }
}
