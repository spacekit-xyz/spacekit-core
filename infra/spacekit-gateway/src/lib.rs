pub mod catalog;
pub mod mcp_proxy;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub storage_mcp_cmd: Vec<String>,
    pub compute_mcp_cmd: Vec<String>,
    pub http_port: u16,
    pub enable_stdio: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            storage_mcp_cmd: vec!["spacekit-storage-node".into(), "mcp".into()],
            compute_mcp_cmd: vec!["spacekit-compute-node".into(), "mcp".into()],
            http_port: 8080,
            enable_stdio: false,
        }
    }
}
