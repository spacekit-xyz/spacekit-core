# SpaceKit CLI - API Integration Analysis

## Executive Summary

This document analyzes the current state of API integration between the **spacekit-cli** and the three major components:
- **spacekit-simulator** - Network simulation and gRPC services
- **spacekit-compute-node** - Distributed computing infrastructure
- **spacekit-storage-node** - Decentralized storage services

**Status:** ⚠️ **PARTIAL INTEGRATION** - The CLI has basic functionality but is missing ~60% of advanced features.

---

## 1. Current Integration Status

### ✅ What's Currently Integrated

#### Compute Node Integration
- ✅ Basic task submission (`task submit`)
- ✅ Task status checking (`task status`)
- ✅ Task listing (`task list`)
- ✅ Task cancellation (`task cancel`)
- ✅ Task result retrieval (`task result`)
- ✅ Task watching (`task watch`)

#### Storage Node Integration
- ✅ File storage (`storage store`)
- ✅ File retrieval (`storage retrieve`)
- ✅ File listing (`storage list`)
- ✅ File sharing (`storage share`)
- ✅ Access revocation (`storage revoke`)
- ✅ Storage statistics (`storage stats`)
- ✅ Storage node operations (`storage node`)

#### DID Integration (via primitives)
- ✅ DID creation (`did create`)
- ✅ DID verification (`did verify`)
- ✅ DID updates (`did update`)
- ✅ DID resolution (`did resolve`)
- ✅ Credential issuance (`did issue`)
- ✅ Credential verification (`did verify-credential`)

#### Network Operations (Basic)
- ✅ Network status (`network status`)
- ✅ Service discovery (`network discover`)
- ✅ Peer listing (`network peers`)
- ✅ Reputation checking (`network reputation`)

#### Consensus Operations (Basic)
- ✅ Proposal submission (`consensus submit-proposal`)
- ✅ Voting (`consensus vote`)
- ✅ Status checking (`consensus status`)
- ✅ Proposal listing (`consensus list`)

---

## 2. Missing Simulator Integration

### ❌ No Simulator Dependency
**Critical Issue:** The CLI does not have `swtchx-simulator` as a dependency in `Cargo.toml`.

### Missing Simulator Features

#### 🚫 Orchestration Commands (WASM Deployments)
**Simulator APIs:**
```rust
// From swtchx-simulator/src/orchestration.rs
pub struct SwtchWasmOrchestrator {
    pub async fn deploy_nodes(request: NodeDeploymentRequest) -> Result<NodeDeploymentResponse>
    pub async fn list_deployments() -> Result<Vec<DeploymentInfo>>
    pub async fn scale_deployment(id: &str, replicas: usize) -> Result<()>
    pub async fn terminate_deployment(id: &str) -> Result<()>
    pub wasm_registry: WasmRegistry
}
```

**Missing CLI Commands:**
```bash
# MISSING: Deploy compute/storage nodes via WASM
swtch orchestration deploy --type compute --replicas 3 --gpu-enabled
swtch orchestration list
swtch orchestration scale deployment_123 --replicas 5
swtch orchestration terminate deployment_123

# MISSING: WASM package management
swtch wasm-registry upload --package compute-node.wasm
swtch wasm-registry list
swtch wasm-registry download swtchx-compute-v1.0.0
```

#### 🚫 VPN Service Commands
**Simulator APIs:**
```rust
// From swtchx-simulator/src/vpn_service.rs
pub struct VpnServiceManager {
    pub async fn establish_vpn_connection(request: VpnConnectionRequest) -> Result<VpnConnection>
    pub async fn get_vpn_status(connection_id: &str) -> Result<VpnStatus>
    pub async fn list_vpn_connections() -> Result<Vec<VpnConnectionInfo>>
    pub async fn terminate_vpn_connection(connection_id: &str) -> Result<()>
    pub async fn list_relay_nodes() -> Result<Vec<RelayNode>>
}

// From swtchx-simulator/src/vpn_tunneling.rs
pub struct CompleteVPNConnection {
    pub async fn establish() -> Result<()>
    pub async fn send_data(data: &[u8]) -> Result<()>
    pub async fn receive_data() -> Result<Vec<u8>>
}
```

**Missing CLI Commands:**
```bash
# MISSING: VPN connection management
swtch vpn establish --target-did did:swtch:user:alice --relay-chain onion
swtch vpn status vpn_conn_123
swtch vpn list
swtch vpn terminate vpn_conn_123

# MISSING: VPN relay management
swtch vpn relays list
swtch vpn relays add --endpoint 192.168.1.100:8080
```

#### 🚫 Cross-Network & Topology Commands
**Simulator APIs:**
```rust
// From swtchx-simulator/src/cross_network/bridge.rs
pub struct CrossNetworkBridge {
    pub async fn connect(config: CrossNetworkConfig) -> Result<()>
    pub async fn transmit_data(data: &[u8], options: TransmissionOptions) -> Result<TransmissionResult>
    pub async fn get_network_health() -> Result<NetworkHealth>
}

// From swtchx-simulator/src/topologies/
pub struct HubSpokeTopology {
    pub async fn configure_as_hub(config: HubConfig) -> Result<()>
    pub async fn add_spoke(spoke_id: &str) -> Result<()>
}

pub struct MeshTopology {
    pub async fn join_mesh(config: MeshConfig) -> Result<()>
    pub async fn get_mesh_status() -> Result<TopologyStatus>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Cross-network connectivity
swtch cross-network connect --peer 192.168.1.50:50051 --secure-channel
swtch cross-network status
swtch cross-network health

# MISSING: Hub-spoke topology
swtch topology hub configure --listen-port 7000
swtch topology spoke join --hub-address 192.168.1.1:7000

# MISSING: Mesh topology
swtch topology mesh join --peers node1.swtch,node2.swtch
swtch topology mesh status
```

#### 🚫 Blockchain Scanner & Faucet Commands
**Simulator APIs:**
```rust
// From swtchx-simulator/src/blockchain_scanner.rs
pub struct BlockchainScanner {
    pub async fn scan_block(block_number: u64) -> Result<BlockData>
    pub async fn scan_address(address: &str) -> Result<AddressData>
    pub async fn subscribe_events(filter: EventFilter) -> Result<EventStream>
}

// From swtchx-simulator/src/faucet_service.rs
pub struct FaucetService {
    pub async fn request_tokens(did: &str, amount: u64) -> Result<FaucetResponse>
    pub async fn get_faucet_balance() -> Result<u64>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Blockchain scanner
swtch scanner scan-block 12345678
swtch scanner scan-address 0xABC123...
swtch scanner subscribe --event-type Transfer

# MISSING: Faucet service
swtch faucet request --did did:swtch:user:alice --amount 100
swtch faucet balance
```

#### 🚫 ML Service Commands
**Simulator APIs:**
```rust
// From swtchx-simulator/src/ml_service.rs
pub struct MLService {
    pub async fn infer(model: &str, input: Vec<f32>) -> Result<Vec<f32>>
    pub async fn list_models() -> Result<Vec<ModelInfo>>
    pub async fn load_model(model_id: &str) -> Result<()>
}
```

**Missing CLI Commands:**
```bash
# MISSING: ML inference
swtch ml infer --model sentiment-analysis --input "This is great!"
swtch ml list-models
swtch ml load-model llama-3.2-1b
```

---

## 3. Missing Compute Node Integration

### Advanced Compute Features Not Exposed

#### 🚫 Collaborative Compute Commands
**Compute Node APIs:**
```rust
// From swtchx-compute-node/src/collaborative_compute.rs
pub struct CollaborativeComputeManager {
    pub async fn create_collaboration(request: CollaborativeComputeRequest) -> Result<String>
    pub async fn join_collaboration(computation_id: &str, participant_did: &str) -> Result<()>
    pub async fn submit_partial_result(computation_id: &str, result: Vec<u8>) -> Result<()>
    pub async fn get_collaboration_status(computation_id: &str) -> Result<CollaborationStatus>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Collaborative compute
swtch collab create --type federated-learning --participants did:alice,did:bob
swtch collab join computation_123 --did did:swtch:user:charlie
swtch collab submit computation_123 --result results.bin
swtch collab status computation_123
```

#### 🚫 Secure Multi-Party Computation (SMPC)
**Compute Node APIs:**
```rust
// From swtchx-compute-node/src/secure_multiparty.rs
pub struct SecureMultiPartyManager {
    pub async fn create_smpc_session(config: SMPCConfig) -> Result<String>
    pub async fn submit_secret_share(session_id: &str, share: SecretContribution) -> Result<()>
    pub async fn compute_smpc(session_id: &str, computation_type: SMPCComputationType) -> Result<SMPCResult>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Secure multi-party computation
swtch smpc create --participants did:alice,did:bob,did:charlie --threshold 2
swtch smpc submit session_123 --share my_secret_share.bin
swtch smpc compute session_123 --type sum
swtch smpc status session_123
```

#### 🚫 P2P Service Discovery
**Compute Node APIs:**
```rust
// From swtchx-compute-node/src/p2p_service_discovery.rs
pub struct P2PServiceDiscoveryManager {
    pub async fn register_service(service: RegisteredService) -> Result<String>
    pub async fn discover_services(service_type: ServiceType) -> Result<Vec<RegisteredService>>
    pub async fn negotiate_capability(service_id: &str, requirements: ServiceRequirements) -> Result<()>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Advanced P2P service discovery
swtch p2p register --type compute --capabilities gpu,cpu --endpoint 0.0.0.0:8080
swtch p2p discover --type storage --min-capacity 100GB
swtch p2p negotiate service_123 --requirements '{"gpu": true}'
```

#### 🚫 LayerZero Cross-Chain Bridge
**Compute Node APIs:**
```rust
// From swtchx-compute-node/src/layerzero_bridge.rs
pub struct LayerZeroBridgeManager {
    pub async fn bridge_tokens(transfer: CrossChainTokenTransfer) -> Result<BridgeTransaction>
    pub async fn execute_cross_chain_task(task: CrossChainTaskExecution) -> Result<BridgeResult>
    pub async fn get_bridge_status(tx_id: &str) -> Result<BridgeStatus>
}
```

**Missing CLI Commands:**
```bash
# MISSING: LayerZero bridge
swtch bridge transfer --from ethereum --to polygon --amount 100 --token SWTCHX
swtch bridge execute-task --chain ethereum --contract 0xABC... --method compute
swtch bridge status bridge_tx_123
```

#### 🚫 Production Metrics & Monitoring
**Compute Node APIs:**
```rust
// From swtchx-compute-node/src/production_metrics.rs
pub struct ProductionMetricsManager {
    pub async fn collect_metrics() -> Result<ProductionMetricsSummary>
    pub async fn export_prometheus() -> Result<String>
    pub async fn get_network_stats() -> Result<NetworkStatistics>
    pub async fn analyze_performance() -> Result<PerformanceAnalytics>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Advanced metrics
swtch metrics collect --format json
swtch metrics export --format prometheus
swtch metrics network-stats
swtch metrics analyze-performance
```

#### 🚫 Metrics Consensus (Security Critical)
**Compute Node APIs:**
```rust
// From swtchx-compute-node/src/metrics_consensus.rs
pub struct MetricsConsensusManager {
    pub async fn attest_metrics(metrics: NodeMetrics) -> Result<MetricsAttestation>
    pub async fn validate_cross_node(attestations: Vec<MetricsAttestation>) -> Result<ConsensusMetrics>
    pub async fn detect_manipulation(metrics: Vec<NodeMetrics>) -> Result<ManipulationDetectionResult>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Metrics consensus (critical for security)
swtch metrics-consensus attest --metrics node_metrics.json
swtch metrics-consensus validate --attestations attestations.json
swtch metrics-consensus detect-fraud --metrics network_metrics.json
```

#### 🚫 Unified Consensus System
**Compute Node APIs:**
```rust
// From swtchx-compute-node/src/swtch_consensus.rs
pub struct UnifiedSWTCHConsensus {
    pub async fn submit_block_proposal(proposal: BlockProposal) -> Result<String>
    pub async fn submit_metrics_proposal(proposal: MetricsProposal) -> Result<String>
    pub async fn vote_on_proposal(proposal_id: &str, vote: Vote) -> Result<()>
    pub async fn get_consensus_status() -> Result<ConsensusStatus>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Unified consensus (advanced)
swtch unified-consensus submit-block --data block.json
swtch unified-consensus submit-metrics --data metrics.json
swtch unified-consensus vote proposal_123 --vote approve
swtch unified-consensus status
```

---

## 4. Missing Storage Node Integration

### Advanced Storage Features Not Exposed

#### 🚫 NFT Storage & Collection Management
**Storage Node APIs:**
```rust
// From swtchx-storage-node/src/nft_storage.rs
pub struct NftStorageManager {
    pub async fn create_nft(metadata: NftMetadata) -> Result<String>
    pub async fn query_nfts(query: NftQuery) -> Result<Vec<NftMetadata>>
    pub async fn transfer_nft(nft_id: &str, to_did: &str) -> Result<NftTransfer>
}

// From swtchx-storage-node/src/nft_collection.rs
pub struct NftCollectionManager {
    pub async fn create_collection(collection: NftCollection) -> Result<String>
    pub async fn mint_to_collection(collection_id: &str, nft: NftMetadata) -> Result<String>
    pub async fn get_collection_stats(collection_id: &str) -> Result<CollectionStats>
}
```

**Missing CLI Commands:**
```bash
# MISSING: NFT storage
swtch nft create --name "My NFT" --image ipfs://... --metadata metadata.json
swtch nft query --owner did:swtch:user:alice
swtch nft transfer nft_123 --to did:swtch:user:bob

# MISSING: NFT collections
swtch nft-collection create --name "My Collection" --symbol MYC --royalty 5%
swtch nft-collection mint collection_123 --metadata nft.json
swtch nft-collection stats collection_123
```

#### 🚫 Fact Package Storage
**Storage Node APIs:**
```rust
// From swtchx-storage-node/src/fact_storage.rs
pub struct FactStorageEngine {
    pub async fn store_fact(content: Vec<u8>, metadata: FactMetadata) -> Result<String>
    pub async fn retrieve_fact(fact_id: &str) -> Result<Vec<u8>>
    pub async fn query_facts(index: FactIndex) -> Result<Vec<String>>
    pub async fn compress_fact(fact_id: &str, algorithm: CompressionAlgorithm) -> Result<()>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Fact package storage
swtch fact store --content data.bin --compression zstd --tier hot
swtch fact retrieve fact_123
swtch fact query --index content-type:image
swtch fact compress fact_123 --algorithm brotli
```

#### 🚫 SQL Query Interface
**Storage Node APIs:**
```rust
// From swtchx-storage-node/src/sql_query.rs
pub struct StorageQueryBuilder {
    pub fn query_files() -> FileQuery
    pub fn query_facts() -> FactQuery
    pub fn query_users() -> UserQuery
    pub fn aggregate(function: AggregateFunction) -> AggregateQuery
}
```

**Missing CLI Commands:**
```bash
# MISSING: SQL-like queries
swtch query files --filter 'size > 1MB' --sort-by created_at --limit 10
swtch query facts --filter 'tier = hot' --aggregate count
swtch query users --filter 'created_at > 2025-01-01'
```

#### 🚫 Storage Rewards System
**Storage Node APIs:**
```rust
// From swtchx-storage-node/src/rewards.rs
pub struct StorageRewardCalculator {
    pub fn calculate_rewards(storage_provided: u64, uptime: f64) -> Result<RewardCalculation>
    pub fn get_reward_history(node_did: &str) -> Result<Vec<RewardRecord>>
    pub fn get_reward_analytics() -> Result<RewardAnalytics>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Storage rewards
swtch storage-rewards calculate --storage 100GB --uptime 99.9
swtch storage-rewards history --did did:swtch:node:storage1
swtch storage-rewards analytics
```

#### 🚫 Database Encryption & Advanced Features
**Storage Node APIs:**
```rust
// From swtchx-storage-node/src/quantum.rs
pub struct QuantumCrypto {
    pub async fn encrypt_data(data: &[u8], algorithm: &str) -> Result<EncryptedData>
    pub async fn decrypt_data(encrypted: &EncryptedData) -> Result<Vec<u8>>
}
```

**Missing CLI Commands:**
```bash
# MISSING: Advanced storage encryption
swtch storage encrypt --file sensitive.db --algorithm kyber1024
swtch storage decrypt encrypted_db.enc --output decrypted.db
```

---

## 5. Recommended Actions

### Priority 1: Critical Missing Features (Immediate)

1. **Add Simulator Dependency**
   ```toml
   # In swtchx-cli/Cargo.toml
   swtchx-simulator = { path = "../swtchx-simulator" }
   ```

2. **Add VPN Commands**
   - Essential for privacy and security features
   - High user demand expected

3. **Add Orchestration Commands**
   - Critical for node deployment and scaling
   - Required for production use

4. **Add Metrics Consensus Commands**
   - **SECURITY CRITICAL** for fraud detection
   - Prevents manipulation attacks

### Priority 2: High-Value Features (1-2 weeks)

5. **Add NFT Storage Commands**
   - High market demand
   - Competitive differentiator

6. **Add Collaborative Compute Commands**
   - Enables federated learning
   - Key for AI/ML workloads

7. **Add Cross-Network/Topology Commands**
   - Essential for multi-datacenter deployments
   - Required for scale

8. **Add LayerZero Bridge Commands**
   - Cross-chain interoperability
   - Expands ecosystem integration

### Priority 3: Nice-to-Have Features (Future)

9. **Add SMPC Commands**
   - Privacy-preserving computation
   - Advanced use cases

10. **Add Fact Storage Commands**
    - Specialized storage optimization
    - Content delivery features

11. **Add SQL Query Commands**
    - Advanced data analytics
    - Power user features

12. **Add ML Service Commands**
    - AI inference capabilities
    - Model management

---

## 6. Implementation Roadmap

### Phase 1: Foundation (Week 1)
- [ ] Add `swtchx-simulator` dependency
- [ ] Create `SimulatorCommands` enum
- [ ] Implement basic simulator connection/health check
- [ ] Add VPN commands (establish, status, list, terminate)
- [ ] Add orchestration commands (deploy, list, scale, terminate)

### Phase 2: Security & Metrics (Week 2)
- [ ] Add metrics consensus commands (attest, validate, detect-fraud)
- [ ] Add production metrics commands (collect, export, analyze)
- [ ] Add unified consensus commands
- [ ] Add blockchain scanner commands

### Phase 3: Storage Enhancement (Week 3)
- [ ] Add NFT storage commands (create, query, transfer)
- [ ] Add NFT collection commands (create, mint, stats)
- [ ] Add fact storage commands (store, retrieve, query, compress)
- [ ] Add storage rewards commands

### Phase 4: Advanced Compute (Week 4)
- [ ] Add collaborative compute commands (create, join, submit, status)
- [ ] Add SMPC commands (create, submit, compute, status)
- [ ] Add P2P service discovery commands (register, discover, negotiate)
- [ ] Add LayerZero bridge commands (transfer, execute-task, status)

### Phase 5: Network Topology (Week 5)
- [ ] Add cross-network commands (connect, status, health)
- [ ] Add hub-spoke topology commands (configure, add-spoke)
- [ ] Add mesh topology commands (join, status)
- [ ] Add faucet service commands (request, balance)

### Phase 6: Advanced Features (Week 6)
- [ ] Add SQL query commands (query files/facts/users, aggregate)
- [ ] Add ML service commands (infer, list-models, load-model)
- [ ] Add advanced storage encryption commands
- [ ] Polish and documentation

---

## 7. Testing Requirements

### Integration Tests Needed
- [ ] Simulator gRPC connectivity
- [ ] VPN connection lifecycle
- [ ] Orchestration deployment/scaling
- [ ] NFT creation and transfer
- [ ] Collaborative compute workflow
- [ ] Cross-chain bridge operations
- [ ] Metrics consensus validation
- [ ] Cross-network communication

### End-to-End Scenarios
- [ ] Deploy compute node → Submit task → Monitor → Get results
- [ ] Create NFT collection → Mint NFTs → Transfer → Query
- [ ] Establish VPN → Route traffic → Monitor → Terminate
- [ ] Create SMPC session → Submit shares → Compute → Get result

---

## 8. Documentation Updates

### README Updates Required
- [ ] Add VPN commands section
- [ ] Add orchestration commands section
- [ ] Add NFT storage examples
- [ ] Add collaborative compute examples
- [ ] Add cross-network topology examples
- [ ] Update feature comparison table

### New Documentation Files Needed
- [ ] `VPN_GUIDE.md` - Complete VPN usage guide
- [ ] `ORCHESTRATION_GUIDE.md` - Node deployment guide
- [ ] `NFT_STORAGE_GUIDE.md` - NFT creation and management
- [ ] `COLLABORATIVE_COMPUTE_GUIDE.md` - Multi-party computation
- [ ] `CROSS_NETWORK_GUIDE.md` - Multi-datacenter setup

---

## 9. Dependency Graph

```
swtchx-cli
├── swtchx-primitives ✅ (already integrated)
├── swtchx-did ✅ (already integrated)
├── swtchx-compute-node ⚠️ (partially integrated)
├── swtchx-storage-node ⚠️ (partially integrated)
└── swtchx-simulator ❌ (NOT integrated) ← CRITICAL MISSING
```

### Transitive Dependencies (via Simulator)
When adding `swtchx-simulator`, we automatically get access to:
- `swtchx-messaging-node` (via simulator)
- `swtchx-recovery` (via simulator)
- VPN tunneling infrastructure
- Cross-network bridge
- Orchestration system
- Blockchain scanner
- Faucet service
- ML service

---

## 10. Conclusion

**Current State:** The CLI has ~40% of the total available features integrated.

**Missing Critical Features:**
1. **Entire Simulator Integration** (VPN, orchestration, cross-network)
2. **60% of Compute Node Features** (collaborative compute, SMPC, metrics consensus)
3. **70% of Storage Node Features** (NFT storage, fact storage, SQL queries)

**Recommendation:** Prioritize Phases 1-2 (Foundation + Security) for immediate implementation. These represent the highest-value, most user-visible features that are currently missing.

**Estimated Effort:** 6 weeks for complete integration of all missing features.

**ROI:** Very high - unlocks entire ecosystem capabilities and competitive positioning.

