//! Local network supervisor: `spacekit network up` / `down` / `start` / `stop`.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use colored::Colorize;
use spacekit_compute_node::ComputeConfig;
use spacekit_messaging_node::MessagingConfig;
use spacekit_storage_node::api::ServerConfig;
use spacekit_storage_node::{StorageNode, StorageNodeConfig};
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;

use crate::network_profile::{
    self, load_spacekit_network_file, NetworkMode, NetworkRuntimeServices, NetworkRuntimeState,
    NetworkService, ServiceRuntimeInfo, SpacekitNetworkFile,
};

struct SupervisorHandles {
    /// In-process storage (only when `SPACEKIT_EMBED_STORAGE=1`; default is subprocess).
    storage: Option<Arc<StorageNode>>,
    _storage_child: Option<tokio::process::Child>,
    _messaging_http: Option<tokio::process::Child>,
    _compute_child: Option<tokio::process::Child>,
    _compute_pid_path: Option<std::path::PathBuf>,
    _gateway: Option<tokio::process::Child>,
    _status_server: Option<tokio::task::JoinHandle<()>>,
    _keymaster_children: Vec<tokio::process::Child>,
}

/// Start the local network stack (foreground or detached).
pub async fn network_up(
    detach: bool,
    only: Option<Vec<NetworkService>>,
    full: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if network_profile::is_network_supervisor_running() {
        let state = network_profile::load_network_runtime_state()?.unwrap();
        return Err(format!(
            "network already running (pid {}, mode {:?})",
            state.pid, state.mode
        )
        .into());
    }

    let mut net = load_spacekit_network_file()?
        .ok_or("network profile not found — run `spacekit network init` first")?;

    if full {
        net.services.storage = true;
        net.services.messaging = true;
        net.services.compute = true;
        net.services.gateway = true;
        net.services.keymaster = true;
        net.blockchain.enabled = true;
        if !net.blockchain.persist_state {
            net.blockchain.persist_state = true;
        }
    }

    let only_for_spawn = only.clone();
    if let Some(ref list) = only {
        net = net.with_only_services(list);
    }

    if net.enabled_embedded_services().is_empty() && net.mode == NetworkMode::Embedded {
        return Err(
            "no embedded services enabled — edit [services] in network config or use --only".into(),
        );
    }

    let did = resolve_network_did().await.or_else(|error| {
        if net.profile == network_profile::NetworkPreset::Local {
            Ok("did:spacekit:network:local".to_string())
        } else {
            Err(error)
        }
    })?;
    network_profile::authorize_network_start(&net, &did)?;

    if net.mode == NetworkMode::External {
        return network_up_external(&net, detach).await;
    }

    if detach {
        let pid = spawn_detached_supervisor(only_for_spawn.as_ref())?;
        print_up_summary(&net, pid, true);
        return Ok(());
    }

    run_supervisor_foreground(&net).await
}

/// Start a single embedded service (profile must allow it).
pub async fn network_start(
    service: NetworkService,
    detach: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if network_profile::is_network_supervisor_running() {
        return Err(
            "network supervisor already running — use `spacekit network down` first, or `spacekit network up --only <services>`"
                .into(),
        );
    }
    network_up(detach, Some(vec![service]), false).await
}

/// Stop the network (whole supervisor). Per-service stop is not supported while others run.
pub async fn network_stop(
    service: Option<NetworkService>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(svc) = service {
        let state = network_profile::load_network_runtime_state()?;
        if let Some(state) = state {
            let running = [
                (NetworkService::Storage, state.services.storage.as_ref()),
                (NetworkService::Messaging, state.services.messaging.as_ref()),
                (NetworkService::Compute, state.services.compute.as_ref()),
                (NetworkService::Keymaster, state.services.keymaster.as_ref()),
            ];
            let count = running
                .iter()
                .filter(|(_, info)| info.map(|i| i.enabled).unwrap_or(false))
                .count();
            if count > 1 {
                return Err(format!(
                    "cannot stop only {} while other services run — use `spacekit network down`",
                    svc.as_str()
                )
                .into());
            }
        }
    }
    network_down().await
}

pub async fn network_down() -> Result<(), Box<dyn std::error::Error>> {
    let Some(state) = network_profile::load_network_runtime_state()? else {
        println!(
            "{}",
            "ℹ  No runtime state — network is not running.".yellow()
        );
        return Ok(());
    };

    if state.mode == NetworkMode::External || state.pid == 0 {
        network_profile::clear_network_runtime_state();
        println!("{}", "✅ External network profile cleared.".green());
        return Ok(());
    }

    if !network_profile::process_alive(state.pid) {
        network_profile::clear_network_runtime_state();
        println!(
            "{} stale runtime state cleared (pid {} not running)",
            "ℹ".yellow(),
            state.pid
        );
        return Ok(());
    }

    network_profile::signal_process(state.pid)?;
    println!(
        "{} sent stop to pid {}",
        "✅".green(),
        state.pid.to_string().cyan()
    );

    for _ in 0..30 {
        if !network_profile::process_alive(state.pid) {
            network_profile::clear_network_runtime_state();
            println!("{}", "✅ Network stopped.".green());
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    println!(
        "{} pid {} still running — try again or kill manually",
        "⚠".yellow(),
        state.pid
    );
    Ok(())
}

pub async fn run_supervisor_from_profile() -> Result<(), Box<dyn std::error::Error>> {
    let only = std::env::var("SPACEKIT_NETWORK_ONLY")
        .ok()
        .map(|s| NetworkService::parse_list(&s))
        .transpose()?;

    let mut net = load_spacekit_network_file()?
        .ok_or("network profile not found — run `spacekit network init` first")?;

    if let Some(list) = only {
        net = net.with_only_services(&list);
    }

    if net.mode == NetworkMode::External {
        return Err("run-supervisor is for embedded mode only".into());
    }

    run_supervisor_foreground(&net).await
}

async fn network_up_external(
    net: &SpacekitNetworkFile,
    _detach: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "{}",
        "🌐 External network mode — validating configured URLs (no embedded nodes).".cyan()
    );

    let storage_url = net.resolved_storage_url();
    let compute_url = net.resolved_compute_url();
    let listen = net.resolved_listen_addr();

    let timeout = Duration::from_secs(net.runtime.health_check_timeout_secs);
    let client = reqwest::Client::builder().timeout(timeout).build()?;

    let mut ok = true;
    if net.services.storage {
        let url = format!("{}/health", storage_url.trim_end_matches('/'));
        match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => {
                println!("   {} storage {}", "✓".green(), storage_url);
            }
            Ok(r) => {
                println!(
                    "   {} storage {} (HTTP {})",
                    "✗".red(),
                    storage_url,
                    r.status()
                );
                ok = false;
            }
            Err(e) => {
                println!("   {} storage {} ({})", "✗".red(), storage_url, e);
                ok = false;
            }
        }
    }
    if net.services.compute {
        let url = format!("{}/health", compute_url.trim_end_matches('/'));
        match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => {
                println!("   {} compute {}", "✓".green(), compute_url);
            }
            Ok(r) => {
                println!(
                    "   {} compute {} (HTTP {})",
                    "✗".red(),
                    compute_url,
                    r.status()
                );
                ok = false;
            }
            Err(e) => {
                println!("   {} compute {} ({})", "✗".red(), compute_url, e);
                ok = false;
            }
        }
    }
    if net.services.messaging {
        println!(
            "   {} messaging (listen {}, bootstrap peers: {})",
            "○".yellow(),
            listen,
            net.messaging.bootstrap_peers.len()
        );
    }

    if !ok {
        return Err("one or more external services failed health check".into());
    }

    let state = build_runtime_state(net, 0, NetworkMode::External);
    network_profile::write_network_runtime_state(&state)?;
    println!("{}", "✅ External network profile active.".green());
    Ok(())
}

fn spawn_detached_supervisor(
    only: Option<&Vec<NetworkService>>,
) -> Result<u32, Box<dyn std::error::Error>> {
    let exe = std::env::current_exe()?;
    let log_path = network_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stderr = stdout.try_clone()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(["network", "run-supervisor"]);
    if let Some(list) = only {
        let s = list
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(",");
        cmd.env("SPACEKIT_NETWORK_ONLY", s);
    }
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()?;
    Ok(child.id())
}

fn build_runtime_state(
    net: &SpacekitNetworkFile,
    pid: u32,
    mode: NetworkMode,
) -> NetworkRuntimeState {
    let storage_url = net.resolved_storage_url();
    let compute_url = net.resolved_compute_url();
    let listen = net.resolved_listen_addr();

    let svc = |enabled: bool, url: Option<String>, listen: Option<String>| {
        if enabled {
            Some(ServiceRuntimeInfo {
                enabled: true,
                url,
                listen,
            })
        } else {
            None
        }
    };

    NetworkRuntimeState {
        pid,
        started_at: Utc::now(),
        mode,
        compute_url: compute_url.clone(),
        storage_url: storage_url.clone(),
        messaging_listen: listen.clone(),
        services: NetworkRuntimeServices {
            storage: svc(net.services.storage, Some(storage_url), None),
            messaging: svc(net.services.messaging, None, Some(listen)),
            compute: svc(net.services.compute, Some(compute_url), None),
            gateway: svc(net.services.gateway, net.urls.gateway.clone(), None),
            keymaster: svc(
                net.services.keymaster,
                Some(net.resolved_keymaster_coordinator_url()),
                None,
            ),
        },
    }
}

fn print_up_summary(net: &SpacekitNetworkFile, pid: u32, detached: bool) {
    let label = if detached { "Started" } else { "Running" };
    println!(
        "{} {} supervisor pid {}",
        format!("✅ {}", label).green(),
        if net.mode == NetworkMode::External {
            "external"
        } else {
            "embedded"
        },
        pid.to_string().cyan()
    );
    if net.services.storage {
        println!("   storage:  {}", net.resolved_storage_url());
    }
    if net.services.compute {
        println!("   compute:  {}", net.resolved_compute_url());
    }
    if net.services.messaging {
        println!("   messaging:  {}", net.resolved_listen_addr());
    }
    if net.services.keymaster {
        println!(
            "   keymaster:  coordinator {}",
            net.resolved_keymaster_coordinator_url()
        );
        println!(
            "               registry   {}",
            net.resolved_keymaster_registry_url()
        );
    }
    println!("   stop with: {}", "spacekit network down".green());
}

async fn run_supervisor_foreground(
    net: &SpacekitNetworkFile,
) -> Result<(), Box<dyn std::error::Error>> {
    let did = resolve_network_did().await.or_else(|error| {
        if net.profile == network_profile::NetworkPreset::Local {
            Ok("did:spacekit:network:local".to_string())
        } else {
            Err(error)
        }
    })?;
    network_profile::authorize_network_start(net, &did)?;
    let state = build_runtime_state(net, std::process::id(), NetworkMode::Embedded);
    network_profile::write_network_runtime_state(&state)?;

    std::env::set_var("SPACEKIT_STORAGE_NODE_URL", net.resolved_storage_url());
    std::env::set_var("SPACEKIT_COMPUTE_URL", net.resolved_compute_url());
    std::env::set_var(
        "KEYMASTER_COORDINATOR_URL",
        net.resolved_keymaster_coordinator_url(),
    );
    std::env::set_var(
        "KEYMASTER_REGISTRY_URL",
        net.resolved_keymaster_registry_url(),
    );

    println!("{}", "🚀 Starting SpaceKit local network…".green().bold());
    println!("   identity: {}", did.cyan());
    println!(
        "   services: {}",
        net.enabled_embedded_services()
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(", ")
            .cyan()
    );

    let mut handles = SupervisorHandles {
        storage: None,
        _storage_child: None,
        _messaging_http: None,
        _compute_child: None,
        _compute_pid_path: None,
        _gateway: None,
        _status_server: None,
        _keymaster_children: Vec::new(),
    };

    if net.services.storage {
        if embed_storage_in_supervisor() {
            handles.storage = Some(start_storage(net, &did).await?);
        } else {
            handles._storage_child = Some(spawn_storage_process(net, &did)?);
            let p2p_label = if network_profile::resolve_enable_p2p(net) {
                format!("port {}", net.ports.storage_p2p)
            } else {
                "off".to_string()
            };
            println!(
                "   {} storage subprocess (HTTP {}, P2P {})",
                "●".green(),
                net.resolved_storage_url().cyan(),
                p2p_label.cyan()
            );
        }
    }
    if net.services.messaging {
        match spawn_messaging_http(net, &did) {
            Ok(child) => {
                println!(
                    "   {} messaging HTTP {} (P2P {})",
                    "●".green(),
                    net.resolved_messaging_http_url().cyan(),
                    net.resolved_listen_addr().cyan()
                );
                handles._messaging_http = Some(child);
            }
            Err(e) => {
                return Err(format!(
                    "messaging HTTP gateway failed to start ({}). \
                     Build: cargo build -p spacekit-messaging-node --features standalone, and set SPACEKIT_MESSAGING_HTTP_BIN to the path of the binary",
                    e
                )
                .into());
            }
        }
    }
    if net.services.compute {
        let (mut child, pid_path) = spawn_compute_process(net, &did)?;
        if let Err(error) = wait_for_compute_ready(net, &mut child).await {
            let _ = std::fs::remove_file(&pid_path);
            return Err(error);
        }
        println!(
            "   {} compute sidecar pid {} (HTTP {}, state {})",
            "●".green(),
            child.id().unwrap_or_default(),
            net.resolved_compute_url().cyan(),
            compute_state_path(net).display()
        );
        handles._compute_child = Some(child);
        handles._compute_pid_path = Some(pid_path);
    }
    if net.services.gateway {
        match spawn_gateway(net) {
            Ok(child) => {
                println!(
                    "   {} gateway (port {})",
                    "✓".green(),
                    net.ports.gateway_http
                );
                handles._gateway = Some(child);
            }
            Err(e) => {
                println!(
                    "   {} gateway (port {} — failed to start: {})",
                    "○".yellow(),
                    net.ports.gateway_http,
                    e
                );
            }
        }
    }

    if net.services.keymaster {
        match spawn_keymaster_stack(net).await {
            Ok(children) => {
                println!(
                    "   {} keymaster coordinator {} + registry {}",
                    "●".green(),
                    net.resolved_keymaster_coordinator_url().cyan(),
                    net.resolved_keymaster_registry_url().cyan(),
                );
                handles._keymaster_children = children;
            }
            Err(e) => {
                println!("   {} keymaster (failed to start: {})", "○".yellow(), e);
            }
        }
    }

    // The compute sidecar is the authoritative blockchain/SwtchVM service.  The supervisor
    // intentionally does not maintain a second reward-only ledger.
    if net.blockchain.enabled {
        println!(
            "   {} blockchain RPC provided by compute (chain_id={}, persistent state {})",
            "✓".green(),
            net.blockchain.chain_id,
            compute_state_path(net).display(),
        );
    }

    // ── Status dashboard server ──
    let status_port = net.ports.status_http;
    let status_state = state.clone();
    let status_net = net.clone();
    handles._status_server = Some(tokio::spawn(async move {
        if let Err(e) = run_status_server(status_port, status_state, status_net).await {
            eprintln!("status server on {}: {}", status_port, e);
        }
    }));

    println!();
    println!("{}", "✅ Network is up".green().bold());
    if net.services.storage {
        println!("   storage API:  {}", net.resolved_storage_url().cyan());
    }
    if net.services.compute {
        println!(
            "   compute API:  {}/health",
            net.resolved_compute_url().cyan()
        );
    }
    if net.services.messaging {
        println!(
            "   messaging HTTP: {}",
            net.resolved_messaging_http_url().cyan()
        );
        println!("   messaging P2P:  {}", net.resolved_listen_addr().cyan());
    }
    if net.blockchain.enabled {
        println!(
            "   blockchain:   chain_id={}, compute RPC authority",
            net.blockchain.chain_id.to_string().cyan(),
        );
    }
    println!(
        "   status:       {}",
        format!("http://{}:{}/status", net.bind_host, status_port).cyan()
    );
    println!("   pid:          {}", state.pid);
    println!();
    println!(
        "   Press Ctrl+C or run {} in another terminal.",
        "spacekit network down".green()
    );

    if net.services.storage || net.services.compute {
        if let Err(e) = wait_for_health(net).await {
            println!("{} health probe: {}", "⚠".yellow(), e);
        }
    }

    let compute_exited = if let Some(child) = handles._compute_child.as_mut() {
        tokio::select! {
            _ = shutdown_signal() => {
                println!("\n{}", "⏹  Shutting down network…".yellow());
                false
            }
            status = child.wait() => {
                match status {
                    Ok(status) => eprintln!(
                        "{} compute sidecar exited unexpectedly with {} (see {})",
                        "✗".red(),
                        status,
                        compute_log_path(net).display()
                    ),
                    Err(error) => eprintln!("{} failed waiting for compute sidecar: {}", "✗".red(), error),
                }
                true
            }
        }
    } else {
        shutdown_signal().await;
        println!("\n{}", "⏹  Shutting down network…".yellow());
        false
    };

    if let Some(mut child) = handles._storage_child {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    if let Some(mut child) = handles._messaging_http {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }

    if let Some(mut child) = handles._compute_child {
        if !compute_exited {
            terminate_child_gracefully(&mut child).await;
        }
    }
    if let Some(pid_path) = handles._compute_pid_path {
        let _ = std::fs::remove_file(pid_path);
    }

    drop(handles.storage);
    network_profile::clear_network_runtime_state();
    println!("{}", "✅ Network stopped.".green());
    if compute_exited {
        Err("compute sidecar exited; network stopped".into())
    } else {
        Ok(())
    }
}

async fn resolve_network_did() -> Result<String, Box<dyn std::error::Error>> {
    let config_dir = dirs::home_dir()
        .ok_or("home directory not found")?
        .join(".spacekit");
    let config_path = config_dir.join("config.toml");
    if !config_path.exists() {
        return Err("no ~/.spacekit/config.toml".into());
    }
    let s = std::fs::read_to_string(config_path)?;
    let v: toml::Value = toml::from_str(&s)?;
    v.get("identity")
        .and_then(|i| i.get("did"))
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "identity.did missing in config.toml".into())
}

async fn start_storage(
    net: &SpacekitNetworkFile,
    did: &str,
) -> Result<Arc<StorageNode>, Box<dyn std::error::Error>> {
    let data_dir = network_profile::resolve_data_dir(net, "storage");
    std::fs::create_dir_all(&data_dir)?;

    if let Some(ref secret) = net.runtime.upload_token_secret {
        let t = secret.trim();
        if !t.is_empty() {
            std::fs::write(data_dir.join(".upload_token_secret"), t.as_bytes())?;
        }
    } else {
        let _ = std::env::var("SPACEKIT_UPLOAD_TOKEN_SECRET")
            .ok()
            .and_then(|s| {
                let t = s.trim();
                if t.is_empty() {
                    None
                } else {
                    std::fs::write(data_dir.join(".upload_token_secret"), t.as_bytes()).ok()
                }
            });
    }

    let blob_fact_auth_mode = net
        .runtime
        .blob_fact_auth
        .as_deref()
        .and_then(spacekit_storage_node::access_policy::BlobFactAuthMode::parse)
        .unwrap_or_else(spacekit_storage_node::access_policy::BlobFactAuthMode::from_env);
    std::env::set_var("SPACEKIT_BLOB_FACT_AUTH", blob_fact_auth_mode.as_str());

    let port = net.ports.storage_http;
    std::env::set_var(
        "SPACEKIT_PUBLIC_HTTP_URL",
        format!("http://127.0.0.1:{port}"),
    );
    std::env::set_var("SPACEKIT_NODE_DID", did);
    let max_bytes = net
        .runtime
        .max_storage_gb
        .saturating_mul(1024 * 1024 * 1024);
    let mut config = StorageNodeConfig::default();
    config.max_storage_bytes = max_bytes;
    config.data_dir = data_dir.clone();
    config.database_path = Some(data_dir.join("storage.db"));
    config.node_did = did.to_string();
    config.preferred_algorithm = net.runtime.quantum_algorithm.clone();
    let enable_p2p = network_profile::resolve_enable_p2p(net);
    config.enable_p2p = enable_p2p;
    config.network_config.listen_port = net.ports.storage_p2p;
    config.network_config.cache_p2p_chunks_in_memory = net.runtime.cache_p2p_chunks_in_memory;
    config.persistence.externalize_documents = true;
    config.persistence.document_inline_max_bytes = 4096;
    config.persistence.blob_cache_max_bytes = 32 * 1024 * 1024;
    config.api_config = Some(ServerConfig {
        port,
        public_key: String::new(),
        enable_cors: true,
        blob_fact_auth_mode,
    });

    let node = StorageNode::new(config).await?;
    node.start().await?;
    if enable_p2p {
        println!(
            "   {} storage (HTTP {}, P2P {} — chunk RAM cache: {})",
            "●".green(),
            port,
            net.ports.storage_p2p,
            if net.runtime.cache_p2p_chunks_in_memory {
                "on"
            } else {
                "off"
            }
        );
    } else {
        println!(
            "   {} storage (HTTP {}, P2P off — disk only)",
            "●".green(),
            port,
        );
    }
    Ok(Arc::new(node))
}

fn spawn_messaging_http(
    net: &SpacekitNetworkFile,
    did: &str,
) -> Result<tokio::process::Child, Box<dyn std::error::Error>> {
    let data_dir = network_profile::resolve_data_dir(net, "messaging");
    std::fs::create_dir_all(&data_dir)?;

    let private_key = load_or_create_messaging_key()?;
    let listen_addr: SocketAddr = net.resolved_listen_addr().parse().map_err(|e| {
        format!(
            "invalid messaging listen `{}`: {}",
            net.resolved_listen_addr(),
            e
        )
    })?;

    let mut config = MessagingConfig::default();
    // Keep the manifest-admitted identity intact across the HTTP and P2P planes.
    config.node_did = did.to_string();
    config.private_key = private_key;
    config.listen_addr = listen_addr;
    config.bootstrap_peers = if net.messaging.bootstrap_peers.is_empty()
        && net.profile == network_profile::NetworkPreset::Local
    {
        vec![net.default_bootstrap_multiaddr()]
    } else {
        net.messaging.bootstrap_peers.clone()
    };
    config.default_quantum_algorithm = net.runtime.quantum_algorithm.clone();
    config.default_cipher_suite = "AES256".to_string();
    config.enable_peer_discovery = true;
    config.network.enable_encryption = true;
    config.storage.storage_path = data_dir.to_string_lossy().to_string();
    config.storage.enable_persistence = true;

    let config_path = data_dir.join("messaging_http_config.json");
    config.to_file(config_path.to_str().unwrap())?;

    let http_listen = format!("{}:{}", net.bind_host.trim(), net.ports.messaging_http);
    let log_path = data_dir.join("messaging.log");
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stderr = stdout.try_clone()?;
    let bin = std::env::var_os("SPACEKIT_MESSAGING_HTTP_BIN").unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|path| {
                path.parent()
                    .map(|parent| parent.join("spacekit-messaging-http"))
            })
            .filter(|path| path.is_file())
            .map(Into::into)
            .unwrap_or_else(|| "spacekit-messaging-http".into())
    });

    let child = tokio::process::Command::new(&bin)
        .env("SPACEKIT_MESSAGING_HTTP_LISTEN", &http_listen)
        .env(
            "SPACEKIT_MESSAGING_CONFIG",
            config_path.to_string_lossy().as_ref(),
        )
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| {
            format!(
                "spawn {} ({}): {}",
                std::path::Path::new(&bin).display(),
                http_listen,
                e
            )
        })?;

    Ok(child)
}

fn compute_data_dir(net: &SpacekitNetworkFile) -> std::path::PathBuf {
    network_profile::resolve_data_dir(net, "compute")
}

fn compute_state_path(net: &SpacekitNetworkFile) -> std::path::PathBuf {
    net.blockchain
        .state_dir
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| compute_data_dir(net))
        .join("swtchvm-state.bin")
}

fn compute_log_path(net: &SpacekitNetworkFile) -> std::path::PathBuf {
    compute_data_dir(net).join("compute.log")
}

pub fn network_log_path() -> std::path::PathBuf {
    network_profile::network_instance_path("network.log")
}

pub fn log_paths(
    net: &SpacekitNetworkFile,
    service: Option<NetworkService>,
) -> Vec<(&'static str, std::path::PathBuf)> {
    match service {
        Some(NetworkService::Compute) => vec![("compute", compute_log_path(net))],
        Some(NetworkService::Storage) => vec![("storage/supervisor", network_log_path())],
        Some(NetworkService::Messaging) => vec![("messaging/supervisor", network_log_path())],
        Some(NetworkService::Gateway) => vec![("gateway", network_log_path())],
        Some(NetworkService::Keymaster) => vec![("keymaster", network_log_path())],
        None => vec![
            ("supervisor", network_log_path()),
            ("compute", compute_log_path(net)),
            (
                "storage",
                network_profile::resolve_data_dir(net, "storage").join("storage.log"),
            ),
            (
                "messaging",
                network_profile::resolve_data_dir(net, "messaging").join("messaging.log"),
            ),
        ],
    }
}

pub fn reset_network_data(
    net: &SpacekitNetworkFile,
) -> Result<Vec<std::path::PathBuf>, Box<dyn std::error::Error>> {
    if network_profile::is_network_supervisor_running() {
        return Err(
            "network is running — run `spacekit network down` before resetting data".into(),
        );
    }

    let mut removed = Vec::new();
    for path in [
        network_profile::resolve_data_dir(net, "storage"),
        network_profile::resolve_data_dir(net, "compute"),
        network_profile::resolve_data_dir(net, "messaging"),
    ] {
        let path = path.canonicalize().unwrap_or(path);
        if path == std::path::Path::new("/")
            || dirs::home_dir().as_ref().is_some_and(|home| home == &path)
            || std::env::current_dir()
                .as_ref()
                .is_ok_and(|cwd| cwd == &path)
        {
            return Err(format!("refusing to remove unsafe data path {}", path.display()).into());
        }
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
            removed.push(path);
        }
    }
    network_profile::clear_network_runtime_state();
    Ok(removed)
}

fn toml_table(entries: impl IntoIterator<Item = (&'static str, toml::Value)>) -> toml::Value {
    toml::Value::Table(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_string(), value))
            .collect(),
    )
}

fn compute_network_settings(
    net: &SpacekitNetworkFile,
) -> Result<(String, Vec<String>), Box<dyn std::error::Error>> {
    let (name, bootstrap) = if let Some(path) = &net.manifest {
        let manifest = network_profile::load_network_manifest(path)?;
        (manifest.network_id, manifest.bootstrap.p2p)
    } else {
        let name = match net.profile {
            network_profile::NetworkPreset::Local => "spacekit-local",
            network_profile::NetworkPreset::Private => "spacekit-private",
            network_profile::NetworkPreset::Public => "spacekit-public",
        };
        (name.to_string(), net.messaging.bootstrap_peers.clone())
    };
    let bootstrap = bootstrap
        .iter()
        .map(|address| compute_bootstrap_socket(address))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((name, bootstrap))
}

fn compute_bootstrap_socket(address: &str) -> Result<String, Box<dyn std::error::Error>> {
    if !address.starts_with('/') {
        return Ok(address.to_string());
    }
    let parts = address
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() == 4 && matches!(parts[0], "ip4" | "ip6" | "dns4" | "dns6") && parts[2] == "tcp"
    {
        let host = if parts[0] == "ip6" {
            format!("[{}]", parts[1])
        } else {
            parts[1].to_string()
        };
        return Ok(format!("{host}:{}", parts[3]));
    }
    Err(format!("unsupported compute bootstrap multiaddr `{address}`").into())
}

fn write_compute_config(
    net: &SpacekitNetworkFile,
    did: &str,
    storage_service_running: bool,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let data_dir = compute_data_dir(net);
    std::fs::create_dir_all(&data_dir)?;
    let state_path = compute_state_path(net);
    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (network_name, compute_bootstrap) = compute_network_settings(net)?;
    let mut config = ComputeConfig::default();
    config.node_did = did.to_string();
    config.quantum_security_enabled = false;
    config.embedded_supervisor_mode = false;
    config.production_metrics_config.enabled = false;
    config.layerzero_bridge_config.enabled = false;
    config.swtchvm_state_path = Some(state_path);
    config.chain_id = net.blockchain.chain_id.to_string();
    // Storage remains a separately managed service. The standalone node must not create a
    // second storage database just because the profile omitted storage.
    config.storage_config.enable_storage_integration = false;
    config.storage_config.storage_data_dir = data_dir.to_string_lossy().to_string();
    config.storage_config.auto_store_inputs = false;

    let key_dir = data_dir.join("keys");
    let mut root = toml::map::Map::new();
    root.insert(
        "identity".into(),
        toml_table([
            ("did", toml::Value::String(did.to_string())),
            (
                "private_key_path",
                toml::Value::String(key_dir.join("private_key.hex").display().to_string()),
            ),
            (
                "public_key_path",
                toml::Value::String(key_dir.join("public_key.hex").display().to_string()),
            ),
            (
                "quantum_algorithm",
                toml::Value::String("Kyber1024".to_string()),
            ),
        ]),
    );
    root.insert("compute".into(), toml::Value::try_from(&config)?);
    root.insert(
        "network".into(),
        toml_table([
            ("name", toml::Value::String(network_name)),
            ("endpoint", toml::Value::String(net.resolved_storage_url())),
            (
                "p2p_port",
                toml::Value::Integer(i64::from(net.ports.compute_p2p)),
            ),
            (
                "rpc_port",
                toml::Value::Integer(i64::from(net.ports.compute_http)),
            ),
            (
                "bootstrap_nodes",
                toml::Value::Array(
                    compute_bootstrap
                        .into_iter()
                        .map(toml::Value::String)
                        .collect(),
                ),
            ),
            ("enable_http_api", toml::Value::Boolean(true)),
            (
                "dev_mode",
                toml::Value::Boolean(net.profile == network_profile::NetworkPreset::Local),
            ),
            (
                "allow_single_validator_finalize",
                toml::Value::Boolean(net.blockchain.validators.self_validate),
            ),
            ("bind_address", toml::Value::String(net.bind_host.clone())),
        ]),
    );
    root.insert(
        "security".into(),
        toml_table([
            ("quantum_encryption", toml::Value::Boolean(false)),
            (
                "supported_algorithms",
                toml::Value::Array(vec![toml::Value::String("Kyber768".to_string())]),
            ),
            (
                "default_algorithm",
                toml::Value::String("Kyber768".to_string()),
            ),
            ("secure_enclaves", toml::Value::Boolean(false)),
        ]),
    );
    root.insert(
        "token".into(),
        toml_table([
            (
                "contract_address",
                toml::Value::String("0x0000000000000000000000000000000000000000".to_string()),
            ),
            ("minimum_stake", toml::Value::Integer(0)),
            ("service_fee_percent", toml::Value::Float(0.0)),
            ("settlement_interval_seconds", toml::Value::Integer(3600)),
        ]),
    );

    let config_path = data_dir.join("standalone.toml");
    let temp_path = data_dir.join("standalone.toml.tmp");
    std::fs::write(
        &temp_path,
        toml::to_string_pretty(&toml::Value::Table(root))?,
    )?;
    std::fs::rename(temp_path, &config_path)?;

    if storage_service_running {
        std::env::set_var("SPACEKIT_STORAGE_NODE_URL", net.resolved_storage_url());
    }
    Ok(config_path)
}

fn spawn_compute_process(
    net: &SpacekitNetworkFile,
    did: &str,
) -> Result<(tokio::process::Child, std::path::PathBuf), Box<dyn std::error::Error>> {
    let data_dir = compute_data_dir(net);
    std::fs::create_dir_all(&data_dir)?;
    let pid_path = data_dir.join("compute.pid");
    if let Ok(raw) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = raw.trim().parse::<u32>() {
            if network_profile::process_alive(pid) {
                return Err(format!(
                    "compute sidecar pid {} is already alive ({}); stop it or remove the stale network",
                    pid,
                    pid_path.display()
                )
                .into());
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    let config_path = write_compute_config(net, did, net.services.storage)?;
    let (network_name, compute_bootstrap) = compute_network_settings(net)?;
    let bin = resolve_sidecar_bin("SPACEKIT_COMPUTE_BIN", "spacekit-compute-node");
    let log_path = compute_log_path(net);
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stderr = stdout.try_clone()?;

    let mut command = tokio::process::Command::new(&bin);
    command
        .arg("--config")
        .arg(&config_path)
        .arg("--node-did")
        .arg(did)
        .arg("--network")
        .arg(network_name)
        .arg("--port")
        .arg(net.ports.compute_http.to_string())
        .arg("--p2p-port")
        .arg(net.ports.compute_p2p.to_string());
    for bootstrap in compute_bootstrap {
        command.arg("--bootstrap").arg(bootstrap);
    }
    command
        .arg("start")
        .env_remove("SPACEKIT_SWTCHVM_DISABLE_PERSIST")
        .env(
            "SPACEKIT_DEV_MODE",
            if net.profile == network_profile::NetworkPreset::Local {
                "1"
            } else {
                "0"
            },
        )
        .env(
            "SPACEKIT_DID_REGISTRY_PATH",
            data_dir.join("did_registry.json"),
        )
        .env("SPACEKIT_STORAGE_NODE_URL", net.resolved_storage_url())
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true);

    let child = command.spawn().map_err(|error| {
        format!(
            "spawn {}: {} — build with `cargo build -p spacekit-compute-node --release --features standalone` or set SPACEKIT_COMPUTE_BIN",
            bin.display(),
            error
        )
    })?;
    let pid = child.id().ok_or("compute sidecar did not report a pid")?;
    std::fs::write(&pid_path, format!("{pid}\n"))?;
    Ok((child, pid_path))
}

async fn wait_for_compute_ready(
    net: &SpacekitNetworkFile,
    child: &mut tokio::process::Child,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()?;
    let health_url = format!(
        "{}/health",
        net.resolved_compute_url().trim_end_matches('/')
    );
    let attempts = net.runtime.health_check_timeout_secs.max(1) * 4;
    for _ in 0..attempts {
        if let Some(status) = child.try_wait()? {
            return Err(format!(
                "compute sidecar exited before readiness with {} (see {})",
                status,
                compute_log_path(net).display()
            )
            .into());
        }
        if client
            .get(&health_url)
            .send()
            .await
            .map(|response| response.status().is_success())
            .unwrap_or(false)
        {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    Err(format!(
        "timed out waiting for compute readiness at {} (see {})",
        health_url,
        compute_log_path(net).display()
    )
    .into())
}

async fn terminate_child_gracefully(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        let _ = network_profile::signal_process(pid);
    }
    if tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .is_err()
    {
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = terminate.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

fn embed_storage_in_supervisor() -> bool {
    std::env::var("SPACEKIT_EMBED_STORAGE")
        .ok()
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

fn resolve_sidecar_bin(env_key: &str, default_name: &str) -> std::path::PathBuf {
    if let Ok(p) = std::env::var(env_key) {
        let t = p.trim();
        if !t.is_empty() {
            return std::path::PathBuf::from(t);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(default_name);
            if sibling.exists() {
                return sibling;
            }
        }
    }
    std::path::PathBuf::from(default_name)
}

async fn spawn_keymaster_stack(
    net: &SpacekitNetworkFile,
) -> Result<Vec<tokio::process::Child>, Box<dyn std::error::Error>> {
    let mut children = Vec::new();
    let storage_url = net.resolved_storage_url();
    let coord_url = net.resolved_keymaster_coordinator_url();
    let registry_url = net.resolved_keymaster_registry_url();

    std::env::set_var("KEYMASTER_COORDINATOR_URL", &coord_url);
    std::env::set_var("KEYMASTER_REGISTRY_URL", &registry_url);
    std::env::set_var("KEYMASTER_STORAGE_URL", &storage_url);

    let coord_bin = resolve_sidecar_bin(
        "SPACEKIT_KEYMASTER_COORDINATOR_BIN",
        "spacekit-keymaster-coordinator",
    );
    let coord_port = net.ports.keymaster_coordinator;
    let coordinator = tokio::process::Command::new(&coord_bin)
        .arg("--port")
        .arg(coord_port.to_string())
        .arg("--storage-url")
        .arg(&storage_url)
        .env("KEYMASTER_STORAGE_URL", &storage_url)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            format!(
                "spawn {} (coordinator {}): {} — build with `cargo build --release -p spacekit-keymaster`",
                coord_bin.display(),
                coord_url,
                e
            )
        })?;
    children.push(coordinator);

    // Wait for coordinator to accept connections.
    let info_url = format!("{coord_url}/v1/coordinator/info");
    for _ in 0..40 {
        if reqwest::get(&info_url).await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    let guardian_bin = resolve_sidecar_bin(
        "SPACEKIT_KEYMASTER_GUARDIAN_BIN",
        "spacekit-keymaster-guardian",
    );
    let operators = ["meridian", "atlas", "vesper", "corona", "halcyon"];
    for (i, op) in operators.iter().enumerate() {
        let port = net.ports.keymaster_guardian_base + i as u16;
        let child = tokio::process::Command::new(&guardian_bin)
            .arg("--port")
            .arg(port.to_string())
            .arg("--operator")
            .arg(op)
            .env("KEYMASTER_COORDINATOR_URL", &coord_url)
            .env("KEYMASTER_DEV", "1")
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("spawn guardian {op} on {port}: {e}"))?;
        children.push(child);
    }

    let registry_bin = resolve_sidecar_bin(
        "SPACEKIT_KEYMASTER_REGISTRY_BIN",
        "spacekit-keymaster-registry",
    );
    let registry = tokio::process::Command::new(&registry_bin)
        .arg("--port")
        .arg(net.ports.keymaster_registry.to_string())
        .env("KEYMASTER_COORDINATOR_URL", &coord_url)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn registry: {e}"))?;
    children.push(registry);

    Ok(children)
}

fn spawn_storage_process(
    net: &SpacekitNetworkFile,
    did: &str,
) -> Result<tokio::process::Child, Box<dyn std::error::Error>> {
    let data_dir = network_profile::resolve_data_dir(net, "storage");
    std::fs::create_dir_all(&data_dir)?;

    if let Some(ref secret) = net.runtime.upload_token_secret {
        let t = secret.trim();
        if !t.is_empty() {
            std::fs::write(data_dir.join(".upload_token_secret"), t.as_bytes())?;
        }
    } else if let Ok(s) = std::env::var("SPACEKIT_UPLOAD_TOKEN_SECRET") {
        let t = s.trim();
        if !t.is_empty() {
            let _ = std::fs::write(data_dir.join(".upload_token_secret"), t.as_bytes());
        }
    }

    let blob_fact_auth_mode = net
        .runtime
        .blob_fact_auth
        .as_deref()
        .and_then(spacekit_storage_node::access_policy::BlobFactAuthMode::parse)
        .unwrap_or_else(spacekit_storage_node::access_policy::BlobFactAuthMode::from_env);
    std::env::set_var("SPACEKIT_BLOB_FACT_AUTH", blob_fact_auth_mode.as_str());

    let port = net.ports.storage_http;
    std::env::set_var(
        "SPACEKIT_PUBLIC_HTTP_URL",
        format!("http://127.0.0.1:{port}"),
    );
    std::env::set_var("SPACEKIT_NODE_DID", did);

    let bin = resolve_sidecar_bin("SPACEKIT_STORAGE_BIN", "spacekit-storage-node");
    let enable_p2p = network_profile::resolve_enable_p2p(net);

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("start")
        // The storage API builds a large Warp filter graph on a Tokio worker.
        // Debug builds can exceed Tokio's default worker stack while assembling
        // it, so managed sidecars use an explicit, bounded worker stack.
        .env(
            "RUST_MIN_STACK",
            std::env::var("RUST_MIN_STACK").unwrap_or_else(|_| "8388608".into()),
        )
        .arg("--data-dir")
        .arg(&data_dir)
        .arg("--port")
        .arg(port.to_string())
        .arg("--did")
        .arg(did)
        .arg("--algorithm")
        .arg(&net.runtime.quantum_algorithm)
        .arg("--max-storage-gb")
        .arg(net.runtime.max_storage_gb.to_string())
        .arg("--externalize-documents")
        .arg("--document-inline-max-bytes")
        .arg("4096")
        .arg("--blob-cache-max-bytes")
        .arg((32 * 1024 * 1024).to_string());
    if !enable_p2p {
        cmd.arg("--disable-p2p");
    } else {
        cmd.arg("--p2p-port").arg(net.ports.storage_p2p.to_string());
    }

    let child = cmd
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| {
            format!(
                "spawn {} (storage HTTP {}): {} — build with `cargo build -p spacekit-storage-node --release --features standalone` or set SPACEKIT_STORAGE_BIN",
                bin.display(),
                net.resolved_storage_url(),
                e
            )
        })?;
    Ok(child)
}

fn spawn_gateway(
    net: &SpacekitNetworkFile,
) -> Result<tokio::process::Child, Box<dyn std::error::Error>> {
    let storage_cmd = format!(
        "spacekit-storage-node mcp --data-dir {}",
        network_profile::resolve_data_dir(net, "storage").display()
    );
    let compute_cmd = "spacekit-compute-node mcp".to_string();

    let child = tokio::process::Command::new("spacekit-gateway")
        .arg("--port")
        .arg(net.ports.gateway_http.to_string())
        .arg("--storage-cmd")
        .arg(&storage_cmd)
        .arg("--compute-cmd")
        .arg(&compute_cmd)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()?;
    Ok(child)
}

fn load_or_create_messaging_key() -> Result<String, Box<dyn std::error::Error>> {
    let path = network_profile::network_messaging_key_path();
    if path.exists() {
        return Ok(std::fs::read_to_string(&path)?.trim().to_string());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let key = hex::encode(rand_bytes(32));
    std::fs::write(&path, &key)?;
    Ok(key)
}

fn rand_bytes(n: usize) -> Vec<u8> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut out = Vec::with_capacity(n);
    let mut s = seed as u64;
    for _ in 0..n {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
        out.push((s >> 33) as u8);
    }
    out
}

/// Serve a JSON status dashboard at `GET /status` and `GET /config`.
/// Aggregates live health probes from each running service.
async fn run_status_server(
    port: u16,
    state: NetworkRuntimeState,
    net: SpacekitNetworkFile,
) -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = format!("{}:{}", net.bind_host, port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    println!(
        "   {} status dashboard (http://{}:{})",
        "✓".green(),
        net.bind_host,
        port
    );

    loop {
        let (mut stream, _) = listener.accept().await?;
        let state = state.clone();
        let net = net.clone();
        let client = client.clone();

        tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            let _ = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await;
            let req = String::from_utf8_lossy(&buf);

            let (status_line, body) = if req.starts_with("GET /config") {
                let config_json = serde_json::json!({
                    "mode": format!("{:?}", net.mode),
                    "bind_host": net.bind_host,
                    "services": {
                        "storage": net.services.storage,
                        "compute": net.services.compute,
                        "messaging": net.services.messaging,
                        "gateway": net.services.gateway,
                    },
                    "ports": {
                        "storage_http": net.ports.storage_http,
                        "storage_p2p": net.ports.storage_p2p,
                        "compute_http": net.ports.compute_http,
                        "messaging_listen": net.ports.messaging_listen,
                        "messaging_http": net.ports.messaging_http,
                        "messaging_bootstrap": net.ports.messaging_bootstrap,
                        "gateway_http": net.ports.gateway_http,
                        "status_http": net.ports.status_http,
                    },
                    "urls": {
                        "storage": net.urls.storage,
                        "compute": net.urls.compute,
                        "gateway": net.urls.gateway,
                    },
                    "config_path": network_profile::spacekit_network_config_path().display().to_string(),
                });
                (
                    "200 OK",
                    serde_json::to_string_pretty(&config_json).unwrap_or_default(),
                )
            } else if req.starts_with("GET /status")
                || req.starts_with("GET / ")
                || req.starts_with("GET /\r")
                || req.starts_with("GET / HTTP")
            {
                let uptime_secs =
                    (chrono::Utc::now() - state.started_at).num_seconds().max(0) as u64;

                let mut services = serde_json::Map::new();

                if net.services.storage {
                    let url = net.resolved_storage_url();
                    let healthy = client
                        .get(format!("{}/health", url.trim_end_matches('/')))
                        .send()
                        .await
                        .map(|r| r.status().is_success())
                        .unwrap_or(false);
                    services.insert(
                        "storage".into(),
                        serde_json::json!({
                            "enabled": true,
                            "url": url,
                            "status": if healthy { "healthy" } else { "unhealthy" },
                        }),
                    );
                } else {
                    services.insert("storage".into(), serde_json::json!({ "enabled": false }));
                }

                if net.services.compute {
                    let url = net.resolved_compute_url();
                    let healthy = client
                        .get(format!("{}/health", url.trim_end_matches('/')))
                        .send()
                        .await
                        .map(|r| r.status().is_success())
                        .unwrap_or(false);
                    services.insert(
                        "compute".into(),
                        serde_json::json!({
                            "enabled": true,
                            "url": url,
                            "status": if healthy { "healthy" } else { "unhealthy" },
                        }),
                    );
                } else {
                    services.insert("compute".into(), serde_json::json!({ "enabled": false }));
                }

                if net.services.messaging {
                    let http_url = net.resolved_messaging_http_url();
                    let healthy = client
                        .get(format!("{}/health", http_url.trim_end_matches('/')))
                        .send()
                        .await
                        .map(|r| r.status().is_success())
                        .unwrap_or(false);
                    services.insert(
                        "messaging".into(),
                        serde_json::json!({
                            "enabled": true,
                            "http": http_url,
                            "listen": net.resolved_listen_addr(),
                            "status": if healthy { "healthy" } else { "unhealthy" },
                        }),
                    );
                } else {
                    services.insert("messaging".into(), serde_json::json!({ "enabled": false }));
                }

                if net.services.gateway {
                    let url = net.urls.gateway.clone().unwrap_or_default();
                    let healthy = if !url.is_empty() {
                        client
                            .get(format!("{}/health", url.trim_end_matches('/')))
                            .send()
                            .await
                            .map(|r| r.status().is_success())
                            .unwrap_or(false)
                    } else {
                        false
                    };
                    services.insert(
                        "gateway".into(),
                        serde_json::json!({
                            "enabled": true,
                            "url": url,
                            "status": if healthy { "healthy" } else { "unhealthy" },
                        }),
                    );
                } else {
                    services.insert("gateway".into(), serde_json::json!({ "enabled": false }));
                }

                let blockchain_status = if net.blockchain.enabled {
                    serde_json::json!({
                        "enabled": true,
                        "chain_id": net.blockchain.chain_id,
                        "authority": "compute-sidecar",
                        "rpc_url": net.resolved_compute_url(),
                        "state_path": compute_state_path(&net).display().to_string(),
                    })
                } else {
                    serde_json::json!({ "enabled": false })
                };

                let body_json = serde_json::json!({
                    "pid": state.pid,
                    "uptime_secs": uptime_secs,
                    "started_at": state.started_at.to_rfc3339(),
                    "mode": format!("{:?}", state.mode),
                    "identity": state.messaging_listen,
                    "services": serde_json::Value::Object(services),
                    "blockchain": blockchain_status,
                    "config_path": network_profile::spacekit_network_config_path().display().to_string(),
                });
                (
                    "200 OK",
                    serde_json::to_string_pretty(&body_json).unwrap_or_default(),
                )
            } else {
                (
                    "404 Not Found",
                    r#"{"error":"not found","endpoints":["/status","/config"]}"#.to_string(),
                )
            };

            let is_browser = req.contains("text/html");
            let (content_type, final_body) = if is_browser && status_line == "200 OK" {
                (
                    "text/html",
                    format!(
                        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>SpaceKit Network Status</title>
<style>
body {{ font-family: -apple-system, system-ui, sans-serif; background: #0a0a0a; color: #e0e0e0; max-width: 720px; margin: 40px auto; padding: 0 20px; }}
h1 {{ color: #67e8f9; font-size: 1.4em; }}
pre {{ background: #1a1a2e; padding: 16px; border-radius: 8px; overflow-x: auto; font-size: 0.9em; line-height: 1.5; }}
.ok {{ color: #4ade80; }} .err {{ color: #f87171; }} .off {{ color: #6b7280; }}
a {{ color: #67e8f9; }}
</style></head><body>
<h1>SpaceKit Network Status</h1>
<pre>{}</pre>
<p style="color:#6b7280;font-size:0.85em">JSON: <a href="/status">/status</a> · <a href="/config">/config</a></p>
</body></html>"#,
                        body.replace("\"healthy\"", "<span class=\"ok\">\"healthy\"</span>")
                            .replace("\"unhealthy\"", "<span class=\"err\">\"unhealthy\"</span>")
                            .replace("\"running\"", "<span class=\"ok\">\"running\"</span>")
                            .replace("false", "<span class=\"off\">false</span>")
                    ),
                )
            } else {
                ("application/json", body)
            };

            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: {}\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                status_line,
                content_type,
                final_body.len(),
                final_body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        });
    }
}

async fn wait_for_health(net: &SpacekitNetworkFile) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    let storage_url = net.resolved_storage_url();
    let compute_url = net.resolved_compute_url();

    for _ in 0..15 {
        let storage_ok = if net.services.storage {
            client
                .get(format!("{}/health", storage_url.trim_end_matches('/')))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        } else {
            true
        };
        let compute_ok = if net.services.compute {
            client
                .get(format!("{}/health", compute_url.trim_end_matches('/')))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false)
        } else {
            true
        };
        if storage_ok && compute_ok {
            println!("{}", "✓ Health checks passed (enabled services)".green());
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    Err("timed out waiting for /health on enabled services".into())
}

pub fn runtime_status_lines() -> Vec<String> {
    let mut lines = Vec::new();
    match network_profile::load_network_runtime_state() {
        Ok(Some(state)) => {
            let alive = if state.mode == NetworkMode::External || state.pid == 0 {
                true
            } else {
                network_profile::process_alive(state.pid)
            };
            lines.push(format!(
                "Supervisor: pid {} ({}) since {} [{:?}]",
                if state.pid == 0 {
                    "—".to_string()
                } else {
                    state.pid.to_string()
                },
                if alive {
                    "running".green().to_string()
                } else {
                    "stopped".red().to_string()
                },
                state.started_at,
                state.mode
            ));
            for (name, info) in [
                ("storage", &state.services.storage),
                ("messaging", &state.services.messaging),
                ("compute", &state.services.compute),
                ("gateway", &state.services.gateway),
            ] {
                if let Some(i) = info {
                    if i.enabled {
                        let detail = i.url.as_deref().or(i.listen.as_deref()).unwrap_or("—");
                        lines.push(format!("  {}: {}", name, detail));
                    }
                }
            }
            if let Ok(Some(net)) = network_profile::load_spacekit_network_file() {
                lines.push(format!(
                    "  status: http://{}:{}",
                    net.bind_host, net.ports.status_http
                ));
            }
        }
        Ok(None) => {
            lines.push("Supervisor: not running".to_string());
            if let Ok(Some(net)) = network_profile::load_spacekit_network_file() {
                lines.push(format!("  profile mode: {:?}", net.mode));
                lines.push(format!(
                    "  configured: {}",
                    net.enabled_embedded_services()
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        Err(e) => {
            lines.push(format!("Supervisor: error reading runtime — {}", e));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_compute_config_uses_profile_rpc_and_durable_state() {
        let unique = format!(
            "spacekit-supervisor-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let data_dir = std::env::temp_dir().join(unique);
        let mut net = SpacekitNetworkFile::for_preset(network_profile::NetworkPreset::Local);
        net.data.compute = Some(data_dir.clone());
        net.ports.compute_http = 19444;
        net.ports.compute_p2p = 19445;
        net.blockchain.chain_id = 31337;

        let config_path =
            write_compute_config(&net, "did:spacekit:test", false).expect("write config");
        let document: toml::Value =
            toml::from_str(&std::fs::read_to_string(config_path).expect("read generated config"))
                .expect("parse generated config");

        assert_eq!(document["network"]["rpc_port"].as_integer(), Some(19444));
        assert_eq!(document["network"]["p2p_port"].as_integer(), Some(19445));
        assert_eq!(document["network"]["name"].as_str(), Some("spacekit-local"));
        assert_eq!(document["compute"]["chain_id"].as_str(), Some("31337"));
        assert_eq!(
            document["compute"]["swtchvm_state_path"].as_str(),
            Some(
                data_dir
                    .join("swtchvm-state.bin")
                    .to_string_lossy()
                    .as_ref()
            )
        );

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn generated_private_compute_config_uses_manifest_identity_and_bootstrap() {
        let unique = format!(
            "spacekit-supervisor-private-config-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&root).unwrap();
        let genesis = serde_json::json!({"chain_id": 4242, "accounts": [], "contracts": []});
        let manifest_path = root.join("manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "version": network_profile::NETWORK_MANIFEST_VERSION,
                "network_id": "spacekit-private-test",
                "profile": "private",
                "chain_id": 4242,
                "protocol": {
                    "name": network_profile::NETWORK_PROTOCOL,
                    "version": network_profile::NETWORK_PROTOCOL_VERSION,
                },
                "genesis": {
                    "hash": network_profile::canonical_genesis_hash(&genesis).unwrap(),
                    "document": genesis,
                },
                "bootstrap": {
                    "p2p": ["/ip4/127.0.0.1/tcp/24101"],
                    "rpc": [],
                },
                "roles": ["validator"],
                "members": [{
                    "did": "did:spacekit:private:test",
                    "roles": ["validator"],
                }],
            }))
            .unwrap(),
        )
        .unwrap();

        let mut net = SpacekitNetworkFile::for_preset(network_profile::NetworkPreset::Private);
        net.data.compute = Some(root.join("compute"));
        net.manifest = Some(manifest_path);
        net.blockchain.chain_id = 4242;
        net.ports.compute_p2p = 24102;
        let config_path =
            write_compute_config(&net, "did:spacekit:private:test", false).expect("write config");
        let document: toml::Value =
            toml::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();

        assert_eq!(
            document["network"]["name"].as_str(),
            Some("spacekit-private-test")
        );
        assert_eq!(document["network"]["p2p_port"].as_integer(), Some(24102));
        assert_eq!(
            document["network"]["bootstrap_nodes"][0].as_str(),
            Some("127.0.0.1:24101")
        );
        assert_eq!(document["compute"]["chain_id"].as_str(), Some("4242"));
        assert_eq!(document["network"]["dev_mode"].as_bool(), Some(false));
        let _ = std::fs::remove_dir_all(root);
    }
}
