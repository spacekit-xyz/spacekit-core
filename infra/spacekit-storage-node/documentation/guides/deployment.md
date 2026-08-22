# Standalone Storage Node Service Deployment

## Overview

The best way to run `spacekit-storage-node` is as a **standalone service managed by spacekit-simulator**. This provides:

- ✅ Full feature set (P2P networking, API server, quantum encryption)
- ✅ Lifecycle management (start, stop, restart, health checks)
- ✅ Service discovery and load balancing
- ✅ Automatic configuration and key management
- ✅ Resource isolation and scaling

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│              SpaceKit Simulator (Orchestrator)          │
│                                                          │
│  ┌──────────────────────────────────────────────────┐  │
│  │         Service Mesh & Discovery                 │  │
│  └──────────────────────────────────────────────────┘  │
│                        │                                 │
│                        ▼                                 │
│  ┌──────────────────────────────────────────────────┐  │
│  │    Storage Node Manager                          │  │
│  │  - Deploy StorageNode instances                  │  │
│  │  - Manage lifecycle                             │  │
│  │  - Handle configuration                          │  │
│  │  - Monitor health                                │  │
│  └──────────────────────────────────────────────────┘  │
│                        │                                 │
│                        ▼                                 │
│  ┌──────────────────────────────────────────────────┐  │
│  │    StorageNode Instances (Standalone Services)   │  │
│  │                                                   │  │
│  │  ┌──────────────┐  ┌──────────────┐            │  │
│  │  │ StorageNode  │  │ StorageNode   │            │  │
│  │  │ Instance 1   │  │ Instance 2    │            │  │
│  │  │              │  │               │            │  │
│  │  │ • P2P        │  │ • P2P         │            │  │
│  │  │ • API Server │  │ • API Server  │            │  │
│  │  │ • Database   │  │ • Database    │            │  │
│  │  │ • Quantum    │  │ • Quantum     │            │  │
│  │  │   Encryption │  │   Encryption  │            │  │
│  │  └──────────────┘  └──────────────┘            │  │
│  └──────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Benefits

### 1. Full Feature Set
- **P2P Networking**: Distributed storage and peer discovery
- **API Server**: RESTful API for file operations
- **Quantum Encryption**: Full data-at-rest encryption
- **Service Discovery**: Automatic registration in service mesh

### 2. Lifecycle Management
- **Automatic Startup**: Configured and started by orchestrator
- **Health Monitoring**: Continuous health checks
- **Graceful Shutdown**: Proper cleanup on stop
- **Auto-Restart**: Automatic recovery from failures

### 3. Resource Management
- **Port Allocation**: Automatic port assignment
- **Resource Isolation**: Separate data directories per instance
- **Scaling**: Easy horizontal scaling with multiple instances
- **Load Balancing**: Service mesh handles load distribution

### 4. Configuration Management
- **Automatic Key Setup**: AWS Secrets Manager or local keys
- **Environment Configuration**: Environment variables managed
- **Database Encryption**: Automatic encryption setup
- **Network Configuration**: P2P and API ports configured

## Implementation

### Enhanced Orchestrator Deployment

The orchestrator should deploy full `StorageNode` instances instead of just `Database`:

```rust
// Enhanced deployment method
async fn deploy_native_storage_nodes(
    &self,
    request: &NodeDeploymentRequest,
    replication_factor: u32,
    quantum_encryption: bool,
    wal_enabled: bool,
) -> Result<DeploymentResponse> {
    let mut deployed_instances = Vec::new();
    
    for i in 0..request.replicas {
        let instance_id = format!("{}-storage-{}", request.did, i);
        
        // Create full StorageNode configuration
        let storage_config = StorageNodeConfig {
            max_storage_bytes: 100 * 1024 * 1024 * 1024, // 100GB
            data_dir: PathBuf::from(format!("./storage_data/{}", instance_id)),
            database_path: Some(PathBuf::from(format!("./storage_data/{}/db", instance_id))),
            node_did: format!("did:spacekit:storage:{}", instance_id),
            preferred_algorithm: if quantum_encryption { 
                "kyber1024".to_string() 
            } else { 
                "none".to_string() 
            },
            network_config: NetworkConfig {
                listen_port: self.port_manager.allocate_port("p2p", &instance_id).await?,
                ..Default::default()
            },
            #[cfg(feature = "api-server")]
            api_config: Some(ServerConfig {
                port: self.port_manager.allocate_port("api", &instance_id).await?,
                enable_cors: true,
                ..Default::default()
            }),
            ..Default::default()
        };
        
        // Create and start full StorageNode
        let storage_node = Arc::new(StorageNode::new(storage_config).await?);
        storage_node.start().await?;
        
        // Register in service mesh
        self.service_mesh.register_storage_node_service(
            &storage_node,
            &instance_id
        ).await?;
        
        // Store in orchestrator
        self.storage_nodes.write().await.insert(
            instance_id.clone(),
            storage_node
        );
        
        deployed_instances.push(NodeInstance {
            instance_id: instance_id.clone(),
            node_type: "native-storage".to_string(),
            status: "running".to_string(),
            endpoint: format!("http://localhost:{}", storage_config.api_config.as_ref().unwrap().port),
        });
    }
    
    Ok(DeploymentResponse {
        deployment_id: format!("deploy-{}", uuid::Uuid::new_v4()),
        instances: deployed_instances,
        status: "success".to_string(),
    })
}
```

### Service Mesh Integration

Enhanced service mesh registration for full StorageNode:

```rust
pub async fn register_storage_node_service(
    &self,
    storage_node: &Arc<StorageNode>,
    instance_id: &str,
) -> Result<()> {
    let config = storage_node.config();
    
    // Register API endpoint
    let api_endpoint = ServiceEndpoint {
        service_name: "storage-api".to_string(),
        instance_id: instance_id.to_string(),
        host: "localhost".to_string(),
        port: config.api_config.as_ref().map(|c| c.port).unwrap_or(3030),
        health_check_path: "/health".to_string(),
        quantum_encrypted: true,
        node_type: ServiceNodeType::Native,
    };
    self.register_service_endpoint(api_endpoint).await?;
    
    // Register P2P endpoint
    let p2p_endpoint = ServiceEndpoint {
        service_name: "storage-p2p".to_string(),
        instance_id: instance_id.to_string(),
        host: "localhost".to_string(),
        port: config.network_config.listen_port,
        health_check_path: "/p2p/health".to_string(),
        quantum_encrypted: true,
        node_type: ServiceNodeType::Native,
    };
    self.register_service_endpoint(p2p_endpoint).await?;
    
    // Start health monitoring
    self.health_monitor.start_monitoring(instance_id).await?;
    
    Ok(())
}
```

## Usage Example

### Deploy Storage Node via Orchestrator

```rust
use spacekit_simulator::orchestration::{Orchestrator, NodeDeploymentRequest, NodeDeploymentType};

let orchestrator = Orchestrator::new().await?;

// Deploy storage node with full features
let deployment = NodeDeploymentRequest {
    deployment_type: NodeDeploymentType::NativeStorage {
        replication_factor: 3,
        quantum_encryption: true,
        wal_enabled: true,
    },
    did: "did:spacekit:storage:cluster1".to_string(),
    replicas: 3, // Deploy 3 instances
    config: Some(serde_json::json!({
        "max_storage_gb": 500,
        "enable_p2p": true,
        "enable_api": true,
        "quantum_algorithm": "kyber1024",
    })),
    namespace: Some("production".to_string()),
    private_key: None,
};

let response = orchestrator.deploy_nodes(deployment).await?;

println!("Deployed {} storage nodes", response.instances.len());
for instance in &response.instances {
    println!("  - {}: {}", instance.instance_id, instance.endpoint);
}
```

### Access Storage Node

```rust
// Get storage node from orchestrator
let storage_nodes = orchestrator.list_storage_nodes().await?;
let storage_node = storage_nodes.first().unwrap();

// Use storage node
let file_id = storage_node.store_file(
    "document.pdf",
    &file_data,
    "did:spacekit:user:alice",
    Some("application/pdf".to_string())
).await?;

// Access via API
let api_url = format!("http://localhost:3030/file/{}", file_id);
```

## Configuration

### Environment Variables

The orchestrator automatically sets up environment variables:

```bash
# Database encryption (automatic)
export DATABASE_KEM_SECRET_NAME="spacekit/storage-node-{instance_id}-keys"
export AWS_DEFAULT_REGION="us-east-1"

# Storage node configuration
export SPACEKIT_DATA_DIR="./storage_data/{instance_id}"
export SPACEKIT_ENABLE_WAL=true
export SPACEKIT_BACKUP_COUNT=5
export SPACEKIT_ENABLE_ENCRYPTION=true
export SPACEKIT_QUANTUM_ALGORITHM=kyber1024
```

### Automatic Key Management

- **Development**: Keys stored locally in `./storage_data/{instance_id}/db.key`
- **Production**: Keys stored in AWS Secrets Manager with KMS encryption
- **Automatic Setup**: Keys generated and stored automatically on first run

## Lifecycle Management

### Start Service

```rust
// Storage node starts automatically when deployed
let deployment = orchestrator.deploy_nodes(request).await?;
// Service is now running
```

### Stop Service

```rust
// Stop specific instance
orchestrator.stop_node("storage-instance-1").await?;

// Stop all storage nodes
orchestrator.stop_all_nodes("storage").await?;
```

### Restart Service

```rust
// Restart with new configuration
orchestrator.restart_node("storage-instance-1", new_config).await?;
```

### Health Checks

```rust
// Check health
let health = orchestrator.check_node_health("storage-instance-1").await?;
if health.healthy {
    println!("Storage node is healthy");
} else {
    println!("Storage node is unhealthy: {}", health.error);
}
```

## Benefits Over Current Approach

### Current Approach (Database Only)
- ❌ No P2P networking
- ❌ No API server
- ❌ Limited features
- ❌ Manual management required

### Enhanced Approach (Full StorageNode)
- ✅ Complete feature set
- ✅ Automatic service discovery
- ✅ Lifecycle management
- ✅ Health monitoring
- ✅ Load balancing
- ✅ Resource isolation

## Migration Path

### Step 1: Update Orchestrator

Enhance `deploy_native_storage_nodes` to create full `StorageNode` instances.

### Step 2: Update Service Mesh

Add `register_storage_node_service` method for full StorageNode registration.

### Step 3: Update Examples

Update all examples to use full StorageNode instead of just Database.

### Step 4: Testing

Test deployment, lifecycle management, and service discovery.

## Next Steps

1. **Implement Enhanced Deployment**: Update orchestrator to deploy full StorageNode
2. **Service Mesh Integration**: Register all StorageNode endpoints
3. **Lifecycle Management**: Add start/stop/restart methods
4. **Health Monitoring**: Enhanced health checks for StorageNode
5. **Documentation**: Update all examples and guides

This architecture provides the best of both worlds: full StorageNode features with orchestrated management.

