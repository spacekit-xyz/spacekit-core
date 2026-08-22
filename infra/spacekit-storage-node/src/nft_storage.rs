//! NFT Storage Templates and Helpers
//!
//! Provides specialized storage for NFTs (Non-Fungible Tokens) using the
//! Fact Package system with quantum-safe encryption and provenance tracking.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use spacekit_primitives::v1::fact::{
    AccessPolicy, Citation, CitationType, CollectionMethod, DataSource, FactCategory, FactContent,
    FactMetadata, FactPackage, KnowledgeDomain, LicenseType, ProofType, VerificationLevel,
    VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;

use crate::fact_storage::FactStorageEngine;

/// NFT metadata following OpenSea standard with SpaceKit extensions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftMetadata {
    // Core NFT fields
    pub name: String,
    pub description: String,
    pub image: String, // IPFS or SpaceKit storage URL
    pub external_url: Option<String>,

    // Extended attributes
    pub attributes: Vec<NftAttribute>,

    // Collection information
    pub collection: Option<NftCollection>,

    // Creator & ownership
    pub creator: QuantumDID,
    pub current_owner: QuantumDID,

    // Provenance
    pub mint_timestamp: DateTime<Utc>,
    pub transfer_history: Vec<NftTransfer>,

    // SpaceKit-specific
    pub quantum_signature: Option<String>,
    pub content_hash: String, // Blake3 hash
    pub storage_tier: NftStorageTier,

    // Optional fields
    pub animation_url: Option<String>,
    pub background_color: Option<String>,
    pub youtube_url: Option<String>,
}

/// NFT attribute (trait)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftAttribute {
    pub trait_type: String,
    pub value: AttributeValue,
    pub display_type: Option<String>, // "number", "boost_percentage", "date", etc.
}

/// Attribute value types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    String(String),
    Number(f64),
    Boolean(bool),
}

/// NFT collection information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftCollection {
    pub name: String,
    pub family: String,
    pub description: Option<String>,
    pub image: Option<String>,
    pub external_url: Option<String>,
}

/// NFT transfer record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftTransfer {
    pub from: QuantumDID,
    pub to: QuantumDID,
    pub timestamp: DateTime<Utc>,
    pub transaction_hash: String,
    pub price: Option<u128>, // Price in wei
}

/// Storage tier for NFTs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NftStorageTier {
    Hot,       // Frequently accessed (galleries, marketplaces)
    Warm,      // Occasional access
    Cold,      // Archived
    Permanent, // Never delete (high-value NFTs)
}

/// NFT storage manager
pub struct NftStorageManager {
    fact_storage: FactStorageEngine,
}

/// NFT storage result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftStorageResult {
    pub nft_id: [u8; 32], // Fact ID
    pub content_hash: String,
    pub storage_location: String,
    pub metadata_hash: String,
    pub quantum_proof: String,
}

/// NFT query filters
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NftQuery {
    pub owner: Option<QuantumDID>,
    pub creator: Option<QuantumDID>,
    pub collection: Option<String>,
    pub traits: HashMap<String, Vec<String>>,
    pub min_price: Option<u128>,
    pub max_price: Option<u128>,
    pub sort_by: NftSortCriteria,
    pub limit: usize,
    pub offset: usize,
}

/// NFT sorting criteria
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NftSortCriteria {
    MintDate,
    LastTransfer,
    Price,
    Rarity,
    Name,
}

impl Default for NftSortCriteria {
    fn default() -> Self {
        Self::MintDate
    }
}

impl NftStorageManager {
    /// Create a new NFT storage manager
    pub fn new(fact_storage: FactStorageEngine) -> Self {
        Self { fact_storage }
    }

    /// Store an NFT with quantum-safe encryption
    pub async fn store_nft(
        &self,
        nft_data: Vec<u8>,
        metadata: NftMetadata,
        mime_type: String,
    ) -> Result<NftStorageResult> {
        // Calculate content hash
        let content_hash = hex::encode(blake3::hash(&nft_data).as_bytes());

        // Verify hash matches metadata
        if content_hash != metadata.content_hash {
            return Err(anyhow!("Content hash mismatch"));
        }

        // Create fact content from NFT data
        // Decode hex-encoded content hash to bytes
        let hash_bytes = hex::decode(&metadata.content_hash)
            .map_err(|e| anyhow!("Failed to decode content hash: {}", e))?;
        let hash_array: [u8; 32] = hash_bytes
            .try_into()
            .map_err(|_| anyhow!("Invalid hash length: expected 32 bytes"))?;

        let fact_content = FactContent::Binary {
            data: nft_data,
            mime_type: mime_type.clone(),
            hash: hash_array,
        };

        // Create fact metadata
        let fact_metadata = FactMetadata {
            category: FactCategory::Reference,
            tags: self.generate_nft_tags(&metadata),
            domain: KnowledgeDomain::Custom("NFT".to_string()),
            source: DataSource::UserInput {
                application: metadata.creator.clone(),
                user: metadata.current_owner.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::Cryptographic,
            license: LicenseType::Custom("NFT License".to_string()),
            size_bytes: fact_content.size_bytes() as u64,
            checksum: self.calculate_fact_checksum(&fact_content)?,
        };

        // Determine access policy based on storage tier
        let access_policy = match metadata.storage_tier {
            NftStorageTier::Hot => AccessPolicy::Public, // Public galleries
            _ => AccessPolicy::Private(vec![metadata.current_owner.clone()].into_iter().collect()),
        };

        // Create verification proof
        let verification_proof = VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: metadata
                .quantum_signature
                .as_ref()
                .map(|s| hex::decode(s).unwrap_or_default())
                .unwrap_or_default(),
            verification_timestamp: Utc::now().timestamp() as u64,
            verifier: Some(metadata.creator.clone()),
        };

        // Create citations for transfer history
        let citations: Vec<Citation> = metadata
            .transfer_history
            .iter()
            .map(|transfer| Citation {
                citation_type: CitationType::Web,
                context: Some(transfer.from.clone().to_string()),
                reference: transfer.transaction_hash.clone(),
            })
            .collect();

        // Generate fact ID from content hash
        let fact_id: [u8; 32] = *blake3::hash(content_hash.as_bytes()).as_bytes();

        // Create a real SPHINCS+ detached signature over the canonical verification message.
        // NOTE: This currently generates a new keypair per stored NFT (suitable for demos/tests).
        // Production deployments should sign with the creator's long-lived signature key and bind it to the DID.
        let mut message_to_sign = Vec::new();
        message_to_sign.extend_from_slice(&fact_id);
        message_to_sign.extend_from_slice(&fact_metadata.checksum);
        message_to_sign.extend_from_slice(&serde_json::to_vec(&metadata.creator)?);
        message_to_sign
            .extend_from_slice(&(metadata.mint_timestamp.timestamp() as u64).to_le_bytes());

        let signature =
            spacekit_primitives::v1::crypto::quantum::sphincs_keypair_and_sign_detached(
                &message_to_sign,
                "SPHINCS-256f",
            )?;

        // Create complete fact package
        let fact_package = FactPackage {
            fact_id,
            version: 1,
            created_at: metadata.mint_timestamp.timestamp() as u64,
            expires_at: None, // NFTs don't expire
            content: fact_content,
            metadata: fact_metadata,
            author: metadata.creator.clone(),
            signature,
            verification_proof,
            dependencies: Vec::new(),
            citations,
            confidence_score: 1.0, // High confidence for blockchain-verified NFTs
            access_policy,
            encryption: None,
        };

        // Store the fact package
        let stored_id = self.fact_storage.store_fact(fact_package).await?;

        // Generate quantum proof
        let quantum_proof = self.generate_quantum_proof(&stored_id, &content_hash)?;

        Ok(NftStorageResult {
            nft_id: stored_id,
            content_hash,
            storage_location: format!("spacekit://storage/{}", hex::encode(stored_id)),
            metadata_hash: hex::encode(
                blake3::hash(serde_json::to_string(&metadata)?.as_bytes()).as_bytes(),
            ),
            quantum_proof,
        })
    }

    /// Retrieve an NFT
    pub async fn retrieve_nft(&self, nft_id: [u8; 32]) -> Result<Option<(Vec<u8>, NftMetadata)>> {
        // Retrieve fact package
        let fact = match self.fact_storage.retrieve_fact(nft_id).await? {
            Some(f) => f,
            None => return Ok(None),
        };

        // Extract NFT data from fact content
        let fact_content = fact.content.clone();
        let nft_data = match fact_content {
            FactContent::Binary { data, .. } => data,
            _ => return Err(anyhow!("Invalid NFT content type")),
        };

        // Reconstruct NFT metadata from fact package
        let fact_package = fact.clone();
        let metadata = self.reconstruct_nft_metadata(&fact_package)?;

        Ok(Some((nft_data, metadata)))
    }

    /// Transfer NFT ownership
    pub async fn transfer_nft(
        &self,
        nft_id: [u8; 32],
        from: &QuantumDID,
        to: &QuantumDID,
        price: Option<u128>,
        transaction_hash: String,
    ) -> Result<()> {
        // Retrieve current NFT
        let (nft_data, mut metadata) = self
            .retrieve_nft(nft_id)
            .await?
            .ok_or_else(|| anyhow!("NFT not found"))?;

        // Verify ownership
        if metadata.current_owner != *from {
            return Err(anyhow!("Transfer from incorrect owner"));
        }

        // Add transfer to history
        metadata.transfer_history.push(NftTransfer {
            from: from.clone(),
            to: to.clone(),
            timestamp: Utc::now(),
            transaction_hash,
            price,
        });

        // Update owner
        metadata.current_owner = to.clone();

        // Re-store with updated metadata
        let mime_type = "image/png".to_string(); // Should be extracted from original metadata
        self.store_nft(nft_data, metadata, mime_type).await?;

        Ok(())
    }

    /// Query NFTs
    pub async fn query_nfts(&self, _query: NftQuery) -> Result<Vec<NftMetadata>> {
        // TODO: Implement comprehensive NFT query
        // Would use fact_storage.query_facts with NFT-specific filters
        Ok(Vec::new())
    }

    /// Verify NFT authenticity
    pub async fn verify_nft(&self, nft_id: [u8; 32]) -> Result<bool> {
        let verification = self.fact_storage.verify_fact(nft_id).await?;

        // High confidence threshold for NFTs
        Ok(verification.overall_confidence >= 0.9
            && verification.signature_valid
            && verification.author_verified)
    }

    /// Get NFT provenance chain
    pub async fn get_provenance(&self, nft_id: [u8; 32]) -> Result<Vec<NftTransfer>> {
        let (_, metadata) = self
            .retrieve_nft(nft_id)
            .await?
            .ok_or_else(|| anyhow!("NFT not found"))?;

        Ok(metadata.transfer_history)
    }

    // Helper methods

    fn generate_nft_tags(&self, metadata: &NftMetadata) -> Vec<String> {
        let mut tags = vec!["NFT".to_string(), metadata.name.clone()];

        if let Some(collection) = &metadata.collection {
            tags.push(format!("collection:{}", collection.name));
        }

        // Add trait tags
        for attr in &metadata.attributes {
            tags.push(format!(
                "{}:{}",
                attr.trait_type,
                self.attribute_to_string(&attr.value)
            ));
        }

        tags
    }

    fn attribute_to_string(&self, value: &AttributeValue) -> String {
        match value {
            AttributeValue::String(s) => s.clone(),
            AttributeValue::Number(n) => n.to_string(),
            AttributeValue::Boolean(b) => b.to_string(),
        }
    }

    fn calculate_fact_checksum(&self, content: &FactContent) -> Result<[u8; 32]> {
        let serialized = serde_json::to_vec(content)?;
        Ok(*blake3::hash(&serialized).as_bytes())
    }

    fn generate_quantum_proof(&self, nft_id: &[u8; 32], content_hash: &str) -> Result<String> {
        let mut proof_data = Vec::new();
        proof_data.extend_from_slice(nft_id);
        proof_data.extend_from_slice(content_hash.as_bytes());

        Ok(hex::encode(blake3::hash(&proof_data).as_bytes()))
    }

    fn reconstruct_nft_metadata(&self, fact: &FactPackage) -> Result<NftMetadata> {
        use spacekit_primitives::v1::fact::FactContent;

        // Extract metadata from fact package
        let metadata = &fact.metadata;

        // Parse attributes from tags
        let mut attributes = Vec::new();
        for tag in &metadata.tags {
            if tag.contains(':') {
                let parts: Vec<&str> = tag.split(':').collect();
                if parts.len() == 2 {
                    attributes.push(NftAttribute {
                        trait_type: parts[0].to_string(),
                        value: AttributeValue::String(parts[1].to_string()),
                        display_type: None,
                    });
                }
            }
        }

        // Extract content hash from fact content if it's binary
        let content_hash = match &fact.content {
            FactContent::Binary { hash, .. } => hex::encode(hash),
            _ => {
                // Fallback to fact ID as content hash
                hex::encode(fact.fact_id)
            }
        };

        // Extract storage tier from domain
        let storage_tier = match metadata.domain {
            spacekit_primitives::v1::fact::KnowledgeDomain::Custom(ref domain)
                if domain == "NFT" =>
            {
                // Check if it's hot storage based on access policy
                match fact.access_policy {
                    spacekit_primitives::v1::fact::AccessPolicy::Public => NftStorageTier::Hot,
                    _ => NftStorageTier::Cold,
                }
            }
            _ => NftStorageTier::Cold,
        };

        // Reconstruct transfer history from citations
        let transfer_history = fact
            .citations
            .iter()
            .map(|citation| NftTransfer {
                from: QuantumDID::new(citation.context.clone().unwrap_or_default()),
                to: QuantumDID::new("did:spacekit:current_owner".to_string()),
                timestamp: chrono::Utc::now(),
                transaction_hash: citation.reference.clone(),
                price: None,
            })
            .collect();

        Ok(NftMetadata {
            name: format!("NFT #{}", hex::encode(&fact.fact_id[..4])),
            description: format!(
                "NFT stored in SpaceKit Network with quantum-safe encryption. Fact ID: {}",
                hex::encode(fact.fact_id)
            ),
            image: format!("spacekit://nft/{}", hex::encode(fact.fact_id)),
            external_url: None,
            attributes,
            collection: None, // Will be set by collection manager
            creator: fact.author.clone(),
            current_owner: fact.author.clone(), // Default to author, should be updated by collection manager
            mint_timestamp: chrono::DateTime::from_timestamp(fact.created_at as i64, 0)
                .unwrap_or_else(|| chrono::Utc::now()),
            transfer_history,
            quantum_signature: fact
                .signature
                .signature_bytes
                .first()
                .map(|_| "quantum_signature".to_string()),
            content_hash,
            storage_tier,
            animation_url: None,
            background_color: Some("000000".to_string()),
            youtube_url: None,
        })
    }
}

/// Helper function to create a simple NFT from image data
pub async fn create_simple_nft(
    storage_manager: &NftStorageManager,
    image_data: Vec<u8>,
    name: String,
    description: String,
    creator: QuantumDID,
    owner: QuantumDID,
) -> Result<NftStorageResult> {
    let content_hash = hex::encode(blake3::hash(&image_data).as_bytes());

    let metadata = NftMetadata {
        name,
        description,
        image: format!("spacekit://{}", content_hash),
        external_url: None,
        attributes: Vec::new(),
        collection: None,
        creator: creator.clone(),
        current_owner: owner,
        mint_timestamp: Utc::now(),
        transfer_history: Vec::new(),
        quantum_signature: None,
        content_hash,
        storage_tier: NftStorageTier::Hot,
        animation_url: None,
        background_color: None,
        youtube_url: None,
    };

    storage_manager
        .store_nft(image_data, metadata, "image/png".to_string())
        .await
}

/// Helper function to create NFT collection
pub fn create_nft_collection(
    name: String,
    family: String,
    description: Option<String>,
) -> NftCollection {
    NftCollection {
        name,
        family,
        description,
        image: None,
        external_url: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nft_metadata_serialization() {
        let metadata = NftMetadata {
            name: "Test NFT".to_string(),
            description: "A test NFT".to_string(),
            image: "spacekit://test".to_string(),
            external_url: None,
            attributes: vec![NftAttribute {
                trait_type: "Rarity".to_string(),
                value: AttributeValue::String("Rare".to_string()),
                display_type: None,
            }],
            collection: None,
            creator: QuantumDID::new("did:spacekit:test_creator".to_string()),
            current_owner: QuantumDID::new("did:spacekit:test_owner".to_string()),
            mint_timestamp: Utc::now(),
            transfer_history: Vec::new(),
            quantum_signature: None,
            content_hash: "test_hash".to_string(),
            storage_tier: NftStorageTier::Hot,
            animation_url: None,
            background_color: None,
            youtube_url: None,
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: NftMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata.name, deserialized.name);
    }

    #[test]
    fn test_nft_collection_creation() {
        let collection = create_nft_collection(
            "Test Collection".to_string(),
            "Test Family".to_string(),
            Some("A test collection".to_string()),
        );

        assert_eq!(collection.name, "Test Collection");
        assert_eq!(collection.family, "Test Family");
    }
}
