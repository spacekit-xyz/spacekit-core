//! Project scaffolds for `spacekit new`.

use clap::ValueEnum;
use std::collections::HashMap;
use std::io;
use std::path::Path;

#[derive(Clone, Debug, ValueEnum)]
pub enum NewProjectKind {
    /// Smart contract project (Cargo → WASM)
    Contracts,
    /// Growformer agent (WASM + brain data + companion UI)
    Agent,
    /// Static web app (HTML at project root)
    Webapp,
    /// React/Vite web app (`ui/` subdirectory)
    #[value(name = "webapp-react")]
    WebappReact,
    /// DeFi: smart contracts + web UI + standard-library agent integration
    Defi,
}

pub struct ScaffoldContext {
    pub project_name: String,
    pub app_name: String,
    pub env_prefix: String,
    pub did: String,
    pub algorithm: String,
    pub version: String,
    pub networks: HashMap<String, String>,
}

impl ScaffoldContext {
    pub fn new(
        project_name: String,
        app_name: Option<String>,
        did: String,
        algorithm: String,
    ) -> Self {
        let app_name = app_name.unwrap_or_else(|| title_case_slug(&project_name));
        let env_prefix = project_name.to_uppercase().replace('-', "_");
        let mut networks = HashMap::new();
        networks.insert(
            "testnet".to_string(),
            "wss://testnet-rpc.spacekit.xyz".to_string(),
        );
        networks.insert(
            "mainnet".to_string(),
            "wss://mainnet-rpc.spacekit.xyz".to_string(),
        );
        networks.insert("localhost".to_string(), "ws://localhost:9944".to_string());
        Self {
            project_name,
            app_name,
            env_prefix,
            did,
            algorithm,
            version: "1.0.0".to_string(),
            networks,
        }
    }
}

pub fn scaffold_project(
    kind: NewProjectKind,
    project_dir: &Path,
    ctx: &ScaffoldContext,
) -> io::Result<()> {
    if project_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "Project directory already exists: {}",
                project_dir.display()
            ),
        ));
    }

    match kind {
        NewProjectKind::Contracts => scaffold_contracts(project_dir, ctx),
        NewProjectKind::Agent => scaffold_agent(project_dir, ctx),
        NewProjectKind::Webapp => scaffold_webapp_basic(project_dir, ctx),
        NewProjectKind::WebappReact => scaffold_webapp_react(project_dir, ctx),
        NewProjectKind::Defi => scaffold_defi(project_dir, ctx),
    }
}

fn scaffold_contracts(project_dir: &Path, ctx: &ScaffoldContext) -> io::Result<()> {
    write_spacekit_toml(project_dir, ctx)?;
    write_gitignore(project_dir, true, false)?;

    let crate_dir = project_dir.join("contracts");
    std::fs::create_dir_all(crate_dir.join("src"))?;
    std::fs::create_dir_all(project_dir.join("scripts"))?;

    write_file(
        &crate_dir.join("Cargo.toml"),
        &contract_cargo_toml(&ctx.project_name),
    )?;
    write_file(
        &crate_dir.join("src").join("lib.rs"),
        HELLO_WORLD_CONTRACT_RS,
    )?;
    write_executable(
        &project_dir.join("scripts").join("build.sh"),
        &contracts_build_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("package.sh"),
        &contracts_package_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("deploy.sh"),
        &contracts_deploy_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("undeploy.sh"),
        &contracts_undeploy_sh(ctx),
    )?;
    write_file(&project_dir.join("README.md"), &contracts_readme(ctx))?;
    Ok(())
}

fn scaffold_agent(project_dir: &Path, ctx: &ScaffoldContext) -> io::Result<()> {
    write_spacekit_toml(project_dir, ctx)?;
    write_gitignore(project_dir, false, true)?;

    std::fs::create_dir_all(project_dir.join("agent"))?;
    std::fs::create_dir_all(project_dir.join("data"))?;
    std::fs::create_dir_all(project_dir.join("ui"))?;
    std::fs::create_dir_all(project_dir.join("scripts"))?;
    std::fs::create_dir_all(project_dir.join("prompts"))?;

    write_file(
        &project_dir.join(format!("{}.gf.toml", ctx.project_name)),
        &agent_gf_toml(ctx),
    )?;
    write_file(&project_dir.join("deploy.toml"), &agent_deploy_toml(ctx))?;
    write_file(
        &project_dir.join("data").join("inference.toml"),
        AGENT_INFERENCE_TOML,
    )?;
    write_file(
        &project_dir.join("data").join("seed.jsonl"),
        AGENT_SEED_JSONL,
    )?;
    write_file(
        &project_dir.join("data").join("knowledge_graph.toml"),
        AGENT_KNOWLEDGE_GRAPH_TOML,
    )?;
    write_file(
        &project_dir.join("ui").join("companion.html"),
        AGENT_COMPANION_HTML,
    )?;
    write_file(&project_dir.join("agent").join(".gitkeep"), "")?;

    write_executable(
        &project_dir.join("scripts").join("build.sh"),
        &agent_build_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("package.sh"),
        &agent_package_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("deploy.sh"),
        &agent_deploy_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("undeploy.sh"),
        &agent_undeploy_sh(ctx),
    )?;
    write_file(&project_dir.join("README.md"), &agent_readme(ctx))?;
    Ok(())
}

fn scaffold_webapp_basic(project_dir: &Path, ctx: &ScaffoldContext) -> io::Result<()> {
    write_spacekit_toml(project_dir, ctx)?;
    write_gitignore(project_dir, false, false)?;

    std::fs::create_dir_all(project_dir.join("scripts"))?;
    std::fs::create_dir_all(project_dir.join("assets"))?;

    write_file(&project_dir.join("index.html"), &webapp_basic_html(ctx))?;
    write_file(
        &project_dir.join("package.json"),
        &webapp_basic_package_json(ctx),
    )?;
    write_webapp_scripts(project_dir, ctx, WebappLayout::Basic)?;
    write_file(
        &project_dir.join("README.md"),
        &webapp_readme(ctx, "Static HTML web app"),
    )?;
    Ok(())
}

fn scaffold_webapp_react(project_dir: &Path, ctx: &ScaffoldContext) -> io::Result<()> {
    write_spacekit_toml(project_dir, ctx)?;
    write_gitignore(project_dir, false, false)?;

    let ui = project_dir.join("ui");
    std::fs::create_dir_all(ui.join("src"))?;
    std::fs::create_dir_all(project_dir.join("scripts"))?;

    write_file(&ui.join("index.html"), &webapp_react_index_html(ctx))?;
    write_file(&ui.join("package.json"), &webapp_react_package_json(ctx))?;
    write_file(&ui.join("vite.config.ts"), WEBAPP_VITE_CONFIG_TS)?;
    write_file(&ui.join("src").join("main.ts"), WEBAPP_REACT_MAIN_TS)?;
    write_file(&ui.join("src").join("styles.css"), WEBAPP_REACT_STYLES_CSS)?;
    write_webapp_scripts(project_dir, ctx, WebappLayout::React)?;
    write_file(
        &project_dir.join("README.md"),
        &webapp_readme(ctx, "React/Vite web app (`ui/`)"),
    )?;
    Ok(())
}

fn scaffold_defi(project_dir: &Path, ctx: &ScaffoldContext) -> io::Result<()> {
    write_spacekit_toml(project_dir, ctx)?;
    write_gitignore(project_dir, true, true)?;

    let contracts = project_dir.join("contracts");
    std::fs::create_dir_all(contracts.join("src"))?;
    std::fs::create_dir_all(project_dir.join("ui"))?;
    std::fs::create_dir_all(project_dir.join("agent"))?;
    std::fs::create_dir_all(project_dir.join("data"))?;
    std::fs::create_dir_all(project_dir.join("scripts"))?;

    write_file(
        &contracts.join("Cargo.toml"),
        &defi_contract_cargo_toml(&ctx.project_name),
    )?;
    write_file(&contracts.join("src").join("lib.rs"), DEFI_CONTRACT_RS)?;
    write_file(
        &project_dir.join("ui").join("index.html"),
        &defi_ui_html(ctx),
    )?;
    write_file(
        &project_dir.join("ui").join("package.json"),
        &defi_ui_package_json(ctx),
    )?;
    write_file(&project_dir.join("deploy.toml"), &defi_deploy_toml(ctx))?;
    write_file(
        &project_dir.join("data").join("inference.toml"),
        DEFI_INFERENCE_TOML,
    )?;
    write_file(&project_dir.join("agent").join(".gitkeep"), "")?;

    write_executable(
        &project_dir.join("scripts").join("build.sh"),
        &defi_build_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("package.sh"),
        &defi_package_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("deploy.sh"),
        &defi_deploy_sh(ctx),
    )?;
    write_executable(
        &project_dir.join("scripts").join("undeploy.sh"),
        &defi_undeploy_sh(ctx),
    )?;
    write_file(&project_dir.join("README.md"), &defi_readme(ctx))?;
    Ok(())
}

enum WebappLayout {
    Basic,
    React,
}

fn write_webapp_scripts(
    project_dir: &Path,
    ctx: &ScaffoldContext,
    layout: WebappLayout,
) -> io::Result<()> {
    write_executable(
        &project_dir.join("scripts").join("build.sh"),
        &webapp_build_sh(ctx, &layout),
    )?;
    write_executable(
        &project_dir.join("scripts").join("package.sh"),
        &webapp_package_sh(ctx, &layout),
    )?;
    write_executable(
        &project_dir.join("scripts").join("deploy.sh"),
        &webapp_deploy_sh(ctx, &layout),
    )?;
    write_executable(
        &project_dir.join("scripts").join("undeploy.sh"),
        &webapp_undeploy_sh(ctx, &layout),
    )?;
    Ok(())
}

fn write_spacekit_toml(project_dir: &Path, ctx: &ScaffoldContext) -> io::Result<()> {
    let mut lines = vec![
        format!("name = \"{}\"", ctx.project_name),
        format!("version = \"{}\"", ctx.version),
        format!("did = \"{}\"", ctx.did),
        String::new(),
        "[networks]".to_string(),
    ];
    for (net, url) in &ctx.networks {
        lines.push(format!("{} = \"{}\"", net, url));
    }
    lines.push(String::new());
    lines.push("[dependencies]".to_string());
    lines.push("spacekit-primitives = \"latest\"".to_string());
    write_file(&project_dir.join("spacekit.toml"), &lines.join("\n"))?;
    Ok(())
}

fn write_gitignore(project_dir: &Path, rust: bool, agent: bool) -> io::Result<()> {
    let mut lines = vec![
        "node_modules/".to_string(),
        "ui/node_modules/".to_string(),
        "ui/dist/".to_string(),
        "dist/".to_string(),
        "*.spkg".to_string(),
        "*.spkg.files/".to_string(),
        ".DS_Store".to_string(),
        "deploy-receipt.json".to_string(),
    ];
    if rust {
        lines.push("target/".to_string());
        lines.push("contracts/target/".to_string());
        lines.push("*.wasm".to_string());
        lines.push(".deploy-contract-id".to_string());
    }
    if agent {
        lines.push("agent/*.bin".to_string());
        lines.push("capture_artifacts/".to_string());
    }
    write_file(&project_dir.join(".gitignore"), &lines.join("\n"))?;
    Ok(())
}

fn write_file(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

fn write_executable(path: &Path, contents: &str) -> io::Result<()> {
    write_file(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

fn title_case_slug(name: &str) -> String {
    name.split(['-', '_', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn contract_cargo_toml(crate_name: &str) -> String {
    let crate_name = crate_name.replace('-', "_");
    format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2021"
description = "SpaceKit smart contract"

[lib]
crate-type = ["cdylib"]
path = "src/lib.rs"

[dependencies]
spacekit-contract-sdk = {{ git = "https://github.com/spacekit-xyz/spacekit-core" }}
wee_alloc = "0.4"

[profile.release]
panic = "abort"
opt-level = "s"
lto = true

[profile.dev]
panic = "abort"
"#
    )
}

fn defi_contract_cargo_toml(crate_name: &str) -> String {
    contract_cargo_toml(crate_name)
}

const HELLO_WORLD_CONTRACT_RS: &str = r##"//! Hello World — minimal SpaceKit WASM contract.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use spacekit_contract_sdk::{ContractError, SpacekitContract, spacekit_contract};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct HelloWorld;

impl SpacekitContract for HelloWorld {
    type Error = ContractError;

    fn init() -> Self {
        HelloWorld
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Ok(b"hello, world".to_vec());
        }
        Ok(input.to_vec())
    }
}

spacekit_contract!(HelloWorld);
"##;

const DEFI_CONTRACT_RS: &str = r##"//! DeFi vault stub — extend with swaps, lending, or oracles.

#![no_std]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use spacekit_contract_sdk::{
    get_caller_did_string, spacekit_contract, ContractError, SpacekitContract,
};

#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[cfg(all(target_arch = "wasm32", not(test)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

struct DefiVault;

const OP_HEALTH: u8 = 0x10;
const OP_BALANCE: u8 = 0x01;

impl SpacekitContract for DefiVault {
    type Error = ContractError;

    fn init() -> Self {
        DefiVault
    }

    fn handle(&mut self, input: &[u8]) -> Result<Vec<u8>, ContractError> {
        if input.is_empty() {
            return Err(ContractError::InvalidInput);
        }
        match input[0] {
            OP_HEALTH => Ok(format!(
                r#"{{"status":"ok","contract":"defi-vault","caller":"{}"}}"#,
                get_caller_did_string()
            )
            .into_bytes()),
            OP_BALANCE => Ok(format!(
                r#"{{"asset":"AUSD","balance":"0","owner":"{}"}}"#,
                get_caller_did_string()
            )
            .into_bytes()),
            _ => Err(ContractError::InvalidInput),
        }
    }
}

spacekit_contract!(DefiVault);
"##;

fn contracts_build_sh(ctx: &ScaffoldContext) -> String {
    let crate_name = ctx.project_name.replace('-', "_");
    format!(
        r#"#!/usr/bin/env bash
# Build the WASM contract (hello_world.wasm).
set -euo pipefail

ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
CRATE="$ROOT/contracts"

echo "→ Building contract WASM..."
if ! command -v cargo >/dev/null 2>&1; then
  echo "Rust/cargo not found. Install from https://rustup.rs/" >&2
  exit 1
fi
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
(cd "$CRATE" && cargo build --release --target wasm32-unknown-unknown)

WASM="$CRATE/target/wasm32-unknown-unknown/release/{crate_name}.wasm"
if [[ ! -f "$WASM" ]]; then
  WASM="$(ls "$CRATE/target/wasm32-unknown-unknown/release/"*.wasm | head -1)"
fi
cp "$WASM" "$ROOT/hello_world.wasm"
echo "✅ Built $ROOT/hello_world.wasm"
"#
    )
}

fn contracts_package_sh(_ctx: &ScaffoldContext) -> String {
    r#"#!/usr/bin/env bash
# Package step for contracts — WASM artifact is produced by ./scripts/build.sh
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ ! -f "$ROOT/hello_world.wasm" ]]; then
  echo "Run ./scripts/build.sh first." >&2
  exit 1
fi
echo "✅ Contract artifact ready: $ROOT/hello_world.wasm"
"#
    .to_string()
}

fn contracts_deploy_sh(ctx: &ScaffoldContext) -> String {
    format!(
        r#"#!/usr/bin/env bash
# Deploy hello_world.wasm to the local SwtchVM compute node.
set -euo pipefail

ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
WASM="${{CONTRACT_WASM:-$ROOT/hello_world.wasm}}"
NAME="${{CONTRACT_NAME:-hello_world}}"

if [[ ! -f "$WASM" ]]; then
  echo "WASM not found. Run ./scripts/build.sh first." >&2
  exit 1
fi

if ! command -v spacekit >/dev/null 2>&1; then
  echo "spacekit CLI not found." >&2
  exit 1
fi

echo "→ Deploying contract $NAME ..."
spacekit contract deploy --contract "$WASM" --name "$NAME" --owner-did "{did}"
echo "✅ Contract deployed"
"#,
        did = ctx.did
    )
}

fn contracts_undeploy_sh(_ctx: &ScaffoldContext) -> String {
    r#"#!/usr/bin/env bash
# Contracts are pinned by contract id on the compute node — record id after deploy.
set -euo pipefail
echo "ℹ️  Remove deployed contracts with compute-node tooling or redeploy over a fresh chain."
echo "   Save contract id from deploy output for your app integration."
"#
    .to_string()
}

fn agent_gf_toml(ctx: &ScaffoldContext) -> String {
    format!(
        r#"# Growformer project manifest for {name}
schema_version = 1

[project]
name = "{app_name}"
author = "{did}"
description = "SpaceKit agent project scaffold."

[train]
auto = true
data_dir = "data"
brain_output = "agent/brain.bin"
encoder = "clifford_e8"
brain_gen_epochs = 500

[inference]
toml = "data/inference.toml"
topic_graph = "data/knowledge_graph.toml"

[infer]
brain = "agent/brain.bin"
"#,
        name = ctx.project_name,
        app_name = ctx.app_name,
        did = ctx.did
    )
}

fn agent_deploy_toml(ctx: &ScaffoldContext) -> String {
    let slug = ctx.project_name.replace('-', "_");
    let hub_companion_ui = format!("{slug}_companion");
    let tag_color = "#6366f1";
    format!(
        r#"# Agent deploy manifest — `spacekit storage deploy --package deploy.toml`
# Build agent WASM from spacekit-standard-library first (see scripts/build.sh).

[artifacts]
wasm = "${{SPACEKIT_STANDARD_LIBRARY:-../spacekit-standard-library}}/target/wasm32-unknown-unknown/release/spacekit_growformer_agent.wasm"
bin = "agent/brain.bin"
inference_toml = "data/inference.toml"
companion_ui = "ui/companion.html"

[agent]
id = "{name}-agent-001"
owner_did = "{did}"

[project]
gf_toml = "{name}.gf.toml"

[storage]
url = "http://127.0.0.1:3030"

[receipt]
path = "deploy-receipt.json"

[hub]
brain_key = "chat_brain"
capabilities = ["Assistant"]
tag_label = "AGENT"
tag_color = "{tag_color}"
hub_response_format = "plain"
hub_companion_ui = "{hub_companion_ui}"
thinking_label = "Thinking"
op = 1
input_format = "op_string"
inference_toml = true
topic_graph = true

[marketplace]
publish = true
title = "{app_name}"
description = "SpaceKit Growformer agent."
category = "ai"
access = "public"
price = "free"
marketplace_id = "default"
"#,
        name = ctx.project_name,
        hub_companion_ui = hub_companion_ui,
        tag_color = tag_color,
        app_name = ctx.app_name,
        did = ctx.did
    )
}

const AGENT_INFERENCE_TOML: &str = r#"[inference]
temperature = 0.7
max_tokens = 256
"#;

const AGENT_SEED_JSONL: &str = r#"{"input":"Hello","output":"Hi! How can I help?"}
{"input":"Who are you?","output":"I'm your SpaceKit companion agent."}
"#;

const AGENT_KNOWLEDGE_GRAPH_TOML: &str = r#"[topics]
default = "general_chat"

[[routes]]
phrase = "hello"
topic = "general_chat"
"#;

const AGENT_COMPANION_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Companion</title>
  <style>
    body { font-family: system-ui, sans-serif; background: #0f1115; color: #e5e7eb; margin: 0; min-height: 100vh; display: grid; place-items: center; }
    .card { width: min(420px, 92vw); background: #171a21; border: 1px solid #2a2f3a; border-radius: 14px; padding: 20px; }
    h1 { margin: 0 0 8px; font-size: 18px; }
    p { color: #9ca3af; font-size: 13px; }
  </style>
</head>
<body>
  <div class="card">
    <h1>SpaceKit Agent</h1>
    <p>Companion UI shell — wire this in Agent Hub after deploy.</p>
  </div>
</body>
</html>
"#;

fn agent_build_sh(ctx: &ScaffoldContext) -> String {
    format!(
        r#"#!/usr/bin/env bash
# Build agent WASM (standard library) and train brain (growformer).
set -euo pipefail

ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
LIB="${{SPACEKIT_STANDARD_LIBRARY:-$ROOT/../spacekit-standard-library}}"

echo "→ Building spacekit_growformer_agent.wasm ..."
if [[ ! -d "$LIB/agents/spacekit-growformer-agent" ]]; then
  echo "Set SPACEKIT_STANDARD_LIBRARY to your spacekit-standard-library checkout." >&2
  exit 1
fi
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
(cd "$LIB" && cargo build --release -p spacekit-growformer-agent --target wasm32-unknown-unknown)

echo ""
echo "→ Training brain (requires growformer on PATH) ..."
if command -v growformer >/dev/null 2>&1; then
  (cd "$ROOT" && growformer --train --project "{name}.gf.toml")
  echo "✅ Brain written to agent/brain.bin"
else
  echo "⚠️  growformer not found — copy a .bin brain to agent/brain.bin before deploy"
fi
"#,
        name = ctx.project_name
    )
}

fn agent_package_sh(_ctx: &ScaffoldContext) -> String {
    r#"#!/usr/bin/env bash
# Verify agent artifacts before deploy.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIB="${SPACEKIT_STANDARD_LIBRARY:-$ROOT/../spacekit-standard-library}"
WASM="$LIB/target/wasm32-unknown-unknown/release/spacekit_growformer_agent.wasm"
[[ -f "$WASM" ]] || { echo "Missing agent WASM. Run ./scripts/build.sh" >&2; exit 1; }
[[ -f "$ROOT/agent/brain.bin" ]] || { echo "Missing agent/brain.bin. Train or copy a brain." >&2; exit 1; }
echo "✅ Agent artifacts ready"
"#
    .to_string()
}

fn agent_deploy_sh(_ctx: &ScaffoldContext) -> String {
    r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if ! command -v spacekit >/dev/null 2>&1; then
  echo "spacekit CLI not found." >&2
  exit 1
fi
"$ROOT/scripts/package.sh"
spacekit storage deploy --package "$ROOT/deploy.toml"
"#
    .to_string()
}

fn agent_undeploy_sh(ctx: &ScaffoldContext) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
RECEIPT="$ROOT/deploy-receipt.json"
APP_ID=""
if [[ -f "$RECEIPT" ]] && command -v node >/dev/null 2>&1; then
  APP_ID="$(node -e "const r=JSON.parse(require('fs').readFileSync(process.argv[1],'utf8')); console.log((r.agent_id||r.fact_id||'').trim());" "$RECEIPT")"
fi
APP_ID="${{1:-${{{prefix}_APP_ID:-$APP_ID}}}}"
if [[ -z "$APP_ID" ]]; then
  echo "Usage: ./scripts/undeploy.sh <agent-or-fact-id>" >&2
  exit 1
fi
STORAGE_NODE="${{SPACEKIT_STORAGE_NODE_URL:-http://127.0.0.1:3030}}"
spacekit app undeploy "$APP_ID" --storage-node "$STORAGE_NODE" --purge
"#,
        prefix = ctx.env_prefix
    )
}

fn webapp_basic_html(ctx: &ScaffoldContext) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{app_name}</title>
  <style>
    body {{ font-family: system-ui, sans-serif; background: #080b0f; color: #e5e7eb; min-height: 100vh; display: grid; place-items: center; margin: 0; }}
    .card {{ max-width: 480px; padding: 28px; border-radius: 16px; background: rgba(255,255,255,0.03); border: 1px solid rgba(255,255,255,0.08); text-align: center; }}
    h1 {{ margin: 0 0 8px; font-size: 22px; }}
    p {{ color: #9ca3af; font-size: 13px; }}
  </style>
</head>
<body>
  <div class="card">
    <h1>{app_name}</h1>
    <p>SpaceKit web app — package with ./scripts/package.sh and deploy with ./scripts/deploy.sh</p>
  </div>
</body>
</html>
"#,
        app_name = ctx.app_name
    )
}

fn webapp_basic_package_json(ctx: &ScaffoldContext) -> String {
    format!(
        r#"{{
  "name": "{name}",
  "version": "{version}",
  "private": true,
  "description": "{app_name} — SpaceKit web app",
  "scripts": {{
    "preview": "npx serve ."
  }}
}}
"#,
        name = ctx.project_name,
        version = ctx.version,
        app_name = ctx.app_name
    )
}

fn webapp_react_index_html(ctx: &ScaffoldContext) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>{app_name}</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
"#,
        app_name = ctx.app_name
    )
}

fn webapp_react_package_json(ctx: &ScaffoldContext) -> String {
    format!(
        r#"{{
  "name": "{name}",
  "version": "{version}",
  "private": true,
  "type": "module",
  "scripts": {{
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview"
  }},
  "devDependencies": {{
    "vite": "^5.4.2",
    "typescript": "^5.5.3"
  }}
}}
"#,
        name = ctx.project_name,
        version = ctx.version
    )
}

const WEBAPP_VITE_CONFIG_TS: &str = r#"import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: {
      output: { inlineDynamicImports: true },
    },
  },
});
"#;

const WEBAPP_REACT_MAIN_TS: &str = r##"import "./styles.css";

const root = document.getElementById("app");
if (root) {
  root.innerHTML = `
    <main class="shell">
      <h1>SpaceKit Web App</h1>
      <p>Edit ui/src/main.ts and run npm run dev in ui/.</p>
    </main>
  `;
}
"##;

const WEBAPP_REACT_STYLES_CSS: &str = r#":root {
  color: #e5e7eb;
  background: #0b0d12;
  font-family: system-ui, sans-serif;
}
body { margin: 0; min-height: 100vh; display: grid; place-items: center; }
.shell { text-align: center; padding: 2rem; }
code { color: #67e8f9; }
"#;

fn webapp_build_sh(_ctx: &ScaffoldContext, layout: &WebappLayout) -> String {
    match layout {
        WebappLayout::Basic => format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
echo "✅ Static webapp ready at $ROOT/index.html"
"#
        ),
        WebappLayout::React => format!(
            r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
UI="$ROOT/ui"
if [[ ! -x "$UI/node_modules/.bin/vite" ]]; then
  (cd "$UI" && npm install)
fi
(cd "$UI" && npm run build)
echo "✅ Built $UI/dist"
"#
        ),
    }
}

fn webapp_package_sh(ctx: &ScaffoldContext, layout: &WebappLayout) -> String {
    let (source, output) = match layout {
        WebappLayout::Basic => (
            "$ROOT",
            format!("$ROOT/{}-{}.spkg", ctx.project_name, ctx.version),
        ),
        WebappLayout::React => (
            "$UI/dist",
            format!("$UI/{}-{}.spkg", ctx.project_name, ctx.version),
        ),
    };
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
UI="$ROOT/ui"
VERSION="${{{prefix}_VERSION:-{version}}}"
OUTPUT="${{{prefix}_PACKAGE_OUTPUT:-{output}}}"
APP_NAME="${{{prefix}_APP_NAME:-{app_name}}}"

if ! command -v spacekit >/dev/null 2>&1; then
  echo "spacekit CLI not found." >&2
  exit 1
fi

"$ROOT/scripts/build.sh"

spacekit app package {source} \
  --name "$APP_NAME" \
  --entry index.html \
  --version "$VERSION" \
  --description "{app_name} — SpaceKit web app" \
  --category utilities \
  -o "$OUTPUT"

echo "✅ Package ready: $OUTPUT"
"#,
        prefix = ctx.env_prefix,
        version = ctx.version,
        output = output,
        app_name = ctx.app_name,
        source = source
    )
}

fn webapp_deploy_sh(ctx: &ScaffoldContext, layout: &WebappLayout) -> String {
    let package_path = match layout {
        WebappLayout::Basic => format!("$ROOT/{}-{}.spkg", ctx.project_name, ctx.version),
        WebappLayout::React => format!("$UI/{}-{}.spkg", ctx.project_name, ctx.version),
    };
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
UI="$ROOT/ui"
VERSION="${{{prefix}_VERSION:-{version}}}"
PACKAGE="${{{prefix}_PACKAGE:-{package}}}"
STORAGE_NODE="${{SPACEKIT_STORAGE_NODE_URL:-http://127.0.0.1:3030}}"
PUBLISH="${{{prefix}_PUBLISH:-1}}"

if [[ ! -f "$PACKAGE" ]]; then
  echo "Run ./scripts/package.sh first." >&2
  exit 1
fi

APP_ID="$(node -e "
  const fs = require('fs');
  const pkg = JSON.parse(fs.readFileSync(process.argv[1], 'utf8'));
  const id = pkg.app_id;
  if (Array.isArray(id)) console.log(id.map(b => b.toString(16).padStart(2,'0')).join(''));
  else console.log(String(id ?? '').replace(/^0x/, ''));
" "$PACKAGE")"

if [[ -n "$APP_ID" ]] && curl -fsS "${{STORAGE_NODE}}/facts/${{APP_ID}}" >/dev/null 2>&1; then
  "$ROOT/scripts/undeploy.sh" "$APP_ID" || true
fi

ARGS=(spacekit app deploy "$PACKAGE" --storage-node "$STORAGE_NODE")
[[ "$PUBLISH" == "1" ]] && ARGS+=(--publish)
"${{ARGS[@]}}"
"#,
        prefix = ctx.env_prefix,
        version = ctx.version,
        package = package_path
    )
}

fn webapp_undeploy_sh(ctx: &ScaffoldContext, _layout: &WebappLayout) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
APP_ID="${{1:-${{{env_prefix}_APP_ID:-}}}}"
if [[ -z "$APP_ID" ]]; then
  echo "Usage: ./scripts/undeploy.sh <app-id-hex>" >&2
  exit 1
fi
STORAGE_NODE="${{SPACEKIT_STORAGE_NODE_URL:-http://127.0.0.1:3030}}"
spacekit app undeploy "$APP_ID" --storage-node "$STORAGE_NODE" --purge
"#,
        env_prefix = ctx.env_prefix
    )
}

fn defi_ui_html(ctx: &ScaffoldContext) -> String {
    webapp_basic_html(ctx)
}

fn defi_ui_package_json(ctx: &ScaffoldContext) -> String {
    format!(
        r#"{{
  "name": "{name}-ui",
  "version": "{version}",
  "private": true,
  "description": "{app_name} DeFi dashboard"
}}
"#,
        name = ctx.project_name,
        version = ctx.version,
        app_name = ctx.app_name
    )
}

fn defi_deploy_toml(ctx: &ScaffoldContext) -> String {
    let tag_color = "#10b981";
    format!(
        r#"[artifacts]
wasm = "${{SPACEKIT_STANDARD_LIBRARY:-../spacekit-standard-library}}/target/wasm32-unknown-unknown/release/spacekit_growformer_fintech_analysis.wasm"
bin = "agent/brain.bin"
inference_toml = "data/inference.toml"
companion_ui = "ui/index.html"

[agent]
id = "{name}-defi-agent-001"
owner_did = "{did}"

[storage]
url = "http://127.0.0.1:3030"

[receipt]
path = "deploy-receipt.json"

[hub]
brain_key = "financial_brain"
capabilities = ["DeFi", "Analytics"]
tag_label = "DEFI"
tag_color = "{tag_color}"
hub_response_format = "plain"
op = 1
input_format = "op_string"
inference_toml = true

[marketplace]
publish = true
title = "{app_name}"
description = "DeFi dashboard with on-chain vault + analysis agent."
category = "finance"
access = "public"
price = "free"
"#,
        name = ctx.project_name,
        app_name = ctx.app_name,
        did = ctx.did,
        tag_color = tag_color
    )
}

const DEFI_INFERENCE_TOML: &str = r#"[inference]
domain = "finance"
temperature = 0.4
"#;

fn defi_build_sh(ctx: &ScaffoldContext) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
LIB="${{SPACEKIT_STANDARD_LIBRARY:-$ROOT/../spacekit-standard-library}}"

echo "→ Building DeFi contract WASM ..."
rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
(cd "$ROOT/contracts" && cargo build --release --target wasm32-unknown-unknown)
cp "$ROOT/contracts/target/wasm32-unknown-unknown/release/"*.wasm "$ROOT/contracts/vault.wasm"

echo "→ Building fintech agent WASM (optional) ..."
if [[ -d "$LIB/agents/spacekit-growformer-fintech-analysis" ]]; then
  (cd "$LIB" && cargo build --release -p spacekit-growformer-fintech-analysis --target wasm32-unknown-unknown)
fi

echo "✅ DeFi build complete"
"#
    )
}

fn defi_package_sh(ctx: &ScaffoldContext) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
VERSION="${{{prefix}_VERSION:-{version}}}"
OUTPUT="${{{prefix}_PACKAGE_OUTPUT:-$ROOT/ui/{name}-$VERSION.spkg}}"
APP_NAME="${{{prefix}_APP_NAME:-{app_name}}}"

spacekit app package "$ROOT/ui" \
  --name "$APP_NAME" \
  --entry index.html \
  --version "$VERSION" \
  --description "{app_name} DeFi dashboard" \
  --category finance \
  -o "$OUTPUT"
echo "✅ Package ready: $OUTPUT"
"#,
        prefix = ctx.env_prefix,
        version = ctx.version,
        name = ctx.project_name,
        app_name = ctx.app_name
    )
}

fn defi_deploy_sh(ctx: &ScaffoldContext) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${{BASH_SOURCE[0]}}")/.." && pwd)"
STORAGE_NODE="${{SPACEKIT_STORAGE_NODE_URL:-http://127.0.0.1:3030}}"

"$ROOT/scripts/build.sh"
"$ROOT/scripts/package.sh"

echo "→ Deploying vault contract ..."
spacekit contract deploy \
  --contract "$ROOT/contracts/vault.wasm" \
  --name "{name}_vault" \
  --owner-did "{did}"

echo "→ Deploying DeFi agent (if brain present) ..."
if [[ -f "$ROOT/agent/brain.bin" ]]; then
  spacekit storage deploy --package "$ROOT/deploy.toml"
else
  echo "⚠️  Skip agent deploy — add agent/brain.bin or train via growformer"
fi

VERSION="${{{prefix}_VERSION:-{version}}}"
PACKAGE="$ROOT/ui/{name}-$VERSION.spkg"
if [[ -f "$PACKAGE" ]]; then
  spacekit app deploy "$PACKAGE" --storage-node "$STORAGE_NODE" --publish
fi
"#,
        name = ctx.project_name,
        did = ctx.did,
        prefix = ctx.env_prefix,
        version = ctx.version
    )
}

fn defi_undeploy_sh(ctx: &ScaffoldContext) -> String {
    agent_undeploy_sh(ctx)
}

fn contracts_readme(ctx: &ScaffoldContext) -> String {
    readme_header(
        ctx,
        "Smart contract project",
        &[
            "contracts/          Cargo cdylib → hello_world.wasm",
            "scripts/build.sh    cargo build --target wasm32-unknown-unknown",
            "scripts/deploy.sh   spacekit contract deploy",
        ],
    )
}

fn agent_readme(ctx: &ScaffoldContext) -> String {
    let gf = format!(
        "{}.gf.toml       Growformer train/infer config",
        ctx.project_name
    );
    readme_header(
        ctx,
        "Growformer agent project",
        &[
            "agent/              Trained .bin brain output",
            "data/               Training + inference corpora",
            "ui/companion.html   Agent Hub companion UI",
            "deploy.toml         spacekit storage deploy manifest",
            gf.as_str(),
        ],
    )
}

fn webapp_readme(ctx: &ScaffoldContext, kind: &str) -> String {
    readme_header(
        ctx,
        kind,
        &[
            "scripts/build.sh    Build UI (Vite for react variant)",
            "scripts/package.sh  Create .spkg",
            "scripts/deploy.sh   Upload + marketplace publish",
            "scripts/undeploy.sh spacekit app undeploy",
        ],
    )
}

fn defi_readme(ctx: &ScaffoldContext) -> String {
    readme_header(
        ctx,
        "DeFi project (contracts + web UI + standard-library agent)",
        &[
            "contracts/          On-chain vault WASM",
            "ui/                 DeFi dashboard (.spkg)",
            "deploy.toml         Fintech analysis agent manifest",
            "scripts/            build → package → deploy all artifacts",
        ],
    )
}

fn readme_header(ctx: &ScaffoldContext, kind: &str, layout: &[&str]) -> String {
    let tree = layout.join("\n");
    format!(
        r#"# {app_name}

{kind} scaffolded with `spacekit new`.

- **DID**: `{did}`
- **Version**: `{version}`

## Quick start

```bash
./scripts/build.sh
./scripts/package.sh   # webapp / defi UI only
./scripts/deploy.sh
./scripts/undeploy.sh <app-id-hex>   # when published
```

## Layout

```
{tree}
```
"#,
        app_name = ctx.app_name,
        kind = kind,
        did = ctx.did,
        version = ctx.version,
        tree = tree
    )
}

pub fn project_kind_label(kind: &NewProjectKind) -> &'static str {
    match kind {
        NewProjectKind::Contracts => "contracts",
        NewProjectKind::Agent => "agent",
        NewProjectKind::Webapp => "webapp",
        NewProjectKind::WebappReact => "webapp-react",
        NewProjectKind::Defi => "defi",
    }
}

pub fn next_steps(kind: &NewProjectKind, project_name: &str) -> Vec<String> {
    let cd = format!("cd {}", project_name);
    match kind {
        NewProjectKind::Contracts => vec![
            cd,
            "./scripts/build.sh".into(),
            "./scripts/deploy.sh".into(),
        ],
        NewProjectKind::Agent => vec![
            "export SPACEKIT_STANDARD_LIBRARY=/path/to/spacekit-standard-library".into(),
            cd,
            "./scripts/build.sh".into(),
            "./scripts/deploy.sh".into(),
        ],
        NewProjectKind::Webapp | NewProjectKind::WebappReact => vec![
            cd,
            "./scripts/build.sh".into(),
            "./scripts/package.sh".into(),
            "./scripts/deploy.sh".into(),
        ],
        NewProjectKind::Defi => vec![
            "export SPACEKIT_STANDARD_LIBRARY=/path/to/spacekit-standard-library".into(),
            cd,
            "./scripts/build.sh".into(),
            "./scripts/package.sh".into(),
            "./scripts/deploy.sh".into(),
        ],
    }
}
