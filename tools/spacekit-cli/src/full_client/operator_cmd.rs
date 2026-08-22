//! `spacekit operator` — publish `spacekit:operator:v1` discovery manifests.

use colored::Colorize;
use sha2::{Digest, Sha256};
use spacekit_primitives::v1::crypto::quantum::{generate_sphincs_keypair, sign_sphincs_detached};
use spacekit_storage_node::access_policy::create_fact_verification_message;
use spacekit_storage_node::migration::load_operator_keypair;
use spacekit_storage_node::operator_manifest::{
    build_operator_fact_package, OperatorManifestContent,
};

use super::{resolve_remote_storage_base_url, CliContext, OperatorCommands};

const SPHINCS_ALG: &str = "sphincs-128s";

fn operator_fact_id_hex(operator_did: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"spacekit-operator-v1\0");
    h.update(operator_did.as_bytes());
    hex::encode(h.finalize())
}

async fn effective_did(override_did: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(d) = override_did {
        return Ok(d.to_string());
    }
    Ok(CliContext::load_sync()?.did)
}

pub async fn handle_operator_command(
    cmd: &OperatorCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        OperatorCommands::Publish {
            storage_url,
            operator_did,
            display_name,
            policy_uri,
            blob_fact_auth,
            feature,
            sign,
        } => {
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let did = effective_did(operator_did.as_deref()).await?;
            let now = chrono::Utc::now().timestamp() as u64;
            let supported_features = if feature.is_empty() {
                vec!["workspaces".into(), "federation_export".into()]
            } else {
                feature.clone()
            };
            let data_dir = crate::network_profile::default_data_dir("storage");
            let keypair = load_operator_keypair(Some(&data_dir));
            let mut migration_versions = vec!["v1".to_string()];
            let mut did_capable = false;
            let mut sphincs_pk_hex = None;
            if let Some(ref kp) = keypair {
                migration_versions.push("v2".to_string());
                did_capable = true;
                sphincs_pk_hex = Some(hex::encode(&kp.public_key));
            }
            let content = OperatorManifestContent {
                operator_did: did.clone(),
                display_name: display_name.clone(),
                storage_http_url: base.trim_end_matches('/').to_string(),
                blob_fact_auth: blob_fact_auth.clone(),
                content_policy_uri: policy_uri.clone(),
                supported_features,
                published_at: now,
                supported_migration_versions: migration_versions,
                did_signature_capable: did_capable,
                sphincs_public_key_hex: sphincs_pk_hex,
            };
            let mut pkg = build_operator_fact_package(content)?;
            if *sign {
                pkg.signature.algorithm = SPHINCS_ALG.to_string();
                let msg = create_fact_verification_message(&pkg)?;
                let (pk, sk) = if let Some(kp) = keypair {
                    (kp.public_key, kp.secret_key)
                } else {
                    generate_sphincs_keypair(SPHINCS_ALG)?
                };
                pkg.signature = sign_sphincs_detached(&msg, SPHINCS_ALG, &pk, &sk)?;
            }
            let client = reqwest::Client::new();
            let resp = client
                .post(format!("{}/facts", base.trim_end_matches('/')))
                .header("Authorization", format!("DID {}", did))
                .json(&pkg)
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                return Err(format!("publish operator manifest HTTP {}: {}", status, text).into());
            }
            let fact_id = operator_fact_id_hex(&did);
            println!(
                "{} operator manifest published (fact_id={})",
                "✓".green(),
                fact_id
            );
            if !sign {
                println!(
                    "{} node in strict mode requires --sign (sphincs-128s)",
                    "hint:".yellow()
                );
            }
            println!("{text}");
        }
        OperatorCommands::FactId { operator_did } => {
            let did = effective_did(operator_did.as_deref()).await?;
            println!("{}", operator_fact_id_hex(&did));
        }
        OperatorCommands::Show {
            storage_url,
            public_url,
        } => {
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let mut url = format!("{}/api/operators/self", base.trim_end_matches('/'));
            if let Some(pu) = public_url {
                use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
                let enc = utf8_percent_encode(pu.trim_end_matches('/'), NON_ALPHANUMERIC);
                url.push_str(&format!("?public_url={enc}"));
            }
            let resp = reqwest::Client::new().get(&url).send().await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                return Err(format!("operator self HTTP {}: {}", status, text).into());
            }
            println!("{text}");
        }
    }
    Ok(())
}
