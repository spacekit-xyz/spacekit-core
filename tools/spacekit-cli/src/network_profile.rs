//! On-disk network profile: `~/.spacekit/network/config.toml` (or `SPACEKIT_NETWORK_CONFIG`).
//! Merged into `CLIConfig.connections` whenever `load_cli_config` runs.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Default compute API (health listener).
pub const DEFAULT_COMPUTE_URL: &str = "http://127.0.0.1:9000";
/// Default storage HTTP API.
pub const DEFAULT_STORAGE_URL: &str = "http://127.0.0.1:3030";
pub const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8080";
pub const DEFAULT_MESSAGING_BOOTSTRAP: &str = "/ip4/127.0.0.1/tcp/7000";
pub const DEFAULT_MESSAGING_LISTEN: &str = "127.0.0.1:7100";
/// Browser / website-api HTTP API (envelope + SSE). Avoid :7000 on macOS (AirPlay).
pub const DEFAULT_MESSAGING_HTTP_PORT: u16 = 17000;

pub const DEFAULT_STORAGE_HTTP_PORT: u16 = 3030;
pub const DEFAULT_STORAGE_P2P_PORT: u16 = 4001;
pub const DEFAULT_COMPUTE_HTTP_PORT: u16 = 9000;
pub const DEFAULT_COMPUTE_P2P_PORT: u16 = 9001;
pub const DEFAULT_MESSAGING_LISTEN_PORT: u16 = 7100;
pub const DEFAULT_MESSAGING_BOOTSTRAP_PORT: u16 = 7000;
pub const DEFAULT_GATEWAY_HTTP_PORT: u16 = 8080;
pub const DEFAULT_STATUS_HTTP_PORT: u16 = 9100;
pub const DEFAULT_KEYMASTER_COORDINATOR_PORT: u16 = 8780;
pub const DEFAULT_KEYMASTER_REGISTRY_PORT: u16 = 8770;
pub const DEFAULT_KEYMASTER_GUARDIAN_BASE_PORT: u16 = 8781;
pub const DEFAULT_BIND_HOST: &str = "127.0.0.1";
pub const NETWORK_PROFILE_VERSION: u32 = 3;
pub const NETWORK_MANIFEST_VERSION: u32 = 1;
pub const NETWORK_PROTOCOL: &str = "spacekit";
pub const NETWORK_PROTOCOL_VERSION: u32 = 1;

/// Deployment presets. Existing v1/v2 profiles migrate to `local`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPreset {
    #[default]
    Local,
    Private,
    Public,
}

/// The duties this node is permitted to perform on a private or public network.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum NetworkRole {
    #[default]
    Subscriber,
    Operator,
    Validator,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkAdmissionPolicy {
    /// DIDs or peer IDs admitted to a private network.
    #[serde(default)]
    pub allowlist: Vec<String>,
    /// Canonical lowercase hex digest of the shared genesis document.
    #[serde(default)]
    pub shared_genesis_hash: Option<String>,
    /// Whether this profile explicitly enables a faucet. Public defaults never do.
    #[serde(default)]
    pub faucet_enabled: bool,
    /// Require a signed manifest before joining.
    #[serde(default)]
    pub require_signed_manifest: bool,
}

impl Default for NetworkAdmissionPolicy {
    fn default() -> Self {
        Self {
            allowlist: Vec::new(),
            shared_genesis_hash: None,
            faucet_enabled: false,
            require_signed_manifest: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ManifestSignatureAlgorithm {
    Sphincs128f,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ManifestSignatureEncoding {
    Hex,
    Base64,
}

/// Detached signature metadata. `signature` signs `NetworkManifest::canonical_unsigned_bytes()`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestSignature {
    pub algorithm: ManifestSignatureAlgorithm,
    pub encoding: ManifestSignatureEncoding,
    pub key_id: String,
    /// Raw SPHINCS+ verification key, encoded according to `encoding`.
    pub public_key: String,
    pub signature: String,
    #[serde(default)]
    pub signed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestGenesis {
    /// Blake3 digest of the canonical genesis document, as lowercase hex.
    pub hash: String,
    #[serde(default)]
    pub uri: Option<String>,
    /// Canonical genesis JSON. Private manifests must include this so every member
    /// verifies the same bytes before startup.
    #[serde(default)]
    pub document: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestProtocol {
    pub name: String,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestMember {
    pub did: String,
    #[serde(default)]
    pub roles: Vec<NetworkRole>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct ManifestBootstrap {
    #[serde(default)]
    pub p2p: Vec<String>,
    #[serde(default)]
    pub rpc: Vec<String>,
}

/// Portable network identity and bootstrap envelope.
///
/// Object keys are serialized canonically and the signature field is omitted from signing bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetworkManifest {
    #[serde(default = "default_manifest_version")]
    pub version: u32,
    pub network_id: String,
    pub profile: NetworkPreset,
    pub chain_id: u64,
    pub protocol: ManifestProtocol,
    pub genesis: ManifestGenesis,
    pub bootstrap: ManifestBootstrap,
    #[serde(default)]
    pub roles: Vec<NetworkRole>,
    /// Explicit role admissions. Private members must be listed; public subscribers
    /// may join without an entry, but operators and validators may not.
    #[serde(default)]
    pub members: Vec<ManifestMember>,
    #[serde(default)]
    pub signature: Option<ManifestSignature>,
}

fn default_manifest_version() -> u32 {
    NETWORK_MANIFEST_VERSION
}

impl NetworkManifest {
    /// Deterministic compact JSON used as the signature payload.
    pub fn canonical_unsigned_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        let mut value = serde_json::to_value(self)?;
        if let Some(object) = value.as_object_mut() {
            object.remove("signature");
        }
        serde_json::to_vec(&sort_json_objects(value))
    }

    pub fn validate(&self) -> Result<(), NetworkValidationError> {
        let mut errors = Vec::new();
        if self.version != NETWORK_MANIFEST_VERSION {
            errors.push(format!(
                "manifest version {} is unsupported (expected {})",
                self.version, NETWORK_MANIFEST_VERSION
            ));
        }
        if self.profile == NetworkPreset::Local {
            errors.push("network manifests are only valid for private or public profiles".into());
        }
        if self.network_id.trim().is_empty() || self.network_id.chars().any(char::is_whitespace) {
            errors.push("network_id must be non-empty and contain no whitespace".into());
        }
        validate_digest("genesis.hash", &self.genesis.hash, &mut errors);
        if self.protocol.name != NETWORK_PROTOCOL
            || self.protocol.version != NETWORK_PROTOCOL_VERSION
        {
            errors.push(format!(
                "protocol {} v{} is incompatible (expected {} v{})",
                self.protocol.name,
                self.protocol.version,
                NETWORK_PROTOCOL,
                NETWORK_PROTOCOL_VERSION
            ));
        }
        if let Some(document) = &self.genesis.document {
            match canonical_json_bytes(document) {
                Ok(bytes) if blake3::hash(&bytes).to_hex().as_str() != self.genesis.hash => errors
                    .push(format!(
                        "genesis.document hash {} does not match genesis.hash {}",
                        blake3::hash(&bytes).to_hex(),
                        self.genesis.hash
                    )),
                Err(error) => {
                    errors.push(format!("genesis.document is not canonicalizable: {error}"))
                }
                _ => {}
            }
        } else {
            errors.push(
                "network manifests require genesis.document for pre-start genesis verification"
                    .into(),
            );
        }
        if self.bootstrap.p2p.is_empty() {
            errors.push("bootstrap.p2p must contain at least one peer".into());
        }
        if self.roles.is_empty() {
            errors.push("roles must contain subscriber, operator, or validator".into());
        } else {
            let mut seen = Vec::new();
            for role in &self.roles {
                if seen.contains(role) {
                    errors.push(format!("roles contains duplicate {:?}", role));
                }
                seen.push(*role);
            }
        }
        if self.profile == NetworkPreset::Public && self.bootstrap.rpc.is_empty() {
            errors.push("public manifests require at least one bootstrap RPC URL".into());
        }
        for member in &self.members {
            if member.did.trim().is_empty() || member.did.chars().any(char::is_whitespace) {
                errors.push("members[].did must be non-empty and contain no whitespace".into());
            }
            if member.roles.is_empty() {
                errors.push(format!("member {} must have at least one role", member.did));
            }
            if member.roles.iter().any(|role| !self.roles.contains(role)) {
                errors.push(format!(
                    "member {} has a role not enabled by the manifest",
                    member.did
                ));
            }
        }
        match &self.signature {
            Some(signature) => signature.validate_into(&mut errors),
            None if self.profile == NetworkPreset::Public => {
                errors.push("public manifests require signature metadata".into())
            }
            None => {}
        }
        finish_validation(errors)
    }
}

impl ManifestSignature {
    fn validate_into(&self, errors: &mut Vec<String>) {
        let key_id = self.key_id.trim();
        if key_id.is_empty() || key_id.chars().any(char::is_whitespace) {
            errors.push("signature.key_id must be non-empty and contain no whitespace".into());
        }
        match self.encoding {
            ManifestSignatureEncoding::Hex => {
                if self.public_key.is_empty()
                    || self.public_key.len() % 2 != 0
                    || !self.public_key.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    errors.push("signature.public_key must be hexadecimal".into());
                }
                if self.signature.len() < 64
                    || self.signature.len() % 2 != 0
                    || !self.signature.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    errors.push(
                        "signature.signature must be even-length hexadecimal (at least 32 bytes)"
                            .into(),
                    );
                }
            }
            ManifestSignatureEncoding::Base64 => {
                if !is_base64_shape(&self.public_key) {
                    errors.push("signature.public_key must have valid base64 shape".into());
                }
                if self.signature.len() < 44 || !is_base64_shape(&self.signature) {
                    errors.push(
                        "signature.signature must have valid base64 shape (at least 32 bytes)"
                            .into(),
                    );
                }
            }
        }
    }
}

impl NetworkManifest {
    pub fn verify_signature(&self) -> Result<(), NetworkValidationError> {
        let Some(signature) = &self.signature else {
            return Err(NetworkValidationError {
                errors: vec!["manifest has no cryptographic signature".into()],
            });
        };
        let (public_key, signature_bytes) = match signature.encoding {
            ManifestSignatureEncoding::Hex => (
                hex::decode(&signature.public_key).map_err(|_| ()),
                hex::decode(&signature.signature).map_err(|_| ()),
            ),
            ManifestSignatureEncoding::Base64 => (
                decode_base64(&signature.public_key),
                decode_base64(&signature.signature),
            ),
        };
        let (public_key, signature_bytes) = match (public_key, signature_bytes) {
            (Ok(public_key), Ok(signature_bytes)) => (public_key, signature_bytes),
            _ => {
                return Err(NetworkValidationError {
                    errors: vec!["manifest signature encoding is invalid".into()],
                })
            }
        };
        let payload = self
            .canonical_unsigned_bytes()
            .map_err(|error| NetworkValidationError {
                errors: vec![format!("cannot canonicalize manifest: {error}")],
            })?;
        let algorithm = match signature.algorithm {
            ManifestSignatureAlgorithm::Sphincs128f => "sphincs-128f",
        };
        let detached = spacekit_primitives::v1::crypto::quantum::SPHINCSSignature::new(
            signature_bytes,
            algorithm.into(),
            public_key,
        );
        match spacekit_primitives::v1::crypto::quantum::verify_sphincs_signature(
            &payload, &detached,
        ) {
            Ok(true) => Ok(()),
            Ok(false) => Err(NetworkValidationError {
                errors: vec!["manifest cryptographic signature is invalid".into()],
            }),
            Err(error) => Err(NetworkValidationError {
                errors: vec![format!("manifest signature verification failed: {error}")],
            }),
        }
    }

    pub fn permits(&self, did: &str, role: NetworkRole) -> bool {
        if !self.roles.contains(&role) {
            return false;
        }
        if self.profile == NetworkPreset::Public && role == NetworkRole::Subscriber {
            return true;
        }
        self.members
            .iter()
            .any(|member| member.did == did && member.roles.contains(&role))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkValidationError {
    pub errors: Vec<String>,
}

impl fmt::Display for NetworkValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid network configuration: {}",
            self.errors.join("; ")
        )
    }
}

impl std::error::Error for NetworkValidationError {}

fn finish_validation(errors: Vec<String>) -> Result<(), NetworkValidationError> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(NetworkValidationError { errors })
    }
}

fn validate_digest(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.len() != 64
        || !value.bytes().all(|b| b.is_ascii_hexdigit())
        || value.bytes().any(|b| b.is_ascii_uppercase())
    {
        errors.push(format!(
            "{field} must be a 32-byte lowercase hexadecimal digest"
        ));
    }
}

fn is_base64_shape(value: &str) -> bool {
    if value.len() % 4 != 0 {
        return false;
    }
    let padding_start = value.find('=').unwrap_or(value.len());
    let (payload, padding) = value.split_at(padding_start);
    padding.len() <= 2
        && padding.bytes().all(|b| b == b'=')
        && payload
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
}

fn decode_base64(value: &str) -> Result<Vec<u8>, ()> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| ())
}

fn canonical_json_bytes(value: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&sort_json_objects(value.clone()))
}

pub fn canonical_genesis_hash(value: &serde_json::Value) -> Result<String, serde_json::Error> {
    Ok(blake3::hash(&canonical_json_bytes(value)?)
        .to_hex()
        .to_string())
}

fn sort_json_objects(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries: Vec<_> = object.into_iter().collect();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, value)| (key, sort_json_objects(value)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(sort_json_objects).collect())
        }
        other => other,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum NetworkService {
    Storage,
    Messaging,
    Compute,
    Gateway,
    Keymaster,
}

impl NetworkService {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Storage => "storage",
            Self::Messaging => "messaging",
            Self::Compute => "compute",
            Self::Gateway => "gateway",
            Self::Keymaster => "keymaster",
        }
    }

    pub fn parse_list(s: &str) -> Result<Vec<Self>, String> {
        let mut out = Vec::new();
        for part in s.split(',') {
            let part = part.trim().to_ascii_lowercase();
            if part.is_empty() {
                continue;
            }
            let svc = match part.as_str() {
                "storage" => Self::Storage,
                "messaging" => Self::Messaging,
                "compute" => Self::Compute,
                "gateway" => Self::Gateway,
                "keymaster" => Self::Keymaster,
                _ => {
                    return Err(format!(
                        "unknown service {:?} (use storage,messaging,compute,gateway,keymaster)",
                        part
                    ))
                }
            };
            if !out.contains(&svc) {
                out.push(svc);
            }
        }
        if out.is_empty() {
            return Err("empty service list".to_string());
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum NetworkMode {
    /// `network up` starts in-process nodes for enabled `[services]`.
    #[default]
    Embedded,
    /// Use external URLs only; `network up` validates connectivity and does not embed nodes.
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacekitNetworkFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub profile: NetworkPreset,
    #[serde(default)]
    pub role: NetworkRole,
    /// Stable local node name. It scopes runtime files and data directories.
    #[serde(default = "default_node_id")]
    pub node_id: String,
    /// Path to the signed/canonical JSON manifest used to join private/public networks.
    #[serde(default)]
    pub manifest: Option<PathBuf>,
    #[serde(default)]
    pub admission: NetworkAdmissionPolicy,
    #[serde(default)]
    pub mode: NetworkMode,
    #[serde(default)]
    pub bind_host: String,
    #[serde(default)]
    pub services: NetworkServicesSection,
    #[serde(default)]
    pub ports: NetworkPortsSection,
    #[serde(default)]
    pub urls: NetworkUrls,
    #[serde(default)]
    pub messaging: NetworkMessagingSection,
    #[serde(default)]
    pub data: NetworkDataDirs,
    #[serde(default)]
    pub runtime: NetworkRuntimeOptions,
    #[serde(default)]
    pub blockchain: BlockchainSection,
}

fn default_version() -> u32 {
    NETWORK_PROFILE_VERSION
}

fn default_node_id() -> String {
    "default".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkServicesSection {
    #[serde(default = "default_true")]
    pub storage: bool,
    #[serde(default = "default_true")]
    pub messaging: bool,
    #[serde(default = "default_true")]
    pub compute: bool,
    #[serde(default)]
    pub gateway: bool,
    #[serde(default)]
    pub keymaster: bool,
}

fn default_true() -> bool {
    true
}

impl Default for NetworkServicesSection {
    fn default() -> Self {
        Self {
            storage: true,
            messaging: true,
            compute: true,
            gateway: false,
            keymaster: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPortsSection {
    #[serde(default = "default_storage_http_port")]
    pub storage_http: u16,
    #[serde(default = "default_storage_p2p_port")]
    pub storage_p2p: u16,
    #[serde(default = "default_compute_http_port")]
    pub compute_http: u16,
    #[serde(default = "default_compute_p2p_port")]
    pub compute_p2p: u16,
    #[serde(default = "default_messaging_listen_port")]
    pub messaging_listen: u16,
    #[serde(default = "default_messaging_bootstrap_port")]
    pub messaging_bootstrap: u16,
    #[serde(default = "default_messaging_http_port")]
    pub messaging_http: u16,
    #[serde(default = "default_gateway_http_port")]
    pub gateway_http: u16,
    #[serde(default = "default_status_http_port")]
    pub status_http: u16,
    #[serde(default = "default_keymaster_coordinator_port")]
    pub keymaster_coordinator: u16,
    #[serde(default = "default_keymaster_registry_port")]
    pub keymaster_registry: u16,
    #[serde(default = "default_keymaster_guardian_base_port")]
    pub keymaster_guardian_base: u16,
}

fn default_storage_http_port() -> u16 {
    DEFAULT_STORAGE_HTTP_PORT
}
fn default_storage_p2p_port() -> u16 {
    DEFAULT_STORAGE_P2P_PORT
}
fn default_compute_http_port() -> u16 {
    DEFAULT_COMPUTE_HTTP_PORT
}
fn default_compute_p2p_port() -> u16 {
    DEFAULT_COMPUTE_P2P_PORT
}
fn default_messaging_listen_port() -> u16 {
    DEFAULT_MESSAGING_LISTEN_PORT
}
fn default_messaging_bootstrap_port() -> u16 {
    DEFAULT_MESSAGING_BOOTSTRAP_PORT
}
fn default_messaging_http_port() -> u16 {
    DEFAULT_MESSAGING_HTTP_PORT
}
fn default_gateway_http_port() -> u16 {
    DEFAULT_GATEWAY_HTTP_PORT
}
fn default_status_http_port() -> u16 {
    DEFAULT_STATUS_HTTP_PORT
}
fn default_keymaster_coordinator_port() -> u16 {
    DEFAULT_KEYMASTER_COORDINATOR_PORT
}
fn default_keymaster_registry_port() -> u16 {
    DEFAULT_KEYMASTER_REGISTRY_PORT
}
fn default_keymaster_guardian_base_port() -> u16 {
    DEFAULT_KEYMASTER_GUARDIAN_BASE_PORT
}

impl Default for NetworkPortsSection {
    fn default() -> Self {
        Self {
            storage_http: DEFAULT_STORAGE_HTTP_PORT,
            storage_p2p: DEFAULT_STORAGE_P2P_PORT,
            compute_http: DEFAULT_COMPUTE_HTTP_PORT,
            compute_p2p: DEFAULT_COMPUTE_P2P_PORT,
            messaging_listen: DEFAULT_MESSAGING_LISTEN_PORT,
            messaging_bootstrap: DEFAULT_MESSAGING_BOOTSTRAP_PORT,
            messaging_http: DEFAULT_MESSAGING_HTTP_PORT,
            gateway_http: DEFAULT_GATEWAY_HTTP_PORT,
            status_http: DEFAULT_STATUS_HTTP_PORT,
            keymaster_coordinator: DEFAULT_KEYMASTER_COORDINATOR_PORT,
            keymaster_registry: DEFAULT_KEYMASTER_REGISTRY_PORT,
            keymaster_guardian_base: DEFAULT_KEYMASTER_GUARDIAN_BASE_PORT,
        }
    }
}

impl NetworkPortsSection {
    /// Stable names and ports reserved by a profile, used for preflight collision checks.
    pub fn allocations(&self) -> [(&'static str, u16); 12] {
        [
            ("storage_http", self.storage_http),
            ("storage_p2p", self.storage_p2p),
            ("compute_http", self.compute_http),
            ("compute_p2p", self.compute_p2p),
            ("messaging_listen", self.messaging_listen),
            ("messaging_bootstrap", self.messaging_bootstrap),
            ("messaging_http", self.messaging_http),
            ("gateway_http", self.gateway_http),
            ("status_http", self.status_http),
            ("keymaster_coordinator", self.keymaster_coordinator),
            ("keymaster_registry", self.keymaster_registry),
            ("keymaster_guardian_base", self.keymaster_guardian_base),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkUrls {
    pub compute: Option<String>,
    pub storage: Option<String>,
    pub gateway: Option<String>,
    pub keymaster_coordinator: Option<String>,
    pub keymaster_registry: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMessagingSection {
    /// Full listen socket `host:port` (overrides `ports.messaging_listen` when set).
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    /// libp2p multiaddrs; when empty, derived from `ports.messaging_bootstrap`.
    #[serde(default)]
    pub bootstrap_peers: Vec<String>,
}

fn default_listen_addr() -> String {
    DEFAULT_MESSAGING_LISTEN.to_string()
}

impl Default for NetworkMessagingSection {
    fn default() -> Self {
        Self {
            listen_addr: default_listen_addr(),
            bootstrap_peers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkDataDirs {
    pub storage: Option<PathBuf>,
    pub compute: Option<PathBuf>,
    pub messaging: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRuntimeOptions {
    #[serde(default = "default_max_storage_gb")]
    pub max_storage_gb: u64,
    #[serde(default = "default_quantum_algorithm")]
    pub quantum_algorithm: String,
    #[serde(default = "default_health_timeout")]
    pub health_check_timeout_secs: u64,
    /// Optional HMAC secret for storage-node upload tokens (also written to storage data_dir).
    #[serde(default)]
    pub upload_token_secret: Option<String>,
    /// Blob/fact auth: `permissive` | `hybrid` | `strict` (overrides env when set).
    #[serde(default)]
    pub blob_fact_auth: Option<String>,
    /// Start libp2p on the embedded storage node (default false for local dev).
    /// Override with `SPACEKIT_ENABLE_P2P=1` or set `enable_p2p = true` here.
    #[serde(default)]
    pub enable_p2p: bool,
    /// Retain P2P chunk bytes in RAM when `enable_p2p` is true (default false).
    #[serde(default)]
    pub cache_p2p_chunks_in_memory: bool,
}

// ── Blockchain / genesis configuration ──────────────────────────────────────

/// Default block production interval for embedded local supervisor (10s).
pub const DEFAULT_BLOCK_TIME_MS: u64 = 10_000;
/// Warn when block time is below this — fast blocks + ledger persistence increase RSS.
pub const MIN_RECOMMENDED_BLOCK_TIME_MS: u64 = 1_000;
pub const DEFAULT_EPOCH_LENGTH: u64 = 100;
pub const DEFAULT_PERSIST_INTERVAL_BLOCKS: u64 = 100;
pub const DEFAULT_OPERATOR_REWARD_PER_BLOCK: u64 = 1000;
pub const DEFAULT_STORAGE_REWARD_PER_GB_EPOCH: u64 = 500;
pub const DEFAULT_COMPUTE_REWARD_PER_GAS: u64 = 1;
pub const DEFAULT_CHAIN_ID: u64 = 31337;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainSection {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_chain_id")]
    pub chain_id: u64,
    #[serde(default = "default_block_time_ms")]
    pub block_time_ms: u64,
    #[serde(default = "default_epoch_length")]
    pub epoch_length: u64,
    /// Write `ledger.json` every N blocks when `persist_state` is true (default 100).
    #[serde(default = "default_persist_interval_blocks")]
    pub persist_interval_blocks: u64,
    /// Persist SwtchVM ledger state across restarts.
    #[serde(default)]
    pub persist_state: bool,
    #[serde(default)]
    pub state_dir: Option<String>,
    #[serde(default)]
    pub genesis: GenesisSection,
    #[serde(default)]
    pub validators: ValidatorSection,
    #[serde(default)]
    pub rewards: RewardSection,
}

fn default_chain_id() -> u64 {
    DEFAULT_CHAIN_ID
}
fn default_block_time_ms() -> u64 {
    DEFAULT_BLOCK_TIME_MS
}
fn default_epoch_length() -> u64 {
    DEFAULT_EPOCH_LENGTH
}
fn default_persist_interval_blocks() -> u64 {
    DEFAULT_PERSIST_INTERVAL_BLOCKS
}

/// Block time for the embedded supervisor loop (`SPACEKIT_BLOCK_TIME_MS` overrides config).
pub fn resolve_block_time_ms(net: &SpacekitNetworkFile) -> u64 {
    if let Ok(v) = std::env::var("SPACEKIT_BLOCK_TIME_MS") {
        if let Ok(ms) = v.trim().parse::<u64>() {
            return ms.max(100);
        }
    }
    net.blockchain.block_time_ms.max(100)
}

pub fn resolve_persist_interval_blocks(net: &SpacekitNetworkFile) -> u64 {
    if let Ok(v) = std::env::var("SPACEKIT_BLOCKCHAIN_PERSIST_EVERY") {
        if let Ok(n) = v.trim().parse::<u64>() {
            return n.max(1);
        }
    }
    net.blockchain.persist_interval_blocks.max(1)
}

impl Default for BlockchainSection {
    fn default() -> Self {
        Self {
            enabled: false,
            chain_id: DEFAULT_CHAIN_ID,
            block_time_ms: DEFAULT_BLOCK_TIME_MS,
            epoch_length: DEFAULT_EPOCH_LENGTH,
            persist_interval_blocks: DEFAULT_PERSIST_INTERVAL_BLOCKS,
            persist_state: false,
            state_dir: None,
            genesis: GenesisSection::default(),
            validators: ValidatorSection::default(),
            rewards: RewardSection::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenesisSection {
    /// Pre-funded accounts: `[{ did = "did:spacekit:...", balance = 1000000 }]`
    #[serde(default)]
    pub accounts: Vec<GenesisAccount>,
    /// WASM contracts deployed at genesis (paths).
    #[serde(default)]
    pub contracts: Vec<GenesisContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAccount {
    pub did: String,
    #[serde(default = "default_genesis_balance")]
    pub balance: u64,
}

fn default_genesis_balance() -> u64 {
    1_000_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisContract {
    pub name: String,
    pub wasm_path: String,
    #[serde(default)]
    pub args: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSection {
    /// Single-node dev mode: this node validates its own blocks.
    #[serde(default = "default_true")]
    pub self_validate: bool,
    /// Additional validator DIDs for multi-node consensus.
    #[serde(default)]
    pub peers: Vec<String>,
    #[serde(default = "default_min_validators")]
    pub min_validators: u32,
}

fn default_min_validators() -> u32 {
    1
}

impl Default for ValidatorSection {
    fn default() -> Self {
        Self {
            self_validate: true,
            peers: Vec::new(),
            min_validators: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardSection {
    #[serde(default = "default_operator_reward")]
    pub operator_reward_per_block: u64,
    #[serde(default = "default_storage_reward")]
    pub storage_reward_per_gb_epoch: u64,
    #[serde(default = "default_compute_reward")]
    pub compute_reward_per_gas_unit: u64,
    #[serde(default = "default_settlement_interval")]
    pub settlement_interval_secs: u64,
}

fn default_operator_reward() -> u64 {
    DEFAULT_OPERATOR_REWARD_PER_BLOCK
}
fn default_storage_reward() -> u64 {
    DEFAULT_STORAGE_REWARD_PER_GB_EPOCH
}
fn default_compute_reward() -> u64 {
    DEFAULT_COMPUTE_REWARD_PER_GAS
}
fn default_settlement_interval() -> u64 {
    5
}

impl Default for RewardSection {
    fn default() -> Self {
        Self {
            operator_reward_per_block: DEFAULT_OPERATOR_REWARD_PER_BLOCK,
            storage_reward_per_gb_epoch: DEFAULT_STORAGE_REWARD_PER_GB_EPOCH,
            compute_reward_per_gas_unit: DEFAULT_COMPUTE_REWARD_PER_GAS,
            settlement_interval_secs: 5,
        }
    }
}

fn default_max_storage_gb() -> u64 {
    10
}
fn default_quantum_algorithm() -> String {
    "kyber1024".to_string()
}
fn default_health_timeout() -> u64 {
    30
}

impl Default for NetworkRuntimeOptions {
    fn default() -> Self {
        Self {
            max_storage_gb: 10,
            quantum_algorithm: "kyber1024".to_string(),
            health_check_timeout_secs: 30,
            upload_token_secret: None,
            blob_fact_auth: None,
            enable_p2p: false,
            cache_p2p_chunks_in_memory: false,
        }
    }
}

/// Whether the embedded storage node should start libp2p.
pub fn resolve_enable_p2p(net: &SpacekitNetworkFile) -> bool {
    if std::env::var("SPACEKIT_ENABLE_P2P")
        .ok()
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
    {
        return true;
    }
    net.runtime.enable_p2p
}

impl Default for SpacekitNetworkFile {
    fn default() -> Self {
        let mut f = Self {
            version: NETWORK_PROFILE_VERSION,
            profile: NetworkPreset::Local,
            role: NetworkRole::Operator,
            node_id: default_node_id(),
            manifest: None,
            admission: NetworkAdmissionPolicy::default(),
            mode: NetworkMode::default(),
            bind_host: DEFAULT_BIND_HOST.to_string(),
            services: NetworkServicesSection::default(),
            ports: NetworkPortsSection::default(),
            urls: NetworkUrls::default(),
            messaging: NetworkMessagingSection::default(),
            data: NetworkDataDirs::default(),
            runtime: NetworkRuntimeOptions::default(),
            blockchain: BlockchainSection::default(),
        };
        f.sync_urls_from_ports();
        f
    }
}

impl SpacekitNetworkFile {
    /// Apply migrations after TOML deserialize. Legacy profiles retain their local behavior.
    pub fn normalize_after_load(&mut self) {
        if self.version < 2 {
            if self.bind_host.is_empty() {
                self.bind_host = DEFAULT_BIND_HOST.to_string();
            }
            if let Some(u) = self.urls.storage.as_ref() {
                self.ports.storage_http = parse_http_port(u, DEFAULT_STORAGE_HTTP_PORT);
            }
            if let Some(u) = self.urls.compute.as_ref() {
                self.ports.compute_http = parse_http_port(u, DEFAULT_COMPUTE_HTTP_PORT);
            }
            if self.messaging.listen_addr == DEFAULT_MESSAGING_LISTEN && self.urls.storage.is_some()
            {
                // keep explicit listen_addr if user set it
            }
        }
        if self.version < NETWORK_PROFILE_VERSION {
            self.profile = NetworkPreset::Local;
            self.role = NetworkRole::Operator;
            self.version = NETWORK_PROFILE_VERSION;
        }
        self.sync_urls_from_ports();
        if self.messaging.bootstrap_peers.is_empty() {
            self.messaging.bootstrap_peers = vec![self.default_bootstrap_multiaddr()];
        }
    }

    /// Build a complete deployment preset. Network-specific identifiers remain explicit.
    pub fn for_preset(profile: NetworkPreset) -> Self {
        let mut file = Self::default();
        file.profile = profile;
        file.blockchain.enabled = true;
        file.blockchain.persist_state = true;

        match profile {
            NetworkPreset::Local => {
                file.role = NetworkRole::Validator;
                file.services.gateway = true;
                file.blockchain.validators.self_validate = true;
                file.admission.faucet_enabled = true;
            }
            NetworkPreset::Private => {
                file.role = NetworkRole::Operator;
                file.bind_host = "0.0.0.0".into();
                file.runtime.enable_p2p = true;
                file.blockchain.validators.self_validate = false;
                file.admission.require_signed_manifest = false;
                file.messaging.listen_addr =
                    format!("{}:{}", file.bind_host, file.ports.messaging_listen);
            }
            NetworkPreset::Public => {
                file.role = NetworkRole::Subscriber;
                file.bind_host = "0.0.0.0".into();
                file.runtime.enable_p2p = true;
                file.blockchain.validators.self_validate = false;
                file.admission.require_signed_manifest = true;
                file.admission.faucet_enabled = false;
                file.messaging.listen_addr =
                    format!("{}:{}", file.bind_host, file.ports.messaging_listen);
            }
        }
        file.sync_urls_from_ports();
        file
    }

    /// Validate profile invariants and reject endpoint collisions before startup or writing.
    pub fn validate(&self) -> Result<(), NetworkValidationError> {
        let mut errors = Vec::new();
        if self.version != NETWORK_PROFILE_VERSION {
            errors.push(format!(
                "profile version {} is unsupported (expected {})",
                self.version, NETWORK_PROFILE_VERSION
            ));
        }
        if self.bind_host.trim().is_empty() {
            errors.push("bind_host must not be empty".into());
        }
        if self.node_id.trim().is_empty()
            || self
                .node_id
                .chars()
                .any(|ch| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
        {
            errors.push("node_id must contain only letters, digits, '-' or '_'".into());
        }

        let mut allocated: HashMap<u16, &str> = HashMap::new();
        for (name, port) in self.ports.allocations() {
            if port == 0 {
                errors.push(format!("ports.{name} must not be zero"));
            } else if let Some(previous) = allocated.insert(port, name) {
                errors.push(format!(
                    "port collision: ports.{previous} and ports.{name} both use {port}"
                ));
            }
        }

        match self.profile {
            NetworkPreset::Local => {}
            NetworkPreset::Private => {
                if !self.runtime.enable_p2p {
                    errors.push("private profiles must enable P2P".into());
                }
                if self.messaging.bootstrap_peers.is_empty() {
                    errors.push("private profiles require at least one bootstrap peer".into());
                }
                if self.admission.allowlist.is_empty() {
                    errors.push("private profiles require admission.allowlist".into());
                }
                match &self.admission.shared_genesis_hash {
                    Some(hash) => {
                        validate_digest("admission.shared_genesis_hash", hash, &mut errors)
                    }
                    None => {
                        errors.push("private profiles require admission.shared_genesis_hash".into())
                    }
                }
            }
            NetworkPreset::Public => {
                if !self.runtime.enable_p2p {
                    errors.push("public profiles must enable P2P".into());
                }
                if self.messaging.bootstrap_peers.is_empty() {
                    errors.push("public profiles require at least one bootstrap peer".into());
                }
                if self.manifest.is_none() {
                    errors.push("public profiles require a signed manifest path".into());
                }
                if !self.admission.require_signed_manifest {
                    errors.push("public profiles must require a signed manifest".into());
                }
                if self.admission.faucet_enabled {
                    errors.push("public profiles cannot implicitly enable a faucet".into());
                }
            }
        }
        finish_validation(errors)
    }

    pub fn http_url(&self, port: u16) -> String {
        format!("http://{}:{}", self.bind_host.trim(), port)
    }

    pub fn default_bootstrap_multiaddr(&self) -> String {
        format!(
            "/ip4/{}/tcp/{}",
            self.bind_host.trim(),
            self.ports.messaging_bootstrap
        )
    }

    pub fn resolved_messaging_http_url(&self) -> String {
        self.http_url(self.ports.messaging_http)
    }

    pub fn resolved_listen_addr(&self) -> String {
        let explicit = self.messaging.listen_addr.trim();
        if explicit.contains(':') && explicit != DEFAULT_MESSAGING_LISTEN {
            return explicit.to_string();
        }
        format!("{}:{}", self.bind_host.trim(), self.ports.messaging_listen)
    }

    /// Fill `urls.*` from ports when not explicitly set.
    pub fn sync_urls_from_ports(&mut self) {
        if self
            .urls
            .storage
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            self.urls.storage = Some(self.http_url(self.ports.storage_http));
        }
        if self
            .urls
            .compute
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            self.urls.compute = Some(self.http_url(self.ports.compute_http));
        }
        if self
            .urls
            .gateway
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            self.urls.gateway = Some(self.http_url(self.ports.gateway_http));
        }
        if self
            .urls
            .keymaster_coordinator
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            self.urls.keymaster_coordinator = Some(self.http_url(self.ports.keymaster_coordinator));
        }
        if self
            .urls
            .keymaster_registry
            .as_ref()
            .map(|s| s.is_empty())
            .unwrap_or(true)
        {
            self.urls.keymaster_registry = Some(self.http_url(self.ports.keymaster_registry));
        }
    }

    pub fn resolved_storage_url(&self) -> String {
        self.urls
            .storage
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.http_url(self.ports.storage_http))
    }

    pub fn resolved_compute_url(&self) -> String {
        self.urls
            .compute
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.http_url(self.ports.compute_http))
    }

    pub fn resolved_keymaster_coordinator_url(&self) -> String {
        self.urls
            .keymaster_coordinator
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.http_url(self.ports.keymaster_coordinator))
    }

    pub fn resolved_keymaster_registry_url(&self) -> String {
        self.urls
            .keymaster_registry
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.http_url(self.ports.keymaster_registry))
    }

    pub fn is_service_enabled(&self, svc: NetworkService) -> bool {
        match svc {
            NetworkService::Storage => self.services.storage,
            NetworkService::Messaging => self.services.messaging,
            NetworkService::Compute => self.services.compute,
            NetworkService::Gateway => self.services.gateway,
            NetworkService::Keymaster => self.services.keymaster,
        }
    }

    pub fn enabled_embedded_services(&self) -> Vec<NetworkService> {
        let mut v = Vec::new();
        if self.services.storage {
            v.push(NetworkService::Storage);
        }
        if self.services.messaging {
            v.push(NetworkService::Messaging);
        }
        if self.services.compute {
            v.push(NetworkService::Compute);
        }
        if self.services.gateway {
            v.push(NetworkService::Gateway);
        }
        if self.services.keymaster {
            v.push(NetworkService::Keymaster);
        }
        v
    }

    /// Apply a one-shot `--only` override for `network up`.
    pub fn with_only_services(&self, only: &[NetworkService]) -> Self {
        let mut c = self.clone();
        c.services.storage = only.contains(&NetworkService::Storage);
        c.services.messaging = only.contains(&NetworkService::Messaging);
        c.services.compute = only.contains(&NetworkService::Compute);
        c.services.gateway = only.contains(&NetworkService::Gateway);
        c.services.keymaster = only.contains(&NetworkService::Keymaster);
        c
    }

    pub fn apply_port_offset(&mut self, offset: u16) -> Result<(), NetworkValidationError> {
        let mut shifted = self.ports.clone();
        for (_, port) in [
            ("storage_http", &mut shifted.storage_http),
            ("storage_p2p", &mut shifted.storage_p2p),
            ("compute_http", &mut shifted.compute_http),
            ("compute_p2p", &mut shifted.compute_p2p),
            ("messaging_listen", &mut shifted.messaging_listen),
            ("messaging_bootstrap", &mut shifted.messaging_bootstrap),
            ("messaging_http", &mut shifted.messaging_http),
            ("gateway_http", &mut shifted.gateway_http),
            ("status_http", &mut shifted.status_http),
            ("keymaster_coordinator", &mut shifted.keymaster_coordinator),
            ("keymaster_registry", &mut shifted.keymaster_registry),
            (
                "keymaster_guardian_base",
                &mut shifted.keymaster_guardian_base,
            ),
        ] {
            *port = port
                .checked_add(offset)
                .ok_or_else(|| NetworkValidationError {
                    errors: vec![format!("port offset {offset} overflows {port}")],
                })?;
        }
        self.ports = shifted;
        self.messaging.listen_addr =
            format!("{}:{}", self.bind_host.trim(), self.ports.messaging_listen);
        self.sync_urls_from_ports_force();
        Ok(())
    }

    fn sync_urls_from_ports_force(&mut self) {
        self.urls.storage = Some(self.http_url(self.ports.storage_http));
        self.urls.compute = Some(self.http_url(self.ports.compute_http));
        self.urls.gateway = Some(self.http_url(self.ports.gateway_http));
        self.urls.keymaster_coordinator = Some(self.http_url(self.ports.keymaster_coordinator));
        self.urls.keymaster_registry = Some(self.http_url(self.ports.keymaster_registry));
    }
}

/// Per-service record while supervisor is running.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRuntimeInfo {
    pub enabled: bool,
    pub url: Option<String>,
    pub listen: Option<String>,
}

/// Written by `spacekit network up` while the supervisor is running.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRuntimeState {
    pub pid: u32,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub mode: NetworkMode,
    pub compute_url: String,
    pub storage_url: String,
    pub messaging_listen: String,
    #[serde(default)]
    pub services: NetworkRuntimeServices,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkRuntimeServices {
    pub storage: Option<ServiceRuntimeInfo>,
    pub messaging: Option<ServiceRuntimeInfo>,
    pub compute: Option<ServiceRuntimeInfo>,
    pub gateway: Option<ServiceRuntimeInfo>,
    pub keymaster: Option<ServiceRuntimeInfo>,
}

pub fn spacekit_network_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".spacekit").join("network"))
        .unwrap_or_else(|| PathBuf::from(".spacekit").join("network"))
}

pub fn spacekit_network_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("SPACEKIT_NETWORK_CONFIG") {
        return PathBuf::from(p);
    }
    spacekit_network_dir().join("config.toml")
}

pub fn network_runtime_state_path() -> PathBuf {
    network_instance_path("runtime.json")
}

pub fn network_messaging_key_path() -> PathBuf {
    network_instance_path("messaging_node.key")
}

pub fn network_instance_path(suffix: &str) -> PathBuf {
    let config = spacekit_network_config_path();
    let parent = config.parent().unwrap_or_else(|| std::path::Path::new("."));
    if config.file_name().and_then(|name| name.to_str()) == Some("config.toml") {
        parent.join(suffix)
    } else {
        let stem = config
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("network");
        parent.join(format!("{stem}.{suffix}"))
    }
}

pub fn default_data_dir(service: &str) -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".spacekit").join("data").join(service))
        .unwrap_or_else(|| PathBuf::from(".spacekit").join("data").join(service))
}

pub fn parse_http_port(url: &str, default: u16) -> u16 {
    let rest = url.split("://").nth(1).unwrap_or(url);
    let host_port = rest.split('/').next().unwrap_or(rest);
    if let Some((_host, port_str)) = host_port.rsplit_once(':') {
        if let Ok(p) = port_str.parse() {
            return p;
        }
    }
    default
}

pub fn load_network_runtime_state(
) -> Result<Option<NetworkRuntimeState>, Box<dyn std::error::Error>> {
    let path = network_runtime_state_path();
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&s)?))
}

pub fn write_network_runtime_state(
    state: &NetworkRuntimeState,
) -> Result<(), Box<dyn std::error::Error>> {
    let path = network_runtime_state_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(state)?)?;
    Ok(())
}

pub fn clear_network_runtime_state() {
    let path = network_runtime_state_path();
    let _ = std::fs::remove_file(path);
}

pub fn is_network_supervisor_running() -> bool {
    let Ok(Some(state)) = load_network_runtime_state() else {
        return false;
    };
    if state.mode == NetworkMode::External || state.pid == 0 {
        return true;
    }
    process_alive(state.pid)
}

#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
pub fn process_alive(pid: u32) -> bool {
    std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid)])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(unix)]
pub fn signal_process(pid: u32) -> Result<(), Box<dyn std::error::Error>> {
    let status = std::process::Command::new("kill")
        .arg(pid.to_string())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("kill {} failed with {}", pid, status).into())
    }
}

#[cfg(not(unix))]
pub fn signal_process(pid: u32) -> Result<(), Box<dyn std::error::Error>> {
    let status = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("taskkill {} failed with {}", pid, status).into())
    }
}

pub fn resolve_data_dir(file: &SpacekitNetworkFile, service: &str) -> PathBuf {
    let custom = match service {
        "storage" => file.data.storage.clone(),
        "compute" => file.data.compute.clone(),
        "messaging" => file.data.messaging.clone(),
        _ => None,
    };
    custom.unwrap_or_else(|| default_data_dir(service))
}

pub fn load_spacekit_network_file(
) -> Result<Option<SpacekitNetworkFile>, Box<dyn std::error::Error>> {
    let path = spacekit_network_config_path();
    if !path.exists() {
        return Ok(None);
    }
    let s = std::fs::read_to_string(&path)?;
    let mut f: SpacekitNetworkFile = toml::from_str(&s)?;
    f.normalize_after_load();
    f.validate()?;
    Ok(Some(f))
}

/// Read and cryptographically validate a canonical network manifest.
pub fn load_network_manifest(
    path: impl AsRef<std::path::Path>,
) -> Result<NetworkManifest, Box<dyn std::error::Error>> {
    let body = std::fs::read_to_string(path)?;
    let manifest: NetworkManifest = serde_json::from_str(&body)?;
    manifest.validate()?;
    if manifest.signature.is_some() || manifest.profile == NetworkPreset::Public {
        manifest.verify_signature()?;
    }
    Ok(manifest)
}

pub fn validate_manifest_join(
    manifest: &NetworkManifest,
    did: &str,
    role: NetworkRole,
) -> Result<(), NetworkValidationError> {
    let mut errors = Vec::new();
    if !manifest.permits(did, role) {
        errors.push(format!(
            "{did} is not admitted as {:?} by network {}",
            role, manifest.network_id
        ));
    }
    if manifest.profile == NetworkPreset::Public && manifest.signature.is_none() {
        errors.push("public join requires a cryptographically signed manifest".into());
    }
    finish_validation(errors)
}

pub fn validate_manifest_compatibility(
    profile: &SpacekitNetworkFile,
    manifest: &NetworkManifest,
) -> Result<(), NetworkValidationError> {
    let mut errors = Vec::new();
    if manifest.profile != profile.profile {
        errors.push("manifest profile does not match local profile".into());
    }
    if manifest.chain_id != profile.blockchain.chain_id {
        errors.push(format!(
            "manifest chain_id {} does not match local chain_id {}",
            manifest.chain_id, profile.blockchain.chain_id
        ));
    }
    if profile.admission.shared_genesis_hash.as_deref() != Some(&manifest.genesis.hash) {
        errors.push("manifest genesis does not match the locally admitted genesis".into());
    }
    finish_validation(errors)
}

/// Admission and compatibility gate invoked before any runtime state or child process is created.
pub fn authorize_network_start(
    profile: &SpacekitNetworkFile,
    local_did: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    profile.validate()?;
    match profile.profile {
        NetworkPreset::Local => return Ok(()),
        NetworkPreset::Private
            if !profile
                .admission
                .allowlist
                .iter()
                .any(|did| did == local_did) =>
        {
            return Err(
                format!("local DID {local_did} is not in the private admission allowlist").into(),
            )
        }
        NetworkPreset::Private
            if profile.role == NetworkRole::Validator
                && !profile
                    .blockchain
                    .validators
                    .peers
                    .iter()
                    .any(|did| did == local_did) =>
        {
            return Err(
                format!("local DID {local_did} has no explicit private validator grant").into(),
            )
        }
        _ => {}
    }

    if let Some(path) = &profile.manifest {
        let manifest = load_network_manifest(path)?;
        validate_manifest_compatibility(profile, &manifest)?;
        validate_manifest_join(&manifest, local_did, profile.role)?;
    } else if profile.profile == NetworkPreset::Public {
        return Err("public startup requires a verified manifest".into());
    }
    Ok(())
}

/// Options for `spacekit network init`.
#[derive(Debug, Default)]
pub struct NetworkInitOptions {
    pub profile: NetworkPreset,
    pub role: Option<NetworkRole>,
    pub node_id: Option<String>,
    pub port_offset: u16,
    pub data_root: Option<PathBuf>,
    pub manifest: Option<PathBuf>,
    pub mode: NetworkMode,
    pub compute_url: Option<String>,
    pub storage_url: Option<String>,
    pub gateway_url: Option<String>,
    pub bootstrap_peer: Vec<String>,
    pub bind_host: Option<String>,
    pub storage_port: Option<u16>,
    pub storage_p2p_port: Option<u16>,
    pub compute_port: Option<u16>,
    pub messaging_listen_port: Option<u16>,
    pub messaging_bootstrap_port: Option<u16>,
    pub gateway_port: Option<u16>,
    pub no_storage: bool,
    pub no_messaging: bool,
    pub no_compute: bool,
    pub enable_gateway: bool,
}

pub fn network_file_from_init(opts: NetworkInitOptions) -> SpacekitNetworkFile {
    let mut f = SpacekitNetworkFile::for_preset(opts.profile);
    if let Some(role) = opts.role {
        f.role = role;
    }
    let explicit_node_id = opts.node_id.is_some();
    if let Some(node_id) = opts.node_id {
        f.node_id = node_id;
    }
    if opts.port_offset != 0 {
        // Deferred validation reports overflow through `write_network_profile`; init
        // inputs use practical offsets, so preserve this infallible builder API.
        let _ = f.apply_port_offset(opts.port_offset);
    }
    if let Some(root) = opts.data_root {
        let node_root = root.join(&f.node_id);
        f.data.storage = Some(node_root.join("storage"));
        f.data.compute = Some(node_root.join("compute"));
        f.data.messaging = Some(node_root.join("messaging"));
    } else if explicit_node_id {
        let node_root = dirs::home_dir()
            .map(|home| home.join(".spacekit").join("data").join(&f.node_id))
            .unwrap_or_else(|| PathBuf::from(".spacekit").join("data").join(&f.node_id));
        f.data.storage = Some(node_root.join("storage"));
        f.data.compute = Some(node_root.join("compute"));
        f.data.messaging = Some(node_root.join("messaging"));
    }
    f.manifest = opts.manifest;
    f.mode = opts.mode;

    if let Some(h) = opts.bind_host.filter(|s| !s.is_empty()) {
        f.bind_host = h;
    }
    if let Some(p) = opts.storage_port {
        f.ports.storage_http = p;
    }
    if let Some(p) = opts.storage_p2p_port {
        f.ports.storage_p2p = p;
    }
    if let Some(p) = opts.compute_port {
        f.ports.compute_http = p;
    }
    if let Some(p) = opts.messaging_listen_port {
        f.ports.messaging_listen = p;
        f.messaging.listen_addr = format!("{}:{}", f.bind_host, p);
    }
    if let Some(p) = opts.messaging_bootstrap_port {
        f.ports.messaging_bootstrap = p;
    }
    if let Some(p) = opts.gateway_port {
        f.ports.gateway_http = p;
    }

    if opts.no_storage {
        f.services.storage = false;
    }
    if opts.no_messaging {
        f.services.messaging = false;
    }
    if opts.no_compute {
        f.services.compute = false;
    }
    if opts.enable_gateway {
        f.services.gateway = true;
    }

    if let Some(u) = opts.compute_url {
        f.urls.compute = Some(u);
    }
    if let Some(u) = opts.storage_url {
        f.urls.storage = Some(u);
    }
    if let Some(u) = opts.gateway_url {
        f.urls.gateway = Some(u);
    }
    if !opts.bootstrap_peer.is_empty() {
        f.messaging.bootstrap_peers = opts.bootstrap_peer;
    }

    f.sync_urls_from_ports();
    if f.messaging.bootstrap_peers.is_empty() && f.profile == NetworkPreset::Local {
        f.messaging.bootstrap_peers = vec![f.default_bootstrap_multiaddr()];
    }
    f
}

pub fn write_network_profile(
    file: &SpacekitNetworkFile,
    force: bool,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    file.validate()?;
    let path = spacekit_network_config_path();

    if path.exists() && !force {
        return Err(format!(
            "already exists: {} (use --force to overwrite)",
            path.display()
        )
        .into());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let body = toml::to_string_pretty(file)?;
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Example TOML for docs / `network init` hint.
pub fn example_network_config_toml() -> &'static str {
    r#"# SpaceKit network profile (v3 local preset)
version = 3
profile = "local"
role = "validator"
mode = "embedded"
bind_host = "127.0.0.1"

[services]
storage = true
messaging = true
compute = true
gateway = false

[ports]
storage_http = 3030
storage_p2p = 4001
compute_http = 9000
messaging_listen = 7100
messaging_bootstrap = 7000
messaging_http = 17000
gateway_http = 8080
status_http = 9100

# [urls] — optional; derived from ports when omitted

[messaging]
listen_addr = "127.0.0.1:7100"
bootstrap_peers = ["/ip4/127.0.0.1/tcp/7000"]

[runtime]
max_storage_gb = 10
quantum_algorithm = "kyber1024"
health_check_timeout_secs = 30
# upload_token_secret = "hex-or-passphrase"
# blob_fact_auth = "permissive"   # permissive | hybrid | strict
# enable_p2p = false            # libp2p on storage node (or SPACEKIT_ENABLE_P2P=1)
# cache_p2p_chunks_in_memory = false  # when enable_p2p: retain chunk bytes in RAM
# SPACEKIT_STORAGE_BIN=../target/release/spacekit-storage-node  # subprocess on `network up` (default)
# SPACEKIT_EMBED_STORAGE=1      # keep storage in-process (legacy; uses much more RAM)

# ── Blockchain (optional; enable with `spacekit network up --full`) ──
# [blockchain]
# enabled = false
# chain_id = 31337
# block_time_ms = 10000          # local dev default; lower values increase CPU/RSS
# persist_interval_blocks = 100  # ledger.json flush cadence when persist_state = true
# epoch_length = 100
# persist_state = true
# # state_dir = ""  # default: ~/.spacekit/data/blockchain
#
# [blockchain.genesis]
# accounts = [
#   { did = "$YOUR_DID", balance = 1_000_000_000 },
# ]
# contracts = []
#
# [blockchain.validators]
# self_validate = true
# peers = []
# min_validators = 1
#
# [blockchain.rewards]
# operator_reward_per_block = 1000
# storage_reward_per_gb_epoch = 500
# compute_reward_per_gas_unit = 1
# settlement_interval_secs = 5
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    fn genesis_document() -> serde_json::Value {
        serde_json::json!({"accounts": [], "contracts": []})
    }

    fn public_manifest() -> NetworkManifest {
        static MANIFEST: std::sync::OnceLock<NetworkManifest> = std::sync::OnceLock::new();
        MANIFEST
            .get_or_init(|| {
                let document = genesis_document();
                let hash = blake3::hash(&canonical_json_bytes(&document).unwrap())
                    .to_hex()
                    .to_string();
                let mut manifest = NetworkManifest {
                    version: NETWORK_MANIFEST_VERSION,
                    network_id: "spacekit-main".into(),
                    profile: NetworkPreset::Public,
                    chain_id: 1,
                    protocol: ManifestProtocol {
                        name: NETWORK_PROTOCOL.into(),
                        version: NETWORK_PROTOCOL_VERSION,
                    },
                    genesis: ManifestGenesis {
                        hash,
                        uri: Some("https://example.net/genesis.json".into()),
                        document: Some(document),
                    },
                    bootstrap: ManifestBootstrap {
                        p2p: vec!["/dns4/bootstrap.example.net/tcp/4001".into()],
                        rpc: vec!["https://rpc.example.net".into()],
                    },
                    roles: vec![
                        NetworkRole::Subscriber,
                        NetworkRole::Operator,
                        NetworkRole::Validator,
                    ],
                    members: vec![ManifestMember {
                        did: "did:spacekit:operator".into(),
                        roles: vec![NetworkRole::Operator, NetworkRole::Validator],
                    }],
                    signature: None,
                };
                let payload = manifest.canonical_unsigned_bytes().unwrap();
                let (public_key, secret_key) =
                    spacekit_primitives::v1::crypto::quantum::generate_sphincs_keypair(
                        "sphincs-128f",
                    )
                    .unwrap();
                let signed = spacekit_primitives::v1::crypto::quantum::sign_sphincs_detached(
                    &payload,
                    "sphincs-128f",
                    &public_key,
                    &secret_key,
                )
                .unwrap();
                manifest.signature = Some(ManifestSignature {
                    algorithm: ManifestSignatureAlgorithm::Sphincs128f,
                    encoding: ManifestSignatureEncoding::Hex,
                    key_id: "did:spacekit:main#network-signing".into(),
                    public_key: hex::encode(public_key),
                    signature: hex::encode(signed.signature_bytes),
                    signed_at: None,
                });
                manifest
            })
            .clone()
    }

    #[test]
    fn profile_serialization_round_trip_preserves_v3_fields() {
        let profile = SpacekitNetworkFile::for_preset(NetworkPreset::Local);
        let encoded = toml::to_string(&profile).unwrap();
        let decoded: SpacekitNetworkFile = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.version, NETWORK_PROFILE_VERSION);
        assert_eq!(decoded.profile, NetworkPreset::Local);
        assert_eq!(decoded.role, NetworkRole::Validator);
        assert!(decoded.blockchain.persist_state);
    }

    #[test]
    fn presets_define_expected_trust_and_runtime_defaults() {
        let local = SpacekitNetworkFile::for_preset(NetworkPreset::Local);
        assert!(local.services.storage && local.services.messaging && local.services.compute);
        assert!(local.services.gateway);
        assert!(local.blockchain.enabled && local.blockchain.persist_state);
        assert!(local.blockchain.validators.self_validate);

        let private = SpacekitNetworkFile::for_preset(NetworkPreset::Private);
        assert!(private.runtime.enable_p2p);
        assert!(!private.blockchain.validators.self_validate);
        assert_eq!(private.role, NetworkRole::Operator);

        let public = SpacekitNetworkFile::for_preset(NetworkPreset::Public);
        assert!(public.runtime.enable_p2p);
        assert!(public.admission.require_signed_manifest);
        assert!(!public.admission.faucet_enabled);
        assert_eq!(public.role, NetworkRole::Subscriber);
    }

    #[test]
    fn manifest_serialization_and_canonical_payload_are_stable() {
        let manifest = public_manifest();
        manifest.validate().unwrap();
        manifest.verify_signature().unwrap();
        let json = serde_json::to_string_pretty(&manifest).unwrap();
        let decoded: NetworkManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, decoded);

        let unsigned = manifest.canonical_unsigned_bytes().unwrap();
        let mut resigned = manifest.clone();
        resigned.signature.as_mut().unwrap().signature = "cd".repeat(64);
        assert_eq!(unsigned, resigned.canonical_unsigned_bytes().unwrap());
        assert!(!String::from_utf8(unsigned).unwrap().contains("signature"));
    }

    #[test]
    fn public_manifest_requires_bootstrap_roles_and_signature() {
        let mut manifest = public_manifest();
        manifest.bootstrap.p2p.clear();
        manifest.bootstrap.rpc.clear();
        manifest.roles.clear();
        manifest.signature = None;
        let error = manifest.validate().unwrap_err().to_string();
        assert!(error.contains("bootstrap.p2p"));
        assert!(error.contains("bootstrap RPC"));
        assert!(error.contains("roles"));
        assert!(error.contains("signature metadata"));
    }

    #[test]
    fn signature_metadata_shape_is_validated() {
        let mut manifest = public_manifest();
        let signature = manifest.signature.as_mut().unwrap();
        signature.key_id = "bad key id".into();
        signature.signature = "not-hex".into();
        let error = manifest.validate().unwrap_err().to_string();
        assert!(error.contains("signature.key_id"));
        assert!(error.contains("even-length hexadecimal"));
    }

    #[test]
    fn signature_rejects_tampering_and_wrong_key() {
        let manifest = public_manifest();
        manifest.verify_signature().unwrap();

        let mut tampered = manifest.clone();
        tampered.chain_id += 1;
        assert!(tampered
            .verify_signature()
            .unwrap_err()
            .to_string()
            .contains("invalid"));

        let mut wrong_key = manifest;
        let (public_key, _) =
            spacekit_primitives::v1::crypto::quantum::generate_sphincs_keypair("sphincs-128f")
                .unwrap();
        wrong_key.signature.as_mut().unwrap().public_key = hex::encode(public_key);
        assert!(wrong_key.verify_signature().is_err());
    }

    #[test]
    fn manifest_rejects_wrong_genesis_and_protocol_version() {
        let mut manifest = public_manifest();
        manifest.genesis.document = Some(serde_json::json!({"tampered": true}));
        manifest.protocol.version += 1;
        let error = manifest.validate().unwrap_err().to_string();
        assert!(error.contains("genesis.document"));
        assert!(error.contains("protocol"));
    }

    #[test]
    fn profile_rejects_wrong_chain_and_admitted_genesis() {
        let manifest = public_manifest();
        let mut profile = SpacekitNetworkFile::for_preset(NetworkPreset::Public);
        profile.blockchain.chain_id = manifest.chain_id + 1;
        profile.admission.shared_genesis_hash = Some("0".repeat(64));
        let error = validate_manifest_compatibility(&profile, &manifest)
            .unwrap_err()
            .to_string();
        assert!(error.contains("chain_id"));
        assert!(error.contains("genesis"));
    }

    #[test]
    fn join_policy_allows_public_subscriber_but_gates_privileged_roles() {
        let manifest = public_manifest();
        validate_manifest_join(
            &manifest,
            "did:spacekit:any-subscriber",
            NetworkRole::Subscriber,
        )
        .unwrap();
        assert!(
            validate_manifest_join(&manifest, "did:spacekit:unknown", NetworkRole::Operator)
                .is_err()
        );
        validate_manifest_join(&manifest, "did:spacekit:operator", NetworkRole::Validator).unwrap();
    }

    #[test]
    fn private_profile_requires_allowlist_shared_genesis_and_bootstrap() {
        let profile = SpacekitNetworkFile::for_preset(NetworkPreset::Private);
        let error = profile.validate().unwrap_err().to_string();
        assert!(error.contains("bootstrap peer"));
        assert!(error.contains("admission.allowlist"));
        assert!(error.contains("shared_genesis_hash"));
    }

    #[test]
    fn port_collisions_are_rejected() {
        let mut profile = SpacekitNetworkFile::for_preset(NetworkPreset::Local);
        profile.ports.compute_http = profile.ports.storage_http;
        let error = profile.validate().unwrap_err().to_string();
        assert!(error.contains("storage_http"));
        assert!(error.contains("compute_http"));
        assert!(error.contains("3030"));
    }

    #[test]
    fn three_local_nodes_have_unique_ports_and_directories() {
        let root = std::env::temp_dir().join("spacekit-three-node-test");
        let nodes: Vec<_> = [("node-a", 0), ("node-b", 20_000), ("node-c", 40_000)]
            .into_iter()
            .map(|(node_id, port_offset)| {
                network_file_from_init(NetworkInitOptions {
                    profile: NetworkPreset::Local,
                    node_id: Some(node_id.into()),
                    port_offset,
                    data_root: Some(root.clone()),
                    ..NetworkInitOptions::default()
                })
            })
            .collect();
        for node in &nodes {
            node.validate().unwrap();
        }
        let mut all_ports = std::collections::HashSet::new();
        for node in &nodes {
            for (_, port) in node.ports.allocations() {
                assert!(
                    all_ports.insert(port),
                    "cross-node port collision on {port}"
                );
            }
        }
        assert_ne!(nodes[0].ports.storage_http, nodes[1].ports.storage_http);
        assert_ne!(nodes[1].ports.compute_http, nodes[2].ports.compute_http);
        assert_ne!(nodes[0].data.storage, nodes[2].data.storage);
    }

    #[test]
    fn private_unknown_member_and_ungranted_validator_are_rejected_before_start() {
        let mut profile = SpacekitNetworkFile::for_preset(NetworkPreset::Private);
        profile.messaging.bootstrap_peers = vec!["/ip4/127.0.0.1/tcp/7000".into()];
        profile.admission.shared_genesis_hash = Some("a".repeat(64));
        profile.admission.allowlist = vec!["did:spacekit:member".into()];
        assert!(authorize_network_start(&profile, "did:spacekit:unknown").is_err());
        profile.role = NetworkRole::Validator;
        assert!(authorize_network_start(&profile, "did:spacekit:member").is_err());
        profile
            .blockchain
            .validators
            .peers
            .push("did:spacekit:member".into());
        authorize_network_start(&profile, "did:spacekit:member").unwrap();
    }

    #[test]
    fn repository_examples_deserialize_and_validate() {
        for body in [
            include_str!("../configs/network-local.toml"),
            include_str!("../configs/network-private.toml"),
            include_str!("../configs/network-public.toml"),
        ] {
            let mut profile: SpacekitNetworkFile = toml::from_str(body).unwrap();
            profile.normalize_after_load();
            profile.validate().unwrap();
        }
        for body in [
            include_str!("../configs/network-private.manifest.json"),
            include_str!("../configs/network-public.manifest.json"),
        ] {
            let manifest: NetworkManifest = serde_json::from_str(body).unwrap();
            manifest.validate().unwrap();
        }
    }
}
