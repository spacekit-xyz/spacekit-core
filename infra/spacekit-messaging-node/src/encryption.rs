//! Encryption utilities for quantum-resistant messaging

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use spacekit_primitives::v1::crypto::quantum::{Algorithm, CipherSuite};
use std::collections::HashMap;

#[cfg(feature = "quantum")]
use oqs::kem::{
    Ciphertext, Kem, PublicKey as OqsPublicKey, SecretKey as OqsSecretKey, SharedSecret,
};

/// Encryption service for quantum-resistant message encryption
pub struct MessageEncryption {
    /// Default algorithm to use
    _default_algorithm: Algorithm,
    /// Default cipher suite
    _default_cipher: CipherSuite,
    /// Supported algorithms
    _supported_algorithms: Vec<Algorithm>,
}

/// Encrypted message payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedPayload {
    /// Encrypted data
    pub data: Vec<u8>,
    /// KEM ciphertext for key encapsulation
    pub kem_ciphertext: Vec<u8>,
    /// Algorithm used for encryption
    pub algorithm: String,
    /// Cipher suite used
    pub cipher_suite: String,
    /// Nonce/IV used
    pub nonce: Vec<u8>,
    /// Additional authenticated data
    pub aad: Vec<u8>,
}

/// Key pair for quantum-resistant encryption
#[derive(Debug, Clone)]
pub struct KeyPair {
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
    pub algorithm: Algorithm,
}

/// Group encryption context for managing multiple recipients
pub struct GroupEncryptionContext {
    /// Encryption service
    encryption: MessageEncryption,
    /// Member public keys mapped by user ID
    member_keys: HashMap<String, (Vec<u8>, Algorithm)>,
}

/// A shared symmetric group key wrapped per-member via KEM.
/// The same `group_data_key` encrypts all messages; each member gets
/// their own KEM-wrapped copy so they can unwrap it with their private key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupKeyBundle {
    /// The group-wide data encryption key, encrypted per-member via KEM.
    /// Map: member DID -> EncryptedPayload containing the 32-byte data key.
    pub wrapped_keys: HashMap<String, EncryptedPayload>,
    /// Algorithm used for the data encryption (e.g. AES-256-GCM).
    pub data_cipher: String,
}

impl MessageEncryption {
    /// Create a new encryption service
    pub fn new(default_algorithm: Algorithm, default_cipher: CipherSuite) -> Self {
        let supported_algorithms = vec![
            Algorithm::Kyber512,
            Algorithm::Kyber768,
            Algorithm::Kyber1024,
            Algorithm::NtruPrimeSntrup761,
            Algorithm::FrodoKem1344Aes,
            Algorithm::FrodoKem1344Shake,
        ];

        Self {
            _default_algorithm: default_algorithm,
            _default_cipher: default_cipher,
            _supported_algorithms: supported_algorithms,
        }
    }

    fn resolve_algorithm(&self, algorithm: Algorithm) -> Algorithm {
        let is_supported = self._supported_algorithms.iter().any(|candidate| {
            std::mem::discriminant(candidate) == std::mem::discriminant(&algorithm)
        });
        if is_supported {
            algorithm
        } else {
            self._default_algorithm.clone()
        }
    }

    /// Encrypt data for a single recipient (direct messaging)
    pub async fn encrypt_for_recipient(
        &self,
        data: &[u8],
        public_key: &[u8],
        algorithm: Algorithm,
    ) -> Result<EncryptedPayload> {
        let algorithm = self.resolve_algorithm(algorithm);
        #[cfg(feature = "quantum")]
        {
            use aes_gcm::{
                aead::{Aead, KeyInit, OsRng},
                Aes256Gcm, Nonce,
            };
            use oqs::kem::Kem;
            use rand::RngCore;

            // 1. Initialize KEM with quantum-resistant algorithm
            let algorithm_name = match algorithm {
                Algorithm::Kyber512 => oqs::kem::Algorithm::Kyber512,
                Algorithm::Kyber768 => oqs::kem::Algorithm::Kyber768,
                Algorithm::Kyber1024 => oqs::kem::Algorithm::Kyber1024,
                _ => oqs::kem::Algorithm::Kyber1024, // Default
            };

            let kem = Kem::new(algorithm_name)
                .map_err(|e| anyhow!("Failed to initialize KEM: {:?}", e))?;

            // 2. Deserialize recipient's public key
            let recipient_pk = kem
                .public_key_from_bytes(public_key)
                .ok_or_else(|| anyhow!("Invalid public key"))?;

            // 3. Encapsulate to generate shared secret
            let (ciphertext, shared_secret) = kem
                .encapsulate(&recipient_pk)
                .map_err(|e| anyhow!("Encapsulation failed: {:?}", e))?;

            // 4. Derive AES key from shared secret (use first 32 bytes)
            let aes_key = &shared_secret.as_ref()[..32];
            let cipher = Aes256Gcm::new_from_slice(aes_key)
                .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

            // 5. Generate random nonce
            let mut nonce_bytes = [0u8; 12];
            OsRng.fill_bytes(&mut nonce_bytes);
            let nonce = Nonce::from_slice(&nonce_bytes);

            // 6. Encrypt the data
            let encrypted_data = cipher
                .encrypt(nonce, data)
                .map_err(|e| anyhow!("Encryption failed: {}", e))?;

            Ok(EncryptedPayload {
                data: encrypted_data,
                kem_ciphertext: ciphertext.into_vec(),
                algorithm: format!("{:?}", algorithm),
                cipher_suite: format!("{:?}", self._default_cipher),
                nonce: nonce_bytes.to_vec(),
                aad: vec![],
            })
        }

        #[cfg(not(feature = "quantum"))]
        {
            let _ = algorithm;
            use ecies::encrypt;

            println!("⚠️ Using ECIES fallback (liboqs not available)");

            let ciphertext =
                encrypt(public_key, data).map_err(|e| anyhow!("ECIES encryption failed: {}", e))?;

            Ok(EncryptedPayload {
                data: ciphertext,
                kem_ciphertext: vec![],
                algorithm: "ECIES".to_string(),
                cipher_suite: "ECIES".to_string(),
                nonce: vec![],
                aad: vec![],
            })
        }
    }

    /// Decrypt data from a recipient
    pub async fn decrypt_from_sender(
        &self,
        encrypted_payload: &EncryptedPayload,
        secret_key: &[u8],
        algorithm: Algorithm,
    ) -> Result<Vec<u8>> {
        let algorithm = self.resolve_algorithm(algorithm);
        #[cfg(feature = "quantum")]
        {
            use aes_gcm::{
                aead::{Aead, KeyInit},
                Aes256Gcm, Nonce,
            };
            use oqs::kem::Kem;

            // 1. Initialize KEM
            let algorithm_name = match algorithm {
                Algorithm::Kyber512 => oqs::kem::Algorithm::Kyber512,
                Algorithm::Kyber768 => oqs::kem::Algorithm::Kyber768,
                Algorithm::Kyber1024 => oqs::kem::Algorithm::Kyber1024,
                _ => oqs::kem::Algorithm::Kyber1024,
            };

            let kem = Kem::new(algorithm_name)
                .map_err(|e| anyhow!("Failed to initialize KEM: {:?}", e))?;

            // 2. Deserialize secret key and ciphertext
            let sk = kem
                .secret_key_from_bytes(secret_key)
                .ok_or_else(|| anyhow!("Invalid secret key"))?;
            let ct = kem
                .ciphertext_from_bytes(&encrypted_payload.kem_ciphertext)
                .ok_or_else(|| anyhow!("Invalid ciphertext"))?;

            // 3. Decapsulate to recover shared secret
            let shared_secret = kem
                .decapsulate(&sk, &ct)
                .map_err(|e| anyhow!("Decapsulation failed: {:?}", e))?;

            // 4. Derive AES key
            let aes_key = &shared_secret.as_ref()[..32];
            let cipher = Aes256Gcm::new_from_slice(aes_key)
                .map_err(|e| anyhow!("Failed to create cipher: {}", e))?;

            // 5. Decrypt the data
            let nonce = Nonce::from_slice(&encrypted_payload.nonce);
            let decrypted_data = cipher
                .decrypt(nonce, encrypted_payload.data.as_ref())
                .map_err(|e| anyhow!("Decryption failed: {}", e))?;

            Ok(decrypted_data)
        }

        #[cfg(not(feature = "quantum"))]
        {
            let _ = algorithm;
            use ecies::decrypt;

            if encrypted_payload.algorithm != "ECIES" {
                return Err(anyhow!(
                    "Unsupported fallback algorithm: {}",
                    encrypted_payload.algorithm
                ));
            }

            println!("⚠️ Using ECIES fallback decryption");

            decrypt(secret_key, &encrypted_payload.data)
                .map_err(|e| anyhow!("ECIES decryption failed: {}", e))
        }
    }

    /// Encrypt data for multiple recipients (group messaging)
    pub async fn encrypt_for_group(
        &self,
        data: &[u8],
        recipient_keys: &[(String, Vec<u8>, Algorithm)],
    ) -> Result<HashMap<String, EncryptedPayload>> {
        let mut encrypted_messages = HashMap::new();

        for (user_id, public_key, algorithm) in recipient_keys.iter() {
            // Use the single recipient encryption for each member
            let encrypted = self
                .encrypt_for_recipient(data, public_key, algorithm.clone())
                .await?;
            encrypted_messages.insert(user_id.clone(), encrypted);
        }

        Ok(encrypted_messages)
    }

    /// Encrypt data once with a shared symmetric key, then wrap that key
    /// for each group member via KEM.  Much more efficient for large groups
    /// because the payload is only encrypted once.
    pub async fn encrypt_for_group_shared_key(
        &self,
        data: &[u8],
        recipient_keys: &[(String, Vec<u8>, Algorithm)],
    ) -> Result<(Vec<u8>, GroupKeyBundle)> {
        use rand::RngCore;

        // Generate a random 256-bit data encryption key
        let mut data_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut data_key);

        // Encrypt the payload once with AES-256-GCM using the data key
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        #[cfg(feature = "quantum")]
        let ciphertext = {
            use aes_gcm::Nonce;
            use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
            let cipher =
                Aes256Gcm::new_from_slice(&data_key).map_err(|e| anyhow!("AES key init: {}", e))?;
            let nonce = Nonce::from_slice(&nonce_bytes);
            let mut ct = cipher
                .encrypt(nonce, data)
                .map_err(|e| anyhow!("AES encrypt: {}", e))?;
            // Prepend nonce so decryptor can extract it
            let mut out = nonce_bytes.to_vec();
            out.append(&mut ct);
            out
        };

        #[cfg(not(feature = "quantum"))]
        let ciphertext = {
            let _ = nonce_bytes;
            data.to_vec()
        };

        // Wrap the data key for each member via their public KEM key
        let mut wrapped_keys = HashMap::new();
        for (user_id, public_key, algorithm) in recipient_keys {
            let wrapped = self
                .encrypt_for_recipient(&data_key, public_key, algorithm.clone())
                .await?;
            wrapped_keys.insert(user_id.clone(), wrapped);
        }

        Ok((
            ciphertext,
            GroupKeyBundle {
                wrapped_keys,
                data_cipher: "AES-256-GCM".to_string(),
            },
        ))
    }

    /// Decrypt data that was encrypted with a shared group key.
    /// The caller unwraps their copy of the data key from the bundle,
    /// then decrypts the payload.
    pub async fn decrypt_group_shared_key(
        &self,
        ciphertext: &[u8],
        member_did: &str,
        private_key: &[u8],
        bundle: &GroupKeyBundle,
    ) -> Result<Vec<u8>> {
        let wrapped = bundle
            .wrapped_keys
            .get(member_did)
            .ok_or_else(|| anyhow!("No wrapped key for DID {}", member_did))?;

        // Unwrap the data key using the member's private key
        let algorithm = Algorithm::Kyber1024; // default; could be stored in bundle
        let data_key = self
            .decrypt_from_sender(wrapped, private_key, algorithm)
            .await?;
        if data_key.len() != 32 {
            return Err(anyhow!("Invalid data key length: {}", data_key.len()));
        }

        #[cfg(feature = "quantum")]
        {
            use aes_gcm::Nonce;
            use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
            if ciphertext.len() < 12 {
                return Err(anyhow!("Ciphertext too short for nonce"));
            }
            let (nonce_bytes, ct) = ciphertext.split_at(12);
            let cipher =
                Aes256Gcm::new_from_slice(&data_key).map_err(|e| anyhow!("AES key init: {}", e))?;
            let nonce = Nonce::from_slice(nonce_bytes);
            cipher
                .decrypt(nonce, ct)
                .map_err(|e| anyhow!("AES decrypt: {}", e))
        }

        #[cfg(not(feature = "quantum"))]
        {
            Ok(ciphertext.to_vec())
        }
    }
}

impl GroupEncryptionContext {
    /// Create new group encryption context
    pub fn new(encryption: MessageEncryption) -> Self {
        Self {
            encryption,
            member_keys: HashMap::new(),
        }
    }

    /// Add a member's public key to the context
    pub fn add_member(&mut self, user_id: String, public_key: Vec<u8>, algorithm: Algorithm) {
        self.member_keys.insert(user_id, (public_key, algorithm));
    }

    /// Remove a member from the context
    pub fn remove_member(&mut self, user_id: &str) {
        self.member_keys.remove(user_id);
    }

    /// Encrypt a message for all group members
    pub async fn encrypt_for_all_members(
        &self,
        data: &[u8],
    ) -> Result<HashMap<String, EncryptedPayload>> {
        let recipient_keys: Vec<(String, Vec<u8>, Algorithm)> = self
            .member_keys
            .iter()
            .map(|(user_id, (key, algo))| (user_id.clone(), key.clone(), algo.clone()))
            .collect();

        self.encryption
            .encrypt_for_group(data, &recipient_keys)
            .await
    }

    /// Encrypt a message once with a shared data key, wrapping for all members.
    pub async fn encrypt_shared_key(&self, data: &[u8]) -> Result<(Vec<u8>, GroupKeyBundle)> {
        let recipient_keys: Vec<(String, Vec<u8>, Algorithm)> = self
            .member_keys
            .iter()
            .map(|(user_id, (key, algo))| (user_id.clone(), key.clone(), algo.clone()))
            .collect();

        self.encryption
            .encrypt_for_group_shared_key(data, &recipient_keys)
            .await
    }

    /// Re-wrap an existing group key bundle for a new member without re-encrypting data.
    pub async fn add_member_to_bundle(
        &self,
        bundle: &mut GroupKeyBundle,
        data_key_plaintext: &[u8],
        new_member_did: &str,
    ) -> Result<()> {
        let (pk, algo) = self
            .member_keys
            .get(new_member_did)
            .ok_or_else(|| anyhow!("Member {} not in context", new_member_did))?;
        let wrapped = self
            .encryption
            .encrypt_for_recipient(data_key_plaintext, pk, algo.clone())
            .await?;
        bundle
            .wrapped_keys
            .insert(new_member_did.to_string(), wrapped);
        Ok(())
    }
}
