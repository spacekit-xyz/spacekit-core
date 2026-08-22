//! Marketplace Replication — P2P propagation of app listings across the network
//!
//! When a new app is published or a listing is updated on one storage node,
//! this module announces it to peer compute/storage nodes so the catalog
//! is eventually consistent across the decentralized network.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppListingAnnouncement {
    pub app_id: String,
    pub title: String,
    pub publisher_did: String,
    pub marketplace_id: String,
    pub version: String,
    pub category: String,
    pub access: String,
    pub pricing_model: String,
    pub artifact_count: usize,
    pub total_size_bytes: u64,
    /// Storage node URL where the listing document and artifacts live
    pub origin_storage_url: String,
    pub updated_at: String,
    pub downloads: u64,
    pub rating_avg: f64,
}

#[derive(Debug, Clone)]
struct CachedAnnouncement {
    announcement: AppListingAnnouncement,
    received_at: Instant,
    propagated_to: Vec<String>,
}

/// Manages propagation of app listings across the P2P network.
pub struct MarketplaceReplicationManager {
    /// Known listings from across the network, keyed by app_id.
    catalog: Arc<RwLock<HashMap<String, CachedAnnouncement>>>,
    /// Peer storage node URLs discovered via P2P service discovery.
    peer_storage_nodes: Arc<RwLock<Vec<String>>>,
    /// How long to cache announcements before re-announcing.
    announcement_ttl: Duration,
    /// HTTP client for fetching/pushing announcements to peers.
    http_client: reqwest::Client,
}

impl MarketplaceReplicationManager {
    pub fn new() -> Self {
        Self {
            catalog: Arc::new(RwLock::new(HashMap::new())),
            peer_storage_nodes: Arc::new(RwLock::new(Vec::new())),
            announcement_ttl: Duration::from_secs(3600),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Register a peer storage node URL for replication.
    pub async fn add_peer(&self, url: String) {
        let mut peers = self.peer_storage_nodes.write().await;
        if !peers.contains(&url) {
            info!("Marketplace replication: added peer {}", url);
            peers.push(url);
        }
    }

    /// Remove a peer that is no longer reachable.
    pub async fn remove_peer(&self, url: &str) {
        let mut peers = self.peer_storage_nodes.write().await;
        peers.retain(|p| p != url);
    }

    /// Called when a new listing is published locally. Announces to all peers.
    pub async fn announce_listing(&self, announcement: AppListingAnnouncement) {
        let app_id = announcement.app_id.clone();
        info!("Marketplace replication: announcing {} to peers", app_id);

        let peers = self.peer_storage_nodes.read().await.clone();
        let mut propagated_to = Vec::new();

        for peer_url in &peers {
            match self.push_announcement(peer_url, &announcement).await {
                Ok(_) => {
                    debug!("  ✓ announced to {}", peer_url);
                    propagated_to.push(peer_url.clone());
                }
                Err(e) => {
                    warn!("  ✗ failed to announce to {}: {}", peer_url, e);
                }
            }
        }

        let mut catalog = self.catalog.write().await;
        catalog.insert(
            app_id,
            CachedAnnouncement {
                announcement,
                received_at: Instant::now(),
                propagated_to,
            },
        );
    }

    /// Push an announcement to a single peer's storage node.
    async fn push_announcement(
        &self,
        peer_url: &str,
        announcement: &AppListingAnnouncement,
    ) -> Result<(), String> {
        let url = format!(
            "{}/api/documents/app_listing_announcements/{}",
            peer_url.trim_end_matches('/'),
            announcement.app_id
        );

        let resp = self
            .http_client
            .put(&url)
            .header("owner-did", &announcement.publisher_did)
            .header("content-type", "application/json")
            .json(announcement)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(format!("HTTP {}", resp.status()))
        }
    }

    /// Receive an announcement from a peer (called when we get a push).
    pub async fn receive_announcement(&self, announcement: AppListingAnnouncement) {
        let app_id = announcement.app_id.clone();
        let mut catalog = self.catalog.write().await;

        if let Some(existing) = catalog.get(&app_id) {
            if existing.announcement.updated_at >= announcement.updated_at {
                debug!("Ignoring stale announcement for {}", app_id);
                return;
            }
        }

        info!("Received marketplace announcement for {}", app_id);
        catalog.insert(
            app_id,
            CachedAnnouncement {
                announcement,
                received_at: Instant::now(),
                propagated_to: Vec::new(),
            },
        );
    }

    /// Pull listings from a peer storage node and merge into local catalog.
    pub async fn pull_from_peer(&self, peer_url: &str) -> Result<usize, String> {
        let url = format!(
            "{}/query/documents/app_listings",
            peer_url.trim_end_matches('/')
        );

        let query = serde_json::json!({
            "filters": [{"path": "status", "op": "Equals", "value": "published"}],
            "limit": 200,
            "sort_by": {"field": "updated_at", "order": "Desc"}
        });

        let resp = self
            .http_client
            .post(&url)
            .header("owner-did", "did:spacekit:replication")
            .json(&query)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        #[derive(Deserialize)]
        struct Doc<T> {
            data: T,
        }
        #[derive(Deserialize)]
        struct QueryResp {
            documents: Vec<Doc<serde_json::Value>>,
        }

        let data: QueryResp = resp.json().await.map_err(|e| e.to_string())?;
        let mut count = 0;
        let mut catalog = self.catalog.write().await;

        for doc in data.documents {
            if let (Some(app_id), Some(title)) = (
                doc.data.get("app_id").and_then(|v| v.as_str()),
                doc.data.get("title").and_then(|v| v.as_str()),
            ) {
                let announcement = AppListingAnnouncement {
                    app_id: app_id.to_string(),
                    title: title.to_string(),
                    publisher_did: doc
                        .data
                        .get("publisher_did")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    marketplace_id: doc
                        .data
                        .get("marketplace_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("default")
                        .to_string(),
                    version: doc
                        .data
                        .get("version")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    category: doc
                        .data
                        .get("category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    access: doc
                        .data
                        .get("access")
                        .and_then(|v| v.as_str())
                        .unwrap_or("public")
                        .to_string(),
                    pricing_model: doc
                        .data
                        .get("pricing")
                        .and_then(|v| v.get("model"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("free")
                        .to_string(),
                    artifact_count: doc
                        .data
                        .get("artifacts")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0),
                    total_size_bytes: 0,
                    origin_storage_url: peer_url.to_string(),
                    updated_at: doc
                        .data
                        .get("updated_at")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    downloads: doc
                        .data
                        .get("downloads")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                    rating_avg: doc
                        .data
                        .get("rating_avg")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                };

                catalog.insert(
                    app_id.to_string(),
                    CachedAnnouncement {
                        announcement,
                        received_at: Instant::now(),
                        propagated_to: Vec::new(),
                    },
                );
                count += 1;
            }
        }

        info!("Pulled {} listings from peer {}", count, peer_url);
        Ok(count)
    }

    /// Get the merged catalog of all known listings across the network.
    pub async fn get_network_catalog(&self) -> Vec<AppListingAnnouncement> {
        let catalog = self.catalog.read().await;
        catalog
            .values()
            .filter(|c| c.received_at.elapsed() < self.announcement_ttl)
            .map(|c| c.announcement.clone())
            .collect()
    }

    /// Periodic cleanup of stale entries.
    pub async fn cleanup_stale(&self) {
        let mut catalog = self.catalog.write().await;
        let before = catalog.len();
        catalog.retain(|_, c| c.received_at.elapsed() < self.announcement_ttl);
        let removed = before - catalog.len();
        if removed > 0 {
            debug!("Cleaned up {} stale marketplace announcements", removed);
        }
    }
}
