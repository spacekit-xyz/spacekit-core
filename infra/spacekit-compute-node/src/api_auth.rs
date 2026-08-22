//! Signed-request authentication for the node HTTP API.
//!
//! Every mutating endpoint requires a request signed by the caller's DID key.
//! The design is fail-closed: if the authenticator cannot resolve a key, cannot
//! verify a signature, or is misconfigured, the request is rejected. There is
//! no "auth disabled" mode that can be reached by omitting configuration.
//!
//! ## Enrollment
//!
//! `POST /v1/did/register` is the proof-of-possession step: the caller proves
//! control of a SPHINCS+ key by self-signing `(sphincs_pk || kyber_pk || network)`.
//! That registration populates [`DidKeyRegistry`], which is what this module
//! resolves against afterwards.
//!
//! ## Canonical signing payload
//!
//! ```text
//! SPACEKIT-API-v1\n
//! {METHOD}\n
//! {PATH}\n
//! {NETWORK}\n
//! {TIMESTAMP}\n
//! {NONCE}\n
//! {hex(sha256(body))}
//! ```
//!
//! The leading domain-separator line prevents a signature produced for some
//! other SpaceKit protocol message from being replayed as an API call, and the
//! network line prevents a testnet-signed request from being replayed on
//! mainnet. Timestamp plus nonce, tracked in a replay cache, prevents reuse.
//!
//! Required headers: `X-SpaceKit-DID`, `X-SpaceKit-Timestamp`,
//! `X-SpaceKit-Nonce`, `X-SpaceKit-Signature` (hex).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use warp::{Filter, Rejection};

pub const HEADER_DID: &str = "x-spacekit-did";
pub const HEADER_TIMESTAMP: &str = "x-spacekit-timestamp";
pub const HEADER_NONCE: &str = "x-spacekit-nonce";
pub const HEADER_SIGNATURE: &str = "x-spacekit-signature";

const DOMAIN: &str = "SPACEKIT-API-v1";

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// A DID enrolled via `/v1/did/register`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredKey {
    pub did: String,
    pub sphincs_pk_hex: String,
    pub kyber_pk_hex: String,
    pub network: String,
    pub registered_at: u64,
}

/// Persistent DID -> public key map.
///
/// Only public material is stored, so the file is integrity-sensitive but not
/// confidentiality-sensitive; it must be write-protected because an attacker
/// who can rewrite it can substitute their own key for a DID.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DidKeyStore {
    pub keys: HashMap<String, RegisteredKey>,
}

#[derive(Clone)]
pub struct DidKeyRegistry {
    store: Arc<RwLock<DidKeyStore>>,
    path: Option<PathBuf>,
}

impl DidKeyRegistry {
    pub fn new(path: Option<PathBuf>) -> Self {
        let store = path
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<DidKeyStore>(&b).ok())
            .unwrap_or_default();
        Self {
            store: Arc::new(RwLock::new(store)),
            path,
        }
    }

    pub async fn len(&self) -> usize {
        self.store.read().await.keys.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Enroll a DID. Re-registering an existing DID with a *different* key is
    /// refused: rotation must go through an authenticated rotation flow, or an
    /// attacker could overwrite a victim's key by replaying registration.
    pub async fn register(&self, key: RegisteredKey) -> Result<(), String> {
        let mut store = self.store.write().await;
        if let Some(existing) = store.keys.get(&key.did) {
            if existing.sphincs_pk_hex != key.sphincs_pk_hex {
                return Err(format!(
                    "DID {} is already registered with a different key",
                    key.did
                ));
            }
            return Ok(());
        }
        store.keys.insert(key.did.clone(), key);
        if let Some(path) = &self.path {
            if let Ok(bytes) = serde_json::to_vec_pretty(&*store) {
                if let Err(e) = std::fs::write(path, bytes) {
                    tracing::warn!("failed to persist DID key registry: {e}");
                }
            }
        }
        Ok(())
    }

    pub async fn resolve(&self, did: &str) -> Option<RegisteredKey> {
        self.store.read().await.keys.get(did).cloned()
    }
}

/// Authentication configuration.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Network label bound into the signing payload.
    pub network: String,
    /// DIDs permitted to call operator-only routes.
    pub admin_dids: Vec<String>,
    /// Accepted clock skew in seconds, in both directions.
    pub max_skew_secs: u64,
    /// How long a nonce is remembered for replay rejection. Must exceed
    /// `max_skew_secs * 2` or a request could be replayed after its nonce ages
    /// out but while its timestamp is still valid.
    pub replay_ttl_secs: u64,
}

impl AuthConfig {
    pub fn from_env(network: &str) -> Self {
        let admin_dids = std::env::var("SPACEKIT_ADMIN_DIDS")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let max_skew_secs = std::env::var("SPACEKIT_API_MAX_SKEW_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120);

        Self {
            network: network.to_string(),
            admin_dids,
            max_skew_secs,
            replay_ttl_secs: max_skew_secs * 4,
        }
    }
}

/// The verified identity behind a request.
#[derive(Debug, Clone)]
pub struct AuthenticatedCaller {
    pub did: String,
    pub is_admin: bool,
}

#[derive(Debug)]
pub enum AuthError {
    MissingHeader(&'static str),
    MalformedHeader(&'static str),
    UnknownDid(String),
    ClockSkew { skew_secs: i64, limit_secs: u64 },
    Replay,
    BadSignature,
    NotAdmin(String),
    BadBody(String),
}

impl warp::reject::Reject for AuthError {}

impl AuthError {
    pub fn status(&self) -> warp::http::StatusCode {
        match self {
            AuthError::NotAdmin(_) => warp::http::StatusCode::FORBIDDEN,
            AuthError::BadBody(_) => warp::http::StatusCode::BAD_REQUEST,
            _ => warp::http::StatusCode::UNAUTHORIZED,
        }
    }

    pub fn message(&self) -> String {
        match self {
            AuthError::MissingHeader(h) => format!("missing required header {h}"),
            AuthError::MalformedHeader(h) => format!("malformed header {h}"),
            AuthError::UnknownDid(d) => {
                format!("DID {d} is not registered on this node; call /v1/did/register first")
            }
            AuthError::ClockSkew {
                skew_secs,
                limit_secs,
            } => format!("timestamp skew {skew_secs}s exceeds limit {limit_secs}s"),
            AuthError::Replay => "nonce has already been used".into(),
            AuthError::BadSignature => "signature verification failed".into(),
            AuthError::NotAdmin(d) => format!("DID {d} is not authorized for operator routes"),
            AuthError::BadBody(e) => format!("invalid request body: {e}"),
        }
    }
}

/// Builds the exact bytes a client must sign.
pub fn canonical_payload(
    method: &str,
    path: &str,
    network: &str,
    timestamp: u64,
    nonce: &str,
    body: &[u8],
) -> Vec<u8> {
    let body_hash = hex::encode(Sha256::digest(body));
    format!("{DOMAIN}\n{method}\n{path}\n{network}\n{timestamp}\n{nonce}\n{body_hash}").into_bytes()
}

/// Verifies signed requests and tracks nonces for replay rejection.
#[derive(Clone)]
pub struct RequestAuthenticator {
    config: AuthConfig,
    registry: DidKeyRegistry,
    seen_nonces: Arc<RwLock<HashMap<String, u64>>>,
}

impl RequestAuthenticator {
    pub fn new(config: AuthConfig, registry: DidKeyRegistry) -> Self {
        Self {
            config,
            registry,
            seen_nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn registry(&self) -> &DidKeyRegistry {
        &self.registry
    }

    pub fn config(&self) -> &AuthConfig {
        &self.config
    }

    async fn check_and_record_nonce(&self, did: &str, nonce: &str) -> Result<(), AuthError> {
        let key = format!("{did}:{nonce}");
        let now = now_secs();
        let mut seen = self.seen_nonces.write().await;
        seen.retain(|_, expires| *expires > now);
        if seen.contains_key(&key) {
            return Err(AuthError::Replay);
        }
        seen.insert(key, now + self.config.replay_ttl_secs);
        Ok(())
    }

    /// Verify a request. `path` must be the full path as routed.
    pub async fn verify(
        &self,
        method: &str,
        path: &str,
        headers: &warp::http::HeaderMap,
        body: &[u8],
    ) -> Result<AuthenticatedCaller, AuthError> {
        let get = |name: &'static str| -> Result<String, AuthError> {
            headers
                .get(name)
                .ok_or(AuthError::MissingHeader(name))?
                .to_str()
                .map(|s| s.to_string())
                .map_err(|_| AuthError::MalformedHeader(name))
        };

        let did = get(HEADER_DID)?;
        let nonce = get(HEADER_NONCE)?;
        let signature_hex = get(HEADER_SIGNATURE)?;
        let timestamp: u64 = get(HEADER_TIMESTAMP)?
            .parse()
            .map_err(|_| AuthError::MalformedHeader(HEADER_TIMESTAMP))?;

        // Reject a stale or future-dated request before doing expensive
        // signature work, so an unauthenticated caller cannot force SPHINCS+
        // verifications as a CPU-exhaustion vector.
        let skew = now_secs() as i64 - timestamp as i64;
        if skew.unsigned_abs() > self.config.max_skew_secs {
            return Err(AuthError::ClockSkew {
                skew_secs: skew,
                limit_secs: self.config.max_skew_secs,
            });
        }

        if nonce.len() < 16 || nonce.len() > 128 {
            return Err(AuthError::MalformedHeader(HEADER_NONCE));
        }

        let registered = self
            .registry
            .resolve(&did)
            .await
            .ok_or_else(|| AuthError::UnknownDid(did.clone()))?;

        let pk = hex::decode(&registered.sphincs_pk_hex).map_err(|_| AuthError::BadSignature)?;
        let signature = hex::decode(&signature_hex).map_err(|_| AuthError::BadSignature)?;
        let payload =
            canonical_payload(method, path, &self.config.network, timestamp, &nonce, body);

        use spacekit_did::sphincs::SphincsPlus;
        if !SphincsPlus::verify(&pk, &payload, &signature) {
            return Err(AuthError::BadSignature);
        }

        // Only record the nonce once the signature is valid, so an attacker
        // cannot burn a legitimate client's nonces with forged requests.
        self.check_and_record_nonce(&did, &nonce).await?;

        let is_admin = self.config.admin_dids.iter().any(|a| a == &did);
        Ok(AuthenticatedCaller { did, is_admin })
    }
}

/// Filter for authenticated requests with a JSON body.
///
/// Yields `(AuthenticatedCaller, T)`. The body is read as raw bytes so the
/// signature can cover the exact bytes received rather than a re-serialization.
pub fn signed_json<T: DeserializeOwned + Send + 'static>(
    auth: Arc<RequestAuthenticator>,
) -> impl Filter<Extract = ((AuthenticatedCaller, T),), Error = Rejection> + Clone {
    warp::method()
        .and(warp::path::full())
        .and(warp::header::headers_cloned())
        .and(warp::body::content_length_limit(1024 * 1024))
        .and(warp::body::bytes())
        .and_then(
            move |method: warp::http::Method,
                  path: warp::path::FullPath,
                  headers: warp::http::HeaderMap,
                  body: bytes::Bytes| {
                let auth = auth.clone();
                async move {
                    let caller = auth
                        .verify(method.as_str(), path.as_str(), &headers, &body)
                        .await
                        .map_err(warp::reject::custom)?;
                    let parsed: T = serde_json::from_slice(&body)
                        .map_err(|e| warp::reject::custom(AuthError::BadBody(e.to_string())))?;
                    Ok::<_, Rejection>((caller, parsed))
                }
            },
        )
}

/// Filter for authenticated requests without a body (GET/DELETE).
pub fn signed_request(
    auth: Arc<RequestAuthenticator>,
) -> impl Filter<Extract = (AuthenticatedCaller,), Error = Rejection> + Clone {
    warp::method()
        .and(warp::path::full())
        .and(warp::header::headers_cloned())
        .and_then(
            move |method: warp::http::Method,
                  path: warp::path::FullPath,
                  headers: warp::http::HeaderMap| {
                let auth = auth.clone();
                async move {
                    auth.verify(method.as_str(), path.as_str(), &headers, &[])
                        .await
                        .map_err(warp::reject::custom)
                }
            },
        )
}

/// Reject a non-admin caller on operator-only routes.
pub fn require_admin(caller: &AuthenticatedCaller) -> Result<(), Rejection> {
    if caller.is_admin {
        Ok(())
    } else {
        Err(warp::reject::custom(AuthError::NotAdmin(
            caller.did.clone(),
        )))
    }
}

/// Turn auth rejections into JSON responses instead of warp's default 500.
pub async fn handle_rejection(
    err: Rejection,
) -> Result<impl warp::Reply, std::convert::Infallible> {
    if let Some(auth_err) = err.find::<AuthError>() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({
                "error": auth_err.message(),
                "authenticated": false,
            })),
            auth_err.status(),
        ));
    }
    if err.is_not_found() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "not found" })),
            warp::http::StatusCode::NOT_FOUND,
        ));
    }
    if err.find::<warp::reject::PayloadTooLarge>().is_some() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&serde_json::json!({ "error": "request body too large" })),
            warp::http::StatusCode::PAYLOAD_TOO_LARGE,
        ));
    }
    tracing::warn!("unhandled rejection: {err:?}");
    Ok(warp::reply::with_status(
        warp::reply::json(&serde_json::json!({ "error": "bad request" })),
        warp::http::StatusCode::BAD_REQUEST,
    ))
}

/// Parse `SPACEKIT_API_ALLOWED_ORIGINS` (comma-separated) into a policy.
///
/// `*` means "any origin" and must be an explicit operator decision. Anything
/// else, including an empty value, yields an explicit allow-list.
pub fn allowed_origins(raw: &str) -> Option<Vec<String>> {
    let trimmed = raw.trim();
    if trimmed == "*" {
        return None;
    }
    Some(
        trimmed
            .split(',')
            .map(str::trim)
            .filter(|o| !o.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

/// CORS policy from `SPACEKIT_API_ALLOWED_ORIGINS` (comma-separated).
///
/// Defaults to no cross-origin access.
///
/// Note the empty-list case: warp treats a builder with *no* `allow_origin`
/// call as "any origin", so an unset variable would silently open the node to
/// every website. Passing an explicitly empty origin set is what makes the
/// default deny rather than allow.
pub fn cors_layer() -> warp::cors::Builder {
    let raw = std::env::var("SPACEKIT_API_ALLOWED_ORIGINS").unwrap_or_default();
    let cors = warp::cors()
        .allow_methods(vec!["GET", "POST", "OPTIONS"])
        .allow_headers(vec![
            "content-type",
            HEADER_DID,
            HEADER_TIMESTAMP,
            HEADER_NONCE,
            HEADER_SIGNATURE,
        ]);

    match allowed_origins(&raw) {
        None => {
            tracing::warn!(
                "SPACEKIT_API_ALLOWED_ORIGINS=* — any website can issue cross-origin requests to this node"
            );
            cors.allow_any_origin()
        }
        Some(origins) => {
            if origins.is_empty() {
                tracing::info!(
                    "SPACEKIT_API_ALLOWED_ORIGINS is unset — refusing all cross-origin requests"
                );
            }
            cors.allow_origins(origins.iter().map(String::as_str))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_with(reg: DidKeyRegistry) -> RequestAuthenticator {
        RequestAuthenticator::new(
            AuthConfig {
                network: "testnet".into(),
                admin_dids: vec!["did:spacekit:admin".into()],
                max_skew_secs: 120,
                replay_ttl_secs: 480,
            },
            reg,
        )
    }

    fn headers(did: &str, ts: u64, nonce: &str, sig: &str) -> warp::http::HeaderMap {
        let mut h = warp::http::HeaderMap::new();
        h.insert(HEADER_DID, did.parse().unwrap());
        h.insert(HEADER_TIMESTAMP, ts.to_string().parse().unwrap());
        h.insert(HEADER_NONCE, nonce.parse().unwrap());
        h.insert(HEADER_SIGNATURE, sig.parse().unwrap());
        h
    }

    #[test]
    fn canonical_payload_is_domain_separated() {
        let p = canonical_payload(
            "POST",
            "/v1/execute",
            "mainnet",
            100,
            "abcdef0123456789",
            b"{}",
        );
        let s = String::from_utf8(p).unwrap();
        assert!(s.starts_with("SPACEKIT-API-v1\n"));
        assert!(s.contains("\nPOST\n/v1/execute\nmainnet\n100\nabcdef0123456789\n"));
    }

    #[test]
    fn canonical_payload_binds_body() {
        let a = canonical_payload("POST", "/p", "n", 1, "0123456789abcdef", b"{\"a\":1}");
        let b = canonical_payload("POST", "/p", "n", 1, "0123456789abcdef", b"{\"a\":2}");
        assert_ne!(a, b);
    }

    #[test]
    fn canonical_payload_binds_network_and_path() {
        let base = canonical_payload("POST", "/p", "mainnet", 1, "0123456789abcdef", b"");
        assert_ne!(
            base,
            canonical_payload("POST", "/p", "testnet", 1, "0123456789abcdef", b"")
        );
        assert_ne!(
            base,
            canonical_payload("POST", "/q", "mainnet", 1, "0123456789abcdef", b"")
        );
        assert_ne!(
            base,
            canonical_payload("GET", "/p", "mainnet", 1, "0123456789abcdef", b"")
        );
    }

    #[tokio::test]
    async fn unregistered_did_is_rejected() {
        let auth = auth_with(DidKeyRegistry::new(None));
        let h = headers("did:spacekit:nobody", now_secs(), "0123456789abcdef", "00");
        let err = auth
            .verify("POST", "/v1/execute", &h, b"{}")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::UnknownDid(_)));
    }

    #[tokio::test]
    async fn missing_headers_are_rejected() {
        let auth = auth_with(DidKeyRegistry::new(None));
        let err = auth
            .verify("POST", "/v1/execute", &warp::http::HeaderMap::new(), b"{}")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::MissingHeader(_)));
    }

    #[tokio::test]
    async fn stale_timestamp_is_rejected_before_key_lookup() {
        let auth = auth_with(DidKeyRegistry::new(None));
        let h = headers("did:spacekit:nobody", 1, "0123456789abcdef", "00");
        let err = auth
            .verify("POST", "/v1/execute", &h, b"{}")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::ClockSkew { .. }));
    }

    #[tokio::test]
    async fn short_nonce_is_rejected() {
        let auth = auth_with(DidKeyRegistry::new(None));
        let h = headers("did:spacekit:nobody", now_secs(), "short", "00");
        let err = auth
            .verify("POST", "/v1/execute", &h, b"{}")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::MalformedHeader(_)));
    }

    #[tokio::test]
    async fn nonce_replay_is_rejected() {
        let auth = auth_with(DidKeyRegistry::new(None));
        auth.check_and_record_nonce("did:a", "0123456789abcdef")
            .await
            .unwrap();
        assert!(matches!(
            auth.check_and_record_nonce("did:a", "0123456789abcdef")
                .await,
            Err(AuthError::Replay)
        ));
        // Same nonce from a different DID is independent.
        assert!(auth
            .check_and_record_nonce("did:b", "0123456789abcdef")
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn registry_refuses_key_substitution() {
        let reg = DidKeyRegistry::new(None);
        reg.register(RegisteredKey {
            did: "did:x".into(),
            sphincs_pk_hex: "aa".into(),
            kyber_pk_hex: String::new(),
            network: "testnet".into(),
            registered_at: 0,
        })
        .await
        .unwrap();

        // Idempotent re-registration is fine.
        assert!(reg
            .register(RegisteredKey {
                did: "did:x".into(),
                sphincs_pk_hex: "aa".into(),
                kyber_pk_hex: String::new(),
                network: "testnet".into(),
                registered_at: 0,
            })
            .await
            .is_ok());

        // Overwriting with a different key is not.
        assert!(reg
            .register(RegisteredKey {
                did: "did:x".into(),
                sphincs_pk_hex: "bb".into(),
                kyber_pk_hex: String::new(),
                network: "testnet".into(),
                registered_at: 0,
            })
            .await
            .is_err());
    }

    /// An unset or empty variable must produce an empty allow-list, not the
    /// "no restriction" case. Warp reads a missing origin set as "allow any",
    /// so this distinction is what keeps the default closed.
    #[test]
    fn empty_origin_config_allows_nothing() {
        for raw in ["", "   ", ",", " , , "] {
            let parsed = allowed_origins(raw);
            assert_eq!(
                parsed,
                Some(Vec::new()),
                "{raw:?} must yield an empty allow-list, not any-origin"
            );
        }
    }

    #[test]
    fn wildcard_origin_is_the_only_any_origin_case() {
        assert_eq!(allowed_origins("*"), None);
        assert_eq!(allowed_origins(" * "), None);
        // A wildcard mixed into a list is treated as a literal entry, not as
        // permission to open the node up.
        assert_eq!(
            allowed_origins("https://a.example,*"),
            Some(vec!["https://a.example".to_string(), "*".to_string()])
        );
    }

    #[test]
    fn origin_list_is_parsed_and_trimmed() {
        assert_eq!(
            allowed_origins(" https://a.example , https://b.example "),
            Some(vec![
                "https://a.example".to_string(),
                "https://b.example".to_string()
            ])
        );
    }

    #[test]
    fn admin_gate_rejects_non_admin() {
        let caller = AuthenticatedCaller {
            did: "did:spacekit:user".into(),
            is_admin: false,
        };
        assert!(require_admin(&caller).is_err());

        let admin = AuthenticatedCaller {
            did: "did:spacekit:admin".into(),
            is_admin: true,
        };
        assert!(require_admin(&admin).is_ok());
    }
}
