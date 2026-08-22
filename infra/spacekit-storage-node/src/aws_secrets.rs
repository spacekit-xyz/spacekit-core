//! AWS Secrets Manager integration for quantum KEM key storage
//!
//! This module provides secure storage and retrieval of quantum-resistant encryption keys
//! from AWS Secrets Manager, enabling production deployments with centralized key management.

use anyhow::Result;
use chrono;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tracing::{debug, info, warn};

#[cfg(feature = "aws-secrets")]
use aws_config::BehaviorVersion;
#[cfg(feature = "aws-secrets")]
use aws_sdk_secretsmanager::Client as SecretsManagerClient;
#[cfg(feature = "aws-secrets")]
use base64::{engine::general_purpose, Engine as _};

/// Quantum KEM keypair stored in AWS Secrets Manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuantumKeypair {
    /// Public key (base64 or hex)
    pub public_key: String,
    /// Private / secret key (base64 or hex). JSON field may be `private_key` or `secret_key`.
    #[serde(alias = "secret_key")]
    pub private_key: String,
    /// Algorithm used (e.g., "Kyber1024", "Kyber768"). Defaults to empty → caller may use Kyber1024.
    #[serde(default)]
    pub algorithm: String,
    /// Key ID or identifier
    pub key_id: Option<String>,
    /// Timestamp when key was created
    pub created_at: Option<String>,
}

/// AWS Secrets Manager key manager
#[cfg(feature = "aws-secrets")]
pub struct AwsKeyManager {
    client: SecretsManagerClient,
}

#[cfg(feature = "aws-secrets")]
impl AwsKeyManager {
    /// Create a new AWS key manager
    pub async fn new() -> Result<Self> {
        let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
        let client = SecretsManagerClient::new(&config);

        info!("AWS Secrets Manager client initialized");
        Ok(Self { client })
    }

    /// Get or create quantum keypair from AWS Secrets Manager
    pub async fn get_or_create_keypair(
        &self,
        secret_name: &str,
        algorithm: &str,
        key_id: &str,
    ) -> Result<QuantumKeypair> {
        // Try to retrieve existing keypair
        match self.get_keypair(secret_name).await {
            Ok(keypair) => {
                info!(
                    "Retrieved existing quantum keypair from AWS Secrets Manager: {}",
                    secret_name
                );
                return Ok(keypair);
            }
            Err(e) => {
                warn!("Failed to retrieve keypair from AWS Secrets Manager: {}. Will create new keypair.", e);
            }
        }

        // Create new keypair (caller should generate the actual keys)
        info!(
            "Creating new quantum keypair in AWS Secrets Manager: {}",
            secret_name
        );
        Err(anyhow::anyhow!(
            "Keypair generation must be done by caller with actual quantum crypto"
        ))
    }

    /// Retrieve quantum keypair from AWS Secrets Manager
    /// Logs access for security auditing
    pub async fn get_keypair(&self, secret_name: &str) -> Result<QuantumKeypair> {
        debug!(
            "Retrieving quantum keypair from AWS Secrets Manager: {}",
            secret_name
        );

        // Log key access for security auditing
        self.log_key_access(secret_name, "GetSecretValue").await?;

        let resp = self
            .client
            .get_secret_value()
            .secret_id(secret_name)
            .send()
            .await
            .map_err(|e| {
                warn!(
                    "SECURITY_AUDIT: Failed key access - Secret: {}, Error: {}",
                    secret_name, e
                );
                anyhow::anyhow!("Failed to get secret from AWS: {}", e)
            })?;

        let secret_string = resp
            .secret_string()
            .ok_or_else(|| anyhow::anyhow!("Secret value not found in AWS response"))?;

        let keypair: QuantumKeypair = serde_json::from_str(secret_string)
            .map_err(|e| anyhow::anyhow!("Failed to parse keypair JSON: {}", e))?;

        info!(
            "Successfully retrieved quantum keypair (algorithm: {})",
            keypair.algorithm
        );
        info!(
            "SECURITY_AUDIT: Key retrieved successfully - Secret: {}",
            secret_name
        );
        Ok(keypair)
    }

    /// Store quantum keypair in AWS Secrets Manager
    /// Optionally uses AWS KMS for additional encryption
    pub async fn store_keypair(
        &self,
        secret_name: &str,
        keypair: &QuantumKeypair,
        description: Option<&str>,
    ) -> Result<()> {
        debug!(
            "Storing quantum keypair in AWS Secrets Manager: {}",
            secret_name
        );

        let secret_value = serde_json::to_string(keypair)
            .map_err(|e| anyhow::anyhow!("Failed to serialize keypair: {}", e))?;

        // Check for KMS key ID from environment (for additional encryption)
        let kms_key_id = std::env::var("AWS_KMS_KEY_ID").ok();

        let mut create_request = self
            .client
            .create_secret()
            .name(secret_name)
            .description(
                description
                    .unwrap_or("Quantum-resistant encryption keypair for SpaceKit Storage Node"),
            )
            .secret_string(&secret_value);

        // Add KMS encryption if configured
        if let Some(kms_key) = &kms_key_id {
            create_request = create_request.kms_key_id(kms_key);
            info!("Using AWS KMS encryption for secret: {}", kms_key);
        }

        // Try to create secret, or update if it already exists
        match create_request.send().await {
            Ok(_) => {
                info!(
                    "Successfully created quantum keypair in AWS Secrets Manager: {}",
                    secret_name
                );
                if kms_key_id.is_some() {
                    info!("✅ Secret encrypted with AWS KMS");
                }
                Ok(())
            }
            Err(e) => {
                // If secret already exists, update it
                if e.to_string().contains("ResourceExistsException") {
                    warn!("Secret already exists, updating: {}", secret_name);
                    let mut update_request = self
                        .client
                        .update_secret()
                        .secret_id(secret_name)
                        .secret_string(&secret_value);

                    // Add KMS encryption if configured
                    if let Some(kms_key) = &kms_key_id {
                        update_request = update_request.kms_key_id(kms_key);
                    }

                    update_request
                        .send()
                        .await
                        .map_err(|e| anyhow::anyhow!("Failed to update secret: {}", e))?;
                    info!(
                        "Successfully updated quantum keypair in AWS Secrets Manager: {}",
                        secret_name
                    );
                    if kms_key_id.is_some() {
                        info!("✅ Secret encrypted with AWS KMS");
                    }
                    Ok(())
                } else {
                    Err(anyhow::anyhow!("Failed to store secret: {}", e))
                }
            }
        }
    }

    /// Log security event for audit purposes
    pub async fn log_key_access(&self, secret_name: &str, operation: &str) -> Result<()> {
        // Log key access for security auditing
        // In production, this should integrate with CloudTrail/CloudWatch
        info!(
            "SECURITY_AUDIT: Key access - Secret: {}, Operation: {}, Timestamp: {}",
            secret_name,
            operation,
            chrono::Utc::now().to_rfc3339()
        );
        Ok(())
    }
}

/// Key storage backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyStorageBackend {
    /// Local file storage (development)
    Local,
    /// AWS Secrets Manager (production)
    AwsSecrets,
}

impl KeyStorageBackend {
    /// Database master KEM keys only: use AWS when `DATABASE_KEM_SECRET_NAME` is set.
    ///
    /// **Do not** tie this to `QUANTUM_KEYPAIR_SECRET_NAME`. The PQ **server** secret is often
    /// pqcrypto-kyber (browser-compatible); the DB uses OQS KEM bytes and cannot parse those keys.
    pub fn from_env() -> Self {
        let db_secret = std::env::var("DATABASE_KEM_SECRET_NAME")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if db_secret.is_some() {
            Self::AwsSecrets
        } else {
            Self::Local
        }
    }
}

/// Helper function to encode keys to base64
#[cfg(feature = "aws-secrets")]
pub fn encode_key_to_base64(key: &[u8]) -> String {
    general_purpose::STANDARD.encode(key)
}

/// Helper function to decode keys from base64
#[cfg(feature = "aws-secrets")]
pub fn decode_key_from_base64(encoded: &str) -> Result<Vec<u8>> {
    general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| anyhow::anyhow!("Failed to decode base64 key: {}", e))
}

/// Key material from Secrets Manager JSON: hex or standard base64.
/// Hex strings are all [0-9a-fA-F] and happen to be valid base64, so we must
/// check for hex first to avoid base64-decoding a hex string into garbage.
#[cfg(feature = "aws-secrets")]
pub fn decode_key_material(encoded: &str) -> Result<Vec<u8>> {
    let s = encoded.trim();
    if s.is_empty() {
        return Err(anyhow::anyhow!("key material is empty"));
    }
    let is_hex = s.len() >= 2 && s.len() % 2 == 0 && s.bytes().all(|b| b.is_ascii_hexdigit());
    if is_hex {
        if let Ok(bytes) = hex::decode(s) {
            if !bytes.is_empty() {
                return Ok(bytes);
            }
        }
    }
    match decode_key_from_base64(s) {
        Ok(b) if !b.is_empty() => Ok(b),
        _ => Err(anyhow::anyhow!(
            "key material is not valid hex or base64 (len={})",
            s.len()
        )),
    }
}

/// Fallback implementation when AWS Secrets feature is not enabled
#[cfg(not(feature = "aws-secrets"))]
pub struct AwsKeyManager;

#[cfg(not(feature = "aws-secrets"))]
impl AwsKeyManager {
    pub async fn new() -> Result<Self> {
        Err(anyhow::anyhow!("AWS Secrets Manager feature is not enabled. Enable 'aws-secrets' feature to use AWS key storage."))
    }

    pub async fn get_keypair(&self, _secret_name: &str) -> Result<QuantumKeypair> {
        Err(anyhow::anyhow!(
            "AWS Secrets Manager feature is not enabled"
        ))
    }

    pub async fn store_keypair(
        &self,
        _secret_name: &str,
        _keypair: &QuantumKeypair,
        _description: Option<&str>,
    ) -> Result<()> {
        Err(anyhow::anyhow!(
            "AWS Secrets Manager feature is not enabled"
        ))
    }
}
