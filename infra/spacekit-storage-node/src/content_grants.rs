//! Local content access grants (pay-per-view + channel subscriptions).
//!
//! MVP persistence until full on-chain entitlement-ledger / AppLicenseNFT wiring.
//! Grants live under `{data_dir}/content_grants/`.

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const GRANTS_FILE: &str = "grants.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GrantKind {
    ContentPpv,
    ChannelSubscription,
    /// DID keychain delegation synced from website-api (`/api/account/keychain`).
    KeychainDelegate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentGrant {
    pub kind: GrantKind,
    pub requester_did: String,
    pub content_id_hex: Option<String>,
    pub channel_did: Option<String>,
    pub granted_at: u64,
    pub expires_at: Option<u64>,
    pub payment_reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entitlement_id_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    /// AppLicenseNFT token id when minted on purchase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_token_id: Option<u64>,
    /// Remaining growformer operations for quota-tracked tiers (`None` = unlimited).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_remaining: Option<u64>,
    /// Keychain: grant id + granter for delegate grants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub granter_did: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scopes: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_file_ids: Option<Vec<String>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct GrantStoreFile {
    grants: Vec<ContentGrant>,
}

pub struct ContentGrantStore {
    path: PathBuf,
}

impl ContentGrantStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("content_grants").join(GRANTS_FILE),
        }
    }

    pub fn from_env_or_data_dir(data_dir: &Path) -> Self {
        if let Ok(p) = std::env::var("SPACEKIT_CONTENT_GRANTS_FILE") {
            if !p.trim().is_empty() {
                return Self {
                    path: PathBuf::from(p),
                };
            }
        }
        Self::new(data_dir)
    }

    fn load(&self) -> Result<GrantStoreFile> {
        if !self.path.exists() {
            return Ok(GrantStoreFile::default());
        }
        let raw = std::fs::read_to_string(&self.path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    fn save(&self, store: &GrantStoreFile) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(store)?)?;
        Ok(())
    }

    fn is_active(g: &ContentGrant, now: u64) -> bool {
        g.expires_at.map(|e| now <= e).unwrap_or(true)
    }

    pub fn grant_content_ppv(
        &self,
        requester_did: &str,
        content_id_hex: &str,
        payment_reference: Option<String>,
        expires_at: Option<u64>,
    ) -> Result<()> {
        self.grant_content_ppv_full(
            requester_did,
            content_id_hex,
            payment_reference,
            expires_at,
            None,
            None,
            None,
            None,
        )
    }

    pub fn grant_content_ppv_full(
        &self,
        requester_did: &str,
        content_id_hex: &str,
        payment_reference: Option<String>,
        expires_at: Option<u64>,
        entitlement_id_hex: Option<String>,
        tier: Option<String>,
        license_token_id: Option<u64>,
        quota_remaining: Option<u64>,
    ) -> Result<()> {
        let mut store = self.load()?;
        let now = chrono::Utc::now().timestamp() as u64;
        store.grants.retain(|g| {
            !(g.kind == GrantKind::ContentPpv
                && g.requester_did == requester_did
                && g.content_id_hex.as_deref() == Some(content_id_hex))
        });
        store.grants.push(ContentGrant {
            kind: GrantKind::ContentPpv,
            requester_did: requester_did.to_string(),
            content_id_hex: Some(content_id_hex.to_string()),
            channel_did: None,
            granted_at: now,
            expires_at,
            payment_reference,
            entitlement_id_hex,
            tier,
            license_token_id,
            quota_remaining,
            grant_id: None,
            granter_did: None,
            resource_type: None,
            resource_id: None,
            scopes: None,
            artifact_file_ids: None,
        });
        self.save(&store)
    }

    /// Decrement quota for an active content grant (growformer licensed-feature tiers).
    pub fn consume_content_quota(
        &self,
        requester_did: &str,
        content_id_hex: &str,
    ) -> Result<Option<u64>> {
        let mut store = self.load()?;
        let now = chrono::Utc::now().timestamp() as u64;
        let grant = store.grants.iter_mut().find(|g| {
            g.kind == GrantKind::ContentPpv
                && g.requester_did == requester_did
                && g.content_id_hex.as_deref() == Some(content_id_hex)
                && Self::is_active(g, now)
        });
        let Some(grant) = grant else {
            return Ok(None);
        };
        let Some(remaining) = grant.quota_remaining.as_mut() else {
            return Ok(None);
        };
        if *remaining == 0 {
            return Err(anyhow!("growformer quota exhausted for this tier"));
        }
        *remaining -= 1;
        let left = *remaining;
        self.save(&store)?;
        Ok(Some(left))
    }

    pub fn quota_remaining_for_content(
        &self,
        requester_did: &str,
        content_id_hex: &str,
    ) -> Option<u64> {
        let now = chrono::Utc::now().timestamp() as u64;
        self.load()
            .ok()?
            .grants
            .into_iter()
            .find(|g| {
                g.kind == GrantKind::ContentPpv
                    && g.requester_did == requester_did
                    && g.content_id_hex.as_deref() == Some(content_id_hex)
                    && Self::is_active(g, now)
            })
            .and_then(|g| g.quota_remaining)
    }

    /// Extend or recreate PPV access (renewal). If expired or `--tier` changes, replaces the grant.
    pub fn renew_content_ppv(
        &self,
        requester_did: &str,
        content_id_hex: &str,
        extend_secs: u64,
        tier: Option<String>,
        payment_reference: Option<String>,
    ) -> Result<ContentGrant> {
        let now = chrono::Utc::now().timestamp() as u64;
        let store = self.load()?;
        let existing = store.grants.iter().find(|g| {
            g.kind == GrantKind::ContentPpv
                && g.requester_did == requester_did
                && g.content_id_hex.as_deref() == Some(content_id_hex)
        });
        let (new_expires, ent_id, quota) = match existing {
            Some(g) if Self::is_active(g, now) && tier.as_deref() == g.tier.as_deref() => {
                let base = g.expires_at.unwrap_or(now);
                (
                    Some(base.saturating_add(extend_secs)),
                    g.entitlement_id_hex.clone(),
                    g.quota_remaining,
                )
            }
            Some(g) => (
                Some(now.saturating_add(extend_secs)),
                g.entitlement_id_hex.clone(),
                g.quota_remaining,
            ),
            None => (Some(now.saturating_add(extend_secs)), None, None),
        };
        self.grant_content_ppv_full(
            requester_did,
            content_id_hex,
            payment_reference,
            new_expires,
            ent_id,
            tier,
            existing.and_then(|g| g.license_token_id),
            quota,
        )?;
        Ok(self
            .load()?
            .grants
            .into_iter()
            .find(|g| {
                g.kind == GrantKind::ContentPpv
                    && g.requester_did == requester_did
                    && g.content_id_hex.as_deref() == Some(content_id_hex)
            })
            .unwrap())
    }

    pub fn grant_channel_subscription(
        &self,
        requester_did: &str,
        channel_did: &str,
        expires_at: Option<u64>,
        payment_reference: Option<String>,
    ) -> Result<()> {
        let mut store = self.load()?;
        let now = chrono::Utc::now().timestamp() as u64;
        store.grants.retain(|g| {
            !(g.kind == GrantKind::ChannelSubscription
                && g.requester_did == requester_did
                && g.channel_did.as_deref() == Some(channel_did))
        });
        store.grants.push(ContentGrant {
            kind: GrantKind::ChannelSubscription,
            requester_did: requester_did.to_string(),
            content_id_hex: None,
            channel_did: Some(channel_did.to_string()),
            granted_at: now,
            expires_at,
            payment_reference,
            entitlement_id_hex: None,
            tier: None,
            license_token_id: None,
            quota_remaining: None,
            grant_id: None,
            granter_did: None,
            resource_type: None,
            resource_id: None,
            scopes: None,
            artifact_file_ids: None,
        });
        self.save(&store)
    }

    pub fn renew_channel_subscription(
        &self,
        requester_did: &str,
        channel_did: &str,
        extend_secs: u64,
        tier: Option<String>,
        payment_reference: Option<String>,
    ) -> Result<()> {
        let now = chrono::Utc::now().timestamp() as u64;
        let store = self.load()?;
        let existing = store.grants.iter().find(|g| {
            g.kind == GrantKind::ChannelSubscription
                && g.requester_did == requester_did
                && g.channel_did.as_deref() == Some(channel_did)
        });
        let new_expires = match existing {
            Some(g) if Self::is_active(g, now) => {
                g.expires_at.unwrap_or(now).saturating_add(extend_secs)
            }
            _ => now.saturating_add(extend_secs),
        };
        self.grant_channel_subscription(
            requester_did,
            channel_did,
            Some(new_expires),
            payment_reference,
        )
    }

    pub fn has_content_grant(&self, requester_did: &str, content_id_hex: &str) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        self.load()
            .ok()
            .map(|s| {
                s.grants.iter().any(|g| {
                    g.kind == GrantKind::ContentPpv
                        && g.requester_did == requester_did
                        && g.content_id_hex.as_deref() == Some(content_id_hex)
                        && Self::is_active(g, now)
                })
            })
            .unwrap_or(false)
    }

    pub fn has_channel_subscription(&self, requester_did: &str, channel_did: &str) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        self.load()
            .ok()
            .map(|s| {
                s.grants.iter().any(|g| {
                    g.kind == GrantKind::ChannelSubscription
                        && g.requester_did == requester_did
                        && g.channel_did.as_deref() == Some(channel_did)
                        && Self::is_active(g, now)
                })
            })
            .unwrap_or(false)
    }

    pub fn list_for_requester(&self, requester_did: &str) -> Result<Vec<ContentGrant>> {
        let now = chrono::Utc::now().timestamp() as u64;
        Ok(self
            .load()?
            .grants
            .into_iter()
            .filter(|g| g.requester_did == requester_did && Self::is_active(g, now))
            .collect())
    }

    pub fn upsert_keychain_delegate(
        &self,
        grant_id: &str,
        granter_did: &str,
        grantee_did: &str,
        resource_type: &str,
        resource_id: Option<&str>,
        scopes: &[String],
        expires_at: Option<u64>,
        artifact_file_ids: Option<Vec<String>>,
    ) -> Result<()> {
        let mut store = self.load()?;
        let now = chrono::Utc::now().timestamp() as u64;
        store.grants.retain(|g| {
            !(g.kind == GrantKind::KeychainDelegate && g.grant_id.as_deref() == Some(grant_id))
        });
        store.grants.push(ContentGrant {
            kind: GrantKind::KeychainDelegate,
            requester_did: grantee_did.to_string(),
            content_id_hex: resource_type
                .eq("content")
                .then(|| resource_id.unwrap_or("").to_string()),
            channel_did: None,
            granted_at: now,
            expires_at,
            payment_reference: None,
            entitlement_id_hex: None,
            tier: None,
            license_token_id: None,
            quota_remaining: None,
            grant_id: Some(grant_id.to_string()),
            granter_did: Some(granter_did.to_string()),
            resource_type: Some(resource_type.to_string()),
            resource_id: resource_id.map(String::from),
            scopes: Some(scopes.to_vec()),
            artifact_file_ids,
        });
        self.save(&store)
    }

    pub fn revoke_keychain_delegate(&self, grant_id: &str) -> Result<bool> {
        let mut store = self.load()?;
        let before = store.grants.len();
        store.grants.retain(|g| {
            !(g.kind == GrantKind::KeychainDelegate && g.grant_id.as_deref() == Some(grant_id))
        });
        let removed = store.grants.len() != before;
        if removed {
            self.save(&store)?;
        }
        Ok(removed)
    }

    fn scope_allows_view(scopes: &[String]) -> bool {
        scopes
            .iter()
            .any(|s| s == "view" || s == "manage" || s == "admin")
    }

    pub fn has_keychain_content_access(
        &self,
        grantee_did: &str,
        granter_did: &str,
        content_id_hex: &str,
    ) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        self.load()
            .ok()
            .map(|s| {
                s.grants.iter().any(|g| {
                    if g.kind != GrantKind::KeychainDelegate || !Self::is_active(g, now) {
                        return false;
                    }
                    if g.requester_did != grantee_did
                        || g.granter_did.as_deref() != Some(granter_did)
                    {
                        return false;
                    }
                    let scopes = g.scopes.as_deref().unwrap_or(&[]);
                    if !Self::scope_allows_view(scopes) {
                        return false;
                    }
                    match g.resource_type.as_deref() {
                        Some("account") => true,
                        Some("content") => {
                            g.resource_id
                                .as_deref()
                                .map(|id| id == content_id_hex)
                                .unwrap_or(false)
                                || g.content_id_hex.as_deref() == Some(content_id_hex)
                        }
                        Some("app") => g
                            .artifact_file_ids
                            .as_ref()
                            .map(|ids| ids.iter().any(|id| id == content_id_hex))
                            .unwrap_or(false),
                        _ => false,
                    }
                })
            })
            .unwrap_or(false)
    }

    pub fn has_keychain_file_access(
        &self,
        grantee_did: &str,
        owner_did: &str,
        file_id: &str,
    ) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        self.load()
            .ok()
            .map(|s| {
                s.grants.iter().any(|g| {
                    if g.kind != GrantKind::KeychainDelegate || !Self::is_active(g, now) {
                        return false;
                    }
                    if g.requester_did != grantee_did || g.granter_did.as_deref() != Some(owner_did)
                    {
                        return false;
                    }
                    let scopes = g.scopes.as_deref().unwrap_or(&[]);
                    if !Self::scope_allows_view(scopes) {
                        return false;
                    }
                    match g.resource_type.as_deref() {
                        Some("account") => true,
                        Some("content") => g.resource_id.as_deref() == Some(file_id),
                        Some("app") => g
                            .artifact_file_ids
                            .as_ref()
                            .map(|ids| ids.iter().any(|id| id == file_id))
                            .unwrap_or(false),
                        _ => false,
                    }
                })
            })
            .unwrap_or(false)
    }
}
