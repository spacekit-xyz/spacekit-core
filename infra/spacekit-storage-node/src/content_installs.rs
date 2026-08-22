//! Per-user content installs in the storage-node database.
//!
//! After `content view`, the CLI materializes bytes and records an install document
//! (`collection = content_installs`) keyed by requester DID + content id, linked to
//! the active grant / entitlement so agent commands can resolve the executable path.

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::content_grants::ContentGrantStore;
use crate::database::DocumentRecord;
use crate::Database;

pub const COLLECTION: &str = "content_installs";

/// How entitled CLI runs resolve this install.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentInstallRuntime {
    /// Run via growformer embedded in the `spacekit` binary (entitlement from DB).
    #[default]
    EmbeddedGrowformer,
    /// Run a materialized file on disk (non-growformer apps).
    MaterializedFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentInstall {
    pub content_id_hex: String,
    /// Fact storage / DB reference, or on-disk path when `runtime = materialized_file`.
    pub materialized_path: String,
    pub filename: String,
    /// Default `embedded_growformer` for legacy installs missing this field.
    #[serde(default)]
    pub runtime: ContentInstallRuntime,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entitlement_id_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    pub size_bytes: u64,
    pub installed_at: u64,
}

pub fn app_slug_from_tags(tags: &[String]) -> Option<String> {
    tags.iter()
        .find_map(|t| t.strip_prefix("app:"))
        .map(|s| s.to_string())
}

pub fn grant_entitlement_for_content(
    grants: &ContentGrantStore,
    requester_did: &str,
    content_id_hex: &str,
) -> Option<(Option<String>, Option<String>)> {
    grants
        .list_for_requester(requester_did)
        .ok()?
        .into_iter()
        .find(|g| g.content_id_hex.as_deref() == Some(content_id_hex))
        .map(|g| (g.entitlement_id_hex.clone(), g.tier.clone()))
}

impl ContentInstall {
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.materialized_path)
    }
}

pub fn register_install(
    db: &Database,
    requester_did: &str,
    install: &ContentInstall,
) -> Result<()> {
    let now = Utc::now();
    let doc = DocumentRecord {
        owner_did: requester_did.to_string(),
        collection: COLLECTION.to_string(),
        id: install.content_id_hex.clone(),
        data: serde_json::to_value(install)?,
        created_at: now,
        updated_at: now,
        blob_ref: None,
    };
    db.upsert_document(&doc)
}

pub fn get_install(
    db: &Database,
    requester_did: &str,
    content_id_hex: &str,
) -> Result<Option<ContentInstall>> {
    let doc = db.get_document(requester_did, COLLECTION, content_id_hex)?;
    match doc {
        Some(d) => Ok(Some(serde_json::from_value(d.data)?)),
        None => Ok(None),
    }
}

pub fn list_installs(db: &Database, requester_did: &str) -> Result<Vec<ContentInstall>> {
    Ok(db
        .list_documents(requester_did, COLLECTION)?
        .into_iter()
        .filter_map(|d| serde_json::from_value(d.data).ok())
        .collect())
}

pub fn find_install_by_app_slug(
    db: &Database,
    requester_did: &str,
    app_slug: &str,
) -> Result<Option<ContentInstall>> {
    let installs = list_installs(db, requester_did)?;
    if let Some(found) = installs.iter().find(|i| {
        i.app_slug.as_deref() == Some(app_slug) || i.filename.eq_ignore_ascii_case(app_slug)
    }) {
        return Ok(Some(found.clone()));
    }
    // Single install + growformer slug: pre-tag publishes used {content_id}.bin only
    if app_slug.eq_ignore_ascii_case("growformer") && installs.len() == 1 {
        return Ok(installs.into_iter().next());
    }
    Ok(None)
}

/// Load the materialized executable path from the install record (access must be checked separately).
pub fn resolve_installed_executable(
    db: &Database,
    requester_did: &str,
    content_id_hex: &str,
) -> Result<PathBuf> {
    let install = get_install(db, requester_did, content_id_hex)?.ok_or_else(|| {
        anyhow!(
            "content not installed — run: spacekit content view --content-id {}",
            content_id_hex
        )
    })?;
    if is_growformer_install(&install) {
        return Err(anyhow!(
            "growformer uses embedded runtime — run: spacekit agent --app growformer exec -- …"
        ));
    }
    let path = install.path();
    if !path.is_file() {
        return Err(anyhow!(
            "installed binary missing at {} — re-run content view",
            path.display()
        ));
    }
    Ok(path)
}

pub fn storage_fact_reference(content_id_hex: &str) -> String {
    format!("storage:fact/{content_id_hex}")
}

/// Published growformer payload heuristic (Mach-O/Linux binary with CLI branding).
pub fn detect_growformer_payload(data: &[u8]) -> bool {
    if data.len() < 500_000 {
        return false;
    }
    data.windows(10)
        .any(|w| w.eq_ignore_ascii_case(b"Growformer"))
}

pub fn is_known_growformer_content_id(content_id_hex: &str) -> bool {
    std::env::var("GROWFORMER_CONTENT_ID")
        .ok()
        .is_some_and(|id| id.trim().eq_ignore_ascii_case(content_id_hex))
}

pub fn is_growformer_install(install: &ContentInstall) -> bool {
    install.runtime == ContentInstallRuntime::EmbeddedGrowformer
        || install.app_slug.as_deref() == Some("growformer")
        || install.filename.eq_ignore_ascii_case("growformer")
        || install.materialized_path.starts_with("storage:fact/")
}

pub fn should_use_embedded_growformer(
    content_id_hex: &str,
    app_flag: Option<&str>,
    install: Option<&ContentInstall>,
    payload: Option<&[u8]>,
) -> bool {
    if app_flag.is_some_and(|a| a.eq_ignore_ascii_case("growformer")) {
        return true;
    }
    if is_known_growformer_content_id(content_id_hex) {
        return true;
    }
    if payload.is_some_and(|p| {
        detect_growformer_payload(p) || crate::licensed_feature::is_growformer_feature_json_bytes(p)
    }) {
        return true;
    }
    if let Some(i) = install {
        if is_growformer_install(i) {
            return true;
        }
    }
    false
}

pub fn build_install_record(
    content_id_hex: &str,
    materialized_path: &str,
    filename: &str,
    size_bytes: u64,
    app_slug: Option<String>,
    entitlement_id_hex: Option<String>,
    tier: Option<String>,
    runtime: ContentInstallRuntime,
) -> ContentInstall {
    ContentInstall {
        content_id_hex: content_id_hex.to_string(),
        materialized_path: materialized_path.to_string(),
        filename: filename.to_string(),
        runtime,
        app_slug,
        entitlement_id_hex,
        tier,
        size_bytes,
        installed_at: chrono::Utc::now().timestamp() as u64,
    }
}
