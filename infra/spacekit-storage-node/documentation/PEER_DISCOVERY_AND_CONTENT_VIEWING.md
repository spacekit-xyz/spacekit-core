# Peer Discovery and Content Viewing Guide

## Overview

This guide explains how to discover peers for messaging and view published content in the SpaceKit Network.

## 🔍 Peer Discovery

### List Available Peers

**Command:**
```bash
spacekit message peers
```

**With Details:**
```bash
spacekit message peers --detailed
```

**What It Shows:**
- **Connected Peers**: Peers you're currently connected to via P2P
- **Discovered Peers**: Peers found via mDNS (local network) or DHT (distributed)
- **Peer Information**:
  - Peer ID (libp2p identifier)
  - Network addresses
  - Supported protocols
  - Connection status
  - Last seen timestamp

**How Peer Discovery Works:**

1. **mDNS (Local Network)**
   - Automatically discovers peers on your local network
   - Service: `_spacekit-messaging._tcp.local`
   - No configuration needed

2. **Kademlia DHT (Distributed)**
   - Global peer discovery
   - DID → Multiaddr resolution
   - Works across the internet

3. **Bootstrap Peers**
   - Connect to known peers on startup
   - Helps bootstrap the network

**Example Output:**
```
🔍 Discovering peers...

   Found 3 peer(s):

   1. Peer ID: 12D3KooWAbc123...
      Address: /ip4/192.168.1.100/tcp/7001
      Protocols: [gossipsub, identify, ping]
      Status: Connected
      Last Seen: 2025-01-15 10:30:00

   2. Peer ID: 12D3KooWDef456...
      Address: /ip4/192.168.1.101/tcp/7001
      Status: Discovered (not connected)
      Last Seen: 2025-01-15 10:25:00
```

### Send Messages to Peers

Once you know a peer's DID, you can send messages:

```bash
spacekit message send \
  --to did:spacekit:user:alice \
  --message "Hello!"
```

## 📺 Content Viewing

### List Channels

**Command:**
```bash
spacekit content list-channels
```

**With Details:**
```bash
spacekit content list-channels --detailed
```

**Show Only Subscribed:**
```bash
spacekit content list-channels --subscribed
```

**What It Shows:**
- Channel IDs (from Fact Packages)
- Channel creators (author DIDs)
- Creation timestamps
- Tags and metadata

**How It Works:**
- Queries Fact Packages with `"channel"` tag from Storage Node
- Channels are stored as Fact Packages with metadata
- Uses FactStorageEngine query interface

**Example Output:**
```
📺 Listing channels...

   Found 2 channel(s):

   1. Channel ID: abc123...
      Author: did:spacekit:user:creator
      Created: 2025-01-15 10:00:00
      Tags: channel, tutorial, education

   2. Channel ID: def456...
      Author: did:spacekit:user:another
      Created: 2025-01-14 15:30:00
      Tags: channel, entertainment
```

### List Content in Channel

**Command:**
```bash
spacekit content list-content \
  --channel channel_123 \
  --limit 20
```

**What It Shows:**
- Content IDs (Fact Package IDs)
- Titles (from metadata)
- Authors (publisher DIDs)
- Content types (MP4, images, PDFs, etc.)
- Access policies (Public, Pay-Per-View, Subscription)
- Creation timestamps

**How It Works:**
- Queries Fact Packages with `"content"` and `"published"` tags
- Filters by channel (if channel metadata is available)
- Sorted by creation date (newest first)
- Respects access policies

**Example Output:**
```
📋 Listing content in channel...
   Channel: channel_123
   Limit: 20

   Found 5 content item(s):

   1. Content ID: fact_abc123...
      Title: Rust Tutorial
      Author: did:spacekit:user:teacher
      Type: video/mp4
      Access: Public
      Created: 2025-01-15 11:00:00

   2. Content ID: fact_def456...
      Title: Advanced Quantum Computing
      Author: did:spacekit:user:researcher
      Type: video/mp4
      Access: Pay-Per-View
      Created: 2025-01-15 10:30:00

   💡 View content: spacekit content view --content-id <ID> --output <file>
```

### View/Download Content

**Command:**
```bash
spacekit content view \
  --content-id fact_abc123... \
  --output video.mp4
```

**What It Does:**
- Retrieves Fact Package from Storage Node
- Decrypts content (requires your private key)
- Saves to specified output file
- Verifies access permissions

**Access Control:**
- **Public**: Anyone can view
- **Pay-Per-View**: Requires payment proof
- **Subscription**: Requires active subscription
- **Private**: Only authorized users

**Example:**
```bash
# View free content
spacekit content view \
  --content-id fact_abc123... \
  --output tutorial.mp4

# View pay-per-view content (will prompt for payment)
spacekit content view \
  --content-id fact_def456... \
  --output premium.mp4
```

## 📊 Content Storage Architecture

### How Content is Stored

1. **Fact Package Creation**
   - File converted to `FactPackage`
   - Metadata added (title, description, tags)
   - Access policy set (Public, Pay-Per-View, etc.)
   - SPHINCS+ signature created

2. **Storage**
   - Stored in `FactStorageEngine`
   - Encrypted with quantum-safe encryption
   - Indexed by author, category, tags, domain
   - Compressed for efficiency

3. **Querying**
   - Uses Fact Package query interface
   - Filters by tags, author, category
   - Supports sorting and pagination
   - Respects access policies

### Content Metadata

Content is stored with rich metadata:

```rust
FactPackage {
    fact_id: [u8; 32],           // Unique content ID
    content: Binary {
        data: encrypted_bytes,
        mime_type: "video/mp4",
        hash: content_hash,
    },
    metadata: {
        category: UserGenerated,
        tags: ["content", "published", "video", "tutorial"],
        domain: KnowledgeDomain::Custom("Content Publishing"),
        source: UserInput { application, user },
    },
    access_policy: Conditional([PaymentRequired]),
    author: publisher_did,
    created_at: timestamp,
}
```

## 🔗 Integration with Other Commands

### Complete Workflow

1. **Discover Peers**
   ```bash
   spacekit message peers
   ```

2. **Send Message to Peer**
   ```bash
   spacekit message send --to <peer_did> --message "Hello!"
   ```

3. **List Channels**
   ```bash
   spacekit content list-channels
   ```

4. **List Content in Channel**
   ```bash
   spacekit content list-content --channel <channel_id>
   ```

5. **View Content**
   ```bash
   spacekit content view --content-id <content_id> --output <file>
   ```

## 🚧 Future Enhancements

### Planned Features

1. **Real-time Peer Updates**
   - Subscribe to peer discovery events
   - Automatic peer list updates
   - Connection status monitoring

2. **Content Search**
   - Full-text search across content
   - Search by title, description, tags
   - Semantic search (vector similarity)

3. **Content Recommendations**
   - Based on viewing history
   - Similar content suggestions
   - Trending content

4. **Channel Subscriptions**
   - Subscribe to channels for notifications
   - Auto-download new content
   - Subscription management

## Troubleshooting

### No Peers Found

**Possible Causes:**
- Messaging node not running
- Not connected to simulator
- No peers on local network
- Firewall blocking mDNS

**Solutions:**
```bash
# 1. Connect to simulator
spacekit connect simulator --url http://localhost:8080

# 2. Check messaging node status
# (via simulator API or logs)

# 3. Ensure mDNS is enabled
# (should be automatic)
```

### No Content Found

**Possible Causes:**
- No content published yet
- Content not tagged correctly
- Access denied (private content)
- Storage node not connected

**Solutions:**
```bash
# 1. Publish some content first
spacekit content publish --channel test --file video.mp4 --title "Test"

# 2. Check storage node connection
spacekit connect storage --url http://localhost:9000

# 3. Verify content tags
# Content should have "content" and "published" tags
```

## Summary

- **Peer Discovery**: `spacekit message peers` - Lists connected and discovered peers
- **Channel Listing**: `spacekit content list-channels` - Lists available channels
- **Content Listing**: `spacekit content list-content --channel <id>` - Lists content in channel
- **Content Viewing**: `spacekit content view --content-id <id> --output <file>` - Downloads content

All content is stored as Fact Packages in the Storage Node, making it queryable, searchable, and accessible with proper access control.

