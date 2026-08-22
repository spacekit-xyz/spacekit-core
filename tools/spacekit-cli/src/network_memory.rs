//! `spacekit network memory` — aggregate RSS, storage diagnostics, and disk scans.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use colored::Colorize;
use serde::{Deserialize, Serialize};

use crate::network_profile;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StorageMemoryReport {
    generated_at: String,
    config: StorageMemConfig,
    database: StorageMemDb,
    in_memory_caches: StorageMemCaches,
    disk: StorageMemDisk,
    suspects: Vec<StorageMemSuspect>,
    hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StorageMemConfig {
    enable_p2p: bool,
    cache_p2p_chunks_in_memory: bool,
    data_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StorageMemDb {
    data_file_bytes: u64,
    file_metadata_rows: usize,
    fact_metadata_rows: usize,
    document_rows: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StorageMemCaches {
    files_cache_entries: usize,
    #[serde(default)]
    p2p_stored_chunks: usize,
    #[serde(default)]
    p2p_stored_chunk_bytes: u64,
    idempotency_entries: usize,
    idempotency_body_bytes: u64,
    idempotency_largest_body_bytes: u64,
    sandbox_rows: usize,
    sandbox_journal_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StorageMemDisk {
    data_dir_total_bytes: u64,
    data_dir_file_count: u64,
    blob_sidecar_bytes: u64,
    fact_json_bytes: u64,
    encrypted_file_blobs_bytes: u64,
    largest_files: Vec<DiskFileRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskFileRow {
    path: String,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StorageMemSuspect {
    id: String,
    label: String,
    estimated_bytes: u64,
    detail: String,
    severity: String,
}

struct ProcessMem {
    pid: u32,
    name: String,
    rss_bytes: u64,
}

pub async fn run_network_memory(
    json: bool,
    sample: bool,
    watch: bool,
    interval_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let net = network_profile::load_spacekit_network_file()?;
    let state = network_profile::load_network_runtime_state()?;

    let Some(state) = state else {
        return Err("network supervisor is not running — start with `spacekit network up`".into());
    };

    if watch {
        return run_memory_watch(&net, &state, interval_secs).await;
    }

    let storage_report = fetch_storage_report(&net).await;

    let mut processes = collect_processes(state.pid);

    let compute_data = net
        .as_ref()
        .map(|n| network_profile::resolve_data_dir(n, "compute"));
    let messaging_data = net
        .as_ref()
        .map(|n| network_profile::resolve_data_dir(n, "messaging"));

    let blockchain = net.as_ref().map(blockchain_runtime_info);

    if json {
        let out = serde_json::json!({
            "supervisor_pid": state.pid,
            "uptime_secs": (chrono::Utc::now() - state.started_at).num_seconds(),
            "processes": processes.iter().map(|p| serde_json::json!({
                "pid": p.pid,
                "name": p.name,
                "rss_bytes": p.rss_bytes,
            })).collect::<Vec<_>>(),
            "blockchain": blockchain,
            "storage_report": storage_report,
            "compute_data_dir": compute_data.as_ref().map(|p| p.display().to_string()),
            "messaging_data_dir": messaging_data.as_ref().map(|p| p.display().to_string()),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    println!("{}", "🔬 SpaceKit network memory diagnostic".green().bold());
    println!();

    let total_rss: u64 = processes.iter().map(|p| p.rss_bytes).sum();
    println!("{}", "Process RSS".cyan().bold());
    if processes.is_empty() {
        println!(
            "   {} no live supervisor process (pid {})",
            "○".yellow(),
            state.pid
        );
    } else {
        for p in &processes {
            let tag = if p.pid == state.pid {
                "supervisor"
            } else {
                "child"
            };
            println!(
                "   pid {:>6}  {:>10}  {}  {}",
                p.pid,
                human_bytes(p.rss_bytes),
                tag,
                p.name
            );
        }
        println!(
            "   {} combined RSS: {}",
            "Σ".cyan(),
            human_bytes(total_rss).yellow()
        );
    }
    println!();

    if let Some(ref bc) = blockchain {
        print_blockchain_runtime(bc);
    }

    match storage_report {
        Some(ref r) => print_storage_report(r),
        None => {
            println!(
                "{}",
                "⚠  Could not fetch GET /api/agentic/memory — rebuild CLI/storage-node and restart network.".yellow()
            );
            if let Some(net) = net.as_ref() {
                let data_dir = network_profile::resolve_data_dir(net, "storage");
                print_disk_fallback(&data_dir);
            }
        }
    }

    if let Some(dir) = compute_data {
        let bytes = dir_size(&dir);
        if bytes > 0 {
            println!();
            println!("{}", "Compute data dir (disk)".cyan().bold());
            println!("   {}  {}", dir.display(), human_bytes(bytes));
            println!(
                "   {} embedded compute task RAM is not exposed via HTTP — high RSS with small disk may be growformer/WASM in-process",
                "ℹ".blue()
            );
        }
    }

    if let Some(dir) = messaging_data {
        let bytes = dir_size(&dir);
        if bytes > 0 {
            println!();
            println!("{}", "Messaging data dir (disk)".cyan().bold());
            println!("   {}  {}", dir.display(), human_bytes(bytes));
        }
    }

    if sample && state.pid > 0 {
        println!();
        println!("{}", "macOS sample (5s)".cyan().bold());
        match run_sample(state.pid) {
            Ok(path) => println!("   wrote {}", path.display()),
            Err(e) => println!("   {} {}", "✗".red(), e),
        }
    }

    println!();
    println!(
        "   Re-run: {}  |  JSON: {}  |  sample: {}  |  watch: {}",
        "spacekit network memory".green(),
        "spacekit network memory --json".green(),
        "spacekit network memory --sample".green(),
        "spacekit network memory --watch".green()
    );

    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BlockchainRuntimeInfo {
    enabled: bool,
    block_time_ms: u64,
    persist_interval_blocks: u64,
    persist_state: bool,
    ledger_path: String,
    ledger_file_bytes: u64,
    block_number: Option<u64>,
}

fn blockchain_runtime_info(net: &network_profile::SpacekitNetworkFile) -> BlockchainRuntimeInfo {
    let ledger_path = net
        .blockchain
        .state_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| network_profile::default_data_dir("blockchain"))
        .join("ledger.json");
    let ledger_file_bytes = std::fs::metadata(&ledger_path)
        .map(|m| m.len())
        .unwrap_or(0);
    let block_number = read_ledger_block_number(&ledger_path);

    BlockchainRuntimeInfo {
        enabled: net.blockchain.enabled,
        block_time_ms: network_profile::resolve_block_time_ms(net),
        persist_interval_blocks: network_profile::resolve_persist_interval_blocks(net),
        persist_state: net.blockchain.persist_state,
        ledger_path: ledger_path.display().to_string(),
        ledger_file_bytes,
        block_number,
    }
}

fn read_ledger_block_number(path: &PathBuf) -> Option<u64> {
    let data = std::fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    v.get("block_number")?.as_u64()
}

fn print_blockchain_runtime(bc: &BlockchainRuntimeInfo) {
    println!("{}", "Blockchain (embedded supervisor)".cyan().bold());
    if !bc.enabled {
        println!("   {} disabled (plain `network up`)", "○".green());
        println!();
        return;
    }
    println!(
        "   {} enabled  block_time={}  persist every {} blocks",
        "●".yellow(),
        format!("{}ms", bc.block_time_ms).yellow(),
        bc.persist_interval_blocks
    );
    if let Some(bn) = bc.block_number {
        println!(
            "   ledger: block #{}  file {}  {}",
            bn,
            human_bytes(bc.ledger_file_bytes),
            bc.ledger_path
        );
    } else {
        println!(
            "   ledger: {}  {}",
            human_bytes(bc.ledger_file_bytes),
            bc.ledger_path
        );
    }
    if bc.block_time_ms < network_profile::MIN_RECOMMENDED_BLOCK_TIME_MS {
        println!(
            "   {} fast block_time_ms increases RSS — raise [blockchain] block_time_ms or use `network up` without --full",
            "⚠".yellow()
        );
    }
    println!();
}

async fn fetch_storage_report(
    net: &Option<network_profile::SpacekitNetworkFile>,
) -> Option<StorageMemoryReport> {
    let storage_url = net
        .as_ref()
        .map(|n| n.resolved_storage_url())
        .unwrap_or_else(|| "http://127.0.0.1:3030".to_string());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;

    let memory_url = format!("{}/api/agentic/memory", storage_url.trim_end_matches('/'));

    match client.get(&memory_url).send().await {
        Ok(resp) if resp.status().is_success() => resp.json().await.ok(),
        _ => None,
    }
}

fn collect_processes(supervisor_pid: u32) -> Vec<ProcessMem> {
    let mut processes = Vec::new();
    if supervisor_pid > 0 && network_profile::process_alive(supervisor_pid) {
        if let Some(p) = process_rss(supervisor_pid) {
            processes.push(p);
        }
        processes.extend(child_processes(supervisor_pid));
    }
    processes
}

async fn run_memory_watch(
    net: &Option<network_profile::SpacekitNetworkFile>,
    state: &network_profile::NetworkRuntimeState,
    interval_secs: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    println!(
        "{}",
        "📈 Watching supervisor RSS (Ctrl-C to stop)".green().bold()
    );
    println!("   pid {}  interval {}s", state.pid, interval_secs);
    println!();

    let mut prev_rss: Option<u64> = None;
    let mut prev_at = std::time::Instant::now();
    let interval = Duration::from_secs(interval_secs.max(1));

    loop {
        let processes = collect_processes(state.pid);
        let total_rss: u64 = processes.iter().map(|p| p.rss_bytes).sum();
        let storage_report = fetch_storage_report(net).await;

        let now = chrono::Utc::now().format("%H:%M:%S");
        let delta = prev_rss.map(|p| total_rss as i64 - p as i64);
        let rate = prev_rss.map(|p| {
            let elapsed = prev_at.elapsed().as_secs_f64().max(0.001);
            (total_rss as f64 - p as f64) / elapsed
        });

        print!("\r[{now}] RSS {} ", human_bytes(total_rss));
        if let Some(d) = delta {
            if d > 0 {
                print!("(+{}) ", human_bytes(d as u64).red());
            } else if d < 0 {
                print!("({}) ", human_bytes((-d) as u64).green());
            }
        }
        if let Some(r) = rate {
            if r.abs() > 1024.0 {
                print!("{}/s ", human_bytes(r.abs() as u64));
                if r > 0.0 {
                    print!("↑ ");
                } else {
                    print!("↓ ");
                }
            }
        }

        if let Some(ref bc) = net.as_ref().map(blockchain_runtime_info) {
            if bc.enabled {
                print!(
                    "| chain:{} {}ms #{} ",
                    "on".yellow(),
                    bc.block_time_ms,
                    bc.block_number.unwrap_or(0)
                );
            }
        }
        if let Some(ref r) = storage_report {
            print!(
                "| P2P:{} chunks:{} idem:{}",
                if r.config.enable_p2p { "on" } else { "off" },
                r.in_memory_caches.p2p_stored_chunks,
                r.in_memory_caches.idempotency_entries
            );
        } else {
            print!("| (no /api/agentic/memory — rebuild + restart)");
        }

        std::io::stdout().flush().ok();

        prev_rss = Some(total_rss);
        prev_at = std::time::Instant::now();
        tokio::time::sleep(interval).await;
    }
}

fn print_storage_report(r: &StorageMemoryReport) {
    println!("{}", "Storage node (live)".cyan().bold());
    println!(
        "   P2P: {}  chunk RAM cache: {}",
        if r.config.enable_p2p {
            "on".yellow()
        } else {
            "off".green()
        },
        if r.config.cache_p2p_chunks_in_memory {
            "on".red()
        } else {
            "off".green()
        }
    );
    println!("   data_dir: {}", r.config.data_dir);

    println!();
    println!("{}", "In-memory estimates".cyan().bold());
    println!(
        "   JSON DB mirror (file):     {}",
        human_bytes(r.database.data_file_bytes).yellow()
    );
    println!(
        "   rows: files={} facts={} docs={}",
        r.database.file_metadata_rows, r.database.fact_metadata_rows, r.database.document_rows
    );
    if r.in_memory_caches.p2p_stored_chunk_bytes > 0 {
        println!(
            "   P2P stored_chunks:         {} ({})",
            human_bytes(r.in_memory_caches.p2p_stored_chunk_bytes).red(),
            r.in_memory_caches.p2p_stored_chunks
        );
    }
    if r.in_memory_caches.idempotency_body_bytes > 0 {
        println!(
            "   Idempotency cache:         {} ({} entries, max body {})",
            human_bytes(r.in_memory_caches.idempotency_body_bytes),
            r.in_memory_caches.idempotency_entries,
            human_bytes(r.in_memory_caches.idempotency_largest_body_bytes)
        );
    }
    if r.in_memory_caches.sandbox_journal_bytes > 0 {
        println!(
            "   Sandbox journals:          {} ({} rows)",
            human_bytes(r.in_memory_caches.sandbox_journal_bytes),
            r.in_memory_caches.sandbox_rows
        );
    }

    println!();
    println!("{}", "On disk".cyan().bold());
    println!(
        "   data_dir total:            {} ({} files)",
        human_bytes(r.disk.data_dir_total_bytes),
        r.disk.data_dir_file_count
    );
    println!(
        "   blobs: {}  facts(json): {}  file blobs: {}",
        human_bytes(r.disk.blob_sidecar_bytes),
        human_bytes(r.disk.fact_json_bytes),
        human_bytes(r.disk.encrypted_file_blobs_bytes)
    );
    if !r.disk.largest_files.is_empty() {
        println!("   largest:");
        for f in r.disk.largest_files.iter().take(8) {
            println!("     {}  {}", human_bytes(f.bytes), f.path);
        }
    }

    if !r.suspects.is_empty() {
        println!();
        println!("{}", "Ranked suspects".cyan().bold());
        for s in r.suspects.iter().take(8) {
            let sev = match s.severity.as_str() {
                "critical" => s.severity.red().to_string(),
                "high" => s.severity.yellow().to_string(),
                _ => s.severity.normal().to_string(),
            };
            println!(
                "   [{}] {} — {} ({})",
                sev,
                s.label,
                human_bytes(s.estimated_bytes),
                s.detail
            );
        }
    }

    if !r.hints.is_empty() {
        println!();
        println!("{}", "Hints".cyan().bold());
        for h in &r.hints {
            println!("   • {}", h);
        }
    }
}

fn print_disk_fallback(data_dir: &PathBuf) {
    println!();
    println!("{}", "Storage disk (offline scan)".cyan().bold());
    let bytes = dir_size(data_dir);
    println!("   {}  {}", data_dir.display(), human_bytes(bytes));
}

fn process_rss(pid: u32) -> Option<ProcessMem> {
    let out = Command::new("ps")
        .args(["-o", "pid=,rss=,comm=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    parse_ps_line(&String::from_utf8_lossy(&out.stdout))
}

fn child_processes(parent_pid: u32) -> Vec<ProcessMem> {
    let out = Command::new("ps")
        .args(["-eo", "pid=,ppid=,rss=,comm="])
        .output();
    let Ok(out) = out else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 4 {
                return None;
            }
            let pid: u32 = parts[0].parse().ok()?;
            let ppid: u32 = parts[1].parse().ok()?;
            if ppid != parent_pid {
                return None;
            }
            let rss_kb: u64 = parts[2].parse().ok()?;
            let name = parts[3..].join(" ");
            Some(ProcessMem {
                pid,
                name,
                rss_bytes: rss_kb * 1024,
            })
        })
        .collect()
}

fn parse_ps_line(line: &str) -> Option<ProcessMem> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    let pid: u32 = parts[0].parse().ok()?;
    let rss_kb: u64 = parts[1].parse().ok()?;
    let name = parts[2..].join(" ");
    Some(ProcessMem {
        pid,
        name,
        rss_bytes: rss_kb * 1024,
    })
}

fn dir_size(path: &PathBuf) -> u64 {
    if !path.exists() {
        return 0;
    }
    let mut total = 0u64;
    walk_dir_size(path, &mut total);
    total
}

fn walk_dir_size(dir: &std::path::Path, total: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_dir_size(&path, total);
        } else if let Ok(m) = entry.metadata() {
            *total += m.len();
        }
    }
}

fn run_sample(pid: u32) -> Result<PathBuf, String> {
    let ts = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let path = std::env::temp_dir().join(format!("spacekit-memory-sample-{}-{}.txt", pid, ts));
    let status = Command::new("sample")
        .args([pid.to_string().as_str(), "5", "1", "-file"])
        .arg(&path)
        .status()
        .map_err(|e| format!("sample failed: {e}"))?;
    if !status.success() {
        return Err("sample exited non-zero (install Xcode CLI tools?)".into());
    }
    Ok(path)
}

pub fn human_bytes(n: u64) -> String {
    spacekit_storage_node::memory_diagnostic::human_bytes(n)
}
