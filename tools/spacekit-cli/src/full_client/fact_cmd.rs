//! `spacekit fact` — build, submit, and fetch [`FactPackage`] records (`POST /facts`).

use std::path::Path;

use colored::Colorize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use spacekit_primitives::v1::crypto::quantum::SPHINCSSignature;
use spacekit_primitives::v1::fact::{
    AccessPolicy, CollectionMethod, DataSource, FactCategory, FactContent, FactID, FactMetadata,
    FactPackage, KnowledgeDomain, LicenseType, ProofType, VerificationLevel, VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;

use super::{
    resolve_effective_did, resolve_remote_storage_base_url, Cli, CliContext, FactCommands,
};

fn placeholder_signature() -> SPHINCSSignature {
    SPHINCSSignature::new(
        vec![0u8; 64],
        "SPHINCS+-SHAKE-256-128s-simple".to_string(),
        vec![0u8; 32],
    )
}

fn parse_parent_ids(parents: &[String]) -> Result<Vec<FactID>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for p in parents {
        let hex_str = p.trim().trim_start_matches("0x");
        let bytes = hex::decode(hex_str)?;
        if bytes.len() != 32 {
            return Err(format!(
                "parent fact id must be 32 bytes (64 hex), got {}",
                bytes.len()
            )
            .into());
        }
        let mut id = [0u8; 32];
        id.copy_from_slice(&bytes);
        out.push(id);
    }
    Ok(out)
}

/// Deterministic id from author + schema + canonical JSON body + sorted parents.
pub fn fact_id_from_json(
    author_did: &str,
    schema: &str,
    data: &Value,
    parents: &[FactID],
) -> Result<FactID, Box<dyn std::error::Error>> {
    let body = serde_json::to_vec(data)?;
    fact_id_from_parts(author_did, schema, &body, parents)
}

fn fact_id_from_parts(
    author_did: &str,
    schema: &str,
    body: &[u8],
    parents: &[FactID],
) -> Result<FactID, Box<dyn std::error::Error>> {
    let mut h = Sha256::new();
    h.update(b"spacekit-fact-v1\0");
    h.update(author_did.as_bytes());
    h.update(b"\0");
    h.update(schema.as_bytes());
    h.update(b"\0");
    let mut p = parents.to_vec();
    p.sort_unstable();
    for id in &p {
        h.update(id);
    }
    h.update(b"\0");
    h.update(body);
    Ok(h.finalize().into())
}

/// Unique id (includes timestamp) for one-off submissions.
fn fact_id_unique(author_did: &str, schema: &str, body: &[u8], parents: &[FactID]) -> FactID {
    let ts = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let mut h = Sha256::new();
    h.update(b"spacekit-fact-unique-v1\0");
    h.update(author_did.as_bytes());
    h.update(b"\0");
    h.update(schema.as_bytes());
    h.update(b"\0");
    h.update(&ts.to_le_bytes());
    h.update(b"\0");
    for id in parents {
        h.update(id);
    }
    h.update(b"\0");
    h.update(body);
    h.finalize().into()
}

pub fn build_json_fact_package(
    author_did: &str,
    schema: &str,
    data: Value,
    parents: Vec<FactID>,
    tags: Vec<String>,
    deterministic: bool,
) -> Result<FactPackage, Box<dyn std::error::Error>> {
    let author = QuantumDID::parse(author_did).map_err(|e| format!("invalid author DID: {}", e))?;
    let body_bytes = serde_json::to_vec(&data)?;
    let fact_id = if deterministic {
        fact_id_from_parts(author_did, schema, &body_bytes, &parents)?
    } else {
        fact_id_unique(author_did, schema, &body_bytes, &parents)
    };
    let created_at = chrono::Utc::now().timestamp() as u64;
    let checksum: [u8; 32] = Sha256::digest(&body_bytes).into();
    let mut all_tags = tags;
    if !all_tags.iter().any(|t| t == "spacekit-fact") {
        all_tags.push("spacekit-fact".to_string());
    }
    all_tags.push(format!("schema:{}", schema));

    Ok(FactPackage {
        fact_id,
        version: 1,
        created_at,
        expires_at: None,
        content: FactContent::Json {
            data,
            schema: Some(schema.to_string()),
        },
        metadata: FactMetadata {
            category: FactCategory::Technical,
            tags: all_tags,
            domain: KnowledgeDomain::ComputerScience,
            source: DataSource::UserInput {
                application: author.clone(),
                user: author.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: LicenseType::MIT,
            size_bytes: body_bytes.len() as u64,
            checksum,
        },
        author: author.clone(),
        signature: placeholder_signature(),
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: Vec::new(),
            verification_timestamp: created_at,
            verifier: Some(author),
        },
        dependencies: parents,
        citations: Vec::new(),
        confidence_score: 1.0,
        access_policy: AccessPolicy::Public,
        encryption: None,
    })
}

fn mime_from_path(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8".to_string(),
        "css" => "text/css; charset=utf-8".to_string(),
        "js" | "mjs" => "application/javascript; charset=utf-8".to_string(),
        "json" => "application/json".to_string(),
        "wasm" => "application/wasm".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "png" => "image/png".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "webp" => "image/webp".to_string(),
        "txt" => "text/plain; charset=utf-8".to_string(),
        "md" | "markdown" => "text/markdown; charset=utf-8".to_string(),
        "pdf" => "application/pdf".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

pub fn build_binary_fact_package(
    author_did: &str,
    schema: &str,
    file_path: &str,
    data: &[u8],
    parents: Vec<FactID>,
    tags: Vec<String>,
    deterministic: bool,
) -> Result<FactPackage, Box<dyn std::error::Error>> {
    let author = QuantumDID::parse(author_did).map_err(|e| format!("invalid author DID: {}", e))?;
    let content_hash: [u8; 32] = Sha256::digest(data).into();
    let fact_id = if deterministic {
        fact_id_from_parts(author_did, schema, data, &parents)?
    } else {
        fact_id_unique(author_did, schema, data, &parents)
    };
    let created_at = chrono::Utc::now().timestamp() as u64;
    let mime_type = mime_from_path(file_path);
    let mut all_tags = tags;
    all_tags.push("spacekit-fact".to_string());
    all_tags.push(format!("schema:{}", schema));

    Ok(FactPackage {
        fact_id,
        version: 1,
        created_at,
        expires_at: None,
        content: FactContent::Binary {
            data: data.to_vec(),
            mime_type,
            hash: content_hash,
        },
        metadata: FactMetadata {
            category: FactCategory::UserGenerated,
            tags: all_tags,
            domain: KnowledgeDomain::Custom("Binary artifact".to_string()),
            source: DataSource::UserInput {
                application: author.clone(),
                user: author.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: LicenseType::MIT,
            size_bytes: data.len() as u64,
            checksum: content_hash,
        },
        author: author.clone(),
        signature: placeholder_signature(),
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: Vec::new(),
            verification_timestamp: created_at,
            verifier: Some(author),
        },
        dependencies: parents,
        citations: Vec::new(),
        confidence_score: 1.0,
        access_policy: AccessPolicy::Public,
        encryption: None,
    })
}

async fn post_fact_remote(
    client: &reqwest::Client,
    base: &str,
    pkg: &FactPackage,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let url = format!("{}/facts", base.trim_end_matches('/'));
    let r = client.post(&url).json(pkg).send().await?;
    let status = r.status();
    let body: serde_json::Value = r.json().await.unwrap_or(serde_json::json!({}));
    if status.is_success() || status == reqwest::StatusCode::CREATED {
        return Ok(body);
    }
    Err(format!("POST {} -> {} {}", url, status, body).into())
}

async fn fetch_fact_remote(
    client: &reqwest::Client,
    base: &str,
    id_hex: &str,
) -> Result<FactPackage, Box<dyn std::error::Error>> {
    let url = format!("{}/facts/{}", base.trim_end_matches('/'), id_hex.trim());
    let r = client.get(&url).send().await?;
    if !r.status().is_success() {
        return Err(format!("GET {} -> {}", url, r.status()).into());
    }
    Ok(r.json().await?)
}

fn resolve_author_did(
    cli: &Cli,
    ctx: &CliContext,
    override_did: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    resolve_effective_did(cli, ctx, override_did)
}

pub(super) async fn handle_fact_command(
    cli: &Cli,
    ctx: &CliContext,
    cmd: &FactCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        FactCommands::Create {
            package,
            data,
            file,
            schema,
            parent,
            tag,
            deterministic,
            output,
            storage_url,
            dry_run,
        } => {
            if [package.is_some(), data.is_some(), file.is_some()]
                .iter()
                .filter(|b| **b)
                .count()
                != 1
            {
                return Err("specify exactly one of --package, --data, or --file".into());
            }

            let author_did = resolve_author_did(cli, ctx, None)?;
            let parents = parse_parent_ids(parent)?;

            let pkg = if let Some(path) = package {
                let s = std::fs::read_to_string(path)?;
                let pkg: FactPackage = serde_json::from_str(&s)?;
                pkg
            } else if let Some(path) = data {
                let schema = schema
                    .as_deref()
                    .ok_or("--schema is required with --data")?;
                let raw = std::fs::read_to_string(path)?;
                let value: Value = serde_json::from_str(&raw)?;
                build_json_fact_package(
                    &author_did,
                    schema,
                    value,
                    parents,
                    tag.clone(),
                    *deterministic,
                )?
            } else if let Some(path) = file {
                let schema = schema
                    .as_deref()
                    .ok_or("--schema is required with --file")?;
                let bytes = std::fs::read(path)?;
                build_binary_fact_package(
                    &author_did,
                    schema,
                    path,
                    &bytes,
                    parents,
                    tag.clone(),
                    *deterministic,
                )?
            } else {
                unreachable!()
            };

            let id_hex = hex::encode(pkg.fact_id);
            println!("📦 FactPackage");
            println!("   fact_id: {}", id_hex.green());
            if let FactContent::Json { schema: s, .. } = &pkg.content {
                if let Some(s) = s {
                    println!("   schema:  {}", s.cyan());
                }
            }
            if let FactContent::Binary { mime_type, .. } = &pkg.content {
                println!("   mime:    {}", mime_type);
            }
            println!("   author:  {}", author_did);
            if !pkg.dependencies.is_empty() {
                println!("   parents: {}", pkg.dependencies.len());
            }

            if let Some(out_path) = output {
                if let Some(parent) = Path::new(out_path).parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(out_path, serde_json::to_string_pretty(&pkg)?)?;
                println!("💾 Wrote {}", out_path);
            }

            if *dry_run {
                println!("{}", "(dry-run: not posted to storage)".yellow());
                return Ok(());
            }

            let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
            println!("   storage: {}", base.cyan());
            let client = reqwest::Client::new();
            let resp = post_fact_remote(&client, &base, &pkg).await?;
            let status = resp["status"].as_str().unwrap_or("created");
            println!("{}", format!("✅ Posted to /facts ({})", status).green());
            Ok(())
        }
        FactCommands::Get {
            fact_id,
            storage_url,
            output,
        } => {
            let id = fact_id.trim().trim_start_matches("0x");
            let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
            let client = reqwest::Client::new();
            let pkg = fetch_fact_remote(&client, &base, id).await?;
            if let Some(path) = output {
                std::fs::write(path, serde_json::to_string_pretty(&pkg)?)?;
                println!("💾 Wrote {}", path);
            } else {
                println!("{}", serde_json::to_string_pretty(&pkg)?);
            }
            Ok(())
        }
        FactCommands::Id {
            data,
            schema,
            author_did,
            parent,
            deterministic,
        } => {
            let schema = schema.as_deref().ok_or("--schema is required")?;
            let author = if let Some(d) = author_did {
                d.clone()
            } else {
                resolve_author_did(cli, ctx, None)?
            };
            let parents = parse_parent_ids(parent)?;
            let raw = std::fs::read_to_string(data)?;
            let value: Value = serde_json::from_str(&raw)?;
            let id = if *deterministic {
                hex::encode(fact_id_from_json(&author, schema, &value, &parents)?)
            } else {
                let body = serde_json::to_vec(&value)?;
                hex::encode(fact_id_unique(&author, schema, &body, &parents))
            };
            println!("{}", id);
            Ok(())
        }
    }
}
