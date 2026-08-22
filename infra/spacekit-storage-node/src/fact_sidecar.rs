//! Externalize large binary fact payloads to `{fact_id}.blob` sidecars so
//! `{fact_id}.json` stays metadata-sized (GET /facts does not load hundreds of MB).

use anyhow::{Context, Result};
use spacekit_primitives::v1::fact::{FactContent, FactPackage};
use std::path::{Path, PathBuf};
use tracing::info;

use crate::stream_mime::{is_generic_stream_mime, resolve_stream_mime_for_fact};

/// Inline binary payloads above this size are moved to a `.blob` sidecar on read or write.
pub const EXTERNALIZE_BINARY_THRESHOLD: usize = 256 * 1024;

pub fn fact_dir(data_dir: &Path, fact_id_hex: &str) -> PathBuf {
    let prefix = fact_id_hex.get(..2).unwrap_or("00");
    data_dir.join("facts").join(prefix)
}

pub fn fact_json_path(data_dir: &Path, fact_id_hex: &str) -> PathBuf {
    fact_dir(data_dir, fact_id_hex).join(format!("{fact_id_hex}.json"))
}

pub fn fact_blob_path(data_dir: &Path, fact_id_hex: &str) -> PathBuf {
    fact_dir(data_dir, fact_id_hex).join(format!("{fact_id_hex}.blob"))
}

pub fn fact_blob_meta_path(data_dir: &Path, fact_id_hex: &str) -> PathBuf {
    fact_dir(data_dir, fact_id_hex).join(format!("{fact_id_hex}.blob.meta"))
}

pub fn blob_sidecar_exists(data_dir: &Path, fact_id_hex: &str) -> bool {
    fact_blob_path(data_dir, fact_id_hex).exists()
}

/// Strip inline bytes from a binary fact once the blob sidecar is on disk.
pub fn slim_binary_fact(fact: &mut FactPackage) {
    if let FactContent::Binary { data, .. } = &mut fact.content {
        data.clear();
    }
}

/// Prepare a fact for JSON persistence: write blob sidecar for large binary bodies.
pub async fn persist_fact_with_sidecar(data_dir: &Path, fact: &FactPackage) -> Result<()> {
    let fact_id_hex = hex::encode(fact.fact_id);
    let dir = fact_dir(data_dir, &fact_id_hex);
    tokio::fs::create_dir_all(&dir).await?;

    let mut stored = fact.clone();
    if let FactContent::Binary { ref data, .. } = fact.content {
        if data.len() >= EXTERNALIZE_BINARY_THRESHOLD {
            let blob_path = fact_blob_path(data_dir, &fact_id_hex);
            tokio::fs::write(&blob_path, data)
                .await
                .with_context(|| format!("write fact blob sidecar {:?}", blob_path))?;
            let meta_path = fact_blob_meta_path(data_dir, &fact_id_hex);
            let effective_mime = resolve_stream_mime_for_fact(fact);
            let _ = tokio::fs::write(&meta_path, effective_mime.as_bytes()).await;
            slim_binary_fact(&mut stored);
        }
    }

    let json_path = fact_json_path(data_dir, &fact_id_hex);
    let serialized = serde_json::to_vec(&stored)?;
    tokio::fs::write(&json_path, &serialized)
        .await
        .with_context(|| format!("write fact json {:?}", json_path))?;
    Ok(())
}

/// One-time migration: extract embedded binary from a legacy oversized JSON fact.
pub async fn ensure_fact_externalized(data_dir: &Path, fact_id_hex: &str) -> Result<bool> {
    if blob_sidecar_exists(data_dir, fact_id_hex) {
        return maybe_reslim_json(data_dir, fact_id_hex).await;
    }

    let json_path = fact_json_path(data_dir, fact_id_hex);
    let meta = match tokio::fs::metadata(&json_path).await {
        Ok(m) => m,
        Err(_) => return Ok(false),
    };
    if meta.len() < EXTERNALIZE_BINARY_THRESHOLD as u64 {
        return Ok(false);
    }

    let raw = tokio::fs::read(&json_path).await?;
    let mut fact: FactPackage = serde_json::from_slice(&raw)
        .with_context(|| format!("parse legacy fact json {:?}", json_path))?;

    let FactContent::Binary { ref data, .. } = fact.content else {
        return Ok(false);
    };

    if data.len() < EXTERNALIZE_BINARY_THRESHOLD {
        return Ok(false);
    }

    let data_len = data.len();
    let blob_path = fact_blob_path(data_dir, fact_id_hex);
    tokio::fs::write(&blob_path, data).await?;
    let meta_path = fact_blob_meta_path(data_dir, fact_id_hex);
    let effective_mime = resolve_stream_mime_for_fact(&fact);
    let _ = tokio::fs::write(&meta_path, effective_mime.as_bytes()).await;

    slim_binary_fact(&mut fact);
    tokio::fs::write(&json_path, serde_json::to_vec(&fact)?).await?;
    info!(
        "Externalized fact {} ({} bytes binary → blob sidecar)",
        fact_id_hex, data_len
    );
    Ok(true)
}

/// Resolve Content-Type for `GET /facts/{id}/stream`, inferring from JSON metadata when
/// `.blob.meta` is missing or still `application/octet-stream` (legacy app deploys).
pub async fn resolve_fact_stream_mime(data_dir: &Path, fact_id_hex: &str) -> String {
    let meta_path = fact_blob_meta_path(data_dir, fact_id_hex);
    if let Ok(raw) = tokio::fs::read_to_string(&meta_path).await {
        let trimmed = raw.trim();
        if !is_generic_stream_mime(trimmed) {
            return trimmed.to_string();
        }
    }

    match read_fact_json(data_dir, fact_id_hex).await {
        Ok(fact) => {
            let resolved = resolve_stream_mime_for_fact(&fact);
            repair_blob_meta_if_needed(data_dir, fact_id_hex, &resolved).await;
            resolved
        }
        Err(_) => "application/octet-stream".to_string(),
    }
}

async fn repair_blob_meta_if_needed(data_dir: &Path, fact_id_hex: &str, mime: &str) {
    if !blob_sidecar_exists(data_dir, fact_id_hex) || is_generic_stream_mime(mime) {
        return;
    }
    let meta_path = fact_blob_meta_path(data_dir, fact_id_hex);
    let needs_repair = match tokio::fs::read_to_string(&meta_path).await {
        Ok(existing) => is_generic_stream_mime(existing.trim()),
        Err(_) => true,
    };
    if needs_repair {
        let _ = tokio::fs::write(&meta_path, mime.as_bytes()).await;
    }
}

/// Load fact metadata JSON (call `ensure_fact_externalized` first for legacy oversized facts).
pub async fn read_fact_json(data_dir: &Path, fact_id_hex: &str) -> Result<FactPackage> {
    let path = fact_json_path(data_dir, fact_id_hex);
    let raw = tokio::fs::read(&path).await?;
    serde_json::from_slice(&raw).with_context(|| format!("parse fact {:?}", path))
}

async fn maybe_reslim_json(data_dir: &Path, fact_id_hex: &str) -> Result<bool> {
    let json_path = fact_json_path(data_dir, fact_id_hex);
    let meta = tokio::fs::metadata(&json_path).await?;
    if meta.len() < EXTERNALIZE_BINARY_THRESHOLD as u64 {
        return Ok(false);
    }
    let raw = tokio::fs::read(&json_path).await?;
    let mut fact: FactPackage = match serde_json::from_slice(&raw) {
        Ok(f) => f,
        Err(_) => return Ok(false),
    };
    let before = match &fact.content {
        FactContent::Binary { data, .. } => data.len(),
        _ => return Ok(false),
    };
    if before < EXTERNALIZE_BINARY_THRESHOLD {
        return Ok(false);
    }
    slim_binary_fact(&mut fact);
    tokio::fs::write(&json_path, serde_json::to_vec(&fact)?).await?;
    info!(
        "Re-slimmed fact {} JSON (dropped {} inline bytes)",
        fact_id_hex, before
    );
    Ok(true)
}
