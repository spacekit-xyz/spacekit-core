//! Durable RouteKit records stored in SpaceKit Storage Node's DID-scoped Documents API.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const API_KEY_COLLECTION: &str = "routekit-api-keys";
const USAGE_COLLECTION: &str = "routekit-completions";

#[derive(Clone)]
pub struct StorageClient {
    client: reqwest::Client,
    base_url: String,
    operator_did: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyRecord {
    pub key_id: String,
    pub key_hash: String,
    pub owner_did: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub expires_at: Option<u64>,
    #[serde(default = "default_rate_limit")]
    pub rate_limit_rpm: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompletionReceipt {
    pub request_id: String,
    pub key_id: String,
    pub owner_did: String,
    pub provider: String,
    pub model: String,
    pub task: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub status: String,
    pub finished_at_unix: u64,
}

#[derive(Deserialize)]
struct DocumentEnvelope<T> {
    document: Document<T>,
}

#[derive(Deserialize)]
struct Document<T> {
    data: T,
}

fn default_true() -> bool {
    true
}

fn default_rate_limit() -> u32 {
    60
}

impl StorageClient {
    pub fn new(
        client: reqwest::Client,
        base_url: impl Into<String>,
        operator_did: impl Into<String>,
    ) -> Self {
        Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            operator_did: operator_did.into(),
        }
    }

    pub async fn health(&self) -> anyhow::Result<()> {
        self.client
            .get(format!("{}/health", self.base_url))
            .timeout(Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    pub async fn get_api_key(&self, key_hash: &str) -> anyhow::Result<Option<ApiKeyRecord>> {
        let response = self
            .client
            .get(self.document_url(API_KEY_COLLECTION, key_hash))
            .header("authorization", format!("DID {}", self.operator_did))
            .timeout(Duration::from_secs(5))
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let envelope = response
            .error_for_status()?
            .json::<DocumentEnvelope<ApiKeyRecord>>()
            .await?;
        Ok(Some(envelope.document.data))
    }

    pub async fn put_completion(&self, receipt: &CompletionReceipt) -> anyhow::Result<()> {
        self.put_document(USAGE_COLLECTION, &receipt.request_id, receipt)
            .await
    }

    async fn put_document<T: Serialize + ?Sized>(
        &self,
        collection: &str,
        id: &str,
        value: &T,
    ) -> anyhow::Result<()> {
        self.client
            .put(self.document_url(collection, id))
            .header("authorization", format!("DID {}", self.operator_did))
            .json(value)
            .timeout(Duration::from_secs(5))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    fn document_url(&self, collection: &str, id: &str) -> String {
        format!("{}/api/documents/{}/{}", self.base_url, collection, id)
    }
}
