//! NFT Collection Management
//!
//! Provides comprehensive NFT collection features including:
//! - Collection creation and management
//! - Minting NFTs to collections
//! - Royalty configuration
//! - Collection analytics
//! - Floor price tracking
//! - Rarity scoring

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use spacekit_primitives::v1::identity::QuantumDID;

use crate::nft_storage::{AttributeValue, NftAttribute, NftMetadata, NftStorageManager};

/// NFT Collection with comprehensive metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftCollection {
    // Core identification
    pub collection_id: String,
    pub name: String,
    pub symbol: String,
    pub description: String,

    // Branding
    pub image: String,
    pub banner_image: Option<String>,
    pub featured_image: Option<String>,
    pub external_url: Option<String>,

    // Creator information
    pub creator: QuantumDID,
    pub verified_creator: bool,

    // Collection metadata
    pub total_supply: u64,
    pub max_supply: Option<u64>, // None = unlimited
    pub minted_count: u64,

    // Royalty configuration
    pub royalty_config: RoyaltyConfig,

    // Collection properties
    pub properties: CollectionProperties,

    // Social links
    pub social_links: SocialLinks,

    // Timestamps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    // Statistics (updated periodically)
    pub stats: CollectionStats,

    // Quantum-safe verification
    pub quantum_signature: Option<String>,
}

/// Royalty configuration for NFT sales
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoyaltyConfig {
    /// Creator royalty percentage (0-100)
    pub creator_royalty_percent: f64,
    /// Creator royalty address
    pub creator_address: QuantumDID,
    /// Platform fee percentage (0-100)
    pub platform_fee_percent: f64,
    /// Additional royalty splits
    pub royalty_splits: Vec<RoyaltySplit>,
}

/// Royalty split for multiple beneficiaries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoyaltySplit {
    pub address: QuantumDID,
    pub percentage: f64,
    pub description: Option<String>,
}

/// Collection properties and traits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionProperties {
    /// Category (Art, Gaming, Music, etc.)
    pub category: CollectionCategory,
    /// Whether collection is revealed
    pub revealed: bool,
    /// Reveal date if not revealed
    pub reveal_date: Option<DateTime<Utc>>,
    /// Base URI for metadata
    pub base_uri: Option<String>,
    /// Contract standard (ERC-721, ERC-1155, etc.)
    pub token_standard: TokenStandard,
    /// Blockchain network
    pub network: String,
    /// Contract address if deployed
    pub contract_address: Option<String>,
}

/// Collection category
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CollectionCategory {
    Art,
    Gaming,
    Music,
    Photography,
    Sports,
    Collectibles,
    Utility,
    Metaverse,
    PFP, // Profile Pictures
    Generative,
    Custom(String),
}

/// Token standard
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TokenStandard {
    ERC721,  // Ethereum NFT standard
    ERC1155, // Ethereum multi-token standard
    SPL,     // Solana Program Library
    Custom(String),
}

/// Social media links
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SocialLinks {
    pub website: Option<String>,
    pub discord: Option<String>,
    pub twitter: Option<String>,
    pub instagram: Option<String>,
    pub telegram: Option<String>,
    pub medium: Option<String>,
}

/// Collection statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectionStats {
    /// Total number of owners
    pub unique_owners: u64,
    /// Floor price in wei
    pub floor_price: Option<u128>,
    /// Total volume traded in wei
    pub total_volume: u128,
    /// Number of sales
    pub total_sales: u64,
    /// Average sale price in wei
    pub average_price: Option<u128>,
    /// 24h volume
    pub volume_24h: u128,
    /// 7d volume
    pub volume_7d: u128,
    /// 30d volume
    pub volume_30d: u128,
    /// Last sale price
    pub last_sale_price: Option<u128>,
    /// Last sale timestamp
    pub last_sale_time: Option<DateTime<Utc>>,
}

/// NFT Collection Manager
pub struct NftCollectionManager {
    nft_storage: Arc<NftStorageManager>,
    collections: Arc<RwLock<HashMap<String, NftCollection>>>,
    collection_nfts: Arc<RwLock<HashMap<String, Vec<[u8; 32]>>>>, // collection_id -> nft_ids
}

/// Minting configuration for new NFTs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintConfig {
    pub collection_id: String,
    pub token_id: u64,
    pub metadata: NftMetadata,
    pub mint_price: Option<u128>,
}

/// Collection query filters
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectionQuery {
    pub category: Option<CollectionCategory>,
    pub min_floor_price: Option<u128>,
    pub max_floor_price: Option<u128>,
    pub min_volume: Option<u128>,
    pub verified_only: bool,
    pub sort_by: CollectionSortCriteria,
    pub limit: usize,
    pub offset: usize,
}

/// Collection sorting criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CollectionSortCriteria {
    Volume,
    FloorPrice,
    TotalSupply,
    CreatedDate,
    UniqueOwners,
}

impl Default for CollectionSortCriteria {
    fn default() -> Self {
        Self::Volume
    }
}

/// Rarity trait configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RarityConfig {
    pub trait_type: String,
    pub total_count: u64,
    pub trait_counts: HashMap<String, u64>,
}

/// Rarity score for an NFT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RarityScore {
    pub token_id: u64,
    pub rank: u64,
    pub score: f64,
    pub trait_scores: HashMap<String, f64>,
}

impl NftCollectionManager {
    /// Create a new NFT collection manager
    pub fn new(nft_storage: Arc<NftStorageManager>) -> Self {
        Self {
            nft_storage,
            collections: Arc::new(RwLock::new(HashMap::new())),
            collection_nfts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new NFT collection
    pub async fn create_collection(&self, mut collection: NftCollection) -> Result<String> {
        // Validate collection
        self.validate_collection(&collection)?;

        // Set timestamps
        collection.created_at = Utc::now();
        collection.updated_at = Utc::now();

        // Initialize stats
        collection.stats = CollectionStats::default();
        collection.minted_count = 0;

        // Generate collection ID if not provided
        if collection.collection_id.is_empty() {
            collection.collection_id = self.generate_collection_id(&collection);
        }

        let collection_id = collection.collection_id.clone();

        // Store collection
        {
            let mut collections = self.collections.write().await;
            collections.insert(collection_id.clone(), collection.clone());
        }

        // Initialize NFT list for collection
        {
            let mut collection_nfts = self.collection_nfts.write().await;
            collection_nfts.insert(collection_id.clone(), Vec::new());
        }

        info!(
            "Created NFT collection: {} ({})",
            collection.name, collection_id
        );
        Ok(collection_id)
    }

    /// Mint a new NFT to a collection
    pub async fn mint_to_collection(
        &self,
        mint_config: MintConfig,
        nft_data: Vec<u8>,
        mime_type: String,
    ) -> Result<[u8; 32]> {
        // Get collection
        let mut collection = {
            let collections = self.collections.read().await;
            collections
                .get(&mint_config.collection_id)
                .ok_or_else(|| anyhow!("Collection not found"))?
                .clone()
        };

        // Check max supply
        if let Some(max_supply) = collection.max_supply {
            if collection.minted_count >= max_supply {
                return Err(anyhow!("Collection max supply reached"));
            }
        }

        // Add collection info to metadata
        let mut metadata = mint_config.metadata;
        metadata.collection = Some(crate::nft_storage::NftCollection {
            name: collection.name.clone(),
            family: collection.collection_id.clone(),
            description: Some(collection.description.clone()),
            image: Some(collection.image.clone()),
            external_url: collection.external_url.clone(),
        });

        // Store NFT
        let nft_result = self
            .nft_storage
            .store_nft(nft_data, metadata, mime_type)
            .await?;

        // Update collection
        collection.minted_count += 1;
        collection.total_supply += 1;
        collection.updated_at = Utc::now();

        // Save updated collection
        {
            let mut collections = self.collections.write().await;
            collections.insert(collection.collection_id.clone(), collection.clone());
        }

        // Add NFT to collection list
        {
            let mut collection_nfts = self.collection_nfts.write().await;
            if let Some(nfts) = collection_nfts.get_mut(&mint_config.collection_id) {
                nfts.push(nft_result.nft_id);
            }
        }

        info!(
            "Minted NFT #{} to collection {}",
            mint_config.token_id, collection.name
        );

        Ok(nft_result.nft_id)
    }

    /// Get collection by ID
    pub async fn get_collection(&self, collection_id: &str) -> Result<Option<NftCollection>> {
        let collections = self.collections.read().await;
        Ok(collections.get(collection_id).cloned())
    }

    /// Update collection metadata
    pub async fn update_collection(
        &self,
        collection_id: &str,
        updates: CollectionUpdate,
    ) -> Result<()> {
        let mut collections = self.collections.write().await;

        if let Some(collection) = collections.get_mut(collection_id) {
            if let Some(description) = updates.description {
                collection.description = description;
            }
            if let Some(image) = updates.image {
                collection.image = image;
            }
            if let Some(external_url) = updates.external_url {
                collection.external_url = Some(external_url);
            }
            if let Some(social_links) = updates.social_links {
                collection.social_links = social_links;
            }
            if let Some(royalty_config) = updates.royalty_config {
                collection.royalty_config = royalty_config;
            }

            collection.updated_at = Utc::now();

            info!("Updated collection: {}", collection_id);
            Ok(())
        } else {
            Err(anyhow!("Collection not found"))
        }
    }

    /// Get all NFTs in a collection
    pub async fn get_collection_nfts(&self, collection_id: &str) -> Result<Vec<[u8; 32]>> {
        let collection_nfts = self.collection_nfts.read().await;
        Ok(collection_nfts
            .get(collection_id)
            .cloned()
            .unwrap_or_default())
    }

    /// Calculate rarity for collection
    pub async fn calculate_rarity(&self, collection_id: &str) -> Result<Vec<RarityScore>> {
        let nft_ids = self.get_collection_nfts(collection_id).await?;

        if nft_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Collect all traits
        let mut trait_counts: HashMap<String, HashMap<String, u64>> = HashMap::new();
        let mut nft_traits: HashMap<[u8; 32], Vec<(String, String)>> = HashMap::new();

        for nft_id in &nft_ids {
            if let Some((_, metadata)) = self.nft_storage.retrieve_nft(*nft_id).await? {
                let mut traits = Vec::new();

                for attr in &metadata.attributes {
                    let trait_type = attr.trait_type.clone();
                    let value = match &attr.value {
                        AttributeValue::String(s) => s.clone(),
                        AttributeValue::Number(n) => n.to_string(),
                        AttributeValue::Boolean(b) => b.to_string(),
                    };

                    traits.push((trait_type.clone(), value.clone()));

                    // Count trait occurrences
                    trait_counts
                        .entry(trait_type)
                        .or_insert_with(HashMap::new)
                        .entry(value)
                        .and_modify(|count| *count += 1)
                        .or_insert(1);
                }

                nft_traits.insert(*nft_id, traits);
            }
        }

        let total_nfts = nft_ids.len() as f64;

        // Calculate rarity scores
        let mut rarity_scores = Vec::new();

        for (nft_id, traits) in nft_traits {
            let mut score = 0.0;
            let mut trait_scores = HashMap::new();

            for (trait_type, value) in traits {
                if let Some(counts) = trait_counts.get(&trait_type) {
                    if let Some(&count) = counts.get(&value) {
                        // Rarity score = 1 / (trait_occurrence / total_nfts)
                        let trait_rarity = total_nfts / count as f64;
                        score += trait_rarity;
                        trait_scores.insert(format!("{}:{}", trait_type, value), trait_rarity);
                    }
                }
            }

            rarity_scores.push(RarityScore {
                token_id: 0, // Would need to track token IDs
                rank: 0,     // Will be calculated after sorting
                score,
                trait_scores,
            });
        }

        // Sort by score descending
        rarity_scores.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        // Assign ranks
        for (i, score) in rarity_scores.iter_mut().enumerate() {
            score.rank = (i + 1) as u64;
        }

        debug!(
            "Calculated rarity for {} NFTs in collection {}",
            rarity_scores.len(),
            collection_id
        );
        Ok(rarity_scores)
    }

    /// Update collection statistics
    pub async fn update_collection_stats(&self, collection_id: &str, sale: SaleData) -> Result<()> {
        let mut collections = self.collections.write().await;

        if let Some(collection) = collections.get_mut(collection_id) {
            let stats = &mut collection.stats;

            // Update total volume
            stats.total_volume += sale.price;
            stats.total_sales += 1;

            // Update average price
            stats.average_price = Some(stats.total_volume / stats.total_sales as u128);

            // Update floor price
            if let Some(floor) = stats.floor_price {
                if sale.price < floor {
                    stats.floor_price = Some(sale.price);
                }
            } else {
                stats.floor_price = Some(sale.price);
            }

            // Update time-based volumes
            let now = Utc::now();
            let sale_age = now - sale.timestamp;

            if sale_age.num_hours() <= 24 {
                stats.volume_24h += sale.price;
            }
            if sale_age.num_days() <= 7 {
                stats.volume_7d += sale.price;
            }
            if sale_age.num_days() <= 30 {
                stats.volume_30d += sale.price;
            }

            // Update last sale
            stats.last_sale_price = Some(sale.price);
            stats.last_sale_time = Some(sale.timestamp);

            collection.updated_at = now;

            debug!("Updated stats for collection {}", collection_id);
            Ok(())
        } else {
            Err(anyhow!("Collection not found"))
        }
    }

    /// Query collections
    pub async fn query_collections(&self, query: CollectionQuery) -> Result<Vec<NftCollection>> {
        let collections = self.collections.read().await;

        let mut results: Vec<NftCollection> = collections
            .values()
            .filter(|c| {
                // Apply filters
                if let Some(cat) = &query.category {
                    if c.properties.category != *cat {
                        return false;
                    }
                }

                if query.verified_only && !c.verified_creator {
                    return false;
                }

                if let Some(min_floor) = query.min_floor_price {
                    if c.stats.floor_price.unwrap_or(0) < min_floor {
                        return false;
                    }
                }

                if let Some(max_floor) = query.max_floor_price {
                    if c.stats.floor_price.unwrap_or(u128::MAX) > max_floor {
                        return false;
                    }
                }

                if let Some(min_volume) = query.min_volume {
                    if c.stats.total_volume < min_volume {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Sort results
        match query.sort_by {
            CollectionSortCriteria::Volume => {
                results.sort_by(|a, b| b.stats.total_volume.cmp(&a.stats.total_volume));
            }
            CollectionSortCriteria::FloorPrice => {
                results.sort_by(|a, b| {
                    b.stats
                        .floor_price
                        .unwrap_or(0)
                        .cmp(&a.stats.floor_price.unwrap_or(0))
                });
            }
            CollectionSortCriteria::TotalSupply => {
                results.sort_by(|a, b| b.total_supply.cmp(&a.total_supply));
            }
            CollectionSortCriteria::CreatedDate => {
                results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
            }
            CollectionSortCriteria::UniqueOwners => {
                results.sort_by(|a, b| b.stats.unique_owners.cmp(&a.stats.unique_owners));
            }
        }

        // Apply pagination
        let start = query.offset;
        let end = (start + query.limit).min(results.len());

        Ok(results[start..end].to_vec())
    }

    /// Get collection analytics
    pub async fn get_collection_analytics(
        &self,
        collection_id: &str,
    ) -> Result<CollectionAnalytics> {
        let collection = self
            .get_collection(collection_id)
            .await?
            .ok_or_else(|| anyhow!("Collection not found"))?;

        let nft_count = self.get_collection_nfts(collection_id).await?.len();

        Ok(CollectionAnalytics {
            collection_id: collection_id.to_string(),
            total_minted: collection.minted_count,
            total_supply: collection.total_supply,
            max_supply: collection.max_supply,
            unique_owners: collection.stats.unique_owners,
            floor_price: collection.stats.floor_price,
            total_volume: collection.stats.total_volume,
            average_price: collection.stats.average_price,
            total_sales: collection.stats.total_sales,
            volume_24h: collection.stats.volume_24h,
            volume_7d: collection.stats.volume_7d,
            volume_30d: collection.stats.volume_30d,
            listed_count: 0, // Would need marketplace integration
            mint_completion_percent: if let Some(max) = collection.max_supply {
                (collection.minted_count as f64 / max as f64) * 100.0
            } else {
                0.0
            },
        })
    }

    // Helper methods

    fn validate_collection(&self, collection: &NftCollection) -> Result<()> {
        if collection.name.is_empty() {
            return Err(anyhow!("Collection name is required"));
        }

        if collection.symbol.is_empty() {
            return Err(anyhow!("Collection symbol is required"));
        }

        if collection.royalty_config.creator_royalty_percent > 100.0 {
            return Err(anyhow!("Creator royalty cannot exceed 100%"));
        }

        if collection.royalty_config.platform_fee_percent > 100.0 {
            return Err(anyhow!("Platform fee cannot exceed 100%"));
        }

        let total_split: f64 = collection
            .royalty_config
            .royalty_splits
            .iter()
            .map(|s| s.percentage)
            .sum();

        if total_split > 100.0 {
            return Err(anyhow!("Total royalty splits cannot exceed 100%"));
        }

        Ok(())
    }

    fn generate_collection_id(&self, collection: &NftCollection) -> String {
        let data = format!(
            "{}{}{}",
            collection.name, collection.symbol, collection.creator
        );
        hex::encode(blake3::hash(data.as_bytes()).as_bytes())
    }
}

/// Collection update parameters
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CollectionUpdate {
    pub description: Option<String>,
    pub image: Option<String>,
    pub external_url: Option<String>,
    pub social_links: Option<SocialLinks>,
    pub royalty_config: Option<RoyaltyConfig>,
}

/// Sale data for statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleData {
    pub price: u128,
    pub timestamp: DateTime<Utc>,
    pub buyer: QuantumDID,
    pub seller: QuantumDID,
}

/// Collection analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionAnalytics {
    pub collection_id: String,
    pub total_minted: u64,
    pub total_supply: u64,
    pub max_supply: Option<u64>,
    pub unique_owners: u64,
    pub floor_price: Option<u128>,
    pub total_volume: u128,
    pub average_price: Option<u128>,
    pub total_sales: u64,
    pub volume_24h: u128,
    pub volume_7d: u128,
    pub volume_30d: u128,
    pub listed_count: u64,
    pub mint_completion_percent: f64,
}

impl Default for RoyaltyConfig {
    fn default() -> Self {
        Self {
            creator_royalty_percent: 5.0, // 5% default royalty
            creator_address: QuantumDID::new("did:spacekit:test_creator".to_string()),
            platform_fee_percent: 2.5, // 2.5% platform fee
            royalty_splits: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collection_validation() {
        let collection = NftCollection {
            collection_id: String::new(),
            name: "Test Collection".to_string(),
            symbol: "TEST".to_string(),
            description: "A test collection".to_string(),
            image: "https://example.com/image.png".to_string(),
            banner_image: None,
            featured_image: None,
            external_url: None,
            creator: QuantumDID::new("did:spacekit:test_creator".to_string()),
            verified_creator: false,
            total_supply: 0,
            max_supply: Some(10000),
            minted_count: 0,
            royalty_config: RoyaltyConfig::default(),
            properties: CollectionProperties {
                category: CollectionCategory::Art,
                revealed: false,
                reveal_date: None,
                base_uri: None,
                token_standard: TokenStandard::ERC721,
                network: "SPACEKIT".to_string(),
                contract_address: None,
            },
            social_links: SocialLinks::default(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            stats: CollectionStats::default(),
            quantum_signature: None,
        };

        // Should be valid
        assert!(collection.royalty_config.creator_royalty_percent <= 100.0);
        assert!(collection.royalty_config.platform_fee_percent <= 100.0);
    }
}
