# Log Emission Sketch for `spacekit-compute-node`

This document sketches where `spacekit-log` events should be emitted in
the existing `spacekit-compute-node` codebase. It is a pattern guide, not
a complete enumeration — the engineer applying it adapts each call site
to the local idioms.

## The pattern

Every emission site follows the same three-step structure:

1. **Construct** the event using `LogEventBuilder`.
2. **Populate** the required fields per the SCHEMA.md contract.
3. **Submit** to the local logger (`log_sink.emit(event)`).

```rust
use spacekit_log::{
    LogEventBuilder, EventKind, SpacetimeEvent, Severity, FieldValue
};

let event = LogEventBuilder::new(
        EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyStrong)
    )
    .severity(Severity::Warning)
    .at_block(current_height)
    .by(self.my_did_hash)
    .message("Validator centroid distance 1.4")
    .field("validator_did", FieldValue::Hash(suspect_did))
    .field("centroid_distance", FieldValue::Float(centroid_distance))
    .field("sigma_threshold", FieldValue::Float(self.sigma_threshold))
    .build(now_ms());

self.log_sink.emit(event);
```

The `log_sink` is a single field on `ConsensusCoordinator` holding any
type implementing `LogSink` (trait shown below). Production uses a sink
that writes to local JSONL; tests use a `MockSink` that captures events
for assertion.

## Trait

```rust
pub trait LogSink: Send + Sync {
    fn emit(&self, event: spacekit_log::LogEvent);
}

pub struct FileLogSink {
    writer: Mutex<BufWriter<File>>,
    rotation_threshold_bytes: u64,
}

impl LogSink for FileLogSink {
    fn emit(&self, event: spacekit_log::LogEvent) {
        let line = match serde_json::to_string(&event) {
            Ok(s) => s,
            Err(_) => return, // never panic in the emit path
        };
        if let Ok(mut w) = self.writer.lock() {
            let _ = writeln!(w, "{}", line);
            // (rotation logic elided for sketch)
        }
    }
}
```

## Emission sites by module

### `ConsensusCoordinator` — PBFT lifecycle

| When | Event |
|------|-------|
| Proposer's block goes out on P2P | `Consensus::BlockProposed` (Info) |
| `pq_finisher::finalize_proposal_if_ready` succeeds | `Consensus::BlockSoftFinalized` (Info) |
| `TieredFinality::on_soft_finalize` promotes a block | `Consensus::BlockHardFinalized` (Info) |
| Fraud proof triggers `on_fraud_proof_accepted` | `Consensus::BlockReverted` (Critical) |
| View change initiated | `Consensus::ViewChange` (Notice) |
| 2/3 quorum reached in `collect_weighted_votes` | `Consensus::QuorumReached` (Info) |
| `has_consensus()` returns false | `Consensus::QuorumFailed` (Warning) |
| New validator admitted | `Consensus::ValidatorAdmitted` (Notice) |
| Validator ejected (stake or reputation) | `Consensus::ValidatorEjected` (Notice) |

```rust
// At the end of pq_finisher::finalize_proposal_if_ready, on success:
self.log_sink.emit(
    LogEventBuilder::new(EventKind::Consensus(ConsensusEvent::BlockSoftFinalized))
        .severity(Severity::Info)
        .at_block(block.height)
        .by(self.my_did_hash)
        .message(format!("Block {} soft-finalized", block.height))
        .field("block_hash", FieldValue::Hash(block.hash()))
        .field("quorum_weight", FieldValue::Float(voting_result.supporting_power))
        .build(now_ms())
);
```

### Spacetime extension — rotor and fingerprint lifecycle

| When | Event |
|------|-------|
| `SpacetimeExtension::compute_transition` runs | `Spacetime::TransitionObserved` (Info) |
| `verify_transition` returns `TransitionMismatch` due to residual | `Spacetime::ResidualMismatch` (Critical) |
| `FingerprintVerkle::apply_batch` updates a fingerprint | `Spacetime::FingerprintUpdated` (Info) |
| `FingerprintRegistry::check_joint` returns true at sigma < 6 | `Spacetime::FingerprintAnomalyMild` (Notice) |
| `FingerprintRegistry::check_joint` returns true at sigma >= 6 | `Spacetime::FingerprintAnomalyStrong` (Warning) |
| Attestation broadcast in `apply_block_spacetime_side_effects` | `Spacetime::AttestationBroadcast` (Info) |
| `FingerprintAttestationCollector::detect_mismatches` returns nonempty | `Spacetime::AttestationMismatchDetected` (Critical) |
| `detect_coordination_clique` finds clique with score >5 | `Spacetime::CliqueDetected` (Warning) |
| `aggregate_rotors` succeeds | `Spacetime::GeometricMedianConverged` (Debug) |
| `geometric_median_rotor` exceeds max iterations | `Spacetime::GeometricMedianDiverged` (Warning) |

```rust
// In SpacetimeExtension::verify_transition, on residual mismatch:
if computed_commit != transition.residual_commitment {
    self.log_sink.emit(
        LogEventBuilder::new(EventKind::Spacetime(SpacetimeEvent::ResidualMismatch))
            .severity(Severity::Critical)
            .at_block(transition.transition_id)
            .by(self.my_did_hash)
            .message("Residual commitment mismatch")
            .field("validator_did", FieldValue::Hash(proposer_did))
            .field("claimed_commit", FieldValue::Hash(transition.residual_commitment))
            .field("computed_commit", FieldValue::Hash(computed_commit))
            .field("residual_delta", FieldValue::Float(computed_norm - transition.residual_norm))
            .build(now_ms())
    );
    return Err(SpacetimeError::TransitionMismatch);
}
```

### Fraud-proof flow

| When | Event |
|------|-------|
| `submit_fraud_proof` called | `Fraud::ProofSubmitted` (Critical) |
| `submit_fraud_proof` succeeds | `Fraud::ProofAccepted` (Critical) |
| `submit_fraud_proof` rejects | `Fraud::ProofRejected` (Warning) |
| `on_fraud_proof_accepted` starts rollback | `Fraud::RollbackInitiated` (Critical) |
| All `rollback_block_spacetime_side_effects` calls complete | `Fraud::RollbackCompleted` (Critical) |
| Slashing applied to a validator | `Fraud::SlashingApplied` (Critical) |
| Bounty paid to submitter | `Fraud::BountyAwarded` (Info) |

### Ratification flow

| When | Event |
|------|-------|
| Coordinator receives `ParameterChangeProposal` | `Ratification::ProposalReceived` (Info) |
| Coordinator casts `ParameterChangeVote` | `Ratification::ProposalVoted` (Info) |
| `evaluate_ratification` returns Ok with quorum | `Ratification::QuorumReached` (Notice) |
| Activation height reached, new parameter binds | `Ratification::ProposalActivated` (Notice) |
| `MalignRatificationEvidence` constructed | `Ratification::MalignRatificationDetected` (Critical) |
| Policy regime transitions | `Ratification::RegimeTransition` (Alert) |

### Agent client

| When | Event |
|------|-------|
| Brain bytes downloaded from storage | `Agent::BrainFetched` (Info) |
| `GrowformerRuntime::load_from_bytes` succeeds | `Agent::BrainLoaded` (Info) |
| Brain hash check fails at startup | `Agent::BrainHashMismatch` (Critical) |
| `infer_batch` returns Ok | `Agent::InferenceCompleted` (Debug) |
| `infer_batch` returns Unavailable | `Agent::InferenceUnavailable` (Notice) |
| `infer_batch` returns ModelMismatch | `Agent::InferenceModelMismatch` (Critical) |
| `infer_batch` returns LowConfidence | `Agent::InferenceLowConfidence` (Notice) |
| `GrowformerHealth::should_call` returns false | `Agent::CircuitBreakerOpened` (Warning) |
| Circuit breaker reset | `Agent::CircuitBreakerClosed` (Info) |

## What NOT to emit

- **Per-message PBFT debug.** The pq_finisher already produces enough Info events; verbose per-vote logging belongs in the `Debug` severity behind a runtime flag.
- **Recoverable internal errors.** A retry loop that succeeds on the second attempt does not need an Error-level log. Use `Debug` or skip entirely.
- **Anything from a tight loop without sampling.** If you'd emit 1000 events per block from a single site, sample (1-in-N) or batch.

## What MUST be emitted

These are non-negotiable:

- Every `Critical` or `Alert` event in the table above. Operators rely on these.
- Every state change to `TieredFinality` (the state machine is opaque without logs).
- Every fraud-proof acceptance + rollback path. These are recovery-relevant and become evidence.
- Every brain hash mismatch. Supply-chain attacks announce themselves here.

## Bottleneck: do not block consensus on log writes

The `LogSink::emit` call is on the hot path. The production `FileLogSink`
must:

1. Write asynchronously (background thread, bounded channel).
2. Drop events under back-pressure rather than block.
3. Emit a `Sink::DroppedEvents` counter (in metrics, not in the log) when
   it drops.

Operators investigating a stuck consensus look at logs FIRST. The logs
must not be what's stuck.

## Testing emission

A `MockSink` collects emitted events for assertion in tests:

```rust
pub struct MockSink {
    pub events: Mutex<Vec<LogEvent>>,
}

impl LogSink for MockSink {
    fn emit(&self, event: LogEvent) {
        self.events.lock().unwrap().push(event);
    }
}
```

Use in tests to confirm: "this code path emits exactly this event":

```rust
#[test]
fn fraud_acceptance_emits_logs() {
    let sink = Arc::new(MockSink::default());
    let mut coordinator = make_coordinator_with_sink(sink.clone());
    coordinator.submit_fraud_proof(submission).await.unwrap();
    let events = sink.events.lock().unwrap();
    assert!(events.iter().any(|e| matches!(e.kind,
        EventKind::Fraud(FraudEvent::ProofAccepted))));
}
```

This is how you keep emission in sync with code: tests assert that
specific code paths produce specific events. Drift becomes a test failure.

## Rollout order

If you're applying this to spacekit-compute-node incrementally:

1. **Add the trait + FileLogSink + MockSink.** No emissions yet; just the
   plumbing.
2. **Emit the Critical and Alert events first.** These are what
   operators page on.
3. **Emit the state-machine transitions** (TieredFinality, Ratification).
   These are what runbooks query against.
4. **Add the Info/Notice events.** Useful but not load-bearing.
5. **Emit Debug events behind a flag.** Operator opts in during
   diagnosis; off by default in production.

Aim to land steps 1-3 before the first testnet. Steps 4-5 can follow.
