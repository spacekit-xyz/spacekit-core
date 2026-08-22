use anyhow::Result;

use crate::types::{Manifest, PlacementGrade};

/// Canonical JSON bytes used for ML-DSA manifest signatures (matches coordinator verify).
pub fn manifest_body_json(m: &Manifest) -> Result<String> {
    #[derive(serde::Serialize)]
    struct Body<'a> {
        v: u8,
        subject: &'a str,
        keystore_id: &'a str,
        blob_ref: &'a crate::types::BlobRef,
        shards: &'a [crate::types::ShardEntry],
        placement_grade: &'a PlacementGrade,
        policy: &'a crate::types::ManifestPolicy,
        created_at: i64,
    }
    Ok(serde_json::to_string(&Body {
        v: m.v,
        subject: &m.subject,
        keystore_id: &m.keystore_id,
        blob_ref: &m.blob_ref,
        shards: &m.shards,
        placement_grade: &m.placement_grade,
        policy: &m.policy,
        created_at: m.created_at,
    })?)
}

pub fn sign_manifest_body(sk: &[u8], body: &Manifest) -> Result<Vec<u8>> {
    let body_json = manifest_body_json(body)?;
    crate::pq_crypto::sign(sk, body_json.as_bytes())
}

pub fn verify_manifest(manifest: &Manifest, signer_pk_b64: &str) -> Result<()> {
    let body_json = manifest_body_json(manifest)?;
    crate::auth::verify_manifest_sig(&body_json, signer_pk_b64, &manifest.sig)
}
