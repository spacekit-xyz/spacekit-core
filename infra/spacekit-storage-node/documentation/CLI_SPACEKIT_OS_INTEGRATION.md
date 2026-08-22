# CLI Integration with SpaceKit OS

## Overview

The CLI can now connect to SpaceKit OS desktop app's messaging and storage nodes to discover peers and view content.

## How It Works

### Peer Discovery

**Command**: `spacekit message peers`

**Implementation:**
1. CLI creates a temporary MessagingNode client
2. Connects to P2P network on port 7100 (to avoid conflicts with SpaceKit OS on 7000)
3. Bootstraps to SpaceKit OS messaging node on `localhost:7000`
4. Uses mDNS to discover peers on local network
5. Reports connected peers and status

**Example:**
```bash
$ spacekit message peers
🔍 Discovering peers...
   Connecting to messaging node on localhost:7000...
   ✅ Messaging node client created
   🔍 Discovering peers via mDNS...

   📊 Messaging Node Status:
      Active Connections: 2
      Registered Users: 5

   ✅ Found 2 connected peer(s)!
```

### Content Viewing

**Commands:**
- `spacekit content list-channels` - Lists all channels
- `spacekit content list-content --channel <id>` - Lists content in a channel
- `spacekit content view --content-id <id> --output <file>` - Downloads content

**Implementation:**
1. CLI connects to Storage Node (default: `localhost:4001`)
2. Queries Fact Packages with specific tags:
   - Channels: `"channel"` tag
   - Content: `"content"` and `"published"` tags
3. Displays metadata (title, author, type, access policy)
4. Respects access policies (Public, Pay-Per-View, Subscription)

## Architecture

```
┌─────────────────────┐
│  SpaceKit OS        │
│  (Desktop App)      │
│                     │
│  ┌───────────────┐ │
│  │ Messaging Node │ │ Port 7000
│  │ (P2P Network)  │ │
│  └───────┬───────┘ │
│          │          │
│  ┌───────▼───────┐ │
│  │ Storage Node  │ │ Port 4001
│  │ (Fact Storage)│ │
│  └───────────────┘ │
└─────────────────────┘
          │
          │ P2P Network (libp2p)
          │ - mDNS Discovery
          │ - Gossipsub
          │ - Kademlia DHT
          │
┌─────────▼─────────┐
│  CLI              │
│                   │
│  ┌──────────────┐ │
│  │ Messaging    │ │ Port 7100
│  │ Node Client  │ │ (Bootstrap: 7000)
│  └──────────────┘ │
│                   │
│  ┌──────────────┐ │
│  │ Storage      │ │ Connects to 4001
│  │ Node Client  │ │
│  └──────────────┘ │
└───────────────────┘
```

## Usage Guide

### 1. Start SpaceKit OS

Ensure SpaceKit OS desktop app is running:
- Messaging node should be on `localhost:7000`
- Storage node should be on `localhost:4001`
- Compute node should be on `localhost:9100`

### 2. Discover Peers

```bash
# List peers
spacekit message peers

# With details
spacekit message peers --detailed
```

**What You'll See:**
- Active connections count
- Registered users count
- Peer discovery status
- Troubleshooting tips if no peers found

### 3. List Channels

```bash
# List all channels
spacekit content list-channels

# With details
spacekit content list-channels --detailed
```

**What You'll See:**
- Channel IDs (Fact Package IDs)
- Channel creators (author DIDs)
- Creation timestamps
- Tags and metadata

### 4. List Content in Channel

```bash
# List content
spacekit content list-content --channel channel_123

# With limit
spacekit content list-content --channel channel_123 --limit 50
```

**What You'll See:**
- Content IDs
- Titles (from metadata)
- Authors
- Content types (MP4, images, PDFs)
- Access policies (Public, Pay-Per-View, Subscription)
- Creation timestamps

### 5. View/Download Content

```bash
# Download content
spacekit content view \
  --content-id fact_abc123... \
  --output video.mp4
```

**What Happens:**
- Retrieves Fact Package from Storage Node
- Decrypts content (requires your private key)
- Saves to specified file
- Verifies access permissions

## Troubleshooting

### "No peers connected yet"

**Causes:**
- SpaceKit OS messaging just started (mDNS takes 5-10 seconds)
- No other peers on local network
- Firewall blocking mDNS (UDP port 5353)

**Solutions:**
1. Wait 5-10 seconds and try again
2. Ensure SpaceKit OS is running with messaging enabled
3. Check firewall settings
4. Verify both nodes are on the same network

### "Could not connect to messaging node"

**Causes:**
- SpaceKit OS not running
- Messaging node not started
- Port conflict

**Solutions:**
1. Start SpaceKit OS desktop app
2. Ensure messaging is enabled in SpaceKit OS
3. Check that port 7000 is not in use
4. Try a different port if needed

### "No content found"

**Causes:**
- No content published yet
- Content not tagged correctly
- Storage node not connected

**Solutions:**
1. Publish content first: `spacekit content publish --channel test --file video.mp4 --title "Test"`
2. Verify content has `"content"` and `"published"` tags
3. Check storage node connection

## Technical Details

### P2P Network Connection

The CLI creates a MessagingNode client that:
- Listens on port 7100 (to avoid conflicts)
- Bootstraps to SpaceKit OS on port 7000
- Uses mDNS for local network discovery
- Connects via libp2p protocols (Gossipsub, Kademlia DHT)

### Fact Package Queries

Content is queried using `FactStorageEngine.query_facts()`:
- Filters by tags: `["channel"]` or `["content", "published"]`
- Sorts by creation date (newest first)
- Paginates results
- Respects access policies

### Access Control

Content access is verified based on:
- **Public**: Anyone can view
- **Pay-Per-View**: Requires payment proof
- **Subscription**: Requires active subscription
- **Private**: Only authorized users

## Future Enhancements

### 1. Direct API Access

Add HTTP API to SpaceKit OS:
- Expose peer information
- Query content metadata
- Faster than P2P discovery

### 2. Shared State File

SpaceKit OS writes peer info to `~/.spacekit/peers.json`:
- CLI reads from this file
- Real-time updates
- Lower resource usage

### 3. IPC Communication

Use inter-process communication:
- Unix domain sockets
- Named pipes
- Direct access without creating new nodes

## Summary

✅ **Peer Discovery**: CLI connects to P2P network and discovers peers via mDNS
✅ **Content Viewing**: CLI queries Fact Packages from Storage Node
✅ **Channel Listing**: Lists channels by querying Fact Packages
✅ **Content Listing**: Lists content with metadata and access policies

The CLI now works seamlessly with SpaceKit OS!

