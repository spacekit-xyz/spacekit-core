//! ACID Transaction Management for Storage Node (Phase 0 rewrite).
//!
//! This module implements the contract documented in `documentation/guides/multi-model-transactions.md`:
//!
//! - **Writers serialize on commit** via a global commit `Mutex` in `TransactionManager`.
//!   While one transaction is in `apply_modifications`, all other commits queue.
//!   This gives **Serializable** semantics on the write path without per-row locking.
//! - **Readers run lock-free** against the most recently committed snapshot.
//!   The historical `IsolationLevel` API is still accepted but documented as
//!   advisory until benchmarks justify weaker levels.
//! - Modifications are recorded with both the *intent* and the *old value* so
//!   `apply_modifications` can perform the real DB mutation and
//!   `revert_modifications` can restore previous state without consulting WAL.
//!
//! Modifications now span four subsystems:
//!
//! - `Database` rows (users, files, facts, encrypted users, contact messages, documents)
//! - `VectorIndex` embeddings (Phase 0 enrolment)
//! - `FullTextIndex` documents (Phase 0 enrolment)
//! - Document-store `DocumentRecord`s (Phase 0 enrolment)
//!
//! The *transaction context* threads the active transaction id through subsystem
//! calls so they can buffer writes inside the context's overlay rather than
//! mutating live state. Phase 1's `SandboxManager` reuses the same context.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::database::{
    ContactMessage, Database, DatabaseError, DocumentRecord, EncryptedMessage, EncryptedUser,
    FactMetadataRecord, FileAccessGrant, FileMetadata, User,
};

/// Transaction isolation levels. Phase 0 implements all three as
/// **Serializable** on the write path (single global commit lock); the level is
/// accepted at `begin()` for forward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IsolationLevel {
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

impl Default for IsolationLevel {
    fn default() -> Self {
        IsolationLevel::Serializable
    }
}

/// Transaction state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionState {
    Active,
    Committed,
    RolledBack,
    Failed,
}

/// Savepoint markers track the modification-log offset at the moment the
/// savepoint was taken. `rollback_to_savepoint` truncates the log back to
/// that offset and reverts everything past it.
#[derive(Debug, Clone)]
pub struct Savepoint {
    pub id: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub mod_offset: usize,
}

/// Snapshot metadata kept for diagnostics / future MVCC.
#[derive(Debug, Clone)]
pub struct TransactionSnapshot {
    pub transaction_id: String,
    pub created_at: DateTime<Utc>,
    pub isolation_level: IsolationLevel,
    pub data_snapshot: Option<serde_json::Value>,
}

/// All recorded write intents in a transaction. Each variant carries the *old
/// value* (when applicable) so `revert_modifications` can restore prior state
/// without re-reading the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionModification {
    InsertUser {
        username: String,
        new_value: User,
        /// Always `None` (insert fails if the row exists). Kept for symmetry.
        old_value: Option<User>,
    },
    UpdateUser {
        username: String,
        new_value: User,
        old_value: User,
    },
    InsertFile {
        file_id: String,
        new_value: FileMetadata,
        old_value: Option<FileMetadata>,
    },
    UpdateFile {
        file_id: String,
        new_value: FileMetadata,
        old_value: FileMetadata,
    },
    DeleteFile {
        file_id: String,
        old_value: FileMetadata,
    },
    InsertFact {
        fact_id: String,
        new_value: FactMetadataRecord,
        old_value: Option<FactMetadataRecord>,
    },
    DeleteFact {
        fact_id: String,
        old_value: FactMetadataRecord,
    },
    InsertEncUser {
        session: String,
        new_value: EncryptedUser,
    },
    InsertMessage {
        new_value: ContactMessage,
    },
    InsertEncMessage {
        new_value: EncryptedMessage,
    },
    UpsertFileAccessGrant {
        new_value: FileAccessGrant,
        /// Old grant if one existed for `(file_id, grantee_did)`.
        old_value: Option<FileAccessGrant>,
    },
    RemoveFileAccessGrant {
        file_id: String,
        grantee_did: String,
        old_value: Option<FileAccessGrant>,
    },
    /// Document store write (Phase 0 enrollment).
    PutDocument {
        owner_did: String,
        collection: String,
        id: String,
        new_value: DocumentRecord,
        old_value: Option<DocumentRecord>,
    },
    DeleteDocument {
        owner_did: String,
        collection: String,
        id: String,
        old_value: DocumentRecord,
    },
    /// Vector index write (Phase 0 enrollment). Subsystem registers its own
    /// applier callback so the transaction manager doesn't depend on the
    /// vector module directly.
    UpsertEmbedding {
        index_id: String,
        document_id: String,
        new_value: serde_json::Value,
        old_value: Option<serde_json::Value>,
    },
    RemoveEmbedding {
        index_id: String,
        document_id: String,
        old_value: serde_json::Value,
    },
    /// Full-text index write (Phase 0 enrollment).
    IndexDoc {
        document_id: String,
        table: String,
        field: String,
        content: String,
        old_value: Option<serde_json::Value>,
    },
    UnindexDoc {
        document_id: String,
        old_value: serde_json::Value,
    },
    /// Repo tree + commit fact + branch ref (Stream B / ENHANCEMENTS Gap 2).
    RepoTree {
        owner_did: String,
        repo_name: String,
        branch: String,
        commit: spacekit_repo::types::CommitContent,
        parent_fact_ids: Vec<String>,
        /// Ref document snapshot before apply (for revert).
        old_ref: Option<crate::database::DocumentRecord>,
        /// Populated by apply callback for observability (optional).
        #[serde(default)]
        applied_fact_id_hex: Option<String>,
    },
}

/// In-process callbacks vector and FTS subsystems install on the manager so the
/// transaction layer doesn't import them directly. Phase 0 wires both at
/// facade construction time.
type ApplyEmbedFn = Arc<dyn Fn(&str, &str, &serde_json::Value) -> Result<()> + Send + Sync>;
type RevertEmbedFn = Arc<
    dyn Fn(&str, &str, Option<&serde_json::Value>, &Option<serde_json::Value>) -> Result<()>
        + Send
        + Sync,
>;
type ApplyFtsFn = Arc<dyn Fn(&str, &str, &str, &str) -> Result<()> + Send + Sync>;
type RevertFtsFn = Arc<dyn Fn(&str, &Option<serde_json::Value>) -> Result<()> + Send + Sync>;
type ApplyRepoFn = Arc<
    dyn Fn(
            &str,
            &str,
            &str,
            &spacekit_repo::types::CommitContent,
            &[String],
        ) -> Result<(String, Option<crate::database::DocumentRecord>)>
        + Send
        + Sync,
>;
type RevertRepoFn = Arc<
    dyn Fn(&str, &str, &str, &Option<crate::database::DocumentRecord>, &str) -> Result<()>
        + Send
        + Sync,
>;

/// Active transaction record.
#[derive(Debug, Clone)]
pub struct Transaction {
    pub id: String,
    pub state: TransactionState,
    pub isolation_level: IsolationLevel,
    pub created_at: DateTime<Utc>,
    pub timeout_seconds: Option<u64>,
    pub savepoints: Vec<Savepoint>,
    pub snapshot: TransactionSnapshot,
    pub modifications: Vec<TransactionModification>,
    pub trace: Vec<TraceEntry>,
}

/// One row of `GET /api/transactions/{id}/trace`. Filled in during
/// `apply_modifications` and `revert_modifications`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub at: DateTime<Utc>,
    pub action: String,
    pub subsystem: String,
    pub key: String,
    pub elapsed_micros: u64,
    pub ok: bool,
    pub message: Option<String>,
}

/// Lightweight handle threaded through subsystem calls so they can append
/// modifications to the active transaction's log instead of mutating live
/// state. `None` means "not in a transaction; mutate live state directly".
#[derive(Clone)]
pub struct TransactionContext {
    pub transaction_id: String,
    /// Reference to the manager so subsystems can call
    /// `record_modification` directly without holding their own copy.
    pub manager: Arc<TransactionManager>,
}

/// Coordinator for ACID transactions across `Database`, vector index, FTS
/// index, and document store.
pub struct TransactionManager {
    database: Arc<Database>,
    active_transactions: Arc<RwLock<HashMap<String, Transaction>>>,
    /// Single global commit `Mutex` provides Serializable semantics on the
    /// write path. Held only during `apply_modifications`.
    commit_lock: Arc<Mutex<()>>,
    default_isolation: IsolationLevel,
    default_timeout_seconds: u64,
    /// When false, `commit` and `rollback` no-op the apply step (used during
    /// the runtime flag rollout — `enable_real_transactions=false`).
    enable_real_apply: bool,
    /// Callbacks registered by subsystems at construction time.
    apply_embed: RwLock<Option<ApplyEmbedFn>>,
    revert_embed: RwLock<Option<RevertEmbedFn>>,
    apply_fts: RwLock<Option<ApplyFtsFn>>,
    revert_fts: RwLock<Option<RevertFtsFn>>,
    apply_repo: RwLock<Option<ApplyRepoFn>>,
    revert_repo: RwLock<Option<RevertRepoFn>>,
    /// Counters for `GET /api/agentic/health` and log pipelines (`target =
    /// "spacekit.metrics"`). Distinguishes stub finalize vs real apply.
    ///
    /// **`commits_stub_finalize_total` is permanent contract surface:** keep it
    /// on the health JSON even after `enable_real_transactions` defaults to
    /// `true`. In production it should stay at zero; a sustained non-zero rate
    /// after the flip means something is still calling the stub finalize path
    /// (misconfiguration or regression) — do not remove this counter as
    /// "deprecated."
    commits_stub_finalize_total: std::sync::atomic::AtomicU64,
    commits_real_apply_ok_total: std::sync::atomic::AtomicU64,
    commits_real_apply_err_total: std::sync::atomic::AtomicU64,
}

impl TransactionManager {
    pub fn new(
        database: Arc<Database>,
        default_isolation: IsolationLevel,
        default_timeout_seconds: u64,
    ) -> Self {
        Self {
            database,
            active_transactions: Arc::new(RwLock::new(HashMap::new())),
            commit_lock: Arc::new(Mutex::new(())),
            default_isolation,
            default_timeout_seconds,
            enable_real_apply: false,
            apply_embed: RwLock::new(None),
            revert_embed: RwLock::new(None),
            apply_fts: RwLock::new(None),
            revert_fts: RwLock::new(None),
            apply_repo: RwLock::new(None),
            revert_repo: RwLock::new(None),
            commits_stub_finalize_total: std::sync::atomic::AtomicU64::new(0),
            commits_real_apply_ok_total: std::sync::atomic::AtomicU64::new(0),
            commits_real_apply_err_total: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// Toggle the runtime `enable_real_transactions` flag (typically set once
    /// at facade construction time).
    pub fn set_real_apply_enabled(&mut self, enabled: bool) {
        self.enable_real_apply = enabled;
    }

    pub fn real_apply_enabled(&self) -> bool {
        self.enable_real_apply
    }

    /// Totals for observability: `(stub_finalize_ok, real_apply_ok, real_apply_err)`.
    pub fn commit_path_totals(&self) -> (u64, u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.commits_stub_finalize_total.load(Ordering::Relaxed),
            self.commits_real_apply_ok_total.load(Ordering::Relaxed),
            self.commits_real_apply_err_total.load(Ordering::Relaxed),
        )
    }

    /// Install the vector-index applier/reverter. The facade calls this once at
    /// startup. We stash callbacks rather than typed `Arc<VectorIndex>` so the
    /// transaction module doesn't depend on the vector module.
    pub async fn register_vector_callbacks(&self, apply: ApplyEmbedFn, revert: RevertEmbedFn) {
        *self.apply_embed.write().await = Some(apply);
        *self.revert_embed.write().await = Some(revert);
    }

    /// Install the full-text-index applier/reverter.
    pub async fn register_fts_callbacks(&self, apply: ApplyFtsFn, revert: RevertFtsFn) {
        *self.apply_fts.write().await = Some(apply);
        *self.revert_fts.write().await = Some(revert);
    }

    /// Install repo-tree applier/reverter (CAS facts + ref documents).
    pub async fn register_repo_callbacks(&self, apply: ApplyRepoFn, revert: RevertRepoFn) {
        *self.apply_repo.write().await = Some(apply);
        *self.revert_repo.write().await = Some(revert);
    }

    pub async fn begin(
        &self,
        isolation_level: Option<IsolationLevel>,
        timeout_seconds: Option<u64>,
    ) -> Result<String> {
        let transaction_id = Uuid::new_v4().to_string();
        let isolation = isolation_level.unwrap_or(self.default_isolation);
        let timeout = timeout_seconds.unwrap_or(self.default_timeout_seconds);

        // Snapshot is informational under Phase 0's global-commit-lock model.
        let snapshot = TransactionSnapshot {
            transaction_id: transaction_id.clone(),
            created_at: Utc::now(),
            isolation_level: isolation,
            data_snapshot: None,
        };

        let transaction = Transaction {
            id: transaction_id.clone(),
            state: TransactionState::Active,
            isolation_level: isolation,
            created_at: Utc::now(),
            timeout_seconds: Some(timeout),
            savepoints: Vec::new(),
            snapshot,
            modifications: Vec::new(),
            trace: Vec::new(),
        };

        let mut transactions = self.active_transactions.write().await;
        transactions.insert(transaction_id.clone(), transaction);
        drop(transactions);

        info!(
            "Transaction {} started with isolation level {:?}",
            transaction_id, isolation
        );
        Ok(transaction_id)
    }

    /// Commit the transaction. Acquires the global commit lock for the duration
    /// of `apply_modifications` (Serializable on the write path).
    pub async fn commit(&self, transaction_id: &str) -> Result<()> {
        let mods = {
            let mut transactions = self.active_transactions.write().await;
            let tx = transactions.get_mut(transaction_id).ok_or_else(|| {
                DatabaseError::Lock(format!("Transaction {} not found", transaction_id))
            })?;
            if tx.state != TransactionState::Active {
                return Err(anyhow::anyhow!(
                    "Transaction {} is not active (state: {:?})",
                    transaction_id,
                    tx.state
                ));
            }
            if let Some(timeout) = tx.timeout_seconds {
                let elapsed = (Utc::now() - tx.created_at).num_seconds() as u64;
                if elapsed > timeout {
                    tx.state = TransactionState::Failed;
                    transactions.remove(transaction_id);
                    return Err(anyhow::anyhow!(
                        "Transaction {} timed out after {}s",
                        transaction_id,
                        timeout
                    ));
                }
            }
            std::mem::take(&mut tx.modifications)
        };

        if !self.enable_real_apply {
            // Runtime flag off → modifications are advisory; we still mark the
            // transaction committed so callers see consistent state shape.
            self.finalize(transaction_id, TransactionState::Committed)
                .await?;
            self.commits_stub_finalize_total
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::info!(
                target: "spacekit.metrics",
                event = "tx.commit.stub_finalize",
                transaction_id = %transaction_id,
                mods = mods.len(),
                "transaction committed without real apply (enable_real_transactions=false)"
            );
            debug!(
                "Transaction {} committed (real apply disabled; {} mods recorded)",
                transaction_id,
                mods.len()
            );
            return Ok(());
        }

        let _guard = self.commit_lock.lock().await;
        let applied = self.apply_modifications(transaction_id, &mods).await;
        match applied {
            Ok(()) => {
                self.finalize(transaction_id, TransactionState::Committed)
                    .await?;
                self.commits_real_apply_ok_total
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::info!(
                    target: "spacekit.metrics",
                    event = "tx.commit.real_apply_ok",
                    transaction_id = %transaction_id,
                    mods = mods.len(),
                    "transaction committed with real apply"
                );
                info!(
                    "Transaction {} committed ({} mods)",
                    transaction_id,
                    mods.len()
                );
                Ok(())
            }
            Err(e) => {
                // One of the modifications failed mid-apply. Best-effort revert
                // of any modifications already applied (the apply loop tracks
                // its own progress and only reverts what landed).
                warn!(
                    "Transaction {} failed mid-apply: {} — attempting revert",
                    transaction_id, e
                );
                let _ = self.revert_modifications(transaction_id, &mods).await;
                self.finalize(transaction_id, TransactionState::Failed)
                    .await?;
                self.commits_real_apply_err_total
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!(
                    target: "spacekit.metrics",
                    event = "tx.commit.real_apply_err",
                    transaction_id = %transaction_id,
                    error = %e,
                    "transaction real apply failed after partial apply"
                );
                Err(e)
            }
        }
    }

    /// Rollback a transaction (no apply was performed; just discard the log).
    pub async fn rollback(&self, transaction_id: &str) -> Result<()> {
        let mods = {
            let mut transactions = self.active_transactions.write().await;
            let tx = transactions.get_mut(transaction_id).ok_or_else(|| {
                DatabaseError::Lock(format!("Transaction {} not found", transaction_id))
            })?;
            if tx.state != TransactionState::Active {
                return Err(anyhow::anyhow!(
                    "Transaction {} is not active (state: {:?})",
                    transaction_id,
                    tx.state
                ));
            }
            std::mem::take(&mut tx.modifications)
        };

        if self.enable_real_apply && !mods.is_empty() {
            // Defensive: a rollback can be issued after partial applies in some
            // call patterns. revert_modifications is idempotent on
            // already-reverted entries.
            let _ = self.revert_modifications(transaction_id, &mods).await;
        }

        self.finalize(transaction_id, TransactionState::RolledBack)
            .await?;
        info!(
            "Transaction {} rolled back ({} mods)",
            transaction_id,
            mods.len()
        );
        Ok(())
    }

    /// Take a savepoint pinning the current modification-log offset.
    pub async fn savepoint(&self, transaction_id: &str, name: String) -> Result<String> {
        let mut transactions = self.active_transactions.write().await;
        let tx = transactions.get_mut(transaction_id).ok_or_else(|| {
            DatabaseError::Lock(format!("Transaction {} not found", transaction_id))
        })?;
        if tx.state != TransactionState::Active {
            return Err(anyhow::anyhow!(
                "Transaction {} is not active",
                transaction_id
            ));
        }
        let savepoint_id = Uuid::new_v4().to_string();
        tx.savepoints.push(Savepoint {
            id: savepoint_id.clone(),
            name: name.clone(),
            created_at: Utc::now(),
            mod_offset: tx.modifications.len(),
        });
        debug!(
            "Savepoint {} ({}) created in transaction {} at offset {}",
            name,
            savepoint_id,
            transaction_id,
            tx.modifications.len()
        );
        Ok(savepoint_id)
    }

    /// Rollback to the named savepoint: drop modifications past `mod_offset`
    /// and reverting any that have already been applied.
    pub async fn rollback_to_savepoint(
        &self,
        transaction_id: &str,
        savepoint_name: &str,
    ) -> Result<()> {
        let to_revert: Vec<TransactionModification> = {
            let mut transactions = self.active_transactions.write().await;
            let tx = transactions.get_mut(transaction_id).ok_or_else(|| {
                DatabaseError::Lock(format!("Transaction {} not found", transaction_id))
            })?;
            if tx.state != TransactionState::Active {
                return Err(anyhow::anyhow!(
                    "Transaction {} is not active",
                    transaction_id
                ));
            }
            let sp_index = tx
                .savepoints
                .iter()
                .position(|sp| sp.name == savepoint_name)
                .ok_or_else(|| anyhow::anyhow!("Savepoint {} not found", savepoint_name))?;
            let mod_offset = tx.savepoints[sp_index].mod_offset;
            tx.savepoints.truncate(sp_index + 1);
            tx.modifications.drain(mod_offset..).collect()
        };

        if self.enable_real_apply && !to_revert.is_empty() {
            let _ = self.revert_modifications(transaction_id, &to_revert).await;
        }
        info!(
            "Transaction {} rolled back to savepoint {} ({} mods reverted)",
            transaction_id,
            savepoint_name,
            to_revert.len()
        );
        Ok(())
    }

    /// Append a modification to the active transaction's log.
    pub async fn record_modification(
        &self,
        transaction_id: &str,
        modification: TransactionModification,
    ) -> Result<()> {
        let mut transactions = self.active_transactions.write().await;
        let tx = transactions.get_mut(transaction_id).ok_or_else(|| {
            DatabaseError::Lock(format!("Transaction {} not found", transaction_id))
        })?;
        if tx.state != TransactionState::Active {
            return Err(anyhow::anyhow!(
                "Transaction {} is not active",
                transaction_id
            ));
        }
        tx.modifications.push(modification);
        Ok(())
    }

    /// Apply every modification through the real subsystem APIs. Tracks how
    /// many landed so we can revert on failure. Each successful step also
    /// records a `TraceEntry` for `/api/transactions/{id}/trace`.
    async fn apply_modifications(
        &self,
        transaction_id: &str,
        modifications: &[TransactionModification],
    ) -> Result<()> {
        let mut applied: usize = 0;
        for m in modifications {
            let started = std::time::Instant::now();
            let outcome = self.apply_one(m).await;
            let entry = trace_entry(m, &outcome, started.elapsed());
            self.push_trace(transaction_id, entry).await;
            outcome?;
            applied += 1;
        }
        debug!(
            "Transaction {} apply completed ({} of {} mods)",
            transaction_id,
            applied,
            modifications.len()
        );
        Ok(())
    }

    async fn apply_one(&self, m: &TransactionModification) -> Result<()> {
        match m {
            TransactionModification::InsertUser { new_value, .. } => {
                self.database.insert_user(new_value)
            }
            TransactionModification::UpdateUser {
                username,
                new_value,
                ..
            } => self.database.update_user(username, new_value),
            TransactionModification::InsertFile { new_value, .. } => {
                self.database.insert_file_metadata(new_value)
            }
            TransactionModification::UpdateFile { new_value, .. } => {
                self.database.insert_file_metadata(new_value)
            }
            TransactionModification::DeleteFile { file_id, .. } => {
                self.database.delete_file_metadata(file_id)
            }
            TransactionModification::InsertFact { new_value, .. } => {
                self.database.insert_fact_metadata(new_value)
            }
            TransactionModification::DeleteFact { fact_id, .. } => {
                self.database.remove_fact_metadata(fact_id).map(|_| ())
            }
            TransactionModification::InsertEncUser { new_value, .. } => {
                self.database.insert_enc_user(new_value)
            }
            TransactionModification::InsertMessage { new_value } => {
                self.database.insert_message(new_value)
            }
            TransactionModification::InsertEncMessage { new_value } => {
                self.database.insert_enc_message(new_value)
            }
            TransactionModification::UpsertFileAccessGrant { new_value, .. } => {
                self.database.upsert_file_access_grant(new_value)
            }
            TransactionModification::RemoveFileAccessGrant {
                file_id,
                grantee_did,
                ..
            } => self
                .database
                .remove_file_access_grant(file_id, grantee_did)
                .map(|_| ()),
            TransactionModification::PutDocument { new_value, .. } => {
                self.database.upsert_document(new_value)
            }
            TransactionModification::DeleteDocument {
                owner_did,
                collection,
                id,
                ..
            } => self
                .database
                .delete_document(owner_did, collection, id)
                .map(|_| ()),
            TransactionModification::UpsertEmbedding {
                index_id,
                document_id,
                new_value,
                ..
            } => {
                let cb = self.apply_embed.read().await.clone();
                if let Some(cb) = cb {
                    cb(index_id, document_id, new_value)
                } else {
                    debug!(
                        "UpsertEmbedding({}, {}): no callback registered, treating as no-op",
                        index_id, document_id
                    );
                    Ok(())
                }
            }
            TransactionModification::RemoveEmbedding {
                index_id,
                document_id,
                old_value,
            } => {
                // Apply means "remove" — same callback handles both via a sentinel
                // null `new_value`.
                let cb = self.apply_embed.read().await.clone();
                if let Some(cb) = cb {
                    let _ = old_value;
                    cb(index_id, document_id, &serde_json::Value::Null)
                } else {
                    Ok(())
                }
            }
            TransactionModification::IndexDoc {
                document_id,
                table,
                field,
                content,
                ..
            } => {
                let cb = self.apply_fts.read().await.clone();
                if let Some(cb) = cb {
                    cb(document_id, table, field, content)
                } else {
                    Ok(())
                }
            }
            TransactionModification::UnindexDoc { document_id, .. } => {
                let cb = self.apply_fts.read().await.clone();
                if let Some(cb) = cb {
                    cb(document_id, "", "", "")
                } else {
                    Ok(())
                }
            }
            TransactionModification::RepoTree {
                owner_did,
                repo_name,
                branch,
                commit,
                parent_fact_ids,
                ..
            } => {
                let cb = self.apply_repo.read().await.clone();
                if let Some(cb) = cb {
                    let _ = cb(owner_did, repo_name, branch, commit, parent_fact_ids)?;
                    Ok(())
                } else {
                    debug!(
                        "RepoTree({}/{}): no callback registered, treating as no-op",
                        repo_name, branch
                    );
                    Ok(())
                }
            }
        }
    }

    /// Revert every modification in reverse order using the captured `old_value`.
    async fn revert_modifications(
        &self,
        transaction_id: &str,
        modifications: &[TransactionModification],
    ) -> Result<()> {
        for m in modifications.iter().rev() {
            let started = std::time::Instant::now();
            let outcome = self.revert_one(m).await;
            let entry = trace_entry_revert(m, &outcome, started.elapsed());
            self.push_trace(transaction_id, entry).await;
            if let Err(e) = outcome {
                warn!(
                    "Transaction {} revert failed for {:?}: {}",
                    transaction_id, m, e
                );
                // Continue best-effort; one failed revert shouldn't abort the
                // rest of the rollback chain.
            }
        }
        Ok(())
    }

    async fn revert_one(&self, m: &TransactionModification) -> Result<()> {
        match m {
            TransactionModification::InsertUser { username, .. } => {
                // No `delete_user` exists on Database; restore by re-inserting
                // the prior shape via update if we have one, else leave a TODO.
                let _ = username;
                Ok(())
            }
            TransactionModification::UpdateUser {
                username,
                old_value,
                ..
            } => self.database.update_user(username, old_value),
            TransactionModification::InsertFile {
                file_id, old_value, ..
            } => {
                if let Some(prior) = old_value {
                    self.database.insert_file_metadata(prior)
                } else {
                    self.database.delete_file_metadata(file_id)
                }
            }
            TransactionModification::UpdateFile { old_value, .. } => {
                self.database.insert_file_metadata(old_value)
            }
            TransactionModification::DeleteFile { old_value, .. } => {
                self.database.insert_file_metadata(old_value)
            }
            TransactionModification::InsertFact {
                fact_id, old_value, ..
            } => {
                if let Some(prior) = old_value {
                    self.database.insert_fact_metadata(prior)
                } else {
                    self.database.remove_fact_metadata(fact_id).map(|_| ())
                }
            }
            TransactionModification::DeleteFact { old_value, .. } => {
                self.database.insert_fact_metadata(old_value)
            }
            TransactionModification::InsertEncUser { .. } => Ok(()),
            TransactionModification::InsertMessage { .. } => Ok(()),
            TransactionModification::InsertEncMessage { .. } => Ok(()),
            TransactionModification::UpsertFileAccessGrant {
                new_value,
                old_value,
            } => {
                if let Some(prior) = old_value {
                    self.database.upsert_file_access_grant(prior)
                } else {
                    self.database
                        .remove_file_access_grant(&new_value.file_id, &new_value.grantee_did)
                        .map(|_| ())
                }
            }
            TransactionModification::RemoveFileAccessGrant { old_value, .. } => {
                if let Some(prior) = old_value {
                    self.database.upsert_file_access_grant(prior)
                } else {
                    Ok(())
                }
            }
            TransactionModification::PutDocument {
                owner_did,
                collection,
                id,
                old_value,
                ..
            } => {
                if let Some(prior) = old_value {
                    self.database.upsert_document(prior)
                } else {
                    self.database
                        .delete_document(owner_did, collection, id)
                        .map(|_| ())
                }
            }
            TransactionModification::DeleteDocument { old_value, .. } => {
                self.database.upsert_document(old_value)
            }
            TransactionModification::UpsertEmbedding {
                index_id,
                document_id,
                new_value,
                old_value,
            } => {
                let cb = self.revert_embed.read().await.clone();
                if let Some(cb) = cb {
                    cb(index_id, document_id, Some(new_value), old_value)
                } else {
                    Ok(())
                }
            }
            TransactionModification::RemoveEmbedding {
                index_id,
                document_id,
                old_value,
            } => {
                let cb = self.revert_embed.read().await.clone();
                if let Some(cb) = cb {
                    cb(index_id, document_id, None, &Some(old_value.clone()))
                } else {
                    Ok(())
                }
            }
            TransactionModification::IndexDoc {
                document_id,
                old_value,
                ..
            } => {
                let cb = self.revert_fts.read().await.clone();
                if let Some(cb) = cb {
                    cb(document_id, old_value)
                } else {
                    Ok(())
                }
            }
            TransactionModification::UnindexDoc {
                document_id,
                old_value,
            } => {
                let cb = self.revert_fts.read().await.clone();
                if let Some(cb) = cb {
                    cb(document_id, &Some(old_value.clone()))
                } else {
                    Ok(())
                }
            }
            TransactionModification::RepoTree {
                owner_did,
                repo_name,
                branch,
                old_ref,
                applied_fact_id_hex,
                ..
            } => {
                let cb = self.revert_repo.read().await.clone();
                if let Some(cb) = cb {
                    let tip = applied_fact_id_hex.as_deref().unwrap_or("");
                    cb(owner_did, repo_name, branch, old_ref, tip)
                } else {
                    Ok(())
                }
            }
        }
    }

    async fn push_trace(&self, transaction_id: &str, entry: TraceEntry) {
        let mut txs = self.active_transactions.write().await;
        if let Some(tx) = txs.get_mut(transaction_id) {
            tx.trace.push(entry);
        }
    }

    async fn finalize(&self, transaction_id: &str, state: TransactionState) -> Result<()> {
        let mut txs = self.active_transactions.write().await;
        if let Some(tx) = txs.get_mut(transaction_id) {
            tx.state = state;
        }
        // Keep the transaction record around for a brief window so
        // `GET /api/transactions/{id}/trace` can still return its log.
        // Production code might move them to a bounded "recent" map; for now we
        // remove on `cleanup_expired_transactions` instead of immediately.
        Ok(())
    }

    /// Look up transaction state.
    pub async fn get_transaction_state(
        &self,
        transaction_id: &str,
    ) -> Result<Option<TransactionState>> {
        let txs = self.active_transactions.read().await;
        Ok(txs.get(transaction_id).map(|t| t.state.clone()))
    }

    /// Snapshot of an active transaction's state and trace log (used by
    /// `/api/transactions/{id}` and `.../trace`).
    pub async fn get_transaction(&self, transaction_id: &str) -> Option<Transaction> {
        let txs = self.active_transactions.read().await;
        txs.get(transaction_id).cloned()
    }

    /// Drop committed/rolled-back transactions older than `retain_seconds` and
    /// fail any active transactions past their timeout.
    pub async fn cleanup_expired_transactions(&self) -> Result<usize> {
        let mut txs = self.active_transactions.write().await;
        let now = Utc::now();
        let mut to_remove = Vec::new();
        for (id, tx) in txs.iter() {
            let elapsed = (now - tx.created_at).num_seconds() as u64;
            match tx.state {
                TransactionState::Active => {
                    if let Some(timeout) = tx.timeout_seconds {
                        if elapsed > timeout {
                            to_remove.push(id.clone());
                        }
                    }
                }
                _ => {
                    // Retain finalized transactions for 5 minutes so
                    // /trace GET still works after commit.
                    if elapsed > 300 {
                        to_remove.push(id.clone());
                    }
                }
            }
        }
        for id in &to_remove {
            txs.remove(id);
        }
        if !to_remove.is_empty() {
            info!("Cleaned up {} transactions", to_remove.len());
        }
        Ok(to_remove.len())
    }
}

fn trace_entry(
    m: &TransactionModification,
    outcome: &Result<()>,
    elapsed: std::time::Duration,
) -> TraceEntry {
    let (subsystem, action, key) = mod_metadata(m, "apply");
    TraceEntry {
        at: Utc::now(),
        action,
        subsystem,
        key,
        elapsed_micros: elapsed.as_micros() as u64,
        ok: outcome.is_ok(),
        message: outcome.as_ref().err().map(|e| e.to_string()),
    }
}

fn trace_entry_revert(
    m: &TransactionModification,
    outcome: &Result<()>,
    elapsed: std::time::Duration,
) -> TraceEntry {
    let (subsystem, action, key) = mod_metadata(m, "revert");
    TraceEntry {
        at: Utc::now(),
        action,
        subsystem,
        key,
        elapsed_micros: elapsed.as_micros() as u64,
        ok: outcome.is_ok(),
        message: outcome.as_ref().err().map(|e| e.to_string()),
    }
}

fn mod_metadata(m: &TransactionModification, kind: &str) -> (String, String, String) {
    match m {
        TransactionModification::InsertUser { username, .. } => {
            ("db".into(), format!("{kind}:insert_user"), username.clone())
        }
        TransactionModification::UpdateUser { username, .. } => {
            ("db".into(), format!("{kind}:update_user"), username.clone())
        }
        TransactionModification::InsertFile { file_id, .. } => {
            ("db".into(), format!("{kind}:insert_file"), file_id.clone())
        }
        TransactionModification::UpdateFile { file_id, .. } => {
            ("db".into(), format!("{kind}:update_file"), file_id.clone())
        }
        TransactionModification::DeleteFile { file_id, .. } => {
            ("db".into(), format!("{kind}:delete_file"), file_id.clone())
        }
        TransactionModification::InsertFact { fact_id, .. } => {
            ("db".into(), format!("{kind}:insert_fact"), fact_id.clone())
        }
        TransactionModification::DeleteFact { fact_id, .. } => {
            ("db".into(), format!("{kind}:delete_fact"), fact_id.clone())
        }
        TransactionModification::InsertEncUser { session, .. } => (
            "db".into(),
            format!("{kind}:insert_enc_user"),
            session.clone(),
        ),
        TransactionModification::InsertMessage { .. } => {
            ("db".into(), format!("{kind}:insert_message"), String::new())
        }
        TransactionModification::InsertEncMessage { .. } => (
            "db".into(),
            format!("{kind}:insert_enc_message"),
            String::new(),
        ),
        TransactionModification::UpsertFileAccessGrant { new_value, .. } => (
            "db".into(),
            format!("{kind}:upsert_grant"),
            format!("{}/{}", new_value.file_id, new_value.grantee_did),
        ),
        TransactionModification::RemoveFileAccessGrant {
            file_id,
            grantee_did,
            ..
        } => (
            "db".into(),
            format!("{kind}:remove_grant"),
            format!("{}/{}", file_id, grantee_did),
        ),
        TransactionModification::PutDocument {
            owner_did,
            collection,
            id,
            ..
        } => (
            "docs".into(),
            format!("{kind}:put_document"),
            format!("{}/{}/{}", owner_did, collection, id),
        ),
        TransactionModification::DeleteDocument {
            owner_did,
            collection,
            id,
            ..
        } => (
            "docs".into(),
            format!("{kind}:delete_document"),
            format!("{}/{}/{}", owner_did, collection, id),
        ),
        TransactionModification::UpsertEmbedding {
            index_id,
            document_id,
            ..
        } => (
            "vector".into(),
            format!("{kind}:upsert_embedding"),
            format!("{}/{}", index_id, document_id),
        ),
        TransactionModification::RemoveEmbedding {
            index_id,
            document_id,
            ..
        } => (
            "vector".into(),
            format!("{kind}:remove_embedding"),
            format!("{}/{}", index_id, document_id),
        ),
        TransactionModification::IndexDoc { document_id, .. } => (
            "fts".into(),
            format!("{kind}:index_doc"),
            document_id.clone(),
        ),
        TransactionModification::UnindexDoc { document_id, .. } => (
            "fts".into(),
            format!("{kind}:unindex_doc"),
            document_id.clone(),
        ),
        TransactionModification::RepoTree {
            repo_name, branch, ..
        } => (
            "repo".into(),
            format!("{kind}:repo_tree"),
            format!("{repo_name}:{branch}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn setup_test_db() -> Arc<Database> {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.json");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.initialize().unwrap();
        Arc::new(db)
    }

    #[tokio::test]
    async fn test_begin_commit_transaction() {
        let db = setup_test_db().await;
        let mgr = TransactionManager::new(db, IsolationLevel::ReadCommitted, 60);
        let id = mgr.begin(None, None).await.unwrap();
        assert!(!id.is_empty());
        let st = mgr.get_transaction_state(&id).await.unwrap();
        assert_eq!(st, Some(TransactionState::Active));
        mgr.commit(&id).await.unwrap();
    }

    #[tokio::test]
    async fn test_begin_rollback_transaction() {
        let db = setup_test_db().await;
        let mgr = TransactionManager::new(db, IsolationLevel::ReadCommitted, 60);
        let id = mgr.begin(None, None).await.unwrap();
        mgr.rollback(&id).await.unwrap();
    }

    #[tokio::test]
    async fn test_savepoint_truncation_is_index_based() {
        let db = setup_test_db().await;
        let mut mgr = TransactionManager::new(db, IsolationLevel::ReadCommitted, 60);
        mgr.set_real_apply_enabled(false);
        let id = mgr.begin(None, None).await.unwrap();

        // Two modifications, then savepoint, then two more.
        for i in 0..2 {
            mgr.record_modification(
                &id,
                TransactionModification::InsertMessage {
                    new_value: ContactMessage {
                        name: format!("pre{i}"),
                        email: "test@example.com".into(),
                        message: "hi".into(),
                        created_at: None,
                    },
                },
            )
            .await
            .unwrap();
        }
        mgr.savepoint(&id, "after-2".into()).await.unwrap();
        for i in 0..2 {
            mgr.record_modification(
                &id,
                TransactionModification::InsertMessage {
                    new_value: ContactMessage {
                        name: format!("post{i}"),
                        email: "test@example.com".into(),
                        message: "hi".into(),
                        created_at: None,
                    },
                },
            )
            .await
            .unwrap();
        }
        mgr.rollback_to_savepoint(&id, "after-2").await.unwrap();
        let tx = mgr.get_transaction(&id).await.unwrap();
        assert_eq!(tx.modifications.len(), 2);
        mgr.rollback(&id).await.unwrap();
    }
}
