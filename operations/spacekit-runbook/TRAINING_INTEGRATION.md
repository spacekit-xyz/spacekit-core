# Training Pipeline: Scenarios → JSONL Corpora

This is the integration that makes `SpacetimeConsensusAgent` deterministic:
**runbook scenarios are the labeling source for training data.**

## The contract

Every scenario file in `scenarios/` has a `recommended_agent_classification`
block. That block IS the training label. When the training pipeline sees
a `spacekit-log` event that matches the scenario's `event_queries`, it
emits a training row labeled with that classification.

The pipeline:

```
spacekit-log events (production)
        ↓
[ filter by scenario_query ]
        ↓
matched event windows
        ↓
[ pair with recommended_agent_classification from scenario file ]
        ↓
JSONL training rows
        ↓
SpacetimeConsensusAgent training
        ↓
agent.brain
```

A scenario change is a training-data change. An agent retrained against
updated scenarios will classify the matching events the same way the
new runbook documents — by construction, not by hope.

## The mapping

Each scenario produces training rows for the domain specified in
`recommended_agent_classification.domain`. The mapping from scenario
fields to JSONL fields:

| Scenario field | JSONL field | Notes |
|----------------|-------------|-------|
| `scenario_id` + event_hash | `task_id` | Deterministic, deduplicates |
| (synthesized from event) | `text` | Human-readable rendering of the event |
| `recommended_agent_classification.intent` | `semantic_intent` | The label |
| `recommended_agent_classification.domain` | `domain` | Which agent domain |
| `recommended_agent_classification.target` | `action_target` | Which parameter or entity |
| (from event or scenario) | `policy_regime` | Network state at time of event |
| `recommended_agent_classification.reasoning` | `expected_response` | The agent's natural-language explanation |

## Text synthesis

The `text` field for each training row is generated from the matched
log event using a deterministic template per event kind. Example for
`FingerprintAnomalyStrong`:

```python
def text_for(event):
    if event.kind == ("Spacetime", "FingerprintAnomalyStrong"):
        validator = event.get_field("validator_did")
        distance = event.get_float("centroid_distance")
        sigma = event.get_float("sigma_threshold")
        return (
            f"Validator {validator[:8]}: centroid distance {distance:.2f}, "
            f"sigma threshold {sigma:.1f}. Anomaly factor {distance/sigma:.1f}x."
        )
```

The template per event kind is checked into the training pipeline source;
changing a template requires retraining (templates are part of the input
distribution).

## Volume management

A single scenario can match thousands of events. Training quality benefits
from diverse examples, but matching events from the same scenario are
near-duplicates. The pipeline applies:

1. **Deduplication by event content_hash.** Identical events produce one row.
2. **Subsampling within scenario.** Cap each scenario at 200 examples per
   training cycle. Beyond 200, sample uniformly across the time range.
3. **Cross-scenario diversity check.** If any domain ends up >80% from
   one scenario, fall back to broader sampling.

## Worked example

Scenario file:
```yaml
scenario_id: S-009
event_queries:
  - kind: {Spacetime: CliqueDetected}
    field_predicates:
      - FloatAtLeast: [coordination_score, 5.0]
recommended_agent_classification:
  domain: clique_assessment
  intent: coordinated
  target: clique_subset
  reasoning: >
    Multiple high-reputation long-tenure validators simultaneously breaking
    from their established fingerprints is the canonical coordinated-sleeper
    signature. ...
```

Matched event:
```json
{
  "kind": {"Spacetime": "CliqueDetected"},
  "fields": [
    ["validator_count", {"Unsigned": 6}],
    ["coordination_score", {"Float": 7.2}],
    ["avg_rotor_distance", {"Float": 0.001}]
  ]
}
```

Produced JSONL row:
```json
{
  "task_id": "S-009_a3b4c5d6e7f8...",
  "text": "Clique detected: 6 validators, coordination_score 7.2, avg_rotor_distance 0.001",
  "semantic_intent": "coordinated",
  "domain": "clique_assessment",
  "action_target": "clique_subset",
  "policy_regime": "default",
  "language_channel": "english",
  "code_language": null,
  "split": "train",
  "expected_response": "Multiple high-reputation long-tenure validators simultaneously breaking from their established fingerprints is the canonical coordinated-sleeper signature. ..."
}
```

## Running the pipeline

```bash
# Generate training corpus from logged events and runbook scenarios:
spacekit-runbook generate-corpus \
  --logs /var/log/spacekit/*.jsonl \
  --scenarios scenarios/ \
  --output ../spacetime-consensus-agent/data/ \
  --time-range "30 days"
```

This writes per-domain JSONL files into the agent training directory,
ready for Growformer to consume.

## Hand-written examples vs. pipeline-generated

The seed JSONL files in the agent project (15 examples per domain) are
hand-written for two reasons:

1. **Bootstrap.** No logged events exist before the first deployment.
2. **Edge cases.** Hand-written examples can cover scenarios that don't
   happen often enough in production to generate corpus from.

Hand-written examples should be **labeled in the same way scenarios
label**. Each hand-written row has a comment indicating which scenario
it corresponds to:

```jsonl
// corresponds to S-009-coordinated-wake-up.yaml
{"task_id":"...","semantic_intent":"coordinated", ...}
```

If a hand-written example has no corresponding scenario, write one. If
a scenario can't reasonably produce hand-written examples, the scenario
is probably too vague.

## Lifecycle of a label change

1. Operator drafts a scenario change (PR).
2. Reviewers approve based on operational reality.
3. Once merged, next nightly training run consumes the updated scenario.
4. Agent retrains; new `model_hash` is published to storage.
5. Network goes through ratification cycle for the new `model_hash`.
6. After activation, agent classifications match the new runbook.

This is the same path as any other parameter change, but with the
parameter being the entire brain.

## Verifying the pipeline

After running corpus generation, sanity-check:

```bash
spacekit-runbook verify-corpus --dir ../spacetime-consensus-agent/data/
```

Checks:
- Every row's `domain` matches a known agent domain
- Every row's `task_id` is unique within its domain
- Every `semantic_intent` is in the allowed enum for its domain
- Hand-written rows reference an existing `scenario_id`
- No domain is dominated (>80%) by a single scenario

Fix any reported issues before training; the agent will inherit any
inconsistencies in the training data.
