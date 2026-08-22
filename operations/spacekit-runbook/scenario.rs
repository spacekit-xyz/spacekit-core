//! Scenario file parser.
//!
//! Reads runbook YAML files and produces in-memory `Scenario` structs that
//! the corpus generator can match log events against. Format must match the
//! YAML schema documented in `spacetime-consensus-runbook/scenarios/`.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use spacekit_log::{EventKind, FieldPredicate, ScenarioQuery, Severity};
use std::path::Path;

/// One scenario, as parsed from a YAML file.
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub scenario_id: String,
    pub version: u32,
    pub summary: String,
    pub severity_floor: String,
    pub event_queries: Vec<RawQuery>,
    pub recommended_agent_classification: RecommendedClassification,
    #[serde(default)]
    pub agent_must_not: Vec<String>,
    #[serde(default)]
    pub escalation: Vec<EscalationRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawQuery {
    pub kind: serde_yaml::Value,
    #[serde(default)]
    pub min_severity: Option<String>,
    #[serde(default)]
    pub field_predicates: Vec<serde_yaml::Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecommendedClassification {
    pub domain: String,
    pub intent: String,
    pub target: String,
    pub reasoning: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EscalationRule {
    pub condition: String,
    pub action: String,
}

impl Scenario {
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("reading scenario file {:?}", path))?;
        let scenario: Self = serde_yaml::from_str(&contents)
            .with_context(|| format!("parsing scenario YAML {:?}", path))?;
        scenario
            .validate()
            .with_context(|| format!("validating scenario {:?}", path))?;
        Ok(scenario)
    }

    /// Sanity checks that catch the kinds of mistakes humans make in YAML.
    pub fn validate(&self) -> Result<()> {
        if self.scenario_id.is_empty() {
            bail!("scenario_id missing");
        }
        if self.event_queries.is_empty() {
            bail!("scenario has no event_queries; cannot match any log events");
        }
        // Check that recommended_agent_classification has known domain.
        let known_domains = [
            "consensus_tuning",
            "anomaly_scoring",
            "clique_assessment",
            "fraud_classification",
            "policy_regime_recommendation",
        ];
        if !known_domains.contains(&self.recommended_agent_classification.domain.as_str()) {
            bail!(
                "recommended_agent_classification.domain '{}' is not a known agent domain",
                self.recommended_agent_classification.domain
            );
        }
        if self
            .recommended_agent_classification
            .reasoning
            .trim()
            .is_empty()
        {
            bail!("recommended_agent_classification.reasoning is empty; the agent needs an explanation");
        }
        Ok(())
    }

    /// Translate raw YAML queries into spacekit-log ScenarioQuery objects.
    pub fn compiled_queries(&self) -> Result<Vec<ScenarioQuery>> {
        self.event_queries
            .iter()
            .map(|raw| compile_query(raw))
            .collect()
    }
}

fn compile_query(raw: &RawQuery) -> Result<ScenarioQuery> {
    let kind = parse_kind(&raw.kind)?;
    let min_severity = match &raw.min_severity {
        Some(s) => Some(parse_severity(s)?),
        None => None,
    };
    let field_predicates = raw
        .field_predicates
        .iter()
        .map(parse_predicate)
        .collect::<Result<Vec<_>>>()?;
    Ok(ScenarioQuery {
        kind,
        min_severity,
        field_predicates,
    })
}

fn parse_kind(value: &serde_yaml::Value) -> Result<EventKind> {
    // Expect {CategoryName: VariantName}
    let mapping = value
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("event kind must be a mapping, got {:?}", value))?;
    if mapping.len() != 1 {
        bail!("event kind mapping must have exactly one entry");
    }
    let (cat_v, variant_v) = mapping.iter().next().unwrap();
    let category = cat_v
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("category must be string"))?;
    let variant = variant_v
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("variant must be string"))?;

    use spacekit_log::*;
    match category {
        "Consensus" => Ok(EventKind::Consensus(parse_consensus(variant)?)),
        "Spacetime" => Ok(EventKind::Spacetime(parse_spacetime(variant)?)),
        "Fraud" => Ok(EventKind::Fraud(parse_fraud(variant)?)),
        "Ratification" => Ok(EventKind::Ratification(parse_ratification(variant)?)),
        "Agent" => Ok(EventKind::Agent(parse_agent(variant)?)),
        "Service" => Ok(EventKind::Service(parse_service(variant)?)),
        other => bail!("unknown event category: {}", other),
    }
}

fn parse_consensus(v: &str) -> Result<spacekit_log::ConsensusEvent> {
    use spacekit_log::ConsensusEvent::*;
    Ok(match v {
        "BlockProposed" => BlockProposed,
        "BlockSoftFinalized" => BlockSoftFinalized,
        "BlockHardFinalized" => BlockHardFinalized,
        "BlockReverted" => BlockReverted,
        "ViewChange" => ViewChange,
        "QuorumReached" => QuorumReached,
        "QuorumFailed" => QuorumFailed,
        "ValidatorAdmitted" => ValidatorAdmitted,
        "ValidatorEjected" => ValidatorEjected,
        other => bail!("unknown Consensus variant: {}", other),
    })
}

fn parse_spacetime(v: &str) -> Result<spacekit_log::SpacetimeEvent> {
    use spacekit_log::SpacetimeEvent::*;
    Ok(match v {
        "TransitionObserved" => TransitionObserved,
        "ResidualMismatch" => ResidualMismatch,
        "FingerprintUpdated" => FingerprintUpdated,
        "FingerprintAnomalyMild" => FingerprintAnomalyMild,
        "FingerprintAnomalyStrong" => FingerprintAnomalyStrong,
        "AttestationBroadcast" => AttestationBroadcast,
        "AttestationMismatchDetected" => AttestationMismatchDetected,
        "CliqueDetected" => CliqueDetected,
        "GeometricMedianConverged" => GeometricMedianConverged,
        "GeometricMedianDiverged" => GeometricMedianDiverged,
        other => bail!("unknown Spacetime variant: {}", other),
    })
}

fn parse_fraud(v: &str) -> Result<spacekit_log::FraudEvent> {
    use spacekit_log::FraudEvent::*;
    Ok(match v {
        "ProofSubmitted" => ProofSubmitted,
        "ProofAccepted" => ProofAccepted,
        "ProofRejected" => ProofRejected,
        "RollbackInitiated" => RollbackInitiated,
        "RollbackCompleted" => RollbackCompleted,
        "SlashingApplied" => SlashingApplied,
        "BountyAwarded" => BountyAwarded,
        other => bail!("unknown Fraud variant: {}", other),
    })
}

fn parse_ratification(v: &str) -> Result<spacekit_log::RatificationEvent> {
    use spacekit_log::RatificationEvent::*;
    Ok(match v {
        "ProposalReceived" => ProposalReceived,
        "ProposalVoted" => ProposalVoted,
        "QuorumReached" => QuorumReached,
        "ProposalActivated" => ProposalActivated,
        "MalignRatificationDetected" => MalignRatificationDetected,
        "RegimeTransition" => RegimeTransition,
        other => bail!("unknown Ratification variant: {}", other),
    })
}

fn parse_service(v: &str) -> Result<spacekit_log::service::ServiceEvent> {
    use spacekit_log::service::ServiceEvent::*;
    Ok(match v {
        "ProposalAccepted" => ProposalAccepted,
        "VoteCorrect" => VoteCorrect,
        "EnvelopeSigned" => EnvelopeSigned,
        "UptimeConfirmed" => UptimeConfirmed,
        "ContractExecuted" => ContractExecuted,
        "HostHookInvoked" => HostHookInvoked,
        "BlobServedRead" => BlobServedRead,
        "BlobServedWrite" => BlobServedWrite,
        "ProofAttested" => ProofAttested,
        "CapacityMaintained" => CapacityMaintained,
        "MessageDelivered" => MessageDelivered,
        "BroadcastSent" => BroadcastSent,
        "KeyResolved" => KeyResolved,
        other => bail!("unknown Service variant: {}", other),
    })
}

fn parse_agent(v: &str) -> Result<spacekit_log::AgentEvent> {
    use spacekit_log::AgentEvent::*;
    Ok(match v {
        "BrainFetched" => BrainFetched,
        "BrainLoaded" => BrainLoaded,
        "BrainHashMismatch" => BrainHashMismatch,
        "InferenceCompleted" => InferenceCompleted,
        "InferenceUnavailable" => InferenceUnavailable,
        "InferenceModelMismatch" => InferenceModelMismatch,
        "InferenceLowConfidence" => InferenceLowConfidence,
        "CircuitBreakerOpened" => CircuitBreakerOpened,
        "CircuitBreakerClosed" => CircuitBreakerClosed,
        other => bail!("unknown Agent variant: {}", other),
    })
}

fn parse_severity(s: &str) -> Result<Severity> {
    Ok(match s {
        "Debug" => Severity::Debug,
        "Info" => Severity::Info,
        "Notice" => Severity::Notice,
        "Warning" => Severity::Warning,
        "Critical" => Severity::Critical,
        "Alert" => Severity::Alert,
        other => bail!("unknown severity: {}", other),
    })
}

fn parse_predicate(value: &serde_yaml::Value) -> Result<FieldPredicate> {
    let mapping = value
        .as_mapping()
        .ok_or_else(|| anyhow::anyhow!("field predicate must be a mapping"))?;
    if mapping.len() != 1 {
        bail!("field predicate must have exactly one entry");
    }
    let (op_v, args_v) = mapping.iter().next().unwrap();
    let op = op_v
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("predicate op must be string"))?;
    match op {
        "Exists" => {
            let key = args_v
                .as_str()
                .or_else(|| {
                    args_v
                        .as_sequence()
                        .and_then(|args| args.get(0))
                        .and_then(|v| v.as_str())
                })
                .ok_or_else(|| anyhow::anyhow!("Exists predicate needs string key"))?;
            Ok(FieldPredicate::Exists(key.to_string()))
        }
        "FloatAtLeast" => {
            let args = args_v
                .as_sequence()
                .ok_or_else(|| anyhow::anyhow!("FloatAtLeast predicate args must be a sequence"))?;
            let key = args
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("FloatAtLeast needs string key"))?;
            let threshold = args
                .get(1)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow::anyhow!("FloatAtLeast needs float threshold"))?;
            Ok(FieldPredicate::FloatAtLeast(key.to_string(), threshold))
        }
        "FloatAtMost" => {
            let args = args_v
                .as_sequence()
                .ok_or_else(|| anyhow::anyhow!("FloatAtMost predicate args must be a sequence"))?;
            let key = args
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("FloatAtMost needs string key"))?;
            let threshold = args
                .get(1)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| anyhow::anyhow!("FloatAtMost needs float threshold"))?;
            Ok(FieldPredicate::FloatAtMost(key.to_string(), threshold))
        }
        "UnsignedEquals" => {
            let args = args_v.as_sequence().ok_or_else(|| {
                anyhow::anyhow!("UnsignedEquals predicate args must be a sequence")
            })?;
            let key = args
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("UnsignedEquals needs string key"))?;
            let value = args
                .get(1)
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("UnsignedEquals needs unsigned value"))?;
            Ok(FieldPredicate::UnsignedEquals(key.to_string(), value))
        }
        "TextEquals" => {
            let args = args_v
                .as_sequence()
                .ok_or_else(|| anyhow::anyhow!("TextEquals predicate args must be a sequence"))?;
            let key = args
                .get(0)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("TextEquals needs string key"))?;
            let value = args
                .get(1)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("TextEquals needs string value"))?;
            Ok(FieldPredicate::TextEquals(
                key.to_string(),
                value.to_string(),
            ))
        }
        other => bail!("unknown field predicate: {}", other),
    }
}

/// Load all scenarios from a directory.
pub fn load_scenarios_from_dir(dir: &Path) -> Result<Vec<Scenario>> {
    let mut scenarios = Vec::new();
    for entry in walkdir::WalkDir::new(dir).max_depth(2) {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
            scenarios.push(Scenario::from_yaml_file(path)?);
        }
    }
    scenarios.sort_by(|a, b| a.scenario_id.cmp(&b.scenario_id));
    Ok(scenarios)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn repository_scenarios_parse_compile_and_have_unique_ids() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let scenarios = load_scenarios_from_dir(root).expect("load repository scenarios");
        assert!(!scenarios.is_empty());

        let mut ids = HashSet::new();
        for scenario in scenarios {
            assert!(
                ids.insert(scenario.scenario_id.clone()),
                "duplicate scenario id {}",
                scenario.scenario_id
            );
            scenario
                .compiled_queries()
                .unwrap_or_else(|error| panic!("{}: {error:#}", scenario.scenario_id));
        }
    }
}
