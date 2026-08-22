# spacekit-runbook

Operational runbook **and** corpus-generation tool for the SpaceKit
spacetime consensus protocol.

This repo serves two audiences:

- **Operators** during incidents — `S-*.yaml` scenarios, response
  procedures in `procedures/`.
- **Engineers** building the agent training pipeline — the Rust CLI in
  this directory consumes scenarios + production logs and produces JSONL training
  corpora for `SpacetimeConsensusAgent`.

Both views work from the same source of truth: each YAML scenario carries
a `recommended_agent_classification` that doubles as the training label
for any log event matching the scenario's `event_queries`. A scenario
change is a documentation change AND a training-data change.

License: Apache-2.0.

---

## Repo layout

```
spacekit-runbook/
├── Cargo.toml
├── README.md                       ← this file
├── EMISSION_SKETCH.md              ← where to emit log events from spacekit-compute-node
├── TRAINING_INTEGRATION.md         ← how scenarios become JSONL training rows
│
├── main.rs / scenario.rs / ...     ← CLI, YAML parser, corpus and log reader
├── S-*.yaml                        ← operator + training scenarios
└── procedures/                     ← reusable response procedures
    ├── P-01-startup-triage.md
    ├── P-02-peer-partition.md
    ├── P-04-manifest-integrity.md
    ├── P-05-key-rotation.md
    ├── P-06-validator-admission.md
    ├── P-07-storage-federation.md
    └── P-08-rollback.md
```

---

# For operators

This section is what you reach for at 3am when something is wrong.

## Quick triage (decision tree)

```
Is the node refusing to start?
  ├─ Yes → S-001-node-startup-failure.yaml
  └─ No → continue
Is consensus stalled (no new blocks)?
  ├─ Yes → S-002-consensus-stalled.yaml
  └─ No → continue
Are fraud proofs being submitted?
  ├─ Yes, accepted → S-003-fraud-proof-accepted.yaml
  └─ No → continue
Is Growformer agent behavior unusual?
  ├─ Brain hash mismatch → S-005-brain-hash-mismatch.yaml
  └─ No → continue
Are fingerprint anomalies surging?
  ├─ Multiple correlated validators → S-009-coordinated-wake-up.yaml
  └─ No → continue
Is a transition's residual commitment failing verification?
  └─ S-013-residual-commitment-mismatch.yaml
```

## Scenario index

Each scenario file has:

- `scenario_id` — stable identifier referenced by agent training
- `summary` — one-line description
- `severity_floor` — lowest severity at which this scenario triggers
- `event_queries` — `spacekit-log` `ScenarioQuery`s that detect the scenario
- `diagnosis_steps` — what an operator does first
- `procedures` — what to do at each diagnostic outcome
- `recommended_agent_classification` — what the agent should output when
  it sees a matching event (the training label)
- `escalation` — when to wake someone up
- `version` — bumped when expectations change

| Scenario ID | Summary | Severity |
|-------------|---------|----------|
| `S-001` | Node startup failure | Critical |
| `S-002` | Consensus stalled (no new blocks) | Alert |
| `S-003` | Fraud proof accepted | Critical |
| `S-005` | Brain hash mismatch | Critical |
| `S-009` | Coordinated wake-up pattern | Alert |
| `S-013` | Residual commitment mismatch | Critical |
| `S-014` | Peer partition | Critical |
| `S-015` | Storage federation failure | Critical |
| `S-016` | Manifest mismatch or tampering | Critical |
| `S-017` | Key rotation | Notice |
| `S-018` | Validator admission | Notice |
| `S-019` | Controlled rollback | Critical |

## Severity-to-pager mapping

| Severity | Pager behavior |
|----------|---------------|
| `Alert` | Page on-call immediately, regardless of hour |
| `Critical` | Page on-call within 15 minutes |
| `Warning` | Page on-call within business hours |
| `Notice` | Add to daily review queue, no page |
| `Info` / `Debug` | No page; available for query |

Tune per your team's on-call structure.

## What goes in `procedures/`

Procedures are reusable response patterns. A scenario references procedures;
a procedure tells the operator exactly what commands to run.

Current procedures:

- `P-01-startup-triage.md`
- `P-02-peer-partition.md`
- `P-04-manifest-integrity.md`
- `P-05-key-rotation.md`
- `P-06-validator-admission.md`
- `P-07-storage-federation.md`
- `P-08-rollback.md`
- `configure-private-cluster-ports.sh`

`P-03-force-brain-refresh.md` is retained as a historical design document;
its `spacekit-cli agent ...` commands are not exposed by the current CLI and
must not be used as an executable procedure.

If a scenario keeps referencing the same response steps, factor them
into `procedures/`. Don't repeat detailed command sequences across
scenarios — that's how runbooks drift out of sync.

## Lifecycle of a runbook change

1. New scenario observed in production.
2. Operator writes a draft `S-XXX-name.yaml`.
3. Review by other operators (PR review).
4. Once merged, the next agent training cycle picks up the new scenario.
5. After agent retraining + ratification + activation, the network now
   responds to that scenario in line with the documented procedure.

The end-to-end cycle (incident → scenario → trained agent → activated
agent) is the rate at which the system learns. Aim for weekly during
testnet, monthly during stable mainnet.

---

# For engineers

This section is for the people building the agent training pipeline and
the spacekit-compute-node integration.

## Building the tool

```bash
cargo build --release -p spacekit-runbook
```

Requires Rust 1.70+. Depends on `spacekit-log` (sibling crate).

## CLI subcommands

### `generate-corpus`

Read logs + scenarios, produce JSONL training rows for
`SpacetimeConsensusAgent`.

```bash
spacekit-runbook generate-corpus \
    --logs /var/log/spacekit/ \
    --scenarios spacekit-runbook/ \
    --output ../spacetime-consensus-agent/data/ \
    --cap-per-scenario 200 \
    --test-split 0.10 \
    --truncate
```

What it does:

1. Loads scenario YAMLs from `--scenarios`.
2. Compiles each scenario's `event_queries` into `spacekit-log::ScenarioQuery`s.
3. Streams all `*.jsonl` files under `--logs` in deterministic order.
4. For each event, checks against every scenario. Matches produce a
   `TrainingRow` with the scenario's `recommended_agent_classification`
   as the label.
5. Deduplicates by content hash (so identical events across log files
   produce one row).
6. Caps per-scenario at `--cap-per-scenario` to prevent any single
   scenario from dominating its domain.
7. Writes per-domain JSONL files to `--output`.

Split assignment is deterministic from the event content hash —
re-running on the same input produces the same train/test split.

### `verify-corpus`

Sanity-check existing JSONL files for consistency.

```bash
spacekit-runbook verify-corpus --dir ../spacetime-consensus-agent/data/
```

Checks:

- Every row's `domain` matches its filename
- `task_id` is unique within its domain
- JSONL rows deserialize and per-split/per-intent counts are reported

### `list-scenarios`

Print a table of loaded scenarios for debugging.

```bash
spacekit-runbook list-scenarios --scenarios spacekit-runbook/
```

## How scenarios become training rows

The mapping from scenario fields to JSONL fields:

| Scenario field | JSONL field |
|----------------|-------------|
| `scenario_id` + event content hash prefix | `task_id` |
| (synthesized from event by render_event_text) | `text` |
| `recommended_agent_classification.intent` | `semantic_intent` |
| `recommended_agent_classification.domain` | `domain` |
| `recommended_agent_classification.target` | `action_target` |
| (from event policy_regime field, or `--policy-regime`) | `policy_regime` |
| `recommended_agent_classification.reasoning` | `expected_response` |

Full details in `TRAINING_INTEGRATION.md`.

## Where to emit log events from `spacekit-compute-node`

The events this tool consumes have to come from somewhere — the consensus
node's emission sites. Full guidance in `EMISSION_SKETCH.md`, including:

- The `LogSink` trait and `FileLogSink` / `MockSink` implementations
- A site-by-site table of where each event kind should fire
- Code examples for the most operationally-important emission points
- The non-blocking-write requirement on the consensus hot path
- A recommended rollout order for incremental integration

If you're integrating logging into a consensus crate that doesn't have
it yet, read `EMISSION_SKETCH.md` first.

## Adding a new scenario

1. Write a new `S-*.yaml` file in this directory, copying an existing scenario
   as template.
2. Choose a `scenario_id` (S-XXX format, next available number).
3. Fill in `event_queries` — these MUST match valid `spacekit-log` event
   kinds (see the SCHEMA.md in the `spacekit-log` repo).
4. Write `recommended_agent_classification` carefully — this becomes
   the training label. The `reasoning` field is the human-readable
   `expected_response` the agent will learn to produce.
5. Run `spacekit-runbook list-scenarios --scenarios spacekit-runbook/` to
   confirm your scenario parses.
6. Optionally run `spacekit-runbook generate-corpus` against a small
   log sample to confirm the scenario matches the events you expect.
7. Submit PR. Reviewers check both operational correctness AND the
   training-data implications.

## Dependencies

- [`spacekit-log`](../spacekit-log) — schema crate (sibling repo)
- `alloy-primitives` — for `B256` hash types
- `serde` + `serde_yaml` + `serde_json` — parsing
- `clap` — CLI argument parsing
- `sha3` — Keccak256 for content hashing
- `walkdir` — file system traversal
- `anyhow` — error handling

No transitive consensus-node dependencies. This tool runs offline
against logs and scenario files; it does not connect to a live network.

---

## Status

The parser validates every YAML file and compiles every event query during
`list-scenarios`; malformed scenarios fail the command instead of being skipped.

**Next priorities:**

1. Author the remaining scenarios (S-004 through S-015 minus what's
   shipped) using the existing six as templates.
2. Author the remaining procedures referenced by existing scenarios.
3. Run the tool against the first month of testnet logs to validate
   that scenarios actually match events as designed.
4. Bootstrap the agent training pipeline using the generated corpus
   plus the hand-written seed corpus in `spacetime-consensus-agent`.

---

Made with care by the SpaceKit.xyz team.
