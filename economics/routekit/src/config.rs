//! RouteKit configuration: provider keys (BYOK), relay settings, model price URL.

use serde::Deserialize;
use std::path::Path;

/// Root config loaded from YAML (e.g. providers.yaml) and env.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub relay: RelayConfig,
    pub prices: PricesConfig,
    pub providers: ProvidersConfig,
    pub safety: SafetyConfig,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub host: String,
    pub port: u16,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3001,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SafetyConfig {
    pub cors_origins: Vec<String>,
    pub max_body_bytes: usize,
    pub max_messages: usize,
    pub max_message_bytes: usize,
    pub max_concurrent_streams: usize,
    pub max_output_tokens: u32,
    pub max_failover_attempts: usize,
    pub stream_idle_timeout_secs: u64,
    pub rate_limit_rpm: u32,
    pub auth_cache_ttl_secs: u64,
    pub bootstrap_keys: Vec<String>,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            cors_origins: Vec::new(),
            max_body_bytes: 256 * 1024,
            max_messages: 50,
            max_message_bytes: 32 * 1024,
            max_concurrent_streams: 32,
            max_output_tokens: 4096,
            max_failover_attempts: 3,
            stream_idle_timeout_secs: 30,
            rate_limit_rpm: 60,
            auth_cache_ttl_secs: 60,
            bootstrap_keys: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub url: Option<String>,
    pub operator_did: String,
    pub required: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            url: None,
            operator_did: "did:spacekit:service:routekit".to_string(),
            required: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PricesConfig {
    /// URL for model prices (LiteLLM JSON). Refreshed on startup and every 6h.
    pub cost_map_url: String,
    /// Refresh interval in seconds (default 6h = 21600).
    pub refresh_interval_secs: u64,
}

impl Default for PricesConfig {
    fn default() -> Self {
        Self {
            cost_map_url: "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json".to_string(),
            refresh_interval_secs: 6 * 3600,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProvidersConfig {
    pub openai: Option<ProviderEntry>,
    pub anthropic: Option<ProviderEntry>,
    pub mistral: Option<ProviderEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderEntry {
    pub api_key: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

/// YAML file shape for providers (BYOK). Keys are env var names; values can be "${VAR}".
#[derive(Debug, Deserialize)]
pub struct ProvidersFile {
    #[serde(default)]
    pub providers: ProvidersConfigFile,
}

#[derive(Debug, Deserialize, Default)]
pub struct ProvidersConfigFile {
    pub openai: Option<ProviderEntry>,
    pub anthropic: Option<ProviderEntry>,
    pub mistral: Option<ProviderEntry>,
}

impl Config {
    /// Load config from optional YAML path and env. Provider keys can be in YAML or env (OPENAI_API_KEY etc).
    pub fn load(providers_yaml_path: Option<&Path>) -> anyhow::Result<Self> {
        let relay = RelayConfig {
            host: std::env::var("ROUTEKIT_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("ROUTEKIT_PORT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3001),
        };

        let prices = PricesConfig {
            cost_map_url: std::env::var("ROUTEKIT_COST_MAP_URL").unwrap_or_else(|_| {
                "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json".to_string()
            }),
            refresh_interval_secs: std::env::var("ROUTEKIT_PRICES_REFRESH_SECS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(6 * 3600),
        };

        let providers = if let Some(p) = providers_yaml_path {
            if p.exists() {
                let s = std::fs::read_to_string(p)?;
                let file: ProvidersFile = serde_yaml::from_str(&s)?;
                resolve_provider_keys(file.providers)
            } else {
                tracing::warn!("Providers file not found at {:?}, using env only", p);
                load_providers_from_env()
            }
        } else {
            load_providers_from_env()
        };

        let safety = SafetyConfig {
            cors_origins: csv_env("ROUTEKIT_CORS_ORIGINS"),
            max_body_bytes: usize_env("ROUTEKIT_MAX_BODY_BYTES", 256 * 1024),
            max_messages: usize_env("ROUTEKIT_MAX_MESSAGES", 50),
            max_message_bytes: usize_env("ROUTEKIT_MAX_MESSAGE_BYTES", 32 * 1024),
            max_concurrent_streams: usize_env("ROUTEKIT_MAX_CONCURRENT_STREAMS", 32),
            max_output_tokens: u32_env("ROUTEKIT_MAX_OUTPUT_TOKENS", 4096),
            max_failover_attempts: usize_env("ROUTEKIT_MAX_FAILOVER_ATTEMPTS", 3),
            stream_idle_timeout_secs: u64_env("ROUTEKIT_STREAM_IDLE_TIMEOUT_SECS", 30),
            rate_limit_rpm: u32_env("ROUTEKIT_RATE_LIMIT_RPM", 60),
            auth_cache_ttl_secs: u64_env("ROUTEKIT_AUTH_CACHE_TTL_SECS", 60),
            bootstrap_keys: csv_env("ROUTEKIT_BOOTSTRAP_KEYS"),
        };

        let storage = StorageConfig {
            url: std::env::var("ROUTEKIT_STORAGE_URL").ok(),
            operator_did: std::env::var("ROUTEKIT_OPERATOR_DID")
                .unwrap_or_else(|_| "did:spacekit:service:routekit".to_string()),
            required: bool_env("ROUTEKIT_STORAGE_REQUIRED", true),
        };

        if storage.required && storage.url.is_none() {
            anyhow::bail!("ROUTEKIT_STORAGE_URL is required; set ROUTEKIT_STORAGE_REQUIRED=false only for isolated development");
        }
        if storage.url.is_none() && safety.bootstrap_keys.is_empty() {
            anyhow::bail!("no authentication source configured: set ROUTEKIT_STORAGE_URL or ROUTEKIT_BOOTSTRAP_KEYS");
        }

        Ok(Self {
            relay,
            prices,
            providers,
            safety,
            storage,
        })
    }
}

fn csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .ok()
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn bool_env(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(default)
}

fn usize_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn u32_env(name: &str, default: u32) -> u32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn u64_env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn resolve_provider_keys(file: ProvidersConfigFile) -> ProvidersConfig {
    ProvidersConfig {
        openai: file.openai.map(|e| ProviderEntry {
            api_key: resolve_env(&e.api_key),
            base_url: e.base_url.as_deref().map(resolve_env),
            models: e.models,
        }),
        anthropic: file.anthropic.map(|e| ProviderEntry {
            api_key: resolve_env(&e.api_key),
            base_url: e.base_url.as_deref().map(resolve_env),
            models: e.models,
        }),
        mistral: file.mistral.map(|e| ProviderEntry {
            api_key: resolve_env(&e.api_key),
            base_url: e.base_url.as_deref().map(resolve_env),
            models: e.models,
        }),
    }
}

fn resolve_env(v: &str) -> String {
    let v = v.trim();
    if v.starts_with("${") && v.ends_with('}') {
        let name = &v[2..v.len() - 1];
        std::env::var(name).unwrap_or_default()
    } else {
        v.to_string()
    }
}

fn load_providers_from_env() -> ProvidersConfig {
    ProvidersConfig {
        openai: std::env::var("OPENAI_API_KEY")
            .ok()
            .map(|api_key| ProviderEntry {
                api_key,
                base_url: None,
                models: vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()],
            }),
        anthropic: std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .map(|api_key| ProviderEntry {
                api_key,
                base_url: None,
                models: vec![
                    "claude-sonnet-4-20250514".to_string(),
                    "claude-3-5-haiku-20241022".to_string(),
                ],
            }),
        mistral: std::env::var("MISTRAL_API_KEY")
            .ok()
            .map(|api_key| ProviderEntry {
                api_key,
                base_url: None,
                models: vec![
                    "mistral-large-latest".to_string(),
                    "mistral-small-latest".to_string(),
                ],
            }),
    }
}
