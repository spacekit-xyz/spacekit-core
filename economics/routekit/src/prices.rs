//! Model prices from LiteLLM JSON. Fetched on startup and refreshed every 6h.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::instrument;

const LITELLM_MODE_CHAT: &str = "chat";

/// Per-model entry from LiteLLM (chat/completion models only).
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct ModelPriceEntry {
    #[serde(default)]
    pub input_cost_per_token: f64,
    #[serde(default)]
    pub output_cost_per_token: f64,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub max_input_tokens: Option<u32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub litellm_provider: String,
    #[serde(default)]
    pub supports_function_calling: Option<bool>,
    #[serde(default)]
    pub supports_vision: Option<bool>,
    #[serde(default)]
    pub mode: Option<String>,
}

/// In-memory cache of model id -> price entry. Only chat/completion models.
#[derive(Debug, Clone, Default)]
pub struct ModelPrices {
    pub by_id: HashMap<String, ModelPriceEntry>,
    #[allow(dead_code)]
    pub updated_at: Option<std::time::SystemTime>,
}

impl ModelPrices {
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Look up by exact id, or by prefix (e.g. "claude-sonnet-4" matches "claude-sonnet-4-20250514").
    pub fn get(&self, model_id: &str) -> Option<&ModelPriceEntry> {
        self.by_id.get(model_id).or_else(|| {
            self.by_id
                .iter()
                .find(|(k, _)| k.as_str().starts_with(model_id) || model_id.starts_with(k.as_str()))
                .map(|(_, v)| v)
        })
    }

    /// Cost in USD for given input/output token counts (best-effort from cached entry).
    pub fn estimate_cost_usd(
        &self,
        model_id: &str,
        input_tokens: u64,
        output_tokens: u64,
    ) -> Option<f64> {
        self.get(model_id).map(|e| {
            e.input_cost_per_token * (input_tokens as f64)
                + e.output_cost_per_token * (output_tokens as f64)
        })
    }
}

/// Shared state for the price cache (used by API and refresh task).
pub type SharedPrices = Arc<RwLock<ModelPrices>>;

/// Fetch LiteLLM JSON from `url`, parse, and return only chat/completion models.
#[instrument(skip(client))]
pub async fn fetch_model_prices(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<ModelPrices> {
    let res = client.get(url).send().await?;
    res.error_for_status_ref()?;
    let json: serde_json::Value = res.json().await?;

    let mut by_id = HashMap::new();
    let obj = json
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("expected JSON object"))?;

    for (id, val) in obj {
        if id == "sample_spec" {
            continue;
        }
        let entry: ModelPriceEntry = match serde_json::from_value(val.clone()) {
            Ok(e) => e,
            Err(_) => continue,
        };
        // Only include chat/completion models (have token costs or mode chat).
        let is_chat = entry.mode.as_deref() == Some(LITELLM_MODE_CHAT)
            || (entry.input_cost_per_token > 0.0 || entry.output_cost_per_token > 0.0);
        if is_chat {
            by_id.insert(id.clone(), entry);
        }
    }

    Ok(ModelPrices {
        by_id,
        updated_at: Some(std::time::SystemTime::now()),
    })
}

/// Spawn a background task that refreshes `shared` every `interval_secs` from `url`.
pub fn spawn_price_refresh(
    shared: SharedPrices,
    client: reqwest::Client,
    url: String,
    interval_secs: u64,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
        interval.tick().await; // first tick immediate
        loop {
            match fetch_model_prices(&client, &url).await {
                Ok(prices) => {
                    let mut w = shared.write().await;
                    *w = prices;
                    tracing::info!("model prices refreshed: {} models", w.len());
                }
                Err(e) => {
                    tracing::warn!("model prices refresh failed: {}", e);
                }
            }
            interval.tick().await;
        }
    });
}
