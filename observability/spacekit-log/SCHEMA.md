# `spacekit-log` Event Schema Reference

Single source of truth for what events look like on disk and on the wire.
Operators grep these; runbooks query these; agent training replays these.

## Schema version

Current: `1`. Bumped on any field rename, kind removal, or value-tag
change. Within a major version, new kinds and new optional fields are
backward-compatible.

## JSON encoding

Each event is one JSON line in the on-disk log:

```json
{
  "wire_version": 1,
  "timestamp_ms": 1700000000000,
  "block_height": 1234,
  "emitter_did_hash": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  "kind": {"Spacetime": "FingerprintAnomalyStrong"},
  "severity": "Warning",
  "message": "validator centroid distance 1.4",
  "fields": [
    ["validator_did", {"Hash": "0xbbbb..."}],
    ["centroid_distance", {"Float": 1.4}],
    ["sigma_threshold", {"Float": 5.0}]
  ]
}
```

Note the field order in `fields` is preserved on disk, but the content
hash sorts them — so two emitters logging the same event in different
field orders produce the same hash.

## Field naming conventions

| Convention | Example | Why |
|------------|---------|-----|
| `snake_case` keys | `validator_did`, `centroid_distance` | grep-friendly |
| Hashes as `B256` hex | `"0xaa...aa"` | Direct match against on-chain values |
| Times as Unix ms | `1700000000000` | Cross-emitter ordering |
| Counts as `Unsigned` | `42` | No negative populations |
| Continuous metrics as `Float` | `1.4` | Matches the agent training corpus |

## Standard fields by event kind

These fields are **expected** for each event kind. Emitting code that
omits them is contract-violating.

### Consensus

| Kind | Required fields |
|------|-----------------|
| `BlockProposed` | `proposer_did`, `round`, `view`, `block_hash` |
| `BlockSoftFinalized` | `block_hash`, `quorum_weight` (Float) |
| `BlockHardFinalized` | `block_hash`, `challenge_window_blocks` (Unsigned) |
| `BlockReverted` | `block_hash`, `fraud_proof_hash` |
| `ViewChange` | `from_view`, `to_view`, `proposer_did` |
| `QuorumReached` | `round`, `view`, `weight` (Float) |
| `QuorumFailed` | `round`, `view`, `weight` (Float) |
| `ValidatorAdmitted` | `validator_did`, `initial_stake` (Unsigned) |
| `ValidatorEjected` | `validator_did`, `reason` (Text) |

### Spacetime

| Kind | Required fields |
|------|-----------------|
| `TransitionObserved` | `transition_id`, `proposer_did`, `rotor_magnitude` (Float), `residual_norm` (Float) |
| `ResidualMismatch` | `validator_did`, `claimed_commit`, `computed_commit`, `residual_delta` (Float) |
| `FingerprintUpdated` | `validator_did`, `new_centroid_distance` (Float), `samples` (Unsigned) |
| `FingerprintAnomalyMild` | `validator_did`, `centroid_distance` (Float), `sigma_threshold` (Float) |
| `FingerprintAnomalyStrong` | `validator_did`, `centroid_distance` (Float), `sigma_threshold` (Float) |
| `AttestationBroadcast` | `validator_did`, `fingerprint_root` |
| `AttestationMismatchDetected` | `validator_a`, `validator_b`, `root_a`, `root_b` |
| `CliqueDetected` | `validator_count` (Unsigned), `avg_rotor_distance` (Float), `coordination_score` (Float) |
| `GeometricMedianConverged` | `iterations` (Unsigned), `max_divergence` (Float) |
| `GeometricMedianDiverged` | `iterations` (Unsigned), `step_norm` (Float) |

### Fraud

| Kind | Required fields |
|------|-----------------|
| `ProofSubmitted` | `submitter_did`, `target_height`, `target_block_hash`, `proof_type` (Text) |
| `ProofAccepted` | `submitter_did`, `target_height`, `rolled_back_count` (Unsigned) |
| `ProofRejected` | `submitter_did`, `target_height`, `rejection_reason` (Text) |
| `RollbackInitiated` | `target_height`, `affected_block_count` (Unsigned) |
| `RollbackCompleted` | `target_height`, `restored_height` |
| `SlashingApplied` | `validator_did`, `severity` (Text), `evidence_hash` |
| `BountyAwarded` | `recipient_did`, `amount` (Unsigned) |

### Ratification

| Kind | Required fields |
|------|-----------------|
| `ProposalReceived` | `proposal_id`, `proposer_did`, `action_target` (Text), `proposed_value` (Float) |
| `ProposalVoted` | `proposal_id`, `voter_did`, `vote` (Boolean) |
| `QuorumReached` | `proposal_id`, `yes_ratio` (Float) |
| `ProposalActivated` | `proposal_id`, `action_target` (Text), `old_value` (Float), `new_value` (Float), `activated_at_height` |
| `MalignRatificationDetected` | `proposal_id`, `bad_voter_did`, `safety_window_blocks` (Unsigned) |
| `RegimeTransition` | `from_regime` (Text), `to_regime` (Text), `trigger` (Text) |

### Service (SRA / operator rewards)

Emitted when an operator performs billable work. Consumed by the Service Reward
Accumulator; each kind maps to a canonical SwtchVM log `topic0` label
(`compute.contract.executed`, etc.).

| Kind | Required fields | SRA topic |
|------|-----------------|-----------|
| `ProposalAccepted` | `proposer_did`, `block_hash` | `consensus.proposal.accepted` |
| `VoteCorrect` | `voter_did`, `proposal_id` | `consensus.vote.correct` |
| `EnvelopeSigned` | `signer_did`, `block_hash` | `consensus.envelope.signed` |
| `UptimeConfirmed` | `validator_did`, `uptime_fraction` (Float) | `consensus.uptime.confirmed` |
| `ContractExecuted` | `resource_units` (Unsigned) | `compute.contract.executed` |
| `HostHookInvoked` | `resource_units` (Unsigned), `hook_name` (Text) | `compute.host_hook.invoked` |
| `BlobServedRead` | `resource_units` (Unsigned), `bytes` (Unsigned) | `storage.blob.served_read` |
| `BlobServedWrite` | `resource_units` (Unsigned), `bytes` (Unsigned) | `storage.blob.served_write` |
| `ProofAttested` | `resource_units` (Unsigned), `bytes_proven` (Unsigned) | `storage.proof.attested` |
| `CapacityMaintained` | `resource_units` (Unsigned), `byte_hours` (Unsigned) | `storage.capacity.maintained` |
| `MessageDelivered` | `resource_units` (Unsigned) | `messaging.message.delivered` |
| `BroadcastSent` | `resource_units` (Unsigned), `recipient_count` (Unsigned) | `messaging.broadcast.sent` |
| `KeyResolved` | `resource_units` (Unsigned) | `messaging.key.resolved` |

### Agent

| Kind | Required fields |
|------|-----------------|
| `BrainFetched` | `model_hash`, `size_bytes` (Unsigned), `source_url` (Text) |
| `BrainLoaded` | `model_hash`, `from_disk` (Boolean) |
| `BrainHashMismatch` | `expected_hash`, `actual_hash`, `source_url` (Text) |
| `InferenceCompleted` | `task_id` (Text), `domain` (Text), `semantic_intent` (Text), `confidence` (Float) |
| `InferenceUnavailable` | `task_id` (Text), `reason` (Text) |
| `InferenceModelMismatch` | `task_id` (Text), `expected_model_hash`, `actual_model_hash` |
| `InferenceLowConfidence` | `task_id` (Text), `confidence` (Float) |
| `CircuitBreakerOpened` | `consecutive_failures` (Unsigned), `reopen_at_height` (Unsigned) |
| `CircuitBreakerClosed` | `total_failures_observed` (Unsigned) |

## Severity guidance

- `Debug` — verbose diagnostic, off by default.
- `Info` — happy-path lifecycle events. Block proposed, brain loaded.
- `Notice` — unusual but expected. Inference unavailable due to timeout, mild fingerprint anomaly.
- `Warning` — operator should look. Strong fingerprint anomaly, clique with `coordination_score > 2.0`, circuit breaker opened.
- `Critical` — correctness or safety at risk. Attestation mismatch detected, fraud proof accepted, brain hash mismatch.
- `Alert` — incident response activation. Network attack classified, regime transition to Secure, malign ratification detected.

## Common grep recipes

Find all anomaly events for a specific validator in a block range:
```bash
jq -c 'select(.kind.Spacetime == "FingerprintAnomalyStrong"
  and .block_height >= 1000 and .block_height <= 2000
  and (.fields[] | select(.[0] == "validator_did" and .[1].Hash == "0xbb..."))
)' spacekit.log
```

Find all critical-or-above events in the last hour:
```bash
jq -c 'select((.severity == "Critical" or .severity == "Alert")
  and .timestamp_ms >= (now - 3600) * 1000
)' spacekit.log
```

Count fraud proofs by submitter over the past day:
```bash
jq -c 'select(.kind.Fraud == "ProofSubmitted"
  and .timestamp_ms >= (now - 86400) * 1000
) | .fields[] | select(.[0] == "submitter_did") | .[1].Hash' spacekit.log \
| sort | uniq -c | sort -rn
```
