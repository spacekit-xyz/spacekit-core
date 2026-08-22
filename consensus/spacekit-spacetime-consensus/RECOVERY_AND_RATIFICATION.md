# Recovery and Ratification Integration Guide

This document covers wiring the three modules that complete the defense
story: fingerprint cross-validation, tiered finality + fraud proofs, and
Growformer parameter ratification.

## Module summary

| Module | Lines | Defense layer | Status |
|--------|-------|---------------|--------|
| `fingerprint_attestation` | ~300 | Layer 4 completion | Code + tests; needs gossip wire-up |
| `finality` + `fraud_proof` | ~600 | Layer 6 (recovery floor) | Code + tests; needs network integration |
| `growformer_ratification` | ~400 | Closes optimizer-disarm risk | Code + tests; needs Growformer hook |

All three compose with what's already shipped — no breaking changes to the
PQ envelope or the Verkle layout.

---

## 1. Fingerprint cross-validation

### What it adds

After each block finalizes, every validator independently computes the
new fingerprint Verkle root. Honest validators with the same vote leaves
MUST arrive at byte-identical roots. Disagreement is free slashing
evidence — no false positives possible, because the EWMA update is fully
deterministic.

### Where to wire in `spacekit-compute-node`

```rust
use spacekit_spacetime_consensus::{
    FingerprintAttestation, FingerprintAttestationCollector,
    FingerprintAttestationMismatchEvidence,
};

pub struct ConsensusCoordinator {
    // ... existing fields ...
    pub fingerprint_attestations: FingerprintAttestationCollector,
}
```

In `apply_block_spacetime_side_effects`, after the fingerprint Verkle has
been updated:

```rust
let fingerprint_root = self.fingerprint_verkle.read().tree.root_hash();
let prev_root = self.fingerprint_verkle.read().last_finalized_root;

let attestation = FingerprintAttestation {
    height: block.height,
    block_hash: block.hash(),
    attester_did_hash: self.my_did_hash,
    fingerprint_root,
    prev_fingerprint_root: prev_root,
    signature_digest: dilithium_sign(&attestation.signing_bytes(), &self.dilithium_key),
};

// Broadcast over the same P2P channel used for PQ votes.
self.broadcast_fingerprint_attestation(attestation).await?;
```

On receiving attestations from peers:

```rust
match self.fingerprint_attestations.ingest(peer_attestation) {
    Ok(()) => {}
    Err(AttestationError::DuplicateAttestation) => {
        // Same validator sent a different attestation. Self-contradiction.
        if let Some(prior) = self.fingerprint_attestations.check_self_contradiction(&peer_attestation) {
            self.queue_self_contradiction_slash(prior, peer_attestation);
        }
    }
    Err(e) => log::warn!("attestation rejected: {:?}", e),
}
```

Periodic mismatch sweep (e.g. once per block):

```rust
let evidence = self.fingerprint_attestations.detect_mismatches(target_height);
for mismatch in evidence {
    // Wrap as a FraudProof and submit through the recovery path:
    let fp = FraudProof::FingerprintAttestationMismatch(mismatch);
    let submission = FraudProofSubmission {
        submitter_did_hash: self.my_did_hash,
        target_height,
        target_block_hash: ...,
        proof: fp,
        signature_digest: ...,
    };
    self.submit_fraud_proof_local(submission).await?;
}
```

### Deferred

- Bounty distribution: when a fraud-proof submitter brings in valid
  mismatch evidence, they should receive a fraction of the slashed
  stake. That schedule is in `spacekit-consensus`, not here.
- Differentiating "isolated minority" (likely a bug — slash partial) from
  "coordinated minority" (likely attack — slash full). Use the clique
  detection from `defense.rs` as the signal.

---

## 2. Tiered finality + fraud proofs

### What it adds

Two finality stages: Soft (PBFT quorum) and Hard (Soft + challenge window
elapsed). Any block in the Soft window can be reverted by a valid fraud
proof, triggering the rollback hook you already have wired.

### Where to wire

Add the state machine to the coordinator:

```rust
use spacekit_spacetime_consensus::{
    TieredFinality, TieredFinalityConfig, FinalityStage,
    FraudProof, FraudProofSubmission, submit_fraud_proof,
};

pub struct ConsensusCoordinator {
    // ... existing ...
    pub finality: TieredFinality,
}

impl ConsensusCoordinator {
    pub fn new(config: ConsensusConfig) -> Self {
        Self {
            // ... existing ...
            finality: TieredFinality::new(
                TieredFinalityConfig {
                    challenge_window: config.challenge_window_blocks,
                    max_pending: 1024,
                },
                config.genesis_height,
            ),
        }
    }
}
```

After `pq_finisher::finalize_proposal_if_ready` succeeds — i.e. soft
finality reached — call:

```rust
let transitioned = self.finality.on_soft_finalize(height, block_hash);
for h in transitioned {
    // These blocks just crossed Soft → Hard. Notify dependents:
    self.emit_event(BlockHardFinalized { height: h }).await?;
}
```

### Fraud-proof submission endpoint

The standalone needs to accept fraud proofs. The simplest path:

```rust
// POST /v1/consensus/fraud_proof
async fn handle_fraud_proof_submission(
    coordinator: &mut ConsensusCoordinator,
    submission: FraudProofSubmission,
) -> Result<FraudProofAcceptance, FraudProofError> {
    // 1. Verify the submitter's signature (existing crypto layer).
    verify_submitter_signature(&submission)?;

    // 2. Run the spacetime-layer fraud-proof verification.
    let acceptance = submit_fraud_proof(
        &mut coordinator.finality,
        &submission,
        keccak256_wrapper,
    )?;

    // 3. Trigger rollback for each rolled-back height, tip-first.
    for h in &acceptance.rolled_back_heights {
        coordinator.rollback_block_spacetime_side_effects(*h).await?;
        coordinator.mempool.requeue_block(*h).await?;
    }

    // 4. Queue slashing proposals for the next block.
    for proposal in &acceptance.slashing_proposals {
        coordinator.queue_slash(proposal.clone()).await?;
    }

    // 5. Drain reverted blocks from finality state.
    coordinator.finality.drain_reverted();

    Ok(acceptance)
}
```

### What this gives end users

Tiered finality is a per-transaction UX choice:

```rust
pub enum FinalityRequirement {
    Soft,           // accept after PBFT quorum; ~2-3s latency
    Hard,           // wait for challenge window elapsed; ~3-5 min latency
}

// In transaction submission:
async fn await_finality(&self, tx_hash: B256, req: FinalityRequirement) -> Result<()> {
    loop {
        let stage = self.coordinator.finality.stage_of(tx_block_height(tx_hash));
        match (req, stage) {
            (FinalityRequirement::Soft, FinalityStage::Soft | FinalityStage::Hard) => return Ok(()),
            (FinalityRequirement::Hard, FinalityStage::Hard) => return Ok(()),
            (_, FinalityStage::Reverted) => return Err(Reverted),
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
}
```

### Recommended config

- `challenge_window`: 100 blocks ≈ 3-5 minutes at your soft-finality cadence
- `max_pending`: 1024 (lets challenge_window grow up to ~10× without OOM)
- Activation delay for parameter changes: ≥ challenge_window (so a poisoned
  parameter change can itself be reverted)

### Deferred

- Mempool requeue logic itself (`mempool.requeue_block`) is a standalone
  concern; we just expose the hook.
- Network-level rate limiting for fraud-proof submissions (spam protection).
  Start with: max 1 submission per validator per height per minute.

---

## 3. Growformer ratification

### What it adds

Parameter changes that affect security thresholds (sigma_threshold,
divergence_threshold, challenge_window itself, etc.) require PBFT quorum,
not silent updates from the optimizer. A YES vote that ratifies a change
later shown to enable an attack is itself slashable.

### Integration with your Growformer training pipeline

Your training JSONL has these fields:

```json
{
  "task_id": "...",
  "semantic_intent": "positive_strong | negative_mild | ...",
  "domain": "sentiment",
  "action_target": "sentiment",
  "policy_regime": "default",
  "expected_response": "..."
}
```

`ParameterChangeProposal::inference` mirrors this shape exactly. To extend
the existing pipeline for consensus tuning, add a new TOML grounding file
(e.g. `consensus-tuning.toml`) and a new JSONL training set where:

- `domain = "consensus_tuning"`
- `action_target` is a parameter path: `"spacetime.divergence_threshold"`,
  `"defense.sigma_threshold"`, `"finality.challenge_window"`, etc.
- `semantic_intent` is one of `tighten | loosen | no_change | alert`
- `policy_regime` is `default | secure | permissive`
- `expected_response` is the human-readable reasoning

Example training row for the consensus-tuning Growformer:

```json
{
  "task_id": "param_div_001",
  "text": "Recent 200 blocks: rotor divergence 95th percentile = 0.62, current threshold = 0.5, false-positive slash rate = 18%.",
  "semantic_intent": "tighten",
  "domain": "consensus_tuning",
  "action_target": "spacetime.divergence_threshold",
  "policy_regime": "default",
  "expected_response": "TIGHTEN — 95th percentile exceeds current threshold; legitimate honest variance is being flagged. Recommend threshold 0.5 → 0.65 to reduce false positive rate."
}
```

At inference time, the Growformer outputs this same shape, and the
coordinator constructs a `GrowformerInference` directly from it.

### Coordinator wiring

```rust
use spacekit_spacetime_consensus::{
    GrowformerInference, GrowformerIntent, ParameterChangeProposal,
    ParameterChangeVote, PolicyRegime, RatificationConfig,
    validator_should_ratify, evaluate_ratification,
};

pub struct ConsensusCoordinator {
    // ... existing ...
    pub ratification_config: RatificationConfig,
    pub current_regime: PolicyRegime,
    pub pending_proposals: BTreeMap<B256, ParameterChangeProposal>,
    pub ratification_votes: BTreeMap<B256, Vec<ParameterChangeVote>>,
}
```

When the local Growformer emits an inference suggesting a change:

```rust
async fn maybe_propose_parameter_change(
    coordinator: &mut ConsensusCoordinator,
    inference: GrowformerInference,
) -> Result<()> {
    if inference.semantic_intent == GrowformerIntent::NoChange { return Ok(()); }
    if inference.confidence < coordinator.ratification_config.min_confidence { return Ok(()); }

    let (current, proposed) = compute_value_change(&inference, coordinator)?;
    let proposal = ParameterChangeProposal {
        proposal_id: domain_hash(b"parameter-proposal", &inference.task_id),
        proposer_did_hash: coordinator.my_did_hash,
        proposed_at_height: coordinator.current_height(),
        inference,
        current_value: current.to_le_bytes(),
        proposed_value: proposed.to_le_bytes(),
        activation_delay: coordinator.ratification_config.min_activation_delay,
    };
    coordinator.broadcast_parameter_proposal(proposal).await
}
```

On receiving a peer's proposal:

```rust
async fn handle_parameter_proposal(
    coordinator: &mut ConsensusCoordinator,
    proposal: ParameterChangeProposal,
) -> Result<()> {
    // Validator runs its OWN Growformer over the same metric window.
    let own_metrics = coordinator.metrics_window_for(proposal.inference.metrics_window_hash)?;
    let own_inference = coordinator.growformer.infer(&own_metrics)?;

    let should_yes = validator_should_ratify(
        &proposal,
        &own_inference,
        coordinator.current_regime,
        &coordinator.ratification_config,
    ).is_ok();

    let vote = ParameterChangeVote {
        proposal_id: proposal.proposal_id,
        voter_did_hash: coordinator.my_did_hash,
        vote: should_yes,
        voter_metrics_window_hash: own_inference.metrics_window_hash,
        signature_digest: dilithium_sign(&vote_signing_bytes(&proposal, should_yes), &coordinator.dilithium_key),
    };
    coordinator.broadcast_ratification_vote(vote).await?;
    coordinator.pending_proposals.insert(proposal.proposal_id, proposal);
    Ok(())
}
```

After collection window, evaluate:

```rust
async fn finalize_ratification(
    coordinator: &mut ConsensusCoordinator,
    proposal_id: B256,
) -> Result<bool> {
    let proposal = coordinator.pending_proposals.get(&proposal_id).ok_or(...)?;
    let votes = coordinator.ratification_votes.get(&proposal_id).cloned().unwrap_or_default();
    let voting_powers: Vec<(B256, f64)> = coordinator.validators.iter()
        .map(|(did, v)| (*did, v.effective_voting_power))
        .collect();

    match evaluate_ratification(
        proposal,
        &votes,
        &voting_powers,
        coordinator.current_regime,
        &coordinator.ratification_config,
    ) {
        Ok(_ratio) => {
            // Queue activation at proposal.proposed_at_height + activation_delay.
            coordinator.schedule_parameter_activation(
                proposal_id,
                proposal.proposed_at_height + proposal.activation_delay,
            ).await?;
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}
```

### Linking ratification to slashing

When `submit_fraud_proof` accepts a proof, check whether the exploited
parameter was recently ratified:

```rust
let recently_ratified = coordinator.find_ratification_within(
    fraud_proof.target_height,
    safety_window: 100,
);
if let Some(ratification) = recently_ratified {
    for vote in ratification.yes_votes {
        let evidence = MalignRatificationEvidence {
            proposal_id: ratification.proposal_id,
            bad_voter_did_hash: vote.voter_did_hash,
            vote,
            activated_at_height: ratification.activated_at_height,
            attack_height: fraud_proof.target_height,
            fraud_proof_digest: ...,
        };
        if evidence.verify(safety_window) {
            coordinator.queue_slash(MalignRatificationSlash { evidence });
        }
    }
}
```

This is what makes the disarmament attack uneconomic: an attacker
compromising the optimizer must convince 2/3 of validators to YES-vote,
and every YES voter is liable if the attack succeeds.

### Deferred

- The actual Growformer training corpus for the consensus-tuning domain.
  This crate doesn't provide it; you generate it from logged consensus
  metrics over operational windows.
- A `PolicyRegime` state machine (when does Default → Secure transition?).
  Recommended: automated by alert-counter-derived heuristics, manually
  overridable by 2/3 validator quorum.
- Multi-parameter proposals (changing two thresholds in one round). Start
  with single-parameter to keep the threat model tractable.

---

## Compose-with-everything checklist

When you finish wiring all three:

- [ ] `apply_block_spacetime_side_effects` broadcasts a `FingerprintAttestation`
- [ ] Peer `FingerprintAttestation`s are ingested into the collector
- [ ] `on_soft_finalize` is called from the finisher path
- [ ] `POST /v1/consensus/fraud_proof` endpoint exists and routes through
  `submit_fraud_proof`
- [ ] Accepted fraud proofs trigger `rollback_block_spacetime_side_effects`
- [ ] Mempool requeue is wired in the rollback path
- [ ] Growformer inference output is converted to `ParameterChangeProposal`
- [ ] Validator vote on proposals is conditioned on
  `validator_should_ratify`
- [ ] Parameter activation is scheduled at `proposed_at_height + activation_delay`
- [ ] `MalignRatificationEvidence` is emitted when a fraud proof exploits
  a recently ratified parameter

Each line passing makes the next defense layer real.
