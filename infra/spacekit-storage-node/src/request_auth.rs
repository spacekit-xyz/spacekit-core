//! Request authentication for the storage node.
//!
//! Before this module, every `Authorization: DID <did>` (or `Bearer <did>`)
//! header was taken at its word: whoever sent it *was* that DID. This module
//! replaces that with three verifiable credentials, plus a legacy switch:
//!
//! 1. **Session tokens** (`sktok1.…`): HMAC-signed by this node. A client gets
//!    one by signing a node-issued challenge with the key behind its DID
//!    (`POST /api/auth/challenge`, then `POST /api/auth/session`).
//! 2. **App-scoped tokens**: session tokens narrowed to one app's document
//!    collections (`POST /api/auth/delegate`). Hosts hand these to embedded
//!    apps' bridges instead of the viewer's full session.
//! 3. **Service assertions**: `Authorization: DID <did>` together with a valid
//!    `X-Storage-Secret`. Only trusted backends (website-api) hold the secret.
//! 4. **Legacy bare DIDs**: accepted only in `SPACEKIT_DID_AUTH=legacy` mode, and
//!    never for protected DIDs (`did:spacekit:admin:*` and
//!    `SPACEKIT_PROTECTED_DIDS`), which always need a token or the secret.
//!
//! Configuration (environment):
//! - `SPACEKIT_DID_AUTH` = `legacy` (default, for migration) | `strict`
//! - `SPACEKIT_STORAGE_SECRET` or `STORAGE_NODE_SECRET`: service secret
//! - `SPACEKIT_AUTH_TOKEN_SECRET`: token signing key (else `{data_dir}/.auth_token_secret`,
//!   created on first start)
//! - `SPACEKIT_PROTECTED_DIDS`: extra comma-separated DIDs that never accept bare claims

#![deny(clippy::all)]

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

const TOKEN_PREFIX: &str = "sktok1.";
const CHALLENGE_PREFIX: &str = "skch1.";
const TOKEN_KEY_CONTEXT: &str = "spacekit-auth-token-v1";
const CHALLENGE_KEY_CONTEXT: &str = "spacekit-auth-challenge-v1";
const TOKEN_SECRET_FILE: &str = ".auth_token_secret";

/// Longest session a login may mint.
pub const MAX_SESSION_TTL_SECONDS: u64 = 24 * 3600;
/// Default session length.
pub const DEFAULT_SESSION_TTL_SECONDS: u64 = 12 * 3600;
/// Longest app-scoped token a delegation may mint.
pub const MAX_APP_TOKEN_TTL_SECONDS: u64 = 3600;
/// How long a login challenge stays valid.
pub const CHALLENGE_TTL_SECONDS: u64 = 120;

/// Prefixes of DIDs that never authenticate by bare claim.
const DEFAULT_PROTECTED_PREFIXES: &[&str] = &["did:spacekit:admin:"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DidAuthMode {
    /// Bare `DID <did>` headers are still accepted for non-protected DIDs.
    Legacy,
    /// Only tokens and service assertions authenticate.
    Strict,
}

impl DidAuthMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "legacy" | "permissive" => Some(Self::Legacy),
            "strict" => Some(Self::Strict),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Strict => "strict",
        }
    }
}

pub struct AuthConfig {
    pub mode: DidAuthMode,
    service_secret: Option<Vec<u8>>,
    token_key: [u8; 32],
    challenge_key: [u8; 32],
    protected_prefixes: Vec<String>,
    protected_dids: HashSet<String>,
    used_challenges: Mutex<HashMap<String, u64>>,
}

impl std::fmt::Debug for AuthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthConfig")
            .field("mode", &self.mode)
            .field("service_secret", &self.service_secret.is_some())
            .field("protected_prefixes", &self.protected_prefixes)
            .field("protected_dids", &self.protected_dids)
            .finish()
    }
}

static CONFIG: OnceLock<AuthConfig> = OnceLock::new();

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn random_secret() -> [u8; 32] {
    // uuid v4 draws from the OS CSPRNG; two of them give 244 random bits.
    let mut hasher = blake3::Hasher::new();
    hasher.update(uuid::Uuid::new_v4().as_bytes());
    hasher.update(uuid::Uuid::new_v4().as_bytes());
    *hasher.finalize().as_bytes()
}

fn load_or_create_token_secret(data_dir: Option<&Path>) -> Vec<u8> {
    if let Some(s) = env_nonempty("SPACEKIT_AUTH_TOKEN_SECRET") {
        return crate::upload_token::normalize_secret_bytes(&s);
    }
    if let Some(dir) = data_dir {
        let path = dir.join(TOKEN_SECRET_FILE);
        if let Ok(raw) = std::fs::read_to_string(&path) {
            let t = raw.trim();
            if !t.is_empty() {
                return crate::upload_token::normalize_secret_bytes(t);
            }
        }
        let fresh = hex::encode(random_secret());
        if std::fs::create_dir_all(dir).is_ok() && std::fs::write(&path, &fresh).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            return crate::upload_token::normalize_secret_bytes(&fresh);
        }
        tracing::warn!("could not persist {}; auth tokens will not survive a restart", path.display());
    }
    random_secret().to_vec()
}

impl AuthConfig {
    pub fn from_env(data_dir: Option<&Path>) -> Self {
        let mode = env_nonempty("SPACEKIT_DID_AUTH")
            .and_then(|v| DidAuthMode::parse(&v))
            .unwrap_or(DidAuthMode::Legacy);
        let service_secret = env_nonempty("SPACEKIT_STORAGE_SECRET")
            .or_else(|| env_nonempty("STORAGE_NODE_SECRET"))
            .map(|s| s.into_bytes());
        let protected_dids = env_nonempty("SPACEKIT_PROTECTED_DIDS")
            .map(|v| {
                v.split(',')
                    .map(|d| d.trim().to_string())
                    .filter(|d| !d.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        Self::new(
            mode,
            service_secret,
            &load_or_create_token_secret(data_dir),
            protected_dids,
        )
    }

    pub fn new(
        mode: DidAuthMode,
        service_secret: Option<Vec<u8>>,
        token_secret: &[u8],
        protected_dids: HashSet<String>,
    ) -> Self {
        Self {
            mode,
            service_secret,
            token_key: blake3::derive_key(TOKEN_KEY_CONTEXT, token_secret),
            challenge_key: blake3::derive_key(CHALLENGE_KEY_CONTEXT, token_secret),
            protected_prefixes: DEFAULT_PROTECTED_PREFIXES.iter().map(|s| s.to_string()).collect(),
            protected_dids,
            used_challenges: Mutex::new(HashMap::new()),
        }
    }

    pub fn has_service_secret(&self) -> bool {
        self.service_secret.is_some()
    }

    pub fn is_protected(&self, did: &str) -> bool {
        self.protected_dids.contains(did) || self.protected_prefixes.iter().any(|p| did.starts_with(p))
    }

    /// Constant-time check of an `X-Storage-Secret` header.
    pub fn service_secret_matches(&self, presented: Option<&str>) -> bool {
        match (&self.service_secret, presented) {
            (Some(expected), Some(given)) => constant_time_eq(expected, given.trim().as_bytes()),
            _ => false,
        }
    }
}

/// Install the process-wide config. Call once at server start; later calls are ignored.
pub fn init(data_dir: Option<&Path>) -> &'static AuthConfig {
    let cfg = CONFIG.get_or_init(|| AuthConfig::from_env(data_dir));
    if cfg.mode == DidAuthMode::Legacy {
        tracing::warn!(
            "SPACEKIT_DID_AUTH=legacy: bare `Authorization: DID <did>` headers are accepted for \
             non-protected DIDs. Anyone can claim such a DID. Migrate clients to session tokens \
             and set SPACEKIT_DID_AUTH=strict."
        );
    }
    if !cfg.has_service_secret() {
        tracing::warn!(
            "No SPACEKIT_STORAGE_SECRET / STORAGE_NODE_SECRET configured: protected DIDs \
             (did:spacekit:admin:*) cannot authenticate by DID header at all."
        );
    }
    cfg
}

pub fn config() -> &'static AuthConfig {
    CONFIG.get_or_init(|| AuthConfig::from_env(None))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn valid_did(did: &str) -> bool {
    did.starts_with("did:") && did.len() > 10 && did.len() <= 512 && !did.chars().any(char::is_whitespace)
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/// Narrowing applied to an app-scoped token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppScope {
    /// Lower-case hex app id.
    pub app: String,
    /// Namespace the app's documents live in (the publisher's DID), when not the viewer's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub act_as: Option<String>,
}

impl AppScope {
    pub fn collection_prefix(&self) -> String {
        format!("app_{}_", self.app)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenClaims {
    pub v: u8,
    pub sub: String,
    pub iat: u64,
    pub exp: u64,
    /// How the subject proved itself: `sig:<alg>` or `service`.
    pub amr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<AppScope>,
}

fn mac_hex(key: &[u8; 32], payload: &[u8]) -> String {
    hex::encode(blake3::keyed_hash(key, payload).as_bytes())
}

fn seal(prefix: &str, key: &[u8; 32], payload: &[u8]) -> String {
    format!("{}{}.{}", prefix, hex::encode(payload), mac_hex(key, payload))
}

fn open(prefix: &str, key: &[u8; 32], token: &str) -> Result<Vec<u8>> {
    let rest = token.strip_prefix(prefix).ok_or_else(|| anyhow!("unknown token prefix"))?;
    let (payload_hex, mac) = rest.split_once('.').ok_or_else(|| anyhow!("malformed token"))?;
    let payload = hex::decode(payload_hex).context("payload hex")?;
    if !constant_time_eq(mac_hex(key, &payload).as_bytes(), mac.as_bytes()) {
        return Err(anyhow!("invalid token signature"));
    }
    Ok(payload)
}

pub fn mint_token(cfg: &AuthConfig, claims: &TokenClaims) -> Result<String> {
    Ok(seal(TOKEN_PREFIX, &cfg.token_key, &serde_json::to_vec(claims)?))
}

pub fn verify_token(cfg: &AuthConfig, token: &str, now: u64) -> Result<TokenClaims> {
    let claims: TokenClaims = serde_json::from_slice(&open(TOKEN_PREFIX, &cfg.token_key, token.trim())?)?;
    if claims.v != 1 {
        return Err(anyhow!("unsupported token version"));
    }
    if claims.exp < now {
        return Err(anyhow!("token expired"));
    }
    if !valid_did(&claims.sub) {
        return Err(anyhow!("invalid token subject"));
    }
    Ok(claims)
}

pub fn is_token(value: &str) -> bool {
    value.trim().starts_with(TOKEN_PREFIX)
}

// ---------------------------------------------------------------------------
// Login challenges and signatures
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChallengeClaims {
    did: String,
    exp: u64,
    n: String,
}

pub fn mint_challenge(cfg: &AuthConfig, did: &str, now: u64) -> Result<(String, u64)> {
    if !valid_did(did) {
        return Err(anyhow!("invalid DID"));
    }
    let exp = now + CHALLENGE_TTL_SECONDS;
    let claims = ChallengeClaims {
        did: did.to_string(),
        exp,
        n: hex::encode(&random_secret()[..16]),
    };
    Ok((seal(CHALLENGE_PREFIX, &cfg.challenge_key, &serde_json::to_vec(&claims)?), exp))
}

/// The exact bytes a client signs to log in.
pub fn login_message(did: &str, challenge: &str) -> String {
    format!("SpaceKit storage login\nDID: {did}\nChallenge: {challenge}")
}

fn base58_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut bytes: Vec<u8> = Vec::new();
    for c in input.bytes() {
        let mut carry = ALPHABET.iter().position(|&a| a == c)? as u32;
        for b in bytes.iter_mut().rev() {
            carry += (*b as u32) * 58;
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
    }
    let leading = input.bytes().take_while(|&c| c == b'1').count();
    let mut out = vec![0u8; leading];
    out.extend(bytes);
    Some(out)
}

/// Whether `public_key` (32-byte Ed25519) is the key behind `did`.
///
/// Accepts the W3C `did:key:z6Mk…` multibase form, and the short form kit.space
/// has always written: `did:key:z6Mk` followed by the first 44 hex characters of
/// the public key.
pub fn ed25519_key_matches_did(did: &str, public_key: &[u8]) -> bool {
    if public_key.len() != 32 {
        return false;
    }
    let Some(suffix) = did.strip_prefix("did:key:z6Mk") else {
        return false;
    };
    let pk_hex = hex::encode(public_key);
    if suffix.len() == 44 && suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
        return constant_time_eq(suffix.to_ascii_lowercase().as_bytes(), &pk_hex.as_bytes()[..44]);
    }
    match base58_decode(&did["did:key:z".len()..]) {
        Some(decoded) => decoded.len() == 34 && decoded[0] == 0xed && decoded[1] == 0x01 && decoded[2..] == *public_key,
        None => false,
    }
}

/// Signature schemes a login may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginAlgorithm {
    Ed25519,
    /// FIPS 205 SLH-DSA-SHA2-128s (32-byte public key, 7856-byte signature).
    SlhDsaSha2_128s,
    /// FIPS 205 SLH-DSA-SHA2-192s (48-byte public key, 16224-byte signature).
    SlhDsaSha2_192s,
}

impl LoginAlgorithm {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ed25519" => Some(Self::Ed25519),
            "slh-dsa-sha2-128s" => Some(Self::SlhDsaSha2_128s),
            "slh-dsa-sha2-192s" => Some(Self::SlhDsaSha2_192s),
            _ => None,
        }
    }

    fn amr(self) -> &'static str {
        match self {
            Self::Ed25519 => "sig:ed25519",
            Self::SlhDsaSha2_128s => "sig:slh-dsa-sha2-128s",
            Self::SlhDsaSha2_192s => "sig:slh-dsa-sha2-192s",
        }
    }
}

/// Whether an SLH-DSA public key is the key behind `did`. kit.space writes
/// quantum identities as `did:key:zQ3s` + the first 44 hex chars of the key.
pub fn slh_dsa_key_matches_did(did: &str, public_key: &[u8]) -> bool {
    let Some(suffix) = did.strip_prefix("did:key:zQ3s") else {
        return false;
    };
    let pk_hex = hex::encode(public_key);
    pk_hex.len() >= 44
        && suffix.len() == 44
        && constant_time_eq(suffix.to_ascii_lowercase().as_bytes(), &pk_hex.as_bytes()[..44])
}

fn verify_slh<P: slh_dsa::ParameterSet>(public_key: &[u8], message: &[u8], signature: &[u8]) -> Result<()> {
    use signature::Verifier;
    let vk = slh_dsa::VerifyingKey::<P>::try_from(public_key).map_err(|_| anyhow!("invalid SLH-DSA public key"))?;
    let sig = slh_dsa::Signature::<P>::try_from(signature).map_err(|_| anyhow!("bad SLH-DSA signature length"))?;
    vk.verify(message, &sig).map_err(|_| anyhow!("signature does not verify"))
}

/// Verify a login signature and that the key is the one behind `did`.
pub fn verify_login_signature(
    algorithm: LoginAlgorithm,
    did: &str,
    public_key: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<()> {
    match algorithm {
        LoginAlgorithm::Ed25519 => {
            if !ed25519_key_matches_did(did, public_key) {
                return Err(anyhow!("public key does not match the DID"));
            }
            let pk: [u8; 32] = public_key.try_into().map_err(|_| anyhow!("bad public key length"))?;
            let sig: [u8; 64] = signature.try_into().map_err(|_| anyhow!("bad signature length"))?;
            ed25519_dalek::VerifyingKey::from_bytes(&pk)
                .map_err(|_| anyhow!("invalid public key"))?
                .verify_strict(message, &ed25519_dalek::Signature::from_bytes(&sig))
                .map_err(|_| anyhow!("signature does not verify"))
        }
        LoginAlgorithm::SlhDsaSha2_128s | LoginAlgorithm::SlhDsaSha2_192s => {
            if !slh_dsa_key_matches_did(did, public_key) {
                return Err(anyhow!("public key does not match the DID"));
            }
            if algorithm == LoginAlgorithm::SlhDsaSha2_128s {
                verify_slh::<slh_dsa::Sha2_128s>(public_key, message, signature)
            } else {
                verify_slh::<slh_dsa::Sha2_192s>(public_key, message, signature)
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SessionLoginRequest {
    pub did: String,
    pub challenge: String,
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    pub public_key_hex: String,
    pub signature_hex: String,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

fn default_algorithm() -> String {
    "ed25519".to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct IssuedToken {
    pub token: String,
    pub did: String,
    pub expires_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<AppScope>,
}

/// Verify a signed login and mint a session token.
pub fn login(cfg: &AuthConfig, req: &SessionLoginRequest, now: u64) -> Result<IssuedToken> {
    let payload = open(CHALLENGE_PREFIX, &cfg.challenge_key, req.challenge.trim())
        .map_err(|_| anyhow!("invalid challenge"))?;
    let ch: ChallengeClaims = serde_json::from_slice(&payload)?;
    if ch.did != req.did {
        return Err(anyhow!("challenge was issued for a different DID"));
    }
    if ch.exp < now {
        return Err(anyhow!("challenge expired"));
    }
    if cfg.is_protected(&req.did) {
        return Err(anyhow!("this DID cannot log in with a signature"));
    }
    let algorithm = LoginAlgorithm::parse(&req.algorithm)
        .ok_or_else(|| anyhow!("unsupported signature algorithm: {}", req.algorithm))?;
    let pk = hex::decode(req.public_key_hex.trim()).context("public_key_hex")?;
    let sig = hex::decode(req.signature_hex.trim()).context("signature_hex")?;
    let message = login_message(&req.did, req.challenge.trim());
    verify_login_signature(algorithm, &req.did, &pk, message.as_bytes(), &sig)?;

    // One login per challenge.
    {
        let mut used = cfg.used_challenges.lock().unwrap_or_else(|p| p.into_inner());
        used.retain(|_, exp| *exp >= now);
        if used.insert(ch.n.clone(), ch.exp).is_some() {
            return Err(anyhow!("challenge already used"));
        }
    }

    let ttl = req
        .ttl_seconds
        .unwrap_or(DEFAULT_SESSION_TTL_SECONDS)
        .clamp(60, MAX_SESSION_TTL_SECONDS);
    issue(cfg, &req.did, algorithm.amr(), None, now, ttl)
}

fn issue(
    cfg: &AuthConfig,
    sub: &str,
    amr: &str,
    scope: Option<AppScope>,
    now: u64,
    ttl: u64,
) -> Result<IssuedToken> {
    let claims = TokenClaims {
        v: 1,
        sub: sub.to_string(),
        iat: now,
        exp: now + ttl,
        amr: amr.to_string(),
        scope: scope.clone(),
    };
    Ok(IssuedToken {
        token: mint_token(cfg, &claims)?,
        did: sub.to_string(),
        expires_at: claims.exp,
        scope,
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceTokenRequest {
    pub did: String,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub act_as: Option<String>,
}

/// Mint a token for a DID a trusted service has authenticated (caller checked the secret).
pub fn service_token(cfg: &AuthConfig, req: &ServiceTokenRequest, now: u64) -> Result<IssuedToken> {
    if !valid_did(&req.did) {
        return Err(anyhow!("invalid DID"));
    }
    let scope = match &req.app_id {
        Some(app) => Some(app_scope(app, req.act_as.as_deref())?),
        None => None,
    };
    let max = if scope.is_some() { MAX_APP_TOKEN_TTL_SECONDS } else { MAX_SESSION_TTL_SECONDS };
    let ttl = req.ttl_seconds.unwrap_or(DEFAULT_SESSION_TTL_SECONDS).clamp(60, max);
    issue(cfg, &req.did, "service", scope, now, ttl)
}

fn app_scope(app_id: &str, act_as: Option<&str>) -> Result<AppScope> {
    let app = app_id.trim().to_ascii_lowercase();
    if app.is_empty() || app.len() > 128 || !app.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(anyhow!("app_id must be hex"));
    }
    let act_as = match act_as.map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) if !valid_did(d) => return Err(anyhow!("invalid act_as DID")),
        other => other.map(str::to_string),
    };
    Ok(AppScope { app, act_as })
}

#[derive(Debug, Clone, Deserialize)]
pub struct DelegateRequest {
    pub app_id: String,
    /// The app publisher's DID, when the app keeps its documents in the publisher's namespace.
    #[serde(default)]
    pub act_as: Option<String>,
    #[serde(default)]
    pub ttl_seconds: Option<u64>,
}

/// Narrow an unscoped session into an app-scoped token.
pub fn delegate(cfg: &AuthConfig, parent: &TokenClaims, req: &DelegateRequest, now: u64) -> Result<IssuedToken> {
    if parent.scope.is_some() {
        return Err(anyhow!("an app-scoped token cannot delegate"));
    }
    let scope = app_scope(&req.app_id, req.act_as.as_deref())?;
    if let Some(target) = &scope.act_as {
        if cfg.is_protected(target) {
            return Err(anyhow!("cannot act in a protected DID's namespace"));
        }
    }
    let ttl = req
        .ttl_seconds
        .unwrap_or(MAX_APP_TOKEN_TTL_SECONDS)
        .clamp(60, MAX_APP_TOKEN_TTL_SECONDS)
        .min(parent.exp.saturating_sub(now).max(60));
    issue(cfg, &parent.sub, &parent.amr, Some(scope), now, ttl)
}

// ---------------------------------------------------------------------------
// Per-request authentication
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthFailure {
    Missing,
    Malformed,
    InvalidToken(String),
    /// A bare DID header where one is not accepted.
    BareDidRejected,
    /// A valid credential used outside its scope.
    OutOfScope(String),
}

impl std::fmt::Display for AuthFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => write!(f, "missing credentials"),
            Self::Malformed => write!(f, "malformed Authorization header"),
            Self::InvalidToken(e) => write!(f, "invalid token: {e}"),
            Self::BareDidRejected => write!(
                f,
                "a bare DID is not a credential here; log in via /api/auth/challenge + /api/auth/session"
            ),
            Self::OutOfScope(e) => write!(f, "token scope does not allow this request: {e}"),
        }
    }
}

enum Presented<'a> {
    Token(&'a str),
    Did(&'a str),
}

fn parse_authorization(header: &str) -> Option<Presented<'_>> {
    let h = header.trim();
    let rest = h
        .strip_prefix("Bearer ")
        .or_else(|| h.strip_prefix("SpaceKit "))
        .or_else(|| h.strip_prefix("DID "))
        .map(str::trim)?;
    if rest.starts_with(TOKEN_PREFIX) {
        Some(Presented::Token(rest))
    } else if valid_did(rest) {
        Some(Presented::Did(rest))
    } else {
        None
    }
}

fn percent_decode(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&segment[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Which document operation a request is, and on which collection.
fn document_op(method: &str, path: &str) -> Option<(&'static str, String)> {
    let segs: Vec<&str> = path.trim_matches('/').split('/').collect();
    match (method, segs.as_slice()) {
        ("GET", ["api", "documents", c, _id]) => Some(("get", percent_decode(c))),
        ("PUT", ["api", "documents", c, _id]) => Some(("put", percent_decode(c))),
        ("DELETE", ["api", "documents", c, _id]) => Some(("delete", percent_decode(c))),
        ("GET", ["api", "documents", c]) => Some(("list", percent_decode(c))),
        ("DELETE", ["api", "documents", c]) => Some(("delete", percent_decode(c))),
        ("POST", ["query", "documents", c]) => Some(("list", percent_decode(c))),
        _ => None,
    }
}

/// App collections whose name continues with `__` after the app prefix
/// (`app_<appId>___subscriptions`, …) hold records only trusted services or the
/// namespace owner may write, such as verified subscriptions.
pub fn is_reserved_app_collection(collection: &str) -> bool {
    let Some(rest) = collection.strip_prefix("app_") else {
        return false;
    };
    match rest.split_once('_') {
        Some((app, tail)) => !app.is_empty() && app.bytes().all(|b| b.is_ascii_hexdigit()) && tail.starts_with("__"),
        None => false,
    }
}

fn is_reserved_write(method: &str, path: &str) -> bool {
    matches!(document_op(method, path), Some(("put" | "delete", c)) if is_reserved_app_collection(&c))
}

/// The DID a scoped token may act as for this request.
fn scoped_did(claims: &TokenClaims, scope: &AppScope, method: &str, path: &str) -> Result<String, AuthFailure> {
    let (op, collection) = document_op(method, path)
        .ok_or_else(|| AuthFailure::OutOfScope("app tokens only reach /api/documents".into()))?;
    let prefix = scope.collection_prefix();
    if !collection.starts_with(&prefix) || collection.len() == prefix.len() {
        return Err(AuthFailure::OutOfScope(format!("collection must start with {prefix}")));
    }
    if (op == "put" || op == "delete") && is_reserved_app_collection(&collection) {
        return Err(AuthFailure::OutOfScope("reserved collections are written by trusted services only".into()));
    }
    match &scope.act_as {
        Some(owner) if owner != &claims.sub => {
            // Viewers may read and write an app's shared documents, but only the
            // owner lists or deletes them.
            if op == "get" || op == "put" {
                Ok(owner.clone())
            } else {
                Err(AuthFailure::OutOfScope(format!("{op} is owner-only")))
            }
        }
        _ => Ok(claims.sub.clone()),
    }
}

/// Authenticate one request. `method` and `path` let app-scoped tokens be checked.
pub fn authenticate(
    cfg: &AuthConfig,
    authorization: Option<&str>,
    storage_secret: Option<&str>,
    method: &str,
    path: &str,
    now: u64,
) -> Result<String, AuthFailure> {
    let header = authorization.ok_or(AuthFailure::Missing)?;
    match parse_authorization(header).ok_or(AuthFailure::Malformed)? {
        Presented::Token(t) => {
            let claims = verify_token(cfg, t, now).map_err(|e| AuthFailure::InvalidToken(e.to_string()))?;
            match &claims.scope {
                Some(scope) => scoped_did(&claims, scope, method, path),
                None => Ok(claims.sub),
            }
        }
        Presented::Did(did) => {
            if cfg.service_secret_matches(storage_secret) {
                return Ok(did.to_string());
            }
            if cfg.mode == DidAuthMode::Legacy && !cfg.is_protected(did) {
                // A bare claim cannot be the namespace owner writing verified records.
                if is_reserved_write(method, path) {
                    return Err(AuthFailure::BareDidRejected);
                }
                return Ok(did.to_string());
            }
            Err(AuthFailure::BareDidRejected)
        }
    }
}

/// Like [`authenticate`], for routes where identity is optional. Scoped tokens
/// and rejected claims yield `None` (anonymous) instead of an error.
pub fn authenticate_optional(cfg: &AuthConfig, authorization: Option<&str>, storage_secret: Option<&str>, now: u64) -> Option<String> {
    let header = authorization?;
    match parse_authorization(header)? {
        Presented::Token(t) => verify_token(cfg, t, now).ok().filter(|c| c.scope.is_none()).map(|c| c.sub),
        Presented::Did(did) => {
            if cfg.service_secret_matches(storage_secret)
                || (cfg.mode == DidAuthMode::Legacy && !cfg.is_protected(did))
            {
                Some(did.to_string())
            } else {
                None
            }
        }
    }
}

/// For handlers that parse `Authorization` themselves (blob, fact and package
/// uploads): when a trusted backend asserts a DID with a valid
/// `X-Storage-Secret`, swap the assertion for a one-minute session token, so the
/// handler's own parsing sees a verifiable credential. Everything else passes
/// through unchanged.
pub fn normalize_service_authorization(
    cfg: &AuthConfig,
    authorization: Option<String>,
    storage_secret: Option<&str>,
    now: u64,
) -> Option<String> {
    let header = authorization?;
    if let Some(Presented::Did(did)) = parse_authorization(&header) {
        if cfg.service_secret_matches(storage_secret) {
            let claims = TokenClaims {
                v: 1,
                sub: did.to_string(),
                iat: now,
                exp: now + 60,
                amr: "service".into(),
                scope: None,
            };
            if let Ok(token) = mint_token(cfg, &claims) {
                return Some(format!("Bearer {token}"));
            }
        }
    }
    Some(header)
}

/// The DID behind an `Authorization` value on routes that parse it themselves:
/// an unscoped session token, or (legacy mode, non-protected DIDs only) a bare DID.
pub fn did_from_authorization(cfg: &AuthConfig, value: &str, now: u64) -> Option<String> {
    authenticate_optional(cfg, Some(value), None, now)
}

/// Convenience wrappers over the process-wide config.
pub fn authenticate_request(
    authorization: Option<&str>,
    storage_secret: Option<&str>,
    method: &str,
    path: &str,
) -> Result<String, AuthFailure> {
    let cfg = config();
    let result = authenticate(cfg, authorization, storage_secret, method, path, now_secs());
    if let (Ok(did), Some(header)) = (&result, authorization) {
        if matches!(parse_authorization(header), Some(Presented::Did(_)))
            && !cfg.service_secret_matches(storage_secret)
        {
            // Migration aid: `RUST_LOG=spacekit_auth_legacy=debug` lists clients
            // that still need to move to tokens before SPACEKIT_DID_AUTH=strict.
            tracing::debug!(target: "spacekit_auth_legacy", did = %did, method, path, "bare DID header accepted");
        }
    }
    result
}

pub fn authenticate_request_optional(authorization: Option<&str>, storage_secret: Option<&str>) -> Option<String> {
    authenticate_optional(config(), authorization, storage_secret, now_secs())
}

pub fn unix_now() -> u64 {
    now_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn cfg(mode: DidAuthMode) -> AuthConfig {
        AuthConfig::new(mode, Some(b"svc-secret".to_vec()), b"token-secret", HashSet::new())
    }

    fn kitspace_did(sk: &SigningKey) -> String {
        format!("did:key:z6Mk{}", &hex::encode(sk.verifying_key().as_bytes())[..44])
    }

    fn signed_login(c: &AuthConfig, sk: &SigningKey, did: &str, now: u64) -> SessionLoginRequest {
        let (challenge, _) = mint_challenge(c, did, now).unwrap();
        let sig = sk.sign(login_message(did, &challenge).as_bytes());
        SessionLoginRequest {
            did: did.to_string(),
            challenge,
            algorithm: "ed25519".into(),
            public_key_hex: hex::encode(sk.verifying_key().as_bytes()),
            signature_hex: hex::encode(sig.to_bytes()),
            ttl_seconds: None,
        }
    }

    #[test]
    fn signed_login_mints_a_session() {
        let c = cfg(DidAuthMode::Strict);
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let did = kitspace_did(&sk);
        let issued = login(&c, &signed_login(&c, &sk, &did, 1000), 1000).unwrap();
        let auth = format!("Bearer {}", issued.token);
        assert_eq!(authenticate(&c, Some(&auth), None, "GET", "/api/documents/notes/1", 1001).unwrap(), did);
        // Expired
        assert!(matches!(
            authenticate(&c, Some(&auth), None, "GET", "/x", 1000 + DEFAULT_SESSION_TTL_SECONDS + 1),
            Err(AuthFailure::InvalidToken(_))
        ));
    }

    #[test]
    fn login_rejects_wrong_key_reuse_and_tampering() {
        let c = cfg(DidAuthMode::Strict);
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let other = SigningKey::from_bytes(&[8u8; 32]);
        let did = kitspace_did(&sk);

        // Someone else's key cannot log in as this DID.
        let mut req = signed_login(&c, &sk, &did, 1000);
        req.public_key_hex = hex::encode(other.verifying_key().as_bytes());
        assert!(login(&c, &req, 1000).is_err());

        // A signature by another key over the right message fails.
        let mut req = signed_login(&c, &sk, &did, 1000);
        req.signature_hex = hex::encode(other.sign(login_message(&did, &req.challenge).as_bytes()).to_bytes());
        assert!(login(&c, &req, 1000).is_err());

        // A challenge works once.
        let req = signed_login(&c, &sk, &did, 1000);
        assert!(login(&c, &req, 1000).is_ok());
        assert!(login(&c, &req, 1001).is_err());

        // Expired challenge.
        let req = signed_login(&c, &sk, &did, 1000);
        assert!(login(&c, &req, 1000 + CHALLENGE_TTL_SECONDS + 1).is_err());

        // Challenge for another DID.
        let other_did = kitspace_did(&other);
        let mut req = signed_login(&c, &sk, &did, 1000);
        req.did = other_did;
        assert!(login(&c, &req, 1000).is_err());

        // Forged token MAC.
        let issued = login(&c, &signed_login(&c, &sk, &did, 2000), 2000).unwrap();
        let mut forged = issued.token.clone();
        let last = forged.pop().unwrap();
        forged.push(if last == '0' { '1' } else { '0' });
        let auth = format!("Bearer {forged}");
        assert!(authenticate(&c, Some(&auth), None, "GET", "/x", 2001).is_err());
        // Changing the claims (e.g. the subject) breaks the MAC too.
        let (payload_hex, mac) = issued.token.strip_prefix(TOKEN_PREFIX).unwrap().split_once('.').unwrap();
        let mut claims: TokenClaims = serde_json::from_slice(&hex::decode(payload_hex).unwrap()).unwrap();
        claims.sub = "did:spacekit:admin:website-api".into();
        let tampered = format!("{TOKEN_PREFIX}{}.{mac}", hex::encode(serde_json::to_vec(&claims).unwrap()));
        assert!(verify_token(&c, &tampered, 2001).is_err());
        // Token from another node's secret.
        let c2 = AuthConfig::new(DidAuthMode::Strict, None, b"different", HashSet::new());
        assert!(verify_token(&c2, &issued.token, 2001).is_err());
    }

    #[test]
    fn w3c_did_key_form_is_accepted() {
        let sk = SigningKey::from_bytes(&[9u8; 32]);
        let pk = sk.verifying_key().to_bytes();
        // Encode 0xed01 || pk in base58btc.
        let mut data = vec![0xed, 0x01];
        data.extend_from_slice(&pk);
        let did = format!("did:key:z{}", base58_encode(&data));
        assert!(did.starts_with("did:key:z6Mk"));
        assert!(ed25519_key_matches_did(&did, &pk));
        assert!(!ed25519_key_matches_did(&did, &[0u8; 32]));
    }

    fn base58_encode(input: &[u8]) -> String {
        const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        let mut digits: Vec<u8> = vec![0];
        for &byte in input {
            let mut carry = byte as u32;
            for d in digits.iter_mut() {
                carry += (*d as u32) << 8;
                *d = (carry % 58) as u8;
                carry /= 58;
            }
            while carry > 0 {
                digits.push((carry % 58) as u8);
                carry /= 58;
            }
        }
        let mut out: String = input.iter().take_while(|&&b| b == 0).map(|_| '1').collect();
        out.extend(digits.iter().rev().map(|&d| ALPHABET[d as usize] as char));
        out
    }

    #[test]
    fn bare_dids_follow_mode_and_protection() {
        let legacy = cfg(DidAuthMode::Legacy);
        let strict = cfg(DidAuthMode::Strict);
        let user = "did:spacekit:user:alice-123";
        let admin = "did:spacekit:admin:website-api";
        let h_user = format!("DID {user}");
        let h_admin = format!("DID {admin}");

        assert_eq!(authenticate(&legacy, Some(&h_user), None, "GET", "/x", 1).unwrap(), user);
        assert_eq!(authenticate(&strict, Some(&h_user), None, "GET", "/x", 1), Err(AuthFailure::BareDidRejected));
        // Protected DIDs never work by bare claim, in either mode.
        assert_eq!(authenticate(&legacy, Some(&h_admin), None, "GET", "/x", 1), Err(AuthFailure::BareDidRejected));
        assert_eq!(
            authenticate(&legacy, Some(&h_admin), Some("wrong"), "GET", "/x", 1),
            Err(AuthFailure::BareDidRejected)
        );
        // With the service secret, a trusted backend may assert any DID.
        assert_eq!(authenticate(&strict, Some(&h_admin), Some("svc-secret"), "GET", "/x", 1).unwrap(), admin);
        // `Bearer <did>` is the same bare claim.
        let bearer = format!("Bearer {admin}");
        assert_eq!(authenticate(&legacy, Some(&bearer), None, "GET", "/x", 1), Err(AuthFailure::BareDidRejected));
        // Optional auth degrades to anonymous.
        assert_eq!(authenticate_optional(&legacy, Some(&h_admin), None, 1), None);
        assert_eq!(authenticate_optional(&legacy, Some(&h_user), None, 1), Some(user.to_string()));
        assert_eq!(authenticate_optional(&strict, Some(&h_user), None, 1), None);
        // No secret configured: service path is closed.
        let no_secret = AuthConfig::new(DidAuthMode::Strict, None, b"t", HashSet::new());
        assert_eq!(authenticate(&no_secret, Some(&h_admin), Some(""), "GET", "/x", 1), Err(AuthFailure::BareDidRejected));
        // Protected DIDs cannot mint sessions by signature.
        assert!(strict.is_protected(admin));
    }

    #[test]
    fn app_tokens_stay_inside_their_app() {
        let c = cfg(DidAuthMode::Strict);
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let viewer = kitspace_did(&sk);
        let session = login(&c, &signed_login(&c, &sk, &viewer, 1000), 1000).unwrap();
        let parent = verify_token(&c, &session.token, 1000).unwrap();
        let app = "ab".repeat(32);
        let publisher = "did:key:z6Mkpublisher00000000000000000000000000000000";

        let shared = delegate(
            &c,
            &parent,
            &DelegateRequest { app_id: app.clone(), act_as: Some(publisher.into()), ttl_seconds: None },
            1000,
        )
        .unwrap();
        let h = format!("Bearer {}", shared.token);
        let coll = format!("app_{app}_scores");
        // get/put land in the publisher's namespace
        assert_eq!(
            authenticate(&c, Some(&h), None, "PUT", &format!("/api/documents/{coll}/me"), 1001).unwrap(),
            publisher
        );
        assert_eq!(
            authenticate(&c, Some(&h), None, "GET", &format!("/api/documents/{coll}/me"), 1001).unwrap(),
            publisher
        );
        // list/delete are the owner's
        assert!(authenticate(&c, Some(&h), None, "GET", &format!("/api/documents/{coll}"), 1001).is_err());
        assert!(authenticate(&c, Some(&h), None, "DELETE", &format!("/api/documents/{coll}/me"), 1001).is_err());
        // other apps' collections and other routes are out of scope
        let other = format!("app_{}_scores", "cd".repeat(32));
        assert!(authenticate(&c, Some(&h), None, "GET", &format!("/api/documents/{other}/me"), 1001).is_err());
        assert!(authenticate(&c, Some(&h), None, "GET", "/api/documents/auth_sessions/x", 1001).is_err());
        assert!(authenticate(&c, Some(&h), None, "POST", "/files/upload", 1001).is_err());
        // percent-encoding does not smuggle a collection past the prefix check
        assert!(authenticate(&c, Some(&h), None, "GET", "/api/documents/auth%5Fsessions/x", 1001).is_err());
        // optional-auth routes treat scoped tokens as anonymous
        assert_eq!(authenticate_optional(&c, Some(&h), None, 1001), None);
        // scoped tokens cannot delegate further
        let scoped = verify_token(&c, &shared.token, 1001).unwrap();
        assert!(delegate(&c, &scoped, &DelegateRequest { app_id: app.clone(), act_as: None, ttl_seconds: None }, 1001).is_err());
        // cannot act as a protected DID
        assert!(delegate(
            &c,
            &parent,
            &DelegateRequest { app_id: app.clone(), act_as: Some("did:spacekit:admin:website-api".into()), ttl_seconds: None },
            1001
        )
        .is_err());

        // Own-namespace app token: all ops, still only this app's collections.
        let own = delegate(&c, &parent, &DelegateRequest { app_id: app.clone(), act_as: None, ttl_seconds: None }, 1000).unwrap();
        let h = format!("Bearer {}", own.token);
        assert_eq!(authenticate(&c, Some(&h), None, "GET", &format!("/api/documents/{coll}"), 1001).unwrap(), viewer);
        assert!(authenticate(&c, Some(&h), None, "GET", "/api/documents/notes", 1001).is_err());
        // app tokens expire within the hour
        assert!(authenticate(&c, Some(&h), None, "GET", &format!("/api/documents/{coll}"), 1000 + MAX_APP_TOKEN_TTL_SECONDS + 1).is_err());
    }

    #[test]
    fn service_assertions_become_tokens_for_self_parsing_routes() {
        let c = cfg(DidAuthMode::Strict);
        let admin = "did:spacekit:admin:website-api";
        let h = Some(format!("DID {admin}"));
        let normalized = normalize_service_authorization(&c, h.clone(), Some("svc-secret"), 5).unwrap();
        assert!(normalized.starts_with("Bearer sktok1."));
        assert_eq!(did_from_authorization(&c, &normalized, 6), Some(admin.to_string()));
        // Without the secret the header is untouched and does not authenticate.
        let untouched = normalize_service_authorization(&c, h, Some("nope"), 5).unwrap();
        assert_eq!(untouched, format!("DID {admin}"));
        assert_eq!(did_from_authorization(&c, &untouched, 6), None);
        // The minted token is short-lived.
        assert_eq!(did_from_authorization(&c, &normalized, 5 + 61), None);
    }

    #[test]
    fn slh_dsa_login() {
        use signature::{Keypair, Signer};
        let c = cfg(DidAuthMode::Strict);
        let sk = slh_dsa::SigningKey::<slh_dsa::Sha2_128s>::slh_keygen_internal(&[1u8; 16], &[2u8; 16], &[3u8; 16]);
        let pk = sk.verifying_key().to_bytes();
        let did = format!("did:key:zQ3s{}", &hex::encode(pk.as_slice())[..44]);
        let (challenge, _) = mint_challenge(&c, &did, 100).unwrap();
        let sig = sk.sign(login_message(&did, &challenge).as_bytes());
        let mut req = SessionLoginRequest {
            did: did.clone(),
            challenge,
            algorithm: "slh-dsa-sha2-128s".into(),
            public_key_hex: hex::encode(pk.as_slice()),
            signature_hex: hex::encode(sig.to_bytes().as_slice()),
            ttl_seconds: None,
        };
        let issued = login(&c, &req, 100).unwrap();
        assert_eq!(verify_token(&c, &issued.token, 101).unwrap().amr, "sig:slh-dsa-sha2-128s");
        // Same key cannot claim an Ed25519 DID, and a wrong algorithm label fails.
        req.algorithm = "slh-dsa-sha2-192s".into();
        assert!(login(&c, &req, 100).is_err());
        req.algorithm = "rsa".into();
        assert!(login(&c, &req, 100).is_err());
    }

    #[test]
    fn reserved_app_collections() {
        let app = "ab".repeat(32);
        assert!(is_reserved_app_collection(&format!("app_{app}___subscriptions")));
        assert!(!is_reserved_app_collection(&format!("app_{app}_scores")));
        assert!(!is_reserved_app_collection("__subscriptions"));
        let legacy = cfg(DidAuthMode::Legacy);
        let owner = "did:spacekit:user:pub";
        let h = format!("DID {owner}");
        let reserved = format!("/api/documents/app_{app}___subscriptions/did_x");
        // Bare claims cannot write reserved records; reads still work in legacy.
        assert_eq!(authenticate(&legacy, Some(&h), None, "PUT", &reserved, 1), Err(AuthFailure::BareDidRejected));
        assert_eq!(authenticate(&legacy, Some(&h), None, "GET", &reserved, 1).unwrap(), owner);
        // The service may write them.
        assert_eq!(authenticate(&legacy, Some(&h), Some("svc-secret"), "PUT", &reserved, 1).unwrap(), owner);
        // App tokens may read but not write them, even acting as the publisher.
        let parent = TokenClaims { v: 1, sub: "did:key:z6Mkviewer".into(), iat: 0, exp: 10_000, amr: "sig:ed25519".into(), scope: None };
        let t = delegate(&legacy, &parent, &DelegateRequest { app_id: app.clone(), act_as: Some(owner.into()), ttl_seconds: None }, 1).unwrap();
        let bh = format!("Bearer {}", t.token);
        assert!(authenticate(&legacy, Some(&bh), None, "PUT", &reserved, 2).is_err());
        assert_eq!(authenticate(&legacy, Some(&bh), None, "GET", &reserved, 2).unwrap(), owner);
        // The owner's own unscoped session may write (e.g. grant a free subscription).
        let own = service_token(&legacy, &ServiceTokenRequest { did: owner.into(), ttl_seconds: None, app_id: None, act_as: None }, 1).unwrap();
        assert_eq!(authenticate(&legacy, Some(&format!("Bearer {}", own.token)), None, "PUT", &reserved, 2).unwrap(), owner);
    }

    #[test]
    fn service_tokens() {
        let c = cfg(DidAuthMode::Strict);
        let t = service_token(&c, &ServiceTokenRequest { did: "did:spacekit:user:u1".into(), ttl_seconds: Some(999_999), app_id: None, act_as: None }, 10).unwrap();
        assert_eq!(t.expires_at, 10 + MAX_SESSION_TTL_SECONDS);
        let h = format!("Bearer {}", t.token);
        assert_eq!(authenticate(&c, Some(&h), None, "GET", "/files/abc/refs", 11).unwrap(), "did:spacekit:user:u1");
    }
}
