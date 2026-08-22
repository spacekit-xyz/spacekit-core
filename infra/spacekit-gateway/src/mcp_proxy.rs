//! Stdio-based MCP backend proxy.
//!
//! Each backend is a child process that speaks JSON-RPC 2.0 over newline-
//! delimited stdin/stdout.  The proxy sends requests and correlates responses
//! by `id`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

// ── Wire types (mirrored from compute/storage MCP) ──────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDescriptor {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub version: u32,
}

#[derive(Debug, Serialize)]
pub struct McpRequest {
    pub jsonrpc: &'static str,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

// ── Backend ─────────────────────────────────────────────────────────────

pub struct StdioBackend {
    pub name: String,
    sender: mpsc::Sender<(serde_json::Value, oneshot::Sender<McpResponse>)>,
    next_id: AtomicU64,
}

impl StdioBackend {
    /// Spawn a child process and start the reader task.
    pub async fn spawn(name: &str, cmd: &[String]) -> Result<Self> {
        if cmd.is_empty() {
            anyhow::bail!("empty command for backend {}", name);
        }

        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .with_context(|| format!("failed to spawn backend {}: {:?}", name, cmd))?;

        let child_stdin = child.stdin.take().expect("stdin piped");
        let child_stdout = child.stdout.take().expect("stdout piped");

        let (tx, rx) = mpsc::channel::<(serde_json::Value, oneshot::Sender<McpResponse>)>(64);

        // Writer + reader coroutine
        tokio::spawn(backend_io_loop(
            name.to_string(),
            child_stdin,
            child_stdout,
            rx,
        ));

        // Keep child alive (drop guard)
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

        Ok(Self {
            name: name.to_string(),
            sender: tx,
            next_id: AtomicU64::new(1),
        })
    }

    /// Send a JSON-RPC request and wait for the response.
    pub async fn call(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<McpResponse> {
        let id = serde_json::Value::Number(self.next_id.fetch_add(1, Ordering::Relaxed).into());
        let req = McpRequest {
            jsonrpc: "2.0",
            id: id.clone(),
            method: method.to_string(),
            params,
        };
        let payload = serde_json::to_value(&req)?;
        let (resp_tx, resp_rx) = oneshot::channel();
        self.sender
            .send((payload, resp_tx))
            .await
            .map_err(|_| anyhow::anyhow!("backend {} channel closed", self.name))?;
        resp_rx
            .await
            .map_err(|_| anyhow::anyhow!("backend {} dropped response", self.name))
    }
}

async fn backend_io_loop(
    name: String,
    mut stdin: tokio::process::ChildStdin,
    stdout: tokio::process::ChildStdout,
    mut rx: mpsc::Receiver<(serde_json::Value, oneshot::Sender<McpResponse>)>,
) {
    let pending: std::sync::Arc<Mutex<HashMap<String, oneshot::Sender<McpResponse>>>> =
        std::sync::Arc::new(Mutex::new(HashMap::new()));
    let pending_clone = pending.clone();

    // Reader task
    let reader_name = name.clone();
    let reader = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<McpResponse>(&line) {
                Ok(resp) => {
                    let id_key = resp.id.to_string();
                    let mut map = pending_clone.lock().await;
                    if let Some(tx) = map.remove(&id_key) {
                        let _ = tx.send(resp);
                    }
                }
                Err(e) => {
                    tracing::warn!("backend {} unparseable response: {}", reader_name, e);
                }
            }
        }
    });

    // Writer loop
    while let Some((req_val, resp_tx)) = rx.recv().await {
        let id_key = req_val.get("id").map(|v| v.to_string()).unwrap_or_default();
        {
            let mut map = pending.lock().await;
            map.insert(id_key, resp_tx);
        }
        let mut bytes = serde_json::to_vec(&req_val).unwrap_or_default();
        bytes.push(b'\n');
        if stdin.write_all(&bytes).await.is_err() {
            tracing::error!("backend {} stdin write failed", name);
            break;
        }
        if stdin.flush().await.is_err() {
            break;
        }
    }

    reader.abort();
}
