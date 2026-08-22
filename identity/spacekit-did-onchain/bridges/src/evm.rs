use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spacekit_did::{QuantumResistantWallet, SphincsPlus, VerifiableCredential};

/// EVM integration utilities for quantum DID management
pub mod evm_integration {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct EVMQuantumDID {
        pub ethereum_address: String, // Traditional ETH address for transactions
        pub quantum_did: String,      // Quantum DID string
        pub quantum_public_key: String, // Hex-encoded quantum public key
        pub did_document: String,     // JSON DID document
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct QuantumCredentialProof {
        pub credential_hash: String,
        pub issuer_did: String,
        pub quantum_signature: String,
        pub verification_message: String,
    }

    pub struct EVMQuantumBridge {
        pub wallet: QuantumResistantWallet,
    }

    impl Default for EVMQuantumBridge {
        fn default() -> Self {
            Self::new()
        }
    }

    impl EVMQuantumBridge {
        pub fn new() -> Self {
            Self {
                wallet: QuantumResistantWallet::new(),
            }
        }

        /// Generate registration data for Solidity contract
        pub fn generate_registration_data(
            &self,
            ethereum_address: &str,
        ) -> Result<(String, String, String), Box<dyn std::error::Error>> {
            let did = self.wallet.identity_doc.did.as_ref();
            let public_key_hex = hex::encode(&self.wallet.key_pairs[0].public_key);
            let _did_document = self.wallet.export_identity_document()?;

            // Create message that proves control of both quantum and ETH keys
            let message = format!("{}{}{}", did, public_key_hex, ethereum_address);
            let quantum_signature = self.wallet.sign_content(&message)?;

            Ok((did.to_string(), public_key_hex, quantum_signature))
        }

        /// Create credential proof for on-chain verification
        pub fn create_credential_proof(
            &self,
            credential_hash: &str,
            recipient_ethereum_address: &str,
        ) -> Result<QuantumCredentialProof, Box<dyn std::error::Error>> {
            let verification_message = format!(
                "CREDENTIAL_VERIFICATION:{}:{}:{}",
                credential_hash,
                self.wallet.identity_doc.did.as_ref(),
                recipient_ethereum_address
            );

            let quantum_signature = self.wallet.sign_content(&verification_message)?;

            Ok(QuantumCredentialProof {
                credential_hash: credential_hash.to_string(),
                issuer_did: self.wallet.identity_doc.did.as_ref().to_string(),
                quantum_signature,
                verification_message,
            })
        }

        /// Verify quantum signature for EVM integration
        pub fn verify_quantum_signature(
            message: &str,
            signature: &str,
            public_key: &str,
        ) -> Result<bool, Box<dyn std::error::Error>> {
            let public_key_bytes = hex::decode(public_key)?;
            let signature_bytes = hex::decode(signature)?;

            let is_valid =
                SphincsPlus::verify(&public_key_bytes, message.as_bytes(), &signature_bytes);

            Ok(is_valid)
        }

        /// Generate EVM-compatible DID representation
        pub fn to_evm_did(&self, ethereum_address: &str) -> EVMQuantumDID {
            EVMQuantumDID {
                ethereum_address: ethereum_address.to_string(),
                quantum_did: self.wallet.identity_doc.did.as_ref().to_string(),
                quantum_public_key: hex::encode(&self.wallet.key_pairs[0].public_key),
                did_document: self.wallet.export_identity_document().unwrap_or_default(),
            }
        }

        /// Generate Solidity function call data
        pub fn encode_registration_call(
            &self,
            ethereum_address: &str,
        ) -> Result<String, Box<dyn std::error::Error>> {
            let (did, public_key_hex, signature) =
                self.generate_registration_data(ethereum_address)?;
            let did_document = self.wallet.export_identity_document()?;

            // This would typically use ethers-rs or similar for proper ABI encoding
            // For demo purposes, showing the conceptual structure
            Ok(format!(
                "registerQuantumDID('{}','0x{}','{}','0x{}')",
                did, public_key_hex, did_document, signature
            ))
        }

        /// Create key rotation data for EVM
        pub fn generate_key_rotation_data(
            &mut self,
            ethereum_address: &str,
        ) -> Result<(String, String, String), Box<dyn std::error::Error>> {
            // Sign with current key before rotation
            let current_key_count = self.wallet.key_pairs.len();
            let message = format!(
                "KEY_ROTATION:{}:{}:{}",
                ethereum_address,
                current_key_count + 1,
                chrono::Utc::now().timestamp()
            );

            let old_signature = self.wallet.sign_content(&message)?;

            // Perform rotation
            self.wallet.rotate_keys()?;

            let new_public_key_hex = hex::encode(&self.wallet.key_pairs[0].public_key);
            let new_did_document = self.wallet.export_identity_document()?;

            Ok((new_public_key_hex, new_did_document, old_signature))
        }
    }

    /// Utility functions for EVM integration
    pub mod utils {
        use super::*;

        /// Calculate credential hash for on-chain storage
        pub fn calculate_credential_hash(credential: &VerifiableCredential) -> String {
            let mut hasher = Sha256::new();

            // Create deterministic hash from credential content
            hasher.update(credential.id.as_bytes());
            hasher.update(credential.issuer.as_bytes());
            hasher.update(credential.subject.as_bytes());
            hasher.update(credential.credential_type.as_bytes());

            // Add claims in sorted order for deterministic hashing
            let mut claim_keys: Vec<_> = credential.claims.keys().collect();
            claim_keys.sort();
            for key in claim_keys {
                hasher.update(key.as_bytes());
                hasher.update(credential.claims[key].as_bytes());
            }

            let hash = hasher.finalize();
            format!("0x{}", hex::encode(hash))
        }

        /// Format DID for EVM storage
        pub fn format_did_for_evm(did: &str) -> String {
            // Remove special characters that might cause issues in Solidity
            did.replace(":", "_").replace("-", "_")
        }

        /// Generate deterministic Ethereum address from quantum DID
        pub fn quantum_did_to_eth_address(quantum_public_key: &[u8]) -> String {
            let mut hasher = Sha256::new();
            hasher.update(quantum_public_key);
            hasher.update(b"QUANTUM_TO_ETH_MAPPING");
            let hash = hasher.finalize();

            // Take last 20 bytes like Ethereum address derivation
            format!("0x{}", hex::encode(&hash[12..32]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::evm_integration::*;
    use std::collections::HashMap;

    #[test]
    fn test_evm_registration_data() {
        let bridge = EVMQuantumBridge::new();
        let eth_address = "0x742d35Cc6aba59532c2A76Ad3d2b1e20C0D0D3c1";

        let result = bridge.generate_registration_data(eth_address);
        assert!(result.is_ok());

        let (did, pubkey, signature) = result.unwrap();
        assert!(did.starts_with("did:spacekit:testnet:"));
        assert!(!pubkey.is_empty());
        assert!(!signature.is_empty());
    }

    #[test]
    fn test_credential_proof_creation() {
        let bridge = EVMQuantumBridge::new();
        let credential_hash = "0x1234567890abcdef";
        let eth_address = "0x742d35Cc6aba59532c2A76Ad3d2b1e20C0D0D3c1";

        let proof = bridge.create_credential_proof(credential_hash, eth_address);
        assert!(proof.is_ok());

        let proof = proof.unwrap();
        assert_eq!(proof.credential_hash, credential_hash);
        assert!(!proof.quantum_signature.is_empty());
    }

    #[test]
    fn test_credential_hash_calculation() {
        let wallet = spacekit_did::QuantumResistantWallet::new();
        let mut claims = HashMap::new();
        claims.insert("name".to_string(), "Test User".to_string());
        claims.insert("email".to_string(), "test@example.com".to_string());

        let credential = wallet
            .issue_credential(
                "did:spacekit:testnet:test123",
                "TestCredential",
                claims,
                Some(365),
            )
            .unwrap();

        let hash = utils::calculate_credential_hash(&credential);
        assert!(hash.starts_with("0x"));
        assert_eq!(hash.len(), 66); // 0x + 64 hex chars
    }

    #[test]
    fn test_quantum_to_eth_address() {
        let wallet = spacekit_did::QuantumResistantWallet::new();
        let eth_address = utils::quantum_did_to_eth_address(&wallet.key_pairs[0].public_key);

        assert!(eth_address.starts_with("0x"));
        assert_eq!(eth_address.len(), 42); // 0x + 40 hex chars
    }
}
