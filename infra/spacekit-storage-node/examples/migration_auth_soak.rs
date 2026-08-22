//! Live HTTP soak for DID-signed migration (layer 2) on a running storage node.
//!
//! Run after hybrid soak; works in hybrid or strict mode. Requires `operator_did`
//! configured on the node (builtin network sets `SPACEKIT_NODE_DID`).
//!
//! ```bash
//! spacekit network up
//!
//! cargo run -p spacekit-storage-node --example migration_auth_soak --features standalone -- \
//!   http://127.0.0.1:3030 did:spacekit:testnet:YOUR_DID
//! ```

use anyhow::{bail, Context, Result};
use reqwest::Client;
use serde_json::json;
use spacekit_primitives::v1::fact::AccessPolicy;
use spacekit_storage_node::workspace::{WorkspaceContent, WorkspaceQuotas, WorkspaceStatus};

const DEFAULT_BASE: &str = "http://127.0.0.1:3030";

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args.next().unwrap_or_else(|| DEFAULT_BASE.into());
    let owner_did = args
        .next()
        .context("usage: migration_auth_soak <base_url> <owner_did>")?;

    let http = Client::new();
    let mut failed = 0u32;

    let health: serde_json::Value = http
        .get(format!("{base}/api/agentic/health"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mig_ok = health["migration_signing_configured"]
        .as_bool()
        .unwrap_or(false);
    let handoff_ok = health["handoff_signing_configured"]
        .as_bool()
        .unwrap_or(false);
    println!(
        "health → migration_signing_configured={mig_ok} handoff_signing_configured={handoff_ok}"
    );
    if mig_ok {
        println!("PASS: operator SPHINCS keypair present");
    } else {
        eprintln!(
            "FAIL: migration_signing_configured=false (restart node to create .operator_sphincs_keypair)"
        );
        failed += 1;
    }

    let op_self: serde_json::Value = http
        .get(format!("{base}/api/operators/self"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let manifest = &op_self["manifest"];
    let versions = manifest["supported_migration_versions"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    if versions.iter().any(|v| *v == "v2") {
        println!("PASS: operator manifest advertises v2");
    } else {
        eprintln!("FAIL: supported_migration_versions missing v2: {versions:?}");
        failed += 1;
    }
    if manifest["did_signature_capable"].as_bool() == Some(true) {
        println!("PASS: did_signature_capable=true");
    } else {
        eprintln!("FAIL: did_signature_capable not true");
        failed += 1;
    }
    if manifest["sphincs_public_key_hex"]
        .as_str()
        .is_some_and(|s| !s.is_empty())
    {
        println!("PASS: sphincs_public_key_hex present");
    } else if mig_ok {
        eprintln!("WARN: sphincs_public_key_hex missing (publish operator manifest with --sign)");
    }

    let ws_id = format!("mig-soak-{}", chrono::Utc::now().timestamp());
    let content = WorkspaceContent {
        workspace_id: ws_id.clone(),
        owner_did: owner_did.clone(),
        collaborators: vec![],
        associated_repos: vec![],
        quotas: WorkspaceQuotas::default(),
        default_access_policy: AccessPolicy::Public,
        status: WorkspaceStatus::Active,
        created_at: 1,
        updated_at: 1,
    };
    let r = http
        .post(format!("{base}/api/workspaces"))
        .header("Authorization", format!("DID {owner_did}"))
        .json(&content)
        .send()
        .await?;
    if r.status().is_success() {
        println!("PASS: POST /api/workspaces → {}", r.status());
    } else {
        eprintln!(
            "FAIL: POST /api/workspaces → {} {:?}",
            r.status(),
            r.text().await.ok()
        );
        failed += 1;
        bail!("cannot continue without workspace");
    }

    let bundle: serde_json::Value = http
        .get(format!("{base}/api/workspaces/{ws_id}/export"))
        .header("Authorization", format!("DID {owner_did}"))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let mig = bundle
        .get("migration_manifest")
        .context("export missing migration_manifest (configure operator_did on node)")?;
    let schema_version = mig["schema_version"].as_str().unwrap_or("?");
    if schema_version == "spacekit:migration:v2" {
        println!("PASS: migration_manifest schema_version=v2");
    } else {
        eprintln!("FAIL: expected v2 migration manifest, got {schema_version}");
        failed += 1;
    }
    let sigs = mig["did_signatures"]
        .as_array()
        .context("did_signatures array")?;
    if sigs.iter().any(|s| s["signer_role"] == "source_operator") {
        println!("PASS: source_operator signature present");
    } else {
        eprintln!("FAIL: no source_operator in did_signatures");
        failed += 1;
    }
    if bundle
        .get("handoff_signature")
        .and_then(|v| v.as_str())
        .is_some()
    {
        println!("PASS: handoff_signature present on bundle");
    } else {
        eprintln!("WARN: handoff_signature absent (set SPACEKIT_HANDOFF_SECRET for layer 1)");
    }

    let tampered = json!({
        "migration_manifest": {
            "schema_version": "spacekit:migration:v2",
            "migration_id": "tampered",
            "workspace_id": "not-the-same"
        }
    });
    if tampered["migration_manifest"]["migration_id"].as_str() == Some("tampered") {
        println!("PASS: tamper fixture built (use migration verify CLI offline)");
    }

    if failed > 0 {
        bail!("migration auth soak: {failed} check(s) failed");
    }
    println!("\nmigration auth soak: all checks passed");
    println!("Next: spacekit migration verify <export.json> after saving bundle to disk");
    Ok(())
}
