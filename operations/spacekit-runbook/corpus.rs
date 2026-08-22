//! Corpus generation: turn matched (event, scenario) pairs into JSONL rows.

use crate::scenario::Scenario;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use spacekit_log::{
    AgentEvent, ConsensusEvent, EventKind, FraudEvent, LogEvent, RatificationEvent, SpacetimeEvent,
};
use std::collections::HashMap;

/// One JSONL training row, matching the format produced by the fintech
/// sentiment pipeline and consumed by Growformer training.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingRow {
    pub task_id: String,
    pub text: String,
    pub semantic_intent: String,
    pub domain: String,
    pub action_target: String,
    pub policy_regime: String,
    pub language_channel: String,
    pub code_language: Option<String>,
    pub split: String,
    pub expected_response: String,
    pub expected_code: Option<String>,
}

/// Render a log event as the human-readable `text` field of a training row.
/// Templates are per-event-kind; missing fields produce best-effort text.
pub fn render_event_text(event: &LogEvent) -> String {
    match &event.kind {
        EventKind::Consensus(ConsensusEvent::BlockSoftFinalized) => {
            let weight = event.get_float("quorum_weight").unwrap_or(0.0);
            format!(
                "Block soft-finalized at height {}. Quorum weight {:.2}.",
                event.block_height, weight
            )
        }
        EventKind::Consensus(ConsensusEvent::BlockReverted) => {
            format!(
                "Block at height {} reverted by fraud proof.",
                event.block_height
            )
        }
        EventKind::Consensus(ConsensusEvent::QuorumFailed) => {
            let weight = event.get_float("weight").unwrap_or(0.0);
            format!(
                "Quorum failed at height {}. Achieved weight {:.2} (below 2/3 threshold).",
                event.block_height, weight
            )
        }
        EventKind::Spacetime(SpacetimeEvent::TransitionObserved) => {
            let rotor_mag = event.get_float("rotor_magnitude").unwrap_or(0.0);
            let residual = event.get_float("residual_norm").unwrap_or(0.0);
            format!(
                "Transition observed at height {}: rotor magnitude {:.3}, residual norm {:.3}.",
                event.block_height, rotor_mag, residual
            )
        }
        EventKind::Spacetime(SpacetimeEvent::ResidualMismatch) => {
            let delta = event.get_float("residual_delta").unwrap_or(0.0);
            format!("Residual commitment mismatch at height {}. Delta from validator's recomputation: {:.4}.",
                    event.block_height, delta)
        }
        EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyMild) => {
            let dist = event.get_float("centroid_distance").unwrap_or(0.0);
            let sigma = event.get_float("sigma_threshold").unwrap_or(0.0);
            format!("Mild fingerprint anomaly at height {}: centroid distance {:.2}, sigma threshold {:.1}, factor {:.1}x.",
                    event.block_height, dist, sigma, dist / sigma.max(0.0001))
        }
        EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyStrong) => {
            let dist = event.get_float("centroid_distance").unwrap_or(0.0);
            let sigma = event.get_float("sigma_threshold").unwrap_or(0.0);
            format!("Strong fingerprint anomaly at height {}: centroid distance {:.2}, sigma threshold {:.1}, factor {:.1}x.",
                    event.block_height, dist, sigma, dist / sigma.max(0.0001))
        }
        EventKind::Spacetime(SpacetimeEvent::CliqueDetected) => {
            let count = event.get_unsigned("validator_count").unwrap_or(0);
            let score = event.get_float("coordination_score").unwrap_or(0.0);
            let avg = event.get_float("avg_rotor_distance").unwrap_or(0.0);
            format!("Clique detected at height {}: {} validators, coordination score {:.2}, average rotor distance {:.4}.",
                    event.block_height, count, score, avg)
        }
        EventKind::Spacetime(SpacetimeEvent::AttestationMismatchDetected) => {
            format!(
                "Fingerprint attestation mismatch detected at height {}.",
                event.block_height
            )
        }
        EventKind::Spacetime(SpacetimeEvent::GeometricMedianDiverged) => {
            let iters = event.get_unsigned("iterations").unwrap_or(0);
            let step = event.get_float("step_norm").unwrap_or(0.0);
            format!("Geometric median failed to converge at height {} after {} iterations. Final step norm {:.4}.",
                    event.block_height, iters, step)
        }
        EventKind::Fraud(FraudEvent::ProofAccepted) => {
            let count = event.get_unsigned("rolled_back_count").unwrap_or(0);
            format!(
                "Fraud proof accepted at height {}. {} blocks rolled back.",
                event.block_height, count
            )
        }
        EventKind::Fraud(FraudEvent::ProofRejected) => {
            let reason = event.get_text("rejection_reason").unwrap_or("unknown");
            format!(
                "Fraud proof rejected at height {}. Reason: {}.",
                event.block_height, reason
            )
        }
        EventKind::Ratification(RatificationEvent::ProposalActivated) => {
            let target = event.get_text("action_target").unwrap_or("?");
            let old = event.get_float("old_value").unwrap_or(0.0);
            let new = event.get_float("new_value").unwrap_or(0.0);
            format!(
                "Parameter {} activated at height {}: {:.3} → {:.3}.",
                target, event.block_height, old, new
            )
        }
        EventKind::Ratification(RatificationEvent::MalignRatificationDetected) => {
            format!("Malign ratification detected at height {}. YES vote on parameter change later exploited.",
                    event.block_height)
        }
        EventKind::Agent(AgentEvent::BrainHashMismatch) => {
            format!("Brain hash mismatch detected at height {}. Loaded brain does not match network-canonical hash.",
                    event.block_height)
        }
        EventKind::Agent(AgentEvent::InferenceModelMismatch) => {
            format!("Inference returned from a Growformer instance with the wrong model_hash at height {}.",
                    event.block_height)
        }
        EventKind::Agent(AgentEvent::CircuitBreakerOpened) => {
            let failures = event.get_unsigned("consecutive_failures").unwrap_or(0);
            format!(
                "Growformer circuit breaker opened at height {} after {} consecutive failures.",
                event.block_height, failures
            )
        }
        // Fallback: synthesize from kind name and message.
        _ => {
            if event.message.is_empty() {
                format!("Event {:?} at height {}.", event.kind, event.block_height)
            } else {
                format!(
                    "Event {:?} at height {}: {}",
                    event.kind, event.block_height, event.message
                )
            }
        }
    }
}

/// Produce a training row from a matched (event, scenario) pair.
/// `task_id` is derived from scenario_id + event content hash for stable
/// dedup across runs.
pub fn build_training_row(
    event: &LogEvent,
    scenario: &Scenario,
    policy_regime: &str,
    split: &str,
) -> TrainingRow {
    let text = render_event_text(event);
    let event_hash = event.content_hash(keccak256_wrapper);
    let task_id = format!(
        "{}_{}",
        scenario.scenario_id,
        hex::encode(&event_hash.as_slice()[..8])
    );
    TrainingRow {
        task_id,
        text,
        semantic_intent: scenario.recommended_agent_classification.intent.clone(),
        domain: scenario.recommended_agent_classification.domain.clone(),
        action_target: scenario.recommended_agent_classification.target.clone(),
        policy_regime: policy_regime.to_string(),
        language_channel: "english".to_string(),
        code_language: None,
        split: split.to_string(),
        expected_response: scenario
            .recommended_agent_classification
            .reasoning
            .trim()
            .to_string(),
        expected_code: None,
    }
}

fn keccak256_wrapper(b: &[u8]) -> [u8; 32] {
    let mut h = Keccak256::new();
    h.update(b);
    let r = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&r);
    out
}

/// Subsampling and deduplication: dedupe by content hash, then cap per-scenario.
pub fn deduplicate_and_cap(
    rows: Vec<(String, TrainingRow)>,
    cap_per_scenario: usize,
) -> Vec<TrainingRow> {
    // Pass 1: dedup by task_id.
    let mut seen: HashMap<String, TrainingRow> = HashMap::new();
    for (_scenario_id, row) in rows {
        seen.entry(row.task_id.clone()).or_insert(row);
    }

    // Pass 2: cap per-scenario. task_id starts with scenario_id, so we can
    // group by the prefix before '_'.
    let mut by_scenario: HashMap<String, Vec<TrainingRow>> = HashMap::new();
    for (_, row) in seen.into_iter() {
        let scenario_prefix = row.task_id.split('_').next().unwrap_or("").to_string();
        by_scenario.entry(scenario_prefix).or_default().push(row);
    }

    // Sort each group deterministically and take up to cap.
    let mut out = Vec::new();
    let mut keys: Vec<String> = by_scenario.keys().cloned().collect();
    keys.sort();
    for k in keys {
        let mut group = by_scenario.remove(&k).unwrap();
        group.sort_by(|a, b| a.task_id.cmp(&b.task_id));
        for row in group.into_iter().take(cap_per_scenario) {
            out.push(row);
        }
    }
    out
}

/// Write rows to per-domain JSONL files in the output directory.
/// Files are named `{domain}.jsonl`. Existing files are appended to (caller
/// is responsible for truncating between runs if desired).
pub fn write_corpus(
    rows: &[TrainingRow],
    output_dir: &std::path::Path,
) -> anyhow::Result<HashMap<String, usize>> {
    use std::io::Write;
    std::fs::create_dir_all(output_dir)?;

    let mut by_domain: HashMap<String, Vec<&TrainingRow>> = HashMap::new();
    for row in rows {
        by_domain.entry(row.domain.clone()).or_default().push(row);
    }

    let mut counts = HashMap::new();
    for (domain, rows) in by_domain {
        let path = output_dir.join(format!("{}.jsonl", domain));
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        for row in &rows {
            let line = serde_json::to_string(row)?;
            writeln!(file, "{}", line)?;
        }
        counts.insert(domain, rows.len());
    }
    Ok(counts)
}

// Tiny hex impl to avoid a hex dep.
mod hex {
    pub fn encode(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for b in bytes {
            s.push_str(&format!("{:02x}", b));
        }
        s
    }
}
