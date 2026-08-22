# Connection Diagnostics - Quick Start Testing

## Quick Test (3 Terminals)

### Terminal 1 - Bootstrap Node
```bash
cd spacekit-storage-node
cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 1 --port 9001
```

**Copy the peer ID** from output (looks like `12D3KooW...`)

### Terminal 2 - Connect to Node 1
```bash
cd spacekit-storage-node
cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 2 --port 9002 --bootstrap /ip4/127.0.0.1/tcp/9001/p2p/<PEER_ID>
```

### Terminal 3 - Connect to Node 1
```bash
cd spacekit-storage-node
cargo run --example connection_diagnostics_test --features p2p,database -- --node-id 3 --port 9003 --bootstrap /ip4/127.0.0.1/tcp/9001/p2p/<PEER_ID>
```

## What to Look For

### ✅ Success Indicators
- `✅ Connection established` messages appear
- Status reports (every 30s) show active connections > 0
- Connections remain stable (don't disconnect immediately)
- No `⚠️ KeepAlive timeout` warnings

### ❌ Problem Indicators
- Connections establish then immediately close
- Status reports show 0 active connections
- Frequent `❌ Connection closed` messages
- `⚠️ KeepAlive timeout` warnings

## Expected Output

**Good Output:**
```
📊 Connection Status Report:
  Active connections: 2
  Stored chunks: 0
  Known DIDs: 0
  Connected peers:
    - 12D3KooW...
    - 12D3KooW...
```

**Problem Output:**
```
📊 Connection Status Report:
  Active connections: 0
  ⚠️  No active connections
     This may indicate:
     - KeepAlive timeouts
     - Network connectivity issues
     - Peers not discovered yet
```

## Troubleshooting

1. **Connections close immediately**: Check logs for close cause
2. **No connections**: Verify bootstrap peer ID is correct
3. **KeepAlive timeouts**: Connections idle too long - this is expected behavior if no activity

See `CONNECTION_DIAGNOSTICS_TESTING.md` for detailed troubleshooting.

