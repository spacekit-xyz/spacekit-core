use serde::{Deserialize, Serialize};

pub const SUITE: &str = "SKKM-1";
pub const SSS_N: u8 = 5;
pub const SSS_T: u8 = 3;

pub type Hex32 = String;
pub type B64 = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub suite: String,
    pub keystore_id: Hex32,
    pub shard_index: u32,
    pub n: u32,
    pub t: u32,
    pub subject: Hex32,
    pub guardian_kid: Hex32,
    pub kem_ct: B64,
    pub nonce: B64,
    pub ct: B64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SealedBlob {
    pub v: u8,
    pub suite: String,
    pub keystore_id: Hex32,
    pub subject: Hex32,
    pub nonce: B64,
    pub ct: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placement {
    pub node_did: String,
    pub object_id: Hex32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardEntry {
    pub index: u32,
    pub guardian_kid: Hex32,
    pub shard_id: Hex32,
    pub placements: Vec<Placement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PlacementGrade {
    Full,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestPolicy {
    pub recovery_auth: Vec<String>,
    pub cooldown_s: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub v: u8,
    pub subject: Hex32,
    pub keystore_id: Hex32,
    pub blob_ref: BlobRef,
    pub shards: Vec<ShardEntry>,
    pub placement_grade: PlacementGrade,
    pub policy: ManifestPolicy,
    pub created_at: i64,
    #[serde(default)]
    pub sig: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobRef {
    pub placements: Vec<Placement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianInfo {
    pub kid: Hex32,
    pub mlkem_pk: B64,
    pub endpoint: String,
    pub operator: String,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptAuth {
    pub user_sig: B64,
    pub coordinator_ticket: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptRequest {
    pub session_id: Hex32,
    pub subject: Hex32,
    pub keystore_id: Hex32,
    pub shard_index: u32,
    pub envelope: Envelope,
    pub client_eph_pk: B64,
    pub auth: DecryptAuth,
    #[serde(default)]
    pub break_glass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecryptResponse {
    pub reshard_kem_ct: B64,
    pub reshard_nonce: B64,
    pub reshard_ct: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardHealth {
    pub index: u32,
    pub verified: bool,
    pub last_checked: Option<i64>,
    pub placements_ok: u32,
    pub placements_total: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageStatus {
    pub covered: bool,
    pub placement_grade: Option<PlacementGrade>,
    pub shards: Vec<ShardHealth>,
    pub blob_replicas_ok: u32,
    pub blob_replicas_total: u32,
    pub enrolled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlaTier {
    Shield,
    ShieldAnnual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlaQuote {
    pub subject: Hex32,
    pub tier: SlaTier,
    pub tier_label: String,
    pub token: String,
    pub amount_display: String,
    pub duration_days: u32,
    pub valid_until: i64,
    pub quote_sig: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementStatus {
    pub active: bool,
    pub paid_until: Option<i64>,
    pub tier: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuditDecision {
    Granted,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    pub ts: i64,
    pub session_id: Hex32,
    pub guardian_kid: Hex32,
    pub shard_index: u32,
    pub decision: AuditDecision,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_sig: Option<B64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubjectIdentity {
    pub subject: Hex32,
    pub account_id: String,
    pub signer_pk_b64: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartRecoveryResponse {
    pub session_id: Hex32,
    pub tickets: std::collections::HashMap<Hex32, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePutRequest {
    pub bytes_b64: B64,
    pub placements: Vec<Placement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoragePutResponse {
    pub object_ids: Vec<Hex32>,
}
