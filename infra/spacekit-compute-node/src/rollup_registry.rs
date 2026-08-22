use crate::rollup_bridge::{BundleStatus, FraudProof, RollupBundle, SlashRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedBundle {
    pub bundle: RollupBundle,
    pub status: BundleStatus,
    pub verified_at: u64,
    pub challenge_window_end: u64,
    pub fraud_proofs: Vec<FraudProof>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RollupRegistry {
    pub bundles: BTreeMap<String, RollupBundle>,
    #[serde(default)]
    pub tracked: BTreeMap<String, TrackedBundle>,
    #[serde(default)]
    pub slash_records: Vec<SlashRecord>,
}

fn default_registry_path() -> PathBuf {
    PathBuf::from("temp_blockchain_storage/rollup_registry.json")
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn load_registry(path: Option<PathBuf>) -> Result<RollupRegistry, String> {
    let path = path.unwrap_or_else(default_registry_path);
    if !path.exists() {
        return Ok(RollupRegistry::default());
    }
    let data = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

pub fn save_registry(path: Option<PathBuf>, registry: &RollupRegistry) -> Result<(), String> {
    let path = path.unwrap_or_else(default_registry_path);
    ensure_parent(&path)?;
    let data = serde_json::to_string_pretty(registry).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())
}

pub fn ingest_bundle(bundle: &RollupBundle) -> Result<(), String> {
    let mut registry = load_registry(None)?;
    registry
        .bundles
        .insert(bundle.bundle_id.clone(), bundle.clone());
    save_registry(None, &registry)
}

pub fn get_bundle(bundle_id: &str) -> Result<Option<RollupBundle>, String> {
    let registry = load_registry(None)?;
    Ok(registry.bundles.get(bundle_id).cloned())
}

pub fn list_bundles() -> Result<Vec<RollupBundle>, String> {
    let registry = load_registry(None)?;
    Ok(registry.bundles.values().cloned().collect())
}

pub fn track_verified_bundle(
    bundle: &RollupBundle,
    status: BundleStatus,
    challenge_window_secs: u64,
) -> Result<(), String> {
    let mut registry = load_registry(None)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let tracked = TrackedBundle {
        bundle: bundle.clone(),
        status,
        verified_at: now,
        challenge_window_end: now + challenge_window_secs,
        fraud_proofs: Vec::new(),
    };
    registry.tracked.insert(bundle.bundle_id.clone(), tracked);
    save_registry(None, &registry)
}

pub fn submit_fraud_proof(proof: FraudProof) -> Result<(), String> {
    let mut registry = load_registry(None)?;
    let tracked = registry
        .tracked
        .get_mut(&proof.bundle_id)
        .ok_or_else(|| format!("bundle {} not tracked", proof.bundle_id))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    if now > tracked.challenge_window_end {
        return Err("challenge window has closed".to_string());
    }
    tracked.status = BundleStatus::Challenged;
    tracked.fraud_proofs.push(proof.clone());
    let sig_key = tracked
        .bundle
        .signature
        .as_ref()
        .map(|s| s.public_key_hex.clone())
        .unwrap_or_default();
    registry.slash_records.push(SlashRecord {
        bundle_id: proof.bundle_id.clone(),
        sequencer_key: sig_key,
        reason: format!(
            "state root mismatch at block {}: expected {} got {}",
            proof.block_index, proof.expected_state_root, proof.computed_state_root
        ),
        fraud_proof: proof,
        slash_amount: 0,
        timestamp: now,
    });
    save_registry(None, &registry)
}

pub fn finalize_bundles() -> Result<Vec<String>, String> {
    let mut registry = load_registry(None)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs();
    let mut finalized = Vec::new();
    for (id, tracked) in registry.tracked.iter_mut() {
        if now > tracked.challenge_window_end
            && matches!(
                tracked.status,
                BundleStatus::Verified | BundleStatus::Pending
            )
            && tracked.fraud_proofs.is_empty()
        {
            tracked.status = BundleStatus::Verified;
            finalized.push(id.clone());
        }
    }
    save_registry(None, &registry)?;
    Ok(finalized)
}

pub fn get_tracked_bundle(bundle_id: &str) -> Result<Option<TrackedBundle>, String> {
    let registry = load_registry(None)?;
    Ok(registry.tracked.get(bundle_id).cloned())
}

pub fn list_slash_records() -> Result<Vec<SlashRecord>, String> {
    let registry = load_registry(None)?;
    Ok(registry.slash_records.clone())
}
