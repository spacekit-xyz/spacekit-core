//! Link local CLI Kyber keys to a spacekit.xyz username (`did:spacekit:user:…`).

use super::{
    load_cli_config, load_cli_config_sync, save_cli_config, CLIConfig, CliContext,
    WebsiteAuthConfig,
};
use colored::Colorize;

#[derive(Debug, clap::Subcommand)]
pub enum IdentityCommands {
    /// Sign in via email magic link (saves session to ~/.spacekit/config.toml)
    Login {
        /// Website username (e.g. astor)
        #[arg(long)]
        username: String,
        /// Recovery email for this account
        #[arg(long)]
        email: String,
        /// Paste token from the magic-link URL (?token=…) instead of prompting
        #[arg(long)]
        token: Option<String>,
        /// Website API base URL
        #[arg(long)]
        api_url: Option<String>,
    },
    /// Link local Kyber public key to your website username (requires login)
    Link {
        /// Website username to link (must match signed-in session)
        #[arg(long)]
        username: String,
        /// Website API base URL
        #[arg(long)]
        api_url: Option<String>,
    },
    /// Show linked website identity and session status
    Status,
    /// Sign out (clear saved website session)
    Logout,
}

pub async fn handle_identity_command(
    cmd: &IdentityCommands,
) -> Result<(), Box<dyn std::error::Error>> {
    match cmd {
        IdentityCommands::Login {
            username,
            email,
            token,
            api_url,
        } => handle_identity_login(username, email, token.as_deref(), api_url.as_deref()).await,
        IdentityCommands::Link { username, api_url } => {
            handle_identity_link(username, api_url.as_deref()).await
        }
        IdentityCommands::Status => handle_identity_status(),
        IdentityCommands::Logout => handle_identity_logout().await,
    }
}

pub fn resolve_website_api_url(explicit: Option<&str>) -> String {
    if let Some(u) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return u.trim_end_matches('/').to_string();
    }
    std::env::var("SPACEKIT_WEBSITE_API_URL")
        .or_else(|_| std::env::var("VITE_API_URL"))
        .unwrap_or_else(|_| "https://api.spacekit.xyz".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn read_public_key_hex(ctx: &CliContext) -> Result<String, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(&ctx.public_key_path)?;
    let hex = raw.trim().to_string();
    if hex.is_empty() {
        return Err("Public key file is empty — run `spacekit init` first".into());
    }
    Ok(hex)
}

async fn handle_identity_login(
    username: &str,
    email: &str,
    token: Option<&str>,
    api_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let api = resolve_website_api_url(api_url);
    let slug = username.trim().to_lowercase();
    if slug.len() < 3 {
        return Err("Username must be at least 3 characters".into());
    }
    if !email.contains('@') {
        return Err("Valid --email required".into());
    }

    let client = reqwest::Client::new();
    let session_token = if let Some(t) = token.map(str::trim).filter(|s| !s.is_empty()) {
        t.to_string()
    } else {
        println!("Sending sign-in link to {} …", email.cyan());
        let send_url = format!("{api}/api/auth/email/send-link");
        let resp = client
            .post(&send_url)
            .json(&serde_json::json!({
                "email": email.trim(),
                "username": slug,
                "purpose": "login",
            }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let detail = resp.text().await.unwrap_or_default();
            return Err(format!("Failed to send magic link: {detail}").into());
        }
        println!("{}", "Check your email for the sign-in link.".green());
        println!("Open the link, copy the token= value from the URL, then re-run:");
        println!(
            "  {}",
            format!("spacekit login --username {slug} --email {email} --token <TOKEN>").yellow()
        );
        return Ok(());
    };

    let verify_url = format!("{api}/api/auth/email/verify");
    let resp = client
        .post(&verify_url)
        .json(&serde_json::json!({ "token": session_token }))
        .send()
        .await?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Verification failed: {}",
            body.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        )
        .into());
    }

    let did = body
        .get("did")
        .and_then(|v| v.as_str())
        .ok_or("API did not return did")?;
    let expected = format!("did:spacekit:user:{slug}");
    if did != expected {
        return Err(format!(
            "Session DID {did} does not match username {slug} (expected {expected})"
        )
        .into());
    }
    let session_token = body
        .get("session_token")
        .and_then(|v| v.as_str())
        .ok_or("API did not return session_token")?;

    let mut config = load_cli_config().await?;
    config.identity.website_auth = Some(WebsiteAuthConfig {
        api_url: api.clone(),
        session_token: session_token.to_string(),
        method: "email".to_string(),
    });
    save_cli_config(&config).await?;

    println!("{}", "Signed in to spacekit.xyz".green());
    println!("  DID:     {}", did.cyan());
    println!(
        "  Next:    {}",
        format!("spacekit identity link --username {slug}").yellow()
    );
    Ok(())
}

async fn handle_identity_link(
    username: &str,
    api_url: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ctx = CliContext::load_sync()?;
    let api = resolve_website_api_url(api_url);
    let slug = username.trim().to_lowercase();
    let expected_did = format!("did:spacekit:user:{slug}");

    let mut config = load_cli_config_sync()?;
    let auth = config
        .identity
        .website_auth
        .as_ref()
        .ok_or("Not signed in — run `spacekit login` first")?;
    if auth.api_url.trim_end_matches('/') != api {
        println!(
            "{}",
            format!("Note: session is for {}; linking via {api}", auth.api_url).yellow()
        );
    }

    let client = reqwest::Client::new();
    let session_resp = client
        .get(format!("{api}/api/auth/session"))
        .header("Authorization", format!("Bearer {}", auth.session_token))
        .send()
        .await?;
    if !session_resp.status().is_success() {
        return Err("Session expired — run `spacekit login` again".into());
    }
    let session_body: serde_json::Value = session_resp.json().await.unwrap_or_default();
    let session_did = session_body
        .get("did")
        .and_then(|v| v.as_str())
        .ok_or("Invalid session response")?;
    if session_did != expected_did {
        return Err(format!("Signed in as {session_did}, cannot link username {slug}").into());
    }

    let kyber_pk = read_public_key_hex(&ctx)?;
    let link_url = format!("{api}/api/did/link-kyber");
    let resp = client
        .post(&link_url)
        .header("Authorization", format!("Bearer {}", auth.session_token))
        .json(&serde_json::json!({
            "username": slug,
            "kyber_public_key": kyber_pk,
        }))
        .send()
        .await?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Link failed: {}",
            body.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
        )
        .into());
    }

    config.identity.did = expected_did.clone();
    config.identity.linked_username = Some(slug.clone());
    save_cli_config(&config).await?;

    println!("{}", "Identity linked".green());
    println!("  DID:     {}", expected_did.cyan());
    println!(
        "  Kyber:   {}…{}",
        &kyber_pk[..8.min(kyber_pk.len())],
        &kyber_pk[kyber_pk.len().saturating_sub(6)..]
    );
    println!("  Config:  ~/.spacekit/config.toml");
    Ok(())
}

fn handle_identity_status() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = CliContext::load_sync()?;
    let config = load_cli_config_sync()?;
    println!("Local CLI identity");
    println!("  Effective DID: {}", ctx.did.green());
    println!("  Public key:  {}", ctx.public_key_path.display());
    if let Some(u) = &config.identity.linked_username {
        println!("  Linked user: {}", u.cyan());
    } else {
        println!(
            "  Linked user: {}",
            "(none — run `spacekit identity link`)".yellow()
        );
    }
    if let Some(auth) = &config.identity.website_auth {
        println!("Website session");
        println!("  API:     {}", auth.api_url);
        println!("  Method:  {}", auth.method);
        println!(
            "  Token:   {}…",
            &auth.session_token[..auth.session_token.len().min(12)]
        );
    } else {
        println!("Website session: {}", "not signed in".yellow());
    }
    Ok(())
}

async fn handle_identity_logout() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = load_cli_config().await?;
    if let Some(auth) = config.identity.website_auth.take() {
        let client = reqwest::Client::new();
        let _ = client
            .post(format!(
                "{}/api/auth/session/logout",
                auth.api_url.trim_end_matches('/')
            ))
            .header("Authorization", format!("Bearer {}", auth.session_token))
            .send()
            .await;
    }
    save_cli_config(&config).await?;
    println!(
        "{}",
        "Signed out of spacekit.xyz (local session cleared)".green()
    );
    Ok(())
}

/// Bearer token for website-api calls (repo push, etc.)
pub fn website_session_token(config: &CLIConfig) -> Option<String> {
    config
        .identity
        .website_auth
        .as_ref()
        .map(|a| a.session_token.clone())
}
