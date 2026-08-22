# SpaceKit Network CLI Specification v2.0

> **Official specification for the unified SpaceKit Network command-line interface supporting quantum-resistant cryptography, distributed computing, storage, consensus, and identity management.**

## Overview

The SpaceKit Network CLI (`swtch`) provides a unified command-line interface for all SpaceKit Network operations, including quantum-resistant cryptography, distributed computing, collaborative storage, consensus operations, and decentralized identity management.

### Design Principles

- **Unified Interface**: Single CLI binary for all SpaceKit Network operations
- **Library Integration**: Leverage existing `swtch-compute-node`, `swtch-storage-node`, and `swtch-network-did` libraries
- **Backward Compatibility**: Maintain existing quantum-resistant cryptography functionality
- **Production Ready**: Built on proven production libraries with comprehensive error handling

## Current Implementation Status

### ✅ Implemented (Production Ready)
- **Quantum Cryptography**: 19 post-quantum algorithms + ECIES
- **Key Management**: `keypair`, `encrypt`, `decrypt`, `encapsulate`, `decapsulate`
- **Multi-Chain Support**: Ethereum, Solana, Bitcoin key generation
- **Modern UX**: Color-coded output, emoji indicators, comprehensive help

### 🔄 To Be Implemented (This Specification)
- **Task Management**: Distributed computing operations
- **Storage Operations**: Collaborative and specialized storage
- **Consensus Operations**: Unified consensus interactions
- **Identity Management**: DID creation and verification
- **Network Discovery**: Service discovery and reputation monitoring
- **Project Management**: Workspace initialization and configuration

## Architecture

### Dependency Strategy

```toml
[dependencies]
# Existing crypto dependencies (keep unchanged)
aes-gcm = "0.10.3"
chacha20poly1305 = "0.10.1"
clap = { version = "4.5.2", features = ["derive"] }
colored = "2.1.0"
ecies = "0.2.6"
ethers = "2.0.14"
hex = "0.4.3"
ring = "0.17.8"
swtch-primitives = { path = "../swtch-network-primitives" }
tokio = { version = "1.39.2", features = ["full"] }

# New SpaceKit Network integrations
swtch-compute-node = { path = "../swtch-compute-node" }
swtch-network-did = { path = "../swtch-network-did" }
# Note: swtch-storage-node is already integrated via swtch-compute-node
```

### Code Structure

```
swtch-network-cli/
├── src/
│   ├── main.rs                 # CLI entry point and routing
│   ├── commands/
│   │   ├── crypto.rs          # Existing crypto commands (unchanged)
│   │   ├── task.rs            # Task management commands
│   │   ├── storage.rs         # Storage operations
│   │   ├── consensus.rs       # Consensus operations
│   │   ├── did.rs             # Identity management
│   │   ├── network.rs         # Network operations
│   │   └── init.rs            # Project initialization
│   ├── utils/
│   │   ├── node_manager.rs    # ComputeNode instance management
│   │   ├── config.rs          # Configuration management
│   │   └── formatting.rs     # Output formatting helpers
│   └── lib.rs                 # Shared types and utilities
├── Cargo.toml
└── README.md
```

## Command Specification

### Task Management Commands

```bash
spacekit task submit --file <WASM_FILE> --runtime <RUNTIME> --owner-did <DID> [--input <INPUT_FILE>]
spacekit task status <TASK_ID>
spacekit task list [--status <STATUS>] [--owner <DID>]
spacekit task cancel <TASK_ID>
spacekit task result <TASK_ID> [--output <FILE>]
spacekit task watch <TASK_ID>
```

**Implementation**: Uses `ComputeNode::submit_task()`, `execute_task()`, `get_task_status()`, `cancel_task()` from `swtch-compute-node`.

**Required Parameters**:
- `--file`: Path to WebAssembly bytecode file
- `--runtime`: Execution runtime (`wasm`, `gpu`, `hybrid`)
- `--owner-did`: DID of task owner

**Optional Parameters**:
- `--input`: Input data file for computation
- `--encryption`: Quantum algorithm for encryption (default: Kyber768)
- `--max-cost`: Maximum acceptable cost for execution

### Storage Operations

```bash
spacekit storage upload --file <FILE> --type <TYPE> [--owners <OWNERS>] [--consensus <POLICY>]
spacekit storage download <FILE_ID> [--output <FILE>]
spacekit storage share <FILE_ID> --with <DID> [--permissions <PERMS>]
spacekit storage list [--owned-by-me] [--type <TYPE>]
spacekit storage collaborative create --owners <OWNERS> --file <FILE> --consensus <POLICY>
spacekit storage stats
```

**Implementation**: Uses `ComputeNode::submit_and_store_task()`, `create_collaborative_compute_task()`, `get_comprehensive_storage_stats()` from `swtch-compute-node`.

**Storage Types**:
- `quantum-safe`: Standard quantum-resistant storage
- `collaborative`: Multi-party ownership with consensus
- `medical`: HIPAA-compliant medical records
- `research`: Academic research data marketplace

**Consensus Policies**:
- `unanimous`: All owners must approve
- `majority`: 51%+ owners must approve
- `threshold:N`: Specific number of owners (e.g., `threshold:3`)

### Consensus Operations

```bash
spacekit consensus submit-proposal --type <TYPE> --data <DATA> [--committee <COMMITTEE>]
spacekit consensus vote --proposal-id <ID> --vote <VOTE>
spacekit consensus status [--proposal-id <ID>]
spacekit consensus list [--status <STATUS>]
spacekit consensus migration status
```

**Implementation**: Uses `UnifiedSpaceKitConsensus::submit_*_proposal()`, `ConsensusMigrationManager` from `swtch-compute-node`.

**Proposal Types**:
- `block`: Traditional block proposal
- `metrics`: Network metrics validation
- `hybrid`: Combined block and metrics proposal

**Vote Options**: `approve`, `reject`, `abstain`

### Identity Management

```bash
spacekit did create [--algorithm <ALGORITHM>] [--save]
spacekit did verify --did <DID>
spacekit did update --did <DID> --add-key <PUBLIC_KEY>
spacekit did resolve <DID>
spacekit did list [--owned-by-me]
```

**Implementation**: Uses `QuantumResistantDID`, DID registry operations from `swtch-network-did`.

**Supported Algorithms**: All 19 quantum-resistant algorithms supported by SpaceKit primitives.

### Network Operations

```bash
spacekit network status
spacekit network discover [--service-type <TYPE>]
spacekit network peers
spacekit network reputation --did <DID>
spacekit network reputation watch --did <DID> [--interval <SECONDS>]
```

**Implementation**: Uses `NetworkService`, `MLReputationEngine`, `P2PServiceDiscoveryManager` from `swtch-compute-node`.

**Service Types**: `compute`, `storage`, `messaging`, `consensus`

### Project Management

```bash
spacekit init [--did <DID>] [--algorithm <ALGORITHM>] [--name <PROJECT_NAME>]
spacekit config set <KEY> <VALUE>
spacekit config get <KEY>
spacekit config list
```

**Configuration Keys**:
- `node.endpoint`: SpaceKit node endpoint URL
- `identity.default-did`: Default DID for operations
- `crypto.default-algorithm`: Default quantum algorithm
- `network.chain`: Default blockchain network

## API Integration Specifications

### ComputeNode Management

```rust
// Global compute node instance management
static COMPUTE_NODE: LazyLock<Arc<RwLock<Option<ComputeNode>>>> = 
    LazyLock::new(|| Arc::new(RwLock::new(None)));

async fn get_or_create_compute_node() -> Result<ComputeNode, CliError> {
    let mut node_guard = COMPUTE_NODE.write().await;
    
    if node_guard.is_none() {
        let config = load_compute_config().await?;
        let mut node = ComputeNode::new(config).await?;
        node.initialize().await?;
        *node_guard = Some(node);
    }
    
    Ok(node_guard.as_ref().unwrap().clone())
}
```

### Task Management Integration

```rust
use swtch_compute_node::{ComputeNode, ComputeTask, TaskStatus};

async fn handle_task_submit(
    file: String,
    runtime: String,
    owner_did: String,
    input_file: Option<String>,
) -> Result<(), CliError> {
    let node = get_or_create_compute_node().await?;
    
    let code = std::fs::read(&file)
        .map_err(|e| CliError::FileRead(file.clone(), e))?;
    
    let input_data = if let Some(input_path) = input_file {
        std::fs::read(&input_path)
            .map_err(|e| CliError::FileRead(input_path, e))?
    } else {
        vec![]
    };
    
    let task = node.submit_task(
        format!("cli_task_{}", chrono::Utc::now().timestamp()),
        runtime,
        code,
        input_data,
        owner_did,
    ).await?;
    
    println!("✅ Task submitted: {}", task.id.green());
    println!("📋 Status: {:?}", task.status);
    println!("💰 Estimated cost: {:?} SpaceKit", task.estimated_cost);
    
    Ok(())
}
```

### Storage Integration

```rust
use swtch_compute_node::{StorageType, CollaborativeComputeResult};

async fn handle_storage_upload(
    file: String,
    storage_type: String,
    owners: Option<String>,
    consensus: Option<String>,
) -> Result<(), CliError> {
    let node = get_or_create_compute_node().await?;
    let code = std::fs::read(&file)?;
    
    if let Some(owners_str) = owners {
        // Collaborative storage
        let owner_list: Vec<String> = owners_str
            .split(',')
            .map(|s| s.trim().to_string())
            .collect();
        
        let result = node.create_collaborative_compute_task(
            format!("collaborative_{}", chrono::Utc::now().timestamp()),
            "storage".to_string(),
            code,
            vec![],
            owner_list.clone(),
            consensus,
        ).await?;
        
        println!("✅ Collaborative storage created: {}", result.file_id.green());
        println!("👥 Owners: {}", owner_list.join(", "));
        println!("🤝 Consensus: {}", result.consensus_policy);
    } else {
        // Standard storage
        let storage_type_enum = parse_storage_type(&storage_type)?;
        let result = node.submit_and_store_task(
            format!("storage_{}", chrono::Utc::now().timestamp()),
            "storage".to_string(),
            code,
            vec![],
            get_default_did()?,
            Some(storage_type_enum),
        ).await?;
        
        println!("✅ File stored: {}", result.task_id.green());
        println!("💾 Storage type: {:?}", result.storage_type);
        println!("🔐 Quantum safe: {}", result.quantum_safe);
    }
    
    Ok(())
}
```

## Error Handling

### Custom Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("File read error: {0} - {1}")]
    FileRead(String, std::io::Error),
    
    #[error("Compute node error: {0}")]
    ComputeNode(#[from] swtch_compute_node::ComputeError),
    
    #[error("DID error: {0}")]
    Did(String),
    
    #[error("Configuration error: {0}")]
    Config(String),
    
    #[error("Network error: {0}")]
    Network(String),
}
```

## Testing Strategy

### Unit Tests
- Command parsing validation
- Error handling scenarios
- Configuration management
- Utility function testing

### Integration Tests
- End-to-end command execution
- ComputeNode integration
- Storage operations
- Consensus interactions

### Test Structure

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_task_submit_command() {
        // Test task submission with mock ComputeNode
    }
    
    #[tokio::test]
    async fn test_storage_collaborative_creation() {
        // Test collaborative storage creation
    }
    
    #[tokio::test]
    async fn test_consensus_proposal_submission() {
        // Test consensus proposal submission
    }
}
```

## Implementation Plan

### Phase 1: Core Infrastructure (Week 1-2)
1. **Dependencies**: Add `swtch-compute-node` dependency
2. **Architecture**: Implement command structure and routing
3. **Node Management**: Implement ComputeNode instance management
4. **Configuration**: Basic configuration management system

### Phase 2: Task Management (Week 3)
1. **Task Commands**: Implement all task management commands
2. **WebAssembly Support**: File handling for WASM bytecode
3. **Runtime Selection**: GPU/CPU/Hybrid runtime support
4. **Status Monitoring**: Real-time task status updates

### Phase 3: Storage Operations (Week 4)
1. **Basic Storage**: Upload/download operations
2. **Collaborative Storage**: Multi-party ownership features
3. **Specialized Storage**: Medical and research storage types
4. **Storage Discovery**: List and search operations

### Phase 4: Advanced Features (Week 5)
1. **Consensus Operations**: Proposal submission and voting
2. **Identity Management**: DID creation and verification
3. **Network Operations**: Service discovery and reputation
4. **Project Management**: Workspace initialization

### Phase 5: Polish & Testing (Week 6)
1. **Error Handling**: Comprehensive error handling
2. **Testing**: Full test suite implementation
3. **Documentation**: Complete command documentation
4. **Performance**: Optimization and performance tuning

## Success Criteria

### Functional Requirements
- ✅ All whitepaper-claimed commands implemented and functional
- ✅ Seamless integration with existing SpaceKit Network libraries
- ✅ Backward compatibility with existing crypto commands
- ✅ Comprehensive error handling and user feedback

### Non-Functional Requirements
- ✅ **Performance**: Command response time < 2 seconds for most operations
- ✅ **Reliability**: 99.9% success rate for valid operations
- ✅ **Usability**: Intuitive command structure with helpful error messages
- ✅ **Maintainability**: Clean code architecture with comprehensive tests

### Delivery Timeline
- **Total Duration**: 6 weeks
- **Development Effort**: ~1200-1500 additional lines of code
- **Testing Coverage**: Minimum 90% test coverage
- **Documentation**: Complete command reference and examples

## Risk Mitigation

### Technical Risks
- **Integration Complexity**: Mitigated by using proven production libraries
- **Performance Issues**: Addressed through lazy loading and efficient node management
- **Breaking Changes**: Prevented by maintaining backward compatibility

### Operational Risks
- **User Confusion**: Mitigated by clear command structure and comprehensive help
- **Configuration Complexity**: Addressed through sensible defaults and clear documentation
- **Debugging Difficulty**: Resolved through comprehensive error messages and logging

This specification provides a complete roadmap for implementing the remaining SpaceKit Network CLI functionality while leveraging existing production-ready libraries and maintaining the high quality of the current implementation.