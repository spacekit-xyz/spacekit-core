//! # spacekit-log
//!
//! Structured event-logging contract for SpaceKit nodes. **One schema, three
//! consumers:**
//!
//! 1. **Operators** grep these events during network incidents.
//! 2. **Runbooks** identify scenarios by querying against these events
//!    (each runbook entry has a `scenario_id` that matches against a
//!    `LogEvent::scenario_match()`).
//! 3. **SpacetimeConsensusAgent training** replays these events against
//!    runbook entries to generate labeled examples deterministically —
//!    the agent learns to classify each scenario the same way the runbook
//!    documents.
//!
//! This last consumer is why the schema is locked: training data
//! determinism requires that *the same event always produces the same bytes*.
//! That means stable field order, canonical numeric serialization, and a
//! content hash any consumer can recompute.
//!
//! ## Event categories
//!
//! Six categories, each with a fixed set of event kinds. New kinds can be
//! added by appending to the enum; existing kinds and their fields are
//! locked.
//!
//!   - **Consensus**: PBFT-level events (proposals, votes, finalizations)
//!   - **Spacetime**: rotor-layer events (transition observed, fingerprint
//!     update, attestation, anomaly, clique)
//!   - **Fraud**: fraud-proof submissions, accepted proofs, rollbacks
//!   - **Ratification**: parameter-change proposals, votes, activations
//!   - **Agent**: Growformer inference, brain-load, model-mismatch, circuit-breaker
//!   - **Service**: operator work units for SRA / ASTRA emission (compute, storage, …)
//!
//! ## Determinism rule
//!
//! Every `LogEvent` has a `content_hash` derived from the canonical
//! serialization (`to_canonical_bytes`). The hash is the primary key for
//! deduplication, replay, and training-example identity. Two events with
//! identical fields produce identical hashes regardless of when or where
//! they were emitted.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod service;
pub mod sra;

pub use service::{classify_sra_topic, service_kind, ServiceEvent, SraSwtchvmLog};
pub use sra::topic0_from_label;

use alloc::string::String;
use alloc::vec::Vec;
use alloy_primitives::B256;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Wire version of the log schema. Bumped on any breaking change to event
/// kinds or field layout. Runbook entries pin to a wire version.
pub const LOG_WIRE_VERSION: u16 = 1;

/// One structured event from a SpaceKit node. Every emitted event must
/// populate ALL non-optional fields; missing fields are a contract violation.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LogEvent {
    /// Schema version. Locked at emission; consumers check before parsing.
    pub wire_version: u16,
    /// Unix milliseconds. Used for ordering across nodes — NOT a precision
    /// timestamp; for causal ordering use `block_height` + `view`.
    pub timestamp_ms: u64,
    /// Block height at which the event was emitted. Anchors the event in
    /// the chain timeline regardless of wall-clock skew.
    pub block_height: u64,
    /// Emitting node's DID hash. Lets multi-node log aggregation reconstruct
    /// per-validator behavior.
    pub emitter_did_hash: B256,
    /// Event category + kind, structured for grep-friendly filtering.
    pub kind: EventKind,
    /// Severity level for operator filtering. Independent of event kind:
    /// an `AgentBrainLoaded` event is Info, a `BrainHashMismatch` is Critical.
    pub severity: Severity,
    /// Free-text human-readable message. Auxiliary to the structured fields,
    /// NOT a primary source of truth. Operators may grep this; runbooks
    /// must NOT match on it (use kind + fields).
    pub message: String,
    /// Structured fields as flat key-value pairs. Keys are stable;
    /// adding new keys is backward-compatible, renaming is a wire-version bump.
    pub fields: Vec<(String, FieldValue)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Severity {
    Debug,
    Info,
    /// Operator should be aware but no immediate action required.
    Notice,
    /// Action may be required; degraded operation.
    Warning,
    /// Action required; correctness or safety at risk.
    Critical,
    /// Incident-response activation.
    Alert,
}

/// Event taxonomy. Six top-level categories, fixed enumeration of kinds
/// within each.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EventKind {
    Consensus(ConsensusEvent),
    Spacetime(SpacetimeEvent),
    Fraud(FraudEvent),
    Ratification(RatificationEvent),
    Agent(AgentEvent),
    /// Operator service events consumed by the Service Reward Accumulator (SRA).
    Service(service::ServiceEvent),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ConsensusEvent {
    BlockProposed,
    BlockSoftFinalized,
    BlockHardFinalized,
    BlockReverted,
    ViewChange,
    QuorumReached,
    QuorumFailed,
    ValidatorAdmitted,
    ValidatorEjected,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SpacetimeEvent {
    TransitionObserved,
    /// v2: transition's residual_commitment didn't match validator's
    /// independent recomputation.
    ResidualMismatch,
    FingerprintUpdated,
    FingerprintAnomalyMild,
    FingerprintAnomalyStrong,
    AttestationBroadcast,
    AttestationMismatchDetected,
    CliqueDetected,
    GeometricMedianConverged,
    GeometricMedianDiverged,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FraudEvent {
    ProofSubmitted,
    ProofAccepted,
    ProofRejected,
    RollbackInitiated,
    RollbackCompleted,
    SlashingApplied,
    BountyAwarded,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RatificationEvent {
    ProposalReceived,
    ProposalVoted,
    QuorumReached,
    ProposalActivated,
    MalignRatificationDetected,
    RegimeTransition,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AgentEvent {
    BrainFetched,
    BrainLoaded,
    BrainHashMismatch,
    InferenceCompleted,
    InferenceUnavailable,
    InferenceModelMismatch,
    InferenceLowConfidence,
    CircuitBreakerOpened,
    CircuitBreakerClosed,
}

/// Field value types. Kept narrow on purpose — extending requires schema bump.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FieldValue {
    Hash(B256),
    Integer(i64),
    Unsigned(u64),
    Float(f64),
    Boolean(bool),
    Text(String),
}

impl LogEvent {
    /// Canonical serialization for content hashing.
    ///
    /// Order: wire_version || timestamp_ms (BE) || block_height (BE) ||
    /// emitter_did_hash || kind_discriminant (single byte category +
    /// single byte kind within category) || severity (single byte) ||
    /// message_len (u32 BE) || message_bytes || field_count (u32 BE) ||
    /// for each field: key_len (u32 BE) || key_bytes || value_tag (u8) ||
    /// value_bytes (variable per type).
    ///
    /// Fields are sorted by key before serialization to make order
    /// independent of insertion sequence.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(256);
        out.extend_from_slice(&self.wire_version.to_be_bytes());
        out.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        out.extend_from_slice(&self.block_height.to_be_bytes());
        out.extend_from_slice(self.emitter_did_hash.as_slice());
        let (cat, kind) = kind_discriminant(&self.kind);
        out.push(cat);
        out.push(kind);
        out.push(severity_discriminant(self.severity));
        out.extend_from_slice(&(self.message.len() as u32).to_be_bytes());
        out.extend_from_slice(self.message.as_bytes());

        // Sort fields by key for deterministic ordering.
        let mut sorted: Vec<&(String, FieldValue)> = self.fields.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        out.extend_from_slice(&(sorted.len() as u32).to_be_bytes());
        for (key, value) in sorted {
            out.extend_from_slice(&(key.len() as u32).to_be_bytes());
            out.extend_from_slice(key.as_bytes());
            match value {
                FieldValue::Hash(h) => {
                    out.push(0x01);
                    out.extend_from_slice(h.as_slice());
                }
                FieldValue::Integer(i) => {
                    out.push(0x02);
                    out.extend_from_slice(&i.to_be_bytes());
                }
                FieldValue::Unsigned(u) => {
                    out.push(0x03);
                    out.extend_from_slice(&u.to_be_bytes());
                }
                FieldValue::Float(f) => {
                    out.push(0x04);
                    out.extend_from_slice(&f.to_be_bytes());
                }
                FieldValue::Boolean(b) => {
                    out.push(0x05);
                    out.push(if *b { 1 } else { 0 });
                }
                FieldValue::Text(t) => {
                    out.push(0x06);
                    out.extend_from_slice(&(t.len() as u32).to_be_bytes());
                    out.extend_from_slice(t.as_bytes());
                }
            }
        }
        out
    }

    /// Content hash for deduplication, replay, and training-example identity.
    /// Same canonical bytes → same hash → same training example.
    pub fn content_hash<F: Fn(&[u8]) -> [u8; 32]>(&self, hash_fn: F) -> B256 {
        const DOMAIN: &[u8] = b"spacekit-log-event-v1";
        let canon = self.to_canonical_bytes();
        let mut buf = Vec::with_capacity(DOMAIN.len() + 1 + canon.len());
        buf.extend_from_slice(DOMAIN);
        buf.push(0x1f);
        buf.extend_from_slice(&canon);
        B256::from(hash_fn(&buf))
    }

    /// Lookup helper for runbook scenario matching.
    pub fn get_field(&self, key: &str) -> Option<&FieldValue> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Convenience: extract a float field. Returns None if missing or
    /// wrong type. Runbook query DSL uses this for threshold comparisons.
    pub fn get_float(&self, key: &str) -> Option<f64> {
        match self.get_field(key)? {
            FieldValue::Float(f) => Some(*f),
            FieldValue::Integer(i) => Some(*i as f64),
            FieldValue::Unsigned(u) => Some(*u as f64),
            _ => None,
        }
    }

    pub fn get_unsigned(&self, key: &str) -> Option<u64> {
        match self.get_field(key)? {
            FieldValue::Unsigned(u) => Some(*u),
            FieldValue::Integer(i) if *i >= 0 => Some(*i as u64),
            _ => None,
        }
    }

    pub fn get_text(&self, key: &str) -> Option<&str> {
        match self.get_field(key)? {
            FieldValue::Text(t) => Some(t.as_str()),
            _ => None,
        }
    }
}

fn severity_discriminant(s: Severity) -> u8 {
    match s {
        Severity::Debug => 0,
        Severity::Info => 1,
        Severity::Notice => 2,
        Severity::Warning => 3,
        Severity::Critical => 4,
        Severity::Alert => 5,
    }
}

fn kind_discriminant(k: &EventKind) -> (u8, u8) {
    match k {
        EventKind::Consensus(e) => (
            0x01,
            match e {
                ConsensusEvent::BlockProposed => 0x01,
                ConsensusEvent::BlockSoftFinalized => 0x02,
                ConsensusEvent::BlockHardFinalized => 0x03,
                ConsensusEvent::BlockReverted => 0x04,
                ConsensusEvent::ViewChange => 0x05,
                ConsensusEvent::QuorumReached => 0x06,
                ConsensusEvent::QuorumFailed => 0x07,
                ConsensusEvent::ValidatorAdmitted => 0x08,
                ConsensusEvent::ValidatorEjected => 0x09,
            },
        ),
        EventKind::Spacetime(e) => (
            0x02,
            match e {
                SpacetimeEvent::TransitionObserved => 0x01,
                SpacetimeEvent::ResidualMismatch => 0x02,
                SpacetimeEvent::FingerprintUpdated => 0x03,
                SpacetimeEvent::FingerprintAnomalyMild => 0x04,
                SpacetimeEvent::FingerprintAnomalyStrong => 0x05,
                SpacetimeEvent::AttestationBroadcast => 0x06,
                SpacetimeEvent::AttestationMismatchDetected => 0x07,
                SpacetimeEvent::CliqueDetected => 0x08,
                SpacetimeEvent::GeometricMedianConverged => 0x09,
                SpacetimeEvent::GeometricMedianDiverged => 0x0A,
            },
        ),
        EventKind::Fraud(e) => (
            0x03,
            match e {
                FraudEvent::ProofSubmitted => 0x01,
                FraudEvent::ProofAccepted => 0x02,
                FraudEvent::ProofRejected => 0x03,
                FraudEvent::RollbackInitiated => 0x04,
                FraudEvent::RollbackCompleted => 0x05,
                FraudEvent::SlashingApplied => 0x06,
                FraudEvent::BountyAwarded => 0x07,
            },
        ),
        EventKind::Ratification(e) => (
            0x04,
            match e {
                RatificationEvent::ProposalReceived => 0x01,
                RatificationEvent::ProposalVoted => 0x02,
                RatificationEvent::QuorumReached => 0x03,
                RatificationEvent::ProposalActivated => 0x04,
                RatificationEvent::MalignRatificationDetected => 0x05,
                RatificationEvent::RegimeTransition => 0x06,
            },
        ),
        EventKind::Agent(e) => (
            0x05,
            match e {
                AgentEvent::BrainFetched => 0x01,
                AgentEvent::BrainLoaded => 0x02,
                AgentEvent::BrainHashMismatch => 0x03,
                AgentEvent::InferenceCompleted => 0x04,
                AgentEvent::InferenceUnavailable => 0x05,
                AgentEvent::InferenceModelMismatch => 0x06,
                AgentEvent::InferenceLowConfidence => 0x07,
                AgentEvent::CircuitBreakerOpened => 0x08,
                AgentEvent::CircuitBreakerClosed => 0x09,
            },
        ),
        EventKind::Service(e) => (
            0x06,
            match e {
                service::ServiceEvent::ProposalAccepted => 0x01,
                service::ServiceEvent::VoteCorrect => 0x02,
                service::ServiceEvent::EnvelopeSigned => 0x03,
                service::ServiceEvent::UptimeConfirmed => 0x04,
                service::ServiceEvent::ContractExecuted => 0x05,
                service::ServiceEvent::HostHookInvoked => 0x06,
                service::ServiceEvent::BlobServedRead => 0x07,
                service::ServiceEvent::BlobServedWrite => 0x08,
                service::ServiceEvent::ProofAttested => 0x09,
                service::ServiceEvent::CapacityMaintained => 0x0A,
                service::ServiceEvent::MessageDelivered => 0x0B,
                service::ServiceEvent::BroadcastSent => 0x0C,
                service::ServiceEvent::KeyResolved => 0x0D,
            },
        ),
    }
}

/// Runbook scenario query — matches a LogEvent against a structured pattern.
/// Each runbook entry has one or more `ScenarioQuery`s; if any matches, the
/// runbook's `recommended_action` is the agent's training label for that event.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ScenarioQuery {
    /// Required: event kind must match exactly.
    pub kind: EventKind,
    /// Optional: severity at or above this level.
    pub min_severity: Option<Severity>,
    /// Optional: structured field predicates. ALL must match for the
    /// query to fire.
    pub field_predicates: Vec<FieldPredicate>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FieldPredicate {
    /// Field exists with any value.
    Exists(String),
    /// Float field >= threshold.
    FloatAtLeast(String, f64),
    /// Float field <= threshold.
    FloatAtMost(String, f64),
    /// Unsigned field equals.
    UnsignedEquals(String, u64),
    /// Text field equals.
    TextEquals(String, String),
}

impl ScenarioQuery {
    pub fn matches(&self, event: &LogEvent) -> bool {
        if &self.kind != &event.kind {
            return false;
        }
        if let Some(min) = self.min_severity {
            if severity_discriminant(event.severity) < severity_discriminant(min) {
                return false;
            }
        }
        for predicate in &self.field_predicates {
            if !match predicate {
                FieldPredicate::Exists(k) => event.get_field(k).is_some(),
                FieldPredicate::FloatAtLeast(k, t) => event.get_float(k).map_or(false, |v| v >= *t),
                FieldPredicate::FloatAtMost(k, t) => event.get_float(k).map_or(false, |v| v <= *t),
                FieldPredicate::UnsignedEquals(k, e) => {
                    event.get_unsigned(k).map_or(false, |v| v == *e)
                }
                FieldPredicate::TextEquals(k, e) => {
                    event.get_text(k).map_or(false, |v| v == e.as_str())
                }
            } {
                return false;
            }
        }
        true
    }
}

/// Helper for building events. Use in the emitting code path:
///
/// ```ignore
/// use spacekit_log::*;
/// let event = LogEventBuilder::new(EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyStrong))
///     .severity(Severity::Warning)
///     .at_block(block_height)
///     .by(emitter_did_hash)
///     .message("validator centroid distance 1.4")
///     .field("validator_did", FieldValue::Hash(suspect_did))
///     .field("centroid_distance", FieldValue::Float(1.4))
///     .field("sigma_threshold", FieldValue::Float(5.0))
///     .build(now_ms);
/// ```
pub struct LogEventBuilder {
    kind: EventKind,
    severity: Severity,
    block_height: u64,
    emitter_did_hash: B256,
    message: String,
    fields: Vec<(String, FieldValue)>,
}

impl LogEventBuilder {
    pub fn new(kind: EventKind) -> Self {
        Self {
            kind,
            severity: Severity::Info,
            block_height: 0,
            emitter_did_hash: B256::ZERO,
            message: String::new(),
            fields: Vec::new(),
        }
    }
    pub fn severity(mut self, s: Severity) -> Self {
        self.severity = s;
        self
    }
    pub fn at_block(mut self, h: u64) -> Self {
        self.block_height = h;
        self
    }
    pub fn by(mut self, did: B256) -> Self {
        self.emitter_did_hash = did;
        self
    }
    pub fn message(mut self, m: impl Into<String>) -> Self {
        self.message = m.into();
        self
    }
    pub fn field(mut self, key: impl Into<String>, value: FieldValue) -> Self {
        self.fields.push((key.into(), value));
        self
    }
    pub fn build(self, timestamp_ms: u64) -> LogEvent {
        LogEvent {
            wire_version: LOG_WIRE_VERSION,
            timestamp_ms,
            block_height: self.block_height,
            emitter_did_hash: self.emitter_did_hash,
            kind: self.kind,
            severity: self.severity,
            message: self.message,
            fields: self.fields,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn h(b: &[u8]) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (i, byte) in b.iter().enumerate() {
            out[i % 32] = out[i % 32].wrapping_add(byte.wrapping_mul(31));
        }
        out
    }

    fn sample_event() -> LogEvent {
        LogEventBuilder::new(EventKind::Spacetime(
            SpacetimeEvent::FingerprintAnomalyStrong,
        ))
        .severity(Severity::Warning)
        .at_block(1234)
        .by(B256::from([0xAA; 32]))
        .message("validator centroid distance 1.4")
        .field("validator_did", FieldValue::Hash(B256::from([0xBB; 32])))
        .field("centroid_distance", FieldValue::Float(1.4))
        .field("sigma_threshold", FieldValue::Float(5.0))
        .build(1_700_000_000_000)
    }

    #[test]
    fn content_hash_is_deterministic() {
        let a = sample_event();
        let b = sample_event();
        assert_eq!(a.content_hash(h), b.content_hash(h));
    }

    #[test]
    fn different_events_produce_different_hashes() {
        let a = sample_event();
        let mut b = sample_event();
        b.block_height = 1235;
        assert_ne!(a.content_hash(h), b.content_hash(h));
    }

    #[test]
    fn field_order_does_not_affect_hash() {
        let a = LogEventBuilder::new(EventKind::Spacetime(SpacetimeEvent::TransitionObserved))
            .at_block(10)
            .by(B256::ZERO)
            .message("")
            .field("a", FieldValue::Float(1.0))
            .field("b", FieldValue::Float(2.0))
            .build(0);
        let b = LogEventBuilder::new(EventKind::Spacetime(SpacetimeEvent::TransitionObserved))
            .at_block(10)
            .by(B256::ZERO)
            .message("")
            .field("b", FieldValue::Float(2.0))
            .field("a", FieldValue::Float(1.0))
            .build(0);
        assert_eq!(a.content_hash(h), b.content_hash(h));
    }

    #[test]
    fn scenario_query_matches_correct_event() {
        let event = sample_event();
        let q = ScenarioQuery {
            kind: EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyStrong),
            min_severity: Some(Severity::Warning),
            field_predicates: vec![
                FieldPredicate::FloatAtLeast("centroid_distance".into(), 1.0),
                FieldPredicate::FloatAtMost("centroid_distance".into(), 2.0),
            ],
        };
        assert!(q.matches(&event));
    }

    #[test]
    fn scenario_query_rejects_below_threshold() {
        let mut event = sample_event();
        // Replace centroid_distance with a value below the query threshold.
        for (k, v) in event.fields.iter_mut() {
            if k == "centroid_distance" {
                *v = FieldValue::Float(0.5);
            }
        }
        let q = ScenarioQuery {
            kind: EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyStrong),
            min_severity: None,
            field_predicates: vec![FieldPredicate::FloatAtLeast(
                "centroid_distance".into(),
                1.0,
            )],
        };
        assert!(!q.matches(&event));
    }

    #[test]
    fn scenario_query_rejects_wrong_kind() {
        let event = sample_event();
        let q = ScenarioQuery {
            kind: EventKind::Spacetime(SpacetimeEvent::CliqueDetected),
            min_severity: None,
            field_predicates: vec![],
        };
        assert!(!q.matches(&event));
    }

    #[test]
    fn severity_below_min_rejected() {
        let event = sample_event(); // severity Warning
        let q = ScenarioQuery {
            kind: EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyStrong),
            min_severity: Some(Severity::Critical), // higher than Warning
            field_predicates: vec![],
        };
        assert!(!q.matches(&event));
    }

    #[test]
    fn service_event_sra_bridge() {
        use service::ServiceEvent;
        let event = LogEventBuilder::new(EventKind::Service(ServiceEvent::ContractExecuted))
            .at_block(42)
            .by(B256::from([0xCC; 32]))
            .message("contract executed")
            .field(service::FIELD_RESOURCE_UNITS, FieldValue::Unsigned(50_000))
            .build(0);
        let bridge = event.to_sra_swtchvm_log().expect("bridge");
        assert_eq!(bridge.data, 50_000u128.to_le_bytes());
        assert_eq!(
            event.sra_topic_label(),
            Some(service::topics::COMPUTE_CONTRACT_EXECUTED)
        );
    }
}
