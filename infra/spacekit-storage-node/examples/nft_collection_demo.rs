#![recursion_limit = "256"]

//! NFT Collection Management Demo
//!
//! Demonstrates comprehensive NFT collection features:
//! - Creating collections with metadata
//! - Minting NFTs to collections
//! - Royalty configuration
//! - Rarity calculation
//! - Collection analytics
//! - Floor price tracking

use anyhow::Result;
use chrono::Utc;
use std::sync::Arc;

use spacekit_primitives::v1::identity::QuantumDID;
use spacekit_storage_node::{
    nft_storage::AttributeValue, nft_storage::NftStorageTier, CollectionCategory,
    CollectionProperties, CollectionQuery, CollectionSortCriteria, FactStorageConfig,
    FactStorageEngine, MintConfig, NftAttribute, NftCollection, NftCollectionManager, NftMetadata,
    NftStorageManager, RoyaltyConfig, RoyaltySplit, SaleData, SocialLinks, StorageNode,
    StorageNodeConfig, TokenStandard,
};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("\n🎨 SpaceKit NFT Collection Manager - Complete Demo\n");
    println!("{}", "=".repeat(70));

    // Setup storage node
    println!("\n📦 1. Setting up Storage Infrastructure...\n");

    let config = StorageNodeConfig {
        max_storage_bytes: 10 * 1024 * 1024 * 1024, // 10GB
        data_dir: std::path::PathBuf::from("./nft_demo_storage"),
        database_path: Some(std::path::PathBuf::from("./nft_demo_storage/nft_demo.json")),
        node_did: "did:spacekit:storage:nft-demo".to_string(),
        preferred_algorithm: "kyber1024".to_string(),
        encryption_keypair: None,
        network_config: Default::default(),
        enable_p2p: false,
        enable_real_transactions: false,
        #[cfg(feature = "api-server")]
        api_config: None,
    };

    let storage_node = Arc::new(StorageNode::new(config).await?);
    storage_node.start().await?;
    println!("✓ Storage node initialized");

    // Create NFT infrastructure
    let database = storage_node.database();
    let quantum_crypto = storage_node.quantum_crypto();
    let fact_config = FactStorageConfig::default();
    let fact_storage =
        FactStorageEngine::new(database.clone(), quantum_crypto, fact_config).await?;

    let nft_storage = Arc::new(NftStorageManager::new(fact_storage));
    let collection_manager = NftCollectionManager::new(nft_storage.clone());

    println!("✓ NFT infrastructure ready");

    // CREATE NFT COLLECTION
    println!("\n🎨 2. Creating NFT Collection...\n");

    let creator = QuantumDID::new("did:spacekit:test_creator".to_string());

    let collection = NftCollection {
        collection_id: String::new(), // Will be generated
        name: "Quantum Art Gallery".to_string(),
        symbol: "QAG".to_string(),
        description:
            "A curated collection of quantum-safe digital art stored forever on SpaceKit Network."
                .to_string(),
        image: "https://spacekit.xyz/collections/quantum-art/logo.png".to_string(),
        banner_image: Some("https://spacekit.xyz/collections/quantum-art/banner.png".to_string()),
        featured_image: Some(
            "https://spacekit.xyz/collections/quantum-art/featured.png".to_string(),
        ),
        external_url: Some("https://quantumartgallery.spacekit.xyz".to_string()),

        creator: creator.clone(),
        verified_creator: true,

        total_supply: 0,
        max_supply: Some(10000), // Limited edition
        minted_count: 0,

        royalty_config: RoyaltyConfig {
            creator_royalty_percent: 7.5, // 7.5% creator royalty
            creator_address: creator.clone(),
            platform_fee_percent: 2.5, // 2.5% platform fee
            royalty_splits: vec![
                RoyaltySplit {
                    address: creator.clone(),
                    percentage: 5.0, // 5% to creator
                    description: Some("Primary creator".to_string()),
                },
                RoyaltySplit {
                    address: QuantumDID::new("did:spacekit:test_community".to_string()), // Community wallet
                    percentage: 2.5, // 2.5% to community
                    description: Some("Community treasury".to_string()),
                },
            ],
        },

        properties: CollectionProperties {
            category: CollectionCategory::Art,
            revealed: true,
            reveal_date: None,
            base_uri: Some("https://spacekit.xyz/collections/quantum-art/".to_string()),
            token_standard: TokenStandard::ERC721,
            network: "SPACEKIT".to_string(),
            contract_address: None,
        },

        social_links: SocialLinks {
            website: Some("https://quantumartgallery.spacekit.xyz".to_string()),
            discord: Some("https://discord.gg/quantumart".to_string()),
            twitter: Some("https://twitter.com/quantumart".to_string()),
            instagram: Some("https://instagram.com/quantumart".to_string()),
            telegram: None,
            medium: Some("https://medium.com/@quantumart".to_string()),
        },

        created_at: Utc::now(),
        updated_at: Utc::now(),
        stats: Default::default(),
        quantum_signature: None,
    };

    let collection_id = collection_manager.create_collection(collection).await?;

    println!("✓ Collection created successfully!");
    println!("   Collection ID: {}", collection_id);
    println!("   Name: Quantum Art Gallery");
    println!("   Symbol: QAG");
    println!("   Max Supply: 10,000 NFTs");
    println!("   Creator Royalty: 7.5%");
    println!("   Platform Fee: 2.5%");

    // MINT NFTs TO COLLECTION
    println!("\n🖼️  3. Minting NFTs to Collection...\n");

    println!("Minting NFT #1: 'Quantum Dreams'");

    let mut nft1_metadata:NftMetadata = NftMetadata {
        name: "Quantum Dreams #1".to_string(),
        description: "The first piece in the Quantum Dreams series - a mesmerizing blend of quantum mechanics and digital art.".to_string(),
        image: "spacekit://quantum-dreams-1".to_string(),
        external_url: None,
        attributes: vec![
            NftAttribute {
                trait_type: "Rarity".to_string(),
                value: AttributeValue::String("Legendary".to_string()),
                display_type: None,
            },
            NftAttribute {
                trait_type: "Artist".to_string(),
                value: AttributeValue::String("QuantumCreator".to_string()),
                display_type: None,
            },
            NftAttribute {
                trait_type: "Background".to_string(),
                value: AttributeValue::String("Nebula".to_string()),
                display_type: None,
            },
            NftAttribute {
                trait_type: "Edition".to_string(),
                value: AttributeValue::Number(1.0),
                display_type: Some("number".to_string()),
            },
            NftAttribute {
                trait_type: "Quantum Entanglement".to_string(),
                value: AttributeValue::Boolean(true),
                display_type: None,
            },
        ],
        collection: None, // Will be set automatically
        creator: creator.clone(),
        current_owner: creator.clone(),
        mint_timestamp: Utc::now(),
        transfer_history: Vec::new(),
        quantum_signature: None,
        content_hash: hex::encode(blake3::hash(b"quantum-dreams-1").as_bytes()),
        storage_tier: NftStorageTier::Hot,
        animation_url: None,
        background_color: Some("000033".to_string()),
        youtube_url: None,
    };

    let nft1_data = b"Quantum Dreams #1 Image Data".to_vec();
    let nft1_content_hash = hex::encode(blake3::hash(&nft1_data).as_bytes());

    // Update metadata with correct content hash
    nft1_metadata.content_hash = nft1_content_hash;

    let mint_config1 = MintConfig {
        collection_id: collection_id.clone(),
        token_id: 1,
        metadata: nft1_metadata,
        mint_price: Some(1_000_000_000_000_000_000), // 1 ASTRA in wei
    };

    let nft1_id = collection_manager
        .mint_to_collection(mint_config1, nft1_data, "image/png".to_string())
        .await?;

    println!("✓ NFT #1 minted successfully!");
    println!("   NFT ID: {}", hex::encode(nft1_id));
    println!("   Rarity: Legendary");
    println!("   Mint Price: 1 ASTRA");

    // Mint more NFTs with different rarities
    println!("\nMinting NFT #2: 'Quantum Waves'");

    let mut nft2_metadata = NftMetadata {
        name: "Quantum Waves #2".to_string(),
        description: "Ripples through quantum foam captured in digital form.".to_string(),
        image: "spacekit://quantum-waves-2".to_string(),
        external_url: None,
        attributes: vec![
            NftAttribute {
                trait_type: "Rarity".to_string(),
                value: AttributeValue::String("Rare".to_string()),
                display_type: None,
            },
            NftAttribute {
                trait_type: "Artist".to_string(),
                value: AttributeValue::String("QuantumCreator".to_string()),
                display_type: None,
            },
            NftAttribute {
                trait_type: "Background".to_string(),
                value: AttributeValue::String("Ocean".to_string()),
                display_type: None,
            },
            NftAttribute {
                trait_type: "Edition".to_string(),
                value: AttributeValue::Number(2.0),
                display_type: Some("number".to_string()),
            },
        ],
        collection: None,
        creator: creator.clone(),
        current_owner: creator.clone(),
        mint_timestamp: Utc::now(),
        transfer_history: Vec::new(),
        quantum_signature: None,
        content_hash: hex::encode(blake3::hash(b"quantum-waves-2").as_bytes()),
        storage_tier: NftStorageTier::Hot,
        animation_url: None,
        background_color: Some("003366".to_string()),
        youtube_url: None,
    };

    let nft2_data = b"Quantum Waves #2 Image Data".to_vec();
    let nft2_content_hash = hex::encode(blake3::hash(&nft2_data).as_bytes());

    // Update metadata with correct content hash
    nft2_metadata.content_hash = nft2_content_hash;

    let mint_config2 = MintConfig {
        collection_id: collection_id.clone(),
        token_id: 2,
        metadata: nft2_metadata,
        mint_price: Some(500_000_000_000_000_000), // 0.5 ASTRA
    };

    let _nft2_id = collection_manager
        .mint_to_collection(mint_config2, nft2_data, "image/png".to_string())
        .await?;

    println!("✓ NFT #2 minted successfully!");
    println!("   Rarity: Rare");
    println!("   Mint Price: 0.5 ASTRA");

    // GET COLLECTION INFO
    println!("\n📊 4. Collection Information...\n");

    if let Some(collection) = collection_manager.get_collection(&collection_id).await? {
        println!("Collection: {}", collection.name);
        println!("   ID: {}", collection.collection_id);
        println!("   Symbol: {}", collection.symbol);
        println!(
            "   Minted: {} / {}",
            collection.minted_count,
            collection.max_supply.unwrap_or(0)
        );
        println!(
            "   Creator Royalty: {}%",
            collection.royalty_config.creator_royalty_percent
        );
        println!(
            "   Platform Fee: {}%",
            collection.royalty_config.platform_fee_percent
        );
        println!("   Category: {:?}", collection.properties.category);
        println!("   Standard: {:?}", collection.properties.token_standard);

        if !collection.social_links.website.is_none() {
            println!("\n   Social Links:");
            if let Some(website) = &collection.social_links.website {
                println!("   - Website: {}", website);
            }
            if let Some(discord) = &collection.social_links.discord {
                println!("   - Discord: {}", discord);
            }
            if let Some(twitter) = &collection.social_links.twitter {
                println!("   - Twitter: {}", twitter);
            }
        }
    }

    // CALCULATE RARITY
    println!("\n🎲 5. Calculating Rarity Scores...\n");

    let rarity_scores = collection_manager.calculate_rarity(&collection_id).await?;

    println!("Rarity Rankings:");
    for (i, score) in rarity_scores.iter().enumerate() {
        println!("   Rank #{}: Score {:.2}", i + 1, score.score);
        for (trait_name, trait_score) in &score.trait_scores {
            println!("      - {}: {:.2}", trait_name, trait_score);
        }
    }

    // UPDATE COLLECTION STATS (simulate sales)
    println!("\n💰 6. Updating Collection Stats (Simulated Sales)...\n");

    let sale1 = SaleData {
        price: 2_000_000_000_000_000_000, // 2 ASTRA
        timestamp: Utc::now(),
        buyer: QuantumDID::new("did:spacekit:test_buyer_1".to_string()),
        seller: creator.clone(),
    };

    collection_manager
        .update_collection_stats(&collection_id, sale1)
        .await?;
    println!("✓ Sale #1 recorded: 2 ASTRA");

    let sale2 = SaleData {
        price: 1_500_000_000_000_000_000, // 1.5 ASTRA
        timestamp: Utc::now(),
        buyer: QuantumDID::new("did:spacekit:test_buyer_2".to_string()),
        seller: creator.clone(),
    };

    collection_manager
        .update_collection_stats(&collection_id, sale2)
        .await?;
    println!("✓ Sale #2 recorded: 1.5 ASTRA");

    // GET COLLECTION ANALYTICS
    println!("\n📈 7. Collection Analytics...\n");

    let analytics = collection_manager
        .get_collection_analytics(&collection_id)
        .await?;

    println!("Analytics for Quantum Art Gallery:");
    println!("   Total Minted: {}", analytics.total_minted);
    println!("   Total Supply: {}", analytics.total_supply);
    println!("   Max Supply: {}", analytics.max_supply.unwrap_or(0));
    println!("   Completion: {:.1}%", analytics.mint_completion_percent);

    if let Some(floor_price) = analytics.floor_price {
        println!(
            "   Floor Price: {} wei ({:.2} ASTRA)",
            floor_price,
            floor_price as f64 / 1e18
        );
    }

    println!(
        "   Total Volume: {} wei ({:.2} ASTRA)",
        analytics.total_volume,
        analytics.total_volume as f64 / 1e18
    );

    if let Some(avg_price) = analytics.average_price {
        println!(
            "   Average Price: {} wei ({:.2} ASTRA)",
            avg_price,
            avg_price as f64 / 1e18
        );
    }

    println!("   Total Sales: {}", analytics.total_sales);
    println!(
        "   24h Volume: {} wei ({:.2} ASTRA)",
        analytics.volume_24h,
        analytics.volume_24h as f64 / 1e18
    );

    // QUERY COLLECTIONS
    println!("\n🔍 8. Querying Collections...\n");

    let query = CollectionQuery {
        category: Some(CollectionCategory::Art),
        min_floor_price: None,
        max_floor_price: None,
        min_volume: None,
        verified_only: true,
        sort_by: CollectionSortCriteria::Volume,
        limit: 10,
        offset: 0,
    };

    let collections = collection_manager.query_collections(query).await?;
    println!("Found {} Art collections", collections.len());

    for collection in collections {
        println!("\n   Collection: {}", collection.name);
        println!(
            "   - Minted: {}/{}",
            collection.minted_count,
            collection.max_supply.unwrap_or(0)
        );
        println!(
            "   - Total Volume: {:.2} ASTRA",
            collection.stats.total_volume as f64 / 1e18
        );
    }

    // ROYALTY CALCULATION DEMO
    println!("\n💎 9. Royalty Calculation Demo...\n");

    if let Some(collection) = collection_manager.get_collection(&collection_id).await? {
        let sale_price: u128 = 10_000_000_000_000_000_000; // 10 ASTRA

        println!("For a sale of {} ASTRA:", sale_price as f64 / 1e18);

        let creator_royalty =
            (sale_price as f64 * collection.royalty_config.creator_royalty_percent / 100.0) as u128;
        let platform_fee =
            (sale_price as f64 * collection.royalty_config.platform_fee_percent / 100.0) as u128;
        let seller_proceeds = sale_price - creator_royalty - platform_fee;

        println!(
            "   Creator Royalty: {} wei ({:.2} ASTRA)",
            creator_royalty,
            creator_royalty as f64 / 1e18
        );
        println!(
            "   Platform Fee: {} wei ({:.2} ASTRA)",
            platform_fee,
            platform_fee as f64 / 1e18
        );
        println!(
            "   Seller Proceeds: {} wei ({:.2} ASTRA)",
            seller_proceeds,
            seller_proceeds as f64 / 1e18
        );

        if !collection.royalty_config.royalty_splits.is_empty() {
            println!("\n   Royalty Distribution:");
            for split in &collection.royalty_config.royalty_splits {
                let split_amount = (creator_royalty as f64 * split.percentage
                    / collection.royalty_config.creator_royalty_percent)
                    as u128;
                println!(
                    "   - {}: {} wei ({:.2} ASTRA)",
                    split.description.as_ref().unwrap_or(&"Unknown".to_string()),
                    split_amount,
                    split_amount as f64 / 1e18
                );
            }
        }
    }

    // SUMMARY
    println!("\n{}", "=".repeat(2));
    println!("{}", "=".repeat(70));
    println!("✓ NFT Collection Demo Complete!");
    println!("{}", "=".repeat(70));

    println!("\n📝 Summary:");
    println!("   • Created NFT collection with comprehensive metadata");
    println!("   • Minted 2 NFTs with different rarity traits");
    println!("   • Configured royalty splits (7.5% creator + 2.5% platform)");
    println!("   • Calculated rarity scores for collection");
    println!("   • Tracked sales and updated statistics");
    println!("   • Generated collection analytics");
    println!("   • Demonstrated royalty calculations");

    println!("\n🎨 Collection Features:");
    println!("   ✓ Quantum-safe storage with Kyber1024 encryption");
    println!("   ✓ Automatic royalty enforcement");
    println!("   ✓ Rarity calculation and ranking");
    println!("   ✓ Real-time floor price tracking");
    println!("   ✓ Comprehensive analytics");
    println!("   ✓ Social media integration");
    println!("   ✓ OpenSea-compatible metadata");

    println!("\n💡 Next Steps:");
    println!("   1. Deploy collections to mainnet");
    println!("   2. Integrate with marketplace for trading");
    println!("   3. Add batch minting capabilities");
    println!("   4. Implement reveal mechanics for unrevealed collections");
    println!("   5. Add collection verification process");

    println!("\n🌟 Thank you for using SpaceKit NFT Collection Manager!\n");

    Ok(())
}
