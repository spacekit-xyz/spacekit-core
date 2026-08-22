use std::net::SocketAddr;

use axum::{routing::get, Json, Router};
use clap::Parser;
use spacekit_keymaster::types::GuardianInfo;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "8770")]
    port: u16,
    #[arg(long, env = "KEYMASTER_COORDINATOR_URL", default_value = "http://127.0.0.1:8780")]
    coordinator_url: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let args = Args::parse();

    let coordinator_url = args.coordinator_url.clone();
    let app = Router::new().route(
        "/v1/guardians",
        get(move || {
            let url = coordinator_url.clone();
            async move {
                let list: Vec<GuardianInfo> = match reqwest::get(format!("{url}/v1/coordinator/guardians")).await {
                    Ok(r) => r.json().await.unwrap_or_default(),
                    Err(_) => Vec::new(),
                };
                Json(list)
            }
        }),
    );

    let addr = SocketAddr::from(([127, 0, 0, 1], args.port));
    tracing::info!("SKKM registry proxy on http://127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
