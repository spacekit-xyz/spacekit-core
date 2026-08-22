use parking_lot::RwLock;

use crate::auth::sign_audit_record;
use crate::types::{AuditDecision, AuditRecord, Hex32};
use zeroize::Zeroizing;

pub struct AuditLog {
    records: RwLock<Vec<AuditRecord>>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self {
            records: RwLock::new(Vec::new()),
        }
    }

    pub fn list(&self, subject_filter: Option<&str>) -> Vec<AuditRecord> {
        self.records
            .read()
            .iter()
            .rev()
            .cloned()
            .filter(|_r| {
                subject_filter.map(|_| true).unwrap_or(true)
            })
            .collect()
    }
}

pub fn append_audit(
    log: &AuditLog,
    sk: &Zeroizing<Vec<u8>>,
    session_id: &str,
    guardian_kid: &str,
    shard_index: u32,
    decision: AuditDecision,
    reason: Option<&str>,
) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let mut rec = AuditRecord {
        ts,
        session_id: session_id.to_string(),
        guardian_kid: guardian_kid.to_string(),
        shard_index,
        decision,
        reason: reason.map(str::to_string),
        record_sig: None,
    };
    if let Ok(payload) = serde_json::to_string(&serde_json::json!({
        "ts": rec.ts,
        "session_id": rec.session_id,
        "guardian_kid": rec.guardian_kid,
        "shard_index": rec.shard_index,
        "decision": rec.decision,
        "reason": rec.reason,
    })) {
        if let Ok(sig) = sign_audit_record(&payload, sk) {
            rec.record_sig = Some(sig);
        }
    }
    log.records.write().push(rec);
}
