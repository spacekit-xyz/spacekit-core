//! Anthropic Messages API streaming. POST /v1/messages with stream: true.
//! See https://docs.anthropic.com/en/api/messages-streaming

use crate::config::ProviderEntry;
use crate::providers::{CompletionRequest, ProviderError, ProviderStream};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;

const DEFAULT_BASE: &str = "https://api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub async fn stream_chat(
    client: &Client,
    entry: &ProviderEntry,
    req: CompletionRequest,
) -> Result<ProviderStream, ProviderError> {
    let base = entry
        .base_url
        .as_deref()
        .unwrap_or(DEFAULT_BASE)
        .trim_end_matches('/');
    let url = format!("{}/v1/messages", base);

    // Anthropic: "system" is top-level; messages are only "user" and "assistant".
    let (system, messages) = split_system_messages(&req.messages);

    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens.unwrap_or(4096),
        "messages": messages,
        "stream": true
    });
    if let Some(s) = system {
        body["system"] = serde_json::Value::String(s);
    }

    let res = client
        .post(&url)
        .header("x-api-key", entry.api_key.as_str())
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(ProviderError::Transport)?;

    if !res.status().is_success() {
        let status = res.status();
        return Err(ProviderError::Upstream { status });
    }

    let stream = res.bytes_stream().map(|r| r.map_err(anyhow::Error::from));
    Ok(Box::pin(stream))
}

/// Split messages into optional system (first if role "system") and user/assistant list.
/// Anthropic expects messages with role "user" or "assistant"; we pass system separately.
fn split_system_messages(
    messages: &[crate::providers::ChatMessage],
) -> (Option<String>, Vec<serde_json::Value>) {
    let mut system = None;
    let mut out = Vec::with_capacity(messages.len());
    for m in messages {
        if m.role.eq_ignore_ascii_case("system") && system.is_none() {
            system = Some(m.content.clone());
        } else {
            let role = if m.role.eq_ignore_ascii_case("assistant") {
                "assistant"
            } else {
                "user"
            };
            out.push(json!({ "role": role, "content": m.content }));
        }
    }
    (system, out)
}
