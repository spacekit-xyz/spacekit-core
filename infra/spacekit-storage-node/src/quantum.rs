//! Quantum-resistant cryptography integration
//!
//! Integrates with spacekit-primitives for comprehensive quantum-resistant encryption
//! Uses real OQS (Open Quantum Safe) KEM algorithms for production-grade security

use anyhow::Result;
use serde::{Deserialize, Serialize};
use spacekit_primitives::v1::crypto::quantum::{Algorithm, CipherSuite};
use tracing::{debug, info, warn};

// Real OQS KEM integration
#[cfg(feature = "quantum")]
use oqs::kem::Kem;

// AES-GCM for symmetric encryption (using shared secret from KEM)
#[cfg(feature = "quantum")]
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};

/// Quantum encryption service
#[derive(Debug, Clone)]
pub struct QuantumCrypto {
    pub(crate) default_algorithm: Algorithm,
    pub(crate) default_cipher: CipherSuite,
}

/// Encryption metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionMetadata {
    pub algorithm: String,
    pub cipher_suite: String,
    pub key_derivation: String,
    pub quantum_resistant: bool,
    pub key_size: usize,
    pub nonce_size: usize,
}

/// Encrypted data wrapper
///
/// For real KEM encryption:
/// - `data`: AES-GCM encrypted payload
/// - `kem_ciphertext`: KEM ciphertext (needed for decryption)
/// - `metadata`: Encryption algorithm and parameters
/// - `integrity_hash`: Blake3 hash for integrity verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedData {
    pub data: Vec<u8>,                   // AES-GCM encrypted data
    pub kem_ciphertext: Option<Vec<u8>>, // KEM ciphertext (for real KEM, None for placeholder)
    pub metadata: EncryptionMetadata,
    pub integrity_hash: String,
}

impl Default for QuantumCrypto {
    fn default() -> Self {
        Self {
            default_algorithm: Algorithm::Kyber1024,
            default_cipher: CipherSuite::AES256,
        }
    }
}

impl QuantumCrypto {
    /// Create a new quantum crypto service
    pub fn new(algorithm: Algorithm, cipher: CipherSuite) -> Self {
        Self {
            default_algorithm: algorithm,
            default_cipher: cipher,
        }
    }

    /// Preferred KEM for this node when a client does not specify an owner key algorithm.
    pub fn server_default_kem_algorithm(&self) -> Algorithm {
        self.default_algorithm.clone()
    }

    /// Encrypt to a recipient public key using an explicit KEM (must match the keypair that produced `public_key`).
    pub async fn encrypt_data_with_algorithm(
        &self,
        data: &[u8],
        public_key: &[u8],
        algorithm: Algorithm,
    ) -> Result<EncryptedData> {
        info!("Encrypting data with quantum algorithm: {:?}", algorithm);

        let (encrypted_data, kem_ciphertext) = self
            .encrypt_with_algorithm(data, public_key, algorithm.clone())
            .await?;

        let metadata = EncryptionMetadata {
            algorithm: format!("{:?}", algorithm),
            cipher_suite: format!("{:?}", self.default_cipher),
            key_derivation: "kem-shared-secret".to_string(),
            quantum_resistant: true,
            key_size: self.get_key_size(algorithm.clone()),
            nonce_size: self.get_nonce_size(self.default_cipher.clone()),
        };

        let mut hash_input = encrypted_data.clone();
        if let Some(ref kem_ct) = kem_ciphertext {
            hash_input.extend_from_slice(kem_ct);
        }
        let integrity_hash = hex::encode(blake3::hash(&hash_input).as_bytes());

        Ok(EncryptedData {
            data: encrypted_data,
            kem_ciphertext,
            metadata,
            integrity_hash,
        })
    }

    /// Encrypt data with quantum-resistant algorithm (REAL KEM) using this node's default KEM.
    pub async fn encrypt_data(&self, data: &[u8], public_key: &[u8]) -> Result<EncryptedData> {
        self.encrypt_data_with_algorithm(data, public_key, self.default_algorithm.clone())
            .await
    }

    /// Decrypt data with quantum-resistant algorithm (REAL KEM)
    pub async fn decrypt_data(
        &self,
        encrypted: &EncryptedData,
        private_key: &[u8],
    ) -> Result<Vec<u8>> {
        debug!(
            "Decrypting data with algorithm: {}",
            encrypted.metadata.algorithm
        );

        // Verify integrity (if integrity hash is provided)
        if !encrypted.integrity_hash.is_empty() {
            let mut hash_input = encrypted.data.clone();
            if let Some(ref kem_ct) = encrypted.kem_ciphertext {
                hash_input.extend_from_slice(kem_ct);
            }
            let computed_hash = hex::encode(blake3::hash(&hash_input).as_bytes());
            if computed_hash != encrypted.integrity_hash {
                return Err(anyhow::anyhow!("Data integrity check failed"));
            }
        }

        // Parse algorithm from metadata
        let algorithm = self.parse_algorithm(&encrypted.metadata.algorithm)?;

        // Decrypt using real KEM or fallback
        let decrypted_data = self
            .decrypt_with_algorithm(
                &encrypted.data,
                encrypted.kem_ciphertext.as_deref(),
                private_key,
                algorithm,
            )
            .await?;

        Ok(decrypted_data)
    }

    /// Encrypt file chunks for distributed storage
    pub async fn encrypt_chunk(
        &self,
        chunk_data: &[u8],
        chunk_id: &str,
        public_key: &[u8],
    ) -> Result<EncryptedData> {
        info!(
            "Encrypting chunk: {} ({} bytes)",
            chunk_id,
            chunk_data.len()
        );
        self.encrypt_data(chunk_data, public_key).await
    }

    /// Decrypt file chunks from distributed storage
    pub async fn decrypt_chunk(
        &self,
        encrypted_chunk: &EncryptedData,
        private_key: &[u8],
    ) -> Result<Vec<u8>> {
        debug!("Decrypting chunk ({} bytes)", encrypted_chunk.data.len());
        self.decrypt_data(encrypted_chunk, private_key).await
    }

    /// Generate a key pair for the specified algorithm (REAL KEM)
    pub async fn generate_keypair(&self, algorithm: Algorithm) -> Result<(Vec<u8>, Vec<u8>)> {
        info!("Generating quantum keypair for algorithm: {:?}", algorithm);

        #[cfg(feature = "quantum")]
        {
            // Use real OQS KEM for key generation
            let kem = self.create_kem_instance(&algorithm)?;
            let (public_key, secret_key) = kem
                .keypair()
                .map_err(|e| anyhow::anyhow!("Failed to generate KEM keypair: {}", e))?;

            Ok((public_key.into_vec(), secret_key.into_vec()))
        }

        #[cfg(not(feature = "quantum"))]
        {
            // Fallback: Generate placeholder keys if quantum feature not enabled
            warn!("Quantum feature not enabled - using placeholder keys");
            match algorithm {
                Algorithm::Kyber512 => Ok((vec![0u8; 800], vec![0u8; 1632])),
                Algorithm::Kyber768 => Ok((vec![0u8; 1088], vec![0u8; 2400])),
                Algorithm::Kyber1024 => Ok((vec![0u8; 1568], vec![0u8; 3168])),
                _ => Err(anyhow::anyhow!(
                    "Unsupported algorithm for key generation: {:?}",
                    algorithm
                )),
            }
        }
    }

    /// Create a KEM instance for the specified algorithm
    #[cfg(feature = "quantum")]
    fn create_kem_instance(&self, algorithm: &Algorithm) -> Result<Kem> {
        let oqs_algorithm = match algorithm {
            Algorithm::Kyber512 => oqs::kem::Algorithm::Kyber512,
            Algorithm::Kyber768 => oqs::kem::Algorithm::Kyber768,
            Algorithm::Kyber1024 => oqs::kem::Algorithm::Kyber1024,
            Algorithm::NtruPrimeSntrup761 => oqs::kem::Algorithm::NtruPrimeSntrup761,
            Algorithm::FrodoKem1344Aes => oqs::kem::Algorithm::FrodoKem1344Aes,
            Algorithm::FrodoKem1344Shake => oqs::kem::Algorithm::FrodoKem1344Shake,
            Algorithm::ClassicMcEliece348864 => oqs::kem::Algorithm::ClassicMcEliece348864,
            Algorithm::BikeL1 => oqs::kem::Algorithm::BikeL1,
            Algorithm::BikeL3 => oqs::kem::Algorithm::BikeL3,
            Algorithm::BikeL5 => oqs::kem::Algorithm::BikeL5,
        };

        Kem::new(oqs_algorithm).map_err(|e| anyhow::anyhow!("Failed to create KEM instance: {}", e))
    }

    /// Raw KEM encapsulation: returns (kem_ciphertext, shared_secret).
    ///
    /// Used by the challenge-response auth flow so the server can produce a challenge
    /// that only the holder of the matching private key can answer.
    pub async fn encrypt_with_kem(
        &self,
        public_key: &[u8],
        algorithm: Algorithm,
    ) -> Result<(Vec<u8>, Vec<u8>)> {
        #[cfg(feature = "quantum")]
        {
            let kem = self.create_kem_instance(&algorithm)?;
            let pk = kem.public_key_from_bytes(public_key).ok_or_else(|| {
                anyhow::anyhow!("Failed to parse public key for KEM encapsulation")
            })?;
            let (kem_ciphertext, shared_secret) = kem
                .encapsulate(&pk)
                .map_err(|e| anyhow::anyhow!("KEM encapsulation failed: {}", e))?;
            Ok((kem_ciphertext.into_vec(), shared_secret.as_ref().to_vec()))
        }

        #[cfg(not(feature = "quantum"))]
        {
            warn!("Quantum feature not enabled — KEM encapsulation unavailable");
            Err(anyhow::anyhow!("KEM requires the quantum feature"))
        }
    }

    /// Raw KEM decapsulation: returns the shared_secret.
    pub async fn decrypt_with_kem(
        &self,
        private_key: &[u8],
        kem_ciphertext: &[u8],
        algorithm: Algorithm,
    ) -> Result<Vec<u8>> {
        #[cfg(feature = "quantum")]
        {
            let kem = self.create_kem_instance(&algorithm)?;
            let sk = kem.secret_key_from_bytes(private_key).ok_or_else(|| {
                anyhow::anyhow!("Failed to parse secret key for KEM decapsulation")
            })?;
            let ct = kem
                .ciphertext_from_bytes(kem_ciphertext)
                .ok_or_else(|| anyhow::anyhow!("Failed to parse KEM ciphertext"))?;
            let shared_secret = kem
                .decapsulate(&sk, &ct)
                .map_err(|e| anyhow::anyhow!("KEM decapsulation failed: {}", e))?;
            Ok(shared_secret.as_ref().to_vec())
        }

        #[cfg(not(feature = "quantum"))]
        {
            warn!("Quantum feature not enabled — KEM decapsulation unavailable");
            Err(anyhow::anyhow!("KEM requires the quantum feature"))
        }
    }

    /// Get supported algorithms
    pub fn get_supported_algorithms(&self) -> Vec<Algorithm> {
        vec![
            Algorithm::Kyber512,
            Algorithm::Kyber768,
            Algorithm::Kyber1024,
            Algorithm::NtruPrimeSntrup761,
            Algorithm::FrodoKem1344Aes,
            Algorithm::FrodoKem1344Shake,
            Algorithm::ClassicMcEliece348864,
            Algorithm::BikeL1,
            Algorithm::BikeL3,
            Algorithm::BikeL5,
        ]
    }

    /// Get cipher suites
    pub fn get_supported_ciphers(&self) -> Vec<CipherSuite> {
        vec![
            CipherSuite::AES256,
            CipherSuite::ChaCha20,
            CipherSuite::XChaCha20,
        ]
    }

    /// Verify that a private key matches a public key (keypair verification)
    ///
    /// For KEM algorithms (like Kyber), we verify by:
    /// 1. Encrypting a test message with the public key
    /// 2. Attempting to decrypt with the private key
    /// 3. If decryption succeeds, the keypair matches
    ///
    /// This is the standard way to verify KEM keypairs since you cannot
    /// directly derive public keys from private keys in KEM schemes.
    pub async fn verify_keypair(
        &self,
        public_key: &[u8],
        private_key: &[u8],
        algorithm: Option<Algorithm>,
    ) -> Result<bool> {
        let algo = algorithm.unwrap_or(self.default_algorithm.clone());

        // Use a small test message for verification
        let test_message = b"KEYPAIR_VERIFICATION_TEST";

        // Encrypt test message with public key (returns encrypted_data and kem_ciphertext)
        let (encrypted_test, kem_ciphertext) = match self
            .encrypt_with_algorithm(test_message, public_key, algo.clone())
            .await
        {
            Ok(result) => result,
            Err(e) => {
                warn!("Keypair verification failed during encryption: {}", e);
                return Ok(false);
            }
        };

        // Attempt to decrypt with private key (using new signature)
        match self
            .decrypt_with_algorithm(
                &encrypted_test,
                kem_ciphertext.as_deref(),
                private_key,
                algo,
            )
            .await
        {
            Ok(decrypted) => {
                // Verify decrypted message matches original
                let matches = decrypted == test_message;
                if matches {
                    debug!("Keypair verification successful");
                } else {
                    warn!("Keypair verification failed: decrypted message doesn't match");
                }
                Ok(matches)
            }
            Err(e) => {
                debug!("Keypair verification failed during decryption: {}", e);
                Ok(false)
            }
        }
    }

    // Private helper methods

    /// Encrypt data using real KEM + AES-GCM
    /// Returns: (encrypted_data, kem_ciphertext)
    async fn encrypt_with_algorithm(
        &self,
        data: &[u8],
        public_key: &[u8],
        algorithm: Algorithm,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
        #[cfg(feature = "quantum")]
        {
            // Real KEM encryption
            let kem = self.create_kem_instance(&algorithm)?;

            // Parse public key from bytes
            let pk = kem
                .public_key_from_bytes(public_key)
                .ok_or_else(|| anyhow::anyhow!("Failed to parse public key from bytes"))?;

            // Encapsulate: Generate shared secret and KEM ciphertext
            let (kem_ciphertext, shared_secret) = kem
                .encapsulate(&pk)
                .map_err(|e| anyhow::anyhow!("KEM encapsulation failed: {}", e))?;

            // Derive AES key from shared secret (use first 32 bytes)
            let shared_secret_bytes = shared_secret.as_ref();
            let aes_key: Vec<u8> = if shared_secret_bytes.len() >= 32 {
                shared_secret_bytes[..32].to_vec()
            } else {
                // If shared secret is shorter, pad with hash
                let mut key = shared_secret_bytes.to_vec();
                key.extend_from_slice(
                    &blake3::hash(shared_secret_bytes).as_bytes()[..32 - shared_secret_bytes.len()],
                );
                key
            };

            // Encrypt data with AES-256-GCM
            let cipher = Aes256Gcm::new_from_slice(&aes_key)
                .map_err(|e| anyhow::anyhow!("Failed to create AES cipher: {}", e))?;
            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

            let encrypted_data = cipher
                .encrypt(&nonce, data)
                .map_err(|e| anyhow::anyhow!("AES encryption failed: {}", e))?;

            // Prepend nonce to encrypted data
            let mut final_encrypted = nonce.to_vec();
            final_encrypted.extend_from_slice(&encrypted_data);

            info!(
                "Data encrypted with real KEM: {} bytes -> {} bytes (KEM ciphertext: {} bytes)",
                data.len(),
                final_encrypted.len(),
                kem_ciphertext.as_ref().len()
            );

            Ok((final_encrypted, Some(kem_ciphertext.into_vec())))
        }

        #[cfg(not(feature = "quantum"))]
        {
            // Fallback: Placeholder XOR encryption
            warn!("Quantum feature not enabled - using placeholder encryption");
            let mut encrypted = data.to_vec();
            for (i, byte) in encrypted.iter_mut().enumerate() {
                *byte ^= public_key[i % public_key.len()];
            }
            Ok((encrypted, None))
        }
    }

    /// Decrypt data using real KEM + AES-GCM
    async fn decrypt_with_algorithm(
        &self,
        encrypted_data: &[u8],
        kem_ciphertext: Option<&[u8]>,
        private_key: &[u8],
        algorithm: Algorithm,
    ) -> Result<Vec<u8>> {
        #[cfg(feature = "quantum")]
        {
            // Real KEM decryption
            let kem_ciphertext = kem_ciphertext.ok_or_else(|| {
                anyhow::anyhow!("KEM ciphertext required for decryption (real KEM mode)")
            })?;

            let kem = self.create_kem_instance(&algorithm)?;

            // Parse secret key from bytes
            let sk = kem
                .secret_key_from_bytes(private_key)
                .ok_or_else(|| anyhow::anyhow!("Failed to parse secret key from bytes"))?;

            // Parse KEM ciphertext from bytes
            let kem_ct = kem
                .ciphertext_from_bytes(kem_ciphertext)
                .ok_or_else(|| anyhow::anyhow!("Failed to parse KEM ciphertext from bytes"))?;

            // Decapsulate: Recover shared secret
            let shared_secret = kem
                .decapsulate(&sk, &kem_ct)
                .map_err(|e| anyhow::anyhow!("KEM decapsulation failed: {}", e))?;

            // Derive AES key from shared secret
            let shared_secret_bytes = shared_secret.as_ref();
            let aes_key: Vec<u8> = if shared_secret_bytes.len() >= 32 {
                shared_secret_bytes[..32].to_vec()
            } else {
                // If shared secret is shorter, pad with hash
                let mut key = shared_secret_bytes.to_vec();
                key.extend_from_slice(
                    &blake3::hash(shared_secret_bytes).as_bytes()[..32 - shared_secret_bytes.len()],
                );
                key
            };

            // Extract nonce (first 12 bytes) and ciphertext
            if encrypted_data.len() < 12 {
                return Err(anyhow::anyhow!("Encrypted data too short (missing nonce)"));
            }
            let nonce = Nonce::from_slice(&encrypted_data[..12]);
            let ciphertext = &encrypted_data[12..];

            // Decrypt with AES-256-GCM
            let cipher = Aes256Gcm::new_from_slice(&aes_key)
                .map_err(|e| anyhow::anyhow!("Failed to create AES cipher: {}", e))?;

            let decrypted_data = cipher
                .decrypt(nonce, ciphertext)
                .map_err(|e| anyhow::anyhow!("AES decryption failed: {}", e))?;

            debug!(
                "Data decrypted with real KEM: {} bytes -> {} bytes",
                encrypted_data.len(),
                decrypted_data.len()
            );

            Ok(decrypted_data)
        }

        #[cfg(not(feature = "quantum"))]
        {
            // Fallback: Placeholder XOR decryption
            warn!("Quantum feature not enabled - using placeholder decryption");
            let mut decrypted = encrypted_data.to_vec();
            for (i, byte) in decrypted.iter_mut().enumerate() {
                *byte ^= private_key[i % private_key.len()];
            }
            Ok(decrypted)
        }
    }

    pub fn parse_algorithm(&self, algorithm_str: &str) -> Result<Algorithm> {
        let a = algorithm_str.trim();
        let a = a.strip_prefix("Algorithm::").unwrap_or(a);
        match a {
            "Kyber512" | "kyber512" => Ok(Algorithm::Kyber512),
            "Kyber768" | "kyber768" => Ok(Algorithm::Kyber768),
            "Kyber1024" | "kyber1024" => Ok(Algorithm::Kyber1024),
            "NtruPrimeSntrup761" | "ntruprimesntrup761" | "ntruprime" | "sntrup761" => {
                Ok(Algorithm::NtruPrimeSntrup761)
            }
            "FrodoKem1344Aes" | "frodokem1344aes" | "frodoaes" | "frodokem1344_aes" => {
                Ok(Algorithm::FrodoKem1344Aes)
            }
            "FrodoKem1344Shake" | "frodokem1344shake" | "frodoshake" | "frodokem1344_shake" => {
                Ok(Algorithm::FrodoKem1344Shake)
            }
            "ClassicMcEliece348864" | "classicmceliece348864" | "classicmceliece" => {
                Ok(Algorithm::ClassicMcEliece348864)
            }
            "BikeL1" | "bikel1" => Ok(Algorithm::BikeL1),
            "BikeL3" | "bikel3" => Ok(Algorithm::BikeL3),
            "BikeL5" | "bikel5" => Ok(Algorithm::BikeL5),
            _ => Err(anyhow::anyhow!("Unknown algorithm: {}", algorithm_str)),
        }
    }

    fn get_key_size(&self, algorithm: Algorithm) -> usize {
        match algorithm {
            Algorithm::Kyber512 => 800,
            Algorithm::Kyber768 => 1088,
            Algorithm::Kyber1024 => 1568,
            Algorithm::NtruPrimeSntrup761 => 1158,
            _ => 1024, // Default
        }
    }

    fn get_nonce_size(&self, cipher: CipherSuite) -> usize {
        match cipher {
            CipherSuite::AES256 => 12, // GCM nonce
            CipherSuite::ChaCha20 => 12,
            CipherSuite::XChaCha20 => 24,
        }
    }

    /// Verify quantum-safe signature
    pub async fn verify_signature(
        &self,
        message: &[u8],
        signature: &spacekit_primitives::v1::crypto::quantum::SPHINCSSignature,
        _author: &spacekit_primitives::v1::identity::QuantumDID,
    ) -> Result<bool> {
        info!(
            "Verifying SPHINCS+ signature for message ({} bytes)",
            message.len()
        );

        // Validate signature format
        if signature.signature_bytes.is_empty() {
            warn!("Empty signature provided");
            return Ok(false);
        }

        if signature.public_key.is_empty() {
            warn!("Empty public key in signature");
            return Ok(false);
        }

        // Verify the signature algorithm is quantum-safe
        if !self.is_quantum_safe_signature_algorithm(&signature.algorithm) {
            warn!(
                "Non-quantum-safe signature algorithm: {}",
                signature.algorithm
            );
            return Ok(false);
        }

        // Real SPHINCS+ verification is implemented in spacekit-primitives via pqcrypto-sphincsplus.
        let verification_result =
            spacekit_primitives::v1::crypto::quantum::verify_sphincs_signature(message, signature)?;

        debug!("Signature verification result: {}", verification_result);
        Ok(verification_result)
    }

    // Private signature verification helpers

    fn is_quantum_safe_signature_algorithm(&self, algorithm: &str) -> bool {
        matches!(
            algorithm,
            "SPHINCS-128f"
                | "SPHINCS-128s"
                | "SPHINCS-192f"
                | "SPHINCS-192s"
                | "SPHINCS-256f"
                | "SPHINCS-256s"
                | "SPHINCS+"
                | "Dilithium2"
                | "Dilithium3"
                | "Dilithium5"
                | "Falcon-512"
                | "Falcon-1024"
        )
    }

    // Note: placeholder verification removed in favor of real verification in spacekit-primitives.
}
