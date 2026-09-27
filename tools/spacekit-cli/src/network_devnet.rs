//! `spacekit network devnet`: a proof-of-authority network on one machine.
//!
//! `init` writes everything a PoA network needs into one directory:
//!
//! ```text
//! <dir>/devnet.json            layout: nodes, DIDs, URLs (read by up/down/status)
//! <dir>/poa-genesis.json       genesis authorities (SPACEKIT_POA_GENESIS_FILE)
//! <dir>/genesis-alloc.json     genesis balances (SPACEKIT_GENESIS_ALLOC_FILE)
//! <dir>/manifest.json          private network manifest (network_id, bootstrap)
//! <dir>/node-<i>/config.toml   compute-only private profile
//! <dir>/node-<i>/wallet.json   the node's SPHINCS+ authority wallet
//! <dir>/node-<i>/home/         HOME for the node's CLI identity
//! ```
//!
//! The first `--authorities` nodes are genesis authorities. Extra nodes
//! (`--observers`) have wallets too but are not authorities: they follow the
//! chain, and start producing once governance admits them.

use crate::network_profile::{
    canonical_genesis_hash, ManifestBootstrap, ManifestGenesis, ManifestMember, ManifestProtocol,
    NetworkManifest, NetworkPreset, NetworkRole, SpacekitNetworkFile, NETWORK_MANIFEST_VERSION,
    NETWORK_PROTOCOL, NETWORK_PROTOCOL_VERSION,
};
use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

pub const DEVNET_NETWORK_ID: &str = "spacekit-devnet";
pub const DEVNET_CHAIN_ID: u64 = 31338;
/// Ports reserved per node (the profile allocates 12; the rest is headroom).
const PORTS_PER_NODE: u16 = 20;

type CmdResult = Result<(), Box<dyn std::error::Error>>;

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum ProductionArg {
    /// Blocks only for transactions, due rewards, and heartbeats
    OnDemand,
    /// A block every block time, empty or not
    Interval,
}

impl ProductionArg {
    pub fn as_str(self) -> &'static str {
        match self {
            ProductionArg::OnDemand => "on_demand",
            ProductionArg::Interval => "interval",
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum DevnetAction {
    /// Generate authority keys, genesis, manifest, and one profile per node
    Init {
        /// Directory for the devnet (default: ~/.spacekit/devnet)
        #[arg(long)]
        dir: Option<PathBuf>,
        /// Genesis authorities (4 tolerates one faulty authority)
        #[arg(long, default_value_t = 4)]
        authorities: usize,
        /// Extra non-authority nodes (admit them later with governance)
        #[arg(long, default_value_t = 0)]
        observers: usize,
        /// Validators needed before `lift_poa` is accepted (default: --authorities)
        #[arg(long)]
        min_validators_to_lift: Option<usize>,
        /// Minimum gap between blocks (on-demand) or the block time (interval)
        #[arg(long, default_value_t = 2000)]
        block_time_ms: u64,
        /// When blocks are produced
        #[arg(long, value_enum, default_value = "on-demand")]
        production: ProductionArg,
        /// On-demand: wait this long after the first pending transaction
        #[arg(long, default_value_t = 500)]
        batch_window_ms: u64,
        /// On-demand: an empty block after this much idle time (0 = never)
        #[arg(long, default_value_t = 300)]
        heartbeat_secs: u64,
        /// Minimum validator stake after the lift, in whole ASTRA
        #[arg(long, default_value_t = 10_000)]
        min_stake_astra: u64,
        /// Unbonding period in days
        #[arg(long, default_value_t = 21)]
        unbonding_days: u64,
        /// Days authorities keep producing unstaked after the lift
        #[arg(long, default_value_t = 30)]
        pos_grace_days: u64,
        /// First port; node i uses base + 20*i .. base + 20*i + 11
        #[arg(long, default_value_t = 39000)]
        base_port: u16,
        /// Genesis balance, `0xADDRESS=AMOUNT` (repeatable). The faucet is off on PoA networks.
        #[arg(long = "fund")]
        fund: Vec<String>,
        /// Enable service rewards (AstraRewards system transactions)
        #[arg(long)]
        rewards: bool,
        /// Reward epoch length in seconds (short, so settlement is visible)
        #[arg(long, default_value_t = 60)]
        reward_epoch_secs: u64,
        /// AstraRewards WASM to install at genesis
        #[arg(long)]
        astra_rewards_wasm: Option<PathBuf>,
        /// DIDs whose PoA credits are locked like the authorities' (repeatable)
        #[arg(long = "affiliated")]
        affiliated: Vec<String>,
        /// Replace an existing devnet directory
        #[arg(long)]
        force: bool,
    },
    /// Start every node (or only `--node i`) in the background
    Up {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long = "node")]
        nodes: Vec<usize>,
    },
    /// Stop every node (or only `--node i`)
    Down {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long = "node")]
        nodes: Vec<usize>,
    },
    /// Heads, proposers, governance mode and agreement across nodes
    Status {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevnetNode {
    pub index: usize,
    pub did: String,
    pub sphincs_pk_hex: String,
    pub authority: bool,
    pub root: PathBuf,
    pub config: PathBuf,
    pub home: PathBuf,
    pub wallet: PathBuf,
    pub compute_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevnetLayout {
    pub network_id: String,
    pub chain_id: u64,
    pub dir: PathBuf,
    pub genesis_file: PathBuf,
    pub alloc_file: PathBuf,
    pub manifest: PathBuf,
    pub nodes: Vec<DevnetNode>,
}

#[derive(Debug, Clone)]
pub struct DevnetOptions {
    pub dir: PathBuf,
    pub authorities: usize,
    pub observers: usize,
    pub min_validators_to_lift: Option<usize>,
    pub block_time_ms: u64,
    /// `"on_demand"` or `"interval"`.
    pub production: String,
    pub batch_window_ms: u64,
    pub heartbeat_secs: u64,
    pub min_stake_astra: u64,
    pub unbonding_days: u64,
    pub pos_grace_days: u64,
    /// 12 ports per node, in profile order. Empty: `base_port + 20*i + k`.
    pub ports: Vec<u16>,
    pub base_port: u16,
    pub fund: Vec<(String, u128)>,
    pub rewards: bool,
    pub reward_epoch_secs: u64,
    pub astra_rewards_wasm: Option<PathBuf>,
    pub affiliated: Vec<String>,
    pub force: bool,
}

pub fn default_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".spacekit/devnet")
}

/// `did:spacekit:devnet:<hex(sha256(pk)[..20])>`, as governance requires.
pub fn devnet_did(public_key: &[u8]) -> String {
    format!(
        "did:spacekit:devnet:{}",
        hex::encode(&Sha256::digest(public_key)[..20])
    )
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    std::fs::write(path, serde_json::to_vec_pretty(value).unwrap())
        .map_err(|e| format!("write {}: {e}", path.display()))
}

fn io<E: std::fmt::Display>(context: &str) -> impl Fn(E) -> String + '_ {
    move |e| format!("{context}: {e}")
}

/// Write a devnet. Pure file generation: nothing is started.
pub fn create(opts: &DevnetOptions) -> Result<DevnetLayout, String> {
    if opts.authorities == 0 {
        return Err("a devnet needs at least one authority".into());
    }
    let total = opts.authorities + opts.observers;
    if !opts.ports.is_empty() && opts.ports.len() < total * 12 {
        return Err(format!("need {} ports, got {}", total * 12, opts.ports.len()));
    }
    let port = |node: usize, k: usize| -> Result<u16, String> {
        if opts.ports.is_empty() {
            let offset = u32::from(PORTS_PER_NODE) * node as u32 + k as u32;
            u16::try_from(u32::from(opts.base_port) + offset)
                .map_err(|_| "base port too high for this many nodes".to_string())
        } else {
            Ok(opts.ports[node * 12 + k])
        }
    };
    if opts.dir.join("devnet.json").exists() {
        if !opts.force {
            return Err(format!(
                "{} already holds a devnet (use --force to replace it)",
                opts.dir.display()
            ));
        }
        std::fs::remove_dir_all(&opts.dir).map_err(io("remove old devnet"))?;
    }
    std::fs::create_dir_all(&opts.dir).map_err(io("create devnet dir"))?;
    let dir = std::fs::canonicalize(&opts.dir).map_err(io("canonicalize devnet dir"))?;

    // Keys first: DIDs feed the genesis, manifest and allowlists.
    let mut keys = Vec::with_capacity(total);
    for index in 0..total {
        let pair = spacekit_did::sphincs::SphincsPlus::generate_keypair();
        let did = devnet_did(&pair.public_key);
        let root = dir.join(format!("node-{index}"));
        std::fs::create_dir_all(&root).map_err(io("create node dir"))?;
        let wallet = root.join("wallet.json");
        write_json(
            &wallet,
            &json!({
                "did": did,
                "sphincs_pk_hex": hex::encode(&pair.public_key),
                "sphincs_sk_hex": hex::encode(&pair.private_key),
                "algorithm": pair.algorithm,
            }),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&wallet, std::fs::Permissions::from_mode(0o600));
        }
        keys.push((did, hex::encode(&pair.public_key), root, wallet));
    }
    let dids: Vec<String> = keys.iter().map(|k| k.0.clone()).collect();

    let genesis = json!({
        "network": DEVNET_NETWORK_ID,
        "min_validators_to_lift": opts.min_validators_to_lift.unwrap_or(opts.authorities),
        "block_production": {
            "mode": opts.production,
            "block_time_ms": opts.block_time_ms,
            "batch_window_ms": opts.batch_window_ms,
            "heartbeat_secs": opts.heartbeat_secs,
        },
        "staking": {
            "min_stake_astra": opts.min_stake_astra,
            "unbonding_secs": opts.unbonding_days * 86_400,
        },
        "pos_grace_days": opts.pos_grace_days,
        "authorities": keys.iter().take(opts.authorities).enumerate().map(|(i, k)| json!({
            "did": k.0,
            "sphincs_pk_hex": k.1,
            "name": format!("Devnet authority {i}"),
        })).collect::<Vec<_>>(),
    });
    let genesis_file = dir.join("poa-genesis.json");
    write_json(&genesis_file, &genesis)?;
    let alloc = json!({
        "accounts": opts.fund.iter().map(|(address, amount)| json!({
            "address": address,
            "balance": amount.to_string(),
        })).collect::<Vec<_>>(),
    });
    let alloc_file = dir.join("genesis-alloc.json");
    write_json(&alloc_file, &alloc)?;

    // The manifest's genesis document covers everything nodes must share.
    let document = json!({
        "chain_id": DEVNET_CHAIN_ID,
        "poa": genesis,
        "alloc": alloc,
    });
    let hash = canonical_genesis_hash(&document).map_err(|e| e.to_string())?;
    let compute_p2p: Vec<String> = (0..opts.authorities)
        .map(|i| port(i, 3).map(|p| format!("/ip4/127.0.0.1/tcp/{p}")))
        .collect::<Result<_, _>>()?;
    let manifest = NetworkManifest {
        version: NETWORK_MANIFEST_VERSION,
        network_id: DEVNET_NETWORK_ID.into(),
        profile: NetworkPreset::Private,
        chain_id: DEVNET_CHAIN_ID,
        protocol: ManifestProtocol {
            name: NETWORK_PROTOCOL.into(),
            version: NETWORK_PROTOCOL_VERSION,
        },
        genesis: ManifestGenesis {
            hash: hash.clone(),
            uri: None,
            document: Some(document),
        },
        bootstrap: ManifestBootstrap {
            p2p: compute_p2p,
            rpc: vec![format!("http://127.0.0.1:{}", port(0, 2)?)],
        },
        roles: vec![NetworkRole::Operator, NetworkRole::Validator],
        members: dids
            .iter()
            .map(|did| ManifestMember {
                did: did.clone(),
                roles: vec![NetworkRole::Operator, NetworkRole::Validator],
            })
            .collect(),
        signature: None,
    };
    manifest.validate().map_err(|e| e.to_string())?;
    let manifest_path = dir.join("manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).unwrap())
        .map_err(io("write manifest"))?;

    let compute_urls: Vec<String> = (0..total)
        .map(|i| port(i, 2).map(|p| format!("http://127.0.0.1:{p}")))
        .collect::<Result<_, _>>()?;
    let rewards_genesis_ts = chrono::Utc::now().timestamp().max(0) as u64;
    let mut nodes = Vec::with_capacity(total);
    for (index, (did, pk_hex, root, wallet)) in keys.into_iter().enumerate() {
        let mut profile = SpacekitNetworkFile::for_preset(NetworkPreset::Private);
        profile.node_id = format!("devnet-node-{index}");
        profile.role = NetworkRole::Validator;
        profile.manifest = Some(manifest_path.clone());
        profile.admission.shared_genesis_hash = Some(hash.clone());
        profile.admission.allowlist = dids.clone();
        profile.bind_host = "127.0.0.1".into();
        profile.services.storage = false;
        profile.services.messaging = false;
        profile.services.compute = true;
        profile.runtime.enable_p2p = true;
        profile.messaging.bootstrap_peers = vec![format!("/ip4/127.0.0.1/tcp/{}", port(0, 4)?)];
        profile.ports.storage_http = port(index, 0)?;
        profile.ports.storage_p2p = port(index, 1)?;
        profile.ports.compute_http = port(index, 2)?;
        profile.ports.compute_p2p = port(index, 3)?;
        profile.ports.messaging_listen = port(index, 4)?;
        profile.ports.messaging_bootstrap = port(index, 5)?;
        profile.ports.messaging_http = port(index, 6)?;
        profile.ports.gateway_http = port(index, 7)?;
        profile.ports.status_http = port(index, 8)?;
        profile.ports.keymaster_coordinator = port(index, 9)?;
        profile.ports.keymaster_registry = port(index, 10)?;
        profile.ports.keymaster_guardian_base = port(index, 11)?;
        profile.messaging.listen_addr = format!("127.0.0.1:{}", port(index, 4)?);
        profile.urls.storage = Some(format!("http://127.0.0.1:{}", port(index, 0)?));
        profile.urls.compute = Some(compute_urls[index].clone());
        profile.data.storage = Some(root.join("storage"));
        profile.data.compute = Some(root.join("compute"));
        profile.data.messaging = Some(root.join("messaging"));

        let chain = &mut profile.blockchain;
        chain.enabled = true;
        chain.persist_state = true;
        chain.chain_id = DEVNET_CHAIN_ID;
        chain.block_time_ms = opts.block_time_ms;
        chain.validators.peers = dids.clone();
        chain.poa.genesis_file = Some(genesis_file.clone());
        chain.poa.authority_wallet = Some(wallet.clone());
        chain.poa.genesis_alloc_file = Some(alloc_file.clone());
        chain.poa.rewards = opts.rewards;
        if opts.rewards {
            chain.poa.reward_epoch_secs = Some(opts.reward_epoch_secs.max(1));
            chain.poa.rewards_genesis_ts = Some(rewards_genesis_ts);
            chain.poa.affiliated_operator_dids = opts.affiliated.clone();
            chain.poa.astra_rewards_wasm = opts
                .astra_rewards_wasm
                .as_ref()
                .map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()));
        }
        profile.validate().map_err(|e| e.to_string())?;
        crate::network_profile::authorize_network_start(&profile, &did)
            .map_err(|e| format!("node {index}: {e}"))?;

        let config = root.join("config.toml");
        std::fs::write(&config, toml::to_string_pretty(&profile).unwrap())
            .map_err(io("write node profile"))?;
        let home = root.join("home");
        let cli_dir = home.join(".spacekit");
        std::fs::create_dir_all(&cli_dir).map_err(io("create node home"))?;
        std::fs::write(
            cli_dir.join("config.toml"),
            format!(
                "[identity]\ndid = \"{did}\"\nalgorithm = \"Kyber1024\"\npublic_key_path = \"{}\"\nprivate_key_path = \"{}\"\n\n[network]\ndefault_network = \"{DEVNET_NETWORK_ID}\"\n\n[network.endpoints]\n\n[project]\nname = \"devnet-node-{index}\"\nversion = \"1.0.0\"\ncreated_at = \"{}\"\n",
                root.join("keys/public.hex").display(),
                root.join("keys/private.hex").display(),
                chrono::Utc::now().to_rfc3339(),
            ),
        )
        .map_err(io("write node identity"))?;
        // `spacekit governance` reads ~/.spacekit/did_wallet.json by default.
        std::fs::copy(&wallet, cli_dir.join("did_wallet.json")).map_err(io("copy wallet"))?;
        nodes.push(DevnetNode {
            index,
            did,
            sphincs_pk_hex: pk_hex,
            authority: index < opts.authorities,
            root,
            config,
            home,
            wallet,
            compute_url: compute_urls[index].clone(),
        });
    }

    let layout = DevnetLayout {
        network_id: DEVNET_NETWORK_ID.into(),
        chain_id: DEVNET_CHAIN_ID,
        dir: dir.clone(),
        genesis_file,
        alloc_file,
        manifest: manifest_path,
        nodes,
    };
    std::fs::write(
        dir.join("devnet.json"),
        serde_json::to_vec_pretty(&layout).unwrap(),
    )
    .map_err(io("write devnet.json"))?;
    Ok(layout)
}

pub fn load(dir: &Path) -> Result<DevnetLayout, String> {
    let path = dir.join("devnet.json");
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("{}: {e} (run `spacekit network devnet init` first)", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", path.display()))
}

/// The command that runs `spacekit …` as one devnet node.
pub fn node_command(exe: &Path, node: &DevnetNode) -> Command {
    let mut command = Command::new(exe);
    command
        .env("HOME", &node.home)
        .env("SPACEKIT_NETWORK_CONFIG", &node.config)
        .env("SPACEKIT_SWTCHVM_DISABLE_PERSIST", "0")
        .env("SPACEKIT_COMPUTE_URL", &node.compute_url);
    if std::env::var_os("SPACEKIT_COMPUTE_BIN").is_none() {
        if let Some(bin) = exe.parent().map(|d| d.join("spacekit-compute-node")) {
            if bin.is_file() {
                command.env("SPACEKIT_COMPUTE_BIN", bin);
            }
        }
    }
    command
}

pub fn start_node(exe: &Path, node: &DevnetNode) -> Result<(), String> {
    let output = node_command(exe, node)
        .args(["network", "up", "--detach"])
        .output()
        .map_err(|e| format!("start node {}: {e}", node.index))?;
    std::fs::write(
        node.root.join("up.log"),
        format!(
            "stdout:\n{}\nstderr:\n{}\nstatus: {}\n",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            output.status
        ),
    )
    .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "node {} failed to start (see {}): {}",
            node.index,
            node.root.join("up.log").display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

pub fn stop_node(exe: &Path, node: &DevnetNode) {
    let _ = node_command(exe, node).args(["network", "down"]).output();
    // `network down` is best effort; make sure the supervisor is gone.
    let runtime = node.root.join("runtime.json");
    if let Ok(body) = std::fs::read_to_string(&runtime) {
        if let Ok(state) =
            serde_json::from_str::<crate::network_profile::NetworkRuntimeState>(&body)
        {
            if state.pid != 0 && crate::network_profile::process_alive(state.pid) {
                let _ = crate::network_profile::signal_process(state.pid);
            }
        }
    }
    let _ = std::fs::remove_file(runtime);
}

async fn node_status(client: &reqwest::Client, node: &DevnetNode) -> Value {
    let base = node.compute_url.trim_end_matches('/');
    let chain = async {
        client
            .get(format!("{base}/v1/chain/status"))
            .send()
            .await
            .ok()?
            .json::<Value>()
            .await
            .ok()
    };
    let governance = async {
        client
            .get(format!("{base}/v1/governance"))
            .send()
            .await
            .ok()?
            .json::<Value>()
            .await
            .ok()
    };
    let (chain, governance) = tokio::join!(chain, governance);
    json!({
        "index": node.index,
        "did": node.did,
        "authority_at_genesis": node.authority,
        "compute_url": node.compute_url,
        "up": chain.is_some(),
        "chain": chain,
        "governance": governance,
    })
}

fn short(value: Option<&str>, n: usize) -> String {
    match value {
        Some(v) if v.len() > n => format!("{}…", &v[..n]),
        Some(v) => v.to_string(),
        None => "—".into(),
    }
}

pub async fn handle(action: &DevnetAction) -> CmdResult {
    let exe = std::env::current_exe()?;
    match action {
        DevnetAction::Init {
            dir,
            authorities,
            observers,
            min_validators_to_lift,
            block_time_ms,
            production,
            batch_window_ms,
            heartbeat_secs,
            min_stake_astra,
            unbonding_days,
            pos_grace_days,
            base_port,
            fund,
            rewards,
            reward_epoch_secs,
            astra_rewards_wasm,
            affiliated,
            force,
        } => {
            let fund = fund
                .iter()
                .map(|entry| {
                    let (address, amount) = entry
                        .split_once('=')
                        .ok_or_else(|| format!("--fund {entry}: expected 0xADDRESS=AMOUNT"))?;
                    let amount: u128 = amount
                        .trim()
                        .parse()
                        .map_err(|_| format!("--fund {entry}: bad amount"))?;
                    Ok((address.trim().to_string(), amount))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let layout = create(&DevnetOptions {
                dir: dir.clone().unwrap_or_else(default_dir),
                authorities: *authorities,
                observers: *observers,
                min_validators_to_lift: *min_validators_to_lift,
                block_time_ms: *block_time_ms,
                production: production.as_str().to_string(),
                batch_window_ms: *batch_window_ms,
                heartbeat_secs: *heartbeat_secs,
                min_stake_astra: *min_stake_astra,
                unbonding_days: *unbonding_days,
                pos_grace_days: *pos_grace_days,
                ports: Vec::new(),
                base_port: *base_port,
                fund,
                rewards: *rewards,
                reward_epoch_secs: *reward_epoch_secs,
                astra_rewards_wasm: astra_rewards_wasm.clone(),
                affiliated: affiliated.clone(),
                force: *force,
            })?;
            println!(
                "{} {} ({} authorities, {} observers)",
                "✅ Devnet written to".green(),
                layout.dir.display().to_string().cyan(),
                authorities,
                observers
            );
            for node in &layout.nodes {
                println!(
                    "   node-{} {} {} {}",
                    node.index,
                    if node.authority { "authority" } else { "observer " },
                    node.compute_url,
                    node.did
                );
            }
            println!("   next: spacekit network devnet up && spacekit network devnet status");
            Ok(())
        }
        DevnetAction::Up { dir, nodes } => {
            let layout = load(&dir.clone().unwrap_or_else(default_dir))?;
            for node in layout.nodes.iter().filter(|n| nodes.is_empty() || nodes.contains(&n.index)) {
                start_node(&exe, node)?;
                println!("   {} node-{} {}", "✓".green(), node.index, node.compute_url);
            }
            Ok(())
        }
        DevnetAction::Down { dir, nodes } => {
            let layout = load(&dir.clone().unwrap_or_else(default_dir))?;
            for node in layout.nodes.iter().filter(|n| nodes.is_empty() || nodes.contains(&n.index)) {
                stop_node(&exe, node);
                println!("   {} node-{} stopped", "○".yellow(), node.index);
            }
            Ok(())
        }
        DevnetAction::Status { dir, json } => {
            let layout = load(&dir.clone().unwrap_or_else(default_dir))?;
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(3))
                .build()?;
            let mut rows = Vec::new();
            for node in &layout.nodes {
                rows.push(node_status(&client, node).await);
            }
            if *json {
                println!("{}", serde_json::to_string_pretty(&json!({ "nodes": rows }))?);
                return Ok(());
            }
            println!(
                "{:<7} {:<6} {:>7} {:<14} {:<22} {:<20} {:<14}",
                "node", "up", "height", "head", "proposer", "mode", "state_hash"
            );
            for row in &rows {
                let chain = &row["chain"];
                let gov = &row["governance"];
                println!(
                    "{:<7} {:<6} {:>7} {:<14} {:<22} {:<20} {:<14}",
                    format!("node-{}", row["index"]),
                    if row["up"] == true { "yes" } else { "no" },
                    chain.pointer("/head/number").and_then(Value::as_u64).map(|n| n.to_string()).unwrap_or_else(|| "—".into()),
                    short(chain.pointer("/head/hash").or(chain.pointer("/head/hash_hex")).and_then(Value::as_str), 12),
                    short(chain.pointer("/head/proposer_did").and_then(Value::as_str).map(|d| d.rsplit(':').next().unwrap_or(d)), 20),
                    short(gov["mode"].as_str(), 20),
                    short(gov["state_hash"].as_str(), 12),
                );
            }
            Ok(())
        }
    }
}
