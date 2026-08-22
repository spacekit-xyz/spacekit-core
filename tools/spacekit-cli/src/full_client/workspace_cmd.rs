//! `spacekit workspace` — local config + push/publish to storage node (`/api/workspaces`).

use std::path::{Path, PathBuf};

use colored::Colorize;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};
use serde::{Deserialize, Serialize};

use super::{resolve_remote_storage_base_url, CliContext, WorkspaceCommands};

const WORKSPACE_DIR: &str = ".spacekit/workspace";
const WEBSITE_ADMIN_DID: &str = "did:spacekit:admin:website-api";

const DOCUMENT_PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'?')
    .add(b'/');

fn encode_document_path_segment(segment: &str) -> String {
    utf8_percent_encode(segment, DOCUMENT_PATH_ENCODE_SET).to_string()
}

fn document_api_url(base: &str, collection: &str, doc_id: &str) -> String {
    format!(
        "{}/api/documents/{}/{}",
        base.trim_end_matches('/'),
        encode_document_path_segment(collection),
        encode_document_path_segment(doc_id),
    )
}

fn workspace_base() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(WORKSPACE_DIR)
}

fn config_path() -> PathBuf {
    workspace_base().join("config.json")
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    let s = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&s)?)
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(v)?)?;
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct CollaboratorBody {
    did: String,
    role: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
struct WorkspaceQuotasLocal {
    #[serde(default)]
    max_sandbox_bytes: u64,
    #[serde(default)]
    max_storage_bytes: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct LocalWorkspaceConfig {
    workspace_id: String,
    #[serde(default)]
    description: String,
    #[serde(default = "default_visibility")]
    visibility: String,
    #[serde(default)]
    collaborators: Vec<CollaboratorBody>,
    #[serde(default)]
    associated_repos: Vec<String>,
    #[serde(default)]
    quotas: WorkspaceQuotasLocal,
    #[serde(default)]
    remote_url: String,
}

fn default_visibility() -> String {
    "public".to_string()
}

#[derive(Debug, Serialize)]
struct WorkspaceApiBody {
    workspace_id: String,
    collaborators: Vec<CollaboratorBody>,
    associated_repos: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quotas: Option<WorkspaceQuotasLocal>,
    visibility: String,
}

#[derive(Debug, Serialize)]
struct WorkspaceUpdateBody {
    collaborators: Vec<CollaboratorBody>,
    associated_repos: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    quotas: Option<WorkspaceQuotasLocal>,
    visibility: String,
}

async fn effective_did(override_did: Option<&str>) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(d) = override_did {
        return Ok(d.to_string());
    }
    Ok(CliContext::load_sync()?.did)
}

fn parse_collaborators(raw: &[String]) -> Vec<CollaboratorBody> {
    raw.iter()
        .map(|s| {
            let (did, role) = s.split_once(':').unwrap_or((s.as_str(), "agent"));
            CollaboratorBody {
                did: did.to_string(),
                role: role.to_string(),
            }
        })
        .collect()
}

fn load_local_config() -> Result<LocalWorkspaceConfig, Box<dyn std::error::Error>> {
    let path = config_path();
    if !path.exists() {
        return Err("no local workspace (run `spacekit workspace init <ID>` first)".into());
    }
    read_json(&path)
}

fn api_body_from_config(cfg: &LocalWorkspaceConfig) -> WorkspaceApiBody {
    let quotas = if cfg.quotas.max_sandbox_bytes > 0 || cfg.quotas.max_storage_bytes > 0 {
        Some(cfg.quotas.clone())
    } else {
        None
    };
    WorkspaceApiBody {
        workspace_id: cfg.workspace_id.clone(),
        collaborators: cfg.collaborators.clone(),
        associated_repos: cfg.associated_repos.clone(),
        quotas,
        visibility: cfg.visibility.clone(),
    }
}

fn update_body_from_config(cfg: &LocalWorkspaceConfig) -> WorkspaceUpdateBody {
    let quotas = if cfg.quotas.max_sandbox_bytes > 0 || cfg.quotas.max_storage_bytes > 0 {
        Some(cfg.quotas.clone())
    } else {
        None
    };
    WorkspaceUpdateBody {
        collaborators: cfg.collaborators.clone(),
        associated_repos: cfg.associated_repos.clone(),
        quotas,
        visibility: cfg.visibility.clone(),
    }
}

async fn push_workspace_to_node(
    base: &str,
    did: &str,
    cfg: &LocalWorkspaceConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let trimmed = base.trim_end_matches('/');
    let get_url = format!("{}/api/workspaces/{}", trimmed, cfg.workspace_id);
    let existing = client
        .get(&get_url)
        .header("Authorization", format!("DID {}", did))
        .send()
        .await?;

    if existing.status().is_success() {
        let body = update_body_from_config(cfg);
        let resp = client
            .put(&get_url)
            .header("Authorization", format!("DID {}", did))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(format!("update workspace HTTP {}: {}", status, text).into());
        }
        println!(
            "{} {}",
            "✓".green(),
            format!("workspace {} updated on storage node", cfg.workspace_id).green()
        );
    } else {
        let body = api_body_from_config(cfg);
        let resp = client
            .post(format!("{}/api/workspaces", trimmed))
            .header("Authorization", format!("DID {}", did))
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(format!("create workspace HTTP {}: {}", status, text).into());
        }
        println!(
            "{} {}",
            "✓".green(),
            format!("workspace {} created on storage node", cfg.workspace_id).green()
        );
    }
    Ok(())
}

async fn publish_workspace_registry(
    base: &str,
    did: &str,
    cfg: &LocalWorkspaceConfig,
    visibility_override: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let visibility = visibility_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| cfg.visibility.clone());
    let registry_url = document_api_url(base, "workspace_registry", &cfg.workspace_id);
    let registry_body = serde_json::json!({
        "workspace_id": cfg.workspace_id,
        "name": cfg.workspace_id,
        "description": cfg.description,
        "owner_did": did,
        "associated_repos": cfg.associated_repos,
        "visibility": visibility,
        "updated_at": chrono::Utc::now().to_rfc3339(),
    });

    let client = reqwest::Client::new();
    for reg_did in [did, WEBSITE_ADMIN_DID] {
        let resp = client
            .put(&registry_url)
            .header("Authorization", format!("DID {}", reg_did))
            .header("content-type", "application/json")
            .json(&registry_body)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!(
                "publish workspace_registry ({}) HTTP {}: {}",
                reg_did,
                status,
                text.chars().take(300).collect::<String>()
            )
            .into());
        }
    }

    let vis_display = if visibility == "private" {
        "private".yellow().to_string()
    } else {
        "public".green().to_string()
    };
    println!(
        "{} {}",
        "✓".green(),
        format!("workspace {} published ({})", cfg.workspace_id, vis_display).green()
    );
    Ok(())
}

async fn run_init(
    workspace_id: &str,
    description: Option<String>,
    visibility: &str,
    repos: &[String],
    collaborators: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let base = workspace_base();
    if config_path().exists() {
        return Err(format!(
            "workspace already initialized at {}",
            config_path().display()
        )
        .into());
    }
    std::fs::create_dir_all(&base)?;
    let cfg = LocalWorkspaceConfig {
        workspace_id: workspace_id.to_string(),
        description: description.unwrap_or_default(),
        visibility: visibility.to_string(),
        collaborators: parse_collaborators(collaborators),
        associated_repos: repos.to_vec(),
        quotas: WorkspaceQuotasLocal::default(),
        remote_url: String::new(),
    };
    write_json(&config_path(), &cfg)?;
    println!(
        "{}",
        format!(
            "✅ Workspace initialized: {} ({})",
            workspace_id,
            if visibility == "private" {
                "private"
            } else {
                "public"
            }
        )
        .green()
    );
    println!(
        "{}",
        "   Next: spacekit workspace push && spacekit workspace publish".dimmed()
    );
    Ok(())
}

async fn run_list_registry(
    storage_url: Option<String>,
    owner_did: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
    let did = effective_did(owner_did.as_deref()).await?;
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/documents/workspace_registry",
        base.trim_end_matches('/')
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("DID {}", did))
        .send()
        .await?;

    if !resp.status().is_success() {
        println!(
            "{}",
            "No workspaces found (or storage node unreachable)".dimmed()
        );
        return Ok(());
    }

    let body: serde_json::Value = resp.json().await?;
    let docs = body
        .get("documents")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    if docs.is_empty() {
        println!("{}", "No workspaces found.".dimmed());
        println!(
            "{}",
            "   💡 Publish one: spacekit workspace init <ID> && spacekit workspace push && spacekit workspace publish"
                .dimmed()
        );
        return Ok(());
    }

    println!(
        "{}",
        format!("🗂️  {} workspace(s) on {}", docs.len(), base).bright_white()
    );
    println!();
    for doc in &docs {
        let data = doc.get("data").cloned().unwrap_or_default();
        let id = data
            .get("workspace_id")
            .or_else(|| data.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("(unnamed)");
        let owner = data
            .get("owner_did")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let visibility = data
            .get("visibility")
            .and_then(|v| v.as_str())
            .unwrap_or("public");
        let repos = data
            .get("associated_repos")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        let vis_display = match visibility {
            "private" => "🔒 private".yellow().to_string(),
            _ => "🌐 public".green().to_string(),
        };
        println!(
            "   {} {} ({} repo{})",
            id.cyan(),
            vis_display,
            repos,
            if repos == 1 { "" } else { "s" }
        );
        if !owner.is_empty() && owner != "unknown" {
            println!("      Owner: {}", owner.dimmed());
        }
        if let Some(desc) = data.get("description").and_then(|v| v.as_str()) {
            if !desc.is_empty() {
                println!("      {}", desc.dimmed());
            }
        }
        println!();
    }
    Ok(())
}

pub async fn handle_workspace_command(
    cmd: &WorkspaceCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        WorkspaceCommands::Init {
            workspace_id,
            description,
            visibility,
            repo,
            collaborator,
        } => {
            run_init(
                workspace_id,
                description.clone(),
                visibility,
                repo,
                collaborator,
            )
            .await?;
        }
        WorkspaceCommands::Push {
            storage_url,
            owner_did,
        } => {
            let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
            let did = effective_did(owner_did.as_deref()).await?;
            let cfg = load_local_config()?;
            push_workspace_to_node(&base, &did, &cfg).await?;
            println!("{}", format!("   visibility: {}", cfg.visibility).dimmed());
            if !cfg.associated_repos.is_empty() {
                println!(
                    "{}",
                    format!("   repos: {}", cfg.associated_repos.join(", ")).dimmed()
                );
            }
        }
        WorkspaceCommands::Publish {
            storage_url,
            owner_did,
            visibility,
        } => {
            let (base, _) = resolve_remote_storage_base_url(storage_url.as_deref())?;
            let did = effective_did(owner_did.as_deref()).await?;
            let cfg = load_local_config()?;
            publish_workspace_registry(&base, &did, &cfg, visibility.as_deref()).await?
        }
        WorkspaceCommands::Create {
            workspace_id,
            storage_url,
            owner_did,
            collaborator,
            repo,
        } => {
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let did = effective_did(owner_did.as_deref()).await?;
            let collaborators = parse_collaborators(collaborator);
            let body = WorkspaceApiBody {
                workspace_id: workspace_id.clone(),
                collaborators,
                associated_repos: repo.clone(),
                quotas: None,
                visibility: "public".to_string(),
            };
            let client = reqwest::Client::new();
            let resp = client
                .post(format!("{}/api/workspaces", base.trim_end_matches('/')))
                .header("Authorization", format!("DID {}", did))
                .json(&body)
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                return Err(format!("create workspace HTTP {}: {}", status, text).into());
            }
            println!(
                "{} {}",
                "✓".green(),
                format!("workspace {workspace_id} created").green()
            );
            println!("{text}");
        }
        WorkspaceCommands::Show {
            workspace_id,
            storage_url,
            owner_did,
        } => {
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let did = effective_did(owner_did.as_deref()).await?;
            let client = reqwest::Client::new();
            let resp = client
                .get(format!(
                    "{}/api/workspaces/{}",
                    base.trim_end_matches('/'),
                    workspace_id
                ))
                .header("Authorization", format!("DID {}", did))
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                return Err(format!("get workspace HTTP {}: {}", status, text).into());
            }
            println!("{text}");
        }
        WorkspaceCommands::List {
            storage_url,
            owner_did,
        } => run_list_registry(storage_url.clone(), owner_did.clone()).await?,
        WorkspaceCommands::Export {
            workspace_id,
            output,
            storage_url,
            owner_did,
        } => {
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let did = effective_did(owner_did.as_deref()).await?;
            let client = reqwest::Client::new();
            let resp = client
                .get(format!(
                    "{}/api/workspaces/{}/export",
                    base.trim_end_matches('/'),
                    workspace_id
                ))
                .header("Authorization", format!("DID {}", did))
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                return Err(format!("export workspace HTTP {}: {}", status, text).into());
            }
            if let Some(parent) = output.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }
            std::fs::write(output, &text)?;
            println!(
                "{} exported {} → {}",
                "✓".green(),
                workspace_id.green(),
                output.display()
            );
        }
        WorkspaceCommands::Import {
            file,
            storage_url,
            owner_did,
            replace,
            source_url,
            source_auth,
        } => {
            let base = resolve_remote_storage_base_url(storage_url.as_deref())?.0;
            let did = effective_did(owner_did.as_deref()).await?;
            let raw = std::fs::read_to_string(file)?;
            let mut body: serde_json::Value = serde_json::from_str(&raw)?;
            if body.get("bundle").is_none() {
                body = serde_json::json!({
                    "bundle": body,
                    "owner_did": did,
                });
            } else if body.get("owner_did").is_none() {
                body["owner_did"] = serde_json::Value::String(did.clone());
            }
            if *replace {
                body["on_conflict"] = serde_json::Value::String("replace".into());
            }
            if let Some(url) = source_url {
                body["replicate_blobs_from"] =
                    serde_json::Value::String(url.trim_end_matches('/').to_string());
            }
            if let Some(auth) = source_auth {
                body["replicate_source_authorization"] = serde_json::Value::String(auth.clone());
            }
            let client = reqwest::Client::new();
            let resp = client
                .post(format!(
                    "{}/api/workspaces/import",
                    base.trim_end_matches('/')
                ))
                .header("Authorization", format!("DID {}", did))
                .json(&body)
                .send()
                .await?;
            let status = resp.status();
            let text = resp.text().await?;
            if !status.is_success() {
                return Err(format!("import workspace HTTP {}: {}", status, text).into());
            }
            println!("{} workspace imported", "✓".green());
            println!("{text}");
        }
    }
    Ok(())
}
