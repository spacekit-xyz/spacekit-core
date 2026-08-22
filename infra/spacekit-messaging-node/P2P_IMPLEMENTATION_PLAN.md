# P2P Messaging Implementation Plan

## Overview
Implement full peer-to-peer messaging using libp2p with automatic discovery and NAT traversal.

## Phase 1: Core libp2p Integration ⏳

### 1.1 Update Dependencies
```toml
[dependencies]
libp2p = { version = "0.53", features = [
    "tcp",
    "noise",
    "yamux",
    "gossipsub",
    "mdns",
    "kad",
    "relay",
    "dcutr",
    "identify",
    "ping"
]}
```

### 1.2 Implement Network Behavior
**File**: `src/network_p2p.rs` (new)
- Custom libp2p behavior combining:
  - Gossipsub for message broadcasting
  - mDNS for local discovery
  - Kademlia DHT for peer routing
  - Identify for peer information
  - Ping for keepalive

### 1.3 Replace Simplified Network
**File**: `src/network.rs`
- Replace TODO implementation with real libp2p
- Event loop for handling swarm events
- Message routing logic

## Phase 2: Discovery Mechanisms ⏳

### 2.1 mDNS (Local Network Discovery)
- Automatic peer discovery on LAN
- Broadcast/listen for SWTCHX nodes
- Service: `_swtchx-messaging._tcp.local`

### 2.2 Bootstrap Peers
- Connect to known peers on startup
- Persistent peer list
- Peer reputation system

### 2.3 Kademlia DHT
- Global peer discovery
- DID → Multiaddr resolution
- Content routing for offline messages

## Phase 3: NAT Traversal ⏳

### 3.1 Circuit Relay
- Relay protocol for NAT traversal
- Public relay servers
- Automatic relay selection

### 3.2 Hole Punching (DCUtR)
- Direct Connection Upgrade through Relay
- STUN-like functionality
- Fallback to relay if direct fails

## Phase 4: Message Routing ⏳

### 4.1 Direct Messages
- Point-to-point delivery
- Offline message queuing
- Delivery receipts

### 4.2 Group Messages
- Gossipsub topics per group
- Efficient multicast
- Message deduplication

### 4.3 Store & Forward
- Queue messages when peer offline
- DHT-based message storage
- Retry logic with backoff

## Phase 5: Integration with SWTCHX OS ⏳

### 5.1 Configuration
- Network mode selection (embedded/p2p/relay)
- Peer management UI
- Discovery settings

### 5.2 Tauri Commands
- `enable_p2p_mode()`
- `add_bootstrap_peer(addr)`
- `get_connected_peers()`
- `get_network_status()`

### 5.3 Real-time Events
- Peer connected/disconnected
- Message received
- Network status changes

## Implementation Order

### Week 1: Core Infrastructure
- [ ] Add libp2p dependencies
- [ ] Create network_p2p.rs with basic behavior
- [ ] Implement swarm initialization
- [ ] Basic peer connection

### Week 2: Discovery
- [ ] mDNS implementation
- [ ] Bootstrap peer logic
- [ ] Peer store persistence

### Week 3: Messaging
- [ ] Gossipsub integration
- [ ] Direct message routing
- [ ] Group message broadcasting

### Week 4: Advanced Features
- [ ] DHT setup
- [ ] Circuit relay
- [ ] NAT traversal

### Week 5: Integration & Testing
- [ ] SWTCHX OS integration
- [ ] Multi-instance testing
- [ ] LAN testing
- [ ] Internet testing via relay

## Testing Strategy

### Unit Tests
- Network behavior
- Message serialization
- Peer discovery

### Integration Tests
- Two-node messaging
- Group messaging
- NAT traversal scenarios

### Manual Testing
- LAN discovery (mDNS)
- Internet via relay
- Offline message delivery

## Success Criteria

- ✅ Two SWTCHX OS instances discover each other on LAN
- ✅ Messages delivered without relay
- ✅ Automatic relay fallback for NAT
- ✅ Group messaging works across network
- ✅ Offline messages delivered when peer online

## Files to Create/Modify

### New Files
1. `src/network_p2p.rs` - libp2p implementation
2. `src/discovery.rs` - Discovery protocols
3. `src/relay.rs` - Relay/NAT traversal
4. `examples/p2p_test.rs` - Testing example

### Modified Files
1. `src/network.rs` - Replace simplified impl
2. `src/lib.rs` - Export new modules
3. `src/config.rs` - Add P2P config options
4. `Cargo.toml` - Dependencies
5. `README.md` - Update docs

### SWTCHX OS Changes
1. `src-tauri/src/lib.rs` - New commands
2. `src/components/MessengerPage.tsx` - Network status UI
3. `src/types/messaging.ts` - Network types

## Next Steps

Start with Week 1 implementation...

