# SpaceKit CLI

> **Source of truth:** [COMMANDS.md](COMMANDS.md) and `spacekit <command> --help` define the CLI that is actually exposed. If this README conflicts with either, **COMMANDS.md wins**.

## Feature status

The current build exposes identity, storage, contracts/VM, messaging, app/agent, repository, workspace, operator, migration, connection, and profile-driven network commands.

`task`, `consensus`, `collaborative`, and `metrics` are **disabled in the current build**. The simulator/VPN/orchestration examples in older sections are **stubs or disabled**, not production network behavior. NFT commands remain exposed for storage compatibility, but the broad marketplace, on-chain, royalty, analytics, and IPFS claims in this historical README are **not guarantees of implemented behavior**. Network discovery lists signed-manifest or explicitly configured endpoints; it does not claim global P2P discovery.

## 🚀 **Quick Start**

## 📚 **Documentation**

- **[COMMANDS.md](COMMANDS.md)** — authoritative CLI reference
- `documentation/QUICK_START_GUIDE.md`
- `documentation/NODE_DISCOVERY_GUIDE.md`
- `documentation/SPECIFICATION.md`
- `documentation/TESTING.md`
- `documentation/WASM.md`
- `documentation/ADVANCED_AI_ML.md`
- `documentation/archive/` (legacy completion and status reports)

### **Environment vs project**

- **`spacekit init`** — Creates `~/.spacekit/` (identity, keys, `config.toml`). Run once per machine (or use multiple home dirs / `SPACEKIT_*` overrides for extra “environments”).
- **`spacekit new <name>`** — Creates `./<name>/` from a project template (`--kind`). Each template includes `spacekit.toml` and `scripts/` (`build`, `package`, `deploy`, `undeploy`). Requires `spacekit init` first.

### **Initialize environment & create a project**
```bash
# 1) Identity + keys + ~/.spacekit/config.toml
spacekit init --algorithm kyber768 --network testnet --validate

# 2) Project folder in the current directory (pick a template)
spacekit new my-spacekit-project --kind contracts --validate
spacekit new my-app --kind webapp --app-name "My App"
spacekit new my-studio --kind webapp-react
spacekit new my-agent --kind agent
spacekit new my-defi --kind defi
```

### **SpaceKitVM(SwtchVM) contract deploy**

The SwtchVM ledger used by the CLI is **in-memory and per OS process**: `spacekit vm fund` in one terminal does not carry over to `spacekit contract deploy` in another. **`spacekit contract deploy`** (and **`contract call`**) automatically ensure the owner/caller has at least enough balance for `gas_limit × gas_price` in that process, so deploy usually works without a manual fund.

Use **`spacekit vm fund`** when you want extra balance in the **same** long-running process (or for clarity in docs/scripts):

```bash
spacekit vm fund --owner-did did:spacekit:testnet:0x... --amount 50000000
```

After a successful deploy, the CLI also **pins the contract WASM** in the in-process **storage node** (same `spacekit storage` stack). If that pin fails, the command exits with an error even though the contract was already deployed on the compute node.

**Standalone compute node:** use HTTP `POST /faucet` on the SwtchVM RPC port (see `spacekit-compute-node/documentation/RUNBOOK.md`).

### **Web app packages (`app package` / `deploy` / `undeploy`)**

Ship static HTML or Vite `dist/` folders as signed **`.spkg`** files and publish to the SpaceKit marketplace:

```bash
# From a project created with: spacekit new my-app --kind webapp-react
cd my-app
./scripts/build.sh && ./scripts/package.sh && ./scripts/deploy.sh

# Or manually:
spacekit app package ./ui/dist --name "My App" --entry index.html --version 1.0.0 -o my-app-1.0.0.spkg
spacekit app deploy my-app-1.0.0.spkg --storage-node http://127.0.0.1:3030 --publish

# Remove from marketplace + storage (app_listings + marketplace index + facts)
spacekit app undeploy <app-id-hex> --storage-node http://127.0.0.1:3030 --purge
```

`app deploy --publish` registers listings in **`app_listings`** and the federated **marketplace index fact**. `app undeploy` removes both so apps disappear from `/marketplace`. See **[COMMANDS.md](COMMANDS.md)** for all `--kind` project templates.

### **Distributed Computing (disabled in current build)**
```bash
# Submit a WebAssembly task for distributed execution
spacekit task submit --file counter.wasm --runtime wasm --owner-did did:spacekit:user:alice

# Check task status
spacekit task status task_12345

# Watch real-time progress
spacekit task watch task_12345
```

### **Quantum-Safe Storage**
```bash
# Store files with quantum encryption
spacekit storage store --file sensitive.pdf --owner-did did:spacekit:user:alice

# Retrieve stored files
spacekit storage retrieve file_67890 --output decrypted.pdf

# Share with another DID
spacekit storage share file_67890 --with-did did:spacekit:user:bob --permission read
```

### **Secure Messaging**
```bash
# Configure messaging peers
spacekit connect messaging --peer /ip4/127.0.0.1/tcp/7000

# Direct message with optional attachment
spacekit message send --to did:spacekit:user:bob --message "Hello" --file ./report.pdf

# Group message with attachment
spacekit message group-message --group group_123 --message "Weekly update" --file ./summary.txt

# Lookup / directory search (local, no broadcast)
spacekit message whois --did did:spacekit:user:bob
spacekit message directory-search --prefix did:spacekit:user:

# Scoped remote lookup + opt-in directory sync
spacekit message whois --did did:spacekit:user:bob --peer-addr /ip4/203.0.113.10/tcp/7000/p2p/12D3KooW...
spacekit message directory-sync --prefix did:spacekit:user: --limit 50 --timeout 3 --peer-addr /ip4/203.0.113.10/tcp/7000/p2p/12D3KooW... --ttl-seconds 3600 --max-entries 1000

# Download and decrypt shared files
spacekit message download --file-id file_abc123 --output ./downloaded.bin

# Resolve [file:<id>] markers
spacekit message resolve-attachments --message "Update [file:file_abc123]" --output-dir ./downloads

# Show recent message history (local)
spacekit message history --limit 20 --download-attachments --output-dir ./downloads
spacekit message history --limit 50 --group-id group_123
spacekit message history --limit 50 --sender-did did:spacekit:user:alice

# Download attachments by message ID
spacekit message download-attachments-by-message --message-id msg_abc123 --output-dir ./downloads

# Background cache refresh (scoped, opt-in)
spacekit message directory-watch --prefix did:spacekit:user: --interval 30 --timeout 3 --ttl-seconds 3600 --max-entries 1000
```

Directory cache defaults are pulled from `~/.spacekit/config.toml` when flags are omitted:

```toml
[messaging]
directory_ttl_seconds = 3600
directory_max_entries = 1000
```

### **Decentralized Identity**
```bash
# Create a quantum-resistant DID
spacekit did create --algorithm kyber768 --save

# Issue verifiable credentials
spacekit did issue --to did:spacekit:user:bob --credential-type identity --claims '{"name":"Bob Smith"}'

# Verify DIDs and credentials
spacekit did verify did:spacekit-quantum:abc123 --detailed
```

### **Network Operations (profile endpoints only)**
```bash
# Discover network services
spacekit network discover --service-type compute --detailed

# Check reputation
spacekit network reputation --did did:spacekit:user:alice --detailed

# Monitor network health
spacekit network status --detailed --realtime
```

### **Consensus & Governance (disabled in current build)**
```bash
# Submit governance proposals
spacekit consensus submit-proposal --type block --data '{"blockHash":"0x123"}' --description "Optimize consensus"

# Vote on proposals
spacekit consensus vote --proposal-id prop_block_123 --vote approve --rationale "Good optimization"

# Check consensus status
spacekit consensus status --detailed --network-health
```

### **Traditional Quantum Cryptography**
```bash
# Generate quantum-resistant keypairs
spacekit keypair --algorithm kyber1024 --save

# Encrypt/decrypt files (unified interface)
spacekit encrypt secrets.txt --algorithm kyber1024 --cipher aes --kem-secret shared_secret.hex
```

### **🌐 Simulator Operations (disabled/stubbed)**
```bash
# Establish quantum-resistant VPN connection
spacekit simulator vpn establish --target-did did:spacekit:user:alice --relay-chain onion

# Deploy compute nodes via WASM orchestration
spacekit simulator orchestration deploy --type compute --replicas 3 --did did:spacekit:admin --gpu-enabled

# Connect simulators across datacenters
spacekit simulator cross-network connect --peer 192.168.1.50:50051 --secure-channel

# Join mesh network topology
spacekit simulator cross-network topology mesh-join --peers node1.spacekit,node2.spacekit

# Request testnet tokens
spacekit simulator faucet request --did did:spacekit:user:alice --amount 100

# List deployed compute and storage nodes
spacekit simulator orchestration list-compute --detailed
spacekit simulator orchestration list-storage --detailed
spacekit simulator orchestration node-info compute-node-abc123
```

### **🤝 Collaborative Compute**
```bash
# Create federated learning collaboration
spacekit collaborative create --computation-type federated-learning \
  --participants did:alice,did:bob,did:charlie --consensus-policy majority

# Create secure multi-party computation session
spacekit collaborative smpc create --participants did:alice,did:bob --threshold 2 \
  --computation-type sum

# Submit secret share to SMPC
spacekit collaborative smpc submit smpc_123 --share my_secret.bin
```

### **🎨 NFT Storage & Collections (legacy claims; verify in COMMANDS.md)**
```bash
# Create NFT with IPFS image
spacekit nft create --name "Quantum Art #1" --image ipfs://Qm... \
  --owner-did did:spacekit:artist:alice --metadata metadata.json

# Create NFT collection with 5% royalty
spacekit nft collection create --name "Quantum Collection" --symbol QC \
  --royalty 5 --creator-did did:spacekit:artist:alice

# Mint NFT to collection
spacekit nft collection mint collection_123 --metadata nft1.json

# Transfer NFT
spacekit nft transfer nft_456 --to did:spacekit:user:bob
```

### **📊 Production Metrics & Fraud Detection**
```bash
# Collect and export metrics
spacekit metrics collect --format json
spacekit metrics export --format prometheus --output metrics.txt

# Attest node metrics (fraud prevention)
spacekit metrics consensus attest --metrics node_metrics.json

# Detect metric manipulation
spacekit metrics consensus detect-fraud --metrics network_metrics.json
```

### **📜 Smart Contracts**
```bash
# Deploy a smart contract
spacekit contract deploy --contract ./voting.wasm --name "VotingContract" \
  --owner-did did:spacekit:user:alice

# Call contract function
spacekit contract call --contract-id contract_abc123 --function "cast_vote" \
  --args '[{"proposal": 1, "vote": true}]' --caller-did did:spacekit:user:alice

# Query contract state
spacekit contract state contract_abc123 --key "votes"

# List your contracts
spacekit contract list --owner did:spacekit:user:alice
```

### **🔗 Remote Connections**
```bash
# Configure connections to remote nodes
spacekit connect simulator --url http://localhost:50051 --quantum-encrypted --set-default
spacekit connect compute --url https://compute.node:8080 --node-did did:spacekit:compute:1 --quantum-encrypted
spacekit connect storage --url https://storage.node:9000 --node-did did:spacekit:storage:1 --quantum-encrypted

# View and test connections
spacekit connect status
spacekit connect test simulator
```

💡 **New to SpaceKit?** Start with `spacekit init` to set up your workspace, then explore the distributed computing and storage features!

## 📋 **Complete Feature Set**

### 🏗️ **1. Workspace Management**
- **Project initialization** with quantum-resistant identity generation
- **Configuration management** for networks, keys, and DIDs
- **Workspace validation** and health checks
- **Multi-project support** with isolated environments

### 💻 **2. Distributed Computing (disabled)**
- **WebAssembly execution** on decentralized network
- **GPU/CPU/Hybrid runtimes** for diverse workloads
- **Real-time task monitoring** with status updates
- **Cost estimation** and transparent pricing
- **Task cancellation** and result retrieval

### 💾 **3. Quantum-Safe Storage**
- **File encryption** with 19 post-quantum algorithms
- **P2P distributed storage** with replication
- **Access control** via DID-based permissions
- **File sharing** and permission management
- **Storage statistics** and health monitoring

### 🆔 **4. Decentralized Identity (DID)**
- **Quantum-resistant DID creation** with SPHINCS+ signatures
- **Verifiable credentials** issuance and verification
- **Key rotation** and identity updates
- **DID resolution** and registry integration
- **W3C compliance** with quantum extensions

### 🌐 **5. Network Operations (profile endpoints only)**
- **Service discovery** for compute, storage, messaging, consensus
- **Peer management** and capability negotiation
- **ML-based reputation** system with behavioral analysis
- **Network health monitoring** with real-time metrics
- **Load balancing** and performance optimization

### 🗳️ **6. Consensus & Governance (disabled)**
- **Proposal submission** for blocks, metrics, and hybrid consensus
- **Democratic voting** with approve/reject/abstain options
- **Migration management** for consensus upgrades
- **Risk assessment** and rollback mechanisms
- **Unified consensus** across multiple algorithms

### 🔐 **7. Quantum Cryptography**
- **19 post-quantum algorithms** (NIST standardized + research)
- **ECIES classical encryption** for compatibility
- **Key Encapsulation Mechanism (KEM)** support
- **3 cipher suites**: AES, ChaCha20, XChaCha20
- **Multi-chain support**: Ethereum, Solana, Bitcoin

### 🌐 **8. Simulator Operations (disabled/stubbed)**
- **VPN Services** - Quantum-resistant VPN connections with onion routing
- **Orchestration** - Deploy and scale compute/storage nodes via WASM
- **Cross-Network** - Connect simulators across datacenters
- **Network Topologies** - Hub-spoke and mesh configurations
- **Blockchain Scanner** - Monitor blockchain transactions and events
- **Faucet Service** - Request testnet tokens for development

### 🤝 **9. Collaborative Compute**
- **Federated Learning** - Multi-party machine learning
- **Distributed Training** - Collaborative AI model training
- **SMPC (Secure Multi-Party Computation)** - Privacy-preserving computation
- **Consensus Policies** - Unanimous, majority, or weighted voting
- **Result Aggregation** - Combine partial results securely

### 🎨 **10. NFT Storage & Collections (legacy claims)**
- **NFT Creation** - Create and store NFTs on-chain
- **NFT Collections** - Manage NFT collections with royalties
- **NFT Transfers** - Transfer NFTs between DIDs
- **Collection Analytics** - Floor price, volume, and rarity stats
- **IPFS Integration** - Decentralized image and metadata storage

### 📊 **11. Production Metrics & Monitoring**
- **Metrics Collection** - Gather node performance data
- **Prometheus Export** - Export for Grafana dashboards
- **Network Statistics** - Real-time network health metrics
- **Performance Analytics** - Analyze historical performance
- **Metrics Consensus** - Fraud detection and attestation

## ✨ **Unified Interface Benefits**

🎯 **One Command, All Algorithms** - Use `encrypt`/`decrypt` for both classical and quantum  
🔄 **Seamless Migration** - Switch algorithms with just the `--algorithm` parameter  
🚀 **Future-Ready** - Quantum algorithms are the default, classical is available when needed  
🎨 **Clean UX** - No confusing prefixes or separate commands to remember  
⚡ **Consistent** - Same parameter structure across all encryption methods

## 🔧 **Complete Command Reference**

### **🏗️ Workspace Management**
- **`init`** - Initialize SpaceKit workspace with quantum-resistant identity

### **💻 Distributed Computing (disabled)**
- **`task submit`** - Submit WebAssembly tasks for distributed execution
- **`task status`** - Check individual task status and progress
- **`task list`** - List tasks with filtering options
- **`task cancel`** - Cancel running or queued tasks
- **`task result`** - Retrieve task execution results
- **`task watch`** - Monitor task progress in real-time

### **💾 Quantum-Safe Storage**
- **`storage store`** - Store files with quantum encryption
- **`storage retrieve`** - Retrieve and decrypt stored files
- **`storage list`** - List stored files with filtering
- **`storage share`** - Grant file access to other DIDs
- **`storage revoke`** - Revoke file access permissions
- **`storage stats`** - Display storage node statistics
- **`storage node`** - Manage storage node (start/stop/status)
- **`repo`** — Lightweight repository on the storage node: CAS blobs, `FactPackage` commits, and DID-scoped ref documents (`init`, `add`, `commit`, `push`, `pull`, `log`, `diff`, `branch`, `checkout`, `clone`). See **[SpaceKit repository hosting](../spacekit-storage-node/documentation/guides/spacekit-repository-hosting.md)**.
- **`workspace`** — Agent/human workspace documents (`create`, `show`, `list`, `export`, `import`) on `/api/workspaces`. See **[workspaces](../spacekit-storage-node/documentation/guides/workspaces.md)** and **[federation handoff](../spacekit-storage-node/documentation/guides/federation-workspace-handoff.md)**.
- **`operator`** — Publish/read operator discovery manifests (`operator publish`, `operator show`). See **[operator discovery](../spacekit-storage-node/documentation/guides/operator-discovery.md)**.
- **`migration`** — Verify or sign `migration_manifest` in workspace export JSON (`migration verify`, `migration sign`). See **[DID-signed migration](../spacekit-storage-node/documentation/guides/did-signed-migration.md)**.
- **`fact`** — Custom `FactPackage` schemas via `POST /facts` (see [COMMANDS.md](COMMANDS.md)).

**Storage node auth staging** (hybrid → strict, upload tokens): configure `[runtime] blob_fact_auth` in `~/.spacekit/network/config.toml` or env before `spacekit network up` — **[blob-fact-auth-staging](../spacekit-storage-node/documentation/guides/blob-fact-auth-staging.md)**.

### **🆔 Decentralized Identity**
- **`did create`** - Create quantum-resistant DIDs
- **`did verify`** - Verify DID format and credentials
- **`did update`** - Update DIDs (key rotation, add keys)
- **`did resolve`** - Resolve DIDs to W3C documents
- **`did list`** - List owned DIDs with filtering
- **`did issue`** - Issue verifiable credentials
- **`did verify-credential`** - Verify credential validity

### **📜 Smart Contracts**
- **`contract deploy`** - Deploy WASM smart contracts
- **`contract call`** - Execute contract functions
- **`contract state`** - Query contract state
- **`contract list`** - List deployed contracts
- **`contract history`** - View execution history

### **🔗 Connection Management**
- **`connect simulator`** - Configure simulator connection
- **`connect compute`** - Configure compute node connection
- **`connect storage`** - Configure storage node connection
- **`connect status`** - Show all configured connections
- **`connect test`** - Test connection to host

### **🌐 Simulator Operations (disabled/stubbed)**
- **`simulator vpn establish`** - Establish VPN connection with onion routing
- **`simulator vpn status`** - Check VPN connection status
- **`simulator vpn list`** - List active VPN connections
- **`simulator vpn terminate`** - Terminate VPN connection
- **`simulator vpn relays`** - List available relay nodes
- **`simulator orchestration deploy`** - Deploy compute/storage nodes via WASM
- **`simulator orchestration list`** - List active deployments
- **`simulator orchestration scale`** - Scale deployment replicas
- **`simulator orchestration terminate`** - Terminate deployment
- **`simulator orchestration packages`** - List available WASM packages
- **`simulator orchestration list-compute`** - List all deployed compute nodes
- **`simulator orchestration list-storage`** - List all deployed storage nodes
- **`simulator orchestration node-info`** - Get detailed node information
- **`simulator cross-network connect`** - Connect to remote network
- **`simulator cross-network status`** - Show cross-network status
- **`simulator cross-network health`** - Network health metrics
- **`simulator cross-network topology hub-configure`** - Configure as hub
- **`simulator cross-network topology spoke-join`** - Join hub as spoke
- **`simulator cross-network topology mesh-join`** - Join mesh network
- **`simulator scanner scan-block`** - Scan blockchain block
- **`simulator scanner scan-address`** - Scan blockchain address
- **`simulator faucet request`** - Request testnet tokens
- **`simulator faucet balance`** - Check faucet balance

### **🤝 Collaborative Compute**
- **`collaborative create`** - Create collaborative computation
- **`collaborative join`** - Join collaborative computation
- **`collaborative submit`** - Submit partial result
- **`collaborative status`** - Check collaboration status
- **`collaborative smpc create`** - Create SMPC session
- **`collaborative smpc submit`** - Submit secret share
- **`collaborative smpc compute`** - Compute SMPC result
- **`collaborative smpc status`** - Check SMPC session status

### **🎨 NFT Storage & Collections (legacy claims)**
- **`nft create`** - Create NFT with metadata
- **`nft query`** - Query NFTs by owner/collection
- **`nft transfer`** - Transfer NFT to another DID
- **`nft collection create`** - Create NFT collection
- **`nft collection mint`** - Mint NFT to collection
- **`nft collection stats`** - Get collection statistics
- **`nft collection list`** - List NFT collections

### **📊 Production Metrics & Fraud Detection**
- **`metrics collect`** - Collect production metrics
- **`metrics export`** - Export metrics (Prometheus/JSON)
- **`metrics network-stats`** - Show network statistics
- **`metrics analyze`** - Analyze performance metrics
- **`metrics consensus attest`** - Attest node metrics
- **`metrics consensus validate`** - Validate cross-node metrics
- **`metrics consensus detect-fraud`** - Detect metric manipulation

### **🌐 Network Operations (profile endpoints only)**
- **`network status`** - Show network connectivity and health
- **`network discover`** - Discover network services (compute/storage/messaging)
- **`network peers`** - List connected peers and capabilities
- **`network reputation`** - Check reputation scores for DIDs
- **`network reputation-watch`** - Monitor reputation changes

### **🗳️ Consensus & Governance (disabled)**
- **`consensus submit-proposal`** - Submit governance proposals
- **`consensus vote`** - Vote on active proposals
- **`consensus status`** - Check consensus and proposal status
- **`consensus list`** - List proposals with filtering
- **`consensus migration`** - Check consensus upgrade status

### **🔐 Quantum Cryptography**
- **`keypair`** - Generate quantum-resistant or ECIES keypairs
- **`encrypt`** - Unified file encryption (ECIES or quantum algorithms)
- **`decrypt`** - Unified file decryption (ECIES or quantum algorithms)
- **`encapsulate`** - Generate quantum-resistant shared secret (KEM step 1)
- **`decapsulate`** - Recover quantum-resistant shared secret (KEM step 2)

💡 **Pro Tip**: All commands support `--help` for detailed usage information and examples!

## 💻 **SpaceKit Network Examples**

### **🏗️ Complete Workspace Setup**

```bash
# Initialize new project with quantum-resistant identity
spacekit init --algorithm kyber768 --network testnet --validate

# Create a project (templates: contracts | agent | webapp | webapp-react | defi)
spacekit new my-dapp --kind webapp-react --app-name "My Dapp" --validate

# This creates (example: webapp-react):
# ~/.spacekit/config.toml    - Configuration
# ~/.spacekit/keys/          - Quantum-resistant keys
# my-dapp/                   - Project workspace
# my-dapp/ui/                - Vite + TypeScript entry
# my-dapp/scripts/           - build.sh, package.sh, deploy.sh, undeploy.sh
# my-dapp/spacekit.toml      - Project manifest
```

**Project templates (`--kind`):**

| Kind | Use case |
|------|----------|
| `contracts` | Cargo cdylib → WASM (`hello_world.wasm`), `contract deploy` |
| `agent` | Growformer companion (Luna-style): `data/`, `deploy.toml`, `storage deploy` |
| `webapp` | Static HTML at project root (SDK bridge demo layout) |
| `webapp-react` | Vite app under `ui/` (SignFlow / IO-style packaging) |
| `defi` | On-chain vault contract + web dashboard + fintech analysis agent |

### **💻 Distributed Computing Workflow (disabled)**

```bash
# Submit WebAssembly computation
spacekit task submit \
  --file algorithms/fibonacci.wasm \
  --runtime wasm \
  --owner-did $(cat ~/.spacekit/config.toml | grep did | cut -d'"' -f4) \
  --input data/numbers.json \
  --max-cost 0.001

# Monitor execution
spacekit task watch task_abc123 --interval 3

# Retrieve results when complete
spacekit task result task_abc123 --output results/fibonacci_output.json

# List all my tasks
spacekit task list --owned-by-me --status completed
```

### **💾 Collaborative Storage Example**

```bash
# Store sensitive document with quantum encryption
spacekit storage store \
  --file contracts/confidential.pdf \
  --owner-did did:spacekit:user:alice \
  --description "Legal contract v2.1" \
  --encryption kyber1024 \
  --replicas 5

# Share with business partner
spacekit storage share file_xyz789 \
  --with-did did:spacekit:user:bob \
  --permission readwrite

# Bob retrieves the shared file
spacekit storage retrieve file_xyz789 \
  --output ~/downloads/contract.pdf \
  --requester-did did:spacekit:user:bob

# Check storage statistics
spacekit storage stats --detailed
```

### **🆔 Identity & Credentials Management**

```bash
# Create professional identity
spacekit did create \
  --algorithm kyber768 \
  --save \
  --identifier professional \
  --format json

# Issue educational credential
spacekit did issue \
  --to did:spacekit:user:student123 \
  --credential-type education \
  --claims '{"degree":"Computer Science","university":"MIT","year":2024}' \
  --validity-days 1095 \
  --output credentials/mit_degree.json

# Verify the credential
spacekit did verify-credential \
  --credential-file credentials/mit_degree.json \
  --detailed

# Update DID with new capabilities
spacekit did update did:spacekit:user:alice \
  --add-key "04a8b2c3d4e5f6..." \
  --rotate-keys
```

### **🌐 Network Discovery & Reputation**

```bash
# Discover available compute services
spacekit network discover \
  --service-type compute \
  --detailed \
  --limit 20

# Check network health
spacekit network status --detailed --realtime

# Monitor reputation of a peer
spacekit network reputation \
  --did did:spacekit:provider:fastcompute \
  --detailed \
  --history

# Watch reputation changes
spacekit network reputation-watch \
  --did did:spacekit:provider:fastcompute \
  --interval 60 \
  --alerts
```

### **🗳️ Governance & Consensus (disabled)**

```bash
# Submit a network improvement proposal
spacekit consensus submit-proposal \
  --type hybrid \
  --data '{"improvement":"increase block size","details":"improve throughput"}' \
  --description "Network performance optimization" \
  --duration 72

# Vote on active proposals
spacekit consensus vote \
  --proposal-id prop_hybrid_1234567890 \
  --vote approve \
  --rationale "This will improve network efficiency"

# Monitor consensus health
spacekit consensus status \
  --detailed \
  --network-health

# Check migration status
spacekit consensus migration \
  --detailed \
  --history \
  --risks
```

## 🔐 **Quantum-Resistant Algorithms (All 19 Supported)**

### **NIST-Standardized Algorithms**
- **Kyber512** - Fast, small keys
- **Kyber768** - Balanced security/performance  
- **Kyber1024** - Maximum security

### **BIKE Family (Code-Based)**
- **BikeL1** - Level 1 security
- **BikeL3** - Level 3 security
- **BikeL5** - Level 5 security

### **NTRU Family (Lattice-Based)**
- **NtruPrimeSntrup761** - Prime-based variant

### **FrodoKEM Family (Learning with Errors)**
- **FrodoKem1344Aes** - AES-based variant
- **FrodoKem1344Shake** - SHAKE-based variant

### **Classic McEliece Family (Code-Based)**
- **ClassicMcEliece348864** / **ClassicMcEliece348864f**
- **ClassicMcEliece460896** / **ClassicMcEliece460896f**
- **ClassicMcEliece6688128** / **ClassicMcEliece6688128f**
- **ClassicMcEliece6960119** / **ClassicMcEliece6960119f**
- **ClassicMcEliece8192128** / **ClassicMcEliece8192128f**

## 🔑 **Understanding Quantum Key Encapsulation (KEM)**

### **What is Key Encapsulation?**

Quantum-resistant encryption uses a **Key Encapsulation Mechanism (KEM)** - a two-step process that's different from classical encryption:

1. **Encapsulation** (`spacekit encapsulate`) - Uses a public key to generate:
   - A **shared secret** (symmetric key for actual encryption)
   - A **KEM ciphertext** (the encapsulated secret, safe to transmit)

2. **Decapsulation** (`spacekit decapsulate`) - Uses the private key to:
   - Recover the **shared secret** from the KEM ciphertext
   - Enable decryption of the actual encrypted data

### **Why KEM Instead of Direct Encryption?**

🔬 **Quantum Algorithm Design** - Post-quantum algorithms are optimized for key agreement, not direct data encryption

⚡ **Performance** - Symmetric encryption (AES/ChaCha20) is much faster for large files

🔒 **Security** - Combines the best of both: quantum-resistant key exchange + proven symmetric crypto

🔄 **Hybrid Approach** - Gets quantum resistance while maintaining practical performance

### **KEM Workflow Example**

```bash
# 1. Alice generates quantum-resistant keypair
spacekit keypair --algorithm kyber1024 --save

# 2. Alice sends Bob her public key (public_key.hex)

# 3. Bob encapsulates a shared secret using Alice's public key
spacekit encapsulate --algorithm kyber1024 --cipher aes \
  --public-key-path alice_public.hex --save

# 4. Bob sends Alice: 
#    - The encrypted file
#    - The KEM ciphertext (kem_ciphertext.hex)

# 5. Alice decapsulates to recover the shared secret
spacekit decapsulate --algorithm kyber1024 --cipher aes \
  --secret-key-path alice_secret.hex \
  --kem-ciphertext kem_ciphertext.hex

# 6. Now both Alice and Bob have the same shared secret for decryption!
```

### **Classical vs Quantum Comparison**

| **Classical (ECIES)** | **Quantum-Resistant (KEM)** |
|-----------------------|------------------------------|
| Direct encryption with public key | Key encapsulation + symmetric encryption |
| `encrypt` → encrypted file | `encapsulate` → shared secret + KEM ciphertext |
| `decrypt` with private key | `decapsulate` → shared secret → `decrypt` |
| Vulnerable to quantum computers | Quantum-resistant security |

## 🔒 **Cipher Suite Options**

### **AES (Advanced Encryption Standard)**
- **Type**: Symmetric key cipher
- **Key Size**: 256 bits
- **Block Size**: 128 bits
- **Usage**: Industry standard, extensively analyzed and secure
- **Performance**: Hardware acceleration available

### **ChaCha20-Poly1305**
- **Type**: Stream cipher with authentication
- **Key Size**: 256 bits
- **Nonce Size**: 96 bits
- **Usage**: High speed, strong security profile
- **Performance**: Excellent in software implementations

### **XChaCha20-Poly1305**
- **Type**: Extended nonce stream cipher
- **Key Size**: 256 bits
- **Nonce Size**: 192 bits
- **Usage**: High-volume applications requiring unique nonces
- **Performance**: Eliminates nonce reuse concerns

## 💻 **Command Examples**

### **Keypair Generation**

```bash
# Generate ECIES keypair (display in terminal)
spacekit keypair

# Generate quantum keypair with save
spacekit keypair --algorithm kyber1024 --save

# Generate with custom paths
spacekit keypair --algorithm frodokem1344aes --save \
  --secret-key-path keys/quantum_secret.hex \
  --public-key-path keys/quantum_public.hex

# Generate for specific chain
spacekit --chain solana keypair --save
```

### **Quantum KEM Operations**

```bash
# Encapsulate shared secret
spacekit encapsulate --save \
  --algorithm kyber768 \
  --cipher xchacha20 \
  --public-key-path keys/public.hex \
  --kem-ciphertext-output cipher/ciphertext.hex \
  --kem-secret-output secrets/shared.hex

# Decapsulate shared secret
spacekit decapsulate \
  --algorithm kyber768 \
  --cipher xchacha20 \
  --secret-key-path keys/secret.hex \
  --kem-ciphertext cipher/ciphertext.hex
```

### **Unified File Encryption**

```bash
# Encrypt file with quantum algorithm
spacekit encrypt documents/sensitive.pdf \
  --algorithm frodokem1344shake \
  --cipher aes \
  --kem-secret secrets/shared.hex \
  --output-path encrypted/sensitive.pdf.enc

# Decrypt quantum-encrypted file
spacekit decrypt encrypted/sensitive.pdf.enc \
  --algorithm frodokem1344shake \
  --cipher aes \
  --kem-secret secrets/shared.hex \
  --output-path decrypted/sensitive.pdf

# Encrypt with classical ECIES
spacekit encrypt documents/file.txt \
  --algorithm ecies \
  --public-key-path keys/ecies_public.hex \
  --output-path encrypted/file.enc

# Decrypt with classical ECIES  
spacekit decrypt encrypted/file.enc \
  --algorithm ecies \
  --secret-key-path keys/ecies_secret.hex \
  --output-path decrypted/file.txt
```

## 🌐 **Multi-Chain Support**

### **Ethereum (Default)**
```bash
spacekit --chain ethereum --network mainnet keypair --save
```

### **Solana**
```bash
spacekit --chain solana --network devnet keypair --save
```

### **Bitcoin**
```bash
spacekit --chain bitcoin --network testnet keypair --save
```

## 🎨 **Beautiful User Experience**

The CLI provides a modern, user-friendly experience with:

- **🎯 Emoji-enhanced output** for clear visual feedback
- **🌈 Color-coded information** (green for success, red for errors, blue for info)
- **💡 Helpful suggestions** when commands are used incorrectly
- **📋 Comprehensive help text** for every command and option
- **⚡ Fast performance** with optimized quantum operations

### **Example Output**
```bash
$ spacekit keypair --algorithm kyber1024

✅ Quantum KEM Kyber1024 Generated Key Pair
🔑 Private Key: 88d6c721845fc62ab39f6071ce10001ee92eeb...
🔑 Public Key: 45c27f85e9422ba2602e289578209c6202467c...
```

## 🔧 **Advanced Usage**

### **Algorithm Recommendation**

- **🎯 Default (Recommended)**: `kyber768` - Perfect balance of security and performance
- **🚀 For Speed**: `kyber512`, `bikel1`
- **🛡️ For Maximum Security**: `kyber1024`, `frodokem1344shake`
- **📦 For Small Keys**: `kyber512`, `bikel1`
- **🏢 For Enterprise**: `kyber768`, `classicmceliece6688128`
- **🔄 For Migration**: `ecies` - Classical encryption for compatibility

### **Cipher Suite Recommendation**

- **🏎️ For Performance**: `chacha20` (software optimized)
- **🔒 For Compliance**: `aes` (hardware accelerated)
- **📊 For High Volume**: `xchacha20` (extended nonce)

### **File Organization**

```
project/
├── keys/
│   ├── quantum_secret.hex
│   ├── quantum_public.hex
│   └── ecies_keys.hex
├── encrypted/
│   ├── file.enc
│   └── file.kem
└── secrets/
    └── shared_secret.hex
```

## 🛠️ **Installation & Setup**

### **Dependencies**

The CLI integrates the complete SpaceKit Network ecosystem:

#### **Core SpaceKit Libraries**
- **`spacekit-primitives`** - Quantum cryptography, encryption, identity foundations
- **`spacekit-compute-node`** - Distributed computing, task management, consensus
- **`spacekit-storage-node`** - Quantum-safe storage, P2P distribution
- **`spacekit-network-did`** - Decentralized identity, verifiable credentials

#### **Cryptography & Networking**
- **`aes-gcm`, `chacha20poly1305`** - Symmetric encryption implementations
- **`ecies`** - Elliptic Curve Integrated Encryption Scheme
- **`tokio`** - Async runtime for networking and blockchain operations

#### **CLI Infrastructure**
- **`clap`** - Modern command line argument parsing
- **`colored`** - Beautiful terminal output with colors
- **`serde`, `toml`** - Configuration management

**Note**: The CLI provides a unified interface to the entire SpaceKit Network platform, with all 19 quantum-resistant algorithms provided through the Open Quantum Safe (OQS) library integration.

### **Build & Install**

```bash
# Build from source
git clone https://github.com/spacekitlabs/spacekit
cd spacekit/spacekit-cli
cargo build --release

# Run directly
cargo run -- keypair --algorithm kyber1024 --help

# Install binary
cargo install --path .

# After installation, use "spacekit" command globally
spacekit keypair --algorithm kyber1024 --save
```

## 🔗 **Integration Examples**

### **Workflow: Complete Quantum Encryption**

This example demonstrates the full KEM workflow explained in the [Understanding Quantum Key Encapsulation](#-understanding-quantum-key-encapsulation-kem) section:

```bash
# 1. Generate quantum-resistant keypair
spacekit keypair --algorithm kyber1024 --save

# 2. KEM Step 1: Encapsulate shared secret (sender side)
spacekit encapsulate --algorithm kyber1024 --cipher aes --save \
  --public-key-path public_key.hex \
  --kem-secret-output shared_secret.hex

# 3. Encrypt file using the shared secret
spacekit encrypt sensitive.doc \
  --algorithm kyber1024 \
  --cipher aes \
  --kem-secret shared_secret.hex

# 4. Securely transmit: encrypted file + KEM ciphertext
#    (The shared secret stays private!)

# 5. KEM Step 2: Decapsulate shared secret (receiver side)
spacekit decapsulate --algorithm kyber1024 --cipher aes \
  --secret-key-path secret_key.hex \
  --kem-ciphertext kem_ciphertext.hex

# 6. Decrypt file using the recovered shared secret
spacekit decrypt sensitive.doc.enc \
  --algorithm kyber1024 \
  --cipher aes \
  --kem-secret shared_secret.hex
```

**Key Point**: Steps 2 & 5 are the KEM process - this is what makes quantum encryption different from classical!

## 📊 **Performance Characteristics**

| **Algorithm** | **Key Size** | **Speed** | **Security Level** | **Use Case** |
|--------------|-------------|-----------|-------------------|--------------|
| Kyber512 | Small | Fast | High | IoT, Mobile |
| Kyber768 | Medium | Fast | Very High | General Purpose |
| Kyber1024 | Large | Medium | Extreme | High Security |
| FrodoKEM | Large | Slow | Very High | Conservative |
| BIKE | Medium | Fast | High | Balanced |
| Classic McEliece | Very Large | Medium | Very High | Long-term |

## 🚨 **Security Best Practices**

### **Key Management**
- ✅ **Generate fresh keys** for each encryption session
- ✅ **Store private keys securely** with proper file permissions
- ✅ **Use strong randomness** (CLI uses cryptographically secure RNG)
- ✅ **Verify key integrity** before encryption operations

### **Algorithm Selection**
- ✅ **Use NIST-standardized** algorithms (Kyber family) for compliance
- ✅ **Consider future-proofing** with conservative parameters
- ✅ **Match algorithm strength** to data sensitivity level
- ✅ **Test compatibility** with receiving systems

### **Operational Security**
- ✅ **Secure deletion** of temporary files and secrets
- ✅ **Network isolation** during key generation
- ✅ **Audit logging** for encryption operations
- ✅ **Regular key rotation** for long-term usage

## 🔄 **Migration from Classical Crypto**

The CLI provides seamless migration from classical to quantum-resistant encryption using the unified interface:

```bash
# Classical ECIES encryption
spacekit encrypt file.txt --algorithm ecies --public-key-path classical_key.hex

# Quantum-resistant equivalent (after encapsulation)
spacekit encrypt file.txt --algorithm kyber768 --cipher aes --kem-secret shared_secret.hex

# The algorithm parameter makes the transition seamless!
```

## 📚 **Additional Resources**

- **🌐 SpaceKit Network**: [https://spacekit.xyz](https://spacekit.xyz)
- **📖 Quantum Cryptography Guide**: [SpaceKit Documentation](https://docs.spacekit.xyz)
- **🔬 Algorithm Specifications**: [NIST Post-Quantum Standards](https://csrc.nist.gov/projects/post-quantum-cryptography)
- **💬 Community Support**: [SpaceKit Discord](https://discord.gg/spacekit)

## 🏆 **Feature Completeness**

| **Feature Category** | **Implementation Status** | **Coverage** |
|---------------------|-------------------------|--------------|
| **🏗️ Workspace Management** | ✅ **COMPLETE** | Full project initialization |
| **💻 Distributed Computing** | ✅ **COMPLETE** | 6/6 task commands |
| **💾 Quantum-Safe Storage** | ✅ **COMPLETE** | 7/7 storage commands |
| **🆔 Decentralized Identity** | ✅ **COMPLETE** | 7/7 DID commands |
| **🌐 Network Operations** | ✅ **COMPLETE** | 5/5 network commands |
| **🗳️ Consensus & Governance** | ✅ **COMPLETE** | 5/5 consensus commands |
| **🔐 Quantum Cryptography** | ✅ **COMPLETE** | 19/19 algorithms |
| **🔗 Multi-Chain Support** | ✅ **COMPLETE** | 3/3 blockchains |
| **🎨 User Experience** | ✅ **COMPLETE** | Modern CLI design |
| **📚 Documentation** | ✅ **COMPLETE** | Comprehensive guides |

**The SpaceKit CLI is the world's most complete quantum-resistant distributed computing platform!** 🚀

### **📊 Command Summary**
- **Total Commands**: 31 commands across 7 categories
- **Quantum Algorithms**: 19 post-quantum + ECIES classical
- **Network Features**: Service discovery, reputation, consensus
- **Storage Types**: Quantum-safe, collaborative, specialized
- **Identity Management**: DIDs, credentials, verification
- **Computing**: WebAssembly, GPU, hybrid execution

**From quantum cryptography to distributed governance - everything you need for the post-quantum future!** ⚡
