use anyhow::Result;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use warp::Filter;

use spacekit_gateway::catalog::{self, BackendId, MergedCatalog};
use spacekit_gateway::mcp_proxy::{McpResponse, StdioBackend};
use spacekit_gateway::GatewayConfig;

#[derive(Parser, Debug)]
#[command(
    name = "spacekit-gateway",
    version,
    about = "SpaceKit MCP Gateway Aggregator"
)]
struct Cli {
    #[arg(long, default_value = "8080")]
    port: u16,

    #[arg(long, help = "Storage-node MCP command (space-separated)")]
    storage_cmd: Option<String>,

    #[arg(long, help = "Compute-node MCP command (space-separated)")]
    compute_cmd: Option<String>,

    #[arg(long, help = "Also serve MCP on own stdin/stdout")]
    enable_stdio: bool,
}

struct GatewayState {
    storage: StdioBackend,
    compute: StdioBackend,
    catalog: MergedCatalog,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let config = build_config(&cli);

    tracing::info!("Starting SpaceKit MCP Gateway on port {}", config.http_port);

    let storage = StdioBackend::spawn("storage", &config.storage_mcp_cmd).await?;
    let compute = StdioBackend::spawn("compute", &config.compute_mcp_cmd).await?;

    // Initialize backends
    let _ = storage
        .call(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "spacekit-gateway", "version": env!("CARGO_PKG_VERSION")}
            })),
        )
        .await?;
    let _ = compute
        .call(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "spacekit-gateway", "version": env!("CARGO_PKG_VERSION")}
            })),
        )
        .await?;

    let merged = catalog::build_catalog(&storage, &compute).await?;
    tracing::info!(
        "Merged catalog: {} tools ({} storage, {} compute)",
        merged.tools.len(),
        merged
            .tools
            .iter()
            .filter(|t| matches!(t.backend, BackendId::Storage))
            .count(),
        merged
            .tools
            .iter()
            .filter(|t| matches!(t.backend, BackendId::Compute))
            .count(),
    );

    let state = Arc::new(GatewayState {
        storage,
        compute,
        catalog: merged,
    });

    let health = warp::path("health")
        .and(warp::get())
        .map(|| warp::reply::json(&serde_json::json!({"status": "ok"})));

    let state_filter = {
        let s = state.clone();
        warp::any().map(move || s.clone())
    };

    let mcp = warp::path("mcp")
        .and(warp::post())
        .and(warp::body::json())
        .and(state_filter)
        .and_then(handle_mcp);

    let routes = health.or(mcp);

    if config.enable_stdio {
        let stdio_state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = run_stdio_gateway(stdio_state).await {
                tracing::error!("stdio gateway error: {}", e);
            }
        });
    }

    warp::serve(routes)
        .run(([0, 0, 0, 0], config.http_port))
        .await;

    Ok(())
}

fn build_config(cli: &Cli) -> GatewayConfig {
    let mut config = GatewayConfig::default();
    config.http_port = cli.port;
    config.enable_stdio = cli.enable_stdio;
    if let Some(ref cmd) = cli.storage_cmd {
        config.storage_mcp_cmd = cmd.split_whitespace().map(String::from).collect();
    }
    if let Some(ref cmd) = cli.compute_cmd {
        config.compute_mcp_cmd = cmd.split_whitespace().map(String::from).collect();
    }
    config
}

async fn handle_mcp(
    body: serde_json::Value,
    state: Arc<GatewayState>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let method = body.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = body.get("id").cloned().unwrap_or(serde_json::Value::Null);

    let result = match method {
        "initialize" => Ok(serde_json::json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {"listChanged": true}},
            "serverInfo": {"name": "spacekit-gateway", "version": env!("CARGO_PKG_VERSION")},
        })),
        "tools/list" => {
            let descriptors: Vec<_> = state.catalog.tools.iter().map(|e| &e.descriptor).collect();
            Ok(serde_json::json!({"tools": descriptors}))
        }
        "tools/call" => {
            let params = body
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            proxy_tool_call(&state, params).await
        }
        "ping" => Ok(serde_json::json!({})),
        _ => Err(serde_json::json!({
            "code": -32601,
            "message": format!("Method not found: {}", method)
        })),
    };

    let response = match result {
        Ok(value) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": value,
        }),
        Err(error) => serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": error,
        }),
    };

    Ok(warp::reply::json(&response))
}

async fn proxy_tool_call(
    state: &GatewayState,
    params: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

    let backend_id = state.catalog.routing.get(tool_name);

    let backend = match backend_id {
        Some(BackendId::Storage) => &state.storage,
        Some(BackendId::Compute) => &state.compute,
        None => {
            return Err(serde_json::json!({
                "code": -32601,
                "message": format!("Unknown tool: {}", tool_name)
            }));
        }
    };

    let resp = backend
        .call("tools/call", Some(params))
        .await
        .map_err(|e| serde_json::json!({"code": -32603, "message": e.to_string()}))?;

    match resp.error {
        Some(e) => Err(e),
        None => resp
            .result
            .ok_or_else(|| serde_json::json!({"code": -32603, "message": "empty response"})),
    }
}

async fn run_stdio_gateway(state: Arc<GatewayState>) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let body: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32700, "message": format!("Parse error: {}", e)}
                });
                let bytes = serde_json::to_vec(&err)?;
                stdout.write_all(&bytes).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };

        let method = body.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let id = body.get("id").cloned().unwrap_or(serde_json::Value::Null);

        let result = match method {
            "initialize" => Ok(serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "spacekit-gateway", "version": env!("CARGO_PKG_VERSION")},
            })),
            "tools/list" => {
                let descriptors: Vec<_> =
                    state.catalog.tools.iter().map(|e| &e.descriptor).collect();
                Ok(serde_json::json!({"tools": descriptors}))
            }
            "tools/call" => {
                let params = body
                    .get("params")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                proxy_tool_call(&state, params).await
            }
            "ping" => Ok(serde_json::json!({})),
            _ => Err(serde_json::json!({
                "code": -32601,
                "message": format!("Method not found: {}", method)
            })),
        };

        let response = match result {
            Ok(value) => serde_json::json!({"jsonrpc": "2.0", "id": id, "result": value}),
            Err(error) => serde_json::json!({"jsonrpc": "2.0", "id": id, "error": error}),
        };

        let bytes = serde_json::to_vec(&response)?;
        stdout.write_all(&bytes).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }
    Ok(())
}
