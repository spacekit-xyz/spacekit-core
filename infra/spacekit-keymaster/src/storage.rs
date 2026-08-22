use anyhow::{anyhow, Result};
use reqwest::Client;

use crate::types::Placement;

/// Maps SKKM placements to storage-node objects. Falls back to coordinator-local store on error.
pub struct StorageGateway {
    base_url: String,
    http: Client,
}

impl StorageGateway {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn put(&self, placement: &Placement, bytes: &[u8]) -> Result<()> {
        let url = format!("{}/v1/keymaster/objects/{}", self.base_url, placement.object_id.trim_start_matches("0x"));
        self.http
            .put(&url)
            .header("x-spacekit-node-did", &placement.node_did)
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|e| anyhow!("storage put: {e}"))?
            .error_for_status()
            .map_err(|e| anyhow!("storage put status: {e}"))?;
        Ok(())
    }

    pub async fn get(&self, placement: &Placement) -> Result<Vec<u8>> {
        let url = format!("{}/v1/keymaster/objects/{}", self.base_url, placement.object_id.trim_start_matches("0x"));
        Ok(self
            .http
            .get(&url)
            .header("x-spacekit-node-did", &placement.node_did)
            .send()
            .await
            .map_err(|e| anyhow!("storage get: {e}"))?
            .error_for_status()
            .map_err(|e| anyhow!("storage get status: {e}"))?
            .bytes()
            .await
            .map_err(|e| anyhow!("storage get body: {e}"))?
            .to_vec())
    }

    pub async fn delete(&self, placement: &Placement) -> Result<()> {
        let url = format!("{}/v1/keymaster/objects/{}", self.base_url, placement.object_id.trim_start_matches("0x"));
        let _ = self
            .http
            .delete(&url)
            .header("x-spacekit-node-did", &placement.node_did)
            .send()
            .await;
        Ok(())
    }
}
