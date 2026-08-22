//! `spacekit migration` — verify and sign DID-signed migration manifests in export bundles.

use colored::Colorize;
use spacekit_storage_node::migration::{
    load_operator_keypair, load_or_create_migration_signer_keypair, load_signing_keypair_for_role,
    migration_signer_key_path, sign_manifest_role, validate_migration_manifest, MigrationManifest,
    MigrationScenario, SCHEMA_VERSION_V2,
};
use spacekit_storage_node::operator_manifest::load_published_operator_manifest;
use spacekit_storage_node::workspace::WorkspaceExportBundle;

use super::{resolve_remote_storage_base_url, MigrationCommands};

pub async fn handle_migration_command(
    cmd: &MigrationCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        MigrationCommands::Keygen {
            signer_did,
            storage_url,
        } => {
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let data_dir = storage_data_dir_from_url(&base)?;
            let kp = load_or_create_migration_signer_keypair(&data_dir, signer_did)?;
            let path = migration_signer_key_path(&data_dir, signer_did);
            println!(
                "{} wrote migration signer key for {} → {}",
                "✓".green(),
                signer_did,
                path.display()
            );
        }
        MigrationCommands::Verify {
            bundle_file,
            storage_url,
        } => {
            let raw = std::fs::read_to_string(bundle_file)?;
            let bundle: WorkspaceExportBundle = serde_json::from_str(&raw)?;
            let mig = bundle
                .migration_manifest
                .as_ref()
                .ok_or("export bundle has no migration_manifest")?;
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let data_dir = storage_data_dir_from_url(&base)?;
            verify_manifest(mig, &data_dir).await?;
            println!(
                "{} migration manifest valid (schema_version={})",
                "✓".green(),
                mig.schema_version
            );
        }
        MigrationCommands::Sign {
            bundle_file,
            role,
            signer_did,
            storage_url,
            stdout,
        } => {
            let raw = std::fs::read_to_string(bundle_file)?;
            let mut bundle: WorkspaceExportBundle = serde_json::from_str(&raw)?;
            let mig = bundle
                .migration_manifest
                .as_mut()
                .ok_or("export bundle has no migration_manifest")?;
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let data_dir = storage_data_dir_from_url(&base)?;
            let did = signer_did
                .clone()
                .or_else(|| std::env::var("SPACEKIT_NODE_DID").ok())
                .filter(|s| !s.trim().is_empty())
                .ok_or("set --signer-did or SPACEKIT_NODE_DID")?;
            let op_did = std::env::var("SPACEKIT_NODE_DID").ok();
            let kp = load_signing_keypair_for_role(
                &data_dir,
                role,
                &did,
                op_did.as_deref(),
            )
            .or_else(|| load_operator_keypair(Some(&data_dir)))
            .ok_or_else(|| {
                if role == "workspace_owner" {
                    "no workspace_owner key — run `spacekit migration keygen --signer-did <owner>`"
                } else {
                    "no .operator_sphincs_keypair in storage data dir"
                }
            })?;
            let now = chrono::Utc::now().timestamp() as u64;
            sign_manifest_role(mig, role, &did, &kp, now)?;
            verify_manifest(mig, &data_dir).await?;
            let schema_version = mig.schema_version.clone();
            let out = serde_json::to_string_pretty(&bundle)?;
            if *stdout {
                println!("{out}");
            } else {
                std::fs::write(bundle_file, out)?;
                println!(
                    "{} signed migration manifest (role={}, schema_version={})",
                    "✓".green(),
                    role,
                    schema_version
                );
            }
        }
    }
    Ok(())
}

fn storage_data_dir_from_url(
    base_url: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if let Ok(p) = std::env::var("SPACEKIT_STORAGE_DATA_DIR") {
        if !p.trim().is_empty() {
            return Ok(std::path::PathBuf::from(p));
        }
    }
    let _ = base_url;
    Ok(crate::network_profile::default_data_dir("storage"))
}

async fn verify_manifest(
    mig: &MigrationManifest,
    data_dir: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let op_did = std::env::var("SPACEKIT_NODE_DID").ok();
    for entry in &mig.did_signatures {
        let pk = load_signing_keypair_for_role(
            data_dir,
            &entry.signer_role,
            &entry.signer_did,
            op_did.as_deref(),
        )
        .map(|kp| kp.public_key);
        let pk = if let Some(p) = pk {
            Some(p)
        } else if let Ok(Some(op)) =
            load_published_operator_manifest(data_dir, &entry.signer_did).await
        {
            op.sphincs_public_key_hex
                .as_ref()
                .and_then(|h| hex::decode(h).ok())
        } else if entry.signer_role == "workspace_owner" {
            std::env::var("SPACEKIT_MIGRATION_OWNER_PUBKEY_HEX")
                .ok()
                .and_then(|h| hex::decode(h.trim()).ok())
        } else {
            None
        };
        let pk = pk.ok_or_else(|| format!("cannot resolve public key for {}", entry.signer_did))?;
        if !spacekit_storage_node::migration::verify_signature_entry(mig, entry, &pk)? {
            return Err(format!(
                "invalid signature: {} ({})",
                entry.signer_did, entry.signer_role
            )
            .into());
        }
    }
    validate_migration_manifest(mig, MigrationScenario::OperatorInitiated, |_, _| None)?;
    if mig.schema_version == SCHEMA_VERSION_V2 && mig.did_signatures.is_empty() {
        return Err("v2 manifest has no signatures".into());
    }
    Ok(())
}
