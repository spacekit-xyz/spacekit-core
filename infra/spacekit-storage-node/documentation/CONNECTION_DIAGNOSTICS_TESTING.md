# Connection Diagnostics Testing Guide

## Overview

This guide explains how to test and diagnose P2P connection issues in the SpaceKit Storage Node network.

## Architecture

- **Storage Node**: Provides basic connection events via `NetworkEvent` (PeerConnected/PeerDisconnected)
- **Simulator**: Aggregates and monitors connection health across all services
- **Connection Diagnostics**: Unified monitoring system in `spacekit-simulator`

## Testing Setup

### Prerequisites

1. Build the storage node with P2P features:
   ```bash
   cd spacekit-storage-node
   cargo build --features p2p,database
   ```

2. Ensure you have multiple terminals available (3+ recommended)

### Test Scenario 1: Basic Connection Test

**Terminal 1 - Node 1 (Bootstrap Node)**
```bash
cd spacekit-storage-node
cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 1 --port 9001
```

**Note the peer ID from the output** (e.g., `12D3KooW...`)

**Terminal 2 - Node 2 (Connects to Node 1)**
```bash
cd spacekit-storage-node
cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 2 --port 9002 --bootstrap /ip4/127.0.0.1/tcp/9001/p2p/<PEER_ID_FROM_NODE_1>
```

**Terminal 3 - Node 3 (Connects to Node 1)**
```bash
cd spacekit-storage-node
cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 3 --port 9003 --bootstrap /ip4/127.0.0.1/tcp/9001/p2p/<PEER_ID_FROM_NODE_1>
```

### What to Observe

1. **Connection Establishment**:
   - Look for `✅ Connection established with peer:` messages
   - Connections should appear in status reports every 30 seconds

2. **Connection Stability**:
   - Monitor if connections persist or disconnect immediately
   - Check for `❌ Connection closed` messages

3. **KeepAlive Timeouts**:
   - Watch for `⚠️ KeepAlive timeout detected` warnings
   - These indicate connections are closing due to inactivity

4. **Status Reports** (every 30 seconds):
   ```
   📊 Connection Status Report:
     Active connections: 2
     Stored chunks: 0
     Known DIDs: 0
     Connected peers:
       - 12D3KooW...
       - 12D3KooW...
   ```

### Expected Behavior

✅ **Good Signs**:
- Connections establish successfully
- Connections remain active (don't disconnect immediately)
- Status reports show consistent peer counts
- No KeepAlive timeout warnings

❌ **Problem Signs**:
- Connections establish then immediately close
- Frequent `❌ Connection closed` messages
- `⚠️ KeepAlive timeout` warnings
- Status reports show 0 active connections despite peers running

## Diagnosing Connection Issues

### Issue: Connections Close Immediately

**Symptoms**:
- `✅ Connection established` followed immediately by `❌ Connection closed`
- Status reports show 0 active connections

**Possible Causes**:
1. **KeepAlive Timeout**: Connection idle too long
   - **Solution**: Reduce keepalive interval or increase connection activity
   
2. **Protocol Mismatch**: Peers using incompatible protocols
   - **Solution**: Ensure all nodes use same libp2p version
   
3. **Network Issues**: Firewall or NAT blocking connections
   - **Solution**: Check firewall rules, ensure ports are open

**Diagnostic Steps**:
1. Check connection close cause in logs:
   ```
   ❌ Connection closed with peer: ... via ...
      Cause: KeepAlive Timeout - connection idle too long
   ```

2. Monitor connection duration:
   - If connections last < 60 seconds → KeepAlive timeout likely
   - If connections last < 1 second → Protocol/network issue

### Issue: Peers Not Discovering Each Other

**Symptoms**:
- Status reports show 0 active connections
- No `✅ Connection established` messages

**Possible Causes**:
1. **mDNS Not Working**: Local network discovery failing
   - **Solution**: Use bootstrap peers explicitly
   
2. **Wrong Network**: Peers on different networks
   - **Solution**: Use public IPs or VPN for cross-network

3. **Port Conflicts**: Multiple nodes trying to use same port
   - **Solution**: Use different ports for each node

**Diagnostic Steps**:
1. Check for mDNS discovery messages:
   ```
   Discovered peer via mDNS: ... at ...
   ```

2. Verify bootstrap configuration:
   - Ensure bootstrap peer ID is correct
   - Check that bootstrap node is running

## Using Connection Diagnostics API

### From Simulator

The simulator's `ConnectionDiagnostics` provides:

```rust
use spacekit_simulator::{ConnectionDiagnostics, ConnectionEvent, ServiceType};

// Get health summary
let summary = diagnostics.get_health_summary().await;
println!("Active connections: {}", summary.active_connections);
println!("KeepAlive timeouts: {}", summary.keepalive_timeouts);

// Get peer statistics
let stats = diagnostics.get_peer_stats(&peer_id).await;
if let Some(stat) = stats {
    println!("Connection count: {}", stat.connection_count);
    println!("Avg duration: {}s", stat.avg_connection_duration);
}

// Get recent events
let events = diagnostics.get_recent_events(10).await;
for event in events {
    match event {
        ConnectionEvent::Connected { peer_id, service, .. } => {
            println!("Connected: {} ({:?})", peer_id, service);
        }
        ConnectionEvent::Disconnected { peer_id, cause, duration, .. } => {
            println!("Disconnected: {} - {} ({}s)", peer_id, cause.unwrap_or_default(), duration.unwrap_or(0));
        }
        _ => {}
    }
}
```

## Integration with SpaceKit OS

When running SpaceKit OS desktop:

1. The simulator automatically tracks connections from:
   - Storage node P2P network
   - Messaging node P2P network
   - Cross-network bridges

2. Connection events are logged to console with emoji indicators:
   - ✅ = Connection established
   - ❌ = Connection closed
   - ⚠️ = KeepAlive timeout

3. Status reports appear every 30 seconds showing:
   - Active connection count
   - Peer list
   - Network statistics

## Troubleshooting Checklist

- [ ] All nodes built with `--features p2p,database`
- [ ] Each node uses unique port
- [ ] Bootstrap peer ID is correct
- [ ] Firewall allows TCP connections on configured ports
- [ ] Nodes are on same network (for mDNS) or using bootstrap
- [ ] No port conflicts between nodes
- [ ] Check logs for connection close causes
- [ ] Monitor connection duration in status reports

## Next Steps

1. **If connections are stable**: ✅ System is working correctly
2. **If KeepAlive timeouts occur**: Reduce keepalive interval or increase activity
3. **If connections fail immediately**: Check network/firewall configuration
4. **If peers don't discover**: Use explicit bootstrap configuration

## Advanced Testing

### Test with SpaceKit OS Desktop

1. Start SpaceKit OS desktop (runs simulator with storage/messaging nodes)
2. Run CLI or additional storage nodes
3. Monitor connection events in SpaceKit OS console
4. Check connection health via simulator diagnostics API

### Test Cross-Network Connections

1. Start simulator on different machines/networks
2. Use cross-network bridge to connect
3. Monitor connection health across networks
4. Test NAT traversal if applicable

