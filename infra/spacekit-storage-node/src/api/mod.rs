//! HTTP API server for the storage node
//!

pub mod agentic_routes;
pub mod content_routes;
mod envelope_delivery;
pub mod keymaster_routes;
pub mod spkg_routes;

use crate::database::{
    ContactMessage, Database, DocumentRecord, EncryptedMessage, EncryptedUser, FeedSubscription,
    FileMetadata, GlobalGroup, GlobalUser, GroupMembership, Server, ServerMembership, User,
};
use crate::envelope::{self, ChallengeResponse, EncryptedChallenge, PendingChallenge};
use crate::models::{EncryptedUserResponse, UserResponse};
use crate::server_routing::ServerRoutingManager;
use crate::{EncryptedData, QuantumCrypto};
use spacekit_primitives::v1::fact::{FactContent, FactPackage};
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use bytes::Bytes;
use chrono;
use serde::{Deserialize, Serialize};
use serde_json;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;
use warp::{http::Method, reject::Rejection, Filter, Reply};

// ---- Production hardening defaults ----
// For public internet exposure, keep limits conservative and tune as needed.
// Larger deployments should front this service with a reverse proxy/WAF.
const MAX_JSON_BODY_BYTES: u64 = 1024 * 1024; // 1 MiB
/// Raw file upload (`POST /files/upload`) — allow large WASM + model / brain `.bin` bundles.
const MAX_UPLOAD_BODY_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB
const MAX_FILE_REQUEST_TIMEOUT_MS: u64 = 30_000; // 30s (downloads, session-key, etc.)
/// Uploads can stream large payloads; keep separate from quick JSON/API paths.
const MAX_FILE_UPLOAD_TIMEOUT_MS: u64 = 30 * 60 * 1000; // 30 min
const MAX_CONCURRENT_REQUESTS: usize = 100;

/// Reject the legacy full-file decrypt/re-encrypt path for large on-disk blobs.
#[cfg(feature = "quantum")]
fn legacy_full_buffer_too_large_reply(file_id: &str, size: u64) -> Box<dyn Reply> {
    boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "error": format!(
                "File {} ({} bytes) exceeds the {}-byte legacy full-buffer limit. \
        Bounded streaming delivery failed; peel nested envelopes or re-upload as a single-layer PQ envelope.",
                file_id,
                size,
                envelope_delivery::MAX_LEGACY_FULL_BUFFER_BYTES
            )
        })),
        warp::http::StatusCode::UNPROCESSABLE_ENTITY,
    ))
}

/// Session keypair for secure private key transmission (LEGACY — prefer envelope + challenge auth)
#[derive(Debug, Clone)]
struct SessionKeypair {
    public_key: Vec<u8>,
    private_key: Vec<u8>,
    created_at: SystemTime,
    expires_at: SystemTime,
}

/// Maximum time a streaming download can take (60 min for large files).
const MAX_STREAM_TIMEOUT_MS: u64 = 60 * 60 * 1000;

/// Rate limiter for query endpoints
#[derive(Debug, Clone)]
struct RateLimiter {
    mode: RateLimitMode,
    max_requests: usize,
    window_seconds: u64,
}

#[derive(Debug, Clone)]
enum RateLimitMode {
    InMemory {
        requests: Arc<RwLock<HashMap<String, Vec<Instant>>>>,
    },
    #[cfg(feature = "rate-limit-spacekit")]
    SpacekitHttp {
        base_url: String,
        prefix: String,
        client: reqwest::Client,
    },
}

/// Authentication errors
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Missing authorization header")]
    MissingHeader,
    #[error("Invalid authorization format")]
    InvalidFormat,
    #[error("Invalid DID format")]
    InvalidDid,
}

/// Rate limit errors
#[derive(Debug, Error)]
pub enum RateLimitError {
    #[error("Rate limit exceeded")]
    Exceeded,
}

impl warp::reject::Reject for AuthError {}
impl warp::reject::Reject for RateLimitError {}

impl RateLimiter {
    fn new(max_requests: usize, window_seconds: u64) -> Self {
        Self {
            mode: RateLimitMode::InMemory {
                requests: Arc::new(RwLock::new(HashMap::new())),
            },
            max_requests,
            window_seconds,
        }
    }

    #[cfg(feature = "rate-limit-spacekit")]
    fn new_spacekit(
        max_requests: usize,
        window_seconds: u64,
        base_url: &str,
        prefix: &str,
    ) -> Result<Self> {
        Ok(Self {
            mode: RateLimitMode::SpacekitHttp {
                base_url: base_url.trim_end_matches('/').to_string(),
                prefix: prefix.to_string(),
                client: reqwest::Client::new(),
            },
            max_requests,
            window_seconds,
        })
    }

    async fn check_rate_limit(&self, key: &str) -> Result<(), RateLimitError> {
        match &self.mode {
            RateLimitMode::InMemory { requests } => {
                let mut requests = requests.write().await;
                let now = Instant::now();
                let window_start = now - Duration::from_secs(self.window_seconds);

                // Clean old requests
                let user_requests = requests.entry(key.to_string()).or_insert_with(Vec::new);
                user_requests.retain(|&time| time > window_start);

                // Check limit
                if user_requests.len() >= self.max_requests {
                    return Err(RateLimitError::Exceeded);
                }

                user_requests.push(now);
                Ok(())
            }
            #[cfg(feature = "rate-limit-spacekit")]
            RateLimitMode::SpacekitHttp {
                base_url,
                prefix,
                client,
            } => {
                #[derive(Serialize)]
                struct RateLimitCheckRequest<'a> {
                    key: &'a str,
                    prefix: &'a str,
                    max_requests: usize,
                    window_seconds: u64,
                }
                #[derive(Deserialize)]
                struct RateLimitCheckResponse {
                    allowed: bool,
                }

                let url = format!("{}/service/rate_limit/check", base_url);
                let req = RateLimitCheckRequest {
                    key,
                    prefix,
                    max_requests: self.max_requests,
                    window_seconds: self.window_seconds,
                };

                let resp = client
                    .post(url)
                    .json(&req)
                    .send()
                    .await
                    .map_err(|_| RateLimitError::Exceeded)?;

                if !resp.status().is_success() {
                    return Err(RateLimitError::Exceeded);
                }

                let parsed: RateLimitCheckResponse =
                    resp.json().await.map_err(|_| RateLimitError::Exceeded)?;
                if parsed.allowed {
                    Ok(())
                } else {
                    Err(RateLimitError::Exceeded)
                }
            }
        }
    }
}

fn build_rate_limiter(max_requests: usize, window_seconds: u64, prefix: &str) -> Arc<RateLimiter> {
    #[cfg(feature = "rate-limit-spacekit")]
    {
        if let Ok(base_url) = std::env::var("SPACEKIT_RATE_LIMIT_URL") {
            match RateLimiter::new_spacekit(max_requests, window_seconds, &base_url, prefix) {
                Ok(limiter) => {
                    tracing::info!("Using SpaceKit distributed rate limiting for {}", prefix);
                    return Arc::new(limiter);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to init SpaceKit rate limiter ({}); falling back to in-memory",
                        e
                    );
                }
            }
        }
    }

    Arc::new(RateLimiter::new(max_requests, window_seconds))
}

pub use crate::envelope::KeySource;

/// Persistent server Kyber keypair for the PQ encryption protocol.
/// Files are encrypted *to* this key on upload; the server decrypts and
/// re-encrypts to each requester's key on download.
#[derive(Debug, Clone)]
pub struct ServerKeypair {
    pub public_key: Vec<u8>,
    pub secret_key: Vec<u8>,
    pub algorithm: String,
    /// Which library produced this keypair. Defaults to `Oqs` for legacy keys.
    pub key_source: KeySource,
}

/// API Server for the storage node
pub struct ApiServer {
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    blob_fact_auth_mode: crate::access_policy::BlobFactAuthMode,
    quantum_crypto: Option<Arc<QuantumCrypto>>,
    server_keypair: Option<Arc<ServerKeypair>>,
    session_keypairs: Arc<RwLock<HashMap<String, SessionKeypair>>>,
    pending_challenges: Arc<RwLock<HashMap<String, PendingChallenge>>>,
    query_builder: Option<Arc<crate::sql_query::StorageQueryBuilder>>,
    rate_limiter: Arc<RateLimiter>,
    ip_rate_limiter: Arc<RateLimiter>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
    server_routing: Option<Arc<ServerRoutingManager>>,
    /// Phase 0/1/3/4 facade. When `Some`, the agentic routes are mounted.
    facade: Option<Arc<crate::storage_facade::Facade>>,
    /// Optional live handles for `GET /api/agentic/memory`.
    memory_route_state: Option<crate::api::agentic_routes::AgenticMemoryRouteState>,
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub public_key: String,
    pub enable_cors: bool,
    /// CAS blob/fact auth: `permissive` (default), `strict`, or `hybrid`.
    #[serde(default)]
    pub blob_fact_auth_mode: crate::access_policy::BlobFactAuthMode,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 3030,
            public_key: String::new(),
            enable_cors: true,
            blob_fact_auth_mode: crate::access_policy::BlobFactAuthMode::from_env(),
        }
    }
}

impl ApiServer {
    /// Create a new API server with database only (backward compatible)
    pub fn new(db: Arc<Database>) -> Self {
        let query_builder = Arc::new(crate::sql_query::StorageQueryBuilder::new(db.clone()));
        let rate_limiter = build_rate_limiter(100, 60, "rate:did");
        let ip_rate_limiter = build_rate_limiter(60, 60, "rate:ip");
        let request_semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REQUESTS));
        Self {
            db,
            data_dir: None,
            blob_fact_auth_mode: crate::access_policy::BlobFactAuthMode::from_env(),
            quantum_crypto: None,
            server_keypair: None,
            session_keypairs: Arc::new(RwLock::new(HashMap::new())),
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
            query_builder: Some(query_builder),
            rate_limiter,
            ip_rate_limiter,
            request_semaphore,
            server_routing: None,
            facade: None,
            memory_route_state: None,
        }
    }

    /// Create a new API server with file retrieval capabilities
    pub fn new_with_file_access(
        db: Arc<Database>,
        data_dir: PathBuf,
        quantum_crypto: Arc<QuantumCrypto>,
    ) -> Self {
        let query_builder = Arc::new(crate::sql_query::StorageQueryBuilder::new(db.clone()));
        let rate_limiter = build_rate_limiter(100, 60, "rate:did");
        let ip_rate_limiter = build_rate_limiter(60, 60, "rate:ip");
        let request_semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REQUESTS));
        Self {
            db,
            data_dir: Some(data_dir),
            blob_fact_auth_mode: crate::access_policy::BlobFactAuthMode::from_env(),
            quantum_crypto: Some(quantum_crypto),
            server_keypair: None,
            session_keypairs: Arc::new(RwLock::new(HashMap::new())),
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
            query_builder: Some(query_builder),
            rate_limiter,
            ip_rate_limiter,
            request_semaphore,
            server_routing: None,
            facade: None,
            memory_route_state: None,
        }
    }

    /// Create a new API server with file access and cross-server routing
    pub fn new_with_file_access_and_routing(
        db: Arc<Database>,
        data_dir: PathBuf,
        quantum_crypto: Arc<QuantumCrypto>,
        server_routing: Arc<ServerRoutingManager>,
    ) -> Self {
        let query_builder = Arc::new(crate::sql_query::StorageQueryBuilder::new(db.clone()));
        let rate_limiter = build_rate_limiter(100, 60, "rate:did");
        let ip_rate_limiter = build_rate_limiter(60, 60, "rate:ip");
        let request_semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_REQUESTS));
        Self {
            db,
            data_dir: Some(data_dir),
            blob_fact_auth_mode: crate::access_policy::BlobFactAuthMode::from_env(),
            quantum_crypto: Some(quantum_crypto),
            server_keypair: None,
            session_keypairs: Arc::new(RwLock::new(HashMap::new())),
            pending_challenges: Arc::new(RwLock::new(HashMap::new())),
            query_builder: Some(query_builder),
            rate_limiter,
            ip_rate_limiter,
            request_semaphore,
            server_routing: Some(server_routing),
            facade: None,
            memory_route_state: None,
        }
    }

    pub fn with_blob_fact_auth_mode(
        mut self,
        mode: crate::access_policy::BlobFactAuthMode,
    ) -> Self {
        self.blob_fact_auth_mode = mode;
        self
    }

    /// Wire the [`crate::storage_facade::Facade`] into the API server. When
    /// set, the agentic routes (`/api/transactions/*`, `/api/sandboxes/*`,
    /// `/api/changes`) are mounted alongside the legacy routes.
    pub fn with_facade(mut self, facade: Arc<crate::storage_facade::Facade>) -> Self {
        self.facade = Some(facade);
        self
    }

    /// Wire live memory diagnostic handles for `GET /api/agentic/memory`.
    pub fn with_memory_route_state(
        mut self,
        state: crate::api::agentic_routes::AgenticMemoryRouteState,
    ) -> Self {
        self.memory_route_state = Some(state);
        self
    }

    /// Initialize the server Kyber keypair (order):
    ///
    /// ## Entitlement trust roots (for `/files/{id}/rewrap`)
    ///
    /// - `SPACEKIT_COMPUTE_NODE_URL` — base URL of the compute node (e.g. `http://localhost:8080`)
    /// - `SPACEKIT_ENTITLEMENT_CONTRACT_ID` — hex address of the deployed entitlement-ledger contract
    /// - `SPACEKIT_ENTITLEMENT_NETWORK` — `local` / `testnet` / `mainnet` (informational)
    ///
    /// ## Server keypair init order
    ///
    /// 1. **AWS Secrets Manager (mandatory if configured)** — when `QUANTUM_KEYPAIR_SECRET_NAME` is
    ///    non-empty, the keypair is loaded **only** from that secret; there is **no** fallback to
    ///    KeyMaster or disk (single source of truth). Requires a build with `aws-secrets`.
    ///    SecretString JSON: [`crate::aws_secrets::QuantumKeypair`] (`public_key`, `private_key` **or**
    ///    `secret_key`, optional `algorithm`). Values may be base64 or hex.
    /// 2. **KeyMaster** — when `SPACEKIT_COMPUTE_URL` + `SPACEKIT_NODE_DID` are set (only if SM env unset).
    /// 3. **Encrypted local file** — `data_dir/server_keypair.enc` (keyed by `SPACEKIT_NODE_DID`).
    /// 4. **Legacy plaintext** — `data_dir/server_keypair.json` (migrates to `.enc` when DID is set).
    /// 5. **Generate** a new keypair if none of the above apply.
    pub async fn init_server_keypair(&mut self) -> Result<()> {
        let data_dir = match &self.data_dir {
            Some(d) => d.clone(),
            None => {
                tracing::warn!("No data_dir configured; skipping server keypair init");
                return Ok(());
            }
        };
        let qc = match &self.quantum_crypto {
            Some(qc) => qc.clone(),
            None => {
                tracing::warn!("No quantum_crypto configured; skipping server keypair init");
                return Ok(());
            }
        };

        let enc_path = data_dir.join("server_keypair.enc");
        let legacy_path = data_dir.join("server_keypair.json");
        let node_did = std::env::var("SPACEKIT_NODE_DID").unwrap_or_default();
        let compute_url = std::env::var("SPACEKIT_COMPUTE_URL").unwrap_or_default();

        let quantum_sm_secret = std::env::var("QUANTUM_KEYPAIR_SECRET_NAME")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        // ── Mandatory AWS Secrets Manager PQ server key (no disk fallback) ──
        if let Some(ref secret_name) = quantum_sm_secret {
            #[cfg(not(feature = "aws-secrets"))]
            {
                return Err(anyhow::anyhow!(
                    "QUANTUM_KEYPAIR_SECRET_NAME is set ({}) but this binary was built without `aws-secrets`. \
Rebuild (e.g. `./build-docker-aws.sh`) so the storage node can load the server Kyber keypair only from Secrets Manager.",
                    secret_name
                ));
            }
            #[cfg(feature = "aws-secrets")]
            {
                let mgr = crate::aws_secrets::AwsKeyManager::new()
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!("AWS Secrets Manager client init failed: {}", e)
                    })?;
                let qp = mgr
                    .get_keypair(secret_name)
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "QUANTUM_KEYPAIR_SECRET_NAME={}: failed to load server keypair (no fallback to disk): {}",
                            secret_name,
                            e
                        )
                    })?;
                let pk = crate::aws_secrets::decode_key_material(&qp.public_key)
                    .map_err(|e| anyhow::anyhow!("secrets_manager.public_key: {}", e))?;
                let sk = crate::aws_secrets::decode_key_material(&qp.private_key).map_err(|e| {
                    anyhow::anyhow!("secrets_manager.private_key/secret_key: {}", e)
                })?;
                let algo = if qp.algorithm.trim().is_empty() {
                    "Kyber1024".to_string()
                } else {
                    qp.algorithm.clone()
                };
                if legacy_path.exists() || enc_path.exists() {
                    tracing::warn!(
                        "QUANTUM_KEYPAIR_SECRET_NAME is authoritative — ignoring on-disk server_keypair.enc / server_keypair.json (remove them to avoid confusion)"
                    );
                }
                let pk_fingerprint = hex::encode(&blake3::hash(&pk).as_bytes()[..8]);
                tracing::info!(
                    "Loaded server keypair from AWS Secrets Manager only ({}, pk {} bytes, sk {} bytes, pk_fingerprint={}) secret={}",
                    algo,
                    pk.len(),
                    sk.len(),
                    pk_fingerprint,
                    secret_name
                );
                // Sanity-check: KEM round-trip with pqcrypto (the library used at runtime)
                #[cfg(feature = "quantum")]
                {
                    let test_payload = b"keypair-sanity-check";
                    match crate::envelope::pqcrypto_kem_encrypt_bytes(test_payload, &pk) {
                        Ok(encrypted) => {
                            match crate::envelope::pqcrypto_kem_decrypt_bytes(&encrypted, &sk) {
                                Ok(decrypted) if decrypted == test_payload => {
                                    tracing::info!("Server keypair sanity check PASSED (pqcrypto) — pk/sk match");
                                }
                                Ok(decrypted) => {
                                    tracing::error!(
                                        "Server keypair sanity check FAILED — decrypted {} bytes but expected {}: DATA MISMATCH",
                                        decrypted.len(), test_payload.len()
                                    );
                                }
                                Err(e) => {
                                    tracing::error!("Server keypair sanity check FAILED — decrypt error: {} (pk/sk mismatch or corrupted key material)", e);
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!("Server keypair sanity check FAILED — encrypt error: {} (invalid public key?)", e);
                        }
                    }
                }
                // Use Pqcrypto (pure Rust) for Kyber1024 to avoid cross-platform
                // OQS vendored-C incompatibilities between macOS ARM and Linux x86_64.
                let source = if algo.to_ascii_lowercase().contains("kyber1024") {
                    KeySource::Pqcrypto
                } else {
                    KeySource::Oqs
                };
                self.server_keypair = Some(Arc::new(ServerKeypair {
                    public_key: pk,
                    secret_key: sk,
                    algorithm: algo,
                    key_source: source,
                }));
                return Ok(());
            }
        }

        // Helper: derive a 32-byte encryption key from the node DID
        let derive_local_key = |did: &str| -> [u8; 32] {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(b"spacekit-server-keypair-v1:");
            h.update(did.as_bytes());
            h.finalize().into()
        };

        // ── Tier 1: Try KeyMaster recovery (requires reqwest) ──
        #[cfg(feature = "reqwest")]
        if !compute_url.is_empty() && !node_did.is_empty() {
            let url = format!(
                "{}/v1/keymaster/register",
                compute_url.trim_end_matches('/')
            );
            let client = reqwest::Client::new();
            match client
                .post(&url)
                .json(&serde_json::json!({ "node_did": node_did }))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        if let (Some(pk_hex), Some(sk_hex)) = (
                            body["server_pk_hex"].as_str(),
                            body["server_sk_hex"].as_str(),
                        ) {
                            let algo = body["algorithm"]
                                .as_str()
                                .unwrap_or("Kyber1024")
                                .to_string();
                            let pk =
                                hex::decode(pk_hex).map_err(|e| anyhow::anyhow!("km pk: {}", e))?;
                            let sk =
                                hex::decode(sk_hex).map_err(|e| anyhow::anyhow!("km sk: {}", e))?;
                            tracing::info!(
                                "Recovered server keypair from KeyMaster ({}, pk {} bytes)",
                                algo,
                                pk.len()
                            );
                            self.server_keypair = Some(Arc::new(ServerKeypair {
                                public_key: pk,
                                secret_key: sk,
                                algorithm: algo,
                                key_source: KeySource::Oqs,
                            }));
                            return Ok(());
                        }
                    }
                }
                Ok(resp) => {
                    tracing::warn!(
                        "KeyMaster returned {}, falling back to local",
                        resp.status()
                    );
                }
                Err(e) => {
                    tracing::warn!("KeyMaster unreachable ({}), falling back to local", e);
                }
            }
        }

        // ── Tier 2: Encrypted local file ──
        if enc_path.exists() && !node_did.is_empty() {
            let blob = tokio::fs::read(&enc_path).await?;
            if blob.len() > 12 {
                let key = derive_local_key(&node_did);
                match decrypt_local_keypair(&blob, &key) {
                    Ok((pk, sk, algo)) => {
                        tracing::info!(
                            "Loaded server keypair from {} ({}, pk {} bytes)",
                            enc_path.display(),
                            algo,
                            pk.len()
                        );
                        self.server_keypair = Some(Arc::new(ServerKeypair {
                            public_key: pk,
                            secret_key: sk,
                            algorithm: algo,
                            key_source: KeySource::Oqs,
                        }));
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to decrypt {}: {}", enc_path.display(), e);
                    }
                }
            }
        }

        // ── Tier 3: Legacy plaintext file (migrate if found) ──
        if legacy_path.exists() {
            let json = tokio::fs::read_to_string(&legacy_path).await?;
            let parsed: serde_json::Value = serde_json::from_str(&json)?;
            let pk_hex = parsed["public_key"].as_str().unwrap_or("");
            let sk_hex = parsed["secret_key"].as_str().unwrap_or("");
            let algo = parsed["algorithm"]
                .as_str()
                .unwrap_or("Kyber1024")
                .to_string();
            let pk =
                hex::decode(pk_hex).map_err(|e| anyhow::anyhow!("bad server PK hex: {}", e))?;
            let sk =
                hex::decode(sk_hex).map_err(|e| anyhow::anyhow!("bad server SK hex: {}", e))?;
            tracing::info!(
                "Loaded server keypair from legacy {} ({}, pk {} bytes)",
                legacy_path.display(),
                algo,
                pk.len()
            );

            // Migrate to encrypted format if we have a DID
            if !node_did.is_empty() {
                let key = derive_local_key(&node_did);
                if let Ok(blob) = encrypt_local_keypair(pk_hex, sk_hex, &algo, &key) {
                    tokio::fs::create_dir_all(&data_dir).await?;
                    tokio::fs::write(&enc_path, blob).await?;
                    tokio::fs::remove_file(&legacy_path).await.ok();
                    tracing::info!(
                        "Migrated server keypair to encrypted format, removed plaintext"
                    );
                }
            }

            self.server_keypair = Some(Arc::new(ServerKeypair {
                public_key: pk,
                secret_key: sk,
                algorithm: algo,
                key_source: KeySource::Oqs,
            }));
            return Ok(());
        }

        // ── No existing keypair: generate new ──
        let algorithm = spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024;
        let (pk, sk) = qc.generate_keypair(algorithm).await?;
        let pk_hex = hex::encode(&pk);
        let sk_hex = hex::encode(&sk);

        tokio::fs::create_dir_all(&data_dir).await?;

        // Save encrypted locally
        if !node_did.is_empty() {
            let key = derive_local_key(&node_did);
            if let Ok(blob) = encrypt_local_keypair(&pk_hex, &sk_hex, "Kyber1024", &key) {
                tokio::fs::write(&enc_path, blob).await?;
                tracing::info!(
                    "Generated new server keypair, saved encrypted to {}",
                    enc_path.display()
                );
            }
        } else {
            // Fallback: save plaintext (no DID configured)
            let obj = serde_json::json!({
                "public_key": pk_hex,
                "secret_key": sk_hex,
                "algorithm": "Kyber1024",
            });
            tokio::fs::write(&legacy_path, serde_json::to_string_pretty(&obj)?).await?;
            tracing::warn!(
                "No SPACEKIT_NODE_DID set — saved server keypair as PLAINTEXT to {}",
                legacy_path.display()
            );
        }

        // Register with KeyMaster (requires reqwest)
        #[cfg(feature = "reqwest")]
        if !compute_url.is_empty() && !node_did.is_empty() {
            let url = format!(
                "{}/v1/keymaster/register",
                compute_url.trim_end_matches('/')
            );
            let client = reqwest::Client::new();
            match client
                .post(&url)
                .json(&serde_json::json!({
                    "node_did": node_did,
                    "server_pk_hex": pk_hex,
                    "server_sk_hex": sk_hex,
                    "algorithm": "Kyber1024",
                }))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!("Registered server keypair with KeyMaster");
                }
                Ok(resp) => {
                    tracing::warn!("KeyMaster registration returned {}", resp.status());
                }
                Err(e) => {
                    tracing::warn!("Could not register with KeyMaster: {}", e);
                }
            }
        }

        tracing::info!(
            "Generated new server keypair (Kyber1024, pk {} bytes)",
            pk.len()
        );
        self.server_keypair = Some(Arc::new(ServerKeypair {
            public_key: pk,
            secret_key: sk,
            algorithm: "Kyber1024".to_string(),
            key_source: KeySource::Oqs,
        }));
        Ok(())
    }

    /// Start the HTTP API server
    pub async fn start(&self, server_config: ServerConfig) -> Result<()> {
        let api_listen_port = server_config.port;
        let db = self.db.clone();
        let public_key = server_config.public_key.clone();
        let blob_fact_auth_mode = server_config.blob_fact_auth_mode;
        let ip_rate_limiter = self.ip_rate_limiter.clone();
        let request_semaphore = self.request_semaphore.clone();
        let server_routing = self.server_routing.clone();

        // CORS: browsers preflight uploads with custom headers (`owner-did`, `owner-public-key`, …). Warp 0.3 has no
        // `allow_any_header`; list every header the SPA may send on `/files/upload` and related routes.
        let cors = warp::cors()
            .max_age(30)
            .allow_any_origin()
            .allow_headers(vec![
                "content-type",
                "authorization",
                "owner-did",
                "owner-public-key",
                "owner-key-algorithm",
                "filename",
                "requester-did",
                "requester-public-key",
                "challenge-id",
                "challenge-response",
                "requester-did",
                "entitlement-id",
                "buyer-did",
                "buyer-public-key",
            ])
            .allow_methods(&[
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::OPTIONS,
            ]);

        // Route filters
        let did_route = warp::path!("did")
            .and(warp::get())
            .and(with_public_key(public_key.clone()))
            .and_then(handle_get_did);

        // User management routes
        let signup_route = warp::path!("service" / "signup")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "signup"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_public_key(public_key.clone()))
            .and(with_db(db.clone()))
            .and_then(handle_signup);

        let encrypted_signup_route = warp::path!("service" / "esignup")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "signup"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_public_key(public_key.clone()))
            .and(with_db(db.clone()))
            .and_then(handle_encrypted_signup);

        // Message routes
        let contact_route = warp::path!("service" / "contact")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "contact"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_contact);

        let encrypted_contact_route = warp::path!("service" / "econtact")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "contact"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_encrypted_contact);

        // Query routes
        let all_users_route = warp::path!("service" / "all_users")
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "debug"))
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and_then(handle_get_all_users);

        let all_enc_users_route = warp::path!("service" / "all_enc_users")
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "debug"))
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and_then(handle_get_all_enc_users);

        let all_messages_route = warp::path!("service" / "all_messages")
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "debug"))
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and_then(handle_get_all_messages);

        // Enhanced file management routes (SECURE - requires user's public key)
        let upload_data_dir = self.data_dir.clone();
        let upload_quantum_crypto = self.quantum_crypto.clone();
        let upload_request_semaphore = request_semaphore.clone();
        let upload_file_route = warp::path!("files" / "upload")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "upload"))
            .and(warp::body::content_length_limit(MAX_UPLOAD_BODY_BYTES))
            .and(warp::body::bytes())
            .and(warp::header::optional::<String>("content-type"))
            .and(warp::header::optional::<String>("filename"))
            .and(warp::header::<String>("owner-did"))
            .and(warp::header::<String>("owner-public-key")) // REQUIRED: User's public key
            .and(warp::header::optional::<String>("owner-key-algorithm")) // KEM for that key (e.g. Kyber1024); default = node default
            .and(with_db(db.clone()))
            .and(with_data_dir(upload_data_dir))
            .and(with_quantum_crypto(upload_quantum_crypto))
            .and(with_request_semaphore(upload_request_semaphore))
            .and_then(handle_file_upload);

        let download_request_semaphore = request_semaphore.clone();
        let download_data_dir = self.data_dir.clone();
        let download_file_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path::end()) // Ensure exact match (don't catch /files/{id}/session-key)
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "download"))
            .and(warp::header::optional::<String>("requester-did"))
            .and(with_db(db.clone()))
            .and(with_data_dir(download_data_dir))
            .and(with_request_semaphore(download_request_semaphore))
            .and_then(handle_file_download);

        // Session keypair endpoint - generates ephemeral keypair for secure private key transmission
        // Pattern: path segments first, then method, then filters
        let session_keypairs = self.session_keypairs.clone();
        let session_quantum_crypto = self.quantum_crypto.clone();
        let session_key_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("session-key"))
            .and(warp::path::end())
            .and(warp::get())
            .and(with_db(db.clone()))
            .and(with_session_keypairs(session_keypairs))
            .and(with_quantum_crypto(session_quantum_crypto))
            .and_then(handle_session_key);

        // Content retrieval endpoint - returns actual decrypted file content (REQUIRES encrypted user's private key)
        let content_data_dir = self.data_dir.clone();
        let content_quantum_crypto = self.quantum_crypto.clone();
        let content_session_keypairs = self.session_keypairs.clone();
        let content_request_semaphore = request_semaphore.clone();
        let file_content_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("content"))
            .and(warp::path::end())
            .and(warp::get())
            .and(warp::header::optional::<String>("requester-did"))
            .and(warp::header::<String>("encrypted-private-key")) // REQUIRED: User's private key encrypted with session public key (hex-encoded)
            .and(warp::header::<String>("session-id")) // REQUIRED: Session ID from session-key endpoint
            .and(with_db(db.clone()))
            .and(with_data_dir(content_data_dir))
            .and(with_quantum_crypto(content_quantum_crypto))
            .and(with_session_keypairs(content_session_keypairs))
            .and(with_request_semaphore(content_request_semaphore))
            .and_then(handle_file_content);

        // ── Envelope endpoints (zero-knowledge: server never sees plaintext or private keys) ──

        // Challenge-response: server KEM-encapsulates to the file's owner pubkey;
        // client decapsulates to prove key possession without sending the private key.
        let challenge_quantum_crypto = self.quantum_crypto.clone();
        let challenge_pending = self.pending_challenges.clone();
        let challenge_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("challenge"))
            .and(warp::path::end())
            .and(warp::get())
            .and(warp::header::optional::<String>("requester-public-key"))
            .and(warp::header::optional::<String>("requester-did"))
            .and(with_db(db.clone()))
            .and(with_quantum_crypto(challenge_quantum_crypto))
            .and(with_pending_challenges(challenge_pending))
            .and_then(handle_challenge);

        // Envelope upload: client sends a pre-encrypted envelope blob; server stores as-is.
        let envelope_upload_data_dir = self.data_dir.clone();
        let envelope_upload_semaphore = request_semaphore.clone();
        let envelope_upload_route = warp::path!("files" / "envelope-upload")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "upload"))
            .and(warp::body::content_length_limit(MAX_UPLOAD_BODY_BYTES))
            .and(warp::body::bytes())
            .and(warp::header::<String>("owner-did"))
            .and(warp::header::<String>("owner-public-key"))
            .and(warp::header::optional::<String>("filename"))
            .and(warp::header::optional::<String>("content-type"))
            .and(with_db(db.clone()))
            .and(with_data_dir(envelope_upload_data_dir))
            .and(with_request_semaphore(envelope_upload_semaphore))
            .and_then(handle_envelope_upload);

        // Chat / shared attachments: server encrypts plaintext to the storage node's Kyber key
        // so any participant can challenge+stream+decrypt, and admin-stream works after key rotation.
        let shared_upload_data_dir = self.data_dir.clone();
        let shared_upload_semaphore = request_semaphore.clone();
        let shared_upload_server_keypair = self.server_keypair.clone();
        let shared_upload_route = warp::path!("files" / "shared-upload")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "upload"))
            .and(warp::body::content_length_limit(MAX_UPLOAD_BODY_BYTES))
            .and(warp::body::bytes())
            .and(warp::header::<String>("owner-did"))
            .and(warp::header::optional::<String>("filename"))
            .and(warp::header::optional::<String>("content-type"))
            .and(with_db(db.clone()))
            .and(with_data_dir(shared_upload_data_dir))
            .and(with_request_semaphore(shared_upload_semaphore))
            .and(warp::any().map(move || shared_upload_server_keypair.clone()))
            .and_then(handle_shared_chat_upload);

        // Streaming download: decrypts with server key, re-encrypts for requester.
        // Auth via challenge-response headers (challenge-id + challenge-response).
        let stream_data_dir = self.data_dir.clone();
        let stream_pending = self.pending_challenges.clone();
        let stream_semaphore = request_semaphore.clone();
        let stream_server_keypair = self.server_keypair.clone();
        let stream_quantum_crypto = self.quantum_crypto.clone();
        let stream_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("stream"))
            .and(warp::path::end())
            .and(warp::get())
            .and(warp::header::<String>("challenge-id"))
            .and(warp::header::<String>("challenge-response"))
            .and(with_db(db.clone()))
            .and(with_data_dir(stream_data_dir))
            .and(with_pending_challenges(stream_pending))
            .and(with_request_semaphore(stream_semaphore))
            .and(warp::any().map(move || stream_server_keypair.clone()))
            .and(warp::any().map(move || stream_quantum_crypto.clone()))
            .and_then(handle_stream_download);

        // Owner-encrypted ciphertext download: after challenge verification return on-disk bytes
        // for client-side Kyber decrypt (legacy `/files/upload` to owner public key).
        let ciphertext_data_dir = self.data_dir.clone();
        let ciphertext_pending = self.pending_challenges.clone();
        let ciphertext_semaphore = request_semaphore.clone();
        let ciphertext_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("ciphertext"))
            .and(warp::path::end())
            .and(warp::get())
            .and(warp::header::<String>("challenge-id"))
            .and(warp::header::<String>("challenge-response"))
            .and(with_db(db.clone()))
            .and(with_data_dir(ciphertext_data_dir))
            .and(with_pending_challenges(ciphertext_pending))
            .and(with_request_semaphore(ciphertext_semaphore))
            .and_then(handle_ciphertext_download);

        // DID-authenticated direct stream: trusted services (e.g. website API)
        // can download file bytes without the KEM challenge-response handshake.
        let admin_stream_data_dir = self.data_dir.clone();
        let admin_stream_semaphore = request_semaphore.clone();
        let admin_stream_server_keypair = self.server_keypair.clone();
        let admin_stream_quantum_crypto = self.quantum_crypto.clone();
        let admin_stream_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("admin-stream"))
            .and(warp::path::end())
            .and(warp::get())
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and(with_data_dir(admin_stream_data_dir))
            .and(with_request_semaphore(admin_stream_semaphore))
            .and(warp::any().map(move || admin_stream_server_keypair.clone()))
            .and(warp::any().map(move || admin_stream_quantum_crypto.clone()))
            .and_then(handle_admin_stream);

        // Entitlement-gated delivery: OP_VERIFY then E2E capsule stream or server DEK re-wrap.
        let rewrap_data_dir = self.data_dir.clone();
        let rewrap_semaphore = request_semaphore.clone();
        let rewrap_server_keypair = self.server_keypair.clone();
        let rewrap_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("rewrap"))
            .and(warp::path::end())
            .and(warp::post())
            .and(warp::header::<String>("entitlement-id"))
            .and(warp::header::<String>("buyer-did"))
            .and(warp::header::<String>("buyer-public-key"))
            .and(with_db(db.clone()))
            .and(with_data_dir(rewrap_data_dir))
            .and(with_request_semaphore(rewrap_semaphore))
            .and(warp::any().map(move || rewrap_server_keypair.clone()))
            .and_then(handle_entitlement_rewrap);

        // Owner posts a recipient-wrapped DEK capsule after OP_GRANT (true E2E).
        let capsule_data_dir = self.data_dir.clone();
        let capsule_semaphore = request_semaphore.clone();
        let delivery_capsule_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("delivery-capsule"))
            .and(warp::path::end())
            .and(warp::put())
            .and(with_did_auth())
            .and(warp::header::<String>("entitlement-id"))
            .and(warp::body::content_length_limit(64 * 1024))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and(with_data_dir(capsule_data_dir))
            .and(with_request_semaphore(capsule_semaphore))
            .and_then(handle_put_delivery_capsule);

        // Diagnostic endpoint: inspect on-disk file format without decrypting.
        let diag_data_dir = self.data_dir.clone();
        let diag_server_keypair = self.server_keypair.clone();
        let diagnostic_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("diagnostic"))
            .and(warp::path::end())
            .and(warp::get())
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and(with_data_dir(diag_data_dir))
            .and(warp::any().map(move || diag_server_keypair.clone()))
            .and_then(handle_file_diagnostic);

        let list_files_route = warp::path!("files" / "list" / String)
            .and(warp::get())
            .and(with_db(db.clone()))
            .and_then(handle_list_files);

        // File delete route
        let delete_data_dir = self.data_dir.clone();
        let delete_request_semaphore = request_semaphore.clone();
        let delete_file_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path::end())
            .and(warp::delete())
            .and(warp::header::optional::<String>("requester-did"))
            .and(warp::query::<FileDeleteQuery>())
            .and(with_db(db.clone()))
            .and(with_data_dir(delete_data_dir))
            .and(with_request_semaphore(delete_request_semaphore))
            .and_then(handle_file_delete);

        let file_refs_route = warp::path("files")
            .and(warp::path::param::<String>())
            .and(warp::path("refs"))
            .and(warp::path::end())
            .and(warp::get())
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and_then(handle_file_refs);

        let orphan_files_route = warp::path!("api" / "admin" / "orphan-files")
            .and(warp::get())
            .and(with_did_auth())
            .and(warp::query::<OrphanFilesQuery>())
            .and(with_db(db.clone()))
            .and_then(handle_admin_orphan_files);

        let keychain_sync_route = warp::path!("api" / "keychain" / "grants")
            .and(warp::post())
            .and(with_did_auth())
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_data_dir(self.data_dir.clone()))
            .and(with_db(db.clone()))
            .and_then(handle_keychain_grant_sync);

        let keychain_revoke_route = warp::path!("api" / "keychain" / "grants" / String)
            .and(warp::delete())
            .and(with_did_auth())
            .and(with_data_dir(self.data_dir.clone()))
            .and_then(handle_keychain_grant_revoke);

        // SQL Query Interface endpoints (with authentication and rate limiting)
        let query_builder = self.query_builder.clone();
        let rate_limiter = self.rate_limiter.clone();

        let query_files_route = warp::path!("query" / "files")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "query"))
            .and(with_did_auth()) // Extract DID from Authorization header
            .and(with_rate_limiter(rate_limiter.clone())) // Check rate limit
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json()) // Extract query from body
            .and(with_query_builder(query_builder.clone())) // Extract query builder
            .and_then(handle_query_files);

        let query_facts_route = warp::path!("query" / "facts")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "query"))
            .and(with_did_auth())
            .and(with_rate_limiter(rate_limiter.clone()))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_query_builder(query_builder.clone()))
            .and_then(handle_query_facts);

        let query_users_route = warp::path!("query" / "users")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "query"))
            .and(with_did_auth())
            .and(with_rate_limiter(rate_limiter.clone()))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_query_builder(query_builder.clone()))
            .and_then(handle_query_users);

        let query_aggregate_route = warp::path!("query" / "aggregate")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "query"))
            .and(with_did_auth())
            .and(with_rate_limiter(rate_limiter.clone()))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_query_builder(query_builder.clone()))
            .and_then(handle_query_aggregate);

        // ============================================================================
        // DID-Scoped Document Store API Endpoints
        // Base: /api/documents
        // ============================================================================

        let put_document_route = warp::path!("api" / "documents" / String / String)
            .and(warp::put())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "api"))
            .and(with_did_auth())
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_put_document);

        let get_document_route = warp::path!("api" / "documents" / String / String)
            .and(warp::get())
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and_then(handle_get_document);

        let delete_document_route = warp::path!("api" / "documents" / String / String)
            .and(warp::delete())
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and_then(handle_delete_document);

        let list_documents_route = warp::path!("api" / "documents" / String)
            .and(warp::get())
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and_then(handle_list_documents);

        let delete_documents_collection_route = warp::path!("api" / "documents" / String)
            .and(warp::delete())
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and_then(handle_delete_documents_collection);

        // Optional but recommended: server-side document filtering
        let query_documents_route = warp::path!("query" / "documents" / String)
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "query"))
            .and(with_did_auth())
            .and(with_rate_limiter(rate_limiter.clone()))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_query_documents);

        // Server public key endpoint — clients fetch this to encrypt uploads
        let server_keypair_for_route = self.server_keypair.clone();
        let server_public_key_route = warp::path!("server-public-key")
            .and(warp::get())
            .and(warp::any().map(move || server_keypair_for_route.clone()))
            .and_then(handle_server_public_key);

        // Server key rotation endpoint — admin-only.
        // POST /api/rotate-server-key
        // Generates a new Kyber keypair, re-wraps all on-disk envelope headers,
        // registers the new key with the KeyMaster, and updates the in-memory
        // server keypair atomically.
        #[cfg(feature = "quantum")]
        let rotation_data_dir = self.data_dir.clone();
        #[cfg(feature = "quantum")]
        let rotation_qc = self.quantum_crypto.clone();
        #[cfg(feature = "quantum")]
        let rotation_db = db.clone();
        #[cfg(feature = "quantum")]
        let rotate_server_key_route = warp::path!("api" / "rotate-server-key")
            .and(warp::post())
            .and(warp::any().map(move || rotation_data_dir.clone()))
            .and(warp::any().map(move || rotation_qc.clone()))
            .and(warp::any().map(move || rotation_db.clone()))
            .and_then(handle_rotate_server_key);

        // Health check endpoint
        let health_route = warp::path!("health")
            .and(warp::get())
            .and(warp::any().map(move || api_listen_port))
            .and_then(handle_health_check);

        // ============================================================================
        // Global User Registry API Endpoints
        // ============================================================================

        let register_global_user_route = warp::path!("api" / "users" / "register")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_register_global_user);

        let get_global_user_route = warp::path!("api" / "users")
            .and(warp::path::param::<String>())
            .and(warp::get())
            .and(with_db(db.clone()))
            .and_then(handle_get_global_user);

        let update_user_presence_route = warp::path!("api" / "users")
            .and(warp::path::param::<String>())
            .and(warp::path("presence"))
            .and(warp::put())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_update_user_presence);

        // ============================================================================
        // Server Registry API Endpoints
        // ============================================================================

        let create_server_route = warp::path!("api" / "servers")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_create_server);

        let get_servers_route = warp::path!("api" / "servers")
            .and(warp::get())
            .and(warp::query::<std::collections::HashMap<String, String>>())
            .and(with_db(db.clone()))
            .and_then(handle_get_servers);

        let get_server_route = warp::path!("api" / "servers")
            .and(warp::path::param::<String>())
            .and(warp::get())
            .and(with_db(db.clone()))
            .and_then(handle_get_server);

        let join_server_route = warp::path!("api" / "servers")
            .and(warp::path::param::<String>())
            .and(warp::path("join"))
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_join_server);

        let get_server_members_route = warp::path!("api" / "servers")
            .and(warp::path::param::<String>())
            .and(warp::path("members"))
            .and(warp::get())
            .and(with_db(db.clone()))
            .and_then(handle_get_server_members);

        let update_member_role_route = warp::path!("api" / "servers")
            .and(warp::path::param::<String>())
            .and(warp::path("members"))
            .and(warp::path::param::<String>())
            .and(warp::path("role"))
            .and(warp::put())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_update_member_role);

        let remove_member_route = warp::path!("api" / "servers")
            .and(warp::path::param::<String>())
            .and(warp::path("members"))
            .and(warp::path::param::<String>())
            .and(warp::delete())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_remove_member);

        let create_invitation_route = warp::path!("api" / "servers")
            .and(warp::path::param::<String>())
            .and(warp::path("invitations"))
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_create_invitation);

        let get_invitations_route = warp::path!("api" / "servers")
            .and(warp::path::param::<String>())
            .and(warp::path("invitations"))
            .and(warp::get())
            .and(warp::query::<std::collections::HashMap<String, String>>())
            .and(with_db(db.clone()))
            .and_then(handle_get_invitations);

        let use_invitation_route = warp::path!("api" / "servers")
            .and(warp::path::param::<String>())
            .and(warp::path("invitations"))
            .and(warp::path("use"))
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_use_invitation);

        // ============================================================================
        // Global Group Registry API Endpoints
        // ============================================================================

        let create_group_route = warp::path!("api" / "groups")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_create_group);

        let get_groups_route = warp::path!("api" / "groups")
            .and(warp::get())
            .and(warp::query::<std::collections::HashMap<String, String>>())
            .and(with_db(db.clone()))
            .and_then(handle_get_groups);

        let get_group_route = warp::path!("api" / "groups")
            .and(warp::path::param::<String>())
            .and(warp::get())
            .and(with_db(db.clone()))
            .and_then(handle_get_group);

        let join_group_route = warp::path!("api" / "groups")
            .and(warp::path::param::<String>())
            .and(warp::path("join"))
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_join_group);

        let get_group_members_route = warp::path!("api" / "groups")
            .and(warp::path::param::<String>())
            .and(warp::path("members"))
            .and(warp::get())
            .and(with_db(db.clone()))
            .and_then(handle_get_group_members);

        // ============================================================================
        // Feed Subscription API Endpoints
        // ============================================================================

        let subscribe_feed_route = warp::path!("api" / "groups")
            .and(warp::path::param::<String>())
            .and(warp::path("subscribe"))
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "public"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_subscribe_feed);

        let get_user_subscriptions_route = warp::path!("api" / "users")
            .and(warp::path::param::<String>())
            .and(warp::path("subscriptions"))
            .and(warp::get())
            .and(with_db(db.clone()))
            .and_then(handle_get_user_subscriptions);

        // ============================================================================
        // Cross-Server Routing API Endpoints
        // ============================================================================
        let connect_server_route = warp::path!("api" / "servers" / String / "connect")
            .and(warp::post())
            .and(with_ip_rate_limiter(
                ip_rate_limiter.clone(),
                "server_routing",
            ))
            .and(with_did_auth())
            .and(with_db(db.clone()))
            .and(with_server_routing(server_routing.clone()))
            .and_then(handle_connect_server);

        let disconnect_server_route = warp::path!("api" / "servers" / String / "disconnect")
            .and(warp::post())
            .and(with_ip_rate_limiter(
                ip_rate_limiter.clone(),
                "server_routing",
            ))
            .and(with_did_auth())
            .and(with_server_routing(server_routing.clone()))
            .and_then(handle_disconnect_server);

        let server_connection_status_route = warp::path!("api" / "servers" / String / "connection")
            .and(warp::get())
            .and(with_ip_rate_limiter(
                ip_rate_limiter.clone(),
                "server_routing",
            ))
            .and(with_did_auth())
            .and(with_server_routing(server_routing.clone()))
            .and_then(handle_server_connection_status);

        let connected_servers_route = warp::path!("api" / "servers" / "connected")
            .and(warp::get())
            .and(with_ip_rate_limiter(
                ip_rate_limiter.clone(),
                "server_routing",
            ))
            .and(with_did_auth())
            .and(with_server_routing(server_routing.clone()))
            .and_then(handle_connected_servers);

        let subscribe_server_topic_route = warp::path!("api" / "servers" / String / "subscribe")
            .and(warp::post())
            .and(with_ip_rate_limiter(
                ip_rate_limiter.clone(),
                "server_routing",
            ))
            .and(with_did_auth())
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_server_routing(server_routing.clone()))
            .and_then(handle_subscribe_server_topic);

        let unsubscribe_server_topic_route =
            warp::path!("api" / "servers" / String / "unsubscribe")
                .and(warp::post())
                .and(with_ip_rate_limiter(
                    ip_rate_limiter.clone(),
                    "server_routing",
                ))
                .and(with_did_auth())
                .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
                .and(warp::body::json())
                .and(with_server_routing(server_routing.clone()))
                .and_then(handle_unsubscribe_server_topic);

        let send_server_message_route = warp::path!("api" / "servers" / String / "send")
            .and(warp::post())
            .and(with_ip_rate_limiter(
                ip_rate_limiter.clone(),
                "server_routing",
            ))
            .and(with_did_auth())
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_server_routing(server_routing.clone()))
            .and_then(handle_send_server_message);

        // ============================================================================
        // Distributed Rate Limit Service (optional)
        // ============================================================================
        // If you want multi-node distributed rate limiting, point all nodes at a single
        // coordinator via `SPACEKIT_RATE_LIMIT_URL` and enable the service on that
        // coordinator with `SPACEKIT_RATE_LIMIT_ENABLE_SERVICE=1`.
        let rate_limit_service_enabled = match std::env::var("SPACEKIT_RATE_LIMIT_ENABLE_SERVICE") {
            Ok(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
            Err(_) => false,
        };

        let rate_limit_service_route = if rate_limit_service_enabled {
            let db = db.clone();
            warp::path!("service" / "rate_limit" / "check")
                .and(warp::post())
                .and(with_ip_rate_limiter(
                    ip_rate_limiter.clone(),
                    "rate_limit_service",
                ))
                .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
                .and(warp::body::json())
                .and(with_db(db))
                .and_then(handle_rate_limit_check)
                .boxed()
        } else {
            warp::any()
                .and(warp::path("service"))
                .and(warp::path("rate_limit"))
                .and(warp::path("check"))
                .and(warp::path::end())
                .map(|| {
                    boxed_reply(warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({"error":"not found"})),
                        warp::http::StatusCode::NOT_FOUND,
                    ))
                })
                .boxed()
        };

        // ============================================================================
        // Server Routing API Endpoints (Cross-Server P2P)
        // ============================================================================
        // NOTE: Server routing routes are not yet wired up because StorageNode
        // cannot be easily cloned. Server routing is currently accessed directly
        // from SpaceKit OS via the StorageNode instance.
        // TODO: Refactor ApiServer to accept StorageNode reference or create
        // a separate server routing API endpoint handler

        // ============================================================================
        // App Package API Endpoints (SpaceKit AppStore)
        // ============================================================================
        // These endpoints provide access to the SpaceKit App Package system,
        // enabling app discovery, listing, and metadata retrieval.

        let list_apps_route = warp::path!("api" / "apps")
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "apps"))
            .and(warp::query::<AppListQuery>())
            .and(with_db(db.clone()))
            .and_then(handle_list_apps);

        let get_app_route = warp::path!("api" / "apps" / String)
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "apps"))
            .and(with_db(db.clone()))
            .and_then(handle_get_app);

        let get_app_versions_route = warp::path!("api" / "apps" / String / "versions")
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "apps"))
            .and(with_db(db.clone()))
            .and_then(handle_get_app_versions);

        let get_featured_apps_route = warp::path!("api" / "apps" / "featured")
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "apps"))
            .and(warp::query::<LimitQuery>())
            .and(with_db(db.clone()))
            .and_then(handle_get_featured_apps);

        let search_apps_route = warp::path!("api" / "apps" / "search")
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "apps"))
            .and(warp::query::<AppSearchQuery>())
            .and(with_db(db.clone()))
            .and_then(handle_search_apps);

        let get_app_stats_route = warp::path!("api" / "apps" / "stats")
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "apps"))
            .and(with_db(db.clone()))
            .and_then(handle_get_app_stats);

        let get_apps_by_category_route = warp::path!("api" / "apps" / "category" / String)
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "apps"))
            .and(warp::query::<LimitQuery>())
            .and(with_db(db.clone()))
            .and_then(handle_get_apps_by_category);

        let get_apps_by_creator_route = warp::path!("api" / "apps" / "creator" / String)
            .and(warp::get())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "apps"))
            .and(warp::query::<LimitQuery>())
            .and(with_db(db.clone()))
            .and_then(handle_get_apps_by_creator);

        let package_routes = spkg_routes::routes(
            self.data_dir.clone(),
            blob_fact_auth_mode,
            request_semaphore.clone(),
        );

        // DID Registry endpoints
        // ---- CAS Blob routes ----
        let blob_upload_data_dir = self.data_dir.clone();
        let blob_upload_semaphore = request_semaphore.clone();
        let put_blob_route = warp::path!("blobs" / String)
            .and(warp::put())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "blob_upload"))
            .and(warp::body::content_length_limit(MAX_UPLOAD_BODY_BYTES))
            .and(warp::body::bytes())
            .and(with_data_dir(blob_upload_data_dir))
            .and(with_request_semaphore(blob_upload_semaphore))
            .and(with_optional_auth_header())
            .and_then(move |hash, body, data_dir, semaphore, auth| {
                handle_put_blob(hash, body, data_dir, semaphore, blob_fact_auth_mode, auth)
            });

        let blob_get_data_dir = self.data_dir.clone();
        let blob_get_semaphore = request_semaphore.clone();
        let get_blob_route = warp::path!("blobs" / String)
            .and(warp::get())
            .and(with_ip_rate_limiter(
                ip_rate_limiter.clone(),
                "blob_download",
            ))
            .and(with_data_dir(blob_get_data_dir))
            .and(with_request_semaphore(blob_get_semaphore))
            .and(with_optional_auth_header())
            .and_then(move |hash, data_dir, semaphore, auth| {
                handle_get_blob(hash, data_dir, semaphore, blob_fact_auth_mode, auth)
            });

        let blob_head_data_dir = self.data_dir.clone();
        let head_blob_route = warp::path!("blobs" / String)
            .and(warp::head())
            .and(with_data_dir(blob_head_data_dir))
            .and_then(handle_head_blob);

        let blob_exists_data_dir = self.data_dir.clone();
        let blob_exists_route = warp::path!("blobs" / "exists")
            .and(warp::post())
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_data_dir(blob_exists_data_dir))
            .and_then(handle_blobs_exist);

        // ---- Fact Package API routes ----
        let fact_post_data_dir = self.data_dir.clone();
        let fact_post_db = db.clone();
        let fact_post_semaphore = request_semaphore.clone();
        let fact_post_quantum = self.quantum_crypto.clone();
        let post_fact_route = warp::path!("facts")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "fact_submit"))
            .and(warp::body::content_length_limit(MAX_UPLOAD_BODY_BYTES))
            .and(warp::body::json())
            .and(with_data_dir(fact_post_data_dir))
            .and(with_db(fact_post_db))
            .and(with_request_semaphore(fact_post_semaphore))
            .and(with_optional_auth_header())
            .and_then(move |fact, data_dir, db, semaphore, auth| {
                let quantum = fact_post_quantum.clone();
                async move {
                    handle_post_fact(
                        fact,
                        data_dir,
                        db,
                        semaphore,
                        blob_fact_auth_mode,
                        auth,
                        quantum,
                    )
                    .await
                }
            });

        let fact_get_data_dir = self.data_dir.clone();
        let fact_get_semaphore = request_semaphore.clone();
        let get_fact_route = warp::path!("facts" / String)
            .and(warp::get())
            // Public manifest/asset reads — do not IP-rate-limit (dev page loads + marketplace
            // validation can exceed 60/min from loopback and break every embedded app).
            .and(with_data_dir(fact_get_data_dir))
            .and(with_request_semaphore(fact_get_semaphore))
            .and(with_optional_requester_did())
            .and_then(move |fact_id_hex, data_dir, semaphore, requester| {
                handle_get_fact(
                    fact_id_hex,
                    data_dir,
                    semaphore,
                    blob_fact_auth_mode,
                    requester,
                )
            });

        let fact_batch_data_dir = self.data_dir.clone();
        let fact_batch_semaphore = request_semaphore.clone();
        let batch_facts_route = warp::path!("facts" / "batch")
            .and(warp::post())
            .and(with_ip_rate_limiter(ip_rate_limiter.clone(), "fact_batch"))
            .and(warp::body::content_length_limit(MAX_JSON_BODY_BYTES))
            .and(warp::body::json())
            .and(with_data_dir(fact_batch_data_dir))
            .and(with_request_semaphore(fact_batch_semaphore))
            .and_then(handle_batch_facts);

        let fact_stream_data_dir = self.data_dir.clone();
        let fact_stream_semaphore = request_semaphore.clone();
        let stream_fact_route = warp::path!("facts" / String / "stream")
            .and(warp::get())
            .and(with_data_dir(fact_stream_data_dir))
            .and(with_request_semaphore(fact_stream_semaphore))
            .and(warp::header::optional::<String>("range"))
            .and_then(handle_stream_fact);

        let did_register_route = warp::path!("api" / "did" / "register")
            .and(warp::post())
            .and(warp::body::json())
            .and(with_db(db.clone()))
            .and_then(handle_did_register);

        let did_resolve_route = warp::path!("api" / "did" / "resolve" / String)
            .and(warp::get())
            .and(with_db(db.clone()))
            .and_then(handle_did_resolve);

        // Combine all routes (session_key_route and file_content_route must come before download_file_route
        // because they have more specific paths: /files/{id}/session-key vs /files/{id})
        // delete_file_route must come before download_file_route since they share path but differ by method
        let routes = did_register_route
            .or(did_resolve_route)
            .or(did_route)
            .or(signup_route)
            .or(encrypted_signup_route)
            .or(contact_route)
            .or(encrypted_contact_route)
            .or(all_users_route)
            .or(all_enc_users_route)
            .or(all_messages_route)
            .or(upload_file_route)
            .or(envelope_upload_route)
            .or(shared_upload_route)
            .or(list_files_route)
            .or(challenge_route)
            .or(stream_route)
            .or(ciphertext_route)
            .or(admin_stream_route)
            .or(file_refs_route)
            .or(rewrap_route)
            .or(delivery_capsule_route)
            .or(diagnostic_route)
            .or(session_key_route)
            .or(file_content_route)
            .or(delete_file_route)
            .or(download_file_route)
            .or(query_files_route)
            .or(query_facts_route)
            .or(query_users_route)
            .or(query_aggregate_route)
            .or(query_documents_route)
            .or(keymaster_routes::put_route(self.data_dir.clone()))
            .or(keymaster_routes::get_route(self.data_dir.clone()))
            .or(keymaster_routes::delete_route(self.data_dir.clone()))
            .or(health_route)
            .or(server_public_key_route);

        // Conditionally add key rotation route
        #[cfg(feature = "quantum")]
        let routes = routes.or(rotate_server_key_route);

        let routes = routes
            // Document store CRUD
            // NOTE: list route must come before get/put/delete routes in case of path conflicts,
            // but warp path! macros are strict enough here (String vs String/String).
            .or(list_documents_route)
            .or(delete_documents_collection_route)
            .or(orphan_files_route)
            .or(keychain_sync_route)
            .or(keychain_revoke_route)
            .or(get_document_route)
            .or(put_document_route)
            .or(delete_document_route)
            // Global User Registry
            .or(register_global_user_route)
            .or(get_global_user_route)
            .or(update_user_presence_route)
            // Server Registry
            .or(create_server_route)
            .or(get_servers_route)
            .or(get_server_route)
            .or(join_server_route)
            .or(get_server_members_route)
            .or(update_member_role_route)
            .or(remove_member_route)
            .or(create_invitation_route)
            .or(get_invitations_route)
            .or(use_invitation_route)
            // Global Group Registry
            .or(create_group_route)
            .or(get_groups_route)
            .or(get_group_route)
            .or(join_group_route)
            .or(get_group_members_route)
            // Feed Subscriptions
            .or(subscribe_feed_route)
            .or(get_user_subscriptions_route)
            // Distributed rate limit service (optional)
            .or(rate_limit_service_route)
            // Server Routing (Cross-Server P2P)
            .or(connect_server_route)
            .or(disconnect_server_route)
            .or(server_connection_status_route)
            .or(connected_servers_route)
            .or(subscribe_server_topic_route)
            .or(unsubscribe_server_topic_route)
            .or(send_server_message_route)
            // App Package API
            .or(get_featured_apps_route) // Must come before get_app_route due to path overlap
            .or(search_apps_route) // Must come before get_app_route due to path overlap
            .or(get_app_stats_route) // Must come before get_app_route due to path overlap
            .or(get_apps_by_category_route)
            .or(get_apps_by_creator_route)
            .or(get_app_versions_route) // Must come before get_app_route due to path overlap
            .or(list_apps_route)
            .or(get_app_route)
            // Native immutable SPKG archives
            .or(package_routes)
            // CAS Blob Storage
            .or(blob_exists_route)
            .or(put_blob_route)
            .or(get_blob_route)
            .or(head_blob_route)
            // Fact Package API (batch before get to avoid path overlap)
            .or(batch_facts_route)
            .or(stream_fact_route)
            .or(post_fact_route)
            .or(get_fact_route);

        // ── Agentic readiness routes (Phase 0/1/3/4) ──
        // Mounted only when the storage_facade has been wired in. Provides
        // /api/transactions/*, /api/sandboxes/*, /api/changes (SSE).
        let routes = match self.facade.clone() {
            Some(facade) => routes
                .map(|reply| -> Box<dyn Reply> { Box::new(reply) })
                .or(agentic_routes::build_routes(
                    facade,
                    self.memory_route_state.clone(),
                ))
                .unify()
                .boxed(),
            None => routes
                .map(|reply| -> Box<dyn Reply> { Box::new(reply) })
                .boxed(),
        };

        let routes = routes.with(cors).with(warp::log("spacekit-storage-api"));

        // Check HOST environment variable, default to 0.0.0.0 for container/VPC deployments
        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let host_addr: std::net::IpAddr = host.parse().unwrap_or_else(|_| {
            tracing::warn!("Invalid HOST '{}', defaulting to 0.0.0.0", host);
            std::net::IpAddr::V4(std::net::Ipv4Addr::new(0, 0, 0, 0))
        });

        tracing::info!(
            "SpaceKit Storage API server starting on {}:{}",
            host_addr,
            server_config.port
        );
        warp::serve(routes)
            .run((host_addr, server_config.port))
            .await;

        Ok(())
    }
}

// Helper filters

fn with_db(
    db: Arc<Database>,
) -> impl Filter<Extract = (Arc<Database>,), Error = Infallible> + Clone {
    warp::any().map(move || db.clone())
}

fn with_public_key(
    public_key: String,
) -> impl Filter<Extract = (String,), Error = Infallible> + Clone {
    warp::any().map(move || public_key.clone())
}

fn with_data_dir(
    data_dir: Option<PathBuf>,
) -> impl Filter<Extract = (Option<PathBuf>,), Error = Infallible> + Clone {
    warp::any().map(move || data_dir.clone())
}

fn with_quantum_crypto(
    quantum_crypto: Option<Arc<QuantumCrypto>>,
) -> impl Filter<Extract = (Option<Arc<QuantumCrypto>>,), Error = Infallible> + Clone {
    warp::any().map(move || quantum_crypto.clone())
}

fn with_session_keypairs(
    session_keypairs: Arc<RwLock<HashMap<String, SessionKeypair>>>,
) -> impl Filter<Extract = (Arc<RwLock<HashMap<String, SessionKeypair>>>,), Error = Infallible> + Clone
{
    warp::any().map(move || session_keypairs.clone())
}

fn with_pending_challenges(
    challenges: Arc<RwLock<HashMap<String, PendingChallenge>>>,
) -> impl Filter<Extract = (Arc<RwLock<HashMap<String, PendingChallenge>>>,), Error = Infallible> + Clone
{
    warp::any().map(move || challenges.clone())
}

fn with_request_semaphore(
    semaphore: Arc<tokio::sync::Semaphore>,
) -> impl Filter<Extract = (Arc<tokio::sync::Semaphore>,), Error = Infallible> + Clone {
    warp::any().map(move || semaphore.clone())
}

fn with_query_builder(
    query_builder: Option<Arc<crate::sql_query::StorageQueryBuilder>>,
) -> impl Filter<Extract = (Option<Arc<crate::sql_query::StorageQueryBuilder>>,), Error = Infallible>
       + Clone {
    warp::any().map(move || query_builder.clone())
}

fn with_server_routing(
    routing: Option<Arc<ServerRoutingManager>>,
) -> impl Filter<Extract = (Option<Arc<ServerRoutingManager>>,), Error = Infallible> + Clone {
    warp::any().map(move || routing.clone())
}

fn boxed_reply<T: Reply + 'static>(reply: T) -> Box<dyn Reply> {
    Box::new(reply)
}

/// Encrypt a server keypair for local at-rest storage (AES-256-GCM).
/// Format: `[12-byte nonce][ciphertext of JSON { public_key, secret_key, algorithm }]`
#[cfg(feature = "quantum")]
fn encrypt_local_keypair(
    pk_hex: &str,
    sk_hex: &str,
    algo: &str,
    key: &[u8; 32],
) -> Result<Vec<u8>> {
    use aes_gcm::aead::rand_core::RngCore;
    use aes_gcm::{
        aead::{Aead, OsRng},
        Aes256Gcm, KeyInit, Nonce,
    };

    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow::anyhow!("aes init: {}", e))?;
    let plaintext = serde_json::to_vec(&serde_json::json!({
        "public_key": pk_hex,
        "secret_key": sk_hex,
        "algorithm": algo,
    }))?;
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(nonce, plaintext.as_slice())
        .map_err(|e| anyhow::anyhow!("encrypt: {}", e))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

/// Decrypt a locally-encrypted server keypair blob.
#[cfg(feature = "quantum")]
fn decrypt_local_keypair(blob: &[u8], key: &[u8; 32]) -> Result<(Vec<u8>, Vec<u8>, String)> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

    if blob.len() < 13 {
        return Err(anyhow::anyhow!("blob too short"));
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow::anyhow!("aes init: {}", e))?;
    let nonce = Nonce::from_slice(&blob[..12]);
    let pt = cipher
        .decrypt(nonce, &blob[12..])
        .map_err(|e| anyhow::anyhow!("decrypt: {}", e))?;
    let v: serde_json::Value = serde_json::from_slice(&pt)?;
    let pk_hex = v["public_key"].as_str().unwrap_or("");
    let sk_hex = v["secret_key"].as_str().unwrap_or("");
    let algo = v["algorithm"].as_str().unwrap_or("Kyber1024").to_string();
    let pk = hex::decode(pk_hex).map_err(|e| anyhow::anyhow!("pk hex: {}", e))?;
    let sk = hex::decode(sk_hex).map_err(|e| anyhow::anyhow!("sk hex: {}", e))?;
    Ok((pk, sk, algo))
}

#[derive(Debug, Clone, Deserialize)]
struct RateLimitCheckBody {
    key: String,
    prefix: String,
    max_requests: usize,
    window_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerTopicRequest {
    topic: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerSendRequest {
    message: String,
}

/// DID-based authentication filter
/// Expects: Authorization: DID <did:spacekit:user:alice>
fn with_did_auth() -> impl Filter<Extract = (String,), Error = Rejection> + Clone {
    warp::header::<String>("authorization")
        .and_then(|auth_header: String| async move {
            // Support both "DID <did>" and "Bearer <did>" formats
            let did = if auth_header.starts_with("DID ") {
                auth_header.strip_prefix("DID ").unwrap().trim().to_string()
            } else if auth_header.starts_with("Bearer ") {
                auth_header
                    .strip_prefix("Bearer ")
                    .unwrap()
                    .trim()
                    .to_string()
            } else {
                return Err(warp::reject::custom(AuthError::InvalidFormat));
            };

            // Basic DID format validation
            if did.starts_with("did:") && did.len() > 10 {
                Ok(did)
            } else {
                Err(warp::reject::custom(AuthError::InvalidDid))
            }
        })
        .or_else(|_| async move { Err(warp::reject::custom(AuthError::MissingHeader)) })
}

/// Optional `Authorization` header (raw value).
fn with_optional_auth_header() -> impl Filter<Extract = (Option<String>,), Error = Rejection> + Clone
{
    warp::header::optional::<String>("authorization")
}

/// Optional `Authorization: DID` (no rejection when absent).
fn with_optional_requester_did(
) -> impl Filter<Extract = (Option<String>,), Error = Rejection> + Clone {
    warp::header::optional::<String>("authorization").map(|auth: Option<String>| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        crate::upload_token::optional_requester_did(auth.as_deref(), None, now)
    })
}

/// Rate limiting filter
fn with_rate_limiter(
    rate_limiter: Arc<RateLimiter>,
) -> impl Filter<Extract = (), Error = Rejection> + Clone {
    warp::any()
        .and(warp::header::optional::<String>("authorization"))
        .and_then(move |auth_header: Option<String>| {
            let rate_limiter = rate_limiter.clone();
            async move {
                // Extract DID from auth header for rate limiting key
                let key = if let Some(header) = auth_header {
                    if header.starts_with("DID ") {
                        header.strip_prefix("DID ").unwrap().trim().to_string()
                    } else if header.starts_with("Bearer ") {
                        header.strip_prefix("Bearer ").unwrap().trim().to_string()
                    } else {
                        "anonymous".to_string()
                    }
                } else {
                    "anonymous".to_string()
                };

                rate_limiter
                    .check_rate_limit(&key)
                    .await
                    .map_err(|e| warp::reject::custom(e))
            }
        })
        .untuple_one()
}

/// IP-based rate limiting filter.
///
/// If you run behind a proxy/load balancer, it may set `X-Forwarded-For`.
/// Only trust that header if it is injected by infrastructure you control.
fn with_ip_rate_limiter(
    rate_limiter: Arc<RateLimiter>,
    category: &'static str,
) -> impl Filter<Extract = (), Error = Rejection> + Clone {
    warp::any()
        .and(warp::header::optional::<String>("x-forwarded-for"))
        .and(warp::addr::remote())
        .and_then(move |xff: Option<String>, remote: Option<SocketAddr>| {
            let rate_limiter = rate_limiter.clone();
            async move {
                let ip = extract_client_ip(xff.as_deref(), remote);
                let key = format!("{}:{}", category, ip);
                rate_limiter
                    .check_rate_limit(&key)
                    .await
                    .map_err(|e| warp::reject::custom(e))
            }
        })
        .untuple_one()
}

fn extract_client_ip(x_forwarded_for: Option<&str>, remote: Option<SocketAddr>) -> String {
    // Never trust X-Forwarded-For by default (client can spoof it).
    // Enable only when running behind a trusted proxy/load balancer you control.
    let trust_xff = std::env::var("SPACEKIT_TRUST_X_FORWARDED_FOR")
        .ok()
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"));

    if trust_xff {
        if let Some(xff) = x_forwarded_for {
            if let Some(first) = xff.split(',').next() {
                let ip = first.trim();
                if !ip.is_empty() {
                    return ip.to_string();
                }
            }
        }
    }
    remote
        .map(|s| s.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

// Route handlers (migrated and enhanced from old implementation)

/// Handle DID request
async fn handle_get_did(public_key: String) -> Result<impl Reply, Rejection> {
    let did = format!("did:spacekit:{}", public_key);
    Ok(warp::reply::with_status(did, warp::http::StatusCode::OK))
}

/// Return the server's Kyber public key (hex) so clients can encrypt uploads to it.
async fn handle_server_public_key(
    keypair: Option<Arc<ServerKeypair>>,
) -> Result<Box<dyn Reply>, Rejection> {
    match keypair {
        Some(kp) => {
            let pk_fingerprint = hex::encode(&blake3::hash(&kp.public_key).as_bytes()[..8]);
            Ok(boxed_reply(warp::reply::json(&serde_json::json!({
                "public_key": hex::encode(&kp.public_key),
                "algorithm": kp.algorithm,
                "key_source": kp.key_source,
                "pk_fingerprint": pk_fingerprint,
            }))))
        }
        None => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Server keypair not initialized"})),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        ))),
    }
}

/// Rotate the server Kyber keypair.
///
/// 1. Generate a new Kyber1024 keypair.
/// 2. Iterate all on-disk envelope files and re-wrap the header's
///    `encrypted_file_key` from the old SK to the new PK.
/// 3. Persist the new keypair (encrypted at rest).
/// 4. Notify the KeyMaster if reachable.
#[cfg(feature = "quantum")]
async fn handle_rotate_server_key(
    data_dir: Option<PathBuf>,
    qc: Option<Arc<QuantumCrypto>>,
    db: Arc<crate::database::Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    use crate::envelope;

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "no data_dir"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };
    let qc = match qc {
        Some(q) => q,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "encryption not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    // Load old keypair from encrypted file
    let enc_path = data_dir.join("server_keypair.enc");
    let legacy_path = data_dir.join("server_keypair.json");
    let node_did = std::env::var("SPACEKIT_NODE_DID").unwrap_or_default();

    let derive_key = |did: &str| -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(b"spacekit-server-keypair-v1:");
        h.update(did.as_bytes());
        h.finalize().into()
    };

    // Load old SK
    let (old_pk, old_sk, old_algo) = if enc_path.exists() && !node_did.is_empty() {
        let blob = tokio::fs::read(&enc_path)
            .await
            .map_err(|_| warp::reject::reject())?;
        let key = derive_key(&node_did);
        decrypt_local_keypair(&blob, &key).map_err(|_| warp::reject::reject())?
    } else if legacy_path.exists() {
        let json = tokio::fs::read_to_string(&legacy_path)
            .await
            .map_err(|_| warp::reject::reject())?;
        let v: serde_json::Value =
            serde_json::from_str(&json).map_err(|_| warp::reject::reject())?;
        let pk = hex::decode(v["public_key"].as_str().unwrap_or(""))
            .map_err(|_| warp::reject::reject())?;
        let sk = hex::decode(v["secret_key"].as_str().unwrap_or(""))
            .map_err(|_| warp::reject::reject())?;
        let algo = v["algorithm"].as_str().unwrap_or("Kyber1024").to_string();
        (pk, sk, algo)
    } else {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "no existing keypair to rotate from"})),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    };

    // Generate new keypair
    let algorithm = spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber1024;
    let (new_pk, new_sk) = qc
        .generate_keypair(algorithm)
        .await
        .map_err(|_| warp::reject::reject())?;

    // Re-wrap all envelope files on disk
    let mut rewrapped = 0u64;
    let mut skipped = 0u64;
    let mut dir = tokio::fs::read_dir(&data_dir)
        .await
        .map_err(|_| warp::reject::reject())?;

    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();
        if path.is_file() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip non-data files
            if name.ends_with(".json")
                || name.ends_with(".enc")
                || name.ends_with(".key")
                || name.ends_with(".raw")
                || name.starts_with('.')
            {
                continue;
            }
            match tokio::fs::read(&path).await {
                Ok(bytes) if bytes.len() > 8 => {
                    match envelope::rewrap_envelope(&bytes, &old_sk, &new_pk, &old_algo) {
                        Ok(new_bytes) => {
                            if let Err(e) = tokio::fs::write(&path, new_bytes).await {
                                tracing::warn!("Failed to write re-wrapped {}: {}", name, e);
                            } else {
                                rewrapped += 1;
                            }
                        }
                        Err(_) => {
                            skipped += 1;
                        }
                    }
                }
                _ => {
                    skipped += 1;
                }
            }
        }
    }

    // Save new keypair encrypted
    let new_pk_hex = hex::encode(&new_pk);
    let new_sk_hex = hex::encode(&new_sk);
    if !node_did.is_empty() {
        let key = derive_key(&node_did);
        if let Ok(blob) = encrypt_local_keypair(&new_pk_hex, &new_sk_hex, "Kyber1024", &key) {
            tokio::fs::write(&enc_path, blob).await.ok();
            tokio::fs::remove_file(&legacy_path).await.ok();
        }
    }

    // Notify KeyMaster
    #[cfg(feature = "reqwest")]
    {
        let compute_url = std::env::var("SPACEKIT_COMPUTE_URL").unwrap_or_default();
        if !compute_url.is_empty() && !node_did.is_empty() {
            let url = format!("{}/v1/keymaster/rotate", compute_url.trim_end_matches('/'));
            let _ = reqwest::Client::new()
                .post(&url)
                .json(&serde_json::json!({
                    "node_did": node_did,
                    "new_server_pk_hex": new_pk_hex,
                    "new_server_sk_hex": new_sk_hex,
                    "algorithm": "Kyber1024",
                }))
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
        }
    }

    tracing::info!(
        "Server key rotation complete: {} envelopes re-wrapped, {} skipped",
        rewrapped,
        skipped
    );

    Ok(boxed_reply(warp::reply::json(&serde_json::json!({
        "status": "rotated",
        "new_public_key": new_pk_hex,
        "envelopes_rewrapped": rewrapped,
        "envelopes_skipped": skipped,
    }))))
}

/// Handle health check requests
async fn handle_health_check(api_listen_port: u16) -> Result<impl Reply, Rejection> {
    let health = serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().timestamp(),
        "node_type": "storage",
        "version": "1.0.0",
        "api": {
            "port": api_listen_port,
            "endpoints": [
                "/health",
                "/did",
                "/query/files",
                "/query/facts",
                "/query/users",
                "/query/aggregate",
                "/query/documents/{collection}",
                "/service/signup",
                "/service/esignup",
                "/service/contact",
                "/service/econtact",
                "/service/all_users",
                "/service/all_enc_users",
                "/service/all_messages",
                "/files/upload",
                "/files/{id}",
                "/files/{id}/content",
                "/files/list/{owner_did}",
                "DELETE /files/{id}",
                "/api/documents/{collection}",
                "/api/documents/{collection}/{id}",
                "DELETE /api/documents/{collection}/{id}"
            ]
        }
    });

    Ok(warp::reply::json(&health))
}

// ============================================================================
// DID-Scoped Document Store handlers
// ============================================================================

/// Percent-decode path segment so stored ids match client expectations (e.g. "spacetime:hash" not "spacetime%3Ahash").
fn decode_document_path_segment(segment: &str) -> String {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8_lossy()
        .into_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum DocumentFilterOp {
    Equals,
    Contains,
    In,
    /// For strings (e.g. ISO8601 ts) and numbers. Enables time-range filters.
    GreaterThanOrEqual,
    LessThan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DocumentFilter {
    pub path: String,
    pub op: DocumentFilterOp,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DocumentSort {
    pub field: String,
    pub order: DocumentSortOrder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
enum DocumentSortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DocumentQuery {
    #[serde(default)]
    pub filters: Vec<DocumentFilter>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub sort_by: Option<DocumentSort>,
}

fn json_value_at_path<'a>(
    root: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    let mut cur = root;
    for part in path.split('.').filter(|s| !s.is_empty()) {
        match cur {
            serde_json::Value::Object(map) => {
                cur = map.get(part)?;
            }
            _ => return None,
        }
    }
    Some(cur)
}

fn document_matches_filters(doc: &DocumentRecord, filters: &[DocumentFilter]) -> bool {
    for f in filters {
        let Some(v) = json_value_at_path(&doc.data, &f.path) else {
            return false;
        };

        let ok = match f.op {
            DocumentFilterOp::Equals => v == &f.value,
            DocumentFilterOp::Contains => match (v, &f.value) {
                (serde_json::Value::String(haystack), serde_json::Value::String(needle)) => {
                    haystack.contains(needle)
                }
                (serde_json::Value::Array(arr), needle) => arr.iter().any(|x| x == needle),
                _ => false,
            },
            DocumentFilterOp::In => {
                if let serde_json::Value::Array(arr) = &f.value {
                    arr.iter().any(|x| x == v)
                } else {
                    v == &f.value
                }
            }
            DocumentFilterOp::GreaterThanOrEqual => json_cmp(v, &f.value)
                .map(|o| o != std::cmp::Ordering::Less)
                .unwrap_or(false),
            DocumentFilterOp::LessThan => json_cmp(v, &f.value)
                .map(|o| o == std::cmp::Ordering::Less)
                .unwrap_or(false),
        };

        if !ok {
            return false;
        }
    }
    true
}

/// Compare two JSON values for ordering (strings lexicographically, numbers numerically). ISO8601 strings sort correctly.
fn json_cmp(a: &serde_json::Value, b: &serde_json::Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (serde_json::Value::String(sa), serde_json::Value::String(sb)) => Some(sa.cmp(sb)),
        (serde_json::Value::Number(na), serde_json::Value::Number(nb)) => {
            let fa = na.as_f64()?;
            let fb = nb.as_f64()?;
            Some(fa.partial_cmp(&fb)?)
        }
        _ => None,
    }
}

async fn handle_put_document(
    collection: String,
    id: String,
    requester_did: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let collection = decode_document_path_segment(&collection);
    let id = decode_document_path_segment(&id);
    let existing = match db.get_document(&requester_did, &collection, &id) {
        Ok(v) => v,
        Err(e) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    let now = chrono::Utc::now();
    let created_at = existing.as_ref().map(|d| d.created_at).unwrap_or(now);

    let doc = DocumentRecord {
        owner_did: requester_did,
        collection,
        id,
        data: body,
        created_at,
        updated_at: now,
        blob_ref: None,
    };

    if let Err(e) = db.upsert_document(&doc) {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    Ok(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({ "document": doc })),
        warp::http::StatusCode::OK,
    ))
}

async fn handle_get_document(
    collection: String,
    id: String,
    requester_did: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let collection_dec = decode_document_path_segment(&collection);
    let id_dec = decode_document_path_segment(&id);
    let out = if is_public_catalog_collection(&collection_dec) {
        db.find_document_in_collection(&collection_dec, &id_dec)
            .ok()
            .flatten()
            .filter(|doc| catalog_document_visible_to_requester(doc, &requester_did))
    } else {
        None
    }
    .or_else(|| {
        db.get_document(&requester_did, &collection_dec, &id_dec)
            .ok()
            .flatten()
    })
    .or_else(|| {
        if id_dec != id {
            db.get_document(&requester_did, &collection, &id)
                .ok()
                .flatten()
        } else {
            None
        }
    });
    match out {
        Some(doc) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "document": doc })),
            warp::http::StatusCode::OK,
        )),
        None => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Document not found" })),
            warp::http::StatusCode::NOT_FOUND,
        )),
    }
}

async fn handle_delete_document(
    collection: String,
    id: String,
    requester_did: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let collection_dec = decode_document_path_segment(&collection);
    let id_dec = decode_document_path_segment(&id);
    let deleted = db
        .delete_document(&requester_did, &collection_dec, &id_dec)
        .ok()
        .unwrap_or(false)
        || (id_dec != id
            && db
                .delete_document(&requester_did, &collection, &id)
                .ok()
                .unwrap_or(false));
    match deleted {
        true => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({})),
            warp::http::StatusCode::NO_CONTENT,
        )),
        false => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Document not found" })),
            warp::http::StatusCode::NOT_FOUND,
        )),
    }
}

/// DELETE /api/documents/{collection} — delete all documents in the collection for the requester.
/// Uses stored doc ids server-side so no path encoding mismatch; one request clears the collection.
async fn handle_delete_documents_collection(
    collection: String,
    requester_did: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let collection = decode_document_path_segment(&collection);
    let docs = match db.list_documents(&requester_did, &collection) {
        Ok(d) => d,
        Err(e) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };
    let mut deleted = 0usize;
    for doc in &docs {
        if db
            .delete_document(&requester_did, &collection, &doc.id)
            .unwrap_or(false)
        {
            deleted += 1;
        }
    }
    Ok(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({ "deleted": deleted })),
        warp::http::StatusCode::OK,
    ))
}

async fn handle_list_documents(
    collection: String,
    requester_did: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let collection = decode_document_path_segment(&collection);
    match db.list_documents(&requester_did, &collection) {
        Ok(docs) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "documents": docs })),
            warp::http::StatusCode::OK,
        )),
        Err(e) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

fn is_public_catalog_collection(collection: &str) -> bool {
    matches!(collection, "app_listings" | "content_listings")
}

/// Mirror DID used when publishing catalog docs for website-api (`spacekit-cli` deploy --publish).
const WEBSITE_CATALOG_OWNER_DID: &str = "did:spacekit:admin:website-api";

fn catalog_doc_updated_at(doc: &DocumentRecord) -> &str {
    json_value_at_path(&doc.data, "updated_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

/// Deploy writes the same listing under publisher DID + website-api mirror DID; dedupe reads by doc id.
fn catalog_doc_should_prefer(candidate: &DocumentRecord, existing: &DocumentRecord) -> bool {
    let tc = catalog_doc_updated_at(candidate);
    let te = catalog_doc_updated_at(existing);
    match tc.cmp(te) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => {
            candidate.owner_did == WEBSITE_CATALOG_OWNER_DID
                && existing.owner_did != WEBSITE_CATALOG_OWNER_DID
        }
    }
}

fn dedupe_catalog_documents(docs: Vec<DocumentRecord>) -> Vec<DocumentRecord> {
    use std::collections::HashMap;
    let mut by_id: HashMap<String, DocumentRecord> = HashMap::new();
    for doc in docs {
        let id = doc.id.clone();
        match by_id.remove(&id) {
            None => {
                by_id.insert(id, doc);
            }
            Some(existing) => {
                let keep_new = catalog_doc_should_prefer(&doc, &existing);
                by_id.insert(id, if keep_new { doc } else { existing });
            }
        }
    }
    by_id.into_values().collect()
}

fn catalog_document_visible_to_requester(doc: &DocumentRecord, requester_did: &str) -> bool {
    if doc.owner_did == requester_did {
        return true;
    }
    let access = json_value_at_path(&doc.data, "access")
        .and_then(|v| v.as_str())
        .unwrap_or("public");
    access == "public"
}

fn requester_can_access_file(
    db: &Database,
    data_dir: Option<&std::path::Path>,
    metadata: &FileMetadata,
    requester_did: &str,
) -> bool {
    if requester_did == metadata.owner_did || requester_did == WEBSITE_CATALOG_OWNER_DID {
        return true;
    }
    if db
        .has_file_access(&metadata.id, requester_did)
        .unwrap_or(false)
    {
        return true;
    }
    if let Some(group_id) = metadata.sharing_mode.strip_prefix("group:") {
        let members = db.get_group_members(group_id).unwrap_or_default();
        if members.iter().any(|m| m.user_did == requester_did) {
            return true;
        }
    }
    if let Some(dir) = data_dir {
        let store = crate::content_grants::ContentGrantStore::from_env_or_data_dir(dir);
        if store.has_keychain_file_access(requester_did, &metadata.owner_did, &metadata.id) {
            return true;
        }
    }
    false
}

#[derive(Debug, Deserialize)]
struct KeychainGrantSyncBody {
    grant_id: String,
    granter_did: String,
    grantee_did: String,
    scopes: Vec<String>,
    resource_type: String,
    #[serde(default)]
    resource_id: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
    #[serde(default)]
    artifact_file_ids: Option<Vec<String>>,
}

async fn handle_keychain_grant_sync(
    requester_did: String,
    body: KeychainGrantSyncBody,
    data_dir: Option<std::path::PathBuf>,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    if requester_did != WEBSITE_CATALOG_OWNER_DID {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Access denied — keychain sync requires website-api admin DID"
            })),
            warp::http::StatusCode::FORBIDDEN,
        )));
    }
    let Some(dir) = data_dir.as_deref() else {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Storage data dir not configured" })),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        )));
    };
    let store = crate::content_grants::ContentGrantStore::from_env_or_data_dir(dir);
    if let Err(e) = store.upsert_keychain_delegate(
        &body.grant_id,
        &body.granter_did,
        &body.grantee_did,
        &body.resource_type,
        body.resource_id.as_deref(),
        &body.scopes,
        body.expires_at,
        body.artifact_file_ids.clone(),
    ) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )));
    }
    if body.resource_type == "app" || body.resource_type == "content" {
        let file_ids: Vec<String> = body
            .artifact_file_ids
            .clone()
            .or_else(|| body.resource_id.clone().map(|id| vec![id]))
            .unwrap_or_default();
        if !file_ids.is_empty() {
            let perm = if body.scopes.iter().any(|s| s == "admin" || s == "manage") {
                "readwrite"
            } else {
                "read"
            };
            for file_id in file_ids {
                let grant = crate::database::FileAccessGrant {
                    file_id: file_id.clone(),
                    grantee_did: body.grantee_did.clone(),
                    granter_did: body.granter_did.clone(),
                    permissions: perm.to_string(),
                    granted_at: chrono::Utc::now(),
                };
                let _ = db.upsert_file_access_grant(&grant);
            }
        }
    }
    Ok(boxed_reply(warp::reply::json(
        &serde_json::json!({ "ok": true }),
    )))
}

async fn handle_keychain_grant_revoke(
    grant_id: String,
    requester_did: String,
    data_dir: Option<std::path::PathBuf>,
) -> Result<Box<dyn Reply>, Rejection> {
    if requester_did != WEBSITE_CATALOG_OWNER_DID {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Access denied — keychain sync requires website-api admin DID"
            })),
            warp::http::StatusCode::FORBIDDEN,
        )));
    }
    let Some(dir) = data_dir.as_deref() else {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Storage data dir not configured" })),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        )));
    };
    let store = crate::content_grants::ContentGrantStore::from_env_or_data_dir(dir);
    match store.revoke_keychain_delegate(&grant_id) {
        Ok(true) => Ok(boxed_reply(warp::reply::json(
            &serde_json::json!({ "ok": true }),
        ))),
        Ok(false) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Grant not found" })),
            warp::http::StatusCode::NOT_FOUND,
        ))),
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ))),
    }
}

async fn handle_query_documents(
    collection: String,
    requester_did: String,
    query: DocumentQuery,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    /// Cap for document query limit (e.g. stats use limit + sort + filter to stay lightweight).
    const MAX_LIMIT: usize = 50_000;
    const MAX_FILTERS: usize = 20;

    if query.filters.len() > MAX_FILTERS {
        return Ok(Box::new(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("Too many filters (max: {})", MAX_FILTERS)
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let docs = if is_public_catalog_collection(&collection) {
        match db.list_documents_in_collection(&collection) {
            Ok(all) => all
                .into_iter()
                .filter(|d| catalog_document_visible_to_requester(d, &requester_did))
                .collect(),
            Err(e) => {
                return Ok(Box::new(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        }
    } else {
        match db.list_documents(&requester_did, &collection) {
            Ok(d) => d,
            Err(e) => {
                return Ok(Box::new(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        }
    };

    // Externalized catalog bodies are stripped in list_*(); hydrate before filter/sort so
    // website-api can match on `status`, `category`, etc. and return full listing JSON.
    let docs: Vec<DocumentRecord> = docs
        .into_iter()
        .filter_map(|d| {
            let id = d.id.clone();
            match db.hydrate_document_record(d) {
                Ok(h) => Some(h),
                Err(e) => {
                    tracing::warn!("query documents/{collection}: failed to hydrate {id}: {e}");
                    None
                }
            }
        })
        .collect();

    let mut filtered: Vec<DocumentRecord> = if query.filters.is_empty() {
        docs
    } else {
        docs.into_iter()
            .filter(|d| document_matches_filters(d, &query.filters))
            .collect()
    };

    if is_public_catalog_collection(&collection) {
        filtered = dedupe_catalog_documents(filtered);
    }

    let total_count = filtered.len();

    if let Some(ref sort_by) = query.sort_by {
        let path = sort_by.field.clone();
        let is_desc = matches!(sort_by.order, DocumentSortOrder::Desc);
        filtered.sort_by(|a, b| {
            let va = json_value_at_path(&a.data, &path);
            let vb = json_value_at_path(&b.data, &path);
            let cmp = match (va, vb) {
                (Some(x), Some(y)) => json_cmp(x, y).unwrap_or(std::cmp::Ordering::Equal),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            };
            if is_desc {
                cmp.reverse()
            } else {
                cmp
            }
        });
    }

    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(50).min(MAX_LIMIT);

    if offset < filtered.len() {
        filtered = filtered.into_iter().skip(offset).take(limit).collect();
    } else {
        filtered.clear();
    }

    Ok(Box::new(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "documents": filtered,
            "total_count": total_count
        })),
        warp::http::StatusCode::OK,
    )))
}

// SQL Query Interface handlers

/// Handle file query (SECURED with authentication, rate limiting, and authorization)
async fn handle_query_files(
    requester_did: String, // From authentication (first from filter chain)
    query: crate::sql_query::FileQuery, // From body (second from filter chain)
    query_builder: Option<Arc<crate::sql_query::StorageQueryBuilder>>, // From with_query_builder (third)
) -> Result<Box<dyn Reply>, Rejection> {
    // SECURITY: Query limits
    const MAX_QUERY_RESULTS: usize = 1000;
    const MAX_QUERY_EXECUTION_TIME_MS: u64 = 5000;
    const MAX_FILTERS: usize = 10;

    // Validate query limits
    if query.filters.len() > MAX_FILTERS {
        return Ok(Box::new(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("Too many filters (max: {})", MAX_FILTERS)
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let query_builder = match query_builder {
        Some(qb) => qb,
        None => {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Query interface not available"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // SECURITY: Enforce row-level security - users can only query their own files
    use crate::sql_query::{Filter, FilterOp, FilterValue};
    let mut authorized_query = query.clone();

    // Add mandatory filter for owner_did (prevent data leakage)
    let owner_filter_exists = authorized_query
        .filters
        .iter()
        .any(|f| f.field == "owner_did");

    if !owner_filter_exists {
        authorized_query.filters.push(Filter {
            field: "owner_did".to_string(),
            op: FilterOp::Equals,
            value: FilterValue::String(requester_did.clone()),
        });
    } else {
        // Ensure owner_did filter matches requester (prevent override)
        authorized_query.filters = authorized_query
            .filters
            .into_iter()
            .map(|f| {
                if f.field == "owner_did" {
                    Filter {
                        field: "owner_did".to_string(),
                        op: FilterOp::Equals,
                        value: FilterValue::String(requester_did.clone()),
                    }
                } else {
                    f
                }
            })
            .collect();
    }

    // Enforce maximum limit
    if let Some(limit) = authorized_query.limit {
        authorized_query.limit = Some(limit.min(MAX_QUERY_RESULTS));
    } else {
        authorized_query.limit = Some(MAX_QUERY_RESULTS);
    }

    // Execute query with timeout
    let query_future = query_builder.query_files(authorized_query);
    let timeout = tokio::time::timeout(
        Duration::from_millis(MAX_QUERY_EXECUTION_TIME_MS),
        query_future,
    );

    match timeout.await {
        Ok(Ok(result)) => {
            tracing::info!(
                "File query executed by {}: {} results in {}ms",
                requester_did,
                result.total_count,
                result.execution_time_ms
            );
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&result),
                warp::http::StatusCode::OK,
            )))
        }
        Ok(Err(e)) => {
            tracing::error!("Query error: {}", e);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
        Err(_) => {
            tracing::warn!("Query timeout for DID: {}", requester_did);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query timeout (max: {}ms)", MAX_QUERY_EXECUTION_TIME_MS)
                })),
                warp::http::StatusCode::REQUEST_TIMEOUT,
            )))
        }
    }
}

/// Handle fact query (SECURED with authentication, rate limiting, and authorization)
async fn handle_query_facts(
    requester_did: String, // From authentication (first from filter chain)
    query: crate::sql_query::FactQuery, // From body (second from filter chain)
    query_builder: Option<Arc<crate::sql_query::StorageQueryBuilder>>, // From with_query_builder (third)
) -> Result<Box<dyn Reply>, Rejection> {
    // SECURITY: Query limits
    const MAX_QUERY_RESULTS: usize = 1000;
    const MAX_QUERY_EXECUTION_TIME_MS: u64 = 5000;
    const MAX_FILTERS: usize = 10;

    // Validate query limits
    if query.filters.len() > MAX_FILTERS {
        return Ok(Box::new(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("Too many filters (max: {})", MAX_FILTERS)
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let query_builder = match query_builder {
        Some(qb) => qb,
        None => {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Query interface not available"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // SECURITY: Enforce row-level security - users can only query facts they authored
    // (For facts, we allow querying by author field)
    use crate::sql_query::{Filter, FilterOp, FilterValue};
    let mut authorized_query = query.clone();

    // Add mandatory filter for author (prevent querying other users' facts)
    let author_filter_exists = authorized_query.filters.iter().any(|f| f.field == "author");

    if !author_filter_exists {
        authorized_query.filters.push(Filter {
            field: "author".to_string(),
            op: FilterOp::Equals,
            value: FilterValue::String(requester_did.clone()),
        });
    } else {
        // Ensure author filter matches requester (prevent override)
        authorized_query.filters = authorized_query
            .filters
            .into_iter()
            .map(|f| {
                if f.field == "author" {
                    Filter {
                        field: "author".to_string(),
                        op: FilterOp::Equals,
                        value: FilterValue::String(requester_did.clone()),
                    }
                } else {
                    f
                }
            })
            .collect();
    }

    // Enforce maximum limit
    if let Some(limit) = authorized_query.limit {
        authorized_query.limit = Some(limit.min(MAX_QUERY_RESULTS));
    } else {
        authorized_query.limit = Some(MAX_QUERY_RESULTS);
    }

    // Execute query with timeout
    let query_future = query_builder.query_facts(authorized_query);
    let timeout = tokio::time::timeout(
        Duration::from_millis(MAX_QUERY_EXECUTION_TIME_MS),
        query_future,
    );

    match timeout.await {
        Ok(Ok(result)) => {
            tracing::info!(
                "Fact query executed by {}: {} results in {}ms",
                requester_did,
                result.total_count,
                result.execution_time_ms
            );
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&result),
                warp::http::StatusCode::OK,
            )))
        }
        Ok(Err(e)) => {
            tracing::error!("Query error: {}", e);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
        Err(_) => {
            tracing::warn!("Query timeout for DID: {}", requester_did);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query timeout (max: {}ms)", MAX_QUERY_EXECUTION_TIME_MS)
                })),
                warp::http::StatusCode::REQUEST_TIMEOUT,
            )))
        }
    }
}

/// Handle user query (SECURED with authentication, rate limiting, and authorization)
/// NOTE: User queries are restricted - users can only query their own user record
async fn handle_query_users(
    requester_did: String, // From authentication (first from filter chain)
    query: crate::sql_query::UserQuery, // From body (second from filter chain)
    query_builder: Option<Arc<crate::sql_query::StorageQueryBuilder>>, // From with_query_builder (third)
) -> Result<Box<dyn Reply>, Rejection> {
    // SECURITY: Query limits
    const MAX_QUERY_RESULTS: usize = 1000;
    const MAX_QUERY_EXECUTION_TIME_MS: u64 = 5000;
    const MAX_FILTERS: usize = 10;

    // Validate query limits
    if query.filters.len() > MAX_FILTERS {
        return Ok(Box::new(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("Too many filters (max: {})", MAX_FILTERS)
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let query_builder = match query_builder {
        Some(qb) => qb,
        None => {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Query interface not available"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // SECURITY: Enforce row-level security - users can only query their own user record
    // Extract username from DID (did:spacekit:user:username) or use DID directly
    use crate::sql_query::{Filter, FilterOp, FilterValue};
    let mut authorized_query = query.clone();

    // For user queries, restrict to requester's own record
    // Extract username from DID or use address field
    // This is a simplified approach - in production, you'd have a DID->username mapping
    authorized_query.filters.push(Filter {
        field: "address".to_string(), // Assuming address contains DID or username
        op: FilterOp::Contains,
        value: FilterValue::String(requester_did.clone()),
    });

    // Enforce maximum limit
    if let Some(limit) = authorized_query.limit {
        authorized_query.limit = Some(limit.min(MAX_QUERY_RESULTS));
    } else {
        authorized_query.limit = Some(MAX_QUERY_RESULTS);
    }

    // Execute query with timeout
    let query_future = query_builder.query_users(authorized_query);
    let timeout = tokio::time::timeout(
        Duration::from_millis(MAX_QUERY_EXECUTION_TIME_MS),
        query_future,
    );

    match timeout.await {
        Ok(Ok(result)) => {
            tracing::info!(
                "User query executed by {}: {} results in {}ms",
                requester_did,
                result.total_count,
                result.execution_time_ms
            );
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&result),
                warp::http::StatusCode::OK,
            )))
        }
        Ok(Err(e)) => {
            tracing::error!("Query error: {}", e);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
        Err(_) => {
            tracing::warn!("Query timeout for DID: {}", requester_did);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query timeout (max: {}ms)", MAX_QUERY_EXECUTION_TIME_MS)
                })),
                warp::http::StatusCode::REQUEST_TIMEOUT,
            )))
        }
    }
}

/// Handle aggregate query (SECURED with authentication, rate limiting, and authorization)
async fn handle_query_aggregate(
    requester_did: String, // From authentication (first from filter chain)
    query: crate::sql_query::AggregateQuery, // From body (second from filter chain)
    query_builder: Option<Arc<crate::sql_query::StorageQueryBuilder>>, // From with_query_builder (third)
) -> Result<Box<dyn Reply>, Rejection> {
    // SECURITY: Query limits
    const MAX_QUERY_EXECUTION_TIME_MS: u64 = 5000;
    const MAX_FILTERS: usize = 10;

    // Validate query limits
    if query.filters.len() > MAX_FILTERS {
        return Ok(Box::new(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("Too many filters (max: {})", MAX_FILTERS)
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let query_builder = match query_builder {
        Some(qb) => qb,
        None => {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Query interface not available"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // SECURITY: Enforce row-level security - users can only aggregate their own facts
    use crate::sql_query::{Filter, FilterOp, FilterValue};
    let mut authorized_query = query.clone();

    // Add mandatory filter for author (prevent aggregating other users' facts)
    let author_filter_exists = authorized_query.filters.iter().any(|f| f.field == "author");

    if !author_filter_exists {
        authorized_query.filters.push(Filter {
            field: "author".to_string(),
            op: FilterOp::Equals,
            value: FilterValue::String(requester_did.clone()),
        });
    } else {
        // Ensure author filter matches requester (prevent override)
        authorized_query.filters = authorized_query
            .filters
            .into_iter()
            .map(|f| {
                if f.field == "author" {
                    Filter {
                        field: "author".to_string(),
                        op: FilterOp::Equals,
                        value: FilterValue::String(requester_did.clone()),
                    }
                } else {
                    f
                }
            })
            .collect();
    }

    // Execute query with timeout
    let query_future = query_builder.aggregate_facts(authorized_query);
    let timeout = tokio::time::timeout(
        Duration::from_millis(MAX_QUERY_EXECUTION_TIME_MS),
        query_future,
    );

    match timeout.await {
        Ok(Ok(result)) => {
            tracing::info!(
                "Aggregate query executed by {}: value={} in {}ms",
                requester_did,
                result.value,
                MAX_QUERY_EXECUTION_TIME_MS
            );
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&result),
                warp::http::StatusCode::OK,
            )))
        }
        Ok(Err(e)) => {
            tracing::error!("Aggregate query error: {}", e);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Aggregate query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
        Err(_) => {
            tracing::warn!("Aggregate query timeout for DID: {}", requester_did);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query timeout (max: {}ms)", MAX_QUERY_EXECUTION_TIME_MS)
                })),
                warp::http::StatusCode::REQUEST_TIMEOUT,
            )))
        }
    }
}

/// Handle user signup
async fn handle_signup(
    new_user: User,
    public_key: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    tracing::info!("User signup attempt: {}", new_user.username);

    match db.user_exists(&new_user.username) {
        Ok(true) => Ok(warp::reply::with_status(
            "User already exists".to_string(),
            warp::http::StatusCode::BAD_REQUEST,
        )),
        Ok(false) => {
            // Encrypt address with public key (from old implementation)
            let public_key_bytes = match hex::decode(&public_key) {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Ok(warp::reply::with_status(
                        "Invalid public key".to_string(),
                        warp::http::StatusCode::BAD_REQUEST,
                    ));
                }
            };

            let encrypted_address =
                match ecies::encrypt(&public_key_bytes, new_user.address.as_bytes()) {
                    Ok(encrypted) => hex::encode(encrypted),
                    Err(_) => {
                        return Ok(warp::reply::with_status(
                            "Encryption failed".to_string(),
                            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                        ));
                    }
                };

            let user = User {
                address: encrypted_address,
                ..new_user
            };

            match db.insert_user(&user) {
                Ok(_) => Ok(warp::reply::with_status(
                    "User created successfully".to_string(),
                    warp::http::StatusCode::CREATED,
                )),
                Err(e) => {
                    tracing::error!("Failed to insert user: {}", e);
                    Ok(warp::reply::with_status(
                        "Database error".to_string(),
                        warp::http::StatusCode::SERVICE_UNAVAILABLE,
                    ))
                }
            }
        }
        Err(e) => {
            tracing::error!("Database error checking user: {}", e);
            Ok(warp::reply::with_status(
                "Database error".to_string(),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    }
}

/// Handle encrypted user signup
async fn handle_encrypted_signup(
    enc_user: EncryptedUser,
    public_key: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    tracing::info!("Encrypted user signup attempt: {}", enc_user.session);

    match db.enc_user_exists(&enc_user.session) {
        Ok(true) => Ok(warp::reply::with_status(
            "User already exists".to_string(),
            warp::http::StatusCode::BAD_REQUEST,
        )),
        Ok(false) => {
            let user = EncryptedUser {
                public_key,
                ..enc_user
            };

            match db.insert_enc_user(&user) {
                Ok(_) => Ok(warp::reply::with_status(
                    "Encrypted user created successfully".to_string(),
                    warp::http::StatusCode::CREATED,
                )),
                Err(e) => {
                    tracing::error!("Failed to insert encrypted user: {}", e);
                    Ok(warp::reply::with_status(
                        "Database error".to_string(),
                        warp::http::StatusCode::SERVICE_UNAVAILABLE,
                    ))
                }
            }
        }
        Err(e) => {
            tracing::error!("Database error checking encrypted user: {}", e);
            Ok(warp::reply::with_status(
                "Database error".to_string(),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    }
}

/// Handle contact message
async fn handle_contact(
    message: ContactMessage,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    tracing::info!("Contact message from: {}", message.name);

    match db.insert_message(&message) {
        Ok(_) => Ok(warp::reply::with_status(
            "Message received successfully".to_string(),
            warp::http::StatusCode::CREATED,
        )),
        Err(e) => {
            tracing::error!("Failed to insert message: {}", e);
            Ok(warp::reply::with_status(
                "Database error".to_string(),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    }
}

/// Handle encrypted contact message
async fn handle_encrypted_contact(
    message: EncryptedMessage,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    tracing::info!("Encrypted message from session: {}", message.session);

    match db.insert_enc_message(&message) {
        Ok(_) => Ok(warp::reply::with_status(
            "Encrypted message received successfully".to_string(),
            warp::http::StatusCode::CREATED,
        )),
        Err(e) => {
            tracing::error!("Failed to insert encrypted message: {}", e);
            Ok(warp::reply::with_status(
                "Database error".to_string(),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    }
}

/// Get all users  
async fn handle_get_all_users(
    _requester_did: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    if !debug_endpoints_enabled() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Not found" })),
            warp::http::StatusCode::NOT_FOUND,
        ));
    }
    match db.select_all_users() {
        Ok(users) => {
            let user_responses: Vec<UserResponse> = users
                .into_iter()
                .map(|user| UserResponse {
                    username: user.username,
                    email: user.email,
                    address: user.address,
                    network: user.network,
                })
                .collect();
            Ok(warp::reply::with_status(
                warp::reply::json(&user_responses),
                warp::http::StatusCode::OK,
            ))
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(
                    &serde_json::json!({ "error": format!("Database error: {}", e) }),
                ),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    }
}

/// Get all encrypted users  
async fn handle_get_all_enc_users(
    _requester_did: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    if !debug_endpoints_enabled() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Not found" })),
            warp::http::StatusCode::NOT_FOUND,
        ));
    }
    match db.select_all_enc_users() {
        Ok(users) => {
            let user_responses: Vec<EncryptedUserResponse> = users
                .into_iter()
                .map(|user| EncryptedUserResponse {
                    session: user.session,
                    message: user.message,
                })
                .collect();
            Ok(warp::reply::with_status(
                warp::reply::json(&user_responses),
                warp::http::StatusCode::OK,
            ))
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(
                    &serde_json::json!({ "error": format!("Database error: {}", e) }),
                ),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    }
}

/// Get all messages  
async fn handle_get_all_messages(
    _requester_did: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    if !debug_endpoints_enabled() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "Not found" })),
            warp::http::StatusCode::NOT_FOUND,
        ));
    }
    match db.select_all_messages() {
        Ok(messages) => Ok(warp::reply::with_status(
            warp::reply::json(&messages),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(
                    &serde_json::json!({ "error": format!("Database error: {}", e) }),
                ),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    }
}

fn debug_endpoints_enabled() -> bool {
    match std::env::var("SPACEKIT_ENABLE_DEBUG_ENDPOINTS") {
        Ok(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => false,
    }
}

// Enhanced file management handlers

/// Handle file upload (SECURE - requires user's public key)
async fn handle_file_upload(
    file_data: Bytes,
    content_type: Option<String>,
    filename: Option<String>,
    owner_did: String,
    owner_public_key_hex: String,
    owner_key_algorithm: Option<String>,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    quantum_crypto: Option<Arc<QuantumCrypto>>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Box<dyn Reply>, Rejection> {
    let timeout = tokio::time::timeout(
        Duration::from_millis(MAX_FILE_UPLOAD_TIMEOUT_MS),
        async move {
            let result: Result<Box<dyn Reply>, Rejection> = {
    let _permit = match request_semaphore.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server busy - too many concurrent requests"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // Decode public key
    let owner_public_key = match hex::decode(&owner_public_key_hex) {
        Ok(key) => key,
        Err(e) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Invalid public key format: {}", e)
                })),
                warp::http::StatusCode::BAD_REQUEST,
            )));
        }
    };

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "File storage not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    let quantum_crypto = match quantum_crypto {
        Some(qc) => qc,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Encryption service not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    let filename = filename.unwrap_or_else(|| format!("file_{}", uuid::Uuid::new_v4()));

    let owner_kem = if let Some(ref alg) = owner_key_algorithm {
        match quantum_crypto.parse_algorithm(alg.trim()) {
            Ok(a) => a,
            Err(e) => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": format!("Invalid owner-key-algorithm: {}", e)
                    })),
                    warp::http::StatusCode::BAD_REQUEST,
                )));
            }
        }
    } else {
        quantum_crypto.server_default_kem_algorithm()
    };

    // Encrypt file data with user's public key (KEM must match the key material)
    let encrypted_data = match quantum_crypto
        .encrypt_data_with_algorithm(&file_data, &owner_public_key, owner_kem)
        .await
    {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Encryption failed: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Encryption failed: {}. Hint: pass header owner-key-algorithm to match your keypair (e.g. Kyber1024 for `spacekit init` defaults).", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };

    let file_id = uuid::Uuid::new_v4().to_string();
    let size = file_data.len() as u64;
    let hash = hex::encode(blake3::hash(&file_data).as_bytes());
    let hash_clone = hash.clone();

    // Create file metadata with public key
    let metadata = FileMetadata {
        id: file_id.clone(),
        filename: filename.clone(),
        size,
        hash: hash_clone,
        owner_did: owner_did.clone(),
        encryption_algorithm: encrypted_data.metadata.algorithm.clone(),
        content_type,
        created_at: chrono::Utc::now(),
        last_accessed: None,
        encryption_public_key: Some(owner_public_key_hex.clone()),
        sharing_mode: "owner".to_string(),
    };

    // Store metadata
    match db.insert_file_metadata(&metadata) {
        Ok(_) => {
            // Write encrypted data to disk
            let data_path = data_dir.join(&file_id);
            if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
                tracing::error!("Failed to create data directory: {}", e);
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": format!("Failed to create data directory: {}", e)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }

            let encrypted_data_json = match serde_json::to_vec(&encrypted_data) {
                Ok(data) => data,
                Err(e) => {
                    tracing::error!("Failed to serialize encrypted data: {}", e);
                    return Ok(boxed_reply(warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({
                            "error": format!("Failed to serialize encrypted data: {}", e)
                        })),
                        warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                    )));
                }
            };

            if let Err(e) = tokio::fs::write(&data_path, &encrypted_data_json).await {
                tracing::error!("Failed to write encrypted data: {}", e);
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": format!("Failed to write encrypted data: {}", e)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }

            tracing::info!("File uploaded and encrypted: {} ({})", filename, file_id);
            Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "file_id": file_id,
                    "filename": filename,
                    "size": size,
                    "hash": hash,
                    "public_key": owner_public_key_hex,
                    "message": "File encrypted with your public key. Store your private key securely - it's required for decryption."
                })),
                warp::http::StatusCode::OK,
            )))
        }
        Err(e) => {
            tracing::error!("Failed to insert file metadata: {}", e);
            Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Failed to store file metadata"
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }
            };
            result
        },
    )
    .await;

    match timeout {
        Ok(result) => result,
        Err(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("Upload timeout (max: {}ms)", MAX_FILE_UPLOAD_TIMEOUT_MS)
            })),
            warp::http::StatusCode::REQUEST_TIMEOUT,
        ))),
    }
}

/// Handle file download (new enhanced feature)
async fn handle_file_download(
    file_id: String,
    requester_did: Option<String>,
    db: Arc<Database>,
    data_dir: Option<std::path::PathBuf>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Box<dyn Reply>, Rejection> {
    let _permit = match request_semaphore.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server busy - too many concurrent requests"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    let requester_did = match requester_did {
        Some(did) => did,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing requester-did header"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            )));
        }
    };

    match db.get_file_metadata(&file_id) {
        Ok(Some(metadata)) => {
            if !requester_can_access_file(&db, data_dir.as_deref(), &metadata, &requester_did) {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": "Access denied — file owner, grant, or keychain delegation required"
                    })),
                    warp::http::StatusCode::FORBIDDEN,
                )));
            }

            // File content retrieval requires the secure /files/{id}/content flow.
            Ok(boxed_reply(warp::reply::json(&metadata)))
        }
        Ok(None) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "File not found"
            })),
            warp::http::StatusCode::NOT_FOUND,
        ))),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Database error"
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }
}

/// Handle list files by owner (new enhanced feature)
async fn handle_list_files(owner_did: String, db: Arc<Database>) -> Result<impl Reply, Rejection> {
    match db.list_files_by_owner(&owner_did) {
        Ok(files) => Ok(warp::reply::json(&files)),
        Err(e) => {
            tracing::error!("Database error: {}", e);
            Ok(warp::reply::json(&format!("Database error: {}", e)))
        }
    }
}

/// Handle file deletion
#[derive(Debug, Deserialize, Default)]
struct FileDeleteQuery {
    /// When true, delete even if catalog documents still reference this file_id.
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize, Default)]
struct OrphanFilesQuery {
    /// Owner DID to scan (defaults to authenticated DID).
    owner_did: Option<String>,
}

async fn handle_file_delete(
    file_id: String,
    requester_did: Option<String>,
    query: FileDeleteQuery,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Box<dyn Reply>, Rejection> {
    let _permit = match request_semaphore.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server busy - too many concurrent requests"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // Get file metadata to check ownership
    let metadata = match db.get_file_metadata(&file_id) {
        Ok(Some(m)) => m,
        Ok(None) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "File not found"
                })),
                warp::http::StatusCode::NOT_FOUND,
            )));
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Database error"
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };

    // Check if requester has permission (owner check)
    // If no requester-did provided, allow deletion (for admin operations)
    if let Some(ref requester) = requester_did {
        if *requester != metadata.owner_did {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Access denied - only the file owner can delete"
                })),
                warp::http::StatusCode::FORBIDDEN,
            )));
        }
    }

    match db.file_artifact_refs(&file_id) {
        Ok(refs) if !refs.is_empty() && !query.force => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "File is still referenced by catalog documents",
                    "references": refs,
                    "hint": "Remove listing/deploy references first, or DELETE with ?force=true as the owner"
                })),
                warp::http::StatusCode::CONFLICT,
            )));
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!("artifact ref lookup failed for {}: {}", file_id, e);
        }
    }

    if query.force {
        if requester_did.is_none() {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "force delete requires requester-did header"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            )));
        }
    }

    // Delete file data from disk if data_dir is configured
    if let Some(ref data_dir) = data_dir {
        let data_path = data_dir.join(&file_id);
        if data_path.exists() {
            if let Err(e) = tokio::fs::remove_file(&data_path).await {
                tracing::warn!(
                    "Failed to delete file data from disk {:?}: {}",
                    data_path,
                    e
                );
                // Continue anyway - metadata will be deleted
            }
        }

        // Also try to delete the key file if it exists
        let key_path = data_dir.join(format!("{}.key", file_id));
        if key_path.exists() {
            if let Err(e) = tokio::fs::remove_file(&key_path).await {
                tracing::warn!("Failed to delete key file from disk {:?}: {}", key_path, e);
            }
        }
    }

    // Delete metadata from database
    match db.delete_file_metadata(&file_id) {
        Ok(_) => {
            tracing::info!("File deleted: {} by {:?}", file_id, requester_did);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "message": "File deleted successfully"
                })),
                warp::http::StatusCode::OK,
            )))
        }
        Err(e) => {
            tracing::error!("Failed to delete file metadata: {}", e);
            Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Failed to delete file: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }
}

/// GET /files/{id}/refs — catalog documents referencing this file blob.
async fn handle_file_refs(
    file_id: String,
    requester_did: String,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    let metadata = match db.get_file_metadata(&file_id) {
        Ok(Some(m)) => m,
        Ok(None) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File not found"})),
                warp::http::StatusCode::NOT_FOUND,
            )));
        }
        Err(e) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": format!("Database error: {}", e)})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };

    if metadata.owner_did != requester_did && requester_did != WEBSITE_CATALOG_OWNER_DID {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Access denied"})),
            warp::http::StatusCode::FORBIDDEN,
        )));
    }

    match db.file_artifact_refs(&file_id) {
        Ok(refs) => Ok(boxed_reply(warp::reply::json(&serde_json::json!({
            "file_id": file_id,
            "reference_count": refs.len(),
            "references": refs,
        })))),
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": e.to_string()})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ))),
    }
}

/// GET /api/admin/orphan-files — file blobs with metadata but no catalog references.
async fn handle_admin_orphan_files(
    requester_did: String,
    query: OrphanFilesQuery,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    let owner = query.owner_did.unwrap_or_else(|| requester_did.clone());
    if owner != requester_did && requester_did != WEBSITE_CATALOG_OWNER_DID {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Access denied — query another owner's orphans only as website-api admin DID"
            })),
            warp::http::StatusCode::FORBIDDEN,
        )));
    }

    match db.list_orphan_files_for_owner(&owner) {
        Ok(orphans) => {
            let total_bytes: u64 = orphans.iter().map(|(m, _)| m.size).sum();
            let files: Vec<_> = orphans
                .into_iter()
                .map(|(m, _)| {
                    serde_json::json!({
                        "file_id": m.id,
                        "filename": m.filename,
                        "size_bytes": m.size,
                        "hash": m.hash,
                        "created_at": m.created_at,
                    })
                })
                .collect();
            Ok(boxed_reply(warp::reply::json(&serde_json::json!({
                "owner_did": owner,
                "orphan_count": files.len(),
                "total_bytes": total_bytes,
                "files": files,
            }))))
        }
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": e.to_string()})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ))),
    }
}

/// Handle session keypair generation - returns server's public key for encrypting user's private key
async fn handle_session_key(
    file_id: String,
    db: Arc<Database>,
    session_keypairs: Arc<RwLock<HashMap<String, SessionKeypair>>>,
    quantum_crypto: Option<Arc<QuantumCrypto>>,
) -> Result<Box<dyn Reply>, Rejection> {
    // Verify file exists
    match db.get_file_metadata(&file_id) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "File not found"
                })),
                warp::http::StatusCode::NOT_FOUND,
            )));
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Database error"
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    }

    let quantum_crypto = match quantum_crypto {
        Some(qc) => qc,
        None => {
            return Ok(Box::new(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Encryption service not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // Generate ephemeral keypair for this session
    use spacekit_primitives::v1::crypto::quantum::Algorithm;
    let (public_key, private_key) =
        match quantum_crypto.generate_keypair(Algorithm::Kyber1024).await {
            Ok(kp) => kp,
            Err(e) => {
                tracing::error!("Failed to generate session keypair: {}", e);
                return Ok(Box::new(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": format!("Failed to generate session keypair: {}", e)
                    })),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        };

    // Create session ID
    let session_id = Uuid::new_v4().to_string();
    let now = SystemTime::now();
    let expires_at = now + Duration::from_secs(300); // 5 minute TTL

    // Store session keypair
    let session_keypair = SessionKeypair {
        public_key: public_key.clone(),
        private_key: private_key.clone(),
        created_at: now,
        expires_at,
    };

    {
        let mut keypairs = session_keypairs.write().await;
        keypairs.insert(session_id.clone(), session_keypair);

        // Clean up expired sessions
        keypairs.retain(|_, v| v.expires_at > SystemTime::now());
    }

    tracing::info!(
        "Generated session keypair for file {}: session_id={}",
        file_id,
        session_id
    );

    Ok(Box::new(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "success": true,
            "session_id": session_id,
            "public_key": hex::encode(&public_key),
        })),
        warp::http::StatusCode::OK,
    )))
}

/// Handle file content retrieval - returns decrypted file content (REQUIRES encrypted user's private key)
async fn handle_file_content(
    file_id: String,
    requester_did: Option<String>,
    encrypted_private_key_hex: String, // REQUIRED: User's private key encrypted with session public key (hex-encoded)
    session_id: String,                // REQUIRED: Session ID from session-key endpoint
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    quantum_crypto: Option<Arc<QuantumCrypto>>,
    session_keypairs: Arc<RwLock<HashMap<String, SessionKeypair>>>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Box<dyn Reply>, Rejection> {
    let timeout = tokio::time::timeout(
        Duration::from_millis(MAX_FILE_REQUEST_TIMEOUT_MS),
        async move {
            let result: Result<Box<dyn Reply>, Rejection> = {
    let _permit = match request_semaphore.try_acquire() {
        Ok(permit) => permit,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server busy - too many concurrent requests"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // Get file metadata first
    let metadata = match db.get_file_metadata(&file_id) {
        Ok(Some(m)) => m,
        Ok(None) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "File not found"
                })),
                warp::http::StatusCode::NOT_FOUND,
            )));
        }
        Err(e) => {
            tracing::error!("Database error: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Database error"
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };

    // Get session keypair
    let session_keypair = {
        let keypairs = session_keypairs.read().await;
        match keypairs.get(&session_id) {
            Some(kp) => {
                // Check if expired
                if kp.expires_at < SystemTime::now() {
                    return Ok(boxed_reply(warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({
                            "error": "Session expired"
                        })),
                        warp::http::StatusCode::UNAUTHORIZED,
                    )));
                }
                kp.clone()
            }
            None => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": "Invalid or expired session"
                    })),
                    warp::http::StatusCode::UNAUTHORIZED,
                )));
            }
        }
    };

    // Decode encrypted private key
    let encrypted_private_key = match hex::decode(&encrypted_private_key_hex) {
        Ok(key) => key,
        Err(e) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Invalid encrypted private key format: {}", e)
                })),
                warp::http::StatusCode::BAD_REQUEST,
            )));
        }
    };

    // Decrypt user's private key using session private key
    let quantum_crypto = match &quantum_crypto {
        Some(qc) => qc,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Encryption service not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // Parse the encrypted data structure
    let encrypted_data: EncryptedData = match serde_json::from_slice(&encrypted_private_key) {
        Ok(data) => data,
        Err(_) => {
            // If not JSON, try to decrypt as raw encrypted bytes
            // For now, we'll assume it's in EncryptedData format
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Invalid encrypted private key format - expected EncryptedData JSON"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            )));
        }
    };

    // Decrypt the private key using session private key
    let user_private_key = match quantum_crypto.decrypt_data(
        &encrypted_data,
        &session_keypair.private_key,
    ).await {
        Ok(key) => key,
        Err(e) => {
            tracing::error!("Failed to decrypt user private key: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Failed to decrypt private key: {}", e)
                })),
                warp::http::StatusCode::BAD_REQUEST,
            )));
        }
    };

    // Remove session after use (one-time use for security)
    {
        let mut keypairs = session_keypairs.write().await;
        keypairs.remove(&session_id);
    }

    // Check access - verify requester is owner or authorized group member
    if metadata.sharing_mode.starts_with("group:") && requester_did.is_none() {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Missing requester-did for group-shared file"
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let requester = requester_did.unwrap_or_else(|| metadata.owner_did.clone());
    if requester != metadata.owner_did {
        if let Some(group_id) = metadata.sharing_mode.strip_prefix("group:") {
            let members = db.get_group_members(group_id).unwrap_or_default();
            let is_member = members.iter().any(|m| m.user_did == requester);
            if !is_member {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": "Access denied - group membership required"
                    })),
                    warp::http::StatusCode::FORBIDDEN,
                )));
            }
        } else {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Access denied - you must be the file owner or have been granted access"
                })),
                warp::http::StatusCode::FORBIDDEN,
            )));
        }
    }

    // Retrieve and decrypt file content
    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "File storage not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    // Read encrypted data from disk
    let data_path = data_dir.join(&file_id);
    if !data_path.exists() {
        tracing::warn!("File data not found on disk: {:?}", data_path);
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "File data not found"
            })),
            warp::http::StatusCode::NOT_FOUND,
        )));
    }

    let encrypted_data_json = match tokio::fs::read(&data_path).await {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Failed to read file data: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Failed to read file: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };

    // Deserialize EncryptedData structure
    // All files are now user-encrypted (zero-knowledge) - private key always required
    let encrypted_data: EncryptedData = match serde_json::from_slice(&encrypted_data_json) {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Failed to deserialize encrypted data: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Invalid file format: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };

    // Verify the provided private key matches the stored public key
    // (In production, derive public key from private key and compare)
    // For now, we'll attempt decryption and handle errors if key doesn't match

    // Decrypt the data using user's private key (storage node never stores private keys)
    let decrypted_data = match quantum_crypto.decrypt_data(&encrypted_data, &user_private_key).await {
        Ok(data) => data,
        Err(e) => {
            tracing::error!("Failed to decrypt file: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Decryption failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };

    // Update last accessed timestamp
    let mut updated_metadata = metadata.clone();
    updated_metadata.last_accessed = Some(chrono::Utc::now());
    let _ = db.insert_file_metadata(&updated_metadata);

    // Return decrypted content with appropriate content type
    let content_type = metadata.content_type
        .as_ref()
        .map(|ct| ct.as_str())
        .unwrap_or("application/octet-stream");

    tracing::info!("File content retrieved: {} ({} bytes)", file_id, decrypted_data.len());
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::with_header(decrypted_data, "Content-Type", content_type),
        warp::http::StatusCode::OK,
    )))
            };
            result
        },
    )
    .await;

    match timeout {
        Ok(result) => result,
        Err(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("File retrieval timeout (max: {}ms)", MAX_FILE_REQUEST_TIMEOUT_MS)
            })),
            warp::http::StatusCode::REQUEST_TIMEOUT,
        ))),
    }
}

// ============================================================================
// Envelope / Zero-Knowledge Streaming Handlers
// ============================================================================

/// Issue a challenge the client must decrypt to prove key ownership.
///
/// The server KEM-encrypts a random nonce to the file's owner public key.
/// The response uses separate `kem_ciphertext_hex`, `nonce_hex`, `ciphertext_hex`
/// fields so both the Rust CLI (OQS) and the browser (kyber_wasm `kyber_decrypt`)
/// can decrypt it.
async fn handle_challenge(
    file_id: String,
    requester_public_key_hex: Option<String>,
    requester_did: Option<String>,
    db: Arc<Database>,
    quantum_crypto: Option<Arc<QuantumCrypto>>,
    pending_challenges: Arc<RwLock<HashMap<String, PendingChallenge>>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let err_resp = |msg: &str, status: warp::http::StatusCode| -> Box<dyn Reply> {
        boxed_reply(warp::reply::with_status(
            warp::reply::json(&ChallengeResponse {
                success: false,
                challenge_id: None,
                encrypted_challenge: None,
                error: Some(msg.to_string()),
            }),
            status,
        ))
    };

    let _quantum_crypto = match quantum_crypto {
        Some(qc) => qc,
        None => {
            return Ok(err_resp(
                "Encryption service not configured",
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            ))
        }
    };

    let metadata = match db.get_file_metadata(&file_id) {
        Ok(Some(m)) => m,
        Ok(None) => {
            return Ok(err_resp(
                "File not found",
                warp::http::StatusCode::NOT_FOUND,
            ))
        }
        Err(e) => {
            return Ok(err_resp(
                &format!("DB error: {}", e),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    };

    // Priority: 1) explicit requester-public-key header, 2) resolve from DID registry, 3) file metadata
    let challenge_key_str = if let Some(ref rk) = requester_public_key_hex {
        rk.clone()
    } else if let Some(ref did) = requester_did {
        // Resolve the Kyber PK from the DID registry
        let doc_id = did.replace(':', "_");
        match db.get_document("system", "did_registry", &doc_id) {
            Ok(Some(record)) => match record.data["document"]["kyber_pk_hex"].as_str() {
                Some(pk) if !pk.is_empty() && pk != "0".repeat(pk.len()) => {
                    tracing::info!("Resolved Kyber PK from DID registry for {}", did);
                    pk.to_string()
                }
                _ => {
                    tracing::warn!("DID {} has no usable kyber_pk_hex in registry", did);
                    match &metadata.encryption_public_key {
                        Some(pk) => pk.clone(),
                        None => {
                            return Ok(err_resp(
                                "DID has no Kyber PK and file has no fallback key",
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    }
                }
            },
            _ => {
                tracing::warn!(
                    "DID {} not found in registry, falling back to file metadata",
                    did
                );
                match &metadata.encryption_public_key {
                    Some(pk) => pk.clone(),
                    None => {
                        return Ok(err_resp(
                            "DID not in registry and no fallback key",
                            warp::http::StatusCode::BAD_REQUEST,
                        ))
                    }
                }
            }
        }
    } else {
        match &metadata.encryption_public_key {
            Some(pk) => pk.clone(),
            None => {
                return Ok(err_resp(
                    "No requester-public-key, requester-did, or file encryption key",
                    warp::http::StatusCode::BAD_REQUEST,
                ))
            }
        }
    };

    let owner_public_key = match envelope::decode_public_key_flexible(&challenge_key_str) {
        Ok(k) => k,
        Err(e) => {
            return Ok(err_resp(
                &format!("Bad public key: {}", e),
                warp::http::StatusCode::BAD_REQUEST,
            ))
        }
    };

    // Determine KEM algorithm: strip "envelope-" prefix if present (envelope uploads)
    let raw_algo = metadata
        .encryption_algorithm
        .strip_prefix("envelope-")
        .unwrap_or(&metadata.encryption_algorithm);
    let kem_algo_str = match _quantum_crypto.parse_algorithm(raw_algo) {
        Ok(a) => format!("{:?}", a),
        Err(_) => format!("{:?}", _quantum_crypto.server_default_kem_algorithm()),
    };

    // Generate a random challenge nonce (32 bytes)
    let challenge_nonce: Vec<u8> = {
        let mut nonce = vec![0u8; 32];
        #[cfg(feature = "quantum")]
        {
            use aes_gcm::aead::rand_core::RngCore;
            aes_gcm::aead::OsRng.fill_bytes(&mut nonce);
        }
        #[cfg(not(feature = "quantum"))]
        {
            for (i, b) in nonce.iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(37).wrapping_add(7);
            }
        }
        nonce
    };

    // KEM-encrypt the nonce to the requester's public key.
    // Use pqcrypto-kyber (browser-compatible) when the requester sent their own key,
    // since the browser WASM uses pqcrypto-kyber for decapsulation.
    #[cfg(feature = "quantum")]
    let encrypted_challenge = if requester_public_key_hex.is_some() {
        match envelope::pqcrypto_kem_encrypt_bytes(&challenge_nonce, &owner_public_key) {
            Ok(efk) => EncryptedChallenge {
                kem_ciphertext_hex: efk.kem_ciphertext_hex,
                nonce_hex: efk.nonce_hex,
                ciphertext_hex: efk.ciphertext_hex,
            },
            Err(e) => {
                tracing::error!("pqcrypto KEM encrypt for challenge failed: {}", e);
                return Ok(err_resp(
                    &format!("KEM error: {}", e),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                ));
            }
        }
    } else {
        match envelope::kem_encrypt_bytes(&challenge_nonce, &owner_public_key, &kem_algo_str) {
            Ok(efk) => EncryptedChallenge {
                kem_ciphertext_hex: efk.kem_ciphertext_hex,
                nonce_hex: efk.nonce_hex,
                ciphertext_hex: efk.ciphertext_hex,
            },
            Err(e) => {
                tracing::error!("OQS KEM encrypt for challenge failed: {}", e);
                return Ok(err_resp(
                    &format!("KEM error: {}", e),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                ));
            }
        }
    };

    #[cfg(not(feature = "quantum"))]
    return Ok(err_resp(
        "Quantum feature required for challenges",
        warp::http::StatusCode::SERVICE_UNAVAILABLE,
    ));

    let challenge_id = Uuid::new_v4().to_string();
    let now = SystemTime::now();

    let pending = PendingChallenge {
        challenge_id: challenge_id.clone(),
        file_id: file_id.clone(),
        challenge_nonce: challenge_nonce.clone(),
        requester_public_key: owner_public_key.clone(),
        created_at: now,
        expires_at: now + Duration::from_secs(300),
    };

    {
        let mut challenges = pending_challenges.write().await;
        challenges.insert(challenge_id.clone(), pending);
        challenges.retain(|_, v| v.expires_at > SystemTime::now());
    }

    tracing::info!("Issued challenge {} for file {}", challenge_id, file_id);

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&ChallengeResponse {
            success: true,
            challenge_id: Some(challenge_id),
            encrypted_challenge: Some(encrypted_challenge),
            error: None,
        }),
        warp::http::StatusCode::OK,
    )))
}

/// Accept a pre-encrypted envelope blob from the client and store it opaquely.
///
/// The server validates the envelope header for sanity but never decrypts.
async fn handle_envelope_upload(
    body: Bytes,
    owner_did: String,
    owner_public_key_hex: String,
    filename: Option<String>,
    content_type: Option<String>,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Box<dyn Reply>, Rejection> {
    let timeout = tokio::time::timeout(
        Duration::from_millis(MAX_FILE_UPLOAD_TIMEOUT_MS),
        async {
    let _permit = match request_semaphore.try_acquire() {
        Ok(p) => p,
        Err(_) => return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Server busy"})),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        ))),
    };

    let data_dir = match data_dir {
        Some(d) => d,
        None => return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "File storage not configured"})),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        ))),
    };

    // Validate the envelope header (server can read the header but not decrypt data)
    let (header, _header_size) = match envelope::deserialize_header(&body) {
        Ok(h) => h,
        Err(e) => return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": format!("Invalid envelope: {}", e)})),
            warp::http::StatusCode::BAD_REQUEST,
        ))),
    };

    // Plaintext-hash dedup: reuse an existing blob for the same owner + content.
    if let Ok(Some(existing)) = db.find_file_by_owner_and_hash(&owner_did, &header.plaintext_hash) {
        let existing_path = data_dir.join(&existing.id);
        if tokio::fs::metadata(&existing_path).await.is_ok() {
            tracing::info!(
                "Envelope dedup: reusing file {} for owner {} (hash {})",
                existing.id,
                owner_did,
                header.plaintext_hash
            );
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "file_id": existing.id,
                    "filename": existing.filename,
                    "plaintext_size": existing.size,
                    "chunks": header.total_chunks,
                    "hash": header.plaintext_hash,
                    "deduplicated": true,
                    "message": "Envelope already stored for this owner — reusing existing file_id."
                })),
                warp::http::StatusCode::OK,
            )));
        }
    }

    let file_id = Uuid::new_v4().to_string();
    let fname = filename.unwrap_or_else(|| format!("file_{}", &file_id[..8]));

    let metadata = FileMetadata {
        id: file_id.clone(),
        filename: fname.clone(),
        size: header.total_plaintext_size,
        hash: header.plaintext_hash.clone(),
        owner_did: owner_did.clone(),
        encryption_algorithm: format!("envelope-{}", header.kem_algorithm),
        content_type,
        created_at: chrono::Utc::now(),
        last_accessed: None,
        encryption_public_key: Some(owner_public_key_hex.clone()),
        sharing_mode: "owner".to_string(),
    };

    if let Err(e) = db.insert_file_metadata(&metadata) {
        tracing::error!("DB insert failed: {}", e);
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Failed to store metadata"})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )));
    }

    let data_path = data_dir.join(&file_id);
    if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
        tracing::error!("mkdir failed: {}", e);
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": format!("mkdir: {}", e)})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )));
    }
    if let Err(e) = tokio::fs::write(&data_path, &body).await {
        tracing::error!("write failed: {}", e);
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": format!("write: {}", e)})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )));
    }

    tracing::info!("Envelope stored: {} ({}, {} chunks, {} bytes plaintext)", fname, file_id, header.total_chunks, header.total_plaintext_size);

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "file_id": file_id,
            "filename": fname,
            "plaintext_size": header.total_plaintext_size,
            "chunks": header.total_chunks,
            "hash": header.plaintext_hash,
            "message": "Envelope stored. Server cannot decrypt — only your private key can."
        })),
        warp::http::StatusCode::OK,
    )))
        }
    ).await;

    match timeout {
        Ok(r) => r,
        Err(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Upload timeout"})),
            warp::http::StatusCode::REQUEST_TIMEOUT,
        ))),
    }
}

/// Shared chat attachment upload: server-side envelope encryption to the storage node's Kyber key.
/// Any network participant with PQ keys can later challenge+stream+decrypt; admin-stream works for ops.
async fn handle_shared_chat_upload(
    body: Bytes,
    owner_did: String,
    filename: Option<String>,
    content_type: Option<String>,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
    server_keypair: Option<Arc<ServerKeypair>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let timeout = tokio::time::timeout(Duration::from_millis(MAX_FILE_UPLOAD_TIMEOUT_MS), async {
        let _permit = match request_semaphore.try_acquire() {
            Ok(p) => p,
            Err(_) => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": "Server busy"})),
                    warp::http::StatusCode::SERVICE_UNAVAILABLE,
                )))
            }
        };

        let data_dir = match data_dir {
            Some(d) => d,
            None => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": "File storage not configured"})),
                    warp::http::StatusCode::SERVICE_UNAVAILABLE,
                )))
            }
        };

        let skp = match server_keypair {
            Some(k) => k,
            None => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": "Server keypair not initialized"}),
                    ),
                    warp::http::StatusCode::SERVICE_UNAVAILABLE,
                )))
            }
        };

        #[cfg(feature = "quantum")]
        let envelope_bytes = match envelope::encrypt_envelope_sourced(
            &body,
            &skp.public_key,
            &skp.algorithm,
            None,
            Some(skp.key_source),
        ) {
            Ok(b) => b,
            Err(e) => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": format!("Envelope encryption failed: {}", e)}),
                    ),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )))
            }
        };

        #[cfg(not(feature = "quantum"))]
        let envelope_bytes: Vec<u8> = body.to_vec();

        let (header, _header_size) = match envelope::deserialize_header(&envelope_bytes) {
            Ok(h) => h,
            Err(e) => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": format!("Invalid envelope: {}", e)}),
                    ),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )))
            }
        };

        let file_id = Uuid::new_v4().to_string();
        let fname = filename.unwrap_or_else(|| format!("chat_{}", &file_id[..8]));
        let owner_pk_hex = hex::encode(&skp.public_key);

        let metadata = FileMetadata {
            id: file_id.clone(),
            filename: fname.clone(),
            size: header.total_plaintext_size,
            hash: header.plaintext_hash.clone(),
            owner_did: owner_did.clone(),
            encryption_algorithm: format!("envelope-{}", header.kem_algorithm),
            content_type,
            created_at: chrono::Utc::now(),
            last_accessed: None,
            encryption_public_key: Some(owner_pk_hex),
            sharing_mode: "chat-shared".to_string(),
        };

        if let Err(e) = db.insert_file_metadata(&metadata) {
            tracing::error!("DB insert failed: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Failed to store metadata"})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }

        let data_path = data_dir.join(&file_id);
        if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
            tracing::error!("mkdir failed: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": format!("mkdir: {}", e)})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
        if let Err(e) = tokio::fs::write(&data_path, &envelope_bytes).await {
            tracing::error!("write failed: {}", e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": format!("write: {}", e)})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }

        tracing::info!(
            "Shared chat upload: {} ({}, {} bytes plaintext, owner {})",
            fname,
            file_id,
            header.total_plaintext_size,
            owner_did
        );

        Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "file_id": file_id,
                "filename": fname,
                "plaintext_size": header.total_plaintext_size,
                "sharing_mode": "chat-shared",
                "message": "Stored as server-envelope; participants can stream with PQ keys."
            })),
            warp::http::StatusCode::OK,
        )))
    })
    .await;

    match timeout {
        Ok(r) => r,
        Err(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Upload timeout"})),
            warp::http::StatusCode::REQUEST_TIMEOUT,
        ))),
    }
}

/// Stream a file to the client after verifying the challenge-response.
///
/// PQ protocol: the on-disk file is an envelope encrypted to the server's key.
/// After challenge verification the server decrypts with its private key and
/// re-encrypts a fresh envelope to the requester's public key (stored in
/// `PendingChallenge` during the challenge step).
async fn handle_stream_download(
    file_id: String,
    challenge_id: String,
    challenge_response_hex: String,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    pending_challenges: Arc<RwLock<HashMap<String, PendingChallenge>>>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
    server_keypair: Option<Arc<ServerKeypair>>,
    quantum_crypto: Option<Arc<QuantumCrypto>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let timeout = tokio::time::timeout(
        Duration::from_millis(MAX_STREAM_TIMEOUT_MS),
        async {
    let _permit = match request_semaphore.try_acquire() {
        Ok(p) => p,
        Err(_) => return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Server busy"})),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        ))),
    };

    let data_dir = match data_dir {
        Some(d) => d,
        None => return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "File storage not configured"})),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        ))),
    };

    // Verify the challenge
    let pending = {
        let mut challenges = pending_challenges.write().await;
        match challenges.remove(&challenge_id) {
            Some(c) => c,
            None => return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Invalid or expired challenge"})),
                warp::http::StatusCode::UNAUTHORIZED,
            ))),
        }
    };

    if pending.expires_at < SystemTime::now() {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Challenge expired"})),
            warp::http::StatusCode::UNAUTHORIZED,
        )));
    }

    if pending.file_id != file_id {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Challenge file_id mismatch"})),
            warp::http::StatusCode::UNAUTHORIZED,
        )));
    }

    let client_nonce = match hex::decode(&challenge_response_hex) {
        Ok(t) => t,
        Err(_) => return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Invalid challenge-response hex"})),
            warp::http::StatusCode::BAD_REQUEST,
        ))),
    };

    if client_nonce != pending.challenge_nonce {
        tracing::warn!("Challenge-response mismatch for file {} (wrong private key?)", file_id);
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Challenge verification failed — wrong private key?"})),
            warp::http::StatusCode::UNAUTHORIZED,
        )));
    }

    match db.get_file_metadata(&file_id) {
        Ok(Some(_)) => {}
        Ok(None) => return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "File not found"})),
            warp::http::StatusCode::NOT_FOUND,
        ))),
        Err(e) => return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": format!("DB error: {}", e)})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ))),
    }

    let data_path = data_dir.join(&file_id);

    // Fast path: peel nested → header DEK re-wrap → chunked re-wrap (~64 KiB RAM).
    #[cfg(feature = "quantum")]
    {
        if let Some(ref skp) = server_keypair {
            let requester_pk = &pending.requester_public_key;
            match envelope_delivery::try_stream_delivery_to_pqcrypto_recipient(
                &data_path,
                envelope_delivery::ServerKeyMaterial {
                    secret_key: &skp.secret_key,
                    algorithm: &skp.algorithm,
                    key_source: Some(skp.key_source),
                },
                requester_pk,
            )
            .await
            {
                Ok(Some(resp)) => {
                    if let Ok(Some(mut meta)) = db.get_file_metadata(&file_id) {
                        meta.last_accessed = Some(chrono::Utc::now());
                        let _ = db.insert_file_metadata(&meta);
                    }
                    return Ok(boxed_reply(resp));
                }
                Ok(None) => {
                    tracing::info!(
                        "Stream: bounded delivery unavailable for {} — checking legacy path",
                        file_id
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Stream: bounded delivery failed for {}: {} — checking legacy path",
                        file_id,
                        e
                    );
                }
            }

            match envelope_delivery::exceeds_legacy_full_buffer_limit(&data_path).await {
                Ok(true) => {
                    let size = tokio::fs::metadata(&data_path)
                        .await
                        .map(|m| m.len())
                        .unwrap_or(0);
                    tracing::error!(
                        "Stream: refusing legacy full-buffer path for {} ({} bytes > {} limit)",
                        file_id,
                        size,
                        envelope_delivery::MAX_LEGACY_FULL_BUFFER_BYTES
                    );
                    return Ok(legacy_full_buffer_too_large_reply(&file_id, size));
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(
                        "Stream: could not stat {} for legacy size check: {}",
                        file_id,
                        e
                    );
                }
            }
        }
    }

    let on_disk = match tokio::fs::read(&data_path).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Failed to read file {}: {}", file_id, e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File data not found on disk"})),
                warp::http::StatusCode::NOT_FOUND,
            )));
        }
    };

    // Determine the plaintext: if we have a server keypair and the file is an
    // envelope encrypted to the server key, decrypt it. Otherwise try legacy
    // paths (.raw plaintext copy, or raw bytes).
    #[allow(unused_assignments)]
    let mut plaintext: Option<Vec<u8>> = None;

    #[cfg(feature = "quantum")]
    {
        if let Some(ref skp) = server_keypair {
            tracing::info!(
                "Stream: attempting envelope decrypt for {} (on_disk {} bytes, sk {} bytes, algo {}, key_source {:?})",
                file_id, on_disk.len(), skp.secret_key.len(), skp.algorithm, skp.key_source
            );
            if let Ok((header, hdr_size)) = envelope::deserialize_header(&on_disk) {
                tracing::info!(
                    "Stream: envelope header OK for {} — kem={}, chunks={}, plaintext_size={}, header_size={}, encrypted_file_key kem_ct len={}",
                    file_id, header.kem_algorithm, header.total_chunks, header.total_plaintext_size,
                    hdr_size, header.encrypted_file_key.kem_ciphertext_hex.len() / 2
                );
            }
            match envelope::decrypt_envelope_server_side_peel_sourced(
                &on_disk,
                &skp.secret_key,
                &skp.algorithm,
                envelope::SERVER_ENVELOPE_PEEL_MAX_LAYERS,
                Some(skp.key_source),
            ) {
                Ok(pt) => {
                    tracing::info!("Decrypted envelope for file {} ({} bytes plaintext)", file_id, pt.len());
                    plaintext = Some(pt);
                }
                Err(e) => {
                    tracing::warn!("Stream: envelope decrypt failed for {}: {} — trying fallback paths", file_id, e);
                }
            }
        }
    }

    // Fallback: .raw plaintext copy (legacy uploads before envelope protocol)
    if plaintext.is_none() {
        let raw_path = data_dir.join(format!("{}.raw", &file_id));
        if let Ok(raw) = tokio::fs::read(&raw_path).await {
            tracing::info!("Using .raw plaintext fallback for file {} ({} bytes)", file_id, raw.len());
            plaintext = Some(raw);
        }
    }

    // Fallback: legacy EncryptedData JSON (from thin CLI / legacy /files/upload)
    if plaintext.is_none() {
        if let Ok(enc) = serde_json::from_slice::<crate::quantum::EncryptedData>(&on_disk) {
            if let (Some(ref skp), Some(ref qc)) = (&server_keypair, &quantum_crypto) {
                match qc.decrypt_data(&enc, &skp.secret_key).await {
                    Ok(pt) => {
                        tracing::info!(
                            "Stream: decrypted legacy EncryptedData JSON for file {} ({} bytes plaintext)",
                            file_id, pt.len()
                        );
                        plaintext = Some(pt);
                    }
                    Err(e) => {
                        tracing::warn!("Stream: legacy EncryptedData decrypt failed for {}: {}", file_id, e);
                    }
                }
            } else {
                tracing::warn!("Stream: file {} is legacy EncryptedData JSON but no server keypair/quantum_crypto available", file_id);
            }
        }
    }

    // Fallback: if not JSON EncryptedData and not envelope, treat as raw bytes
    if plaintext.is_none() {
        if serde_json::from_slice::<crate::quantum::EncryptedData>(&on_disk).is_err() {
            if envelope::deserialize_header(&on_disk).is_ok() {
                tracing::warn!(
                    "Stream: file {} is a PQ envelope on disk but server-side decrypt failed",
                    file_id
                );
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": "Cannot decrypt file — server keypair mismatch or missing plaintext"})),
                    warp::http::StatusCode::UNPROCESSABLE_ENTITY,
                )));
            }
            tracing::info!("File {} is not encrypted JSON — treating as raw bytes ({} bytes)", file_id, on_disk.len());
            plaintext = Some(on_disk.clone());
        }
    }

    let plaintext = match plaintext {
        Some(pt) => pt,
        None => {
            tracing::error!("Cannot decrypt file {} — no server key or plaintext fallback", file_id);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Cannot decrypt file — server keypair mismatch or missing plaintext"})),
                warp::http::StatusCode::UNPROCESSABLE_ENTITY,
            )));
        }
    };

    // Re-encrypt the plaintext as an envelope for the requester's public key.
    // Use pqcrypto-kyber (browser-compatible) for the re-encryption KEM since
    // the browser WASM uses pqcrypto-kyber for decapsulation.
    #[cfg(feature = "quantum")]
    let response_data = {
        let requester_pk = &pending.requester_public_key;
        match envelope::pqcrypto_encrypt_envelope(&plaintext, requester_pk, None) {
            Ok(env) => {
                tracing::info!("Re-encrypted envelope (pqcrypto) for file {} ({} bytes) -> requester ({} bytes envelope)", file_id, plaintext.len(), env.len());
                env
            }
            Err(e) => {
                tracing::error!("Re-encryption failed for file {}: {}", file_id, e);
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": format!("Re-encryption failed: {}", e)})),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        }
    };

    #[cfg(not(feature = "quantum"))]
    let response_data = plaintext;

    if let Ok(Some(mut meta)) = db.get_file_metadata(&file_id) {
        meta.last_accessed = Some(chrono::Utc::now());
        let _ = db.insert_file_metadata(&meta);
    }

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::with_header(
            warp::reply::with_header(response_data, "Content-Type", "application/octet-stream"),
            "X-Envelope-Version",
            "1",
        ),
        warp::http::StatusCode::OK,
    )))
        }
    ).await;

    match timeout {
        Ok(r) => r,
        Err(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Stream timeout"})),
            warp::http::StatusCode::REQUEST_TIMEOUT,
        ))),
    }
}

/// Returns on-disk ciphertext after challenge-response verification.
/// Used for legacy owner-key `/files/upload` blobs the server cannot decrypt;
/// the browser decrypts locally with its Kyber private key.
async fn handle_ciphertext_download(
    file_id: String,
    challenge_id: String,
    challenge_response_hex: String,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    pending_challenges: Arc<RwLock<HashMap<String, PendingChallenge>>>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Box<dyn Reply>, Rejection> {
    let _permit = match request_semaphore.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Server busy"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let pending = {
        let mut challenges = pending_challenges.write().await;
        match challenges.remove(&challenge_id) {
            Some(c) => c,
            None => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": "Invalid or expired challenge"}),
                    ),
                    warp::http::StatusCode::UNAUTHORIZED,
                )))
            }
        }
    };

    if pending.expires_at < SystemTime::now() {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Challenge expired"})),
            warp::http::StatusCode::UNAUTHORIZED,
        )));
    }

    if pending.file_id != file_id {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Challenge file_id mismatch"})),
            warp::http::StatusCode::UNAUTHORIZED,
        )));
    }

    let client_nonce = match hex::decode(&challenge_response_hex) {
        Ok(t) => t,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Invalid challenge-response hex"})),
                warp::http::StatusCode::BAD_REQUEST,
            )))
        }
    };

    if client_nonce != pending.challenge_nonce {
        tracing::warn!(
            "Challenge-response mismatch for ciphertext {} (wrong private key?)",
            file_id
        );
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({"error": "Challenge verification failed — wrong private key?"}),
            ),
            warp::http::StatusCode::UNAUTHORIZED,
        )));
    }

    match db.get_file_metadata(&file_id) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File not found"})),
                warp::http::StatusCode::NOT_FOUND,
            )))
        }
        Err(e) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": format!("DB error: {}", e)})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }

    let data_path = data_dir.join(&file_id);

    #[cfg(feature = "quantum")]
    {
        match envelope_delivery::try_stream_ciphertext_file(&data_path).await {
            Ok(resp) => {
                tracing::info!(
                    "Ciphertext download for file {} (streaming, {} bytes)",
                    file_id,
                    resp.headers()
                        .get("content-length")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("?")
                );
                return Ok(boxed_reply(resp));
            }
            Err(e) => {
                tracing::error!("Failed to stream ciphertext for file {}: {}", file_id, e);
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": "File data not found on disk"})),
                    warp::http::StatusCode::NOT_FOUND,
                )));
            }
        }
    }

    #[cfg(not(feature = "quantum"))]
    {
        let on_disk = match tokio::fs::read(&data_path).await {
            Ok(d) => d,
            Err(e) => {
                tracing::error!("Failed to read ciphertext for file {}: {}", file_id, e);
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": "File data not found on disk"})),
                    warp::http::StatusCode::NOT_FOUND,
                )));
            }
        };

        tracing::info!(
            "Ciphertext download for file {} ({} bytes on disk)",
            file_id,
            on_disk.len()
        );
        Ok(boxed_reply(warp::reply::with_header(
            on_disk,
            "Content-Type",
            "application/octet-stream",
        )))
    }
}

/// DID-authenticated direct file stream for trusted services.
/// Bypasses KEM challenge-response — access control is the caller's responsibility.
/// Decrypts envelope-encrypted files using the server keypair and returns plaintext.
async fn handle_admin_stream(
    file_id: String,
    _did: String,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
    server_keypair: Option<Arc<ServerKeypair>>,
    quantum_crypto: Option<Arc<QuantumCrypto>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let _permit = match request_semaphore.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Server busy"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    match db.get_file_metadata(&file_id) {
        Ok(Some(mut meta)) => {
            let mut plaintext: Option<Vec<u8>> = None;

            let data_path = data_dir.join(&file_id);

            #[cfg(feature = "quantum")]
            {
                if let Some(ref skp) = server_keypair {
                    match envelope_delivery::try_stream_admin_plaintext(
                        &data_path,
                        envelope_delivery::ServerKeyMaterial {
                            secret_key: &skp.secret_key,
                            algorithm: &skp.algorithm,
                            key_source: Some(skp.key_source),
                        },
                    )
                    .await
                    {
                        Ok(Some(resp)) => {
                            tracing::info!(
                                "Admin-stream file {} (streaming plaintext) for DID {}",
                                file_id,
                                _did
                            );
                            meta.last_accessed = Some(chrono::Utc::now());
                            let _ = db.insert_file_metadata(&meta);
                            return Ok(boxed_reply(resp));
                        }
                        Ok(None) => {
                            tracing::info!(
                                "Admin-stream: chunked decrypt unavailable for {} — checking legacy path",
                                file_id
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Admin-stream: chunked decrypt failed for {}: {} — checking legacy path",
                                file_id,
                                e
                            );
                        }
                    }

                    match envelope_delivery::exceeds_legacy_full_buffer_limit(&data_path).await {
                        Ok(true) => {
                            let size = tokio::fs::metadata(&data_path)
                                .await
                                .map(|m| m.len())
                                .unwrap_or(0);
                            tracing::error!(
                                "Admin-stream: refusing legacy full-buffer path for {} ({} bytes > {} limit)",
                                file_id,
                                size,
                                envelope_delivery::MAX_LEGACY_FULL_BUFFER_BYTES
                            );
                            return Ok(legacy_full_buffer_too_large_reply(&file_id, size));
                        }
                        Ok(false) => {}
                        Err(e) => {
                            tracing::warn!(
                                "Admin-stream: could not stat {} for legacy size check: {}",
                                file_id,
                                e
                            );
                        }
                    }
                }
            }

            let on_disk = match tokio::fs::read(&data_path).await {
                Ok(d) => d,
                Err(_) => {
                    // Try .raw fallback
                    let raw_path = data_dir.join(format!("{}.raw", &file_id));
                    match tokio::fs::read(&raw_path).await {
                        Ok(d) => {
                            plaintext = Some(d);
                            vec![]
                        }
                        Err(e) => {
                            tracing::error!("Failed to read file {}: {}", file_id, e);
                            return Ok(boxed_reply(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({"error": "File data not found on disk"}),
                                ),
                                warp::http::StatusCode::NOT_FOUND,
                            )));
                        }
                    }
                }
            };

            // Try envelope decryption with server keypair
            #[cfg(feature = "quantum")]
            if plaintext.is_none() {
                if server_keypair.is_none() {
                    tracing::warn!("Admin-stream: no server keypair available for file {} — cannot decrypt envelope", file_id);
                }
                if let Some(ref skp) = server_keypair {
                    match envelope::decrypt_envelope_server_side_peel_sourced(
                        &on_disk,
                        &skp.secret_key,
                        &skp.algorithm,
                        envelope::SERVER_ENVELOPE_PEEL_MAX_LAYERS,
                        Some(skp.key_source),
                    ) {
                        Ok(pt) => {
                            tracing::info!(
                                "Admin-stream: decrypted envelope for file {} ({} bytes)",
                                file_id,
                                pt.len()
                            );
                            plaintext = Some(pt);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "Admin-stream: envelope decrypt failed for {}: {}",
                                file_id,
                                e
                            );
                        }
                    }
                }
            }

            // Fallback: .raw plaintext copy
            if plaintext.is_none() {
                let raw_path = data_dir.join(format!("{}.raw", &file_id));
                if let Ok(raw) = tokio::fs::read(&raw_path).await {
                    plaintext = Some(raw);
                }
            }

            // Fallback: legacy EncryptedData JSON (from thin CLI / legacy /files/upload)
            if plaintext.is_none() && !on_disk.is_empty() {
                if let Ok(enc) = serde_json::from_slice::<crate::quantum::EncryptedData>(&on_disk) {
                    if let (Some(ref skp), Some(ref qc)) = (&server_keypair, &quantum_crypto) {
                        match qc.decrypt_data(&enc, &skp.secret_key).await {
                            Ok(pt) => {
                                tracing::info!(
                                    "Admin-stream: decrypted legacy EncryptedData JSON for file {} ({} bytes plaintext)",
                                    file_id, pt.len()
                                );
                                plaintext = Some(pt);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Admin-stream: legacy EncryptedData decrypt failed for {}: {}",
                                    file_id,
                                    e
                                );
                            }
                        }
                    } else {
                        tracing::warn!("Admin-stream: file {} is legacy EncryptedData JSON but no server keypair/quantum_crypto available", file_id);
                    }
                }
            }

            // Fallback: if not JSON EncryptedData, treat as raw bytes — but never stream a PQ
            // envelope ciphertext as "plaintext" (first u64 length can begin with 0x3c, which
            // confuses WASM validators and clients expecting \0asm).
            if plaintext.is_none() && !on_disk.is_empty() {
                if serde_json::from_slice::<crate::quantum::EncryptedData>(&on_disk).is_err() {
                    if envelope::deserialize_header(&on_disk).is_ok() {
                        tracing::warn!(
                            "Admin-stream: file {} is still a PQ envelope on disk but server-side decrypt failed — refusing to stream ciphertext as plaintext",
                            file_id
                        );
                        return Ok(boxed_reply(warp::reply::with_status(
                            warp::reply::json(
                                &serde_json::json!({"error": "File is encrypted and cannot be decrypted. Re-deploy with the updated CLI."}),
                            ),
                            warp::http::StatusCode::UNPROCESSABLE_ENTITY,
                        )));
                    }
                    tracing::warn!("Admin-stream: file {} treated as raw bytes ({} bytes) — no server keypair or envelope decrypt failed", file_id, on_disk.len());
                    plaintext = Some(on_disk);
                }
            }

            let data = match plaintext {
                Some(pt) => pt,
                None => {
                    return Ok(boxed_reply(warp::reply::with_status(
                        warp::reply::json(
                            &serde_json::json!({"error": "File is encrypted and cannot be decrypted. Re-deploy with the updated CLI."}),
                        ),
                        warp::http::StatusCode::UNPROCESSABLE_ENTITY,
                    )));
                }
            };

            tracing::info!(
                "Admin-stream file {} ({} bytes plaintext) for DID {}",
                file_id,
                data.len(),
                _did
            );
            meta.last_accessed = Some(chrono::Utc::now());
            let _ = db.insert_file_metadata(&meta);
            Ok(boxed_reply(warp::reply::with_header(
                data,
                "Content-Type",
                "application/octet-stream",
            )))
        }
        Ok(None) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "File not found"})),
            warp::http::StatusCode::NOT_FOUND,
        ))),
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": format!("DB error: {}", e)})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ))),
    }
}

/// Owner posts a delivery capsule: DEK KEM-wrapped to the entitled recipient.
/// Required for true E2E files (envelope encrypted to owner PK, not server PK).
async fn handle_put_delivery_capsule(
    file_id: String,
    owner_did: String,
    entitlement_id_hex: String,
    body: envelope::EncryptedFileKey,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Box<dyn Reply>, Rejection> {
    let _permit = match request_semaphore.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Server busy"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let ent_hex_ok = hex::decode(entitlement_id_hex.trim_start_matches("0x"))
        .map(|b| b.len() == 32)
        .unwrap_or(false);
    if !ent_hex_ok {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Invalid entitlement-id: expected 64-char hex (32 bytes)"
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    if body.kem_ciphertext_hex.is_empty()
        || body.nonce_hex.is_empty()
        || body.ciphertext_hex.is_empty()
    {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Capsule must include kem_ciphertext_hex, nonce_hex, ciphertext_hex"
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let meta = match db.get_file_metadata(&file_id) {
        Ok(Some(m)) => m,
        Ok(None) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File not found"})),
                warp::http::StatusCode::NOT_FOUND,
            )))
        }
        Err(e) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": format!("DB error: {}", e)})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    };

    if meta.owner_did != owner_did {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Only the file owner may post a delivery capsule"
            })),
            warp::http::StatusCode::FORBIDDEN,
        )));
    }

    match crate::delivery_capsule::store_delivery_capsule(
        &data_dir,
        &file_id,
        &entitlement_id_hex,
        &body,
    )
    .await
    {
        Ok(path) => {
            tracing::info!(
                "Stored E2E delivery capsule for file {} entitlement {} at {}",
                file_id,
                entitlement_id_hex,
                path.display()
            );
            Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "ok": true,
                    "file_id": file_id,
                    "entitlement_id": entitlement_id_hex.trim_start_matches("0x").to_ascii_lowercase(),
                    "delivery_mode": "e2e-capsule"
                })),
                warp::http::StatusCode::OK,
            )))
        }
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({"error": format!("Failed to store capsule: {}", e)}),
            ),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ))),
    }
}

/// Entitlement-gated content delivery.
///
/// After `OP_VERIFY`, prefers an owner-posted E2E delivery capsule (storage never
/// sees the DEK). Falls back to server-key DEK re-wrap for server-encrypted blobs.
async fn handle_entitlement_rewrap(
    file_id: String,
    entitlement_id_hex: String,
    buyer_did: String,
    buyer_public_key_hex: String,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    request_semaphore: Arc<tokio::sync::Semaphore>,
    server_keypair: Option<Arc<ServerKeypair>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let _permit = match request_semaphore.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Server busy"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    // Parse entitlement ID
    let ent_id_bytes = match hex::decode(&entitlement_id_hex) {
        Ok(b) if b.len() == 32 => b,
        _ => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(
                    &serde_json::json!({"error": "Invalid entitlement-id: expected 64-char hex (32 bytes)"}),
                ),
                warp::http::StatusCode::BAD_REQUEST,
            )))
        }
    };

    // Parse buyer public key (hex or base64)
    let buyer_pk = match envelope::decode_public_key_flexible(&buyer_public_key_hex) {
        Ok(pk) => pk,
        Err(e) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(
                    &serde_json::json!({"error": format!("Invalid buyer-public-key: {}", e)}),
                ),
                warp::http::StatusCode::BAD_REQUEST,
            )))
        }
    };

    // ── Entitlement verification via compute node ──
    //
    // Build the OP_VERIFY wire payload:
    //   [0x03][entitlement_id:32 raw bytes][buyer_did:string][file_id:string][buyer_pk_hash:32]
    let buyer_pk_hash = envelope_delivery::buyer_public_key_hash(&buyer_pk);
    let compute_url = std::env::var("SPACEKIT_COMPUTE_NODE_URL").unwrap_or_default();
    let contract_id = std::env::var("SPACEKIT_ENTITLEMENT_CONTRACT_ID").unwrap_or_default();

    if compute_url.is_empty() || contract_id.is_empty() {
        tracing::warn!("Entitlement rewrap: SPACEKIT_COMPUTE_NODE_URL or SPACEKIT_ENTITLEMENT_CONTRACT_ID not configured");
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({"error": "Entitlement verification not configured on this storage node"}),
            ),
            warp::http::StatusCode::SERVICE_UNAVAILABLE,
        )));
    }

    let mut verify_payload = Vec::with_capacity(1 + 32 + 4 + buyer_did.len() + file_id.len() + 32);
    verify_payload.push(0x03u8); // OP_VERIFY
    verify_payload.extend_from_slice(&ent_id_bytes);
    verify_payload.extend_from_slice(&(buyer_did.len() as u16).to_le_bytes());
    verify_payload.extend_from_slice(buyer_did.as_bytes());
    verify_payload.extend_from_slice(&(file_id.len() as u16).to_le_bytes());
    verify_payload.extend_from_slice(file_id.as_bytes());
    verify_payload.extend_from_slice(&buyer_pk_hash);

    let verify_result = {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        let url = format!("{}/api/contracts/{}/call", compute_url, contract_id);
        let resp = client
            .post(&url)
            .header("Content-Type", "application/octet-stream")
            .body(verify_payload)
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => match r.bytes().await {
                Ok(b) => b.to_vec(),
                Err(e) => {
                    tracing::error!("Entitlement verify: failed to read compute response: {}", e);
                    return Ok(boxed_reply(warp::reply::with_status(
                        warp::reply::json(
                            &serde_json::json!({"error": "Entitlement verification failed (read error)"}),
                        ),
                        warp::http::StatusCode::BAD_GATEWAY,
                    )));
                }
            },
            Ok(r) => {
                let status = r.status();
                let body = r.text().await.unwrap_or_default();
                tracing::error!(
                    "Entitlement verify: compute node returned {}: {}",
                    status,
                    body
                );
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": format!("Entitlement verification failed (compute: {})", status)}),
                    ),
                    warp::http::StatusCode::BAD_GATEWAY,
                )));
            }
            Err(e) => {
                tracing::error!(
                    "Entitlement verify: cannot reach compute node at {}: {}",
                    compute_url,
                    e
                );
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": "Cannot reach compute node for entitlement verification"}),
                    ),
                    warp::http::StatusCode::BAD_GATEWAY,
                )));
            }
        }
    };

    // Expected response: [1, status_byte]  where status_byte == 1 means valid
    if verify_result.len() < 2 || verify_result[0] != 1 || verify_result[1] != 1 {
        let status_byte = verify_result.get(1).copied().unwrap_or(255);
        let reason = match status_byte {
            0 => "entitlement expired",
            2 => "buyer DID mismatch",
            3 => "file ID mismatch",
            4 => "entitlement revoked",
            5 => "buyer public key mismatch",
            _ => "verification failed",
        };
        tracing::info!(
            "Entitlement rewrap denied for file {}: {} (status={})",
            file_id,
            reason,
            status_byte
        );
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": reason, "status": status_byte})),
            warp::http::StatusCode::FORBIDDEN,
        )));
    }

    tracing::info!(
        "Entitlement verified for file {} / buyer {}",
        file_id,
        buyer_did
    );

    // ── File retrieval: E2E capsule (preferred) → server DEK re-wrap → small legacy ──

    match db.get_file_metadata(&file_id) {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File not found"})),
                warp::http::StatusCode::NOT_FOUND,
            )))
        }
        Err(e) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": format!("DB error: {}", e)})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }

    let data_path = data_dir.join(&file_id);

    #[cfg(feature = "quantum")]
    {
        // True E2E: owner-posted capsule — storage never unwraps the DEK.
        match crate::delivery_capsule::load_delivery_capsule(
            &data_dir,
            &file_id,
            &entitlement_id_hex,
        )
        .await
        {
            Ok(Some(capsule)) => {
                match envelope_delivery::try_stream_capsule_envelope(&data_path, capsule).await {
                    Ok(Some(resp)) => {
                        if let Ok(Some(mut meta)) = db.get_file_metadata(&file_id) {
                            meta.last_accessed = Some(chrono::Utc::now());
                            let _ = db.insert_file_metadata(&meta);
                        }
                        return Ok(boxed_reply(resp));
                    }
                    Ok(None) => {
                        tracing::warn!(
                            "Rewrap: E2E capsule present but on-disk envelope unreadable for {}",
                            file_id
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Rewrap: E2E capsule stream failed for {}: {}", file_id, e);
                    }
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("Rewrap: capsule load error for {}: {}", file_id, e);
            }
        }

        let mut server_can_unwrap = false;
        if let Some(ref skp) = server_keypair {
            match envelope_delivery::try_stream_delivery_to_pqcrypto_recipient(
                &data_path,
                envelope_delivery::ServerKeyMaterial {
                    secret_key: &skp.secret_key,
                    algorithm: &skp.algorithm,
                    key_source: Some(skp.key_source),
                },
                &buyer_pk,
            )
            .await
            {
                Ok(Some(resp)) => {
                    if let Ok(Some(mut meta)) = db.get_file_metadata(&file_id) {
                        meta.last_accessed = Some(chrono::Utc::now());
                        let _ = db.insert_file_metadata(&meta);
                    }
                    return Ok(boxed_reply(resp));
                }
                Ok(None) => {
                    tracing::info!(
                        "Rewrap: server-key delivery unavailable for {} — checking legacy / E2E hint",
                        file_id
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Rewrap: server-key delivery failed for {}: {} — checking legacy / E2E hint",
                        file_id,
                        e
                    );
                }
            }

            server_can_unwrap = match envelope::read_envelope_header_prefix(&data_path).await {
                Ok((prefix, _)) => match envelope::deserialize_header(&prefix) {
                    Ok((hdr, _)) => envelope::server_file_key_from_header(
                        &hdr,
                        &skp.secret_key,
                        &skp.algorithm,
                        Some(skp.key_source),
                    )
                    .is_ok(),
                    Err(_) => false,
                },
                Err(_) => false,
            };

            if server_can_unwrap {
                match envelope_delivery::exceeds_legacy_full_buffer_limit(&data_path).await {
                    Ok(true) => {
                        let size = tokio::fs::metadata(&data_path)
                            .await
                            .map(|m| m.len())
                            .unwrap_or(0);
                        tracing::error!(
                            "Rewrap: refusing legacy full-buffer path for {} ({} bytes > {} limit)",
                            file_id,
                            size,
                            envelope_delivery::MAX_LEGACY_FULL_BUFFER_BYTES
                        );
                        return Ok(legacy_full_buffer_too_large_reply(&file_id, size));
                    }
                    Ok(false) => {}
                    Err(e) => {
                        tracing::warn!(
                            "Rewrap: could not stat {} for legacy size check: {}",
                            file_id,
                            e
                        );
                    }
                }
            }
        }

        if !server_can_unwrap {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "E2E delivery capsule required from the file owner before entitled download",
                    "delivery_mode": "e2e-capsule",
                    "hint": "Owner must OP_GRANT (or purchase) then PUT /files/{id}/delivery-capsule"
                })),
                warp::http::StatusCode::UNPROCESSABLE_ENTITY,
            )));
        }
    }

    let on_disk = match tokio::fs::read(&data_path).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("Rewrap: failed to read file {}: {}", file_id, e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "File data not found on disk"})),
                warp::http::StatusCode::NOT_FOUND,
            )));
        }
    };

    #[allow(unused_assignments)]
    let mut plaintext: Option<Vec<u8>> = None;

    #[cfg(feature = "quantum")]
    {
        plaintext = None;
        if let Some(ref skp) = server_keypair {
            match envelope::decrypt_envelope_server_side_peel_sourced(
                &on_disk,
                &skp.secret_key,
                &skp.algorithm,
                envelope::SERVER_ENVELOPE_PEEL_MAX_LAYERS,
                Some(skp.key_source),
            ) {
                Ok(pt) => {
                    tracing::info!(
                        "Rewrap: decrypted envelope for file {} ({} bytes)",
                        file_id,
                        pt.len()
                    );
                    plaintext = Some(pt);
                }
                Err(e) => {
                    tracing::warn!("Rewrap: envelope decrypt failed for {}: {}", file_id, e);
                }
            }
        }
    }

    #[cfg(not(feature = "quantum"))]
    {
        plaintext = None;
    }

    // Fallback: .raw plaintext copy
    if plaintext.is_none() {
        let raw_path = data_dir.join(format!("{}.raw", &file_id));
        if let Ok(raw) = tokio::fs::read(&raw_path).await {
            plaintext = Some(raw);
        }
    }

    // Fallback: raw bytes if not an envelope
    if plaintext.is_none() {
        if envelope::deserialize_header(&on_disk).is_ok() {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(
                    &serde_json::json!({"error": "Cannot decrypt file — server keypair mismatch"}),
                ),
                warp::http::StatusCode::UNPROCESSABLE_ENTITY,
            )));
        }
        plaintext = Some(on_disk);
    }

    let plaintext = match plaintext {
        Some(pt) => pt,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Cannot decrypt file"})),
                warp::http::StatusCode::UNPROCESSABLE_ENTITY,
            )))
        }
    };

    // ── Re-encrypt to buyer's PK ──
    #[cfg(feature = "quantum")]
    let response_data = {
        match envelope::pqcrypto_encrypt_envelope(&plaintext, &buyer_pk, None) {
            Ok(env) => {
                tracing::info!(
                    "Rewrap: re-encrypted file {} ({} bytes) -> buyer {} ({} bytes envelope)",
                    file_id,
                    plaintext.len(),
                    buyer_did,
                    env.len()
                );
                env
            }
            Err(e) => {
                tracing::error!("Rewrap: re-encryption failed for file {}: {}", file_id, e);
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": format!("Re-encryption failed: {}", e)}),
                    ),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        }
    };

    #[cfg(not(feature = "quantum"))]
    let response_data = plaintext;

    if let Ok(Some(mut meta)) = db.get_file_metadata(&file_id) {
        meta.last_accessed = Some(chrono::Utc::now());
        let _ = db.insert_file_metadata(&meta);
    }

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::with_header(
            warp::reply::with_header(response_data, "Content-Type", "application/octet-stream"),
            "X-Envelope-Version",
            "1",
        ),
        warp::http::StatusCode::OK,
    )))
}

/// Admin-only diagnostic: inspect on-disk file format without decrypting.
async fn handle_file_diagnostic(
    file_id: String,
    _did: String,
    db: Arc<Database>,
    data_dir: Option<PathBuf>,
    server_keypair: Option<Arc<ServerKeypair>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::json(
                &serde_json::json!({"error": "File storage not configured"}),
            )))
        }
    };

    let meta = match db.get_file_metadata(&file_id) {
        Ok(Some(m)) => Some(serde_json::json!({
            "filename": m.filename,
            "size": m.size,
            "hash": m.hash,
            "owner_did": m.owner_did,
            "encryption_algorithm": m.encryption_algorithm,
            "content_type": m.content_type,
            "created_at": m.created_at.to_rfc3339(),
        })),
        Ok(None) => None,
        Err(e) => Some(serde_json::json!({"db_error": format!("{}", e)})),
    };

    let data_path = data_dir.join(&file_id);
    let on_disk = tokio::fs::read(&data_path).await.ok();

    let mut result = serde_json::json!({
        "file_id": file_id,
        "metadata": meta,
        "on_disk_exists": on_disk.is_some(),
    });

    if let Some(ref bytes) = on_disk {
        result["on_disk_size"] = serde_json::json!(bytes.len());

        let is_encrypted_data_json =
            serde_json::from_slice::<crate::quantum::EncryptedData>(bytes).is_ok();
        result["format_encrypted_data_json"] = serde_json::json!(is_encrypted_data_json);

        let envelope_parse = envelope::deserialize_header(bytes);
        match &envelope_parse {
            Ok((header, header_size)) => {
                result["format_envelope"] = serde_json::json!(true);
                result["envelope_header"] = serde_json::json!({
                    "version": header.version,
                    "kem_algorithm": header.kem_algorithm,
                    "cipher_suite": header.cipher_suite,
                    "chunk_size": header.chunk_size,
                    "total_chunks": header.total_chunks,
                    "total_plaintext_size": header.total_plaintext_size,
                    "plaintext_hash": header.plaintext_hash,
                    "header_size": header_size,
                });
            }
            Err(e) => {
                result["format_envelope"] = serde_json::json!(false);
                result["envelope_parse_error"] = serde_json::json!(format!("{}", e));
            }
        }

        if !is_encrypted_data_json && envelope_parse.is_err() {
            let first_bytes: Vec<u8> = bytes.iter().take(16).copied().collect();
            result["format_raw_bytes"] = serde_json::json!(true);
            result["first_16_bytes_hex"] = serde_json::json!(hex::encode(&first_bytes));
        }
    }

    if let Some(ref skp) = server_keypair {
        result["server_key"] = serde_json::json!({
            "algorithm": skp.algorithm,
            "key_source": skp.key_source,
            "public_key_len": skp.public_key.len(),
        });
    }

    let raw_path = data_dir.join(format!("{}.raw", &file_id));
    result["raw_fallback_exists"] = serde_json::json!(raw_path.exists());

    Ok(boxed_reply(warp::reply::json(&result)))
}

// ============================================================================
// Global User Registry Handlers
// ============================================================================

/// Handle global user registration
async fn handle_register_global_user(
    user: GlobalUser,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    tracing::info!("Registering global user: {} ({})", user.username, user.did);

    match db.register_global_user(&user) {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "user": user,
            })),
            warp::http::StatusCode::CREATED,
        )),
        Err(e) => {
            tracing::error!("Failed to register global user: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Registration failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle get global user by DID
async fn handle_get_global_user(did: String, db: Arc<Database>) -> Result<impl Reply, Rejection> {
    match db.get_global_user(&did) {
        Ok(Some(user)) => Ok(warp::reply::with_status(
            warp::reply::json(&user),
            warp::http::StatusCode::OK,
        )),
        Ok(None) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "User not found"
            })),
            warp::http::StatusCode::NOT_FOUND,
        )),
        Err(e) => {
            tracing::error!("Failed to get global user: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle update user presence
async fn handle_update_user_presence(
    did: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let is_online = body
        .get("is_online")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    match db.update_global_user_presence(&did, is_online) {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "is_online": is_online,
            })),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to update user presence: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Update failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

// ============================================================================
// Server Registry Handlers
// ============================================================================

/// Handle server creation
async fn handle_create_server(server: Server, db: Arc<Database>) -> Result<impl Reply, Rejection> {
    tracing::info!("Creating server: {} ({})", server.name, server.id);

    match db.create_server(&server) {
        Ok(_) => {
            // Create membership for owner
            let membership = ServerMembership {
                server_id: server.id.clone(),
                user_did: server.owner_did.clone(),
                role: "Owner".to_string(),
                joined_at: chrono::Utc::now(),
                invited_by: None,
            };

            if let Err(e) = db.add_server_membership(&membership) {
                tracing::warn!("Failed to create owner membership: {}", e);
            }

            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "success": true,
                    "server": server,
                })),
                warp::http::StatusCode::CREATED,
            ))
        }
        Err(e) => {
            tracing::error!("Failed to create server: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Creation failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle get servers (with optional filtering)
async fn handle_get_servers(
    query_params: HashMap<String, String>,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let server_type = query_params.get("type").map(|s| s.as_str());

    match db.get_all_servers(server_type) {
        Ok(servers) => Ok(warp::reply::with_status(
            warp::reply::json(&servers),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to get servers: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle get server by ID
async fn handle_get_server(server_id: String, db: Arc<Database>) -> Result<impl Reply, Rejection> {
    match db.get_server(&server_id) {
        Ok(Some(server)) => Ok(warp::reply::with_status(
            warp::reply::json(&server),
            warp::http::StatusCode::OK,
        )),
        Ok(None) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Server not found"
            })),
            warp::http::StatusCode::NOT_FOUND,
        )),
        Err(e) => {
            tracing::error!("Failed to get server: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle join server
async fn handle_join_server(
    server_id: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let user_did = match body.get("user_did").and_then(|v| v.as_str()) {
        Some(did) => did,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing user_did"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    // Verify server exists and is joinable
    let server = match db.get_server(&server_id) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server not found"
                })),
                warp::http::StatusCode::NOT_FOUND,
            ));
        }
        Err(e) => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ));
        }
    };

    // Check access for private servers (invitation required)
    if server.server_type == "Private" {
        let invitation_code = match body.get("invitation_code").and_then(|v| v.as_str()) {
            Some(code) => code,
            None => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": "Invitation required for private server",
                        "details": "Provide invitation_code or use /api/servers/{id}/invitations/use"
                    })),
                    warp::http::StatusCode::FORBIDDEN,
                ));
            }
        };

        match db.use_server_invitation(&server_id, invitation_code, user_did) {
            Ok(invitation) => {
                let membership = ServerMembership {
                    server_id: server_id.clone(),
                    user_did: user_did.to_string(),
                    role: invitation.role.clone(),
                    joined_at: chrono::Utc::now(),
                    invited_by: Some(invitation.inviter_did.clone()),
                };

                if let Err(e) = db.add_server_membership(&membership) {
                    tracing::error!("Failed to add membership for private server: {}", e);
                    return Ok(warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({
                            "error": "Failed to create membership"
                        })),
                        warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                    ));
                }

                return Ok(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "success": true,
                        "membership": membership,
                        "invitation": invitation
                    })),
                    warp::http::StatusCode::OK,
                ));
            }
            Err(e) => {
                return Ok(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": format!("Invalid invitation: {}", e)
                    })),
                    warp::http::StatusCode::FORBIDDEN,
                ));
            }
        }
    }

    let membership = ServerMembership {
        server_id: server_id.clone(),
        user_did: user_did.to_string(),
        role: "Member".to_string(),
        joined_at: chrono::Utc::now(),
        invited_by: body
            .get("invited_by")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };

    match db.add_server_membership(&membership) {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "membership": membership,
            })),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to join server: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Join failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle get server members
async fn handle_get_server_members(
    server_id: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    match db.get_server_members(&server_id) {
        Ok(members) => Ok(warp::reply::with_status(
            warp::reply::json(&members),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to get server members: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle update member role
async fn handle_update_member_role(
    server_id: String,
    user_did: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let new_role = match body.get("role").and_then(|v| v.as_str()) {
        Some(role) => role,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing role"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    let updated_by = match body.get("updated_by").and_then(|v| v.as_str()) {
        Some(did) => did,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing updated_by"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    match db.update_server_member_role(&server_id, &user_did, new_role, updated_by) {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "message": "Role updated successfully"
            })),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to update member role: {}", e);
            let status = if e.to_string().contains("Permission denied") {
                warp::http::StatusCode::FORBIDDEN
            } else if e.to_string().contains("not found") {
                warp::http::StatusCode::NOT_FOUND
            } else {
                warp::http::StatusCode::INTERNAL_SERVER_ERROR
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Update failed: {}", e)
                })),
                status,
            ))
        }
    }
}

/// Handle remove member
async fn handle_remove_member(
    server_id: String,
    user_did: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let removed_by = match body.get("removed_by").and_then(|v| v.as_str()) {
        Some(did) => did,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing removed_by"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    match db.remove_server_member(&server_id, &user_did, removed_by) {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "message": "Member removed successfully"
            })),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to remove member: {}", e);
            let status = if e.to_string().contains("Permission denied") {
                warp::http::StatusCode::FORBIDDEN
            } else if e.to_string().contains("not found") {
                warp::http::StatusCode::NOT_FOUND
            } else {
                warp::http::StatusCode::INTERNAL_SERVER_ERROR
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Remove failed: {}", e)
                })),
                status,
            ))
        }
    }
}

/// Handle create invitation
async fn handle_create_invitation(
    server_id: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    use crate::database::ServerInvitation;
    use uuid::Uuid;

    let inviter_did = match body.get("inviter_did").and_then(|v| v.as_str()) {
        Some(did) => did,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing inviter_did"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    let invitee_did = body
        .get("invitee_did")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let role = body
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("Member");
    let expires_in_hours = body
        .get("expires_in_hours")
        .and_then(|v| v.as_u64())
        .unwrap_or(168); // Default 7 days

    let invitation = ServerInvitation {
        invitation_id: Uuid::new_v4().to_string(),
        server_id: server_id.clone(),
        inviter_did: inviter_did.to_string(),
        invitee_did,
        invitation_code: Uuid::new_v4()
            .to_string()
            .replace("-", "")
            .chars()
            .take(16)
            .collect(),
        role: role.to_string(),
        created_at: chrono::Utc::now(),
        expires_at: Some(chrono::Utc::now() + chrono::Duration::hours(expires_in_hours as i64)),
        used_at: None,
        used_by: None,
        is_active: true,
    };

    match db.create_server_invitation(&invitation) {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "invitation": invitation,
            })),
            warp::http::StatusCode::CREATED,
        )),
        Err(e) => {
            tracing::error!("Failed to create invitation: {}", e);
            let status = if e.to_string().contains("Permission denied") {
                warp::http::StatusCode::FORBIDDEN
            } else {
                warp::http::StatusCode::INTERNAL_SERVER_ERROR
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Creation failed: {}", e)
                })),
                status,
            ))
        }
    }
}

/// Handle get invitations
async fn handle_get_invitations(
    server_id: String,
    query_params: std::collections::HashMap<String, String>,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let active_only = query_params
        .get("active_only")
        .and_then(|v| v.parse::<bool>().ok())
        .unwrap_or(true);

    match db.get_server_invitations(&server_id, active_only) {
        Ok(invitations) => Ok(warp::reply::with_status(
            warp::reply::json(&invitations),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to get invitations: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle use invitation
async fn handle_use_invitation(
    server_id: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let invitation_code = match body.get("invitation_code").and_then(|v| v.as_str()) {
        Some(code) => code,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing invitation_code"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    let used_by = match body.get("user_did").and_then(|v| v.as_str()) {
        Some(did) => did,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing user_did"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    match db.use_server_invitation(&server_id, invitation_code, used_by) {
        Ok(invitation) => {
            // Create membership with the role from invitation
            let role = invitation.role.clone();
            let inviter_did = invitation.inviter_did.clone();

            let membership = crate::database::ServerMembership {
                server_id: server_id.clone(),
                user_did: used_by.to_string(),
                role: role.clone(),
                joined_at: chrono::Utc::now(),
                invited_by: Some(inviter_did.clone()),
            };

            if let Err(e) = db.add_server_membership(&membership) {
                tracing::error!("Failed to create membership after using invitation: {}", e);
            }

            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "success": true,
                    "invitation": invitation,
                    "membership": membership,
                })),
                warp::http::StatusCode::OK,
            ))
        }
        Err(e) => {
            tracing::error!("Failed to use invitation: {}", e);
            let status = if e.to_string().contains("expired")
                || e.to_string().contains("Invalid operation")
            {
                warp::http::StatusCode::BAD_REQUEST
            } else if e.to_string().contains("Permission denied") {
                warp::http::StatusCode::FORBIDDEN
            } else if e.to_string().contains("not found") {
                warp::http::StatusCode::NOT_FOUND
            } else {
                warp::http::StatusCode::INTERNAL_SERVER_ERROR
            };
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Failed to use invitation: {}", e)
                })),
                status,
            ))
        }
    }
}

// ============================================================================
// Global Group Registry Handlers
// ============================================================================

/// Handle create group
async fn handle_create_group(
    group: GlobalGroup,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    tracing::info!("Creating group: {} ({})", group.name, group.id);

    match db.create_global_group(&group) {
        Ok(_) => {
            // Create membership for creator
            let membership = GroupMembership {
                group_id: group.id.clone(),
                user_did: group.creator_did.clone(),
                role: "Creator".to_string(),
                joined_at: chrono::Utc::now(),
                invited_by: None,
            };

            if let Err(e) = db.add_group_membership(&membership) {
                tracing::warn!("Failed to create creator membership: {}", e);
            }

            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "success": true,
                    "group": group,
                })),
                warp::http::StatusCode::CREATED,
            ))
        }
        Err(e) => {
            tracing::error!("Failed to create group: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Creation failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle get groups (with optional filtering)
async fn handle_get_groups(
    query_params: HashMap<String, String>,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let server_id = query_params.get("server_id").map(|s| s.as_str());
    let group_type = query_params.get("type").map(|s| s.as_str());

    match db.get_all_global_groups(server_id, group_type) {
        Ok(groups) => Ok(warp::reply::with_status(
            warp::reply::json(&groups),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to get groups: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle get group by ID
async fn handle_get_group(group_id: String, db: Arc<Database>) -> Result<impl Reply, Rejection> {
    match db.get_global_group(&group_id) {
        Ok(Some(group)) => Ok(warp::reply::with_status(
            warp::reply::json(&group),
            warp::http::StatusCode::OK,
        )),
        Ok(None) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Group not found"
            })),
            warp::http::StatusCode::NOT_FOUND,
        )),
        Err(e) => {
            tracing::error!("Failed to get group: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle join group
async fn handle_join_group(
    group_id: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let user_did = match body.get("user_did").and_then(|v| v.as_str()) {
        Some(did) => did,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing user_did"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    let membership = GroupMembership {
        group_id: group_id.clone(),
        user_did: user_did.to_string(),
        role: "Member".to_string(),
        joined_at: chrono::Utc::now(),
        invited_by: body
            .get("invited_by")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };

    match db.add_group_membership(&membership) {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "membership": membership,
            })),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to join group: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Join failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle get group members
async fn handle_get_group_members(
    group_id: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    match db.get_group_members(&group_id) {
        Ok(members) => Ok(warp::reply::with_status(
            warp::reply::json(&members),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to get group members: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

// ============================================================================
// Feed Subscription Handlers
// ============================================================================

/// Handle subscribe to feed
async fn handle_subscribe_feed(
    group_id: String,
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    let subscriber_did = match body.get("subscriber_did").and_then(|v| v.as_str()) {
        Some(did) => did,
        None => {
            return Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Missing subscriber_did"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            ));
        }
    };

    let notification_preferences = body
        .get("notification_preferences")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));

    let subscription = FeedSubscription {
        subscriber_did: subscriber_did.to_string(),
        group_id: group_id.clone(),
        subscribed_at: chrono::Utc::now(),
        notification_preferences,
        last_read_at: None,
    };

    match db.create_feed_subscription(&subscription) {
        Ok(_) => Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "subscription": subscription,
            })),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to subscribe to feed: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Subscription failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

/// Handle get user subscriptions
async fn handle_get_user_subscriptions(
    user_did: String,
    db: Arc<Database>,
) -> Result<impl Reply, Rejection> {
    match db.get_user_subscriptions(&user_did) {
        Ok(subscriptions) => Ok(warp::reply::with_status(
            warp::reply::json(&subscriptions),
            warp::http::StatusCode::OK,
        )),
        Err(e) => {
            tracing::error!("Failed to get user subscriptions: {}", e);
            Ok(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            ))
        }
    }
}

async fn handle_rate_limit_check(
    body: RateLimitCheckBody,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    if body.key.is_empty() || body.key.len() > 256 || body.prefix.len() > 64 {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "allowed": false,
                "error": "Invalid key/prefix"
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }
    if body.max_requests == 0 || body.window_seconds == 0 || body.window_seconds > 3600 {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "allowed": false,
                "error": "Invalid rate limit parameters"
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let allowed = db
        .rate_limit_check(
            &body.prefix,
            &body.key,
            body.max_requests,
            body.window_seconds,
        )
        .unwrap_or(false);

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({ "allowed": allowed })),
        warp::http::StatusCode::OK,
    )))
}

// ============================================================================
// Cross-Server Routing handlers
// ============================================================================

async fn handle_connect_server(
    server_id: String,
    _requester_did: String,
    db: Arc<Database>,
    routing: Option<Arc<ServerRoutingManager>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let routing = match routing {
        Some(r) => r,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server routing not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    let server = match db.get_server(&server_id) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server not found"
                })),
                warp::http::StatusCode::NOT_FOUND,
            )));
        }
        Err(e) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": format!("Query failed: {}", e)
                })),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };

    match routing.connect_to_server(server).await {
        Ok(connection_id) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true,
                "connection_id": connection_id
            })),
            warp::http::StatusCode::OK,
        ))),
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("Connect failed: {}", e)
            })),
            warp::http::StatusCode::BAD_REQUEST,
        ))),
    }
}

async fn handle_disconnect_server(
    server_id: String,
    _requester_did: String,
    routing: Option<Arc<ServerRoutingManager>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let routing = match routing {
        Some(r) => r,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server routing not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    match routing.disconnect_from_server(&server_id).await {
        Ok(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "success": true
            })),
            warp::http::StatusCode::OK,
        ))),
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": format!("Disconnect failed: {}", e)
            })),
            warp::http::StatusCode::BAD_REQUEST,
        ))),
    }
}

async fn handle_server_connection_status(
    server_id: String,
    _requester_did: String,
    routing: Option<Arc<ServerRoutingManager>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let routing = match routing {
        Some(r) => r,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server routing not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    let status = routing.get_server_connection_status(&server_id).await;
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "server_id": server_id,
            "status": status
        })),
        warp::http::StatusCode::OK,
    )))
}

async fn handle_connected_servers(
    _requester_did: String,
    routing: Option<Arc<ServerRoutingManager>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let routing = match routing {
        Some(r) => r,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server routing not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    let servers = routing.get_connected_servers().await;
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&servers),
        warp::http::StatusCode::OK,
    )))
}

async fn handle_subscribe_server_topic(
    server_id: String,
    _requester_did: String,
    body: ServerTopicRequest,
    routing: Option<Arc<ServerRoutingManager>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let routing = match routing {
        Some(r) => r,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server routing not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    match routing
        .subscribe_to_server_topic(&server_id, body.topic)
        .await
    {
        Ok(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "success": true })),
            warp::http::StatusCode::OK,
        ))),
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": format!("Subscribe failed: {}", e) })),
            warp::http::StatusCode::BAD_REQUEST,
        ))),
    }
}

async fn handle_unsubscribe_server_topic(
    server_id: String,
    _requester_did: String,
    body: ServerTopicRequest,
    routing: Option<Arc<ServerRoutingManager>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let routing = match routing {
        Some(r) => r,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server routing not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    match routing
        .unsubscribe_from_server_topic(&server_id, &body.topic)
        .await
    {
        Ok(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "success": true })),
            warp::http::StatusCode::OK,
        ))),
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({ "error": format!("Unsubscribe failed: {}", e) }),
            ),
            warp::http::StatusCode::BAD_REQUEST,
        ))),
    }
}

async fn handle_send_server_message(
    server_id: String,
    _requester_did: String,
    body: ServerSendRequest,
    routing: Option<Arc<ServerRoutingManager>>,
) -> Result<Box<dyn Reply>, Rejection> {
    let routing = match routing {
        Some(r) => r,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Server routing not configured"
                })),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )));
        }
    };

    match routing
        .send_message_to_server(&server_id, body.message.as_bytes())
        .await
    {
        Ok(_) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "success": true })),
            warp::http::StatusCode::OK,
        ))),
        Err(e) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": format!("Send failed: {}", e) })),
            warp::http::StatusCode::BAD_REQUEST,
        ))),
    }
}

// ============================================================================
// App Package API Types and Handlers
// ============================================================================

/// Query parameters for listing apps
#[derive(Debug, Clone, Deserialize)]
struct AppListQuery {
    category: Option<String>,
    creator: Option<String>,
    search: Option<String>,
    featured: Option<bool>,
    free_only: Option<bool>,
    limit: Option<usize>,
    offset: Option<usize>,
}

/// Query parameters for app search
#[derive(Debug, Clone, Deserialize)]
struct AppSearchQuery {
    q: String,
    limit: Option<usize>,
}

/// Query parameters for limit
#[derive(Debug, Clone, Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

/// App summary for API responses
#[derive(Debug, Clone, Serialize)]
struct AppSummary {
    app_id: String,
    name: String,
    description: String,
    tagline: Option<String>,
    creator_did: String,
    version: String,
    category: String,
    pricing: String,
    total_size: u64,
    created_at: u64,
    download_count: u64,
    rating: Option<f32>,
    keywords: Vec<String>,
    icon: Option<String>,
}

/// App details for API responses
#[derive(Debug, Clone, Serialize)]
struct AppDetails {
    app_id: String,
    name: String,
    description: String,
    tagline: Option<String>,
    creator_did: String,
    version: String,
    category: String,
    pricing: serde_json::Value,
    total_size: u64,
    created_at: u64,
    download_count: u64,
    rating: Option<f32>,
    keywords: Vec<String>,
    icon: Option<String>,
    screenshots: Vec<String>,
    entry_points: Vec<serde_json::Value>,
    permissions: Vec<String>,
    platforms: Vec<String>,
    content_count: usize,
}

/// Handler for listing apps
async fn handle_list_apps(
    query: AppListQuery,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    // Query apps from the database (stored as facts with "app-package" tag)
    // For now, return a placeholder response since we need the AppStorageEngine
    // In production, this would query the AppStorageEngine

    let apps: Vec<AppSummary> = Vec::new();
    let response = serde_json::json!({
        "apps": apps,
        "total_count": 0,
        "has_more": false,
        "query": {
            "category": query.category,
            "creator": query.creator,
            "search": query.search,
            "featured": query.featured,
            "free_only": query.free_only,
            "limit": query.limit.unwrap_or(50),
            "offset": query.offset.unwrap_or(0),
        }
    });

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&response),
        warp::http::StatusCode::OK,
    )))
}

/// Handler for getting a specific app
async fn handle_get_app(app_id: String, db: Arc<Database>) -> Result<Box<dyn Reply>, Rejection> {
    // Validate app_id format (should be 64 hex chars for 32 bytes)
    if app_id.len() != 64 || !app_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Invalid app ID format. Expected 64 hex characters."
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    // In production, this would fetch from AppStorageEngine
    // For now, return a not found response
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "error": "App not found",
            "app_id": app_id
        })),
        warp::http::StatusCode::NOT_FOUND,
    )))
}

/// Handler for getting app versions
async fn handle_get_app_versions(
    app_id: String,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    // Validate app_id format
    if app_id.len() != 64 || !app_id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Invalid app ID format. Expected 64 hex characters."
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    // Return empty versions list for now
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "app_id": app_id,
            "versions": []
        })),
        warp::http::StatusCode::OK,
    )))
}

/// Handler for getting featured apps
async fn handle_get_featured_apps(
    query: LimitQuery,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    let limit = query.limit.unwrap_or(10).min(50);

    // Return empty list for now
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "apps": [],
            "limit": limit
        })),
        warp::http::StatusCode::OK,
    )))
}

/// Handler for searching apps
async fn handle_search_apps(
    query: AppSearchQuery,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    let limit = query.limit.unwrap_or(20).min(100);

    // Return empty results for now
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "query": query.q,
            "apps": [],
            "total_count": 0
        })),
        warp::http::StatusCode::OK,
    )))
}

/// Handler for getting app stats
async fn handle_get_app_stats(db: Arc<Database>) -> Result<Box<dyn Reply>, Rejection> {
    // Return placeholder stats
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "total_apps": 0,
            "total_creators": 0,
            "total_downloads": 0,
            "featured_count": 0,
            "categories": {}
        })),
        warp::http::StatusCode::OK,
    )))
}

/// Handler for getting apps by category
async fn handle_get_apps_by_category(
    category: String,
    query: LimitQuery,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    let limit = query.limit.unwrap_or(20).min(100);

    // Valid categories
    let valid_categories = [
        "productivity",
        "communication",
        "finance",
        "entertainment",
        "social",
        "utilities",
        "developer_tools",
        "education",
        "health",
        "lifestyle",
        "gaming",
        "media",
        "other",
    ];

    let category_lower = category.to_lowercase();
    if !valid_categories.contains(&category_lower.as_str()) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Invalid category",
                "valid_categories": valid_categories
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    // Return empty list for now
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "category": category,
            "apps": [],
            "total_count": 0
        })),
        warp::http::StatusCode::OK,
    )))
}

/// Handler for getting apps by creator
async fn handle_get_apps_by_creator(
    creator_did: String,
    query: LimitQuery,
    db: Arc<Database>,
) -> Result<Box<dyn Reply>, Rejection> {
    let limit = query.limit.unwrap_or(20).min(100);

    // Validate DID format
    if !creator_did.starts_with("did:") {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Invalid DID format. Expected 'did:...' format."
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    // Return empty list for now
    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "creator": creator_did,
            "apps": [],
            "total_count": 0
        })),
        warp::http::StatusCode::OK,
    )))
}

// ═══════════════════════════════════════════════════════════════════════════════
// DID Registry API handlers
// ═══════════════════════════════════════════════════════════════════════════════

/// POST /api/did/register
/// Body: { "did": "did:spacekit:testnet:abc...", "document": { ... } }
/// Stores the DID document in the database under `did_registry` collection.
async fn handle_did_register(
    body: serde_json::Value,
    db: Arc<Database>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let did = match body["did"].as_str() {
        Some(d) => d.to_string(),
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "missing did"})),
                warp::http::StatusCode::BAD_REQUEST,
            )));
        }
    };

    if !did.starts_with("did:spacekit:") {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "invalid DID format"})),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let document = body
        .get("document")
        .cloned()
        .unwrap_or_else(|| body.clone());

    let doc_id = did.replace(':', "_");
    let now = chrono::Utc::now();
    let record = DocumentRecord {
        owner_did: "system".to_string(),
        collection: "did_registry".to_string(),
        id: doc_id,
        data: serde_json::json!({
            "did": did,
            "document": document,
            "registered_at": now.to_rfc3339(),
        }),
        created_at: now,
        updated_at: now,
        blob_ref: None,
    };

    if let Err(e) = db.upsert_document(&record) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": e.to_string()})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )));
    }

    tracing::info!("DID registered: {}", did);

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "did": did,
            "status": "registered",
        })),
        warp::http::StatusCode::CREATED,
    )))
}

/// GET /api/did/resolve/{did_encoded}
/// The DID is URL-path encoded (e.g. did_spacekit_testnet_abc123).
async fn handle_did_resolve(
    did_encoded: String,
    db: Arc<Database>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let doc_id = did_encoded.replace(':', "_");

    match db.get_document("system", "did_registry", &doc_id) {
        Ok(Some(record)) => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "did": record.data["did"],
                "document": record.data["document"],
                "resolved": true,
            })),
            warp::http::StatusCode::OK,
        ))),
        _ => Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "did": did_encoded,
                "resolved": false,
                "error": "DID not found",
            })),
            warp::http::StatusCode::NOT_FOUND,
        ))),
    }
}

// ---------------------------------------------------------------------------
// CAS Blob Storage handlers
// ---------------------------------------------------------------------------

fn blob_dir(data_dir: &std::path::Path, hash: &str) -> PathBuf {
    let prefix = &hash[..2.min(hash.len())];
    data_dir.join("blobs").join(prefix)
}

fn blob_path(data_dir: &std::path::Path, hash: &str) -> PathBuf {
    blob_dir(data_dir, hash).join(hash)
}

fn is_valid_blake3_hex(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// PUT /blobs/{hash} — upload raw bytes, verify BLAKE3, dedup on disk
async fn handle_put_blob(
    hash: String,
    body: Bytes,
    data_dir: Option<PathBuf>,
    semaphore: Arc<tokio::sync::Semaphore>,
    auth_mode: crate::access_policy::BlobFactAuthMode,
    auth_header: Option<String>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let _permit = semaphore.acquire().await.map_err(|_| warp::reject())?;

    let now = unix_now_secs();
    let secret = crate::upload_token::load_signing_secret(data_dir.as_deref());
    if auth_mode.blobs_require_did_on_write() {
        let authorized = crate::upload_token::authorize_blob_write(
            auth_header.as_deref(),
            &hash,
            secret.as_deref(),
            now,
        );
        if authorized.is_none() {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "Authorization required (DID or UploadToken for put_blob)"
                })),
                warp::http::StatusCode::UNAUTHORIZED,
            )));
        }
    }

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    if !is_valid_blake3_hex(&hash) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({"error": "Invalid BLAKE3 hash (expected 64 hex chars)"}),
            ),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let actual_hash = hex::encode(blake3::hash(&body).as_bytes());
    if actual_hash != hash {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": "Hash mismatch",
                "expected": hash,
                "actual": actual_hash,
            })),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let path = blob_path(&data_dir, &hash);

    if path.exists() {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "hash": hash,
                "size": body.len(),
                "status": "exists",
            })),
            warp::http::StatusCode::OK,
        )));
    }

    let dir = blob_dir(&data_dir, &hash);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        tracing::error!("Failed to create blob directory: {}", e);
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": format!("Storage error: {}", e)})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )));
    }

    if let Err(e) = tokio::fs::write(&path, &body).await {
        tracing::error!("Failed to write blob: {}", e);
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": format!("Write error: {}", e)})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )));
    }

    tracing::info!("Stored blob {} ({} bytes)", hash, body.len());

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "hash": hash,
            "size": body.len(),
            "status": "created",
        })),
        warp::http::StatusCode::CREATED,
    )))
}

/// GET /blobs/{hash} — stream raw bytes from CAS
fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

async fn handle_get_blob(
    hash: String,
    data_dir: Option<PathBuf>,
    semaphore: Arc<tokio::sync::Semaphore>,
    auth_mode: crate::access_policy::BlobFactAuthMode,
    auth_header: Option<String>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let _permit = semaphore.acquire().await.map_err(|_| warp::reject())?;

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let now = unix_now_secs();
    let secret = crate::upload_token::load_signing_secret(Some(&data_dir));
    if auth_mode.blobs_require_did_on_read() {
        let requester = match crate::upload_token::authorize_blob_read(
            auth_header.as_deref(),
            &hash,
            secret.as_deref(),
            now,
        ) {
            Some(d) => d,
            None => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": "Authorization required (DID or UploadToken for get_blob)"}),
                    ),
                    warp::http::StatusCode::UNAUTHORIZED,
                )));
            }
        };
        match crate::access_policy::blob_allows_reader(&data_dir, &hash, &requester).await {
            Ok(true) => {}
            Ok(false) => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": "forbidden"})),
                    warp::http::StatusCode::FORBIDDEN,
                )));
            }
            Err(e) => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": e.to_string()})),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        }
    }

    if !is_valid_blake3_hex(&hash) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Invalid BLAKE3 hash"})),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let path = blob_path(&data_dir, &hash);
    let meta = match tokio::fs::metadata(&path).await {
        Ok(m) => m,
        Err(_) => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Blob not found"})),
                warp::http::StatusCode::NOT_FOUND,
            )));
        }
    };
    let (stream, _file_meta) =
        crate::streaming::file_stream(&path, crate::streaming::StreamingConfig::default(), None)
            .await
            .map_err(|_| warp::reject())?;
    let body = warp::hyper::Body::wrap_stream(stream);
    let mut response = warp::reply::Response::new(body);
    *response.status_mut() = warp::http::StatusCode::OK;
    response.headers_mut().insert(
        warp::http::header::CONTENT_TYPE,
        warp::http::HeaderValue::from_static("application/octet-stream"),
    );
    response.headers_mut().insert(
        warp::http::header::CONTENT_LENGTH,
        warp::http::HeaderValue::from_str(&meta.len().to_string())
            .unwrap_or(warp::http::HeaderValue::from_static("0")),
    );
    Ok(boxed_reply(response))
}

/// HEAD /blobs/{hash} — existence check with Content-Length
async fn handle_head_blob(
    hash: String,
    data_dir: Option<PathBuf>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    if !is_valid_blake3_hex(&hash) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({})),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let path = blob_path(&data_dir, &hash);
    match tokio::fs::metadata(&path).await {
        Ok(meta) => Ok(boxed_reply(warp::reply::with_header(
            warp::reply::with_status(Vec::<u8>::new(), warp::http::StatusCode::OK),
            "content-length",
            meta.len().to_string(),
        ))),
        Err(_) => Ok(boxed_reply(warp::reply::with_status(
            Vec::<u8>::new(),
            warp::http::StatusCode::NOT_FOUND,
        ))),
    }
}

/// POST /blobs/exists — batch existence check
/// Body: { "hashes": ["abc...", "def..."] }
/// Returns: { "missing": ["def..."], "found": ["abc..."] }
async fn handle_blobs_exist(
    body: serde_json::Value,
    data_dir: Option<PathBuf>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let hashes = match body.get("hashes").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>(),
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Expected { \"hashes\": [...] }"})),
                warp::http::StatusCode::BAD_REQUEST,
            )))
        }
    };

    let mut missing = Vec::new();
    let mut found = Vec::new();

    for h in &hashes {
        if !is_valid_blake3_hex(h) {
            missing.push(h.clone());
            continue;
        }
        let path = blob_path(&data_dir, h);
        if path.exists() {
            found.push(h.clone());
        } else {
            missing.push(h.clone());
        }
    }

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "missing": missing,
            "found": found,
        })),
        warp::http::StatusCode::OK,
    )))
}

// ---------------------------------------------------------------------------
// Fact Package API handlers
// ---------------------------------------------------------------------------

fn fact_dir(data_dir: &std::path::Path, fact_id_hex: &str) -> PathBuf {
    crate::fact_sidecar::fact_dir(data_dir, fact_id_hex)
}

fn fact_path(data_dir: &std::path::Path, fact_id_hex: &str) -> PathBuf {
    crate::fact_sidecar::fact_json_path(data_dir, fact_id_hex)
}

fn fact_blob_path(data_dir: &std::path::Path, fact_id_hex: &str) -> PathBuf {
    crate::fact_sidecar::fact_blob_path(data_dir, fact_id_hex)
}

fn fact_blob_meta_path(data_dir: &std::path::Path, fact_id_hex: &str) -> PathBuf {
    crate::fact_sidecar::fact_blob_meta_path(data_dir, fact_id_hex)
}

/// POST /facts — submit a signed FactPackage (JSON body)
async fn handle_post_fact(
    fact: FactPackage,
    data_dir: Option<PathBuf>,
    db: Arc<Database>,
    semaphore: Arc<tokio::sync::Semaphore>,
    auth_mode: crate::access_policy::BlobFactAuthMode,
    auth_header: Option<String>,
    quantum_crypto: Option<Arc<QuantumCrypto>>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let _permit = semaphore.acquire().await.map_err(|_| warp::reject())?;

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    if auth_mode.facts_require_did() {
        let now = unix_now_secs();
        let secret = crate::upload_token::load_signing_secret(Some(&data_dir));
        let requester = match crate::upload_token::authorize_fact_post(
            auth_header.as_deref(),
            secret.as_deref(),
            now,
        ) {
            Some(d) => d,
            None => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": "Authorization required (DID or UploadToken for put_fact)"}),
                    ),
                    warp::http::StatusCode::UNAUTHORIZED,
                )));
            }
        };
        let author_did = fact.author.to_string();
        if !crate::access_policy::fact_post_allowed(&author_did, &requester) {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(
                    &serde_json::json!({"error": "author DID must match Authorization"}),
                ),
                warp::http::StatusCode::FORBIDDEN,
            )));
        }
    }

    if crate::access_policy::fact_requires_signature(auth_mode) {
        if fact.signature.signature_bytes.is_empty() {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({
                    "error": "strict mode requires a non-empty SPHINCS+ signature on facts"
                })),
                warp::http::StatusCode::BAD_REQUEST,
            )));
        }
        let crypto = match quantum_crypto {
            Some(c) => c,
            None => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "error": "strict mode requires quantum feature for signature verification"
                    })),
                    warp::http::StatusCode::SERVICE_UNAVAILABLE,
                )));
            }
        };
        match crate::access_policy::verify_fact_signature(&crypto, &fact).await {
            Ok(true) => {}
            Ok(false) => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": "invalid fact signature"})),
                    warp::http::StatusCode::FORBIDDEN,
                )));
            }
            Err(e) => {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": e.to_string()})),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        }
    }

    let fact_id_hex = hex::encode(fact.fact_id);

    let path = fact_path(&data_dir, &fact_id_hex);
    if path.exists() {
        tracing::info!("Overwriting existing fact at {}", fact_id_hex);
    }

    if let Err(e) = crate::fact_sidecar::persist_fact_with_sidecar(&data_dir, &fact).await {
        tracing::error!("Failed to write fact: {}", e);
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": format!("Write error: {}", e)})),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )));
    }

    let dep_ids: Vec<String> = fact.dependencies.iter().map(hex::encode).collect();
    let author_did = fact.author.to_string();

    let schema_opt = match &fact.content {
        FactContent::Json { schema, .. } => schema.clone(),
        _ => None,
    };
    let mirror_for_website_api_index =
        schema_opt.as_deref() == Some("spacekit:agent:deployment:v1");

    let created_at_for_sort = match &fact.content {
        FactContent::Json { data, .. } => data
            .get("created_at")
            .cloned()
            .unwrap_or(serde_json::json!(fact.created_at)),
        _ => serde_json::json!(fact.created_at),
    };

    let index_data = serde_json::json!({
        "fact_id": fact_id_hex,
        "version": fact.version,
        "created_at": created_at_for_sort,
        "author": author_did,
        "dependencies": dep_ids,
        "content_type": fact.content.content_type(),
        "tags": fact.metadata.tags,
        "schema": schema_opt,
        "category": fact.metadata.category,
    });

    let now = chrono::Utc::now();
    let index_doc = DocumentRecord {
        owner_did: author_did.clone(),
        collection: "fact_index".to_string(),
        id: fact_id_hex.clone(),
        data: index_data,
        created_at: now,
        updated_at: now,
        blob_ref: None,
    };
    let _ = db.upsert_document(&index_doc);

    /// Same DID used when storing deployment receipts for AgentHub (`spacekit storage deploy`).
    const WEBSITE_API_DEPLOY_INDEX_DID: &str = "did:spacekit:admin:website-api";
    if mirror_for_website_api_index {
        let mirror = DocumentRecord {
            owner_did: WEBSITE_API_DEPLOY_INDEX_DID.to_string(),
            ..index_doc.clone()
        };
        let _ = db.upsert_document(&mirror);
    }

    if let Ok(commit) = spacekit_repo::parse_commit_from_fact_package(&fact) {
        let _ = crate::access_policy::register_commit_tree_refs(
            &data_dir,
            &fact_id_hex,
            &fact.author.to_string(),
            &fact.access_policy,
            &commit.tree,
        )
        .await;
    }

    let stored_bytes = tokio::fs::metadata(fact_path(&data_dir, &fact_id_hex))
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    tracing::info!(
        "Stored fact {} ({} bytes json on disk)",
        fact_id_hex,
        stored_bytes
    );

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "fact_id": fact_id_hex,
            "status": "created",
        })),
        warp::http::StatusCode::CREATED,
    )))
}

/// GET /facts/{fact_id} — retrieve full FactPackage
async fn handle_get_fact(
    fact_id_hex: String,
    data_dir: Option<PathBuf>,
    semaphore: Arc<tokio::sync::Semaphore>,
    auth_mode: crate::access_policy::BlobFactAuthMode,
    requester_did: Option<String>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let _permit = semaphore.acquire().await.map_err(|_| warp::reject())?;

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    if fact_id_hex.len() != 64 || !fact_id_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({"error": "Invalid fact_id (expected 64 hex chars)"}),
            ),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let path = fact_path(&data_dir, &fact_id_hex);
    if let Err(e) = crate::fact_sidecar::ensure_fact_externalized(&data_dir, &fact_id_hex).await {
        tracing::warn!("fact externalize {}: {}", fact_id_hex, e);
    }
    match crate::fact_sidecar::read_fact_json(&data_dir, &fact_id_hex).await {
        Ok(fact) => {
            if auth_mode.facts_require_did() {
                let requester = match requester_did {
                    Some(d) => d,
                    None => {
                        return Ok(boxed_reply(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "error": "Authorization: DID required"
                            })),
                            warp::http::StatusCode::UNAUTHORIZED,
                        )));
                    }
                };
                if !crate::access_policy::fact_allows_reader(
                    &fact.access_policy,
                    &requester,
                    &fact.author.to_string(),
                ) {
                    return Ok(boxed_reply(warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({"error": "forbidden"})),
                        warp::http::StatusCode::FORBIDDEN,
                    )));
                }
            }
            Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&fact),
                warp::http::StatusCode::OK,
            )))
        }
        Err(e) => {
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
            {
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": "Fact not found"})),
                    warp::http::StatusCode::NOT_FOUND,
                )));
            }
            tracing::error!("Failed to read fact {}: {}", fact_id_hex, e);
            Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": format!("Read error: {}", e)})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )))
        }
    }
}

/// GET /facts/{fact_id}/stream — serve raw binary content with HTTP Range support.
///
/// Streams a binary fact's content with bounded memory. If a `.blob` sidecar file
/// exists (written at POST time for binary facts), uses true chunked I/O from disk —
/// memory stays at ~64 KiB per active stream regardless of file size.
/// Falls back to loading from JSON for facts stored before the sidecar was introduced.
async fn handle_stream_fact(
    fact_id_hex: String,
    data_dir: Option<PathBuf>,
    semaphore: Arc<tokio::sync::Semaphore>,
    range_header: Option<String>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let _permit = semaphore.acquire().await.map_err(|_| warp::reject())?;

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    if fact_id_hex.len() != 64 || !fact_id_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({"error": "Invalid fact_id"})),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    if let Err(e) = crate::fact_sidecar::ensure_fact_externalized(&data_dir, &fact_id_hex).await {
        tracing::warn!("fact externalize {}: {}", fact_id_hex, e);
    }

    let blob_path = fact_blob_path(&data_dir, &fact_id_hex);
    let json_path = fact_path(&data_dir, &fact_id_hex);

    // Fast path: .blob sidecar exists → chunked streaming (bounded memory)
    if blob_path.exists() {
        let mime_type =
            crate::fact_sidecar::resolve_fact_stream_mime(&data_dir, &fact_id_hex).await;

        let file_size = match tokio::fs::metadata(&blob_path).await {
            Ok(m) => m.len(),
            Err(e) => {
                tracing::error!("stream fact blob metadata {}: {}", fact_id_hex, e);
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({"error": format!("Blob error: {}", e)})),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        };

        let range = range_header
            .as_deref()
            .and_then(|h| crate::streaming::ByteRange::parse(h, file_size));

        let (stream, meta) = match crate::streaming::file_stream(
            &blob_path,
            crate::streaming::StreamingConfig::default(),
            range,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("stream fact file_stream {}: {}", fact_id_hex, e);
                return Ok(boxed_reply(warp::reply::with_status(
                    warp::reply::json(
                        &serde_json::json!({"error": format!("Stream error: {}", e)}),
                    ),
                    warp::http::StatusCode::INTERNAL_SERVER_ERROR,
                )));
            }
        };

        let body = hyper::Body::wrap_stream(stream);

        let status = if range.is_some() {
            warp::http::StatusCode::PARTIAL_CONTENT
        } else {
            warp::http::StatusCode::OK
        };

        let mut builder = warp::http::Response::builder()
            .status(status)
            .header("content-type", &mime_type)
            .header("accept-ranges", "bytes")
            .header("content-length", meta.length.to_string());
        if range.is_some() {
            builder = builder.header("content-range", meta.content_range());
        }
        let resp = builder.body(body).unwrap();
        return Ok(boxed_reply(resp));
    }

    // Fallback: no blob sidecar (legacy facts) — load from JSON (full memory read)
    let raw = match tokio::fs::read(&json_path).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!("stream fact fallback read {}: {}", fact_id_hex, e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": format!("Read error: {}", e)})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };
    let fact: FactPackage = match serde_json::from_slice(&raw) {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("stream fact fallback parse {}: {}", fact_id_hex, e);
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Corrupt fact data"})),
                warp::http::StatusCode::INTERNAL_SERVER_ERROR,
            )));
        }
    };
    let (bytes, mime_type) = match &fact.content {
        FactContent::Binary { data, .. } => {
            let mime = crate::stream_mime::resolve_stream_mime_for_fact(&fact);
            (data.clone(), mime)
        }
        _ => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Fact is not binary content"})),
                warp::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
            )));
        }
    };
    let total = bytes.len();

    if let Some(ref range_str) = range_header {
        if let Some(range) = range_str.strip_prefix("bytes=") {
            let parts: Vec<&str> = range.splitn(2, '-').collect();
            if parts.len() == 2 {
                let start: usize = parts[0].parse().unwrap_or(0);
                let end: usize = if parts[1].is_empty() {
                    total.saturating_sub(1)
                } else {
                    parts[1]
                        .parse()
                        .unwrap_or(total.saturating_sub(1))
                        .min(total.saturating_sub(1))
                };
                if start < total && start <= end {
                    let slice = bytes[start..=end].to_vec();
                    let content_range = format!("bytes {}-{}/{}", start, end, total);
                    let resp = warp::http::Response::builder()
                        .status(warp::http::StatusCode::PARTIAL_CONTENT)
                        .header("content-type", &mime_type)
                        .header("content-range", content_range)
                        .header("accept-ranges", "bytes")
                        .header("content-length", slice.len().to_string())
                        .body(slice)
                        .unwrap();
                    return Ok(boxed_reply(resp));
                }
            }
        }
    }

    let resp = warp::http::Response::builder()
        .status(warp::http::StatusCode::OK)
        .header("content-type", &mime_type)
        .header("accept-ranges", "bytes")
        .header("content-length", total.to_string())
        .body(bytes)
        .unwrap();
    Ok(boxed_reply(resp))
}

/// POST /facts/batch — retrieve multiple facts by ID list
/// Body: { "fact_ids": ["abc...", "def..."] }
async fn handle_batch_facts(
    body: serde_json::Value,
    data_dir: Option<PathBuf>,
    semaphore: Arc<tokio::sync::Semaphore>,
) -> Result<Box<dyn warp::Reply>, warp::Rejection> {
    let _permit = semaphore.acquire().await.map_err(|_| warp::reject())?;

    let data_dir = match data_dir {
        Some(d) => d,
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(&serde_json::json!({"error": "Storage not configured"})),
                warp::http::StatusCode::SERVICE_UNAVAILABLE,
            )))
        }
    };

    let fact_ids = match body.get("fact_ids").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect::<Vec<_>>(),
        None => {
            return Ok(boxed_reply(warp::reply::with_status(
                warp::reply::json(
                    &serde_json::json!({"error": "Expected { \"fact_ids\": [...] }"}),
                ),
                warp::http::StatusCode::BAD_REQUEST,
            )))
        }
    };

    const MAX_BATCH: usize = 100;
    if fact_ids.len() > MAX_BATCH {
        return Ok(boxed_reply(warp::reply::with_status(
            warp::reply::json(
                &serde_json::json!({"error": format!("Max {} facts per batch", MAX_BATCH)}),
            ),
            warp::http::StatusCode::BAD_REQUEST,
        )));
    }

    let mut facts: Vec<serde_json::Value> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    for fid in &fact_ids {
        if fid.len() != 64 || !fid.chars().all(|c| c.is_ascii_hexdigit()) {
            missing.push(fid.clone());
            continue;
        }
        let path = fact_path(&data_dir, fid);
        let _ = crate::fact_sidecar::ensure_fact_externalized(&data_dir, fid).await;
        match crate::fact_sidecar::read_fact_json(&data_dir, fid).await {
            Ok(fact) => {
                if let Ok(val) = serde_json::to_value(&fact) {
                    facts.push(val);
                } else {
                    missing.push(fid.clone());
                }
            }
            Err(_) => missing.push(fid.clone()),
        }
    }

    Ok(boxed_reply(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({
            "facts": facts,
            "missing": missing,
        })),
        warp::http::StatusCode::OK,
    )))
}
