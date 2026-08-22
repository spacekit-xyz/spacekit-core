//! In-process and on-disk memory diagnostics for operator debugging.
//!
//! Surfaces counts and byte estimates for structures that commonly balloon RSS
//! during long-running `network up` sessions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::RwLock;

use crate::database::Database;
use crate::idempotency::IdempotencyCache;
use crate::sandbox::SandboxManager;
use crate::storage_facade::Facade;

#[cfg(feature = "p2p")]
use crate::network::P2PNetwork;

use crate::{StorageNodeConfig, StoredFile};

/// Optional live handles wired from [`crate::StorageNode::start`].
#[derive(Clone, Default)]
pub struct MemoryDiagnosticSources {
    pub files: Option<Arc<RwLock<HashMap<String, StoredFile>>>>,
    #[cfg(feature = "p2p")]
    pub p2p: Option<Arc<P2PNetwork>>,
    pub enable_p2p: bool,
    pub cache_p2p_chunks_in_memory: bool,
}

/// Full operator snapshot returned by `GET /api/agentic/memory`.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryDiagnosticReport {
    pub generated_at: String,
    pub config: MemoryConfigSection,
    pub database: MemoryDatabaseSection,
    pub in_memory_caches: MemoryInMemorySection,
    pub disk: MemoryDiskSection,
    pub suspects: Vec<MemorySuspect>,
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryConfigSection {
    pub enable_p2p: bool,
    pub cache_p2p_chunks_in_memory: bool,
    pub data_dir: String,
    pub max_storage_gb: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryDatabaseSection {
    pub data_file_bytes: u64,
    pub file_metadata_rows: usize,
    pub fact_metadata_rows: usize,
    pub document_rows: usize,
    pub message_rows: usize,
    pub metadata_size_sum_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryInMemorySection {
    pub files_cache_entries: usize,
    pub files_cache_metadata_bytes: u64,
    #[cfg(feature = "p2p")]
    pub p2p_stored_chunks: usize,
    #[cfg(feature = "p2p")]
    pub p2p_stored_chunk_bytes: u64,
    pub idempotency_entries: usize,
    pub idempotency_body_bytes: u64,
    pub idempotency_largest_body_bytes: u64,
    pub sandbox_rows: usize,
    pub sandbox_journal_bytes: u64,
    pub session_keypairs: Option<usize>,
    pub pending_challenges: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryDiskSection {
    pub data_dir_total_bytes: u64,
    pub data_dir_file_count: u64,
    pub blob_sidecar_bytes: u64,
    pub fact_json_bytes: u64,
    pub encrypted_file_blobs_bytes: u64,
    pub sandbox_snapshot_bytes: u64,
    pub largest_files: Vec<DiskFileEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskFileEntry {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemorySuspect {
    pub id: String,
    pub label: String,
    pub estimated_bytes: u64,
    pub detail: String,
    pub severity: String,
}

/// Collect a full memory diagnostic report.
pub async fn collect_memory_report(
    config: &StorageNodeConfig,
    database: &Arc<Database>,
    facade: &Arc<Facade>,
    sources: &MemoryDiagnosticSources,
    session_keypairs: Option<usize>,
    pending_challenges: Option<usize>,
) -> MemoryDiagnosticReport {
    let db_stats = database.get_storage_stats().ok();

    let (idempotency_entries, idempotency_body_bytes, idempotency_largest_body_bytes) =
        facade.idempotency.memory_stats().await;
    let (sandbox_rows, sandbox_journal_bytes) = facade.sandboxes.memory_stats().await;

    let (files_cache_entries, files_cache_metadata_bytes) =
        files_cache_stats(sources.files.as_ref()).await;

    #[cfg(feature = "p2p")]
    let (p2p_stored_chunks, p2p_stored_chunk_bytes) = if let Some(p2p) = sources.p2p.as_ref() {
        p2p.stored_chunks_memory_estimate().await
    } else {
        (0, 0)
    };

    let disk = scan_data_dir(&config.data_dir).await;

    let database_section = MemoryDatabaseSection {
        data_file_bytes: db_stats.as_ref().map(|s| s.data_file_size).unwrap_or(0),
        file_metadata_rows: db_stats.as_ref().map(|s| s.file_count).unwrap_or(0),
        fact_metadata_rows: db_stats
            .as_ref()
            .map(|s| s.fact_metadata_count)
            .unwrap_or(0),
        document_rows: db_stats.as_ref().map(|s| s.document_count).unwrap_or(0),
        message_rows: db_stats.as_ref().map(|s| s.message_count).unwrap_or(0),
        metadata_size_sum_bytes: db_stats.as_ref().map(|s| s.total_file_size).unwrap_or(0),
    };

    let in_memory = MemoryInMemorySection {
        files_cache_entries,
        files_cache_metadata_bytes,
        #[cfg(feature = "p2p")]
        p2p_stored_chunks,
        #[cfg(feature = "p2p")]
        p2p_stored_chunk_bytes,
        idempotency_entries,
        idempotency_body_bytes,
        idempotency_largest_body_bytes,
        sandbox_rows,
        sandbox_journal_bytes,
        session_keypairs,
        pending_challenges,
    };

    let config_section = MemoryConfigSection {
        enable_p2p: sources.enable_p2p,
        cache_p2p_chunks_in_memory: sources.cache_p2p_chunks_in_memory,
        data_dir: config.data_dir.display().to_string(),
        max_storage_gb: config.max_storage_bytes / (1024 * 1024 * 1024),
    };

    let mut suspects = rank_suspects(&database_section, &in_memory, &disk);
    suspects.sort_by(|a, b| b.estimated_bytes.cmp(&a.estimated_bytes));

    let hints = build_hints(&config_section, &in_memory, &disk, &suspects);

    MemoryDiagnosticReport {
        generated_at: chrono::Utc::now().to_rfc3339(),
        config: config_section,
        database: database_section,
        in_memory_caches: in_memory,
        disk,
        suspects,
        hints,
    }
}

async fn files_cache_stats(
    files: Option<&Arc<RwLock<HashMap<String, StoredFile>>>>,
) -> (usize, u64) {
    let Some(files) = files else {
        return (0, 0);
    };
    let guard = files.read().await;
    let bytes: u64 = guard
        .values()
        .map(|f| {
            (f.metadata.filename.len()
                + f.metadata.id.len()
                + f.metadata.owner_did.len()
                + f.data_chunks.len() * 64) as u64
        })
        .sum();
    (guard.len(), bytes)
}

fn rank_suspects(
    db: &MemoryDatabaseSection,
    mem: &MemoryInMemorySection,
    disk: &MemoryDiskSection,
) -> Vec<MemorySuspect> {
    let mut out = Vec::new();

    #[cfg(feature = "p2p")]
    if mem.p2p_stored_chunk_bytes > 0 {
        out.push(MemorySuspect {
            id: "p2p_stored_chunks".into(),
            label: "P2P in-memory chunk cache".into(),
            estimated_bytes: mem.p2p_stored_chunk_bytes,
            detail: format!(
                "{} chunks retained (cache_p2p_chunks_in_memory should be false for local dev)",
                mem.p2p_stored_chunks
            ),
            severity: severity_for(mem.p2p_stored_chunk_bytes),
        });
    }

    if mem.idempotency_body_bytes > 10 * 1024 * 1024 {
        out.push(MemorySuspect {
            id: "idempotency_cache".into(),
            label: "Idempotency response cache".into(),
            estimated_bytes: mem.idempotency_body_bytes,
            detail: format!(
                "{} entries, largest body {} bytes",
                mem.idempotency_entries, mem.idempotency_largest_body_bytes
            ),
            severity: severity_for(mem.idempotency_body_bytes),
        });
    }

    if db.data_file_bytes > 100 * 1024 * 1024 {
        out.push(MemorySuspect {
            id: "json_database_mirror".into(),
            label: "JSON database file (fully loaded in RAM)".into(),
            estimated_bytes: db.data_file_bytes,
            detail: format!(
                "storage.db/json is {} — entire structure is deserialized into process memory",
                human_bytes(db.data_file_bytes)
            ),
            severity: severity_for(db.data_file_bytes),
        });
    }

    if disk.encrypted_file_blobs_bytes > 200 * 1024 * 1024 {
        out.push(MemorySuspect {
            id: "disk_file_blobs".into(),
            label: "Encrypted file blobs on disk".into(),
            estimated_bytes: disk.encrypted_file_blobs_bytes,
            detail: "Large on-disk blobs; downloads may spike transient RSS".into(),
            severity: "info".into(),
        });
    }

    if mem.sandbox_journal_bytes > 5 * 1024 * 1024 {
        out.push(MemorySuspect {
            id: "sandbox_journals".into(),
            label: "Sandbox in-memory journals".into(),
            estimated_bytes: mem.sandbox_journal_bytes,
            detail: format!("{} sandbox rows", mem.sandbox_rows),
            severity: severity_for(mem.sandbox_journal_bytes),
        });
    }

    if disk.data_dir_total_bytes > 500 * 1024 * 1024 {
        out.push(MemorySuspect {
            id: "data_dir_total".into(),
            label: "Total storage data directory".into(),
            estimated_bytes: disk.data_dir_total_bytes,
            detail: format!(
                "{} files under data_dir — compare to process RSS",
                disk.data_dir_file_count
            ),
            severity: "info".into(),
        });
    }

    out
}

fn build_hints(
    config: &MemoryConfigSection,
    mem: &MemoryInMemorySection,
    disk: &MemoryDiskSection,
    suspects: &[MemorySuspect],
) -> Vec<String> {
    let mut hints = Vec::new();

    if config.enable_p2p && mem.p2p_stored_chunk_bytes > 50 * 1024 * 1024 {
        hints.push(
            "P2P chunk RAM is high — set [runtime] enable_p2p = false or cache_p2p_chunks_in_memory = false in network config, then restart.".into(),
        );
    }

    if !config.enable_p2p && mem.p2p_stored_chunk_bytes > 0 {
        hints.push(
            "P2P is disabled but stored_chunks is non-zero — likely started before the fix; restart network supervisor.".into(),
        );
    }

    if mem.idempotency_largest_body_bytes > 1024 * 1024 {
        hints.push(
            "Idempotency cache holds multi-MB response bodies — large artifact routes may be cached for 24h.".into(),
        );
    }

    if disk.largest_files.len() >= 3 {
        let top = &disk.largest_files[0];
        hints.push(format!(
            "Largest on-disk artifact: {} ({})",
            top.path,
            human_bytes(top.bytes)
        ));
    }

    if suspects.is_empty() {
        hints.push(
            "No dominant in-process suspects from storage node — check compute node tasks, growformer brain, and child processes (messaging-http, gateway) via `spacekit network memory`.".into(),
        );
    }

    hints.push(
        "Run `spacekit network memory --sample` to capture a macOS stack sample of the supervisor PID.".into(),
    );

    hints
}

fn severity_for(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        "critical".into()
    } else if bytes >= 256 * 1024 * 1024 {
        "high".into()
    } else if bytes >= 64 * 1024 * 1024 {
        "medium".into()
    } else {
        "low".into()
    }
}

pub fn human_bytes(n: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if n >= GB {
        format!("{:.2} GB", n as f64 / GB as f64)
    } else if n >= MB {
        format!("{:.1} MB", n as f64 / MB as f64)
    } else if n >= KB {
        format!("{:.1} KB", n as f64 / KB as f64)
    } else {
        format!("{} B", n)
    }
}

#[derive(Default)]
struct DiskScan {
    total_bytes: u64,
    file_count: u64,
    blob_sidecar_bytes: u64,
    fact_json_bytes: u64,
    encrypted_file_blobs_bytes: u64,
    sandbox_snapshot_bytes: u64,
    largest: Vec<(PathBuf, u64)>,
}

async fn scan_data_dir(data_dir: &Path) -> MemoryDiskSection {
    let mut scan = DiskScan::default();
    if data_dir.exists() {
        walk_dir(data_dir, data_dir, &mut scan);
    }
    scan.largest.sort_by(|a, b| b.1.cmp(&a.1));
    let largest_files = scan
        .largest
        .into_iter()
        .take(15)
        .map(|(p, bytes)| DiskFileEntry {
            path: p.display().to_string(),
            bytes,
        })
        .collect();

    MemoryDiskSection {
        data_dir_total_bytes: scan.total_bytes,
        data_dir_file_count: scan.file_count,
        blob_sidecar_bytes: scan.blob_sidecar_bytes,
        fact_json_bytes: scan.fact_json_bytes,
        encrypted_file_blobs_bytes: scan.encrypted_file_blobs_bytes,
        sandbox_snapshot_bytes: scan.sandbox_snapshot_bytes,
        largest_files,
    }
}

fn walk_dir(root: &Path, dir: &Path, scan: &mut DiskScan) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            walk_dir(root, &path, scan);
            continue;
        }
        let bytes = meta.len();
        scan.total_bytes += bytes;
        scan.file_count += 1;

        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        if rel.ends_with(".blob") {
            scan.blob_sidecar_bytes += bytes;
        } else if rel.contains("/facts/") && rel.ends_with(".json") {
            scan.fact_json_bytes += bytes;
        } else if rel.starts_with("sandboxes/") {
            scan.sandbox_snapshot_bytes += bytes;
        } else if looks_like_uuid_file(&rel) {
            scan.encrypted_file_blobs_bytes += bytes;
        }

        scan.largest.push((path.clone(), bytes));
        if scan.largest.len() > 40 {
            scan.largest.sort_by(|a, b| b.1.cmp(&a.1));
            scan.largest.truncate(20);
        }
    }
}

fn looks_like_uuid_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.len() == 36
        && name.chars().filter(|c| *c == '-').count() == 4
        && !name.ends_with(".json")
        && !name.ends_with(".bak")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats() {
        assert!(human_bytes(7_670_000_000).contains("GB"));
    }
}
