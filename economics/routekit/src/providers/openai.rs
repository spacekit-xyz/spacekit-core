//! OpenAI-compatible chat completion streaming (also works with OpenAI, Azure, LiteLLM proxy).

use crate::config::ProviderEntry;
use crate::providers::{CompletionRequest, ProviderError, ProviderStream};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;

const DEFAULT_BASE: &str = "https://api.openai.com/v1";

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
    let url = format!("{}/chat/completions", base);

    let body = json!({
        "model": req.model,
        "messages": req.messages.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect::<Vec<_>>(),
        "stream": true,
        "max_tokens": req.max_tokens.unwrap_or(4096)
    });

    let res = client
        .post(&url)
        .bearer_auth(entry.api_key.as_str())
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
