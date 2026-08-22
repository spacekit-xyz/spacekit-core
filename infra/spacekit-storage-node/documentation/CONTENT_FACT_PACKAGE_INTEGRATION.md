# Content Publishing with Fact Package System & Smart Contract Governance

## Overview

This document outlines how content publishing integrates with the Fact Package system and smart contract governance for storage and P2P distribution.

## Architecture

### 1. Content as Fact Packages

All published content is stored as **Fact Packages** with comprehensive metadata:

```rust
FactPackage {
    fact_id: ContentID,           // Unique content identifier
    content: FactContent::Binary {
        data: encrypted_mp4_bytes,
        mime_type: "video/mp4",
        hash: content_hash,
    },
    metadata: FactMetadata {
        category: FactCategory::UserGenerated,
        tags: vec!["video", "tutorial", "education"],
        domain: KnowledgeDomain::ComputerScience,
        source: DataSource::UserInput {
            application: channel_did,
            user: publisher_did,
        },
        collection_method: CollectionMethod::Manual,
        verification_level: VerificationLevel::SelfClaimed,
        license: LicenseType::Proprietary,
        size_bytes: file_size,
        checksum: content_hash,
    },
    access_policy: AccessPolicy::Conditional(vec![
        AccessCondition {
            condition_type: ConditionType::PaymentRequired,
            parameters: {
                "content_id": content_id,
                "price": "0.1",
                "currency": "ASTRA",
            },
        },
    ]),
    encryption: Some(QuantumEncryption {
        algorithm: EncryptionAlgorithm::Kyber1024,
        reader_keys: vec![(subscriber_did, encrypted_key)],
        metadata: EncryptionMetadata { ... },
    }),
}
```

### 2. Smart Contract Governance

#### Storage Governance Contract

```rust
// Smart contract deployed on Compute Node
CONTRACT StorageGovernance {
    // Content storage policies
    content_policies: Map<ContentID, StoragePolicy>,
    
    // P2P distribution rules
    distribution_rules: Map<ChannelID, DistributionRule>,
    
    // Access control registry
    access_registry: Map<ContentID, AccessControl>,
    
    FUNCTION register_content(
        content_id: ContentID,
        fact_package_id: FactID,
        storage_policy: StoragePolicy,
        distribution_rule: DistributionRule
    ) -> Result {
        // Verify fact package exists
        verify_fact_package(fact_package_id);
        
        // Register storage policy
        content_policies[content_id] = storage_policy;
        
        // Register distribution rule
        distribution_rules[channel_id] = distribution_rule;
        
        // Emit event
        emit ContentRegistered(content_id, fact_package_id);
    }
    
    FUNCTION get_storage_policy(content_id: ContentID) -> StoragePolicy {
        return content_policies[content_id];
    }
    
    FUNCTION get_distribution_rule(channel_id: ChannelID) -> DistributionRule {
        return distribution_rules[channel_id];
    }
    
    FUNCTION verify_access(
        content_id: ContentID,
        requester_did: DID,
        payment_proof: Option<PaymentProof>
    ) -> bool {
        let policy = content_policies[content_id];
        let access = access_registry[content_id];
        
        // Check payment if required
        if policy.requires_payment {
            if payment_proof.is_none() {
                return false;
            }
            verify_payment(payment_proof);
        }
        
        // Check access control
        return check_access_control(access, requester_did);
    }
}
```

#### P2P Distribution Governance

```rust
CONTRACT P2PDistributionGovernance {
    // Chunk distribution registry
    chunk_registry: Map<ChunkID, ChunkMetadata>,
    
    // Replication requirements
    replication_policies: Map<ContentID, ReplicationPolicy>,
    
    FUNCTION register_chunk(
        chunk_id: ChunkID,
        content_id: ContentID,
        storage_nodes: Vec<NodeID>,
        replication_factor: u32
    ) -> Result {
        chunk_registry[chunk_id] = ChunkMetadata {
            content_id,
            storage_nodes,
            replication_factor,
            registered_at: now(),
        };
        
        emit ChunkRegistered(chunk_id, content_id);
    }
    
    FUNCTION get_chunk_locations(chunk_id: ChunkID) -> Vec<NodeID> {
        return chunk_registry[chunk_id].storage_nodes;
    }
    
    FUNCTION verify_replication(content_id: ContentID) -> bool {
        let policy = replication_policies[content_id];
        let chunks = get_content_chunks(content_id);
        
        for chunk in chunks {
            let locations = get_chunk_locations(chunk.id);
            if locations.len() < policy.min_replication {
                return false;
            }
        }
        
        return true;
    }
}
```

### 3. Content Publishing Flow (Updated)

```rust
// 1. Create Fact Package from content
let fact_package = FactPackage {
    fact_id: generate_content_id(),
    content: FactContent::Binary {
        data: mp4_bytes,
        mime_type: "video/mp4".to_string(),
        hash: compute_hash(&mp4_bytes),
    },
    metadata: FactMetadata {
        category: FactCategory::UserGenerated,
        tags: vec!["video", "tutorial"],
        domain: KnowledgeDomain::ComputerScience,
        source: DataSource::UserInput {
            application: channel_did.clone(),
            user: publisher_did.clone(),
        },
        // ... other metadata
    },
    access_policy: AccessPolicy::Conditional(vec![
        AccessCondition {
            condition_type: ConditionType::PaymentRequired,
            parameters: {
                "price": "0.1",
                "currency": "ASTRA",
            },
        },
    ]),
    // ... other fields
};

// 2. Store Fact Package in Storage Node
let fact_id = storage_node.fact_storage()
    .store_fact(fact_package)
    .await?;

// 3. Register content with Storage Governance Contract
let storage_policy = StoragePolicy {
    requires_payment: true,
    payment_amount: 0.1,
    access_control: AccessControl::ChannelSubscribers,
    replication_factor: 5,
};

let distribution_rule = DistributionRule {
    p2p_enabled: true,
    chunk_size: 1_000_000, // 1MB chunks
    replication_factor: 5,
    storage_nodes: vec![], // Auto-assigned
};

compute_node.execute_contract(
    &storage_governance_contract_id,
    "register_content",
    vec![
        json!({ "content_id": content_id }),
        json!({ "fact_package_id": fact_id }),
        json!({ "storage_policy": storage_policy }),
        json!({ "distribution_rule": distribution_rule }),
    ],
    &publisher_did,
    1_000_000,
).await?;

// 4. Chunk content and register with P2P Distribution Contract
let chunks = storage_node.chunk_file(&content_id, distribution_rule.chunk_size).await?;

for chunk in chunks {
    // Store chunk on storage node
    storage_node.store_chunk(chunk.clone()).await?;
    
    // Register chunk with P2P governance contract
    compute_node.execute_contract(
        &p2p_distribution_contract_id,
        "register_chunk",
        vec![
            json!({ "chunk_id": chunk.chunk_id }),
            json!({ "content_id": content_id }),
            json!({ "storage_nodes": vec![storage_node_id] }),
            json!({ "replication_factor": distribution_rule.replication_factor }),
        ],
        &publisher_did,
        500_000,
    ).await?;
}

// 5. Publish notification via Messaging Node (Gossipsub)
messaging_node.publish_to_topic(
    &format!("channel:{}", channel_id),
    &json!({
        "type": "content_published",
        "content_id": content_id,
        "fact_package_id": fact_id,
        "title": title,
        "pricing": pricing,
    }),
).await?;
```

### 4. Content Access Flow (Updated)

```rust
// 1. Subscriber receives notification via Gossipsub
// Notification includes: content_id, fact_package_id, pricing

// 2. Verify access via Storage Governance Contract
let has_access = compute_node.execute_contract(
    &storage_governance_contract_id,
    "verify_access",
    vec![
        json!({ "content_id": content_id }),
        json!({ "requester_did": subscriber_did }),
        json!({ "payment_proof": None }),
    ],
    &subscriber_did,
    100_000,
).await?;

// 3. If payment required, process payment
if !has_access && pricing.requires_payment {
    let payment_result = compute_node.execute_contract(
        &payment_contract_id,
        "pay_for_content",
        vec![
            json!({ "content_id": content_id }),
            json!({ "publisher_did": publisher_did }),
            json!({ "amount": pricing.price }),
        ],
        &subscriber_did,
        1_000_000,
    ).await?;
    
    // Grant access after payment
    compute_node.execute_contract(
        &storage_governance_contract_id,
        "grant_access",
        vec![
            json!({ "content_id": content_id }),
            json!({ "requester_did": subscriber_did }),
            json!({ "payment_proof": payment_result["tx_hash"] }),
        ],
        &publisher_did,
        200_000,
    ).await?;
}

// 4. Retrieve Fact Package from Storage Node
let fact_package = storage_node.fact_storage()
    .retrieve_fact(fact_package_id)
    .await?;

// 5. Get chunk locations from P2P Distribution Contract
let chunks = get_content_chunks(content_id);
let mut all_chunks = Vec::new();

for chunk_id in chunks {
    let locations = compute_node.execute_contract(
        &p2p_distribution_contract_id,
        "get_chunk_locations",
        vec![json!({ "chunk_id": chunk_id })],
        &subscriber_did,
        50_000,
    ).await?;
    
    // Retrieve chunk from nearest storage node
    let chunk = retrieve_chunk_from_p2p(chunk_id, locations).await?;
    all_chunks.push(chunk);
}

// 6. Reassemble and decrypt content
let content = reassemble_chunks(all_chunks);
let decrypted = decrypt_fact_content(fact_package, subscriber_private_key).await?;
```

## Benefits

### Fact Package Integration
- ✅ **Rich Metadata**: Categories, tags, domains, verification levels
- ✅ **Access Control**: Multi-policy access control (Public, Private, Role-based, Attribute-based, Conditional)
- ✅ **Sharing Metadata**: Built-in sharing and access policies
- ✅ **Verification**: SPHINCS+ signatures and verification proofs
- ✅ **Content Types**: Supports Binary (MP4, images, etc.)

### Smart Contract Governance
- ✅ **On-Chain Policies**: Storage and distribution policies stored on-chain
- ✅ **Access Verification**: Trustless access verification via smart contracts
- ✅ **P2P Coordination**: Chunk distribution and replication managed on-chain
- ✅ **Payment Integration**: Payment verification integrated with access control
- ✅ **Transparency**: All policies and access records on-chain

## Implementation Status

### ✅ Completed
- Fact Package system in Storage Node
- Fact storage engine with access control
- Smart contract execution in Compute Node
- P2P network in Storage Node

### 🚧 In Progress
- Content publishing using Fact Packages
- Storage Governance Contract
- P2P Distribution Governance Contract
- CLI integration for content publishing

### ⏳ Planned
- Payment contract integration
- Channel key distribution
- Subscription management contracts

