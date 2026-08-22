//! App Package Storage Implementation
//!
//! This module provides storage capabilities for SpaceKit App Packages, building
//! on top of the FactStorageEngine to provide app-specific operations like
//! indexing by category, versioning, and app discovery.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use spacekit_primitives::v1::app::{
    AppCategory, AppID, AppManifest, AppPackage, AppPricing, AppVerificationResult, ContentRef,
    EntryPoint, LoadedApp, SemVer,
};
use spacekit_primitives::v1::fact::{AccessPolicy, FactContent, FactID, FactPackage};
use spacekit_primitives::v1::identity::QuantumDID;

use crate::database::Database;
use crate::fact_storage::FactStorageEngine;

/// App storage engine for managing App Packages
pub struct AppStorageEngine {
    /// Underlying fact storage engine
    fact_storage: Arc<FactStorageEngine>,
    /// Database connection
    database: Arc<Database>,
    /// In-memory app index for fast queries
    app_index: Arc<RwLock<AppIndex>>,
}

impl std::fmt::Debug for AppStorageEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppStorageEngine")
            .field("fact_storage", &"<FactStorageEngine>")
            .field("database", &self.database)
            .field("app_index", &self.app_index)
            .finish()
    }
}

/// In-memory index for fast app queries
#[derive(Debug, Default)]
pub struct AppIndex {
    /// Map from app ID to app metadata
    pub apps: HashMap<AppID, IndexedAppMetadata>,
    /// Index by creator DID
    pub by_creator: HashMap<String, HashSet<AppID>>,
    /// Index by category
    pub by_category: HashMap<AppCategory, HashSet<AppID>>,
    /// Index by keyword/tag
    pub by_keyword: HashMap<String, HashSet<AppID>>,
    /// Version history for each app (app_id -> versions sorted newest first)
    pub versions: HashMap<AppID, Vec<SemVer>>,
    /// Featured apps (curated list)
    pub featured: HashSet<AppID>,
}

/// Metadata stored in the app index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedAppMetadata {
    pub app_id: AppID,
    pub name: String,
    pub description: String,
    pub tagline: Option<String>,
    pub creator_did: String,
    pub latest_version: SemVer,
    pub category: AppCategory,
    pub pricing: AppPricing,
    pub total_size: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub download_count: u64,
    pub rating: Option<f32>,
    pub keywords: Vec<String>,
    pub icon: Option<String>,
    pub fact_id: FactID,
}

/// Query parameters for listing apps
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppQuery {
    pub category: Option<AppCategory>,
    pub creator: Option<String>,
    pub search: Option<String>,
    pub featured_only: bool,
    pub free_only: bool,
    pub min_rating: Option<f32>,
    pub sort_by: AppSortBy,
    pub sort_order: AppSortOrder,
    pub limit: usize,
    pub offset: usize,
}

/// Sort options for app listing
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum AppSortBy {
    #[default]
    CreatedAt,
    UpdatedAt,
    Downloads,
    Rating,
    Name,
    Size,
}

/// Sort order for app queries
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum AppSortOrder {
    #[default]
    Descending,
    Ascending,
}

/// Result of an app query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppQueryResult {
    pub apps: Vec<IndexedAppMetadata>,
    pub total_count: usize,
    pub has_more: bool,
}

impl AppStorageEngine {
    /// Create a new app storage engine
    pub async fn new(
        fact_storage: Arc<FactStorageEngine>,
        database: Arc<Database>,
    ) -> Result<Self> {
        let engine = Self {
            fact_storage,
            database,
            app_index: Arc::new(RwLock::new(AppIndex::default())),
        };

        // Load existing apps into the index
        engine.rebuild_index().await?;

        Ok(engine)
    }

    /// Store an app package
    pub async fn store_app(&self, app_package: &AppPackage) -> Result<FactID> {
        // Create a FactPackage from the AppPackage
        let fact_content = FactContent::Json {
            data: serde_json::to_value(app_package)?,
            schema: Some("spacekit:app-package:v1".to_string()),
        };

        // Build metadata tags
        let mut tags = vec![
            "app-package".to_string(),
            app_package.manifest.name.clone(),
            format!("category:{:?}", app_package.category),
            format!("version:{}", app_package.version.to_string()),
        ];
        tags.extend(app_package.manifest.keywords.clone());

        // Create the fact package
        use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
        use spacekit_primitives::v1::fact::{
            CollectionMethod, DataSource, FactCategory, FactMetadata, KnowledgeDomain, LicenseType,
            ProofType, VerificationLevel, VerificationProof,
        };

        let fact_package = FactPackage {
            fact_id: app_package.app_id,
            version: 1,
            created_at: app_package.created_at,
            expires_at: None,
            content: fact_content,
            metadata: FactMetadata {
                category: FactCategory::Technical,
                tags,
                domain: KnowledgeDomain::ComputerScience,
                source: DataSource::UserInput {
                    application: app_package.creator_did.clone(),
                    user: app_package.creator_did.clone(),
                },
                collection_method: CollectionMethod::Manual,
                verification_level: VerificationLevel::SelfClaimed,
                license: app_package.license_type.clone(),
                size_bytes: app_package.manifest.total_size,
                checksum: app_package.manifest.checksum,
            },
            author: app_package.creator_did.clone(),
            signature: app_package.signature.clone(),
            verification_proof: VerificationProof {
                proof_type: ProofType::QuantumSignature,
                proof_data: vec![],
                verification_timestamp: app_package.created_at,
                verifier: Some(app_package.creator_did.clone()),
            },
            dependencies: Vec::new(),
            citations: Vec::new(),
            confidence_score: 0.9,
            access_policy: app_package.access_policy.clone(),
            encryption: None,
        };

        // Store in fact storage
        let fact_id = self.fact_storage.store_fact(fact_package).await?;

        // Update the index
        self.index_app(app_package, fact_id).await?;

        info!(
            "Stored app package: {} v{}",
            app_package.manifest.name,
            app_package.version.to_string()
        );

        Ok(fact_id)
    }

    /// Retrieve an app package by ID
    pub async fn get_app(&self, app_id: &AppID) -> Result<Option<AppPackage>> {
        let fact = self.fact_storage.retrieve_fact(*app_id).await?;

        match fact {
            Some(fact_package) => {
                if let FactContent::Json { data, schema } = &fact_package.content {
                    if schema.as_deref() == Some("spacekit:app-package:v1") {
                        let app: AppPackage = serde_json::from_value(data.clone())?;
                        return Ok(Some(app));
                    }
                }
                Ok(None)
            }
            None => Ok(None),
        }
    }

    /// Get a specific version of an app
    pub async fn get_app_version(
        &self,
        app_id: &AppID,
        version: &SemVer,
    ) -> Result<Option<AppPackage>> {
        // For now, we only support the latest version
        // In the future, we could store version-specific fact IDs
        let app = self.get_app(app_id).await?;

        match app {
            Some(app) if &app.version == version => Ok(Some(app)),
            _ => Ok(None),
        }
    }

    /// List apps matching query parameters
    pub async fn list_apps(&self, query: &AppQuery) -> Result<AppQueryResult> {
        let index = self.app_index.read().await;

        // Start with all apps or filtered set
        let mut candidates: Vec<&IndexedAppMetadata> = if query.featured_only {
            index
                .featured
                .iter()
                .filter_map(|id| index.apps.get(id))
                .collect()
        } else if let Some(category) = &query.category {
            index
                .by_category
                .get(category)
                .map(|ids| ids.iter().filter_map(|id| index.apps.get(id)).collect())
                .unwrap_or_default()
        } else if let Some(creator) = &query.creator {
            index
                .by_creator
                .get(creator)
                .map(|ids| ids.iter().filter_map(|id| index.apps.get(id)).collect())
                .unwrap_or_default()
        } else {
            index.apps.values().collect()
        };

        // Apply search filter
        if let Some(search) = &query.search {
            let search_lower = search.to_lowercase();
            candidates.retain(|app| {
                app.name.to_lowercase().contains(&search_lower)
                    || app.description.to_lowercase().contains(&search_lower)
                    || app
                        .keywords
                        .iter()
                        .any(|k| k.to_lowercase().contains(&search_lower))
            });
        }

        // Apply free_only filter
        if query.free_only {
            candidates.retain(|app| matches!(app.pricing, AppPricing::Free));
        }

        // Apply min_rating filter
        if let Some(min_rating) = query.min_rating {
            candidates.retain(|app| app.rating.map_or(false, |r| r >= min_rating));
        }

        // Sort
        match (&query.sort_by, &query.sort_order) {
            (AppSortBy::CreatedAt, AppSortOrder::Descending) => {
                candidates.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            }
            (AppSortBy::CreatedAt, AppSortOrder::Ascending) => {
                candidates.sort_by(|a, b| a.created_at.cmp(&b.created_at));
            }
            (AppSortBy::UpdatedAt, AppSortOrder::Descending) => {
                candidates.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            }
            (AppSortBy::UpdatedAt, AppSortOrder::Ascending) => {
                candidates.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
            }
            (AppSortBy::Downloads, AppSortOrder::Descending) => {
                candidates.sort_by(|a, b| b.download_count.cmp(&a.download_count));
            }
            (AppSortBy::Downloads, AppSortOrder::Ascending) => {
                candidates.sort_by(|a, b| a.download_count.cmp(&b.download_count));
            }
            (AppSortBy::Rating, AppSortOrder::Descending) => {
                candidates.sort_by(|a, b| {
                    b.rating
                        .unwrap_or(0.0)
                        .partial_cmp(&a.rating.unwrap_or(0.0))
                        .unwrap()
                });
            }
            (AppSortBy::Rating, AppSortOrder::Ascending) => {
                candidates.sort_by(|a, b| {
                    a.rating
                        .unwrap_or(0.0)
                        .partial_cmp(&b.rating.unwrap_or(0.0))
                        .unwrap()
                });
            }
            (AppSortBy::Name, AppSortOrder::Descending) => {
                candidates.sort_by(|a, b| b.name.cmp(&a.name));
            }
            (AppSortBy::Name, AppSortOrder::Ascending) => {
                candidates.sort_by(|a, b| a.name.cmp(&b.name));
            }
            (AppSortBy::Size, AppSortOrder::Descending) => {
                candidates.sort_by(|a, b| b.total_size.cmp(&a.total_size));
            }
            (AppSortBy::Size, AppSortOrder::Ascending) => {
                candidates.sort_by(|a, b| a.total_size.cmp(&b.total_size));
            }
        }

        let total_count = candidates.len();
        let limit = if query.limit == 0 {
            50
        } else {
            query.limit.min(100)
        };
        let offset = query.offset;

        let apps: Vec<IndexedAppMetadata> = candidates
            .into_iter()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect();

        let has_more = offset + apps.len() < total_count;

        Ok(AppQueryResult {
            apps,
            total_count,
            has_more,
        })
    }

    /// Search apps by keyword
    pub async fn search_apps(
        &self,
        keyword: &str,
        limit: usize,
    ) -> Result<Vec<IndexedAppMetadata>> {
        let query = AppQuery {
            search: Some(keyword.to_string()),
            limit,
            ..Default::default()
        };

        let result = self.list_apps(&query).await?;
        Ok(result.apps)
    }

    /// Get all versions of an app
    pub async fn get_app_versions(&self, app_id: &AppID) -> Result<Vec<SemVer>> {
        let index = self.app_index.read().await;
        Ok(index.versions.get(app_id).cloned().unwrap_or_default())
    }

    /// Get featured apps
    pub async fn get_featured_apps(&self, limit: usize) -> Result<Vec<IndexedAppMetadata>> {
        let query = AppQuery {
            featured_only: true,
            limit,
            sort_by: AppSortBy::Downloads,
            ..Default::default()
        };

        let result = self.list_apps(&query).await?;
        Ok(result.apps)
    }

    /// Get apps by category
    pub async fn get_apps_by_category(
        &self,
        category: AppCategory,
        limit: usize,
    ) -> Result<Vec<IndexedAppMetadata>> {
        let query = AppQuery {
            category: Some(category),
            limit,
            sort_by: AppSortBy::Downloads,
            ..Default::default()
        };

        let result = self.list_apps(&query).await?;
        Ok(result.apps)
    }

    /// Get apps by creator
    pub async fn get_apps_by_creator(
        &self,
        creator_did: &str,
        limit: usize,
    ) -> Result<Vec<IndexedAppMetadata>> {
        let query = AppQuery {
            creator: Some(creator_did.to_string()),
            limit,
            sort_by: AppSortBy::UpdatedAt,
            ..Default::default()
        };

        let result = self.list_apps(&query).await?;
        Ok(result.apps)
    }

    /// Mark an app as featured
    pub async fn set_featured(&self, app_id: &AppID, featured: bool) -> Result<()> {
        let mut index = self.app_index.write().await;

        if featured {
            index.featured.insert(*app_id);
        } else {
            index.featured.remove(app_id);
        }

        Ok(())
    }

    /// Increment download count for an app
    pub async fn increment_download_count(&self, app_id: &AppID) -> Result<u64> {
        let mut index = self.app_index.write().await;

        if let Some(app) = index.apps.get_mut(app_id) {
            app.download_count += 1;
            Ok(app.download_count)
        } else {
            Err(anyhow!("App not found"))
        }
    }

    /// Update app rating
    pub async fn update_rating(&self, app_id: &AppID, rating: f32) -> Result<()> {
        let mut index = self.app_index.write().await;

        if let Some(app) = index.apps.get_mut(app_id) {
            app.rating = Some(rating);
            Ok(())
        } else {
            Err(anyhow!("App not found"))
        }
    }

    /// Verify an app's signature and integrity
    pub async fn verify_app(&self, app_id: &AppID) -> Result<AppVerificationResult> {
        let app = self
            .get_app(app_id)
            .await?
            .ok_or_else(|| anyhow!("App not found"))?;

        // Verify signature (placeholder - in production would verify SPHINCS+ sig)
        let signature_valid = !app.signature.signature_bytes.is_empty();

        // Verify content integrity (would need to fetch all content)
        let content_integrity = true; // Placeholder

        // Check if creator DID is valid
        let creator_verified = !app.creator_did.as_str().is_empty();

        // Check dependencies
        let all_dependencies_available = app.dependencies.is_empty(); // Placeholder

        // Permissions check
        let permissions_acceptable = true; // Placeholder

        let overall_valid = signature_valid && content_integrity && creator_verified;

        Ok(AppVerificationResult {
            signature_valid,
            creator_verified,
            content_integrity,
            all_dependencies_available,
            permissions_acceptable,
            overall_valid,
            warnings: Vec::new(),
        })
    }

    /// Index an app in memory
    async fn index_app(&self, app: &AppPackage, fact_id: FactID) -> Result<()> {
        let mut index = self.app_index.write().await;

        let metadata = IndexedAppMetadata {
            app_id: app.app_id,
            name: app.manifest.name.clone(),
            description: app.manifest.description.clone(),
            tagline: app.manifest.tagline.clone(),
            creator_did: app.creator_did.as_str().to_string(),
            latest_version: app.version.clone(),
            category: app.category.clone(),
            pricing: app.pricing.clone(),
            total_size: app.manifest.total_size,
            created_at: app.created_at,
            updated_at: app.created_at,
            download_count: 0,
            rating: None,
            keywords: app.manifest.keywords.clone(),
            icon: app.manifest.icon.clone(),
            fact_id,
        };

        // Update main index
        index.apps.insert(app.app_id, metadata);

        // Update creator index
        index
            .by_creator
            .entry(app.creator_did.as_str().to_string())
            .or_default()
            .insert(app.app_id);

        // Update category index
        index
            .by_category
            .entry(app.category.clone())
            .or_default()
            .insert(app.app_id);

        // Update keyword index
        for keyword in &app.manifest.keywords {
            index
                .by_keyword
                .entry(keyword.to_lowercase())
                .or_default()
                .insert(app.app_id);
        }

        // Update version history
        index
            .versions
            .entry(app.app_id)
            .or_default()
            .push(app.version.clone());

        // Sort versions (newest first)
        if let Some(versions) = index.versions.get_mut(&app.app_id) {
            versions.sort_by(|a, b| b.cmp(a));
        }

        Ok(())
    }

    /// Rebuild the index from stored facts
    async fn rebuild_index(&self) -> Result<()> {
        info!("Rebuilding app index...");

        // Query all app-package facts
        use spacekit_primitives::v1::fact::types::{
            FactQuery, PaginationParams, SortCriteria, SortOrder as FactSortOrder,
        };

        // Create a dummy requester DID for the query
        let requester = QuantumDID::new("did:spacekit:system:app-indexer".to_string());

        let query = FactQuery {
            author: None,
            category: None,
            tags: vec!["app-package".to_string()],
            domain: None,
            content_type: None,
            text_search: None,
            verification_level: None,
            min_confidence: None,
            created_after: None,
            created_before: None,
            depends_on: None,
            referenced_by: None,
            sort_by: SortCriteria::CreatedAt(FactSortOrder::Descending),
            pagination: PaginationParams {
                offset: 0,
                limit: 10000,
            },
            requester,
            start_time: chrono::Utc::now().timestamp() as u64,
        };

        let results = self.fact_storage.query_facts(query).await?;

        let mut count = 0;
        for fact in results.facts {
            if let FactContent::Json { data, schema } = &fact.content {
                if schema.as_deref() == Some("spacekit:app-package:v1") {
                    if let Ok(app) = serde_json::from_value::<AppPackage>(data.clone()) {
                        self.index_app(&app, fact.fact_id).await?;
                        count += 1;
                    }
                }
            }
        }

        info!("App index rebuilt with {} apps", count);
        Ok(())
    }

    /// Get storage statistics
    pub async fn get_stats(&self) -> AppStorageStats {
        let index = self.app_index.read().await;

        let mut category_counts = HashMap::new();
        for (category, apps) in &index.by_category {
            category_counts.insert(format!("{:?}", category), apps.len());
        }

        let total_size: u64 = index.apps.values().map(|a| a.total_size).sum();
        let total_downloads: u64 = index.apps.values().map(|a| a.download_count).sum();

        AppStorageStats {
            total_apps: index.apps.len(),
            total_creators: index.by_creator.len(),
            total_size,
            total_downloads,
            featured_count: index.featured.len(),
            category_counts,
        }
    }
}

/// Storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStorageStats {
    pub total_apps: usize,
    pub total_creators: usize,
    pub total_size: u64,
    pub total_downloads: u64,
    pub featured_count: usize,
    pub category_counts: HashMap<String, usize>,
}
