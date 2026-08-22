//! RouteKit relay — AI model routing and intent forwarding.
//!
//! One endpoint, every model, automatic failover.
//! Task routing aligns with spacekit-agent-microgpt (chat, search, summarize, classify, code_review, analyze).

mod api;
mod auth;
mod config;
mod cost_tracker;
#[cfg(feature = "intent")]
mod intent;
mod prices;
mod providers;
mod router;
mod storage_client;
#[cfg(feature = "vault")]
mod vault_relay;

use std::path::Path;
use std::time::Instant;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(env_filter)
        .init();

    let started_at = Instant::now();

    // Config: optional ROUTEKIT_CONFIG or config/providers.yaml
    let config_path = std::env::var("ROUTEKIT_CONFIG")
        .ok()
        .map(|s| Path::new(&s).to_path_buf())
        .or_else(|| {
            let p = Path::new("config/providers.yaml");
            if p.exists() {
                Some(p.to_path_buf())
            } else {
                None
            }
        });
    let config = config::Config::load(config_path.as_deref())?;
    tracing::info!(
        "config loaded: relay {}:{}",
        config.relay.host,
        config.relay.port
    );

    // Model prices: fetch once, then refresh every 6h
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(120))
        .user_agent(format!("routekit/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let shared_prices = prices::SharedPrices::default();
    match prices::fetch_model_prices(&client, &config.prices.cost_map_url).await {
        Ok(p) => {
            let mut w = shared_prices.write().await;
            *w = p;
            tracing::info!("model prices loaded: {} models", w.len());
        }
        Err(e) => {
            tracing::warn!(
                "initial model prices fetch failed: {} (will retry in background)",
                e
            );
        }
    }
    prices::spawn_price_refresh(
        shared_prices.clone(),
        client.clone(),
        config.prices.cost_map_url.clone(),
        config.prices.refresh_interval_secs,
    );

    let storage = config.storage.url.as_ref().map(|url| {
        storage_client::StorageClient::new(client.clone(), url, &config.storage.operator_did)
    });
    if let Some(storage) = &storage {
        if let Err(error) = storage.health().await {
            if config.storage.required {
                return Err(anyhow::anyhow!(
                    "SpaceKit Storage Node is required but unavailable: {error}"
                ));
            }
            tracing::warn!(error = %error, "Storage Node unavailable; readiness will remain false");
        }
    }

    let auth = std::sync::Arc::new(auth::AuthService::new(
        storage.clone(),
        &config.safety.bootstrap_keys,
        std::time::Duration::from_secs(config.safety.auth_cache_ttl_secs),
        config.safety.rate_limit_rpm,
    ));
    if !auth.is_configured() {
        return Err(anyhow::anyhow!("RouteKit has no authentication source"));
    }

    let app_state = api::AppState {
        prices: shared_prices,
        started_at,
        providers: config.providers.clone(),
        cost_tracker: cost_tracker::CostTracker::shared(),
        http_client: client,
        safety: config.safety.clone(),
        storage,
        storage_required: config.storage.required,
        auth,
        stream_semaphore: std::sync::Arc::new(tokio::sync::Semaphore::new(
            config.safety.max_concurrent_streams,
        )),
    };
    let metrics_addr =
        std::env::var("ROUTEKIT_METRICS_ADDR").unwrap_or_else(|_| "127.0.0.1:9091".to_string());
    let metrics_listener = tokio::net::TcpListener::bind(&metrics_addr).await?;
    let metrics_state = app_state.clone();
    tokio::spawn(async move {
        tracing::info!("RouteKit internal metrics listening on http://{metrics_addr}");
        if let Err(error) = axum::serve(metrics_listener, api::internal_router(metrics_state)).await
        {
            tracing::error!(error = %error, "internal metrics server stopped");
        }
    });

    let listener =
        tokio::net::TcpListener::bind(format!("{}:{}", config.relay.host, config.relay.port))
            .await?;
    let addr = listener.local_addr()?;
    tracing::info!("RouteKit relay listening on http://{}", addr);

    axum::serve(listener, api::router(app_state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %error, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(error = %error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
