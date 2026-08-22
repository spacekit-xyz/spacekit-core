//! Consensus-tuning Growformer agent: load brain from storage at startup,
//! cache version under `~/.spacekit`, run periodic inference for parameter proposals.
//!
//! Requires `SPACEKIT_API_KEY` (or `.spacekit/api_key`) for storage-node auth and
//! `growformer-inference` + `storage-integration` on the compute node.

#![cfg(all(
    feature = "spacetime-consensus",
    feature = "growformer-inference",
    feature = "storage-integration"
))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use spacekit_spacetime_consensus::{GrowformerInference, GrowformerIntent, PolicyRegime};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::spacekitvm::swtchvm_node::SwtchvmNode;

/// Local manifest: which brain version is installed and which storage key to fetch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusAgentManifest {
    pub agent_id: String,
    pub storage_brain_key: String,
    pub brain_version: String,
    pub growformer_binary_hint: Option<String>,
    #[serde(default = "default_interval_secs")]
    pub inference_interval_secs: u64,
}

fn default_interval_secs() -> u64 {
    300
}

impl Default for ConsensusAgentManifest {
    fn default() -> Self {
        Self {
            agent_id: "consensus-tuning".into(),
            storage_brain_key: "consensus-tuning-agent-brain".into(),
            brain_version: "0".into(),
            growformer_binary_hint: None,
            inference_interval_secs: default_interval_secs(),
        }
    }
}

/// Host-side consensus Growformer worker (brain in SwtchVM + native runtime).
pub struct ConsensusGrowformerAgent {
    pub manifest: ConsensusAgentManifest,
    manifest_path: PathBuf,
    node: Arc<SwtchvmNode>,
    last_inference: Arc<RwLock<Option<GrowformerInference>>>,
}

impl ConsensusGrowformerAgent {
    pub fn spacekit_dir() -> PathBuf {
        std::env::var("SPACEKIT_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".spacekit")))
            .unwrap_or_else(|_| PathBuf::from(".spacekit"))
    }

    pub fn manifest_path() -> PathBuf {
        Self::spacekit_dir().join("consensus-agent.json")
    }

    pub fn api_key_from_env_or_file() -> Option<String> {
        if let Ok(k) = std::env::var("SPACEKIT_API_KEY") {
            if !k.is_empty() {
                return Some(k);
            }
        }
        let path = Self::spacekit_dir().join("api_key");
        std::fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn load_or_create_manifest() -> Result<ConsensusAgentManifest> {
        let path = Self::manifest_path();
        if path.exists() {
            let raw = std::fs::read_to_string(&path).context("read consensus-agent.json")?;
            return serde_json::from_str(&raw).context("parse consensus-agent.json");
        }
        let manifest = ConsensusAgentManifest::default();
        Self::save_manifest(&manifest)?;
        Ok(manifest)
    }

    pub fn save_manifest(manifest: &ConsensusAgentManifest) -> Result<()> {
        let dir = Self::spacekit_dir();
        std::fs::create_dir_all(&dir).context("create .spacekit")?;
        let path = Self::manifest_path();
        let raw = serde_json::to_string_pretty(manifest).context("serialize manifest")?;
        std::fs::write(&path, raw).context("write consensus-agent.json")?;
        Ok(())
    }

    /// Bootstrap: fetch brain from storage node (if newer than local version), load into VM.
    pub async fn bootstrap(
        node: Arc<SwtchvmNode>,
        storage: Arc<spacekit_storage_node::StorageNode>,
        wallet_did: &str,
        desired_version: Option<&str>,
    ) -> Result<Self> {
        let _api_key = Self::api_key_from_env_or_file();
        let mut manifest = Self::load_or_create_manifest()?;
        if let Some(v) = desired_version {
            manifest.brain_version = v.to_string();
        }

        let key = manifest.storage_brain_key.clone();
        let bytes = storage
            .retrieve_key_value(&key, wallet_did)
            .await
            .map_err(|e| anyhow!("storage retrieve {}: {}", key, e))?
            .filter(|b| !b.is_empty())
            .ok_or_else(|| anyhow!("no brain bytes at storage key {}", key))?;

        let loaded = node.growformer_apply_brain_bytes(bytes.clone());
        if loaded <= 0 {
            return Err(anyhow!(
                "growformer_apply_brain_bytes failed for {} (code {})",
                key,
                loaded
            ));
        }

        info!(
            agent = %manifest.agent_id,
            version = %manifest.brain_version,
            bytes = bytes.len(),
            "consensus Growformer agent brain loaded"
        );
        Self::save_manifest(&manifest)?;

        Ok(Self {
            manifest,
            manifest_path: Self::manifest_path(),
            node,
            last_inference: Arc::new(RwLock::new(None)),
        })
    }

    /// Run inference on a metrics summary prompt; parse JSON into [`GrowformerInference`].
    pub async fn infer_consensus_tuning(
        &self,
        metrics_prompt: &str,
    ) -> Result<GrowformerInference> {
        let json = self
            .node
            .growformer_run_prompt_json(metrics_prompt.trim())
            .map_err(|_| anyhow!("growformer inference failed"))?;
        let inference = parse_growformer_consensus_json(&json, metrics_prompt)?;
        *self.last_inference.write().await = Some(inference.clone());
        Ok(inference)
    }

    pub async fn last_inference(&self) -> Option<GrowformerInference> {
        self.last_inference.read().await.clone()
    }

    /// Spawn periodic consensus-tuning inference until the process exits.
    pub fn spawn_periodic_inference(
        self: Arc<Self>,
        metrics_fn: impl Fn() -> String + Send + Sync + 'static,
    ) {
        let interval = Duration::from_secs(self.manifest.inference_interval_secs.max(30));
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                let prompt = metrics_fn();
                match self.infer_consensus_tuning(&prompt).await {
                    Ok(inf) => {
                        info!(
                            intent = ?inf.semantic_intent,
                            target = %inf.action_target,
                            confidence = inf.confidence,
                            "consensus Growformer periodic inference"
                        );
                    }
                    Err(e) => warn!("consensus Growformer periodic inference failed: {}", e),
                }
            }
        });
    }

    /// Update local manifest when compute-node signals a new agent version.
    pub fn apply_version_update(
        &mut self,
        new_version: &str,
        storage_key: Option<&str>,
    ) -> Result<()> {
        self.manifest.brain_version = new_version.to_string();
        if let Some(k) = storage_key {
            self.manifest.storage_brain_key = k.to_string();
        }
        Self::save_manifest(&self.manifest)
    }
}

/// Parse Growformer JSON output into ratification wire types.
pub fn parse_growformer_consensus_json(
    json: &str,
    metrics_prompt: &str,
) -> Result<GrowformerInference> {
    use alloy_primitives::{keccak256, B256};
    use spacekit_spacetime_consensus::GrowformerIntent;

    let v: serde_json::Value = serde_json::from_str(json).or_else(|_| extract_json_object(json))?;

    let semantic = v
        .get("semantic_intent")
        .or_else(|| v.get("intent"))
        .and_then(|x| x.as_str())
        .unwrap_or("no_change");
    let intent = match semantic.to_lowercase().as_str() {
        "tighten" | "positive_strong" => GrowformerIntent::Tighten,
        "loosen" | "negative_mild" => GrowformerIntent::Loosen,
        "alert" => GrowformerIntent::Alert,
        _ => GrowformerIntent::NoChange,
    };

    let regime = v
        .get("policy_regime")
        .and_then(|x| x.as_str())
        .unwrap_or("default");
    let policy_regime = match regime.to_lowercase().as_str() {
        "secure" => PolicyRegime::Secure,
        "permissive" => PolicyRegime::Permissive,
        _ => PolicyRegime::Default,
    };

    let confidence = v.get("confidence").and_then(|x| x.as_f64()).unwrap_or(0.85);

    Ok(GrowformerInference {
        task_id: v
            .get("task_id")
            .and_then(|x| x.as_str())
            .unwrap_or("consensus-periodic")
            .to_string(),
        domain: v
            .get("domain")
            .and_then(|x| x.as_str())
            .unwrap_or("consensus_tuning")
            .to_string(),
        semantic_intent: intent,
        action_target: v
            .get("action_target")
            .and_then(|x| x.as_str())
            .unwrap_or("spacetime.divergence_threshold")
            .to_string(),
        policy_regime,
        expected_response: v
            .get("expected_response")
            .or_else(|| v.get("response"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        metrics_window_hash: B256::from(keccak256(metrics_prompt.as_bytes())),
        confidence,
    })
}

fn extract_json_object(raw: &str) -> Result<serde_json::Value> {
    let start = raw
        .find('{')
        .ok_or_else(|| anyhow!("no JSON object in growformer output"))?;
    let end = raw
        .rfind('}')
        .ok_or_else(|| anyhow!("unclosed JSON in growformer output"))?;
    serde_json::from_str(&raw[start..=end]).context("parse embedded JSON")
}
