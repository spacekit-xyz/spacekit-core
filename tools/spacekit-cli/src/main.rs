//! `spacekit` — single **fat** CLI: embedded compute / storage / messaging plus local crypto.
//! Optional `~/.spacekit/network/config.toml` (or `SPACEKIT_NETWORK_CONFIG`) is merged into
//! runtime `connections` when `~/.spacekit/config.toml` is loaded, so you can aim the same
//! binary at external node URLs.

#![recursion_limit = "512"]
#![allow(unused_imports)]
#![allow(unused_variables)]

mod content_integration;
mod content_monetization;
mod full_client;
mod growformer_entitlement;
mod growformer_model_manager;
mod marketplace_integration;
mod network_e2e;
mod network_memory;
mod network_profile;
mod network_supervisor;
mod project_scaffold;
mod spkg;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    full_client::run_full_client().await
}
