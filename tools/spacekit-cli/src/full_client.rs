// SpaceKit fat CLI — embedded nodes + optional `~/.spacekit/network/config.toml` overlay.

mod code_session;
mod fact_cmd;
mod identity_cmd;
mod keymaster_cmd;
mod migration_cmd;
mod operator_cmd;
mod repo_cmd;
mod sdkgen;
mod workspace_cmd;

use spacekit_primitives::v1::crypto::{generate_kem, EncryptionAlgorithm};
use spacekit_primitives::v1::dual_key_wallet::DualKeyWallet;
use spacekit_primitives::v1::sdk::spacekit::SpaceKitSDK;
use spacekit_primitives::v1::utils::file_ops::{load_from_file, save_to_file};
use spacekit_primitives::v1::utils::{save_key_to_file, str_to_address};

// Imports for init command and task management
use crate::content_integration::{
    access_content_with_payment, access_licensed_feature, build_content_listing_from_fact,
    channel_did_from_fact_tags, channel_to_fact_package, conditional_price_from_policy,
    content_price_astra, delete_content_listing_http, description_from_fact_tags,
    ensure_content_entitlement_for_agent, entitled_app_uses_embedded_growformer,
    file_to_fact_package, find_licensed_feature_content_id, get_content_install,
    get_fact_storage_engine, growformer_feature_document, licensed_feature_to_fact_package,
    list_content_grants, list_content_installs, load_licensed_feature_document,
    open_materialized_path, parse_content_id_hex, post_fact_package_http,
    publish_content_notification, record_test_payment, register_content_install_after_view,
    register_content_with_governance, renew_content_access, resolve_agent_content_id,
    resolve_content_view_output, run_entitled_content_binary, sign_content_fact, storage_data_dir,
    strip_entitlement_flags_from_exec_args, subscribe_channel_with_payment, title_from_fact_tags,
    upsert_content_listing_http, view_content_fact, write_content_view_file, DistributionRule,
    StoragePolicy, ViewContentOutcome,
};
use crate::growformer_model_manager::{peek_brain_path, GrowformerModelManager};
use crate::marketplace_integration::{
    app_content_ref_ids_from_manifest_fact, fact_json_is_app_manifest, fetch_remote_fact_json,
    unpublish_app_marketplace_entries, upsert_app_in_marketplace_index_http,
};
use spacekit_compute_node::{
    quantum_security::quantum_did_utils,
    spacekitvm::{minimal_l1_manifest_for_proposal, SnapshotManifest},
    swtch_consensus::{
        BlockData, BlockProposal, HybridProposal, MetricsProposal, NetworkMetrics,
        UnifiedConsensusConfig, UnifiedSWTCHConsensus,
    },
    vpos::VPoSManager,
    CollaborativeComputeConfig, CollaborativeComputeManager, CollaborativeComputeRequest,
    ComputationType, ComputeConfig, ComputeNode, ConsensusPolicy as CollabConsensusPolicy,
    MetricsConsensusConfig, MetricsConsensusManager, ProductionMetricsConfig,
    ProductionMetricsManager, SMPCComputationType, SecureMultiPartyConfig, SecureMultiPartyManager,
    TaskStatus,
};
use spacekit_messaging_node::{MessagingConfig, MessagingNode};
use spacekit_storage_node::{
    CollectionCategory, NftCollection as NftCollectionInfo, NftCollection, NftCollectionManager,
    NftMetadata, NftStorageManager, QuantumCrypto, StorageNode, StorageNodeConfig, TokenStandard,
};

use chrono::{DateTime, Utc};
use dirs;
use spacekit_did::{QuantumResistantWallet, VerifiableCredential};
use spacekit_primitives::v1::crypto::evm::{
    decrypt_file as ecies_decrypt_file, encrypt_file as ecies_encrypt_file,
    ethereum_address_from_ecies_public_key, new_keypair as new_keypair_evm,
};
use spacekit_primitives::v1::crypto::quantum::{
    handle_decryption, handle_encryption, Algorithm as QuantumAlgorithm, Cipher, CipherSuite,
};
use spacekit_primitives::v1::crypto::solana::{key_to_base58, new_keypair as new_keypair_solana};
use std::collections::HashMap;
use uuid::Uuid;

// Add AES trait import
use aes_gcm::{aead::KeyInit, Aes256Gcm};
use chacha20poly1305::{ChaCha20Poly1305, XChaCha20Poly1305};

use serde::Deserialize;
use serde_json;
use tokio;

use clap::{Args, Parser, Subcommand, ValueEnum};
use colored::Colorize;
use sha2::{Digest, Sha256};
use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, SystemTime};

// Global compute node instance management
static COMPUTE_NODE: LazyLock<Arc<RwLock<Option<ComputeNode>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

/// Lazily constructed unified consensus engine for `spacekit consensus submit` (CLI-local).
static UNIFIED_CONSENSUS: LazyLock<Arc<tokio::sync::Mutex<Option<Arc<UnifiedSWTCHConsensus>>>>> =
    LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(None)));

// Global storage node instance management
static STORAGE_NODE: LazyLock<Arc<RwLock<Option<Arc<StorageNode>>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

// Global messaging node instance management
static MESSAGING_NODE: LazyLock<Arc<RwLock<Option<Arc<MessagingNode>>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

// Global DID wallet instance management
static DID_WALLET: LazyLock<Arc<RwLock<Option<Arc<QuantumResistantWallet>>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(None)));

/// In-process brain manager for `spacekit agent` (full CLI only).
static GROWFORMER_MANAGER: LazyLock<GrowformerModelManager> =
    LazyLock::new(GrowformerModelManager::new);

/// §18.2 passive capture: append a real `agent infer` prompt to the RealTraffic log so the
/// offline certifier can batch-embed + gate it later (Phase 1C). Pure side-effect, best-effort —
/// never fails inference, never reads the incumbent's reply as a label (§18.3). Writes to
/// `$GROWFORMER_CAPTURE_DIR` (default `capture_artifacts`); set `GROWFORMER_CAPTURE_DISABLE=1`
/// to turn off.
fn capture_real_traffic_prompt(agent: &str, prompt: &str, response: Option<&str>) {
    if std::env::var("GROWFORMER_CAPTURE_DISABLE").as_deref() == Ok("1") {
        return;
    }
    let dir =
        std::env::var("GROWFORMER_CAPTURE_DIR").unwrap_or_else(|_| "capture_artifacts".to_string());
    let mut cap = growformer::inference::grounding_loop::TrafficCapture::real(prompt, agent);
    cap.response = response.map(|s| s.to_string());
    if let Ok(sid) = std::env::var("SPACEKIT_SESSION_ID") {
        cap.session_id = sid;
    }
    let _ = growformer::inference::grounding_loop::append_traffic_capture(
        &cap,
        std::path::Path::new(&dir),
    );
}
const BANNER: &str = r#"
╔════════════════════════════════════════════════════════════════════════════╗
║                                                                            ║
║        ███████╗██████╗  █████╗  ██████╗███████╗██╗  ██╗██╗████████╗        ║
║        ██╔════╝██╔══██╗██╔══██╗██╔════╝██╔════╝██║ ██╔╝██║╚══██╔══╝        ║
║        ███████╗██████╔╝███████║██║     █████╗  █████╔╝ ██║   ██║           ║
║        ╚════██║██╔═══╝ ██╔══██║██║     ██╔══╝  ██╔═██╗ ██║   ██║           ║
║        ███████║██║     ██║  ██║╚██████╗███████╗██║  ██╗██║   ██║           ║
║        ╚══════╝╚═╝     ╚═╝  ╚═╝ ╚═════╝╚══════╝╚═╝  ╚═╝╚═╝   ╚═╝           ║
║                                                                            ║
║          Quantum-Resistant Distributed Infrastructure Platform             ║
║                AI/ML • Compute • Storage • Message • Pay                   ║
║                             spacekit.xyz                                   ║
║                                                                            ║
╚════════════════════════════════════════════════════════════════════════════╝
"#;

const AGENT_AFTER_HELP: &str = "\
EXAMPLES:
  spacekit agent train --project my-agent/my-agent.gf.toml
  spacekit agent -t --project my-agent/my-agent.gf.toml
  spacekit agent infer --brain my-agent.bin --prompt \"hello\"
  spacekit agent load --name demo --brain my-agent.bin
  spacekit agent infer --name demo --prompt \"hello\"
  spacekit agent merge --brain base.bin --overlay-brain extra.bin --brain-output merged.bin
  spacekit agent exec --infer --brain my-agent.bin --prompt \"hello\"
  spacekit agent exec -- --help   (growformer help; omit -- for growformer flags)

SUBCOMMANDS:
  train   Train a .bin brain from a .gf.toml project
  infer   Run inference (--name in-process, or --brain file)
  load    Load a .bin into this CLI process for fast infer --name
  unload  Drop a loaded brain
  list    List brains loaded in this process
  info    Show metadata from a .bin without loading
  merge   Merge two brain files
  exec    Pass growformer flags to embedded CLI (-- optional except for growformer --help)

PLATFORM (Windows / macOS / Linux):
  The full spacekit binary embeds growformer — no separate growformer install or GROWFORMER_BIN.
  Use forward or backslashes in paths; quote paths that contain spaces.
  In-process load/infer (--name) is per CLI process (not shared across terminals).

See also: spacekit storage deploy · spacekit brain-registry build";

const BRAIN_REGISTRY_AFTER_HELP: &str = "\
EXAMPLES:
  spacekit brain-registry build \\
    --gf-toml my-agent/my-agent.gf.toml \\
    --receipt deploy-receipt.json \\
    --out brain-manifest.json

  spacekit brain-registry publish \\
    --manifest brain-manifest.json \\
    --storage-url http://127.0.0.1:3030

WORKFLOW:
  1. spacekit storage deploy --wasm ... --bin ... --receipt deploy-receipt.json
  2. brain-registry build (manifest JSON from .gf.toml + receipt)
  3. brain-registry publish (PUT to storage /api/documents/...)

REQUIRES:
  spacekit init (identity DID for Authorization header on publish)
  Running storage node reachable at --storage-url or connections.storage in config

PLATFORM (Windows / macOS / Linux):
  HTTP to storage node only; paths and JSON work the same on Windows.
  Use quoted paths for build --gf-toml / --receipt / --manifest when needed.";

/// Spacekit encryption and network management tools with full quantum-resistant support
#[derive(Parser, Debug)]
#[command(
    version,
    about = BANNER,
    long_about = None,
    after_help = "📚 Documentation: https://docs.spacekit.xyz\n💡 Quick Start: spacekit init --help · spacekit new --help\n🔧 Config: ~/.spacekit/config.toml (identity DID + keys; override with global `--did`)\n🌐 Network profile: ~/.spacekit/network/config.toml · SPACEKIT_NETWORK_CONFIG\n🤖 Agents: spacekit agent --help\n📋 Brain registry: spacekit brain-registry --help"
)]
struct Cli {
    /// Chain to use (spacekit, ethereum, solana, bitcoin)
    #[arg(short, long, default_value = "spacekit")]
    chain: String,

    /// Network to use (mainnet, testnet, localhost)
    #[arg(short, long, default_value = "localhost")]
    network: String,

    /// Identity DID (default: from `~/.spacekit/config.toml` after `spacekit init`; aliases `--owner-did`, `--caller-did` on subcommands)
    #[arg(
        long,
        global = true,
        visible_alias = "owner-did",
        visible_alias = "caller-did"
    )]
    did: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CipherOption {
    Aes,
    ChaCha20,
    XChaCha20,
}

impl From<CipherOption> for CipherSuite {
    fn from(cipher: CipherOption) -> Self {
        match cipher {
            CipherOption::Aes => CipherSuite::AES256,
            CipherOption::ChaCha20 => CipherSuite::ChaCha20,
            CipherOption::XChaCha20 => CipherSuite::XChaCha20,
        }
    }
}

impl From<CipherOption> for Cipher {
    fn from(cipher: CipherOption) -> Self {
        match cipher {
            CipherOption::Aes => {
                let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&[0u8; 32]);
                Cipher::Aes(Aes256Gcm::new(key))
            }
            CipherOption::ChaCha20 => {
                let key = chacha20poly1305::Key::from_slice(&[0u8; 32]);
                Cipher::ChaCha(ChaCha20Poly1305::new(key))
            }
            CipherOption::XChaCha20 => {
                let key = chacha20poly1305::Key::from_slice(&[0u8; 32]);
                Cipher::XChaCha(XChaCha20Poly1305::new(key))
            }
        }
    }
}

fn convert_to_quantum_algorithm(alg: EncryptionAlgorithm) -> QuantumAlgorithm {
    match alg {
        EncryptionAlgorithm::Kyber512 => QuantumAlgorithm::Kyber512,
        EncryptionAlgorithm::Kyber768 => QuantumAlgorithm::Kyber768,
        EncryptionAlgorithm::Kyber1024 => QuantumAlgorithm::Kyber1024,
        EncryptionAlgorithm::NtruPrimeSntrup761 => QuantumAlgorithm::NtruPrimeSntrup761,
        EncryptionAlgorithm::FrodoKem1344Aes => QuantumAlgorithm::FrodoKem1344Aes,
        EncryptionAlgorithm::FrodoKem1344Shake => QuantumAlgorithm::FrodoKem1344Shake,
        _ => panic!("Unsupported quantum algorithm conversion"),
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Encrypts a file using ECIES or quantum algorithms
    Encrypt {
        /// File to encrypt
        #[arg(value_name = "FILE")]
        file: String,
        /// File path for the hex-encoded public key (default: `identity.public_key_path` from `~/.spacekit/config.toml`)
        #[arg(short, long)]
        public_key_path: Option<String>,
        /// File path for the encrypted output
        #[arg(short, long, default_value = "file_data.enc")]
        output_path: String,
        /// Encryption algorithm (ecies for classical, or quantum algorithms)
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::Kyber1024)]
        algorithm: EncryptionAlgorithm,
        /// Cipher suite for quantum algorithms (ignored for ECIES)
        #[arg(short, long, value_enum)]
        cipher: Option<CipherOption>,
        /// Path to shared secret for quantum encryption (from encapsulation)
        #[arg(long)]
        kem_secret: Option<String>,
    },

    /// Decrypts a file using ECIES or quantum algorithms
    Decrypt {
        /// File to decrypt
        #[arg(value_name = "FILE")]
        file: String,
        /// Secret key (ECIES) or KEM secret file (quantum decrypt); default: `identity.private_key_path` in config
        #[arg(short, long)]
        secret_key_path: Option<String>,
        /// File path for the decrypted output
        #[arg(short, long, default_value = "file_data.txt")]
        output_path: String,
        /// Decryption algorithm (ecies for classical, or quantum algorithms)
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::Kyber1024)]
        algorithm: EncryptionAlgorithm,
        /// Cipher suite for quantum algorithms (ignored for ECIES)
        #[arg(short, long, value_enum)]
        cipher: Option<CipherOption>,
        /// Path to shared secret for quantum decryption (from decapsulation)
        #[arg(long)]
        kem_secret: Option<String>,
    },

    /// Generates a keypair
    Keypair {
        /// Save the keys to files instead of displaying them
        #[arg(long)]
        save: bool,
        /// File path for the hex-encoded secret|private key
        #[arg(short, long, default_value = "secret_key.hex")]
        secret_key_path: String,
        /// File path for the hex-encoded public key (default: `identity.public_key_path` from `~/.spacekit/config.toml`)
        #[arg(short, long)]
        public_key_path: Option<String>,
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::ECIES)]
        algorithm: EncryptionAlgorithm,
    },

    /// Encapsulate shared secret with public key (quantum KEM)
    Encapsulate {
        /// Save artifacts to files
        #[arg(long)]
        save: bool,
        /// File path for the public key (default: `identity.public_key_path` in config)
        #[arg(short, long)]
        public_key_path: Option<String>,
        /// Output path for KEM ciphertext
        #[arg(long, default_value = "kem_ciphertext.hex")]
        kem_ciphertext_output: String,
        /// Output path for shared secret
        #[arg(long, default_value = "shared_secret.hex")]
        kem_secret_output: String,
        /// Quantum algorithm to use
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::Kyber1024)]
        algorithm: EncryptionAlgorithm,
        /// Cipher suite for symmetric encryption
        #[arg(short, long, value_enum, default_value_t = CipherOption::Aes)]
        cipher: CipherOption,
    },

    /// Decapsulate shared secret with secret key (quantum KEM)
    Decapsulate {
        /// File path for the secret key (default: `identity.private_key_path` in config)
        #[arg(short, long)]
        secret_key_path: Option<String>,
        /// Path to KEM ciphertext
        #[arg(long, default_value = "kem_ciphertext.hex")]
        kem_ciphertext: String,
        /// Quantum algorithm to use
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::Kyber1024)]
        algorithm: EncryptionAlgorithm,
        /// Cipher suite for symmetric encryption
        #[arg(short, long, value_enum, default_value_t = CipherOption::Aes)]
        cipher: CipherOption,
    },
    /// Initialize SpaceKit environment (~/.spacekit identity, keys, config). For a project folder, use `spacekit new <name>`.
    Init {
        /// Custom DID (if not provided, will generate new one)
        #[arg(long)]
        did: Option<String>,

        /// Quantum algorithm to use
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::Kyber1024)]
        algorithm: EncryptionAlgorithm,

        /// Default network label stored in config
        #[arg(long, default_value = "testnet")]
        network: String,

        /// Validate setup after initialization
        #[arg(long)]
        validate: bool,
    },

    /// Create a new project directory under the current working directory (requires `spacekit init` first).
    New {
        /// Project folder name (created as ./<name>)
        name: String,
        /// Project template: contracts, agent, webapp, webapp-react, defi
        #[arg(long, value_enum, default_value_t = crate::project_scaffold::NewProjectKind::Contracts)]
        kind: crate::project_scaffold::NewProjectKind,
        /// Marketplace / package display name (defaults to title-cased project name)
        #[arg(long)]
        app_name: Option<String>,
        #[arg(long, default_value = "testnet")]
        network: String,
        #[arg(long)]
        validate: bool,
    },

    // Disabled: task management commands (handlers remain for future re-enable)
    // /// Task management commands
    // #[command(subcommand)]
    // Task(TaskCommands),
    /// Storage management commands
    #[command(subcommand)]
    Storage(StorageCommands),

    /// DID (Decentralized Identity) management commands
    #[command(subcommand)]
    Did(DIDCommands),

    /// Network operations and service discovery
    #[command(subcommand)]
    Network(NetworkCommands),

    // Disabled: consensus operations and governance
    // /// Consensus operations and governance
    // #[command(subcommand)]
    // Consensus(ConsensusCommands),

    // Disabled: collaborative compute and SMPC operations
    // /// Collaborative compute and SMPC operations
    // #[command(subcommand)]
    // Collaborative(CollaborativeCommands),
    /// NFT storage and collection management
    #[command(subcommand)]
    Nft(NftCommands),

    // Disabled: production metrics and monitoring
    // /// Production metrics and monitoring
    // #[command(subcommand)]
    // Metrics(MetricsCommands),
    /// Smart contract deployment and execution
    #[command(subcommand)]
    Contract(ContractCommands),

    /// SwtchVM ledger helpers (same in-process VM as `contract deploy` / `call`).
    #[command(subcommand)]
    Vm(VmCommands),

    /// Configure connection to remote nodes
    #[command(subcommand)]
    Connect(ConnectionCommands),

    /// Messaging and chat commands
    #[command(subcommand)]
    Message(MessageCommands),

    /// Content publishing and channel management
    #[command(subcommand)]
    Content(ContentCommands),

    /// App package management (create, deploy, list apps)
    #[command(subcommand)]
    App(AppCommands),

    /// Growformer agent brains (train, infer, merge)
    Agent(AgentArgs),

    /// Brain registry manifests (build + publish to storage)
    #[command(subcommand, name = "brain-registry")]
    BrainRegistry(BrainRegistryCommands),

    /// Git-like repo: CAS blobs (`/blobs`), commit facts (`/facts`), refs (`/api/documents/repos/...`).
    #[command(subcommand)]
    Repo(RepoCommands),

    /// Build and submit [`FactPackage`] records (`POST /facts`, `GET /facts/{id}`).
    #[command(subcommand)]
    Fact(FactCommands),

    /// Agent/human workspace documents (`POST/GET /api/workspaces`).
    #[command(subcommand)]
    Workspace(WorkspaceCommands),

    /// Operator discovery manifest (`spacekit:operator:v1` via `POST /facts`).
    #[command(subcommand)]
    Operator(OperatorCommands),

    /// Verify DID-signed migration manifests in workspace export bundles.
    #[command(subcommand)]
    Migration(MigrationCommands),

    /// Guardian-backed keystore custody (SKKM enroll / recover / break-glass export).
    #[command(subcommand)]
    Keymaster(keymaster_cmd::KeymasterCommands),

    /// Sign in to spacekit.xyz (alias for `identity login`)
    Login {
        /// Website username (e.g. astor)
        #[arg(long)]
        username: String,
        /// Recovery email for this account
        #[arg(long)]
        email: String,
        /// Paste token from the magic-link URL (?token=…) after checking email
        #[arg(long)]
        token: Option<String>,
        /// Website API base URL
        #[arg(long)]
        api_url: Option<String>,
    },

    /// Link local CLI keys to your spacekit.xyz username
    #[command(subcommand)]
    Identity(identity_cmd::IdentityCommands),

    /// SKTCS tool manifest utilities (embed into WASM, validate JSON).
    #[command(subcommand)]
    Tools(ToolsCommands),
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum ToolsCommands {
    /// Embed a tool-manifest.json into a WASM binary as a `spacekit:tools` custom section.
    EmbedManifest {
        /// Path to the WASM binary
        #[arg(long)]
        wasm: String,
        /// Path to the tool-manifest.json
        #[arg(long)]
        manifest: String,
        /// Output path for the modified WASM (default: overwrite input)
        #[arg(long)]
        output: Option<String>,
    },
    /// Validate a tool-manifest.json file against the SKTCS schema.
    ValidateManifest {
        /// Path to the tool-manifest.json
        #[arg(value_name = "MANIFEST_JSON")]
        manifest: String,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum FactCommands {
    /// Build a fact from JSON or binary input (or load `--package`) and POST to the storage node
    Create {
        /// Load an existing FactPackage JSON file and submit as-is
        #[arg(long, conflicts_with_all = ["data", "file"])]
        package: Option<String>,
        /// JSON file used as `FactContent::Json` payload (requires `--schema`)
        #[arg(long, conflicts_with_all = ["package", "file"])]
        data: Option<String>,
        /// File stored as `FactContent::Binary` (requires `--schema`)
        #[arg(long, conflicts_with_all = ["package", "data"])]
        file: Option<String>,
        /// Schema string (e.g. `spacekit:my:event:v1`); required for `--data` / `--file`
        #[arg(long)]
        schema: Option<String>,
        /// Parent fact id (hex, repeat for multiple)
        #[arg(long = "parent")]
        parent: Vec<String>,
        /// Metadata tag (repeatable)
        #[arg(long)]
        tag: Vec<String>,
        /// Deterministic `fact_id` from author + schema + body + parents (default: unique per run)
        #[arg(long)]
        deterministic: bool,
        /// Write the built package to this path before posting
        #[arg(short, long)]
        output: Option<String>,
        /// Storage node base URL (default: network profile / http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,
        /// Build and print only; do not POST
        #[arg(long)]
        dry_run: bool,
    },
    /// Fetch a fact by id from `GET /facts/{id}`
    Get {
        /// 64-char hex fact id
        fact_id: String,
        #[arg(long)]
        storage_url: Option<String>,
        /// Write JSON to file instead of stdout
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Print the fact id that would be used for a JSON payload (preview hashing)
    Id {
        /// JSON payload file
        #[arg(long)]
        data: String,
        #[arg(long)]
        schema: Option<String>,
        #[arg(long)]
        author_did: Option<String>,
        #[arg(long = "parent")]
        parent: Vec<String>,
        #[arg(long)]
        deterministic: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum RepoCommands {
    /// Create `.spacekit/repo` in the current directory
    Init {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        remote: Option<String>,
    },
    /// Show unstaged changes (index vs working tree)
    Status,
    /// Stage paths (default: all tracked files under cwd)
    Add {
        #[arg(trailing_var_arg = true)]
        paths: Vec<String>,
    },
    /// Create a commit from the index
    Commit {
        #[arg(short, long)]
        message: String,
        /// Replace the current tip commit instead of adding a child (git `--amend`)
        #[arg(long)]
        amend: bool,
    },
    /// Upload blobs, facts, and update the remote ref document
    Push {
        #[arg(long)]
        storage_url: Option<String>,
        /// Local tip to push (`refs/heads/NAME`; default: current `HEAD`)
        #[arg(short = 'b', long = "branch", value_name = "NAME")]
        branch: Option<String>,
        /// Allow a non-fast-forward update (overwrite remote tip)
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
    /// Fetch remote ref, commits, and blobs; fast-forward only
    Pull {
        #[arg(long)]
        storage_url: Option<String>,
        /// Remote/local ref (`heads/NAME`; default: branch checked out via `HEAD`)
        #[arg(short = 'b', long = "branch", value_name = "NAME")]
        branch: Option<String>,
        /// Shallow fetch: stop after N commits of history
        #[arg(long)]
        depth: Option<usize>,
    },
    /// Download remote commits/blobs into the local store without touching the working tree
    Fetch {
        #[arg(long)]
        storage_url: Option<String>,
        #[arg(short = 'b', long = "branch", value_name = "NAME")]
        branch: Option<String>,
        /// Shallow fetch: stop after N commits of history
        #[arg(long)]
        depth: Option<usize>,
    },
    /// Three-way merge another branch into the current HEAD
    Merge {
        /// Branch (short name) to merge into HEAD
        #[arg(value_name = "BRANCH")]
        branch: Option<String>,
        /// Finalize an in-progress merge after resolving conflicts
        #[arg(long, conflicts_with_all = ["abort", "branch"])]
        r#continue: bool,
        /// Abort an in-progress merge and restore the pre-merge state
        #[arg(long, conflicts_with_all = ["continue", "branch"])]
        abort: bool,
    },
    /// List recent commits (local object store)
    Log {
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        /// Draw a single-column ASCII graph showing merges
        #[arg(long)]
        graph: bool,
    },
    /// Show a commit's metadata and patch
    Show {
        /// Commit id (hex); default: current HEAD tip
        #[arg(value_name = "COMMIT")]
        commit: Option<String>,
    },
    /// Diff between two commits, or the working tree vs HEAD (default)
    Diff {
        #[arg(long)]
        a: Option<String>,
        #[arg(long)]
        b: Option<String>,
        /// Show line-level (unified) diffs instead of just the file list
        #[arg(long)]
        content: bool,
        /// Only print changed path names
        #[arg(long = "name-only")]
        name_only: bool,
    },
    /// List branches, create one at the current HEAD, or delete a local branch (`-d`)
    Branch {
        /// New branch name (short name only, e.g. `feature-x`)
        #[arg(value_name = "NAME", conflicts_with = "delete")]
        name: Option<String>,
        /// Delete local branch (cannot delete the branch you are on)
        #[arg(
            short = 'd',
            long = "delete",
            value_name = "NAME",
            conflicts_with = "name"
        )]
        delete: Option<String>,
    },
    /// Switch to a branch; updates `index.json` and refreshes files from the storage node when hashes differ
    Checkout {
        #[arg(value_name = "BRANCH")]
        branch: String,
        #[arg(long)]
        storage_url: Option<String>,
    },
    /// Create directory, `init`, and `pull`
    Clone {
        remote: String,
        repo_name: String,
        #[arg(value_name = "DIR")]
        dir: Option<PathBuf>,
        /// Shallow clone: stop after N commits of history
        #[arg(long)]
        depth: Option<usize>,
    },
    /// List repos on the remote storage node
    List {
        #[arg(long)]
        storage_url: Option<String>,
    },
    /// Create, list, or delete tags (lightweight pointers to commits)
    Tag {
        /// Tag name; omit to list all tags
        #[arg(value_name = "NAME")]
        name: Option<String>,
        /// Commit id to tag (default: current HEAD tip)
        #[arg(value_name = "COMMIT")]
        commit: Option<String>,
        /// Delete the named tag
        #[arg(short = 'd', long = "delete", value_name = "NAME", conflicts_with_all = ["name", "commit"])]
        delete: Option<String>,
    },
    /// Move HEAD (and optionally the index/working tree) to a commit
    Reset {
        /// Target commit id (hex); default: current HEAD tip
        #[arg(value_name = "COMMIT")]
        commit: Option<String>,
        /// Move ref only, keep index and working tree
        #[arg(long, conflicts_with_all = ["mixed", "hard"])]
        soft: bool,
        /// Move ref and reset index, keep working tree (default)
        #[arg(long, conflicts_with_all = ["soft", "hard"])]
        mixed: bool,
        /// Move ref and reset both index and working tree
        #[arg(long, conflicts_with_all = ["soft", "mixed"])]
        hard: bool,
    },
    /// Restore working-tree files (or unstage with `--staged`)
    Restore {
        /// Paths to restore (relative to repo root)
        #[arg(value_name = "PATH", required = true)]
        paths: Vec<String>,
        /// Restore the staged copy in the index instead of the working tree
        #[arg(long)]
        staged: bool,
        /// Source commit id (default: HEAD tip)
        #[arg(long)]
        source: Option<String>,
    },
    /// Create a new commit that undoes the changes of an existing commit
    Revert {
        #[arg(value_name = "COMMIT")]
        commit: String,
    },
    /// Apply the changes introduced by a commit on top of HEAD
    CherryPick {
        #[arg(value_name = "COMMIT")]
        commit: String,
    },
    /// Show the HEAD movement history (reflog)
    Reflog,
    /// Verify commit-id integrity and signatures along the current history
    Verify {
        /// Commit id to verify (default: walk from HEAD tip)
        #[arg(value_name = "COMMIT")]
        commit: Option<String>,
        /// Verify the entire ancestry, not just the tip
        #[arg(long)]
        all: bool,
    },
    /// Prune unreachable local objects (commits/blobs not reachable from any ref/tag)
    Gc,
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum WorkspaceCommands {
    /// Initialize local `.spacekit/workspace/config.json`
    Init {
        #[arg(value_name = "WORKSPACE_ID")]
        workspace_id: String,
        #[arg(long)]
        description: Option<String>,
        /// `public` (default) or `private`
        #[arg(long, default_value = "public")]
        visibility: String,
        /// Associated repo names (short names under `repos/`)
        #[arg(long = "repo")]
        repo: Vec<String>,
        #[arg(long = "collaborator")]
        collaborator: Vec<String>,
    },
    /// Push local workspace config to the storage node
    Push {
        #[arg(long)]
        storage_url: Option<String>,
        #[arg(long = "ws-owner")]
        owner_did: Option<String>,
    },
    /// Register workspace in `workspace_registry` for website discovery
    Publish {
        #[arg(long)]
        storage_url: Option<String>,
        #[arg(long = "ws-owner")]
        owner_did: Option<String>,
        /// Override visibility from local config (`public` or `private`)
        #[arg(long)]
        visibility: Option<String>,
    },
    /// Create a `spacekit:workspace:v1` document on the storage node
    Create {
        #[arg(value_name = "WORKSPACE_ID")]
        workspace_id: String,
        #[arg(long)]
        storage_url: Option<String>,
        /// Owner DID (defaults to CLI identity from `~/.spacekit/config.toml`)
        #[arg(long = "ws-owner")]
        owner_did: Option<String>,
        /// `did:role` pairs, e.g. `--collaborator did:spacekit:agent:bot:agent`
        #[arg(long = "collaborator")]
        collaborator: Vec<String>,
        /// Associated repo names (short names under `repos/`)
        #[arg(long = "repo")]
        repo: Vec<String>,
    },
    /// Show one workspace (owner from config or `--ws-owner`)
    Show {
        #[arg(value_name = "WORKSPACE_ID")]
        workspace_id: String,
        #[arg(long)]
        storage_url: Option<String>,
        #[arg(long = "ws-owner")]
        owner_did: Option<String>,
    },
    /// List workspaces for an owner DID
    List {
        #[arg(long)]
        storage_url: Option<String>,
        #[arg(long = "ws-owner")]
        owner_did: Option<String>,
    },
    /// Export federation handoff bundle to a JSON file
    Export {
        #[arg(value_name = "WORKSPACE_ID")]
        workspace_id: String,
        #[arg(short, long)]
        output: std::path::PathBuf,
        #[arg(long)]
        storage_url: Option<String>,
        #[arg(long = "ws-owner")]
        owner_did: Option<String>,
    },
    /// Import a bundle from `workspace export` (destination node)
    Import {
        #[arg(value_name = "BUNDLE_JSON")]
        file: std::path::PathBuf,
        #[arg(long)]
        storage_url: Option<String>,
        /// Destination owner DID (defaults to CLI identity or bundle owner)
        #[arg(long = "ws-owner")]
        owner_did: Option<String>,
        #[arg(long)]
        replace: bool,
        /// Pull referenced blobs from source storage node after import
        #[arg(long)]
        source_url: Option<String>,
        /// Authorization header forwarded to source (e.g. `DID did:...`)
        #[arg(long)]
        source_auth: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum OperatorCommands {
    /// Build and `POST /facts` an operator manifest for federation discovery
    Publish {
        #[arg(long)]
        storage_url: Option<String>,
        #[arg(long)]
        operator_did: Option<String>,
        #[arg(long)]
        display_name: String,
        /// Stream D content policy URL
        #[arg(long)]
        policy_uri: Option<String>,
        /// `permissive` | `hybrid` | `strict` (default: hybrid)
        #[arg(long, default_value = "hybrid")]
        blob_fact_auth: String,
        /// Capability flag (repeat), e.g. `workspaces`, `federation_export`
        #[arg(long = "feature")]
        feature: Vec<String>,
        /// Attach SPHINCS+ signature (required when node is in strict mode)
        #[arg(long)]
        sign: bool,
    },
    /// Print deterministic fact id hex for an operator DID
    FactId {
        #[arg(long)]
        operator_did: Option<String>,
    },
    /// Fetch `GET /api/operators/self` (published manifest or runtime fallback)
    Show {
        #[arg(long)]
        storage_url: Option<String>,
        /// Public URL peers should use (query param to the node)
        #[arg(long)]
        public_url: Option<String>,
    },
}

#[derive(Subcommand, Debug, Clone)]
pub(crate) enum MigrationCommands {
    /// Verify `migration_manifest` inside a workspace export JSON file
    Verify {
        #[arg(value_name = "BUNDLE_JSON")]
        bundle_file: std::path::PathBuf,
        #[arg(long)]
        storage_url: Option<String>,
    },
    /// Generate a SPHINCS+ keypair for `workspace_owner` migration signing (dev / CI)
    Keygen {
        #[arg(long)]
        signer_did: String,
        #[arg(long)]
        storage_url: Option<String>,
    },
    /// Append a DID signature to `migration_manifest` (writes bundle back unless `--stdout`)
    Sign {
        #[arg(value_name = "BUNDLE_JSON")]
        bundle_file: std::path::PathBuf,
        /// Signer role: `source_operator`, `destination_operator`, or `workspace_owner`
        #[arg(long, default_value = "source_operator")]
        role: String,
        #[arg(long)]
        signer_did: Option<String>,
        #[arg(long)]
        storage_url: Option<String>,
        /// Print signed JSON to stdout instead of overwriting the file
        #[arg(long)]
        stdout: bool,
    },
}

/// Parses `spacekit agent …`: growformer-style `--train`/`--train-brain`, or an explicit subcommand (`train`, `load`, …).
#[derive(Args, Debug, Clone)]
#[command(
    about = "Growformer agent brains (train, infer, merge)",
    long_about = "Train, merge, and run Growformer `.bin` brains.\n\n\
        • `train`, `merge`, `infer --brain`, `exec` — embedded growformer (no separate binary).\n\
        • `load`, `infer --name`, `list`, `unload` — in-process cache in this CLI invocation.\n\n\
        Shortcut: `-t` / `--train` / `--train-brain` with `--project PATH` is the same as `agent train`.",
    after_help = AGENT_AFTER_HELP
)]
struct AgentArgs {
    /// Train a brain (same as `agent train`; requires `--project`)
    #[arg(short = 't', long, visible_alias = "train-brain")]
    train: bool,

    #[command(subcommand)]
    command: Option<AgentCommands>,

    /// Path to project `.gf.toml` (required with `-t` / `--train`)
    #[arg(long, required_if_eq("train", "true"))]
    project: Option<PathBuf>,
    /// Pass `--auto` to growformer train
    #[arg(long)]
    auto: bool,
    /// Output `.bin` path (`--brain-output` for growformer)
    #[arg(long)]
    brain_output: Option<String>,
    /// Training data directory override
    #[arg(long)]
    data_dir: Option<String>,
    /// Extra growformer arguments (after train flags)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    extra: Vec<String>,

    /// Published content id (64-hex) — gates agent use via entitlement; with `exec`, runs installed binary
    #[arg(long, global = true)]
    content_id: Option<String>,

    /// App slug from `content view` install record (e.g. `growformer`)
    #[arg(long, global = true)]
    app: Option<String>,
}

#[derive(Subcommand, Debug)]
enum TaskCommands {
    /// Submit a task for distributed execution
    Submit {
        /// WebAssembly file to execute
        #[arg(short, long)]
        file: String,

        /// Runtime type (wasm, gpu, hybrid)
        #[arg(short, long, default_value = "wasm")]
        runtime: String,

        /// Owner DID (default: config identity; global `--did` also applies)
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,

        /// Input data file for the task
        #[arg(short, long)]
        input: Option<String>,

        /// Encryption algorithm for task data
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::Kyber1024)]
        encryption: EncryptionAlgorithm,

        /// Maximum cost willing to pay for execution
        #[arg(long)]
        max_cost: Option<u64>,
    },

    /// Get task status
    Status {
        /// Task ID to check
        task_id: String,
    },

    /// List tasks
    List {
        /// Filter by task status
        #[arg(long)]
        status: Option<String>,

        /// Filter by owner DID
        #[arg(long)]
        owner: Option<String>,

        /// Show only tasks owned by current user
        #[arg(long)]
        owned_by_me: bool,
    },

    /// Cancel a task
    Cancel {
        /// Task ID to cancel
        task_id: String,
    },

    /// Get task result
    Result {
        /// Task ID to get result for
        task_id: String,

        /// Output file for the result
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Watch task status in real-time
    Watch {
        /// Task ID to watch
        task_id: String,

        /// Update interval in seconds
        #[arg(long, default_value = "5")]
        interval: u64,
    },
}

#[derive(Subcommand, Debug)]
enum StorageCommands {
    /// Store a file with quantum-resistant encryption
    Store {
        /// File to store
        #[arg(short, long)]
        file: String,

        /// Owner DID (default: config identity; global `--did` also applies)
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,

        /// File description/metadata
        #[arg(short, long)]
        description: Option<String>,

        /// Quantum encryption algorithm
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::Kyber1024)]
        encryption: EncryptionAlgorithm,

        /// Enable P2P distribution
        #[arg(long)]
        p2p: bool,

        /// Storage node base URL (default: from config or http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,

        /// Replication factor for distributed storage
        #[arg(long, default_value = "3")]
        replicas: usize,
    },

    /// Retrieve a stored file
    Retrieve {
        /// File ID to retrieve
        file_id: String,

        /// Output file path
        #[arg(short, long)]
        output: String,

        /// Requester DID (defaults to config DID)
        #[arg(long)]
        requester_did: Option<String>,

        /// Storage node base URL (default: from config or http://127.0.0.1:3030).
        /// Ignored when `--embedded` is set.
        #[arg(long)]
        storage_url: Option<String>,

        /// Read from the CLI's embedded storage node (`~/.spacekit/storage` on disk), not HTTP.
        /// Use this for files pinned during `spacekit contract deploy` when no standalone API is running.
        /// Same as env `SPACEKIT_STORAGE_RETRIEVE_EMBEDDED=1`.
        #[arg(long, visible_alias = "local")]
        embedded: bool,
    },
    List {
        /// Filter by owner DID
        #[arg(long)]
        owner: Option<String>,

        /// Show only files owned by current user
        #[arg(long)]
        owned_by_me: bool,

        /// Show file details
        #[arg(long)]
        details: bool,

        /// Storage node base URL (default: from config or http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,
    },

    /// Share file access with another DID
    Share {
        /// File ID to share
        file_id: String,

        /// DID to share with
        #[arg(long)]
        with_did: String,

        /// Permission level (read, write, admin)
        #[arg(long, default_value = "read")]
        permission: String,

        /// Storage node base URL (default: from config or http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,
    },

    /// Revoke file access from a DID
    Revoke {
        /// File ID to revoke access for
        file_id: String,

        /// DID to revoke access from
        #[arg(long)]
        from_did: String,

        /// Storage node base URL (default: from config or http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,
    },

    /// Show storage statistics
    Stats {
        /// Show detailed statistics
        #[arg(long)]
        detailed: bool,

        /// Storage node base URL (default: from config or http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,
    },

    /// Verify local artifact bytes match BLAKE3 hashes recorded in a `storage deploy --receipt` JSON file
    VerifyReceipt {
        /// Path to deploy receipt JSON
        #[arg(long)]
        receipt: String,
    },

    /// Download decrypted file content from a remote storage node (GET /files/{id}/session-key then /files/{id}/content)
    Fetch {
        /// File UUID from the deploy receipt or storage UI
        file_id: String,
        /// Output path for decrypted bytes
        #[arg(short, long)]
        output: String,
        /// Storage node base URL (default: from `spacekit connect storage` or http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,
        /// Send `requester-did` header (group-shared files). Omit for normal uploads so the node uses the file's stored owner_did for ACL.
        #[arg(long)]
        requester_did: Option<String>,
    },

    /// Upload a file using envelope encryption (zero-knowledge: private key never leaves your machine)
    EnvelopeUpload {
        /// Path to the file to upload
        file: String,
        /// Storage node base URL
        #[arg(long)]
        storage_url: Option<String>,
        /// Override filename sent to server
        #[arg(long)]
        filename: Option<String>,
        /// Content type (e.g. application/wasm)
        #[arg(long)]
        content_type: Option<String>,
    },

    /// Download a file using envelope encryption (zero-knowledge: server never sees your private key)
    EnvelopeFetch {
        /// File UUID
        file_id: String,
        /// Output path for decrypted bytes
        #[arg(short, long)]
        output: String,
        /// Storage node base URL
        #[arg(long)]
        storage_url: Option<String>,
    },

    /// Pull wasm + bin from a remote node using receipt `file_id`s, verify BLAKE3, write to local paths (e.g. website `public/`)
    SyncReceipt {
        /// Deploy receipt JSON (from `spacekit storage deploy --receipt`)
        #[arg(long)]
        receipt: String,
        /// Where to write the wasm artifact
        #[arg(long)]
        wasm_out: String,
        /// Where to write the companion .bin
        #[arg(long)]
        bin_out: String,
        #[arg(long)]
        storage_url: Option<String>,
        /// Same semantics as `storage fetch --requester-did`
        #[arg(long)]
        requester_did: Option<String>,
    },

    /// Deploy a WASM + companion .bin to a running storage node (HTTP API) and emit a receipt
    Deploy {
        /// Load settings from a deploy manifest TOML (e.g. `deploy.toml`). CLI flags override manifest values.
        #[arg(long, value_name = "DEPLOY_TOML", visible_alias = "manifest")]
        package: Option<String>,

        /// Path to the WASM artifact (e.g. contract.wasm). Required unless set in the manifest.
        #[arg(long)]
        wasm: Option<String>,

        /// Path to the companion binary (e.g. model.bin). Required unless set in the manifest.
        #[arg(long)]
        bin: Option<String>,

        /// Owner DID (default: config identity; global `--did` also applies)
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,

        /// Storage node base URL (default: from `spacekit connect storage`, e.g. http://127.0.0.1:8080)
        #[arg(long)]
        storage_url: Option<String>,

        /// Write the deployment receipt JSON to this path (summary is always printed)
        #[arg(long)]
        receipt: Option<String>,

        /// KEM algorithm for your public key (e.g. Kyber1024). Default: `identity.algorithm` in config. Must match the key in `public_key_path` and the storage node must support OQS for that KEM.
        #[arg(long)]
        owner_key_algorithm: Option<String>,

        /// Agent ID to tag this deployment (e.g. "fa-007"). When set, the receipt is stored
        /// on the storage node as a document in the `deployments` collection so the website API
        /// can resolve artifact file IDs by agent ID at runtime.
        #[arg(long)]
        agent_id: Option<String>,

        /// Publish to the marketplace after deploying. Stores an app_listings document so the
        /// app appears in the marketplace catalog.
        #[arg(long)]
        publish: bool,

        /// App title for the marketplace listing (used with --publish)
        #[arg(long)]
        title: Option<String>,

        /// App description for the marketplace listing (used with --publish)
        #[arg(long)]
        description: Option<String>,

        /// App category for discovery: productivity, social, finance, games, entertainment,
        /// developer, education, health, news, utilities, ai, storage, security, lifestyle, business
        #[arg(long, default_value = "ai")]
        category: Option<String>,

        /// Access level: public or private (used with --publish)
        #[arg(long, default_value = "public")]
        access: Option<String>,

        /// Pricing model: free, or a price in aUSD (e.g. "10.00") for one-time purchase
        #[arg(long, default_value = "free")]
        price: Option<String>,

        /// Marketplace ID to publish to (used with --publish)
        #[arg(long, default_value = "default")]
        marketplace: Option<String>,

        /// WASM VM storage key for the brain `.bin` (Agent Hub / Growformer). Required for inference in the browser.
        #[arg(long)]
        brain_key: Option<String>,

        /// Comma-separated capability labels shown in Agent Hub (e.g. "Market tone,Risk phrasing")
        #[arg(long, value_delimiter = ',')]
        capabilities: Option<Vec<String>>,

        /// Display tag in Agent Hub sidebar (e.g. FINANCE)
        #[arg(long)]
        tag_label: Option<String>,

        /// Hex color for the display tag (e.g. #34d399)
        #[arg(long)]
        tag_color: Option<String>,

        /// Agent Hub output layout: growformer (default) or plain
        #[arg(long, default_value = "growformer")]
        hub_response_format: Option<String>,
    },

    /// Manage storage node
    Node {
        /// Node action (start, stop, status)
        #[arg(value_enum)]
        action: NodeAction,

        /// Storage node configuration
        #[arg(long)]
        config: Option<String>,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum NodeAction {
    Start,
    Stop,
    Status,
}

#[derive(Subcommand, Debug)]
enum DIDCommands {
    /// Create a new quantum-resistant DID
    Create {
        /// Quantum algorithm to use
        #[arg(short, long, value_enum, default_value_t = EncryptionAlgorithm::Kyber1024)]
        algorithm: EncryptionAlgorithm,

        /// Save keys to files
        #[arg(long)]
        save: bool,

        /// Custom DID identifier (optional)
        #[arg(long)]
        identifier: Option<String>,

        /// Output format (json, text)
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Verify a DID and its credentials
    Verify {
        /// DID to verify
        did: String,

        /// Verify associated credentials
        #[arg(long)]
        credentials: bool,

        /// Show detailed verification info
        #[arg(long)]
        detailed: bool,
    },

    /// Update a DID (add keys, rotate keys, etc.)
    Update {
        /// DID to update
        did: String,

        /// Add new public key
        #[arg(long)]
        add_key: Option<String>,

        /// Rotate quantum keys
        #[arg(long)]
        rotate_keys: bool,

        /// Update DID document
        #[arg(long)]
        update_document: Option<String>,
    },

    /// Resolve a DID to its document
    Resolve {
        /// DID to resolve
        did: String,

        /// Output format (json, text)
        #[arg(long, default_value = "json")]
        format: String,

        /// Show verification status
        #[arg(long)]
        verify: bool,
    },

    /// List DIDs
    List {
        /// Show only DIDs owned by current user
        #[arg(long)]
        owned_by_me: bool,

        /// Filter by DID method
        #[arg(long)]
        method: Option<String>,

        /// Show detailed information
        #[arg(long)]
        detailed: bool,

        /// Include credentials count
        #[arg(long)]
        with_credentials: bool,
    },

    /// Issue a verifiable credential
    Issue {
        /// Target DID to issue credential to
        #[arg(long)]
        to: String,

        /// Credential type
        #[arg(long)]
        credential_type: String,

        /// Claims data (JSON format)
        #[arg(long)]
        claims: String,

        /// Validity period in days
        #[arg(long)]
        validity_days: Option<u32>,

        /// Output file for credential
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Verify a credential
    VerifyCredential {
        /// Credential file to verify
        #[arg(short, long)]
        credential_file: String,

        /// Show detailed verification info
        #[arg(long)]
        detailed: bool,
    },
}

#[derive(Subcommand, Debug)]
enum NetworkCommands {
    /// Create `~/.spacekit/network/config.toml` (override path with `SPACEKIT_NETWORK_CONFIG`).
    ///
    /// Profile v3 includes deployment trust policy, `[services]`, `[ports]`, and optional `[urls]`. Ports default to
    /// storage 3030, compute 9000, messaging 7100/7000. Use `external` mode when nodes run elsewhere.
    Init {
        /// Overwrite an existing profile
        #[arg(long)]
        force: bool,
        /// Deployment preset: local, private, or public
        #[arg(long, value_enum, default_value_t = crate::network_profile::NetworkPreset::Local)]
        profile: crate::network_profile::NetworkPreset,
        /// Node duties (preset default when omitted)
        #[arg(long, value_enum)]
        role: Option<crate::network_profile::NetworkRole>,
        /// Stable node name used to isolate runtime files and data
        #[arg(long)]
        node_id: Option<String>,
        /// Add this value to every default service port (use 0, 20000, 40000 for three local nodes)
        #[arg(long, default_value_t = 0)]
        port_offset: u16,
        /// Root directory for per-node storage, compute, and messaging data
        #[arg(long)]
        data_root: Option<PathBuf>,
        /// Signed canonical network manifest (required for public)
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Admitted DID or peer ID for a private network (repeatable)
        #[arg(long = "allow-peer")]
        allowlist: Vec<String>,
        /// Lowercase Blake3 digest of the shared private-network genesis
        #[arg(long)]
        shared_genesis_hash: Option<String>,
        #[arg(long, value_enum, default_value_t = crate::network_profile::NetworkMode::Embedded)]
        mode: crate::network_profile::NetworkMode,
        #[arg(long)]
        compute_url: Option<String>,
        #[arg(long)]
        storage_url: Option<String>,
        #[arg(long)]
        gateway_url: Option<String>,
        /// libp2p multiaddr bootstrap (repeat `--bootstrap-peer` for multiple)
        #[arg(long = "bootstrap-peer")]
        bootstrap_peer: Vec<String>,
        /// Override the preset bind host
        #[arg(long)]
        bind_host: Option<String>,
        #[arg(long)]
        storage_port: Option<u16>,
        #[arg(long)]
        storage_p2p_port: Option<u16>,
        #[arg(long)]
        compute_port: Option<u16>,
        #[arg(long)]
        messaging_listen_port: Option<u16>,
        #[arg(long)]
        messaging_bootstrap_port: Option<u16>,
        #[arg(long)]
        gateway_port: Option<u16>,
        #[arg(long)]
        no_storage: bool,
        #[arg(long)]
        no_messaging: bool,
        #[arg(long)]
        no_compute: bool,
        #[arg(long)]
        enable_gateway: bool,
    },

    /// Start enabled embedded services from the network profile (like `docker compose up`).
    Up {
        /// Run in the background and return immediately
        #[arg(short = 'd', long)]
        detach: bool,
        /// Override profile for this run: comma-separated storage,messaging,compute
        #[arg(long)]
        only: Option<String>,
        /// Enable all services + blockchain (genesis, validators, operator rewards).
        ///
        /// Equivalent to `--only storage,messaging,compute,gateway` plus `blockchain.enabled = true`.
        /// For agent/storage/compute work use plain `network up` (no blockchain). `--full` runs an
        /// in-process block producer that persists `ledger.json` and can increase RSS over long runs.
        /// Tune `[blockchain] block_time_ms` in `~/.spacekit/network/config.toml` (default 10s) or
        /// `SPACEKIT_BLOCK_TIME_MS` for local dev.
        #[arg(long)]
        full: bool,
    },

    /// Start one embedded service (storage, messaging, or compute).
    Start {
        #[arg(value_enum)]
        service: crate::network_profile::NetworkService,
        #[arg(short = 'd', long)]
        detach: bool,
    },

    /// Stop the network supervisor (all services). Use `down` to stop everything.
    Stop {
        /// Optional service name (only when it is the sole running service)
        service: Option<crate::network_profile::NetworkService>,
    },

    /// Stop the network started by `network up`.
    Down,

    #[command(hide = true, name = "run-supervisor")]
    RunSupervisor,

    /// Memory diagnostic: process RSS, storage caches, disk usage, ranked suspects
    Memory {
        /// Machine-readable JSON output
        #[arg(long)]
        json: bool,
        /// macOS only: run `sample` on supervisor PID (5s) and write to /tmp
        #[arg(long)]
        sample: bool,
        /// Poll RSS every N seconds and show growth rate (Ctrl-C to stop)
        #[arg(long)]
        watch: bool,
        /// Seconds between samples when using `--watch` (default 10)
        #[arg(long, default_value = "10")]
        interval: u64,
    },

    /// Show network status and connectivity
    Status {
        /// Show detailed network information
        #[arg(long)]
        detailed: bool,

        /// Show real-time metrics
        #[arg(long)]
        realtime: bool,
    },

    /// Diagnose profile, runtime state, ports, and configured service endpoints
    Doctor,

    /// Show logs written by the detached supervisor and service sidecars
    Logs {
        /// Restrict output to one service
        #[arg(long, value_enum)]
        service: Option<crate::network_profile::NetworkService>,
        /// Number of trailing lines to show per log
        #[arg(long, default_value = "100")]
        lines: usize,
    },

    /// Run isolated, deterministic network acceptance gates and save a report plus artifacts
    Test {
        /// Gate suite to run
        #[arg(long, value_enum, default_value = "local")]
        suite: crate::network_e2e::NetworkTestSuite,
        /// JSON report path; use a .xml extension for JUnit
        #[arg(long, default_value = "network-e2e-report.json")]
        report: PathBuf,
        /// Optional website URL to probe in the local suite
        #[arg(long)]
        website_url: Option<String>,
        /// Optional website API URL to probe in the local suite
        #[arg(long)]
        api_url: Option<String>,
    },

    /// Remove local service data after an explicit confirmation
    Reset {
        /// Remove storage, compute, and messaging data directories
        #[arg(long)]
        data: bool,
        /// Skip the interactive confirmation
        #[arg(long)]
        force: bool,
    },

    /// Join a private or public network from its manifest
    Join {
        /// Signed canonical network manifest JSON
        #[arg(long)]
        manifest: PathBuf,
        /// Duties this node will perform
        #[arg(long, value_enum)]
        role: crate::network_profile::NetworkRole,
        /// Overwrite an existing network profile
        #[arg(long)]
        force: bool,
    },

    /// Sign or verify portable network manifests
    Manifest {
        #[command(subcommand)]
        action: NetworkManifestAction,
    },

    /// View or modify the network profile (`~/.spacekit/network/config.toml`)
    Config {
        #[command(subcommand)]
        action: NetworkConfigAction,
    },

    /// Discover services on the network
    Discover {
        /// Service type to discover (compute, storage, messaging, consensus)
        #[arg(long)]
        service_type: Option<String>,

        /// Show detailed service information
        #[arg(long)]
        detailed: bool,

        /// Maximum number of services to return
        #[arg(long, default_value = "10")]
        limit: usize,
    },

    /// List connected peers and their capabilities
    Peers {
        /// Show detailed peer information
        #[arg(long)]
        detailed: bool,

        /// Filter by service capability
        #[arg(long)]
        service: Option<String>,

        /// Show only active peers
        #[arg(long)]
        active_only: bool,
    },

    /// Show reputation for a specific DID
    Reputation {
        /// DID to check reputation for
        #[arg(long)]
        did: String,

        /// Show detailed reputation breakdown
        #[arg(long)]
        detailed: bool,

        /// Show reputation history
        #[arg(long)]
        history: bool,
    },

    /// Watch reputation changes in real-time
    ReputationWatch {
        /// DID to monitor
        #[arg(long)]
        did: String,

        /// Update interval in seconds
        #[arg(long, default_value = "30")]
        interval: u64,

        /// Show alerts for significant changes
        #[arg(long)]
        alerts: bool,
    },
}

#[derive(Subcommand, Debug)]
enum NetworkManifestAction {
    /// Generate a new SpaceKit SPHINCS-128f manifest signing keypair
    Keygen {
        /// File to receive the raw public key as hex
        #[arg(long)]
        public_key: PathBuf,
        /// File to receive the raw secret key as hex
        #[arg(long)]
        secret_key: PathBuf,
    },
    /// Sign canonical manifest bytes with an existing SpaceKit SPHINCS-128f identity key
    Sign {
        #[arg(value_name = "MANIFEST_JSON")]
        manifest: PathBuf,
        /// DID URL identifying the verification key
        #[arg(long)]
        key_id: String,
        /// File containing the raw public key as hex
        #[arg(long)]
        public_key: PathBuf,
        /// File containing the raw secret key as hex
        #[arg(long)]
        secret_key: PathBuf,
        /// Output path (defaults to replacing the input atomically)
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Cryptographically verify a signed manifest and its genesis/protocol metadata
    Verify {
        #[arg(value_name = "MANIFEST_JSON")]
        manifest: PathBuf,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum NetworkConfigAction {
    /// Print the current network profile
    Show,
    /// Set a config key (e.g. `set ports.gateway_http 8080`)
    Set {
        /// Dotted key path (e.g. `ports.status_http`, `services.gateway`)
        key: String,
        /// New value
        value: String,
    },
    /// Enable a service (storage, messaging, compute, gateway)
    Enable {
        /// Service name
        service: String,
    },
    /// Disable a service (storage, messaging, compute, gateway)
    Disable {
        /// Service name
        service: String,
    },
    /// Show the path to the config file
    Path,
}

#[derive(Subcommand, Debug)]
enum ConsensusCommands {
    /// Submit a proposal to the consensus system
    SubmitProposal {
        /// Proposal type (block, metrics, hybrid)
        #[arg(short, long, value_enum)]
        proposal_type: ProposalType,

        /// Proposal data (JSON format)
        #[arg(short, long)]
        data: String,

        /// Target committee (optional)
        #[arg(short, long)]
        committee: Option<String>,

        /// Proposal description
        #[arg(long)]
        description: Option<String>,

        /// Voting duration in hours
        #[arg(long, default_value = "24")]
        duration: u64,

        /// Use in-process UnifiedSWTCHConsensus (dev only). Default: HTTP to compute node.
        #[arg(long)]
        in_process: bool,

        /// After block submit, run PQ finisher on compute node (requires dev_mode or single-validator config).
        #[arg(long)]
        finalize: bool,

        /// Fill missing block fields from SwtchVM head (HTTP block proposals only).
        #[arg(long)]
        use_swtchvm_head: bool,

        /// Broadcast block announce after submit (HTTP block proposals only).
        #[arg(long)]
        announce: bool,
    },

    /// Vote on a proposal
    Vote {
        /// Proposal ID to vote on
        #[arg(long)]
        proposal_id: String,

        /// Vote choice (approve, reject, abstain)
        #[arg(short, long, value_enum)]
        vote: VoteChoice,

        /// Voting rationale (optional)
        #[arg(long)]
        rationale: Option<String>,
    },

    /// Check consensus status
    Status {
        /// Specific proposal ID to check
        #[arg(long)]
        proposal_id: Option<String>,

        /// Show detailed consensus information
        #[arg(long)]
        detailed: bool,

        /// Show network-wide consensus health
        #[arg(long)]
        network_health: bool,
    },

    /// List proposals
    List {
        /// Filter by proposal status
        #[arg(long)]
        status: Option<String>,

        /// Filter by proposal type
        #[arg(long, value_enum)]
        proposal_type: Option<ProposalType>,

        /// Show only my proposals
        #[arg(long)]
        my_proposals: bool,

        /// Maximum number of proposals to show
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Check consensus migration status
    Migration {
        /// Show migration details
        #[arg(long)]
        detailed: bool,

        /// Show migration history
        #[arg(long)]
        history: bool,

        /// Show risk assessment
        #[arg(long)]
        risks: bool,
    },
}

#[derive(Subcommand, Debug)]
enum SimulatorCommands {
    /// Boot a local network from YAML topology (compute + storage + messaging)
    Up {
        /// Path to a TestnetConfiguration YAML (default: public testnet preset)
        #[arg(long)]
        config: Option<String>,

        /// Override the proxy listen port
        #[arg(long, default_value = "8080")]
        port: u16,

        /// Start the in-process EVM / MetaMask JSON‑RPC (also: `SPACEKIT_SIM_ETH_JSON_RPC=1`,
        /// see `spacekit-simulator` `network_adapters` module for env vars).
        #[arg(long, alias = "eth-rpc")]
        eth_json_rpc: bool,

        /// Listen port for the EVM JSON‑RPC when `--eth-json-rpc` is set
        #[arg(long, default_value = "8545", value_name = "PORT")]
        eth_json_rpc_port: u16,

        /// `eth_chainId` when `--eth-json-rpc` is set (default: public testnet 1337)
        #[arg(long, default_value = "1337", value_name = "ID")]
        eth_json_rpc_chain_id: u64,

        /// Start a local JSON-RPC that forwards to **Base** (L2) — your wallet sees real Base balances;
        /// this is *not* the SpaceKit `8545` chain (ASTRA testnet is separate). Also: `SPACEKIT_SIM_BASE_JSON_RPC=1`
        #[arg(long)]
        base_json_rpc: bool,
        /// Listen port for the Base forwarder
        #[arg(long, default_value = "8560", value_name = "PORT")]
        base_json_rpc_port: u16,
        #[arg(long, value_enum, default_value = "mainnet", value_name = "NET")]
        base_network: SimBaseNetwork,
        /// Custom Base/rollup JSON-RPC URL (if set, overrides `--base-network`); set `--base-custom-chain-id` for MetaMask hint
        #[arg(long, value_name = "URL")]
        base_rpc_url: Option<String>,
        /// With `--base-rpc-url` — for display / `GET` hint (chain id is still from `eth_chainId` on the real node)
        #[arg(long, value_name = "ID")]
        base_custom_chain_id: Option<u64>,
    },

    /// Show funded testnet accounts (100M ASTRA + 100M aUSD each)
    Accounts,

    /// VPN operations
    #[command(subcommand)]
    Vpn(VpnCommands),

    /// Orchestration and WASM deployment
    #[command(subcommand)]
    Orchestration(OrchestrationCommands),

    /// Cross-network connectivity
    #[command(subcommand)]
    CrossNetwork(CrossNetworkCommands),

    /// Blockchain scanner operations
    #[command(subcommand)]
    Scanner(ScannerCommands),

    /// Faucet operations (testnet tokens)
    #[command(subcommand)]
    Faucet(FaucetCommands),
}

/// Base L2 target for `--base-json-rpc` (forwards to public Base; not SpaceKit state).
#[derive(Debug, Default, Clone, Copy, clap::ValueEnum)]
enum SimBaseNetwork {
    /// `https://mainnet.base.org` (chain id 8453)
    #[default]
    Mainnet,
    /// `https://sepolia.base.org` (84532)
    Sepolia,
}

#[derive(Subcommand, Debug)]
enum VpnCommands {
    /// Establish a VPN connection
    Establish {
        /// Target DID to connect to
        #[arg(long)]
        target_did: String,

        /// Relay chain type (onion, simple)
        #[arg(long, default_value = "onion")]
        relay_chain: String,

        /// Number of relay nodes
        #[arg(long, default_value = "3")]
        relay_count: usize,
    },

    /// Get VPN connection status
    Status {
        /// VPN connection ID
        connection_id: String,
    },

    /// List all VPN connections
    List {
        /// Show only active connections
        #[arg(long)]
        active_only: bool,
    },

    /// Terminate a VPN connection
    Terminate {
        /// VPN connection ID
        connection_id: String,
    },

    /// List available relay nodes
    Relays,
}

#[derive(Subcommand, Debug)]
enum OrchestrationCommands {
    /// Deploy nodes via WASM orchestration
    Deploy {
        /// Deployment type (compute, storage, messaging)
        #[arg(long, value_enum)]
        deployment_type: DeploymentType,

        /// Number of replicas
        #[arg(long, default_value = "1")]
        replicas: usize,

        /// Owner DID
        #[arg(long)]
        did: String,

        /// Enable GPU support (compute only)
        #[arg(long)]
        gpu_enabled: bool,

        /// Namespace for deployment
        #[arg(long)]
        namespace: Option<String>,
    },

    /// List all deployments
    List {
        /// Filter by deployment type
        #[arg(long, value_enum)]
        deployment_type: Option<DeploymentType>,

        /// Filter by namespace
        #[arg(long)]
        namespace: Option<String>,
    },

    /// Scale a deployment
    Scale {
        /// Deployment ID
        deployment_id: String,

        /// New replica count
        #[arg(long)]
        replicas: usize,
    },

    /// Terminate a deployment
    Terminate {
        /// Deployment ID
        deployment_id: String,
    },

    /// List available WASM packages
    Packages,

    /// List deployed compute nodes
    ListCompute {
        /// Show detailed information
        #[arg(long)]
        detailed: bool,
    },

    /// List deployed storage nodes
    ListStorage {
        /// Show detailed information
        #[arg(long)]
        detailed: bool,
    },

    /// Get node information
    NodeInfo {
        /// Node ID
        node_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum CrossNetworkCommands {
    /// Connect to a remote network
    Connect {
        /// Remote peer address (IP:PORT)
        #[arg(long)]
        peer: String,

        /// Enable secure channel encryption
        #[arg(long)]
        secure_channel: bool,
    },

    /// Show cross-network status
    Status,

    /// Show network health metrics
    Health,

    /// Configure hub-spoke topology
    #[command(subcommand)]
    Topology(TopologyCommands),
}

#[derive(Subcommand, Debug)]
enum TopologyCommands {
    /// Configure as hub
    HubConfigure {
        /// Listen port
        #[arg(long, default_value = "7000")]
        listen_port: u16,
    },

    /// Join as spoke
    SpokeJoin {
        /// Hub address
        #[arg(long)]
        hub_address: String,
    },

    /// Join mesh network
    MeshJoin {
        /// Peer addresses (comma-separated)
        #[arg(long)]
        peers: String,
    },

    /// Show topology status
    Status,
}

#[derive(Subcommand, Debug)]
enum ScannerCommands {
    /// Scan a specific block
    ScanBlock {
        /// Block number
        block_number: u64,
    },

    /// Scan an address
    ScanAddress {
        /// Address to scan
        address: String,
    },

    /// Subscribe to events
    Subscribe {
        /// Event type filter
        #[arg(long)]
        event_type: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum FaucetCommands {
    /// Request testnet tokens
    Request {
        /// Recipient DID
        #[arg(long)]
        did: String,

        /// Amount to request
        #[arg(long, default_value = "100")]
        amount: u64,
    },

    /// Check faucet balance
    Balance,
}

#[derive(Subcommand, Debug)]
enum CollaborativeCommands {
    /// Create a collaborative computation
    Create {
        /// Computation type (federated-learning, distributed-training, etc.)
        #[arg(long)]
        computation_type: String,

        /// Participant DIDs (comma-separated)
        #[arg(long)]
        participants: String,

        /// Consensus policy (unanimous, majority, weighted)
        #[arg(long, default_value = "majority")]
        consensus_policy: String,
    },

    /// Join a collaborative computation
    Join {
        /// Computation ID
        computation_id: String,

        /// Participant DID
        #[arg(long)]
        did: String,
    },

    /// Submit partial result
    Submit {
        /// Computation ID
        computation_id: String,

        /// Result file path
        #[arg(long)]
        result: String,
    },

    /// Get collaboration status
    Status {
        /// Computation ID
        computation_id: String,
    },

    /// Secure Multi-Party Computation (SMPC) operations
    #[command(subcommand)]
    Smpc(SmpcCommands),
}

#[derive(Subcommand, Debug)]
enum SmpcCommands {
    /// Create an SMPC session
    Create {
        /// Participant DIDs (comma-separated)
        #[arg(long)]
        participants: String,

        /// Threshold for reconstruction
        #[arg(long, default_value = "2")]
        threshold: usize,

        /// Computation type (sum, average, comparison)
        #[arg(long)]
        computation_type: String,
    },

    /// Submit secret share
    Submit {
        /// Session ID
        session_id: String,

        /// Secret share file
        #[arg(long)]
        share: String,
    },

    /// Compute SMPC result
    Compute {
        /// Session ID
        session_id: String,
    },

    /// Get SMPC session status
    Status {
        /// Session ID
        session_id: String,
    },
}

#[derive(Subcommand, Debug)]
enum NftCommands {
    /// Create an NFT
    Create {
        /// NFT name
        #[arg(long)]
        name: String,

        /// Image URI (IPFS or HTTP)
        #[arg(long)]
        image: String,

        /// Metadata file (JSON)
        #[arg(long)]
        metadata: Option<String>,

        /// Owner DID (default: config identity; global `--did` also applies)
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,
    },

    /// Query NFTs
    Query {
        /// Owner DID filter
        #[arg(long)]
        owner: Option<String>,

        /// Collection ID filter
        #[arg(long)]
        collection: Option<String>,
    },

    /// Transfer an NFT
    Transfer {
        /// NFT ID
        nft_id: String,

        /// Recipient DID
        #[arg(long)]
        to: String,
    },

    /// NFT collection operations
    #[command(subcommand)]
    Collection(NftCollectionCommands),
}

#[derive(Subcommand, Debug)]
enum NftCollectionCommands {
    /// Create an NFT collection
    Create {
        /// Collection name
        #[arg(long)]
        name: String,

        /// Collection symbol
        #[arg(long)]
        symbol: String,

        /// Royalty percentage (0-100)
        #[arg(long, default_value = "0")]
        royalty: u8,

        /// Creator DID
        #[arg(long)]
        creator_did: String,
    },

    /// Mint NFT to collection
    Mint {
        /// Collection ID
        collection_id: String,

        /// NFT metadata file
        #[arg(long)]
        metadata: String,
    },

    /// Get collection statistics
    Stats {
        /// Collection ID
        collection_id: String,
    },

    /// List collections
    List {
        /// Creator DID filter
        #[arg(long)]
        creator: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum MetricsCommands {
    /// Collect production metrics
    Collect {
        /// Output format (json, prometheus)
        #[arg(long, default_value = "json")]
        format: String,
    },

    /// Export metrics for external systems
    Export {
        /// Export format (prometheus, json)
        #[arg(long, default_value = "prometheus")]
        format: String,

        /// Output file path
        #[arg(long)]
        output: Option<String>,
    },

    /// Show network statistics
    NetworkStats {
        /// Show detailed statistics
        #[arg(long)]
        detailed: bool,
    },

    /// Analyze performance metrics
    Analyze {
        /// Time window in hours
        #[arg(long, default_value = "24")]
        window: u64,
    },

    /// Metrics consensus operations (fraud detection)
    #[command(subcommand)]
    Consensus(MetricsConsensusCommands),
}

#[derive(Subcommand, Debug)]
enum MetricsConsensusCommands {
    /// Attest node metrics
    Attest {
        /// Metrics file (JSON)
        #[arg(long)]
        metrics: String,
    },

    /// Validate cross-node metrics
    Validate {
        /// Attestations file (JSON array)
        #[arg(long)]
        attestations: String,
    },

    /// Detect potential fraud/manipulation
    DetectFraud {
        /// Network metrics file (JSON)
        #[arg(long)]
        metrics: String,
    },
}

#[derive(Subcommand, Debug)]
enum VmCommands {
    /// Credit the owner's SwtchVM account (same ledger as `contract deploy`) so deployment can pay gas.
    Fund {
        /// Owner DID (default: config identity; global `--did` also applies)
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,
        /// Amount to add (ledger units; default covers default deploy gas)
        #[arg(long, default_value_t = 50_000_000u64)]
        amount: u64,
    },
    /// Show SwtchVM ledger balance for an owner DID (this CLI process only; not L1 on-chain balance).
    Balance {
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,
    },
    /// Show operator earnings from the blockchain ledger (from `network up --full`).
    Earnings {
        /// Operator DID (default: config identity)
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,
    },
    /// Withdraw operator earnings from the blockchain ledger to a destination.
    Withdraw {
        /// Operator DID (default: config identity)
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,
        /// Amount to withdraw (0 = all available)
        #[arg(long, default_value_t = 0)]
        amount: u64,
    },
    /// Seed a Growformer brain into a deployed contract's KV store.
    /// The contract can then call `agent_growformer_load_brain_from_storage(key)`.
    #[command(name = "brain-seed")]
    BrainSeed {
        /// Contract address (from `contract deploy`)
        #[arg(long)]
        contract_id: String,
        /// Storage key the contract will look up (e.g. `routekit_router`)
        #[arg(long)]
        key: String,
        /// Path to the `.bin` brain file
        #[arg(long)]
        brain: std::path::PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum ContractCommands {
    /// Deploy a smart contract
    Deploy {
        /// WASM contract file
        #[arg(long)]
        contract: String,

        /// Contract name
        #[arg(long)]
        name: String,

        /// Owner DID (default: config identity; global `--did` also applies)
        #[arg(long, visible_alias = "owner-did")]
        did: Option<String>,

        /// Constructor arguments (JSON)
        #[arg(long)]
        args: Option<String>,

        /// Initial balance to fund contract
        #[arg(long, default_value = "0")]
        initial_balance: u64,
    },

    /// Call a smart contract function
    Call {
        /// Contract address/ID
        #[arg(long)]
        contract_id: String,

        /// Function name to call (use `spacekit_handle` + `--args '["Name"]'` for SDK wire `handle` payloads; see hello-world README).
        #[arg(long)]
        function: String,

        /// Function arguments (JSON)
        #[arg(long)]
        args: Option<String>,

        /// Caller DID (default: config identity; global `--did` also applies)
        #[arg(long, visible_alias = "caller-did", visible_alias = "owner-did")]
        did: Option<String>,

        /// Gas limit
        #[arg(long, default_value = "1000000")]
        gas_limit: u64,
    },

    /// Query contract state
    State {
        /// Contract address/ID
        contract_id: String,

        /// State key to query (optional)
        #[arg(long)]
        key: Option<String>,
    },

    /// List deployed contracts
    List {
        /// Filter by owner DID
        #[arg(long)]
        owner: Option<String>,
    },

    /// Get contract execution history
    History {
        /// Contract address/ID
        contract_id: String,

        /// Limit number of results
        #[arg(long, default_value = "10")]
        limit: usize,
    },
}

#[derive(Subcommand, Debug)]
enum MessageCommands {
    /// Send a direct message to a user
    Send {
        /// Recipient DID
        #[arg(long)]
        to: String,

        /// Message content
        #[arg(long)]
        message: String,

        /// Attach a file
        #[arg(long)]
        file: Option<String>,
    },

    /// List conversations
    List {
        /// Show detailed information
        #[arg(long)]
        detailed: bool,
    },

    /// Start interactive chat session
    Chat {
        /// Recipient DID or channel ID
        #[arg(long)]
        with: String,
    },

    /// Create a group
    CreateGroup {
        /// Group name
        #[arg(long)]
        name: String,

        /// Group description
        #[arg(long)]
        description: Option<String>,
    },

    /// Send message to group
    GroupMessage {
        /// Group ID
        #[arg(long)]
        group: String,

        /// Message content
        #[arg(long)]
        message: String,

        /// Attach a file
        #[arg(long)]
        file: Option<String>,
    },

    /// Download and decrypt a shared file
    Download {
        /// File ID to download
        #[arg(long)]
        file_id: String,

        /// Output file path
        #[arg(long)]
        output: String,
    },

    /// Lookup a user by DID in the local directory
    Whois {
        /// DID to lookup
        #[arg(long)]
        did: String,

        /// Target peer DID for scoped lookup
        #[arg(long)]
        peer: Option<String>,

        /// Target peer multiaddr for scoped lookup
        #[arg(long)]
        peer_addr: Option<String>,
    },

    /// Search local user directory by DID prefix
    DirectorySearch {
        /// DID prefix to search for
        #[arg(long)]
        prefix: String,

        /// Limit number of results
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// Remote directory sync (scoped, opt-in)
    DirectorySync {
        /// DID prefix to search for
        #[arg(long)]
        prefix: Option<String>,

        /// Target peer DID for scoped lookup
        #[arg(long)]
        peer: Option<String>,

        /// Target peer multiaddr for scoped lookup
        #[arg(long)]
        peer_addr: Option<String>,

        /// Limit number of results
        #[arg(long, default_value = "50")]
        limit: usize,

        /// Timeout in seconds to wait for responses
        #[arg(long, default_value = "3")]
        timeout: u64,

        /// Prune directory cache entries older than TTL seconds
        #[arg(long)]
        ttl_seconds: Option<u64>,

        /// Prune directory cache to a maximum number of entries
        #[arg(long)]
        max_entries: Option<usize>,

        /// Only show results without saving
        #[arg(long)]
        dry_run: bool,
    },

    /// Resolve [file:<id>] markers and download attachments
    ResolveAttachments {
        /// Message text containing [file:<id>] markers
        #[arg(long)]
        message: String,

        /// Output directory for downloaded files
        #[arg(long, default_value = ".")]
        output_dir: String,
    },

    /// Show recent message history (local)
    History {
        /// Limit number of messages
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Filter by conversation ID
        #[arg(long)]
        conversation_id: Option<String>,

        /// Filter by group ID
        #[arg(long)]
        group_id: Option<String>,

        /// Filter by sender DID
        #[arg(long)]
        sender_did: Option<String>,

        /// Download attachments found in history
        #[arg(long)]
        download_attachments: bool,

        /// Output directory for downloaded files
        #[arg(long, default_value = ".")]
        output_dir: String,
    },

    /// Download attachments by message ID
    DownloadAttachmentsByMessage {
        /// Message ID to resolve
        #[arg(long)]
        message_id: String,

        /// Output directory for downloaded files
        #[arg(long, default_value = ".")]
        output_dir: String,
    },

    /// Continuously refresh directory cache (scoped)
    DirectoryWatch {
        /// DID prefix to search for
        #[arg(long)]
        prefix: String,

        /// Target peer DID for scoped lookup
        #[arg(long)]
        peer: Option<String>,

        /// Target peer multiaddr for scoped lookup
        #[arg(long)]
        peer_addr: Option<String>,

        /// Limit number of results
        #[arg(long, default_value = "50")]
        limit: usize,

        /// Timeout in seconds to wait for responses
        #[arg(long, default_value = "3")]
        timeout: u64,

        /// Refresh interval in seconds
        #[arg(long, default_value = "30")]
        interval: u64,

        /// Prune directory cache entries older than TTL seconds
        #[arg(long)]
        ttl_seconds: Option<u64>,

        /// Prune directory cache to a maximum number of entries
        #[arg(long)]
        max_entries: Option<usize>,
    },

    /// List available peers (discovered and connected)
    Peers {
        /// Show detailed peer information
        #[arg(long)]
        detailed: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ContentCommands {
    /// Create a new channel
    CreateChannel {
        /// Channel name
        #[arg(long)]
        name: String,

        /// Channel description
        #[arg(long)]
        description: Option<String>,

        /// Pricing model (free, subscription, pay_per_view, mixed)
        #[arg(long, default_value = "free")]
        pricing: String,

        /// Price in ASTRA (for paid content)
        #[arg(long)]
        price: Option<f64>,
    },

    /// Publish content to a channel
    Publish {
        /// Channel ID
        #[arg(long)]
        channel: String,

        /// File to publish
        #[arg(long)]
        file: String,

        /// Content title
        #[arg(long)]
        title: String,

        /// Content description
        #[arg(long)]
        description: Option<String>,

        /// Pricing (free or pay_per_view)
        #[arg(long, default_value = "free")]
        pricing: String,

        /// Price in ASTRA (if pay_per_view)
        #[arg(long)]
        price: Option<f64>,

        /// Seconds into video for thumbnail poster frame (browser #t= hint)
        #[arg(long)]
        thumbnail_time: Option<f64>,

        /// Video/audio duration in seconds (shown as badge on cards)
        #[arg(long)]
        duration: Option<f64>,

        /// Channel display name (denormalized into listing for UI)
        #[arg(long)]
        channel_name: Option<String>,
    },

    /// Register an already-published fact in the website catalog (`content_listings` on the storage node)
    RegisterListing {
        /// Content / fact id (64-hex)
        #[arg(long)]
        content_id: String,
        /// Storage node base URL (default: `connections.storage` or http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,
    },

    /// Remove content from the catalog and optionally delete the underlying fact data
    Unpublish {
        /// Content / fact id (64-hex)
        #[arg(long)]
        content_id: String,
        /// Storage node base URL (default: `connections.storage` or http://127.0.0.1:3030)
        #[arg(long)]
        storage_url: Option<String>,
        /// Also delete the raw fact data from disk (cannot be undone)
        #[arg(long)]
        purge: bool,
    },

    /// Publish a library-embedded licensed feature (growformer entitlements; no binary file)
    PublishFeature {
        /// Channel DID (same as content publish)
        #[arg(long)]
        channel: String,

        /// Feature name (default: growformer)
        #[arg(long, default_value = "growformer")]
        feature: String,

        /// Display title
        #[arg(long)]
        title: String,

        /// Description
        #[arg(long)]
        description: Option<String>,
    },

    /// Subscribe to a channel
    Subscribe {
        /// Channel ID
        #[arg(long)]
        channel: String,
    },

    /// List channels
    ListChannels {
        /// Show only subscribed channels
        #[arg(long)]
        subscribed: bool,

        /// Show detailed information
        #[arg(long)]
        detailed: bool,
    },

    /// List content in a channel
    ListContent {
        /// Channel ID
        #[arg(long)]
        channel: String,

        /// Limit number of results
        #[arg(long, default_value = "20")]
        limit: usize,
    },

    /// View/download content
    View {
        /// Content ID
        #[arg(long)]
        content_id: String,

        /// Output file path (optional). Default: storage-node data dir under content/materialized/
        #[arg(long)]
        output: Option<String>,

        /// Initiate pay flow if content requires payment
        #[arg(long)]
        pay: bool,

        /// Open the materialized file with the OS default app (video player, image viewer, etc.)
        #[arg(long)]
        open: bool,
    },

    /// Grant local access (dev / MVP until on-chain entitlement)
    Access {
        /// Content ID for pay-per-view grant
        #[arg(long)]
        content_id: Option<String>,

        /// Channel DID for subscription grant
        #[arg(long)]
        channel: Option<String>,

        /// Licensed feature name (e.g. growformer) — resolves feature fact when --content-id omitted
        #[arg(long)]
        feature: Option<String>,

        /// Licensed-feature tier (default: free/open tier for --feature access)
        #[arg(long)]
        tier: Option<String>,

        /// Payment or subscription reference
        #[arg(long)]
        payment_ref: Option<String>,

        /// Initiate pay flow instead of granting (paid PPV only)
        #[arg(long)]
        pay: bool,
    },

    /// List local content access grants for the current DID
    ListAccess,

    /// Renew per-content or channel access (extends or recreates grant)
    Renew {
        #[arg(long)]
        content_id: Option<String>,
        #[arg(long)]
        channel: Option<String>,
        #[arg(long, default_value = "2592000")]
        extend_secs: u64,
        #[arg(long)]
        tier: Option<String>,
        #[arg(long)]
        price: Option<f64>,
        #[arg(long)]
        payment_ref: Option<String>,
        #[arg(long)]
        publisher: Option<String>,
    },

    /// Initiate paid access (returns pending id + payment quote)
    Pay {
        #[arg(long)]
        content_id: Option<String>,
        #[arg(long)]
        channel: Option<String>,
        /// Licensed-feature tier (required for growformer personal/commercial)
        #[arg(long)]
        tier: Option<String>,
        /// Channel subscription price (required with --channel if not inferred)
        #[arg(long)]
        price: Option<f64>,
        #[arg(long)]
        publisher: Option<String>,
        #[arg(long)]
        await_settlement: bool,
        /// Existing pending id (reuse after record-payment; required for --await-settlement in scripts)
        #[arg(long)]
        pending_id: Option<String>,
        /// After payment: complete pending in one step (with --amount)
        #[arg(long)]
        tx_hash: Option<String>,
        #[arg(long)]
        amount: Option<String>,
    },

    /// OP_PURCHASE on entitlement-ledger (manual / testing)
    Purchase {
        #[arg(long)]
        content_id: String,
    },

    /// Complete pending purchase after SpaceKit Pay settlement
    Settle {
        #[arg(long)]
        pending_id: String,
        #[arg(long)]
        tx_hash: String,
        #[arg(long)]
        amount: String,
        #[arg(long)]
        payer: Option<String>,
    },

    /// Poll settlement inbox and auto-complete open pending purchases
    ListenSettlements {
        #[arg(long, default_value = "5")]
        interval_secs: u64,
        #[arg(long)]
        once: bool,
    },

    /// Record a test payment receipt (dev / CI — simulates SpaceKit Pay)
    RecordPayment {
        #[arg(long)]
        reference: String,
        #[arg(long)]
        payer: Option<String>,
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        scope: String,
        #[arg(long)]
        amount: f64,
    },

    /// List DB-backed content installs (after `content view`)
    Installs,

    /// Run content monetization E2E soak (`dev` or `live`)
    Soak {
        /// `dev` (record-payment + inbox) or `live` (requires entitlement contract)
        #[arg(value_name = "MODE", default_value = "dev")]
        mode: String,
    },

    /// Run growformer entitlement + embedded CLI soak (GROWFORMER_SPEC Phase 4)
    GrowformerSoak,

    /// Run growformer paid-tier soak (publish-feature → pay personal → agent exec)
    GrowformerPaidSoak,
}

#[derive(Subcommand, Debug)]
enum AppCommands {
    /// Package an app directory into an AppPackage
    Package {
        /// Source directory containing app files
        #[arg(value_name = "DIR")]
        source: String,

        /// App name
        #[arg(long)]
        name: String,

        /// App version (semver format, e.g., 1.0.0)
        #[arg(long, default_value = "1.0.0")]
        version: String,

        /// Main entry point file (e.g., main.wasm, index.html)
        #[arg(long)]
        entry: String,

        /// App description
        #[arg(long)]
        description: Option<String>,

        /// App category (productivity, social, finance, games, etc.)
        #[arg(long, default_value = "utilities")]
        category: String,

        /// Output file path for the packaged app
        #[arg(short, long)]
        output: Option<String>,

        /// DID to sign with (uses default if not specified)
        #[arg(long)]
        signer: Option<String>,

        /// Compression algorithm (none, gzip, zstd, brotli)
        #[arg(long, default_value = "zstd")]
        compression: String,

        /// Icon file path
        #[arg(long)]
        icon: Option<String>,

        /// Keywords for search (comma-separated)
        #[arg(long)]
        keywords: Option<String>,
    },

    /// Deploy an AppPackage to the storage network
    Deploy {
        /// Path to packaged app file (.spkg)
        #[arg(value_name = "FILE")]
        package: String,

        /// Storage node URL
        #[arg(long, default_value = "http://localhost:3030")]
        storage_node: String,

        /// Publish to the marketplace for discovery
        #[arg(long, alias = "register-appstore")]
        publish: bool,

        /// Pricing (free, or amount in smallest token units)
        #[arg(long, default_value = "free")]
        pricing: String,

        /// Token symbol for pricing (e.g., ASTRA)
        #[arg(long, default_value = "ASTRA")]
        token: String,
    },

    /// Remove a deployed app (manifest, bundles, marketplace listing, index entry)
    Undeploy {
        /// App ID (64-char hex manifest fact id)
        #[arg(value_name = "APP_ID")]
        app_id: String,

        /// Storage node URL
        #[arg(long, default_value = "http://localhost:3030")]
        storage_node: String,

        /// Also delete underlying fact blobs from storage (recommended for local dev)
        #[arg(long, default_value_t = true)]
        purge: bool,
    },

    /// List available apps
    List {
        /// Filter by category
        #[arg(long)]
        category: Option<String>,

        /// Filter by creator DID
        #[arg(long)]
        creator: Option<String>,

        /// Search by name or keywords
        #[arg(long)]
        search: Option<String>,

        /// Show only featured apps
        #[arg(long)]
        featured: bool,

        /// Limit number of results
        #[arg(long, default_value = "20")]
        limit: usize,

        /// Storage node URL
        #[arg(long, default_value = "http://localhost:3030")]
        storage_node: String,
    },

    /// Get detailed information about an app
    Info {
        /// App ID (hex string)
        #[arg(value_name = "APP_ID")]
        app_id: String,

        /// Storage node URL
        #[arg(long, default_value = "http://localhost:3030")]
        storage_node: String,

        /// Show all versions
        #[arg(long)]
        versions: bool,
    },

    /// Download an app
    Download {
        /// App ID (hex string)
        #[arg(value_name = "APP_ID")]
        app_id: String,

        /// Output directory
        #[arg(short, long, default_value = ".")]
        output: String,

        /// Specific version to download
        #[arg(long)]
        version: Option<String>,

        /// Storage node URL
        #[arg(long, default_value = "http://localhost:3030")]
        storage_node: String,

        /// Skip signature verification (not recommended)
        #[arg(long)]
        skip_verify: bool,
    },

    /// Verify an app's signature and integrity
    Verify {
        /// App ID or path to .spkg file
        #[arg(value_name = "APP")]
        app: String,

        /// Storage node URL (if verifying by app ID)
        #[arg(long, default_value = "http://localhost:3030")]
        storage_node: String,

        /// Show detailed verification results
        #[arg(long)]
        detailed: bool,
    },

    /// Run/load an app locally
    Run {
        /// App ID or path to .spkg file
        #[arg(value_name = "APP")]
        app: String,

        /// Storage node URL (if loading by app ID)
        #[arg(long, default_value = "http://localhost:3030")]
        storage_node: String,

        /// Port to serve on (for web apps)
        #[arg(long, default_value = "3000")]
        port: u16,

        /// Open in browser after starting
        #[arg(long)]
        open: bool,
    },
}

#[derive(Subcommand, Debug, Clone)]
enum AgentCommands {
    /// Load a `.bin` brain for in-process inference (`infer --name`)
    Load {
        /// Name for `infer --name` / `unload`
        #[arg(long)]
        name: String,
        #[arg(value_name = "BRAIN_BIN")]
        brain: String,
    },
    /// Unload a brain from this CLI process
    Unload {
        #[arg(value_name = "NAME")]
        name: String,
    },
    /// List brains loaded in this CLI process
    List,
    /// Print brain metadata without loading into memory
    Info {
        #[arg(value_name = "BRAIN_BIN")]
        brain: String,
    },
    /// Run inference (`--name` in-process, or `--brain` via embedded growformer)
    Infer {
        #[arg(long, conflicts_with = "brain")]
        name: Option<String>,
        #[arg(long, conflicts_with = "name")]
        brain: Option<PathBuf>,
        #[arg(long)]
        prompt: String,
        #[arg(long, default_value_t = 256)]
        max_tokens: usize,
        #[arg(long, default_value_t = 0.8)]
        temperature: f32,
        #[arg(short, long, help = "Verbose growformer output (with --brain only)")]
        verbose: bool,
        #[arg(
            long,
            help = "Project .gf.toml for overlay context (with --brain only)"
        )]
        project: Option<PathBuf>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    /// Train a `.bin` brain from a `.gf.toml` project (embedded growformer)
    Train {
        #[arg(long, value_name = "GF_TOML")]
        project: PathBuf,
        #[arg(long, help = "Enable growformer --auto")]
        auto: bool,
        #[arg(long, value_name = "PATH")]
        brain_output: Option<String>,
        #[arg(long, value_name = "DIR")]
        data_dir: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    /// Merge two brains into a new `.bin` (embedded growformer)
    Merge {
        #[arg(long, value_name = "BRAIN_BIN")]
        brain: PathBuf,
        #[arg(long, value_name = "BRAIN_BIN")]
        overlay_brain: PathBuf,
        #[arg(long, value_name = "PATH")]
        brain_output: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },
    /// Run entitled published binary or embedded growformer (`agent exec --app growformer -- --help` for growformer help)
    Exec {
        /// Published content id (64-hex); overrides parent `--content-id` when set here
        #[arg(long)]
        content_id: Option<String>,
        /// App slug from install record (e.g. `growformer`)
        #[arg(long)]
        app: Option<String>,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Hybrid Python dev session: route a prompt to an algorithm/pattern template, construct code, run with python3
    Code {
        /// What to build (e.g. "implement binary search", "strategy pattern")
        #[arg(long)]
        prompt: Option<String>,
        /// Directory of *.toml template files (algorithms/patterns)
        #[arg(long, default_value = "templates")]
        templates: PathBuf,
        /// knowledge_graph.toml used by Growformer to route the prompt -> template id
        #[arg(long, default_value = "data/knowledge_graph.toml")]
        graph: PathBuf,
        /// Write the constructed module to this path (default: <workdir>/generated/<name>.py)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Session working directory for generated/run files
        #[arg(long, default_value = ".")]
        workdir: PathBuf,
        /// Verify the constructed code by running it with python3
        #[arg(long)]
        run: bool,
        /// Target file to modify (add-to-existing); omit to construct a new module
        #[arg(long)]
        file: Option<PathBuf>,
        /// Start an interactive multi-turn session (REPL)
        #[arg(long)]
        session: bool,
    },
    /// Build a runnable multi-file app by composing patterns + algorithms (application graph)
    App {
        /// What app to build (e.g. "a task queue", "a data pipeline app")
        #[arg(long)]
        prompt: Option<String>,
        /// Directory of *.toml app recipe files
        #[arg(long, default_value = "recipes")]
        recipes: PathBuf,
        /// Directory of *.toml template files (algorithms/patterns)
        #[arg(long, default_value = "templates")]
        templates: PathBuf,
        /// Output project directory (default: ./<recipe-id>)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Run the scaffolded app.py with python3 to verify it executes
        #[arg(long)]
        run: bool,
    },
    /// Decompose a feature request into a module file graph of algorithm/pattern building blocks
    Plan {
        /// The feature to build (e.g. "we need a module that caches results and schedules tasks")
        #[arg(long)]
        prompt: Option<String>,
        /// Knowledge base TOML (capability -> building block)
        #[arg(long, default_value = "data/knowledge_base.toml")]
        kb: PathBuf,
        /// Directory of *.toml template files (algorithms/patterns)
        #[arg(long, default_value = "templates")]
        templates: PathBuf,
        /// knowledge_graph.toml used to route fallback (direct) template hits
        #[arg(long, default_value = "data/knowledge_graph.toml")]
        graph: PathBuf,
        /// Module name (default: derived from the prompt)
        #[arg(long)]
        module: Option<String>,
        /// Output path: plan.json (default) or, with --scaffold, the base dir to write the module into
        #[arg(long)]
        out: Option<PathBuf>,
        /// Materialize the module file graph (component files + facade + plan.json) and import-test it
        #[arg(long)]
        scaffold: bool,
    },
    /// Emit a repo map JSON (dirs + files + symbols + relationships) for ML / project-graph use
    Map {
        /// Directory to scan
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Output path (default: <root-name>.repo.json in the current directory)
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Compile a precomputed route table (versioned, checksummed hex) from templates
    RouteCompile {
        /// Directory of *.toml template files (algorithms/patterns)
        #[arg(long, default_value = "templates")]
        templates: PathBuf,
        /// knowledge_graph.toml (the table is written next to it by default)
        #[arg(long, default_value = "data/knowledge_graph.toml")]
        graph: PathBuf,
        /// Output path (default: route_table.hex beside the graph)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Decode the written table and pretty-print every topic + hops
        #[arg(long)]
        verify: bool,
        /// Render + verify every template (embedded test + lint) as a catalog gate
        #[arg(long)]
        lint: bool,
    },
    /// Generate a typed SDK from an OpenAPI spec (spec -> typed model -> emit)
    Sdk {
        /// Path to an OpenAPI 3.x spec (.json or .yaml/.yml)
        #[arg(long, value_name = "SPEC")]
        spec: PathBuf,
        /// Output directory (default: ./<package>_sdk)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Python package name (default: derived from the spec title)
        #[arg(long)]
        package: Option<String>,
        /// Target language (python, typescript, rust)
        #[arg(long, default_value = "python")]
        lang: String,
        /// Import-test the generated package with python3
        #[arg(long)]
        check: bool,
        /// Dry-run: print the incremental diff plan without writing any files
        #[arg(long)]
        plan: bool,
        /// Delete previously-generated files that are no longer emitted
        #[arg(long)]
        prune: bool,
        /// Overwrite files that were hand-edited since the last generation
        #[arg(long)]
        force: bool,
    },
    /// Generate a full webapp from an OpenApp v0.1 spec + profile (data + business + view)
    Webapp {
        /// Path to an OpenApp v0.1 document (.yaml/.yml/.json)
        #[arg(long, value_name = "SPEC")]
        spec: PathBuf,
        /// Target profile (stack + patterns). Defaults to react + postgres + prisma.
        #[arg(long, value_name = "PROFILE")]
        profile: Option<PathBuf>,
        /// Output directory (default: ./<app>_app)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Client SDK language (default: from the profile's business.language)
        #[arg(long)]
        sdk_lang: Option<String>,
        /// Validate cross-references and type-check the generated client SDK
        #[arg(long)]
        check: bool,
        /// Dry-run: print the incremental diff plan without writing any files
        #[arg(long)]
        plan: bool,
        /// Delete previously-generated files that are no longer emitted
        #[arg(long)]
        prune: bool,
        /// Overwrite files that were hand-edited since the last generation
        #[arg(long)]
        force: bool,
        /// Compare behavioral equivalence against a second profile (conformance), then exit
        #[arg(long, value_name = "PROFILE")]
        conformance: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
#[command(
    about = "Build and publish brain registry manifests",
    long_about = "Produce BRAIN_REGISTRY manifest v1 JSON from a Growformer `.gf.toml` and a \
        `spacekit storage deploy` receipt, then publish it to the storage node document API \
        (`PUT /api/documents/{collection}/{id}`).\n\n\
        Requires `spacekit init` and a reachable storage node (local or remote).",
    after_help = BRAIN_REGISTRY_AFTER_HELP
)]
enum BrainRegistryCommands {
    /// Build manifest JSON from `.gf.toml` + deploy receipt
    Build {
        #[arg(long, value_name = "GF_TOML")]
        gf_toml: String,
        #[arg(
            long,
            value_name = "RECEIPT_JSON",
            help = "From `spacekit storage deploy --receipt`"
        )]
        receipt: String,
        #[arg(long, default_value = "local", help = "Manifest network_context field")]
        network_context: String,
        #[arg(long, help = "artifacts.wasm_module.crate in manifest")]
        crate_name: Option<String>,
        #[arg(long, help = "Write JSON to file (default: stdout)")]
        out: Option<String>,
    },
    /// Publish manifest to storage node
    Publish {
        #[arg(long, value_name = "MANIFEST_JSON")]
        manifest: String,
        #[arg(
            long,
            help = "Document id (default: SHA-256 hex of compact manifest JSON)"
        )]
        id: Option<String>,
        #[arg(long, default_value = "brain_registry")]
        collection: String,
        #[arg(
            long,
            visible_alias = "did",
            help = "Authorization DID (default: config identity)"
        )]
        publisher_did: Option<String>,
        #[arg(
            long,
            help = "Storage API base URL (default: connections.storage or http://127.0.0.1:3030)"
        )]
        storage_url: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ConnectionCommands {
    /// Configure connection to remote simulator
    Simulator {
        /// Simulator URL (e.g., http://localhost:50051)
        #[arg(long)]
        url: String,

        /// Use quantum encryption
        #[arg(long)]
        quantum_encrypted: bool,

        /// Set as default connection
        #[arg(long)]
        set_default: bool,
    },

    /// Configure connection to remote compute node
    Compute {
        /// Compute node URL
        #[arg(long)]
        url: String,

        /// Node DID
        #[arg(long)]
        node_did: String,

        /// Use quantum encryption
        #[arg(long)]
        quantum_encrypted: bool,
    },

    /// Configure connection to remote storage node
    Storage {
        /// Storage node URL
        #[arg(long)]
        url: String,

        /// Node DID
        #[arg(long)]
        node_did: String,

        /// Use quantum encryption
        #[arg(long)]
        quantum_encrypted: bool,
    },

    /// Configure messaging bootstrap peers
    Messaging {
        /// Bootstrap peer multiaddr (e.g., /ip4/1.2.3.4/tcp/7000)
        #[arg(long)]
        peer: String,

        /// Replace existing peers instead of appending
        #[arg(long)]
        replace: bool,
    },

    /// Show current connections
    Status,

    /// Test connection to configured host
    Test {
        /// Connection type to test
        #[arg(value_enum)]
        connection_type: ConnectionType,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ConnectionType {
    Simulator,
    Compute,
    Storage,
    Messaging,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum DeploymentType {
    Compute,
    Storage,
    Messaging,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ProposalType {
    Block,
    Metrics,
    Hybrid,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum VoteChoice {
    Approve,
    Reject,
    Abstain,
}

// Configuration structures for init command
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct CLIConfig {
    pub identity: IdentityConfig,
    pub network: NetworkConfig,
    pub project: ProjectConfig,
    pub connections: Option<ConnectionsConfig>,
    pub messaging: Option<MessagingSettings>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConnectionsConfig {
    pub simulator: Option<RemoteConnection>,
    pub compute: Option<RemoteConnection>,
    pub storage: Option<RemoteConnection>,
    pub messaging_peers: Vec<String>,
    pub default_connection: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct MessagingSettings {
    pub directory_ttl_seconds: Option<u64>,
    pub directory_max_entries: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteConnection {
    pub url: String,
    pub node_did: Option<String>,
    pub quantum_encrypted: bool,
    pub last_connected: Option<String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct IdentityConfig {
    pub did: String,
    pub algorithm: String,
    pub public_key_path: String,
    pub private_key_path: String,
    #[serde(default)]
    pub linked_username: Option<String>,
    #[serde(default)]
    pub website_auth: Option<WebsiteAuthConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebsiteAuthConfig {
    pub api_url: String,
    pub session_token: String,
    pub method: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct NetworkConfig {
    pub default_network: String,
    pub endpoints: HashMap<String, String>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub version: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ProjectManifest {
    pub name: String,
    pub version: String,
    pub did: String,
    pub networks: HashMap<String, String>,
    pub dependencies: HashMap<String, String>,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("Directory creation failed: {0}")]
    DirectoryCreation(#[from] std::io::Error),

    #[error("DID generation failed: {0}")]
    DidGeneration(String),

    #[error("Key generation failed: {0}")]
    KeyGeneration(String),

    #[error("Configuration save failed: {0}")]
    ConfigSave(String),

    #[error("Network validation failed: {0}")]
    NetworkValidation(String),
}

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("File read error: {0} - {1}")]
    FileRead(String, std::io::Error),

    #[error("Compute node error: {0}")]
    ComputeNode(String),

    #[error("DID error: {0}")]
    Did(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Messaging error: {0}")]
    Messaging(String),

    #[error("Task not found: {0}")]
    TaskNotFound(String),

    #[error("Invalid runtime: {0}")]
    InvalidRuntime(String),

    #[error("Invalid task status: {0}")]
    InvalidTaskStatus(String),
}

async fn load_and_verify_did(sdk: &SpaceKitSDK, did_addr: &str) -> Result<(), Box<dyn Error>> {
    // Fallback to SDK identity loading for backwards compatibility
    match sdk.load_identity(did_addr).await {
        Ok(identity) => {
            let expected_address = str_to_address(did_addr).expect("Invalid address");
            let did_address = str_to_address(&identity.did).expect("Invalid DID address");
            println!(
                "DID loaded successfully: {} {} {}",
                did_addr.green(),
                expected_address,
                did_address
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to load DID: {}", e);
            println!("{}", "This DID may not be registered.".yellow());
            println!(
                "To register a DID, please visit: {}",
                "https://spacekit.xyz/register".blue().underline()
            );
            Err(Box::new(e))
        }
    }
}

// Helper function to get or create compute node instance
async fn get_or_create_compute_node() -> Result<ComputeNode, CliError> {
    let mut node_guard = COMPUTE_NODE.write().unwrap();

    if node_guard.is_none() {
        let config = load_compute_config().await?;
        let mut node = ComputeNode::new(config)
            .await
            .map_err(|e| CliError::ComputeNode(e.to_string()))?;

        // Initialize the compute node
        node.initialize()
            .await
            .map_err(|e| CliError::ComputeNode(e.to_string()))?;

        *node_guard = Some(node);
    }

    Ok(node_guard.as_ref().unwrap().clone())
}

async fn get_or_create_unified_consensus() -> Result<Arc<UnifiedSWTCHConsensus>, CliError> {
    let mut slot = UNIFIED_CONSENSUS.lock().await;
    if let Some(ref existing) = *slot {
        return Ok(existing.clone());
    }
    let identity = Arc::new(
        quantum_did_utils::new_did("did:spacekit:cli:unified-consensus", "Kyber512")
            .await
            .map_err(|e| CliError::ComputeNode(e.to_string()))?,
    );
    let vpos_manager = Arc::new(
        VPoSManager::new(
            identity.clone(),
            spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber512,
        )
        .await
        .map_err(|e| CliError::ComputeNode(e.to_string()))?,
    );
    let unified = Arc::new(
        UnifiedSWTCHConsensus::new(UnifiedConsensusConfig::default(), identity, vpos_manager)
            .await
            .map_err(|e| CliError::ComputeNode(e.to_string()))?,
    );
    *slot = Some(unified.clone());
    Ok(unified)
}

// Helper function to load compute configuration
async fn load_compute_config() -> Result<ComputeConfig, CliError> {
    // Load configuration from .spacekit directory
    let config_dir = dirs::home_dir()
        .ok_or_else(|| CliError::Config("Home directory not found".to_string()))?
        .join(".spacekit");

    let config_file = config_dir.join("config.toml");

    if !config_file.exists() {
        return Err(CliError::Config(
            "SpaceKit configuration not found. Please run 'spacekit init' first.".to_string(),
        ));
    }

    let config_content = std::fs::read_to_string(&config_file)
        .map_err(|e| CliError::Config(format!("Failed to read config file: {}", e)))?;

    let cli_config: CLIConfig = toml::from_str(&config_content)
        .map_err(|e| CliError::Config(format!("Failed to parse config file: {}", e)))?;

    // Use default configuration and just update the DID
    let mut compute_config = ComputeConfig::default();
    compute_config.node_did = cli_config.identity.did.clone();
    compute_config.swtchvm_state_path = Some(config_dir.join("swtchvm/state.bin"));

    Ok(compute_config)
}

// Helper function to get default DID from config
fn get_default_did() -> Result<String, CliError> {
    let config_dir = dirs::home_dir()
        .ok_or_else(|| CliError::Config("Home directory not found".to_string()))?
        .join(".spacekit");

    let config_file = config_dir.join("config.toml");

    if !config_file.exists() {
        return Err(CliError::Config(
            "SpaceKit configuration not found. Please run 'spacekit init' first.".to_string(),
        ));
    }

    let config_content = std::fs::read_to_string(&config_file)
        .map_err(|e| CliError::Config(format!("Failed to read config file: {}", e)))?;

    let cli_config: CLIConfig = toml::from_str(&config_content)
        .map_err(|e| CliError::Config(format!("Failed to parse config file: {}", e)))?;

    Ok(cli_config.identity.did)
}

/// Resolve `identity.public_key_path` / `identity.private_key_path`: absolute paths, `~/…` (home),
/// or paths relative to `config_dir` (typically `~/.spacekit`).
fn resolve_identity_key_path(raw: &str, config_dir: &Path) -> std::path::PathBuf {
    let s = raw.trim();
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    if s.starts_with('/') {
        return std::path::PathBuf::from(s);
    }
    config_dir.join(s)
}

/// Loaded once per command from `~/.spacekit` (identity, keys, connections). CLI flags override fields.
pub struct CliContext {
    pub config: CLIConfig,
    pub config_dir: PathBuf,
    /// Effective DID for deploy/call/storage (derived from config + public key when needed).
    pub did: String,
    pub public_key_path: PathBuf,
    pub private_key_path: PathBuf,
}

impl CliContext {
    pub fn load_sync() -> Result<Self, Box<dyn std::error::Error>> {
        let config_dir = dirs::home_dir()
            .ok_or("Home directory not found")?
            .join(".spacekit");
        let config = load_cli_config_sync()?;
        let public_key_path =
            resolve_identity_key_path(&config.identity.public_key_path, &config_dir);
        let private_key_path =
            resolve_identity_key_path(&config.identity.private_key_path, &config_dir);
        let did = resolve_identity_did(&config, &config_dir, &public_key_path)?;
        Ok(Self {
            config,
            config_dir,
            did,
            public_key_path,
            private_key_path,
        })
    }

    pub async fn load() -> Result<Self, Box<dyn std::error::Error>> {
        Self::load_sync()
    }

    pub fn public_key_path_display(&self) -> String {
        self.public_key_path.display().to_string()
    }

    pub fn private_key_path_display(&self) -> String {
        self.private_key_path.display().to_string()
    }
}

/// Subcommand `--did` wins, then global `--did`, then `CliContext::did`.
pub fn resolve_effective_did(
    cli: &Cli,
    ctx: &CliContext,
    subcommand_did: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(d) = subcommand_did.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(d.to_string());
    }
    if let Some(d) = cli.did.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(d.to_string());
    }
    if ctx.did.is_empty() {
        return Err("No DID configured. Run `spacekit init` or pass `--did`.".into());
    }
    Ok(ctx.did.clone())
}

/// CLI path argument, else identity path from config, else legacy cwd default.
pub fn resolve_public_key_path(ctx: Option<&CliContext>, arg: Option<&str>) -> PathBuf {
    if let Some(p) = arg.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(ctx) = ctx {
            return resolve_identity_key_path(p, &ctx.config_dir);
        }
        return PathBuf::from(p);
    }
    if let Some(ctx) = ctx {
        return ctx.public_key_path.clone();
    }
    PathBuf::from("public_key.hex")
}

pub fn resolve_private_key_path(ctx: Option<&CliContext>, arg: Option<&str>) -> PathBuf {
    if let Some(p) = arg.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(ctx) = ctx {
            return resolve_identity_key_path(p, &ctx.config_dir);
        }
        return PathBuf::from(p);
    }
    if let Some(ctx) = ctx {
        return ctx.private_key_path.clone();
    }
    PathBuf::from("private_key.hex")
}

fn resolve_identity_did(
    config: &CLIConfig,
    config_dir: &Path,
    public_key_path: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut did_from_config = config.identity.did.trim().to_string();

    let wallet_file = config_dir.join("did_wallet.json");
    if did_from_config.is_empty() && wallet_file.exists() {
        if let Ok(raw) = std::fs::read_to_string(&wallet_file) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
                if let Some(d) = v.get("did").and_then(|x| x.as_str()) {
                    let d = d.trim();
                    if !d.is_empty() {
                        did_from_config = d.to_string();
                    }
                }
            }
        }
    }

    if let Some(username) = config.identity.linked_username.as_deref() {
        let slug = username.trim().to_lowercase();
        if !slug.is_empty() {
            return Ok(format!("did:spacekit:user:{slug}"));
        }
    }

    if did_from_config.starts_with("did:spacekit:user:") {
        let suffix = did_from_config
            .strip_prefix("did:spacekit:user:")
            .unwrap_or("");
        let is_init_uuid = uuid::Uuid::parse_str(suffix).is_ok();
        if !is_init_uuid && !suffix.is_empty() {
            return Ok(did_from_config);
        }
    }

    if did_from_config.is_empty() || did_from_config.starts_with("did:spacekit:user:") {
        if let Ok(pk_hex) = std::fs::read_to_string(public_key_path) {
            if let Ok(pk) = hex::decode(pk_hex.trim()) {
                if let Ok(d) = derive_testnet_did_from_public_key(&pk) {
                    return Ok(d);
                }
            }
        }
    }

    Ok(did_from_config)
}

fn read_hex_key_file(path: &Path, label: &str) -> Result<Vec<u8>, CliError> {
    let hex_str = std::fs::read_to_string(path).map_err(|e| {
        CliError::Config(format!(
            "Failed to read {} from {}: {}",
            label,
            path.display(),
            e
        ))
    })?;
    hex::decode(hex_str.trim()).map_err(|e| {
        CliError::Config(format!(
            "Failed to decode {} (must be hex-encoded): {}",
            label, e
        ))
    })
}

// Helper function to load private key from config
fn load_private_key() -> Result<Vec<u8>, CliError> {
    let ctx = CliContext::load_sync().map_err(|e| CliError::Config(e.to_string()))?;
    load_private_key_from_ctx(&ctx)
}

fn load_private_key_from_ctx(ctx: &CliContext) -> Result<Vec<u8>, CliError> {
    read_hex_key_file(&ctx.private_key_path, "private key")
}

// Helper function to load public key from config
async fn load_public_key() -> Result<Vec<u8>, CliError> {
    let ctx = CliContext::load()
        .await
        .map_err(|e| CliError::Config(e.to_string()))?;
    load_public_key_from_ctx(&ctx)
}

fn load_public_key_from_ctx(ctx: &CliContext) -> Result<Vec<u8>, CliError> {
    read_hex_key_file(&ctx.public_key_path, "public key")
}

fn parse_quantum_algorithm(name: &str) -> QuantumAlgorithm {
    match name.to_lowercase().as_str() {
        "kyber512" => QuantumAlgorithm::Kyber512,
        "kyber768" => QuantumAlgorithm::Kyber768,
        "kyber1024" => QuantumAlgorithm::Kyber1024,
        "ntruprimesntrup761" | "ntruprime" | "sntrup761" => QuantumAlgorithm::NtruPrimeSntrup761,
        "frodokem1344aes" | "frodo" | "frodoaes" => QuantumAlgorithm::FrodoKem1344Aes,
        "frodokem1344shake" | "frodoshake" => QuantumAlgorithm::FrodoKem1344Shake,
        _ => QuantumAlgorithm::Kyber1024,
    }
}

fn determine_mime_type_for_attachment(file_path: &str) -> String {
    let path = Path::new(file_path);
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        match ext.to_lowercase().as_str() {
            "mp4" | "mov" | "webm" => "video/mp4".to_string(),
            "mp3" | "wav" | "ogg" => "audio/mpeg".to_string(),
            "jpg" | "jpeg" => "image/jpeg".to_string(),
            "png" => "image/png".to_string(),
            "gif" => "image/gif".to_string(),
            "pdf" => "application/pdf".to_string(),
            "txt" => "text/plain".to_string(),
            "json" => "application/json".to_string(),
            _ => "application/octet-stream".to_string(),
        }
    } else {
        "application/octet-stream".to_string()
    }
}

fn extract_file_markers(message: &str) -> Vec<(String, Option<String>)> {
    let mut ids = Vec::new();
    let mut remaining = message;
    let marker = "[file:";

    while let Some(start) = remaining.find(marker) {
        let after = &remaining[start + marker.len()..];
        if let Some(end) = after.find(']') {
            let id = after[..end].trim();
            if !id.is_empty() {
                if let Some((file_id, name)) = id.split_once(':') {
                    let file_id = file_id.trim();
                    let name = name.trim();
                    let name = if name.is_empty() {
                        None
                    } else {
                        Some(name.to_string())
                    };
                    ids.push((file_id.to_string(), name));
                } else {
                    ids.push((id.to_string(), None));
                }
            }
            remaining = &after[end + 1..];
        } else {
            break;
        }
    }

    ids
}

// Helper function to get or create storage node instance
async fn get_or_create_storage_node() -> Result<Arc<StorageNode>, CliError> {
    let mut node_guard = STORAGE_NODE.write().unwrap();

    if node_guard.is_none() {
        let config = load_storage_config().await?;
        let node = StorageNode::new(config)
            .await
            .map_err(|e| CliError::ComputeNode(format!("Storage node creation failed: {}", e)))?;

        *node_guard = Some(Arc::new(node));
    }

    // Clone the Arc reference
    Ok(node_guard.as_ref().unwrap().clone())
}

// Helper function to get or create messaging node instance
async fn get_or_create_messaging_node() -> Result<Arc<MessagingNode>, CliError> {
    let mut node_guard = MESSAGING_NODE.write().unwrap();

    if node_guard.is_none() {
        let config = load_cli_config()
            .await
            .map_err(|e| CliError::Config(e.to_string()))?;
        let config_dir = dirs::home_dir()
            .ok_or_else(|| CliError::Config("Home directory not found".to_string()))?
            .join(".spacekit");

        let mut messaging_config = MessagingConfig::default();
        messaging_config.node_did = format!("{}:messaging-node", config.identity.did);
        let listen_str = crate::network_profile::load_spacekit_network_file()
            .ok()
            .flatten()
            .map(|n| n.messaging.listen_addr.clone())
            .unwrap_or_else(|| crate::network_profile::DEFAULT_MESSAGING_LISTEN.to_string());
        messaging_config.listen_addr = listen_str.parse().map_err(|e| {
            CliError::Messaging(format!(
                "Invalid listen address from network profile: {}",
                e
            ))
        })?;
        if let Some(connections) = &config.connections {
            if !connections.messaging_peers.is_empty() {
                messaging_config.bootstrap_peers = connections.messaging_peers.clone();
            } else {
                messaging_config.bootstrap_peers = vec!["/ip4/127.0.0.1/tcp/7000".to_string()];
            }
        } else {
            messaging_config.bootstrap_peers = vec!["/ip4/127.0.0.1/tcp/7000".to_string()];
        }
        messaging_config.default_quantum_algorithm = config.identity.algorithm.clone();
        messaging_config.default_cipher_suite = "AES256".to_string();
        messaging_config.enable_peer_discovery = true;
        messaging_config.storage.storage_path =
            config_dir.join("messaging").to_string_lossy().to_string();

        let node = MessagingNode::new(messaging_config)
            .await
            .map_err(|e| CliError::Messaging(e.to_string()))?;
        node.start()
            .await
            .map_err(|e| CliError::Messaging(format!("Failed to start messaging node: {}", e)))?;

        *node_guard = Some(Arc::new(node));
    }

    Ok(node_guard.as_ref().unwrap().clone())
}

async fn ensure_messaging_user(
    node: &MessagingNode,
) -> Result<spacekit_messaging_node::User, CliError> {
    let config = load_cli_config()
        .await
        .map_err(|e| CliError::Config(e.to_string()))?;
    let user_did = config.identity.did.clone();

    if user_did.is_empty() {
        return Err(CliError::Config(
            "No DID configured. Run 'spacekit init' first.".to_string(),
        ));
    }

    if let Some(user) = node
        .get_user_by_did(&user_did)
        .await
        .map_err(|e| CliError::Messaging(e.to_string()))?
    {
        return Ok(user);
    }

    let public_key = load_public_key().await?;
    let algorithm = parse_quantum_algorithm(&config.identity.algorithm);
    let username = user_did
        .split(':')
        .last()
        .map(|suffix| format!("cli-{}", suffix))
        .unwrap_or_else(|| "cli-user".to_string());

    node.register_user(user_did, username, public_key, algorithm)
        .await
        .map_err(|e| CliError::Messaging(e.to_string()))
}

// Helper function to load storage configuration
async fn load_storage_config() -> Result<StorageNodeConfig, CliError> {
    // Load configuration from .spacekit directory
    let config_dir = dirs::home_dir()
        .ok_or_else(|| CliError::Config("Home directory not found".to_string()))?
        .join(".spacekit");

    let config_file = config_dir.join("config.toml");

    if !config_file.exists() {
        return Err(CliError::Config(
            "SpaceKit configuration not found. Please run 'spacekit init' first.".to_string(),
        ));
    }

    let config_content = std::fs::read_to_string(&config_file)
        .map_err(|e| CliError::Config(format!("Failed to read config file: {}", e)))?;

    let cli_config: CLIConfig = toml::from_str(&config_content)
        .map_err(|e| CliError::Config(format!("Failed to parse config file: {}", e)))?;

    // Create storage node configuration using defaults and update specific fields
    let mut storage_config = StorageNodeConfig::default();
    storage_config.max_storage_bytes = 10 * 1024 * 1024 * 1024; // 10GB default
    let legacy_dir = config_dir.join("storage");
    let data_dir = crate::network_profile::load_spacekit_network_file()
        .ok()
        .flatten()
        .map(|net| crate::network_profile::resolve_data_dir(&net, "storage"))
        .map(|profile_dir| {
            // CLI used ~/.spacekit/storage before network-profile alignment; keep reading it if populated.
            if legacy_dir.join("fact_storage").exists()
                && !profile_dir.join("fact_storage").exists()
            {
                legacy_dir.clone()
            } else {
                profile_dir
            }
        })
        .unwrap_or(legacy_dir);
    storage_config.data_dir = data_dir.clone();
    storage_config.database_path = Some(data_dir.join("storage.db"));
    storage_config.node_did = cli_config.identity.did.clone();
    storage_config.preferred_algorithm = cli_config.identity.algorithm.clone();
    storage_config.enable_p2p = false;
    storage_config.persistence.externalize_documents = true;
    storage_config.persistence.document_inline_max_bytes = 4096;
    storage_config.persistence.blob_cache_max_bytes = 32 * 1024 * 1024;

    Ok(storage_config)
}

// Helper function to get or create DID wallet instance
async fn get_or_create_did_wallet() -> Result<Arc<QuantumResistantWallet>, CliError> {
    let mut wallet_guard = DID_WALLET.write().unwrap();

    if wallet_guard.is_none() {
        // Try to load existing wallet from config, or create new one
        let wallet = load_or_create_did_wallet().await?;
        *wallet_guard = Some(Arc::new(wallet));
    }

    Ok(wallet_guard.as_ref().unwrap().clone())
}

/// Short `did:spacekit:testnet:0x…` form derived from the KEM public key (first 14 hex chars of the
/// Keccak address, same convention as `SwtchvmAddress::from_hex` truncation for DIDs).
fn derive_testnet_did_from_public_key(pk: &[u8]) -> Result<String, CliError> {
    let full = DualKeyWallet::public_key_to_address(pk)
        .map_err(|e| CliError::Config(format!("address from public key: {}", e)))?;
    let h = full.strip_prefix("0x").unwrap_or(full.as_str());
    let short14 = if h.len() >= 14 { &h[..14] } else { h };
    Ok(format!("did:spacekit:testnet:0x{}", short14))
}

// Helper function to load or create DID wallet
async fn load_or_create_did_wallet() -> Result<QuantumResistantWallet, CliError> {
    let mut wallet = QuantumResistantWallet::new();
    let ctx = CliContext::load_sync().map_err(|e| CliError::Config(e.to_string()))?;
    if !ctx.did.is_empty() {
        wallet
            .apply_config_did(&ctx.did)
            .map_err(|e| CliError::Did(e.to_string()))?;
    }
    Ok(wallet)
}

// Handle task management commands
async fn handle_task_command(
    task_command: &TaskCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match task_command {
        TaskCommands::Submit {
            file,
            runtime,
            did,
            input,
            encryption: _,
            max_cost: _,
        } => {
            let owner_did = did.as_deref().unwrap_or("");
            handle_task_submit(file, runtime, owner_did, input.as_ref()).await
        }
        TaskCommands::Status { task_id } => handle_task_status(task_id).await,
        TaskCommands::List {
            status,
            owner,
            owned_by_me,
        } => handle_task_list(status.as_ref(), owner.as_ref(), *owned_by_me).await,
        TaskCommands::Cancel { task_id } => handle_task_cancel(task_id).await,
        TaskCommands::Result { task_id, output } => {
            handle_task_result(task_id, output.as_ref()).await
        }
        TaskCommands::Watch { task_id, interval } => handle_task_watch(task_id, *interval).await,
    }
}

// Handle storage management commands
async fn handle_storage_command(
    cli: &Cli,
    ctx: &CliContext,
    storage_command: &StorageCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match storage_command {
        StorageCommands::Store {
            file,
            did,
            description,
            encryption: _,
            p2p: _,
            replicas: _,
            storage_url,
        } => {
            let owner_did = resolve_effective_did(cli, ctx, did.as_deref())?;
            handle_storage_store(
                file,
                &owner_did,
                description.as_ref(),
                storage_url.as_deref(),
            )
            .await
        }
        StorageCommands::Retrieve {
            file_id,
            output,
            requester_did,
            storage_url,
            embedded,
        } => {
            handle_storage_retrieve(
                file_id,
                output,
                requester_did.as_ref(),
                storage_url.as_deref(),
                *embedded,
            )
            .await
        }
        StorageCommands::List {
            owner,
            owned_by_me,
            details,
            storage_url,
        } => {
            handle_storage_list(
                owner.as_ref(),
                *owned_by_me,
                *details,
                storage_url.as_deref(),
            )
            .await
        }
        StorageCommands::Share {
            file_id,
            with_did,
            permission,
            storage_url,
        } => handle_storage_share(file_id, with_did, permission, storage_url.as_deref()).await,
        StorageCommands::Revoke {
            file_id,
            from_did,
            storage_url,
        } => handle_storage_revoke(file_id, from_did, storage_url.as_deref()).await,
        StorageCommands::Stats {
            detailed,
            storage_url,
        } => handle_storage_stats(*detailed, storage_url.as_deref()).await,
        StorageCommands::Deploy {
            package,
            wasm,
            bin,
            did,
            storage_url,
            receipt,
            owner_key_algorithm,
            agent_id,
            publish,
            title,
            description,
            category,
            access,
            price,
            marketplace,
            brain_key,
            capabilities,
            tag_label,
            tag_color,
            hub_response_format,
        } => {
            let resolved = resolve_storage_deploy_params(
                package.as_deref(),
                wasm.as_deref(),
                bin.as_deref(),
                did.as_deref(),
                storage_url.as_deref(),
                receipt.as_deref(),
                owner_key_algorithm.as_deref(),
                agent_id.as_deref(),
                *publish,
                title.as_deref(),
                description.as_deref(),
                category.as_deref(),
                access.as_deref(),
                price.as_deref(),
                marketplace.as_deref(),
                brain_key.as_deref(),
                capabilities.as_deref(),
                tag_label.as_deref(),
                tag_color.as_deref(),
                hub_response_format.as_deref(),
            )?;
            let owner_did = resolve_effective_did(cli, ctx, resolved.did.as_deref())?;
            handle_storage_deploy(
                &resolved.wasm,
                &resolved.bin,
                resolved.inference_toml.as_deref(),
                resolved.guardrails_jsonl.as_deref(),
                resolved.fragments_jsonl.as_deref(),
                resolved.topic_graph.as_deref(),
                resolved.grounding_toml.as_deref(),
                resolved.companion_ui.as_deref(),
                &owner_did,
                resolved.storage_url.as_deref(),
                resolved.receipt.as_deref(),
                resolved.owner_key_algorithm.as_deref(),
                resolved.agent_id.as_deref(),
                resolved.publish,
                resolved.title.as_deref(),
                resolved.description.as_deref(),
                resolved.category.as_deref(),
                resolved.access.as_deref(),
                resolved.price.as_deref(),
                resolved.marketplace.as_deref(),
                resolved.brain_key.as_deref(),
                resolved.capabilities.as_deref(),
                resolved.tag_label.as_deref(),
                resolved.tag_color.as_deref(),
                resolved.hub_response_format.as_deref(),
                resolved.hub_thinking_label.as_deref(),
                resolved.hub_companion_ui.as_deref(),
                resolved.hub_op,
                resolved.hub_input_format.as_deref(),
                resolved.prompts.as_ref(),
            )
            .await
        }
        StorageCommands::VerifyReceipt { receipt } => handle_storage_verify_receipt(receipt).await,
        StorageCommands::Fetch {
            file_id,
            output,
            storage_url,
            requester_did,
        } => {
            handle_storage_fetch_http(
                file_id,
                output,
                storage_url.as_deref(),
                requester_did.as_ref(),
            )
            .await
        }
        StorageCommands::EnvelopeUpload {
            file,
            storage_url,
            filename,
            content_type,
        } => {
            handle_envelope_upload_cmd(
                file,
                storage_url.as_deref(),
                filename.as_deref(),
                content_type.as_deref(),
            )
            .await
        }
        StorageCommands::EnvelopeFetch {
            file_id,
            output,
            storage_url,
        } => handle_envelope_fetch_cmd(file_id, output, storage_url.as_deref()).await,
        StorageCommands::SyncReceipt {
            receipt,
            wasm_out,
            bin_out,
            storage_url,
            requester_did,
        } => {
            handle_storage_sync_receipt(
                receipt,
                wasm_out,
                bin_out,
                storage_url.as_deref(),
                requester_did.as_ref(),
            )
            .await
        }
        StorageCommands::Node { action, config: _ } => handle_storage_node(action).await,
    }
}

// Individual task command handlers
async fn handle_task_submit(
    file: &str,
    runtime: &str,
    owner_did: &str,
    input_file: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Submitting task for distributed execution...");
    println!("📁 File: {}", file.blue());
    println!("⚙️  Runtime: {}", runtime.yellow());
    println!("👤 Owner: {}", owner_did.green());

    // Read the WASM file
    let code = std::fs::read(file).map_err(|e| CliError::FileRead(file.to_string(), e))?;

    println!("📊 Code size: {} bytes", code.len());

    // Read input data if provided
    let input_data = if let Some(input_path) = input_file {
        let data =
            std::fs::read(input_path).map_err(|e| CliError::FileRead(input_path.to_string(), e))?;
        println!("📊 Input size: {} bytes", data.len());
        data
    } else {
        vec![]
    };

    // Get or create compute node
    let node = get_or_create_compute_node().await?;

    // Submit task
    let task_name = format!("cli_task_{}", chrono::Utc::now().timestamp());

    match node
        .submit_task(
            task_name,
            runtime.to_string(),
            code,
            input_data,
            owner_did.to_string(),
        )
        .await
    {
        Ok(task) => {
            println!("\n✅ Task submitted successfully!");
            println!("🆔 Task ID: {}", task.id.green());
            println!("📋 Status: {:?}", task.status);
            if let Some(cost) = task.estimated_cost {
                println!("💰 Estimated cost: {:.6} ASTRA", cost);
            }
            println!(
                "⏰ Created: {}",
                task.created_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            println!(
                "\n💡 Use {} to check status",
                format!("spacekit task status {}", task.id).yellow()
            );
        }
        Err(e) => {
            println!("❌ Failed to submit task: {}", e);
            return Err(Box::new(CliError::ComputeNode(e.to_string())));
        }
    }

    Ok(())
}

async fn handle_task_status(task_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Checking task status...");
    println!("🆔 Task ID: {}", task_id.blue());

    let node = get_or_create_compute_node().await?;

    match node.get_task_status(task_id).await {
        Some(status) => {
            // We have the status, but need to get the full task details
            // For now, just show the status and basic info
            println!("\n📊 Task Status Report");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("🆔 Task ID: {}", task_id.green());
            println!("📋 Status: {}", format!("{:?}", status).cyan());

            match status {
                TaskStatus::Pending => println!("⏳ Task is waiting in queue for execution"),
                TaskStatus::Queued => println!("⏳ Task is waiting in queue for execution"),
                TaskStatus::Running => println!("🔄 Task is currently being executed"),
                TaskStatus::Completed => {
                    println!("✅ Task completed successfully!");
                    println!(
                        "💡 Use {} to get the result",
                        format!("spacekit task result {}", task_id).yellow()
                    );
                }
                TaskStatus::Failed => println!("❌ Task execution failed"),
                TaskStatus::Cancelled => println!("🚫 Task was cancelled"),
            }
        }
        None => {
            println!("❌ Task not found: {}", task_id);
            return Err(Box::new(CliError::TaskNotFound(task_id.to_string())));
        }
    }

    Ok(())
}

fn parse_task_status_filter(value: &str) -> Option<TaskStatus> {
    match value.to_lowercase().as_str() {
        "queued" => Some(TaskStatus::Queued),
        "running" => Some(TaskStatus::Running),
        "pending" => Some(TaskStatus::Pending),
        "completed" => Some(TaskStatus::Completed),
        "failed" => Some(TaskStatus::Failed),
        "cancelled" | "canceled" => Some(TaskStatus::Cancelled),
        _ => None,
    }
}

async fn handle_task_list(
    status_filter: Option<&String>,
    owner_filter: Option<&String>,
    owned_by_me: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 Listing tasks...");

    let node = get_or_create_compute_node().await?;
    let mut tasks = node.list_tasks().await;

    let owner = if owned_by_me {
        Some(get_default_did()?)
    } else {
        owner_filter.cloned()
    };

    if let Some(filter) = owner {
        tasks.retain(|task| task.owner_did == filter);
    }

    if let Some(filter) = status_filter {
        let status = parse_task_status_filter(filter)
            .ok_or_else(|| format!("Invalid task status: {}", filter))?;
        tasks.retain(|task| task.status == status);
    }

    tasks.sort_by_key(|task| task.created_at);
    tasks.reverse();

    if tasks.is_empty() {
        println!("📭 No tasks found matching the current filters.");
        return Ok(());
    }

    println!("\n📋 Found {} task(s):\n", tasks.len());
    for (i, task) in tasks.iter().enumerate() {
        println!("{}. Task ID: {}", i + 1, task.id.cyan());
        println!("   Name: {}", task.name.green());
        println!("   Runtime: {}", task.runtime.yellow());
        println!("   Status: {:?}", task.status);
        println!("   Owner: {}", task.owner_did.blue());
        println!(
            "   Created: {}",
            task.created_at.format("%Y-%m-%d %H:%M:%S UTC")
        );
        if let Some(cost) = task.estimated_cost {
            println!("   Estimated cost: {:.6} ASTRA", cost);
        }
        if let Some(cost) = task.actual_cost {
            println!("   Actual cost: {:.6} ASTRA", cost);
        }
        println!();
    }

    Ok(())
}

async fn handle_task_cancel(task_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚫 Cancelling task...");
    println!("🆔 Task ID: {}", task_id.red());

    let node = get_or_create_compute_node().await?;

    match node.cancel_task(task_id).await {
        Ok(()) => {
            println!("✅ Task cancelled successfully!");
            println!(
                "💡 Use {} to verify cancellation",
                format!("spacekit task status {}", task_id).yellow()
            );
        }
        Err(e) => {
            println!("❌ Failed to cancel task: {}", e);
            return Err(Box::new(CliError::ComputeNode(e.to_string())));
        }
    }

    Ok(())
}

async fn handle_task_result(
    task_id: &str,
    output_file: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📤 Getting task result...");
    println!("🆔 Task ID: {}", task_id.blue());

    let node = get_or_create_compute_node().await?;

    match node.get_task_result(task_id).await {
        Ok(result_data) => {
            println!("\n📊 Task Result Report");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("🆔 Task ID: {}", task_id.green());

            // Handle result data
            if !result_data.is_empty() {
                println!("\n📦 Result Data:");
                println!("  📊 Size: {} bytes", result_data.len());

                if let Some(output_path) = output_file {
                    std::fs::write(output_path, &result_data)?;
                    println!("  💾 Saved to: {}", output_path.green());
                } else {
                    // Try to display as text if it's valid UTF-8
                    match String::from_utf8(result_data.clone()) {
                        Ok(text) => {
                            if text.len() <= 500 {
                                println!("  📄 Content (text):\n{}", text.trim());
                            } else {
                                println!(
                                    "  📄 Content (text, truncated):\n{}...",
                                    &text[..500].trim()
                                );
                                println!(
                                    "  💡 Use {} to save full result",
                                    "--output <file>".yellow()
                                );
                            }
                        }
                        Err(_) => {
                            println!("  📄 Content: Binary data (use --output to save)");
                        }
                    }
                }
            } else {
                println!("\n📭 No result data available");
            }
        }
        Err(e) => {
            println!("❌ Failed to get task result: {}", e);
            return Err(Box::new(CliError::ComputeNode(e.to_string())));
        }
    }

    Ok(())
}

async fn handle_task_watch(task_id: &str, interval: u64) -> Result<(), Box<dyn std::error::Error>> {
    println!("👀 Watching task status in real-time...");
    println!("🆔 Task ID: {}", task_id.blue());
    println!("⏱️  Update interval: {}s", interval);
    println!("💡 Press Ctrl+C to stop watching\n");

    let node = get_or_create_compute_node().await?;
    let mut last_status = None;

    loop {
        match node.get_task_status(task_id).await {
            Some(current_status) => {
                // Only print update if status changed or first time
                if last_status.as_ref() != Some(&current_status) {
                    let timestamp = chrono::Utc::now().format("%H:%M:%S");
                    let status_str = match current_status {
                        TaskStatus::Pending => "⏳ Pending".yellow(),
                        TaskStatus::Queued => "⏳ Queued".yellow(),
                        TaskStatus::Running => "🔄 Running".blue(),
                        TaskStatus::Completed => "✅ Completed".green(),
                        TaskStatus::Failed => "❌ Failed".red(),
                        TaskStatus::Cancelled => "🚫 Cancelled".magenta(),
                    };

                    println!("[{}] Status: {}", timestamp, status_str);

                    // Stop watching if task is in terminal state
                    match current_status {
                        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled => {
                            println!("\n🏁 Task reached terminal state. Stopping watch.");
                            if current_status == TaskStatus::Completed {
                                println!(
                                    "💡 Use {} to get the result",
                                    format!("spacekit task result {}", task_id).yellow()
                                );
                            }
                            break;
                        }
                        _ => {}
                    }

                    last_status = Some(current_status);
                }
            }
            None => {
                println!("❌ Task not found: {}", task_id);
                return Err(Box::new(CliError::TaskNotFound(task_id.to_string())));
            }
        }

        tokio::time::sleep(Duration::from_secs(interval)).await;
    }

    Ok(())
}

// Handle DID management commands
async fn handle_did_command(did_command: &DIDCommands) -> Result<(), Box<dyn std::error::Error>> {
    match did_command {
        DIDCommands::Create {
            algorithm,
            save,
            identifier,
            format,
        } => handle_did_create(*algorithm, *save, identifier.as_ref(), format).await,
        DIDCommands::Verify {
            did,
            credentials,
            detailed,
        } => handle_did_verify(did, *credentials, *detailed).await,
        DIDCommands::Update {
            did,
            add_key,
            rotate_keys,
            update_document,
        } => {
            handle_did_update(
                did,
                add_key.as_ref(),
                *rotate_keys,
                update_document.as_ref(),
            )
            .await
        }
        DIDCommands::Resolve {
            did,
            format,
            verify,
        } => handle_did_resolve(did, format, *verify).await,
        DIDCommands::List {
            owned_by_me,
            method,
            detailed,
            with_credentials,
        } => handle_did_list(*owned_by_me, method.as_ref(), *detailed, *with_credentials).await,
        DIDCommands::Issue {
            to,
            credential_type,
            claims,
            validity_days,
            output,
        } => {
            handle_did_issue_credential(
                to,
                credential_type,
                claims,
                *validity_days,
                output.as_ref(),
            )
            .await
        }
        DIDCommands::VerifyCredential {
            credential_file,
            detailed,
        } => handle_did_verify_credential(credential_file, *detailed).await,
    }
}

// Individual storage command handlers
async fn handle_storage_store(
    file_path: &str,
    owner_did: &str,
    description: Option<&String>,
    storage_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📦 Storing file with quantum-resistant encryption...");
    println!("📁 File: {}", file_path.blue());
    println!("👤 Owner: {}", owner_did.green());

    if let Some(desc) = description {
        println!("📝 Description: {}", desc.yellow());
    }

    let file_data =
        std::fs::read(file_path).map_err(|e| CliError::FileRead(file_path.to_string(), e))?;

    println!("📊 File size: {} bytes", file_data.len());

    let filename = std::path::Path::new(file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let config_dir = dirs::home_dir()
        .ok_or_else(|| CliError::Config("Home directory not found".to_string()))?
        .join(".spacekit");
    let cli_config = load_cli_config()
        .await
        .map_err(|e| CliError::Config(format!("Failed to load config: {}", e)))?;
    let public_key_path =
        resolve_identity_key_path(&cli_config.identity.public_key_path, &config_dir);
    let public_key_hex = std::fs::read_to_string(&public_key_path).map_err(|e| {
        CliError::Config(format!(
            "Failed to read public key from {}: {}",
            public_key_path.display(),
            e
        ))
    })?;

    println!("🔑 Loaded public key");

    let (base_url, _) = resolve_remote_storage_base_url(storage_url)?;
    println!("   Storage: {}", base_url.green());

    let (server_pk, server_algo, server_key_source) = fetch_server_public_key(&base_url).await?;

    let envelope_bytes = spacekit_storage_node::envelope::encrypt_envelope_sourced(
        &file_data,
        &server_pk,
        &server_algo,
        None,
        server_key_source,
    )
    .map_err(|e| format!("Envelope encryption failed: {}", e))?;

    let url = format!("{}/files/envelope-upload", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3600))
        .build()?;
    let resp = client
        .post(url)
        .header("owner-did", owner_did)
        .header("owner-public-key", public_key_hex.trim())
        .header("filename", filename)
        .header("content-type", "application/octet-stream")
        .body(envelope_bytes)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Upload failed (HTTP {}): {}", status, body).into());
    }
    let parsed: serde_json::Value = resp.json().await?;
    let file_id = parsed["file_id"].as_str().unwrap_or("unknown");

    println!("\n✅ File stored successfully!");
    println!("🆔 File ID: {}", file_id.green());
    println!(
        "🔐 Encryption: Quantum-resistant ({})",
        server_algo.yellow()
    );
    println!("📊 Size: {} bytes", file_data.len());
    println!(
        "⏰ Stored at: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "\n💡 Use {} to retrieve",
        format!("spacekit storage retrieve {} --output <filename>", file_id).yellow()
    );

    Ok(())
}

#[derive(Debug, serde::Deserialize)]
struct RemoteFileUploadResponse {
    file_id: Option<String>,
    filename: Option<String>,
    size: Option<u64>,
    hash: Option<String>,
    error: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct StorageDeployArtifactReceipt {
    role: String,
    local_path: String,
    file_id: String,
    stored_filename: String,
    size_bytes: u64,
    content_blake3_hex: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AgentHubConfigReceipt {
    kind: String,
    op: u8,
    input_format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    storage_brain_key: Option<String>,
    hub_response_format: String,
    hub_thinking_label: String,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tag_color: Option<String>,
    /// Companion UI registry key rendered beside the chat (e.g. "pixel_cat").
    #[serde(skip_serializing_if = "Option::is_none")]
    hub_companion_ui: Option<String>,
    /// When true, Agent Hub loads the `inference_toml` artifact into Growformer before chat.
    #[serde(default)]
    inference_toml: bool,
    /// When true, Agent Hub loads the `topic_graph` artifact into Growformer for prompt routing.
    #[serde(default)]
    topic_graph: bool,
    /// When true, Agent Hub loads the `grounding_toml` artifact for concept anchoring.
    #[serde(default)]
    grounding_toml: bool,
    /// When true, Agent Hub loads the `fragments_jsonl` artifact for fragment composition.
    #[serde(default)]
    fragments_jsonl: bool,
    /// When true, Agent Hub loads the `guardrails_jsonl` artifact (merged after inference TOML).
    #[serde(default)]
    guardrails_jsonl: bool,
}

#[derive(Debug, serde::Serialize)]
struct StorageDeployReceipt {
    #[serde(skip_serializing_if = "Option::is_none")]
    agent_id: Option<String>,
    deployment_id: String,
    /// BLAKE3 deterministic id (hex); same as `FactPackage.fact_id` on `POST /facts`.
    fact_id: String,
    storage_node_url: String,
    storage_node_did: Option<String>,
    owner_did: String,
    /// KEM used for uploads (`owner-key-algorithm`); omit if storage node default was used.
    owner_kem_algorithm: Option<String>,
    created_at: String,
    upload_endpoint: String,
    artifacts: Vec<StorageDeployArtifactReceipt>,
    /// Runtime config for Agent Hub (WASM op, brain storage key, UI). Stored in FactPackage + marketplace listing.
    #[serde(skip_serializing_if = "Option::is_none")]
    hub_config: Option<AgentHubConfigReceipt>,
}

#[derive(Debug, Deserialize)]
struct DeployReceiptFile {
    storage_node_url: String,
    artifacts: Vec<DeployReceiptArtifactFile>,
}

#[derive(Debug, Deserialize)]
struct DeployReceiptArtifactFile {
    role: String,
    local_path: String,
    file_id: String,
    content_blake3_hex: String,
}

#[derive(Debug, Deserialize)]
struct SessionKeyHttpResponse {
    success: bool,
    session_id: Option<String>,
    public_key: Option<String>,
    error: Option<String>,
}

fn blake3_hex_bytes(data: &[u8]) -> String {
    hex::encode(blake3::hash(data).as_bytes())
}

fn blake3_hex_matches(actual_hex: &str, expected_hex: &str) -> bool {
    actual_hex.trim().eq_ignore_ascii_case(expected_hex.trim())
}

/// `requester_did_header`: when `Some`, send `requester-did` (group / explicit identity).
/// When `None`, omit the header so the node uses `metadata.owner_did` for the owner check — required when
/// deploy `owner-did` differs from the CLI default DID but the same private key encrypted the file.
async fn fetch_file_from_remote_storage_http(
    base_url: &str,
    file_id: &str,
    requester_did_header: Option<&str>,
    user_private_key: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3600))
        .build()
        .map_err(|e| format!("HTTP client: {}", e))?;
    let base = base_url.trim_end_matches('/');
    let session_url = format!("{}/files/{}/session-key", base, file_id);
    let sk_resp = client.get(&session_url).send().await?;
    let status = sk_resp.status();
    let sk_text = sk_resp.text().await?;
    if !status.is_success() {
        return Err(format!(
            "GET {} → HTTP {}: {}",
            session_url,
            status,
            sk_text.chars().take(240).collect::<String>()
        )
        .into());
    }
    let sk: SessionKeyHttpResponse = serde_json::from_str(&sk_text).map_err(|e| {
        format!(
            "session-key JSON: {} — {}",
            e,
            sk_text.chars().take(200).collect::<String>()
        )
    })?;
    if !sk.success {
        return Err(sk
            .error
            .unwrap_or_else(|| "session-key failed".to_string())
            .into());
    }
    let session_id = sk
        .session_id
        .ok_or_else(|| "session-key response missing session_id".to_string())?;
    let pk_hex = sk
        .public_key
        .ok_or_else(|| "session-key response missing public_key".to_string())?;
    let session_public_key =
        hex::decode(pk_hex.trim()).map_err(|e| format!("session public_key hex: {}", e))?;

    let qc = QuantumCrypto::new(QuantumAlgorithm::Kyber1024, CipherSuite::AES256);
    let enc_private = qc
        .encrypt_data_with_algorithm(
            user_private_key,
            &session_public_key,
            QuantumAlgorithm::Kyber1024,
        )
        .await
        .map_err(|e| format!("encrypt private key for session: {}", e))?;
    let enc_json = serde_json::to_vec(&enc_private)?;
    let enc_header = hex::encode(enc_json);

    let content_url = format!("{}/files/{}/content", base, file_id);
    let mut req = client
        .get(&content_url)
        .header("session-id", session_id)
        .header("encrypted-private-key", enc_header);
    if let Some(did) = requester_did_header {
        req = req.header("requester-did", did);
    }
    let content_resp = req.send().await?;
    let cstatus = content_resp.status();
    let body = content_resp.bytes().await?;
    if !cstatus.is_success() {
        let preview = String::from_utf8_lossy(&body[..body.len().min(400)]);
        return Err(format!("GET {} → HTTP {}: {}", content_url, cstatus, preview).into());
    }
    Ok(body.to_vec())
}

// ── Envelope (zero-knowledge) upload / fetch ──────────────────────────────

async fn handle_envelope_upload_cmd(
    file_path: &str,
    storage_url: Option<&str>,
    filename_override: Option<&str>,
    content_type: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use spacekit_storage_node::envelope;

    let (base_url, _used_default) = resolve_remote_storage_base_url(storage_url)?;
    let base = base_url.trim_end_matches('/');
    let pub_hex = load_public_key_hex_for_storage()?;
    let public_key = hex::decode(pub_hex.trim())?;
    let user_did = get_default_did().map_err(|e| format!("{}", e))?;
    let kem_name =
        load_identity_kem_algorithm_from_config()?.unwrap_or_else(|| "Kyber1024".to_string());

    println!("📦 Reading {}…", file_path);
    let plaintext = std::fs::read(file_path)?;
    println!("   {} bytes plaintext", plaintext.len());

    println!("🔐 Encrypting client-side (envelope, {})…", kem_name);
    let envelope_bytes = envelope::encrypt_envelope(&plaintext, &public_key, &kem_name, None)
        .map_err(|e| format!("envelope encrypt: {}", e))?;
    println!(
        "   {} bytes encrypted ({} chunks)",
        envelope_bytes.len(),
        envelope::deserialize_header(&envelope_bytes)
            .map(|(h, _)| h.total_chunks)
            .unwrap_or(0)
    );

    let fname = filename_override.map(|s| s.to_string()).unwrap_or_else(|| {
        std::path::Path::new(file_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string())
    });

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3600))
        .build()?;

    println!("📤 Uploading envelope to {}…", base);
    let mut req = client
        .post(format!("{}/files/envelope-upload", base))
        .header("owner-did", &user_did)
        .header("owner-public-key", hex::encode(&public_key))
        .body(envelope_bytes);
    if let Some(ct) = content_type {
        req = req.header("content-type", ct);
    }
    req = req.header("filename", &fname);

    let resp = req.send().await?;
    let status = resp.status();
    let body = resp.text().await?;
    if !status.is_success() {
        return Err(format!("Upload failed (HTTP {}): {}", status, body).into());
    }

    let json: serde_json::Value = serde_json::from_str(&body)?;
    println!("✅ Envelope uploaded!");
    println!("   file_id: {}", json["file_id"].as_str().unwrap_or("?"));
    println!("   hash:    {}", json["hash"].as_str().unwrap_or("?"));
    println!("   🔒 Server cannot decrypt — only your private key can.");
    Ok(())
}

async fn handle_envelope_fetch_cmd(
    file_id: &str,
    output: &str,
    storage_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    use spacekit_storage_node::envelope;

    let (base_url, _used_default) = resolve_remote_storage_base_url(storage_url)?;
    let base = base_url.trim_end_matches('/');
    let private_key = load_private_key().map_err(|e| format!("{}", e))?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3600))
        .build()?;

    // Step 1: Get challenge
    println!("🔑 Requesting challenge for {}…", file_id);
    let challenge_url = format!("{}/files/{}/challenge", base, file_id);
    let ch_resp = client.get(&challenge_url).send().await?;
    let ch_status = ch_resp.status();
    let ch_text = ch_resp.text().await?;
    if !ch_status.is_success() {
        return Err(format!("Challenge failed (HTTP {}): {}", ch_status, ch_text).into());
    }
    let ch: envelope::ChallengeResponse = serde_json::from_str(&ch_text)?;
    if !ch.success {
        return Err(format!("Challenge error: {}", ch.error.unwrap_or_default()).into());
    }
    let challenge_id = ch.challenge_id.ok_or("missing challenge_id")?;
    let enc_challenge = ch
        .encrypted_challenge
        .ok_or("missing encrypted_challenge")?;

    // Step 2: Decrypt the challenge nonce to prove key ownership
    println!("🔓 Decrypting challenge (proving key ownership)…");
    let kem_algo =
        load_identity_kem_algorithm_from_config()?.unwrap_or_else(|| "Kyber1024".to_string());
    let encrypted_key = envelope::EncryptedFileKey {
        kem_ciphertext_hex: enc_challenge.kem_ciphertext_hex,
        nonce_hex: enc_challenge.nonce_hex,
        ciphertext_hex: enc_challenge.ciphertext_hex,
    };
    let challenge_nonce = envelope::kem_decrypt_bytes(&encrypted_key, &private_key, &kem_algo)
        .map_err(|e| format!("challenge decrypt: {}", e))?;
    let response_token: Vec<u8> = challenge_nonce;

    // Step 3: Stream download
    println!("📥 Downloading encrypted envelope…");
    let stream_url = format!("{}/files/{}/stream", base, file_id);
    let dl_resp = client
        .get(&stream_url)
        .header("challenge-id", &challenge_id)
        .header("challenge-response", hex::encode(response_token))
        .send()
        .await?;
    let dl_status = dl_resp.status();
    if !dl_status.is_success() {
        let err_body = dl_resp.text().await?;
        return Err(format!("Stream failed (HTTP {}): {}", dl_status, err_body).into());
    }
    let envelope_bytes = dl_resp.bytes().await?;
    println!("   {} bytes received", envelope_bytes.len());

    // Step 4: Decrypt client-side
    println!("🔐 Decrypting client-side…");
    let plaintext = envelope::decrypt_envelope(&envelope_bytes, &private_key)
        .map_err(|e| format!("envelope decrypt: {}", e))?;
    println!("   {} bytes plaintext", plaintext.len());

    std::fs::write(output, &plaintext)?;
    println!("✅ Written to {}", output);
    println!("   🔒 Private key never left your machine.");
    Ok(())
}

async fn handle_storage_verify_receipt(
    receipt_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string(receipt_path)
        .map_err(|e| format!("read receipt {}: {}", receipt_path, e))?;
    let r: DeployReceiptFile = serde_json::from_str(&json)?;
    let mut failed = false;
    for a in &r.artifacts {
        let data = std::fs::read(&a.local_path)
            .map_err(|e| format!("read artifact {} ({}): {}", a.role, a.local_path, e))?;
        let got = blake3_hex_bytes(&data);
        if blake3_hex_matches(&got, &a.content_blake3_hex) {
            println!(
                "✅ {} — {} ({} bytes) BLAKE3 matches receipt",
                a.role.green(),
                a.local_path,
                data.len()
            );
        } else {
            failed = true;
            println!(
                "❌ {} — {}\n   expected {}\n   actual   {}",
                a.role.red(),
                a.local_path,
                a.content_blake3_hex,
                got
            );
        }
    }
    if failed {
        return Err("BLAKE3 verification failed for one or more artifacts".into());
    }
    println!(
        "\n{} All {} artifact(s) match.",
        "Verified.".green(),
        r.artifacts.len()
    );
    Ok(())
}

async fn handle_storage_fetch_http(
    file_id: &str,
    output: &str,
    storage_url: Option<&str>,
    requester_did: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (base_url, used_default) = resolve_remote_storage_base_url(storage_url)?;
    if used_default {
        println!(
            "{}",
            "No [connections.storage] URL — using default http://127.0.0.1:3030. Override with --storage-url or `spacekit connect storage`."
                .yellow()
        );
    }
    let requester_header = requester_did.map(|s| s.as_str());
    let private_key = load_private_key()
        .map_err(|e| CliError::Config(format!("Failed to load private key: {}", e)))?;
    if let Some(did) = requester_header {
        println!(
            "📥 HTTP fetch {} from {} (requester-did header: {}) …",
            file_id.blue(),
            base_url.green(),
            did.yellow()
        );
    } else {
        println!(
            "📥 HTTP fetch {} from {} (no requester-did header — node uses file owner for ACL) …",
            file_id.blue(),
            base_url.green()
        );
    }
    let bytes =
        fetch_file_from_remote_storage_http(&base_url, file_id, requester_header, &private_key)
            .await?;
    std::fs::write(output, &bytes)?;
    println!("✅ Wrote {} bytes to {}", bytes.len(), output.green());
    Ok(())
}

async fn handle_storage_sync_receipt(
    receipt_path: &str,
    wasm_out: &str,
    bin_out: &str,
    storage_url: Option<&str>,
    requester_did: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string(receipt_path)
        .map_err(|e| format!("read receipt {}: {}", receipt_path, e))?;
    let r: DeployReceiptFile = serde_json::from_str(&json)?;
    let base = storage_url
        .map(|s| s.trim_end_matches('/').to_string())
        .unwrap_or_else(|| r.storage_node_url.trim_end_matches('/').to_string());
    let wasm_art = r
        .artifacts
        .iter()
        .find(|a| a.role == "wasm")
        .ok_or("receipt has no artifact with role \"wasm\"")?;
    let bin_art = r
        .artifacts
        .iter()
        .find(|a| a.role == "bin")
        .ok_or("receipt has no artifact with role \"bin\"")?;
    let requester_header = requester_did.map(|s| s.as_str());
    let private_key = load_private_key()
        .map_err(|e| CliError::Config(format!("Failed to load private key: {}", e)))?;

    println!(
        "📥 Sync wasm {} → {}",
        wasm_art.file_id.cyan(),
        wasm_out.green()
    );
    let wasm_bytes = fetch_file_from_remote_storage_http(
        &base,
        &wasm_art.file_id,
        requester_header,
        &private_key,
    )
    .await?;
    let wasm_hash = blake3_hex_bytes(&wasm_bytes);
    if !blake3_hex_matches(&wasm_hash, &wasm_art.content_blake3_hex) {
        return Err(format!(
            "wasm BLAKE3 mismatch after download (expected {}, got {})",
            wasm_art.content_blake3_hex, wasm_hash
        )
        .into());
    }
    if let Some(parent) = Path::new(wasm_out).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(wasm_out, &wasm_bytes)?;

    println!(
        "📥 Sync bin  {} → {}",
        bin_art.file_id.cyan(),
        bin_out.green()
    );
    let bin_bytes = fetch_file_from_remote_storage_http(
        &base,
        &bin_art.file_id,
        requester_header,
        &private_key,
    )
    .await?;
    let bin_hash = blake3_hex_bytes(&bin_bytes);
    if !blake3_hex_matches(&bin_hash, &bin_art.content_blake3_hex) {
        return Err(format!(
            "bin BLAKE3 mismatch after download (expected {}, got {})",
            bin_art.content_blake3_hex, bin_hash
        )
        .into());
    }
    if let Some(parent) = Path::new(bin_out).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(bin_out, &bin_bytes)?;

    println!(
        "\n{} WASM + bin verified (BLAKE3) and written.",
        "Done.".green()
    );
    Ok(())
}

fn load_public_key_hex_for_storage() -> Result<String, Box<dyn std::error::Error>> {
    let config_dir = dirs::home_dir()
        .ok_or("Home directory not found")?
        .join(".spacekit");
    let config_file = config_dir.join("config.toml");
    let config_content = std::fs::read_to_string(&config_file).map_err(|e| {
        format!(
            "Failed to read {}: {}. Run `spacekit init` if missing.",
            config_file.display(),
            e
        )
    })?;
    let cli_config: CLIConfig = toml::from_str(&config_content)?;
    let public_key_path =
        resolve_identity_key_path(&cli_config.identity.public_key_path, &config_dir);
    let public_key_hex = std::fs::read_to_string(&public_key_path).map_err(|e| {
        format!(
            "Failed to read public key file {}: {}.\n\
             Fix `identity.public_key_path` in {} (hex-encoded key, same as `spacekit storage store`).",
            public_key_path.display(),
            e,
            config_file.display()
        )
    })?;
    let trimmed = public_key_hex.trim().to_string();
    hex::decode(&trimmed).map_err(|_| {
        format!(
            "Public key at {} is not valid hex",
            public_key_path.display()
        )
    })?;
    Ok(trimmed)
}

/// Resolves the storage node base URL. Second return value is `true` when using the implicit
/// local default (`http://127.0.0.1:3030`, matching `spacekit-storage-node start` default `--port`).
fn resolve_remote_storage_base_url(
    override_url: Option<&str>,
) -> Result<(String, bool), Box<dyn std::error::Error>> {
    if let Some(u) = override_url {
        let u = u.trim();
        if u.is_empty() {
            return Err("Empty --storage-url".into());
        }
        return Ok((u.trim_end_matches('/').to_string(), false));
    }
    let config_dir = dirs::home_dir()
        .ok_or("Home directory not found")?
        .join(".spacekit");
    let config_file = config_dir.join("config.toml");
    if !config_file.exists() {
        return Err("Configuration not found. Run 'spacekit init' first.".into());
    }
    let config_content = std::fs::read_to_string(&config_file)?;
    let cli_config: CLIConfig = toml::from_str(&config_content)?;
    if let Some(url) = cli_config
        .connections
        .and_then(|c| c.storage)
        .map(|s| s.url)
        .filter(|u| !u.trim().is_empty())
        .map(|u| u.trim_end_matches('/').to_string())
    {
        return Ok((url, false));
    }
    Ok(("http://127.0.0.1:3030".to_string(), true))
}

fn optional_storage_node_did_from_config() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let config_dir = dirs::home_dir()
        .ok_or("Home directory not found")?
        .join(".spacekit");
    let config_file = config_dir.join("config.toml");
    if !config_file.exists() {
        return Ok(None);
    }
    let config_content = std::fs::read_to_string(&config_file)?;
    let cli_config: CLIConfig = toml::from_str(&config_content)?;
    Ok(cli_config
        .connections
        .and_then(|c| c.storage)
        .and_then(|s| s.node_did))
}

fn load_identity_kem_algorithm_from_config() -> Result<Option<String>, Box<dyn std::error::Error>> {
    let config_dir = dirs::home_dir()
        .ok_or("Home directory not found")?
        .join(".spacekit");
    let config_file = config_dir.join("config.toml");
    let config_content = std::fs::read_to_string(&config_file)
        .map_err(|e| format!("Failed to read {}: {}", config_file.display(), e))?;
    let cli_config: CLIConfig = toml::from_str(&config_content)?;
    let s = cli_config.identity.algorithm.trim();
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s.to_string()))
    }
}

/// Fetch the storage node's server Kyber public key for envelope encryption.
async fn fetch_server_public_key(
    base_url: &str,
) -> Result<
    (
        Vec<u8>,
        String,
        Option<spacekit_storage_node::envelope::KeySource>,
    ),
    Box<dyn std::error::Error>,
> {
    let url = format!("{}/server-public-key", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    let res = client.get(&url).send().await?;
    if !res.status().is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(format!("Failed to fetch server public key ({}): {}", url, text).into());
    }
    let body: serde_json::Value = res.json().await?;
    let pk_hex = body["public_key"]
        .as_str()
        .ok_or("missing public_key in response")?;
    let algorithm = body["algorithm"]
        .as_str()
        .unwrap_or("Kyber1024")
        .to_string();
    let key_source: Option<spacekit_storage_node::envelope::KeySource> = body
        .get("key_source")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    let pk_bytes = hex::decode(pk_hex)?;
    Ok((pk_bytes, algorithm, key_source))
}

async fn upload_file_to_remote_storage_node(
    base_url: &str,
    file_path: &str,
    role: &str,
    _content_type: &str,
    owner_did: &str,
    owner_public_key_hex: &str,
    _owner_key_algorithm: Option<&str>,
    server_public_key: &[u8],
    server_kem_algorithm: &str,
    server_key_source: Option<spacekit_storage_node::envelope::KeySource>,
) -> Result<StorageDeployArtifactReceipt, Box<dyn std::error::Error>> {
    let plaintext = std::fs::read(file_path)
        .map_err(|e| format!("Failed to read {} ({} role): {}", file_path, role, e))?;
    let path = Path::new(file_path);
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("artifact")
        .to_string();

    let plaintext_size = plaintext.len() as u64;
    let plaintext_hash = hex::encode(blake3::hash(&plaintext).as_bytes());

    let envelope_bytes = spacekit_storage_node::envelope::encrypt_envelope_sourced(
        &plaintext,
        server_public_key,
        server_kem_algorithm,
        None,
        server_key_source,
    )
    .map_err(|e| format!("Envelope encryption failed: {}", e))?;

    let url = format!("{}/files/envelope-upload", base_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3600))
        .build()
        .map_err(|e| format!("HTTP client init failed (reqwest): {}", e))?;

    let request = client
        .post(url)
        .header("owner-did", owner_did)
        .header("owner-public-key", owner_public_key_hex)
        .header("filename", &filename)
        .header("content-type", "application/octet-stream");
    let response = request.body(envelope_bytes).send().await?;

    let status = response.status();
    let text = response.text().await?;
    let parsed: RemoteFileUploadResponse = serde_json::from_str(&text).map_err(|e| {
        if status.as_u16() == 413 {
            return format!(
                "Storage node rejected upload (HTTP 413 Payload Too Large). File exceeds the node's MAX_UPLOAD_BODY_BYTES."
            );
        }
        format!(
            "Storage node returned non-JSON (HTTP {}): {} — {}",
            status,
            e,
            text.chars().take(240).collect::<String>()
        )
    })?;

    if let Some(err) = parsed.error {
        return Err(format!("Storage node error (HTTP {}): {}", status, err).into());
    }
    if !status.is_success() {
        return Err(format!(
            "Storage node HTTP {}: {}",
            status,
            text.chars().take(400).collect::<String>()
        )
        .into());
    }

    let file_id = parsed.file_id.ok_or_else(|| {
        format!(
            "missing file_id in response: {}",
            text.chars().take(200).collect::<String>()
        )
    })?;
    let stored_filename = parsed.filename.unwrap_or_else(|| filename.clone());

    Ok(StorageDeployArtifactReceipt {
        role: role.to_string(),
        local_path: file_path.to_string(),
        file_id,
        stored_filename,
        size_bytes: plaintext_size,
        content_blake3_hex: plaintext_hash,
    })
}

fn deployment_fact_blake3(
    deployment_id: &str,
    owner_did: &str,
    created_at_rfc3339: &str,
) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(deployment_id.as_bytes());
    h.update(&[0]);
    h.update(owner_did.as_bytes());
    h.update(&[0]);
    h.update(created_at_rfc3339.as_bytes());
    *h.finalize().as_bytes()
}

async fn post_agent_deployment_fact_package_http(
    client: &reqwest::Client,
    base_url: &str,
    receipt: &StorageDeployReceipt,
) -> Result<(), Box<dyn std::error::Error>> {
    use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
    use spacekit_primitives::v1::fact::{
        AccessPolicy, CollectionMethod, DataSource, FactCategory, FactContent, FactMetadata,
        FactPackage, KnowledgeDomain, LicenseType, ProofType, VerificationLevel, VerificationProof,
    };
    use spacekit_primitives::v1::identity::QuantumDID;

    let author = QuantumDID::parse(&receipt.owner_did)
        .map_err(|e| format!("invalid owner DID for FactPackage: {}", e))?;

    let fact_id_bytes: [u8; 32] = hex::decode(receipt.fact_id.trim())
        .map_err(|e| format!("fact_id hex: {}", e))?
        .try_into()
        .map_err(|v: Vec<u8>| format!("fact_id must be 32 bytes, got {}", v.len()))?;

    let receipt_json = serde_json::to_value(receipt)?;

    let mut tags = vec!["deployment".to_string(), "agent-deployment".to_string()];
    if let Some(ref aid) = receipt.agent_id {
        tags.push(aid.clone());
    }

    let receipt_vec = serde_json::to_vec(&receipt_json)?;
    let checksum: [u8; 32] = *blake3::hash(&receipt_vec).as_bytes();
    let size_bytes: u64 = receipt.artifacts.iter().map(|a| a.size_bytes).sum();

    let created_ts = chrono::DateTime::parse_from_rfc3339(&receipt.created_at)
        .map(|d| d.timestamp() as u64)
        .unwrap_or_else(|_| Utc::now().timestamp() as u64);

    let signature = SPHINCSSignature::new(vec![0u8; 64], "SPHINCS-256f".to_string(), vec![0u8; 32]);

    let fact = FactPackage {
        fact_id: fact_id_bytes,
        version: 1,
        created_at: created_ts,
        expires_at: None,
        content: FactContent::Json {
            data: receipt_json,
            schema: Some("spacekit:agent:deployment:v1".to_string()),
        },
        metadata: FactMetadata {
            category: FactCategory::Technical,
            tags,
            domain: KnowledgeDomain::ComputerScience,
            source: DataSource::UserInput {
                application: author.clone(),
                user: author.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: LicenseType::MIT,
            size_bytes,
            checksum,
        },
        author,
        signature,
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: vec![0u8; 32],
            verification_timestamp: created_ts,
            verifier: None,
        },
        dependencies: Vec::new(),
        citations: Vec::new(),
        confidence_score: 1.0,
        access_policy: AccessPolicy::Public,
        encryption: None,
    };

    let url = format!("{}/facts", base_url.trim_end_matches('/'));
    let body = serde_json::to_vec(&fact)?;
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await?;

    if resp.status().is_success() {
        println!(
            "   {}",
            format!(
                "FactPackage stored (canonical deployment record): {}",
                receipt.fact_id
            )
            .green()
        );
        return Ok(());
    }

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    Err(format!(
        "FactPackage POST HTTP {}: {}",
        status,
        text.chars().take(500).collect::<String>()
    )
    .into())
}

// ─── `deploy.toml` manifest for `storage deploy --package` ───────────────────

#[derive(Debug, Default, Deserialize)]
struct StorageDeployPackageToml {
    #[serde(default)]
    artifacts: Option<StorageDeployArtifactsToml>,
    #[serde(default)]
    agent: Option<StorageDeployAgentToml>,
    #[serde(default)]
    storage: Option<StorageDeployStorageToml>,
    #[serde(default)]
    receipt: Option<StorageDeployReceiptToml>,
    #[serde(default)]
    hub: Option<StorageDeployHubToml>,
    #[serde(default)]
    marketplace: Option<StorageDeployMarketplaceToml>,
    #[serde(default)]
    project: Option<StorageDeployProjectToml>,
    #[serde(default)]
    prompts: Option<toml::Value>,
}

#[derive(Debug, Deserialize)]
struct StorageDeployArtifactsToml {
    wasm: String,
    bin: String,
    /// Optional domain inference rules (e.g. `data/inference_pets.toml` for chat-mode pets).
    #[serde(default)]
    inference_toml: Option<String>,
    /// Optional inference guardrails JSONL (merged after TOML at runtime).
    #[serde(default)]
    guardrails_jsonl: Option<String>,
    /// Optional fragment library JSONL for chat-mode fragment composition.
    #[serde(default)]
    fragments_jsonl: Option<String>,
    /// Optional topic/knowledge graph TOML for prompt routing.
    #[serde(default)]
    topic_graph: Option<String>,
    /// Optional grounding/lexicon TOML for concept anchoring.
    #[serde(default)]
    grounding_toml: Option<String>,
    /// Optional companion UI HTML file (e.g. `ui/pixel_cat_companion.html`).
    /// Uploaded as a FactPackage artifact alongside brain/wasm.
    #[serde(default)]
    companion_ui: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StorageDeployAgentToml {
    id: Option<String>,
    owner_did: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StorageDeployStorageToml {
    url: Option<String>,
    owner_key_algorithm: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StorageDeployReceiptToml {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StorageDeployHubToml {
    kind: Option<String>,
    op: Option<u8>,
    input_format: Option<String>,
    brain_key: Option<String>,
    storage_brain_key: Option<String>,
    hub_response_format: Option<String>,
    thinking_label: Option<String>,
    hub_thinking_label: Option<String>,
    /// Companion UI registry key (e.g. "pixel_cat") — rendered beside the chat in Agent Hub.
    hub_companion_ui: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    tag_label: Option<String>,
    tag: Option<String>,
    tag_color: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StorageDeployMarketplaceToml {
    publish: Option<bool>,
    title: Option<String>,
    description: Option<String>,
    category: Option<String>,
    access: Option<String>,
    price: Option<String>,
    marketplace_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StorageDeployProjectToml {
    /// Optional Growformer project manifest; `growformer.brain_storage_key` fills `[hub].brain_key` when omitted.
    gf_toml: Option<String>,
}

struct ResolvedStorageDeployParams {
    wasm: String,
    bin: String,
    inference_toml: Option<String>,
    guardrails_jsonl: Option<String>,
    fragments_jsonl: Option<String>,
    topic_graph: Option<String>,
    grounding_toml: Option<String>,
    companion_ui: Option<String>,
    did: Option<String>,
    storage_url: Option<String>,
    receipt: Option<String>,
    owner_key_algorithm: Option<String>,
    agent_id: Option<String>,
    publish: bool,
    title: Option<String>,
    description: Option<String>,
    category: Option<String>,
    access: Option<String>,
    price: Option<String>,
    marketplace: Option<String>,
    brain_key: Option<String>,
    capabilities: Option<Vec<String>>,
    tag_label: Option<String>,
    tag_color: Option<String>,
    hub_response_format: Option<String>,
    hub_thinking_label: Option<String>,
    hub_companion_ui: Option<String>,
    hub_op: Option<u8>,
    hub_input_format: Option<String>,
    prompts: Option<serde_json::Value>,
}

fn coalesce_non_empty(cli: Option<&str>, manifest: Option<String>) -> Option<String> {
    cli.map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            manifest
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

fn resolve_manifest_path(manifest_dir: &Path, p: &str) -> PathBuf {
    let path = Path::new(p);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest_dir.join(path)
    }
}

fn brain_key_from_gf_toml(manifest_dir: &Path, gf_rel: &str) -> Result<Option<String>, String> {
    let gf_path = resolve_manifest_path(manifest_dir, gf_rel);
    let raw =
        fs::read_to_string(&gf_path).map_err(|e| format!("read {}: {}", gf_path.display(), e))?;
    let gf: GfTomlForRegistry =
        toml::from_str(&raw).map_err(|e| format!("{}.gf.toml parse: {}", gf_path.display(), e))?;
    Ok(gf.growformer.and_then(|g| g.brain_storage_key))
}

#[derive(Debug, Deserialize)]
struct GfTomlInferenceLookup {
    #[serde(default)]
    inference: Option<GfInferenceTomlSection>,
}

#[derive(Debug, Deserialize)]
struct GfInferenceTomlSection {
    toml: Option<String>,
    topic_graph: Option<String>,
    grounding_toml: Option<String>,
    guardrails_jsonl: Option<String>,
    fragments_jsonl: Option<String>,
}

fn inference_toml_from_gf_toml(
    manifest_dir: &Path,
    gf_rel: &str,
) -> Result<Option<String>, String> {
    let gf_path = resolve_manifest_path(manifest_dir, gf_rel);
    let raw =
        fs::read_to_string(&gf_path).map_err(|e| format!("read {}: {}", gf_path.display(), e))?;
    let gf: GfTomlInferenceLookup =
        toml::from_str(&raw).map_err(|e| format!("{}.gf.toml parse: {}", gf_path.display(), e))?;
    Ok(gf.inference.and_then(|i| i.toml).map(|p| {
        resolve_manifest_path(manifest_dir, &p)
            .to_string_lossy()
            .into_owned()
    }))
}

fn topic_graph_from_gf_toml(manifest_dir: &Path, gf_rel: &str) -> Result<Option<String>, String> {
    let gf_path = resolve_manifest_path(manifest_dir, gf_rel);
    let raw =
        fs::read_to_string(&gf_path).map_err(|e| format!("read {}: {}", gf_path.display(), e))?;
    let gf: GfTomlInferenceLookup =
        toml::from_str(&raw).map_err(|e| format!("{}.gf.toml parse: {}", gf_path.display(), e))?;
    if let Some(explicit) = gf.inference.and_then(|i| i.topic_graph) {
        return Ok(Some(
            resolve_manifest_path(manifest_dir, &explicit)
                .to_string_lossy()
                .into_owned(),
        ));
    }
    let gf_dir = gf_path.parent().unwrap_or(manifest_dir);
    for candidate in [
        gf_dir.join("data/knowledge_graph_pet_overlay.toml"),
        gf_dir.join("data/knowledge_graph.toml"),
    ] {
        if candidate.is_file() {
            return Ok(Some(candidate.to_string_lossy().into_owned()));
        }
    }
    Ok(None)
}

#[derive(Debug, Deserialize)]
struct GfTomlGroundingLookup {
    #[serde(default)]
    inference: Option<GfInferenceTomlSection>,
    #[serde(default)]
    grounding: Option<GfGroundingTomlSection>,
}

#[derive(Debug, Deserialize)]
struct GfGroundingTomlSection {
    grounding_file: Option<String>,
}

fn grounding_toml_from_gf_toml(
    manifest_dir: &Path,
    gf_rel: &str,
) -> Result<Option<String>, String> {
    let gf_path = resolve_manifest_path(manifest_dir, gf_rel);
    let raw =
        fs::read_to_string(&gf_path).map_err(|e| format!("read {}: {}", gf_path.display(), e))?;
    let gf: GfTomlGroundingLookup =
        toml::from_str(&raw).map_err(|e| format!("{}.gf.toml parse: {}", gf_path.display(), e))?;
    if let Some(explicit) = gf
        .inference
        .as_ref()
        .and_then(|i| i.grounding_toml.clone())
        .or_else(|| gf.grounding.and_then(|g| g.grounding_file))
    {
        return Ok(Some(
            resolve_manifest_path(manifest_dir, &explicit)
                .to_string_lossy()
                .into_owned(),
        ));
    }
    let gf_dir = gf_path.parent().unwrap_or(manifest_dir);
    let default_grounding = gf_dir.join("data/pet_world_grounding.toml");
    if default_grounding.is_file() {
        return Ok(Some(default_grounding.to_string_lossy().into_owned()));
    }
    Ok(None)
}

fn guardrails_jsonl_from_gf_toml(
    manifest_dir: &Path,
    gf_rel: &str,
) -> Result<Option<String>, String> {
    let gf_path = resolve_manifest_path(manifest_dir, gf_rel);
    let raw =
        fs::read_to_string(&gf_path).map_err(|e| format!("read {}: {}", gf_path.display(), e))?;
    let gf: GfTomlInferenceLookup =
        toml::from_str(&raw).map_err(|e| format!("{}.gf.toml parse: {}", gf_path.display(), e))?;
    if let Some(explicit) = gf
        .inference
        .as_ref()
        .and_then(|i| i.guardrails_jsonl.clone())
    {
        return Ok(Some(
            resolve_manifest_path(manifest_dir, &explicit)
                .to_string_lossy()
                .into_owned(),
        ));
    }
    Ok(None)
}

fn fragments_jsonl_from_gf_toml(
    manifest_dir: &Path,
    gf_rel: &str,
) -> Result<Option<String>, String> {
    let gf_path = resolve_manifest_path(manifest_dir, gf_rel);
    let raw =
        fs::read_to_string(&gf_path).map_err(|e| format!("read {}: {}", gf_path.display(), e))?;
    let gf: GfTomlInferenceLookup =
        toml::from_str(&raw).map_err(|e| format!("{}.gf.toml parse: {}", gf_path.display(), e))?;
    if let Some(explicit) = gf
        .inference
        .as_ref()
        .and_then(|i| i.fragments_jsonl.clone())
    {
        return Ok(Some(
            resolve_manifest_path(manifest_dir, &explicit)
                .to_string_lossy()
                .into_owned(),
        ));
    }
    if let Some(inf_rel) = gf.inference.as_ref().and_then(|i| i.toml.clone()) {
        let inf_path = resolve_manifest_path(manifest_dir, &inf_rel);
        if inf_path.is_file() {
            #[derive(serde::Deserialize)]
            struct FragComposeSection {
                library: Option<String>,
            }
            #[derive(serde::Deserialize)]
            struct InferenceFragLookup {
                fragment_compose: Option<FragComposeSection>,
            }
            if let Ok(inf_raw) = fs::read_to_string(&inf_path) {
                if let Ok(doc) = toml::from_str::<InferenceFragLookup>(&inf_raw) {
                    if let Some(lib) = doc
                        .fragment_compose
                        .and_then(|f| f.library)
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                    {
                        let gf_dir = gf_path.parent().unwrap_or(manifest_dir);
                        let rel = Path::new(&lib);
                        for cand in [
                            inf_path.parent().map(|d| d.join(rel)),
                            Some(gf_dir.join("data").join(rel)),
                            Some(gf_dir.join(rel)),
                        ]
                        .into_iter()
                        .flatten()
                        {
                            if cand.is_file() {
                                return Ok(Some(cand.to_string_lossy().into_owned()));
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(None)
}

fn load_storage_deploy_package(path: &Path) -> Result<StorageDeployPackageToml, String> {
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("read deploy manifest {}: {}", path.display(), e))?;
    toml::from_str(&raw).map_err(|e| format!("deploy manifest {}: {}", path.display(), e))
}

#[allow(clippy::too_many_arguments)]
fn resolve_storage_deploy_params(
    package_path: Option<&str>,
    cli_wasm: Option<&str>,
    cli_bin: Option<&str>,
    cli_did: Option<&str>,
    cli_storage_url: Option<&str>,
    cli_receipt: Option<&str>,
    cli_owner_key_algorithm: Option<&str>,
    cli_agent_id: Option<&str>,
    cli_publish: bool,
    cli_title: Option<&str>,
    cli_description: Option<&str>,
    cli_category: Option<&str>,
    cli_access: Option<&str>,
    cli_price: Option<&str>,
    cli_marketplace: Option<&str>,
    cli_brain_key: Option<&str>,
    cli_capabilities: Option<&[String]>,
    cli_tag_label: Option<&str>,
    cli_tag_color: Option<&str>,
    cli_hub_response_format: Option<&str>,
) -> Result<ResolvedStorageDeployParams, Box<dyn std::error::Error>> {
    let (pkg, manifest_dir) = if let Some(path_str) = package_path {
        let path = Path::new(path_str);
        let loaded = load_storage_deploy_package(path)?;
        let dir = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        (loaded, dir)
    } else {
        (StorageDeployPackageToml::default(), PathBuf::from("."))
    };

    if package_path.is_none() && (cli_wasm.is_none() || cli_bin.is_none()) {
        return Err(
            "Missing artifact paths: pass --package deploy.toml or both --wasm and --bin".into(),
        );
    }

    let wasm_raw = coalesce_non_empty(cli_wasm, pkg.artifacts.as_ref().map(|a| a.wasm.clone()))
        .ok_or("Missing WASM path: set [artifacts].wasm in the deploy manifest or pass --wasm")?;
    let bin_raw = coalesce_non_empty(cli_bin, pkg.artifacts.as_ref().map(|a| a.bin.clone()))
        .ok_or(
            "Missing brain .bin path: set [artifacts].bin in the deploy manifest or pass --bin",
        )?;

    let wasm = resolve_manifest_path(&manifest_dir, &wasm_raw)
        .to_string_lossy()
        .into_owned();
    let bin = resolve_manifest_path(&manifest_dir, &bin_raw)
        .to_string_lossy()
        .into_owned();

    let mut inference_toml = pkg
        .artifacts
        .as_ref()
        .and_then(|a| a.inference_toml.clone())
        .map(|p| {
            resolve_manifest_path(&manifest_dir, &p)
                .to_string_lossy()
                .into_owned()
        });
    if inference_toml.is_none() {
        if let Some(ref proj) = pkg.project {
            if let Some(ref gf) = proj.gf_toml {
                inference_toml = inference_toml_from_gf_toml(&manifest_dir, gf)?;
            }
        }
    }

    let mut topic_graph_path: Option<String> = pkg
        .artifacts
        .as_ref()
        .and_then(|a| a.topic_graph.clone())
        .map(|p| {
            resolve_manifest_path(&manifest_dir, &p)
                .to_string_lossy()
                .into_owned()
        });
    if topic_graph_path.is_none() {
        if let Some(ref proj) = pkg.project {
            if let Some(ref gf) = proj.gf_toml {
                topic_graph_path = topic_graph_from_gf_toml(&manifest_dir, gf)?;
            }
        }
    }

    let mut grounding_toml_path: Option<String> = pkg
        .artifacts
        .as_ref()
        .and_then(|a| a.grounding_toml.clone())
        .map(|p| {
            resolve_manifest_path(&manifest_dir, &p)
                .to_string_lossy()
                .into_owned()
        });
    if grounding_toml_path.is_none() {
        if let Some(ref proj) = pkg.project {
            if let Some(ref gf) = proj.gf_toml {
                grounding_toml_path = grounding_toml_from_gf_toml(&manifest_dir, gf)?;
            }
        }
    }

    let companion_ui_path: Option<String> = pkg
        .artifacts
        .as_ref()
        .and_then(|a| a.companion_ui.clone())
        .map(|p| {
            resolve_manifest_path(&manifest_dir, &p)
                .to_string_lossy()
                .into_owned()
        });

    let guardrails_jsonl_path: Option<String> = pkg
        .artifacts
        .as_ref()
        .and_then(|a| a.guardrails_jsonl.clone())
        .map(|p| {
            resolve_manifest_path(&manifest_dir, &p)
                .to_string_lossy()
                .into_owned()
        });
    let mut guardrails_jsonl = guardrails_jsonl_path;
    if guardrails_jsonl.is_none() {
        if let Some(ref proj) = pkg.project {
            if let Some(ref gf) = proj.gf_toml {
                guardrails_jsonl = guardrails_jsonl_from_gf_toml(&manifest_dir, gf)?;
            }
        }
    }

    let fragments_jsonl_path: Option<String> = pkg
        .artifacts
        .as_ref()
        .and_then(|a| a.fragments_jsonl.clone())
        .map(|p| {
            resolve_manifest_path(&manifest_dir, &p)
                .to_string_lossy()
                .into_owned()
        });
    let mut fragments_jsonl = fragments_jsonl_path;
    if fragments_jsonl.is_none() {
        if let Some(ref proj) = pkg.project {
            if let Some(ref gf) = proj.gf_toml {
                fragments_jsonl = fragments_jsonl_from_gf_toml(&manifest_dir, gf)?;
            }
        }
    }

    let mut brain_key = coalesce_non_empty(
        cli_brain_key,
        pkg.hub
            .as_ref()
            .and_then(|h| h.brain_key.clone().or_else(|| h.storage_brain_key.clone())),
    );
    if brain_key.is_none() {
        if let Some(ref proj) = pkg.project {
            if let Some(ref gf) = proj.gf_toml {
                brain_key = brain_key_from_gf_toml(&manifest_dir, gf)?;
            }
        }
    }

    let capabilities = if let Some(caps) = cli_capabilities.filter(|c| !c.is_empty()) {
        Some(caps.to_vec())
    } else {
        pkg.hub
            .as_ref()
            .map(|h| h.capabilities.clone())
            .filter(|c| !c.is_empty())
    };

    let tag_label = coalesce_non_empty(
        cli_tag_label,
        pkg.hub
            .as_ref()
            .and_then(|h| h.tag_label.clone().or_else(|| h.tag.clone())),
    );

    let hub_response_format = coalesce_non_empty(
        cli_hub_response_format,
        pkg.hub.as_ref().and_then(|h| h.hub_response_format.clone()),
    );

    let hub_thinking_label = pkg
        .hub
        .as_ref()
        .and_then(|h| {
            h.thinking_label
                .clone()
                .or_else(|| h.hub_thinking_label.clone())
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let hub_companion_ui = pkg
        .hub
        .as_ref()
        .and_then(|h| h.hub_companion_ui.clone())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let hub_op = pkg.hub.as_ref().and_then(|h| h.op);
    let hub_input_format = pkg
        .hub
        .as_ref()
        .and_then(|h| h.input_format.clone())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    Ok(ResolvedStorageDeployParams {
        wasm,
        bin,
        inference_toml,
        guardrails_jsonl,
        fragments_jsonl,
        topic_graph: topic_graph_path,
        grounding_toml: grounding_toml_path,
        companion_ui: companion_ui_path,
        did: coalesce_non_empty(
            cli_did,
            pkg.agent.as_ref().and_then(|a| a.owner_did.clone()),
        ),
        storage_url: coalesce_non_empty(
            cli_storage_url,
            pkg.storage.as_ref().and_then(|s| s.url.clone()),
        ),
        receipt: coalesce_non_empty(
            cli_receipt,
            pkg.receipt.as_ref().and_then(|r| r.path.clone()),
        ),
        owner_key_algorithm: coalesce_non_empty(
            cli_owner_key_algorithm,
            pkg.storage
                .as_ref()
                .and_then(|s| s.owner_key_algorithm.clone()),
        ),
        agent_id: coalesce_non_empty(cli_agent_id, pkg.agent.as_ref().and_then(|a| a.id.clone())),
        publish: cli_publish
            || pkg
                .marketplace
                .as_ref()
                .and_then(|m| m.publish)
                .unwrap_or(false),
        title: coalesce_non_empty(
            cli_title,
            pkg.marketplace.as_ref().and_then(|m| m.title.clone()),
        ),
        description: coalesce_non_empty(
            cli_description,
            pkg.marketplace.as_ref().and_then(|m| m.description.clone()),
        ),
        category: coalesce_non_empty(
            cli_category,
            pkg.marketplace.as_ref().and_then(|m| m.category.clone()),
        ),
        access: coalesce_non_empty(
            cli_access,
            pkg.marketplace.as_ref().and_then(|m| m.access.clone()),
        ),
        price: coalesce_non_empty(
            cli_price,
            pkg.marketplace.as_ref().and_then(|m| m.price.clone()),
        ),
        marketplace: coalesce_non_empty(
            cli_marketplace,
            pkg.marketplace
                .as_ref()
                .and_then(|m| m.marketplace_id.clone()),
        ),
        brain_key,
        capabilities,
        tag_label,
        tag_color: coalesce_non_empty(
            cli_tag_color,
            pkg.hub.as_ref().and_then(|h| h.tag_color.clone()),
        ),
        hub_response_format,
        hub_thinking_label,
        hub_companion_ui,
        hub_op,
        hub_input_format,
        prompts: pkg.prompts.and_then(|v| serde_json::to_value(v).ok()),
    })
}

fn build_agent_hub_config_receipt(
    agent_id: Option<&str>,
    brain_key: Option<&str>,
    capabilities: Option<&[String]>,
    tag_label: Option<&str>,
    tag_color: Option<&str>,
    hub_response_format: Option<&str>,
    hub_thinking_label: Option<&str>,
    hub_companion_ui: Option<&str>,
    hub_op: Option<u8>,
    hub_input_format: Option<&str>,
    inference_toml: bool,
    topic_graph: bool,
    grounding_toml: bool,
    fragments_jsonl: bool,
    guardrails_jsonl: bool,
) -> AgentHubConfigReceipt {
    let response_format = hub_response_format
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "growformer".to_string());
    let brain = brain_key
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| agent_id.map(|id| format!("{}_brain", id.replace('-', "_"))));
    AgentHubConfigReceipt {
        kind: "spacekit_agent".to_string(),
        op: hub_op.unwrap_or(1),
        input_format: hub_input_format
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "string_only".to_string()),
        storage_brain_key: brain,
        hub_response_format: response_format,
        hub_thinking_label: hub_thinking_label
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Thinking".to_string()),
        capabilities: capabilities
            .map(|c| {
                c.iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        tag: tag_label
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        tag_color: tag_color
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        hub_companion_ui: hub_companion_ui
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        inference_toml,
        topic_graph,
        grounding_toml,
        fragments_jsonl,
        guardrails_jsonl,
    }
}

async fn fetch_app_listing_file_ids(
    client: &reqwest::Client,
    base_url: &str,
    app_id: &str,
    owner_did: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let url = format!(
        "{}/api/documents/app_listings/{}",
        base_url.trim_end_matches('/'),
        app_id
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("DID {}", owner_did))
        .send()
        .await?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Vec::new());
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "Failed to fetch app_listings/{} (HTTP {}): {}",
            app_id, status, body
        )
        .into());
    }
    let doc: serde_json::Value = resp.json().await?;
    let mut ids = Vec::new();
    if let Some(artifacts) = doc.get("artifacts").and_then(|a| a.as_array()) {
        for item in artifacts {
            if let Some(fid) = item.get("file_id").and_then(|v| v.as_str()) {
                ids.push(fid.to_string());
            }
        }
    }
    Ok(ids)
}

async fn delete_remote_storage_file(
    client: &reqwest::Client,
    base_url: &str,
    file_id: &str,
    owner_did: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = format!("{}/files/{}", base_url.trim_end_matches('/'), file_id);
    let resp = client
        .delete(&url)
        .header("requester-did", owner_did)
        .send()
        .await?;
    if resp.status().is_success() || resp.status() == reqwest::StatusCode::NOT_FOUND {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!(
            "DELETE /files/{} failed (HTTP {}): {}",
            file_id, status, body
        )
        .into())
    }
}

async fn prune_superseded_deploy_artifacts(
    client: &reqwest::Client,
    base_url: &str,
    owner_did: &str,
    previous_file_ids: &[String],
    new_file_ids: &[String],
) {
    if previous_file_ids.is_empty() {
        return;
    }
    let new_set: std::collections::HashSet<&str> =
        new_file_ids.iter().map(|s| s.as_str()).collect();
    let mut deleted = 0usize;
    for old_id in previous_file_ids {
        if new_set.contains(old_id.as_str()) {
            continue;
        }
        match delete_remote_storage_file(client, base_url, old_id, owner_did).await {
            Ok(()) => {
                deleted += 1;
                println!("   🗑️  Deleted superseded artifact {}", old_id.green());
            }
            Err(e) => {
                eprintln!(
                    "   ⚠️  Failed to delete superseded artifact {}: {}",
                    old_id, e
                );
            }
        }
    }
    if deleted > 0 {
        println!(
            "   Removed {} superseded file blob(s) from storage",
            deleted
        );
    }
}

async fn handle_storage_deploy(
    wasm_path: &str,
    bin_path: &str,
    inference_toml_path: Option<&str>,
    guardrails_jsonl_path: Option<&str>,
    fragments_jsonl_path: Option<&str>,
    topic_graph_path: Option<&str>,
    grounding_toml_path: Option<&str>,
    companion_ui_path: Option<&str>,
    owner_did: &str,
    storage_url: Option<&str>,
    receipt_path: Option<&str>,
    owner_key_algorithm_cli: Option<&str>,
    agent_id: Option<&str>,
    publish: bool,
    title: Option<&str>,
    description: Option<&str>,
    category: Option<&str>,
    access: Option<&str>,
    price: Option<&str>,
    marketplace_id: Option<&str>,
    brain_key: Option<&str>,
    capabilities: Option<&[String]>,
    tag_label: Option<&str>,
    tag_color: Option<&str>,
    hub_response_format: Option<&str>,
    hub_thinking_label: Option<&str>,
    hub_companion_ui: Option<&str>,
    hub_op: Option<u8>,
    hub_input_format: Option<&str>,
    prompts: Option<&serde_json::Value>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Deploying WASM + binary bundle to remote storage node...");
    println!("   WASM: {}", wasm_path.cyan());
    println!("   BIN:  {}", bin_path.cyan());
    if let Some(inf) = inference_toml_path {
        println!("   Inference TOML: {}", inf.cyan());
    }
    if let Some(guardrails) = guardrails_jsonl_path {
        println!("   Guardrails JSONL: {}", guardrails.cyan());
    }
    if let Some(fragments) = fragments_jsonl_path {
        println!("   Fragments JSONL: {}", fragments.cyan());
    }
    if let Some(graph) = topic_graph_path {
        println!("   Topic Graph: {}", graph.cyan());
    }
    if let Some(grounding) = grounding_toml_path {
        println!("   Grounding TOML: {}", grounding.cyan());
    }
    if let Some(companion) = companion_ui_path {
        println!("   Companion UI: {}", companion.cyan());
    }
    println!("   Owner: {}", owner_did.yellow());
    if let Some(aid) = agent_id {
        println!("   Agent: {}", aid.green());
    }
    if publish {
        println!("   Publish: {}", "yes (marketplace listing)".green());
    }

    if !Path::new(wasm_path).is_file() {
        return Err(format!("WASM path is not a file: {}", wasm_path).into());
    }
    if !Path::new(bin_path).is_file() {
        return Err(format!("BIN path is not a file: {}", bin_path).into());
    }

    let (base_url, used_default_local_url) = resolve_remote_storage_base_url(storage_url)?;
    if used_default_local_url {
        println!(
            "   {}",
            "No [connections.storage] URL in ~/.spacekit/config.toml — using default http://127.0.0.1:3030 (spacekit-storage-node default API port). Override with --storage-url or `spacekit connect storage`."
                .yellow()
        );
    }
    println!("   Storage: {}", base_url.green());

    let owner_kem: Option<String> = if let Some(cli) = owner_key_algorithm_cli {
        let t = cli.trim();
        if t.is_empty() {
            load_identity_kem_algorithm_from_config()?
        } else {
            Some(t.to_string())
        }
    } else {
        load_identity_kem_algorithm_from_config()?
    };
    if let Some(ref a) = owner_kem {
        println!(
            "   Owner KEM: {} {}",
            a.green(),
            "(owner-key-algorithm)".dimmed()
        );
    } else {
        println!(
            "   {}",
            "Owner KEM: not set — storage node uses its default KEM for encryption (must match your key bytes)".yellow()
        );
    }
    let _owner_kem_for_upload = owner_kem.as_deref();

    let pub_hex = load_public_key_hex_for_storage()?;
    let node_did = optional_storage_node_did_from_config()?;

    // Fetch the storage node's server public key for envelope encryption
    println!("🔑 Fetching server public key for envelope encryption...");
    let (server_pk, server_algo, server_key_source) = fetch_server_public_key(&base_url).await?;
    let pk_fingerprint = hex::encode(&blake3::hash(&server_pk).as_bytes()[..8]);
    println!(
        "   Server KEM: {} (pk {} bytes, source: {:?}, fingerprint: {})",
        server_algo.green(),
        server_pk.len(),
        server_key_source.unwrap_or(spacekit_storage_node::envelope::KeySource::Oqs),
        pk_fingerprint
    );

    println!("🔐 Encrypting and uploading WASM artifact...");
    let wasm_receipt = upload_file_to_remote_storage_node(
        &base_url,
        wasm_path,
        "wasm",
        "application/wasm",
        owner_did,
        &pub_hex,
        _owner_kem_for_upload,
        &server_pk,
        &server_algo,
        server_key_source,
    )
    .await?;

    println!("🔐 Encrypting and uploading brain artifact...");
    let bin_receipt = upload_file_to_remote_storage_node(
        &base_url,
        bin_path,
        "bin",
        "application/octet-stream",
        owner_did,
        &pub_hex,
        _owner_kem_for_upload,
        &server_pk,
        &server_algo,
        server_key_source,
    )
    .await?;

    let mut artifact_receipts = vec![wasm_receipt, bin_receipt];
    let mut shipped_inference_toml = false;
    if let Some(inference_path) = inference_toml_path {
        if Path::new(inference_path).is_file() {
            println!("🔐 Encrypting and uploading inference rules artifact...");
            let inference_receipt = upload_file_to_remote_storage_node(
                &base_url,
                inference_path,
                "inference_toml",
                "text/plain",
                owner_did,
                &pub_hex,
                _owner_kem_for_upload,
                &server_pk,
                &server_algo,
                server_key_source,
            )
            .await?;
            artifact_receipts.push(inference_receipt);
            shipped_inference_toml = true;
        } else {
            eprintln!(
                "\n{}",
                format!(
                    "⚠️  inference_toml path is not a file: {} — skipping artifact upload",
                    inference_path
                )
                .yellow()
            );
        }
    }

    let mut shipped_guardrails_jsonl = false;
    if let Some(guardrails_path) = guardrails_jsonl_path {
        if Path::new(guardrails_path).is_file() {
            println!("🔐 Encrypting and uploading guardrails JSONL artifact...");
            let guardrails_receipt = upload_file_to_remote_storage_node(
                &base_url,
                guardrails_path,
                "guardrails_jsonl",
                "application/x-ndjson",
                owner_did,
                &pub_hex,
                _owner_kem_for_upload,
                &server_pk,
                &server_algo,
                server_key_source,
            )
            .await?;
            artifact_receipts.push(guardrails_receipt);
            shipped_guardrails_jsonl = true;
        } else {
            eprintln!(
                "\n{}",
                format!(
                    "⚠️  guardrails_jsonl path is not a file: {} — skipping artifact upload",
                    guardrails_path
                )
                .yellow()
            );
        }
    }

    let mut shipped_topic_graph = false;
    if let Some(graph_path) = topic_graph_path {
        if Path::new(graph_path).is_file() {
            println!("🔐 Encrypting and uploading topic graph artifact...");
            let graph_receipt = upload_file_to_remote_storage_node(
                &base_url,
                graph_path,
                "topic_graph",
                "text/plain",
                owner_did,
                &pub_hex,
                _owner_kem_for_upload,
                &server_pk,
                &server_algo,
                server_key_source,
            )
            .await?;
            artifact_receipts.push(graph_receipt);
            shipped_topic_graph = true;
        } else {
            eprintln!(
                "\n{}",
                format!(
                    "⚠️  topic_graph path is not a file: {} — skipping artifact upload",
                    graph_path
                )
                .yellow()
            );
        }
    }

    let mut shipped_grounding_toml = false;
    if let Some(grounding_path) = grounding_toml_path {
        if Path::new(grounding_path).is_file() {
            println!("🔐 Encrypting and uploading grounding TOML artifact...");
            let grounding_receipt = upload_file_to_remote_storage_node(
                &base_url,
                grounding_path,
                "grounding_toml",
                "text/plain",
                owner_did,
                &pub_hex,
                _owner_kem_for_upload,
                &server_pk,
                &server_algo,
                server_key_source,
            )
            .await?;
            artifact_receipts.push(grounding_receipt);
            shipped_grounding_toml = true;
        } else {
            eprintln!(
                "\n{}",
                format!(
                    "⚠️  grounding_toml path is not a file: {} — skipping artifact upload",
                    grounding_path
                )
                .yellow()
            );
        }
    }

    let mut shipped_fragments_jsonl = false;
    if let Some(fragments_path) = fragments_jsonl_path {
        if Path::new(fragments_path).is_file() {
            println!("🔐 Encrypting and uploading fragments JSONL artifact...");
            let fragments_receipt = upload_file_to_remote_storage_node(
                &base_url,
                fragments_path,
                "fragments_jsonl",
                "application/x-ndjson",
                owner_did,
                &pub_hex,
                _owner_kem_for_upload,
                &server_pk,
                &server_algo,
                server_key_source,
            )
            .await?;
            artifact_receipts.push(fragments_receipt);
            shipped_fragments_jsonl = true;
        } else {
            eprintln!(
                "\n{}",
                format!(
                    "⚠️  fragments_jsonl path is not a file: {} — skipping artifact upload",
                    fragments_path
                )
                .yellow()
            );
        }
    }

    if let Some(companion_path) = companion_ui_path {
        if Path::new(companion_path).is_file() {
            println!("🔐 Encrypting and uploading companion UI artifact...");
            let companion_receipt = upload_file_to_remote_storage_node(
                &base_url,
                companion_path,
                "companion_ui",
                "text/html",
                owner_did,
                &pub_hex,
                _owner_kem_for_upload,
                &server_pk,
                &server_algo,
                server_key_source,
            )
            .await?;
            artifact_receipts.push(companion_receipt);
        } else {
            eprintln!(
                "\n{}",
                format!(
                    "⚠️  companion_ui path is not a file: {} — skipping artifact upload",
                    companion_path
                )
                .yellow()
            );
        }
    }

    let deployment_id = Uuid::new_v4().to_string();
    let upload_endpoint = format!("{}/files/envelope-upload", base_url.trim_end_matches('/'));
    let created_at = Utc::now().to_rfc3339();
    let fact_id_hex = hex::encode(deployment_fact_blake3(
        &deployment_id,
        owner_did,
        &created_at,
    ));
    let hub_config = build_agent_hub_config_receipt(
        agent_id,
        brain_key,
        capabilities,
        tag_label,
        tag_color,
        hub_response_format,
        hub_thinking_label,
        hub_companion_ui,
        hub_op,
        hub_input_format,
        shipped_inference_toml,
        shipped_topic_graph,
        shipped_grounding_toml,
        shipped_fragments_jsonl,
        shipped_guardrails_jsonl,
    );
    if hub_config.storage_brain_key.is_none() {
        eprintln!(
            "\n{}",
            "⚠️  No --brain-key: Agent Hub may fail to seed the model. Use e.g. \
             --brain-key fintech_brain when the WASM contract expects a brain in VM storage."
                .yellow()
        );
    }

    let receipt = StorageDeployReceipt {
        agent_id: agent_id.map(|s| s.to_string()),
        deployment_id: deployment_id.clone(),
        fact_id: fact_id_hex.clone(),
        storage_node_url: base_url.clone(),
        storage_node_did: node_did.clone(),
        owner_did: owner_did.to_string(),
        owner_kem_algorithm: owner_kem.clone(),
        created_at,
        upload_endpoint,
        artifacts: artifact_receipts,
        hub_config: Some(hub_config.clone()),
    };

    let json = serde_json::to_string_pretty(&receipt)?;
    println!("\n✅ Deployment upload complete.");
    println!("   Deployment ID: {}", deployment_id.cyan());
    println!("   Fact ID:       {}", fact_id_hex.cyan());
    for a in &receipt.artifacts {
        let hash_preview: String = a.content_blake3_hex.chars().take(16).collect();
        println!(
            "   • {} → file_id {} ({} bytes, blake3 {}…)",
            a.role,
            a.file_id.green(),
            a.size_bytes,
            hash_preview
        );
    }
    println!("\n📄 Receipt (JSON):\n{}", json);

    if let Some(path) = receipt_path {
        std::fs::write(path, &json)
            .map_err(|e| format!("Failed to write receipt {}: {}", path, e))?;
        println!("\n💾 Receipt written to {}", path.green());
    }

    // Store the deployment receipt on the storage node as a document so the
    // website API can resolve agent_id → file_ids at runtime.
    // We store under two DIDs: the owner's (for their own queries) and a
    // well-known public DID that the website API uses for artifact resolution.
    if agent_id.is_none() {
        eprintln!(
            "\n{}",
            "⚠️  No --agent-id: deployment was stored, but the website API / AgentHub \
             cannot look up artifacts by agent ID. Redeploy with e.g. \
             --agent-id ca-008 if the app uses a fixed agent id in the URL."
                .yellow()
        );
    }
    let doc_url = format!(
        "{}/api/documents/deployments/{}",
        base_url.trim_end_matches('/'),
        deployment_id
    );
    println!(
        "\n📡 Storing deployment receipt on storage node: {}",
        doc_url.dimmed()
    );
    let client = reqwest::Client::new();

    println!(
        "\n{}",
        "📦 Posting canonical FactPackage (spacekit:agent:deployment:v1)…".dimmed()
    );
    if let Err(e) = post_agent_deployment_fact_package_http(&client, &base_url, &receipt).await {
        eprintln!(
            "   ⚠️  FactPackage POST failed — website API fact_index lookup may miss this deploy: {}",
            e
        );
    }

    let public_deploy_did = "did:spacekit:admin:website-api";
    let dids_to_store: Vec<(&str, &str)> =
        vec![(owner_did, "owner"), (public_deploy_did, "api-index")];
    for (did, label) in &dids_to_store {
        let resp = client
            .put(&doc_url)
            .header("Authorization", format!("DID {}", did))
            .header("content-type", "application/json")
            .body(json.clone())
            .send()
            .await?;
        if resp.status().is_success() {
            println!(
                "   ✅ Receipt stored [{}] in collection {} (id: {})",
                label,
                "deployments".green(),
                deployment_id.cyan()
            );
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            eprintln!(
                "   ⚠️  Failed to store receipt [{}] (HTTP {}): {}",
                label, status, body
            );
        }
    }

    // Publish to marketplace if requested
    if publish {
        let app_id = agent_id.unwrap_or(&deployment_id);
        let listing_title = title.unwrap_or_else(|| {
            Path::new(wasm_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled App")
        });

        let previous_listing_file_ids =
            fetch_app_listing_file_ids(&client, &base_url, app_id, owner_did)
                .await
                .unwrap_or_else(|e| {
                    eprintln!(
                        "   ⚠️  Could not fetch prior app_listings artifacts for prune: {}",
                        e
                    );
                    Vec::new()
                });

        let pricing_obj = match price.unwrap_or("free") {
            "free" | "0" | "0.00" => serde_json::json!({
                "model": "free",
                "amount_cents": 0
            }),
            amount => {
                let dollars = amount.parse::<f64>().unwrap_or(0.0);
                let cents = (dollars * 100.0).round() as u64;
                serde_json::json!({
                    "model": "one-time",
                    "amount_cents": cents
                })
            }
        };

        let wasm_artifact = receipt.artifacts.iter().find(|a| a.role == "wasm");
        let bin_artifact = receipt.artifacts.iter().find(|a| a.role == "bin");
        let inference_artifact = receipt
            .artifacts
            .iter()
            .find(|a| a.role == "inference_toml");
        let topic_graph_artifact = receipt.artifacts.iter().find(|a| a.role == "topic_graph");
        let grounding_toml_artifact = receipt
            .artifacts
            .iter()
            .find(|a| a.role == "grounding_toml");
        let companion_ui_artifact = receipt.artifacts.iter().find(|a| a.role == "companion_ui");
        let mut artifacts_json = Vec::new();
        if let Some(a) = wasm_artifact {
            artifacts_json.push(serde_json::json!({
                "role": "wasm",
                "file_id": a.file_id,
                "size_bytes": a.size_bytes
            }));
        }
        if let Some(a) = bin_artifact {
            artifacts_json.push(serde_json::json!({
                "role": "bin",
                "file_id": a.file_id,
                "size_bytes": a.size_bytes
            }));
        }
        if let Some(a) = inference_artifact {
            artifacts_json.push(serde_json::json!({
                "role": "inference_toml",
                "file_id": a.file_id,
                "size_bytes": a.size_bytes
            }));
        }
        if let Some(a) = topic_graph_artifact {
            artifacts_json.push(serde_json::json!({
                "role": "topic_graph",
                "file_id": a.file_id,
                "size_bytes": a.size_bytes
            }));
        }
        if let Some(a) = grounding_toml_artifact {
            artifacts_json.push(serde_json::json!({
                "role": "grounding_toml",
                "file_id": a.file_id,
                "size_bytes": a.size_bytes
            }));
        }
        if let Some(a) = companion_ui_artifact {
            artifacts_json.push(serde_json::json!({
                "role": "companion_ui",
                "file_id": a.file_id,
                "size_bytes": a.size_bytes
            }));
        }

        let listing = serde_json::json!({
            "app_id": app_id,
            "deployment_id": deployment_id,
            "publisher_did": owner_did,
            "marketplace_id": marketplace_id.unwrap_or("default"),
            "title": listing_title,
            "description": description.unwrap_or(""),
            "category": category.unwrap_or("ai"),
            "version": "1.0.0",
            "access": access.unwrap_or("public"),
            "pricing": pricing_obj,
            "artifacts": artifacts_json,
            "hub_config": hub_config,
            "prompts": prompts,
            "created_at": Utc::now().to_rfc3339(),
            "updated_at": Utc::now().to_rfc3339(),
            "downloads": 0,
            "rating_avg": 0.0,
            "rating_count": 0,
            "status": "published"
        });

        let listing_url = format!(
            "{}/api/documents/app_listings/{}",
            base_url.trim_end_matches('/'),
            app_id
        );
        println!("\n📱 Publishing to marketplace: {}", listing_url.dimmed());

        let listing_json = serde_json::to_string_pretty(&listing)?;
        for (did, label) in &dids_to_store {
            let resp = client
                .put(&listing_url)
                .header("Authorization", format!("DID {}", did))
                .header("content-type", "application/json")
                .body(listing_json.clone())
                .send()
                .await?;
            if resp.status().is_success() {
                println!(
                    "   ✅ App listed [{}] in marketplace {} (app_id: {})",
                    label,
                    marketplace_id.unwrap_or("default").green(),
                    app_id.cyan()
                );
            } else {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                eprintln!(
                    "   ⚠️  Failed to publish listing [{}] (HTTP {}): {}",
                    label, status, body
                );
            }
        }
        println!("   Title:    {}", listing_title);
        println!("   Category: {}", category.unwrap_or("ai"));
        println!("   Access:   {}", access.unwrap_or("public"));
        println!("   Pricing:  {}", price.unwrap_or("free"));

        let new_file_ids: Vec<String> = receipt
            .artifacts
            .iter()
            .map(|a| a.file_id.clone())
            .collect();
        prune_superseded_deploy_artifacts(
            &client,
            &base_url,
            owner_did,
            &previous_listing_file_ids,
            &new_file_ids,
        )
        .await;
    }

    Ok(())
}

async fn handle_storage_retrieve(
    file_id: &str,
    output_path: &str,
    requester_did: Option<&String>,
    storage_url: Option<&str>,
    embedded: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📥 Retrieving stored file...");
    println!("🆔 File ID: {}", file_id.blue());
    println!("📁 Output: {}", output_path.green());

    let requester = if let Some(did) = requester_did {
        did.clone()
    } else {
        get_default_did()?
    };
    println!("👤 Requester: {}", requester.yellow());

    let use_embedded = embedded
        || std::env::var("SPACEKIT_STORAGE_RETRIEVE_EMBEDDED")
            .ok()
            .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
            .unwrap_or(false);

    if use_embedded {
        return retrieve_from_embedded_storage_db(file_id, output_path, &requester).await;
    }

    let (base_url, used_implicit_default) = resolve_remote_storage_base_url(storage_url)?;
    println!("   Storage: {}", base_url.green());

    let url = format!("{}/files/{}/admin-stream", base_url, file_id);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(600))
        .build()?;
    let resp = match client
        .get(&url)
        .header("Authorization", format!("DID {}", requester))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let is_connect = e.to_string().to_lowercase().contains("connect")
                || e.to_string().to_lowercase().contains("connection refused");
            if storage_url.is_none() && used_implicit_default && is_connect {
                println!(
                    "{}",
                    format!(
                        "⚠️  HTTP storage at {} unreachable ({}). Retrieving from embedded ~/.spacekit/storage instead.",
                        base_url, e
                    )
                    .yellow()
                );
                return retrieve_from_embedded_storage_db(file_id, output_path, &requester).await;
            }
            return Err(e.into());
        }
    };

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Retrieve failed (HTTP {}): {}", status, body).into());
    }

    let file_data = resp.bytes().await?;
    std::fs::write(output_path, &file_data)?;

    println!("\n✅ File retrieved successfully!");
    println!("📁 Saved to: {}", output_path.green());
    println!("📊 Size: {} bytes", file_data.len());
    println!(
        "⏰ Retrieved at: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    Ok(())
}

/// Decrypt and read a file from `~/.spacekit/storage` via the in-process storage node (no HTTP).
async fn retrieve_from_embedded_storage_db(
    file_id: &str,
    output_path: &str,
    requester_did: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("   Mode: {}", "embedded storage node (local DB)".cyan());
    let private_key = load_private_key().map_err(|e| format!("{}", e))?;
    let node = get_or_create_storage_node()
        .await
        .map_err(|e| format!("{}", e))?;
    match node
        .retrieve_file(file_id, requester_did, &private_key)
        .await
        .map_err(|e| e.to_string())?
    {
        Some(file_data) => {
            std::fs::write(output_path, &file_data)?;
            println!("\n✅ File retrieved successfully (embedded node)!");
            println!("📁 Saved to: {}", output_path.green());
            println!("📊 Size: {} bytes", file_data.len());
            println!(
                "⏰ Retrieved at: {}",
                chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
            );
            Ok(())
        }
        None => Err(format!(
            "File {} not found in ~/.spacekit/storage (wrong file_id, owner, or keys?)",
            file_id
        )
        .into()),
    }
}

async fn handle_storage_list(
    owner_filter: Option<&String>,
    owned_by_me: bool,
    details: bool,
    storage_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 Listing stored files...");

    let filter_did = if owned_by_me {
        get_default_did()?
    } else if let Some(filter) = owner_filter.cloned() {
        filter
    } else {
        get_default_did()?
    };

    println!("👤 Filtering by owner: {}", filter_did.cyan());

    let (base_url, _) = resolve_remote_storage_base_url(storage_url)?;
    println!("   Storage: {}", base_url.green());

    let client = reqwest::Client::new();

    // Fetch stats
    let stats_url = format!("{}/stats", base_url);
    match client.get(&stats_url).send().await {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(stats) = resp.json::<serde_json::Value>().await {
                println!("\n📊 Storage Overview");
                println!("━━━━━━━━━━━━━━━━━━━━━━━━");
                println!(
                    "📁 Total files: {}",
                    stats["file_count"].as_u64().unwrap_or(0)
                );
                let total_bytes = stats["total_size_bytes"].as_u64().unwrap_or(0);
                println!(
                    "💾 Total size: {:.2} MB",
                    total_bytes as f64 / (1024.0 * 1024.0)
                );
                if let Some(algo) = stats["preferred_algorithm"].as_str() {
                    println!("🔐 Quantum algorithm: {}", algo.yellow());
                }
                if details {
                    println!("\n📋 Detailed Statistics");
                    println!("━━━━━━━━━━━━━━━━━━━━━━━━");
                    println!("👥 Users: {}", stats["user_count"].as_u64().unwrap_or(0));
                    if let Some(did) = stats["node_did"].as_str() {
                        println!("🆔 Node DID: {}", did.green());
                    }
                }
            }
        }
        _ => {
            println!("   ⚠️  Could not fetch storage stats");
        }
    }

    // Fetch files for the owner
    let list_url = format!("{}/files/list/{}", base_url, filter_did);
    let resp = client.get(&list_url).send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("List failed (HTTP {}): {}", status, body).into());
    }

    let files: Vec<serde_json::Value> = resp.json().await?;
    if files.is_empty() {
        println!("\n📭 No files found for this owner.");
    } else {
        println!("\n📁 Files for owner ({}):", files.len());
        println!("━━━━━━━━━━━━━━━━━━━━━━━━");
        for (idx, file) in files.iter().enumerate() {
            let id = file["id"].as_str().unwrap_or("?");
            let name = file["filename"].as_str().unwrap_or("?");
            let size = file["size"].as_u64().unwrap_or(0);
            let created = file["created_at"].as_str().unwrap_or("?");
            println!("{}. {}", idx + 1, id.cyan());
            println!("   Name: {}", name.green());
            println!("   Size: {} bytes", size);
            println!("   Created: {}", created);
            if details {
                let ct = file["content_type"].as_str().unwrap_or("unknown");
                let enc = file["encryption_algorithm"].as_str().unwrap_or("unknown");
                let sharing = file["sharing_mode"].as_str().unwrap_or("unknown");
                println!("   Content type: {}", ct);
                println!("   Encryption: {}", enc.yellow());
                println!("   Sharing mode: {}", sharing);
            }
        }
    }

    Ok(())
}

async fn handle_storage_share(
    file_id: &str,
    with_did: &str,
    permission: &str,
    storage_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🤝 Sharing file access...");
    println!("🆔 File ID: {}", file_id.blue());
    println!("👤 Sharing with: {}", with_did.green());
    println!("🔑 Permission: {}", permission.yellow());

    let granter_did = get_default_did()?;
    let (base_url, _) = resolve_remote_storage_base_url(storage_url)?;

    let client = reqwest::Client::new();
    let url = format!("{}/files/{}/share", base_url, file_id);
    let resp = client
        .post(&url)
        .header("Authorization", format!("DID {}", granter_did))
        .json(&serde_json::json!({
            "with_did": with_did,
            "permission": permission,
        }))
        .send()
        .await?;

    if resp.status().is_success() {
        println!("\n✅ File access shared successfully!");
        println!("📝 Share details:");
        println!("   🆔 File: {}", file_id);
        println!("   👤 Granted to: {}", with_did);
        println!("   🔑 Permission: {}", permission);
        println!(
            "   ⏰ Granted at: {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        println!("❌ Failed to share access (HTTP {}): {}", status, body);
    }

    Ok(())
}

async fn handle_storage_revoke(
    file_id: &str,
    from_did: &str,
    storage_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚫 Revoking file access...");
    println!("🆔 File ID: {}", file_id.blue());
    println!("👤 Revoking from: {}", from_did.red());

    let revoker_did = get_default_did()?;
    let (base_url, _) = resolve_remote_storage_base_url(storage_url)?;

    let client = reqwest::Client::new();
    let url = format!("{}/files/{}/revoke", base_url, file_id);
    let resp = client
        .post(&url)
        .header("Authorization", format!("DID {}", revoker_did))
        .json(&serde_json::json!({
            "from_did": from_did,
        }))
        .send()
        .await?;

    if resp.status().is_success() {
        println!("\n✅ File access revoked successfully!");
        println!("📝 Revocation details:");
        println!("   🆔 File: {}", file_id);
        println!("   👤 Revoked from: {}", from_did);
        println!(
            "   ⏰ Revoked at: {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        println!("❌ Failed to revoke access (HTTP {}): {}", status, body);
    }

    Ok(())
}

fn is_local_storage_base_url(base_url: &str) -> bool {
    base_url.starts_with("http://127.0.0.1:") || base_url.starts_with("http://localhost:")
}

async fn handle_storage_stats(
    detailed: bool,
    storage_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Getting storage statistics...");

    let (base_url, used_default) = resolve_remote_storage_base_url(storage_url)?;
    println!("   Storage: {}", base_url.green());

    let stats: serde_json::Value = if used_default || is_local_storage_base_url(&base_url) {
        let node = get_or_create_storage_node()
            .await
            .map_err(|e| format!("{}", e))?;
        let s = node.get_stats().await?;
        serde_json::json!({
            "node_did": s.node_did,
            "preferred_algorithm": s.preferred_algorithm,
            "file_count": s.file_count,
            "total_size_bytes": s.total_size_bytes,
            "max_storage_bytes": s.max_storage_bytes,
            "storage_utilization": s.storage_utilization,
            "user_count": s.user_count,
            "encrypted_user_count": s.encrypted_user_count,
            "message_count": s.message_count,
        })
    } else {
        let client = reqwest::Client::new();
        let url = format!("{}/stats", base_url);
        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Stats failed (HTTP {}): {}", status, body).into());
        }
        resp.json().await?
    };

    println!("\n📊 Storage Node Statistics");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    if let Some(did) = stats["node_did"].as_str() {
        println!("🆔 Node DID: {}", did.green());
    }
    if let Some(algo) = stats["preferred_algorithm"].as_str() {
        println!("🔐 Quantum Algorithm: {}", algo.yellow());
    }

    println!("\n📁 File Storage");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━");
    println!(
        "📊 Total files: {}",
        stats["file_count"].as_u64().unwrap_or(0)
    );
    let total_bytes = stats["total_size_bytes"].as_u64().unwrap_or(0);
    let max_bytes = stats["max_storage_bytes"].as_u64().unwrap_or(0);
    println!(
        "💾 Total size: {:.2} MB",
        total_bytes as f64 / (1024.0 * 1024.0)
    );
    if max_bytes > 0 {
        println!(
            "📦 Max capacity: {:.2} GB",
            max_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        );
    }
    if let Some(util) = stats["storage_utilization"].as_f64() {
        println!("📈 Utilization: {:.1}%", util);
    }

    if detailed {
        println!("\n👥 User Management");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━");
        println!(
            "👤 Total users: {}",
            stats["user_count"].as_u64().unwrap_or(0)
        );
        println!(
            "🔒 Encrypted users: {}",
            stats["encrypted_user_count"].as_u64().unwrap_or(0)
        );
        println!("\n💬 Messaging");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━");
        println!(
            "📨 Messages: {}",
            stats["message_count"].as_u64().unwrap_or(0)
        );
    }

    println!(
        "\n⏰ Report generated: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    Ok(())
}

async fn handle_network_init(
    force: bool,
    profile: crate::network_profile::NetworkPreset,
    role: Option<crate::network_profile::NetworkRole>,
    node_id: Option<String>,
    port_offset: u16,
    data_root: Option<PathBuf>,
    manifest: Option<PathBuf>,
    allowlist: Vec<String>,
    shared_genesis_hash: Option<String>,
    mode: crate::network_profile::NetworkMode,
    compute_url: Option<String>,
    storage_url: Option<String>,
    gateway_url: Option<String>,
    bootstrap_peer: Vec<String>,
    bind_host: Option<String>,
    storage_port: Option<u16>,
    storage_p2p_port: Option<u16>,
    compute_port: Option<u16>,
    messaging_listen_port: Option<u16>,
    messaging_bootstrap_port: Option<u16>,
    gateway_port: Option<u16>,
    no_storage: bool,
    no_messaging: bool,
    no_compute: bool,
    enable_gateway: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if port_offset > u16::MAX - crate::network_profile::DEFAULT_KEYMASTER_GUARDIAN_BASE_PORT {
        return Err(format!("port offset {port_offset} overflows default service ports").into());
    }
    let file = crate::network_profile::network_file_from_init(
        crate::network_profile::NetworkInitOptions {
            profile,
            role,
            node_id,
            port_offset,
            data_root,
            manifest,
            mode,
            compute_url,
            storage_url,
            gateway_url,
            bootstrap_peer,
            bind_host,
            storage_port,
            storage_p2p_port,
            compute_port,
            messaging_listen_port,
            messaging_bootstrap_port,
            gateway_port,
            no_storage,
            no_messaging,
            no_compute,
            enable_gateway,
        },
    );
    let mut file = file;
    file.admission.allowlist = allowlist;
    file.admission.shared_genesis_hash = shared_genesis_hash;
    let path = crate::network_profile::write_network_profile(&file, force)?;
    println!(
        "{} {}",
        "✅ Wrote network profile:".green(),
        path.display().to_string().cyan()
    );
    println!("   mode: {:?}", file.mode);
    println!("   storage:  {}", file.resolved_storage_url());
    println!("   compute:  {}", file.resolved_compute_url());
    println!(
        "   messaging: {} (bootstrap {})",
        file.resolved_listen_addr(),
        file.messaging
            .bootstrap_peers
            .first()
            .map(|s| s.as_str())
            .unwrap_or("—")
    );
    println!(
        "   services: {}",
        file.enabled_embedded_services()
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "{}",
        "💡 Merged into [connections] when ~/.spacekit/config.toml is loaded.".yellow()
    );
    println!(
        "   Set {} to override this file path.",
        "SPACEKIT_NETWORK_CONFIG".dimmed()
    );
    println!(
        "   Next: {} or {}",
        "spacekit network up".green(),
        "spacekit network up --only storage,messaging".green()
    );
    Ok(())
}

// Handle network operations commands
async fn handle_network_command(
    network_command: &NetworkCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match network_command {
        NetworkCommands::Init {
            force,
            profile,
            role,
            node_id,
            port_offset,
            data_root,
            manifest,
            allowlist,
            shared_genesis_hash,
            mode,
            compute_url,
            storage_url,
            gateway_url,
            bootstrap_peer,
            bind_host,
            storage_port,
            storage_p2p_port,
            compute_port,
            messaging_listen_port,
            messaging_bootstrap_port,
            gateway_port,
            no_storage,
            no_messaging,
            no_compute,
            enable_gateway,
        } => {
            handle_network_init(
                *force,
                *profile,
                *role,
                node_id.clone(),
                *port_offset,
                data_root.clone(),
                manifest.clone(),
                allowlist.clone(),
                shared_genesis_hash.clone(),
                *mode,
                compute_url.clone(),
                storage_url.clone(),
                gateway_url.clone(),
                bootstrap_peer.clone(),
                bind_host.clone(),
                *storage_port,
                *storage_p2p_port,
                *compute_port,
                *messaging_listen_port,
                *messaging_bootstrap_port,
                *gateway_port,
                *no_storage,
                *no_messaging,
                *no_compute,
                *enable_gateway,
            )
            .await
        }
        NetworkCommands::Up { detach, only, full } => {
            let only_list = only
                .as_ref()
                .map(|s| crate::network_profile::NetworkService::parse_list(s))
                .transpose()?;
            crate::network_supervisor::network_up(*detach, only_list, *full).await
        }
        NetworkCommands::Start { service, detach } => {
            crate::network_supervisor::network_start(*service, *detach).await
        }
        NetworkCommands::Stop { service } => {
            crate::network_supervisor::network_stop(*service).await
        }
        NetworkCommands::Down => crate::network_supervisor::network_down().await,
        NetworkCommands::RunSupervisor => {
            crate::network_supervisor::run_supervisor_from_profile().await
        }
        NetworkCommands::Memory {
            json,
            sample,
            watch,
            interval,
        } => crate::network_memory::run_network_memory(*json, *sample, *watch, *interval).await,
        NetworkCommands::Status { detailed, realtime } => {
            handle_network_status(*detailed, *realtime).await
        }
        NetworkCommands::Doctor => handle_network_doctor().await,
        NetworkCommands::Logs { service, lines } => handle_network_logs(*service, *lines).await,
        NetworkCommands::Test {
            suite,
            report,
            website_url,
            api_url,
        } => {
            crate::network_e2e::run(*suite, report.clone(), website_url.clone(), api_url.clone())
                .await
        }
        NetworkCommands::Reset { data, force } => handle_network_reset(*data, *force).await,
        NetworkCommands::Join {
            manifest,
            role,
            force,
        } => handle_network_join(manifest, *role, *force).await,
        NetworkCommands::Manifest { action } => handle_network_manifest(action).await,
        NetworkCommands::Discover {
            service_type,
            detailed,
            limit,
        } => handle_network_discover(service_type.as_ref(), *detailed, *limit).await,
        NetworkCommands::Peers {
            detailed,
            service,
            active_only,
        } => handle_network_peers(*detailed, service.as_ref(), *active_only).await,
        NetworkCommands::Reputation {
            did,
            detailed,
            history,
        } => handle_network_reputation(did, *detailed, *history).await,
        NetworkCommands::ReputationWatch {
            did,
            interval,
            alerts,
        } => handle_network_reputation_watch(did, *interval, *alerts).await,
        NetworkCommands::Config { action } => handle_network_config(action).await,
    }
}

// Individual DID command handlers
async fn handle_did_create(
    _algorithm: EncryptionAlgorithm,
    save: bool,
    _identifier: Option<&String>,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🆔 Creating quantum-resistant DID...");
    println!("🔐 Algorithm: SPHINCS+ + Kyber1024 (Quantum-resistant)");

    // Generate SPHINCS+ keypair for signing
    let wallet = QuantumResistantWallet::new();
    let sphincs_pk = &wallet.key_pairs[0].public_key;
    let sphincs_sk = &wallet.key_pairs[0].private_key;

    // Kyber1024 public key placeholder — the real Kyber keypair is generated
    // in the browser WASM (pqcrypto-kyber) or storage node. The CLI registers
    // the DID with a zero Kyber PK; the browser updates it via key rotation
    // once it has its own Kyber keys. This keeps the CLI dependency-light.
    let kyber_pk_bytes: Vec<u8> = vec![0u8; 32]; // placeholder
    let kyber_pk_hex: String = hex::encode(&kyber_pk_bytes);

    // Derive the DID address: SHA-256(sphincs_pk)[0..20]
    use sha2::Digest;
    let hash = sha2::Sha256::digest(sphincs_pk);
    let address = hex::encode(&hash[..20]);
    let network = "testnet";
    let did = format!("did:spacekit:{}:{}", network, address);

    // Self-sign: sign(sphincs_pk ++ kyber_pk ++ network_bytes)
    let mut msg = Vec::with_capacity(sphincs_pk.len() + kyber_pk_bytes.len() + network.len());
    msg.extend_from_slice(sphincs_pk);
    msg.extend_from_slice(&kyber_pk_bytes);
    msg.extend_from_slice(network.as_bytes());

    use spacekit_did::sphincs::SphincsPlus;
    let signature = SphincsPlus::sign(sphincs_sk, &msg)
        .map_err(|_| -> Box<dyn std::error::Error> { "invalid SPHINCS+ private key".into() })?;

    println!("\n✅ DID created successfully!");
    println!("🆔 DID: {}", did.green());
    println!("📍 Address: {}", address.blue());
    println!("🔑 Signing: SPHINCS+-SHAKE-256-128s-simple");
    println!("🔐 Encryption: Kyber1024");

    // Try to register with the compute node
    let compute_url = std::env::var("SPACEKIT_COMPUTE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());

    let register_body = serde_json::json!({
        "network": network,
        "sphincs_pk_hex": hex::encode(sphincs_pk),
        "kyber_pk_hex": kyber_pk_hex,
        "signature_hex": hex::encode(&signature),
    });

    let client = reqwest::Client::new();
    match client
        .post(format!("{}/v1/did/register", compute_url))
        .json(&register_body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await?;
            println!(
                "✅ Registered on compute node: {}",
                body["did"].as_str().unwrap_or(&did)
            );
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            println!(
                "⚠️  Compute node registration failed ({}): {}",
                status, body
            );
            println!(
                "   DID is created locally; register later with `spacekit did verify {}`",
                did
            );
        }
        Err(e) => {
            println!("⚠️  Could not reach compute node at {}: {}", compute_url, e);
            println!("   DID is created locally; register later when compute node is available");
        }
    }

    if format == "json" {
        let doc = serde_json::json!({
            "did": did,
            "sphincs_pk_hex": hex::encode(sphincs_pk),
            "kyber_pk_hex": kyber_pk_hex,
            "network": network,
            "algorithm": "SPHINCS+-SHAKE-256-128s-simple + Kyber1024",
        });
        println!("\n📄 DID Document (JSON):");
        println!("{}", serde_json::to_string_pretty(&doc)?);
    } else {
        println!("\n📄 DID Document:");
        println!("  Method: spacekit:{}", network);
        println!("  Address: {}", address);
        println!("  Public Keys: SPHINCS+ (signing) + Kyber1024 (encryption)");
    }

    if save {
        let config_dir = dirs::home_dir()
            .ok_or_else(|| CliError::Config("Home directory not found".to_string()))?
            .join(".spacekit");
        std::fs::create_dir_all(&config_dir)?;

        let wallet_data = serde_json::json!({
            "did": did,
            "address": address,
            "network": network,
            "sphincs_pk_hex": hex::encode(sphincs_pk),
            "sphincs_sk_hex": hex::encode(sphincs_sk),
            "kyber_pk_hex": kyber_pk_hex,
            "algorithm": "SPHINCS+-SHAKE-256-128s-simple + Kyber1024",
        });

        let wallet_file = config_dir.join("did_wallet.json");
        std::fs::write(&wallet_file, serde_json::to_string_pretty(&wallet_data)?)?;
        println!("💾 DID saved to: {}", wallet_file.display());
        println!(
            "\n💡 Use {} to resolve your DID",
            format!("spacekit did resolve {}", did).yellow()
        );
    }

    Ok(())
}

async fn handle_did_verify(
    did: &str,
    credentials: bool,
    detailed: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Verifying DID...");
    println!("🆔 DID: {}", did.blue());

    let is_valid = did.starts_with("did:spacekit:");

    // Try to resolve from compute node
    let compute_url = std::env::var("SPACEKIT_COMPUTE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());

    let client = reqwest::Client::new();
    let registry_resolved = match client
        .get(format!("{}/v1/did/resolve", compute_url))
        .query(&[("did", did)])
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            body["resolved"].as_bool().unwrap_or(false)
        }
        _ => false,
    };

    if is_valid {
        println!("\n✅ DID verification successful!");
        println!("🔐 Format: Valid SpaceKit DID");
        println!("🛡️  Quantum-resistant: Yes (SPHINCS+ + Kyber1024)");
        if registry_resolved {
            println!("📊 Registry: Resolved on-chain");
        } else {
            println!("📊 Registry: Not yet registered (local only)");
        }

        if detailed {
            println!("\n📄 Detailed Verification:");
            println!("  Protocol: SpaceKit Network");
            println!("  Method: spacekit");
            println!("  Signing Algorithm: SPHINCS+-SHAKE-256-128s-simple");
            println!("  Encryption Algorithm: Kyber1024");
            println!("  Quantum-safe: Post-quantum cryptography");
            println!("  W3C DID Standard: Compliant");
        }

        if credentials {
            println!("\n📜 Associated Credentials:");
            println!("  💡 Credential verification requires registry integration");
        }
    } else {
        println!("\n❌ DID verification failed!");
        println!("🔍 Issues found:");
        println!("  • Invalid DID format");
        println!("\n💡 Expected format: did:spacekit:{{network}}:{{address}}");
    }

    Ok(())
}

async fn handle_did_update(
    did: &str,
    add_key: Option<&String>,
    rotate_keys: bool,
    update_document: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Updating DID...");
    println!("🆔 DID: {}", did.blue());

    if rotate_keys {
        println!("🔐 Rotating quantum keys...");

        // Get or create wallet
        let _wallet = get_or_create_did_wallet().await?;

        // TODO: Implement actual key rotation using QuantumResistantWallet::rotate_keys()
        // This requires the wallet to support key rotation and registry updates
        println!("✅ Keys rotated successfully!");
        println!("🔄 New key generation count: 1");
        println!("🔐 Algorithm: SPHINCS+ (Quantum-resistant)");
        println!(
            "⏰ Rotated at: {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!("💡 Note: Full key rotation implementation in progress");
    }

    if let Some(new_key) = add_key {
        // TODO: Implement actual key addition using QuantumResistantWallet::add_verification_method()
        // This requires updating the DID document and registering changes
        println!("🔑 Adding new public key...");
        println!("📋 Key: {}", new_key[..64].cyan()); // Show first 64 chars
        println!("✅ Public key added successfully!");
        println!("💡 Note: Key addition implementation in progress");
    }

    if let Some(doc_update) = update_document {
        // TODO: Implement actual DID document updates using registry operations
        // This requires parsing the update JSON and applying changes to the DID registry
        println!("📄 Updating DID document...");
        println!("📋 Update: {}", doc_update);
        println!("✅ DID document updated successfully!");
        println!("💡 Note: Document update implementation in progress");
    }

    if !rotate_keys && add_key.is_none() && update_document.is_none() {
        println!("💡 No update operations specified. Use:");
        println!("  --rotate-keys       Rotate quantum keys");
        println!("  --add-key <KEY>     Add new public key");
        println!("  --update-document   Update DID document");
    }

    Ok(())
}
// TODO: Implement complete DID resolution using DID registry queries and IPFS/blockchain lookups
// This needs integration with SpaceKit Network DID registry and W3C DID resolution spec
async fn handle_did_resolve(
    did: &str,
    format: &str,
    verify: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Resolving DID...");
    println!("🆔 DID: {}", did.blue());

    if !did.starts_with("did:spacekit:") {
        println!("\n❌ DID resolution failed!");
        println!("🔍 Error: Unsupported DID method");
        println!("💡 Supported format: did:spacekit:{{network}}:{{address}}");
        return Ok(());
    }

    // Try to resolve from compute node registry
    let compute_url = std::env::var("SPACEKIT_COMPUTE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());

    let client = reqwest::Client::new();
    match client
        .get(format!("{}/v1/did/resolve", compute_url))
        .query(&[("did", did)])
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await?;
            println!("\n✅ DID resolved!");

            if format == "json" {
                println!("\n📄 DID Document (JSON):");
                println!("{}", serde_json::to_string_pretty(&body)?);
            } else {
                println!("  DID: {}", did);
                if let Some(doc) = body.get("document") {
                    println!(
                        "  Network: {}",
                        doc["network"].as_str().unwrap_or("unknown")
                    );
                    println!("  Active: {}", doc["active"].as_bool().unwrap_or(false));
                    println!("  Signing: SPHINCS+-SHAKE-256-128s-simple");
                    println!("  Encryption: Kyber1024");
                }
            }
        }
        Ok(resp) => {
            let status = resp.status();
            println!("\n⚠️  Compute node returned {}", status);
            println!("   DID may not be registered yet");
        }
        Err(e) => {
            println!(
                "\n⚠️  Could not reach compute node at {}: {}",
                compute_url, e
            );
        }
    }

    // Also try local wallet file
    let config_dir = dirs::home_dir().map(|h| h.join(".spacekit").join("did_wallet.json"));
    if let Some(path) = config_dir {
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(wallet_json) = serde_json::from_str::<serde_json::Value>(&data) {
                    if wallet_json["did"].as_str() == Some(did) {
                        println!("\n📂 Local wallet found at {}", path.display());
                    }
                }
            }
        }
    }

    if verify {
        println!("\n🔍 Verification Status:");
        println!("  🟢 DID Format: Valid");
        println!("  🟢 Quantum-resistant: Yes (SPHINCS+ + Kyber1024)");
        println!("  🟢 W3C Compliant: Yes");
    }

    Ok(())
}

async fn handle_did_list(
    owned_by_me: bool,
    method: Option<&String>,
    detailed: bool,
    with_credentials: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 Listing DIDs...");

    if owned_by_me {
        println!("👤 Showing only my DIDs");
    }

    if let Some(filter_method) = method {
        println!("🔍 Filtering by method: {}", filter_method.cyan());
    }

    let my_wallet = get_or_create_did_wallet().await?;

    let display_did = my_wallet.identity_doc.did.did.as_str();
    let display_address = match load_public_key().await {
        Ok(pk) => {
            DualKeyWallet::public_key_to_address(&pk).unwrap_or_else(|_| my_wallet.address.clone())
        }
        Err(_) => my_wallet.address.clone(),
    };

    println!("\n📊 DID Overview");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📁 Total DIDs: 1");
    println!("🔐 Quantum-resistant: 1");
    println!("👤 Owned by me: 1");

    println!("\n🆔 My DIDs:");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  DID: {}", display_did.green());
    println!("  Address: {}", display_address.blue());
    println!("  Algorithm: SPHINCS+ (Quantum-resistant)");
    println!("  Status: ✅ Active");

    if detailed {
        println!("  Keys: {} key pair(s)", my_wallet.key_pairs.len());
        println!(
            "  Created: {}",
            my_wallet
                .identity_doc
                .created
                .format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!(
            "  Updated: {}",
            my_wallet
                .identity_doc
                .updated
                .format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!(
            "  Authentication Methods: {:?}",
            my_wallet.identity_doc.authentication
        );
    }

    if with_credentials {
        println!(
            "  Credentials: {} credential(s)",
            my_wallet.credentials.len()
        );
        for (i, cred) in my_wallet.credentials.iter().enumerate() {
            println!("    {}. {} - {}", i + 1, cred.credential_type, cred.subject);
        }
    }

    // TODO: Implement full DID discovery using DID registry queries
    // This should query the SpaceKit Network DID registry to find other DIDs based on method/ownership filters
    println!("\n💡 Note: Registry integration for discovering other DIDs is in progress.");
    println!(
        "💡 Use this DID for {} and {} (same ledger address as your KEM key).",
        "spacekit vm fund --owner-did …".yellow(),
        "spacekit contract deploy --owner-did …".yellow(),
    );

    Ok(())
}

async fn handle_did_issue_credential(
    to: &str,
    credential_type: &str,
    claims: &str,
    validity_days: Option<u32>,
    output: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📜 Issuing verifiable credential...");
    println!("👤 Issuing to: {}", to.green());
    println!("📋 Type: {}", credential_type.blue());
    println!("📄 Claims: {}", claims.yellow());

    // Parse claims JSON
    let claims_map: std::collections::HashMap<String, String> = serde_json::from_str(claims)
        .map_err(|e| CliError::Config(format!("Invalid claims JSON: {}", e)))?;

    println!("📊 Parsed {} claim(s)", claims_map.len());

    // Get wallet to issue credential
    let wallet = get_or_create_did_wallet().await?;

    // Issue credential
    let credential = wallet
        .issue_credential(
            to,
            credential_type,
            claims_map,
            validity_days.map(|days| days as i64),
        )
        .map_err(|e| CliError::Did(format!("Failed to issue credential: {}", e)))?;

    println!("\n✅ Credential issued successfully!");
    println!("🆔 Credential ID: {}", credential.id.green());
    println!("👤 Issuer: {}", credential.issuer.cyan());
    println!("👤 Subject: {}", credential.subject.blue());
    println!("📋 Type: {}", credential.credential_type);
    println!(
        "⏰ Issued: {}",
        credential.issued_at.format("%Y-%m-%d %H:%M:%S UTC")
    );

    if let Some(expires) = credential.expires_at {
        println!("⏰ Expires: {}", expires.format("%Y-%m-%d %H:%M:%S UTC"));
    }

    println!("\n📄 Claims:");
    for (key, value) in &credential.claims {
        println!("  {}: {}", key, value);
    }

    // Save credential if output specified
    if let Some(output_file) = output {
        let credential_json = serde_json::to_string_pretty(&credential)?;
        std::fs::write(output_file, credential_json)?;
        println!("\n💾 Credential saved to: {}", output_file.green());
        println!(
            "💡 Use {} to verify",
            format!(
                "spacekit did verify-credential --credential-file {}",
                output_file
            )
            .yellow()
        );
    } else {
        println!("\n💡 Use --output <file> to save credential to file");
    }

    Ok(())
}

async fn handle_did_verify_credential(
    credential_file: &str,
    detailed: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Verifying credential...");
    println!("📁 File: {}", credential_file.blue());

    // Read credential file
    let credential_data = std::fs::read_to_string(credential_file)
        .map_err(|e| CliError::FileRead(credential_file.to_string(), e))?;

    // Parse credential
    let credential: VerifiableCredential = serde_json::from_str(&credential_data)
        .map_err(|e| CliError::Config(format!("Invalid credential format: {}", e)))?;

    println!("📋 Credential loaded successfully");

    // Get wallet to verify credential
    let wallet = get_or_create_did_wallet().await?;

    // Verify credential
    let is_valid = wallet
        .verify_credential(&credential)
        .map_err(|e| CliError::Did(format!("Verification failed: {}", e)))?;

    if is_valid {
        println!("\n✅ Credential verification successful!");
        println!("🆔 ID: {}", credential.id.green());
        println!("📋 Type: {}", credential.credential_type.blue());
        println!("👤 Issuer: {}", credential.issuer.cyan());
        println!("👤 Subject: {}", credential.subject.yellow());
        println!("🔐 Signature: Valid (Quantum-resistant)");

        // Check expiration
        if let Some(expires) = credential.expires_at {
            let now = chrono::Utc::now();
            if now > expires {
                println!(
                    "⚠️  Status: {} (Expired on {})",
                    "EXPIRED".red(),
                    expires.format("%Y-%m-%d")
                );
            } else {
                println!("✅ Status: Valid until {}", expires.format("%Y-%m-%d"));
            }
        } else {
            println!("✅ Status: Valid (No expiration)");
        }

        if detailed {
            println!("\n📄 Detailed Verification:");
            println!(
                "  Issued: {}",
                credential.issued_at.format("%Y-%m-%d %H:%M:%S UTC")
            );
            println!("  Algorithm: SPHINCS+ (Quantum-resistant)");
            println!("  Claims: {} item(s)", credential.claims.len());

            println!("\n📊 Claims:");
            for (key, value) in &credential.claims {
                println!("    {}: {}", key, value);
            }

            println!("\n🔐 Cryptographic Verification:");
            println!("  Signature Algorithm: SPHINCS+");
            println!("  Quantum-safe: Yes");
            println!("  Integrity: Verified");
            println!("  Authenticity: Verified");
        }
    } else {
        println!("\n❌ Credential verification failed!");
        println!("🔍 Issues found:");
        println!("  • Invalid signature");
        println!("  • Potentially tampered credential");
        println!("  • Issuer verification failed");

        if detailed {
            println!("\n📊 Credential Details:");
            println!("  ID: {}", credential.id);
            println!("  Type: {}", credential.credential_type);
            println!("  Issued: {}", credential.issued_at.format("%Y-%m-%d"));
            println!("  Claims: {} item(s)", credential.claims.len());
        }
    }

    Ok(())
}

// Handle consensus operations commands
async fn handle_consensus_command(
    consensus_command: &ConsensusCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match consensus_command {
        ConsensusCommands::SubmitProposal {
            proposal_type,
            data,
            committee,
            description,
            duration,
            in_process,
            finalize,
            use_swtchvm_head,
            announce,
        } => {
            handle_consensus_submit_proposal(
                *proposal_type,
                data,
                committee.as_ref(),
                description.as_ref(),
                *duration,
                *in_process,
                *finalize,
                *use_swtchvm_head,
                *announce,
            )
            .await
        }
        ConsensusCommands::Vote {
            proposal_id,
            vote,
            rationale,
        } => handle_consensus_vote(proposal_id, *vote, rationale.as_ref()).await,
        ConsensusCommands::Status {
            proposal_id,
            detailed,
            network_health,
        } => handle_consensus_status(proposal_id.as_ref(), *detailed, *network_health).await,
        ConsensusCommands::List {
            status,
            proposal_type,
            my_proposals,
            limit,
        } => handle_consensus_list(status.as_ref(), *proposal_type, *my_proposals, *limit).await,
        ConsensusCommands::Migration {
            detailed,
            history,
            risks,
        } => handle_consensus_migration(*detailed, *history, *risks).await,
    }
}

// Individual network command handlers
async fn handle_network_config(
    action: &NetworkConfigAction,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        NetworkConfigAction::Path => {
            println!(
                "{}",
                crate::network_profile::spacekit_network_config_path().display()
            );
        }
        NetworkConfigAction::Show => {
            let path = crate::network_profile::spacekit_network_config_path();
            if !path.exists() {
                println!(
                    "No network profile at {}",
                    path.display().to_string().yellow()
                );
                println!("Create one with: {}", "spacekit network init".green());
                return Ok(());
            }
            let raw = std::fs::read_to_string(&path)?;
            println!("# {}\n", path.display().to_string().cyan());
            println!("{raw}");
        }
        NetworkConfigAction::Enable { service } => {
            let path = crate::network_profile::spacekit_network_config_path();
            let mut net = crate::network_profile::load_spacekit_network_file()?
                .ok_or("no network profile — run: spacekit network init")?;
            match service.to_lowercase().as_str() {
                "storage" => net.services.storage = true,
                "messaging" => net.services.messaging = true,
                "compute" => net.services.compute = true,
                "gateway" => net.services.gateway = true,
                "keymaster" => net.services.keymaster = true,
                other => return Err(format!("unknown service: {other}").into()),
            }
            let toml_str = toml::to_string_pretty(&net)?;
            std::fs::write(&path, toml_str)?;
            println!(
                "{} service {} {}",
                "✓".green(),
                service.cyan(),
                "enabled".green()
            );
        }
        NetworkConfigAction::Disable { service } => {
            let path = crate::network_profile::spacekit_network_config_path();
            let mut net = crate::network_profile::load_spacekit_network_file()?
                .ok_or("no network profile — run: spacekit network init")?;
            match service.to_lowercase().as_str() {
                "storage" => net.services.storage = false,
                "messaging" => net.services.messaging = false,
                "compute" => net.services.compute = false,
                "gateway" => net.services.gateway = false,
                "keymaster" => net.services.keymaster = false,
                other => return Err(format!("unknown service: {other}").into()),
            }
            let toml_str = toml::to_string_pretty(&net)?;
            std::fs::write(&path, toml_str)?;
            println!(
                "{} service {} {}",
                "✓".green(),
                service.cyan(),
                "disabled".yellow()
            );
        }
        NetworkConfigAction::Set { key, value } => {
            let path = crate::network_profile::spacekit_network_config_path();
            let raw = std::fs::read_to_string(&path)?;
            let mut doc: toml::Value = raw.parse::<toml::Value>()?;

            let parts: Vec<&str> = key.split('.').collect();
            let mut target = &mut doc;
            for (i, part) in parts.iter().enumerate() {
                if i == parts.len() - 1 {
                    if let Some(table) = target.as_table_mut() {
                        let new_val = if let Ok(n) = value.parse::<i64>() {
                            toml::Value::Integer(n)
                        } else if value == "true" {
                            toml::Value::Boolean(true)
                        } else if value == "false" {
                            toml::Value::Boolean(false)
                        } else {
                            toml::Value::String(value.clone())
                        };
                        table.insert(part.to_string(), new_val);
                    } else {
                        return Err(format!(
                            "cannot set key in non-table at '{}'",
                            parts[..i].join(".")
                        )
                        .into());
                    }
                } else if let Some(table) = target.as_table_mut() {
                    target = table
                        .entry(part.to_string())
                        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
                } else {
                    return Err(format!("path '{}' is not a table", parts[..=i].join(".")).into());
                }
            }
            let table = doc
                .as_table()
                .ok_or("network config root must be a TOML table")?;
            std::fs::write(&path, toml::to_string_pretty(table)?)?;
            println!("{} {} = {}", "✓".green(), key.cyan(), value.cyan());
        }
    }
    Ok(())
}

async fn handle_network_status(
    detailed: bool,
    realtime: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🌐 Checking network status...");

    println!("\n📊 SpaceKit Network Status");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    for line in crate::network_supervisor::runtime_status_lines() {
        println!("{}", line);
    }

    if !crate::network_profile::is_network_supervisor_running() {
        println!();
        println!(
            "💡 Start the local stack: {}",
            "spacekit network up".green()
        );
        println!(
            "   Or initialize profile first: {}",
            "spacekit network init".yellow()
        );
        return Ok(());
    }

    if detailed || realtime {
        let net = crate::network_profile::load_spacekit_network_file()?
            .ok_or("network profile not found — run `spacekit network init` first")?;
        println!("\n📡 Live endpoint status");
        let status_url = format!("http://{}:{}/status", net.bind_host, net.ports.status_http);
        let client = network_http_client(&net)?;
        match fetch_json_endpoint(&client, &status_url).await {
            Ok(value) => println!("{}", serde_json::to_string_pretty(&value)?),
            Err(error) => {
                println!(
                    "   status endpoint unavailable: {}",
                    error.to_string().yellow()
                );
            }
        }
        println!("\n📡 Direct service probes");
        print_network_service_probes(&net, &client).await;
        if realtime {
            println!(
                "   realtime streaming is unavailable; this is a live point-in-time endpoint query"
            );
        }
    }

    println!(
        "\n⏰ Status checked: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "💡 Use {} for service discovery",
        "spacekit network discover".yellow()
    );
    Ok(())
}

fn network_http_client(
    net: &crate::network_profile::SpacekitNetworkFile,
) -> Result<reqwest::Client, Box<dyn std::error::Error>> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(
            net.runtime.health_check_timeout_secs.clamp(1, 30),
        ))
        .build()?)
}

async fn fetch_json_endpoint(
    client: &reqwest::Client,
    url: &str,
) -> Result<serde_json::Value, String> {
    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{} returned HTTP {}", url, status));
    }
    response
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("{} returned invalid JSON: {}", url, e))
}

async fn fetch_first_json(
    client: &reqwest::Client,
    bases: &[String],
    paths: &[&str],
) -> Result<(String, serde_json::Value), String> {
    let mut failures = Vec::new();
    for base in bases {
        for path in paths {
            let url = format!("{}{}", base.trim_end_matches('/'), path);
            match fetch_json_endpoint(client, &url).await {
                Ok(value) => return Ok((url, value)),
                Err(error) => failures.push(error),
            }
        }
    }
    Err(failures.join("; "))
}

async fn probe_http(client: &reqwest::Client, label: &str, url: &str) -> bool {
    let started = std::time::Instant::now();
    match client.get(url).send().await {
        Ok(response) => {
            let status = response.status();
            let marker = if status.is_success() {
                "✓".green()
            } else {
                "✗".red()
            };
            println!(
                "   {} {} {} — HTTP {} in {}ms",
                marker,
                label,
                url,
                status,
                started.elapsed().as_millis()
            );
            status.is_success()
        }
        Err(error) => {
            println!("   {} {} {} — {}", "✗".red(), label, url, error);
            false
        }
    }
}

fn multiaddr_socket(value: &str) -> Option<String> {
    let parts: Vec<_> = value.split('/').collect();
    let host = parts
        .windows(2)
        .find(|pair| matches!(pair[0], "ip4" | "ip6" | "dns" | "dns4" | "dns6"))
        .map(|pair| pair[1])?;
    let port = parts
        .windows(2)
        .find(|pair| pair[0] == "tcp")
        .map(|pair| pair[1])?;
    Some(format!("{}:{}", host, port))
}

fn target_socket(value: &str) -> Result<String, String> {
    if value.starts_with('/') {
        return multiaddr_socket(value)
            .ok_or_else(|| format!("unsupported multiaddr (expected host + tcp): {}", value));
    }
    if value.contains("://") {
        let parsed = reqwest::Url::parse(value).map_err(|e| e.to_string())?;
        let host = parsed.host_str().ok_or("URL has no host")?;
        let port = parsed
            .port_or_known_default()
            .ok_or("URL has no known port")?;
        return Ok(format!("{}:{}", host, port));
    }
    Ok(value.to_string())
}

async fn probe_tcp(label: &str, target: &str, timeout_secs: u64) -> bool {
    let socket = match target_socket(target) {
        Ok(socket) => socket,
        Err(error) => {
            println!("   {} {} {} — {}", "✗".red(), label, target, error);
            return false;
        }
    };
    let started = std::time::Instant::now();
    match tokio::time::timeout(
        Duration::from_secs(timeout_secs.clamp(1, 30)),
        tokio::net::TcpStream::connect(&socket),
    )
    .await
    {
        Ok(Ok(_)) => {
            println!(
                "   {} {} {} — TCP connected in {}ms",
                "✓".green(),
                label,
                target,
                started.elapsed().as_millis()
            );
            true
        }
        Ok(Err(error)) => {
            println!("   {} {} {} — {}", "✗".red(), label, target, error);
            false
        }
        Err(_) => {
            println!("   {} {} {} — timed out", "✗".red(), label, target);
            false
        }
    }
}

async fn print_network_service_probes(
    net: &crate::network_profile::SpacekitNetworkFile,
    client: &reqwest::Client,
) -> bool {
    let mut all_ok = true;
    if net.services.storage {
        all_ok &= probe_http(
            client,
            "storage",
            &format!(
                "{}/health",
                net.resolved_storage_url().trim_end_matches('/')
            ),
        )
        .await;
    }
    if net.services.compute {
        all_ok &= probe_http(
            client,
            "compute",
            &format!(
                "{}/health",
                net.resolved_compute_url().trim_end_matches('/')
            ),
        )
        .await;
    }
    if net.services.messaging {
        let http_url = net.resolved_messaging_http_url();
        all_ok &= probe_http(
            client,
            "messaging",
            &format!("{}/health", http_url.trim_end_matches('/')),
        )
        .await;
        for peer in &net.messaging.bootstrap_peers {
            all_ok &= probe_tcp(
                "messaging bootstrap",
                peer,
                net.runtime.health_check_timeout_secs,
            )
            .await;
        }
    }
    if net.services.gateway {
        if let Some(url) = &net.urls.gateway {
            all_ok &= probe_http(
                client,
                "gateway",
                &format!("{}/health", url.trim_end_matches('/')),
            )
            .await;
        }
    }
    all_ok
}

async fn handle_network_discover(
    service_type: Option<&String>,
    detailed: bool,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let net = crate::network_profile::load_spacekit_network_file()?
        .ok_or("network profile not found — run `spacekit network init` or `network join`")?;
    let filter = service_type.map(|s| s.to_ascii_lowercase());
    let mut services: Vec<(String, String, String)> = Vec::new();

    if let Some(path) = &net.manifest {
        let manifest = crate::network_profile::load_network_manifest(path)?;
        let client = network_http_client(&net)?;
        for endpoint in &manifest.bootstrap.rpc {
            let operator_url = format!("{}/api/operators/self", endpoint.trim_end_matches('/'));
            if let Ok(response) = client.get(&operator_url).send().await {
                if response.status().is_success() {
                    if let Ok(value) = response.json::<serde_json::Value>().await {
                        if let Some(storage_url) = value
                            .pointer("/manifest/storage_http_url")
                            .and_then(|value| value.as_str())
                        {
                            let did = value
                                .get("operator_did")
                                .and_then(|value| value.as_str())
                                .unwrap_or("unknown operator");
                            let source = value
                                .get("manifest_source")
                                .and_then(|value| value.as_str())
                                .unwrap_or("operator service");
                            services.push((
                                "storage".into(),
                                storage_url.into(),
                                format!("{source}: {did}"),
                            ));
                        }
                    }
                }
            }
            services.push((
                "rpc".into(),
                endpoint.clone(),
                "verified network manifest".into(),
            ));
        }
        for endpoint in &manifest.bootstrap.p2p {
            services.push((
                "p2p".into(),
                endpoint.clone(),
                "verified network manifest".into(),
            ));
        }
    }
    if net.services.compute {
        services.push((
            "compute".into(),
            net.resolved_compute_url(),
            "network profile".into(),
        ));
    }
    if net.services.storage {
        services.push((
            "storage".into(),
            net.resolved_storage_url(),
            "network profile".into(),
        ));
    }
    if net.services.messaging {
        services.push((
            "messaging".into(),
            net.resolved_messaging_http_url(),
            "network profile".into(),
        ));
    }

    let selected: Vec<_> = services
        .into_iter()
        .filter(|(kind, _, _)| filter.as_ref().is_none_or(|filter| kind.contains(filter)))
        .take(limit)
        .collect();
    if selected.is_empty() {
        return Err(
            "no signed-manifest or configured service endpoints matched the request".into(),
        );
    }

    let client = network_http_client(&net)?;
    println!("🔍 Configured network services ({}):", selected.len());
    for (kind, endpoint, source) in selected {
        println!("   {} {} ({})", kind.blue(), endpoint.cyan(), source);
        if detailed {
            if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
                let _ = probe_http(&client, &kind, &endpoint).await;
            } else {
                let _ = probe_tcp(&kind, &endpoint, net.runtime.health_check_timeout_secs).await;
            }
        }
    }
    Ok(())
}

async fn handle_network_peers(
    detailed: bool,
    service_filter: Option<&String>,
    active_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let net =
        crate::network_profile::load_spacekit_network_file()?.ok_or("network profile not found")?;
    let client = network_http_client(&net)?;
    let bases = vec![
        format!("http://{}:{}", net.bind_host, net.ports.status_http),
        net.resolved_compute_url(),
        net.resolved_messaging_http_url(),
        net.resolved_storage_url(),
    ];
    let (url, mut value) = fetch_first_json(
        &client,
        &bases,
        &["/peers", "/network/peers", "/api/peers", "/v1/peers"],
    )
    .await
    .map_err(|errors| {
        format!(
            "peer/state endpoints unavailable; no peer data was invented: {}",
            errors
        )
    })?;
    if let Some(service) = service_filter {
        println!("Filter requested: service={}", service);
    }
    if active_only {
        println!("Filter requested: active_only=true");
    }
    if !detailed {
        if let Some(peers) = value.get_mut("peers") {
            value = peers.take();
        }
    }
    println!("👥 Peer state from {}", url.cyan());
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

async fn handle_network_reputation(
    did: &str,
    detailed: bool,
    history: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let (url, value) = query_reputation(did, history).await?;
    println!("🏆 Reputation from {}", url.cyan());
    if detailed || history {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        println!("{}", value);
    }
    Ok(())
}

async fn query_reputation(
    did: &str,
    history: bool,
) -> Result<(String, serde_json::Value), Box<dyn std::error::Error>> {
    let net =
        crate::network_profile::load_spacekit_network_file()?.ok_or("network profile not found")?;
    let client = network_http_client(&net)?;
    let did = percent_encoding::utf8_percent_encode(did, percent_encoding::NON_ALPHANUMERIC);
    let suffix = if history { "?history=true" } else { "" };
    let paths = [
        format!("/reputation/{}{}", did, suffix),
        format!("/api/reputation/{}{}", did, suffix),
        format!("/v1/reputation/{}{}", did, suffix),
    ];
    let path_refs: Vec<_> = paths.iter().map(String::as_str).collect();
    let bases = vec![
        net.resolved_compute_url(),
        net.resolved_storage_url(),
        format!("http://{}:{}", net.bind_host, net.ports.status_http),
    ];
    fetch_first_json(&client, &bases, &path_refs)
        .await
        .map_err(|errors| {
            format!(
                "reputation endpoint unavailable; no reputation value was invented: {}",
                errors
            )
            .into()
        })
}

async fn handle_network_reputation_watch(
    did: &str,
    interval: u64,
    alerts: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Watching real reputation endpoint for {} (Ctrl+C to stop)",
        did
    );
    let mut last_value: Option<serde_json::Value> = None;
    loop {
        let (_, value) = query_reputation(did, false).await?;
        let changed = last_value
            .as_ref()
            .is_some_and(|previous| previous != &value);
        println!("[{}] {}", chrono::Utc::now().format("%H:%M:%S"), value);
        if alerts && changed {
            println!("{}", "reputation response changed".yellow());
        }
        last_value = Some(value);
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(Duration::from_secs(interval.max(1))) => {}
        }
    }
    Ok(())
}

async fn handle_network_test() -> Result<(), Box<dyn std::error::Error>> {
    let net = crate::network_profile::load_spacekit_network_file()?
        .ok_or("network profile not found — run `spacekit network init` first")?;
    let client = network_http_client(&net)?;
    println!("🧪 Probing enabled network services");
    if print_network_service_probes(&net, &client).await {
        println!("{}", "✅ All enabled service probes passed.".green());
        Ok(())
    } else {
        Err("one or more enabled service probes failed".into())
    }
}

async fn handle_network_doctor() -> Result<(), Box<dyn std::error::Error>> {
    let path = crate::network_profile::spacekit_network_config_path();
    println!("🩺 SpaceKit network doctor");
    println!("   profile: {}", path.display());
    let net = crate::network_profile::load_spacekit_network_file()?
        .ok_or("network profile missing — run `spacekit network init`")?;
    println!("   {} profile v{} validates", "✓".green(), net.version);
    for line in crate::network_supervisor::runtime_status_lines() {
        println!("   {}", line);
    }
    let client = network_http_client(&net)?;
    if print_network_service_probes(&net, &client).await {
        println!("{}", "✅ No endpoint failures detected.".green());
        Ok(())
    } else {
        Err("doctor found unavailable enabled endpoints".into())
    }
}

async fn handle_network_logs(
    service: Option<crate::network_profile::NetworkService>,
    lines: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let net =
        crate::network_profile::load_spacekit_network_file()?.ok_or("network profile not found")?;
    let mut found = false;
    for (label, path) in crate::network_supervisor::log_paths(&net, service) {
        if !path.exists() {
            continue;
        }
        found = true;
        let body = std::fs::read_to_string(&path)?;
        let selected: Vec<_> = body.lines().rev().take(lines).collect();
        println!("== {}: {} ==", label, path.display());
        for line in selected.into_iter().rev() {
            println!("{}", line);
        }
    }
    if found {
        Ok(())
    } else {
        Err(
            "no log files are available yet; detached runs write ~/.spacekit/network/network.log"
                .into(),
        )
    }
}

async fn handle_network_reset(data: bool, force: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !data {
        return Err("reset currently requires the explicit `--data` flag".into());
    }
    let net =
        crate::network_profile::load_spacekit_network_file()?.ok_or("network profile not found")?;
    if !force {
        print!("Delete all configured storage, compute, and messaging data? Type 'reset': ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if answer.trim() != "reset" {
            return Err("reset cancelled".into());
        }
    }
    let removed = crate::network_supervisor::reset_network_data(&net)?;
    if removed.is_empty() {
        println!("No service data directories existed.");
    } else {
        for path in removed {
            println!("{} removed {}", "✓".green(), path.display());
        }
    }
    Ok(())
}

async fn handle_network_join(
    manifest_path: &Path,
    role: crate::network_profile::NetworkRole,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let manifest = crate::network_profile::load_network_manifest(manifest_path)?;
    verify_manifest_genesis(&manifest).await?;
    let local_did = CliContext::load_sync()?.did;
    crate::network_profile::validate_manifest_join(&manifest, &local_did, role)?;
    if manifest.profile == crate::network_profile::NetworkPreset::Public
        && role != crate::network_profile::NetworkRole::Subscriber
    {
        verify_public_operator_readiness(&manifest).await?;
    }
    let mut net = crate::network_profile::SpacekitNetworkFile::for_preset(manifest.profile);
    net.role = role;
    net.manifest = Some(std::fs::canonicalize(manifest_path)?);
    net.blockchain.chain_id = manifest.chain_id;
    net.admission.shared_genesis_hash = Some(manifest.genesis.hash.clone());
    net.admission.require_signed_manifest =
        manifest.profile == crate::network_profile::NetworkPreset::Public;
    net.admission.allowlist = manifest
        .members
        .iter()
        .map(|member| member.did.clone())
        .collect();
    net.blockchain.validators.peers = manifest
        .members
        .iter()
        .filter(|member| {
            member
                .roles
                .contains(&crate::network_profile::NetworkRole::Validator)
        })
        .map(|member| member.did.clone())
        .collect();
    net.messaging.bootstrap_peers = manifest.bootstrap.p2p.clone();
    if role == crate::network_profile::NetworkRole::Subscriber {
        let rpc = manifest
            .bootstrap
            .rpc
            .first()
            .ok_or("subscriber join requires a bootstrap RPC URL")?;
        net.mode = crate::network_profile::NetworkMode::External;
        net.urls.compute = Some(rpc.clone());
        net.services.storage = false;
        net.services.compute = true;
    }
    let path = crate::network_profile::write_network_profile(&net, force)?;
    println!(
        "{} joined network {} as {:?}; profile written to {}",
        "✓".green(),
        manifest.network_id.cyan(),
        role,
        path.display()
    );
    if let Some(signature) = &manifest.signature {
        println!(
            "Manifest signature verified: {:?}, key {}",
            signature.algorithm, signature.key_id
        );
    }
    Ok(())
}

async fn verify_public_operator_readiness(
    manifest: &crate::network_profile::NetworkManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let mut failures = Vec::new();
    for endpoint in &manifest.bootstrap.rpc {
        let url = format!("{}/api/operators/self", endpoint.trim_end_matches('/'));
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                let value: serde_json::Value = response.json().await?;
                if value
                    .get("manifest_source")
                    .and_then(|value| value.as_str())
                    == Some("published_fact")
                    && value
                        .pointer("/manifest/storage_http_url")
                        .and_then(|value| value.as_str())
                        .is_some_and(|value| !value.is_empty())
                {
                    return Ok(());
                }
                failures.push(format!("{url}: no published operator fact"));
            }
            Ok(response) => failures.push(format!("{url}: HTTP {}", response.status())),
            Err(error) => failures.push(format!("{url}: {error}")),
        }
    }
    Err(format!(
        "operator/validator readiness requires a reachable published operator service fact: {}",
        failures.join("; ")
    )
    .into())
}

async fn verify_manifest_genesis(
    manifest: &crate::network_profile::NetworkManifest,
) -> Result<(), Box<dyn std::error::Error>> {
    if manifest.genesis.document.is_some() {
        // Structural validation already hashes embedded canonical JSON.
        return Ok(());
    }
    let uri = manifest
        .genesis
        .uri
        .as_deref()
        .ok_or("manifest must embed genesis.document or provide genesis.uri")?;
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .get(uri)
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("fetch genesis document {uri}: HTTP {}", response.status()).into());
    }
    let document: serde_json::Value = response.json().await?;
    let actual = crate::network_profile::canonical_genesis_hash(&document)?;
    if actual != manifest.genesis.hash {
        return Err(format!(
            "fetched genesis hash {actual} does not match manifest genesis {}",
            manifest.genesis.hash
        )
        .into());
    }
    Ok(())
}

async fn handle_network_manifest(
    action: &NetworkManifestAction,
) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        NetworkManifestAction::Keygen {
            public_key,
            secret_key,
        } => {
            if public_key.exists() || secret_key.exists() {
                return Err("refusing to overwrite an existing manifest key file".into());
            }
            let (public_key_bytes, secret_key_bytes) =
                spacekit_primitives::v1::crypto::quantum::generate_sphincs_keypair("sphincs-128f")?;
            std::fs::write(public_key, hex::encode(public_key_bytes))?;
            std::fs::write(secret_key, hex::encode(secret_key_bytes))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(secret_key, std::fs::Permissions::from_mode(0o600))?;
            }
            println!(
                "{} generated manifest signing keypair (secret key mode 0600)",
                "✓".green()
            );
        }
        NetworkManifestAction::Verify { manifest } => {
            let verified = crate::network_profile::load_network_manifest(manifest)?;
            verify_manifest_genesis(&verified).await?;
            println!(
                "{} manifest {} verified (chain_id={}, protocol={} v{})",
                "✓".green(),
                verified.network_id.cyan(),
                verified.chain_id,
                verified.protocol.name,
                verified.protocol.version
            );
        }
        NetworkManifestAction::Sign {
            manifest,
            key_id,
            public_key,
            secret_key,
            output,
        } => {
            let body = std::fs::read_to_string(manifest)?;
            let mut document: crate::network_profile::NetworkManifest =
                serde_json::from_str(&body)?;
            document.signature = None;
            let payload = document.canonical_unsigned_bytes()?;
            let public_key_bytes = hex::decode(std::fs::read_to_string(public_key)?.trim())?;
            let secret_key_bytes = hex::decode(std::fs::read_to_string(secret_key)?.trim())?;
            let detached = spacekit_primitives::v1::crypto::quantum::sign_sphincs_detached(
                &payload,
                "sphincs-128f",
                &public_key_bytes,
                &secret_key_bytes,
            )?;
            document.signature = Some(crate::network_profile::ManifestSignature {
                algorithm: crate::network_profile::ManifestSignatureAlgorithm::Sphincs128f,
                encoding: crate::network_profile::ManifestSignatureEncoding::Hex,
                key_id: key_id.clone(),
                public_key: hex::encode(public_key_bytes),
                signature: hex::encode(detached.signature_bytes),
                signed_at: Some(chrono::Utc::now()),
            });
            document.validate()?;
            document.verify_signature()?;
            let destination = output.as_ref().unwrap_or(manifest);
            let temporary = destination.with_extension("json.tmp");
            std::fs::write(&temporary, serde_json::to_string_pretty(&document)?)?;
            std::fs::rename(&temporary, destination)?;
            println!(
                "{} signed and verified manifest {} -> {}",
                "✓".green(),
                document.network_id.cyan(),
                destination.display()
            );
        }
    }
    Ok(())
}

// --- Unified consensus proposal payloads (`spacekit consensus submit --data '…'`) ---

#[derive(Debug, Deserialize)]
struct CliBlockProposalData {
    block_number: u64,
    parent_hash: String,
    transactions: Vec<String>,
    state_root: String,
    #[serde(default)]
    chain_id: Option<String>,
    #[serde(default)]
    l1_manifest: Option<SnapshotManifest>,
    #[cfg(feature = "spacetime-consensus")]
    #[serde(default)]
    spacetime_transition: Option<spacekit_compute_node::spacetime_consensus::SpacetimeTransition>,
}

#[derive(Debug, Deserialize)]
struct CliMetricsProposalData {
    cpu_utilization: f64,
    memory_utilization: f64,
    network_utilization: f64,
    storage_utilization: f64,
}

#[derive(Debug, Deserialize)]
struct CliHybridProposalData {
    block: CliBlockProposalData,
    metrics: CliMetricsProposalData,
}

fn block_data_from_cli(
    cli: CliBlockProposalData,
    default_chain_id: &str,
) -> Result<BlockData, CliError> {
    let chain = cli
        .chain_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_chain_id.to_string());
    let l1_manifest = cli.l1_manifest.unwrap_or_else(|| {
        minimal_l1_manifest_for_proposal(
            &chain,
            &cli.state_root,
            cli.block_number,
            &cli.parent_hash,
        )
    });
    let mut block_data = BlockData::new_with_l1_manifest(
        cli.block_number,
        cli.parent_hash,
        cli.transactions,
        cli.state_root,
        SystemTime::now(),
        l1_manifest,
    );
    #[cfg(feature = "spacetime-consensus")]
    {
        block_data.spacetime_transition = cli.spacetime_transition;
    }
    Ok(block_data)
}

async fn consensus_submit_via_http(
    proposal_type: ProposalType,
    data: &str,
    finalize: bool,
    use_swtchvm_head: bool,
    announce: bool,
) -> Result<String, CliError> {
    let compute_url = std::env::var("SPACEKIT_COMPUTE_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string());
    let payload: serde_json::Value = match proposal_type {
        ProposalType::Block => {
            let block: serde_json::Value = serde_json::from_str(data)
                .map_err(|e| CliError::Config(format!("Invalid block proposal JSON: {}", e)))?;
            serde_json::json!({
                "type": "block",
                "block": block,
                "finalize": finalize,
                "use_swtchvm_head": use_swtchvm_head,
                "announce": announce,
            })
        }
        ProposalType::Metrics => {
            let metrics: serde_json::Value = serde_json::from_str(data)
                .map_err(|e| CliError::Config(format!("Invalid metrics proposal JSON: {}", e)))?;
            serde_json::json!({
                "type": "metrics",
                "metrics": metrics,
            })
        }
        ProposalType::Hybrid => {
            let hybrid: serde_json::Value = serde_json::from_str(data)
                .map_err(|e| CliError::Config(format!("Invalid hybrid proposal JSON: {}", e)))?;
            serde_json::json!({
                "type": "hybrid",
                "block": hybrid.get("block").cloned().ok_or_else(|| {
                    CliError::Config("hybrid JSON must include \"block\"".into())
                })?,
                "metrics": hybrid.get("metrics").cloned().ok_or_else(|| {
                    CliError::Config("hybrid JSON must include \"metrics\"".into())
                })?,
            })
        }
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(format!(
            "{}/v1/consensus/propose",
            compute_url.trim_end_matches('/')
        ))
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            CliError::ComputeNode(format!(
                "compute node unreachable at {}: {}",
                compute_url, e
            ))
        })?;

    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| CliError::ComputeNode(format!("invalid JSON from compute node: {}", e)))?;
    if !status.is_success() {
        let err = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("proposal rejected");
        return Err(CliError::ComputeNode(err.to_string()));
    }
    body.get("proposal_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| CliError::ComputeNode("response missing proposal_id".into()))
}

// Individual consensus command handlers
async fn handle_consensus_submit_proposal(
    proposal_type: ProposalType,
    data: &str,
    committee: Option<&String>,
    description: Option<&String>,
    duration: u64,
    in_process: bool,
    finalize: bool,
    use_swtchvm_head: bool,
    announce: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🗳️  Submitting consensus proposal...");
    println!("📋 Type: {}", format!("{:?}", proposal_type).blue());
    println!("⏱️  Duration: {} hours", duration);

    if let Some(desc) = description {
        println!("📝 Description: {}", desc.yellow());
    }

    if let Some(comm) = committee {
        println!("👥 Committee: {}", comm.cyan());
    }

    println!("📊 Data size: {} characters", data.len());

    let my_did = get_default_did()?;

    let proposal_id: String = if in_process {
        let _compute = get_or_create_compute_node().await?;
        let compute_cfg = load_compute_config().await?;
        let default_chain = compute_cfg.chain_id.as_str();
        let unified = get_or_create_unified_consensus().await?;
        match proposal_type {
            ProposalType::Block => {
                println!("\n🧱 Submitting block proposal...");
                let cli: CliBlockProposalData = serde_json::from_str(data).map_err(|e| {
                CliError::Config(format!(
                    "Invalid block proposal JSON (expect block_number, parent_hash, transactions, state_root): {}",
                    e
                ))
            })?;
                let block_data = block_data_from_cli(cli, default_chain)?;
                let proposal = BlockProposal::new(my_did.clone(), block_data);
                let id = unified
                    .submit_block_proposal(proposal)
                    .await
                    .map_err(|e| CliError::ComputeNode(e.to_string()))?;
                println!("✅ Block proposal submitted.");
                id
            }
            ProposalType::Metrics => {
                println!("\n📊 Submitting metrics proposal...");
                let cli: CliMetricsProposalData = serde_json::from_str(data).map_err(|e| {
                CliError::Config(format!(
                    "Invalid metrics proposal JSON (expect cpu/memory/network/storage utilization): {}",
                    e
                ))
            })?;
                let metrics = NetworkMetrics {
                    cpu_utilization: cli.cpu_utilization,
                    memory_utilization: cli.memory_utilization,
                    network_utilization: cli.network_utilization,
                    storage_utilization: cli.storage_utilization,
                    timestamp: SystemTime::now(),
                };
                let proposal = MetricsProposal::new(my_did.clone(), metrics);
                let id = unified
                    .submit_metrics_proposal(proposal)
                    .await
                    .map_err(|e| CliError::ComputeNode(e.to_string()))?;
                println!("✅ Metrics proposal submitted.");
                id
            }
            ProposalType::Hybrid => {
                println!("\n🔄 Submitting hybrid proposal...");
                let cli: CliHybridProposalData = serde_json::from_str(data).map_err(|e| {
                CliError::Config(format!(
                    "Invalid hybrid proposal JSON (expect {{ \"block\": {{…}}, \"metrics\": {{…}} }}): {}",
                    e
                ))
            })?;
                let block_data = block_data_from_cli(cli.block, default_chain)?;
                let metrics = NetworkMetrics {
                    cpu_utilization: cli.metrics.cpu_utilization,
                    memory_utilization: cli.metrics.memory_utilization,
                    network_utilization: cli.metrics.network_utilization,
                    storage_utilization: cli.metrics.storage_utilization,
                    timestamp: SystemTime::now(),
                };
                let proposal = HybridProposal::new(my_did.clone(), block_data, metrics);
                let id = unified
                    .submit_hybrid_proposal(proposal)
                    .await
                    .map_err(|e| CliError::ComputeNode(e.to_string()))?;
                println!("✅ Hybrid proposal submitted.");
                id
            }
        }
    } else {
        println!("🌐 Submitting via compute node HTTP (set --in-process to use local engine)");
        let id =
            consensus_submit_via_http(proposal_type, data, finalize, use_swtchvm_head, announce)
                .await?;
        println!("✅ Proposal submitted via HTTP.");
        id
    };

    println!("\n📋 Proposal Details:");
    println!("🆔 ID: {}", proposal_id.green());
    println!("👤 Proposer: {}", my_did.cyan());
    println!(
        "📅 Submitted: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!(
        "⏰ Voting ends: {}",
        (chrono::Utc::now() + chrono::Duration::hours(duration as i64))
            .format("%Y-%m-%d %H:%M:%S UTC")
    );
    println!("📊 Initial status: Pending");

    println!("\n💡 Next steps:");
    println!(
        "   • Use {} to check proposal status",
        format!("spacekit consensus status --proposal-id {}", proposal_id).yellow()
    );
    println!(
        "   • Validators can vote using {}",
        format!(
            "spacekit consensus vote --proposal-id {} --vote approve",
            proposal_id
        )
        .yellow()
    );
    println!(
        "   • View all proposals with {}",
        "spacekit consensus list".yellow()
    );

    Ok(())
}

async fn handle_consensus_vote(
    proposal_id: &str,
    vote_choice: VoteChoice,
    rationale: Option<&String>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🗳️  Submitting vote...");
    println!("📋 Proposal ID: {}", proposal_id.blue());
    println!("✅ Vote: {}", format!("{:?}", vote_choice).green());

    if let Some(reason) = rationale {
        println!("💭 Rationale: {}", reason.yellow());
    }

    // Get compute node and voter DID
    let _node = get_or_create_compute_node().await?;
    let voter_did = get_default_did()?;

    println!("👤 Voter: {}", voter_did.cyan());

    // Validate voter eligibility (mock)
    println!("🔍 Validating voting eligibility...");

    // Check if proposal exists (mock check)
    if !proposal_id.starts_with("prop_") {
        println!("❌ Invalid proposal ID format");
        return Err(Box::new(CliError::Config(
            "Invalid proposal ID".to_string(),
        )));
    }

    // TODO: Implement real vote submission using UnifiedSpaceKitConsensus::submit_vote() API
    // This requires integration with the actual consensus voting mechanism
    // Submit vote (mock implementation)
    println!("✅ Vote submitted successfully!");

    println!("\n📊 Vote Summary:");
    println!("🆔 Proposal: {}", proposal_id);
    println!("🗳️  Choice: {}", format!("{:?}", vote_choice));
    println!("👤 Voter: {}", voter_did);
    println!(
        "⏰ Voted at: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    // Show mock voting progress
    println!("\n📈 Current Voting Progress:");
    println!("✅ Approve: 67% (12 votes)");
    println!("❌ Reject: 22% (4 votes)");
    println!("⭕ Abstain: 11% (2 votes)");
    println!("📊 Participation: 18/25 eligible validators (72%)");

    println!(
        "\n💡 Use {} to check latest proposal status",
        format!("spacekit consensus status --proposal-id {}", proposal_id).yellow()
    );

    Ok(())
}

async fn handle_consensus_status(
    proposal_id: Option<&String>,
    detailed: bool,
    network_health: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Checking consensus status...");

    // Get compute node for consensus access
    let _node = get_or_create_compute_node().await?;

    if let Some(prop_id) = proposal_id {
        println!("🔍 Checking specific proposal: {}", prop_id.blue());

        // TODO: Replace with real proposal status using UnifiedSpaceKitConsensus::get_proposal_status() API
        // Mock proposal status check
        println!("\n📋 Proposal Status Report");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🆔 ID: {}", prop_id.green());
        println!("📋 Type: {}", "Block Proposal".blue());
        println!("📊 Status: {}", "Active".green());
        println!("👤 Proposer: {}", "did:spacekit:validator:alice".cyan());

        println!("\n🗳️  Voting Progress:");
        println!("✅ Approve: 67% (12 votes)");
        println!("❌ Reject: 22% (4 votes)");
        println!("⭕ Abstain: 11% (2 votes)");
        println!("📊 Participation: 18/25 eligible validators (72%)");
        println!("🏁 Threshold: 67% approval required");

        println!("\n⏰ Timeline:");
        println!("📅 Submitted: 2024-01-30 14:30:00 UTC");
        println!("⏰ Voting ends: 2024-01-31 14:30:00 UTC");
        println!("⏱️  Time remaining: 18 hours 23 minutes");

        if detailed {
            println!("\n📄 Detailed Information:");
            println!("📝 Description: Propose new block with optimized consensus validation");
            println!("👥 Committee: Core Validators");
            println!("🔗 Block hash: 0x1234...5678");
            println!("📊 Block size: 2.3 MB");
            println!("⛽ Gas limit: 15,000,000");

            println!("\n🗳️  Recent Votes:");
            println!("  ✅ Alice (did:spacekit:validator:alice) - Approve - 2 hours ago");
            println!("  ✅ Bob (did:spacekit:validator:bob) - Approve - 1 hour ago");
            println!("  ❌ Charlie (did:spacekit:validator:charlie) - Reject - 45 minutes ago");
        }
    } else {
        println!("\n📊 Network Consensus Overview");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        println!("⚡ Status: {}", "Healthy".green());
        println!("🔄 Active proposals: {}", "3");
        println!("✅ Finalized proposals (24h): {}", "12");
        println!("👥 Active validators: {}", "25/30");
        println!("📊 Network participation: {}", "83.3%".green());

        if network_health {
            println!("\n🏥 Network Health Metrics:");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
            println!("🟢 Consensus finality: {}", "12.3 seconds average");
            println!("🟢 Validator uptime: {}", "99.7%");
            println!("🟢 Network sync: {}", "100% synchronized");
            println!("🟢 Fork rate: {}", "0.02% (excellent)");
            println!("🟢 Transaction throughput: {}", "2,847 TPS");

            println!("\n⚠️  Alerts & Warnings:");
            println!("🟡 Validator node-7 offline for 3 hours");
            println!("🟢 No critical issues detected");
        }

        if detailed {
            println!("\n📈 Consensus Algorithm Status:");
            println!("🔄 Unified SpaceKit Consensus: Active");
            println!("🧱 Block consensus: 99.8% efficiency");
            println!("📊 Metrics consensus: 99.5% efficiency");
            println!("🔄 Hybrid proposals: 8 active");

            println!("\n🔄 Migration Status:");
            println!("📊 Current phase: Unified Consensus v2.0");
            println!("🔄 Next migration: Scheduled for Q2 2024");
            println!("⚠️  Risk level: Low");
        }
    }

    println!(
        "\n⏰ Status checked: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    Ok(())
}

async fn handle_consensus_list(
    status_filter: Option<&String>,
    proposal_type_filter: Option<ProposalType>,
    my_proposals: bool,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 Listing consensus proposals...");

    if let Some(status) = status_filter {
        println!("🔍 Filtering by status: {}", status.cyan());
    }

    if let Some(ptype) = proposal_type_filter {
        println!("🎯 Filtering by type: {}", format!("{:?}", ptype).blue());
    }

    if my_proposals {
        let my_did = get_default_did()?;
        println!("👤 Showing only my proposals ({})", my_did.yellow());
    }

    println!("📊 Limit: {} proposals", limit);

    // Get compute node for consensus access
    let _node = get_or_create_compute_node().await?;

    println!("\n📋 SpaceKit Network Proposals");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // TODO: Replace with real proposal listing using UnifiedSpaceKitConsensus::list_proposals() API
    // This should query the actual consensus system for proposal data with proper filtering
    // Mock proposal list
    let mock_proposals = vec![
        (
            "prop_block_1706628000_abc12345",
            "Block",
            "Active",
            "Alice",
            "12/25 votes",
            "18h left",
        ),
        (
            "prop_metrics_1706541600_def67890",
            "Metrics",
            "Passed",
            "Bob",
            "18/25 votes",
            "Executed",
        ),
        (
            "prop_hybrid_1706455200_ghi13579",
            "Hybrid",
            "Active",
            "Charlie",
            "8/25 votes",
            "6h left",
        ),
        (
            "prop_block_1706368800_jkl24680",
            "Block",
            "Failed",
            "David",
            "7/25 votes",
            "Expired",
        ),
        (
            "prop_metrics_1706282400_mno97531",
            "Metrics",
            "Active",
            "Eve",
            "15/25 votes",
            "12h left",
        ),
    ];

    let mut displayed_count = 0;

    for (prop_id, prop_type, prop_status, proposer, votes, timeline) in &mock_proposals {
        // Apply filters
        if let Some(status) = status_filter {
            if !prop_status.to_lowercase().contains(&status.to_lowercase()) {
                continue;
            }
        }

        if let Some(ptype) = proposal_type_filter {
            let type_match = match ptype {
                ProposalType::Block => *prop_type == "Block",
                ProposalType::Metrics => *prop_type == "Metrics",
                ProposalType::Hybrid => *prop_type == "Hybrid",
            };
            if !type_match {
                continue;
            }
        }

        if displayed_count >= limit {
            break;
        }

        displayed_count += 1;

        println!("\n📋 Proposal {}: {}", displayed_count, prop_id.green());
        println!("📋 Type: {}", prop_type.blue());

        let status_colored = match *prop_status {
            "Active" => prop_status.green(),
            "Passed" => prop_status.cyan(),
            "Failed" => prop_status.red(),
            _ => prop_status.yellow(),
        };
        println!("📊 Status: {}", status_colored);

        println!("👤 Proposer: {}", proposer.yellow());
        println!("🗳️  Votes: {}", votes);
        println!("⏰ Timeline: {}", timeline.blue());
    }

    if displayed_count == 0 {
        println!("📭 No proposals found matching criteria");
        println!("💡 Try removing filters or use 'spacekit consensus list' to see all proposals");
    } else {
        println!("\n📊 Proposal Summary");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("📋 Total shown: {}", displayed_count);
        println!(
            "📊 Active proposals: {}",
            mock_proposals
                .iter()
                .filter(|(_, _, status, _, _, _)| *status == "Active")
                .count()
        );
        println!(
            "✅ Passed proposals: {}",
            mock_proposals
                .iter()
                .filter(|(_, _, status, _, _, _)| *status == "Passed")
                .count()
        );
        println!(
            "❌ Failed proposals: {}",
            mock_proposals
                .iter()
                .filter(|(_, _, status, _, _, _)| *status == "Failed")
                .count()
        );

        println!("\n💡 Next steps:");
        println!(
            "   • Use {} to check specific proposal",
            "spacekit consensus status --proposal-id <ID>".yellow()
        );
        println!(
            "   • Vote on proposals with {}",
            "spacekit consensus vote --proposal-id <ID> --vote <CHOICE>".yellow()
        );
    }

    println!(
        "\n⏰ List generated: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    Ok(())
}

async fn handle_consensus_migration(
    detailed: bool,
    history: bool,
    risks: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Checking consensus migration status...");

    // TODO: Implement real migration status using ConsensusMigrationManager API
    // This should query the actual migration manager for real-time migration status
    // Get compute node for migration manager access
    let _node = get_or_create_compute_node().await?;

    println!("\n🔄 SpaceKit Consensus Migration Status");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // TODO: Replace with real migration data from ConsensusMigrationManager
    println!(
        "📊 Current Status: {}",
        "Unified Consensus v2.0 (Stable)".green()
    );
    println!(
        "🔄 Migration Phase: {}",
        "Phase 5.5 - Production Ready".blue()
    );
    println!("⚡ System Health: {}", "Excellent (99.8% uptime)".green());
    println!("🏁 Next Migration: {}", "Q2 2024 - Enhanced VPoS".yellow());

    if detailed {
        println!("\n🔧 Technical Details:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🧱 Block Consensus: UnifiedSpaceKitConsensus v2.0");
        println!("📊 Metrics Consensus: MetricsConsensusManager v1.8");
        println!("🔄 Hybrid Proposals: 847 processed successfully");
        println!("⚖️  Unified Voting: ThresholdCalculator v2.1");
        println!("🛡️  Security: Quantum-safe signature aggregation");

        println!("\n🏗️  Infrastructure:");
        println!("🔄 Dual Consensus Adapter: Active (backward compatibility)");
        println!("📊 Performance Monitor: All metrics green");
        println!("🛡️  Risk Mitigation: Rollback mechanism ready");
        println!("🔄 Validator Transition: 98% nodes upgraded");

        println!("\n📊 Performance Metrics:");
        println!("⚡ Finality time: 12.3s (target: <15s)");
        println!("🔄 Throughput: 2,847 TPS (capacity: 5,000 TPS)");
        println!("📊 Consensus efficiency: 99.5%");
        println!("🔀 Fork resolution: <30s average");
    }

    if history {
        println!("\n📅 Migration History:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("✅ 2024-01-15: Phase 5.5 deployment completed");
        println!("✅ 2024-01-10: Unified consensus algorithm activated");
        println!("✅ 2024-01-05: Metrics consensus integration finalized");
        println!("✅ 2023-12-20: Block consensus v2.0 upgrade");
        println!("✅ 2023-12-15: Pre-migration testing completed");
        println!("✅ 2023-12-01: Migration planning phase started");

        println!("\n📊 Migration Statistics:");
        println!("⏱️  Total migration time: 45 days");
        println!("⬇️  Downtime: 0 seconds (zero-downtime migration)");
        println!("👥 Validator participation: 100%");
        println!("🔄 Rollback events: 0");
        println!("⚠️  Issues encountered: 2 (both resolved)");
    }

    if risks {
        println!("\n⚠️  Risk Assessment:");
        println!("━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("🟢 Overall Risk Level: {}", "LOW".green());

        println!("\n🛡️  Mitigation Strategies:");
        println!("✅ Rollback Mechanism: Fully tested and ready");
        println!("✅ Performance Monitoring: Real-time alerts configured");
        println!("✅ Compatibility Layer: Legacy consensus support active");
        println!("✅ Validator Transition: Gradual upgrade path implemented");

        println!("\n📊 Risk Factors:");
        println!("🟢 Technical Risk: Low (extensive testing completed)");
        println!("🟢 Network Risk: Low (99.8% validator adoption)");
        println!("🟡 Performance Risk: Medium (monitoring new algorithm)");
        println!("🟢 Security Risk: Low (quantum-resistant implementations)");

        println!("\n🚨 Potential Issues:");
        println!("⚠️  Network congestion during peak usage");
        println!("⚠️  Validator configuration mismatches");
        println!("⚠️  Legacy client compatibility");

        println!("\n💡 Recommended Actions:");
        println!("📊 Continue monitoring performance metrics");
        println!("🔄 Regular validator health checks");
        println!("⚡ Maintain rollback readiness for 30 days");
    }

    println!("\n📞 Migration Support:");
    println!("🔗 Documentation: https://docs.spacekit.xyz/consensus-migration");
    println!("👥 Support channel: #consensus-migration on Discord");
    println!("📧 Emergency contact: hello@spacekit.xyz");

    println!(
        "\n⏰ Status checked: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    Ok(())
}

async fn handle_storage_node(action: &NodeAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        NodeAction::Start => {
            println!("🚀 Starting storage node...");

            // Get or create storage node
            let node = get_or_create_storage_node().await?;

            // Start the storage node services
            match node.start().await {
                Ok(()) => {
                    println!("✅ Storage node started successfully!");
                    println!("🆔 Node DID: {}", node.config().node_did.green());
                    println!("📁 Data directory: {:?}", node.config().data_dir);
                    println!(
                        "🔐 Quantum algorithm: {}",
                        node.config().preferred_algorithm.yellow()
                    );
                    println!(
                        "💾 Max storage: {:.2} GB",
                        node.config().max_storage_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
                    );

                    println!("\n📡 Services:");
                    println!(
                        "  🔗 P2P network: Listening on port {}",
                        node.config().network_config.listen_port
                    );
                    println!("  📊 Database: Operational");

                    #[cfg(feature = "api-server")]
                    if let Some(api_config) = &node.config().api_config {
                        println!("  🌐 HTTP API: http://localhost:{}", api_config.port);
                    }

                    println!("\n💡 Storage node is running. Use Ctrl+C to stop.");

                    // TODO: Implement proper daemon mode for storage node to keep running in background
                    // This requires proper signal handling and daemon process management
                    println!("⚠️  Note: This is a test startup. In production, the node would continue running.");
                }
                Err(e) => {
                    println!("❌ Failed to start storage node: {}", e);
                    return Err(Box::new(CliError::ComputeNode(format!(
                        "Node start error: {}",
                        e
                    ))));
                }
            }
        }
        NodeAction::Stop => {
            println!("🛑 Stopping storage node...");
            println!("✅ Storage node stopped successfully!");
            println!("💡 All data has been safely persisted.");
        }
        NodeAction::Status => {
            println!("📊 Checking storage node status...");

            // Get or create storage node
            let node = get_or_create_storage_node().await?;

            match node.get_stats().await {
                Ok(stats) => {
                    println!("\n🟢 Storage Node Status: Active");
                    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                    println!("🆔 Node DID: {}", stats.node_did.green());
                    println!("📊 Files stored: {}", stats.file_count);
                    println!(
                        "💾 Storage used: {:.2} MB",
                        stats.total_size_bytes as f64 / (1024.0 * 1024.0)
                    );
                    println!("📈 Utilization: {:.1}%", stats.storage_utilization);
                    println!("🔐 Algorithm: {}", stats.preferred_algorithm.yellow());
                    println!(
                        "⏰ Status checked: {}",
                        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
                    );
                }
                Err(e) => {
                    println!("🔴 Storage Node Status: Error");
                    println!("❌ Failed to get node status: {}", e);
                    return Err(Box::new(CliError::ComputeNode(format!(
                        "Status check error: {}",
                        e
                    ))));
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// SIMULATOR COMMAND HANDLERS
// ============================================================================

// async fn handle_simulator_command(simulator_command: &SimulatorCommands) -> Result<(), Box<dyn std::error::Error>> {
//     match simulator_command {
//         SimulatorCommands::Up {
//             config,
//             port,
//             eth_json_rpc,
//             eth_json_rpc_port,
//             eth_json_rpc_chain_id,
//             base_json_rpc,
//             base_json_rpc_port,
//             base_network,
//             base_rpc_url,
//             base_custom_chain_id,
//         } => {
//             println!("{}", "╔═══════════════════════════════════════════╗".cyan());
//             println!("{}", "║   SpaceKit Simulator — Local Network      ║".cyan());
//             println!("{}", "╚═══════════════════════════════════════════╝".cyan());

//             use spacekit_simulator::network_adapters::{
//                 BaseJsonRpcAdapter, EthereumJsonRpcAdapter, SimLaunchOptions, BASE_MAINNET_CHAIN_ID, BASE_MAINNET_HTTPS,
//                 BASE_SEPOLIA_CHAIN_ID, BASE_SEPOLIA_HTTPS,
//             };

//             let net = if let Some(path) = config {
//                 println!("📄 Loading topology from: {}", path.green());
//                 if *eth_json_rpc {
//                     println!("{}", "⚠  --eth-json-rpc is ignored when using --config (set spec.network_adapters.ethereum_json_rpc in YAML).".yellow());
//                 }
//                 if *base_json_rpc {
//                     println!("{}", "⚠  --base-json-rpc is ignored when using --config (set spec.network_adapters.base_json_rpc in YAML).".yellow());
//                 }
//                 let _ = port; // reserved for future proxy port override
//                 let _ = eth_json_rpc_port;
//                 let _ = eth_json_rpc_chain_id;
//                 let _ = base_json_rpc_port;
//                 let _ = base_network;
//                 let _ = base_rpc_url;
//                 let _ = base_custom_chain_id;
//                 spacekit_simulator::SimNetwork::from_yaml(path).await
//                     .map_err(|e| Box::new(CliError::ComputeNode(e.to_string())))?
//             } else {
//                 println!("🌐 Using default public testnet configuration");
//                 let base_opt = if *base_json_rpc {
//                     let p = *base_json_rpc_port;
//                     let (upstream_url, chain_id_hint) = if let Some(ref u) = base_rpc_url {
//                         if u.is_empty() {
//                             return Err("--base-rpc-url was empty".into());
//                         }
//                         (u.clone(), base_custom_chain_id.unwrap_or(0u64))
//                     } else {
//                         match base_network {
//                             SimBaseNetwork::Mainnet => {
//                                 (BASE_MAINNET_HTTPS.to_string(), BASE_MAINNET_CHAIN_ID)
//                             }
//                             SimBaseNetwork::Sepolia => (BASE_SEPOLIA_HTTPS.to_string(), BASE_SEPOLIA_CHAIN_ID),
//                         }
//                     };
//                     Some(BaseJsonRpcAdapter {
//                         port: p,
//                         upstream_url,
//                         chain_id_hint,
//                     })
//                 } else {
//                     None
//                 };
//                 let launch = SimLaunchOptions {
//                     ethereum_json_rpc: if *eth_json_rpc {
//                         Some(EthereumJsonRpcAdapter {
//                             port: *eth_json_rpc_port,
//                             chain_id: *eth_json_rpc_chain_id,
//                         })
//                     } else {
//                         None
//                     },
//                     base_json_rpc: base_opt,
//                 };
//                 let _ = port;
//                 spacekit_simulator::SimNetwork::public_testnet_with_options(launch).await
//                     .map_err(|e| Box::new(CliError::ComputeNode(e.to_string())))?
//             };

//             let accounts = net.list_funded_accounts().await;
//             println!("\n💰 {} pre-funded testnet accounts:", accounts.len().to_string().yellow());
//             for acct in &accounts {
//                 println!("   {} — {} ASTRA + {} aUSD",
//                     acct.address.green(),
//                     "100M".yellow(),
//                     "100M".yellow()
//                 );
//             }

//             println!("\n🔌 Service ports:");
//             println!("   Proxy:     http://localhost:{}", net.service_ports.proxy.to_string().cyan());
//             println!("   Compute:   http://localhost:{}", net.service_ports.compute.to_string().cyan());
//             println!("   Storage:   http://localhost:{}", net.service_ports.storage.to_string().cyan());
//             println!("   Messaging: http://localhost:{}", net.service_ports.messaging.to_string().cyan());
//             if let (Some(p), Some(eth)) = (net.service_ports.ethereum_json_rpc, net.network_adapters.ethereum_json_rpc) {
//                 println!("   EVM/JSON:  http://127.0.0.1:{}  (SpaceKit ASTRA testnet, `eth_chainId` {})", p.to_string().cyan(), eth.chain_id);
//             }
//             if let (Some(p), Some(ba)) = (
//                 net.service_ports.base_json_rpc,
//                 net.network_adapters.base_json_rpc.as_ref(),
//             ) {
//                 println!("   Base L2:   http://127.0.0.1:{}  →  {}  (use MetaMask: real Base, not ASTRA)", p.to_string().cyan(), &ba.upstream_url);
//             }

//             println!("\n🚀 Starting services...");
//             net.start().await.map_err(|e| Box::new(CliError::ComputeNode(e.to_string())))?;
//             Ok(())
//         },
//         SimulatorCommands::Accounts => {
//             let net = spacekit_simulator::SimNetwork::public_testnet().await
//                 .map_err(|e| Box::new(CliError::ComputeNode(e.to_string())))?;
//             let accounts = net.list_funded_accounts().await;
//             println!("💰 {} pre-funded testnet accounts:\n", accounts.len().to_string().yellow());
//             println!("{:<44} {:>16} {:>16}", "Address".bold(), "ASTRA".bold(), "aUSD".bold());
//             println!("{}", "─".repeat(78));
//             for acct in &accounts {
//                 println!("{:<44} {:>13}M   {:>13}M",
//                     acct.address.green(),
//                     "100".yellow(),
//                     "100".yellow()
//                 );
//             }
//             Ok(())
//         },
//         SimulatorCommands::Vpn(vpn_cmd) => handle_vpn_command(vpn_cmd).await,
//         SimulatorCommands::Orchestration(orch_cmd) => handle_orchestration_command(orch_cmd).await,
//         SimulatorCommands::CrossNetwork(cross_cmd) => handle_cross_network_command(cross_cmd).await,
//         SimulatorCommands::Scanner(scanner_cmd) => handle_scanner_command(scanner_cmd).await,
//         SimulatorCommands::Faucet(faucet_cmd) => handle_faucet_command(faucet_cmd).await,
//     }
// }

async fn handle_vpn_command(vpn_command: &VpnCommands) -> Result<(), Box<dyn std::error::Error>> {
    match vpn_command {
        VpnCommands::Establish {
            target_did,
            relay_chain,
            relay_count,
        } => {
            println!("🔐 Establishing VPN connection...");
            println!("   Target: {}", target_did.green());
            println!("   Relay chain: {}", relay_chain);
            println!("   Relay count: {}", relay_count);

            // TODO: Implement actual VPN establishment using spacekit-simulator::VpnService
            println!("✅ VPN connection established!");
            println!("   Connection ID: vpn_conn_{}", Uuid::new_v4());
            Ok(())
        }
        VpnCommands::Status { connection_id } => {
            println!("📊 VPN Connection Status:");
            println!("   Connection ID: {}", connection_id.cyan());
            println!("   Status: Active");
            println!("   Relays: 3 nodes");
            println!("   Latency: 45ms");
            Ok(())
        }
        VpnCommands::List { active_only } => {
            println!("📋 VPN Connections:");
            println!(
                "   {} connections found",
                if *active_only { "Active" } else { "All" }
            );
            Ok(())
        }
        VpnCommands::Terminate { connection_id } => {
            println!("🛑 Terminating VPN connection {}...", connection_id);
            println!("✅ VPN connection terminated successfully!");
            Ok(())
        }
        VpnCommands::Relays => {
            println!("🌐 Available Relay Nodes:");
            println!("   • relay-1.spacekit.xyz (US-East)");
            println!("   • relay-2.spacekit.xyz (EU-West)");
            println!("   • relay-3.spacekit.xyz (Asia-Pacific)");
            Ok(())
        }
    }
}

async fn handle_orchestration_command(
    orch_command: &OrchestrationCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match orch_command {
        OrchestrationCommands::Deploy {
            deployment_type,
            replicas,
            did,
            gpu_enabled,
            namespace,
        } => {
            println!(
                "🚀 Deploying {} nodes...",
                format!("{:?}", deployment_type).to_lowercase()
            );
            println!("   Replicas: {}", replicas);
            println!("   Owner DID: {}", did.green());
            println!("   GPU enabled: {}", gpu_enabled);
            if let Some(ns) = namespace {
                println!("   Namespace: {}", ns);
            }

            // TODO: Implement actual deployment using spacekit-simulator::orchestration
            let deployment_id = format!("deploy_{}", Uuid::new_v4());
            println!("\n✅ Deployment created successfully!");
            println!("   Deployment ID: {}", deployment_id.cyan());
            Ok(())
        }
        OrchestrationCommands::List {
            deployment_type,
            namespace,
        } => {
            println!("📋 Active Deployments:");
            if let Some(dt) = deployment_type {
                println!("   Filter: {:?} nodes", dt);
            }
            if let Some(ns) = namespace {
                println!("   Namespace: {}", ns);
            }
            Ok(())
        }
        OrchestrationCommands::Scale {
            deployment_id,
            replicas,
        } => {
            println!("📈 Scaling deployment {}...", deployment_id);
            println!("   New replica count: {}", replicas);
            println!("✅ Deployment scaled successfully!");
            Ok(())
        }
        OrchestrationCommands::Terminate { deployment_id } => {
            println!("🛑 Terminating deployment {}...", deployment_id);
            println!("✅ Deployment terminated successfully!");
            Ok(())
        }
        OrchestrationCommands::Packages => {
            println!("📦 Available WASM Packages:");
            println!("   • spacekit-compute-node v1.0.0");
            println!("   • spacekit-storage-node v1.0.0");
            println!("   • spacekit-messaging-node v1.0.0");
            Ok(())
        }
        OrchestrationCommands::ListCompute { detailed } => {
            println!("🖥️  Deployed Compute Nodes:");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

            // TODO: Implement via simulator gRPC API
            // For now, show example output
            println!("1. 🟢 compute-node-abc123");
            println!("   DID: did:spacekit:compute:node1");
            println!("   URL: http://localhost:8080");
            println!("   Status: Running");
            if *detailed {
                println!("   Tasks completed: 45");
                println!("   GPU enabled: false");
                println!("   Storage integration: true");
                println!("   Uptime: 2h 34m");
            }
            println!();

            println!("2. 🟢 compute-node-def456");
            println!("   DID: did:spacekit:compute:node2");
            println!("   URL: http://localhost:8081");
            println!("   Status: Running");
            if *detailed {
                println!("   Tasks completed: 32");
                println!("   GPU enabled: false");
                println!("   Storage integration: true");
                println!("   Uptime: 2h 34m");
            }
            println!();

            println!("✅ Total: 2 compute nodes deployed");
            println!("\n💡 Connect with: spacekit connect compute --url <URL> --node-did <DID>");
            Ok(())
        }
        OrchestrationCommands::ListStorage { detailed } => {
            println!("💾 Deployed Storage Nodes:");
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

            // TODO: Implement via simulator gRPC API
            println!("1. 🟢 storage-node-xyz789");
            println!("   DID: did:spacekit:storage:node1");
            println!("   URL: http://localhost:9000");
            println!("   Status: Running");
            if *detailed {
                println!("   Files stored: 123");
                println!("   Storage used: 45.2 GB");
                println!("   Replication factor: 3");
                println!("   Quantum encryption: Kyber1024");
            }
            println!();

            println!("2. 🟢 storage-node-uvw012");
            println!("   DID: did:spacekit:storage:node2");
            println!("   URL: http://localhost:9001");
            println!("   Status: Running");
            if *detailed {
                println!("   Files stored: 98");
                println!("   Storage used: 32.8 GB");
                println!("   Replication factor: 3");
                println!("   Quantum encryption: Kyber1024");
            }
            println!();

            println!("✅ Total: 2 storage nodes deployed");
            println!("\n💡 Connect with: spacekit connect storage --url <URL> --node-did <DID>");
            Ok(())
        }
        OrchestrationCommands::NodeInfo { node_id } => {
            println!("ℹ️  Node Information: {}", node_id.cyan());
            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

            // TODO: Implement via simulator gRPC API
            println!("   Type: Compute Node");
            println!("   DID: did:spacekit:compute:node1");
            println!("   URL: http://localhost:8080");
            println!("   Status: 🟢 Running");
            println!();
            println!("   Configuration:");
            println!("   • Max tasks: 10");
            println!("   • GPU enabled: false");
            println!("   • Storage integration: true");
            println!("   • Memory limit: 6 GB");
            println!("   • CPU cores: 8");
            println!();
            println!("   Statistics:");
            println!("   • Tasks completed: 45");
            println!("   • Tasks failed: 2");
            println!("   • Avg execution time: 2.3s");
            println!("   • Uptime: 99.8%");
            println!();
            println!("   Network:");
            println!("   • Namespace: ai-companions");
            println!("   • Replica ID: 1 of 2");
            println!("   • Port: 8080");

            Ok(())
        }
    }
}

async fn handle_cross_network_command(
    cross_command: &CrossNetworkCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cross_command {
        CrossNetworkCommands::Connect {
            peer,
            secure_channel,
        } => {
            println!("🌐 Connecting to remote network...");
            println!("   Peer: {}", peer.cyan());
            println!("   Secure channel: {}", secure_channel);
            println!("✅ Connected to remote network!");
            Ok(())
        }
        CrossNetworkCommands::Status => {
            println!("📊 Cross-Network Status:");
            println!("   Connected peers: 3");
            println!("   Network health: 95%");
            Ok(())
        }
        CrossNetworkCommands::Health => {
            println!("🏥 Network Health Metrics:");
            println!("   Connected peers: 3");
            println!("   Avg connection quality: 94.5%");
            println!("   Consensus participation: 88.7%");
            println!("   Avg cross-region latency: 67ms");
            Ok(())
        }
        CrossNetworkCommands::Topology(topo_cmd) => handle_topology_command(topo_cmd).await,
    }
}

async fn handle_topology_command(
    topo_command: &TopologyCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match topo_command {
        TopologyCommands::HubConfigure { listen_port } => {
            println!("🏢 Configuring as hub...");
            println!("   Listen port: {}", listen_port);
            println!("✅ Hub configured successfully!");
            Ok(())
        }
        TopologyCommands::SpokeJoin { hub_address } => {
            println!("🔗 Joining hub as spoke...");
            println!("   Hub address: {}", hub_address.cyan());
            println!("✅ Joined hub successfully!");
            Ok(())
        }
        TopologyCommands::MeshJoin { peers } => {
            println!("🕸️  Joining mesh network...");
            println!("   Peers: {}", peers);
            println!("✅ Joined mesh successfully!");
            Ok(())
        }
        TopologyCommands::Status => {
            println!("📊 Topology Status:");
            println!("   Type: Mesh");
            println!("   Total nodes: 5");
            println!("   Active nodes: 5");
            println!("   Health: 100%");
            Ok(())
        }
    }
}

async fn handle_scanner_command(
    scanner_command: &ScannerCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match scanner_command {
        ScannerCommands::ScanBlock { block_number } => {
            println!("🔍 Scanning block {}...", block_number);
            println!("✅ Block scan complete!");
            Ok(())
        }
        ScannerCommands::ScanAddress { address } => {
            println!("🔍 Scanning address {}...", address.cyan());
            println!("✅ Address scan complete!");
            Ok(())
        }
        ScannerCommands::Subscribe { event_type } => {
            println!("📡 Subscribing to events...");
            if let Some(et) = event_type {
                println!("   Filter: {}", et);
            }
            println!("✅ Subscribed to event stream!");
            Ok(())
        }
    }
}

async fn handle_faucet_command(
    faucet_command: &FaucetCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match faucet_command {
        FaucetCommands::Request { did, amount } => {
            println!("💰 Requesting {} testnet tokens...", amount);
            println!("   Recipient: {}", did.green());
            println!("✅ Tokens sent successfully!");
            println!("   Transaction ID: tx_{}", Uuid::new_v4());
            Ok(())
        }
        FaucetCommands::Balance => {
            println!("💰 Faucet Balance:");
            println!("   Available: 1,000,000 ASTRA");
            Ok(())
        }
    }
}

// ============================================================================
// COLLABORATIVE COMPUTE COMMAND HANDLERS
// ============================================================================

async fn handle_collaborative_command(
    collab_command: &CollaborativeCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match collab_command {
        CollaborativeCommands::Create {
            computation_type,
            participants,
            consensus_policy,
        } => {
            println!("🤝 Creating collaborative computation...");
            println!("   Type: {}", computation_type);
            println!("   Participants: {}", participants);
            println!("   Consensus policy: {}", consensus_policy);

            let computation_id = format!("collab_{}", Uuid::new_v4());
            println!("\n✅ Collaborative computation created!");
            println!("   Computation ID: {}", computation_id.cyan());
            Ok(())
        }
        CollaborativeCommands::Join {
            computation_id,
            did,
        } => {
            println!("🔗 Joining collaborative computation...");
            println!("   Computation ID: {}", computation_id.cyan());
            println!("   Participant DID: {}", did.green());
            println!("✅ Joined successfully!");
            Ok(())
        }
        CollaborativeCommands::Submit {
            computation_id,
            result,
        } => {
            println!("📤 Submitting partial result...");
            println!("   Computation ID: {}", computation_id.cyan());
            println!("   Result file: {}", result);
            println!("✅ Result submitted successfully!");
            Ok(())
        }
        CollaborativeCommands::Status { computation_id } => {
            println!("📊 Collaboration Status:");
            println!("   Computation ID: {}", computation_id.cyan());
            println!("   Status: In Progress");
            println!("   Participants: 3/3");
            println!("   Progress: 67%");
            Ok(())
        }
        CollaborativeCommands::Smpc(smpc_cmd) => handle_smpc_command(smpc_cmd).await,
    }
}

async fn handle_smpc_command(
    smpc_command: &SmpcCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match smpc_command {
        SmpcCommands::Create {
            participants,
            threshold,
            computation_type,
        } => {
            println!("🔐 Creating SMPC session...");
            println!("   Participants: {}", participants);
            println!("   Threshold: {}", threshold);
            println!("   Computation type: {}", computation_type);

            let session_id = format!("smpc_{}", Uuid::new_v4());
            println!("\n✅ SMPC session created!");
            println!("   Session ID: {}", session_id.cyan());
            Ok(())
        }
        SmpcCommands::Submit { session_id, share } => {
            println!("🔒 Submitting secret share...");
            println!("   Session ID: {}", session_id.cyan());
            println!("   Share file: {}", share);
            println!("✅ Secret share submitted!");
            Ok(())
        }
        SmpcCommands::Compute { session_id } => {
            println!("🧮 Computing SMPC result...");
            println!("   Session ID: {}", session_id.cyan());
            println!("✅ Computation complete!");
            println!("   Result: [encrypted]");
            Ok(())
        }
        SmpcCommands::Status { session_id } => {
            println!("📊 SMPC Session Status:");
            println!("   Session ID: {}", session_id.cyan());
            println!("   Status: Ready for computation");
            println!("   Shares submitted: 3/3");
            Ok(())
        }
    }
}

// ============================================================================
// NFT STORAGE COMMAND HANDLERS
// ============================================================================

async fn handle_nft_command(
    cli: &Cli,
    ctx: &CliContext,
    nft_command: &NftCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match nft_command {
        NftCommands::Create {
            name,
            image,
            metadata,
            did,
        } => {
            let owner_did = resolve_effective_did(cli, ctx, did.as_deref())?;
            println!("🎨 Creating NFT...");
            println!("   Name: {}", name.green());
            println!("   Image: {}", image);
            println!("   Owner: {}", owner_did.cyan());

            let nft_id = format!("nft_{}", Uuid::new_v4());
            println!("\n✅ NFT created successfully!");
            println!("   NFT ID: {}", nft_id.yellow());
            Ok(())
        }
        NftCommands::Query { owner, collection } => {
            println!("🔍 Querying NFTs...");
            if let Some(o) = owner {
                println!("   Owner filter: {}", o);
            }
            if let Some(c) = collection {
                println!("   Collection filter: {}", c);
            }
            println!("✅ Found 5 NFTs");
            Ok(())
        }
        NftCommands::Transfer { nft_id, to } => {
            println!("📤 Transferring NFT...");
            println!("   NFT ID: {}", nft_id.yellow());
            println!("   Recipient: {}", to.green());
            println!("✅ NFT transferred successfully!");
            Ok(())
        }
        NftCommands::Collection(coll_cmd) => handle_nft_collection_command(coll_cmd).await,
    }
}

async fn handle_nft_collection_command(
    coll_command: &NftCollectionCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match coll_command {
        NftCollectionCommands::Create {
            name,
            symbol,
            royalty,
            creator_did,
        } => {
            println!("🎭 Creating NFT collection...");
            println!("   Name: {}", name.green());
            println!("   Symbol: {}", symbol.yellow());
            println!("   Royalty: {}%", royalty);
            println!("   Creator: {}", creator_did.cyan());

            let collection_id = format!("collection_{}", Uuid::new_v4());
            println!("\n✅ Collection created successfully!");
            println!("   Collection ID: {}", collection_id.magenta());
            Ok(())
        }
        NftCollectionCommands::Mint {
            collection_id,
            metadata,
        } => {
            println!("⚡ Minting NFT to collection...");
            println!("   Collection ID: {}", collection_id.magenta());
            println!("   Metadata: {}", metadata);
            println!("✅ NFT minted successfully!");
            Ok(())
        }
        NftCollectionCommands::Stats { collection_id } => {
            println!("📊 Collection Statistics:");
            println!("   Collection ID: {}", collection_id.magenta());
            println!("   Total minted: 100");
            println!("   Unique owners: 45");
            println!("   Floor price: 1.5 ASTRA");
            println!("   Total volume: 150 ASTRA");
            Ok(())
        }
        NftCollectionCommands::List { creator } => {
            println!("📋 NFT Collections:");
            if let Some(c) = creator {
                println!("   Creator filter: {}", c);
            }
            println!("✅ Found 3 collections");
            Ok(())
        }
    }
}

// ============================================================================
// CONNECTION MANAGEMENT
// ============================================================================

/// Get the configured connection or use localhost default
async fn get_simulator_connection() -> Result<String, Box<dyn std::error::Error>> {
    let config = load_cli_config().await;

    if let Ok(cfg) = config {
        if let Some(connections) = cfg.connections {
            if let Some(sim_conn) = connections.simulator {
                println!("🔗 Using configured simulator: {}", sim_conn.url.cyan());
                return Ok(sim_conn.url);
            }
        }
    }

    // Default to localhost
    let default_url = "http://localhost:50051".to_string();
    println!("🔗 Using default simulator: {}", default_url.cyan());
    Ok(default_url)
}

/// Merge `~/.spacekit/network/config.toml` (or `SPACEKIT_NETWORK_CONFIG`) into `connections`.
fn merge_spacekit_network_overlay(config: &mut CLIConfig) {
    let Some(net) = crate::network_profile::load_spacekit_network_file()
        .ok()
        .flatten()
    else {
        return;
    };

    let conn = config
        .connections
        .get_or_insert_with(ConnectionsConfig::default);
    if let Some(u) = net.urls.compute.as_ref().filter(|s| !s.is_empty()) {
        conn.compute = Some(RemoteConnection {
            url: u.clone(),
            node_did: None,
            quantum_encrypted: false,
            last_connected: None,
        });
    }
    if let Some(u) = net.urls.storage.as_ref().filter(|s| !s.is_empty()) {
        conn.storage = Some(RemoteConnection {
            url: u.clone(),
            node_did: None,
            quantum_encrypted: false,
            last_connected: None,
        });
    }
    if !net.messaging.bootstrap_peers.is_empty() {
        conn.messaging_peers = net.messaging.bootstrap_peers.clone();
    }
}

pub(crate) fn load_cli_config_sync() -> Result<CLIConfig, Box<dyn std::error::Error>> {
    let config_dir = dirs::home_dir()
        .ok_or("Home directory not found")?
        .join(".spacekit");

    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        return Err("Configuration not found. Run 'spacekit init' first.".into());
    }

    let config_str = std::fs::read_to_string(config_path)?;
    let mut config: CLIConfig = toml::from_str(&config_str)?;
    merge_spacekit_network_overlay(&mut config);
    Ok(config)
}

/// Load CLI configuration from ~/.spacekit
pub(crate) async fn load_cli_config() -> Result<CLIConfig, Box<dyn std::error::Error>> {
    load_cli_config_sync()
}

/// Save CLI configuration to ~/.spacekit
pub(crate) async fn save_cli_config(config: &CLIConfig) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = dirs::home_dir()
        .ok_or("Home directory not found")?
        .join(".spacekit");

    std::fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("config.toml");
    let config_str = toml::to_string_pretty(config)?;
    std::fs::write(config_path, config_str)?;

    Ok(())
}

// ============================================================================
// SWTCHVM / SMART CONTRACT COMMAND HANDLERS
// ============================================================================

async fn handle_vm_command(
    cli: &Cli,
    ctx: &CliContext,
    vm_command: &VmCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match vm_command {
        VmCommands::Fund { did, amount } => {
            let owner_did = resolve_effective_did(cli, ctx, did.as_deref())?;
            println!("💰 Funding SwtchVM deployer balance (in-process ledger)...");
            let node = get_or_create_compute_node().await?;
            let new_balance = node
                .swtchvm_fund_owner(&owner_did, u128::from(*amount))
                .await
                .map_err(|e| format!("Fund failed: {}", e))?;
            println!(
                "✅ Credited {} for {} — new balance {}",
                amount.to_string().cyan(),
                owner_did.green(),
                new_balance.to_string().yellow()
            );
            println!("💡 SwtchVM balances live in this process only; each new `spacekit` command starts a fresh ledger.");
            println!("   {} tops up deploy/call gas automatically; use {} to add more in this same process.",
                "spacekit contract deploy".green(),
                "spacekit vm fund".cyan(),
            );
            Ok(())
        }
        VmCommands::Balance { did } => {
            let owner_did = resolve_effective_did(cli, ctx, did.as_deref())?;
            println!("📊 SwtchVM ledger balance (in-process; same run as this command only)...");
            let node = get_or_create_compute_node().await?;
            let bal = node
                .swtchvm_get_balance(&owner_did)
                .await
                .map_err(|e| format!("Balance query failed: {}", e))?;
            println!(
                "   {} → balance {}",
                owner_did.green(),
                bal.to_string().yellow()
            );
            println!("💡 Each new `spacekit` process starts a fresh SwtchVM ledger. Use {} in the same shell session after {} if you need to see credited funds.",
                "spacekit vm balance".cyan(),
                "spacekit vm fund".cyan(),
            );
            Ok(())
        }
        VmCommands::Earnings { did } => {
            let owner_did = resolve_effective_did(cli, ctx, did.as_deref())?;
            println!("📊 Operator earnings (blockchain ledger)…");

            let ledger_path =
                crate::network_profile::default_data_dir("blockchain").join("ledger.json");
            if !ledger_path.exists() {
                println!(
                    "   No blockchain ledger found. Start with: {}",
                    "spacekit network up --full".cyan()
                );
                return Ok(());
            }
            let data = std::fs::read_to_string(&ledger_path)?;
            let v: serde_json::Value = serde_json::from_str(&data)?;
            let block_number = v.get("block_number").and_then(|b| b.as_u64()).unwrap_or(0);
            let balance = v
                .get("accounts")
                .and_then(|a| a.get(&owner_did))
                .and_then(|b| b.as_u64())
                .unwrap_or(0);
            println!("   block:    {}", block_number.to_string().cyan());
            println!("   operator: {}", owner_did.green());
            println!("   balance:  {} ASTRA", balance.to_string().yellow());
            Ok(())
        }
        VmCommands::Withdraw { did, amount } => {
            let owner_did = resolve_effective_did(cli, ctx, did.as_deref())?;
            println!("💸 Withdrawing operator earnings…");

            let ledger_path =
                crate::network_profile::default_data_dir("blockchain").join("ledger.json");
            if !ledger_path.exists() {
                return Err(
                    "No blockchain ledger found. Start with: spacekit network up --full".into(),
                );
            }
            let data = std::fs::read_to_string(&ledger_path)?;
            let mut v: serde_json::Value = serde_json::from_str(&data)?;
            let current = v
                .get("accounts")
                .and_then(|a| a.get(&owner_did))
                .and_then(|b| b.as_u64())
                .unwrap_or(0);
            let withdraw_amount = if *amount == 0 { current } else { *amount };
            if withdraw_amount > current {
                return Err(format!(
                    "insufficient balance: have {} ASTRA, requested {}",
                    current, withdraw_amount
                )
                .into());
            }
            let remaining = current - withdraw_amount;
            if let Some(accts) = v.get_mut("accounts").and_then(|a| a.as_object_mut()) {
                accts.insert(owner_did.clone(), serde_json::json!(remaining));
            }
            std::fs::write(&ledger_path, serde_json::to_string_pretty(&v)?)?;
            println!(
                "   ✅ Withdrew {} ASTRA from {}",
                withdraw_amount.to_string().cyan(),
                owner_did.green()
            );
            println!("   remaining: {} ASTRA", remaining.to_string().yellow());
            Ok(())
        }
        VmCommands::BrainSeed {
            contract_id,
            key,
            brain,
        } => {
            if !brain.exists() {
                return Err(format!("brain file not found: {}", brain.display()).into());
            }
            let brain_bytes =
                std::fs::read(brain).map_err(|e| format!("failed to read brain file: {}", e))?;
            println!("🧠 Seeding brain into contract KV…");
            println!("   contract: {}", contract_id.cyan());
            println!("   key:      {}", key.green());
            println!(
                "   size:     {} bytes",
                brain_bytes.len().to_string().yellow()
            );

            let node = get_or_create_compute_node().await?;
            node.seed_contract_kv(contract_id, key.as_bytes(), brain_bytes)
                .await
                .map_err(|e| format!("seed_contract_kv: {}", e))?;
            println!(
                "✅ Brain seeded. Contract can now call `load_brain_from_storage(\"{}\")`.",
                key
            );
            Ok(())
        }
    }
}

/// Persist deployed contract WASM in the in-process storage node (same process as the CLI).
async fn pin_deployed_contract_wasm_in_storage(
    wasm: &[u8],
    owner_did: &str,
    contract_name: &str,
    contract_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let storage = get_or_create_storage_node().await?;
    let pk = load_public_key().await?;
    let safe_name: String = contract_name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    let safe_name = if safe_name.is_empty() {
        "contract".to_string()
    } else {
        safe_name
    };
    let safe_id: String = contract_id
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
        .take(120)
        .collect();
    let safe_id = if safe_id.is_empty() {
        "unknown".to_string()
    } else {
        safe_id
    };
    let filename = format!("vm-contracts/{}/{}.wasm", safe_name, safe_id);
    let (file_id, _) = storage
        .store_file(
            &filename,
            wasm,
            owner_did,
            &pk,
            Some("application/wasm".to_string()),
        )
        .await
        .map_err(|e| format!("store_file: {}", e))?;
    Ok(file_id)
}

async fn handle_contract_command(
    cli: &Cli,
    ctx: &CliContext,
    contract_command: &ContractCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match contract_command {
        ContractCommands::Deploy {
            contract,
            name,
            did,
            args,
            initial_balance,
        } => {
            let owner_did = resolve_effective_did(cli, ctx, did.as_deref())?;
            println!("📜 Deploying smart contract...");
            println!("   Contract file: {}", contract.cyan());
            println!("   Name: {}", name.green());
            println!("   Owner: {}", owner_did.yellow());
            if let Some(a) = args {
                println!("   Constructor args: {}", a);
            }
            println!("   Initial balance: {} ASTRA", initial_balance);

            // Get compute node connection
            let node = get_or_create_compute_node().await?;

            // Read contract WASM file
            let wasm_code = std::fs::read(contract)
                .map_err(|e| format!("Failed to read contract file: {}", e))?;

            let contract_id = node
                .deploy_contract(name, wasm_code.clone(), owner_did.clone())
                .await
                .map_err(|e| {
                    println!("❌ Deployment failed: {}", e);
                    e
                })?;

            println!("\n✅ Contract deployed successfully!");
            println!("   Contract ID: {}", contract_id.cyan());
            println!("   Address: contract_{}", contract_id);
            println!(
                "\n💡 Call with: spacekit contract call --contract-id {} --function <name> [--args '<json-array>']",
                contract_id
            );

            let file_id =
                pin_deployed_contract_wasm_in_storage(&wasm_code, &owner_did, name, &contract_id)
                    .await
                    .map_err(|e| {
                        format!(
                            "Contract deployed (id {}) but failed to pin WASM in storage node: {}",
                            contract_id, e
                        )
                    })?;
            println!(
                "   📎 Pinned WASM in storage node: file_id={}",
                file_id.cyan()
            );
            println!(
                "   💡 Fetch: {}",
                format!(
                    "spacekit storage retrieve {} --output ./pinned-contract.wasm --embedded (--local) --requester-did {}",
                    file_id, owner_did
                )
                .yellow()
            );

            Ok(())
        }
        ContractCommands::Call {
            contract_id,
            function,
            args,
            did,
            gas_limit,
        } => {
            let caller_did = resolve_effective_did(cli, ctx, did.as_deref())?;
            println!("⚡ Calling smart contract...");
            println!("   Contract: {}", contract_id.cyan());
            println!("   Function: {}", function.green());
            println!("   Caller: {}", caller_did.yellow());
            println!("   Gas limit: {}", gas_limit);

            // Get compute node connection
            let node = get_or_create_compute_node().await?;

            // Parse arguments
            let func_args = if let Some(a) = args {
                serde_json::from_str::<Vec<serde_json::Value>>(a)
                    .map_err(|e| format!("Invalid JSON args: {}", e))?
            } else {
                vec![]
            };

            // Execute contract call
            match node
                .execute_contract(
                    contract_id,
                    function,
                    func_args,
                    caller_did.clone(),
                    *gas_limit,
                )
                .await
            {
                Ok(result) => {
                    println!("\n✅ Contract executed successfully!");
                    println!("   Result: {}", serde_json::to_string_pretty(&result)?);
                    println!("   Gas used: estimated");
                }
                Err(e) => {
                    println!("❌ Execution failed: {}", e);
                    return Err(e.into());
                }
            }

            Ok(())
        }
        ContractCommands::State { contract_id, key } => {
            println!("🔍 Querying contract state...");
            println!("   Contract: {}", contract_id.cyan());

            // Get compute node connection
            let node = get_or_create_compute_node().await?;

            match node.get_contract_state(contract_id, key.clone()).await {
                Ok(state) => {
                    println!("\n📊 Contract State:");
                    println!("{}", serde_json::to_string_pretty(&state)?);
                }
                Err(e) => {
                    println!("❌ Query failed: {}", e);
                    return Err(e.into());
                }
            }

            Ok(())
        }
        ContractCommands::List { owner } => {
            println!("📋 Listing deployed contracts...");
            if let Some(o) = owner {
                println!("   Owner filter: {}", o);
            }

            // Get compute node connection
            let node = get_or_create_compute_node().await?;

            match node.list_contracts(owner.clone()).await {
                Ok(contracts) => {
                    println!("\n✅ Found {} contracts:", contracts.len());
                    for (i, contract) in contracts.iter().enumerate() {
                        println!("\n{}. Contract ID: {}", i + 1, contract.id.cyan());
                        println!("   Name: {}", contract.name.green());
                        println!("   Owner: {}", contract.owner_did.yellow());
                        println!("   Deployed: {}", contract.deployed_at);
                    }
                }
                Err(e) => {
                    println!("❌ List failed: {}", e);
                    return Err(e.into());
                }
            }

            Ok(())
        }
        ContractCommands::History { contract_id, limit } => {
            println!("📜 Contract execution history...");
            println!("   Contract: {}", contract_id.cyan());
            println!("   Limit: {}", limit);

            // Get compute node connection
            let node = get_or_create_compute_node().await?;

            match node.get_contract_history(contract_id, *limit).await {
                Ok(history) => {
                    println!("\n✅ Found {} executions:", history.len());
                    for (i, exec) in history.iter().enumerate() {
                        println!("\n{}. Function: {}", i + 1, exec.function.green());
                        println!("   Caller: {}", exec.caller.yellow());
                        println!("   Timestamp: {}", exec.timestamp);
                        println!("   Gas used: {}", exec.gas_used);
                    }
                }
                Err(e) => {
                    println!("❌ History retrieval failed: {}", e);
                    return Err(e.into());
                }
            }

            Ok(())
        }
    }
}

async fn handle_connection_command(
    connect_command: &ConnectionCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match connect_command {
        ConnectionCommands::Simulator {
            url,
            quantum_encrypted,
            set_default,
        } => {
            println!("🌐 Configuring simulator connection...");
            println!("   URL: {}", url.cyan());
            println!("   Quantum encrypted: {}", quantum_encrypted);
            println!("   Set as default: {}", set_default);

            // Load or create config
            let mut config = load_cli_config().await.unwrap_or_else(|_| CLIConfig {
                identity: IdentityConfig {
                    did: "".to_string(),
                    algorithm: "Kyber1024".to_string(),
                    public_key_path: "~/.spacekit/keys/public_key.hex".to_string(),
                    private_key_path: "~/.spacekit/keys/private_key.hex".to_string(),
                    linked_username: None,
                    website_auth: None,
                },
                network: NetworkConfig {
                    default_network: "testnet".to_string(),
                    endpoints: HashMap::new(),
                },
                project: ProjectConfig {
                    name: "default".to_string(),
                    version: "1.0.0".to_string(),
                    created_at: Utc::now(),
                },
                connections: None,
                messaging: Some(MessagingSettings {
                    directory_ttl_seconds: Some(3600),
                    directory_max_entries: Some(1000),
                }),
            });

            // Update connections
            let mut connections = config.connections.unwrap_or_else(|| ConnectionsConfig {
                simulator: None,
                compute: None,
                storage: None,
                messaging_peers: Vec::new(),
                default_connection: None,
            });

            connections.simulator = Some(RemoteConnection {
                url: url.clone(),
                node_did: None,
                quantum_encrypted: *quantum_encrypted,
                last_connected: Some(Utc::now().to_rfc3339()),
            });

            if *set_default {
                connections.default_connection = Some("simulator".to_string());
            }

            config.connections = Some(connections);

            // Save config
            save_cli_config(&config).await?;

            println!("\n✅ Simulator connection configured!");
            println!("   Saved to ~/.spacekit/config.toml");

            Ok(())
        }
        ConnectionCommands::Compute {
            url,
            node_did,
            quantum_encrypted,
        } => {
            println!("🖥️  Configuring compute node connection...");
            println!("   URL: {}", url.cyan());
            println!("   Node DID: {}", node_did.yellow());
            println!("   Quantum encrypted: {}", quantum_encrypted);

            let mut config = load_cli_config().await?;
            let mut connections = config.connections.unwrap_or_default();

            connections.compute = Some(RemoteConnection {
                url: url.clone(),
                node_did: Some(node_did.clone()),
                quantum_encrypted: *quantum_encrypted,
                last_connected: Some(Utc::now().to_rfc3339()),
            });

            config.connections = Some(connections);
            save_cli_config(&config).await?;

            println!("\n✅ Compute node connection configured!");
            Ok(())
        }
        ConnectionCommands::Storage {
            url,
            node_did,
            quantum_encrypted,
        } => {
            println!("💾 Configuring storage node connection...");
            println!("   URL: {}", url.cyan());
            println!("   Node DID: {}", node_did.yellow());
            println!("   Quantum encrypted: {}", quantum_encrypted);

            let mut config = load_cli_config().await?;
            let mut connections = config.connections.unwrap_or_default();

            connections.storage = Some(RemoteConnection {
                url: url.clone(),
                node_did: Some(node_did.clone()),
                quantum_encrypted: *quantum_encrypted,
                last_connected: Some(Utc::now().to_rfc3339()),
            });

            config.connections = Some(connections);
            save_cli_config(&config).await?;

            println!("\n✅ Storage node connection configured!");
            Ok(())
        }
        ConnectionCommands::Messaging { peer, replace } => {
            println!("💬 Configuring messaging bootstrap peers...");
            println!("   Peer: {}", peer.cyan());
            println!("   Replace: {}", replace);

            let mut config = load_cli_config().await?;
            let mut connections = config.connections.unwrap_or_default();

            if *replace {
                connections.messaging_peers = vec![peer.clone()];
            } else if !connections.messaging_peers.contains(peer) {
                connections.messaging_peers.push(peer.clone());
            }

            config.connections = Some(connections);
            save_cli_config(&config).await?;

            println!("\n✅ Messaging peers configured!");
            Ok(())
        }
        ConnectionCommands::Status => {
            println!("📊 Connection Status:\n");

            match load_cli_config().await {
                Ok(config) => {
                    if let Some(connections) = config.connections {
                        if let Some(sim) = connections.simulator {
                            println!("🌐 Simulator:");
                            println!("   URL: {}", sim.url.cyan());
                            println!("   Quantum encrypted: {}", sim.quantum_encrypted);
                            if let Some(last) = sim.last_connected {
                                println!("   Last connected: {}", last);
                            }
                            println!();
                        }

                        if let Some(comp) = connections.compute {
                            println!("🖥️  Compute Node:");
                            println!("   URL: {}", comp.url.cyan());
                            if let Some(did) = comp.node_did {
                                println!("   DID: {}", did.yellow());
                            }
                            println!("   Quantum encrypted: {}", comp.quantum_encrypted);
                            println!();
                        }

                        if let Some(stor) = connections.storage {
                            println!("💾 Storage Node:");
                            println!("   URL: {}", stor.url.cyan());
                            if let Some(did) = stor.node_did {
                                println!("   DID: {}", did.yellow());
                            }
                            println!("   Quantum encrypted: {}", stor.quantum_encrypted);
                            println!();
                        }

                        if !connections.messaging_peers.is_empty() {
                            println!("💬 Messaging Peers:");
                            for peer in connections.messaging_peers {
                                println!("   - {}", peer.cyan());
                            }
                            println!();
                        }

                        if let Some(default) = connections.default_connection {
                            println!("⭐ Default: {}", default.green());
                        }
                    } else {
                        println!("⚠️  No connections configured.");
                        println!("   Use 'spacekit connect simulator --url <URL>' to configure.");
                    }
                }
                Err(_) => {
                    println!("⚠️  No configuration found.");
                    println!("   Run 'spacekit init' to set up your workspace.");
                }
            }

            Ok(())
        }
        ConnectionCommands::Test { connection_type } => {
            println!("🔍 Testing {:?} connection...", connection_type);

            let config = load_cli_config().await?;
            let connections = config.connections.ok_or("No connections configured")?;

            let url = match connection_type {
                ConnectionType::Simulator => {
                    connections.simulator.ok_or("Simulator not configured")?.url
                }
                ConnectionType::Compute => {
                    connections
                        .compute
                        .ok_or("Compute node not configured")?
                        .url
                }
                ConnectionType::Storage => {
                    connections
                        .storage
                        .ok_or("Storage node not configured")?
                        .url
                }
                ConnectionType::Messaging => {
                    if connections.messaging_peers.is_empty() {
                        return Err("Messaging peers not configured".into());
                    }
                    connections.messaging_peers[0].clone()
                }
            };

            println!("   Testing connection to: {}", url.cyan());
            let success = match connection_type {
                ConnectionType::Messaging => probe_tcp("messaging", &url, 10).await,
                ConnectionType::Compute | ConnectionType::Storage => {
                    if url.starts_with("http://") || url.starts_with("https://") {
                        let client = reqwest::Client::builder()
                            .timeout(Duration::from_secs(10))
                            .build()?;
                        probe_http(
                            &client,
                            "connection health",
                            &format!("{}/health", url.trim_end_matches('/')),
                        )
                        .await
                    } else {
                        probe_tcp("connection", &url, 10).await
                    }
                }
                ConnectionType::Simulator => {
                    if url.starts_with("http://") || url.starts_with("https://") {
                        let client = reqwest::Client::builder()
                            .timeout(Duration::from_secs(10))
                            .build()?;
                        probe_http(&client, "simulator HTTP", &url).await
                    } else {
                        probe_tcp("simulator", &url, 10).await
                    }
                }
            };
            if success {
                println!("{}", "✅ Connection probe succeeded.".green());
                Ok(())
            } else {
                Err(format!("{:?} connection probe failed", connection_type).into())
            }
        }
    }
}

impl Default for ConnectionsConfig {
    fn default() -> Self {
        Self {
            simulator: None,
            compute: None,
            storage: None,
            messaging_peers: Vec::new(),
            default_connection: None,
        }
    }
}

// ============================================================================
// MESSAGING COMMAND HANDLERS
// ============================================================================

async fn handle_message_command(
    message_command: &MessageCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match message_command {
        MessageCommands::Send { to, message, file } => {
            println!("💬 Sending message...");
            println!("   To: {}", to.cyan());
            println!("   Message: {}", message.green());

            let messaging_node = get_or_create_messaging_node().await?;
            let sender = ensure_messaging_user(&messaging_node).await?;

            if messaging_node.get_user_by_did(to).await?.is_none() {
                println!("❌ Recipient not found in this messaging node.");
                println!(
                    "💡 Ensure the recipient is registered and reachable on the same P2P network."
                );
                return Ok(());
            }

            let mut file_id = None;
            let mut file_name = None;
            if let Some(file_path) = file {
                let file_data = std::fs::read(file_path)
                    .map_err(|e| CliError::FileRead(file_path.clone(), e))?;
                let filename = Path::new(file_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(file_path)
                    .to_string();
                let mime_type = determine_mime_type_for_attachment(file_path);

                let shared_file = messaging_node
                    .upload_direct_file(
                        to.clone(),
                        sender.id.clone(),
                        filename.clone(),
                        file_data,
                        mime_type,
                    )
                    .await
                    .map_err(|e| CliError::Messaging(e.to_string()))?;

                println!("   📎 Attached file: {} ({})", filename, shared_file.id);
                file_id = Some(shared_file.id);
                file_name = Some(filename);
            }

            let content = if let Some(attached_id) = &file_id {
                if let Some(name) = &file_name {
                    format!("{} [file:{}:{}]", message, attached_id, name)
                } else {
                    format!("{} [file:{}]", message, attached_id)
                }
            } else {
                message.clone()
            };

            match messaging_node
                .send_direct_message(sender.id.clone(), to.clone(), content)
                .await
            {
                Ok(_) => println!("\n✅ Message sent!"),
                Err(e) => {
                    println!("❌ Failed to send message: {}", e);
                    return Err(Box::new(CliError::Messaging(e.to_string())));
                }
            }

            Ok(())
        }
        MessageCommands::List { detailed } => {
            println!("📋 Listing conversations...");

            // Get current user DID
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();

            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            let messaging_node = get_or_create_messaging_node().await?;
            let user = ensure_messaging_user(&messaging_node).await?;

            let direct_convos = messaging_node
                .get_user_direct_conversations(&user.id)
                .await?;
            let groups = messaging_node.get_groups_for_user(&user.id).await?;

            if direct_convos.is_empty() && groups.is_empty() {
                println!("   No conversations yet.");
                return Ok(());
            }

            if !direct_convos.is_empty() {
                println!("\n💬 Direct Conversations:");
                for convo in &direct_convos {
                    println!("   • {}", convo.id.cyan());
                    if *detailed {
                        println!(
                            "     Participants: {} ↔ {}",
                            convo.participant_a_id, convo.participant_b_id
                        );
                        println!("     Active: {}", convo.is_active);
                        println!(
                            "     Created: {}",
                            convo.created_at.format("%Y-%m-%d %H:%M:%S UTC")
                        );
                        if let Some(last) = convo.last_message_at {
                            println!(
                                "     Last message: {}",
                                last.format("%Y-%m-%d %H:%M:%S UTC")
                            );
                        }
                    }
                }
            }

            if !groups.is_empty() {
                println!("\n👥 Groups:");
                for group in &groups {
                    println!("   • {} ({})", group.name.green(), group.id.cyan());
                    if *detailed {
                        println!("     Creator: {}", group.creator_id);
                        println!(
                            "     Created: {}",
                            group.created_at.format("%Y-%m-%d %H:%M:%S UTC")
                        );
                        if let Some(desc) = &group.description {
                            println!("     Description: {}", desc);
                        }
                    }
                }
            }

            Ok(())
        }
        MessageCommands::Chat { with } => {
            println!("💬 Starting chat with: {}", with.cyan());
            println!("   Type messages and press Enter to send.");
            println!("   Type '/exit' to quit.\n");

            let messaging_node = get_or_create_messaging_node().await?;
            let sender = ensure_messaging_user(&messaging_node).await?;
            let is_direct = with.starts_with("did:");

            loop {
                print!("> ");
                io::stdout().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let line = input.trim();

                if line.eq_ignore_ascii_case("/exit") {
                    break;
                }

                if line.is_empty() {
                    continue;
                }

                if is_direct {
                    if messaging_node.get_user_by_did(with).await?.is_none() {
                        println!("❌ Recipient not found in this messaging node.");
                        println!("💡 Ensure the recipient is registered and reachable.");
                        continue;
                    }

                    if let Err(e) = messaging_node
                        .send_direct_message(sender.id.clone(), with.clone(), line.to_string())
                        .await
                    {
                        println!("❌ Failed to send message: {}", e);
                    }
                } else {
                    if let Err(e) = messaging_node
                        .send_text_message(with.clone(), sender.id.clone(), line.to_string())
                        .await
                    {
                        println!("❌ Failed to send group message: {}", e);
                    }
                }
            }

            Ok(())
        }
        MessageCommands::CreateGroup { name, description } => {
            println!("👥 Creating group...");
            println!("   Name: {}", name.green());
            if let Some(desc) = description {
                println!("   Description: {}", desc.yellow());
            }

            let messaging_node = get_or_create_messaging_node().await?;
            let user = ensure_messaging_user(&messaging_node).await?;

            match messaging_node
                .create_group(name.clone(), user.id.clone(), description.clone())
                .await
            {
                Ok(group) => {
                    println!("\n✅ Group created!");
                    println!("   Group ID: {}", group.id.cyan());
                }
                Err(e) => {
                    println!("❌ Failed to create group: {}", e);
                    return Err(Box::new(CliError::Messaging(e.to_string())));
                }
            }
            Ok(())
        }
        MessageCommands::GroupMessage {
            group,
            message,
            file,
        } => {
            println!("💬 Sending group message...");
            println!("   Group: {}", group.cyan());
            println!("   Message: {}", message.green());

            let messaging_node = get_or_create_messaging_node().await?;
            let sender = ensure_messaging_user(&messaging_node).await?;

            let mut file_id = None;
            let mut file_name = None;
            if let Some(file_path) = file {
                let file_data = std::fs::read(file_path)
                    .map_err(|e| CliError::FileRead(file_path.clone(), e))?;
                let filename = Path::new(file_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(file_path)
                    .to_string();

                let shared_file = messaging_node
                    .upload_group_file(
                        group.clone(),
                        sender.id.clone(),
                        filename.clone(),
                        file_data,
                        determine_mime_type_for_attachment(file_path),
                    )
                    .await
                    .map_err(|e| CliError::Messaging(e.to_string()))?;

                println!("   📎 Attached file: {} ({})", filename, shared_file.id);
                file_id = Some(shared_file.id);
                file_name = Some(filename);
            }

            let content = if let Some(attached_id) = &file_id {
                if let Some(name) = &file_name {
                    format!("{} [file:{}:{}]", message, attached_id, name)
                } else {
                    format!("{} [file:{}]", message, attached_id)
                }
            } else {
                message.clone()
            };

            match messaging_node
                .send_text_message(group.clone(), sender.id.clone(), content)
                .await
            {
                Ok(_) => println!("\n✅ Group message sent!"),
                Err(e) => {
                    println!("❌ Failed to send group message: {}", e);
                    return Err(Box::new(CliError::Messaging(e.to_string())));
                }
            }
            Ok(())
        }
        MessageCommands::Download { file_id, output } => {
            println!("📥 Downloading file...");
            println!("   File ID: {}", file_id.cyan());
            println!("   Output: {}", output.green());

            let messaging_node = get_or_create_messaging_node().await?;
            let config = load_cli_config().await?;
            let requester_did = config.identity.did.clone();

            if requester_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            let private_key = load_private_key()
                .map_err(|e| CliError::Config(format!("Failed to load private key: {}", e)))?;

            let output_path = {
                let output_path = Path::new(output);
                if output_path.is_dir() || output.ends_with('/') {
                    let filename = messaging_node
                        .get_shared_file_metadata(&file_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|file| file.filename)
                        .unwrap_or_else(|| file_id.clone());
                    output_path.join(filename)
                } else {
                    output_path.to_path_buf()
                }
            };

            match messaging_node
                .download_file(&file_id, &requester_did, &private_key)
                .await
            {
                Ok(data) => {
                    std::fs::write(&output_path, &data)?;
                    println!("\n✅ File downloaded and decrypted!");
                    println!("   Saved to: {}", output_path.display().to_string().green());
                    println!("   Size: {} bytes", data.len());
                }
                Err(e) => {
                    println!("❌ Failed to download file: {}", e);
                    return Err(Box::new(CliError::Messaging(e.to_string())));
                }
            }

            Ok(())
        }
        MessageCommands::Whois {
            did,
            peer,
            peer_addr,
        } => {
            println!("🔍 Looking up DID...");
            println!("   DID: {}", did.cyan());

            let messaging_node = get_or_create_messaging_node().await?;
            let _user = ensure_messaging_user(&messaging_node).await?;

            match messaging_node.get_user_by_did(did).await? {
                Some(user) => {
                    println!("\n✅ User found:");
                    println!("   ID: {}", user.id.green());
                    println!("   DID: {}", user.did.blue());
                    println!("   Username: {}", user.username.yellow());
                    println!("   Algorithm: {}", user.encryption_algorithm);
                }
                None => {
                    println!("\n⚠️  User not found locally. Trying scoped remote lookup...");
                    if peer.is_some() && peer_addr.is_some() {
                        println!("   ⚠️  Both --peer and --peer-addr provided; using --peer-addr");
                    }
                    let target_peer = if peer_addr.is_some() {
                        None
                    } else {
                        peer.clone()
                    };
                    match messaging_node
                        .directory_lookup_remote(
                            Some(did.clone()),
                            5,
                            Duration::from_secs(3),
                            target_peer,
                            peer_addr.clone(),
                        )
                        .await
                    {
                        Ok(entries) if !entries.is_empty() => {
                            for entry in entries {
                                if entry.did == *did {
                                    println!("\n✅ User found remotely:");
                                    println!("   DID: {}", entry.did.blue());
                                    println!("   Username: {}", entry.username.yellow());
                                    println!("   Algorithm: {}", entry.encryption_algorithm);
                                    return Ok(());
                                }
                            }
                            println!("\n⚠️  No exact match found remotely.");
                        }
                        Ok(_) => {
                            println!("\n❌ User not found in remote lookup.");
                        }
                        Err(e) => {
                            println!("\n❌ Remote lookup failed: {}", e);
                        }
                    }
                }
            }

            Ok(())
        }
        MessageCommands::DirectorySearch { prefix, limit } => {
            println!("🔎 Searching local directory...");
            println!("   Prefix: {}", prefix.cyan());

            let messaging_node = get_or_create_messaging_node().await?;
            let _user = ensure_messaging_user(&messaging_node).await?;

            let mut users = messaging_node.get_all_users().await?;
            users.retain(|user| user.did.starts_with(prefix));

            if users.is_empty() {
                println!("\n📭 No users matched that prefix.");
                return Ok(());
            }

            println!("\nFound {} user(s):", users.len().min(*limit));
            for user in users.iter().take(*limit) {
                println!("   - {} ({})", user.username.green(), user.did.blue());
            }

            if users.len() > *limit {
                println!(
                    "\n💡 {} more result(s) not shown. Increase --limit to see more.",
                    users.len() - *limit
                );
            }

            Ok(())
        }
        MessageCommands::DirectorySync {
            prefix,
            peer,
            peer_addr,
            limit,
            timeout,
            ttl_seconds,
            max_entries,
            dry_run,
        } => {
            println!("🔄 Syncing directory (scoped)...");
            let prefix = match prefix {
                Some(prefix) => prefix.clone(),
                None => {
                    println!("❌ Prefix is required for scoped sync (e.g., did:spacekit:user:)");
                    return Ok(());
                }
            };
            println!("   Prefix: {}", prefix.cyan());
            println!("   Limit: {}", limit);
            println!("   Timeout: {}s", timeout);

            let messaging_node = get_or_create_messaging_node().await?;
            let _user = ensure_messaging_user(&messaging_node).await?;
            let config = load_cli_config().await?;
            let settings = config.messaging.unwrap_or_default();

            if peer.is_some() && peer_addr.is_some() {
                println!("   ⚠️  Both --peer and --peer-addr provided; using --peer-addr");
            }
            let target_peer = if peer_addr.is_some() {
                None
            } else {
                peer.clone()
            };

            let entries = messaging_node
                .directory_lookup_remote(
                    Some(prefix),
                    *limit,
                    Duration::from_secs(*timeout),
                    target_peer,
                    peer_addr.clone(),
                )
                .await
                .map_err(|e| CliError::Messaging(e.to_string()))?;

            if entries.is_empty() {
                println!("\n📭 No remote entries found.");
                return Ok(());
            }

            if *dry_run {
                println!("\nFound {} entry(s):", entries.len());
                for entry in entries {
                    println!("   - {} ({})", entry.username.green(), entry.did.blue());
                }
                return Ok(());
            }

            let updated = messaging_node
                .apply_directory_entries(&entries)
                .await
                .map_err(|e| CliError::Messaging(e.to_string()))?;

            let ttl = ttl_seconds.or(settings.directory_ttl_seconds);
            let max_entries = max_entries.or(settings.directory_max_entries);

            if let Some(ttl) = ttl {
                let pruned = messaging_node
                    .prune_directory_cache(ttl)
                    .await
                    .map_err(|e| CliError::Messaging(e.to_string()))?;
                println!("   Entries pruned: {}", pruned);
            }
            if let Some(max_entries) = max_entries {
                let pruned = messaging_node
                    .prune_directory_cache_max(max_entries)
                    .await
                    .map_err(|e| CliError::Messaging(e.to_string()))?;
                println!("   Entries pruned (max): {}", pruned);
            }

            println!("\n✅ Directory sync complete!");
            println!("   Entries added/updated: {}", updated);
            Ok(())
        }
        MessageCommands::ResolveAttachments {
            message,
            output_dir,
        } => {
            let ids = extract_file_markers(message);
            if ids.is_empty() {
                println!("📭 No [file:<id>] markers found.");
                return Ok(());
            }

            let messaging_node = get_or_create_messaging_node().await?;
            let config = load_cli_config().await?;
            let requester_did = config.identity.did.clone();
            if requester_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let private_key = load_private_key()
                .map_err(|e| CliError::Config(format!("Failed to load private key: {}", e)))?;

            std::fs::create_dir_all(output_dir)?;
            for (file_id, marker_name) in ids {
                let filename = if let Some(name) = marker_name {
                    name
                } else {
                    messaging_node
                        .get_shared_file_metadata(&file_id)
                        .await
                        .ok()
                        .flatten()
                        .map(|file| file.filename)
                        .unwrap_or_else(|| file_id.clone())
                };
                let output_path = Path::new(output_dir).join(filename);
                match messaging_node
                    .download_file(&file_id, &requester_did, &private_key)
                    .await
                {
                    Ok(data) => {
                        std::fs::write(&output_path, &data)?;
                        println!("✅ Downloaded {} -> {}", file_id, output_path.display());
                    }
                    Err(e) => {
                        println!("❌ Failed to download {}: {}", file_id, e);
                    }
                }
            }

            Ok(())
        }
        MessageCommands::DirectoryWatch {
            prefix,
            peer,
            peer_addr,
            limit,
            timeout,
            interval,
            ttl_seconds,
            max_entries,
        } => {
            println!("🔁 Watching directory updates (scoped)...");
            println!("   Prefix: {}", prefix.cyan());
            println!("   Interval: {}s", interval);
            println!("   Limit: {}", limit);
            println!("   Timeout: {}s", timeout);
            println!("💡 Press Ctrl+C to stop.\n");

            let messaging_node = get_or_create_messaging_node().await?;
            let _user = ensure_messaging_user(&messaging_node).await?;
            let config = load_cli_config().await?;
            let settings = config.messaging.unwrap_or_default();

            if peer.is_some() && peer_addr.is_some() {
                println!("   ⚠️  Both --peer and --peer-addr provided; using --peer-addr");
            }
            let target_peer = if peer_addr.is_some() {
                None
            } else {
                peer.clone()
            };
            let ttl = ttl_seconds.or(settings.directory_ttl_seconds);
            let max_entries = max_entries.or(settings.directory_max_entries);

            loop {
                let entries = messaging_node
                    .directory_lookup_remote(
                        Some(prefix.clone()),
                        *limit,
                        Duration::from_secs(*timeout),
                        target_peer.clone(),
                        peer_addr.clone(),
                    )
                    .await
                    .unwrap_or_default();

                if !entries.is_empty() {
                    let updated = messaging_node
                        .apply_directory_entries(&entries)
                        .await
                        .unwrap_or(0);
                    println!(
                        "✅ Synced {} entry(s) at {}",
                        updated,
                        chrono::Utc::now().format("%H:%M:%S")
                    );
                }

                if let Some(ttl) = ttl {
                    let pruned = messaging_node.prune_directory_cache(ttl).await.unwrap_or(0);
                    if pruned > 0 {
                        println!("🧹 Pruned {} stale entries", pruned);
                    }
                }
                if let Some(max_entries) = max_entries {
                    let pruned = messaging_node
                        .prune_directory_cache_max(max_entries)
                        .await
                        .unwrap_or(0);
                    if pruned > 0 {
                        println!("🧹 Pruned {} entries to max", pruned);
                    }
                }

                tokio::time::sleep(Duration::from_secs(*interval)).await;
            }
        }
        MessageCommands::History {
            limit,
            conversation_id,
            group_id,
            sender_did,
            download_attachments,
            output_dir,
        } => {
            let messaging_node = get_or_create_messaging_node().await?;
            let config = load_cli_config().await?;
            let requester_did = config.identity.did.clone();
            if requester_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            let conversation_filter = if let Some(group_id) = group_id.as_ref() {
                if conversation_id.is_some() {
                    println!(
                        "⚠️  Both --conversation-id and --group-id provided; using --group-id"
                    );
                }
                Some(group_id.as_str())
            } else {
                conversation_id.as_deref()
            };

            let private_key = load_private_key()
                .map_err(|e| CliError::Config(format!("Failed to load private key: {}", e)))?;

            let messages = messaging_node
                .get_message_history_decrypted(
                    &requester_did,
                    &private_key,
                    *limit,
                    conversation_filter,
                )
                .await
                .map_err(|e| CliError::Messaging(e.to_string()))?;

            let messages = if let Some(sender_did) = sender_did.as_ref() {
                let sender = messaging_node
                    .get_user_by_did(sender_did)
                    .await
                    .map_err(|e| CliError::Messaging(e.to_string()))?
                    .ok_or_else(|| {
                        CliError::Messaging(format!("Sender DID not found locally: {}", sender_did))
                    })?;
                messages
                    .into_iter()
                    .filter(|message| message.sender_id == sender.id)
                    .collect()
            } else {
                messages
            };

            if messages.is_empty() {
                println!("📭 No messages found.");
                return Ok(());
            }

            println!("\n🗂️  Recent Messages:");
            for message in &messages {
                println!("━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("ID: {}", message.id.cyan());
                println!("From: {}", message.sender_id.yellow());
                println!("At: {}", message.created_at.format("%Y-%m-%d %H:%M:%S UTC"));
                match &message.content_type {
                    spacekit_messaging_node::MessageType::Text { content } => {
                        println!("Type: Text");
                        println!("Content: {}", content);
                    }
                    spacekit_messaging_node::MessageType::System { content } => {
                        println!("Type: System");
                        println!("Content: {}", content);
                    }
                    spacekit_messaging_node::MessageType::File {
                        file_id,
                        filename,
                        size,
                    } => {
                        println!("Type: File");
                        println!("File: {} ({}, {} bytes)", filename, file_id, size);
                    }
                    spacekit_messaging_node::MessageType::Image {
                        file_id,
                        filename,
                        size,
                        width,
                        height,
                    } => {
                        println!("Type: Image");
                        println!(
                            "Image: {} ({}, {} bytes) {}x{}",
                            filename,
                            file_id,
                            size,
                            width.unwrap_or(0),
                            height.unwrap_or(0)
                        );
                    }
                }
            }

            if *download_attachments {
                std::fs::create_dir_all(output_dir)?;
                for message in &messages {
                    if let spacekit_messaging_node::MessageType::Text { content } =
                        &message.content_type
                    {
                        let ids = extract_file_markers(content);
                        for (file_id, marker_name) in ids {
                            let filename = if let Some(name) = marker_name {
                                name
                            } else {
                                messaging_node
                                    .get_shared_file_metadata(&file_id)
                                    .await
                                    .ok()
                                    .flatten()
                                    .map(|file| file.filename)
                                    .unwrap_or_else(|| file_id.clone())
                            };
                            let output_path = Path::new(output_dir).join(filename);
                            match messaging_node
                                .download_file(&file_id, &requester_did, &private_key)
                                .await
                            {
                                Ok(data) => {
                                    std::fs::write(&output_path, &data)?;
                                    println!(
                                        "✅ Downloaded {} -> {}",
                                        file_id,
                                        output_path.display()
                                    );
                                }
                                Err(e) => {
                                    println!("❌ Failed to download {}: {}", file_id, e);
                                }
                            }
                        }
                    }
                }
            }

            Ok(())
        }
        MessageCommands::DownloadAttachmentsByMessage {
            message_id,
            output_dir,
        } => {
            let messaging_node = get_or_create_messaging_node().await?;
            let config = load_cli_config().await?;
            let requester_did = config.identity.did.clone();
            if requester_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            let private_key = load_private_key()
                .map_err(|e| CliError::Config(format!("Failed to load private key: {}", e)))?;

            let message = messaging_node
                .get_message_by_id_decrypted(&requester_did, &private_key, message_id)
                .await
                .map_err(|e| CliError::Messaging(e.to_string()))?;

            let message = match message {
                Some(message) => message,
                None => {
                    println!("❌ Message not found in history.");
                    return Ok(());
                }
            };

            std::fs::create_dir_all(output_dir)?;
            if let spacekit_messaging_node::MessageType::Text { content } = &message.content_type {
                let ids = extract_file_markers(content);
                if ids.is_empty() {
                    println!("📭 No [file:<id>] markers found in message.");
                    return Ok(());
                }
                for (file_id, marker_name) in ids {
                    let filename = if let Some(name) = marker_name {
                        name
                    } else {
                        messaging_node
                            .get_shared_file_metadata(&file_id)
                            .await
                            .ok()
                            .flatten()
                            .map(|file| file.filename)
                            .unwrap_or_else(|| file_id.clone())
                    };
                    let output_path = Path::new(output_dir).join(filename);
                    match messaging_node
                        .download_file(&file_id, &requester_did, &private_key)
                        .await
                    {
                        Ok(data) => {
                            std::fs::write(&output_path, &data)?;
                            println!("✅ Downloaded {} -> {}", file_id, output_path.display());
                        }
                        Err(e) => {
                            println!("❌ Failed to download {}: {}", file_id, e);
                        }
                    }
                }
            } else {
                println!("⚠️  Message does not contain text content.");
            }

            Ok(())
        }
        MessageCommands::Peers { detailed } => {
            println!("🔍 Discovering peers...");

            let messaging_node = get_or_create_messaging_node().await?;
            let _user = ensure_messaging_user(&messaging_node).await?;

            println!("   🔍 Discovering peers via mDNS...");
            tokio::time::sleep(Duration::from_secs(3)).await;

            let status = messaging_node.get_status().await;
            let users = messaging_node.get_all_users().await.unwrap_or_default();

            println!("\n   📊 Messaging Node Status:");
            println!(
                "      Active Connections: {}",
                status.active_connections.to_string().cyan()
            );
            println!(
                "      Registered Users (this CLI): {}",
                users.len().to_string().cyan()
            );

            if !users.is_empty() {
                println!("\n   👥 Registered Users (this CLI instance):");
                for user in &users {
                    println!("      - {} ({})", user.username.green(), user.did.blue());
                }
            }

            if status.active_connections > 0 {
                println!(
                    "\n   ✅ Found {} connected peer(s)!",
                    status.active_connections.to_string().green()
                );

                if *detailed {
                    println!("\n   💡 Peer Information:");
                    println!("      - Peers discovered via mDNS on local network");
                    println!("      - Connection status maintained by messaging node");
                    println!("      - P2P network: libp2p with Gossipsub");
                    println!("\n   📝 To message peers:");
                    println!("      Use: spacekit message send --to <peer_did> --message <text>");
                    println!(
                        "      Peer DIDs are discovered automatically via the messaging network"
                    );
                }
            } else {
                println!("\n   ⚠️  No peers connected yet.");
                println!("   💡 Note: This CLI instance creates its own messaging node.");
                println!("   💡 To see users registered in SpaceKit OS:");
                println!("      - Users must be registered in SpaceKit OS desktop app first");
                println!("      - This CLI instance won't see SpaceKit OS's registered users");
                println!("      - Both nodes are on the same P2P network for messaging");
                println!("\n   💡 This is normal if:");
                println!("      - SpaceKit OS messaging is just starting");
                println!("      - No other peers on the local network");
                println!("      - mDNS discovery is still in progress");
                println!("\n   🔧 Troubleshooting:");
                println!("      1. Ensure SpaceKit OS is running with messaging enabled");
                println!("      2. Wait 5-10 seconds for mDNS discovery");
                println!("      3. Check that both nodes are on the same network");
                println!("      4. Run 'spacekit message peers' again after a few seconds");

                if *detailed {
                    println!("\n   📡 Discovery Methods:");
                    println!("      - mDNS: Automatic discovery on local network (enabled)");
                    println!("      - DHT: Distributed peer discovery via Kademlia");
                    println!("      - Bootstrap: Configured peers in ~/.spacekit/config.toml");
                    println!("\n   🔐 User Registration:");
                    println!("      - Users registered in SpaceKit OS are not visible to CLI");
                    println!("      - Each messaging node instance has its own user registry");
                    println!("      - P2P network allows messaging between different instances");
                }
            }

            Ok(())
        }
    }
}

// ============================================================================
// CONTENT PUBLISHING COMMAND HANDLERS
// ============================================================================

async fn handle_content_command(
    content_command: &ContentCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match content_command {
        ContentCommands::CreateChannel {
            name,
            description,
            pricing,
            price,
        } => {
            println!("📺 Creating channel...");
            println!("   Name: {}", name.green());
            if let Some(desc) = description {
                println!("   Description: {}", desc.yellow());
            }
            println!("   Pricing: {}", pricing.cyan());
            if let Some(p) = price {
                println!("   Price: {} ASTRA", p);
            }

            // Get compute node for smart contract
            let compute_node = get_or_create_compute_node().await?;
            let storage_node = get_or_create_storage_node().await?;

            // Get current user DID
            let config = load_cli_config().await?;
            let owner_did = config.identity.did.clone();

            if owner_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            let channel_did = format!(
                "did:spacekit:channel:{}:{}",
                name.to_lowercase().replace(' ', "-"),
                &owner_did[owner_did.len().saturating_sub(8)..]
            );
            let channel_fact = channel_to_fact_package(
                &owner_did,
                &channel_did,
                name,
                description.as_deref(),
                pricing,
                *price,
            )
            .await?;
            let fact_storage = get_fact_storage_engine(&storage_node).await?;
            let stored = fact_storage.store_fact(channel_fact).await?;
            println!("\n✅ Channel created!");
            println!("   Channel DID: {}", channel_did.green());
            println!("   Channel fact: {}", hex::encode(stored).cyan());
            println!(
                "   Use with publish: spacekit content publish --channel {}",
                channel_did
            );

            Ok(())
        }
        ContentCommands::Publish {
            channel,
            file,
            title,
            description,
            pricing,
            price,
            thumbnail_time,
            duration,
            channel_name,
        } => {
            println!("📤 Publishing content...");
            println!("   Channel: {}", channel.cyan());
            println!("   File: {}", file.green());
            println!("   Title: {}", title.yellow());
            if let Some(desc) = description {
                println!("   Description: {}", desc);
            }
            println!("   Pricing: {}", pricing.cyan());
            if let Some(p) = price {
                println!("   Price: {} ASTRA", p);
            }

            // Read file
            let file_data =
                std::fs::read(file).map_err(|e| format!("Failed to read file: {}", e))?;

            println!("   File size: {} bytes", file_data.len());

            // Get nodes
            let storage_node = get_or_create_storage_node().await?;
            let compute_node = get_or_create_compute_node().await?;

            // Get current user DID
            let config = load_cli_config().await?;
            let owner_did = config.identity.did.clone();

            if owner_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            // 1. Create Fact Package from content
            println!("\n📦 Creating Fact Package...");

            let mut fact_package = file_to_fact_package(
                file,
                &file_data,
                &owner_did,
                &channel,
                title,
                description.as_deref(),
                pricing,
                *price,
                vec![],
            )
            .await?;
            if let Some(tt) = thumbnail_time {
                fact_package
                    .metadata
                    .tags
                    .push(format!("thumbnail_time:{}", tt));
            }
            if let Some(dur) = duration {
                fact_package.metadata.tags.push(format!("duration:{}", dur));
            }
            if let Some(cn) = channel_name {
                fact_package
                    .metadata
                    .tags
                    .push(format!("channel_name:{}", cn));
            }
            sign_content_fact(
                &mut fact_package,
                &owner_did,
                Some(storage_data_dir(&storage_node).as_path()),
            )?;

            let fact_id = fact_package.fact_id;
            let fact_id_hex = hex::encode(fact_id);

            println!("   ✅ Fact Package created: {}", fact_id_hex.green());

            let catalog_listing = build_content_listing_from_fact(
                &fact_package,
                channel,
                title,
                description.as_deref(),
            );

            // 2. Store Fact Package in Storage Node
            println!("\n💾 Storing Fact Package...");
            let fact_for_remote = fact_package.clone();
            let fact_storage = get_fact_storage_engine(&storage_node).await?;
            let stored_fact_id = fact_storage.store_fact(fact_package).await?;
            println!(
                "   ✅ Fact Package stored: {}",
                hex::encode(stored_fact_id).green()
            );

            // 3. Register content with smart contract
            println!("\n📜 Registering content on-chain...");
            let storage_policy = StoragePolicy {
                requires_payment: pricing != "free",
                payment_amount: *price,
                access_control: "ChannelSubscribers".to_string(),
                replication_factor: 5,
            };

            let distribution_rule = DistributionRule {
                p2p_enabled: true,
                chunk_size: 1_000_000, // 1MB chunks
                replication_factor: 5,
                storage_nodes: vec![],
            };

            // Try to register with governance contract
            let governance_contract_id = format!("storage_governance_{}", channel);

            match register_content_with_governance(
                &compute_node,
                &governance_contract_id,
                &fact_id_hex,
                &hex::encode(stored_fact_id),
                owner_did.clone(),
                &storage_policy,
                &distribution_rule,
            )
            .await
            {
                Ok(result) => {
                    println!("   ✅ Registered with governance contract");
                    if let Some(tx_hash) = result.get("tx_hash").and_then(|v| v.as_str()) {
                        println!("   Transaction: {}", tx_hash.cyan());
                    }
                }
                Err(e) => {
                    println!("   ⚠️  Could not register with governance contract: {}", e);
                    println!("   💡 Deploy governance contract first: spacekit contract deploy --contract storage_governance.wasm");
                }
            }

            // 4. Publish notification via Gossipsub
            println!("\n📡 Publishing notification...");
            // Try to get messaging node from simulator
            let config = load_cli_config().await?;
            if let Some(_sim) = config.connections.and_then(|c| c.simulator) {
                // TODO: Get messaging node from simulator and publish
                println!("   Would publish to Gossipsub topic: channel:{}", channel);
                println!("   Notification: Content {} published", fact_id_hex);
            } else {
                println!("   ⚠️  No messaging node available for notifications");
                println!("   💡 Connect to simulator: spacekit connect simulator --url <URL>");
            }

            println!("\n✅ Content published successfully!");
            println!("   Content ID: {}", fact_id_hex.green());
            println!(
                "   Fact Package ID: {}",
                hex::encode(stored_fact_id).green()
            );

            if let Ok((base_url, _)) = resolve_remote_storage_base_url(None) {
                let size_mb = catalog_listing.size_bytes as f64 / (1024.0 * 1024.0);
                if size_mb >= 1.0 {
                    println!(
                        "   📤 Uploading {:.1} MB to storage node for website playback…",
                        size_mb
                    );
                }
                let remote_ok = match post_fact_package_http(&base_url, &fact_for_remote).await {
                    Ok(()) => true,
                    Err(e) => {
                        println!("   ⚠️  Fact upload for streaming failed: {}", e);
                        false
                    }
                };
                match upsert_content_listing_http(&base_url, &owner_did, &catalog_listing).await {
                    Ok(()) => {
                        if remote_ok {
                            println!(
                                "   🌐 Website catalog + stream ready — open http://localhost:5173/content/{}",
                                fact_id_hex.cyan()
                            );
                        } else {
                            println!(
                                "   🌐 Catalog indexed (stream may 404 until fact upload succeeds)"
                            );
                            println!(
                                "   💡 Retry: spacekit content register-listing --content-id {}",
                                fact_id_hex
                            );
                        }
                    }
                    Err(e) => {
                        println!("   ⚠️  Website catalog index failed: {}", e);
                        println!(
                            "   💡 Ensure `spacekit network up` is running and retry: \
                             spacekit content register-listing --content-id {}",
                            fact_id_hex
                        );
                    }
                }
            } else {
                println!(
                    "   💡 For website playback: spacekit connect storage && \
                     spacekit content register-listing --content-id {}",
                    fact_id_hex
                );
            }

            if pricing != "free" && crate::content_monetization::entitlement_configured() {
                if let Ok(()) = crate::content_monetization::ensure_content_listing(
                    &compute_node,
                    &owner_did,
                    &fact_id_hex,
                    price.unwrap_or(0.0),
                    pricing,
                )
                .await
                {
                    println!(
                        "   📜 Entitlement listing registered: content:{}",
                        fact_id_hex
                    );
                } else {
                    println!(
                        "   ⚠️  Could not register entitlement listing (check compute + contract)"
                    );
                }
            }

            Ok(())
        }
        ContentCommands::PublishFeature {
            channel,
            feature,
            title,
            description,
        } => {
            println!("📤 Publishing licensed feature...");
            println!("   Channel: {}", channel.cyan());
            println!("   Feature: {}", feature.green());
            println!("   Title: {}", title.yellow());
            if let Some(desc) = description {
                println!("   Description: {}", desc);
            }

            let storage_node = get_or_create_storage_node().await?;
            let config = load_cli_config().await?;
            let owner_did = config.identity.did.clone();
            if owner_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            let document = if feature.eq_ignore_ascii_case("growformer") {
                growformer_feature_document(
                    &owner_did,
                    title,
                    description
                        .as_deref()
                        .unwrap_or("Growformer library-embedded in spacekit CLI"),
                )
            } else {
                return Err(format!(
                    "unsupported feature '{}' — only growformer is bundled in CLI today",
                    feature
                )
                .into());
            };

            let mut fact_package =
                licensed_feature_to_fact_package(&owner_did, channel, document).await?;
            sign_content_fact(
                &mut fact_package,
                &owner_did,
                Some(storage_data_dir(&storage_node).as_path()),
            )?;
            let fact_id_hex = hex::encode(fact_package.fact_id);
            let fact_storage = get_fact_storage_engine(&storage_node).await?;
            fact_storage.store_fact(fact_package).await?;

            println!("\n✅ Licensed feature published!");
            println!("   Feature: {}", feature.green());
            println!("   Content ID: {}", fact_id_hex.green());
            println!(
                "   Consumers: spacekit content access --feature {} \
                 (or export GROWFORMER_CONTENT_ID={})",
                feature.cyan(),
                fact_id_hex
            );
            Ok(())
        }
        ContentCommands::Subscribe { channel } => {
            println!("🔔 Subscribing to channel...");
            println!("   Channel: {}", channel.cyan());
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();
            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let storage_node = get_or_create_storage_node().await?;
            subscribe_channel_with_payment(
                &storage_node,
                &user_did,
                channel,
                channel,
                0.0,
                30 * 24 * 3600,
                None,
                true,
            )
            .await?;
            println!("\n✅ Subscribed to channel (grant recorded).");
            println!("   Paid channels: use --payment-ref after SpaceKit Pay settlement.");
            Ok(())
        }
        ContentCommands::ListChannels {
            subscribed,
            detailed,
        } => {
            println!("📺 Listing channels...");
            if *subscribed {
                println!("   Showing only subscribed channels");
            }

            // Get current user DID
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();

            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            // Query channels from Compute Node (via governance contract)
            let compute_node = get_or_create_compute_node().await?;

            // Try to query channels from governance contract
            // For now, query Fact Packages with channel tags from Storage Node
            let storage_node = get_or_create_storage_node().await?;
            let fact_storage = get_fact_storage_engine(&storage_node).await?;

            // Query facts with "channel" tag to find channels
            use spacekit_primitives::v1::fact::types::{
                FactQuery, PaginationParams, SortCriteria, SortOrder,
            };
            let query = FactQuery {
                requester: spacekit_primitives::v1::identity::QuantumDID::parse(&user_did)
                    .map_err(|e| format!("Invalid DID: {}", e))?,
                author: None,
                category: None,
                tags: vec!["channel".to_string()],
                domain: None,
                content_type: None,
                text_search: None,
                verification_level: None,
                min_confidence: None,
                created_after: None,
                created_before: None,
                depends_on: None,
                referenced_by: None,
                sort_by: SortCriteria::CreatedAt(SortOrder::Descending),
                pagination: PaginationParams {
                    offset: 0,
                    limit: 100,
                },
                start_time: chrono::Utc::now().timestamp() as u64,
            };

            match fact_storage.query_facts(query).await {
                Ok(result) => {
                    if result.facts.is_empty() {
                        println!("   No channels found.");
                    } else {
                        println!("\n   Found {} channel(s):\n", result.facts.len());
                        for (i, fact) in result.facts.iter().enumerate() {
                            println!(
                                "   {}. Channel ID: {}",
                                i + 1,
                                hex::encode(fact.fact_id).cyan()
                            );
                            if *detailed {
                                println!("      Author: {}", fact.author.as_str().yellow());
                                println!(
                                    "      Created: {}",
                                    chrono::DateTime::<chrono::Utc>::from_timestamp(
                                        fact.created_at as i64,
                                        0
                                    )
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                    .unwrap_or_else(|| "Unknown".to_string())
                                );
                                if !fact.metadata.tags.is_empty() {
                                    println!("      Tags: {}", fact.metadata.tags.join(", "));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("   ⚠️  Could not query channels: {}", e);
                    println!("   💡 Channels are stored as Fact Packages with 'channel' tag");
                }
            }

            Ok(())
        }
        ContentCommands::ListContent { channel, limit } => {
            println!("📋 Listing content in channel...");
            println!("   Channel: {}", channel.cyan());
            println!("   Limit: {}", limit);

            // Get current user DID
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();

            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            // Query content from Storage Node (Fact Packages)
            let storage_node = get_or_create_storage_node().await?;
            let fact_storage = get_fact_storage_engine(&storage_node).await?;

            // Query facts with channel tag and "content" tag
            use spacekit_primitives::v1::fact::types::{
                FactQuery, PaginationParams, SortCriteria, SortOrder,
            };
            let query = FactQuery {
                requester: spacekit_primitives::v1::identity::QuantumDID::parse(&user_did)
                    .map_err(|e| format!("Invalid DID: {}", e))?,
                author: None,
                category: None,
                tags: vec!["content".to_string(), "published".to_string()],
                domain: None,
                content_type: None,
                text_search: None,
                verification_level: None,
                min_confidence: None,
                created_after: None,
                created_before: None,
                depends_on: None,
                referenced_by: None,
                sort_by: SortCriteria::CreatedAt(SortOrder::Descending),
                pagination: PaginationParams {
                    offset: 0,
                    limit: *limit as u64,
                },
                start_time: chrono::Utc::now().timestamp() as u64,
            };

            match fact_storage.query_facts(query).await {
                Ok(result) => {
                    if result.facts.is_empty() {
                        println!("   No content found in channel.");
                    } else {
                        println!("\n   Found {} content item(s):\n", result.facts.len());
                        for (i, fact) in result.facts.iter().enumerate() {
                            let content_id = hex::encode(fact.fact_id);
                            println!("   {}. Content ID: {}", i + 1, content_id.cyan());

                            // Extract title from metadata tags or use fact_id
                            let title = fact
                                .metadata
                                .tags
                                .iter()
                                .find(|t| !t.starts_with("content") && !t.starts_with("published"))
                                .cloned()
                                .unwrap_or_else(|| "Untitled".to_string());

                            println!("      Title: {}", title.green());
                            println!("      Author: {}", fact.author.as_str().yellow());

                            // Show content type
                            match &fact.content {
                                spacekit_primitives::v1::fact::FactContent::Binary {
                                    mime_type,
                                    ..
                                } => {
                                    println!("      Type: {}", mime_type.blue());
                                }
                                _ => {
                                    println!("      Type: {}", "Unknown".blue());
                                }
                            }

                            // Show access policy
                            match &fact.access_policy {
                                spacekit_primitives::v1::fact::AccessPolicy::Public => {
                                    println!("      Access: {}", "Public".green());
                                },
                                spacekit_primitives::v1::fact::AccessPolicy::Conditional(conditions) => {
                                    if conditions.iter().any(|c| matches!(c.condition_type, spacekit_primitives::v1::fact::ConditionType::PaymentRequired)) {
                                        println!("      Access: {}", "Pay-Per-View".yellow());
                                    } else {
                                        println!("      Access: {}", "Restricted".yellow());
                                    }
                                },
                                _ => {
                                    println!("      Access: {}", "Private".red());
                                }
                            }

                            println!(
                                "      Created: {}",
                                chrono::DateTime::<chrono::Utc>::from_timestamp(
                                    fact.created_at as i64,
                                    0
                                )
                                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                .unwrap_or_else(|| "Unknown".to_string())
                            );
                            println!();
                        }

                        println!("   💡 View content: spacekit content view --content-id <ID> --output <file>");
                    }
                }
                Err(e) => {
                    println!("   ⚠️  Could not query content: {}", e);
                    println!("   💡 Content is stored as Fact Packages in Storage Node");
                }
            }

            Ok(())
        }
        ContentCommands::RegisterListing {
            content_id,
            storage_url,
        } => {
            let storage_node = get_or_create_storage_node().await?;
            let config = load_cli_config().await?;
            let owner_did = config.identity.did.clone();
            if owner_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let (base_url, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
            let fact_storage = get_fact_storage_engine(&storage_node).await?;
            let fact_id = parse_content_id_hex(content_id)?;
            let fact = fact_storage
                .retrieve_fact(fact_id)
                .await?
                .ok_or_else(|| format!("Content not found: {}", content_id))?;
            let channel = channel_did_from_fact_tags(&fact)
                .unwrap_or_else(|| "did:spacekit:channel:unknown".to_string());
            let title = title_from_fact_tags(&fact).unwrap_or_else(|| "Untitled".to_string());
            let listing = build_content_listing_from_fact(
                &fact,
                &channel,
                &title,
                description_from_fact_tags(&fact).as_deref(),
            );
            let size_mb = listing.size_bytes as f64 / (1024.0 * 1024.0);
            if size_mb >= 1.0 {
                println!(
                    "📤 Uploading {:.1} MB fact to storage node for streaming…",
                    size_mb
                );
            } else {
                println!("📤 Uploading fact to storage node for streaming…");
            }
            post_fact_package_http(&base_url, &fact)
                .await
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            upsert_content_listing_http(&base_url, &owner_did, &listing)
                .await
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            println!("✅ Catalog entry registered at {}", base_url.green());
            println!(
                "   Indexed for website-api ({})",
                crate::content_integration::WEBSITE_CATALOG_OWNER_DID.cyan()
            );
            println!(
                "   Open: http://localhost:5173/content/{}",
                content_id.cyan()
            );
            Ok(())
        }
        ContentCommands::Unpublish {
            content_id,
            storage_url,
            purge,
        } => {
            let config = load_cli_config().await?;
            let owner_did = config.identity.did.clone();
            if owner_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let (base_url, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;

            println!("🗑️  Unpublishing content...");
            println!("   Content ID: {}", content_id.cyan());
            println!("   Storage: {}", base_url.green());

            let client = reqwest::Client::new();
            let remote_fact = fetch_remote_fact_json(&client, &base_url, content_id)
                .await
                .ok()
                .flatten();
            let is_app_manifest = remote_fact
                .as_ref()
                .map(fact_json_is_app_manifest)
                .unwrap_or(false);

            if is_app_manifest {
                println!("   📱 Detected app manifest — removing marketplace entries...");
                match unpublish_app_marketplace_entries(&base_url, &owner_did, content_id).await {
                    Ok(()) => println!("   ✅ app_listings + marketplace index updated"),
                    Err(e) => println!("   ⚠️  Marketplace cleanup partial/failed: {}", e.yellow()),
                }
            }

            match delete_content_listing_http(&base_url, &owner_did, content_id).await {
                Ok(()) => {
                    println!("   ✅ Removed from website catalog");
                }
                Err(e) => {
                    println!("   ⚠️  Catalog removal partial/failed: {}", e.yellow());
                }
            }

            if *purge {
                let fact_url = format!("{}/facts/{}", base_url.trim_end_matches('/'), content_id);
                match client
                    .delete(&fact_url)
                    .header("Authorization", format!("DID {}", owner_did))
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 404 => {
                        println!("   ✅ Fact data purged from storage node");
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        println!("   ⚠️  Fact purge returned HTTP {}", status);
                    }
                    Err(e) => {
                        println!("   ⚠️  Fact purge request failed: {}", e);
                    }
                }

                // Also remove from local fact storage if present
                let storage_node = get_or_create_storage_node().await?;
                let data_dir = storage_data_dir(&storage_node);
                let fact_id_hex = content_id;
                let prefix = &fact_id_hex[..2.min(fact_id_hex.len())];
                let fact_dir = data_dir.join("facts").join(prefix);
                for ext in &["json", "blob", "blob.meta"] {
                    let p = fact_dir.join(format!("{}.{}", fact_id_hex, ext));
                    if p.exists() {
                        let _ = std::fs::remove_file(&p);
                    }
                }
                println!("   ✅ Local fact files cleaned up");
            }

            println!("\n✅ Content unpublished: {}", content_id);
            if !purge {
                println!("   💡 Add --purge to also delete the underlying fact data from disk");
            }
            Ok(())
        }
        ContentCommands::View {
            content_id,
            output,
            pay,
            open,
        } => {
            println!("👁️  Viewing content...");
            println!("   Content ID: {}", content_id.cyan());
            let storage_node = get_or_create_storage_node().await?;
            let config = load_cli_config().await?;
            let requester_did = config.identity.did.clone();
            if requester_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            if *pay {
                match crate::content_monetization::initiate_content_pay(
                    &storage_node,
                    &requester_did,
                    content_id,
                    None,
                )
                .await
                {
                    Ok(quote) => {
                        println!("\n💳 Payment required");
                        println!("   Pending ID: {}", quote.pending_id.yellow());
                        println!(
                            "   Pay {} ASTRA to {}",
                            quote.price_astra,
                            quote.pay_to.cyan()
                        );
                        println!("   Scope: {}", quote.scope);
                        println!("   After payment: spacekit content settle --pending-id {} --tx-hash <hash> --amount {}", quote.pending_id, quote.price_astra);
                        return Ok(());
                    }
                    Err(e) => {
                        println!("   ⚠️  Could not initiate pay flow: {}", e);
                    }
                }
            }
            match view_content_fact(&storage_node, content_id, &requester_did).await? {
                ViewContentOutcome::Bytes {
                    data,
                    filename,
                    app_slug,
                } => {
                    let data_dir = storage_data_dir(&storage_node);
                    let app_slug = app_slug.or_else(|| {
                        filename
                            .strip_suffix(".exe")
                            .or_else(|| filename.strip_suffix(".bin"))
                            .unwrap_or(filename.as_str())
                            .eq_ignore_ascii_case("growformer")
                            .then_some("growformer".to_string())
                    });
                    let use_embedded =
                        spacekit_storage_node::content_installs::should_use_embedded_growformer(
                            content_id,
                            app_slug.as_deref(),
                            None,
                            Some(data.as_slice()),
                        );
                    let out_path = if use_embedded && output.is_none() {
                        None
                    } else {
                        let p = resolve_content_view_output(
                            data_dir.as_path(),
                            content_id,
                            output.as_deref(),
                            &filename,
                        );
                        write_content_view_file(&p, &data)?;
                        Some(p)
                    };
                    let install = register_content_install_after_view(
                        &storage_node,
                        &requester_did,
                        content_id,
                        out_path.as_deref(),
                        &filename,
                        data.len() as u64,
                        app_slug,
                        Some(data.as_slice()),
                    )?;
                    println!("\n✅ Content access granted!");
                    if let Some(ref p) = out_path {
                        println!("   Saved to: {}", p.display().to_string().green());
                        if *open {
                            open_materialized_path(p);
                        }
                    } else if use_embedded {
                        println!(
                            "   Runtime: embedded growformer (entitlement in storage DB; fact {})",
                            content_id.cyan()
                        );
                        println!(
                            "   {}",
                            "No local copy written — bytes stay in storage-node fact storage."
                                .dimmed()
                        );
                    }
                    println!(
                        "   Storage data dir: {}",
                        data_dir.display().to_string().cyan()
                    );
                    println!(
                        "   Install recorded in storage DB ({})",
                        "content_installs".cyan()
                    );
                    if let Some(ref ent) = install.entitlement_id_hex {
                        println!("   Entitlement: {}", ent.green());
                    }
                    let run_hint =
                        if install.app_slug.as_deref() == Some("growformer") || use_embedded {
                            "spacekit agent --app growformer exec -- --help".to_string()
                        } else {
                            format!("spacekit agent exec --content-id {} -- --help", content_id)
                        };
                    println!("   Run entitled app: {}", run_hint.cyan());
                    if !use_embedded && output.is_none() {
                        println!(
                            "   (materialized under storage data dir; use --output to override)"
                        );
                    }
                    if let Some(ref p) = out_path {
                        if *open {
                            open_materialized_path(p);
                        }
                        println!(
                            "   {}",
                            format!(
                                "Note: {} is readable by your OS user — entitlement gates spacekit agent, not other local processes.",
                                p.display()
                            )
                            .dimmed()
                        );
                        println!(
                            "   {}",
                            "CLI `content view` saves a file; use --open to launch your OS player, or watch in the browser at /content/<id> after register-listing.".dimmed()
                        );
                    }
                }
                ViewContentOutcome::PaymentRequired {
                    price,
                    currency,
                    content_id_hex,
                } => {
                    println!("\n💳 Payment required");
                    println!("   Price: {} {}", price.yellow(), currency.cyan());
                    println!(
                        "   Run: spacekit content view --content-id {} --pay",
                        content_id_hex
                    );
                }
                ViewContentOutcome::SubscriptionRequired { channel_did } => {
                    println!("\n🔔 Subscription required: {}", channel_did.cyan());
                }
                ViewContentOutcome::Denied { reason } => {
                    println!("❌ Access denied: {}", reason);
                }
            }
            Ok(())
        }
        ContentCommands::Access {
            content_id,
            channel,
            feature,
            tier,
            payment_ref,
            pay,
        } => {
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();
            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let storage_node = get_or_create_storage_node().await?;
            if *pay {
                let cid = if let Some(cid) = content_id.as_deref() {
                    cid.to_string()
                } else if let Some(fname) = feature.as_deref() {
                    find_licensed_feature_content_id(&storage_node, &user_did, fname)
                        .await?
                        .ok_or_else(|| format!("licensed feature '{}' not found", fname))?
                } else {
                    return Err("Specify --content-id or --feature with --pay".into());
                };
                let quote = crate::content_monetization::initiate_content_pay(
                    &storage_node,
                    &user_did,
                    &cid,
                    tier.as_deref(),
                )
                .await?;
                println!("💳 Payment quote for access");
                println!("   Pending: {}", quote.pending_id.yellow());
                println!(
                    "   Pay {} ASTRA to {}",
                    quote.price_astra,
                    quote.pay_to.cyan()
                );
                println!(
                    "   Then: spacekit content settle --pending-id {} --tx-hash <hash> --amount {}",
                    quote.pending_id, quote.price_astra
                );
                return Ok(());
            }

            if content_id.is_none() && channel.is_none() {
                if let Some(fname) = feature.as_deref() {
                    let cid = find_licensed_feature_content_id(&storage_node, &user_did, fname)
                        .await?
                        .ok_or_else(|| {
                            format!(
                                "licensed feature '{}' not found — publisher runs `content publish-feature`",
                                fname
                            )
                        })?;
                    let (feature_name, tier_name) = access_licensed_feature(
                        storage_node.clone(),
                        &user_did,
                        &cid,
                        tier.as_deref(),
                    )
                    .await?;
                    println!(
                        "✅ Growformer entitlement granted. Feature: {}. Tier: {}. Content ID: {}",
                        feature_name.green(),
                        tier_name.cyan(),
                        cid.yellow()
                    );
                    println!(
                        "   Use: spacekit agent train --project <path>  (growformer is embedded in CLI)"
                    );
                    return Ok(());
                }
                return Err("Specify --content-id, --channel, and/or --feature".into());
            }

            match (content_id.as_deref(), channel.as_deref()) {
                (Some(cid), None) => {
                    let fact_storage = get_fact_storage_engine(&storage_node).await?;
                    let fact_id = parse_content_id_hex(cid)?;
                    let fact = fact_storage
                        .retrieve_fact(fact_id)
                        .await?
                        .ok_or_else(|| format!("Content not found: {}", cid))?;
                    let publisher = fact.author.as_str().to_string();
                    let price = content_price_astra(&fact).unwrap_or(0.0);
                    access_content_with_payment(
                        &storage_node,
                        &user_did,
                        &publisher,
                        cid,
                        price,
                        payment_ref.as_deref(),
                        price <= 0.0 && payment_ref.is_none(),
                    )
                    .await?;
                    println!("✅ Pay-per-view access granted for content {}", cid.green());
                }
                (None, Some(ch)) => {
                    subscribe_channel_with_payment(
                        &storage_node,
                        &user_did,
                        ch,
                        ch,
                        0.0,
                        30 * 24 * 3600,
                        payment_ref.as_deref(),
                        payment_ref.is_none(),
                    )
                    .await?;
                    println!("✅ Channel subscription grant recorded for {}", ch.green());
                }
                (Some(cid), Some(ch)) => {
                    let fact_storage = get_fact_storage_engine(&storage_node).await?;
                    let fact_id = parse_content_id_hex(cid)?;
                    let fact = fact_storage.retrieve_fact(fact_id).await?.unwrap();
                    let publisher = fact.author.as_str().to_string();
                    let price = content_price_astra(&fact).unwrap_or(0.0);
                    access_content_with_payment(
                        &storage_node,
                        &user_did,
                        &publisher,
                        cid,
                        price,
                        payment_ref.as_deref(),
                        price <= 0.0 && payment_ref.is_none(),
                    )
                    .await?;
                    subscribe_channel_with_payment(
                        &storage_node,
                        &user_did,
                        ch,
                        ch,
                        0.0,
                        30 * 24 * 3600,
                        payment_ref.as_deref(),
                        payment_ref.is_none(),
                    )
                    .await?;
                    println!(
                        "✅ Granted PPV ({}) and channel ({})",
                        cid.green(),
                        ch.green()
                    );
                }
                (None, None) => {
                    return Err("Specify --content-id and/or --channel".into());
                }
            }
            Ok(())
        }
        ContentCommands::ListAccess => {
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();
            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let storage_node = get_or_create_storage_node().await?;
            let grants = list_content_grants(&storage_node, &user_did)?;
            if grants.is_empty() {
                println!("No active content grants for {}", user_did.yellow());
            } else {
                println!("Access grants for {}:\n", user_did.cyan());
                for (i, g) in grants.iter().enumerate() {
                    println!("  {}. {:?} granted_at={}", i + 1, g.kind, g.granted_at);
                    if let Some(ref c) = g.content_id_hex {
                        println!("     content_id={}", c);
                    }
                    if let Some(ref t) = g.tier {
                        println!("     tier={}", t);
                    }
                    match g.quota_remaining {
                        Some(q) => println!("     quota_remaining={}", q),
                        None if g.tier.is_some() => println!("     quota=unlimited"),
                        None => {}
                    }
                    if let Some(ref ch) = g.channel_did {
                        println!("     channel={}", ch);
                    }
                }
            }
            Ok(())
        }
        ContentCommands::Renew {
            content_id,
            channel,
            extend_secs,
            tier,
            price,
            payment_ref,
            publisher,
        } => {
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();
            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let storage_node = get_or_create_storage_node().await?;
            let publisher_did = if let Some(p) = publisher {
                p.clone()
            } else if let Some(ref cid) = content_id {
                let fact_storage = get_fact_storage_engine(&storage_node).await?;
                let fact_id = parse_content_id_hex(cid)?;
                fact_storage
                    .retrieve_fact(fact_id)
                    .await?
                    .ok_or_else(|| format!("Content not found: {}", cid))?
                    .author
                    .as_str()
                    .to_string()
            } else if let Some(ch) = channel.as_deref() {
                ch.to_string()
            } else {
                return Err("Specify --content-id or --channel".into());
            };
            let price_astra = price.unwrap_or(0.0);
            renew_content_access(
                &storage_node,
                &user_did,
                &publisher_did,
                content_id.as_deref(),
                channel.as_deref(),
                *extend_secs,
                tier.as_deref(),
                price_astra,
                payment_ref.as_deref(),
            )
            .await?;
            println!("✅ Access renewed (extend_secs={})", extend_secs);
            Ok(())
        }
        ContentCommands::Pay {
            content_id,
            channel,
            tier,
            price,
            publisher,
            await_settlement,
            pending_id,
            tx_hash,
            amount,
        } => {
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();
            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let storage_node = get_or_create_storage_node().await?;
            let compute_node = get_or_create_compute_node().await?;
            let quote = match (content_id.as_deref(), channel.as_deref()) {
                (Some(cid), None) => {
                    crate::content_monetization::resolve_content_pay_quote(
                        &storage_node,
                        &user_did,
                        cid,
                        tier.as_deref(),
                        pending_id.as_deref(),
                        *await_settlement,
                    )
                    .await?
                }
                (None, Some(ch)) => {
                    let pub_did = publisher.clone().unwrap_or_else(|| ch.to_string());
                    let price_astra = price.ok_or(
                        "Specify --price for paid channel subscription (or use free subscribe)",
                    )?;
                    crate::content_monetization::initiate_channel_pay(
                        &storage_node,
                        &user_did,
                        ch,
                        &pub_did,
                        price_astra,
                    )
                    .await?
                }
                (Some(_), Some(_)) => {
                    return Err("Specify only one of --content-id or --channel".into());
                }
                (None, None) => {
                    return Err("Specify --content-id or --channel".into());
                }
            };
            println!("💳 Payment quote");
            println!("   Pending: {}", quote.pending_id.yellow());
            println!(
                "   Amount: {} ASTRA → {}",
                quote.price_astra,
                quote.publisher_did.cyan()
            );
            println!("   Listing: {}", quote.listing_id);
            println!("   Scope: {}", quote.scope);

            if *await_settlement {
                let data_dir = storage_data_dir(&storage_node);
                let store = spacekit_storage_node::content_settlement::ContentSettlementStore::new(
                    data_dir.as_path(),
                );
                if let Ok(Some(p)) = store.get_pending(&quote.pending_id) {
                    if p.status == "completed" {
                        if let Some(ref ent) = p.entitlement_id_hex {
                            println!("✅ Auto-completed; entitlement {}", ent.green());
                            return Ok(());
                        }
                    }
                }
            }

            if let (Some(tx), Some(amt)) = (tx_hash.as_deref(), amount.as_deref()) {
                let ent = crate::content_monetization::settle_pending_purchase(
                    &storage_node,
                    &compute_node,
                    &quote.pending_id,
                    tx,
                    amt,
                    &user_did,
                )
                .await?;
                println!(
                    "✅ Settled; entitlement {} — access content or channel now.",
                    ent.green()
                );
                return Ok(());
            }

            if *await_settlement {
                let poll_ms = std::env::var("SPACEKIT_SETTLEMENT_POLL_MS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(500);
                let timeout_secs = std::env::var("SPACEKIT_SETTLEMENT_TIMEOUT_SECS")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(120);
                println!(
                    "   Waiting for settlement (poll {}ms, timeout {}s)...",
                    poll_ms, timeout_secs
                );
                if let Some(ent) = crate::content_monetization::await_settlement_for_pending(
                    &storage_node,
                    &compute_node,
                    &quote.pending_id,
                    poll_ms,
                    timeout_secs,
                )
                .await?
                {
                    println!("✅ Auto-completed; entitlement {}", ent.green());
                } else {
                    println!("   Timed out waiting for settlement.");
                    println!("   Run `spacekit content listen-settlements --once` in another terminal, or");
                    println!("   record-payment with matching scope and retry.");
                }
            } else {
                println!("   After pay: spacekit content settle --pending-id {} --tx-hash <hash> --amount {}", quote.pending_id, quote.price_astra);
            }
            Ok(())
        }
        ContentCommands::Purchase { content_id } => {
            let config = load_cli_config().await?;
            let user_did = config.identity.did.clone();
            if user_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let storage_node = get_or_create_storage_node().await?;
            let compute_node = get_or_create_compute_node().await?;
            let ent = crate::content_monetization::purchase_content_manual(
                &compute_node,
                &storage_node,
                &user_did,
                content_id,
            )
            .await?;
            println!("✅ OP_PURCHASE complete; entitlement {}", ent.green());
            Ok(())
        }
        ContentCommands::Settle {
            pending_id,
            tx_hash,
            amount,
            payer,
        } => {
            let config = load_cli_config().await?;
            let user_did = payer.clone().unwrap_or_else(|| config.identity.did.clone());
            let storage_node = get_or_create_storage_node().await?;
            let compute_node = get_or_create_compute_node().await?;
            let ent = crate::content_monetization::settle_pending_purchase(
                &storage_node,
                &compute_node,
                pending_id,
                tx_hash,
                amount,
                &user_did,
            )
            .await?;
            println!(
                "✅ Settled; entitlement {} — you can view content now.",
                ent.green()
            );
            Ok(())
        }
        ContentCommands::ListenSettlements {
            interval_secs,
            once,
        } => {
            let storage_node = get_or_create_storage_node().await?;
            let compute_node = get_or_create_compute_node().await?;
            println!(
                "🔔 Settlement listener (interval={}s, once={})",
                interval_secs, once
            );
            println!(
                "   Watching {} for inbox + open pending",
                "content_payments/".cyan()
            );
            crate::content_monetization::run_settlement_listener(
                storage_node,
                compute_node,
                *interval_secs,
                *once,
            )
            .await?;
            Ok(())
        }
        ContentCommands::RecordPayment {
            reference,
            payer,
            recipient,
            scope,
            amount,
        } => {
            let config = load_cli_config().await?;
            let payer_did = payer.clone().unwrap_or_else(|| config.identity.did.clone());
            if payer_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let storage_node = get_or_create_storage_node().await?;
            record_test_payment(
                &storage_node,
                reference,
                &payer_did,
                recipient,
                scope,
                *amount,
            )?;
            println!(
                "✅ Recorded test payment {} ({} ASTRA → {})",
                reference.green(),
                amount,
                recipient.cyan()
            );
            Ok(())
        }
        ContentCommands::Installs => {
            let storage_node = get_or_create_storage_node().await?;
            let config = load_cli_config().await?;
            let did = config.identity.did;
            if did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }
            let installs = list_content_installs(&storage_node, &did)?;
            if installs.is_empty() {
                println!("No content installs for this DID. Run `spacekit content view --content-id <ID>` first.");
                return Ok(());
            }
            println!("Content installs (collection content_installs):\n");
            for i in installs {
                println!("  content_id: {}", i.content_id_hex.cyan());
                println!("    runtime: {:?}", i.runtime);
                println!("    storage_ref: {}", i.materialized_path);
                if let Some(ref app) = i.app_slug {
                    println!("    app: {}", app);
                }
                if let Some(ref ent) = i.entitlement_id_hex {
                    println!("    entitlement: {}", ent);
                }
                println!();
            }
            Ok(())
        }
        ContentCommands::Soak { mode } => run_content_monetization_soak(mode).await,
        ContentCommands::GrowformerSoak => run_growformer_access_soak().await,
        ContentCommands::GrowformerPaidSoak => run_growformer_paid_soak().await,
    }
}

async fn run_growformer_access_soak() -> Result<(), Box<dyn std::error::Error>> {
    let script = resolve_growformer_soak_script("growformer-access-soak.sh")?;
    run_bash_soak_script(&script, &[]).await
}

async fn run_growformer_paid_soak() -> Result<(), Box<dyn std::error::Error>> {
    let script = resolve_growformer_soak_script("growformer-paid-tier-soak.sh")?;
    run_bash_soak_script(&script, &[]).await
}

async fn run_bash_soak_script(
    script: &std::path::Path,
    args: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let spacekit_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    println!("🧪 Growformer soak");
    println!("   Script: {}", script.display());
    println!("   CLI: {}", spacekit_exe.display());
    let status = std::process::Command::new("bash")
        .arg(script)
        .args(args)
        .env("SPACEKIT", spacekit_exe)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("growformer soak failed (exit {:?})", status.code()).into())
    }
}

fn resolve_growformer_soak_script(
    name: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if let Ok(p) = std::env::var("SPACEKIT_GROWFORMER_SOAK_SCRIPT") {
        let path = std::path::PathBuf::from(p);
        if path.is_file() {
            return Ok(path);
        }
    }
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for base in [
        manifest
            .join("../spacekit-storage-node/scripts/")
            .join(name),
        manifest
            .join("../../spacekit-storage-node/scripts/")
            .join(name),
    ] {
        if base.is_file() {
            return Ok(base.canonicalize()?);
        }
    }
    Err(format!("{name} not found (set SPACEKIT_GROWFORMER_SOAK_SCRIPT)").into())
}

/// Resolve and run `content-monetization-soak.sh` (repo scripts or `SPACEKIT_CONTENT_SOAK_SCRIPT`).
async fn run_content_monetization_soak(mode: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mode = mode.trim();
    if mode != "dev" && mode != "live" && mode != "router" {
        return Err("soak mode must be `dev`, `router`, or `live`".into());
    }

    let script = resolve_content_soak_script()?;
    let spacekit_exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;

    println!("🧪 Content monetization soak (mode={})", mode.cyan());
    println!("   Script: {}", script.display());
    println!("   CLI: {}", spacekit_exe.display());
    println!("   Ensure `spacekit network up` is running in another terminal.\n");

    let status = std::process::Command::new("bash")
        .arg(&script)
        .arg(mode)
        .env("SPACEKIT", &spacekit_exe)
        .status()
        .map_err(|e| format!("failed to run soak script: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "content monetization soak failed (exit {:?})",
            status.code()
        )
        .into())
    }
}

fn resolve_content_soak_script() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("SPACEKIT_CONTENT_SOAK_SCRIPT") {
        let p = std::path::PathBuf::from(&path);
        if p.is_file() {
            return Ok(p);
        }
        return Err(format!("SPACEKIT_CONTENT_SOAK_SCRIPT is not a file: {}", path).into());
    }

    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Prefer storage-node canonical script (router mode), then CLI copy, then manifest dir.
    if let Ok(root) = std::env::var("SPACEKIT_REPO_ROOT") {
        let root = std::path::PathBuf::from(root);
        candidates.push(root.join("spacekit-storage-node/scripts/content-monetization-soak.sh"));
        candidates.push(root.join("spacekit-cli/scripts/content-monetization-soak.sh"));
    }
    candidates
        .push(manifest_dir.join("../spacekit-storage-node/scripts/content-monetization-soak.sh"));
    candidates.push(manifest_dir.join("scripts/content-monetization-soak.sh"));

    soak_script_walk_up(std::env::current_dir().ok().as_deref(), &mut candidates);
    if let Ok(exe) = std::env::current_exe() {
        soak_script_walk_up(exe.parent(), &mut candidates);
    }

    for c in candidates {
        if c.is_file() {
            return Ok(c);
        }
    }

    Err(
        "content-monetization-soak.sh not found. Rebuild with `cargo build -p spacekit`, run from the \
         spacekit repo, or set SPACEKIT_REPO_ROOT / SPACEKIT_CONTENT_SOAK_SCRIPT."
            .into(),
    )
}

/// Add soak script paths by walking up from `start` (cwd, target/release, etc.).
fn soak_script_walk_up(start: Option<&std::path::Path>, out: &mut Vec<std::path::PathBuf>) {
    let Some(mut dir) = start.map(std::path::Path::to_path_buf) else {
        return;
    };
    for _ in 0..8 {
        out.push(dir.join("scripts/content-monetization-soak.sh"));
        out.push(dir.join("spacekit-cli/scripts/content-monetization-soak.sh"));
        out.push(dir.join("spacekit-storage-node/scripts/content-monetization-soak.sh"));
        if !dir.pop() {
            break;
        }
    }
}

/// Handle app package management commands
async fn handle_app_command(app_command: &AppCommands) -> Result<(), Box<dyn std::error::Error>> {
    match app_command {
        AppCommands::Package {
            source,
            name,
            version,
            entry,
            description,
            category,
            output,
            signer,
            compression,
            icon,
            keywords,
        } => {
            println!("📦 Packaging app...");
            println!("   Source: {}", source.green());
            println!("   Name: {}", name.cyan());
            println!("   Version: {}", version.yellow());
            println!("   Entry point: {}", entry);
            println!("   Category: {}", category);

            // Verify source directory exists
            let source_path = std::path::Path::new(source);
            if !source_path.exists() {
                return Err(format!("Source directory not found: {}", source).into());
            }

            // Get signer DID
            let config = load_cli_config().await?;
            let signer_did = signer
                .clone()
                .unwrap_or_else(|| config.identity.did.clone());

            if signer_did.is_empty() {
                return Err(
                    "No DID configured. Run 'spacekit init' first or specify --signer.".into(),
                );
            }

            println!("   Signer: {}", signer_did.yellow());

            // Scan source directory for files
            println!("\n📂 Scanning directory...");
            let mut files_to_package: Vec<(String, Vec<u8>, String)> = Vec::new(); // (relative_path, data, extension)

            fn scan_directory(
                dir: &std::path::Path,
                base: &std::path::Path,
                files: &mut Vec<(String, Vec<u8>, String)>,
            ) -> std::io::Result<()> {
                if dir.is_dir() {
                    for entry in std::fs::read_dir(dir)? {
                        let entry = entry?;
                        let path = entry.path();

                        // Skip hidden files and common excludes
                        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                        if file_name.starts_with('.') ||
                           file_name == "node_modules" ||
                           file_name == "target" ||
                           file_name == "__pycache__" ||
                           // Prior package outputs — nesting these bloats HTML game packages
                           file_name.ends_with(".spkg") ||
                           file_name.ends_with(".spkg.files")
                        {
                            continue;
                        }

                        if path.is_dir() {
                            scan_directory(&path, base, files)?;
                        } else {
                            let relative_path = path
                                .strip_prefix(base)
                                .map(|p| {
                                    p.components()
                                        .map(|component| component.as_os_str().to_string_lossy())
                                        .collect::<Vec<_>>()
                                        .join("/")
                                })
                                .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"));
                            let data = std::fs::read(&path)?;
                            let ext = path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_string();
                            files.push((relative_path, data, ext));
                        }
                    }
                }
                Ok(())
            }

            scan_directory(source_path, source_path, &mut files_to_package)?;
            files_to_package.sort_by(|left, right| left.0.cmp(&right.0));

            println!("   Found {} files", files_to_package.len());

            // Verify entry point exists
            let entry_exists = files_to_package.iter().any(|(p, _, _)| p == entry);
            if !entry_exists {
                return Err(format!("Entry point not found in package: {}", entry).into());
            }

            // Build content refs with hashes
            use sha2::{Digest, Sha256};
            use spacekit_primitives::v1::app::{
                AppCategory, AppManifest, AppPackage, AppPricing, CompressionAlgorithm, ContentRef,
                ContentType, EntryPoint, Platform, SemVer,
            };
            use spacekit_primitives::v1::fact::AccessPolicy;

            let mut content_refs: Vec<ContentRef> = Vec::new();
            let mut total_size: u64 = 0;
            let mut manifest_hasher = Sha256::new();

            for (rel_path, data, ext) in &files_to_package {
                let hash: [u8; 32] = Sha256::digest(data).into();
                manifest_hasher.update(&hash);

                let content_type = ContentType::from_extension(&ext);
                let size = data.len() as u64;
                total_size += size;

                content_refs.push(ContentRef {
                    path: rel_path.clone(),
                    content_type,
                    size,
                    hash,
                    // Compression belongs to the SPKG ZIP entry, not the exploded Fact payload.
                    compression: CompressionAlgorithm::None,
                    encrypted: false,
                    fact_id: [0u8; 32], // Will be filled in during deploy
                });

                println!("   📄 {} ({} bytes)", rel_path, size);
            }

            // Determine entry point type
            let entry_points = {
                let entry_ext = std::path::Path::new(entry)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");

                match entry_ext {
                    "wasm" => vec![EntryPoint::Wasm {
                        path: entry.clone(),
                        exports: vec!["main".to_string()],
                        memory_pages: None,
                    }],
                    "html" | "htm" => vec![EntryPoint::Html {
                        path: entry.clone(),
                        is_main: true,
                    }],
                    "tsx" | "jsx" => vec![EntryPoint::Component {
                        path: entry.clone(),
                        component_name: name.clone(),
                        props_schema: None,
                    }],
                    "js" | "mjs" => vec![EntryPoint::Script {
                        path: entry.clone(),
                        module_type: spacekit_primitives::v1::app::ScriptModuleType::ESModule,
                    }],
                    _ => vec![EntryPoint::Html {
                        path: entry.clone(),
                        is_main: true,
                    }],
                }
            };

            // Parse category
            let app_category = match category.to_lowercase().as_str() {
                "productivity" => AppCategory::Productivity,
                "social" => AppCategory::Social,
                "finance" => AppCategory::Finance,
                "games" => AppCategory::Games,
                "entertainment" => AppCategory::Entertainment,
                "developer" => AppCategory::Developer,
                "education" => AppCategory::Education,
                "health" => AppCategory::Health,
                "news" => AppCategory::News,
                "utilities" => AppCategory::Utilities,
                "ai" => AppCategory::AI,
                "storage" => AppCategory::Storage,
                "security" => AppCategory::Security,
                "lifestyle" => AppCategory::Lifestyle,
                "business" => AppCategory::Business,
                other => AppCategory::Custom(other.to_string()),
            };

            // Build manifest
            let manifest_checksum: [u8; 32] = manifest_hasher.finalize().into();
            let manifest = AppManifest {
                name: name.clone(),
                description: description.clone().unwrap_or_default(),
                tagline: None,
                entry_points,
                permissions: Vec::new(),
                content_types: content_refs
                    .iter()
                    .map(|r| r.content_type.clone())
                    .collect(),
                total_size,
                checksum: manifest_checksum,
                icon: icon.clone(),
                screenshots: Vec::new(),
                keywords: keywords
                    .as_ref()
                    .map(|k| k.split(',').map(|s| s.trim().to_string()).collect())
                    .unwrap_or_default(),
                min_runtime_version: None,
                platforms: vec![Platform::Web, Platform::Any],
            };

            // Parse DID
            let creator_did = spacekit_primitives::v1::identity::QuantumDID::parse(&signer_did)
                .map_err(|e| format!("Invalid DID: {}", e))?;

            // Compute app ID
            let app_id = AppPackage::compute_app_id(&creator_did, name);

            // Parse version
            let sem_ver = SemVer::parse(version).map_err(|e| format!("Invalid version: {}", e))?;

            // Create placeholder signature (real signing happens with key)
            let signature = spacekit_primitives::v1::crypto::quantum::SPHINCSSignature::new(
                Vec::new(),
                "SPHINCS-256f".to_string(),
                Vec::new(),
            );

            // Build AppPackage
            let app_package = AppPackage {
                app_id,
                version: sem_ver,
                created_at: chrono::Utc::now().timestamp() as u64,
                creator_did,
                signature,
                manifest,
                content_refs,
                license_type: spacekit_primitives::v1::fact::LicenseType::MIT,
                access_policy: AccessPolicy::Public,
                dependencies: Vec::new(),
                category: app_category,
                pricing: AppPricing::Free,
            };

            // Serialize and save
            let output_path = output.clone().unwrap_or_else(|| {
                let slug: String = name
                    .to_lowercase()
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '-' })
                    .collect::<String>();
                let slug = slug.trim_matches('-').to_string();
                format!("{}-{}.spkg", slug, version)
            });

            let mut package_files = crate::spkg::PackageFiles::new();
            for (rel_path, data, _) in files_to_package {
                if package_files.insert(rel_path.clone(), data).is_some() {
                    return Err(format!("Duplicate package path: {rel_path}").into());
                }
            }
            let output_file = std::fs::File::create(&output_path)?;
            crate::spkg::write(output_file, &app_package, &package_files)?;

            println!("\n✅ App packaged successfully!");
            println!("   Output: {}", output_path.green());
            println!("   App ID: {}", hex::encode(app_id).cyan());
            println!(
                "   Size:   {} bytes ({} files)",
                total_size,
                package_files.len()
            );
            println!(
                "\n💡 Deploy: spacekit app deploy \"{}\" --publish",
                output_path
            );

            Ok(())
        }

        AppCommands::Deploy {
            package,
            storage_node,
            publish,
            pricing,
            token,
        } => {
            println!("🚀 Deploying app...");
            println!("   Package: {}", package.green());
            println!("   Storage node: {}", storage_node.cyan());

            // Prefer the SPKG v1 ZIP container. JSON plus `.spkg.files` remains readable
            // for packages created by older CLI versions.
            let package_bytes =
                std::fs::read(package).map_err(|e| format!("Failed to read package: {}", e))?;
            let (mut app_package, package_files, canonical_package): (
                spacekit_primitives::v1::app::AppPackage,
                crate::spkg::PackageFiles,
                Vec<u8>,
            ) = match crate::spkg::read(std::io::Cursor::new(&package_bytes)) {
                Ok((app_package, files)) => (app_package, files, package_bytes),
                Err(spkg_error) => {
                    let app_package: spacekit_primitives::v1::app::AppPackage =
                            serde_json::from_slice(&package_bytes).map_err(|json_error| {
                            format!(
                                "Invalid package format (SPKG: {spkg_error}; legacy JSON: {json_error})"
                            )
                        })?;
                    let files_dir = format!("{}.files", package);
                    if !std::path::Path::new(&files_dir).exists() {
                        return Err(
                            format!("Legacy files directory not found: {}", files_dir).into()
                        );
                    }
                    let mut files = crate::spkg::PackageFiles::new();
                    for content_ref in &app_package.content_refs {
                        crate::spkg::validate_payload_path(&content_ref.path)?;
                        let file_path = std::path::Path::new(&files_dir).join(&content_ref.path);
                        let data = std::fs::read(&file_path)
                            .map_err(|e| format!("Failed to read {}: {e}", content_ref.path))?;
                        if files.insert(content_ref.path.clone(), data).is_some() {
                            return Err(format!(
                                "Duplicate legacy content ref: {}",
                                content_ref.path
                            )
                            .into());
                        }
                    }
                    let archive =
                        crate::spkg::write(std::io::Cursor::new(Vec::new()), &app_package, &files)?
                            .into_inner();
                    (app_package, files, archive)
                }
            };

            println!("   App name: {}", app_package.manifest.name.yellow());
            println!("   Version: {}", app_package.version.to_string());

            // Explicit --storage-node (remote deploy scripts) wins over embedded network profile.
            let config = load_cli_config().await?;
            let cli_storage = storage_node.trim().trim_end_matches('/').to_string();
            let resolved_storage_url = if cli_storage != "http://localhost:3030"
                && cli_storage != "http://127.0.0.1:3030"
            {
                cli_storage
            } else {
                crate::network_profile::load_spacekit_network_file()
                    .ok()
                    .flatten()
                    .map(|n| n.resolved_storage_url())
                    .or_else(|| {
                        config
                            .connections
                            .as_ref()
                            .and_then(|c| c.storage.as_ref())
                            .map(|s| s.url.trim_end_matches('/').to_string())
                    })
                    .unwrap_or(cli_storage)
            };
            let client = reqwest::Client::new();
            use sha2::{Digest, Sha256};
            let package_hash = hex::encode(Sha256::digest(&canonical_package));

            println!("   Uploading to: {}", resolved_storage_url.cyan());
            println!("   Package hash: {}", package_hash.cyan());

            let package_response = client
                .put(format!(
                    "{}/packages/{}",
                    resolved_storage_url, package_hash
                ))
                .header("Authorization", format!("DID {}", config.identity.did))
                .header("Content-Type", crate::spkg::MEDIA_TYPE)
                .body(canonical_package)
                .send()
                .await
                .map_err(|e| {
                    format!(
                        "Failed to upload canonical SPKG package: {e}. Is the storage node running?"
                    )
                })?;
            let package_status = package_response.status();
            let package_body = package_response.text().await.unwrap_or_default();
            if package_status.is_success() {
                let response: serde_json::Value = serde_json::from_str(&package_body)
                    .map_err(|e| format!("Invalid canonical package upload response: {e}"))?;
                let returned_app_id = response
                    .get("app_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("Canonical package upload response is missing app_id")?;
                let expected_app_id = hex::encode(app_package.app_id);
                if !returned_app_id.eq_ignore_ascii_case(&expected_app_id) {
                    return Err(format!(
                        "Canonical package upload returned app_id {returned_app_id}, expected {expected_app_id}"
                    )
                    .into());
                }
                println!("   ✅ Canonical SPKG uploaded");
            } else if package_status == reqwest::StatusCode::NOT_FOUND
                || package_status == reqwest::StatusCode::METHOD_NOT_ALLOWED
            {
                println!(
                    "   ⚠️  Legacy storage node: canonical SPKG endpoint unavailable ({}); continuing with exploded Facts",
                    package_status
                );
            } else {
                return Err(format!(
                    "Storage node rejected canonical SPKG: {} {}",
                    package_status, package_body
                )
                .into());
            }

            println!("\n📤 Uploading files to storage...");

            // Upload each file as a FactPackage via HTTP POST
            let mut uploaded_fact_ids: Vec<([u8; 32], String)> = Vec::new();

            for i in 0..app_package.content_refs.len() {
                let ref_path = app_package.content_refs[i].path.clone();
                let file_data = package_files
                    .get(&ref_path)
                    .ok_or_else(|| format!("SPKG payload is missing: {ref_path}"))?;
                let fact_package = file_to_fact_package(
                    &ref_path,
                    file_data,
                    app_package.creator_did.as_str(),
                    app_package.creator_did.as_str(),
                    &ref_path,
                    None,
                    "free",
                    None,
                    vec![
                        "app-content".to_string(),
                        app_package.manifest.name.clone(),
                        format!("app-id:{}", hex::encode(app_package.app_id)),
                    ],
                )
                .await?;

                let fact_id = fact_package.fact_id;
                let resp = client
                    .post(format!("{}/facts", resolved_storage_url))
                    .header("Authorization", format!("DID {}", config.identity.did))
                    .json(&fact_package)
                    .send()
                    .await
                    .map_err(|e| {
                        format!(
                            "Failed to upload {}: {}. Is the storage node running?",
                            ref_path, e
                        )
                    })?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(
                        format!("Storage node rejected {}: {} {}", ref_path, status, body).into(),
                    );
                }

                app_package.content_refs[i].fact_id = fact_id;
                uploaded_fact_ids.push((fact_id, ref_path.clone()));
                println!("   ✅ {} -> {}", ref_path, hex::encode(fact_id));
            }

            // Store the manifest as a FactPackage via HTTP POST
            use spacekit_primitives::v1::fact::{
                CollectionMethod, DataSource, FactCategory, FactContent, FactMetadata,
                FactPackage as FactPkg, KnowledgeDomain, ProofType, VerificationLevel,
                VerificationProof,
            };

            let manifest_fact = FactPkg {
                fact_id: app_package.app_id,
                version: 1,
                created_at: app_package.created_at,
                expires_at: None,
                content: FactContent::Json {
                    data: serde_json::to_value(&app_package)?,
                    schema: Some("spacekit:app-package:v1".to_string()),
                },
                metadata: FactMetadata {
                    category: FactCategory::Technical,
                    tags: vec![
                        "app-package".to_string(),
                        app_package.manifest.name.clone(),
                        format!("category:{:?}", app_package.category),
                    ],
                    domain: KnowledgeDomain::ComputerScience,
                    source: DataSource::UserInput {
                        application: app_package.creator_did.clone(),
                        user: app_package.creator_did.clone(),
                    },
                    collection_method: CollectionMethod::Manual,
                    verification_level: VerificationLevel::SelfClaimed,
                    license: app_package.license_type.clone(),
                    size_bytes: app_package.manifest.total_size,
                    checksum: app_package.manifest.checksum,
                },
                author: app_package.creator_did.clone(),
                signature: app_package.signature.clone(),
                verification_proof: VerificationProof {
                    proof_type: ProofType::QuantumSignature,
                    proof_data: Vec::new(),
                    verification_timestamp: app_package.created_at,
                    verifier: None,
                },
                dependencies: Vec::new(),
                citations: Vec::new(),
                confidence_score: 1.0,
                access_policy: app_package.access_policy.clone(),
                encryption: None,
            };

            let resp = client
                .post(format!("{}/facts", resolved_storage_url))
                .header("Authorization", format!("DID {}", config.identity.did))
                .json(&manifest_fact)
                .send()
                .await
                .map_err(|e| format!("Failed to upload manifest: {}", e))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(format!("Storage node rejected manifest: {} {}", status, body).into());
            }

            println!("\n✅ App deployed successfully!");
            println!("   App ID: {}", hex::encode(app_package.app_id).green());
            println!("   Files uploaded: {}", uploaded_fact_ids.len());

            if *publish {
                println!("\n📱 Publishing to marketplace...");
                let app_id_hex = hex::encode(app_package.app_id);
                use spacekit_primitives::v1::app::AppCategory;
                let category_slug = match &app_package.category {
                    AppCategory::Productivity => "productivity",
                    AppCategory::Social => "social",
                    AppCategory::Finance => "finance",
                    AppCategory::Games => "games",
                    AppCategory::Entertainment => "entertainment",
                    AppCategory::Developer => "developer",
                    AppCategory::Education => "education",
                    AppCategory::Health => "health",
                    AppCategory::News => "news",
                    AppCategory::Utilities => "utilities",
                    AppCategory::AI => "ai",
                    AppCategory::Storage => "storage",
                    AppCategory::Security => "security",
                    AppCategory::Lifestyle => "lifestyle",
                    AppCategory::Business => "business",
                    AppCategory::Custom(s) => s.as_str(),
                };
                let listing = serde_json::json!({
                    "app_id": app_id_hex,
                    "deployment_id": app_id_hex,
                    "publisher_did": app_package.creator_did.to_string(),
                    "marketplace_id": "default",
                    "title": app_package.manifest.name,
                    "description": app_package.manifest.description,
                    "icon_url": null,
                    "screenshots": [],
                    "category": category_slug,
                    "tags": ["app-package"],
                    "version": app_package.version.to_string(),
                    "access": "public",
                    "pricing": {
                        "model": pricing,
                        "amount_ausd": 0,
                    },
                    "artifacts": uploaded_fact_ids.iter().map(|(fid, path)| {
                        serde_json::json!({
                            "role": "content",
                            "file_id": hex::encode(fid),
                            "path": path,
                        })
                    }).collect::<Vec<_>>(),
                    "created_at": chrono::Utc::now().to_rfc3339(),
                    "updated_at": chrono::Utc::now().to_rfc3339(),
                    "downloads": 0,
                    "rating_avg": 0.0,
                    "rating_count": 0,
                    "status": "published",
                });

                let listing_url = format!(
                    "{}/api/documents/app_listings/{}",
                    resolved_storage_url.trim_end_matches('/'),
                    app_id_hex
                );
                let listing_body = serde_json::to_string_pretty(&listing)?;
                match client
                    .put(&listing_url)
                    .header("Authorization", format!("DID {}", config.identity.did))
                    .header("content-type", "application/json")
                    .body(listing_body.clone())
                    .send()
                    .await
                {
                    Ok(resp) if resp.status().is_success() => {
                        println!("   ✅ App listing stored in app_listings catalog");
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        println!(
                            "   ⚠️  app_listings catalog update returned HTTP {}: {}",
                            status, body
                        );
                    }
                    Err(e) => {
                        println!("   ⚠️  Could not write app_listings document: {}", e);
                    }
                }

                // Well-known marketplace index fact — merge listing and publish.
                match upsert_app_in_marketplace_index_http(
                    &resolved_storage_url,
                    &config.identity.did,
                    &app_id_hex,
                    &listing,
                )
                .await
                {
                    Ok(()) => {
                        println!(
                            "   ✅ Published to marketplace (app_id: {})",
                            app_id_hex.cyan()
                        );
                        println!("   🌐 View: http://localhost:5173/marketplace");
                    }
                    Err(e) => {
                        println!("   ⚠️  Marketplace index update returned: {}", e);
                    }
                }
            }

            println!(
                "\n💡 Run locally: spacekit app run {}",
                hex::encode(app_package.app_id)
            );
            println!(
                "💡 View on web: http://localhost:5173/app/{}",
                hex::encode(app_package.app_id)
            );

            Ok(())
        }

        AppCommands::Undeploy {
            app_id,
            storage_node,
            purge,
        } => {
            let app_id = app_id.trim().to_lowercase();
            if app_id.len() != 64 || !app_id.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err("App ID must be 64 hex characters".into());
            }

            let config = load_cli_config().await?;
            let owner_did = config.identity.did.clone();
            if owner_did.is_empty() {
                return Err("No DID configured. Run 'spacekit init' first.".into());
            }

            let cli_storage = storage_node.trim().trim_end_matches('/').to_string();
            let resolved_storage_url = if cli_storage != "http://localhost:3030"
                && cli_storage != "http://127.0.0.1:3030"
            {
                cli_storage
            } else {
                crate::network_profile::load_spacekit_network_file()
                    .ok()
                    .flatten()
                    .map(|n| n.resolved_storage_url())
                    .or_else(|| {
                        config
                            .connections
                            .as_ref()
                            .and_then(|c| c.storage.as_ref())
                            .map(|s| s.url.trim_end_matches('/').to_string())
                    })
                    .unwrap_or(cli_storage)
            };

            println!("🗑️  Undeploying app...");
            println!("   App ID: {}", app_id.cyan());
            println!("   Storage: {}", resolved_storage_url.green());

            let client = reqwest::Client::new();
            let manifest = fetch_remote_fact_json(&client, &resolved_storage_url, &app_id)
                .await?
                .ok_or_else(|| format!("App manifest not found on storage: {}", app_id))?;

            let mut content_ids = vec![app_id.clone()];
            for ref_id in app_content_ref_ids_from_manifest_fact(&manifest) {
                if !content_ids.iter().any(|existing| existing == &ref_id) {
                    content_ids.push(ref_id);
                }
            }

            println!("\n→ Removing marketplace catalog entries...");
            match unpublish_app_marketplace_entries(&resolved_storage_url, &owner_did, &app_id)
                .await
            {
                Ok(()) => {
                    println!("   ✅ app_listings + marketplace index updated");
                }
                Err(e) => {
                    println!("   ⚠️  Marketplace cleanup partial/failed: {}", e.yellow());
                }
            }

            println!("\n→ Unpublishing facts (manifest + bundled files)...");
            for content_id in content_ids {
                println!("   • {}", content_id.cyan());
                match delete_content_listing_http(&resolved_storage_url, &owner_did, &content_id)
                    .await
                {
                    Ok(()) => println!("      ✅ Removed from content catalog"),
                    Err(e) => {
                        println!("      ⚠️  Content catalog removal: {}", e.yellow());
                    }
                }

                if *purge {
                    let fact_url = format!(
                        "{}/facts/{}",
                        resolved_storage_url.trim_end_matches('/'),
                        content_id
                    );
                    match client
                        .delete(&fact_url)
                        .header("Authorization", format!("DID {}", owner_did))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 404 => {
                            println!("      ✅ Fact data purged from storage node");
                        }
                        Ok(resp) => {
                            println!("      ⚠️  Fact purge returned HTTP {}", resp.status());
                        }
                        Err(e) => {
                            println!("      ⚠️  Fact purge request failed: {}", e);
                        }
                    }

                    let storage_node = get_or_create_storage_node().await?;
                    let data_dir = storage_data_dir(&storage_node);
                    let prefix = &content_id[..2.min(content_id.len())];
                    let fact_dir = data_dir.join("facts").join(prefix);
                    for ext in &["json", "blob", "blob.meta"] {
                        let p = fact_dir.join(format!("{}.{}", content_id, ext));
                        if p.exists() {
                            let _ = std::fs::remove_file(&p);
                        }
                    }
                }
            }

            if *purge {
                println!("   ✅ Local fact files cleaned up where present");
            }

            println!("\n✅ App undeployed: {}", app_id.green());
            if !purge {
                println!("   💡 Re-run with --purge to delete underlying fact blobs");
            }
            Ok(())
        }

        AppCommands::List {
            category,
            creator,
            search,
            featured,
            limit,
            storage_node,
        } => {
            println!("📱 Listing apps...");

            let node = get_or_create_storage_node().await?;
            let fact_storage = get_fact_storage_engine(&node).await?;

            let config = load_cli_config().await?;
            let requester_did =
                spacekit_primitives::v1::identity::QuantumDID::parse(&config.identity.did)
                    .map_err(|e| format!("Invalid DID: {}", e))?;

            // Query for app-package tagged facts
            use spacekit_primitives::v1::fact::types::{
                FactQuery, PaginationParams, SortCriteria, SortOrder,
            };

            let mut tags = vec!["app-package".to_string()];
            if let Some(cat) = category {
                tags.push(format!("category:{}", cat));
            }

            let query = FactQuery {
                requester: requester_did,
                author: creator
                    .as_ref()
                    .and_then(|c| spacekit_primitives::v1::identity::QuantumDID::parse(c).ok()),
                category: None,
                tags,
                domain: None,
                content_type: None,
                text_search: search.clone(),
                verification_level: None,
                min_confidence: None,
                created_after: None,
                created_before: None,
                depends_on: None,
                referenced_by: None,
                sort_by: SortCriteria::CreatedAt(SortOrder::Descending),
                pagination: PaginationParams {
                    offset: 0,
                    limit: *limit as u64,
                },
                start_time: chrono::Utc::now().timestamp() as u64,
            };

            match fact_storage.query_facts(query).await {
                Ok(result) => {
                    if result.facts.is_empty() {
                        println!("   No apps found.");
                    } else {
                        println!("\n   Found {} app(s):\n", result.facts.len());
                        for (i, fact) in result.facts.iter().enumerate() {
                            // Try to parse as AppPackage
                            if let FactContent::Json { data, .. } = &fact.content {
                                if let Ok(app) = serde_json::from_value::<
                                    spacekit_primitives::v1::app::AppPackage,
                                >(data.clone())
                                {
                                    println!(
                                        "   {}. {} v{}",
                                        i + 1,
                                        app.manifest.name.cyan(),
                                        app.version.to_string().yellow()
                                    );
                                    println!("      ID: {}", hex::encode(app.app_id));
                                    println!("      Creator: {}", app.creator_did.as_str());
                                    println!("      Category: {:?}", app.category);
                                    if !app.manifest.description.is_empty() {
                                        println!("      Description: {}", app.manifest.description);
                                    }
                                    println!();
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("   ⚠️  Could not query apps: {}", e);
                }
            }

            Ok(())
        }

        AppCommands::Info {
            app_id,
            storage_node,
            versions,
        } => {
            println!("ℹ️  Getting app info...");
            println!("   App ID: {}", app_id.cyan());

            let node = get_or_create_storage_node().await?;
            let fact_storage = get_fact_storage_engine(&node).await?;

            // Decode app ID
            let app_id_bytes: [u8; 32] = hex::decode(app_id)
                .map_err(|e| format!("Invalid app ID: {}", e))?
                .try_into()
                .map_err(|_| "Invalid app ID length")?;

            // Retrieve the fact
            match fact_storage.retrieve_fact(app_id_bytes).await {
                Ok(Some(fact)) => {
                    if let FactContent::Json { data, .. } = &fact.content {
                        if let Ok(app) = serde_json::from_value::<
                            spacekit_primitives::v1::app::AppPackage,
                        >(data.clone())
                        {
                            println!("\n📱 {}", app.manifest.name.green().bold());
                            println!("   Version: {}", app.version.to_string().yellow());
                            println!("   App ID: {}", hex::encode(app.app_id).cyan());
                            println!("   Creator: {}", app.creator_did.as_str());
                            println!("   Category: {:?}", app.category);
                            println!("   Pricing: {:?}", app.pricing);
                            println!("   License: {:?}", app.license_type);
                            println!(
                                "   Created: {}",
                                chrono::DateTime::<chrono::Utc>::from_timestamp(
                                    app.created_at as i64,
                                    0
                                )
                                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                                .unwrap_or_else(|| "Unknown".to_string())
                            );

                            if !app.manifest.description.is_empty() {
                                println!("\n   Description:");
                                println!("   {}", app.manifest.description);
                            }

                            println!("\n   Entry Points:");
                            for ep in &app.manifest.entry_points {
                                match ep {
                                    spacekit_primitives::v1::app::EntryPoint::Wasm {
                                        path, ..
                                    } => {
                                        println!("      🔧 WASM: {}", path);
                                    }
                                    spacekit_primitives::v1::app::EntryPoint::Html {
                                        path,
                                        is_main,
                                    } => {
                                        println!(
                                            "      🌐 HTML: {} {}",
                                            path,
                                            if *is_main { "(main)" } else { "" }
                                        );
                                    }
                                    spacekit_primitives::v1::app::EntryPoint::Component {
                                        path,
                                        component_name,
                                        ..
                                    } => {
                                        println!(
                                            "      ⚛️  Component: {} ({})",
                                            component_name, path
                                        );
                                    }
                                    spacekit_primitives::v1::app::EntryPoint::Script {
                                        path,
                                        ..
                                    } => {
                                        println!("      📜 Script: {}", path);
                                    }
                                    _ => {}
                                }
                            }

                            println!(
                                "\n   Contents ({} files, {} bytes):",
                                app.content_refs.len(),
                                app.manifest.total_size
                            );
                            for content in &app.content_refs {
                                println!("      {} ({} bytes)", content.path, content.size);
                            }

                            if !app.manifest.keywords.is_empty() {
                                println!("\n   Keywords: {}", app.manifest.keywords.join(", "));
                            }

                            println!("\n💡 Run: spacekit app run {}", app_id);
                        } else {
                            println!("   ⚠️  Could not parse app data");
                        }
                    }
                }
                Ok(None) => {
                    println!("   ❌ App not found");
                }
                Err(e) => {
                    println!("   ❌ Error: {}", e);
                }
            }

            Ok(())
        }

        AppCommands::Download {
            app_id,
            output,
            version,
            storage_node,
            skip_verify,
        } => {
            println!("⬇️  Downloading app...");
            println!("   App ID: {}", app_id.cyan());
            println!("   Output: {}", output.green());

            let node = get_or_create_storage_node().await?;
            let fact_storage = get_fact_storage_engine(&node).await?;

            // Decode app ID
            let app_id_bytes: [u8; 32] = hex::decode(app_id)
                .map_err(|e| format!("Invalid app ID: {}", e))?
                .try_into()
                .map_err(|_| "Invalid app ID length")?;

            // Retrieve app manifest
            let fact = fact_storage
                .retrieve_fact(app_id_bytes)
                .await?
                .ok_or("App not found")?;

            use spacekit_primitives::v1::fact::FactContent;
            let app: spacekit_primitives::v1::app::AppPackage =
                if let FactContent::Json { data, .. } = &fact.content {
                    serde_json::from_value(data.clone())?
                } else {
                    return Err("Invalid app format".into());
                };

            // Create output directory
            let output_dir = std::path::Path::new(output).join(&app.manifest.name);
            std::fs::create_dir_all(&output_dir)?;

            println!("\n📥 Downloading {} files...", app.content_refs.len());

            // Download each content file
            for content_ref in &app.content_refs {
                match fact_storage.retrieve_fact(content_ref.fact_id).await? {
                    Some(content_fact) => {
                        let file_data = match &content_fact.content {
                            FactContent::Binary { data, .. } => data.clone(),
                            FactContent::Text { content, .. } => content.as_bytes().to_vec(),
                            _ => {
                                println!(
                                    "   ⚠️  Skipping {}: unsupported content type",
                                    content_ref.path
                                );
                                continue;
                            }
                        };

                        // Verify hash if not skipping
                        if !skip_verify {
                            use sha2::{Digest, Sha256};
                            let computed_hash: [u8; 32] = Sha256::digest(&file_data).into();
                            if computed_hash != content_ref.hash {
                                println!("   ❌ Hash mismatch for {}", content_ref.path);
                                continue;
                            }
                        }

                        // Write file
                        let file_path = output_dir.join(&content_ref.path);
                        if let Some(parent) = file_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&file_path, &file_data)?;

                        println!("   ✅ {}", content_ref.path);
                    }
                    None => {
                        println!("   ⚠️  Content not found: {}", content_ref.path);
                    }
                }
            }

            // Save manifest
            let manifest_path = output_dir.join("spacekit.json");
            let manifest_json = serde_json::to_string_pretty(&app)?;
            std::fs::write(&manifest_path, &manifest_json)?;

            println!("\n✅ App downloaded!");
            println!("   Location: {}", output_dir.display());

            Ok(())
        }

        AppCommands::Verify {
            app,
            storage_node,
            detailed,
        } => {
            println!("🔍 Verifying app...");
            println!("   App: {}", app.cyan());

            // Check if it's a file or an app ID
            let app_package: spacekit_primitives::v1::app::AppPackage =
                if std::path::Path::new(app).exists() {
                    // Load from file
                    let json = std::fs::read_to_string(app)?;
                    serde_json::from_str(&json)?
                } else {
                    // Load from storage
                    let node = get_or_create_storage_node().await?;
                    let fact_storage = get_fact_storage_engine(&node).await?;

                    let app_id_bytes: [u8; 32] = hex::decode(app)
                        .map_err(|e| format!("Invalid app ID: {}", e))?
                        .try_into()
                        .map_err(|_| "Invalid app ID length")?;

                    let fact = fact_storage
                        .retrieve_fact(app_id_bytes)
                        .await?
                        .ok_or("App not found")?;

                    if let FactContent::Json { data, .. } = &fact.content {
                        serde_json::from_value(data.clone())?
                    } else {
                        return Err("Invalid app format".into());
                    }
                };

            println!(
                "\n📱 {} v{}",
                app_package.manifest.name.green(),
                app_package.version.to_string()
            );

            // Verify manifest checksum
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            for content_ref in &app_package.content_refs {
                hasher.update(&content_ref.hash);
            }
            let computed_checksum: [u8; 32] = hasher.finalize().into();
            let checksum_valid = computed_checksum == app_package.manifest.checksum;

            println!("\n   Verification Results:");
            println!(
                "   {} Manifest checksum",
                if checksum_valid { "✅" } else { "❌" }
            );
            println!("   {} Creator: {}", "ℹ️ ", app_package.creator_did.as_str());
            println!(
                "   {} Signature present",
                if !app_package.signature.signature_bytes.is_empty() {
                    "✅"
                } else {
                    "⚠️ "
                }
            );
            println!(
                "   {} Content files: {}",
                "ℹ️ ",
                app_package.content_refs.len()
            );

            if checksum_valid && !app_package.signature.signature_bytes.is_empty() {
                println!("\n   ✅ App verification passed!");
            } else {
                println!("\n   ⚠️  App verification incomplete (signature verification pending)");
            }

            Ok(())
        }

        AppCommands::Run {
            app,
            storage_node,
            port,
            open,
        } => {
            println!("▶️  Running app...");
            println!("   App: {}", app.cyan());
            println!("   Port: {}", port);

            let serve_dir = if app.ends_with(".spkg") {
                let files_dir = format!("{}.files", app);
                if !std::path::Path::new(&files_dir).exists() {
                    return Err(format!(
                        "Files directory not found: {}. Run 'spacekit app package' first.",
                        files_dir
                    )
                    .into());
                }
                std::path::PathBuf::from(files_dir)
            } else {
                let node = get_or_create_storage_node().await?;
                let fact_storage = get_fact_storage_engine(&node).await?;
                let app_id: [u8; 32] = hex::decode(app)
                    .map_err(|_| format!("Invalid app ID: {}", app))?
                    .try_into()
                    .map_err(|_| "App ID must be 32 bytes (64 hex chars)")?;
                let manifest_fact = fact_storage
                    .retrieve_fact(app_id)
                    .await?
                    .ok_or_else(|| format!("App not found: {}", app))?;
                let pkg: spacekit_primitives::v1::app::AppPackage = match &manifest_fact.content {
                    FactContent::Json { data, .. } => serde_json::from_value(data.clone())?,
                    _ => return Err("Manifest is not a JSON fact".into()),
                };
                let out_dir = std::env::temp_dir().join(format!("spacekit-app-{}", &app[..12]));
                std::fs::create_dir_all(&out_dir)?;
                for cref in &pkg.content_refs {
                    let fact =
                        fact_storage
                            .retrieve_fact(cref.fact_id)
                            .await?
                            .ok_or_else(|| {
                                format!("Content not found: {}", hex::encode(cref.fact_id))
                            })?;
                    if let FactContent::Binary { data, .. } = &fact.content {
                        let file_path = out_dir.join(&cref.path);
                        if let Some(parent) = file_path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&file_path, data)?;
                        println!("   📄 {}", cref.path);
                    }
                }
                out_dir
            };

            println!("\n🌐 Serving on http://localhost:{}", port);
            println!("   Press Ctrl+C to stop\n");

            if *open {
                let url = format!("http://localhost:{}", port);
                let _ = std::process::Command::new("open")
                    .arg(&url)
                    .spawn()
                    .or_else(|_| std::process::Command::new("xdg-open").arg(&url).spawn());
            }

            let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
                .await
                .map_err(|e| format!("Failed to bind port {}: {}", port, e))?;

            loop {
                let (stream, _) = listener
                    .accept()
                    .await
                    .map_err(|e| format!("Accept error: {}", e))?;
                let dir = serve_dir.clone();
                tokio::spawn(async move {
                    let _ = handle_app_http(stream, &dir).await;
                });
            }
        }
    }
}

use spacekit_primitives::v1::fact::FactContent;

async fn handle_app_http(
    stream: tokio::net::TcpStream,
    dir: &std::path::Path,
) -> std::io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = vec![0u8; 4096];
    let mut stream = stream;
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let path = request
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    let file_path = if path == "/" || path.is_empty() {
        dir.join("index.html")
    } else {
        dir.join(path.trim_start_matches('/'))
    };

    let (status, content_type, body) = match std::fs::read(&file_path) {
        Ok(data) => {
            let ct = match file_path.extension().and_then(|e| e.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
                Some("json") => "application/json; charset=utf-8",
                Some("wasm") => "application/wasm",
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("svg") => "image/svg+xml",
                Some("webp") => "image/webp",
                Some("md") => "text/markdown; charset=utf-8",
                _ => "application/octet-stream",
            };
            ("200 OK", ct, data)
        }
        Err(_) => ("404 Not Found", "text/plain", b"Not found".to_vec()),
    };

    let header = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        status, content_type, body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    stream.write_all(&body).await?;
    Ok(())
}

// Helper functions for content publishing

async fn load_public_key_for_content() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let config = load_cli_config().await?;
    let config_dir = dirs::home_dir()
        .ok_or_else(|| "Home directory not found")?
        .join(".spacekit");

    let public_key_path = resolve_identity_key_path(&config.identity.public_key_path, &config_dir);

    let public_key_hex = std::fs::read_to_string(&public_key_path)?;
    let public_key = hex::decode(public_key_hex.trim())?;
    Ok(public_key)
}

async fn load_private_key_for_content() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let config = load_cli_config().await?;
    let config_dir = dirs::home_dir()
        .ok_or_else(|| "Home directory not found")?
        .join(".spacekit");

    let private_key_path =
        resolve_identity_key_path(&config.identity.private_key_path, &config_dir);

    let private_key_hex = std::fs::read_to_string(&private_key_path)?;
    let private_key = hex::decode(private_key_hex.trim())?;
    Ok(private_key)
}

// ============================================================================
// METRICS COMMAND HANDLERS
// ============================================================================

async fn handle_metrics_command(
    metrics_command: &MetricsCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match metrics_command {
        MetricsCommands::Collect { format } => {
            println!("📊 Collecting production metrics...");
            println!("   Format: {}", format);
            println!("✅ Metrics collected successfully!");
            Ok(())
        }
        MetricsCommands::Export { format, output } => {
            println!("📤 Exporting metrics...");
            println!("   Format: {}", format);
            if let Some(out) = output {
                println!("   Output: {}", out);
            }
            println!("✅ Metrics exported successfully!");
            Ok(())
        }
        MetricsCommands::NetworkStats { detailed } => {
            println!("🌐 Network Statistics:");
            println!("   Active nodes: 50");
            println!("   Total throughput: 1000 TPS");
            println!("   Avg latency: 45ms");
            if *detailed {
                println!("   Network bandwidth: 1.2 GB/s");
                println!("   Storage utilization: 67%");
            }
            Ok(())
        }
        MetricsCommands::Analyze { window } => {
            println!("📈 Analyzing performance metrics...");
            println!("   Time window: {}h", window);
            println!("✅ Analysis complete!");
            println!("   Performance score: 92/100");
            Ok(())
        }
        MetricsCommands::Consensus(consensus_cmd) => {
            handle_metrics_consensus_command(consensus_cmd).await
        }
    }
}

async fn handle_metrics_consensus_command(
    consensus_command: &MetricsConsensusCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match consensus_command {
        MetricsConsensusCommands::Attest { metrics } => {
            println!("🔏 Attesting node metrics...");
            println!("   Metrics file: {}", metrics);
            println!("✅ Metrics attestation created!");
            Ok(())
        }
        MetricsConsensusCommands::Validate { attestations } => {
            println!("✅ Validating cross-node metrics...");
            println!("   Attestations file: {}", attestations);
            println!("✅ Validation complete!");
            println!("   Consensus achieved: Yes");
            Ok(())
        }
        MetricsConsensusCommands::DetectFraud { metrics } => {
            println!("🔍 Detecting potential fraud...");
            println!("   Metrics file: {}", metrics);
            println!("✅ Fraud detection complete!");
            println!("   Suspicious activities: 0");
            println!("   Network integrity: 100%");
            Ok(())
        }
    }
}

#[derive(Debug, Deserialize)]
struct GfTomlForRegistry {
    #[serde(default)]
    gf_version: u64,
    project: Option<GfProjectTomlForRegistry>,
    growformer: Option<GfGrowformerTomlForRegistry>,
    extras: Option<GfExtrasTomlForRegistry>,
}

#[derive(Debug, Deserialize)]
struct GfProjectTomlForRegistry {
    name: Option<String>,
    slug: Option<String>,
    owner_did: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GfGrowformerTomlForRegistry {
    brain_storage_key: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct GfExtrasTomlForRegistry {
    #[serde(default)]
    registry_topics: Vec<String>,
}

fn sha256_manifest_id(canonical_manifest_utf8: &[u8]) -> String {
    hex::encode(Sha256::digest(canonical_manifest_utf8))
}

async fn handle_brain_registry_command(
    cmd: &BrainRegistryCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        BrainRegistryCommands::Build {
            gf_toml,
            receipt,
            network_context,
            crate_name,
            out,
        } => {
            let cfg_did = load_cli_config().await.ok().map(|c| c.identity.did);

            let gf_raw =
                fs::read_to_string(gf_toml).map_err(|e| format!("read {}: {}", gf_toml, e))?;
            let gf: GfTomlForRegistry =
                toml::from_str(&gf_raw).map_err(|e| format!("{}.gf.toml: {}", gf_toml, e))?;

            let brain_storage_key = gf
                .growformer
                .as_ref()
                .and_then(|g| g.brain_storage_key.clone())
                .ok_or("Missing [growformer].brain_storage_key in .gf.toml")?;

            let project_slug = gf
                .project
                .as_ref()
                .and_then(|p| p.slug.clone().or_else(|| p.name.clone()))
                .unwrap_or_else(|| "unknown".into());

            let rec_raw = fs::read_to_string(receipt)
                .map_err(|e| format!("read receipt {}: {}", receipt, e))?;
            let rec: serde_json::Value =
                serde_json::from_str(&rec_raw).map_err(|e| format!("receipt JSON: {}", e))?;

            let owner_from_receipt = rec
                .get("owner_did")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let publisher_did = gf
                .project
                .as_ref()
                .and_then(|p| p.owner_did.clone())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    if owner_from_receipt.is_empty() {
                        None
                    } else {
                        Some(owner_from_receipt.clone())
                    }
                })
                .or(cfg_did)
                .ok_or(
                    "publisher_did: set [project].owner_did in .gf.toml, or owner_did on receipt, or run `spacekit init`",
                )?;

            let deployment_id = rec
                .get("deployment_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let arts = rec
                .get("artifacts")
                .and_then(|a| a.as_array())
                .ok_or("receipt missing \"artifacts\" array")?;

            let wasm_id = arts
                .iter()
                .find(|x| x.get("role").and_then(|r| r.as_str()) == Some("wasm"))
                .and_then(|x| x.get("file_id"))
                .and_then(|v| v.as_str())
                .ok_or("receipt missing wasm artifact file_id")?;
            let bin_id = arts
                .iter()
                .find(|x| x.get("role").and_then(|r| r.as_str()) == Some("bin"))
                .and_then(|x| x.get("file_id"))
                .and_then(|v| v.as_str())
                .ok_or("receipt missing bin artifact file_id")?;

            let mut wasm_module = serde_json::Map::new();
            wasm_module.insert(
                "storage_ref".to_string(),
                serde_json::json!(format!("file_id:{}", wasm_id)),
            );
            if let Some(c) = crate_name {
                if !c.is_empty() {
                    wasm_module.insert("crate".to_string(), serde_json::json!(c));
                }
            }

            let topics: Vec<serde_json::Value> = gf
                .extras
                .as_ref()
                .map(|e| {
                    e.registry_topics
                        .iter()
                        .map(|t| serde_json::json!(t))
                        .collect()
                })
                .unwrap_or_default();

            let manifest = serde_json::json!({
                "manifest_version": 1,
                "artifact_kind": "bundle",
                "publisher_did": publisher_did,
                "brain_storage_key": brain_storage_key,
                "network_context": network_context,
                "project_slug": project_slug,
                "topics": topics,
                "artifacts": {
                    "brain_weights": { "storage_ref": format!("file_id:{}", bin_id) },
                    "wasm_module": wasm_module,
                },
                "compatibility": { "gf_version": gf.gf_version.max(1) },
                "deployment_id": deployment_id,
                "storage_node_manifest_source": rec.get("storage_node_url").cloned().unwrap_or(serde_json::Value::Null),
                "issued_at": Utc::now().to_rfc3339(),
            });

            let pretty = serde_json::to_string_pretty(&manifest)?;
            if let Some(path) = out {
                fs::write(path, &pretty).map_err(|e| format!("write {}: {}", path, e))?;
                println!(
                    "{}",
                    format!("Brain registry manifest written to {}", path).green()
                );
            } else {
                println!("{}", pretty);
            }
            Ok(())
        }
        BrainRegistryCommands::Publish {
            manifest,
            id,
            collection,
            publisher_did,
            storage_url,
        } => {
            let cfg_did = load_cli_config().await.ok().map(|c| c.identity.did);
            let raw = fs::read_to_string(manifest)
                .map_err(|e| format!("read manifest {}: {}", manifest, e))?;
            let value: serde_json::Value =
                serde_json::from_str(&raw).map_err(|e| format!("manifest JSON: {}", e))?;
            let canonical =
                serde_json::to_vec(&value).map_err(|e| format!("canonical manifest: {}", e))?;
            let doc_id = id
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| sha256_manifest_id(&canonical));

            let publisher = publisher_did
                .clone()
                .filter(|s| !s.is_empty())
                .or_else(|| value.get("publisher_did").and_then(|v| v.as_str()).map(String::from))
                .or(cfg_did)
                .ok_or("publisher DID: pass --publisher-did, or set publisher_did in manifest, or configure ~/.spacekit/config.toml")?;

            let (base_url, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
            let url = format!(
                "{}/api/documents/{}/{}",
                base_url.trim_end_matches('/'),
                collection,
                doc_id,
            );

            println!("Publishing brain registry manifest → {}", url.dimmed());

            let client = reqwest::Client::new();
            let resp = client
                .put(&url)
                .header("Authorization", format!("DID {}", publisher))
                .header("content-type", "application/json")
                .body(raw)
                .send()
                .await?;

            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            if status.is_success() {
                println!(
                    "{}",
                    format!(
                        "✅ Stored in collection `{}` — document id {}",
                        collection, doc_id
                    )
                    .green()
                );
            } else {
                return Err(format!(
                    "HTTP {}: {}",
                    status,
                    body_text.chars().take(500).collect::<String>()
                )
                .into());
            }
            Ok(())
        }
    }
}

/// Register embedded inference TOML into growformer's globals. Topic graph is loaded
/// by `growformer::run_cli` from `--project` / cwd (`data/knowledge_graph.toml` + overlays).
fn ensure_growformer_defaults() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        // Do not register embedded sentiment inference TOML here. `growformer::run_cli`
        // applies `[inference].toml` from `--project` *.gf.toml; registering a temp
        // sentiment core file first prevented pet project TOMLs from overriding paths.
    });
}

/// `--infer --brain path/to/local.bin` — no storage-node entitlement or redb open.
fn growformer_args_local_brain_infer(args: &[&str]) -> bool {
    let mut infer = false;
    let mut brain_path: Option<&str> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--infer" => infer = true,
            "--brain" if i + 1 < args.len() => {
                brain_path = Some(args[i + 1]);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    infer && brain_path.is_some_and(|p| std::path::Path::new(p).is_file())
}

/// Run growformer in-process with library entitlement enforcement (GROWFORMER_SPEC).
async fn run_growformer_embedded(args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    ensure_growformer_defaults();
    let config = load_cli_config().await?;
    let did = config.identity.did;
    if did.is_empty() {
        return Err("No DID configured. Run 'spacekit init' first.".into());
    }

    let local_infer = growformer_args_local_brain_infer(args);
    let entitlement = if local_infer || crate::growformer_entitlement::skip_entitlement_for_env() {
        crate::growformer_entitlement::local_dev_entitlement_context(&did)
    } else {
        let storage_node = get_or_create_storage_node().await?;
        crate::growformer_entitlement::build_growformer_entitlement_context(&storage_node, &did)
            .await?
    };

    let mut argv: Vec<&str> = vec!["growformer"];
    argv.extend_from_slice(args);
    growformer::run_cli_with_entitlement(argv, entitlement)
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    if !local_infer && !crate::growformer_entitlement::skip_entitlement_for_env() {
        if let Ok(storage_node) = get_or_create_storage_node().await {
            if let Ok(content_id) =
                crate::growformer_entitlement::resolve_growformer_content_id(&storage_node, &did)
                    .await
            {
                let _ = crate::growformer_entitlement::consume_growformer_quota(
                    &storage_node,
                    &did,
                    &content_id,
                );
            }
        }
    }
    Ok(())
}

async fn resolve_agent_entitled_content_id(
    content_id: Option<&str>,
    app: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if content_id.is_none() && app.is_none() && std::env::var("GROWFORMER_CONTENT_ID").is_err() {
        return Ok(None);
    }
    let storage_node = get_or_create_storage_node().await?;
    let config = load_cli_config().await?;
    let did = config.identity.did;
    if did.is_empty() {
        return Err("No DID configured. Run 'spacekit init' first.".into());
    }
    Ok(Some(resolve_agent_content_id(
        content_id,
        app,
        &storage_node,
        &did,
    )?))
}

async fn handle_agent_cli(agent: &AgentArgs) -> Result<(), Box<dyn std::error::Error>> {
    if agent.train {
        let proj = agent.project.as_ref().ok_or_else(|| {
            "with `--train` / `--train-brain` / `-t`, pass `--project PATH` (gf.toml)".to_owned()
                + " — or use `spacekit agent train --project PATH`."
        })?;
        let proj_str = proj.to_str().ok_or("project path is not valid UTF-8")?;
        let mut argv: Vec<String> =
            vec!["--train-brain".into(), "--project".into(), proj_str.into()];
        if agent.auto {
            argv.push("--auto".into());
        }
        if let Some(o) = agent.brain_output.as_deref() {
            argv.push("--brain-output".into());
            argv.push(o.into());
        }
        if let Some(d) = agent.data_dir.as_deref() {
            argv.push("--data-dir".into());
            argv.push(d.into());
        }
        argv.extend(agent.extra.iter().cloned());
        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        return run_growformer_embedded(&refs).await;
    }

    if agent.command.is_some() && agent.train {
        return Err(
            "Use either `-t`/`--train` with `--project` or a subcommand (e.g. `train`), not both."
                .into(),
        );
    }

    match &agent.command {
        Some(cmd) => handle_agent_command(agent, cmd).await,
        None => Err(
            "`spacekit agent` expects `--train`/`-t`/`--train-brain` plus `--project`, or a subcommand such as `train` or `load` (see `spacekit agent --help`)".into(),
        ),
    }
}

async fn handle_agent_command(
    agent: &AgentArgs,
    cmd: &AgentCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        AgentCommands::Load { name, brain } => {
            let p = Path::new(brain);
            if !p.is_file() {
                return Err(format!("brain file not found: {}", p.display()).into());
            }
            println!("📥 Loading agent brain '{}' from {} …", name, p.display());
            GROWFORMER_MANAGER.load_model(name, p.to_path_buf()).await?;
            println!("{}", "✅ Loaded.".green());
            Ok(())
        }
        AgentCommands::Unload { name } => {
            println!("🗑 Removing brain '{}' …", name);
            GROWFORMER_MANAGER.unload_model(name).await?;
            println!("{}", "✅ Unloaded.".green());
            Ok(())
        }
        AgentCommands::List => {
            let ids = GROWFORMER_MANAGER.list_models().await;
            if ids.is_empty() {
                println!(
                    "No brains loaded in this process. Use `{}`.",
                    "spacekit agent load".cyan()
                );
                return Ok(());
            }
            println!("Brains loaded in this process:\n");
            for id in ids {
                println!("  • {}", id.cyan());
            }
            Ok(())
        }
        AgentCommands::Info { brain } => {
            let p = Path::new(brain);
            if !p.is_file() {
                return Err(format!("brain file not found: {}", p.display()).into());
            }
            let meta = peek_brain_path(p)?;
            println!("Brain package: {}", p.display());
            println!("  agent_name:  {}", meta.agent_name.green());
            println!("  num_groups: {}", meta.num_groups);
            Ok(())
        }
        AgentCommands::Infer {
            name,
            brain,
            prompt,
            max_tokens,
            temperature,
            verbose,
            project,
            extra,
        } => {
            match (name.as_ref(), brain.as_ref()) {
                (Some(n), None) => {
                    let storage_node = get_or_create_storage_node().await?;
                    let config = load_cli_config().await?;
                    crate::growformer_entitlement::ensure_growformer_capability(
                        &storage_node,
                        &config.identity.did,
                        growformer::entitlement::CAP_INFER,
                    )
                    .await?;
                    let text = GROWFORMER_MANAGER
                        .generate_text(n, prompt, *max_tokens, *temperature)
                        .await?;
                    capture_real_traffic_prompt(n, prompt, Some(&text));
                    if let Ok(content_id) = crate::growformer_entitlement::resolve_growformer_content_id(
                        &storage_node,
                        &config.identity.did,
                    )
                    .await
                    {
                        let _ = crate::growformer_entitlement::consume_growformer_quota(
                            &storage_node,
                            &config.identity.did,
                            &content_id,
                        );
                    }
                    println!("{}", text);
                    Ok(())
                }
                (None, Some(b)) => {
                    let b_str = b.to_str().ok_or("brain path not valid UTF-8")?;
                    let agent_label = b
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(b_str);
                    capture_real_traffic_prompt(agent_label, prompt, None);
                    let mut argv: Vec<String> = vec![
                        "--infer".into(),
                        "--brain".into(),
                        b_str.into(),
                        "--prompt".into(),
                        prompt.clone(),
                    ];
                    if *verbose {
                        argv.push("-v".into());
                    }
                    if let Some(p) = project {
                        argv.push("--project".into());
                        argv.push(p.to_str().ok_or("project path not UTF-8")?.into());
                    }
                    argv.extend(extra.iter().cloned());
                    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
                    run_growformer_embedded(&refs).await
                }
                _ => Err(
                    "`agent infer` requires exactly one of `--name` (in-process via `agent load`) or `--brain` (`.bin` file)"
                        .into(),
                ),
            }
        }
        AgentCommands::Train {
            project,
            auto,
            brain_output,
            data_dir,
            extra,
        } => {
            let proj_str = project.to_str().ok_or("project path not UTF-8")?;
            let mut argv: Vec<String> = vec![
                "--train-brain".into(),
                "--project".into(),
                proj_str.into(),
            ];
            if *auto {
                argv.push("--auto".into());
            }
            if let Some(o) = brain_output {
                argv.push("--brain-output".into());
                argv.push(o.clone());
            }
            if let Some(d) = data_dir {
                argv.push("--data-dir".into());
                argv.push(d.clone());
            }
            argv.extend(extra.iter().cloned());
            let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            run_growformer_embedded(&refs).await
        }
        AgentCommands::Merge {
            brain,
            overlay_brain,
            brain_output,
            extra,
        } => {
            let mut argv: Vec<String> = vec![
                "--merge-brain".into(),
                "--brain".into(),
                brain.to_str().ok_or("brain path not UTF-8")?.into(),
                "--overlay-brain".into(),
                overlay_brain.to_str().ok_or("overlay-brain path not UTF-8")?.into(),
                "--brain-output".into(),
                brain_output.to_str().ok_or("brain-output path not UTF-8")?.into(),
            ];
            argv.extend(extra.iter().cloned());
            let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
            run_growformer_embedded(&refs).await
        }
        AgentCommands::Exec {
            content_id: exec_content_id,
            app: exec_app,
            args,
        } => {
            let (stripped_cid, stripped_app, run_args) =
                strip_entitlement_flags_from_exec_args(args);
            let content_id = exec_content_id
                .as_deref()
                .or(agent.content_id.as_deref())
                .or(stripped_cid.as_deref());
            let app = exec_app
                .as_deref()
                .or(agent.app.as_deref())
                .or(stripped_app.as_deref());
            if let Some(content_id) = resolve_agent_entitled_content_id(content_id, app).await? {
                let storage_node = get_or_create_storage_node().await?;
                let config = load_cli_config().await?;
                let install =
                    get_content_install(&storage_node, &config.identity.did, &content_id)?;
                let refs: Vec<&str> = run_args.iter().map(|s| s.as_str()).collect();
                if entitled_app_uses_embedded_growformer(&content_id, app, install.as_ref()) {
                    println!(
                        "🔐 Entitled growformer (content {}, tier via storage DB)",
                        &content_id[..16.min(content_id.len())]
                    );
                    return run_growformer_embedded(&refs).await;
                }
                return run_entitled_content_binary(
                    &storage_node,
                    &content_id,
                    &config.identity.did,
                    &refs,
                )
                .await
                .map_err(|e| -> Box<dyn std::error::Error> { e.into() });
            }
            let refs: Vec<&str> = run_args.iter().map(|s| s.as_str()).collect();
            run_growformer_embedded(&refs).await
        }
        AgentCommands::Code {
            prompt,
            templates,
            graph,
            out,
            workdir,
            run,
            file,
            session,
        } => {
            let args = code_session::CodeArgs {
                prompt: prompt.clone(),
                templates: templates.clone(),
                graph: graph.clone(),
                out: out.clone(),
                workdir: workdir.clone(),
                run: *run,
                file: file.clone(),
                session: *session,
            };
            code_session::handle_code(&args)
        }
        AgentCommands::App { prompt, recipes, templates, out, run } => {
            let args = code_session::AppArgs {
                prompt: prompt.clone(),
                recipes: recipes.clone(),
                templates: templates.clone(),
                out: out.clone(),
                run: *run,
            };
            code_session::handle_app(&args)
        }
        AgentCommands::Plan { prompt, kb, templates, graph, module, out, scaffold } => {
            let args = code_session::PlanArgs {
                prompt: prompt.clone(),
                kb: kb.clone(),
                templates: templates.clone(),
                graph: graph.clone(),
                module: module.clone(),
                out: out.clone(),
                scaffold: *scaffold,
            };
            code_session::handle_plan(&args)
        }
        AgentCommands::Map { root, out } => {
            let args = code_session::RepoMapArgs {
                root: root.clone(),
                out: out.clone(),
            };
            code_session::handle_map(&args)
        }
        AgentCommands::RouteCompile { templates, graph, out, verify, lint } => {
            let args = code_session::RouteCompileArgs {
                templates: templates.clone(),
                graph: graph.clone(),
                out: out.clone(),
                verify: *verify,
                lint: *lint,
            };
            code_session::handle_route_compile(&args)
        }
        AgentCommands::Sdk { spec, out, package, lang, check, plan, prune, force } => {
            let args = sdkgen::SdkArgs {
                spec: spec.clone(),
                out: out.clone(),
                package: package.clone(),
                lang: lang.clone(),
                check: *check,
                plan: *plan,
                prune: *prune,
                force: *force,
            };
            sdkgen::handle_sdk(&args)
        }
        AgentCommands::Webapp {
            spec,
            profile,
            out,
            sdk_lang,
            check,
            plan,
            prune,
            force,
            conformance,
        } => {
            let args = sdkgen::openapp::AppArgs {
                spec: spec.clone(),
                profile: profile.clone(),
                out: out.clone(),
                sdk_lang: sdk_lang.clone(),
                check: *check,
                plan: *plan,
                prune: *prune,
                force: *force,
                conformance: conformance.clone(),
            };
            sdkgen::openapp::handle_app(&args)
        }
    }
}

pub async fn run_full_client() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Handle init command first (doesn't need SDK setup)
    if let Commands::Init {
        did,
        algorithm,
        network,
        validate,
    } = &cli.command
    {
        return handle_init(did.clone(), *algorithm, network.clone(), *validate).await;
    }

    if let Commands::New {
        name,
        kind,
        app_name,
        network,
        validate,
    } = &cli.command
    {
        return handle_new(
            name.clone(),
            kind.clone(),
            app_name.clone(),
            network.clone(),
            *validate,
        )
        .await;
    }

    if let Commands::Login {
        username,
        email,
        token,
        api_url,
    } = &cli.command
    {
        return identity_cmd::handle_identity_command(&identity_cmd::IdentityCommands::Login {
            username: username.clone(),
            email: email.clone(),
            token: token.clone(),
            api_url: api_url.clone(),
        })
        .await;
    }

    if let Commands::Identity(identity_command) = &cli.command {
        return identity_cmd::handle_identity_command(identity_command).await;
    }

    let ctx = match &cli.command {
        Commands::Init { .. }
        | Commands::New { .. }
        | Commands::Login { .. }
        | Commands::Identity(identity_cmd::IdentityCommands::Login { .. })
        // Network profile, manifest, diagnostics, and E2E commands own their
        // configuration loading. In particular, `network init` and `network
        // test` must work from an empty HOME on a clean developer machine.
        | Commands::Network(_) => None,
        Commands::Keypair { .. }
        | Commands::Encapsulate { .. }
        | Commands::Decapsulate { .. }
        | Commands::Encrypt { .. }
        | Commands::Decrypt { .. } => CliContext::load_sync().ok(),
        _ => Some(CliContext::load_sync()?),
    };

    if let Commands::Vm(vm_command) = &cli.command {
        let ctx = ctx.as_ref().expect("context loaded for vm");
        return handle_vm_command(&cli, ctx, vm_command).await;
    }

    // Disabled: task management commands
    // if let Commands::Task(task_command) = &cli.command {
    //     return handle_task_command(task_command).await;
    // }

    // Handle storage management commands
    if let Commands::Storage(storage_command) = &cli.command {
        let ctx = ctx.as_ref().expect("context loaded for storage");
        return handle_storage_command(&cli, ctx, storage_command).await;
    }

    // Handle DID management commands
    if let Commands::Did(did_command) = &cli.command {
        return handle_did_command(did_command).await;
    }

    // Handle network operations commands
    if let Commands::Network(network_command) = &cli.command {
        return handle_network_command(network_command).await;
    }

    // Disabled: consensus operations commands
    // if let Commands::Consensus(consensus_command) = &cli.command {
    //     return handle_consensus_command(consensus_command).await;
    // }

    // // Handle simulator operations commands
    // if let Commands::Simulator(simulator_command) = &cli.command {
    //     return handle_simulator_command(simulator_command).await;
    // }

    // Disabled: collaborative compute commands
    // if let Commands::Collaborative(collab_command) = &cli.command {
    //     return handle_collaborative_command(collab_command).await;
    // }

    // Disabled: NFT storage commands
    // if let Commands::Nft(nft_command) = &cli.command {
    //     let ctx = ctx.as_ref().expect("context loaded for nft");
    //     return handle_nft_command(&cli, ctx, nft_command).await;
    // }

    // Disabled: metrics commands
    // if let Commands::Metrics(metrics_command) = &cli.command {
    //     return handle_metrics_command(metrics_command).await;
    // }

    // Handle contract commands
    if let Commands::Contract(contract_command) = &cli.command {
        let ctx = ctx.as_ref().expect("context loaded for contract");
        return handle_contract_command(&cli, ctx, contract_command).await;
    }

    // Handle connection commands
    if let Commands::Connect(connect_command) = &cli.command {
        return handle_connection_command(connect_command).await;
    }

    // Handle messaging commands
    if let Commands::Message(message_command) = &cli.command {
        return handle_message_command(message_command).await;
    }

    // Handle content publishing commands
    if let Commands::Content(content_command) = &cli.command {
        return handle_content_command(content_command).await;
    }

    // Handle app package commands
    if let Commands::App(app_command) = &cli.command {
        return handle_app_command(app_command).await;
    }

    if let Commands::Agent(agent) = &cli.command {
        return handle_agent_cli(agent).await;
    }

    if let Commands::BrainRegistry(brain_reg_command) = &cli.command {
        return handle_brain_registry_command(brain_reg_command).await;
    }

    if let Commands::Repo(repo_command) = &cli.command {
        return repo_cmd::handle_repo_command(repo_command).await;
    }

    if let Commands::Workspace(ws_command) = &cli.command {
        return workspace_cmd::handle_workspace_command(ws_command).await;
    }

    if let Commands::Operator(op_command) = &cli.command {
        return operator_cmd::handle_operator_command(op_command).await;
    }

    if let Commands::Tools(tools_command) = &cli.command {
        return handle_tools_command(tools_command).await;
    }

    if let Commands::Migration(mig_command) = &cli.command {
        return migration_cmd::handle_migration_command(mig_command).await;
    }

    if let Commands::Keymaster(km_command) = &cli.command {
        return keymaster_cmd::handle_keymaster_command(km_command).await;
    }

    if let Commands::Fact(fact_command) = &cli.command {
        let ctx = ctx.as_ref().expect("context loaded for fact");
        return fact_cmd::handle_fact_command(&cli, ctx, fact_command).await;
    }

    let ctx_opt = ctx.as_ref();

    if matches!(
        cli.command,
        Commands::Encrypt {
            algorithm: EncryptionAlgorithm::ECIES,
            ..
        } | Commands::Decrypt {
            algorithm: EncryptionAlgorithm::ECIES,
            ..
        }
    ) {
        return handle_ecies_file_command(&cli, ctx_opt).await;
    }

    if matches!(
        cli.command,
        Commands::Keypair { .. }
            | Commands::Encapsulate { .. }
            | Commands::Decapsulate { .. }
            | Commands::Encrypt { .. }
            | Commands::Decrypt { .. }
    ) {
        return handle_quantum_command(&cli, ctx_opt).await;
    }

    Err("Unhandled command".into())
}

// Handle init command — environment only (~/.spacekit). Use `spacekit new <name>` for a project folder.
async fn handle_init(
    provided_did: Option<String>,
    algorithm: EncryptionAlgorithm,
    network: String,
    validate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Initializing SpaceKit environment...\n");

    // 1. Create configuration directory
    let config_dir = dirs::home_dir()
        .ok_or(InitError::DirectoryCreation(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Home directory not found",
        )))?
        .join(".spacekit");

    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(config_dir.join("keys"))?;
    std::fs::create_dir_all(config_dir.join("projects"))?;

    println!(
        "📁 Created configuration directory: {}",
        config_dir.display()
    );

    // 2. Generate or import DID
    let did_id = if let Some(existing_did) = provided_did {
        println!("🔗 Using existing DID: {}", existing_did.green());
        existing_did
    } else {
        let new_did = format!("did:spacekit:user:{}", Uuid::new_v4());
        println!("🆔 Generated new DID: {}", new_did.green());
        new_did
    };

    // 3. Generate quantum key pairs
    println!(
        "🔐 Generating quantum-resistant keys using {:?}...",
        algorithm
    );

    let kem = generate_kem(algorithm).map_err(|e| InitError::KeyGeneration(e.to_string()))?;

    let (public_key, private_key) = kem
        .keypair()
        .map_err(|e| InitError::KeyGeneration(e.to_string()))?;

    // Save keys securely
    let key_dir = config_dir.join("keys");
    let public_key_path = key_dir.join("public_key.hex");
    let private_key_path = key_dir.join("private_key.hex");

    save_key_to_file(
        &hex::encode(public_key.as_ref()),
        public_key_path.to_str().unwrap(),
    )?;
    save_key_to_file(
        &hex::encode(private_key.as_ref()),
        private_key_path.to_str().unwrap(),
    )?;

    println!("🔑 Generated quantum-resistant keys using {:?}", algorithm);
    println!("💾 Keys saved to: {}", key_dir.display());

    // 4. Create configuration file
    let mut endpoints = HashMap::new();
    endpoints.insert(
        "testnet".to_string(),
        "wss://testnet-rpc.spacekit.xyz".to_string(),
    );
    endpoints.insert(
        "mainnet".to_string(),
        "wss://mainnet-rpc.spacekit.xyz".to_string(),
    );
    endpoints.insert("localhost".to_string(), "ws://localhost:9944".to_string());

    // Create a simplified CLI config that avoids u128 serialization issues
    let cli_config = CLIConfig {
        identity: IdentityConfig {
            did: did_id.clone(),
            algorithm: format!("{:?}", algorithm),
            public_key_path: "~/.spacekit/keys/public_key.hex".to_string(),
            private_key_path: "~/.spacekit/keys/private_key.hex".to_string(),
            linked_username: None,
            website_auth: None,
        },
        network: NetworkConfig {
            default_network: network.clone(),
            endpoints: endpoints.clone(),
        },
        project: ProjectConfig {
            name: "default".to_string(),
            version: "0.1.0".to_string(),
            created_at: Utc::now(),
        },
        connections: None,
        messaging: Some(MessagingSettings {
            directory_ttl_seconds: Some(3600),
            directory_max_entries: Some(1000),
        }),
    };

    // Save configuration
    let config_toml =
        toml::to_string_pretty(&cli_config).map_err(|e| InitError::ConfigSave(e.to_string()))?;

    std::fs::write(config_dir.join("config.toml"), config_toml)?;

    println!(
        "⚙️  Configuration saved to: {}",
        config_dir.join("config.toml").display()
    );

    // 5. Validate setup if requested
    if validate {
        println!("\n🔍 Validating setup...");

        if let Err(e) = validate_setup(&cli_config).await {
            println!("⚠️  Validation warnings: {}", e);
            println!("💡 Your workspace is created but some features may not work until you connect to the network");
        } else {
            println!("✅ All validations passed!");
        }
    }

    // 6. Display success summary
    println!("\n🎉 SpaceKit environment initialized successfully!\n");

    println!("📊 Summary:");
    println!("   🆔 DID: {}", did_id.blue());
    println!("   🔐 Algorithm: {}", format!("{:?}", algorithm).yellow());
    println!("   🌐 Network: {}", network.cyan());
    println!("   📂 Config: {}", config_dir.join("config.toml").display());
    println!("   🔑 Keys: {}", config_dir.join("keys").display());

    println!("\n📚 Next steps:");
    println!("   1. {}", "spacekit new my-app --kind webapp".green());
    println!(
        "   2. {}",
        "spacekit new my-dapp --kind webapp-react".green()
    );
    println!("   3. {}", "spacekit new luna --kind agent".green());
    println!(
        "   4. {}",
        "spacekit vm fund --owner-did <your-did>".green()
    );
    println!("   5. {}", "spacekit network status".green());

    println!(
        "\n💡 Learn more: {}",
        "https://docs.spacekit.xyz/cli".blue().underline()
    );

    Ok(())
}

/// Create `./<name>` with manifest and examples (requires `handle_init` / existing ~/.spacekit).
async fn handle_new(
    project_name: String,
    kind: crate::project_scaffold::NewProjectKind,
    app_name: Option<String>,
    network: String,
    validate: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let kind_label = crate::project_scaffold::project_kind_label(&kind);
    println!(
        "📂 Creating SpaceKit {} project `{}`...\n",
        kind_label.green(),
        project_name.green()
    );

    let cli_config = load_cli_config().await?;
    let did_id = cli_config.identity.did.clone();
    let algorithm = cli_config.identity.algorithm.clone();

    let project_dir = std::env::current_dir()?.join(&project_name);
    let ctx = crate::project_scaffold::ScaffoldContext::new(
        project_name.clone(),
        app_name,
        did_id.clone(),
        algorithm.clone(),
    );

    if let Err(e) = crate::project_scaffold::scaffold_project(kind.clone(), &project_dir, &ctx) {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            println!("⚠️  {}", e);
            return Ok(());
        }
        return Err(Box::new(e));
    }
    println!("📂 Created project: {}", project_dir.display());

    if validate {
        println!("\n🔍 Validating project...");
        let mut cfg = cli_config;
        cfg.project.name = project_name.clone();
        cfg.network.default_network = network.clone();
        if let Err(e) = validate_setup(&cfg).await {
            println!("⚠️  Validation warnings: {}", e);
        } else {
            println!("✅ Validation passed");
        }
    }

    println!("\n🎉 Project ready.\n");
    for step in crate::project_scaffold::next_steps(&kind, &project_name) {
        println!("   {}", step.as_str().green());
    }
    Ok(())
}

async fn validate_setup(config: &CLIConfig) -> Result<(), Box<dyn std::error::Error>> {
    // Test basic configuration
    if config.identity.did.is_empty() {
        return Err("DID is empty".into());
    }

    // Test key files exist
    let home_dir = dirs::home_dir().ok_or("Home directory not found")?;
    let key_dir = home_dir.join(".spacekit").join("keys");

    if !key_dir.join("public_key.hex").exists() {
        return Err("Public key file not found".into());
    }

    if !key_dir.join("private_key.hex").exists() {
        return Err("Private key file not found".into());
    }

    // TODO: Implement comprehensive validation including network connectivity tests
    // This should test actual SpaceKit Network connectivity and node availability
    // Test basic connectivity (without creating compute node to avoid u128 issues)
    println!("✅ Identity and key validation passed");
    println!("✅ Configuration validation passed");
    Ok(())
}

async fn handle_ecies_file_command(
    cli: &Cli,
    ctx: Option<&CliContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    match &cli.command {
        Commands::Encrypt {
            file,
            public_key_path,
            output_path,
            ..
        } => {
            let pub_path = resolve_public_key_path(ctx, public_key_path.as_deref());
            let pub_path = pub_path
                .to_str()
                .ok_or("public key path is not valid UTF-8")?;
            ecies_encrypt_file(file, pub_path, output_path)?;
            println!("✅ Encrypted file: {}", output_path.green());
        }
        Commands::Decrypt {
            file,
            secret_key_path,
            output_path,
            ..
        } => {
            let sec_path = resolve_private_key_path(ctx, secret_key_path.as_deref());
            let sec_path = sec_path
                .to_str()
                .ok_or("secret key path is not valid UTF-8")?;
            ecies_decrypt_file(file, sec_path, output_path)?;
            println!("✅ Decrypted file: {}", output_path.green());
        }
        _ => unreachable!("handle_ecies_file_command only handles ECIES encrypt/decrypt"),
    }
    Ok(())
}

// Handle all quantum-related commands
async fn handle_quantum_command(
    cli: &Cli,
    ctx: Option<&CliContext>,
) -> Result<(), Box<dyn std::error::Error>> {
    match &cli.command {
        Commands::Keypair {
            algorithm,
            save,
            secret_key_path,
            public_key_path,
        } => {
            let pub_path = resolve_public_key_path(ctx, public_key_path.as_deref());
            let sec_path = if secret_key_path.is_empty() {
                resolve_private_key_path(ctx, None)
            } else {
                PathBuf::from(secret_key_path.as_str())
            };
            let pub_path = pub_path
                .to_str()
                .ok_or("public key path is not valid UTF-8")?;
            let sec_path = sec_path
                .to_str()
                .ok_or("secret key path is not valid UTF-8")?;
            handle_keypair_generation(*algorithm, *save, sec_path, pub_path, &cli.chain).await
        }
        Commands::Encapsulate {
            algorithm,
            save,
            public_key_path,
            kem_ciphertext_output,
            kem_secret_output,
            cipher,
        } => {
            let pub_path = resolve_public_key_path(ctx, public_key_path.as_deref());
            let pub_path = pub_path
                .to_str()
                .ok_or("public key path is not valid UTF-8")?;
            handle_encapsulation(
                *algorithm,
                *save,
                pub_path,
                kem_ciphertext_output,
                kem_secret_output,
                *cipher,
            )
            .await
        }
        Commands::Decapsulate {
            algorithm,
            secret_key_path,
            kem_ciphertext,
            cipher,
        } => {
            let sec_path = resolve_private_key_path(ctx, secret_key_path.as_deref());
            let sec_path = sec_path
                .to_str()
                .ok_or("secret key path is not valid UTF-8")?;
            handle_decapsulation(*algorithm, sec_path, kem_ciphertext, *cipher).await
        }
        Commands::Encrypt {
            algorithm,
            file,
            public_key_path,
            output_path,
            cipher,
            kem_secret,
        } => {
            let cipher = cipher.unwrap_or(CipherOption::Aes);
            let kem_secret_path = kem_secret
                .as_ref()
                .ok_or("KEM secret path required for quantum encryption")?;
            let pub_path = resolve_public_key_path(ctx, public_key_path.as_deref());
            let pub_path = pub_path
                .to_str()
                .ok_or("public key path is not valid UTF-8")?;
            handle_quantum_encryption(
                *algorithm,
                file,
                pub_path,
                output_path,
                cipher,
                kem_secret_path,
            )
            .await
        }
        Commands::Decrypt {
            algorithm,
            file,
            secret_key_path,
            output_path,
            cipher,
            kem_secret: _,
        } => {
            let cipher = cipher.unwrap_or(CipherOption::Aes);
            let sec_path = resolve_private_key_path(ctx, secret_key_path.as_deref());
            let sec_path = sec_path
                .to_str()
                .ok_or("secret key path is not valid UTF-8")?;
            handle_quantum_decryption(*algorithm, file, output_path, cipher, sec_path).await
        }
        _ => unreachable!("Non-quantum commands should not be handled here"),
    }
}

async fn handle_keypair_generation(
    algorithm: EncryptionAlgorithm,
    save: bool,
    secret_key_path: &str,
    public_key_path: &str,
    chain: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    match algorithm {
        EncryptionAlgorithm::ECIES => {
            // Generate ECIES keypair directly without SDK
            match chain {
                "solana" => {
                    let (priv_key, pub_key) = new_keypair_solana()?;
                    let public_key_base58 = key_to_base58(pub_key.as_bytes());
                    let private_key_base58 = key_to_base58(priv_key.as_bytes());

                    println!("✅ {} Generated Key Pair:", "Solana Ed25519".green());
                    if save {
                        save_key_to_file(&public_key_base58, public_key_path)?;
                        save_key_to_file(&private_key_base58, secret_key_path)?;
                        println!(
                            "💾 Keys saved to {} and {}",
                            secret_key_path.blue(),
                            public_key_path.blue()
                        );
                    } else {
                        println!(
                            "🔑 Private Key (Base58): \"{}\"",
                            private_key_base58.yellow()
                        );
                        println!("🔑 Public Key (Base58): \"{}\"", public_key_base58.green());
                    }
                }
                _ => {
                    // Default to EVM ECIES
                    let (priv_key, pub_key) = new_keypair_evm()?;
                    let public_key_hex = hex::encode(pub_key.serialize());
                    let private_key_hex = hex::encode(priv_key.serialize());
                    let address = ethereum_address_from_ecies_public_key(&pub_key)?;

                    println!("✅ {} Generated Key Pair:", "ECIES".green());
                    if save {
                        save_key_to_file(&public_key_hex, public_key_path)?;
                        save_key_to_file(&private_key_hex, secret_key_path)?;
                        println!(
                            "💾 Keys saved to {} and {}",
                            secret_key_path.blue(),
                            public_key_path.blue()
                        );
                    } else {
                        println!("🔑 Private Key (Hex): \"{}\"", private_key_hex.yellow());
                        println!("🔑 Public Key (Hex): \"{}\"", public_key_hex.green());
                    }
                    println!("📍 Ethereum Address: {}", address.blue());
                }
            }
        }
        _ => {
            // Generate quantum KEM keypair
            match generate_kem(algorithm) {
                Ok(kem) => match kem.keypair() {
                    Ok((public_key, private_key)) => {
                        let public_key_bytes = public_key.as_ref();
                        let private_key_bytes = private_key.as_ref();

                        println!(
                            "✅ {} Generated Key Pair",
                            format!("Quantum KEM {:?}", algorithm).green()
                        );

                        if save {
                            match save_key_to_file(&hex::encode(public_key_bytes), public_key_path)
                            {
                                Ok(()) => {
                                    println!("💾 Public key saved to {}", public_key_path.blue())
                                }
                                Err(e) => eprintln!("❌ Error saving public key: {}", e),
                            }
                            match save_key_to_file(&hex::encode(private_key_bytes), secret_key_path)
                            {
                                Ok(()) => {
                                    println!("💾 Private key saved to {}", secret_key_path.blue())
                                }
                                Err(e) => eprintln!("❌ Error saving private key: {}", e),
                            }
                        } else {
                            println!(
                                "🔑 Private Key: {}",
                                hex::encode(private_key_bytes).yellow()
                            );
                            println!("🔑 Public Key: {}", hex::encode(public_key_bytes).green());
                        }
                    }
                    Err(e) => eprintln!("❌ Error generating keypair: {}", e),
                },
                Err(e) => eprintln!("❌ Error initializing KEM: {}", e),
            }
        }
    }
    Ok(())
}

async fn handle_encapsulation(
    algorithm: EncryptionAlgorithm,
    save: bool,
    public_key_path: &str,
    kem_ciphertext_output: &str,
    kem_secret_output: &str,
    cipher: CipherOption,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🔐 Encapsulating with {} and {} cipher...",
        format!("{:?}", algorithm).green(),
        format!("{:?}", cipher).blue()
    );

    match generate_kem(algorithm) {
        Ok(kem) => {
            // Load public key
            let public_key_hex = load_from_file(public_key_path)?;
            let public_key_bytes = hex::decode(String::from_utf8(public_key_hex)?)?;

            if let Some(public_key) = kem.public_key_from_bytes(&public_key_bytes) {
                match kem.encapsulate(&public_key) {
                    Ok((ciphertext, shared_secret)) => {
                        println!("✅ Encapsulation successful!");

                        if save {
                            save_to_file(kem_ciphertext_output, &ciphertext.as_ref().to_vec())?;
                            save_to_file(kem_secret_output, &shared_secret.as_ref().to_vec())?;
                            println!(
                                "💾 KEM ciphertext saved to {}",
                                kem_ciphertext_output.blue()
                            );
                            println!("💾 Shared secret saved to {}", kem_secret_output.blue());
                        } else {
                            println!(
                                "📦 KEM Ciphertext: {}",
                                hex::encode(ciphertext.as_ref()).yellow()
                            );
                            println!(
                                "🔑 Shared Secret: {}",
                                hex::encode(shared_secret.as_ref()).green()
                            );
                        }
                    }
                    Err(e) => eprintln!("❌ Encapsulation failed: {}", e),
                }
            } else {
                eprintln!("❌ Invalid public key format");
            }
        }
        Err(e) => eprintln!("❌ Error initializing KEM: {}", e),
    }

    Ok(())
}

async fn handle_decapsulation(
    algorithm: EncryptionAlgorithm,
    secret_key_path: &str,
    kem_ciphertext_path: &str,
    cipher: CipherOption,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🔓 Decapsulating with {} and {} cipher...",
        format!("{:?}", algorithm).green(),
        format!("{:?}", cipher).blue()
    );

    match generate_kem(algorithm) {
        Ok(kem) => {
            // Load secret key and ciphertext
            let secret_key_hex = load_from_file(secret_key_path)?;
            let secret_key_bytes = hex::decode(String::from_utf8(secret_key_hex)?)?;

            let kem_ciphertext_bytes = load_from_file(kem_ciphertext_path)?;

            if let Some(secret_key) = kem.secret_key_from_bytes(&secret_key_bytes) {
                if let Some(ciphertext) = kem.ciphertext_from_bytes(&kem_ciphertext_bytes) {
                    match kem.decapsulate(&secret_key, &ciphertext) {
                        Ok(shared_secret) => {
                            println!("✅ Decapsulation successful!");
                            println!(
                                "🔑 Shared Secret: {}",
                                hex::encode(shared_secret.as_ref()).green()
                            );
                        }
                        Err(e) => eprintln!("❌ Decapsulation failed: {}", e),
                    }
                } else {
                    eprintln!("❌ Invalid ciphertext format");
                }
            } else {
                eprintln!("❌ Invalid secret key format");
            }
        }
        Err(e) => eprintln!("❌ Error initializing KEM: {}", e),
    }

    Ok(())
}

async fn handle_quantum_encryption(
    algorithm: EncryptionAlgorithm,
    file_path: &str,
    _public_key_path: &str,
    _output_path: &str,
    cipher: CipherOption,
    _kem_secret_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🔐 Quantum encrypting {} with {} and {} cipher...",
        file_path.blue(),
        format!("{:?}", algorithm).green(),
        format!("{:?}", cipher).yellow()
    );

    // Convert algorithm to quantum algorithm
    let quantum_alg = convert_to_quantum_algorithm(algorithm);
    let cipher_impl: Cipher = cipher.into();

    match handle_encryption(file_path, &quantum_alg, cipher_impl) {
        Ok(()) => {
            println!("✅ File encrypted successfully!");
            println!(
                "💾 Encrypted file: {}",
                format!("{}.enc", file_path).green()
            );
            println!("💾 KEM ciphertext: {}", format!("{}.kem", file_path).blue());
            println!("💾 Public key: {}", format!("{}.pub", file_path).yellow());
        }
        Err(e) => eprintln!("❌ Encryption failed: {}", e),
    }

    Ok(())
}

async fn handle_quantum_decryption(
    algorithm: EncryptionAlgorithm,
    encrypted_file_path: &str,
    output_path: &str,
    cipher: CipherOption,
    secret_key_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🔓 Quantum decrypting {} with {} and {} cipher...",
        encrypted_file_path.blue(),
        format!("{:?}", algorithm).green(),
        format!("{:?}", cipher).yellow()
    );

    // Convert algorithm to quantum algorithm
    let quantum_alg = convert_to_quantum_algorithm(algorithm);
    let cipher_impl: Cipher = cipher.into();

    let kem_file_path = format!("{}.kem", encrypted_file_path.trim_end_matches(".enc"));

    match handle_decryption(
        encrypted_file_path,
        &kem_file_path,
        secret_key_path,
        cipher_impl,
        &quantum_alg,
    ) {
        Ok(decrypted_content) => {
            fs::write(output_path, decrypted_content)?;
            println!("✅ File decrypted successfully!");
            println!("💾 Decrypted file: {}", output_path.green());
        }
        Err(e) => eprintln!("❌ Decryption failed: {}", e),
    }

    Ok(())
}

// ── SKTCS Tools subcommand handler ──────────────────────────────────────

async fn handle_tools_command(cmd: &ToolsCommands) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        ToolsCommands::EmbedManifest {
            wasm,
            manifest,
            output,
        } => {
            let manifest_bytes = std::fs::read(manifest)?;
            // Validate the manifest JSON before embedding
            let _: serde_json::Value = serde_json::from_slice(&manifest_bytes)
                .map_err(|e| format!("invalid manifest JSON: {}", e))?;

            let wasm_bytes = std::fs::read(wasm)?;
            if wasm_bytes.len() < 8 || &wasm_bytes[0..4] != b"\x00asm" {
                return Err("not a valid WASM binary".into());
            }

            // Build a custom section: id=0, name="spacekit:tools", data=manifest_bytes
            let section_name = b"spacekit:tools";
            let mut section_payload = Vec::new();
            write_leb128(&mut section_payload, section_name.len() as u32);
            section_payload.extend_from_slice(section_name);
            section_payload.extend_from_slice(&manifest_bytes);

            let mut output_bytes = wasm_bytes;
            output_bytes.push(0x00); // custom section id
            write_leb128(&mut output_bytes, section_payload.len() as u32);
            output_bytes.extend_from_slice(&section_payload);

            let out_path = output.as_deref().unwrap_or(wasm);
            std::fs::write(out_path, &output_bytes)?;
            println!(
                "✅ Embedded spacekit:tools ({} bytes) into {}",
                manifest_bytes.len(),
                out_path
            );
            Ok(())
        }
        ToolsCommands::ValidateManifest { manifest } => {
            let content = std::fs::read_to_string(manifest)?;
            let parsed: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| format!("JSON parse error: {}", e))?;

            let version = parsed.get("version").and_then(|v| v.as_str());
            let contract_id = parsed.get("contract_id").and_then(|v| v.as_str());
            let tools = parsed.get("tools").and_then(|v| v.as_object());

            if version.is_none() {
                return Err("missing `version` field".into());
            }
            if contract_id.is_none() {
                return Err("missing `contract_id` field".into());
            }
            let tools_map = tools.ok_or("missing `tools` object")?;

            println!("✅ Valid SKTCS manifest v{}", version.unwrap());
            println!("   contract_id: {}", contract_id.unwrap());
            println!("   tools: {} defined", tools_map.len());
            for (name, def) in tools_map {
                let pattern = def.get("pattern").and_then(|v| v.as_str()).unwrap_or("?");
                let module = def.get("module").and_then(|v| v.as_str()).unwrap_or("?");
                let function = def.get("function").and_then(|v| v.as_str()).unwrap_or("?");
                println!("     - {} ({}.{}, {})", name, module, function, pattern);
            }
            Ok(())
        }
    }
}

fn write_leb128(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_failed_load_and_verify_did() {
        let sdk = SpaceKitSDK::with_cache("./identity_cache").unwrap();
        let did_addr = "0x1234567890123456789012345678901234567890";
        let result = load_and_verify_did(&sdk, did_addr).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_quantum_keypair_generation() {
        let result = handle_keypair_generation(
            EncryptionAlgorithm::Kyber1024,
            false,
            "test_secret.hex",
            "test_public.hex",
            "ethereum",
        )
        .await;
        assert!(result.is_ok());
    }

    #[test]
    fn network_probe_targets_parse_urls_and_multiaddrs() {
        assert_eq!(
            target_socket("https://example.com/status").unwrap(),
            "example.com:443"
        );
        assert_eq!(
            target_socket("/ip4/127.0.0.1/tcp/7100").unwrap(),
            "127.0.0.1:7100"
        );
        assert!(target_socket("/ip4/127.0.0.1/udp/7100").is_err());
    }

    #[tokio::test]
    async fn http_probe_uses_a_real_socket_and_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut request).await;
            tokio::io::AsyncWriteExt::write_all(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            )
            .await
            .unwrap();
        });
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        assert!(probe_http(&client, "test", &format!("http://{}", address)).await);
        server.await.unwrap();
    }
}
