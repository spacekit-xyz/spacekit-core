//! Branch-CoW sandbox manager (Phase 1).
//!
//! Sandboxes are the agent-facing primitive for ephemeral isolated workspaces.
//! Each sandbox:
//!
//! - Branches off a base snapshot (`tip` of a repo, the live database state,
//!   or both).
//! - Records every write into a journal of FactPackages chained off
//!   `refs/sandboxes/<id>` in the CAS (preferred) or, when the CAS-backed path
//!   doesn't meet the p99 < 50ms benchmark, a local WAL.
//! - Tracks per-mod-type conflict policy (see [`ConflictPolicy`]).
//!
//! ## Process restarts and `Committing`
//!
//! When [`FacadeConfig::sandbox_persistence_root`](crate::storage_facade::FacadeConfig)
//! is set, each sandbox is snapshotted under `<root>/state/<id>.json` and a
//! monotonic `<root>/boot_epoch.txt` detects rows stuck in `committing` from a
//! prior process. On startup the facade runs [`SandboxManager::reconcile_stuck_committing`]
//! to replay the journal through [`crate::transaction::TransactionManager`] or
//! transition the sandbox to [`SandboxState::Failed`].

#![deny(clippy::all)]

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::change_feed::{ChangeEvent, ChangeFeed};
use crate::transaction::TransactionModification;

/// Per-sandbox configuration. Defaults are conservative.
#[derive(Debug, Clone, Copy)]
pub struct SandboxConfig {
    /// Time-to-live in seconds. Sandboxes auto-evict via [`SandboxReaper`].
    pub ttl_seconds: u64,
    /// Hard cap on `bytes_written`. 0 = unlimited (operator policy enforces).
    pub max_bytes_written: u64,
    /// Hard cap on vector index operations.
    pub max_vector_ops: u64,
    /// Hard cap on fact puts.
    pub max_fact_puts: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            ttl_seconds: 3600,
            max_bytes_written: 100 * 1024 * 1024,
            max_vector_ops: 10_000,
            max_fact_puts: 1_000,
        }
    }
}

/// Per-sandbox usage counters.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct SandboxQuotas {
    pub bytes_written: u64,
    pub vector_ops: u64,
    pub fact_puts: u64,
}

/// Conflict policy applied at commit time.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConflictPolicy {
    /// Default: reject on conflict, return 409 with structured diff.
    #[default]
    Reject,
    /// Last-writer-wins (opt-in, vector/FTS only).
    LastWriterWins,
    /// Three-way merge (repo trees only). Conflicts in the merge surface
    /// the `spacekit_diff` conflict markers.
    ThreeWayMerge,
    /// Optimistic with `If-Match` etag (document store).
    OptimisticIfMatch,
}

/// One journal entry. Ordered; each carries the modification that was
/// recorded against the sandbox plus optional metadata for the conflict
/// detector at commit time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalEntry {
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub modification: TransactionModification,
    pub conflict_policy: ConflictPolicy,
}

/// Sandbox state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SandboxState {
    Active,
    /// Commit or dry-run in progress — not eligible for TTL reap.
    Committing,
    Committed,
    Discarded,
    Expired,
    /// Reconciliation or real commit could not complete; operator may discard.
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sandbox {
    pub id: String,
    pub owner_did: String,
    /// DIDs allowed to read the sandbox and extend TTL (not commit/discard).
    #[serde(default)]
    pub collaborator_dids: Vec<String>,
    pub state: SandboxState,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub base_snapshot: Option<String>,
    /// Optional link to a `spacekit:workspace:v1` document for quota enforcement.
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub config: SandboxConfigSerde,
    pub quotas: SandboxQuotas,
    pub journal: Vec<JournalEntry>,
    /// When persistence is enabled, set to [`SandboxManager::boot_epoch`] while `committing`.
    #[serde(default)]
    pub commit_started_boot_epoch: Option<u64>,
    #[serde(default)]
    pub failure_reason: Option<String>,
}

/// `SandboxConfig` is `Copy`; `Sandbox` is serialized for HTTP responses, so we
/// mirror it as a serde-friendly variant.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SandboxConfigSerde {
    pub ttl_seconds: u64,
    pub max_bytes_written: u64,
    pub max_vector_ops: u64,
    pub max_fact_puts: u64,
}

impl From<SandboxConfig> for SandboxConfigSerde {
    fn from(cfg: SandboxConfig) -> Self {
        Self {
            ttl_seconds: cfg.ttl_seconds,
            max_bytes_written: cfg.max_bytes_written,
            max_vector_ops: cfg.max_vector_ops,
            max_fact_puts: cfg.max_fact_puts,
        }
    }
}

/// HTTP / tool authorization against a [`Sandbox`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxAccess {
    /// `GET` sandbox, `GET` journal.
    Read,
    /// `POST` extend.
    ExtendTtl,
    /// Append to sandbox journal via `X-Sandbox-Id` (same principals as [`Self::ExtendTtl`]).
    RecordJournal,
    /// `POST` commit (incl. dry-run), `POST` discard.
    OwnerWrite,
}

/// Returns true when `caller_did` may perform `access` on `sb`.
pub fn caller_may_access_sandbox(
    caller_did: Option<&str>,
    sb: &Sandbox,
    access: SandboxAccess,
) -> bool {
    let caller = caller_did.map(str::trim).filter(|s| !s.is_empty());
    let owner = sb.owner_did.trim();
    let is_anonymous_owner = owner == "did:spacekit:anonymous";

    let is_owner = match caller {
        Some(c) => c == owner,
        None => false,
    };

    if access == SandboxAccess::OwnerWrite {
        if is_owner {
            return true;
        }
        if is_anonymous_owner && caller.is_none() {
            return true;
        }
        return false;
    }

    // Read, extend TTL, or journal append: owner, collaborator, or anonymous-owner with no caller.
    if is_owner {
        return true;
    }
    if let Some(c) = caller {
        if sb.collaborator_dids.iter().any(|d| d.trim() == c) {
            return true;
        }
    }
    if is_anonymous_owner && caller.is_none() {
        return true;
    }
    false
}

#[derive(Debug)]
struct SandboxDisk {
    store: Arc<SandboxStore>,
    boot_epoch: u64,
}

/// Manager for sandboxes. Wired into [`crate::storage_facade::Facade`].
pub struct SandboxManager {
    sandboxes: RwLock<HashMap<String, Sandbox>>,
    seq: std::sync::atomic::AtomicU64,
    change_feed: Arc<ChangeFeed>,
    disk: Option<SandboxDisk>,
}

impl SandboxManager {
    pub fn new(change_feed: Arc<ChangeFeed>) -> Self {
        Self {
            sandboxes: RwLock::new(HashMap::new()),
            seq: std::sync::atomic::AtomicU64::new(0),
            change_feed,
            disk: None,
        }
    }

    /// In-memory manager plus on-disk snapshots under `persistence_root`.
    pub fn with_disk(
        change_feed: Arc<ChangeFeed>,
        store: Arc<SandboxStore>,
        boot_epoch: u64,
    ) -> Self {
        Self {
            sandboxes: RwLock::new(HashMap::new()),
            seq: std::sync::atomic::AtomicU64::new(0),
            change_feed,
            disk: Some(SandboxDisk { store, boot_epoch }),
        }
    }

    pub fn persistence_enabled(&self) -> bool {
        self.disk.is_some()
    }

    pub fn boot_epoch(&self) -> u64 {
        self.disk.as_ref().map(|d| d.boot_epoch).unwrap_or(0)
    }

    /// Load `state/*.json` into the in-memory map (replaces any prior entries with same ids).
    pub async fn load_from_disk(&self) -> Result<()> {
        let Some(disk) = &self.disk else {
            return Ok(());
        };
        let list = disk.store.load_all().await?;
        let mut max_seq = 0u64;
        let mut map = self.sandboxes.write().await;
        for sb in list {
            for e in &sb.journal {
                max_seq = max_seq.max(e.seq);
            }
            map.insert(sb.id.clone(), sb);
        }
        drop(map);
        let cur = self.seq.load(std::sync::atomic::Ordering::Relaxed);
        if max_seq > cur {
            self.seq
                .store(max_seq, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(())
    }

    /// Replay commit for sandboxes stuck in `committing` from a previous boot.
    pub async fn reconcile_stuck_committing(
        &self,
        tx_manager: Arc<crate::transaction::TransactionManager>,
    ) -> Result<()> {
        let Some(disk) = &self.disk else {
            return Ok(());
        };
        let boot = disk.boot_epoch;
        let ids: Vec<String> = {
            let g = self.sandboxes.read().await;
            g.iter()
                .filter(|(_, sb)| {
                    sb.state == SandboxState::Committing
                        && sb
                            .commit_started_boot_epoch
                            .map(|e| e < boot)
                            .unwrap_or(true)
                })
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in ids {
            if let Err(e) = self
                .resume_commit_after_crash(&id, tx_manager.clone())
                .await
            {
                warn!(sandbox = %id, error = %e, "sandbox commit reconciliation failed");
                {
                    let mut w = self.sandboxes.write().await;
                    if let Some(sb) = w.get_mut(&id) {
                        if sb.state == SandboxState::Committing {
                            sb.state = SandboxState::Failed;
                            sb.failure_reason = Some(e.to_string());
                            sb.commit_started_boot_epoch = None;
                        }
                    }
                }
                self.persist(&id).await;
            }
        }
        Ok(())
    }

    async fn persist(&self, id: &str) {
        let Some(disk) = &self.disk else {
            return;
        };
        let sb = self.sandboxes.read().await.get(id).cloned();
        if let Some(sb) = sb {
            if let Err(e) = disk.store.save(&sb).await {
                warn!(sandbox = %id, "sandbox disk persist: {}", e);
            }
        }
    }

    async fn persist_all_matching(&self, pred: impl Fn(&Sandbox) -> bool) {
        if self.disk.is_none() {
            return;
        }
        let ids: Vec<String> = {
            let g = self.sandboxes.read().await;
            g.iter()
                .filter(|(_, sb)| pred(sb))
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in ids {
            self.persist(&id).await;
        }
    }

    /// Open a new sandbox. Returns the sandbox id (used as
    /// `X-Sandbox-Id: <id>` on subsequent writes).
    /// Sum `bytes_written` across active sandboxes tagged with `workspace_id`.
    pub async fn workspace_bytes_in_use(&self, workspace_id: &str) -> u64 {
        let sandboxes = self.sandboxes.read().await;
        sandboxes
            .values()
            .filter(|sb| {
                sb.workspace_id.as_deref() == Some(workspace_id)
                    && matches!(sb.state, SandboxState::Active | SandboxState::Committing)
            })
            .map(|sb| sb.quotas.bytes_written)
            .sum()
    }

    pub async fn create(
        &self,
        owner_did: &str,
        cfg: SandboxConfig,
        base_snapshot: Option<String>,
        collaborator_dids: Vec<String>,
        workspace_id: Option<String>,
    ) -> Result<Sandbox> {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let mut collaborator_dids: Vec<String> = collaborator_dids
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s.as_str() != owner_did.trim())
            .collect();
        collaborator_dids.sort();
        collaborator_dids.dedup();

        let sandbox = Sandbox {
            id: id.clone(),
            owner_did: owner_did.to_string(),
            collaborator_dids,
            state: SandboxState::Active,
            created_at: now,
            expires_at: now + chrono::Duration::seconds(cfg.ttl_seconds as i64),
            base_snapshot,
            workspace_id: workspace_id.filter(|s| !s.is_empty()),
            config: cfg.into(),
            quotas: SandboxQuotas::default(),
            journal: Vec::new(),
            commit_started_boot_epoch: None,
            failure_reason: None,
        };
        self.sandboxes
            .write()
            .await
            .insert(id.clone(), sandbox.clone());
        self.change_feed
            .publish(ChangeEvent {
                seq: 0,
                occurred_at: now,
                did: Some(owner_did.to_string()),
                kind: "sandbox.created".into(),
                key: id.clone(),
                payload: None,
            })
            .await;
        info!("Sandbox {} created (owner_did={})", id, owner_did);
        self.persist(&id).await;
        Ok(sandbox)
    }

    pub async fn get(&self, id: &str) -> Option<Sandbox> {
        self.sandboxes.read().await.get(id).cloned()
    }

    /// Append a modification to a sandbox's journal. Updates quota counters.
    pub async fn record(
        &self,
        id: &str,
        modification: TransactionModification,
        policy: ConflictPolicy,
        bytes_written: u64,
    ) -> Result<()> {
        let mut sandboxes = self.sandboxes.write().await;
        let sb = sandboxes
            .get_mut(id)
            .ok_or_else(|| anyhow::anyhow!("sandbox {} not found", id))?;
        if sb.state != SandboxState::Active {
            return Err(anyhow::anyhow!(
                "sandbox {} is {:?}, cannot record",
                id,
                sb.state
            ));
        }
        let seq = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        sb.journal.push(JournalEntry {
            seq,
            at: Utc::now(),
            modification: modification.clone(),
            conflict_policy: policy,
        });
        sb.quotas.bytes_written = sb.quotas.bytes_written.saturating_add(bytes_written);
        match modification {
            TransactionModification::UpsertEmbedding { .. }
            | TransactionModification::RemoveEmbedding { .. } => {
                sb.quotas.vector_ops = sb.quotas.vector_ops.saturating_add(1);
            }
            TransactionModification::InsertFact { .. } => {
                sb.quotas.fact_puts = sb.quotas.fact_puts.saturating_add(1);
            }
            _ => {}
        }
        if let Some(reason) = quota_exceeded(&sb.quotas, &sb.config) {
            warn!(sandbox = %id, "quota exceeded: {}", reason);
        }
        drop(sandboxes);
        self.persist(id).await;
        Ok(())
    }

    /// Commit a sandbox. If `dry_run = true`, conflicts are reported but
    /// nothing is applied. The actual apply path runs through the
    /// transaction manager.
    pub async fn commit(
        &self,
        id: &str,
        tx_manager: Arc<crate::transaction::TransactionManager>,
        dry_run: bool,
    ) -> Result<CommitReport> {
        let modifications = {
            let mut sandboxes = self.sandboxes.write().await;
            let sb = sandboxes
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("sandbox {} not found", id))?;
            if sb.state != SandboxState::Active {
                return Err(anyhow::anyhow!(
                    "sandbox {} is {:?}, cannot commit",
                    id,
                    sb.state
                ));
            }
            let modifications = sb
                .journal
                .iter()
                .map(|e| e.modification.clone())
                .collect::<Vec<_>>();
            sb.state = SandboxState::Committing;
            if let Some(disk) = &self.disk {
                sb.commit_started_boot_epoch = Some(disk.boot_epoch);
            }
            modifications
        };

        self.persist(id).await;

        let restore_active = || async {
            let mut sandboxes = self.sandboxes.write().await;
            if let Some(sb) = sandboxes.get_mut(id) {
                if sb.state == SandboxState::Committing {
                    sb.state = SandboxState::Active;
                    sb.commit_started_boot_epoch = None;
                }
            }
            drop(sandboxes);
            self.persist(id).await;
        };

        let tx_id = match tx_manager.begin(None, None).await {
            Ok(id_tx) => id_tx,
            Err(e) => {
                restore_active().await;
                return Err(e);
            }
        };

        for m in &modifications {
            if let Err(e) = tx_manager.record_modification(&tx_id, m.clone()).await {
                let _ = tx_manager.rollback(&tx_id).await;
                restore_active().await;
                return Err(e);
            }
        }

        let report = if dry_run {
            let rb = tx_manager.rollback(&tx_id).await;
            restore_active().await;
            rb?;
            CommitReport {
                ok: true,
                applied: 0,
                conflicts: Vec::new(),
                dry_run: true,
                journal_size: modifications.len(),
            }
        } else {
            let applied = modifications.len();
            match tx_manager.commit(&tx_id).await {
                Ok(()) => {
                    let mut sandboxes = self.sandboxes.write().await;
                    if let Some(sb) = sandboxes.get_mut(id) {
                        if sb.state == SandboxState::Committing {
                            sb.state = SandboxState::Committed;
                            sb.commit_started_boot_epoch = None;
                            sb.failure_reason = None;
                        }
                    }
                    drop(sandboxes);
                    self.persist(id).await;
                    self.change_feed
                        .publish(ChangeEvent {
                            seq: 0,
                            occurred_at: Utc::now(),
                            did: None,
                            kind: "sandbox.committed".into(),
                            key: id.to_string(),
                            payload: None,
                        })
                        .await;
                    CommitReport {
                        ok: true,
                        applied,
                        conflicts: Vec::new(),
                        dry_run: false,
                        journal_size: modifications.len(),
                    }
                }
                Err(e) => {
                    let mut sandboxes = self.sandboxes.write().await;
                    if let Some(sb) = sandboxes.get_mut(id) {
                        if sb.state == SandboxState::Committing {
                            sb.state = SandboxState::Discarded;
                            sb.commit_started_boot_epoch = None;
                        }
                    }
                    drop(sandboxes);
                    self.persist(id).await;
                    return Err(e);
                }
            }
        };
        debug!("sandbox {} commit (dry_run={}) → {:?}", id, dry_run, report);
        Ok(report)
    }

    async fn resume_commit_after_crash(
        &self,
        id: &str,
        tx_manager: Arc<crate::transaction::TransactionManager>,
    ) -> Result<CommitReport> {
        let modifications = {
            let sb = self
                .sandboxes
                .read()
                .await
                .get(id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("sandbox {} not found", id))?;
            if sb.state != SandboxState::Committing {
                return Ok(CommitReport {
                    ok: true,
                    applied: 0,
                    conflicts: Vec::new(),
                    dry_run: false,
                    journal_size: 0,
                });
            }
            sb.journal
                .iter()
                .map(|e| e.modification.clone())
                .collect::<Vec<_>>()
        };

        let tx_id = tx_manager.begin(None, None).await?;
        for m in &modifications {
            if let Err(e) = tx_manager.record_modification(&tx_id, m.clone()).await {
                let _ = tx_manager.rollback(&tx_id).await;
                return Err(e);
            }
        }
        tx_manager.commit(&tx_id).await?;

        {
            let mut sandboxes = self.sandboxes.write().await;
            if let Some(sb) = sandboxes.get_mut(id) {
                if sb.state == SandboxState::Committing {
                    sb.state = SandboxState::Committed;
                    sb.commit_started_boot_epoch = None;
                    sb.failure_reason = None;
                }
            }
        }
        self.persist(id).await;
        self.change_feed
            .publish(ChangeEvent {
                seq: 0,
                occurred_at: Utc::now(),
                did: None,
                kind: "sandbox.reconciled".into(),
                key: id.to_string(),
                payload: None,
            })
            .await;

        Ok(CommitReport {
            ok: true,
            applied: modifications.len(),
            conflicts: Vec::new(),
            dry_run: false,
            journal_size: modifications.len(),
        })
    }

    /// Discard the sandbox and all its journal entries.
    pub async fn discard(&self, id: &str) -> Result<()> {
        let owner_for_feed = {
            let mut sandboxes = self.sandboxes.write().await;
            let sb = sandboxes
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("sandbox {} not found", id))?;
            if sb.state == SandboxState::Committing {
                return Err(anyhow::anyhow!(
                    "sandbox {} is mid-commit; wait for commit to finish before discard",
                    id
                ));
            }
            let owner = sb.owner_did.clone();
            sb.state = SandboxState::Discarded;
            sb.journal.clear();
            sb.commit_started_boot_epoch = None;
            sb.failure_reason = None;
            owner
        };
        self.change_feed
            .publish(ChangeEvent {
                seq: 0,
                occurred_at: Utc::now(),
                did: Some(owner_for_feed),
                kind: "sandbox.discarded".into(),
                key: id.to_string(),
                payload: None,
            })
            .await;
        self.persist(id).await;
        Ok(())
    }

    /// Extend the TTL of an active sandbox.
    pub async fn extend(&self, id: &str, ttl_seconds: u64) -> Result<DateTime<Utc>> {
        let exp = {
            let mut sandboxes = self.sandboxes.write().await;
            let sb = sandboxes
                .get_mut(id)
                .ok_or_else(|| anyhow::anyhow!("sandbox {} not found", id))?;
            if sb.state != SandboxState::Active {
                return Err(anyhow::anyhow!(
                    "sandbox {} is {:?}, cannot extend",
                    id,
                    sb.state
                ));
            }
            sb.expires_at = Utc::now() + chrono::Duration::seconds(ttl_seconds as i64);
            sb.expires_at
        };
        self.persist(id).await;
        Ok(exp)
    }

    /// Reap sandboxes whose TTL has elapsed.
    pub async fn reap(&self) -> usize {
        let mut sandboxes = self.sandboxes.write().await;
        let now = Utc::now();
        let mut reaped = 0;
        let to_expire: Vec<String> = sandboxes
            .iter()
            .filter(|(_, sb)| sb.state == SandboxState::Active && sb.expires_at < now)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &to_expire {
            if let Some(sb) = sandboxes.get_mut(id) {
                sb.state = SandboxState::Expired;
                sb.journal.clear();
                reaped += 1;
            }
        }
        drop(sandboxes);
        if reaped > 0 {
            info!("SandboxReaper expired {} sandboxes", reaped);
            self.persist_all_matching(|sb| sb.state == SandboxState::Expired)
                .await;
        }
        reaped
    }

    /// In-memory sandbox row count and estimated journal bytes.
    pub async fn memory_stats(&self) -> (usize, u64) {
        let g = self.sandboxes.read().await;
        let journal_bytes: u64 = g
            .values()
            .map(|sb| {
                sb.journal
                    .iter()
                    .filter_map(|e| serde_json::to_vec(e).ok())
                    .map(|v| v.len() as u64)
                    .sum::<u64>()
            })
            .sum();
        (g.len(), journal_bytes)
    }

    /// Aggregate counts for `/api/agentic/health` (total rows, per-state, quota sums).
    pub async fn health_aggregation(
        &self,
    ) -> (
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        usize,
        SandboxQuotas,
    ) {
        let g = self.sandboxes.read().await;
        let mut active = 0usize;
        let mut committing = 0usize;
        let mut committed = 0usize;
        let mut discarded = 0usize;
        let mut expired = 0usize;
        let mut failed = 0usize;
        let mut q = SandboxQuotas::default();
        for sb in g.values() {
            match sb.state {
                SandboxState::Active => active += 1,
                SandboxState::Committing => committing += 1,
                SandboxState::Committed => committed += 1,
                SandboxState::Discarded => discarded += 1,
                SandboxState::Expired => expired += 1,
                SandboxState::Failed => failed += 1,
            }
            q.bytes_written = q.bytes_written.saturating_add(sb.quotas.bytes_written);
            q.vector_ops = q.vector_ops.saturating_add(sb.quotas.vector_ops);
            q.fact_puts = q.fact_puts.saturating_add(sb.quotas.fact_puts);
        }
        let total = g.len();
        (
            total, active, committing, committed, discarded, expired, failed, q,
        )
    }
}

// ---- On-disk layout (`<data_dir>/sandboxes/`) ----

const STATE_DIR: &str = "state";
const BOOT_EPOCH_FILE: &str = "boot_epoch.txt";

/// Atomic JSON snapshots for [`Sandbox`].
#[derive(Debug)]
pub struct SandboxStore {
    root: PathBuf,
}

impl SandboxStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn state_dir(&self) -> PathBuf {
        self.root.join(STATE_DIR)
    }

    fn path_for(&self, sandbox_id: &str) -> PathBuf {
        self.state_dir().join(format!("{}.json", sandbox_id))
    }

    fn boot_epoch_path(&self) -> PathBuf {
        self.root.join(BOOT_EPOCH_FILE)
    }

    /// Ensure directories exist and bump the persisted boot epoch.
    pub async fn init_and_bump_boot_epoch(&self) -> Result<u64> {
        fs::create_dir_all(self.state_dir())
            .await
            .with_context(|| format!("create sandbox state dir {:?}", self.state_dir()))?;

        let path = self.boot_epoch_path();
        let mut n: u64 = match fs::read_to_string(&path).await {
            Ok(s) => s.trim().parse().unwrap_or(0),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => return Err(e).context("read boot_epoch.txt")?,
        };
        n = n.saturating_add(1);
        let tmp = path.with_extension("txt.tmp");
        fs::write(&tmp, format!("{}\n", n))
            .await
            .context("write boot_epoch tmp")?;
        fs::rename(&tmp, &path)
            .await
            .context("rename boot_epoch tmp")?;
        Ok(n)
    }

    pub async fn save(&self, sb: &Sandbox) -> Result<()> {
        fs::create_dir_all(self.state_dir())
            .await
            .with_context(|| format!("create sandbox state dir {:?}", self.state_dir()))?;
        let path = self.path_for(&sb.id);
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(sb).context("serialize sandbox")?;
        fs::write(&tmp, &json)
            .await
            .with_context(|| format!("write {:?}", tmp))?;
        fs::rename(&tmp, &path)
            .await
            .with_context(|| format!("rename sandbox snapshot {:?}", path))?;
        Ok(())
    }

    pub async fn load_all(&self) -> Result<Vec<Sandbox>> {
        let dir = self.state_dir();
        let mut out = Vec::new();
        let mut rd = match fs::read_dir(&dir).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(e).context("read sandbox state dir")?,
        };
        while let Some(ent) = rd.next_entry().await? {
            let p = ent.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = fs::read(&p)
                .await
                .with_context(|| format!("read {:?}", p))?;
            if bytes.is_empty() {
                continue;
            }
            match serde_json::from_slice::<Sandbox>(&bytes) {
                Ok(sb) => out.push(sb),
                Err(e) => {
                    warn!(
                        path = %p.display(),
                        error = %e,
                        "skip corrupt sandbox snapshot"
                    );
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitReport {
    pub ok: bool,
    pub applied: usize,
    pub conflicts: Vec<ConflictReport>,
    pub dry_run: bool,
    pub journal_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictReport {
    pub kind: String,
    pub key: String,
    pub message: String,
}

fn quota_exceeded(q: &SandboxQuotas, cfg: &SandboxConfigSerde) -> Option<&'static str> {
    if cfg.max_bytes_written > 0 && q.bytes_written > cfg.max_bytes_written {
        return Some("bytes_written");
    }
    if cfg.max_vector_ops > 0 && q.vector_ops > cfg.max_vector_ops {
        return Some("vector_ops");
    }
    if cfg.max_fact_puts > 0 && q.fact_puts > cfg.max_fact_puts {
        return Some("fact_puts");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_and_record() {
        let feed = Arc::new(ChangeFeed::new(8));
        let mgr = SandboxManager::new(feed);
        let sb = mgr
            .create(
                "did:spacekit:agent:1",
                SandboxConfig::default(),
                None,
                vec![],
                None,
            )
            .await
            .unwrap();
        assert_eq!(sb.state, SandboxState::Active);
        mgr.record(
            &sb.id,
            TransactionModification::InsertMessage {
                new_value: crate::database::ContactMessage {
                    name: "n".into(),
                    email: "e".into(),
                    message: "c".into(),
                    created_at: None,
                },
            },
            ConflictPolicy::Reject,
            42,
        )
        .await
        .unwrap();
        let s = mgr.get(&sb.id).await.unwrap();
        assert_eq!(s.journal.len(), 1);
        assert_eq!(s.quotas.bytes_written, 42);
    }

    #[tokio::test]
    async fn discard_and_reap() {
        let feed = Arc::new(ChangeFeed::new(8));
        let mgr = SandboxManager::new(feed);
        let mut cfg = SandboxConfig::default();
        cfg.ttl_seconds = 0;
        let sb = mgr
            .create("did:spacekit:reaper", cfg, None, vec![], None)
            .await
            .unwrap();
        {
            let mut sandboxes = mgr.sandboxes.write().await;
            if let Some(s) = sandboxes.get_mut(&sb.id) {
                s.expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
            }
        }
        let reaped = mgr.reap().await;
        assert_eq!(reaped, 1);
    }

    #[tokio::test]
    async fn reaper_does_not_expire_committing_sandbox() {
        let feed = Arc::new(ChangeFeed::new(8));
        let mgr = SandboxManager::new(feed);
        let mut cfg = SandboxConfig::default();
        cfg.ttl_seconds = 0;
        let sb = mgr
            .create("did:spacekit:committing", cfg, None, vec![], None)
            .await
            .unwrap();
        {
            let mut sandboxes = mgr.sandboxes.write().await;
            let s = sandboxes.get_mut(&sb.id).unwrap();
            s.expires_at = Utc::now() - chrono::Duration::seconds(1);
            s.state = SandboxState::Committing;
        }
        assert_eq!(mgr.reap().await, 0);
        let got = mgr.get(&sb.id).await.expect("sandbox still present");
        assert_eq!(got.state, SandboxState::Committing);
    }

    #[tokio::test]
    async fn disk_roundtrip_and_reconcile() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sandboxes");
        let store = Arc::new(SandboxStore::new(root.clone()));
        let boot1 = store.init_and_bump_boot_epoch().await.unwrap();
        let feed = Arc::new(ChangeFeed::new(8));
        let mgr = SandboxManager::with_disk(feed.clone(), store.clone(), boot1);
        let sb = mgr
            .create(
                "did:spacekit:owner",
                SandboxConfig::default(),
                None,
                vec![],
                None,
            )
            .await
            .unwrap();
        mgr.record(
            &sb.id,
            TransactionModification::InsertMessage {
                new_value: crate::database::ContactMessage {
                    name: "n".into(),
                    email: "e".into(),
                    message: "c".into(),
                    created_at: None,
                },
            },
            ConflictPolicy::Reject,
            1,
        )
        .await
        .unwrap();
        {
            let mut w = mgr.sandboxes.write().await;
            let s = w.get_mut(&sb.id).unwrap();
            s.state = SandboxState::Committing;
            s.commit_started_boot_epoch = Some(boot1);
        }
        mgr.persist(&sb.id).await;

        let boot2 = store.init_and_bump_boot_epoch().await.unwrap();
        assert!(boot2 > boot1);
        let mgr2 = SandboxManager::with_disk(feed, store.clone(), boot2);
        mgr2.load_from_disk().await.unwrap();
        let loaded = mgr2.get(&sb.id).await.unwrap();
        assert_eq!(loaded.state, SandboxState::Committing);

        let db_path = dir.path().join("db.json");
        let db =
            Arc::new(crate::database::Database::new(db_path.to_str().unwrap()).expect("test db"));
        let tx = Arc::new(crate::transaction::TransactionManager::new(
            db,
            crate::transaction::IsolationLevel::Serializable,
            300,
        ));
        mgr2.reconcile_stuck_committing(tx.clone()).await.unwrap();
        let after = mgr2.get(&sb.id).await.unwrap();
        assert_eq!(after.state, SandboxState::Committed);
    }

    #[tokio::test]
    async fn caller_may_access_collaborator_read_not_commit() {
        let sb = Sandbox {
            id: "1".into(),
            owner_did: "did:o:alice".into(),
            collaborator_dids: vec!["did:o:bob".into()],
            workspace_id: None,
            state: SandboxState::Active,
            created_at: Utc::now(),
            expires_at: Utc::now(),
            base_snapshot: None,
            config: SandboxConfig::default().into(),
            quotas: SandboxQuotas::default(),
            journal: vec![],
            commit_started_boot_epoch: None,
            failure_reason: None,
        };
        assert!(caller_may_access_sandbox(
            Some("did:o:bob"),
            &sb,
            SandboxAccess::Read
        ));
        assert!(caller_may_access_sandbox(
            Some("did:o:bob"),
            &sb,
            SandboxAccess::ExtendTtl
        ));
        assert!(caller_may_access_sandbox(
            Some("did:o:bob"),
            &sb,
            SandboxAccess::RecordJournal
        ));
        assert!(!caller_may_access_sandbox(
            Some("did:o:bob"),
            &sb,
            SandboxAccess::OwnerWrite
        ));
        assert!(caller_may_access_sandbox(
            Some("did:o:alice"),
            &sb,
            SandboxAccess::OwnerWrite
        ));
    }
}
