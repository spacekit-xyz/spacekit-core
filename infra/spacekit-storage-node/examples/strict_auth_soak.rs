//! Live HTTP soak for `SPACEKIT_BLOB_FACT_AUTH=strict` (Stream A cutover).
//!
//! Run after hybrid soak passes. Requires upload tokens and quantum-enabled node
//! (default `standalone` build).
//!
//! ```bash
//! export SPACEKIT_BLOB_FACT_AUTH=strict
//! spacekit network down && spacekit network up
//!
//! cargo run -p spacekit-storage-node --example strict_auth_soak --features standalone -- \
//!   http://127.0.0.1:3030 did:spacekit:testnet:YOUR_DID
//! ```

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::json;
use spacekit_primitives::v1::crypto::quantum::{generate_sphincs_keypair, sign_sphincs_detached};
use spacekit_primitives::v1::fact::AccessPolicy;
use spacekit_primitives::v1::identity::QuantumDID;
use spacekit_storage_node::access_policy::create_fact_verification_message;
use spacekit_storage_node::workspace::{
    build_workspace_fact_package, WorkspaceContent, WorkspaceStatus,
};

const DEFAULT_BASE: &str = "http://127.0.0.1:3030";
const DEFAULT_DID: &str = "did:spacekit:strict:soak";
const SPHINCS_ALG: &str = "sphincs-128s";

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| DEFAULT_BASE.into());
    let did = args.next().unwrap_or_else(|| DEFAULT_DID.into());

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

    if mode != "strict" {
        eprintln!(
            "FAIL: expected blob_fact_auth_mode=strict (got {mode}). \
             Set SPACEKIT_BLOB_FACT_AUTH=strict or blob_fact_auth in network.toml, then restart."
        );
        failed += 1;
    } else {
        println!("PASS: strict mode active");
    }

    let metrics = http
        .get(format!("{base}/api/agentic/metrics"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if metrics.contains("spacekit_blob_fact_auth_mode{mode=\"strict\"}") {
        println!("PASS: Prometheus strict mode gauge");
    } else {
        eprintln!("FAIL: metrics missing strict mode gauge");
        failed += 1;
    }

    let payload = format!("strict-soak-{}", chrono::Utc::now().timestamp());
    let hash = hex::encode(blake3::hash(payload.as_bytes()).as_bytes());

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

    let r = http
        .put(format!("{base}/blobs/{hash}"))
        .header("Authorization", format!("DID {did}"))
        .body(payload.clone())
        .send()
        .await?;
    if r.status().is_success() {
        println!("PASS: PUT /blobs with DID → {}", r.status());
    } else {
        eprintln!("FAIL: PUT /blobs with DID → {}", r.status());
        failed += 1;
    }

    let r = http.get(format!("{base}/blobs/{hash}")).send().await?;
    if r.status() == reqwest::StatusCode::UNAUTHORIZED {
        println!("PASS: GET /blobs without auth → 401");
    } else {
        eprintln!(
            "FAIL: GET /blobs without auth → {} (strict requires auth)",
            r.status()
        );
        failed += 1;
    }

    if tokens_ok {
        let mint: serde_json::Value = http
            .post(format!("{base}/api/upload-tokens"))
            .header("Authorization", format!("DID {did}"))
            .json(&json!({
                "operation": "get_blob",
                "resource": hash,
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
            .get(format!("{base}/blobs/{hash}"))
            .header("Authorization", format!("UploadToken {token}"))
            .send()
            .await?;
        if r.status().is_success() {
            println!("PASS: GET /blobs with UploadToken → {}", r.status());
        } else {
            eprintln!("FAIL: GET /blobs with UploadToken → {}", r.status());
            failed += 1;
        }
    } else {
        eprintln!("SKIP: get_blob token (upload_tokens_configured=false)");
    }

    let ephemeral_author = "did:spacekit:strict:soak:signed";
    let mut fact_pkg = build_workspace_fact_package(WorkspaceContent {
        workspace_id: format!("strict-ws-{}", chrono::Utc::now().timestamp()),
        owner_did: ephemeral_author.into(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: Default::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    })?;
    fact_pkg.author = QuantumDID::parse(ephemeral_author)
        .map_err(|e| anyhow::anyhow!("ephemeral author DID: {e}"))?;
    fact_pkg.signature.algorithm = SPHINCS_ALG.to_string();
    let msg = create_fact_verification_message(&fact_pkg)?;
    let (pk, sk) = generate_sphincs_keypair(SPHINCS_ALG)?;
    fact_pkg.signature = sign_sphincs_detached(&msg, SPHINCS_ALG, &pk, &sk)?;

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

    let mut unsigned = fact_pkg.clone();
    unsigned.signature.signature_bytes.clear();
    let r = http
        .post(format!("{base}/facts"))
        .header("Authorization", format!("DID {ephemeral_author}"))
        .json(&unsigned)
        .send()
        .await?;
    if r.status() == reqwest::StatusCode::BAD_REQUEST {
        println!("PASS: POST /facts empty signature → 400");
    } else {
        eprintln!("FAIL: POST /facts empty signature → {}", r.status());
        failed += 1;
    }

    let r = http
        .post(format!("{base}/facts"))
        .header("Authorization", format!("DID {did}"))
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
        .header("Authorization", format!("DID {ephemeral_author}"))
        .json(&fact_pkg)
        .send()
        .await?;
    if r.status().is_success() {
        println!("PASS: POST /facts signed + matching DID → {}", r.status());
    } else {
        eprintln!(
            "FAIL: POST /facts signed → {} {:?}",
            r.status(),
            r.text().await.ok()
        );
        failed += 1;
    }

    if failed > 0 {
        bail!("strict auth soak: {failed} check(s) failed");
    }
    println!("\nstrict auth soak: all checks passed");
    Ok(())
}
