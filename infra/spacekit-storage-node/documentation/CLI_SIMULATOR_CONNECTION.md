# CLI Connection to SpaceKit OS Simulator

## Problem

The CLI cannot directly access the messaging node running in SpaceKit OS desktop app because:
- SpaceKit OS is a desktop app (Tauri), not a server
- No HTTP API is exposed
- CLI can't call Tauri commands directly

## Solution Implemented

### Peer Discovery

The CLI now creates its own messaging node client that:
1. **Connects to the same P2P network** as SpaceKit OS
2. **Uses mDNS** to discover peers on the local network
3. **Bootstraps to SpaceKit OS** messaging node on `localhost:7000`
4. **Discovers peers automatically** via libp2p protocols

**Command:**
```bash
spacekit message peers
```

**How It Works:**
1. CLI creates a temporary MessagingNode instance
2. Connects to P2P network on port 7100 (to avoid conflicts)
3. Bootstraps to SpaceKit OS messaging node on port 7000
4. Uses mDNS to discover other peers on local network
5. Reports connected peers and status

### Content Viewing

Content viewing works by:
1. **Querying Fact Packages** from Storage Node
2. **Filtering by tags** (`"channel"`, `"content"`, `"published"`)
3. **Displaying metadata** (title, author, type, access policy)

**Commands:**
```bash
# List channels
spacekit content list-channels

# List content in channel
spacekit content list-content --channel <channel_id>

# View/download content
spacekit content view --content-id <id> --output <file>
```

## Architecture

```
SpaceKit OS Desktop App
    │
    ├─► Simulator (Tauri)
    │   ├─► Messaging Node (port 7000)
    │   ├─► Storage Node (port 4001)
    │   └─► Compute Node (port 9100)
    │
    └─► P2P Network (libp2p)
        ├─► mDNS Discovery
        ├─► Gossipsub (pub/sub)
        └─► Kademlia DHT

CLI
    │
    ├─► Messaging Node Client (port 7100)
    │   └─► Connects to P2P Network
    │       ├─► Bootstrap: localhost:7000
    │       ├─► mDNS Discovery
    │       └─► Discovers peers
    │
    └─► Storage Node Client
        └─► Queries Fact Packages
            └─► Lists channels and content
```

## Usage

### 1. Start SpaceKit OS

```bash
# SpaceKit OS desktop app starts automatically
# Messaging node runs on localhost:7000
# Storage node runs on localhost:4001
```

### 2. Discover Peers

```bash
spacekit message peers
```

**Expected Output:**
```
🔍 Discovering peers...
   Connecting to messaging node on localhost:7000...
   ✅ Messaging node client created
   🔍 Discovering peers via mDNS...

   📊 Messaging Node Status:
      Active Connections: 2
      Registered Users: 5

   ✅ Found 2 connected peer(s)!
```

### 3. List Channels

```bash
spacekit content list-channels
```

**Expected Output:**
```
📺 Listing channels...

   Found 2 channel(s):

   1. Channel ID: abc123...
      Author: did:spacekit:user:creator
      Created: 2025-01-15 10:00:00
      Tags: channel, tutorial, education
```

### 4. List Content

```bash
spacekit content list-content --channel channel_123
```

**Expected Output:**
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
```

## Troubleshooting

### No Peers Found

**Possible Causes:**
- SpaceKit OS messaging not running
- mDNS discovery still in progress
- Firewall blocking mDNS
- Nodes on different networks

**Solutions:**
1. Ensure SpaceKit OS is running with messaging enabled
2. Wait 5-10 seconds for mDNS discovery
3. Check firewall settings (mDNS uses UDP port 5353)
4. Ensure both nodes are on the same local network

### Cannot Connect to Messaging Node

**Error:** `Could not connect to messaging node`

**Solutions:**
1. Verify SpaceKit OS is running
2. Check that messaging node is listening on port 7000
3. Try connecting to a different port if SpaceKit OS uses a different one
4. Check for port conflicts

### No Content Found

**Possible Causes:**
- No content published yet
- Content not tagged correctly
- Storage node not connected

**Solutions:**
1. Publish some content first: `spacekit content publish --channel test --file video.mp4 --title "Test"`
2. Verify content has correct tags (`"content"`, `"published"`)
3. Check storage node connection: `spacekit connect storage --url http://localhost:4001`

## Future Enhancements

### 1. HTTP API in SpaceKit OS

Add a simple HTTP server to SpaceKit OS to expose:
- Peer information
- Node status
- Content metadata

**Benefits:**
- Direct access without creating new nodes
- Lower resource usage
- Faster queries

### 2. Shared Configuration File

Have SpaceKit OS write peer information to a shared file:
- `~/.spacekit/peers.json`
- CLI reads from this file
- Updated in real-time

### 3. IPC Mechanism

Use inter-process communication:
- Unix domain sockets
- Named pipes
- Shared memory

## Summary

✅ **Peer Discovery**: CLI connects to P2P network and discovers peers via mDNS
✅ **Content Viewing**: CLI queries Fact Packages from Storage Node
✅ **Channel Listing**: CLI lists channels by querying Fact Packages with `"channel"` tag
✅ **Content Listing**: CLI lists content by querying Fact Packages with `"content"` and `"published"` tags

The CLI now works with SpaceKit OS by connecting to the same P2P network!

