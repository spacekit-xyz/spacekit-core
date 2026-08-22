use bs58;
use hex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use spacekit_did::{QuantumResistantWallet, SphincsPlus, VerifiableCredential};

/// Solana integration utilities for quantum DID management
pub mod solana_integration {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SolanaQuantumDID {
        pub solana_address: String,          // Solana wallet address (Ed25519)
        pub quantum_did: String,             // Quantum DID string
        pub quantum_public_key: String,      // Hex-encoded quantum public key
        pub did_document: String,            // JSON DID document
        pub program_derived_address: String, // PDA for DID storage
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SolanaCredentialProof {
        pub credential_hash: String,
        pub issuer_did: String,
        pub quantum_signature: String,
        pub verification_message: String,
        pub slot: u64, // Solana slot for timestamp
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SolanaTransactionData {
        pub instruction_data: Vec<u8>,
        pub accounts: Vec<SolanaAccountMeta>,
        pub program_id: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct SolanaAccountMeta {
        pub pubkey: String,
        pub is_signer: bool,
        pub is_writable: bool,
    }

    pub struct SolanaQuantumBridge {
        pub wallet: QuantumResistantWallet,
    }

    impl Default for SolanaQuantumBridge {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SolanaQuantumBridge {
        pub fn new() -> Self {
            Self {
                wallet: QuantumResistantWallet::new(),
            }
        }

        /// Generate registration data for Solana program
        pub fn generate_registration_data(
            &self,
            solana_address: &str,
        ) -> Result<(String, String, String, String), Box<dyn std::error::Error>> {
            let did = self.wallet.identity_doc.did.as_ref();
            let public_key_hex = hex::encode(&self.wallet.key_pairs[0].public_key);
            let _did_document = self.wallet.export_identity_document()?;

            // Generate Program Derived Address (PDA) for DID storage
            let pda = Self::derive_did_pda(did, solana_address)?;

            // Create message that proves control of both quantum and Solana keys
            let message = format!("{}{}{}{}", did, public_key_hex, solana_address, pda);
            let quantum_signature = self.wallet.sign_content(&message)?;

            Ok((did.to_string(), public_key_hex, quantum_signature, pda))
        }

        /// Create credential proof for Solana program verification
        pub fn create_credential_proof(
            &self,
            credential_hash: &str,
            recipient_solana_address: &str,
            slot: u64,
        ) -> Result<SolanaCredentialProof, Box<dyn std::error::Error>> {
            let verification_message = format!(
                "SOLANA_CREDENTIAL_VERIFICATION:{}:{}:{}:{}",
                credential_hash,
                self.wallet.identity_doc.did.as_ref(),
                recipient_solana_address,
                slot
            );

            let quantum_signature = self.wallet.sign_content(&verification_message)?;

            Ok(SolanaCredentialProof {
                credential_hash: credential_hash.to_string(),
                issuer_did: self.wallet.identity_doc.did.as_ref().to_string(),
                quantum_signature,
                verification_message,
                slot,
            })
        }

        /// Generate Solana transaction instruction data
        pub fn encode_register_did_instruction(
            &self,
            solana_address: &str,
        ) -> Result<SolanaTransactionData, Box<dyn std::error::Error>> {
            let (did, public_key_hex, signature, pda) =
                self.generate_registration_data(solana_address)?;
            let did_document = self.wallet.export_identity_document()?;

            // Serialize instruction data for Solana program
            let instruction_data = Self::serialize_register_did_data(
                &did,
                &public_key_hex,
                &signature,
                &did_document,
            )?;

            // Define required accounts for the instruction
            let accounts = vec![
                SolanaAccountMeta {
                    pubkey: solana_address.to_string(),
                    is_signer: true,
                    is_writable: false,
                },
                SolanaAccountMeta {
                    pubkey: pda.clone(),
                    is_signer: false,
                    is_writable: true,
                },
                SolanaAccountMeta {
                    pubkey: "11111111111111111111111111111112".to_string(), // System Program
                    is_signer: false,
                    is_writable: false,
                },
            ];

            Ok(SolanaTransactionData {
                instruction_data,
                accounts,
                program_id: "QuantumDIDProgram1111111111111111111111111".to_string(), // Placeholder
            })
        }

        /// Generate key rotation instruction for Solana
        pub fn generate_key_rotation_instruction(
            &mut self,
            solana_address: &str,
            current_slot: u64,
        ) -> Result<SolanaTransactionData, Box<dyn std::error::Error>> {
            // Sign with current key before rotation
            let message = format!(
                "SOLANA_KEY_ROTATION:{}:{}:{}",
                solana_address,
                current_slot,
                chrono::Utc::now().timestamp()
            );

            let old_signature = self.wallet.sign_content(&message)?;

            // Perform rotation
            self.wallet.rotate_keys()?;

            let new_public_key_hex = hex::encode(&self.wallet.key_pairs[0].public_key);
            let new_did_document = self.wallet.export_identity_document()?;

            // Create instruction data
            let instruction_data = Self::serialize_key_rotation_data(
                &new_public_key_hex,
                &new_did_document,
                &old_signature,
                current_slot,
            )?;

            let pda = Self::derive_did_pda(self.wallet.identity_doc.did.as_ref(), solana_address)?;

            let accounts = vec![
                SolanaAccountMeta {
                    pubkey: solana_address.to_string(),
                    is_signer: true,
                    is_writable: false,
                },
                SolanaAccountMeta {
                    pubkey: pda,
                    is_signer: false,
                    is_writable: true,
                },
            ];

            Ok(SolanaTransactionData {
                instruction_data,
                accounts,
                program_id: "QuantumDIDProgram1111111111111111111111111".to_string(),
            })
        }

        /// Generate Program Derived Address for DID storage
        pub fn derive_did_pda(
            did: &str,
            solana_address: &str,
        ) -> Result<String, Box<dyn std::error::Error>> {
            // In real Solana integration, this would use:
            // Pubkey::find_program_address(&[did.as_bytes(), solana_address.as_bytes()], &program_id)

            // For demo purposes, create deterministic address
            let mut hasher = Sha256::new();
            hasher.update(did.as_bytes());
            hasher.update(solana_address.as_bytes());
            hasher.update(b"QUANTUM_DID_PDA");
            let hash = hasher.finalize();

            // Convert to base58 (Solana address format)
            Ok(bs58::encode(&hash[..32]).into_string())
        }

        /// Convert to Solana-compatible DID representation
        pub fn to_solana_did(
            &self,
            solana_address: &str,
        ) -> Result<SolanaQuantumDID, Box<dyn std::error::Error>> {
            let pda = Self::derive_did_pda(self.wallet.identity_doc.did.as_ref(), solana_address)?;

            Ok(SolanaQuantumDID {
                solana_address: solana_address.to_string(),
                quantum_did: self.wallet.identity_doc.did.as_ref().to_string(),
                quantum_public_key: hex::encode(&self.wallet.key_pairs[0].public_key),
                did_document: self.wallet.export_identity_document().unwrap_or_default(),
                program_derived_address: pda,
            })
        }

        /// Verify quantum signature (Solana-compatible)
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

        // Private helper methods
        fn serialize_register_did_data(
            did: &str,
            public_key: &str,
            signature: &str,
            did_document: &str,
        ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            // In real implementation, use borsh or bincode for Solana serialization
            let data = format!(
                "REGISTER_DID:{}:{}:{}:{}",
                did, public_key, signature, did_document
            );
            Ok(data.into_bytes())
        }

        fn serialize_key_rotation_data(
            new_public_key: &str,
            new_did_document: &str,
            old_signature: &str,
            slot: u64,
        ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            let data = format!(
                "ROTATE_KEYS:{}:{}:{}:{}",
                new_public_key, new_did_document, old_signature, slot
            );
            Ok(data.into_bytes())
        }
    }

    /// Utility functions for Solana integration
    pub mod utils {
        use super::*;

        /// Calculate credential hash for Solana storage
        pub fn calculate_credential_hash_solana(credential: &VerifiableCredential) -> String {
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

            // Add Solana-specific prefix
            hasher.update(b"SOLANA_CREDENTIAL");

            let hash = hasher.finalize();
            bs58::encode(hash).into_string() // Solana uses base58 encoding
        }

        /// Format DID for Solana program storage
        pub fn format_did_for_solana(did: &str) -> String {
            // Solana programs prefer base58 encoding
            bs58::encode(did.as_bytes()).into_string()
        }

        /// Generate deterministic Solana address from quantum DID
        pub fn quantum_did_to_solana_address(quantum_public_key: &[u8]) -> String {
            let mut hasher = Sha256::new();
            hasher.update(quantum_public_key);
            hasher.update(b"QUANTUM_TO_SOLANA_MAPPING");
            let hash = hasher.finalize();

            // Take 32 bytes for Solana address and encode as base58
            bs58::encode(&hash[..32]).into_string()
        }

        /// Create Solana transaction signature format
        pub fn create_solana_transaction_message(
            instruction_data: &[u8],
            accounts: &[SolanaAccountMeta],
            recent_blockhash: &str,
        ) -> String {
            // Simplified transaction message format for demo
            format!(
                "SOLANA_TX:{}:{}:{}",
                hex::encode(instruction_data),
                accounts.len(),
                recent_blockhash
            )
        }

        /// Estimate Solana transaction cost
        pub fn estimate_transaction_cost(instruction_size: usize, num_accounts: usize) -> u64 {
            // Basic Solana transaction cost estimation (in lamports)
            let base_fee = 5000; // Base transaction fee
            let instruction_fee = instruction_size as u64 * 2; // Per-byte instruction fee
            let account_fee = num_accounts as u64 * 1000; // Per-account fee

            base_fee + instruction_fee + account_fee
        }
    }
}

#[cfg(test)]
mod tests {
    use super::solana_integration::*;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_solana_registration_data() {
        let bridge = SolanaQuantumBridge::new();
        let solana_address = "DQW8VtTLqFLbG9qKJTMx9UvJ8JbCpTHdGw4xBgTQA3L";

        let result = bridge.generate_registration_data(solana_address);
        assert!(result.is_ok());

        let (did, pubkey, signature, pda) = result.unwrap();
        assert!(did.starts_with("did:spacekit:testnet:"));
        assert!(!pubkey.is_empty());
        assert!(!signature.is_empty());
        assert!(!pda.is_empty());
    }

    #[test]
    fn test_solana_credential_proof() {
        let bridge = SolanaQuantumBridge::new();
        let credential_hash = "5J7XfCDt6rHZPJe8E3S8mGdJ2nF8qYk9";
        let solana_address = "DQW8VtTLqFLbG9qKJTMx9UvJ8JbCpTHdGw4xBgTQA3L";
        let slot = 123456789;

        let proof = bridge.create_credential_proof(credential_hash, solana_address, slot);
        assert!(proof.is_ok());

        let proof = proof.unwrap();
        assert_eq!(proof.credential_hash, credential_hash);
        assert_eq!(proof.slot, slot);
        assert!(!proof.quantum_signature.is_empty());
    }

    #[test]
    fn test_solana_pda_derivation() {
        let did = "did:spacekit:testnet:test123";
        let solana_address = "DQW8VtTLqFLbG9qKJTMx9UvJ8JbCpTHdGw4xBgTQA3L";

        let pda1 = SolanaQuantumBridge::derive_did_pda(did, solana_address).unwrap();
        let pda2 = SolanaQuantumBridge::derive_did_pda(did, solana_address).unwrap();

        // PDA should be deterministic
        assert_eq!(pda1, pda2);
        assert!(!pda1.is_empty());
    }

    #[test]
    fn test_solana_transaction_instruction() {
        let bridge = SolanaQuantumBridge::new();
        let solana_address = "DQW8VtTLqFLbG9qKJTMx9UvJ8JbCpTHdGw4xBgTQA3L";

        let instruction = bridge.encode_register_did_instruction(solana_address);
        assert!(instruction.is_ok());

        let instruction = instruction.unwrap();
        assert!(!instruction.instruction_data.is_empty());
        assert!(!instruction.accounts.is_empty());
        assert!(!instruction.program_id.is_empty());
    }

    #[test]
    fn test_solana_credential_hash() {
        let wallet = spacekit_did::QuantumResistantWallet::new();
        let mut claims = HashMap::new();
        claims.insert("name".to_string(), "Test User".to_string());
        claims.insert("platform".to_string(), "Solana".to_string());

        let credential = wallet
            .issue_credential(
                "did:spacekit:testnet:test123",
                "SolanaCredential",
                claims,
                Some(365),
            )
            .unwrap();

        let hash = utils::calculate_credential_hash_solana(&credential);
        assert!(!hash.is_empty());

        // Should be valid base58
        assert!(bs58::decode(&hash).into_vec().is_ok());
    }

    #[test]
    fn test_quantum_to_solana_address() {
        let wallet = spacekit_did::QuantumResistantWallet::new();
        let solana_address = utils::quantum_did_to_solana_address(&wallet.key_pairs[0].public_key);

        assert!(!solana_address.is_empty());

        // Should be valid base58
        assert!(bs58::decode(&solana_address).into_vec().is_ok());
    }
}
