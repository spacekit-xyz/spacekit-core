# Content Publishing & Subscription System

## Overview

This document explains how the **SpaceKit ecosystem** (Storage Node + Messaging Node + Compute Node) can support a channel-based content publishing and subscription model, similar to YouTube, Patreon, or OnlyFans, but with quantum-safe encryption, P2P distribution, and blockchain-based payments.

## Use Case: Encrypted MP4 Channel with Subscriptions

### Scenario
- **Publisher**: Creates a channel and publishes encrypted MP4 videos
- **Subscribers**: Subscribe to channels and receive new content
- **Access Control**: Free content vs. pay-per-view content
- **Distribution**: Content distributed via P2P network
- **Payments**: Blockchain-based payment processing via Compute Node
- **Notifications**: Real-time notifications via Messaging Node

---

## 🏗️ Integrated Architecture

### Component Roles

```
┌─────────────────────────────────────────────────────────────┐
│                    Content Publishing Flow                   │
└─────────────────────────────────────────────────────────────┘

Publisher                    Subscribers
    │                            │
    ├─► Storage Node             │
    │   - Encrypt MP4            │
    │   - Store encrypted file   │
    │   - Create content metadata│
    │   - P2P chunk distribution │
    │                            │
    ├─► Compute Node             │
    │   - Deploy channel contract│
    │   - Process subscriptions  │
    │   - Handle payments        │
    │   - Verify access rights   │
    │   - On-chain state mgmt   │
    │                            │
    ├─► Messaging Node           │
    │   - Publish to Gossipsub    │
    │   - Topic: channel_id       │
    │   - Real-time notifications│
    │   - Automatic delivery     │
    │                            │
    └─► P2P Network              │
        - Distribute chunks      │
        - Announce availability  │
        └───────────────────────►│
                                  │
                                  ├─► Messaging Node
                                  │   - Auto-receive notification
                                  │   - Gossipsub delivery
                                  │
                                  ├─► Compute Node
                                  │   - Verify payment (if needed)
                                  │   - Check access on-chain
                                  │   - Grant access
                                  │
                                  └─► Storage Node
                                      - Retrieve encrypted content
                                      - Decrypt with channel key
```

### Key Components

1. **Storage Node**: Encrypted content storage and P2P distribution
2. **Compute Node**: Smart contracts for subscriptions and payments (on-chain)
3. **Messaging Node**: Real-time notifications via Gossipsub topics (pub/sub)
4. **P2P Network**: Distributed content delivery

### 🎯 Key Advantages of Integrated Architecture

**Real-Time Sync via Messaging Node:**
- ✅ **Gossipsub Pub/Sub**: Instant notifications to all subscribers
- ✅ **No Polling**: Subscribers automatically receive updates
- ✅ **Scalable**: Handles thousands of subscribers efficiently
- ✅ **Decentralized**: No central notification server

**Trustless Payments via Compute Node:**
- ✅ **Smart Contracts**: On-chain payment processing
- ✅ **Access Control**: Verified on blockchain
- ✅ **Transparency**: All transactions recorded
- ✅ **Automated**: No manual payment verification

**Secure Storage via Storage Node:**
- ✅ **Quantum-Safe**: Kyber1024 encryption
- ✅ **P2P Distribution**: Chunks distributed across network
- ✅ **Zero-Knowledge**: Storage node can't decrypt content

---

## Architecture Components

### 1. Channel Management

A channel is essentially a group with metadata:

```rust
pub struct Channel {
    pub channel_id: String,
    pub owner_did: String,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub subscriber_count: u64,
    pub content_count: u64,
    pub pricing_model: PricingModel, // Free, Subscription, PayPerView
}
```

### 2. Subscription System

Subscriptions link users to channels:

```rust
pub struct Subscription {
    pub subscription_id: String,
    pub channel_id: String,
    pub subscriber_did: String,
    pub subscribed_at: DateTime<Utc>,
    pub status: SubscriptionStatus, // Active, Cancelled, Expired
    pub payment_tier: Option<String>, // For tiered subscriptions
}
```

### 3. Content Publishing

When a publisher posts content:

```rust
// 1. Publisher encrypts MP4 with their key
let (file_id, _) = storage_node.store_file(
    "video.mp4",
    &mp4_data,
    &publisher_did,
    &publisher_public_key,
    Some("video/mp4".to_string()),
).await?;

// 2. Create content metadata
let content = ContentMetadata {
    content_id: file_id.clone(),
    channel_id: channel_id.clone(),
    title: "My Video".to_string(),
    description: "Video description".to_string(),
    content_type: "video/mp4".to_string(),
    pricing: ContentPricing::Free, // or PayPerView(amount)
    published_at: Utc::now(),
};

// 3. Store content metadata
storage_node.create_content(&content).await?;

// 4. Distribute via P2P network
if let Some(p2p_network) = storage_node.p2p_network() {
    // Announce content to network
    p2p_network.announce_file(&file_id, vec![chunk_id]).await?;
}
```

### 4. Subscription Notification (Using Messaging Node)

When content is published, notify subscribers via **Gossipsub topics**:

```rust
// 1. Publish to Gossipsub topic (channel_id as topic)
let topic = format!("channel:{}", channel_id);
let notification = serde_json::json!({
    "type": "content_published",
    "channel_id": channel_id,
    "content_id": content_id,
    "title": content.title,
    "pricing": content.pricing,
    "published_at": content.published_at,
});

messaging_node.publish_to_topic(&topic, &notification).await?;

// 2. All subscribers automatically receive notification (they're subscribed to the topic)
// 3. Subscribers check access via Compute Node smart contract
```

**Key Advantage**: Gossipsub provides **real-time pub/sub** - all subscribers automatically receive notifications without polling!

---

## Implementation Using Integrated Ecosystem

### Step 1: Create Channel (Compute Node Smart Contract)

```rust
// Deploy channel contract on Compute Node
let channel_contract_id = compute_node.deploy_contract(
    "channel_registry",
    channel_contract_wasm,
    &publisher_did,
).await?;

// Initialize channel
compute_node.execute_contract(
    &channel_contract_id,
    "create_channel",
    vec![
        json!({ "name": "My Channel" }),
        json!({ "description": "Educational videos" }),
        json!({ "pricing_model": "mixed" }),
    ],
    &publisher_did,
    1_000_000, // gas limit
).await?;
```

### Step 2: Store Encrypted Content (Storage Node)

```rust
// Publisher encrypts MP4 with quantum-safe encryption
let (file_id, public_key) = storage_node.store_file(
    "my_video.mp4",
    &mp4_bytes,
    &publisher_did,
    &publisher_public_key,
    Some("video/mp4".to_string()),
).await?;
```

### Step 3: Subscribe to Channel (Compute Node + Messaging Node)

```rust
// 1. Subscribe on-chain (Compute Node)
compute_node.execute_contract(
    &channel_contract_id,
    "subscribe_to_group",
    vec![json!({ "group_id": channel_id })],
    &subscriber_did,
    500_000,
).await?;

// 2. Subscribe to Gossipsub topic (Messaging Node)
let topic = format!("channel:{}", channel_id);
messaging_node.subscribe_to_topic(&topic).await?;

// 3. Receive channel key (encrypted with subscriber's public key)
// This happens via direct message or secure channel
let channel_key = receive_channel_key(&subscriber_did).await?;
```

### Step 4: Publish Content to Channel

```rust
// 1. Store content encrypted with channel's symmetric key (Storage Node)
let content_file_id = storage_node.share_file_with_group(
    &file_id,
    &publisher_did,
    &publisher_private_key,
    &channel_id,
    &channel_key, // Symmetric key
).await?;

// 2. Register content on-chain (Compute Node)
compute_node.execute_contract(
    &channel_contract_id,
    "publish_content",
    vec![
        json!({ "content_id": content_file_id }),
        json!({ "title": "New Video" }),
        json!({ "pricing": "pay_per_view" }),
        json!({ "price": 0.1 }), // 0.1 ASTRA
    ],
    &publisher_did,
    1_000_000,
).await?;

// 3. Publish notification via Gossipsub (Messaging Node)
let topic = format!("channel:{}", channel_id);
let notification = serde_json::json!({
    "type": "content_published",
    "content_id": content_file_id,
    "title": "New Video",
    "pricing": "pay_per_view",
    "price": 0.1,
});
messaging_node.publish_to_topic(&topic, &notification).await?;

// 4. Distribute chunks via P2P (Storage Node)
if let Some(p2p_network) = storage_node.p2p_network() {
    p2p_network.announce_file(&content_file_id, vec![chunk_id]).await?;
}
```

### Step 5: Subscriber Receives Notification & Accesses Content

```rust
// 1. Subscriber receives notification via Gossipsub (automatic)
// Messaging node automatically delivers to all topic subscribers

// 2. Check access via Compute Node smart contract
let has_access = compute_node.execute_contract(
    &channel_contract_id,
    "check_content_access",
    vec![
        json!({ "content_id": content_file_id }),
        json!({ "subscriber_did": subscriber_did }),
    ],
    &subscriber_did,
    100_000,
).await?;

// 3. If pay-per-view, process payment
if !has_access && content.pricing == "pay_per_view" {
    // Process payment via Compute Node
    let payment_tx = compute_node.execute_contract(
        &payment_contract_id,
        "pay_for_content",
        vec![
            json!({ "content_id": content_file_id }),
            json!({ "amount": 0.1 }),
        ],
        &subscriber_did,
        500_000,
    ).await?;
    
    // Wait for confirmation
    wait_for_transaction_confirmation(&payment_tx).await?;
    
    // Grant access on-chain
    compute_node.execute_contract(
        &channel_contract_id,
        "grant_content_access",
        vec![
            json!({ "content_id": content_file_id }),
            json!({ "subscriber_did": subscriber_did }),
        ],
        &publisher_did,
        200_000,
    ).await?;
}

// 4. Retrieve content (Storage Node)
let video_data = storage_node.retrieve_file(
    &content_file_id,
    &subscriber_did,
    &channel_key, // Symmetric key for decryption
).await?;
```

---

## Payment Integration (Compute Node)

### Pay-Per-View Flow

```rust
// 1. Subscriber receives notification via Gossipsub (Messaging Node)
// Notification includes: content_id, price, publisher_did

// 2. Check access via Compute Node smart contract
let access_status = compute_node.execute_contract(
    &channel_contract_id,
    "check_content_access",
    vec![
        json!({ "content_id": content_id }),
        json!({ "subscriber_did": subscriber_did }),
    ],
    &subscriber_did,
    100_000,
).await?;

// 3. If payment required, process via Compute Node
if access_status["has_access"] == false {
    // Execute payment contract
    let payment_result = compute_node.execute_contract(
        &payment_contract_id,
        "pay_for_content",
        vec![
            json!({ "content_id": content_id }),
            json!({ "publisher_did": publisher_did }),
            json!({ "amount": 0.1 }), // ASTRA amount
        ],
        &subscriber_did,
        1_000_000,
    ).await?;
    
    // Payment contract automatically:
    // - Transfers ASTRA from subscriber to publisher
    // - Records payment on-chain
    // - Emits payment event
    
    // 4. Grant access (called by payment contract or publisher)
    compute_node.execute_contract(
        &channel_contract_id,
        "grant_content_access",
        vec![
            json!({ "content_id": content_id }),
            json!({ "subscriber_did": subscriber_did }),
            json!({ "payment_tx": payment_result["tx_hash"] }),
        ],
        &publisher_did,
        200_000,
    ).await?;
}

// 5. Retrieve content (Storage Node)
let video_data = storage_node.retrieve_file(
    &content_id,
    &subscriber_did,
    &channel_key,
).await?;
```

### Benefits of Compute Node Integration

- **On-Chain Verification**: Payment and access verified on blockchain
- **Smart Contracts**: Automated payment processing
- **Transparency**: All transactions recorded on-chain
- **Trustless**: No need to trust storage node for payments

---

## P2P Distribution

### Content Chunking and Distribution

```rust
// When content is published, it's automatically chunked
let chunks = storage_node.chunk_file(&file_id, chunk_size).await?;

// Each chunk is distributed to multiple peers
for chunk in chunks {
    // Store chunk on local node
    p2p_network.store_chunk(chunk.clone()).await?;
    
    // Announce chunk availability
    p2p_network.announce_file(&file_id, vec![chunk.chunk_id]).await?;
}

// Subscribers can retrieve chunks from any peer
let chunk = p2p_network.retrieve_chunk(&chunk_id).await?;
```

### Benefits
- **Redundancy**: Content stored on multiple nodes
- **Performance**: Chunks retrieved from nearest peers
- **Scalability**: No single point of failure
- **Cost**: Distributed storage costs

---

## Database Schema Extensions

### Channels Table

```rust
pub struct ChannelRecord {
    pub channel_id: String,
    pub owner_did: String,
    pub name: String,
    pub description: String,
    pub symmetric_key_hash: String, // Hash of symmetric key (not stored)
    pub created_at: DateTime<Utc>,
    pub subscriber_count: u64,
}
```

### Subscriptions Table

```rust
pub struct SubscriptionRecord {
    pub subscription_id: String,
    pub channel_id: String,
    pub subscriber_did: String,
    pub subscribed_at: DateTime<Utc>,
    pub status: String, // "active", "cancelled", "expired"
    pub payment_tier: Option<String>,
}
```

### Content Metadata Table

```rust
pub struct ContentRecord {
    pub content_id: String,
    pub channel_id: String,
    pub file_id: String, // Links to FileMetadata
    pub title: String,
    pub description: String,
    pub pricing_model: String, // "free", "pay_per_view", "subscription"
    pub price_amount: Option<f64>,
    pub published_at: DateTime<Utc>,
}
```

---

## API Endpoints (To Be Added)

### Channel Management

```rust
POST /channels
  - Create a new channel
  - Body: { name, description, pricing_model }

GET /channels/:channel_id
  - Get channel information

GET /channels/:channel_id/content
  - List all content in channel
```

### Subscription Management

```rust
POST /channels/:channel_id/subscribe
  - Subscribe to a channel
  - Body: { subscriber_did, payment_tier? }

DELETE /channels/:channel_id/subscribe
  - Unsubscribe from channel

GET /channels/:channel_id/subscribers
  - List subscribers (owner only)
```

### Content Publishing

```rust
POST /channels/:channel_id/content
  - Publish new content
  - Body: { file_data, title, description, pricing }

GET /content/:content_id
  - Get content metadata

GET /content/:content_id/download
  - Download content (requires access)
```

---

## Security Considerations

### 1. Access Control

- **Free Content**: All subscribers get access via group sharing
- **Pay-Per-View**: Access granted only after payment verification
- **Subscription Tiers**: Access based on subscription level

### 2. Encryption

- **Content Encryption**: All content encrypted with quantum-safe keys
- **Channel Keys**: Symmetric keys for group sharing (distributed securely)
- **Payment Verification**: Blockchain-based payment verification

### 3. Privacy

- **Zero-Knowledge**: Storage node never sees decrypted content
- **DID-Based**: All access control based on DIDs, not identities
- **P2P Distribution**: Content distributed without central authority

---

## Integration with Messaging Node (Gossipsub Pub/Sub)

### Real-Time Notification System

**Key Advantage**: Gossipsub provides **automatic pub/sub** - no need to track individual subscribers!

```rust
// When content is published, publish to Gossipsub topic
async fn notify_subscribers(channel_id: &str, content: &ContentMetadata) -> Result<()> {
    // 1. Create topic from channel_id
    let topic = format!("channel:{}", channel_id);
    
    // 2. Publish notification to topic
    let notification = serde_json::json!({
        "type": "content_published",
        "channel_id": channel_id,
        "content_id": content.content_id,
        "title": content.title,
        "pricing": content.pricing,
        "price": content.price,
        "published_at": content.published_at,
    });
    
    // 3. All subscribers automatically receive (they're subscribed to the topic)
    messaging_node.publish_to_topic(&topic, &notification).await?;
    
    Ok(())
}
```

### Subscriber Receives Notification

```rust
// Subscriber automatically receives notification via Gossipsub
// No polling needed - real-time delivery!

messaging_node.on_topic_message(&topic, |message| {
    if message["type"] == "content_published" {
        let content_id = message["content_id"].as_str().unwrap();
        let pricing = message["pricing"].as_str().unwrap();
        
        // Handle notification
        if pricing == "free" {
            // Automatically grant access
            grant_access(&content_id).await?;
        } else if pricing == "pay_per_view" {
            // Show payment prompt
            show_payment_prompt(&content_id, message["price"].as_f64().unwrap()).await?;
        }
    }
});
```

### Benefits of Gossipsub

- **Real-Time**: Instant delivery to all subscribers
- **Efficient**: Single publish reaches all subscribers
- **Scalable**: Handles thousands of subscribers
- **Decentralized**: No central notification server
- **Automatic**: Subscribers don't need to poll

---

## Example: Complete Publishing Flow (Integrated)

```rust
// ============================================
// SETUP PHASE
// ============================================

// 1. Publisher creates channel (Compute Node)
let channel_contract_id = compute_node.deploy_contract(
    "channel_registry",
    channel_contract_wasm,
    &publisher_did,
).await?;

compute_node.execute_contract(
    &channel_contract_id,
    "create_channel",
    vec![
        json!({ "name": "My Video Channel" }),
        json!({ "description": "Educational videos" }),
        json!({ "pricing_model": "mixed" }),
    ],
    &publisher_did,
    1_000_000,
).await?;

// 2. Publisher creates Gossipsub topic (Messaging Node)
let topic = format!("channel:{}", channel_contract_id);
messaging_node.create_topic(&topic).await?;

// ============================================
// SUBSCRIPTION PHASE
// ============================================

// 3. Subscriber subscribes on-chain (Compute Node)
compute_node.execute_contract(
    &channel_contract_id,
    "subscribe_to_group",
    vec![json!({ "group_id": channel_contract_id })],
    &subscriber_did,
    500_000,
).await?;

// 4. Subscriber subscribes to Gossipsub topic (Messaging Node)
messaging_node.subscribe_to_topic(&topic).await?;

// 5. Subscriber receives channel key (encrypted direct message)
let encrypted_key = messaging_node.receive_direct_message(&subscriber_did).await?;
let channel_key = decrypt_with_private_key(&encrypted_key, &subscriber_private_key).await?;

// ============================================
// PUBLISHING PHASE
// ============================================

// 6. Publisher uploads and encrypts MP4 (Storage Node)
let (file_id, _) = storage_node.store_file(
    "tutorial.mp4",
    &mp4_data,
    &publisher_did,
    &publisher_public_key,
    Some("video/mp4".to_string()),
).await?;

// 7. Publisher shares content with channel group (Storage Node)
let content_file_id = storage_node.share_file_with_group(
    &file_id,
    &publisher_did,
    &publisher_private_key,
    &channel_contract_id,
    &channel_key,
).await?;

// 8. Publisher registers content on-chain (Compute Node)
compute_node.execute_contract(
    &channel_contract_id,
    "publish_content",
    vec![
        json!({ "content_id": content_file_id }),
        json!({ "title": "Rust Tutorial Part 1" }),
        json!({ "pricing": "pay_per_view" }),
        json!({ "price": 0.1 }),
    ],
    &publisher_did,
    1_000_000,
).await?;

// 9. Publisher notifies subscribers via Gossipsub (Messaging Node)
let notification = serde_json::json!({
    "type": "content_published",
    "content_id": content_file_id,
    "title": "Rust Tutorial Part 1",
    "pricing": "pay_per_view",
    "price": 0.1,
});
messaging_node.publish_to_topic(&topic, &notification).await?;

// 10. Distribute chunks via P2P (Storage Node)
if let Some(p2p_network) = storage_node.p2p_network() {
    p2p_network.announce_file(&content_file_id, vec![chunk_id]).await?;
}

// ============================================
// SUBSCRIBER ACCESS PHASE
// ============================================

// 11. Subscriber receives notification automatically (Messaging Node)
// Gossipsub delivers notification to all topic subscribers

// 12. Subscriber checks access (Compute Node)
let has_access = compute_node.execute_contract(
    &channel_contract_id,
    "check_content_access",
    vec![
        json!({ "content_id": content_file_id }),
        json!({ "subscriber_did": subscriber_did }),
    ],
    &subscriber_did,
    100_000,
).await?;

// 13. If pay-per-view, process payment (Compute Node)
if !has_access["has_access"].as_bool().unwrap() {
    let payment_result = compute_node.execute_contract(
        &payment_contract_id,
        "pay_for_content",
        vec![
            json!({ "content_id": content_file_id }),
            json!({ "publisher_did": publisher_did }),
            json!({ "amount": 0.1 }),
        ],
        &subscriber_did,
        1_000_000,
    ).await?;
    
    // Grant access after payment
    compute_node.execute_contract(
        &channel_contract_id,
        "grant_content_access",
        vec![
            json!({ "content_id": content_file_id }),
            json!({ "subscriber_did": subscriber_did }),
        ],
        &publisher_did,
        200_000,
    ).await?;
}

// 14. Subscriber retrieves content (Storage Node)
let video_data = storage_node.retrieve_file(
    &content_file_id,
    &subscriber_did,
    &channel_key,
).await?;
```

---

## Current Status

### ✅ Already Implemented

**Storage Node:**
- File storage with quantum-safe encryption
- File sharing (individual and group)
- Access control system
- P2P network for distribution
- Database for metadata storage

**Messaging Node:**
- Gossipsub pub/sub messaging
- Topic-based subscriptions
- Real-time notifications
- P2P networking with libp2p
- Direct messaging

**Compute Node:**
- Smart contract execution (SpaceKitVM)
- Subscription registry contract
- Payment processing capabilities
- Blockchain integration
- On-chain state management

### 🚧 Needs Implementation

- Channel management API (wrapper around smart contracts)
- Content metadata API (links storage to channels)
- Payment contract for pay-per-view
- Channel key distribution system
- Integration layer between all three nodes
- Subscription feed/API

---

## Next Steps

1. **Channel Smart Contract**: Extend subscription_registry contract for channels
2. **Payment Smart Contract**: Create pay-per-view payment contract
3. **Channel Key Distribution**: Secure method to distribute channel keys to subscribers
4. **Integration Layer**: API/service that coordinates Storage + Messaging + Compute nodes
5. **Content Metadata API**: Link storage files to channel contracts
6. **Notification Handler**: Client-side handler for Gossipsub notifications
7. **Access Verification**: On-chain access checking before content retrieval

## Key Advantages of Integrated Architecture

### 1. **Real-Time Notifications** (Messaging Node)
- Gossipsub provides instant pub/sub delivery
- No polling required
- Scales to thousands of subscribers

### 2. **Trustless Payments** (Compute Node)
- On-chain payment verification
- Smart contracts handle access control
- Transparent transaction history

### 3. **Decentralized Storage** (Storage Node)
- P2P content distribution
- No single point of failure
- Quantum-safe encryption

### 4. **Complete Ecosystem**
- Storage: Encrypted content storage
- Messaging: Real-time notifications
- Compute: Payments and access control
- P2P: Distributed delivery

---

## Benefits of This Architecture

1. **Decentralized**: No central authority controls content
2. **Private**: All content encrypted, storage node can't decrypt
3. **Scalable**: P2P distribution handles large files efficiently
4. **Quantum-Safe**: Future-proof encryption
5. **Flexible**: Supports free, subscription, and pay-per-view models
6. **Censorship-Resistant**: P2P network prevents single point of failure

