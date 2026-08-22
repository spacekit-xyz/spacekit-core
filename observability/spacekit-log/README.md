# spacekit-log

Structured event-logging contract for SpaceKit nodes. One schema, three
consumers: operators (grep), runbooks (scenario queries), and
`SpacetimeConsensusAgent` training (deterministic content hashes).

**License:** Apache-2.0.

**Status:** Schema locked at wire version 1. Library stable; no breaking
changes without a wire-version bump.

---

## What this crate is

A small, dependency-light Rust library that defines:

- The on-disk and on-wire format for SpaceKit consensus events
- An ergonomic builder (`LogEventBuilder`) for emitting events from
  consensus and agent code paths
- A query primitive (`ScenarioQuery`) for matching events against
  runbook scenarios
- A deterministic content hash any consumer can recompute

It is intentionally minimal. No I/O, no async, no logging framework
integration. Pure data types + serialization. Consumers wrap it with
whatever sink, transport, or storage they need.

---

## Why a separate crate

`spacekit-log` is **upstream of everything else** in the SpaceKit stack
that touches events:

```
spacekit-log (this crate)
    ↑
    │ depended on by
    │
    ├── spacekit-compute-node      (emits events at consensus sites)
    ├── spacekit-runbook           (consumes events to match scenarios)
    └── spacetime-consensus-agent  (consumes events during training)
```

The schema's value comes from being slim, stable, and easy to depend on.
Three properties to preserve:

- **No transitive dependencies the consumers don't already have.** Only
  `alloy-primitives` and optionally `serde`. No async runtime, no
  logging framework, no consensus types.
- **No breaking changes within a wire version.** Adding new event kinds
  or optional fields is backward-compatible. Renaming, removing, or
  changing field types is a wire-version bump and a hard fork.
- **Independently publishable to crates.io.** The discipline of being
  a standalone crate is what keeps it from accidentally becoming
  entangled.

---

## Schema at a glance

Every event has:

```rust
LogEvent {
    wire_version: u16,               // schema version, currently 1
    timestamp_ms: u64,               // Unix milliseconds
    block_height: u64,               // anchors event in chain time
    emitter_did_hash: B256,          // which node emitted it
    kind: EventKind,                 // category + variant
    severity: Severity,              // Debug..Alert
    message: String,                 // free-text human-readable
    fields: Vec<(String, FieldValue)>, // structured key-value pairs
}
```

Six top-level event categories:

| Category | Examples |
|----------|----------|
| `Consensus` | BlockProposed, BlockSoftFinalized, QuorumFailed, ViewChange |
| `Spacetime` | TransitionObserved, ResidualMismatch, FingerprintAnomalyStrong, CliqueDetected |
| `Fraud` | ProofSubmitted, ProofAccepted, RollbackInitiated, SlashingApplied |
| `Ratification` | ProposalReceived, ProposalActivated, RegimeTransition |
| `Agent` | BrainFetched, BrainHashMismatch, InferenceCompleted, CircuitBreakerOpened |
| `Service` | ContractExecuted, BlobServedWrite, MessageDelivered, VoteCorrect (SRA input) |

Full event-kind enumeration and required-field reference in
[`SCHEMA.md`](SCHEMA.md).

---

## Quickstart

### Emitting an event

```rust
use spacekit_log::{
    LogEventBuilder, EventKind, SpacetimeEvent, Severity, FieldValue
};

let event = LogEventBuilder::new(
        EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyStrong)
    )
    .severity(Severity::Warning)
    .at_block(current_height)
    .by(my_did_hash)
    .message("Validator centroid distance 1.4")
    .field("validator_did", FieldValue::Hash(suspect_did))
    .field("centroid_distance", FieldValue::Float(1.4))
    .field("sigma_threshold", FieldValue::Float(5.0))
    .build(now_ms);

// Pass to your sink — see consumer crates for FileLogSink, etc.
my_sink.emit(event);
```

### Querying events (runbook scenario matching)

```rust
use spacekit_log::{ScenarioQuery, EventKind, SpacetimeEvent, Severity, FieldPredicate};

let query = ScenarioQuery {
    kind: EventKind::Spacetime(SpacetimeEvent::FingerprintAnomalyStrong),
    min_severity: Some(Severity::Warning),
    field_predicates: vec![
        FieldPredicate::FloatAtLeast("centroid_distance".into(), 1.0),
    ],
};

if query.matches(&event) {
    // scenario claims this event
}
```

### Content hashing for dedup and training-data identity

```rust
let hash = event.content_hash(|bytes| {
    use sha3::{Digest, Keccak256};
    let mut h = Keccak256::new();
    h.update(bytes);
    h.finalize().into()
});
// Same event content → same hash, regardless of when or where emitted.
```

---

## On-disk JSON format

Each event serializes to one JSON line:

```json
{
  "wire_version": 1,
  "timestamp_ms": 1700000000000,
  "block_height": 1234,
  "emitter_did_hash": "0xaaaa...",
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

Field order on disk is preserved for human readability, but the content
hash sorts fields alphabetically — so two emitters logging the same
event in different field orders produce the same hash. This is what
makes the hash safe to use as a primary key.

See [`SCHEMA.md`](SCHEMA.md) for full encoding details, common grep
recipes, and the per-event-kind required-field reference.

---

## Determinism rules

The content hash is the consumer-visible primary key. For it to be
useful, three properties must hold:

1. **Same fields → same hash.** Field-order independence enforced by
   the canonical serializer sorting fields by key before hashing.
2. **Same hash → same event content.** Domain-tagged hashing
   (`b"spacekit-log-event-v1"` prefix) prevents collision with other
   hash uses in the system.
3. **Stable across builds, platforms, and Rust versions.** Achieved by
   pinning byte-order (big-endian for integers), float bit-representation
   (`f64::to_be_bytes`), and field-tag bytes.

If a code change causes a previously-emitted event to hash differently,
that is a wire-version-breaking change.

---

## Features

- `default` — std + serde
- `std` — standard library (turn off for `no_std` environments)
- `serde` — `Serialize`/`Deserialize` for all types

No async runtime dependency. The crate is sync; consumers wrap with
their own async sinks if needed.

---

## What this crate does NOT do

- **No I/O.** No file writes, no network sends, no database persistence.
  Consumers implement their own `LogSink` trait (see
  `spacekit-runbook/EMISSION_SKETCH.md` for the recommended pattern).
- **No log levels / verbose / etc.** Use the `Severity` enum, which
  reflects operational urgency rather than verbosity.
- **No filtering or rate-limiting.** Consumers handle back-pressure.
- **No replay / event-sourcing.** The content hash supports
  deduplication and replay, but the crate itself does not orchestrate
  either.
- **No correlation IDs or distributed tracing.** Add those at the consumer
  layer if needed; the schema can be extended with optional fields.

---

## Versioning

`LOG_WIRE_VERSION = 1` is locked at testnet. Changes to the wire format
require:

1. A new wire-version constant
2. A migration path for existing logs
3. A coordinated update across all consumer crates
4. A hard-fork-equivalent operator notification

Additive changes (new event kinds, new optional field types) within
wire version 1 are backward-compatible and do not require a version bump.

---

## Build

```bash
cargo build
cargo test
```

No external services required for the crate itself. Tests are
self-contained.

---

Made with care by the SpaceKit.xyz team.