//! Provider adapters: OpenAI, Anthropic, Mistral. Streaming chat completion.

mod anthropic;
mod mistral;
mod openai;

use crate::config::ProviderEntry;
use crate::router::ProviderKind;
use serde::Serialize;
use std::pin::Pin;

use futures_util::Stream;

/// Unified stream type so all providers can return the same opaque type.
pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, anyhow::Error>> + Send>>;

/// OpenAI-shaped message (used for all providers that accept this format).
#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Request for chat completion (provider-agnostic shape).
#[derive(Clone)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{0} provider is not configured")]
    NotConfigured(&'static str),
    #[error("provider transport failed: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("provider returned HTTP {status}")]
    Upstream { status: reqwest::StatusCode },
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(error) => {
                error.is_connect() || error.is_timeout() || error.is_request()
            }
            Self::Upstream { status, .. } => status.is_server_error(),
            Self::NotConfigured(_) => true,
        }
    }
}

/// Stream a chat completion from the selected provider. Returns a stream of raw bytes (SSE format from provider).
pub async fn stream_completion(
    client: &reqwest::Client,
    provider: ProviderKind,
    req: CompletionRequest,
    config: &ProviderConfigs,
) -> Result<ProviderStream, ProviderError> {
    match provider {
        ProviderKind::OpenAI => {
            let entry = config
                .openai
                .as_ref()
                .ok_or(ProviderError::NotConfigured("OpenAI"))?;
            openai::stream_chat(client, entry, req).await
        }
        ProviderKind::Anthropic => {
            let entry = config
                .anthropic
                .as_ref()
                .ok_or(ProviderError::NotConfigured("Anthropic"))?;
            anthropic::stream_chat(client, entry, req).await
        }
        ProviderKind::Mistral => {
            let entry = config
                .mistral
                .as_ref()
                .ok_or(ProviderError::NotConfigured("Mistral"))?;
            mistral::stream_chat(client, entry, req).await
        }
    }
}

/// Provider config passed to adapters (API key + base URL per provider).
#[derive(Clone, Default)]
pub struct ProviderConfigs {
    pub openai: Option<ProviderEntry>,
    pub anthropic: Option<ProviderEntry>,
    pub mistral: Option<ProviderEntry>,
}

impl From<&crate::config::ProvidersConfig> for ProviderConfigs {
    fn from(c: &crate::config::ProvidersConfig) -> Self {
        Self {
            openai: c.openai.clone(),
            anthropic: c.anthropic.clone(),
            mistral: c.mistral.clone(),
        }
    }
}
