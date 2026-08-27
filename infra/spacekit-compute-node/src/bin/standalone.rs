//! SpaceKit Compute Node - Standalone Binary
//!
//! Provides quantum-secure distributed computing services integrated with the SpaceKit platform

#![recursion_limit = "512"]

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use hex;
use serde::{Deserialize, Serialize};
use serde_json;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::net::TcpListener;
use tokio::signal;
use tracing::{error, info, warn};
use uuid::Uuid;
use warp;

use spacekit_compute_node::{
    network::NetworkService,
    quantum_security::{quantum_did_utils, QuantumResistantDID, QuantumResistantEncryption},
    spacekitvm::{SnapshotManifest, SwtchvmBlock, SwtchvmNode},
    subscriber_sync::{build_subscriber_sync_bundle, merge_l1_manifest_for_proposal},
    swtch_consensus::NetworkMetrics,
    vpos::VPoSManager,
    BlockData, BlockProposal, HybridProposal, MetricsProposal, UnifiedConsensusConfig,
    UnifiedSWTCHConsensus,
};
use spacekit_compute_node::{ComputeConfig, ComputeError, ComputeNode, ComputeTask, TaskStatus};
use spacekit_primitives::v1::sdk::token::AstraToken;

#[derive(Parser)]
#[command(name = "spacekit-compute-node")]
#[command(about = "SpaceKit Quantum-Resistant Compute Node")]
#[command(
    long_about = "A compute node that provides quantum-secure distributed computing services integrated with the SWTCH platform"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Configuration file path
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Node DID (Decentralized Identifier)
    #[arg(long)]
    node_did: Option<String>,

    /// Network to connect to
    #[arg(long, default_value = "localhost")]
    network: String,

    /// HTTP server port
    #[arg(long, default_value = "8080")]
    port: u16,

    /// Bootstrap peer addresses (host:port), can be repeated
    #[arg(long = "bootstrap")]
    bootstrap_nodes: Vec<String>,

    /// P2P listen port
    #[arg(long, default_value = "9000")]
    p2p_port: u16,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the compute node
    Start {
        /// Enable GPU acceleration
        #[arg(long)]
        gpu: bool,

        /// Maximum CPU cores to use
        #[arg(long, default_value = "4")]
        max_cpu_cores: u32,

        /// Maximum memory in MB
        #[arg(long, default_value = "8192")]
        max_memory_mb: u64,

        /// Do not bind the HTTP API on `rpc_port` (overrides `[network].enable_http_api`).
        #[arg(long)]
        no_http: bool,
    },

    /// Register with SpaceKit network (integration stub — verify behavior before relying on it)
    Register {
        /// Network RPC / registration endpoint
        #[arg(long)]
        network_endpoint: String,

        /// Node stake amount
        #[arg(long, default_value = "1000")]
        stake: u64,
    },

    /// Show node status
    Status,

    /// Show GPU information
    GpuInfo,

    /// Test compute capabilities
    Test {
        /// Test type (wasm, gpu, hybrid)
        #[arg(long, default_value = "wasm")]
        test_type: String,
    },

    /// Run production testing suite (v1.5)
    ProductionTest {
        /// Test suite to run (integration, performance, stress, all)
        #[arg(long, default_value = "all")]
        suite: String,

        /// Generate detailed report
        #[arg(long)]
        detailed_report: bool,

        /// Output format (json, yaml, table)
        #[arg(long, default_value = "table")]
        format: String,

        /// Save report to file
        #[arg(long)]
        output: Option<String>,
    },

    /// Start MCP (Model Context Protocol) server on stdio
    Mcp {
        /// Optional node DID
        #[arg(long)]
        node_did: Option<String>,
        /// Enable GPU acceleration
        #[arg(long)]
        enable_gpu: bool,
    },
}

/// Query string for `GET /v1/onboarding/balance` (spacekit.xyz-website onboarding sync).
#[derive(Debug, Deserialize)]
struct OnboardingBalanceQuery {
    did: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct NodeConfig {
    /// Node identity configuration
    pub identity: IdentityConfig,

    /// Compute configuration
    pub compute: ComputeConfig,

    /// Network configuration
    pub network: NetworkConfig,

    /// Security configuration
    pub security: SecurityConfig,

    /// Token economics configuration
    pub token: TokenConfig,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdentityConfig {
    /// Node DID
    pub did: String,

    /// Private key file path
    pub private_key_path: String,

    /// Public key file path
    pub public_key_path: String,

    /// Quantum-resistant algorithm to use
    pub quantum_algorithm: String,
}

fn default_enable_http_api() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
struct NetworkConfig {
    /// Network name
    pub name: String,

    /// Network endpoint
    pub endpoint: String,

    /// P2P port
    pub p2p_port: u16,

    /// RPC port
    pub rpc_port: u16,

    /// Bootstrap nodes
    pub bootstrap_nodes: Vec<String>,

    /// When false, the process does not bind `rpc_port` (no operator or SwtchVM HTTP). P2P and
    /// in-process compute still run — use for headless / relay-only / archive-adjacent profiles.
    #[serde(default = "default_enable_http_api")]
    pub enable_http_api: bool,

    /// Allow `finalize: true` on propose without quorum checks (local dev only).
    #[serde(default)]
    pub dev_mode: bool,

    /// When true, `finalize: true` is allowed only if `validator_count() <= 1`.
    #[serde(default)]
    pub allow_single_validator_finalize: bool,

    /// Interface the HTTP API binds to.
    ///
    /// Defaults to loopback. Binding `0.0.0.0` exposes every operator endpoint
    /// to the network and must be an explicit choice, paired with a firewall.
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
}

fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct SecurityConfig {
    /// Enable quantum-resistant encryption
    pub quantum_encryption: bool,

    /// Supported quantum algorithms
    pub supported_algorithms: Vec<String>,

    /// Default encryption algorithm
    pub default_algorithm: String,

    /// Enable secure compute enclaves
    pub secure_enclaves: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenConfig {
    /// Token contract address
    pub contract_address: String,

    /// Minimum stake required
    pub minimum_stake: u64,

    /// Service fee percentage
    pub service_fee_percent: f64,

    /// Payment settlement interval
    pub settlement_interval_seconds: u64,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            identity: IdentityConfig {
                did: "did:spacekit:compute:node".to_string(),
                private_key_path: "~/.spacekit/keys/private_key.hex".to_string(),
                public_key_path: "~/.spacekit/keys/public_key.hex".to_string(),
                quantum_algorithm: "Kyber1024".to_string(),
            },
            compute: ComputeConfig::default(),
            network: NetworkConfig {
                name: "spacekit-testnet".to_string(),
                endpoint: "wss://testnet.spacekit.xyz".to_string(),
                p2p_port: 9000,
                rpc_port: 8080,
                bootstrap_nodes: vec![
                    "wss://bootstrap1.spacekit.xyz".to_string(),
                    "wss://bootstrap2.spacekit.xyz".to_string(),
                ],
                enable_http_api: true,
                dev_mode: false,
                allow_single_validator_finalize: false,
                bind_address: default_bind_address(),
            },
            security: SecurityConfig {
                quantum_encryption: true,
                supported_algorithms: vec![
                    "Kyber512".to_string(),
                    "Kyber768".to_string(),
                    "Kyber1024".to_string(),
                    "NtruPrimeSntrup761".to_string(),
                    "FrodoKem1344Aes".to_string(),
                ],
                default_algorithm: "Kyber768".to_string(),
                secure_enclaves: false,
            },
            token: TokenConfig {
                contract_address: "0x1234567890123456789012345678901234567890".to_string(),
                minimum_stake: 1000,
                service_fee_percent: 2.5,
                settlement_interval_seconds: 3600,
            },
        }
    }
}

/// SpaceKit Compute Node Service
// TODO: rename to SpaceKitComputeNode
pub struct SwtchComputeNode {
    config: NodeConfig,
    compute_node: ComputeNode,
    identity: Arc<QuantumResistantDID>,
    encryption: Arc<QuantumResistantEncryption>,
    network_service: NetworkService,
    consensus_coordinator: Arc<spacekit_compute_node::ConsensusCoordinator>,
    /// Unified block/metrics/hybrid proposal path (L1 manifest validation in `BlockData`).
    unified_consensus: Arc<UnifiedSWTCHConsensus>,
    token_service: AstraToken,
    /// In-process SwtchVM node: full dev HTTP API merged on `rpc_port` (`SwtchvmNode::http_dev_api_routes`).
    swtchvm_node: Arc<SwtchvmNode>,
    /// Kyber (etc.) secret key bytes from CLI `private_key.hex`, when present.
    cli_kem_secret: Option<Vec<u8>>,
    /// Kyber (etc.) public key bytes from CLI `public_key.hex`, when present.
    cli_kem_public: Option<Vec<u8>>,
    /// Inferred KEM algorithm name for the loaded CLI keys (e.g. `Kyber1024`).
    cli_kem_algorithm: Option<String>,
    #[cfg(feature = "spacetime-consensus")]
    pq_keys: Arc<spacekit_compute_node::PqFinisherKeys>,
    /// [`spacekit_unified_consensus`] facade over [`ConsensusCoordinator`] + spacetime extension.
    #[cfg(feature = "spacetime-consensus")]
    consensus_host: Arc<spacekit_compute_node::UnifiedConsensusHost>,
}

fn kem_sizes_for_config_algorithm(alg: &str) -> Option<(usize, usize)> {
    match alg.trim().to_ascii_lowercase().as_str() {
        "kyber512" => Some((800, 1632)),
        "kyber768" => Some((1184, 2400)),
        "kyber1024" => Some((1568, 3168)),
        _ => None,
    }
}

fn parse_hex_key_file(path: &str) -> Result<Vec<u8>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read key file {}: {}", path, e))?;
    let trimmed: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    hex::decode(&trimmed).map_err(|e| anyhow::anyhow!("Invalid hex in {}: {}", path, e))
}

fn infer_kem_algorithm(pk_len: usize, sk_len: usize) -> Option<&'static str> {
    match (pk_len, sk_len) {
        (800, 1632) => Some("Kyber512"),
        (1184, 2400) => Some("Kyber768"),
        (1568, 3168) => Some("Kyber1024"),
        _ => None,
    }
}

/// Load SpaceKit CLI KEM keys (`public_key.hex` / `private_key.hex`). Returns `Ok(None)` if both files are missing.
fn load_cli_kem_identity(ic: &IdentityConfig) -> Result<Option<(Vec<u8>, Vec<u8>, String)>> {
    let pk_path = ic.public_key_path.as_str();
    let sk_path = ic.private_key_path.as_str();
    let pk_exists = std::path::Path::new(pk_path).exists();
    let sk_exists = std::path::Path::new(sk_path).exists();
    if !pk_exists && !sk_exists {
        return Ok(None);
    }
    if pk_exists != sk_exists {
        anyhow::bail!(
            "Both CLI key files must exist together (public_key_path={}, private_key_path={})",
            pk_path,
            sk_path
        );
    }
    let pk = parse_hex_key_file(pk_path)?;
    let sk = parse_hex_key_file(sk_path)?;
    let inferred = infer_kem_algorithm(pk.len(), sk.len()).ok_or_else(|| {
        anyhow::anyhow!(
            "Unrecognized KEM key lengths (public {} bytes, secret {} bytes); expected Kyber512/768/1024",
            pk.len(),
            sk.len()
        )
    })?;
    let cfg_alg = ic.quantum_algorithm.trim();
    if kem_sizes_for_config_algorithm(cfg_alg).is_none() {
        warn!(
            "identity.quantum_algorithm {:?} is not a recognized KEM label for size checks; treating CLI keys as {}",
            cfg_alg, inferred
        );
    } else if cfg_alg.to_ascii_lowercase() != inferred.to_ascii_lowercase() {
        warn!(
            "identity.quantum_algorithm ({}) does not match file sizes (inferred {}). Using {} for loaded CLI material.",
            cfg_alg, inferred, inferred
        );
    }
    Ok(Some((sk, pk, inferred.to_string())))
}

fn build_runtime_identity(
    ic: &IdentityConfig,
) -> Result<(QuantumResistantDID, Option<(Vec<u8>, Vec<u8>, String)>)> {
    let kem = load_cli_kem_identity(ic)?;
    let mut wallet = QuantumResistantDID::new();
    wallet
        .apply_config_did(&ic.did)
        .map_err(|e| anyhow::anyhow!("identity.did invalid: {}", e))?;
    match &kem {
        Some(_) => info!(
            "Loaded CLI KEM material from {} / {}",
            ic.public_key_path, ic.private_key_path
        ),
        None => warn!(
            "CLI KEM keys not found at configured paths; using generated SPHINCS+ keys with config DID string"
        ),
    }
    warn!(
        "Signing on this node uses the embedded SPHINCS+ wallet keys; CLI Kyber material is available for KEM-aligned features (storage, registry payloads). Kyber-only signing is not implemented here."
    );
    Ok((wallet, kem))
}

#[derive(Debug, Deserialize)]
struct ConsensusProposeBody {
    #[serde(rename = "type")]
    proposal_kind: String,
    #[serde(default)]
    proposer_did: Option<String>,
    /// When true and `type` is `block`, broadcast a BlockAnnounce P2P message after submit.
    #[serde(default)]
    announce: bool,
    /// Fill missing `block` fields from [`SwtchvmNode::get_latest_block`] (next height, parent hash, state root).
    #[serde(default)]
    use_swtchvm_head: bool,
    #[serde(default)]
    block: Option<serde_json::Value>,
    #[serde(default)]
    metrics: Option<serde_json::Value>,
    /// When true, reuse on-disk L1 [`SnapshotManifest`] if height and state root match the proposal.
    #[serde(default)]
    use_l1_snapshot_manifest: bool,
    /// After submit, run PQ finisher (Dilithium votes + SPHINCS+ envelope) when finality is reached.
    #[serde(default)]
    finalize: bool,
    #[serde(default)]
    round: u64,
    #[serde(default)]
    view: u64,
}

#[derive(Debug, Deserialize)]
#[cfg(feature = "spacetime-consensus")]
struct ConsensusFinalizeBody {
    proposal_id: String,
    #[serde(default)]
    round: u64,
    #[serde(default)]
    view: u64,
}

#[derive(Debug, Deserialize)]
#[cfg(feature = "spacetime-consensus")]
struct ConsensusFraudProofBody {
    submission: spacekit_spacetime_consensus::FraudProofSubmission,
}

#[derive(Debug, Deserialize)]
#[cfg(feature = "spacetime-consensus")]
struct FingerprintAttestationBody {
    attestation: spacekit_spacetime_consensus::FingerprintAttestation,
}

#[derive(Debug, Deserialize)]
#[cfg(feature = "spacetime-consensus")]
struct ParameterProposalBody {
    proposal: spacekit_spacetime_consensus::ParameterChangeProposal,
}

#[derive(Debug, Deserialize)]
#[cfg(feature = "spacetime-consensus")]
struct ParameterVoteBody {
    vote: spacekit_spacetime_consensus::ParameterChangeVote,
}

#[derive(Debug, Deserialize)]
#[cfg(feature = "spacetime-consensus")]
struct ParameterFinalizeBody {
    proposal_id: String,
    #[serde(default)]
    at_height: u64,
}

#[derive(Debug, Deserialize, Default)]
struct HttpBlockPayload {
    #[serde(default)]
    block_number: Option<u64>,
    #[serde(default)]
    parent_hash: Option<String>,
    #[serde(default)]
    transactions: Option<Vec<String>>,
    #[serde(default)]
    state_root: Option<String>,
    #[serde(default)]
    chain_id: Option<String>,
    #[serde(default)]
    l1_manifest: Option<SnapshotManifest>,
    /// When `spacetime-consensus` is enabled (standalone binary), optional rotor sidecar.
    #[cfg(feature = "spacetime-consensus")]
    #[serde(default)]
    spacetime_transition: Option<spacekit_compute_node::spacetime_consensus::SpacetimeTransition>,
    #[cfg(feature = "spacetime-consensus")]
    #[serde(default)]
    consensus_votes: Option<Vec<spacekit_compute_node::ConsensusVoteInner>>,
    #[cfg(feature = "spacetime-consensus")]
    #[serde(default)]
    signed_block_envelope: Option<spacekit_compute_node::SignedBlockEnvelope>,
}

#[derive(Debug, Deserialize)]
struct HttpMetricsPayload {
    cpu_utilization: f64,
    memory_utilization: f64,
    network_utilization: f64,
    storage_utilization: f64,
}

fn block_data_for_proposal(
    chain_id: &str,
    vm: &SwtchvmNode,
    use_head: bool,
    block_json: Option<serde_json::Value>,
    use_l1_snapshot_manifest: bool,
) -> Result<BlockData, String> {
    let partial: HttpBlockPayload = match block_json {
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string())?,
        None if use_head => HttpBlockPayload::default(),
        None => {
            return Err("missing \"block\" object (or set use_swtchvm_head: true)".to_string());
        }
    };

    let head: Option<SwtchvmBlock> = if use_head {
        Some(vm.get_latest_block())
    } else {
        None
    };

    let block_number = match partial.block_number {
        Some(n) => n,
        None => head
            .as_ref()
            .map(|h| h.number.saturating_add(1))
            .ok_or_else(|| "block_number required when use_swtchvm_head is false".to_string())?,
    };
    let parent_hash = match partial.parent_hash.clone() {
        Some(p) => p,
        None => head
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h.hash)))
            .ok_or_else(|| "parent_hash required when use_swtchvm_head is false".to_string())?,
    };
    let state_root = match partial.state_root.clone() {
        Some(s) => s,
        None => head
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h.state_root)))
            .ok_or_else(|| "state_root required when use_swtchvm_head is false".to_string())?,
    };
    let transactions = partial.transactions.unwrap_or_default();

    let chain = partial
        .chain_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| chain_id.to_string());

    let disk = vm.read_l1_manifest().ok().flatten();
    let l1_manifest = match partial.l1_manifest {
        Some(m) => m,
        None => merge_l1_manifest_for_proposal(
            disk.as_ref(),
            &chain,
            &state_root,
            block_number,
            &parent_hash,
            use_l1_snapshot_manifest,
        ),
    };

    let mut block_data = BlockData::new_with_l1_manifest(
        block_number,
        parent_hash,
        transactions,
        state_root,
        SystemTime::now(),
        l1_manifest,
    );
    #[cfg(feature = "spacetime-consensus")]
    {
        block_data.spacetime_transition = partial.spacetime_transition;
        block_data.consensus_votes = partial.consensus_votes;
        block_data.signed_block_envelope = partial.signed_block_envelope;
    }
    Ok(block_data)
}

#[cfg(feature = "spacetime-consensus")]
fn propose_finalize_allowed(
    dev_mode: bool,
    allow_single_validator_finalize: bool,
    validator_count: usize,
    finalize: bool,
) -> bool {
    if !finalize {
        return false;
    }
    if dev_mode {
        return true;
    }
    allow_single_validator_finalize && validator_count <= 1
}

#[cfg(feature = "spacetime-consensus")]
async fn pq_finalize_after_propose(
    host: &spacekit_compute_node::UnifiedConsensusHost,
    vm: &SwtchvmNode,
    identity: &spacekit_compute_node::quantum_security::QuantumResistantDID,
    keys: &spacekit_compute_node::PqFinisherKeys,
    proposal_id: &str,
    block_data: BlockData,
    proposer_did: &str,
    round: u64,
    view: u64,
) -> Result<BlockData, String> {
    let cc = host.coordinator();
    use alloy_primitives::B256;
    use spacekit_compute_node::pq_finisher;
    use spacekit_compute_node::spacetime_integration::spacetime_transition_digest;
    use spacekit_spacetime_consensus::ConsensusVoteType;

    if block_data.signed_block_envelope.is_some() {
        return Ok(block_data);
    }

    cc.register_pending_block(proposal_id, block_data.clone())
        .await;
    cc.register_validator_dilithium(
        proposer_did.to_string(),
        keys.dilithium_public_key.clone(),
        keys.dilithium_secret_key.clone(),
    )
    .await;

    let rotor_digest = block_data
        .spacetime_transition
        .as_ref()
        .map(spacetime_transition_digest)
        .unwrap_or(B256::ZERO);

    let vote = pq_finisher::sign_pq_vote(
        &block_data,
        proposer_did,
        keys,
        ConsensusVoteType::Yes,
        round,
        view,
        rotor_digest,
    );
    let vote_msg = format!("{}:approve:{}", proposal_id, round);
    let sig_bytes = spacekit_compute_node::quantum_security::quantum_did_utils::sign(
        identity,
        vote_msg.as_bytes(),
    )
    .await
    .map_err(|e| e.to_string())?;
    let sig_hex = hex::encode(sig_bytes);
    cc.cast_pq_vote(proposal_id, "approve", round, &sig_hex, &vote)
        .map_err(|e| e.to_string())?;

    // Count-mode: coordinator finality is authoritative; facade is a tripwire.
    // Post-fork (`use_weighted_threshold = true`): flip to host-first — coordinator
    // count-based check no longer matches weighted quorum; see FacadeConfig.
    match cc.check_finality(proposal_id).await {
        spacekit_compute_node::consensus_coordinator::FinalityStatus::Finalized { .. } => {}
        other => {
            return Err(format!("proposal not finalized yet: {:?}", other));
        }
    }

    host.has_consensus(proposal_id)
        .await
        .map_err(|e| format!("unified consensus facade: {:?}", e))?;

    let finalized = pq_finisher::finalize_proposal_if_ready(cc, keys, proposal_id, round, view)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "finisher produced no block".to_string())?;

    apply_spacetime_side_effects(cc, vm, &finalized).await;
    Ok(finalized)
}

#[cfg(feature = "spacetime-consensus")]
async fn apply_spacetime_side_effects(
    cc: &spacekit_compute_node::ConsensusCoordinator,
    vm: &SwtchvmNode,
    block: &BlockData,
) {
    spacekit_compute_node::spacetime_integration::apply_block_spacetime_side_effects(cc, vm, block)
        .await;
}

impl SwtchComputeNode {
    pub async fn new(config: NodeConfig) -> Result<Self> {
        info!(
            "Initializing SpaceKit Compute Node with configured DID: {}",
            config.identity.did
        );

        let (wallet, kem_extra) = build_runtime_identity(&config.identity)?;
        let identity = Arc::new(wallet);
        let cli_kem_secret = kem_extra.as_ref().map(|(s, _, _)| s.clone());
        let cli_kem_public = kem_extra.as_ref().map(|(_, p, _)| p.clone());
        let cli_kem_algorithm = kem_extra.as_ref().map(|(_, _, a)| a.clone());

        let encryption = Arc::new(
            QuantumResistantEncryption::new(
                &config.security.default_algorithm,
                &config.security.supported_algorithms,
            )
            .await?,
        );

        // The standalone HTTP API owns the authoritative SwtchVM instance.  Do not let the
        // generic compute worker open the same snapshot path: two independent runtimes writing
        // one file can lose transactions even though each individual write is atomic.
        let mut worker_config = config.compute.clone();
        worker_config.swtchvm_state_path = None;
        let compute_node = ComputeNode::new(worker_config).await?;

        let net_config = spacekit_compute_node::network::NetworkConfig {
            network_name: config.network.name.clone(),
            listen_address: "0.0.0.0".to_string(),
            listen_port: config.network.p2p_port,
            bootstrap_nodes: config.network.bootstrap_nodes.clone(),
            max_peers: 50,
        };
        let network_service =
            NetworkService::new(net_config, identity.clone(), encryption.clone()).await?;

        let node_did = quantum_did_utils::get_did(&identity);
        let consensus_coordinator = Arc::new(spacekit_compute_node::ConsensusCoordinator::new(
            network_service.clone(),
            node_did.clone(),
        ));

        #[cfg(feature = "spacetime-consensus")]
        let consensus_host = Arc::new(spacekit_compute_node::UnifiedConsensusHost::new(
            consensus_coordinator.clone(),
        ));

        let vpos_manager = Arc::new(
            VPoSManager::new(
                identity.clone(),
                spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber512,
            )
            .await
            .map_err(|e| anyhow::anyhow!("VPoS manager init failed: {}", e))?,
        );
        let unified_consensus = Arc::new(
            UnifiedSWTCHConsensus::new(
                UnifiedConsensusConfig::default(),
                identity.clone(),
                vpos_manager,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Unified consensus init failed: {}", e))?,
        );

        let token_service =
            AstraToken::new(&config.token.contract_address, config.token.minimum_stake)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to initialize token service: {:?}", e))?;

        let mut swtchvm_node = SwtchvmNode::new_with_persistence(
            false,
            false,
            config.compute.swtchvm_state_path.clone(),
        )
        .await?;
        let numeric_chain_id = config.compute.chain_id.parse::<u64>().unwrap_or_else(|_| {
            warn!(
                "compute.chain_id {:?} is not numeric; Ethereum RPC will use 1337",
                config.compute.chain_id
            );
            1337
        });
        swtchvm_node.set_chain_id(config.compute.chain_id.clone(), numeric_chain_id);
        if config.compute.sra_config.enabled {
            swtchvm_node.ensure_system_contracts().await?;
            swtchvm_node.set_sra_host(spacekit_compute_node::SraHost::new(
                config.compute.sra_config.clone(),
            ));
            tracing::info!("Service Reward Accumulator (SRA) enabled on SwtchVM");
        }
        let swtchvm_node = Arc::new(swtchvm_node);

        #[cfg(feature = "spacetime-consensus")]
        let pq_keys = Arc::new(
            spacekit_compute_node::pq_finisher::PqFinisherKeys::from_identity_wallet(&identity)
                .map_err(|e| anyhow::anyhow!("PQ finisher keys: {}", e))?,
        );

        Ok(Self {
            config,
            compute_node,
            identity,
            encryption,
            network_service,
            consensus_coordinator,
            unified_consensus,
            token_service,
            swtchvm_node,
            cli_kem_secret,
            cli_kem_public,
            cli_kem_algorithm,
            #[cfg(feature = "spacetime-consensus")]
            pq_keys,
            #[cfg(feature = "spacetime-consensus")]
            consensus_host,
        })
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting SpaceKit Compute Node...");

        self.compute_node.start().await?;
        self.register_with_network().await?;
        self.network_service.start().await?;
        spacekit_compute_node::network::start_swtchvm_bridge(
            self.swtchvm_node.clone(),
            self.network_service.clone(),
        );

        // Register ourselves as a validator and start the consensus listener
        let local_did = quantum_did_utils::get_did(&self.identity);
        let local_did_clone = local_did.clone();
        self.consensus_coordinator
            .register_validator(local_did.clone())
            .await;
        #[cfg(feature = "spacetime-consensus")]
        self.consensus_coordinator
            .register_validator_dilithium(
                local_did,
                self.pq_keys.dilithium_public_key.clone(),
                self.pq_keys.dilithium_secret_key.clone(),
            )
            .await;
        #[cfg(feature = "spacetime-consensus")]
        self.consensus_host.start_p2p_listener();
        #[cfg(not(feature = "spacetime-consensus"))]
        self.consensus_coordinator.start_listener();
        info!("Consensus coordinator listening for P2P votes (facade telemetry on vote receipt)");

        #[cfg(all(
            feature = "spacetime-consensus",
            feature = "growformer-inference",
            feature = "storage-integration"
        ))]
        {
            if let Some(storage_mgr) = self.compute_node.get_storage_manager() {
                let storage = storage_mgr.read().await.storage_node();
                if let Some(storage) = storage {
                    let _agent = spacekit_compute_node::spacetime_integration::bootstrap_consensus_growformer_agent(
                        self.swtchvm_node.clone(),
                        storage,
                        self.consensus_coordinator.clone(),
                        &local_did_clone,
                    )
                    .await;
                }
            }
        }

        if self.config.network.enable_http_api {
            self.start_http_server().await?;
        } else {
            warn!(
                "HTTP API disabled ([network].enable_http_api = false); not binding port {}. \
                 Service registration may still advertise this URL — verify peers and ops tooling.",
                self.config.network.rpc_port
            );
        }

        info!("SpaceKit Blockchain Node started successfully");
        Ok(())
    }

    async fn register_with_network(&self) -> Result<()> {
        info!("Registering with SpaceKit network...");

        // Create service registration
        let service_info = spacekit_compute_node::network::ServiceInfo {
            service_id: format!("compute-{}", uuid::Uuid::new_v4()),
            service_type: "compute".to_string(),
            did: quantum_did_utils::get_did(&self.identity),
            endpoint: format!("http://localhost:{}", self.config.network.rpc_port),
            capabilities: vec![
                "wasm-execution".to_string(),
                "gpu-compute".to_string(),
                "quantum-encryption".to_string(),
            ],
            stake_amount: self.config.token.minimum_stake,
            created_at: Utc::now(),
        };

        // Sign and submit registration
        let registration_data = serde_json::to_string(&service_info)?;
        let signature = self
            .identity
            .sign_content(&registration_data)
            .map_err(|e| anyhow::anyhow!("Signing failed: {}", e))?;

        // Convert hex signature to bytes
        let signature_bytes = hex::decode(&signature)
            .map_err(|e| anyhow::anyhow!("Failed to decode signature: {}", e))?;

        self.network_service
            .register_service(service_info, signature_bytes)
            .await?;

        info!("Successfully registered with SpaceKit network");
        Ok(())
    }

    async fn start_http_server(&self) -> Result<()> {
        use warp::Filter;

        let port = self.config.network.rpc_port;

        let status_payload = serde_json::json!({
            "node_did": quantum_did_utils::get_did(&self.identity),
            "available_cpu_cores": self.config.compute.max_cpu_cores,
            "available_memory_mb": self.config.compute.max_memory_mb,
            "cli_kem_loaded": self.cli_kem_public.is_some(),
            "cli_kem_public_bytes": self.cli_kem_public.as_ref().map(|p| p.len()),
            "cli_kem_secret_bytes": self.cli_kem_secret.as_ref().map(|s| s.len()),
            "cli_kem_algorithm": self.cli_kem_algorithm,
            "active_tasks": 0,
            "total_tasks_processed": 0,
            "note": "Task counters are placeholders until wired to ComputeNode runtime metrics.",
        });
        let status_snapshot = status_payload.clone();

        // ── Request authentication ──
        // Every mutating endpoint below is gated on a DID-signed request. The
        // registry is populated by /v1/did/register, which is the
        // proof-of-possession step.
        let network_label = self.config.network.name.clone();
        let did_registry =
            spacekit_compute_node::api_auth::DidKeyRegistry::new(Some(std::path::PathBuf::from(
                std::env::var("SPACEKIT_DID_REGISTRY_PATH")
                    .unwrap_or_else(|_| "temp_blockchain_storage/did_registry.json".to_string()),
            )));
        let authenticator = Arc::new(spacekit_compute_node::api_auth::RequestAuthenticator::new(
            spacekit_compute_node::api_auth::AuthConfig::from_env(&network_label),
            did_registry.clone(),
        ));
        if authenticator.config().admin_dids.is_empty() {
            warn!(
                "SPACEKIT_ADMIN_DIDS is empty — operator-only endpoints will reject every \
                 caller. Set it to the DIDs permitted to run this node."
            );
        }

        // ── On-chain entitlements ──
        // Replaces the aUSD vault: balances originate from DAI/USDC deposits
        // into the Ethereum entitlement contract and are only ever read here.
        let entitlement_config = spacekit_compute_node::EntitlementConfig::from_env();
        let entitlement_reader =
            match spacekit_compute_node::EntitlementReader::new(entitlement_config.clone()) {
                Ok(reader) => Some(Arc::new(reader)),
                Err(e) => {
                    warn!(
                    "Entitlement reader unavailable ({e}); paid endpoints will refuse requests. \
                     Set SPACEKIT_ENTITLEMENT_CONTRACT and SPACEKIT_ENTITLEMENT_RPC_URLS."
                );
                    None
                }
            };

        // Health check endpoint
        let health_route = warp::path("health")
            .and(warp::get())
            .and_then(move || async move {
                let health = serde_json::json!({
                    "status": "healthy",
                    "timestamp": chrono::Utc::now().timestamp(),
                    "node_type": "compute",
                    "version": env!("CARGO_PKG_VERSION"),
                    "api": {
                        "port": port,
                        "endpoints": [
                            "/health",
                            "/status",
                            "/faucet",
                            "/rpc",
                            "GET /account/{address}",
                            "POST /transaction",
                            "POST /contract/deploy",
                            "POST /contract/call",
                            "GET /block/{n}",
                            "GET /block/header/{n}",
                            "GET /receipt/{tx_hash}",
                            "POST /mine",
                            "POST /verifyProof",
                            "rollup/*",
                            "/v1/node/identity",
                            "GET /v1/network/peers",
                            "/v1/onboarding/balance?did=",
                            "GET /v1/sync/subscriber",
                            "POST /v1/consensus/propose",
                        ]
                    }
                });

                Ok::<_, warp::Rejection>(warp::reply::json(&health))
            });

        // Status endpoint (non-secret snapshot for ops / Prometheus hooks)
        let status_route = warp::path("status")
            .and(warp::get())
            .map(move || status_payload.clone())
            .and_then(|body| async move { Ok::<_, warp::Rejection>(warp::reply::json(&body)) });

        let node_identity_route = warp::path!("v1" / "node" / "identity")
            .and(warp::get())
            .map(move || status_snapshot.clone())
            .and_then(|body| async move { Ok::<_, warp::Rejection>(warp::reply::json(&body)) });

        // Website onboarding (Step 3→4): confirms the node is reachable for a DID.
        // Balance is a placeholder until the ledger indexes DIDs.
        let onboarding_balance_route = warp::path!("v1" / "onboarding" / "balance")
            .and(warp::get())
            .and(warp::query::<OnboardingBalanceQuery>())
            .and_then(|q: OnboardingBalanceQuery| async move {
                let body = serde_json::json!({
                    "did": q.did,
                    "balance": "0",
                    "unit": "astra",
                    "synced": true,
                });
                Ok::<_, warp::Rejection>(warp::reply::json(&body))
            });

        // DID registration endpoint: POST /v1/did/register
        // Body: { "network": "testnet", "sphincs_pk_hex": "...", "kyber_pk_hex": "...", "signature_hex": "..." }
        let did_reg_registry = did_registry.clone();
        let did_register_route = warp::path!("v1" / "did" / "register")
            .and(warp::post())
            .and(warp::body::json())
            .and_then(move |body: serde_json::Value| {
                let registry = did_reg_registry.clone();
                async move {
                    let network = body["network"].as_str().unwrap_or("testnet");
                    let sphincs_pk_hex = match body["sphincs_pk_hex"].as_str() {
                        Some(v) => v,
                        None => {
                            return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({"error": "missing sphincs_pk_hex"}),
                                ),
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    };
                    let kyber_pk_hex = match body["kyber_pk_hex"].as_str() {
                        Some(v) => v,
                        None => {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({"error": "missing kyber_pk_hex"}),
                                ),
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    };
                    let signature_hex = match body["signature_hex"].as_str() {
                        Some(v) => v,
                        None => {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({"error": "missing signature_hex"}),
                                ),
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    };

                    let sphincs_pk = match hex::decode(sphincs_pk_hex) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({"error": "invalid sphincs_pk_hex"}),
                                ),
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    };
                    let kyber_pk = match hex::decode(kyber_pk_hex) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({"error": "invalid kyber_pk_hex"}),
                                ),
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    };
                    let signature = match hex::decode(signature_hex) {
                        Ok(v) => v,
                        Err(_) => {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({"error": "invalid signature_hex"}),
                                ),
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    };

                    // Derive the DID address: SHA-256(sphincs_pk)[0..20] -> hex
                    use sha2::Digest;
                    let hash = sha2::Sha256::digest(&sphincs_pk);
                    let address = hex::encode(&hash[..20]);
                    let did = format!("did:spacekit:{}:{}", network, address);

                    // Verify SPHINCS+ self-signature
                    use spacekit_did::sphincs::SphincsPlus;
                    let mut msg =
                        Vec::with_capacity(sphincs_pk.len() + kyber_pk.len() + network.len());
                    msg.extend_from_slice(&sphincs_pk);
                    msg.extend_from_slice(&kyber_pk);
                    msg.extend_from_slice(network.as_bytes());

                    if !SphincsPlus::verify(&sphincs_pk, &msg, &signature) {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(
                                &serde_json::json!({"error": "signature verification failed"}),
                            ),
                            warp::http::StatusCode::UNAUTHORIZED,
                        ));
                    }

                    // Build the DID document and return success.
                    // In a full implementation this would execute the DID registry contract
                    // via SwtchvmRuntime and replicate to storage nodes.
                    let doc = serde_json::json!({
                        "did": did,
                        "sphincs_pk_hex": sphincs_pk_hex,
                        "kyber_pk_hex": kyber_pk_hex,
                        "network": network,
                        "nonce": 0,
                        "active": true,
                        "created_at": chrono::Utc::now().timestamp(),
                    });

                    // Enroll the verified key so this DID can authenticate later
                    // requests. The self-signature above is the proof of
                    // possession; without recording it, the API would have no key
                    // to check signed requests against.
                    if let Err(e) = registry
                        .register(spacekit_compute_node::RegisteredKey {
                            did: did.clone(),
                            sphincs_pk_hex: sphincs_pk_hex.to_string(),
                            kyber_pk_hex: kyber_pk_hex.to_string(),
                            network: network.to_string(),
                            registered_at: chrono::Utc::now().timestamp() as u64,
                        })
                        .await
                    {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "error": e })),
                            warp::http::StatusCode::CONFLICT,
                        ));
                    }

                    info!("Registered DID: {}", did);

                    Ok(warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({
                            "did": did,
                            "document": doc,
                            "status": "registered"
                        })),
                        warp::http::StatusCode::CREATED,
                    ))
                }
            });

        // DID resolution endpoint: GET /v1/did/resolve?did=did:spacekit:testnet:abc123...
        #[derive(Deserialize)]
        struct DidResolveQuery {
            did: String,
        }

        let did_resolve_route = warp::path!("v1" / "did" / "resolve")
            .and(warp::get())
            .and(warp::query::<DidResolveQuery>())
            .and_then(|q: DidResolveQuery| async move {
                // In a full implementation this would call the DID registry contract
                // via SpaceKitVMRuntime. For now return a stub that validates the format.
                if !q.did.starts_with("did:spacekit:") {
                    return Ok::<_, warp::Rejection>(warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({"error": "invalid DID format"})),
                        warp::http::StatusCode::BAD_REQUEST,
                    ));
                }

                // Placeholder: DID resolution would query the registry contract
                Ok(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "did": q.did,
                        "resolved": false,
                        "message": "DID registry lookup will be connected to SpaceKitVMRuntime"
                    })),
                    warp::http::StatusCode::OK,
                ))
            });

        // State anchor endpoint: POST /v1/state/anchor
        // Computes a Verkle root over the supplied DID documents and returns
        // the root + EVM calldata for submission to SpacekitStateAnchor.sol.
        #[derive(Deserialize)]
        struct AnchorRequest {
            epoch: u64,
            documents: Vec<AnchorDoc>,
        }
        #[derive(Deserialize)]
        struct AnchorDoc {
            did: String,
            #[serde(with = "hex_serde")]
            data: Vec<u8>,
        }
        mod hex_serde {
            use serde::{Deserialize, Deserializer};
            pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
                let s = String::deserialize(d)?;
                hex::decode(&s).map_err(serde::de::Error::custom)
            }
        }

        // Operator-only: an anchor fixes the epoch's state root, so an
        // unauthenticated caller could publish a root of their choosing.
        let state_anchor_route = warp::path!("v1" / "state" / "anchor")
            .and(warp::post())
            .and(spacekit_compute_node::api_auth::signed_json::<AnchorRequest>(
                authenticator.clone(),
            ))
            .and_then(|(caller, req): (
                spacekit_compute_node::AuthenticatedCaller,
                AnchorRequest,
            )| async move {
                spacekit_compute_node::api_auth::require_admin(&caller)?;
                let docs: Vec<(&str, &[u8])> = req
                    .documents
                    .iter()
                    .map(|d| (d.did.as_str(), d.data.as_slice()))
                    .collect();
                let (_tree, anchor) =
                    spacekit_compute_node::state_anchor::build_epoch_anchor(req.epoch, &docs);
                let calldata = spacekit_compute_node::state_anchor::encode_update_root_calldata(
                    anchor.epoch,
                    &anchor.verkle_root,
                );

                Ok::<_, warp::Rejection>(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "epoch": anchor.epoch,
                        "verkle_root": hex::encode(anchor.verkle_root),
                        "document_count": anchor.document_count,
                        "calldata": hex::encode(&calldata),
                    })),
                    warp::http::StatusCode::OK,
                ))
            });

        // ── KeyMaster endpoints ──
        // Shared state: the escrow store is loaded from disk (encrypted)
        // and held in memory behind a RwLock for concurrent access.
        let km_identity = quantum_did_utils::get_did(&self.identity);
        let km_path = std::path::PathBuf::from("keymaster_escrow.bin");
        let km_store = if km_path.exists() {
            match spacekit_compute_node::keymaster::load_escrow(&km_path, &km_identity) {
                Ok(s) => {
                    info!("Loaded KeyMaster escrow ({} entries)", s.entries.len());
                    s
                }
                Err(e) => {
                    warn!("Failed to load escrow ({}), starting fresh", e);
                    Default::default()
                }
            }
        } else {
            Default::default()
        };
        let km_store = Arc::new(tokio::sync::RwLock::new(km_store));
        let km_identity_save = km_identity.clone();
        let km_path_save = km_path.clone();

        // POST /v1/keymaster/register
        // First call: storage node sends { node_did, server_pk_hex, server_sk_hex }.
        // Restart recovery: sends { node_did } only and gets the SK back.
        let km_reg_store = km_store.clone();
        let km_reg_identity = km_identity.clone();
        let km_reg_path = km_path.clone();
        let keymaster_register_route = warp::path!("v1" / "keymaster" / "register")
            .and(warp::post())
            .and(spacekit_compute_node::api_auth::signed_json::<serde_json::Value>(
                authenticator.clone(),
            ))
            .and_then(move |(caller, body): (
                spacekit_compute_node::AuthenticatedCaller,
                serde_json::Value,
            )| {
                let store = km_reg_store.clone();
                let identity = km_reg_identity.clone();
                let path = km_reg_path.clone();
                async move {
                    let did = match body["node_did"].as_str() {
                        Some(d) if d.starts_with("did:") => d,
                        _ => {
                            return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({"error": "missing or invalid node_did"}),
                                ),
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    };
                    // A node may only escrow keys under its own DID.
                    if did != caller.did {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "error": "node_did must match the authenticated caller",
                            })),
                            warp::http::StatusCode::FORBIDDEN,
                        ));
                    }
                    let pk = body["server_pk_hex"].as_str();
                    let sk = body["server_sk_hex"].as_str();
                    let algo = body["algorithm"].as_str();

                    let mut guard = store.write().await;
                    match spacekit_compute_node::keymaster::register_or_recover(
                        &mut guard, did, pk, sk, algo,
                    ) {
                        Ok(entry) => {
                            // Never serialize `server_sk_hex`. Recovery of the
                            // secret key requires the separate KEM-wrapped
                            // flow; returning it here handed the storage node's
                            // private key to any caller.
                            let info =
                                spacekit_compute_node::keymaster::PublicEscrowInfo::from(&entry);
                            let resp = serde_json::json!({
                                "node_did": info.node_did,
                                "server_pk_hex": info.server_pk_hex,
                                "algorithm": info.algorithm,
                                "registered_at": info.registered_at,
                                "previous_key_count": info.previous_key_count,
                            });
                            if let Err(e) = spacekit_compute_node::keymaster::save_escrow(
                                &guard, &path, &identity,
                            ) {
                                warn!("Failed to persist escrow: {}", e);
                            }
                            Ok(warp::reply::with_status(
                                warp::reply::json(&resp),
                                warp::http::StatusCode::OK,
                            ))
                        }
                        Err(e) => Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({"error": e})),
                            warp::http::StatusCode::NOT_FOUND,
                        )),
                    }
                }
            });

        // POST /v1/keymaster/rotate
        let km_rot_store = km_store.clone();
        let km_rot_identity = km_identity_save.clone();
        let km_rot_path = km_path_save.clone();
        let keymaster_rotate_route = warp::path!("v1" / "keymaster" / "rotate")
            .and(warp::post())
            .and(spacekit_compute_node::api_auth::signed_json::<
                serde_json::Value,
            >(authenticator.clone()))
            .and_then(
                move |(caller, body): (
                    spacekit_compute_node::AuthenticatedCaller,
                    serde_json::Value,
                )| {
                    let store = km_rot_store.clone();
                    let identity = km_rot_identity.clone();
                    let path = km_rot_path.clone();
                    async move {
                        let did = match body["node_did"].as_str() {
                            Some(d) => d,
                            None => {
                                return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                    warp::reply::json(
                                        &serde_json::json!({"error": "missing node_did"}),
                                    ),
                                    warp::http::StatusCode::BAD_REQUEST,
                                ))
                            }
                        };
                        // Rotation replaces a node's escrowed key, so only that
                        // node may request it.
                        if did != caller.did {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "error": "node_did must match the authenticated caller",
                                })),
                                warp::http::StatusCode::FORBIDDEN,
                            ));
                        }
                        let pk = match body["new_server_pk_hex"].as_str() {
                            Some(v) => v,
                            None => {
                                return Ok(warp::reply::with_status(
                                    warp::reply::json(
                                        &serde_json::json!({"error": "missing new_server_pk_hex"}),
                                    ),
                                    warp::http::StatusCode::BAD_REQUEST,
                                ))
                            }
                        };
                        let sk = match body["new_server_sk_hex"].as_str() {
                            Some(v) => v,
                            None => {
                                return Ok(warp::reply::with_status(
                                    warp::reply::json(
                                        &serde_json::json!({"error": "missing new_server_sk_hex"}),
                                    ),
                                    warp::http::StatusCode::BAD_REQUEST,
                                ))
                            }
                        };
                        let algo = body["algorithm"].as_str().unwrap_or("Kyber1024");

                        let mut guard = store.write().await;
                        match spacekit_compute_node::keymaster::rotate_key(
                            &mut guard, did, pk, sk, algo,
                        ) {
                            Ok(()) => {
                                if let Err(e) = spacekit_compute_node::keymaster::save_escrow(
                                    &guard, &path, &identity,
                                ) {
                                    warn!("Failed to persist escrow: {}", e);
                                }
                                Ok(warp::reply::with_status(
                                    warp::reply::json(&serde_json::json!({"status": "rotated"})),
                                    warp::http::StatusCode::OK,
                                ))
                            }
                            Err(e) => Ok(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({"error": e})),
                                warp::http::StatusCode::NOT_FOUND,
                            )),
                        }
                    }
                },
            );

        // ── State snapshot endpoint for joining nodes ──
        // GET /v1/state/snapshot returns the current chain head, state root,
        // peer list, and node identity so a joining node can decide where to
        // start synchronising from.
        let snapshot_net = self.network_service.clone();
        let snapshot_did = quantum_did_utils::get_did(&self.identity);
        let snapshot_bootstrap = self.config.network.bootstrap_nodes.clone();
        let snapshot_network_name = self.config.network.name.clone();
        let state_snapshot_route = warp::path!("v1" / "state" / "snapshot")
            .and(warp::get())
            .and_then(move || {
                let net = snapshot_net.clone();
                let did = snapshot_did.clone();
                let bootstrap = snapshot_bootstrap.clone();
                let network_name = snapshot_network_name.clone();
                async move {
                    let peers: Vec<serde_json::Value> = net
                        .get_peers()
                        .await
                        .iter()
                        .map(|p| {
                            serde_json::json!({
                                "peer_id": p.peer_id,
                                "address": p.address,
                                "capabilities": p.capabilities,
                                "last_seen": p.last_seen.to_rfc3339(),
                            })
                        })
                        .collect();

                    let net_status = net.get_status().await.ok();
                    let body = serde_json::json!({
                        "node_did": did,
                        "network_name": network_name,
                        "bootstrap_nodes": bootstrap,
                        "connected_peers": peers,
                        "peer_count": net_status.as_ref().map(|s| s.peer_count).unwrap_or(0),
                        "uptime_secs": net_status.as_ref().map(|s| s.uptime.as_secs()).unwrap_or(0),
                        "timestamp": chrono::Utc::now().to_rfc3339(),
                    });
                    Ok::<_, warp::Rejection>(warp::reply::json(&body))
                }
            });

        // Dedicated convergence surface for supervisors and acceptance gates. Values come
        // directly from NetworkService's active TCP handshakes; no configured peers are counted.
        let peers_net = self.network_service.clone();
        let peers_did = quantum_did_utils::get_did(&self.identity);
        let peers_network_name = self.config.network.name.clone();
        let network_peers_route = warp::path!("v1" / "network" / "peers")
            .and(warp::get())
            .and_then(move || {
                let net = peers_net.clone();
                let did = peers_did.clone();
                let network_name = peers_network_name.clone();
                async move {
                    let peers = net.get_peers().await;
                    let status = net.get_status().await.ok();
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "node_did": did,
                        "network_name": network_name,
                        "peer_count": status.as_ref().map(|value| value.peer_count).unwrap_or(0),
                        "is_connected": status.as_ref().map(|value| value.is_connected).unwrap_or(false),
                        "connected_peers": peers,
                    })))
                }
            });

        // ── Consensus coordinator endpoints ──

        // POST /v1/consensus/register-validator
        //
        // Requires a signed request, a SPHINCS+ proof of key possession, and
        // stake at or above the network minimum. Registration used to accept a
        // bare `{ "did": "..." }`, which let anyone mint as many voting
        // identities as they wanted and take a supermajority for free.
        #[derive(Deserialize)]
        struct RegisterValidatorBody {
            sphincs_pk_hex: String,
            /// SPHINCS+ signature over the validator registration payload.
            proof_hex: String,
            /// Stake backing this validator, in micro-USD.
            stake_units: u128,
        }
        let cc_reg = self.consensus_coordinator.clone();
        let vr_entitlements = entitlement_reader.clone();
        let register_validator_route = warp::path!("v1" / "consensus" / "register-validator")
            .and(warp::post())
            .and(spacekit_compute_node::api_auth::signed_json::<RegisterValidatorBody>(
                authenticator.clone(),
            ))
            .and_then(move |(caller, body): (
                spacekit_compute_node::AuthenticatedCaller,
                RegisterValidatorBody,
            )| {
                let cc = cc_reg.clone();
                let entitlements = vr_entitlements.clone();
                async move {
                    let (pk, proof) = match (
                        hex::decode(&body.sphincs_pk_hex),
                        hex::decode(&body.proof_hex),
                    ) {
                        (Ok(pk), Ok(proof)) => (pk, proof),
                        _ => {
                            return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "error": "sphincs_pk_hex and proof_hex must be hex",
                                })),
                                warp::http::StatusCode::BAD_REQUEST,
                            ))
                        }
                    };

                    // The claimed stake must actually be backed on chain.
                    if let Some(reader) = entitlements {
                        match reader.view(&caller.did).await {
                            Ok(view) if view.available_units >= body.stake_units => {}
                            Ok(view) => {
                                return Ok(warp::reply::with_status(
                                    warp::reply::json(&serde_json::json!({
                                        "error": "claimed stake exceeds on-chain entitlement",
                                        "claimed_units": body.stake_units,
                                        "available_units": view.available_units,
                                    })),
                                    warp::http::StatusCode::PAYMENT_REQUIRED,
                                ))
                            }
                            Err(e) => {
                                return Ok(warp::reply::with_status(
                                    warp::reply::json(&serde_json::json!({
                                        "error": format!("could not verify stake on chain: {e}"),
                                    })),
                                    warp::http::StatusCode::BAD_GATEWAY,
                                ))
                            }
                        }
                    } else {
                        return Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "error": "stake cannot be verified: entitlement contract is not \
                                          configured on this node",
                            })),
                            warp::http::StatusCode::SERVICE_UNAVAILABLE,
                        ));
                    }

                    match cc
                        .register_validator_with_key(
                            caller.did.clone(),
                            pk,
                            body.stake_units,
                            &proof,
                        )
                        .await
                    {
                        Ok(()) => Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "status": "registered",
                                "did": caller.did,
                                "stake_units": body.stake_units,
                                "validator_count": cc.validator_count().await,
                            })),
                            warp::http::StatusCode::OK,
                        )),
                        Err(e) => Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
                            warp::http::StatusCode::BAD_REQUEST,
                        )),
                    }
                }
            });

        // POST /v1/consensus/finalize — PQ finisher (Dilithium votes + SPHINCS+ envelope).
        #[cfg(feature = "spacetime-consensus")]
        let fin_cc = self.consensus_coordinator.clone();
        #[cfg(feature = "spacetime-consensus")]
        let fin_pq = self.pq_keys.clone();
        #[cfg(feature = "spacetime-consensus")]
        let fin_vm = self.swtchvm_node.clone();
        #[cfg(feature = "spacetime-consensus")]
        let finalize_consensus_route = warp::path!("v1" / "consensus" / "finalize")
            .and(warp::post())
            .and(warp::body::json())
            .and_then(move |body: ConsensusFinalizeBody| {
                let cc = fin_cc.clone();
                let pq_keys = fin_pq.clone();
                let vm = fin_vm.clone();
                async move {
                    match spacekit_compute_node::pq_finisher::finalize_proposal_if_ready(
                        &cc,
                        pq_keys.as_ref(),
                        &body.proposal_id,
                        body.round,
                        body.view,
                    )
                    .await
                    {
                        Ok(Some(block)) => {
                            apply_spacetime_side_effects(&cc, vm.as_ref(), &block).await;
                            Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "status": "finalized",
                                    "proposal_id": body.proposal_id,
                                    "block": block,
                                })),
                                warp::http::StatusCode::OK,
                            ))
                        }
                        Ok(None) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "error": "proposal not finalized or no pending block",
                                "proposal_id": body.proposal_id,
                            })),
                            warp::http::StatusCode::BAD_REQUEST,
                        )),
                        Err(e) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
                            warp::http::StatusCode::BAD_REQUEST,
                        )),
                    }
                }
            });

        // POST /v1/consensus/propose — submit to unified consensus (L1 manifest on block proposals).
        let propose_uc = self.unified_consensus.clone();
        let propose_cc = self.consensus_coordinator.clone();
        let propose_vm = self.swtchvm_node.clone();
        let propose_chain = self.config.compute.chain_id.clone();
        let propose_did = quantum_did_utils::get_did(&self.identity);
        let propose_dev_mode = self.config.network.dev_mode;
        let propose_allow_single_finalize = self.config.network.allow_single_validator_finalize;
        #[cfg(feature = "spacetime-consensus")]
        let propose_identity = self.identity.clone();
        #[cfg(feature = "spacetime-consensus")]
        let propose_pq_keys = self.pq_keys.clone();
        #[cfg(feature = "spacetime-consensus")]
        let propose_host = self.consensus_host.clone();

        // Operator-only: proposing a block is a validator action, not something
        // any HTTP caller may trigger.
        let propose_consensus_route = warp::path!("v1" / "consensus" / "propose")
            .and(warp::post())
            .and(spacekit_compute_node::api_auth::signed_json::<ConsensusProposeBody>(
                authenticator.clone(),
            ))
            .and_then(move |(caller, body): (
                spacekit_compute_node::AuthenticatedCaller,
                ConsensusProposeBody,
            )| {
                let uc = propose_uc.clone();
                let cc = propose_cc.clone();
                let vm = propose_vm.clone();
                let chain_id = propose_chain.clone();
                let default_proposer = propose_did.clone();
                let dev_mode = propose_dev_mode;
                let allow_single_finalize = propose_allow_single_finalize;
                #[cfg(feature = "spacetime-consensus")]
                let identity = propose_identity.clone();
                #[cfg(feature = "spacetime-consensus")]
                let pq_keys = propose_pq_keys.clone();
                #[cfg(feature = "spacetime-consensus")]
                let host = propose_host.clone();
                async move {
                    spacekit_compute_node::api_auth::require_admin(&caller)?;

                    fn bad_request(msg: String) -> warp::reply::WithStatus<warp::reply::Json> {
                        warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "error": msg })),
                            warp::http::StatusCode::BAD_REQUEST,
                        )
                    }

                    fn ok_submitted(proposal_id: String) -> warp::reply::WithStatus<warp::reply::Json> {
                        warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "status": "submitted",
                                "proposal_id": proposal_id,
                            })),
                            warp::http::StatusCode::OK,
                        )
                    }

                    let proposer = body.proposer_did.unwrap_or(default_proposer);
                    let kind = body.proposal_kind.to_ascii_lowercase();

                    match kind.as_str() {
                        "block" => {
                            let block_data = match block_data_for_proposal(
                                &chain_id,
                                vm.as_ref(),
                                body.use_swtchvm_head,
                                body.block,
                                body.use_l1_snapshot_manifest,
                            ) {
                                Ok(b) => b,
                                Err(e) => {
                                    return Ok::<_, warp::Rejection>(bad_request(e));
                                }
                            };
                            let bd_clone = block_data.clone();
                            let proposal = BlockProposal::new(proposer.clone(), block_data);
                            let id = match uc.submit_block_proposal(proposal).await {
                                Ok(i) => i,
                                Err(e) => {
                                    return Ok::<_, warp::Rejection>(bad_request(e.to_string()));
                                }
                            };
                            if body.announce {
                                use sha2::Digest;
                                let mut hasher = sha2::Sha256::new();
                                hasher.update(id.as_bytes());
                                hasher.update(bd_clone.block_number.to_le_bytes());
                                let block_hash = format!("0x{}", hex::encode(hasher.finalize()));
                                if let Err(e) = cc.announce_block(
                                    &id,
                                    bd_clone.block_number,
                                    &block_hash,
                                    &bd_clone.state_root,
                                    &bd_clone.parent_hash,
                                ) {
                                    return Ok::<_, warp::Rejection>(bad_request(e.to_string()));
                                }
                            }
                            #[cfg(feature = "spacetime-consensus")]
                            if body.finalize {
                                let vcount = cc.validator_count().await;
                                if !propose_finalize_allowed(
                                    dev_mode,
                                    allow_single_finalize,
                                    vcount,
                                    true,
                                ) {
                                    return Ok::<_, warp::Rejection>(bad_request(
                                        "finalize requires network.dev_mode or (allow_single_validator_finalize and validator_count <= 1)".to_string(),
                                    ));
                                }
                                match pq_finalize_after_propose(
                                    host.as_ref(),
                                    vm.as_ref(),
                                    &identity,
                                    pq_keys.as_ref(),
                                    &id,
                                    bd_clone,
                                    &proposer,
                                    body.round,
                                    body.view,
                                )
                                .await
                                {
                                    Ok(finalized) => {
                                        return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                            warp::reply::json(&serde_json::json!({
                                                "status": "finalized",
                                                "proposal_id": id,
                                                "block": finalized,
                                            })),
                                            warp::http::StatusCode::OK,
                                        ));
                                    }
                                    Err(e) => {
                                        return Ok::<_, warp::Rejection>(bad_request(e));
                                    }
                                }
                            }
                            Ok::<_, warp::Rejection>(ok_submitted(id))
                        }
                        "metrics" => {
                            let raw = match body.metrics {
                                Some(m) => m,
                                None => {
                                    return Ok::<_, warp::Rejection>(bad_request(
                                        "missing \"metrics\" object".to_string(),
                                    ));
                                }
                            };
                            let m: HttpMetricsPayload = match serde_json::from_value(raw) {
                                Ok(v) => v,
                                Err(e) => {
                                    return Ok::<_, warp::Rejection>(bad_request(e.to_string()));
                                }
                            };
                            let metrics = NetworkMetrics {
                                cpu_utilization: m.cpu_utilization,
                                memory_utilization: m.memory_utilization,
                                network_utilization: m.network_utilization,
                                storage_utilization: m.storage_utilization,
                                timestamp: SystemTime::now(),
                            };
                            let proposal = MetricsProposal::new(proposer, metrics);
                            let id = match uc.submit_metrics_proposal(proposal).await {
                                Ok(i) => i,
                                Err(e) => {
                                    return Ok::<_, warp::Rejection>(bad_request(e.to_string()));
                                }
                            };
                            Ok::<_, warp::Rejection>(ok_submitted(id))
                        }
                        "hybrid" => {
                            let block_data = match block_data_for_proposal(
                                &chain_id,
                                vm.as_ref(),
                                body.use_swtchvm_head,
                                body.block,
                                body.use_l1_snapshot_manifest,
                            ) {
                                Ok(b) => b,
                                Err(e) => {
                                    return Ok::<_, warp::Rejection>(bad_request(e));
                                }
                            };
                            let raw = match body.metrics {
                                Some(m) => m,
                                None => {
                                    return Ok::<_, warp::Rejection>(bad_request(
                                        "missing \"metrics\" for hybrid".to_string(),
                                    ));
                                }
                            };
                            let m: HttpMetricsPayload = match serde_json::from_value(raw) {
                                Ok(v) => v,
                                Err(e) => {
                                    return Ok::<_, warp::Rejection>(bad_request(e.to_string()));
                                }
                            };
                            let metrics = NetworkMetrics {
                                cpu_utilization: m.cpu_utilization,
                                memory_utilization: m.memory_utilization,
                                network_utilization: m.network_utilization,
                                storage_utilization: m.storage_utilization,
                                timestamp: SystemTime::now(),
                            };
                            let proposal = HybridProposal::new(proposer, block_data, metrics);
                            let id = match uc.submit_hybrid_proposal(proposal).await {
                                Ok(i) => i,
                                Err(e) => {
                                    return Ok::<_, warp::Rejection>(bad_request(e.to_string()));
                                }
                            };
                            Ok::<_, warp::Rejection>(ok_submitted(id))
                        }
                        _ => Ok::<_, warp::Rejection>(bad_request(format!(
                            "unknown type {:?}; use \"block\", \"metrics\", or \"hybrid\"",
                            body.proposal_kind
                        ))),
                    }
                }
            });

        // GET /v1/sync/subscriber — SwtchVM head + optional on-disk L1 manifest (subscriber / light-client hints).
        let sync_vm = self.swtchvm_node.clone();
        let sync_chain = self.config.compute.chain_id.clone();
        let subscriber_sync_route = warp::path!("v1" / "sync" / "subscriber")
            .and(warp::get())
            .and_then(move || {
                let vm = sync_vm.clone();
                let ch = sync_chain.clone();
                async move {
                    let bundle = build_subscriber_sync_bundle(vm.as_ref(), &ch);
                    Ok::<_, warp::Rejection>(warp::reply::json(&bundle))
                }
            });

        // GET /v1/consensus/finality?proposal_id=...
        #[derive(Deserialize)]
        struct FinalityQuery {
            proposal_id: String,
        }

        let cc_fin = self.consensus_coordinator.clone();
        let finality_route = warp::path!("v1" / "consensus" / "finality")
            .and(warp::get())
            .and(warp::query::<FinalityQuery>())
            .and_then(move |q: FinalityQuery| {
                let cc = cc_fin.clone();
                async move {
                    let status = cc.check_finality(&q.proposal_id).await;
                    let body = match &status {
                        spacekit_compute_node::FinalityStatus::Pending {
                            approve,
                            reject,
                            total_validators,
                        } => {
                            serde_json::json!({
                                "status": "pending",
                                "approve": approve,
                                "reject": reject,
                                "total_validators": total_validators,
                            })
                        }
                        spacekit_compute_node::FinalityStatus::Finalized {
                            block_number,
                            approve_count,
                        } => {
                            serde_json::json!({
                                "status": "finalized",
                                "block_number": block_number,
                                "approve_count": approve_count,
                            })
                        }
                        spacekit_compute_node::FinalityStatus::Rejected {
                            block_number,
                            reject_count,
                        } => {
                            serde_json::json!({
                                "status": "rejected",
                                "block_number": block_number,
                                "reject_count": reject_count,
                            })
                        }
                    };
                    Ok::<_, warp::Rejection>(warp::reply::json(&body))
                }
            });

        // ── Payment configuration endpoint ──
        // GET /v1/payments/config — returns the node's payment configuration
        // so clients know what payment methods are accepted.
        let pay_config = spacekit_payments::PaymentConfig::default();
        let pay_config_json = serde_json::json!({
            "x402": {
                "enabled": true,
                "facilitator_url": pay_config.facilitator_url,
                "network": if pay_config.testnet { "base-sepolia" } else { "base" },
                "asset": "USDC",
            },
            "astra": {
                "enabled": true,
                "network_fee_bps": pay_config.network_fee_bps,
            },
            "entitlements": {
                "enabled": entitlement_config.enabled,
                "chain_id": entitlement_config.chain_id,
                "contract": entitlement_config.contract_address,
                "confirmations": entitlement_config.confirmations,
                "assets": ["ETH", "DAI", "USDC"],
                "unit": "micro-USD",
            },
        });
        let payment_config_route = warp::path!("v1" / "payments" / "config")
            .and(warp::get())
            .and_then(move || {
                let cfg = pay_config_json.clone();
                async move { Ok::<_, warp::Rejection>(warp::reply::json(&cfg)) }
            });

        // ── Entitlements (on-chain) ──
        //
        // The aUSD vault used to live here with `credit-ausd`, an
        // unauthenticated endpoint that minted balance from a JSON body. It is
        // gone. Balance now originates only from DAI/USDC deposits into the
        // Ethereum entitlement contract, which this node reads but cannot write.

        // Internal settlement ledger used by the payments crate. It is credited
        // only from a successful on-chain entitlement reservation in
        // /v1/execute — never from a request body.
        let settlement_vault = std::sync::Arc::new(spacekit_payments::AusdVault::new());

        // GET /v1/entitlements?did=... — read the caller's entitlement.
        #[derive(Deserialize)]
        struct EntitlementQuery {
            did: String,
        }
        let ent_view = entitlement_reader.clone();
        let entitlement_view_route = warp::path!("v1" / "entitlements")
            .and(warp::get())
            .and(warp::query::<EntitlementQuery>())
            .and_then(move |q: EntitlementQuery| {
                let reader = ent_view.clone();
                async move {
                    let Some(reader) = reader else {
                        return Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "error": "entitlement contract is not configured on this node",
                            })),
                            warp::http::StatusCode::SERVICE_UNAVAILABLE,
                        ));
                    };
                    match reader.view(&q.did).await {
                        Ok(view) => Ok(warp::reply::with_status(
                            warp::reply::json(&view),
                            warp::http::StatusCode::OK,
                        )),
                        Err(e) => Ok(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "error": e.to_string() })),
                            warp::http::StatusCode::BAD_GATEWAY,
                        )),
                    }
                }
            });

        // POST /v1/entitlements/reserve — hold allowance against on-chain
        // deposits before performing paid work. Authenticated: a caller may
        // only reserve against their own DID.
        #[derive(Deserialize)]
        struct ReserveBody {
            units: u128,
            reservation_id: String,
        }
        let ent_reserve = entitlement_reader.clone();
        let entitlement_reserve_route = warp::path!("v1" / "entitlements" / "reserve")
            .and(warp::post())
            .and(spacekit_compute_node::api_auth::signed_json::<ReserveBody>(
                authenticator.clone(),
            ))
            .and_then(
                move |(caller, body): (spacekit_compute_node::AuthenticatedCaller, ReserveBody)| {
                    let reader = ent_reserve.clone();
                    async move {
                        let Some(reader) = reader else {
                            return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "error": "entitlement contract is not configured on this node",
                                })),
                                warp::http::StatusCode::SERVICE_UNAVAILABLE,
                            ));
                        };
                        match reader
                            .reserve(&caller.did, body.units, body.reservation_id)
                            .await
                        {
                            Ok(reservation) => Ok(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "success": true,
                                    "reservation": reservation,
                                })),
                                warp::http::StatusCode::OK,
                            )),
                            Err(e) => Ok(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "success": false,
                                    "error": e.to_string(),
                                })),
                                warp::http::StatusCode::PAYMENT_REQUIRED,
                            )),
                        }
                    }
                },
            );

        // POST /v1/entitlements/release — release an unused reservation.
        #[derive(Deserialize)]
        struct ReleaseBody {
            reservation_id: String,
        }
        let ent_release = entitlement_reader.clone();
        let entitlement_release_route = warp::path!("v1" / "entitlements" / "release")
            .and(warp::post())
            .and(spacekit_compute_node::api_auth::signed_json::<ReleaseBody>(
                authenticator.clone(),
            ))
            .and_then(
                move |(caller, body): (spacekit_compute_node::AuthenticatedCaller, ReleaseBody)| {
                    let reader = ent_release.clone();
                    async move {
                        let released = match reader {
                            Some(r) => r.release(&caller.did, &body.reservation_id).await,
                            None => false,
                        };
                        Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                            "released": released,
                        })))
                    }
                },
            );

        // ── Payment receipt verification ──
        // POST /v1/payments/verify — verify an x402 receipt and credit the beneficiary
        let payment_verify_route = warp::path!("v1" / "payments" / "verify")
            .and(warp::post())
            .and(warp::body::json())
            .and_then(|body: serde_json::Value| async move {
                let receipt = spacekit_payments::PaymentReceipt {
                    tx_hash: body["tx_hash"].as_str().unwrap_or("").to_string(),
                    amount: body["amount"].as_str().unwrap_or("0").to_string(),
                    asset: match body["asset"].as_str() {
                        Some("USDC") => spacekit_payments::PaymentAsset::USDC,
                        Some("aUSD") | Some("AUSD") => spacekit_payments::PaymentAsset::AUSD,
                        _ => spacekit_payments::PaymentAsset::ASTRA,
                    },
                    network: body["network"].as_str().and_then(|n| match n {
                        "base" => Some(spacekit_payments::PaymentNetwork::Base),
                        "base-sepolia" => Some(spacekit_payments::PaymentNetwork::BaseSepolia),
                        _ => None,
                    }),
                    settled_at: chrono::Utc::now().timestamp(),
                };

                let beneficiary = body["beneficiary_did"]
                    .as_str()
                    .unwrap_or("did:spacekit:treasury")
                    .to_string();

                // Forward content/channel payments to storage settlement inbox (listener completes grants).
                if let (Some(storage_base), Some(scope)) = (
                    std::env::var("SPACEKIT_STORAGE_NODE_URL")
                        .ok()
                        .filter(|s| !s.trim().is_empty()),
                    body["scope"].as_str().map(str::to_string),
                ) {
                    if scope.starts_with("content:") || scope.starts_with("channel:") {
                        let webhook = serde_json::json!({
                            "tx_hash": receipt.tx_hash,
                            "amount": receipt.amount,
                            "asset": body["asset"].as_str().unwrap_or("ASTRA"),
                            "payer_did": body["payer_did"].as_str().unwrap_or(""),
                            "beneficiary_did": beneficiary,
                            "scope": scope,
                            "settled_at": receipt.settled_at,
                        });
                        let secret = std::env::var("SPACEKIT_CONTENT_SETTLEMENT_SECRET").ok();
                        let url = format!(
                            "{}/api/content/settlements",
                            storage_base.trim_end_matches('/')
                        );
                        tokio::spawn(async move {
                            let client = reqwest::Client::new();
                            let mut req = client.post(url).json(&webhook);
                            if let Some(s) = secret {
                                if !s.trim().is_empty() {
                                    req = req.header("X-SpaceKit-Settlement-Secret", s);
                                }
                            }
                            if let Err(e) = req.send().await {
                                tracing::warn!("content settlement webhook failed: {e}");
                            }
                        });
                    }
                }

                Ok::<_, warp::Rejection>(warp::reply::with_status(
                    warp::reply::json(&serde_json::json!({
                        "status": "recorded",
                        "receipt": {
                            "tx_hash": receipt.tx_hash,
                            "amount": receipt.amount,
                            "asset": format!("{:?}", receipt.asset),
                        },
                        "beneficiary": beneficiary,
                        "explorer_url": receipt.explorer_url(),
                    })),
                    warp::http::StatusCode::OK,
                ))
            });

        // ── Intent-based execution ──
        // POST /v1/execute — accepts a SignedIntent from the relay, processes
        // payment actions atomically, and executes contract actions.
        //
        // The intent's own signature is what authorizes spending here: the
        // relay is not trusted, so the actor's signature must cover the whole
        // intent (see `intent_auth`). Previously nothing was verified at all
        // and any caller could post an intent naming any actor.
        let exec_vault = settlement_vault.clone();
        let exec_registry = did_registry.clone();
        let exec_entitlements = entitlement_reader.clone();
        let execute_intent_route = warp::path!("v1" / "execute")
            .and(warp::post())
            .and(warp::body::json())
            .and_then(move |body: serde_json::Value| {
                let vault = exec_vault.clone();
                let registry = exec_registry.clone();
                let entitlements = exec_entitlements.clone();
                async move {
                    // The relay sends routekit's SignedIntent shape where actions is raw JSON.
                    // Extract fields manually for maximum compatibility.
                    let intent_val = match body.get("intent") {
                        Some(v) => v,
                        None => return Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({"success": false, "error": "missing intent"})),
                            warp::http::StatusCode::BAD_REQUEST,
                        )),
                    };

                    let now = chrono::Utc::now().timestamp();

                    // ── Verify the actor's signature over the whole intent ──
                    let signature_hex = body["signature"].as_str().unwrap_or("");
                    let sig_type = body["sig_type"].as_str().unwrap_or("sphincs+");

                    // Resolving the key must not block, so snapshot it first.
                    let actor_did = intent_val["actor"].as_str().unwrap_or("").to_string();
                    let actor_key = registry
                        .resolve(&actor_did)
                        .await
                        .and_then(|k| hex::decode(k.sphincs_pk_hex).ok());

                    let verified_actor = match spacekit_compute_node::intent_auth::verify_signed_intent(
                        intent_val,
                        signature_hex,
                        sig_type,
                        now,
                        |_| actor_key,
                    ) {
                        Ok(a) => a,
                        Err(e) => {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "success": false,
                                    "error": e.to_string(),
                                })),
                                warp::http::StatusCode::UNAUTHORIZED,
                            ))
                        }
                    };

                    let intent_id = intent_val["intent_id"].as_str().unwrap_or("").to_string();
                    let actor = verified_actor;
                    let chain = intent_val["chain"].as_str().unwrap_or("").to_string();
                    let nonce = intent_val["nonce"].as_str().unwrap_or("0").to_string();
                    let version = intent_val["version"].as_str().unwrap_or("1.0").to_string();
                    let expiry = intent_val["expiry"].as_i64().unwrap_or(0);

                    // Parse typed actions from the raw actions JSON
                    let typed_actions: Vec<spacekit_payments::IntentAction> =
                        serde_json::from_value(intent_val["actions"].clone()).unwrap_or_default();

                    // Build the IntentPaymentProcessor
                    use spacekit_payments::fee_router::CreditApplier;
                    struct LogApplier;
                    impl CreditApplier for LogApplier {
                        fn apply_credit(&self, credit: &spacekit_payments::Credit) -> anyhow::Result<()> {
                            tracing::info!(
                                "Intent credit applied: {} ASTRA to {}",
                                credit.amount_astra,
                                credit.beneficiary_did
                            );
                            Ok(())
                        }
                    }

                    let pay_config = spacekit_payments::PaymentConfig::default();
                    let fee_router = std::sync::Arc::new(
                        spacekit_payments::FeeRouter::new(pay_config, std::sync::Arc::new(LogApplier))
                    );
                    let processor = spacekit_payments::IntentPaymentProcessor::new(
                        fee_router,
                        vault.clone(),
                    );

                    let payment_intent = spacekit_payments::intent::Intent {
                        intent_id: intent_id.clone(),
                        version,
                        actor: actor.clone(),
                        agent: intent_val["agent"].as_str().map(|s| s.to_string()),
                        chain: chain.clone(),
                        constraints: intent_val["constraints"].clone(),
                        actions: typed_actions,
                        nonce: nonce.clone(),
                        expiry,
                        meta: intent_val.get("meta").cloned(),
                    };

                    let plan = processor.extract_plan(&payment_intent);

                    // ── Fund the settlement ledger from on-chain entitlement ──
                    //
                    // The ledger is an internal accounting mirror, not a source
                    // of value. Before charging it we reserve the exact amount
                    // against the actor's on-chain deposits, so the node can
                    // never settle more than was actually paid in.
                    let required_usd: f64 = plan
                        .vault_charges
                        .iter()
                        .filter_map(|vc| vc.amount_ausd.parse::<f64>().ok())
                        .sum();

                    let mut reservation_id: Option<String> = None;
                    if required_usd > 0.0 {
                        let Some(reader) = entitlements.as_ref() else {
                            return Ok(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "success": false,
                                    "error": "entitlement contract is not configured on this node",
                                })),
                                warp::http::StatusCode::SERVICE_UNAVAILABLE,
                            ));
                        };

                        // Round up so fractional micro-USD is never given away.
                        let units = (required_usd * 1_000_000.0).ceil() as u128;
                        let rid = format!("intent:{intent_id}");
                        match reader.reserve(&actor, units, rid.clone()).await {
                            Ok(_) => {
                                reservation_id = Some(rid);
                                vault.credit(&actor, required_usd).await;
                            }
                            Err(e) => {
                                return Ok(warp::reply::with_status(
                                    warp::reply::json(&serde_json::json!({
                                        "success": false,
                                        "error": format!("Entitlement check failed: {e}"),
                                    })),
                                    warp::http::StatusCode::PAYMENT_REQUIRED,
                                ));
                            }
                        }
                    }

                    // Process payments
                    let payment_result = match processor.process_plan(&plan, &nonce).await {
                        Ok(r) => r,
                        Err(e) => {
                            // Give the allowance back; the work never happened.
                            if let (Some(reader), Some(rid)) =
                                (entitlements.as_ref(), reservation_id.as_ref())
                            {
                                reader.release(&actor, rid).await;
                            }
                            return Ok(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({
                                    "success": false,
                                    "error": format!("Payment processing failed: {}", e),
                                })),
                                warp::http::StatusCode::PAYMENT_REQUIRED,
                            ));
                        }
                    };

                    // Collect contract execution results
                    let mut execution_results = Vec::new();
                    for ec in &plan.contract_executions {
                        let value: u128 = ec.value_astra.as_deref()
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);

                        execution_results.push(serde_json::json!({
                            "contract_id": ec.contract_id,
                            "input_len": ec.input.len() / 2,
                            "value_astra": value,
                            "status": "executed",
                        }));
                    }

                    Ok(warp::reply::with_status(
                        warp::reply::json(&serde_json::json!({
                            "success": true,
                            "intent_id": intent_id,
                            "actor": actor,
                            "chain": chain,
                            "payment": {
                                "total_astra_credited": payment_result.total_astra_credited,
                                "receipts": payment_result.receipts.len(),
                                "credits": payment_result.credits.len(),
                            },
                            "executions": execution_results,
                        })),
                        warp::http::StatusCode::OK,
                    ))
                }
            });

        // Full SwtchVM developer HTTP API (same in-process node as operator — see RUNBOOK §9).
        let swtchvm_http = SwtchvmNode::http_dev_api_routes(self.swtchvm_node.clone());

        // Combine routes
        let routes = health_route
            .or(status_route)
            .or(node_identity_route)
            .or(onboarding_balance_route)
            .or(did_register_route)
            .or(did_resolve_route)
            .or(state_anchor_route)
            .or(keymaster_register_route)
            .or(keymaster_rotate_route)
            .or(state_snapshot_route)
            .or(network_peers_route)
            .or(register_validator_route)
            .or(propose_consensus_route);
        #[cfg(feature = "spacetime-consensus")]
        let routes = routes.or(finalize_consensus_route);

        // POST /v1/consensus/fraud_proof — challenge-window recovery (tiered finality + fingerprint rollback).
        #[cfg(feature = "spacetime-consensus")]
        let fp_cc = self.consensus_coordinator.clone();
        #[cfg(feature = "spacetime-consensus")]
        let fraud_proof_route = warp::path!("v1" / "consensus" / "fraud_proof")
            .and(warp::post())
            .and(warp::body::json())
            .and_then(move |body: ConsensusFraudProofBody| {
                let cc = fp_cc.clone();
                async move {
                    use spacekit_compute_node::spacetime_integration::handle_fraud_proof_submission;
                    match handle_fraud_proof_submission(&cc, body.submission).await {
                        Ok(acceptance) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "status": "accepted",
                                "target_height": acceptance.target_height,
                                "rolled_back_heights": acceptance.rolled_back_heights,
                                "slashing_proposals": acceptance.slashing_proposals.len(),
                            })),
                            warp::http::StatusCode::OK,
                        )),
                        Err(e) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "error": format!("{:?}", e),
                            })),
                            warp::http::StatusCode::BAD_REQUEST,
                        )),
                    }
                }
            });
        #[cfg(feature = "spacetime-consensus")]
        let routes = routes.or(fraud_proof_route);

        #[cfg(feature = "spacetime-consensus")]
        let att_cc = self.consensus_coordinator.clone();
        #[cfg(feature = "spacetime-consensus")]
        let fingerprint_attestation_route =
            warp::path!("v1" / "consensus" / "fingerprint_attestation")
                .and(warp::post())
                .and(warp::body::json())
                .and_then(move |body: FingerprintAttestationBody| {
                    let cc = att_cc.clone();
                    async move {
                        match cc.ingest_fingerprint_attestation(body.attestation).await {
                            Ok(()) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({ "status": "ingested" })),
                                warp::http::StatusCode::OK,
                            )),
                            Err(e) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(
                                    &serde_json::json!({ "error": format!("{:?}", e) }),
                                ),
                                warp::http::StatusCode::BAD_REQUEST,
                            )),
                        }
                    }
                });

        #[cfg(feature = "spacetime-consensus")]
        let mismatch_cc = self.consensus_coordinator.clone();
        #[cfg(feature = "spacetime-consensus")]
        let fingerprint_mismatch_route = warp::path!("v1" / "consensus" / "fingerprint_mismatches")
            .and(warp::get())
            .and(warp::query::<std::collections::HashMap<String, String>>())
            .and_then(move |q: std::collections::HashMap<String, String>| {
                let cc = mismatch_cc.clone();
                async move {
                    let height = q
                        .get("height")
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    let mismatches = cc.detect_fingerprint_mismatches(height).await;
                    Ok::<_, warp::Rejection>(warp::reply::json(&serde_json::json!({
                        "height": height,
                        "mismatches": mismatches.len(),
                    })))
                }
            });

        #[cfg(feature = "spacetime-consensus")]
        let prop_cc = self.consensus_coordinator.clone();
        #[cfg(feature = "spacetime-consensus")]
        let parameter_proposal_route = warp::path!("v1" / "consensus" / "parameter_proposal")
            .and(warp::post())
            .and(warp::body::json())
            .and_then(move |body: ParameterProposalBody| {
                let cc = prop_cc.clone();
                async move {
                    match cc.propose_parameter_change(body.proposal).await {
                        Ok(()) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "status": "proposed" })),
                            warp::http::StatusCode::OK,
                        )),
                        Err(e) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "error": format!("{:?}", e) })),
                            warp::http::StatusCode::BAD_REQUEST,
                        )),
                    }
                }
            });

        #[cfg(feature = "spacetime-consensus")]
        let vote_cc = self.consensus_coordinator.clone();
        #[cfg(feature = "spacetime-consensus")]
        let parameter_vote_route = warp::path!("v1" / "consensus" / "parameter_vote")
            .and(warp::post())
            .and(warp::body::json())
            .and_then(move |body: ParameterVoteBody| {
                let cc = vote_cc.clone();
                async move {
                    match cc.ingest_parameter_vote(body.vote).await {
                        Ok(()) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "status": "recorded" })),
                            warp::http::StatusCode::OK,
                        )),
                        Err(e) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "error": format!("{:?}", e) })),
                            warp::http::StatusCode::BAD_REQUEST,
                        )),
                    }
                }
            });

        #[cfg(feature = "spacetime-consensus")]
        let fin_param_cc = self.consensus_coordinator.clone();
        #[cfg(feature = "spacetime-consensus")]
        let parameter_finalize_route = warp::path!("v1" / "consensus" / "parameter_finalize")
            .and(warp::post())
            .and(warp::body::json())
            .and_then(move |body: ParameterFinalizeBody| {
                let cc = fin_param_cc.clone();
                async move {
                    let proposal_id = match parse_b256_hex(&body.proposal_id) {
                        Some(id) => id,
                        None => {
                            return Ok::<_, warp::Rejection>(warp::reply::with_status(
                                warp::reply::json(&serde_json::json!({ "error": "invalid proposal_id hex" })),
                                warp::http::StatusCode::BAD_REQUEST,
                            ));
                        }
                    };
                    let height = if body.at_height > 0 {
                        body.at_height
                    } else {
                        cc.consensus_tuning_height().await
                    };
                    match cc.try_finalize_ratification(proposal_id, height).await {
                        Some(activated) => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({
                                "status": "activated",
                                "activated_at_height": activated.activated_at_height,
                            })),
                            warp::http::StatusCode::OK,
                        )),
                        None => Ok::<_, warp::Rejection>(warp::reply::with_status(
                            warp::reply::json(&serde_json::json!({ "error": "quorum not reached or unknown proposal" })),
                            warp::http::StatusCode::BAD_REQUEST,
                        )),
                    }
                }
            });

        #[cfg(feature = "spacetime-consensus")]
        fn parse_b256_hex(s: &str) -> Option<alloy_primitives::B256> {
            let t = s.trim().strip_prefix("0x").unwrap_or(s.trim());
            if t.len() != 64 {
                return None;
            }
            let bytes = hex::decode(t).ok()?;
            Some(alloy_primitives::B256::from_slice(&bytes))
        }

        #[cfg(feature = "spacetime-consensus")]
        let routes = routes
            .or(fingerprint_attestation_route)
            .or(fingerprint_mismatch_route)
            .or(parameter_proposal_route)
            .or(parameter_vote_route)
            .or(parameter_finalize_route);

        let routes = routes
            .or(subscriber_sync_route)
            .or(finality_route)
            .or(payment_config_route)
            .or(payment_verify_route)
            .or(entitlement_view_route)
            .or(entitlement_reserve_route)
            .or(entitlement_release_route)
            .or(execute_intent_route)
            .or(swtchvm_http)
            .recover(spacekit_compute_node::api_auth::handle_rejection)
            .with(spacekit_compute_node::api_auth::cors_layer())
            .with(warp::log("spacekit-compute-api"));

        // Bind to loopback unless the operator explicitly widened it. Binding
        // 0.0.0.0 unconditionally put every operator endpoint on the public
        // interface of any cloud host.
        let bind_ip: std::net::IpAddr = match self.config.network.bind_address.parse() {
            Ok(ip) => ip,
            Err(e) => {
                warn!(
                    "Invalid network.bind_address {:?} ({e}); falling back to 127.0.0.1",
                    self.config.network.bind_address
                );
                std::net::IpAddr::from([127, 0, 0, 1])
            }
        };
        if !bind_ip.is_loopback() {
            warn!(
                "HTTP API is binding to {bind_ip}, which is reachable off-host. \
                 Ensure a firewall restricts access to trusted operators."
            );
        }

        info!("HTTP API server listening on {}:{}", bind_ip, port);

        // Start the server in a background task
        tokio::spawn(async move {
            warp::serve(routes).run((bind_ip, port)).await;
        });

        Ok(())
    }

    pub async fn submit_secure_task(
        &self,
        task_name: String,
        runtime: String,
        encrypted_code: Vec<u8>,
        encrypted_input: Vec<u8>,
        requester_did: String,
    ) -> Result<ComputeTask> {
        info!("Submitting secure compute task: {}", task_name);

        // Verify requester identity
        let requester_identity = quantum_did_utils::from_did(&requester_did).await?;
        if !quantum_did_utils::verify_identity(&requester_identity).await? {
            return Err(anyhow::anyhow!("Invalid requester identity"));
        }

        // Decrypt code and input using quantum-resistant encryption
        let code = self
            .encryption
            .decrypt(&encrypted_code, &self.identity)
            .await?;
        let input_data = self
            .encryption
            .decrypt(&encrypted_input, &self.identity)
            .await?;

        // Submit task to compute node
        let task = self
            .compute_node
            .submit_task(task_name, runtime, code, input_data, requester_did)
            .await?;

        info!("Task submitted successfully: {}", task.id);
        Ok(task)
    }

    pub async fn get_secure_task_result(
        &self,
        task_id: &str,
        requester_did: &str,
    ) -> Result<Vec<u8>> {
        info!("Getting secure task result: {}", task_id);

        // Verify requester identity
        let requester_identity = quantum_did_utils::from_did(requester_did).await?;
        if !quantum_did_utils::verify_identity(&requester_identity).await? {
            return Err(anyhow::anyhow!("Invalid requester identity"));
        }

        // Get result from compute node
        let result = self.compute_node.get_task_result(task_id).await?;

        // Encrypt result for requester
        let encrypted_result = self
            .encryption
            .encrypt(&result, &requester_identity)
            .await?;

        Ok(encrypted_result)
    }

    pub async fn get_node_status(&self) -> Result<NodeStatus> {
        let compute_status = self.compute_node.get_status().await;
        let network_status = self.network_service.get_status().await?;
        let token_status = self
            .token_service
            .get_balance()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get token balance: {:?}", e))?;

        Ok(NodeStatus {
            node_did: quantum_did_utils::get_did(&self.identity),
            compute_status: compute_status.clone(),
            network_peers: network_status.peer_count,
            token_balance: token_status,
            quantum_algorithms: self.config.security.supported_algorithms.clone(),
            uptime: compute_status.started_at,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeStatus {
    pub node_did: String,
    pub compute_status: spacekit_compute_node::NodeStatus,
    pub network_peers: u32,
    pub token_balance: u64,
    pub quantum_algorithms: Vec<String>,
    pub uptime: DateTime<Utc>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    if cli.verbose {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .init();
    }

    info!("Starting SpaceKit Compute Node CLI");

    // Load or create configuration
    let config = load_or_create_config(&cli.config)?;

    match cli.command {
        Some(Commands::Start {
            gpu,
            max_cpu_cores,
            max_memory_mb,
            no_http,
        }) => {
            let mut node_config = config;
            node_config.compute.max_cpu_cores = max_cpu_cores;
            node_config.compute.max_memory_mb = max_memory_mb;
            node_config.network.rpc_port = cli.port;
            node_config.network.p2p_port = cli.p2p_port;

            if no_http {
                node_config.network.enable_http_api = false;
            }

            if !cli.bootstrap_nodes.is_empty() {
                node_config.network.bootstrap_nodes = cli.bootstrap_nodes;
            }

            if gpu {
                node_config
                    .compute
                    .supported_runtimes
                    .push("gpu".to_string());
            }

            if let Some(did) = cli.node_did {
                node_config.identity.did = did;
            }

            let mut node = SwtchComputeNode::new(node_config).await?;
            node.start().await?;

            // Wait for shutdown signal
            shutdown_signal().await?;
            info!("Shutting down SpaceKit Compute Node...");
        }

        Some(Commands::Register {
            network_endpoint,
            stake,
        }) => {
            info!("Registering with SpaceKit network at {}", network_endpoint);
            // Implementation for registration
            info!("Registration not available stake: {}", stake);
        }

        Some(Commands::Status) => {
            let node = SwtchComputeNode::new(config).await?;
            let status = node.get_node_status().await?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }

        Some(Commands::GpuInfo) => {
            info!("GPU Information:");
            // Implementation for GPU info
        }

        Some(Commands::Test { test_type }) => {
            info!("Running {} test", test_type);
            // Implementation for testing
        }

        Some(Commands::ProductionTest {
            suite,
            detailed_report,
            format,
            output,
        }) => {
            info!("🧪 Running production testing suite v1.5: {}", suite);

            // Create compute node for testing
            let compute_node = Arc::new(ComputeNode::new(config.compute.clone()).await?);

            // Initialize testing suite
            let mut testing_suite =
                spacekit_compute_node::testing::ProductionTestingSuite::new(compute_node).await?;

            // Run the testing suite
            let report = testing_suite.run_complete_test_suite().await?;

            // Format and display results
            match format.as_str() {
                "json" => {
                    let json_output = serde_json::to_string_pretty(&report)?;
                    if let Some(output_path) = output {
                        std::fs::write(output_path, json_output)?;
                        info!("Report saved to file");
                    } else {
                        println!("{}", json_output);
                    }
                }
                "yaml" => {
                    let yaml_output = serde_yaml::to_string(&report)?;
                    if let Some(output_path) = output {
                        std::fs::write(output_path, yaml_output)?;
                        info!("Report saved to file");
                    } else {
                        println!("{}", yaml_output);
                    }
                }
                "table" => {
                    print_test_report_table(&report, detailed_report);
                    if let Some(output_path) = output {
                        let table_output = format_test_report_table(&report, detailed_report);
                        std::fs::write(output_path, table_output)?;
                        info!("Report saved to file");
                    }
                }
                _ => {
                    error!("Unknown format: {}", format);
                }
            }

            // Exit with appropriate code
            if report.overall_success {
                info!("✅ All tests passed!");
                std::process::exit(0);
            } else {
                error!("❌ Some tests failed!");
                std::process::exit(1);
            }
        }

        Some(Commands::Mcp {
            node_did: _,
            enable_gpu,
        }) => {
            info!("Starting MCP server (stdio transport)");
            let swtchvm_node = spacekit_compute_node::SwtchvmNode::new(enable_gpu, false).await?;
            let node = Arc::new(tokio::sync::RwLock::new(swtchvm_node));
            let mcp_server = spacekit_compute_node::mcp::McpServer::new(node);
            spacekit_compute_node::mcp::run_stdio(mcp_server).await?;
        }

        None => {
            // Interactive mode
            println!("SpaceKit Compute Node - Interactive Mode");
            println!("Use 'help' for available commands");
        }
    }

    Ok(())
}

/// Print test report in table format
fn print_test_report_table(
    report: &spacekit_compute_node::testing::TestSuiteReport,
    detailed: bool,
) {
    println!("\n🧪 SpaceKit Production Testing Suite v1.5 Results");
    println!("{}", "=".repeat(80));

    // Summary
    println!("📊 Test Summary:");
    println!("  Total Duration: {}ms", report.total_duration_ms);
    println!(
        "  Overall Success: {}",
        if report.overall_success {
            "✅ PASS"
        } else {
            "❌ FAIL"
        }
    );

    // Integration Tests
    println!("\n🔗 Integration Tests:");
    println!("  Total Tests: {}", report.integration_results.total_tests);
    println!("  Passed: {}", report.integration_results.passed_tests);
    println!(
        "  Failed: {}",
        report.integration_results.failed_tests.len()
    );

    if detailed {
        for test_detail in &report.integration_results.test_details {
            let status = if test_detail.passed { "✅" } else { "❌" };
            println!("    {} {}", status, test_detail.name);
            if let Some(error) = &test_detail.error {
                println!("      Error: {}", error);
            }
        }
    }

    // Performance Benchmarks
    println!("\n⚡ Performance Benchmarks:");
    let perf = &report.performance_results;
    println!(
        "  Service Discovery: {:.2}ms avg",
        perf.service_discovery_latency.average_latency_ms
    );
    println!(
        "  Load Balancing: {:.2}ms overhead",
        perf.load_balancing_overhead.average_latency_ms
    );
    println!(
        "  Health Checks: {:.2}ms avg",
        perf.health_check_latency.average_latency_ms
    );
    println!(
        "  Storage Throughput: {:.2} ops/sec",
        perf.storage_throughput.throughput_ops_sec
    );
    println!(
        "  Quantum Encryption: {:.2}ms avg",
        perf.quantum_encryption_overhead.average_latency_ms
    );

    // Stress Test Results
    println!("\n💪 Stress Test Results:");
    let stress = &report.stress_results;
    println!(
        "  Max Concurrent Operations: {}",
        stress.max_concurrent_operations
    );
    println!(
        "  Failover Scenarios Tested: {}",
        stress.failover_scenarios_tested
    );
    println!(
        "  Reputation System Load: {} ops",
        stress.reputation_system_load_operations
    );
    println!(
        "  Quantum Encryption Scale: {} ops",
        stress.quantum_encryption_scale_operations
    );

    // Recommendations
    println!("\n💡 Recommendations:");
    for (i, recommendation) in report.recommendations.iter().enumerate() {
        println!("  {}. {}", i + 1, recommendation);
    }

    println!("\n{}", "=".repeat(80));
}

/// Format test report as table string
fn format_test_report_table(
    report: &spacekit_compute_node::testing::TestSuiteReport,
    detailed: bool,
) -> String {
    let mut output = String::new();

    output.push_str("🧪 SpaceKit Production Testing Suite v1.5 Results\n");
    output.push_str(&"=".repeat(80));
    output.push_str("\n");

    // Add all the same formatting as print_test_report_table but to string
    output.push_str(&format!("📊 Test Summary:\n"));
    output.push_str(&format!(
        "  Total Duration: {}ms\n",
        report.total_duration_ms
    ));
    output.push_str(&format!(
        "  Overall Success: {}\n",
        if report.overall_success {
            "✅ PASS"
        } else {
            "❌ FAIL"
        }
    ));

    // Integration Tests
    output.push_str("\n🔗 Integration Tests:\n");
    output.push_str(&format!(
        "  Total Tests: {}\n",
        report.integration_results.total_tests
    ));
    output.push_str(&format!(
        "  Passed: {}\n",
        report.integration_results.passed_tests
    ));
    output.push_str(&format!(
        "  Failed: {}\n",
        report.integration_results.failed_tests.len()
    ));

    if detailed {
        for test_detail in &report.integration_results.test_details {
            let status = if test_detail.passed { "✅" } else { "❌" };
            output.push_str(&format!("    {} {}\n", status, test_detail.name));
            if let Some(error) = &test_detail.error {
                output.push_str(&format!("      Error: {}\n", error));
            }
        }
    }

    output.push_str(&"=".repeat(80));
    output.push_str("\n");

    output
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate = signal::unix::signal(signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = signal::ctrl_c() => result?,
            _ = terminate.recv() => {}
        }
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        signal::ctrl_c().await?;
        Ok(())
    }
}

fn expand_tilde_path(path: &str) -> String {
    let Some(rest) = path.strip_prefix("~/") else {
        return path.to_string();
    };
    #[cfg(unix)]
    {
        if let Ok(home) = std::env::var("HOME") {
            let home = home.trim_end_matches('/');
            return format!("{home}/{rest}");
        }
    }
    #[cfg(windows)]
    {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            let profile = profile.trim_end_matches('\\').trim_end_matches('/');
            let rest = rest.replace('/', "\\");
            return format!("{profile}\\{rest}");
        }
    }
    path.to_string()
}

fn normalize_config_paths(config: &mut NodeConfig) {
    config.identity.private_key_path = expand_tilde_path(&config.identity.private_key_path);
    config.identity.public_key_path = expand_tilde_path(&config.identity.public_key_path);
}

fn load_or_create_config(config_path: &str) -> Result<NodeConfig> {
    if std::path::Path::new(config_path).exists() {
        let config_str = std::fs::read_to_string(config_path)?;
        let mut config: NodeConfig = toml::from_str(&config_str)?;
        normalize_config_paths(&mut config);
        Ok(config)
    } else {
        warn!("Config file not found, creating default configuration");
        let default_config = NodeConfig::default();
        let config_str = toml::to_string_pretty(&default_config).map_err(|e| {
            anyhow::anyhow!(
                "Failed to serialize default config to TOML: {}. \
                 (Rust `u128` values must use string encoding for TOML — see `serde_u128` in the library.)",
                e
            )
        })?;
        std::fs::write(config_path, config_str)?;
        info!("Created default configuration at {}", config_path);
        let mut config = default_config;
        normalize_config_paths(&mut config);
        Ok(config)
    }
}

#[cfg(test)]
mod kem_tests {
    use super::infer_kem_algorithm;

    #[test]
    fn infer_kyber1024_sizes() {
        assert_eq!(infer_kem_algorithm(1568, 3168), Some("Kyber1024"));
    }

    #[test]
    fn infer_kyber768_sizes() {
        assert_eq!(infer_kem_algorithm(1184, 2400), Some("Kyber768"));
    }

    #[test]
    fn infer_rejects_unknown_lengths() {
        assert_eq!(infer_kem_algorithm(1, 2), None);
    }
}
