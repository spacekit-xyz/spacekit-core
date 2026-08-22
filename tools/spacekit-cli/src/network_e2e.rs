//! Deterministic, isolated network acceptance gates.

use crate::network_profile::{
    authorize_network_start, canonical_genesis_hash, ManifestBootstrap, ManifestGenesis,
    ManifestMember, ManifestProtocol, ManifestSignature, ManifestSignatureAlgorithm,
    ManifestSignatureEncoding, NetworkManifest, NetworkPreset, NetworkRole, SpacekitNetworkFile,
    NETWORK_MANIFEST_VERSION, NETWORK_PROTOCOL, NETWORK_PROTOCOL_VERSION,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum NetworkTestSuite {
    Local,
    Private,
    Public,
    All,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum GateStatus {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Serialize)]
struct Gate {
    name: String,
    status: GateStatus,
    duration_ms: u128,
    detail: String,
}

#[derive(Debug, Serialize)]
struct SuiteReport {
    suite: String,
    gates: Vec<Gate>,
}

#[derive(Debug, Serialize)]
struct Report {
    schema_version: u32,
    started_at: DateTime<Utc>,
    finished_at: DateTime<Utc>,
    isolated_home: PathBuf,
    artifacts_dir: PathBuf,
    suites: Vec<SuiteReport>,
    passed: usize,
    failed: usize,
    skipped: usize,
}

struct Context {
    root: PathBuf,
    home: PathBuf,
    artifacts: PathBuf,
    exe: PathBuf,
    client: reqwest::Client,
}

impl Context {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.exe);
        command
            .env("HOME", &self.home)
            .env(
                "SPACEKIT_NETWORK_CONFIG",
                self.root.join("network/config.toml"),
            )
            .env("SPACEKIT_SWTCHVM_DISABLE_PERSIST", "0");
        // Keep the gate deterministic even when the developer shell exports
        // release-sidecar overrides from an older build.
        if let Some(bin_dir) = self.exe.parent() {
            command
                .env(
                    "SPACEKIT_COMPUTE_BIN",
                    bin_dir.join("spacekit-compute-node"),
                )
                .env(
                    "SPACEKIT_STORAGE_BIN",
                    bin_dir.join("spacekit-storage-node"),
                )
                .env(
                    "SPACEKIT_MESSAGING_HTTP_BIN",
                    bin_dir.join("spacekit-messaging-http"),
                );
        }
        command
    }

    fn run_cli(&self, label: &str, args: &[&str]) -> Result<Output, String> {
        let output = self
            .command()
            .args(args)
            .output()
            .map_err(|error| format!("spawn spacekit: {error}"))?;
        let log = format!(
            "$ spacekit {}\n\nstdout:\n{}\n\nstderr:\n{}\nexit: {}\n",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            output.status
        );
        std::fs::write(self.artifacts.join(format!("{label}.log")), log)
            .map_err(|error| error.to_string())?;
        Ok(output)
    }

    fn stop_network(&self, label: &str) {
        let _ = self.run_cli(label, &["network", "down"]);
        let runtime_path = self.root.join("network/runtime.json");
        if let Ok(body) = std::fs::read_to_string(&runtime_path) {
            let state = serde_json::from_str::<crate::network_profile::NetworkRuntimeState>(&body);
            if let Ok(state) = state {
                if state.pid != 0 && crate::network_profile::process_alive(state.pid) {
                    let _ = crate::network_profile::signal_process(state.pid);
                }
            }
        }
        let _ = std::fs::remove_file(runtime_path);
    }
}

struct Cleanup<'a>(&'a Context);

impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        self.0.stop_network("cleanup");
    }
}

pub async fn run(
    suite: NetworkTestSuite,
    report_path: PathBuf,
    website_url: Option<String>,
    api_url: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let started_at = Utc::now();
    let absolute_report = if report_path.is_absolute() {
        report_path
    } else {
        std::env::current_dir()?.join(report_path)
    };
    let parent = absolute_report
        .parent()
        .ok_or("report path must have a parent directory")?;
    std::fs::create_dir_all(parent)?;
    let stem = absolute_report
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("network-e2e");
    let artifacts = parent.join(format!("{stem}.artifacts"));
    if artifacts.exists() {
        std::fs::remove_dir_all(&artifacts)?;
    }
    std::fs::create_dir_all(&artifacts)?;
    let root = artifacts.join("run");
    let home = root.join("home");
    std::fs::create_dir_all(&home)?;
    let context = Context {
        root,
        home,
        artifacts: artifacts.clone(),
        exe: std::env::current_exe()?,
        client: reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()?,
    };
    let _cleanup = Cleanup(&context);

    let mut suites = Vec::new();
    match suite {
        NetworkTestSuite::Local => {
            suites.push(run_local(&context, website_url.as_deref(), api_url.as_deref()).await)
        }
        NetworkTestSuite::Private => suites.push(run_private(&context).await),
        NetworkTestSuite::Public => suites.push(run_public(&context).await),
        NetworkTestSuite::All => {
            suites.push(run_local(&context, website_url.as_deref(), api_url.as_deref()).await);
            suites.push(run_private(&context).await);
            suites.push(run_public(&context).await);
        }
    }

    let passed = count_status(&suites, GateStatus::Passed);
    let failed = count_status(&suites, GateStatus::Failed);
    let skipped = count_status(&suites, GateStatus::Skipped);
    context.stop_network("final-cleanup");
    archive_runtime_artifacts(&context.root, &context.artifacts.join("runtime"))?;
    if context.root.exists() {
        std::fs::remove_dir_all(&context.root)?;
    }
    let report = Report {
        schema_version: 1,
        started_at,
        finished_at: Utc::now(),
        isolated_home: context.home.clone(),
        artifacts_dir: artifacts,
        suites,
        passed,
        failed,
        skipped,
    };
    write_report(&absolute_report, &report)?;
    println!(
        "network E2E: {passed} passed, {failed} failed, {skipped} skipped; report {}",
        absolute_report.display()
    );
    if failed > 0 {
        Err(format!("{failed} network E2E gate(s) failed").into())
    } else {
        Ok(())
    }
}

fn count_status(suites: &[SuiteReport], status: GateStatus) -> usize {
    suites
        .iter()
        .flat_map(|suite| &suite.gates)
        .filter(|gate| gate.status == status)
        .count()
}

fn archive_runtime_artifacts(source: &Path, destination: &Path) -> std::io::Result<()> {
    if !source.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            archive_runtime_artifacts(&path, &target)?;
        } else if matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("log" | "toml" | "json")
        ) {
            std::fs::create_dir_all(destination)?;
            std::fs::copy(path, target)?;
        }
    }
    Ok(())
}

fn gate(name: &str, started: Instant, result: Result<String, String>) -> Gate {
    match result {
        Ok(detail) => Gate {
            name: name.into(),
            status: GateStatus::Passed,
            duration_ms: started.elapsed().as_millis(),
            detail,
        },
        Err(detail) => Gate {
            name: name.into(),
            status: GateStatus::Failed,
            duration_ms: started.elapsed().as_millis(),
            detail,
        },
    }
}

fn skip(name: &str, detail: &str) -> Gate {
    Gate {
        name: name.into(),
        status: GateStatus::Skipped,
        duration_ms: 0,
        detail: detail.into(),
    }
}

fn free_ports(count: usize) -> Result<Vec<u16>, String> {
    let mut ports = Vec::with_capacity(count);
    while ports.len() < count {
        let listener =
            std::net::TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    Ok(ports)
}

async fn run_local(
    context: &Context,
    website_url: Option<&str>,
    api_url: Option<&str>,
) -> SuiteReport {
    let mut gates = Vec::new();
    let config = context.root.join("network/config.toml");
    let data = context.root.join("data");
    std::fs::create_dir_all(config.parent().unwrap()).ok();

    let started = Instant::now();
    let setup = (|| -> Result<String, String> {
        let output = context.run_cli(
            "local-init",
            &[
                "network",
                "init",
                "--force",
                "--profile",
                "local",
                "--node-id",
                "e2e-local",
                "--data-root",
                data.to_str().ok_or("non-UTF8 data path")?,
            ],
        )?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned());
        }
        let mut profile: SpacekitNetworkFile =
            toml::from_str(&std::fs::read_to_string(&config).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let ports = free_ports(12)?;
        profile.ports.storage_http = ports[0];
        profile.ports.storage_p2p = ports[1];
        profile.ports.compute_http = ports[2];
        profile.ports.compute_p2p = ports[3];
        profile.ports.messaging_listen = ports[4];
        profile.ports.messaging_bootstrap = ports[5];
        profile.ports.messaging_http = ports[6];
        profile.ports.gateway_http = ports[7];
        profile.ports.status_http = ports[8];
        profile.ports.keymaster_coordinator = ports[9];
        profile.ports.keymaster_guardian_base = ports[10];
        profile.ports.keymaster_registry = ports[11];
        profile.messaging.listen_addr = format!("127.0.0.1:{}", ports[4]);
        profile.messaging.bootstrap_peers = vec![format!("/ip4/127.0.0.1/tcp/{}", ports[5])];
        profile.urls.storage = Some(format!("http://127.0.0.1:{}", ports[0]));
        profile.urls.compute = Some(format!("http://127.0.0.1:{}", ports[2]));
        profile.urls.gateway = Some(format!("http://127.0.0.1:{}", ports[7]));
        profile.services.gateway = false;
        profile.services.keymaster = false;
        profile.runtime.enable_p2p = false;
        profile.blockchain.enabled = true;
        profile.blockchain.persist_state = true;
        profile.validate().map_err(|error| error.to_string())?;
        std::fs::write(&config, toml::to_string_pretty(&profile).unwrap())
            .map_err(|error| error.to_string())?;
        std::fs::copy(&config, context.artifacts.join("local-config.toml"))
            .map_err(|error| error.to_string())?;
        Ok(format!(
            "isolated HOME={}, config={}, data={}",
            context.home.display(),
            config.display(),
            data.display()
        ))
    })();
    gates.push(gate("isolated init and profile", started, setup));

    let profile = std::fs::read_to_string(&config)
        .ok()
        .and_then(|body| toml::from_str::<SpacekitNetworkFile>(&body).ok());
    let Some(profile) = profile else {
        gates.push(skip("live local stack", "profile setup failed"));
        return SuiteReport {
            suite: "local".into(),
            gates,
        };
    };
    let storage = profile.resolved_storage_url();
    let compute = profile.resolved_compute_url();
    let messaging = profile.resolved_messaging_http_url();

    let started = Instant::now();
    let up = context
        .run_cli("local-up", &["network", "up", "--detach"])
        .and_then(|output| {
            if output.status.success() {
                Ok("detached supervisor started".into())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).into_owned())
            }
        });
    gates.push(gate("local stack startup", started, up));

    let started = Instant::now();
    gates.push(gate(
        "service readiness",
        started,
        wait_for_json(
            &context.client,
            &[
                format!("{storage}/health"),
                format!("{compute}/health"),
                format!("{messaging}/health"),
            ],
            Duration::from_secs(45),
        )
        .await,
    ));

    let did = "did:spacekit:e2e:local";
    let document_url = format!("{storage}/api/documents/e2e/gate");
    let document = json!({"nonce":"spacekit-e2e-v1","value":42});
    let started = Instant::now();
    gates.push(gate(
        "storage write and read",
        started,
        put_and_get_document(&context.client, &document_url, did, &document).await,
    ));

    let started = Instant::now();
    gates.push(gate(
        "messaging health send and receive",
        started,
        send_and_receive_message(&context.client, &messaging).await,
    ));

    let address = "0x1111111111111111111111111111111111111111";
    let started = Instant::now();
    gates.push(gate(
        "real SwtchVM faucet and RPC",
        started,
        faucet_and_rpc(&context.client, &compute, address).await,
    ));
    let started = Instant::now();
    gates.push(gate(
        "real SwtchVM block endpoint",
        started,
        get_json(&context.client, &format!("{compute}/block/0"))
            .await
            .map(|value| format!("genesis block number={}", value["number"])),
    ));
    let started = Instant::now();
    let wasm_result = deploy_call_wasm(&context.client, &compute, address, None).await;
    let persisted_contract = wasm_result
        .as_ref()
        .ok()
        .map(|(_, contract)| contract.clone());
    gates.push(gate(
        "WASM deploy call receipt block",
        started,
        wasm_result.map(|(detail, _)| detail),
    ));

    context.stop_network("local-down-before-restart");
    let started = Instant::now();
    let restart = context
        .run_cli("local-restart", &["network", "up", "--detach"])
        .and_then(|output| {
            if output.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&output.stderr).into_owned())
            }
        });
    let persistence = match restart {
        Ok(()) => {
            let ready = wait_for_json(
                &context.client,
                &[format!("{storage}/health"), format!("{compute}/health")],
                Duration::from_secs(45),
            )
            .await;
            match ready {
                Ok(_) => get_document(&context.client, &document_url, did, &document).await,
                Err(error) => Err(error),
            }
        }
        Err(error) => Err(error),
    };
    gates.push(gate("restart storage persistence", started, persistence));
    let started = Instant::now();
    let vm_persistence = match persisted_contract {
        Some(contract) => get_json(
            &context.client,
            &format!("{compute}/account/{}", contract.trim_start_matches("0x")),
        )
        .await
        .and_then(|account| {
            if account["code"].is_null() {
                Err(format!("contract code missing after restart: {account}"))
            } else {
                Ok(format!(
                    "deployed contract {contract} retained WASM code after restart"
                ))
            }
        }),
        None => Err("deploy gate did not produce a contract address".into()),
    };
    gates.push(gate(
        "restart SwtchVM contract persistence",
        started,
        vm_persistence,
    ));

    for (name, url) in [
        ("optional website connectivity", website_url),
        ("optional API connectivity", api_url),
    ] {
        if let Some(url) = url {
            let started = Instant::now();
            gates.push(gate(
                name,
                started,
                http_success(&context.client, url).await,
            ));
        } else {
            gates.push(skip(name, "not requested; pass --website-url or --api-url"));
        }
    }
    context.stop_network("local-final-down");
    SuiteReport {
        suite: "local".into(),
        gates,
    }
}

async fn run_private(context: &Context) -> SuiteReport {
    let mut gates = Vec::new();
    let started = Instant::now();
    let result = (|| -> Result<String, String> {
        let ports = free_ports(36)?;
        let genesis = json!({"chain_id": 4242, "accounts": [], "contracts": []});
        let hash = canonical_genesis_hash(&genesis).map_err(|error| error.to_string())?;
        let members: Vec<String> = (0..3)
            .map(|index| format!("did:spacekit:private:node-{index}"))
            .collect();
        let manifest = NetworkManifest {
            version: NETWORK_MANIFEST_VERSION,
            network_id: "spacekit-e2e-private".into(),
            profile: NetworkPreset::Private,
            chain_id: 4242,
            protocol: ManifestProtocol {
                name: NETWORK_PROTOCOL.into(),
                version: NETWORK_PROTOCOL_VERSION,
            },
            genesis: ManifestGenesis {
                hash: hash.clone(),
                uri: None,
                document: Some(genesis),
            },
            bootstrap: ManifestBootstrap {
                p2p: vec![format!("/ip4/127.0.0.1/tcp/{}", ports[3])],
                rpc: vec![format!("http://127.0.0.1:{}", ports[2])],
            },
            roles: vec![
                NetworkRole::Subscriber,
                NetworkRole::Operator,
                NetworkRole::Validator,
            ],
            members: members
                .iter()
                .map(|did| ManifestMember {
                    did: did.clone(),
                    roles: vec![NetworkRole::Operator, NetworkRole::Validator],
                })
                .collect(),
            signature: None,
        };
        manifest.validate().map_err(|error| error.to_string())?;
        let manifest_path = context.artifacts.join("private-manifest.json");
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .map_err(|error| error.to_string())?;
        for index in 0..3 {
            let mut profile = SpacekitNetworkFile::for_preset(NetworkPreset::Private);
            profile.node_id = format!("private-node-{index}");
            profile.role = NetworkRole::Validator;
            profile.manifest = Some(manifest_path.clone());
            profile.blockchain.chain_id = manifest.chain_id;
            profile.admission.shared_genesis_hash = Some(hash.clone());
            profile.admission.allowlist = members.clone();
            profile.blockchain.validators.peers = members.clone();
            profile.messaging.bootstrap_peers = vec![format!("/ip4/127.0.0.1/tcp/{}", ports[4])];
            profile.bind_host = "127.0.0.1".into();
            profile.services.storage = true;
            profile.services.messaging = true;
            profile.services.compute = true;
            profile.ports.storage_http = ports[index * 12];
            profile.ports.storage_p2p = ports[index * 12 + 1];
            profile.ports.compute_http = ports[index * 12 + 2];
            profile.ports.compute_p2p = ports[index * 12 + 3];
            profile.ports.messaging_listen = ports[index * 12 + 4];
            profile.ports.messaging_bootstrap = ports[index * 12 + 5];
            profile.ports.messaging_http = ports[index * 12 + 6];
            profile.ports.gateway_http = ports[index * 12 + 7];
            profile.ports.status_http = ports[index * 12 + 8];
            profile.ports.keymaster_coordinator = ports[index * 12 + 9];
            profile.ports.keymaster_registry = ports[index * 12 + 10];
            profile.ports.keymaster_guardian_base = ports[index * 12 + 11];
            profile.messaging.listen_addr = format!("127.0.0.1:{}", ports[index * 12 + 4]);
            profile.urls.storage = Some(format!("http://127.0.0.1:{}", ports[index * 12]));
            profile.urls.compute = Some(format!("http://127.0.0.1:{}", ports[index * 12 + 2]));
            profile.runtime.blob_fact_auth = Some("hybrid".into());
            profile.runtime.enable_p2p = true;
            let node_root = context.root.join(format!("private/node-{index}"));
            profile.data.storage = Some(node_root.join("storage"));
            profile.data.compute = Some(node_root.join("compute"));
            profile.data.messaging = Some(node_root.join("messaging"));
            profile.validate().map_err(|error| error.to_string())?;
            authorize_network_start(&profile, &members[index])
                .map_err(|error| error.to_string())?;
            std::fs::create_dir_all(&node_root).map_err(|error| error.to_string())?;
            std::fs::create_dir_all(node_root.join("storage"))
                .map_err(|error| error.to_string())?;
            std::fs::write(
                node_root.join("storage/.handoff_secret"),
                b"spacekit-private-e2e-shared-handoff-secret",
            )
            .map_err(|error| error.to_string())?;
            let body = toml::to_string_pretty(&profile).unwrap();
            std::fs::write(node_root.join("config.toml"), &body)
                .map_err(|error| error.to_string())?;
            std::fs::write(
                context.artifacts.join(format!("private-node-{index}.toml")),
                body,
            )
            .map_err(|error| error.to_string())?;
            let home = node_root.join("home/.spacekit");
            std::fs::create_dir_all(&home).map_err(|error| error.to_string())?;
            std::fs::write(
                home.join("config.toml"),
                format!(
                    "[identity]\ndid = \"{}\"\nalgorithm = \"Kyber1024\"\npublic_key_path = \"{}\"\nprivate_key_path = \"{}\"\n\n[network]\ndefault_network = \"spacekit-e2e-private\"\n\n[network.endpoints]\n\n[project]\nname = \"private-node-{index}\"\nversion = \"1.0.0\"\ncreated_at = \"{}\"\n",
                    members[index],
                    node_root.join("keys/public.hex").display(),
                    node_root.join("keys/private.hex").display(),
                    Utc::now().to_rfc3339(),
                ),
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(
            "three unique validated configs, ports, data roots, shared manifest and bootstrap"
                .into(),
        )
    })();
    gates.push(gate(
        "three-node config manifest bootstrap",
        started,
        result,
    ));

    let started = Instant::now();
    let mut rejected = SpacekitNetworkFile::for_preset(NetworkPreset::Private);
    rejected.messaging.bootstrap_peers = vec!["/ip4/127.0.0.1/tcp/1".into()];
    rejected.admission.allowlist = vec!["did:spacekit:private:allowed".into()];
    rejected.admission.shared_genesis_hash = Some("a".repeat(64));
    rejected.role = NetworkRole::Operator;
    let rejection = authorize_network_start(&rejected, "did:spacekit:private:intruder")
        .err()
        .filter(|error| error.to_string().contains("allowlist"))
        .map(|error| error.to_string())
        .ok_or_else(|| "private intruder was not rejected by allowlist gate".into());
    gates.push(gate("private allowlist rejection", started, rejection));
    let started = Instant::now();
    let live = start_private_cluster(context, 2).await;
    gates.push(gate(
        "two-node live startup",
        started,
        live.as_ref()
            .map(|_| "two compute supervisors reached live HTTP readiness".into())
            .map_err(Clone::clone),
    ));
    let started = Instant::now();
    let convergence = match &live {
        Ok(urls) => wait_for_peer_convergence(&context.client, urls, Duration::from_secs(30)).await,
        Err(error) => Err(error.clone()),
    };
    gates.push(gate("peer and discovery convergence", started, convergence));
    let started = Instant::now();
    let chain_convergence = match live {
        Ok(urls) => run_private_chain_convergence(context, urls).await,
        Err(error) => Err(error),
    };
    gates.push(gate(
        "chain convergence and late join",
        started,
        chain_convergence,
    ));
    let started = Instant::now();
    gates.push(gate(
        "storage federation replication",
        started,
        run_private_storage_federation(context).await,
    ));
    let started = Instant::now();
    gates.push(gate(
        "message convergence",
        started,
        run_private_message_convergence(context).await,
    ));
    stop_private_cluster(context);
    SuiteReport {
        suite: "private".into(),
        gates,
    }
}

async fn start_private_cluster(context: &Context, count: usize) -> Result<Vec<String>, String> {
    let mut urls = Vec::new();
    for index in 0..count {
        match start_private_node(context, index).await {
            Ok(url) => urls.push(url),
            Err(error) => {
                stop_private_cluster(context);
                return Err(error);
            }
        }
    }
    Ok(urls)
}

async fn start_private_node(context: &Context, index: usize) -> Result<String, String> {
    let node_root = context.root.join(format!("private/node-{index}"));
    let config = node_root.join("config.toml");
    let home = node_root.join("home");
    let messaging_bin = context
        .exe
        .parent()
        .ok_or("spacekit executable has no parent directory")?
        .join("spacekit-messaging-http");
    if !messaging_bin.is_file() {
        return Err(format!(
            "messaging sidecar not built at {}",
            messaging_bin.display()
        ));
    }
    let output = Command::new(&context.exe)
        .env("HOME", &home)
        .env("SPACEKIT_NETWORK_CONFIG", &config)
        .env("SPACEKIT_SWTCHVM_DISABLE_PERSIST", "0")
        .env("SPACEKIT_MESSAGING_HTTP_BIN", &messaging_bin)
        .env(
            "SPACEKIT_HANDOFF_SECRET",
            "spacekit-private-e2e-shared-handoff-secret",
        )
        .env("SPACEKIT_REQUIRE_HANDOFF_SIGNATURE", "true")
        .args(["network", "up", "--detach"])
        .output()
        .map_err(|error| format!("start private node {index}: {error}"))?;
    std::fs::write(
        context
            .artifacts
            .join(format!("private-node-{index}-up.log")),
        format!(
            "stdout:\n{}\nstderr:\n{}\nstatus: {}\n",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            output.status
        ),
    )
    .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "private node {index} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let profile: SpacekitNetworkFile =
        toml::from_str(&std::fs::read_to_string(&config).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let storage_url = profile.resolved_storage_url();
    let compute_url = profile.resolved_compute_url();
    let messaging_url = profile.resolved_messaging_http_url();
    wait_for_json(
        &context.client,
        &[
            format!("{}/health", storage_url.trim_end_matches('/')),
            format!("{}/health", compute_url.trim_end_matches('/')),
            format!("{}/health", messaging_url.trim_end_matches('/')),
        ],
        Duration::from_secs(60),
    )
    .await?;
    Ok(compute_url)
}

async fn run_private_storage_federation(context: &Context) -> Result<String, String> {
    const OWNER: &str = "did:spacekit:private:federation-owner";
    const WORKSPACE: &str = "private-federation-workspace";
    const REPO: &str = "private-federation-repo";
    const BLOB: &[u8] = b"spacekit private federation replication fixture\nexact bytes v1\n";

    let mut storage_urls = Vec::new();
    for index in 0..3 {
        let node_root = context.root.join(format!("private/node-{index}"));
        let config = node_root.join("config.toml");
        let profile: SpacekitNetworkFile =
            toml::from_str(&std::fs::read_to_string(&config).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        let storage_url = profile.resolved_storage_url();
        if index == 2
            && !node_root.join("runtime.json").exists()
            && context
                .client
                .get(format!("{}/health", storage_url.trim_end_matches('/')))
                .send()
                .await
                .map(|response| !response.status().is_success())
                .unwrap_or(true)
        {
            start_private_node(context, index).await?;
        }
        storage_urls.push(storage_url);
    }
    wait_for_json(
        &context.client,
        &storage_urls
            .iter()
            .map(|url| format!("{}/health", url.trim_end_matches('/')))
            .collect::<Vec<_>>(),
        Duration::from_secs(60),
    )
    .await?;

    let source = storage_urls[0].trim_end_matches('/');
    let blob_hash = hex::encode(blake3::hash(BLOB).as_bytes());
    let auth = format!("DID {OWNER}");

    let blob_response = context
        .client
        .put(format!("{source}/blobs/{blob_hash}"))
        .header("Authorization", &auth)
        .body(BLOB.to_vec())
        .send()
        .await
        .map_err(|error| format!("write source blob: {error}"))?;
    require_status("write source blob", blob_response, &[200, 201]).await?;

    let commit = spacekit_storage_node::repo_commit::commit_from_tree(
        std::collections::BTreeMap::from([("federation.txt".to_string(), blob_hash.clone())]),
        "private federation fixture".into(),
        OWNER.into(),
        1,
    );
    let fact = spacekit_repo::build_commit_fact_package(OWNER, Vec::new(), commit)
        .map_err(|error| error.to_string())?;
    let commit_fact_id = hex::encode(fact.fact_id);
    let fact_response = context
        .client
        .post(format!("{source}/facts"))
        .header("Authorization", &auth)
        .json(&fact)
        .send()
        .await
        .map_err(|error| format!("write source commit fact: {error}"))?;
    require_status("write source commit fact", fact_response, &[200, 201]).await?;

    let ref_collection = spacekit_storage_node::repo_commit::ref_collection(REPO);
    let ref_document_id = spacekit_storage_node::repo_commit::ref_document_id("main");
    let collection =
        percent_encoding::utf8_percent_encode(&ref_collection, percent_encoding::NON_ALPHANUMERIC);
    let document_id =
        percent_encoding::utf8_percent_encode(&ref_document_id, percent_encoding::NON_ALPHANUMERIC);
    let ref_document = json!({"tip": commit_fact_id});
    let document_response = context
        .client
        .put(format!("{source}/api/documents/{collection}/{document_id}"))
        .header("Authorization", &auth)
        .json(&ref_document)
        .send()
        .await
        .map_err(|error| format!("write source repo-ref document: {error}"))?;
    require_status("write source repo-ref document", document_response, &[200]).await?;

    let workspace_response = context
        .client
        .post(format!("{source}/api/workspaces"))
        .header("Authorization", &auth)
        .json(&json!({
            "workspace_id": WORKSPACE,
            "collaborators": [],
            "associated_repos": [REPO],
            "visibility": "public"
        }))
        .send()
        .await
        .map_err(|error| format!("create source workspace: {error}"))?;
    require_status("create source workspace", workspace_response, &[201]).await?;

    let source_workspace = get_authorized_json(
        &context.client,
        &format!("{source}/api/workspaces/{WORKSPACE}"),
        &auth,
    )
    .await?;
    let export_response = context
        .client
        .get(format!("{source}/api/workspaces/{WORKSPACE}/export"))
        .header("Authorization", &auth)
        .send()
        .await
        .map_err(|error| format!("export source workspace: {error}"))?;
    let export_status = export_response.status();
    let bundle: Value = export_response
        .json()
        .await
        .map_err(|error| format!("decode export bundle: {error}"))?;
    if !export_status.is_success() {
        return Err(format!(
            "export source workspace returned {export_status}: {bundle}"
        ));
    }
    if bundle["handoff_signature"].as_str().is_none() {
        return Err(format!(
            "export did not contain handoff_signature: {bundle}"
        ));
    }
    if bundle["referenced_blob_hashes"] != json!([blob_hash.clone()]) {
        return Err(format!(
            "export referenced_blob_hashes did not exactly match source blob: {}",
            bundle["referenced_blob_hashes"]
        ));
    }
    std::fs::write(
        context.artifacts.join("private-storage-handoff.json"),
        serde_json::to_vec_pretty(&bundle).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let mut tampered_bundle = bundle.clone();
    tampered_bundle
        .as_object_mut()
        .ok_or("export bundle was not an object")?
        .remove("handoff_signature");
    let tampered_response = context
        .client
        .post(format!(
            "{}/api/workspaces/import",
            storage_urls[1].trim_end_matches('/')
        ))
        .header("Authorization", &auth)
        .json(&json!({"bundle": tampered_bundle, "owner_did": OWNER}))
        .send()
        .await
        .map_err(|error| format!("submit tampered handoff: {error}"))?;
    let tampered_status = tampered_response.status();
    let tampered_body = tampered_response
        .text()
        .await
        .map_err(|error| error.to_string())?;
    std::fs::write(
        context
            .artifacts
            .join("private-storage-tamper-rejection.log"),
        format!("status: {tampered_status}\nbody: {tampered_body}\n"),
    )
    .map_err(|error| error.to_string())?;
    if tampered_status.is_success() || !tampered_body.contains("handoff_signature required") {
        return Err(format!(
            "unsigned tampered handoff was not rejected as required: {tampered_status} {tampered_body}"
        ));
    }

    let mut import_results = Vec::new();
    for destination in storage_urls.iter().skip(1) {
        let response = context
            .client
            .post(format!(
                "{}/api/workspaces/import",
                destination.trim_end_matches('/')
            ))
            .header("Authorization", &auth)
            .json(&json!({
                "bundle": bundle,
                "owner_did": OWNER,
                "replicate_blobs_from": source,
                "replicate_source_authorization": auth
            }))
            .send()
            .await
            .map_err(|error| format!("import workspace to {destination}: {error}"))?;
        let status = response.status();
        let result: Value = response
            .json()
            .await
            .map_err(|error| format!("decode import response from {destination}: {error}"))?;
        if status.as_u16() != 201
            || result["created"] != true
            || result["blob_replication"]["fetched"] != 1
            || result["blob_replication"]["failed"] != json!([])
        {
            return Err(format!(
                "federation import to {destination} failed exact checks: HTTP {status} {result}"
            ));
        }
        import_results.push(result);
    }
    std::fs::write(
        context
            .artifacts
            .join("private-storage-import-results.json"),
        serde_json::to_vec_pretty(&import_results).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    for destination in storage_urls.iter().skip(1) {
        let workspace = get_authorized_json(
            &context.client,
            &format!(
                "{}/api/workspaces/{WORKSPACE}",
                destination.trim_end_matches('/')
            ),
            &auth,
        )
        .await?;
        if workspace_without_import_timestamps(&workspace)
            != workspace_without_import_timestamps(&source_workspace)
        {
            return Err(format!(
                "workspace document content mismatch on {destination}: source={source_workspace} destination={workspace}"
            ));
        }
        let source_created = source_workspace["created_at"].as_u64().unwrap_or(0);
        let destination_created = workspace["created_at"].as_u64().unwrap_or(0);
        let destination_updated = workspace["updated_at"].as_u64().unwrap_or(0);
        if destination_created < source_created || destination_updated < source_created {
            return Err(format!(
                "workspace import timestamps regressed on {destination}: source={source_created}, destination={workspace}"
            ));
        }
        let response = context
            .client
            .get(format!(
                "{}/blobs/{blob_hash}",
                destination.trim_end_matches('/')
            ))
            .send()
            .await
            .map_err(|error| format!("read replicated blob from {destination}: {error}"))?;
        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("read replicated blob body from {destination}: {error}"))?;
        let actual_hash = hex::encode(blake3::hash(&bytes).as_bytes());
        if !status.is_success() || bytes.as_ref() != BLOB || actual_hash != blob_hash {
            return Err(format!(
                "replicated blob mismatch on {destination}: HTTP {status}, expected {blob_hash}, got {actual_hash}"
            ));
        }
    }

    Ok(format!(
        "signed workspace handoff replicated exact workspace fields (with destination import timestamps) and BLAKE3 blob {blob_hash} from node A to nodes B/C over hybrid-auth HTTP; unsigned tamper rejected"
    ))
}

fn workspace_without_import_timestamps(value: &Value) -> Value {
    let mut value = value.clone();
    if let Some(object) = value.as_object_mut() {
        object.remove("created_at");
        object.remove("updated_at");
    }
    value
}

async fn require_status(
    operation: &str,
    response: reqwest::Response,
    expected: &[u16],
) -> Result<(), String> {
    let status = response.status();
    if expected.contains(&status.as_u16()) {
        return Ok(());
    }
    let body = response.text().await.unwrap_or_default();
    Err(format!("{operation} returned {status}: {body}"))
}

async fn get_authorized_json(
    client: &reqwest::Client,
    url: &str,
    authorization: &str,
) -> Result<Value, String> {
    let response = client
        .get(url)
        .header("Authorization", authorization)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if status.is_success() {
        Ok(value)
    } else {
        Err(format!("GET {url} returned {status}: {value}"))
    }
}

async fn wait_for_peer_convergence(
    client: &reqwest::Client,
    urls: &[String],
    timeout: Duration,
) -> Result<String, String> {
    let started = Instant::now();
    let mut last = Vec::new();
    while started.elapsed() < timeout {
        let mut counts = Vec::new();
        let mut network_names = Vec::new();
        for url in urls {
            match get_json(
                client,
                &format!("{}/v1/network/peers", url.trim_end_matches('/')),
            )
            .await
            {
                Ok(value) => {
                    counts.push(value["peer_count"].as_u64().unwrap_or(0));
                    network_names.push(
                        value["network_name"]
                            .as_str()
                            .unwrap_or("<missing>")
                            .to_string(),
                    );
                }
                Err(error) => {
                    last = vec![error];
                    counts.clear();
                    break;
                }
            }
        }
        if counts.len() == urls.len()
            && counts.iter().all(|count| *count >= 1)
            && network_names
                .iter()
                .all(|name| name == "spacekit-e2e-private")
        {
            return Ok(format!(
                "live NetworkService handshakes converged with peer counts {counts:?} on spacekit-e2e-private"
            ));
        }
        last = counts.iter().map(ToString::to_string).collect();
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!(
        "peer convergence timeout; last observations {last:?}"
    ))
}

async fn run_private_chain_convergence(
    context: &Context,
    mut urls: Vec<String>,
) -> Result<String, String> {
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    use sha3::{Digest as _, Keccak256};
    let signing_key = k256::ecdsa::SigningKey::random(&mut rand::thread_rng());
    let point = signing_key.verifying_key().to_encoded_point(false);
    let digest: [u8; 32] = Keccak256::digest(&point.as_bytes()[1..]).into();
    let address = format!("0x{}", hex::encode(&digest[12..]));
    for (index, url) in urls.iter().enumerate() {
        fund_private_node(&context.client, url, &address, index).await?;
    }
    let (_, contract) =
        deploy_call_wasm(&context.client, &urls[0], &address, Some(&signing_key)).await?;
    wait_for_chain_head(
        &context.client,
        &urls,
        2,
        &contract,
        Duration::from_secs(30),
    )
    .await?;

    let late_url = start_private_node(context, 2).await?;
    fund_private_node(&context.client, &late_url, &address, 2).await?;
    urls.push(late_url);
    let detail = wait_for_chain_head(
        &context.client,
        &urls,
        2,
        &contract,
        Duration::from_secs(45),
    )
    .await?;
    Ok(format!(
        "{detail}; third node started after deploy/call and caught up by verified block replay; malformed/fork rejection is covered by the compute-node P2P bridge test"
    ))
}

async fn fund_private_node(
    client: &reqwest::Client,
    base: &str,
    address: &str,
    index: usize,
) -> Result<(), String> {
    let response = client
        .post(format!("{}/faucet", base.trim_end_matches('/')))
        .json(&json!({
            "did": format!("did:spacekit:e2e:private-funding-{index}"),
            "address": address,
            "amount": 10_000_000
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success() || body["success"] != true {
        return Err(format!(
            "private node {index} faucet failed ({status}): {body}"
        ));
    }
    Ok(())
}

async fn wait_for_chain_head(
    client: &reqwest::Client,
    urls: &[String],
    expected_height: u64,
    contract: &str,
    timeout: Duration,
) -> Result<String, String> {
    let started = Instant::now();
    let mut last = Vec::new();
    while started.elapsed() < timeout {
        let mut heads = Vec::new();
        let mut accounts = Vec::new();
        for url in urls {
            let head = get_json(
                client,
                &format!("{}/v1/sync/subscriber", url.trim_end_matches('/')),
            )
            .await;
            let account = get_json(
                client,
                &format!(
                    "{}/account/{}",
                    url.trim_end_matches('/'),
                    contract.trim_start_matches("0x")
                ),
            )
            .await;
            match (head, account) {
                (Ok(head), Ok(account)) => {
                    heads.push((
                        head.pointer("/head/number").and_then(Value::as_u64),
                        head.pointer("/head/hash_hex")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        head.pointer("/head/state_root_hex")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    ));
                    accounts.push(account);
                }
                (Err(error), _) | (_, Err(error)) => {
                    last = vec![error];
                    heads.clear();
                    break;
                }
            }
        }
        if heads.len() == urls.len()
            && heads
                .iter()
                .all(|(height, _, _)| *height == Some(expected_height))
            && heads.windows(2).all(|pair| pair[0] == pair[1])
            && accounts.iter().all(|account| !account["code"].is_null())
            && accounts.windows(2).all(|pair| pair[0] == pair[1])
        {
            return Ok(format!(
                "{} nodes share height {}, block hash {}, state root {}, and deployed contract state",
                urls.len(),
                expected_height,
                heads[0].1.as_deref().unwrap_or("<missing>"),
                heads[0].2.as_deref().unwrap_or("<missing>")
            ));
        }
        last = heads
            .iter()
            .map(|head| format!("{head:?}"))
            .collect::<Vec<_>>();
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!(
        "chain convergence timeout; last observations {last:?}"
    ))
}

async fn run_private_message_convergence(context: &Context) -> Result<String, String> {
    let mut urls = Vec::new();
    let mut dids = Vec::new();
    for index in 0..3 {
        let config = context
            .root
            .join(format!("private/node-{index}/config.toml"));
        let profile: SpacekitNetworkFile =
            toml::from_str(&std::fs::read_to_string(&config).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        urls.push(profile.resolved_messaging_http_url());
        dids.push(format!("did:spacekit:private:node-{index}"));
    }

    let started = Instant::now();
    let mut peer_counts = Vec::new();
    while started.elapsed() < Duration::from_secs(30) {
        peer_counts.clear();
        for url in &urls {
            let health = get_json(
                &context.client,
                &format!("{}/health", url.trim_end_matches('/')),
            )
            .await?;
            peer_counts.push(health["peer_count"].as_u64().unwrap_or(0));
        }
        if peer_counts.iter().all(|count| *count >= 1) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    if peer_counts.len() != 3 || peer_counts.iter().any(|count| *count < 1) {
        return Err(format!(
            "messaging peer convergence timeout; peer counts {peer_counts:?}"
        ));
    }

    let payload = "spacekit-private-message-convergence-v1";
    let response = context
        .client
        .post(format!(
            "{}/api/messages/envelope",
            urls[0].trim_end_matches('/')
        ))
        .json(&json!({
            "message": {
                "kind": "chat",
                "payload": payload,
                "context": {
                    "did": dids[0],
                    "timestamp": Utc::now().timestamp_millis().max(0) as u64,
                    "source": "network-e2e-private"
                }
            },
            "conversation_type": "direct",
            "recipient_did": dids[1],
            "recipient_dids": [dids[1]],
            "group_id": null
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let sent: Value = response.json().await.map_err(|error| error.to_string())?;
    let message_id = sent["message_id"]
        .as_str()
        .ok_or_else(|| format!("envelope response missing message_id ({status}): {sent}"))?;
    if !status.is_success() || sent["status"] != "ok" {
        return Err(format!("envelope send failed ({status}): {sent}"));
    }

    let history_url = format!(
        "{}/api/messages/history?did={}",
        urls[1].trim_end_matches('/'),
        percent_encoding::utf8_percent_encode(&dids[1], percent_encoding::NON_ALPHANUMERIC)
    );
    let recipient_config = spacekit_messaging_node::MessagingConfig::from_file(
        context
            .root
            .join("private/node-1/messaging/messaging_http_config.json")
            .to_str()
            .ok_or("non-UTF8 recipient messaging config path")?,
    )
    .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let mut last = Value::Null;
    while started.elapsed() < Duration::from_secs(15) {
        let response = context
            .client
            .get(&history_url)
            .bearer_auth(&recipient_config.private_key)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        let status = response.status();
        last = response.json().await.map_err(|error| error.to_string())?;
        if !status.is_success() {
            return Err(format!("recipient history returned {status}: {last}"));
        }
        if last["messages"].as_array().is_some_and(|messages| {
            messages.iter().any(|message| {
                message["message_id"] == message_id
                    && message["sender"]["did"] == dids[0]
                    && message["content"] == payload
                    && message["participants"]
                        .as_array()
                        .is_some_and(|participants| participants.iter().any(|did| did == &dids[1]))
            })
        }) {
            return Ok(format!(
                "three MessagingNode/libp2p processes converged with peer counts {peer_counts:?}; signed transport envelope {message_id} was delivered to {} and verified through recipient-scoped history",
                dids[1]
            ));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Err(format!(
        "recipient history did not contain exact envelope {message_id}: {last}"
    ))
}

fn stop_private_cluster(context: &Context) {
    for index in 0..3 {
        let node_root = context.root.join(format!("private/node-{index}"));
        let runtime_path = node_root.join("runtime.json");
        if let Ok(body) = std::fs::read_to_string(&runtime_path) {
            if let Ok(state) =
                serde_json::from_str::<crate::network_profile::NetworkRuntimeState>(&body)
            {
                if state.pid != 0 && crate::network_profile::process_alive(state.pid) {
                    let _ = crate::network_profile::signal_process(state.pid);
                }
            }
        }
        let _ = std::fs::remove_file(runtime_path);
    }
}

const PUBLIC_OPERATOR_DID: &str = "did:spacekit:public:operator";
const PUBLIC_VALIDATOR_DID: &str = "did:spacekit:public:validator";
const PUBLIC_SUBSCRIBER_DID: &str = "did:spacekit:public:subscriber";

struct PublicFixture {
    children: Vec<Child>,
}

impl Drop for PublicFixture {
    fn drop(&mut self) {
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn run_public(context: &Context) -> SuiteReport {
    let mut gates = Vec::new();
    let started = Instant::now();
    let fixture = start_public_fixture(context).await;
    gates.push(gate(
        "isolated public service fixture",
        started,
        fixture
            .as_ref()
            .map(|(_, compute, storage, _)| {
                format!(
                    "standalone compute {compute} and storage/operator registry {storage} ready"
                )
            })
            .map_err(Clone::clone),
    ));

    let Ok((fixture, compute_url, storage_url, compute_p2p)) = fixture else {
        for name in [
            "manifest cryptographic verification",
            "signed operator service publication",
            "subscriber join genesis head protocol",
            "live public operator discovery",
            "operator validator readiness",
            "public admission rejection matrix",
        ] {
            gates.push(gate(
                name,
                Instant::now(),
                Err("isolated public fixture setup failed".into()),
            ));
        }
        return SuiteReport {
            suite: "public".into(),
            gates,
        };
    };

    let manifest = signed_public_manifest(&compute_url, &storage_url, &compute_p2p);
    let started = Instant::now();
    let verified = manifest
        .as_ref()
        .map_err(Clone::clone)
        .and_then(|manifest| {
            manifest.validate().map_err(|error| error.to_string())?;
            manifest
                .verify_signature()
                .map_err(|error| error.to_string())?;
            std::fs::write(
                context.artifacts.join("public-manifest.json"),
                serde_json::to_vec_pretty(manifest).unwrap(),
            )
            .map_err(|error| error.to_string())?;
            Ok("SPHINCS-128f signature verified over canonical manifest bytes".into())
        });
    gates.push(gate(
        "manifest cryptographic verification",
        started,
        verified,
    ));

    if let Ok(manifest) = manifest {
        let started = Instant::now();
        gates.push(gate(
            "signed operator service publication",
            started,
            publish_and_verify_operator_fact(context, &storage_url).await,
        ));

        let started = Instant::now();
        gates.push(gate(
            "subscriber join genesis head protocol",
            started,
            join_and_verify_subscriber(context, &manifest, &compute_url).await,
        ));

        let started = Instant::now();
        gates.push(gate(
            "live public operator discovery",
            started,
            verify_live_public_discovery(context, &compute_url, &storage_url),
        ));

        let started = Instant::now();
        gates.push(gate(
            "operator validator readiness",
            started,
            verify_public_role_readiness(context, &manifest),
        ));

        let started = Instant::now();
        gates.push(gate(
            "public admission rejection matrix",
            started,
            verify_public_rejections(context, &manifest),
        ));
    } else {
        for name in [
            "signed operator service publication",
            "subscriber join genesis head protocol",
            "live public operator discovery",
            "operator validator readiness",
            "public admission rejection matrix",
        ] {
            gates.push(gate(
                name,
                Instant::now(),
                Err("manifest fixture generation failed".into()),
            ));
        }
    }
    drop(fixture);
    SuiteReport {
        suite: "public".into(),
        gates,
    }
}

async fn start_public_fixture(
    context: &Context,
) -> Result<(PublicFixture, String, String, String), String> {
    let ports = free_ports(4)?;
    let compute_url = format!("http://127.0.0.1:{}", ports[0]);
    let storage_url = format!("http://127.0.0.1:{}", ports[2]);
    let compute_p2p = format!("/ip4/127.0.0.1/tcp/{}", ports[1]);
    let bin_dir = context
        .exe
        .parent()
        .ok_or("spacekit executable has no parent directory")?;
    let compute_bin = bin_dir.join("spacekit-compute-node");
    let storage_bin = bin_dir.join("spacekit-storage-node");
    for binary in [&compute_bin, &storage_bin] {
        if !binary.is_file() {
            return Err(format!(
                "required public fixture binary missing: {}",
                binary.display()
            ));
        }
    }

    write_cli_identity(context, PUBLIC_SUBSCRIBER_DID)?;
    let public_root = context.root.join("public");
    std::fs::create_dir_all(&public_root).map_err(|error| error.to_string())?;
    let compute_config = public_root.join("compute.toml");
    let config_output = Command::new(&compute_bin)
        .args([
            "--config",
            compute_config.to_str().ok_or("non-UTF8 compute config")?,
            "status",
        ])
        .output()
        .map_err(|error| format!("generate compute config: {error}"))?;
    std::fs::write(
        context
            .artifacts
            .join("public-compute-config-generation.log"),
        format!(
            "stdout:\n{}\nstderr:\n{}\nstatus: {}\n",
            String::from_utf8_lossy(&config_output.stdout),
            String::from_utf8_lossy(&config_output.stderr),
            config_output.status
        ),
    )
    .map_err(|error| error.to_string())?;
    if !config_output.status.success() || !compute_config.is_file() {
        return Err(format!(
            "compute config generation failed: {}",
            String::from_utf8_lossy(&config_output.stderr)
        ));
    }
    configure_public_compute(&compute_config, &public_root)?;
    std::fs::copy(
        &compute_config,
        context.artifacts.join("public-compute.toml"),
    )
    .map_err(|error| error.to_string())?;

    let mut children = Vec::new();
    children.push(spawn_logged(
        &storage_bin,
        &[
            "start",
            "--did",
            PUBLIC_OPERATOR_DID,
            "--data-dir",
            public_root
                .join("storage")
                .to_str()
                .ok_or("non-UTF8 storage path")?,
            "--port",
            &ports[2].to_string(),
            "--p2p-port",
            &ports[3].to_string(),
            "--disable-p2p",
        ],
        &[
            ("SPACEKIT_NODE_DID", PUBLIC_OPERATOR_DID),
            ("SPACEKIT_BLOB_FACT_AUTH", "hybrid"),
            ("SPACEKIT_PUBLIC_HTTP_URL", &storage_url),
            ("RUST_MIN_STACK", "33554432"),
        ],
        &context.artifacts.join("public-storage.log"),
    )?);
    children.push(spawn_logged(
        &compute_bin,
        &[
            "--config",
            compute_config.to_str().ok_or("non-UTF8 compute config")?,
            "--node-did",
            PUBLIC_VALIDATOR_DID,
            "--network",
            "spacekit-e2e-public",
            "--port",
            &ports[0].to_string(),
            "--p2p-port",
            &ports[1].to_string(),
            "start",
            "--max-cpu-cores",
            "1",
            "--max-memory-mb",
            "256",
        ],
        &[("SPACEKIT_CHAIN_ID", "777")],
        &context.artifacts.join("public-compute.log"),
    )?);
    let fixture = PublicFixture { children };
    wait_for_json(
        &context.client,
        &[
            format!("{storage_url}/health"),
            format!("{compute_url}/health"),
            format!("{compute_url}/v1/sync/subscriber"),
        ],
        Duration::from_secs(60),
    )
    .await?;
    Ok((fixture, compute_url, storage_url, compute_p2p))
}

fn spawn_logged(
    binary: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    log_path: &Path,
) -> Result<Child, String> {
    let stdout = std::fs::File::create(log_path).map_err(|error| error.to_string())?;
    let stderr = stdout.try_clone().map_err(|error| error.to_string())?;
    let mut command = Command::new(binary);
    command
        .args(args)
        .envs(env.iter().copied())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    command
        .spawn()
        .map_err(|error| format!("spawn {}: {error}", binary.display()))
}

fn configure_public_compute(path: &Path, _root: &Path) -> Result<(), String> {
    let mut config: toml::Value =
        toml::from_str(&std::fs::read_to_string(path).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    let root = config
        .as_table_mut()
        .ok_or("generated compute config root is not a TOML table")?;
    root.get_mut("identity")
        .and_then(toml::Value::as_table_mut)
        .ok_or("generated compute config lacks [identity]")?
        .insert(
            "did".into(),
            toml::Value::String(PUBLIC_VALIDATOR_DID.into()),
        );
    let compute = root
        .get_mut("compute")
        .and_then(toml::Value::as_table_mut)
        .ok_or("generated compute config lacks [compute]")?;
    compute.insert("chain_id".into(), toml::Value::String("777".into()));
    compute.insert(
        "embedded_supervisor_mode".into(),
        toml::Value::Boolean(true),
    );
    let network = root
        .get_mut("network")
        .and_then(toml::Value::as_table_mut)
        .ok_or("generated compute config lacks [network]")?;
    network.insert(
        "name".into(),
        toml::Value::String("spacekit-e2e-public".into()),
    );
    network.insert(
        "endpoint".into(),
        toml::Value::String("http://127.0.0.1".into()),
    );
    network.insert("bootstrap_nodes".into(), toml::Value::Array(Vec::new()));
    network.insert(
        "bind_address".into(),
        toml::Value::String("127.0.0.1".into()),
    );
    std::fs::write(
        path,
        toml::to_string_pretty(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

fn write_cli_identity(context: &Context, did: &str) -> Result<(), String> {
    let dir = context.home.join(".spacekit");
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    std::fs::write(
        dir.join("config.toml"),
        format!(
            "[identity]\ndid = \"{did}\"\nalgorithm = \"SPHINCS-128f\"\npublic_key_path = \"keys/public.hex\"\nprivate_key_path = \"keys/private.hex\"\n\n[network]\ndefault_network = \"spacekit-e2e-public\"\n\n[network.endpoints]\n\n[project]\nname = \"public-e2e\"\nversion = \"1.0.0\"\ncreated_at = \"{}\"\n",
            Utc::now().to_rfc3339()
        ),
    )
    .map_err(|error| error.to_string())
}

async fn publish_and_verify_operator_fact(
    context: &Context,
    storage_url: &str,
) -> Result<String, String> {
    let output = context.run_cli(
        "public-operator-publish",
        &[
            "operator",
            "publish",
            "--storage-url",
            storage_url,
            "--operator-did",
            PUBLIC_OPERATOR_DID,
            "--display-name",
            "SpaceKit E2E Operator",
            "--blob-fact-auth",
            "hybrid",
            "--feature",
            "subscriber-sync",
            "--sign",
        ],
    )?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let operator = get_json(
        &context.client,
        &format!("{storage_url}/api/operators/self"),
    )
    .await?;
    if operator["manifest_source"] != "published_fact"
        || operator["operator_did"] != PUBLIC_OPERATOR_DID
        || operator
            .pointer("/manifest/storage_http_url")
            .and_then(Value::as_str)
            != Some(storage_url)
    {
        return Err(format!(
            "published operator discovery response mismatch: {operator}"
        ));
    }
    let fact_id = operator["fact_id"]
        .as_str()
        .ok_or_else(|| format!("published operator response missing fact_id: {operator}"))?;
    let response = context
        .client
        .get(format!("{storage_url}/facts/{fact_id}"))
        .header("Authorization", format!("DID {PUBLIC_OPERATOR_DID}"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let fact: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success()
        || fact
            .pointer("/signature/signature_bytes")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        || fact.pointer("/signature/algorithm").and_then(Value::as_str) != Some("sphincs-128s")
    {
        return Err(format!(
            "operator fact is not signed as expected ({status}): {fact}"
        ));
    }
    std::fs::write(
        context.artifacts.join("public-operator-self.json"),
        serde_json::to_vec_pretty(&operator).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    std::fs::write(
        context.artifacts.join("public-operator-fact.json"),
        serde_json::to_vec_pretty(&fact).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "published signed SPHINCS operator fact {fact_id} and resolved it from CAS"
    ))
}

async fn join_and_verify_subscriber(
    context: &Context,
    manifest: &NetworkManifest,
    compute_url: &str,
) -> Result<String, String> {
    write_cli_identity(context, PUBLIC_SUBSCRIBER_DID)?;
    let manifest_path = context.artifacts.join("public-manifest.json");
    let output = context.run_cli(
        "public-subscriber-join",
        &[
            "network",
            "join",
            "--manifest",
            manifest_path.to_str().ok_or("non-UTF8 manifest path")?,
            "--role",
            "subscriber",
            "--force",
        ],
    )?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    let profile: SpacekitNetworkFile = toml::from_str(
        &std::fs::read_to_string(context.root.join("network/config.toml"))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    authorize_network_start(&profile, PUBLIC_SUBSCRIBER_DID).map_err(|error| error.to_string())?;
    if profile.role != NetworkRole::Subscriber
        || profile.resolved_compute_url() != compute_url
        || profile.admission.shared_genesis_hash.as_deref() != Some(&manifest.genesis.hash)
    {
        return Err(format!(
            "subscriber profile does not match manifest: {profile:?}"
        ));
    }
    let sync = get_json(
        &context.client,
        &format!("{compute_url}/v1/sync/subscriber"),
    )
    .await?;
    let genesis = get_json(&context.client, &format!("{compute_url}/block/0")).await?;
    if sync["wire_version"] != 1
        || sync["chain_id"] != manifest.chain_id.to_string()
        || sync.pointer("/head/number").and_then(Value::as_u64) != Some(0)
        || sync
            .pointer("/head/hash_hex")
            .and_then(Value::as_str)
            .is_none()
        || genesis["number"].as_u64() != Some(0)
        || manifest.protocol.name != NETWORK_PROTOCOL
        || manifest.protocol.version != NETWORK_PROTOCOL_VERSION
    {
        return Err(format!(
            "subscriber genesis/head/protocol mismatch: sync={sync}, genesis={genesis}"
        ));
    }
    std::fs::write(
        context.artifacts.join("public-subscriber-sync.json"),
        serde_json::to_vec_pretty(&sync).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "network join admitted subscriber; chain {}, genesis/head 0, sync wire {}, protocol {} v{} verified",
        manifest.chain_id, sync["wire_version"], manifest.protocol.name, manifest.protocol.version
    ))
}

fn verify_live_public_discovery(
    context: &Context,
    compute_url: &str,
    storage_url: &str,
) -> Result<String, String> {
    let output = context.run_cli(
        "public-live-discovery",
        &["network", "discover", "--detailed", "--limit", "10"],
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success()
        || !stdout.contains(compute_url)
        || !stdout.contains(storage_url)
        || !stdout.contains(PUBLIC_OPERATOR_DID)
        || !stdout.contains("published_fact")
    {
        return Err(format!(
            "live discovery did not resolve exact operator endpoints: {stdout}"
        ));
    }
    Ok("network discover queried the live registry and returned published operator storage plus compute RPC".into())
}

fn verify_public_role_readiness(
    context: &Context,
    manifest: &NetworkManifest,
) -> Result<String, String> {
    let manifest_path = context.artifacts.join("public-manifest.json");
    for (did, role, label) in [
        (PUBLIC_OPERATOR_DID, "operator", "public-operator-join"),
        (PUBLIC_VALIDATOR_DID, "validator", "public-validator-join"),
    ] {
        write_cli_identity(context, did)?;
        let output = context.run_cli(
            label,
            &[
                "network",
                "join",
                "--manifest",
                manifest_path.to_str().ok_or("non-UTF8 manifest path")?,
                "--role",
                role,
                "--force",
            ],
        )?;
        if !output.status.success() {
            return Err(format!(
                "{role} readiness join failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        crate::network_profile::validate_manifest_join(
            manifest,
            did,
            if role == "operator" {
                NetworkRole::Operator
            } else {
                NetworkRole::Validator
            },
        )
        .map_err(|error| error.to_string())?;
    }
    write_cli_identity(context, PUBLIC_SUBSCRIBER_DID)?;
    let restore = context.run_cli(
        "public-subscriber-restore",
        &[
            "network",
            "join",
            "--manifest",
            manifest_path.to_str().ok_or("non-UTF8 manifest path")?,
            "--role",
            "subscriber",
            "--force",
        ],
    )?;
    if !restore.status.success() {
        return Err("failed to restore subscriber profile after readiness checks".into());
    }
    Ok("listed operator and validator passed manifest admission and reachable published-service readiness".into())
}

fn verify_public_rejections(
    context: &Context,
    manifest: &NetworkManifest,
) -> Result<String, String> {
    let mut cases = Vec::new();
    let mut tampered = manifest.clone();
    tampered.chain_id += 1;
    cases.push((
        "tampered",
        tampered,
        PUBLIC_SUBSCRIBER_DID,
        "subscriber",
        "signature",
    ));
    let mut wrong_genesis = manifest.clone();
    wrong_genesis.genesis.document =
        Some(json!({"chain_id": 999, "accounts": [], "contracts": []}));
    cases.push((
        "wrong-genesis",
        wrong_genesis,
        PUBLIC_SUBSCRIBER_DID,
        "subscriber",
        "genesis",
    ));
    let mut wrong_version = manifest.clone();
    wrong_version.protocol.version += 1;
    cases.push((
        "wrong-version",
        wrong_version,
        PUBLIC_SUBSCRIBER_DID,
        "subscriber",
        "incompatible",
    ));
    cases.push((
        "unregistered-operator",
        manifest.clone(),
        "did:spacekit:public:intruder",
        "operator",
        "not admitted",
    ));

    for (label, candidate, did, role, expected) in cases {
        let path = context
            .artifacts
            .join(format!("public-{label}-manifest.json"));
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&candidate).map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        write_cli_identity(context, did)?;
        let output = context.run_cli(
            &format!("public-{label}-rejection"),
            &[
                "network",
                "join",
                "--manifest",
                path.to_str().ok_or("non-UTF8 rejection manifest path")?,
                "--role",
                role,
                "--force",
            ],
        )?;
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .to_ascii_lowercase();
        if output.status.success() || !combined.contains(expected) {
            return Err(format!(
                "{label} was not explicitly rejected for {expected}: {combined}"
            ));
        }
    }
    write_cli_identity(context, PUBLIC_SUBSCRIBER_DID)?;
    Ok("tampered signature, wrong genesis, wrong protocol version, and unregistered operator were explicitly rejected".into())
}

fn signed_public_manifest(
    compute_url: &str,
    storage_url: &str,
    compute_p2p: &str,
) -> Result<NetworkManifest, String> {
    let genesis = json!({"chain_id": 777, "accounts": [], "contracts": []});
    let mut manifest = NetworkManifest {
        version: NETWORK_MANIFEST_VERSION,
        network_id: "spacekit-e2e-public".into(),
        profile: NetworkPreset::Public,
        chain_id: 777,
        protocol: ManifestProtocol {
            name: NETWORK_PROTOCOL.into(),
            version: NETWORK_PROTOCOL_VERSION,
        },
        genesis: ManifestGenesis {
            hash: canonical_genesis_hash(&genesis).map_err(|error| error.to_string())?,
            uri: None,
            document: Some(genesis),
        },
        bootstrap: ManifestBootstrap {
            p2p: vec![compute_p2p.into()],
            rpc: vec![compute_url.into(), storage_url.into()],
        },
        roles: vec![
            NetworkRole::Subscriber,
            NetworkRole::Operator,
            NetworkRole::Validator,
        ],
        members: vec![
            ManifestMember {
                did: PUBLIC_OPERATOR_DID.into(),
                roles: vec![NetworkRole::Operator],
            },
            ManifestMember {
                did: PUBLIC_VALIDATOR_DID.into(),
                roles: vec![NetworkRole::Validator],
            },
        ],
        signature: None,
    };
    let payload = manifest
        .canonical_unsigned_bytes()
        .map_err(|error| error.to_string())?;
    let (public_key, secret_key) =
        spacekit_primitives::v1::crypto::quantum::generate_sphincs_keypair("sphincs-128f")
            .map_err(|error| error.to_string())?;
    let signed = spacekit_primitives::v1::crypto::quantum::sign_sphincs_detached(
        &payload,
        "sphincs-128f",
        &public_key,
        &secret_key,
    )
    .map_err(|error| error.to_string())?;
    manifest.signature = Some(ManifestSignature {
        algorithm: ManifestSignatureAlgorithm::Sphincs128f,
        encoding: ManifestSignatureEncoding::Hex,
        key_id: "did:spacekit:e2e#network-signing".into(),
        public_key: hex::encode(public_key),
        signature: hex::encode(signed.signature_bytes),
        signed_at: None,
    });
    Ok(manifest)
}

async fn wait_for_json(
    client: &reqwest::Client,
    urls: &[String],
    timeout: Duration,
) -> Result<String, String> {
    let started = Instant::now();
    let mut last = String::new();
    while started.elapsed() < timeout {
        let mut all = true;
        for url in urls {
            if let Err(error) = get_json(client, url).await {
                all = false;
                last = format!("{url}: {error}");
                break;
            }
        }
        if all {
            return Ok(format!("queried live JSON from {}", urls.join(", ")));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err(format!("readiness timeout: {last}"))
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<Value, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("{url} returned HTTP {status}"));
    }
    response.json().await.map_err(|error| error.to_string())
}

async fn put_and_get_document(
    client: &reqwest::Client,
    url: &str,
    did: &str,
    expected: &Value,
) -> Result<String, String> {
    let response = client
        .put(url)
        .header("authorization", format!("DID {did}"))
        .json(expected)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("PUT {url} returned {}", response.status()));
    }
    get_document(client, url, did, expected).await
}

async fn get_document(
    client: &reqwest::Client,
    url: &str,
    did: &str,
    expected: &Value,
) -> Result<String, String> {
    let response = client
        .get(url)
        .header("authorization", format!("DID {did}"))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("GET {url} returned {}", response.status()));
    }
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if value.pointer("/document/data") != Some(expected) {
        return Err(format!("stored document mismatch: {value}"));
    }
    Ok("live document round trip matched exact JSON".into())
}

async fn send_and_receive_message(client: &reqwest::Client, base: &str) -> Result<String, String> {
    let stream_url = format!(
        "{}/api/messages/stream?did={}",
        base.trim_end_matches('/'),
        percent_encoding::utf8_percent_encode(
            "did:spacekit:e2e:recipient",
            percent_encoding::NON_ALPHANUMERIC
        )
    );
    let mut stream = client
        .get(&stream_url)
        .send()
        .await
        .map_err(|error| format!("open SSE stream: {error}"))?;
    if !stream.status().is_success() {
        return Err(format!("SSE stream returned {}", stream.status()));
    }
    let response = client
        .post(format!(
            "{}/api/messages/envelope",
            base.trim_end_matches('/')
        ))
        .json(&json!({
            "message": {
                "kind": "chat",
                "payload": "spacekit-e2e",
                "context": {
                    "did": "did:spacekit:network:local",
                    "timestamp": 1,
                    "source": "network-e2e"
                }
            },
            "conversation_type": "direct",
            "recipient_did": "did:spacekit:e2e:recipient",
            "recipient_dids": ["did:spacekit:e2e:recipient"],
            "group_id": null
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let value: Value = response.json().await.map_err(|error| error.to_string())?;
    if !status.is_success() || value["status"] != "ok" || value["message_id"].as_str().is_none() {
        return Err(format!("message send failed ({status}): {value}"));
    }
    let received = tokio::time::timeout(Duration::from_secs(5), async {
        let mut body = Vec::new();
        while let Some(chunk) = stream.chunk().await.map_err(|error| error.to_string())? {
            body.extend_from_slice(&chunk);
            if String::from_utf8_lossy(&body).contains("spacekit-e2e") {
                return Ok::<_, String>(());
            }
            if body.len() > 64 * 1024 {
                return Err("SSE event exceeded 64 KiB without expected message".into());
            }
        }
        Err("SSE stream ended before receiving the message".into())
    })
    .await
    .map_err(|_| "timed out waiting for the SSE message".to_string())??;
    let _ = received;
    Ok(format!(
        "gateway accepted {} and recipient SSE observed exact payload",
        value["message_id"]
    ))
}

async fn deploy_call_wasm(
    client: &reqwest::Client,
    base: &str,
    address: &str,
    signing_key: Option<&k256::ecdsa::SigningKey>,
) -> Result<(String, String), String> {
    let wasm = wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "main") (param i32 i32) (result i32)
                i32.const 0)
        )"#,
    )
    .map_err(|error| error.to_string())?;
    let submit = |path: &str, body: Value| {
        client
            .post(format!("{}{}", base.trim_end_matches('/'), path))
            .json(&body)
            .send()
    };
    let deploy_data = hex::encode(&wasm);
    let deploy_signature = signing_key
        .map(|key| sign_swtchvm_http_tx(key, address, None, 0, 0, &deploy_data))
        .transpose()?;
    let deploy = submit(
        "/contract/deploy",
        json!({
            "from": address,
            "wasm_hex": deploy_data,
            "gas_limit": "1000000",
            "gas_price": "1",
            "value": "0",
            "nonce": 0,
            "signature": deploy_signature
        }),
    )
    .await
    .map_err(|error| error.to_string())?;
    let deploy_status = deploy.status();
    let deploy_body: Value = deploy.json().await.map_err(|error| error.to_string())?;
    if !deploy_status.is_success() {
        return Err(format!(
            "deploy submission returned {deploy_status}: {deploy_body}"
        ));
    }
    let deploy_hash = deploy_body["tx_hash"]
        .as_str()
        .ok_or_else(|| format!("deploy response missing tx_hash: {deploy_body}"))?;
    let deploy_block = submit("/mine", json!({}))
        .await
        .map_err(|error| error.to_string())?;
    if !deploy_block.status().is_success() {
        return Err(format!("deploy mine returned {}", deploy_block.status()));
    }
    let deploy_receipt = get_json(
        client,
        &format!(
            "{}/receipt/{}",
            base.trim_end_matches('/'),
            deploy_hash.trim_start_matches("0x")
        ),
    )
    .await?;
    if deploy_receipt["success"] != true {
        return Err(format!("deploy receipt failed: {deploy_receipt}"));
    }
    let contract = json_address(&deploy_receipt["created_address"])
        .ok_or_else(|| format!("deploy receipt missing created_address: {deploy_receipt}"))?;

    let call_signature = signing_key
        .map(|key| sign_swtchvm_http_tx(key, address, Some(&contract), 0, 1, ""))
        .transpose()?;
    let call = submit(
        "/contract/call",
        json!({
            "from": address,
            "contract": contract,
            "data_hex": "",
            "gas_limit": "1000000",
            "gas_price": "1",
            "value": "0",
            "nonce": 1,
            "signature": call_signature
        }),
    )
    .await
    .map_err(|error| error.to_string())?;
    let call_status = call.status();
    let call_body: Value = call.json().await.map_err(|error| error.to_string())?;
    if !call_status.is_success() {
        return Err(format!(
            "call submission returned {call_status}: {call_body}"
        ));
    }
    let call_hash = call_body["tx_hash"]
        .as_str()
        .ok_or_else(|| format!("call response missing tx_hash: {call_body}"))?;
    let call_block = submit("/mine", json!({}))
        .await
        .map_err(|error| error.to_string())?;
    if !call_block.status().is_success() {
        return Err(format!("call mine returned {}", call_block.status()));
    }
    let call_receipt = get_json(
        client,
        &format!(
            "{}/receipt/{}",
            base.trim_end_matches('/'),
            call_hash.trim_start_matches("0x")
        ),
    )
    .await?;
    if call_receipt["success"] != true {
        return Err(format!("call receipt failed: {call_receipt}"));
    }
    Ok((
        format!(
            "deployed {contract} in block {}, called in block {}; real receipts {} and {}",
            deploy_receipt["block_number"], call_receipt["block_number"], deploy_hash, call_hash
        ),
        contract,
    ))
}

fn sign_swtchvm_http_tx(
    key: &k256::ecdsa::SigningKey,
    from: &str,
    to: Option<&str>,
    value: u128,
    nonce: u64,
    data_hex: &str,
) -> Result<Value, String> {
    use k256::ecdsa::signature::hazmat::PrehashSigner;
    use sha2::{Digest as _, Sha256};

    let canonical = format!(
        "{}|{}|{}|{}|{}",
        from.trim_start_matches("0x").to_ascii_lowercase(),
        to.unwrap_or_default()
            .trim_start_matches("0x")
            .to_ascii_lowercase(),
        value,
        nonce,
        data_hex.trim_start_matches("0x").to_ascii_lowercase()
    );
    let prehash: [u8; 32] = Sha256::digest(canonical.as_bytes()).into();
    let (signature, recovery_id): (k256::ecdsa::Signature, k256::ecdsa::RecoveryId) = key
        .sign_prehash(&prehash)
        .map_err(|error| error.to_string())?;
    let bytes = signature.to_bytes();
    Ok(json!({
        "v": recovery_id.to_byte() + 27,
        "r_hex": hex::encode(&bytes[..32]),
        "s_hex": hex::encode(&bytes[32..]),
    }))
}

fn json_address(value: &Value) -> Option<String> {
    if let Some(address) = value.as_str() {
        return Some(if address.starts_with("0x") {
            address.to_string()
        } else {
            format!("0x{address}")
        });
    }
    let bytes = value.as_array()?;
    if bytes.len() != 20 {
        return None;
    }
    let raw = bytes
        .iter()
        .map(|byte| byte.as_u64().and_then(|value| u8::try_from(value).ok()))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("0x{}", hex::encode(raw)))
}

async fn faucet_and_rpc(
    client: &reqwest::Client,
    base: &str,
    address: &str,
) -> Result<String, String> {
    let faucet = client
        .post(format!("{}/faucet", base.trim_end_matches('/')))
        .json(&json!({
            "did": "did:spacekit:e2e:faucet",
            "address": address,
            "amount": 10000000
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let faucet_status = faucet.status();
    let faucet_value: Value = faucet.json().await.map_err(|error| error.to_string())?;
    if !faucet_status.is_success() || faucet_value["success"] != true {
        return Err(format!("faucet returned {faucet_status}: {faucet_value}"));
    }
    let rpc = client
        .post(format!("{}/rpc", base.trim_end_matches('/')))
        .json(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "eth_getBalance",
            "params": [address, "latest"]
        }))
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let value: Value = rpc.json().await.map_err(|error| error.to_string())?;
    let balance = value["result"]
        .as_str()
        .and_then(|raw| u128::from_str_radix(raw.trim_start_matches("0x"), 16).ok())
        .ok_or_else(|| format!("invalid RPC balance response: {value}"))?;
    if balance < 10_000_000 {
        return Err(format!("faucet balance did not reach 10000000: {value}"));
    }
    Ok(format!(
        "live faucet credited balance {balance}; JSON-RPC verified it"
    ))
}

async fn http_success(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if response.status().is_success() {
        Ok(format!("GET {url} returned {}", response.status()))
    } else {
        Err(format!("GET {url} returned {}", response.status()))
    }
}

fn write_report(path: &Path, report: &Report) -> Result<(), Box<dyn std::error::Error>> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if extension.eq_ignore_ascii_case("xml") {
        let mut body = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites tests=\"{}\" failures=\"{}\" skipped=\"{}\">\n",
            report.passed + report.failed + report.skipped,
            report.failed,
            report.skipped
        );
        for suite in &report.suites {
            body.push_str(&format!(
                "  <testsuite name=\"{}\" tests=\"{}\">\n",
                xml(&suite.suite),
                suite.gates.len()
            ));
            for gate in &suite.gates {
                body.push_str(&format!(
                    "    <testcase name=\"{}\" time=\"{:.3}\">",
                    xml(&gate.name),
                    gate.duration_ms as f64 / 1000.0
                ));
                match gate.status {
                    GateStatus::Failed => {
                        body.push_str(&format!("<failure message=\"{}\"/>", xml(&gate.detail)))
                    }
                    GateStatus::Skipped => {
                        body.push_str(&format!("<skipped message=\"{}\"/>", xml(&gate.detail)))
                    }
                    GateStatus::Passed => {
                        body.push_str(&format!("<system-out>{}</system-out>", xml(&gate.detail)))
                    }
                }
                body.push_str("</testcase>\n");
            }
            body.push_str("  </testsuite>\n");
        }
        body.push_str("</testsuites>\n");
        std::fs::write(path, body)?;
    } else {
        std::fs::write(path, serde_json::to_vec_pretty(report)?)?;
    }
    Ok(())
}

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
