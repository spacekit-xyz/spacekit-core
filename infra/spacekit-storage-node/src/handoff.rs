//! Signed workspace export attestations for federation handoff (Phase 3).

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use std::path::Path;

use crate::workspace::WorkspaceExportBundle;

const HANDOFF_SECRET_FILE: &str = ".handoff_secret";

fn derive_mac_key(secret: &[u8]) -> [u8; 32] {
    blake3::derive_key("spacekit-workspace-handoff-v1", secret)
}

/// Load HMAC secret: `SPACEKIT_HANDOFF_SECRET`, else upload-token secret, else files.
pub fn load_handoff_secret(data_dir: Option<&Path>) -> Option<Vec<u8>> {
    if let Ok(s) = std::env::var("SPACEKIT_HANDOFF_SECRET") {
        let t = s.trim();
        if !t.is_empty() {
            return Some(crate::upload_token::normalize_secret_bytes(t));
        }
    }
    if let Some(s) = crate::upload_token::load_signing_secret(data_dir) {
        return Some(s);
    }
    let dir = data_dir?;
    let path = dir.join(HANDOFF_SECRET_FILE);
    let raw = std::fs::read_to_string(&path).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(crate::upload_token::normalize_secret_bytes(t))
    }
}

pub fn handoff_signature_required() -> bool {
    std::env::var("SPACEKIT_REQUIRE_HANDOFF_SIGNATURE")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Canonical export bytes used for signing (excludes attestations added after the core export).
pub fn export_signing_bytes(bundle: &WorkspaceExportBundle) -> Result<Vec<u8>> {
    let mut unsigned = bundle.clone();
    unsigned.handoff_signature = None;
    unsigned.migration_manifest = None;
    Ok(serde_json::to_vec(&unsigned)?)
}

pub fn sign_export_bundle(secret: &[u8], bundle: &mut WorkspaceExportBundle) -> Result<()> {
    let bytes = export_signing_bytes(bundle)?;
    let mac = blake3::keyed_hash(&derive_mac_key(secret), &bytes);
    bundle.handoff_signature = Some(hex::encode(mac.as_bytes()));
    Ok(())
}

pub fn verify_export_bundle(secret: &[u8], bundle: &WorkspaceExportBundle) -> Result<bool> {
    let Some(sig_hex) = bundle.handoff_signature.as_ref() else {
        return Ok(false);
    };
    let expected = blake3::keyed_hash(&derive_mac_key(secret), &export_signing_bytes(bundle)?);
    let actual = hex::decode(sig_hex).map_err(|e| anyhow!("invalid handoff_signature hex: {e}"))?;
    Ok(actual.as_slice() == expected.as_bytes())
}

pub fn validate_import_bundle(secret: Option<&[u8]>, bundle: &WorkspaceExportBundle) -> Result<()> {
    let required = handoff_signature_required();
    let has_sig = bundle.handoff_signature.is_some();
    if required && !has_sig {
        return Err(anyhow!(
            "handoff_signature required (SPACEKIT_REQUIRE_HANDOFF_SIGNATURE)"
        ));
    }
    if has_sig {
        let secret =
            secret.ok_or_else(|| anyhow!("handoff signing not configured on this node"))?;
        if !verify_export_bundle(secret, bundle)? {
            return Err(anyhow!("invalid handoff_signature"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::{WorkspaceContent, WorkspaceStatus};

    #[test]
    fn sign_and_verify_roundtrip() {
        let secret = b"handoff-test-secret";
        let mut bundle = WorkspaceExportBundle {
            schema: crate::workspace::SCHEMA_WORKSPACE_V1.to_string(),
            fact_id: "ab".repeat(32),
            owner_did: "did:spacekit:src".into(),
            workspace_id: "ws".into(),
            content: WorkspaceContent {
                workspace_id: "ws".into(),
                owner_did: "did:spacekit:src".into(),
                collaborators: vec![],
                associated_repos: vec![],
                quotas: Default::default(),
                default_access_policy: spacekit_primitives::v1::fact::AccessPolicy::Public,
                status: WorkspaceStatus::Active,
                created_at: 1,
                updated_at: 1,
            },
            exported_at: 100,
            referenced_blob_hashes: vec![],
            handoff_signature: None,
            migration_manifest: None,
        };
        sign_export_bundle(secret, &mut bundle).unwrap();
        assert!(verify_export_bundle(secret, &bundle).unwrap());
        bundle.content.workspace_id = "tampered".into();
        assert!(!verify_export_bundle(secret, &bundle).unwrap());
    }
}
