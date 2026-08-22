//! Live HTTP soak for `SPACEKIT_BLOB_FACT_AUTH=hybrid` (Stream A staging).
//!
//! Run against a node already in hybrid mode with upload tokens configured:
//!
//! ```bash
//! export SPACEKIT_BLOB_FACT_AUTH=hybrid
//! export SPACEKIT_UPLOAD_TOKEN_SECRET="$(openssl rand -hex 32)"
//! spacekit network down && spacekit network up
//!
//! cargo run -p spacekit-storage-node --example hybrid_auth_soak --features standalone -- \
//!   http://127.0.0.1:3030 did:spacekit:testnet:0x1aa6b39a086e67
//! ```

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::json;
use spacekit_primitives::v1::fact::AccessPolicy;
use spacekit_storage_node::workspace::{
    build_workspace_fact_package, WorkspaceContent, WorkspaceStatus,
};

const DEFAULT_BASE: &str = "http://127.0.0.1:3030";
const DEFAULT_DID: &str = "did:spacekit:hybrid:soak";

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| DEFAULT_BASE.into());
    let did = args.next().unwrap_or_else(|| DEFAULT_DID.into());
    let other_did = "did:spacekit:hybrid:other";

    let http = Client::new();
    let mut failed = 0u32;

    let health: serde_json::Value = http
        .get(format!("{base}/api/agentic/health"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mode = health["blob_fact_auth_mode"].as_str().unwrap_or("?");
    let tokens_ok = health["upload_tokens_configured"]
        .as_bool()
        .unwrap_or(false);
    println!("health → blob_fact_auth_mode={mode} upload_tokens_configured={tokens_ok}");

    if mode != "hybrid" {
        eprintln!(
            "FAIL: expected blob_fact_auth_mode=hybrid (got {mode}). \
             Set SPACEKIT_BLOB_FACT_AUTH=hybrid or [runtime] blob_fact_auth in network.toml, then restart."
        );
        failed += 1;
    } else {
        println!("PASS: hybrid mode active");
    }

    let metrics = http
        .get(format!("{base}/api/agentic/metrics"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if metrics.contains("spacekit_blob_fact_auth_mode{mode=\"hybrid\"}") {
        println!("PASS: Prometheus hybrid mode gauge");
    } else {
        eprintln!("FAIL: metrics missing spacekit_blob_fact_auth_mode{{mode=\"hybrid\"}}");
        failed += 1;
    }

    let payload = format!("hybrid-soak-{}", chrono::Utc::now().timestamp());
    let hash = hex::encode(blake3::hash(payload.as_bytes()).as_bytes());

    // Blob write without auth → 401
    let r = http
        .put(format!("{base}/blobs/{hash}"))
        .body(payload.clone())
        .send()
        .await?;
    if r.status() == reqwest::StatusCode::UNAUTHORIZED {
        println!("PASS: PUT /blobs without auth → 401");
    } else {
        eprintln!("FAIL: PUT /blobs without auth → {}", r.status());
        failed += 1;
    }

    // Blob write with DID → 201 or 200 (exists)
    let r = http
        .put(format!("{base}/blobs/{hash}"))
        .header("Authorization", format!("DID {did}"))
        .body(payload.clone())
        .send()
        .await?;
    if r.status().is_success() {
        println!("PASS: PUT /blobs with DID → {}", r.status());
    } else {
        eprintln!(
            "FAIL: PUT /blobs with DID → {} {:?}",
            r.status(),
            r.text().await.ok()
        );
        failed += 1;
    }

    // Blob read without auth → 200 (hybrid keeps GET open)
    let r = http.get(format!("{base}/blobs/{hash}")).send().await?;
    if r.status().is_success() {
        println!("PASS: GET /blobs without auth → {}", r.status());
    } else {
        eprintln!("FAIL: GET /blobs without auth → {}", r.status());
        failed += 1;
    }

    if tokens_ok {
        let token_body = b"hybrid-soak-token-path";
        let hash2 = hex::encode(blake3::hash(token_body).as_bytes());
        let mint: serde_json::Value = http
            .post(format!("{base}/api/upload-tokens"))
            .header("Authorization", format!("DID {did}"))
            .json(&json!({
                "operation": "put_blob",
                "resource": hash2,
                "ttl_seconds": 120
            }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let token = mint["token"]
            .as_str()
            .context("mint response missing token")?;
        let r = http
            .put(format!("{base}/blobs/{hash2}"))
            .header("Authorization", format!("UploadToken {token}"))
            .body(token_body.to_vec())
            .send()
            .await?;
        if r.status().is_success() {
            println!("PASS: PUT /blobs with UploadToken → {}", r.status());
        } else {
            eprintln!("FAIL: PUT /blobs with UploadToken → {}", r.status());
            failed += 1;
        }
    } else {
        eprintln!("SKIP: upload token mint (upload_tokens_configured=false)");
    }

    let ws_id = format!("hybrid-soak-{}", chrono::Utc::now().timestamp());
    let fact_pkg = build_workspace_fact_package(WorkspaceContent {
        workspace_id: ws_id.clone(),
        owner_did: did.clone(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })?;

    let r = http
        .post(format!("{base}/facts"))
        .json(&fact_pkg)
        .send()
        .await?;
    if r.status() == reqwest::StatusCode::UNAUTHORIZED {
        println!("PASS: POST /facts without auth → 401");
    } else {
        eprintln!("FAIL: POST /facts without auth → {}", r.status());
        failed += 1;
    }

    let r = http
        .post(format!("{base}/facts"))
        .header("Authorization", format!("DID {other_did}"))
        .json(&fact_pkg)
        .send()
        .await?;
    if r.status() == reqwest::StatusCode::FORBIDDEN {
        println!("PASS: POST /facts author mismatch → 403");
    } else {
        eprintln!("FAIL: POST /facts author mismatch → {}", r.status());
        failed += 1;
    }

    let r = http
        .post(format!("{base}/facts"))
        .header("Authorization", format!("DID {did}"))
        .json(&fact_pkg)
        .send()
        .await?;
    if r.status().is_success() {
        println!("PASS: POST /facts with matching DID → {}", r.status());
    } else {
        eprintln!(
            "FAIL: POST /facts with matching DID → {} {:?}",
            r.status(),
            r.text().await.ok()
        );
        failed += 1;
    }

    if failed > 0 {
        bail!("hybrid auth soak: {failed} check(s) failed");
    }
    println!("\nhybrid auth soak: all checks passed");
    Ok(())
}
