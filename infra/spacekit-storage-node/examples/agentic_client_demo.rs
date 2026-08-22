//! Downstream client demo for the agentic-readiness API surface.
//!
//! Demonstrates the patterns every Phase 3 client should implement:
//!
//! - `Authorization: DID <did>` so per-DID rate limiting and per-DID
//!   idempotency caches key correctly.
//! - `Idempotency-Key: <ulid|uuid>` on every write so retries don't
//!   duplicate work. Storage Node returns the cached response for repeat
//!   keys with the same body fingerprint, `422 Unprocessable Entity` for
//!   the same key with a different body, and blocks (up to 30s by default)
//!   on in-flight identical requests.
//! - `X-Sandbox-Id: <id>` to scope writes into an ephemeral sandbox so the
//!   agent can `commit` or `discard` cleanly.
//! - `POST /api/transactions/{id}/modifications` with optional `X-Sandbox-Id`
//!   to append the same `TransactionModification` to the open transaction **and**
//!   mirror it into the sandbox journal (same ACL as extend).
//! - Workspace → sandbox (`workspace_id`) → `RepoTree` → sandbox commit (Phase 1 loop).
//!
//! Run against a Storage Node started with the agentic facade enabled:
//!
//! ```bash
//! cargo run -p spacekit-storage-node --example agentic_client_demo --features standalone -- \
//!   http://localhost:3030 did:spacekit:agent:demo
//! ```

use anyhow::Result;
use reqwest::Client;
use serde_json::json;
use uuid::Uuid;

const DEFAULT_BASE_URL: &str = "http://localhost:3030";
const DEFAULT_DID: &str = "did:spacekit:agent:demo";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| DEFAULT_BASE_URL.into());
    let did = args.next().unwrap_or_else(|| DEFAULT_DID.into());

    let client = AgenticClient::new(base, did);

    let health = client.agentic_health().await?;
    println!(
        "AGENTIC HEALTH → real_apply={}",
        health["enable_real_transactions"]
    );

    let workspace_id = format!("demo-{}", Uuid::new_v4().simple());
    let ws = client
        .create_workspace(
            &workspace_id,
            json!({
                "collaborators": [],
                "associated_repos": ["demo-repo"],
                "quotas": { "max_sandbox_bytes": 32 * 1024 * 1024, "max_storage_bytes": 128 * 1024 * 1024 }
            }),
        )
        .await?;
    println!("WORKSPACE → {ws}");

    let sb = client
        .create_sandbox(json!({
            "workspace_id": workspace_id,
            "ttl_seconds": 600,
            "max_bytes_written": 64 * 1024 * 1024
        }))
        .await?;
    let sandbox_id = sb["id"].as_str().unwrap().to_string();
    println!(
        "SANDBOX → id = {sandbox_id}, workspace_id = {}, expires_at = {}",
        sb["workspace_id"].as_str().unwrap_or("—"),
        sb["expires_at"]
    );

    let begin = client
        .begin_transaction(Some("serializable"), Some(60))
        .await?;
    let tx_id = begin["transaction_id"].as_str().unwrap();
    println!("BEGIN → transaction_id = {tx_id}");

    let blob_hash = "2222222222222222222222222222222222222222222222222222222222222222";
    let repo_mod = json!({
        "modification": {
            "RepoTree": {
                "owner_did": client.did,
                "repo_name": "demo-repo",
                "branch": "main",
                "commit": {
                    "schema": "spacekit:repo:commit:v1",
                    "tree": { "README.md": blob_hash },
                    "message": "agentic demo repo commit",
                    "author_name": "Agentic Demo",
                    "timestamp": chrono::Utc::now().timestamp() as u64
                },
                "parent_fact_ids": []
            }
        },
        "conflict_policy": "three_way_merge",
        "bytes_written": 128u64
    });
    let recorded = client
        .record_transaction_modification(tx_id, Some(&sandbox_id), repo_mod)
        .await?;
    println!("RECORD RepoTree (tx + sandbox journal) → {recorded}");

    let journal = client.get_sandbox_journal(&sandbox_id).await?;
    let jlen = journal["journal"].as_array().map(|a| a.len()).unwrap_or(0);
    println!("JOURNAL → {jlen} entries");

    let rolled = client.rollback_transaction(tx_id).await?;
    println!("ROLLBACK TX (journal kept for sandbox commit) → {rolled}");

    let dry = client.commit_sandbox(&sandbox_id, true).await?;
    println!(
        "DRY RUN → applied = {}, dry_run = {}",
        dry["applied"], dry["dry_run"]
    );

    let final_commit = client.commit_sandbox(&sandbox_id, false).await?;
    println!("COMMIT SANDBOX → applied = {}", final_commit["applied"]);

    Ok(())
}

struct AgenticClient {
    base: String,
    did: String,
    http: Client,
}

impl AgenticClient {
    fn new(base: String, did: String) -> Self {
        Self {
            base,
            did,
            http: Client::new(),
        }
    }

    fn fresh_key() -> String {
        Uuid::new_v4().to_string()
    }

    fn auth_header(&self) -> (&'static str, String) {
        ("Authorization", format!("DID {}", self.did))
    }

    async fn agentic_health(&self) -> Result<serde_json::Value> {
        let resp = self
            .http
            .get(format!("{}/api/agentic/health", self.base))
            .send()
            .await?;
        Ok(resp.json().await?)
    }

    async fn create_workspace(
        &self,
        workspace_id: &str,
        extra: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let (auth_k, auth_v) = self.auth_header();
        let body = json!({
            "workspace_id": workspace_id,
            "collaborators": extra.get("collaborators").cloned().unwrap_or(json!([])),
            "associated_repos": extra.get("associated_repos").cloned().unwrap_or(json!([])),
            "quotas": extra.get("quotas").cloned(),
        });
        let resp = self
            .http
            .post(format!("{}/api/workspaces", self.base))
            .header(auth_k, auth_v)
            .header("Idempotency-Key", Self::fresh_key())
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let val = resp.json::<serde_json::Value>().await?;
        if !status.is_success() {
            anyhow::bail!("create workspace HTTP {}: {}", status, val);
        }
        Ok(val)
    }

    async fn begin_transaction(
        &self,
        isolation: Option<&str>,
        timeout_seconds: Option<u64>,
    ) -> Result<serde_json::Value> {
        let body = json!({
            "isolation": isolation,
            "timeout_seconds": timeout_seconds,
        });
        let (auth_k, auth_v) = self.auth_header();
        let resp = self
            .http
            .post(format!("{}/api/transactions", self.base))
            .header(auth_k, auth_v)
            .header("Idempotency-Key", Self::fresh_key())
            .json(&body)
            .send()
            .await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }

    async fn rollback_transaction(&self, transaction_id: &str) -> Result<serde_json::Value> {
        let (auth_k, auth_v) = self.auth_header();
        let resp = self
            .http
            .post(format!(
                "{}/api/transactions/{}/rollback",
                self.base, transaction_id
            ))
            .header(auth_k, auth_v)
            .send()
            .await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }

    async fn record_transaction_modification(
        &self,
        transaction_id: &str,
        sandbox_id: Option<&str>,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let (auth_k, auth_v) = self.auth_header();
        let mut req = self
            .http
            .post(format!(
                "{}/api/transactions/{}/modifications",
                self.base, transaction_id
            ))
            .header(auth_k, auth_v)
            .header("Idempotency-Key", Self::fresh_key())
            .json(&body);
        if let Some(sid) = sandbox_id {
            if !sid.is_empty() {
                req = req.header("X-Sandbox-Id", sid);
            }
        }
        let resp = req.send().await?;
        let status = resp.status();
        let val = resp.json::<serde_json::Value>().await?;
        if !status.is_success() {
            anyhow::bail!("record modification HTTP {}: {}", status, val);
        }
        Ok(val)
    }

    async fn get_sandbox_journal(&self, sandbox_id: &str) -> Result<serde_json::Value> {
        let (auth_k, auth_v) = self.auth_header();
        let resp = self
            .http
            .get(format!(
                "{}/api/sandboxes/{}/journal",
                self.base, sandbox_id
            ))
            .header(auth_k, auth_v)
            .send()
            .await?;
        let status = resp.status();
        let val = resp.json::<serde_json::Value>().await?;
        if !status.is_success() {
            anyhow::bail!("get journal HTTP {}: {}", status, val);
        }
        Ok(val)
    }

    async fn create_sandbox(&self, body: serde_json::Value) -> Result<serde_json::Value> {
        let (auth_k, auth_v) = self.auth_header();
        let resp = self
            .http
            .post(format!("{}/api/sandboxes", self.base))
            .header(auth_k, auth_v)
            .header("Idempotency-Key", Self::fresh_key())
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let val = resp.json::<serde_json::Value>().await?;
        if !status.is_success() {
            anyhow::bail!("create sandbox HTTP {}: {}", status, val);
        }
        Ok(val)
    }

    async fn commit_sandbox(&self, sandbox_id: &str, dry_run: bool) -> Result<serde_json::Value> {
        let (auth_k, auth_v) = self.auth_header();
        let resp = self
            .http
            .post(format!(
                "{}/api/sandboxes/{}/commit?dry_run={}",
                self.base, sandbox_id, dry_run
            ))
            .header(auth_k, auth_v)
            .header("Idempotency-Key", Self::fresh_key())
            .send()
            .await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }
}
