# SpaceKit Messaging Node

A quantum-resistant P2P messaging library that supports both group and direct messaging with asymmetric encryption. Can be embedded in applications or run as standalone infrastructure nodes.

Cross-cutting architecture and current operational references are indexed in
[`docs/README.md`](../../docs/README.md).

**ASTRA / tokenomics:** Canonical specs in
[`economics/spacekit-tokenomics`](../../economics/spacekit-tokenomics/).
Messaging-specific notes: [`TOKENOMICS.md`](./TOKENOMICS.md).

## Features

### 🔐 Quantum-Resistant Security
- **19 Post-Quantum Algorithms**: Kyber, NTRU, FrodoKEM, ClassicMcEliece, BIKE variants
- **Multiple Cipher Suites**: AES256, ChaCha20, XChaCha20
- **SPHINCS+ Signatures**: Quantum-resistant digital signatures for identity verification
- **Asymmetric Encryption**: Each message encrypted individually for recipients

### 💬 Messaging Capabilities
- **Group Messaging**: Create groups, manage members, send messages to multiple recipients
- **Direct Messaging**: 1-on-1 conversations with automatic conversation management
- **File Sharing**: Upload and share encrypted files in both groups and direct conversations
- **Message Types**: Text, files, images, system messages
- **Real-time Events**: Subscribe to message events for real-time applications

### 🏗️ Architecture
- **Embeddable Library**: Use as a dependency in your applications
- **Standalone Node**: Run as an independent messaging infrastructure node
- **Decentralized Identity**: DID-based user management
- **P2P Networking**: Direct peer-to-peer communication
- **Event-Driven**: Reactive architecture with message event broadcasting

### Compression status

The public legacy `MessageCompressor` API delegates gzip/LZMA codecs to the
shared `spacekit-compressor` crate while preserving its existing thresholds,
result types, and method-out-of-band contract. Compression is not yet enabled
on live P2P messages, encrypted history, or persisted payloads; those formats
remain unchanged. A future wire rollout must be capability-negotiated,
default-off, and preserve passthrough decoding for legacy uncompressed JSON.

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
spacekit-messaging-node = "0.1.0"
```

### Basic Usage

```rust
use spacekit-messaging-node::{MessagingNode, MessagingConfig};
use spacekit-primitives::v1::crypto::quantum::Algorithm;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create messaging node
    let config = MessagingConfig::default();
    let node = MessagingNode::new(config).await?;
    
    // Start the node
    node.start().await?;
    
    // Register users
    let alice = node.register_user(
        "did:example:alice".to_string(),
        "Alice".to_string(),
        vec![1, 2, 3, 4], // Public key
        Algorithm::Kyber768,
    ).await?;
    
    let bob = node.register_user(
        "did:example:bob".to_string(),
        "Bob".to_string(),
        vec![5, 6, 7, 8], // Public key
        Algorithm::Kyber768,
    ).await?;
    
    // Send direct message
    let events = node.send_direct_message(
        alice.id.clone(),
        bob.did.clone(),
        "Hello Bob! 👋".to_string(),
    ).await?;
    
    println!("Message sent! Events: {:#?}", events);
    Ok(())
}
```

## Group Messaging

### Creating and Managing Groups

```rust
// Create a group
let group = node.create_group(
    "My Group".to_string(),
    alice.id.clone(),
    Some("A quantum-secure group chat".to_string()),
).await?;

// Send group message
let events = node.send_text_message(
    group.id.clone(),
    alice.id.clone(),
    "Hello everyone!".to_string(),
).await?;
```

### Group Features
- **Role-based permissions**: Creator, Admin, Member roles
- **Invitation system**: Invite users by DID
- **Member management**: Add/remove users with proper permissions
- **File sharing**: Upload and share encrypted files within groups
- **Message history**: Persistent message storage with encryption

## Direct Messaging

### Starting Conversations

```rust
// Send direct message (creates conversation automatically)
let events = node.send_direct_message(
    sender_id,
    recipient_did,
    "Private message content".to_string(),
).await?;

// Or create conversation explicitly
let conversation = node.create_or_get_direct_conversation(
    alice.id.clone(),
    bob.id.clone(),
).await?;
```

### Managing Conversations

```rust
// Get user's direct conversations
let conversations = node.get_user_direct_conversations(&alice.id).await?;

// Get conversation info
let info = node.get_conversation_info(
    &conversation.id,
    &ConversationType::Direct { 
        conversation_id: conversation.id.clone() 
    },
).await?;

// Get encrypted messages for user
let messages = node.get_user_encrypted_messages(&alice.id).await?;
```

## Event Handling

Subscribe to real-time message events:

```rust
let mut event_receiver = node.subscribe_events();

tokio::spawn(async move {
    while let Ok(event) = event_receiver.recv().await {
        match event {
            MessageEvent::MessageReceived { message, sender, conversation_type, .. } => {
                match conversation_type {
                    ConversationType::Direct { conversation_id } => {
                        println!("📱 Direct message from {}: {:#?}", sender.username, message);
                    }
                    ConversationType::Group { group_id } => {
                        println!("👥 Group message from {}: {:#?}", sender.username, message);
                    }
                }
            }
            MessageEvent::DirectMessageDelivered { message_id, recipient_id, .. } => {
                println!("✅ Message {} delivered to {}", message_id, recipient_id);
            }
            MessageEvent::DirectConversationCreated { conversation, initiator, recipient } => {
                println!("💬 New conversation between {} and {}", 
                         initiator.username, recipient.username);
            }
            _ => {}
        }
    }
});
```

## Security Features

### Quantum-Resistant Algorithms

The node supports multiple post-quantum algorithms:

```rust
// Different algorithms for different users
let alice = node.register_user(
    "did:example:alice".to_string(),
    "Alice".to_string(),
    alice_public_key,
    Algorithm::Kyber768,  // NIST standardized
).await?;

let bob = node.register_user(
    "did:example:bob".to_string(),
    "Bob".to_string(),
    bob_public_key,
    Algorithm::NtruPrimeSntrup761,  // Lattice-based
).await?;
```

### Message Encryption

- **Asymmetric Encryption**: Each message is encrypted individually for each recipient
- **Key Encapsulation**: Quantum-resistant key exchange using KEM algorithms
- **Forward Secrecy**: Automatic key rotation capabilities
- **Multi-Algorithm Support**: Different users can use different quantum-resistant algorithms

### Identity Security

- **DID-based Identity**: Decentralized identifiers for user management
- **SPHINCS+ Signatures**: Quantum-resistant digital signatures
- **Public Key Verification**: Cryptographic verification of user identities
- **Permission System**: Role-based access control for groups

## Node Status and Monitoring

```rust
// Check node status
let status = node.get_status().await;
println!("Node Status: {:#?}", status);

// Status includes:
// - active_connections: u32
// - active_groups: u32
// - active_direct_conversations: u32
// - registered_users: u32
// - messages_sent_today: u64
// - direct_messages_sent_today: u64
// - last_activity: DateTime<Utc>
```

## Configuration

### Default Configuration

```rust
use swtch_messaging_node::MessagingConfig;

let config = MessagingConfig {
    node_did: "did:swtch:messaging-node-001".to_string(),
    listen_addr: "127.0.0.1:8080".parse().unwrap(),
    max_connections: 1000,
    enable_metrics: true,
    storage_path: "./data".to_string(),
};
```

### Network Configuration

```rust
// Customize network settings
let config = MessagingConfig {
    listen_addr: "0.0.0.0:6090".parse().unwrap(),  // Changed from 9090 to avoid conflicts
    max_connections: 5000,
    bootstrap_peers: vec![
        "/ip4/127.0.0.1/tcp/8080".parse().unwrap(),
    ],
    ..Default::default()
};
```

## Integration Examples

### Embedding in Tauri Application

```rust
// In your Tauri application
#[tauri::command]
async fn send_message(
    state: tauri::State<'_, Arc<MessagingNode>>,
    recipient_did: String,
    content: String,
) -> Result<String, String> {
    let sender_id = get_current_user_id().await;
    
    let events = state.send_direct_message(
        sender_id,
        recipient_did,
        content,
    ).await.map_err(|e| e.to_string())?;
    
    Ok(format!("Message sent with {} events", events.len()))
}
```

### REST API Wrapper

```rust
use axum::{extract::State, Json, routing::post, Router};

async fn send_direct_message_api(
    State(node): State<Arc<MessagingNode>>,
    Json(request): Json<DirectMessageRequest>,
) -> Result<Json<Vec<MessageEvent>>, StatusCode> {
    let events = node.send_direct_message(
        request.sender_id,
        request.recipient_did,
        request.content,
    ).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(events))
}

let app = Router::new()
    .route("/api/messages/direct", post(send_direct_message_api))
    .with_state(Arc::new(messaging_node));
```

## Performance Considerations

### Message Throughput
- **Asymmetric Encryption**: Each recipient requires individual encryption
- **Batch Operations**: Group messages are processed efficiently
- **Memory Usage**: Encrypted messages are stored in memory by default
- **Persistence**: Consider implementing custom storage backends for large deployments

### Scalability
- **Connection Limits**: Configure `max_connections` based on hardware
- **Storage**: Implement custom storage backends for production use
- **Network**: P2P networking scales well with proper bootstrap peers
- **Encryption**: Quantum-resistant algorithms have different performance characteristics

## Comparison with Old Implementation

### Improvements from WebSocket-based Version

The new implementation provides significant improvements over the old WebSocket-based messaging system:

#### **Enhanced Security**
- **Multiple Quantum Algorithms**: 19 different post-quantum algorithms vs basic public key exchange
- **Individual Encryption**: Each message encrypted per recipient vs shared group encryption
- **DID Integration**: Decentralized identity vs simple UUID-based identification
- **Algorithm Flexibility**: Users can choose different quantum-resistant algorithms

#### **Better Architecture**
- **Library-First Design**: Embeddable library vs standalone WebSocket server
- **Type Safety**: Comprehensive Rust type system vs JSON message parsing
- **Event-Driven**: Reactive event system vs simple message broadcasting
- **Modular Design**: Separate concerns vs monolithic actor system

#### **Direct Messaging**
- **Persistent Conversations**: Automatic conversation management vs transient whispers
- **Message History**: Encrypted message storage vs ephemeral messages
- **Delivery Confirmation**: Message delivery events vs fire-and-forget
- **Rich Message Types**: Support for files, images, system messages vs text-only

#### **Backwards Compatibility**
```rust
// Legacy group messaging (similar to old rooms)
let events = node.send_text_message(group_id, sender_id, content).await?;

// New direct messaging (replaces whisper functionality)
let events = node.send_direct_message(sender_id, recipient_did, content).await?;
```

## Contributing

### Running Tests

```bash
cargo test
```

### Development Setup

```bash
# Clone and build
git clone https://github.com/spacekit-xyz/spacekit-messaging-node
cd spacekit-messaging-node
cargo build

# Run with logging
RUST_LOG=debug cargo test
```

### Integration with Other SpaceKit Components

This messaging node integrates with:
- **spacekit-primitives**: Quantum-resistant cryptography and DID management
- **spacekit-storage-node**: Encrypted file storage and sharing
- **spacekit-compute-node**: Distributed computation with messaging
