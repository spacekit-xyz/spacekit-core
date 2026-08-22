//! Unified storage facade — the single seam every read/write goes through.
//!
//! Phase 0 introduces this module as the substrate for transactions,
//! sandboxes, idempotency, change events, and the in-process MCP server. It
//! holds shared `Arc`s to the existing subsystems (`Database`, `VectorIndex`,
//! `FullTextIndex`) plus the new coordinators (`TransactionManager`,
//! `IdempotencyCache`, `DidRateLimiter`, `ChangeFeed`, `SandboxManager`).
//!
//! Existing API handlers in [`crate::api`] will be migrated to call
//! `Facade::*` instead of holding `Arc<Database>` directly. The migration is
//! incremental: with `enable_real_transactions = true` (the Phase 1+ default),
//! commits run the real Serializable apply/revert path. Set
//! `SPACEKIT_ENABLE_REAL_TRANSACTIONS=false` to keep the stub finalize path.

#![deny(clippy::all)]

use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use crate::change_feed::ChangeFeed;
use crate::database::Database;
use crate::fulltext_search::FullTextIndex;
use crate::idempotency::{DidRateLimiter, IdempotencyCache};
use crate::sandbox::{caller_may_access_sandbox, SandboxAccess, SandboxManager, SandboxStore};
use crate::transaction::{
    IsolationLevel, Transaction, TransactionManager, TransactionModification,
};
use crate::vector_search::VectorIndex;

/// Resolve whether transaction commits use real apply/revert.
///
/// When `SPACEKIT_ENABLE_REAL_TRANSACTIONS` is set, it overrides `config_flag`.
/// When unset, `config_flag` applies (defaults to `true` in [`FacadeConfig::default`]).
pub fn resolve_enable_real_transactions(config_flag: bool) -> bool {
    match std::env::var("SPACEKIT_ENABLE_REAL_TRANSACTIONS").ok() {
        Some(v) if v == "0" || v.eq_ignore_ascii_case("false") => false,
        Some(v) if v == "1" || v.eq_ignore_ascii_case("true") => true,
        Some(_) => config_flag,
        None => config_flag,
    }
}

pub fn default_enable_real_transactions() -> bool {
    true
}

/// Facade configuration. Defaults are sensible for a single-node dev setup.
#[derive(Debug, Clone)]
pub struct FacadeConfig {
    /// Runtime flag for persisted apply/revert on commit. Default true (Phase 1+).
    pub enable_real_transactions: bool,
    /// Default transaction timeout (seconds) when a `BEGIN` doesn't specify.
    pub default_transaction_timeout_seconds: u64,
    /// Idempotency cache capacity.
    pub idempotency_capacity: usize,
    /// Per-DID rate limit defaults (writes/sec).
    pub did_write_per_sec: f64,
    /// Per-DID rate limit burst capacity (writes).
    pub did_write_burst: f64,
    /// Vector index dimension. Set to 0 to skip vector enrolment.
    pub vector_dimension: usize,
    /// When set, sandbox metadata and journals are snapshotted under this directory
    /// (`state/<id>.json`, `boot_epoch.txt`) and stuck `committing` rows reconcile on boot.
    pub sandbox_persistence_root: Option<PathBuf>,
    /// CAS root (`blobs/`, `facts/`, `blob_refs/`). Required for repo-tree apply.
    pub cas_data_dir: Option<PathBuf>,
    /// `/blobs` and `/facts` auth enforcement (env `SPACEKIT_BLOB_FACT_AUTH` overrides default).
    pub blob_fact_auth_mode: crate::access_policy::BlobFactAuthMode,
    /// HMAC material for upload tokens (resolved from env / `cas_data_dir` when unset).
    pub upload_token_secret: Option<Vec<u8>>,
    /// Operator DID for `GET /api/operators/self` (defaults to `SPACEKIT_NODE_DID`).
    pub operator_did: Option<String>,
}

impl Default for FacadeConfig {
    fn default() -> Self {
        Self {
            enable_real_transactions: default_enable_real_transactions(),
            default_transaction_timeout_seconds: 300,
            idempotency_capacity: 4096,
            did_write_per_sec: 1.0,
            did_write_burst: 60.0,
            vector_dimension: 0,
            sandbox_persistence_root: None,
            cas_data_dir: None,
            blob_fact_auth_mode: crate::access_policy::BlobFactAuthMode::from_env(),
            upload_token_secret: None,
            operator_did: None,
        }
    }
}

/// Operator-facing snapshot for `GET /api/agentic/health`.
#[derive(Debug, Clone, Serialize)]
pub struct AgenticHealth {
    pub enable_real_transactions: bool,
    /// Stub finalize path (`enable_real_transactions=false`). **Keep forever:**
    /// should read ~0 after real-apply is default; non-zero ⇒ regression.
    pub tx_commits_stub_finalize_total: u64,
    pub tx_commits_real_apply_ok_total: u64,
    pub tx_commits_real_apply_err_total: u64,
    pub idempotency_cached_hits_total: u64,
    pub idempotency_fresh_proceeds_total: u64,
    /// `cached_hits / (cached_hits + fresh_proceeds)`, or `0.0` if denominator is zero.
    pub idempotency_cache_hit_rate: f64,
    pub did_rate_limit_rejections_total: u64,
    pub did_rate_limit_rejections_last_60s: u64,
    pub change_feed_live_subscribers: usize,
    pub change_feed_dropped_subscribers_total: u64,
    pub change_feed_current_seq: u64,
    pub sandboxes_total: usize,
    pub sandboxes_active: usize,
    pub sandboxes_committing: usize,
    pub sandboxes_committed: usize,
    pub sandboxes_discarded: usize,
    pub sandboxes_expired: usize,
    pub sandboxes_failed: usize,
    pub sandboxes_quota_bytes_written: u64,
    pub sandboxes_quota_vector_ops: u64,
    pub sandboxes_quota_fact_puts: u64,
    /// Whether upload-token minting is configured on this node.
    pub upload_tokens_configured: bool,
    /// Active `/blobs` + `/facts` auth mode (`permissive` | `strict` | `hybrid`).
    pub blob_fact_auth_mode: String,
    pub handoff_signing_configured: bool,
    pub require_handoff_signature: bool,
    pub migration_signing_configured: bool,
}

/// The unified facade. Hands out `Arc`s to subsystems and coordinates
/// transactions across them.
pub struct Facade {
    pub database: Arc<Database>,
    pub vector_index: Arc<VectorIndex>,
    pub fulltext_index: Arc<FullTextIndex>,
    pub transactions: Arc<TransactionManager>,
    pub idempotency: Arc<IdempotencyCache>,
    pub did_rate_limiter: Arc<DidRateLimiter>,
    pub change_feed: Arc<ChangeFeed>,
    pub sandboxes: Arc<SandboxManager>,
    cfg: FacadeConfig,
}

impl Facade {
    /// Build a facade and register cross-module callbacks so the transaction
    /// manager can apply/revert vector and FTS modifications without a typed
    /// dependency on those modules.
    pub async fn new(database: Arc<Database>, mut cfg: FacadeConfig) -> Result<Self> {
        if cfg.upload_token_secret.is_none() {
            cfg.upload_token_secret =
                crate::upload_token::load_signing_secret(cfg.cas_data_dir.as_deref());
        }
        let vector_index = Arc::new(VectorIndex::new(
            cfg.vector_dimension.max(1),
            crate::vector_search::IndexType::BruteForce,
        ));
        let fulltext_index = Arc::new(FullTextIndex::new());

        let mut tx_mgr = TransactionManager::new(
            database.clone(),
            IsolationLevel::Serializable,
            cfg.default_transaction_timeout_seconds,
        );
        tx_mgr.set_real_apply_enabled(cfg.enable_real_transactions);
        let transactions = Arc::new(tx_mgr);

        // Register callbacks. These bridge the typed `VectorIndex` /
        // `FullTextIndex` APIs into the transaction module's untyped variants.
        let vec_for_apply = vector_index.clone();
        let vec_for_revert = vector_index.clone();
        transactions
            .register_vector_callbacks(
                Arc::new(
                    move |index_id: &str, document_id: &str, value: &serde_json::Value| {
                        let vi = vec_for_apply.clone();
                        let index_id = index_id.to_string();
                        let document_id = document_id.to_string();
                        let value = value.clone();
                        // Spawn-and-wait pattern (callback is sync).
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async move {
                                apply_vector(&vi, &index_id, &document_id, &value).await
                            })
                        })
                    },
                ),
                Arc::new(
                    move |index_id: &str,
                          document_id: &str,
                          _new: Option<&serde_json::Value>,
                          old: &Option<serde_json::Value>| {
                        let vi = vec_for_revert.clone();
                        let index_id = index_id.to_string();
                        let document_id = document_id.to_string();
                        let old = old.clone();
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async move {
                                revert_vector(&vi, &index_id, &document_id, &old).await
                            })
                        })
                    },
                ),
            )
            .await;

        let fts_for_apply = fulltext_index.clone();
        let fts_for_revert = fulltext_index.clone();
        transactions
            .register_fts_callbacks(
                Arc::new(
                    move |document_id: &str, table: &str, field: &str, content: &str| {
                        let fts = fts_for_apply.clone();
                        let document_id = document_id.to_string();
                        let table = table.to_string();
                        let field = field.to_string();
                        let content = content.to_string();
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async move {
                                apply_fts(&fts, &document_id, &table, &field, &content).await
                            })
                        })
                    },
                ),
                Arc::new(move |document_id: &str, _old: &Option<serde_json::Value>| {
                    let fts = fts_for_revert.clone();
                    let document_id = document_id.to_string();
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current()
                            .block_on(async move { revert_fts(&fts, &document_id).await })
                    })
                }),
            )
            .await;

        if let Some(cas_dir) = cfg.cas_data_dir.clone() {
            let db_apply = database.clone();
            let db_revert = database.clone();
            let cas_for_apply = cas_dir.clone();
            let cas_for_revert = cas_dir.clone();
            transactions
                .register_repo_callbacks(
                    Arc::new(move |owner, repo, branch, commit, parents| {
                        let db = db_apply.clone();
                        let cas = cas_for_apply.clone();
                        let owner = owner.to_string();
                        let repo = repo.to_string();
                        let branch = branch.to_string();
                        let commit = commit.clone();
                        let parents: Vec<String> = parents.iter().map(|s| s.to_string()).collect();
                        futures::executor::block_on(async move {
                            crate::repo_commit::apply_repo_tree(
                                &cas, &db, &owner, &repo, &branch, commit, &parents,
                            )
                            .await
                        })
                    }),
                    Arc::new(move |owner, repo, branch, old_ref, applied_hex| {
                        let db = db_revert.clone();
                        let cas = cas_for_revert.clone();
                        let owner = owner.to_string();
                        let repo = repo.to_string();
                        let branch = branch.to_string();
                        let applied_hex = applied_hex.to_string();
                        futures::executor::block_on(async move {
                            crate::repo_commit::revert_repo_tree(
                                &cas,
                                &db,
                                &owner,
                                &repo,
                                &branch,
                                old_ref.clone(),
                                &applied_hex,
                            )
                            .await
                        })
                    }),
                )
                .await;
        }

        let idempotency = Arc::new(IdempotencyCache::new(cfg.idempotency_capacity));
        let did_rate_limiter = Arc::new(DidRateLimiter::new(
            cfg.did_write_per_sec,
            cfg.did_write_burst,
        ));
        let change_feed = Arc::new(ChangeFeed::new(2048));
        let sandboxes: Arc<SandboxManager> = match &cfg.sandbox_persistence_root {
            Some(root) => {
                let store = Arc::new(SandboxStore::new(root.clone()));
                let boot = store.init_and_bump_boot_epoch().await?;
                let mgr = Arc::new(SandboxManager::with_disk(
                    change_feed.clone(),
                    store.clone(),
                    boot,
                ));
                mgr.load_from_disk().await?;
                mgr.reconcile_stuck_committing(transactions.clone()).await?;
                mgr
            }
            None => Arc::new(SandboxManager::new(change_feed.clone())),
        };

        info!(
            "Facade initialised (enable_real_transactions={}, sandbox_disk={})",
            cfg.enable_real_transactions,
            cfg.sandbox_persistence_root.is_some(),
        );

        Ok(Self {
            database,
            vector_index,
            fulltext_index,
            transactions,
            idempotency,
            did_rate_limiter,
            change_feed,
            sandboxes,
            cfg,
        })
    }

    pub fn config(&self) -> &FacadeConfig {
        &self.cfg
    }

    /// Create a sandbox, optionally scoped to a workspace (quota + ACL).
    pub async fn create_sandbox(
        &self,
        owner_did: &str,
        caller_did: &str,
        mut cfg: crate::sandbox::SandboxConfig,
        base_snapshot: Option<String>,
        collaborator_dids: Vec<String>,
        workspace_id: Option<String>,
    ) -> Result<crate::sandbox::Sandbox> {
        let ws_id = workspace_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        if let Some(ref wid) = ws_id {
            let ws = self
                .get_workspace(owner_did, wid)
                .await?
                .ok_or_else(|| anyhow::anyhow!("NOTFOUND:workspace {}", wid))?;
            if !crate::workspace::workspace_allows_actor(&ws, caller_did) {
                return Err(anyhow::anyhow!("FORBIDDEN:workspace"));
            }
            if ws.quotas.max_storage_bytes > 0 {
                let used = self.sandboxes.workspace_bytes_in_use(wid).await;
                if used >= ws.quotas.max_storage_bytes {
                    return Err(anyhow::anyhow!(
                        "FORBIDDEN:workspace storage quota exhausted"
                    ));
                }
            }
            crate::workspace::cap_sandbox_config(&mut cfg, &ws.quotas);
        }

        self.sandboxes
            .create(owner_did, cfg, base_snapshot, collaborator_dids, ws_id)
            .await
    }

    /// Begin a new transaction.
    pub async fn begin_transaction(
        &self,
        isolation: Option<IsolationLevel>,
        timeout: Option<u64>,
    ) -> Result<String> {
        self.transactions.begin(isolation, timeout).await
    }

    /// Commit a transaction. Acquires the global commit lock; serializable
    /// semantics on the write path.
    pub async fn commit_transaction(&self, id: &str) -> Result<()> {
        let result = self.transactions.commit(id).await;
        if result.is_ok() {
            self.change_feed
                .publish(crate::change_feed::ChangeEvent {
                    seq: 0, // assigned in publish
                    occurred_at: chrono::Utc::now(),
                    did: None,
                    kind: "tx.committed".to_string(),
                    key: id.to_string(),
                    payload: None,
                })
                .await;
        }
        result
    }

    pub async fn rollback_transaction(&self, id: &str) -> Result<()> {
        let result = self.transactions.rollback(id).await;
        if result.is_ok() {
            self.change_feed
                .publish(crate::change_feed::ChangeEvent {
                    seq: 0,
                    occurred_at: chrono::Utc::now(),
                    did: None,
                    kind: "tx.rolled_back".to_string(),
                    key: id.to_string(),
                    payload: None,
                })
                .await;
        }
        result
    }

    /// Snapshot a transaction (used by `/api/transactions/{id}` and
    /// `/api/transactions/{id}/trace`).
    pub async fn get_transaction(&self, id: &str) -> Option<Transaction> {
        self.transactions.get_transaction(id).await
    }

    /// Append a modification to an open transaction, and optionally mirror it
    /// into a sandbox journal when `sandbox_id` is set (`X-Sandbox-Id`).
    ///
    /// The transaction row is always updated first. If the sandbox append
    /// fails, callers should roll back the transaction to avoid divergence.
    pub async fn record_transaction_modification(
        &self,
        transaction_id: &str,
        modification: TransactionModification,
        policy: crate::sandbox::ConflictPolicy,
        bytes_written: u64,
        sandbox_id: Option<&str>,
        caller_did: Option<&str>,
    ) -> Result<()> {
        let modification = self.enrich_repo_tree_modification(modification)?;
        self.transactions
            .record_modification(transaction_id, modification.clone())
            .await?;

        if let Some(raw) = sandbox_id {
            let sid = raw.trim();
            if sid.is_empty() {
                return Ok(());
            }
            let sb = self
                .sandboxes
                .get(sid)
                .await
                .ok_or_else(|| anyhow::anyhow!("NOTFOUND:sandbox {}", sid))?;
            if !caller_may_access_sandbox(caller_did, &sb, SandboxAccess::RecordJournal) {
                return Err(anyhow::anyhow!("FORBIDDEN:sandbox journal"));
            }
            self.sandboxes
                .record(sid, modification, policy, bytes_written)
                .await?;
        }
        Ok(())
    }

    fn enrich_repo_tree_modification(
        &self,
        modification: TransactionModification,
    ) -> Result<TransactionModification> {
        if let TransactionModification::RepoTree {
            owner_did,
            repo_name,
            branch,
            commit,
            parent_fact_ids,
            old_ref,
            applied_fact_id_hex,
        } = modification
        {
            let collection = crate::repo_commit::ref_collection(&repo_name);
            let ref_id = crate::repo_commit::ref_document_id(&branch);
            let snap = if old_ref.is_none() {
                self.database
                    .get_document(&owner_did, &collection, &ref_id)
                    .ok()
                    .flatten()
            } else {
                old_ref
            };
            return Ok(TransactionModification::RepoTree {
                owner_did,
                repo_name,
                branch,
                commit,
                parent_fact_ids,
                old_ref: snap,
                applied_fact_id_hex,
            });
        }
        Ok(modification)
    }

    /// Create a `spacekit:workspace:v1` fact and index row.
    pub async fn create_workspace(
        &self,
        content: crate::workspace::WorkspaceContent,
    ) -> Result<String> {
        let cas = self
            .cfg
            .cas_data_dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("cas_data_dir not configured"))?;
        let pkg = crate::workspace::build_workspace_fact_package(content)?;
        let id = crate::repo_commit::persist_fact_package(cas, &self.database, &pkg).await?;
        let ws = crate::workspace::parse_workspace_from_fact(&pkg)?;
        let now = chrono::Utc::now();
        let index = crate::database::DocumentRecord {
            owner_did: ws.owner_did.clone(),
            collection: "workspace_index".to_string(),
            id: ws.workspace_id.clone(),
            data: serde_json::to_value(&ws)?,
            created_at: now,
            updated_at: now,
            blob_ref: None,
        };
        self.database.upsert_document(&index)?;
        Ok(id)
    }

    /// Update an existing workspace fact + index (preserves `created_at`).
    pub async fn update_workspace(
        &self,
        content: crate::workspace::WorkspaceContent,
    ) -> Result<String> {
        let existing = self
            .get_workspace(&content.owner_did, &content.workspace_id)
            .await?;
        let Some(existing) = existing else {
            return Err(anyhow::anyhow!("workspace not found"));
        };
        let mut content = content;
        content.created_at = existing.created_at;
        let cas = self
            .cfg
            .cas_data_dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("cas_data_dir not configured"))?;
        let pkg = crate::workspace::build_workspace_fact_package(content.clone())?;
        crate::workspace::upsert_workspace_fact(cas, &self.database, &pkg, &content).await
    }

    pub async fn get_workspace(
        &self,
        owner_did: &str,
        workspace_id: &str,
    ) -> Result<Option<crate::workspace::WorkspaceContent>> {
        if let Some(doc) = self
            .database
            .get_document(owner_did, "workspace_index", workspace_id)
            .ok()
            .flatten()
        {
            return Ok(serde_json::from_value(doc.data).ok());
        }
        let cas = match &self.cfg.cas_data_dir {
            Some(d) => d,
            None => return Ok(None),
        };
        let fact_id = hex::encode(crate::workspace::workspace_fact_id(owner_did, workspace_id));
        let path = crate::repo_commit::fact_path(cas, &fact_id);
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read(&path).await?;
        let pkg: spacekit_primitives::v1::fact::FactPackage = serde_json::from_slice(&raw)?;
        Ok(crate::workspace::parse_workspace_from_fact(&pkg).ok())
    }

    pub async fn list_workspaces_for_owner(
        &self,
        owner_did: &str,
    ) -> Result<Vec<crate::workspace::WorkspaceContent>> {
        let docs = self
            .database
            .list_documents(owner_did, "workspace_index")
            .unwrap_or_default();
        let mut out = Vec::new();
        for doc in docs {
            if let Ok(ws) = serde_json::from_value::<crate::workspace::WorkspaceContent>(doc.data) {
                out.push(ws);
            }
        }
        Ok(out)
    }

    /// CAS root when configured (`blobs/`, `facts/`, upload-token secret file).
    pub fn cas_data_dir(&self) -> Option<&std::path::Path> {
        self.cfg.cas_data_dir.as_deref()
    }

    /// HMAC secret for upload-token mint/verify (configured at facade init).
    pub fn upload_signing_secret(&self) -> Option<&[u8]> {
        self.cfg.upload_token_secret.as_deref()
    }

    /// Aggregated counters for agent-fleet operators (`GET /api/agentic/health`).
    pub async fn agentic_health(&self) -> AgenticHealth {
        let (stub, real_ok, real_err) = self.transactions.commit_path_totals();
        let (hits, fresh) = self.idempotency.idempotency_totals();
        let denom = hits.saturating_add(fresh);
        let hit_rate = if denom == 0 {
            0.0
        } else {
            hits as f64 / denom as f64
        };
        let (
            sandboxes_total,
            sandboxes_active,
            sandboxes_committing,
            sandboxes_committed,
            sandboxes_discarded,
            sandboxes_expired,
            sandboxes_failed,
            q,
        ) = self.sandboxes.health_aggregation().await;
        AgenticHealth {
            enable_real_transactions: self.cfg.enable_real_transactions,
            tx_commits_stub_finalize_total: stub,
            tx_commits_real_apply_ok_total: real_ok,
            tx_commits_real_apply_err_total: real_err,
            idempotency_cached_hits_total: hits,
            idempotency_fresh_proceeds_total: fresh,
            idempotency_cache_hit_rate: hit_rate,
            did_rate_limit_rejections_total: self.did_rate_limiter.rate_limit_rejections_total(),
            did_rate_limit_rejections_last_60s: self
                .did_rate_limiter
                .rate_limit_rejections_in_window(60),
            change_feed_live_subscribers: self.change_feed.live_subscriber_count().await,
            change_feed_dropped_subscribers_total: self.change_feed.dropped_subscribers_total(),
            change_feed_current_seq: self.change_feed.current_seq(),
            sandboxes_total,
            sandboxes_active,
            sandboxes_committing,
            sandboxes_committed,
            sandboxes_discarded,
            sandboxes_expired,
            sandboxes_failed,
            sandboxes_quota_bytes_written: q.bytes_written,
            sandboxes_quota_vector_ops: q.vector_ops,
            sandboxes_quota_fact_puts: q.fact_puts,
            upload_tokens_configured: self.upload_signing_secret().is_some(),
            blob_fact_auth_mode: self.cfg.blob_fact_auth_mode.as_str().to_string(),
            handoff_signing_configured: crate::handoff::load_handoff_secret(self.cas_data_dir())
                .is_some(),
            require_handoff_signature: crate::handoff::handoff_signature_required(),
            migration_signing_configured: self
                .cas_data_dir()
                .and_then(|d| crate::migration::load_operator_keypair(Some(d)))
                .is_some(),
        }
    }

    async fn lookup_migration_signer_pubkey(
        &self,
        role: &str,
        signer_did: &str,
        remote_fetch_base: Option<&str>,
    ) -> Option<Vec<u8>> {
        if role == "workspace_owner" {
            let cas = self.cfg.cas_data_dir.as_ref()?;
            if let Some(kp) = crate::migration::load_migration_signer_keypair(cas, signer_did) {
                return Some(kp.public_key);
            }
            if let Ok(hex_pk) = std::env::var("SPACEKIT_MIGRATION_OWNER_PUBKEY_HEX") {
                if !hex_pk.trim().is_empty() {
                    return hex::decode(hex_pk.trim()).ok();
                }
            }
            return None;
        }
        if role != "source_operator" && role != "destination_operator" {
            return None;
        }
        if self.cfg.operator_did.as_deref() == Some(signer_did) && role == "destination_operator" {
            if let Some(cas) = self.cas_data_dir() {
                if let Some(kp) = crate::migration::load_operator_keypair(Some(cas)) {
                    return Some(kp.public_key);
                }
            }
        }
        let cas = self.cfg.cas_data_dir.as_ref()?;
        if let Ok(Some(manifest)) =
            crate::operator_manifest::load_published_operator_manifest(cas, signer_did).await
        {
            if let Some(pk) = manifest
                .sphincs_public_key_hex
                .as_ref()
                .and_then(|h| hex::decode(h.trim()).ok())
            {
                return Some(pk);
            }
        }
        let base = remote_fetch_base?;
        #[cfg(feature = "api-server")]
        {
            let url = format!("{}/api/operators/self", base.trim_end_matches('/'));
            if let Ok(resp) = reqwest::Client::new().get(&url).send().await {
                if resp.status().is_success() {
                    if let Ok(body) = resp
                        .json::<crate::operator_manifest::OperatorSelfResponse>()
                        .await
                    {
                        if body.operator_did == signer_did {
                            return body
                                .manifest
                                .sphincs_public_key_hex
                                .as_ref()
                                .and_then(|h| hex::decode(h.trim()).ok());
                        }
                    }
                }
            }
        }
        None
    }

    /// Federation discovery: published manifest fact or runtime synthesis.
    pub async fn operator_self(
        &self,
        storage_http_url: String,
    ) -> Result<crate::operator_manifest::OperatorSelfResponse> {
        let operator_did = self
            .cfg
            .operator_did
            .clone()
            .or_else(|| std::env::var("SPACEKIT_NODE_DID").ok())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("operator_did not configured"))?;
        let health = self.agentic_health().await;
        let cas = self
            .cfg
            .cas_data_dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("cas_data_dir not configured"))?;
        if let Some(manifest) =
            crate::operator_manifest::load_published_operator_manifest(cas, &operator_did).await?
        {
            return Ok(crate::operator_manifest::OperatorSelfResponse {
                schema: crate::operator_manifest::SCHEMA_OPERATOR_SELF_V1.to_string(),
                fact_id: Some(hex::encode(crate::operator_manifest::operator_fact_id(
                    &operator_did,
                ))),
                operator_did: operator_did.clone(),
                manifest,
                manifest_source: "published_fact".to_string(),
            });
        }
        let sphincs_hex = self
            .cas_data_dir()
            .and_then(|d| crate::migration::load_operator_keypair(Some(d)))
            .map(|kp| hex::encode(&kp.public_key));
        let manifest = crate::operator_manifest::synthetic_operator_manifest(
            &operator_did,
            storage_http_url,
            &health.blob_fact_auth_mode,
            health.upload_tokens_configured,
            health.handoff_signing_configured,
            health.migration_signing_configured,
            sphincs_hex,
        );
        Ok(crate::operator_manifest::OperatorSelfResponse {
            schema: crate::operator_manifest::SCHEMA_OPERATOR_SELF_V1.to_string(),
            operator_did,
            manifest,
            manifest_source: "runtime".to_string(),
            fact_id: None,
        })
    }

    /// Import a workspace export bundle onto this node (federation destination).
    pub async fn import_workspace(
        &self,
        caller_did: &str,
        bundle: crate::workspace::WorkspaceExportBundle,
        conflict: crate::workspace::WorkspaceImportConflict,
        owner_override: Option<String>,
        replicate_blobs_from: Option<&str>,
        replicate_source_auth: Option<&str>,
    ) -> Result<crate::workspace::WorkspaceImportResult> {
        if bundle.schema != crate::workspace::SCHEMA_WORKSPACE_V1 {
            return Err(anyhow::anyhow!("unsupported workspace schema"));
        }
        let handoff_secret = crate::handoff::load_handoff_secret(self.cas_data_dir());
        crate::handoff::validate_import_bundle(handoff_secret.as_deref(), &bundle)?;
        let mut migration_manifest = bundle.migration_manifest.clone();
        if let Some(ref mig) = migration_manifest {
            let remote_base = mig.source_operator_url.as_str();
            for entry in &mig.did_signatures {
                let pk = self
                    .lookup_migration_signer_pubkey(
                        &entry.signer_role,
                        &entry.signer_did,
                        Some(remote_base),
                    )
                    .await
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "no migration verification key for {} ({})",
                            entry.signer_did,
                            entry.signer_role
                        )
                    })?;
                if !crate::migration::verify_signature_entry(mig, entry, &pk)? {
                    return Err(anyhow::anyhow!(
                        "invalid migration DID signature for {}",
                        entry.signer_did
                    ));
                }
            }
            if mig.schema_version == crate::migration::SCHEMA_VERSION_V2
                || crate::migration::migration_requires_did_signatures()
            {
                if mig.did_signatures.is_empty() {
                    return Err(anyhow::anyhow!("migration v2 requires did_signatures"));
                }
                let scenario = migration_import_scenario();
                if !crate::migration::has_required_signers_at_import(mig, scenario) {
                    return Err(anyhow::anyhow!(
                        "migration missing required inbound signatures for {:?}",
                        scenario
                    ));
                }
            }
        }
        let owner = owner_override.unwrap_or_else(|| bundle.owner_did.clone());
        if caller_did != owner {
            return Err(anyhow::anyhow!(
                "FORBIDDEN: caller DID must match destination owner_did"
            ));
        }
        let cas = self
            .cfg
            .cas_data_dir
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("cas_data_dir not configured"))?;
        let workspace_id = bundle.workspace_id.clone();
        let exists = self.get_workspace(&owner, &workspace_id).await?.is_some();
        if exists && conflict == crate::workspace::WorkspaceImportConflict::Reject {
            return Err(anyhow::anyhow!("CONFLICT: workspace already exists"));
        }
        let now = chrono::Utc::now().timestamp() as u64;
        let mut content = bundle.content;
        content.owner_did = owner.clone();
        content.workspace_id = workspace_id.clone();
        content.updated_at = now;
        if !exists {
            content.created_at = now;
        }
        let pkg = crate::workspace::build_workspace_fact_package(content.clone())?;
        let fact_id =
            crate::workspace::upsert_workspace_fact(cas, &self.database, &pkg, &content).await?;

        let migration_record_fact_id = if let Some(mut mig) = migration_manifest.take() {
            let now = chrono::Utc::now().timestamp() as u64;
            if let (Some(op_did), Some(kp)) = (
                self.cfg.operator_did.clone(),
                crate::migration::load_operator_keypair(Some(cas)),
            ) {
                let already = mig
                    .did_signatures
                    .iter()
                    .any(|s| s.signer_role == "destination_operator");
                mig.destination_operator_url = std::env::var("SPACEKIT_PUBLIC_HTTP_URL")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| s.trim_end_matches('/').to_string());
                if !already {
                    crate::migration::sign_manifest_role(
                        &mut mig,
                        "destination_operator",
                        &op_did,
                        &kp,
                        now,
                    )?;
                }
            }
            let scenario = migration_import_scenario();
            if (mig.schema_version == crate::migration::SCHEMA_VERSION_V2
                || crate::migration::migration_requires_did_signatures())
                && !crate::migration::has_required_signers(&mig, scenario)
            {
                return Err(anyhow::anyhow!(
                    "migration missing required signatures after import for {:?}",
                    scenario
                ));
            }
            let author = self.cfg.operator_did.as_deref().unwrap_or(&owner);
            crate::migration::persist_migration_record(cas, &mig, author)
                .await
                .ok()
        } else {
            None
        };

        #[cfg(feature = "api-server")]
        let blob_replication = if let Some(source_url) = replicate_blobs_from {
            if bundle.referenced_blob_hashes.is_empty() {
                None
            } else {
                Some(
                    crate::federation::replicate_blobs_from_source(
                        cas,
                        source_url,
                        &bundle.referenced_blob_hashes,
                        replicate_source_auth,
                    )
                    .await?,
                )
            }
        } else {
            None
        };
        #[cfg(not(feature = "api-server"))]
        let blob_replication: Option<crate::federation::BlobReplicateReport> = None;

        Ok(crate::workspace::WorkspaceImportResult {
            fact_id,
            workspace_id,
            owner_did: owner,
            created: !exists,
            replaced: exists,
            blob_replication,
            migration_record_fact_id,
        })
    }

    /// Federation handoff bundle: workspace index row + deterministic fact id.
    pub async fn export_workspace(
        &self,
        owner_did: &str,
        workspace_id: &str,
    ) -> Result<Option<crate::workspace::WorkspaceExportBundle>> {
        let content = match self.get_workspace(owner_did, workspace_id).await? {
            Some(c) => c,
            None => return Ok(None),
        };
        let fact_id = hex::encode(crate::workspace::workspace_fact_id(owner_did, workspace_id));
        let referenced_blob_hashes = self
            .cfg
            .cas_data_dir
            .as_ref()
            .map(|cas| {
                crate::federation::collect_workspace_blob_hashes(
                    cas,
                    &self.database,
                    owner_did,
                    &content,
                )
                .into_iter()
                .collect()
            })
            .unwrap_or_default();
        let mut bundle = crate::workspace::WorkspaceExportBundle {
            schema: crate::workspace::SCHEMA_WORKSPACE_V1.to_string(),
            fact_id,
            owner_did: owner_did.to_string(),
            workspace_id: workspace_id.to_string(),
            content,
            exported_at: chrono::Utc::now().timestamp() as u64,
            referenced_blob_hashes,
            handoff_signature: None,
            migration_manifest: None,
        };
        if let Some(secret) = crate::handoff::load_handoff_secret(self.cas_data_dir()) {
            crate::handoff::sign_export_bundle(&secret, &mut bundle)?;
        }
        let source_url = std::env::var("SPACEKIT_PUBLIC_HTTP_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:3030".to_string());
        if let Some(op_did) = self.cfg.operator_did.clone() {
            let mut mig = crate::migration::build_manifest_from_export(
                &bundle,
                source_url.trim_end_matches('/'),
                &op_did,
                None,
                7 * 24 * 3600,
            )?;
            let dest_url = std::env::var("SPACEKIT_MIGRATION_DEST_URL")
                .ok()
                .filter(|s| !s.trim().is_empty());
            if let Some(ref dest) = dest_url {
                mig.destination_operator_url = Some(dest.trim_end_matches('/').to_string());
            }
            let local_versions = self
                .cas_data_dir()
                .and_then(|d| crate::migration::load_operator_keypair(Some(d)))
                .map(|_| vec!["v1".to_string(), "v2".to_string()])
                .unwrap_or_else(|| vec!["v1".to_string()]);
            let remote_versions = if let Some(ref dest) = dest_url {
                crate::migration::fetch_remote_migration_versions(dest)
                    .await
                    .unwrap_or_else(|_| vec!["v1".to_string()])
            } else {
                local_versions.clone()
            };
            let negotiated =
                crate::migration::negotiated_migration_version(&local_versions, &remote_versions);
            crate::migration::apply_negotiated_schema_version(&mut mig, negotiated);
            if negotiated == crate::migration::SCHEMA_VERSION_V2 {
                if let Some(kp) = crate::migration::load_operator_keypair(self.cas_data_dir()) {
                    crate::migration::sign_manifest_role(
                        &mut mig,
                        "source_operator",
                        &op_did,
                        &kp,
                        bundle.exported_at,
                    )?;
                }
            }
            bundle.migration_manifest = Some(mig);
        }
        Ok(Some(bundle))
    }
}

fn migration_import_scenario() -> crate::migration::MigrationScenario {
    let scenario = std::env::var("SPACEKIT_MIGRATION_SCENARIO")
        .ok()
        .map(|s| s.trim().to_lowercase());
    match scenario.as_deref() {
        Some("user" | "user_initiated" | "owner") => {
            crate::migration::MigrationScenario::UserInitiated
        }
        Some("bilateral") => crate::migration::MigrationScenario::Bilateral,
        _ => crate::migration::MigrationScenario::OperatorInitiated,
    }
}

async fn apply_vector(
    vi: &VectorIndex,
    _index_id: &str,
    document_id: &str,
    value: &serde_json::Value,
) -> Result<()> {
    if value.is_null() {
        // Sentinel: remove.
        return vi.remove_embedding(document_id).await;
    }
    let embedding: crate::vector_search::VectorEmbedding = serde_json::from_value(value.clone())
        .map_err(|e| anyhow::anyhow!("vector embedding decode: {e}"))?;
    vi.add_embedding(embedding).await
}

async fn revert_vector(
    vi: &VectorIndex,
    _index_id: &str,
    document_id: &str,
    old: &Option<serde_json::Value>,
) -> Result<()> {
    match old {
        Some(prior) if !prior.is_null() => {
            let embedding: crate::vector_search::VectorEmbedding =
                serde_json::from_value(prior.clone())
                    .map_err(|e| anyhow::anyhow!("vector embedding decode: {e}"))?;
            vi.add_embedding(embedding).await
        }
        _ => vi.remove_embedding(document_id).await,
    }
}

async fn apply_fts(
    fts: &FullTextIndex,
    document_id: &str,
    table: &str,
    field: &str,
    content: &str,
) -> Result<()> {
    if table.is_empty() && field.is_empty() && content.is_empty() {
        // Sentinel: remove.
        return fts.remove_document(document_id).await;
    }
    fts.index_document(
        document_id.to_string(),
        table.to_string(),
        field.to_string(),
        content.to_string(),
    )
    .await
}

async fn revert_fts(fts: &FullTextIndex, document_id: &str) -> Result<()> {
    fts.remove_document(document_id).await
}
