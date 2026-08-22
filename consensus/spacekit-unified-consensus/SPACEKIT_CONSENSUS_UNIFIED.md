# SpaceKit Unified Consensus

**Identity-native consensus with reputation weighting, post-quantum cryptography, and spacetime defense layers.**

This document has two parts:

- **§1 As-built (current testnet target)** — what ships today, file by file
- **§2 Target architecture** — the design intent the as-built converges toward

If you want to know what to call in the repo right now, read §1. If you want to know what the design *will* be once milestones land, read §2. The two should be read together; what's in §2 but not §1 is roadmap, not bug.

**Not two-tier consensus.** Unified consensus is a **standalone** BFT facade
over `ConsensusCoordinator`. Spacetime is an **optional reference extension**
that augments PBFT (rotors, fingerprints, recovery) — it does not sit
underneath the facade as a required second consensus tier. See §1.3.

---

## §1 As-built (current testnet target)

### §1.3 Architecture layering

| Layer | Type | Standalone? |
|-------|------|-------------|
| 1 | `ReputationSource` (trait) | Pluggable; default `EqualWeightReputation` |
| 2 | `CoordinatorHandle` (trait) | Production: `CoordinatorRoundHandle` → `ConsensusCoordinator` |
| 3 | `ReputationWeightedConsensus` | **Yes** — complete count-mode BFT API; PBFT engine remains in coordinator |
| Optional | `SpacetimeExtension` (struct) | Reference impl in `spacekit-spacetime-consensus`; feature `spacetime` / node `spacetime-consensus` |

**Quorum / safety today:** `ConsensusCoordinator::check_finality` (count-based
2/3). Facade `has_consensus` is a **tripwire** in count mode; flip to
**host-first** after hard fork with `FacadeConfig::use_weighted_threshold`.

**Production host:** `UnifiedConsensusHost` (`spacekit-compute-node`) owns
`Arc<ConsensusCoordinator>` + `SpacetimeExtension` + `ReputationSource`.

**Governance (separate):** `UnifiedSWTCHConsensus` — L1 manifest proposal
queue only; not network PBFT.

**Spacetime call sites (split intentionally):**

| Path | Location |
|------|----------|
| Rotor aggregate, transition verify | Facade `spacetime_integration` → `UnifiedConsensusHost` |
| Fingerprints, tiered finality, fraud proofs, ratification, post-finalize | `ConsensusCoordinator` + `spacetime_integration.rs` |

### Component map


| Doc concept                              | Real type                                                                  | File                                                                        |
| ---------------------------------------- | -------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| Consensus host                           | `ConsensusCoordinator`                                                     | `spacekit-compute-node/src/consensus_coordinator.rs`                        |
| Reputation-weighted facade               | `ReputationWeightedConsensus`                                              | `spacekit-unified-consensus/src/lib.rs` (new)                               |
| Validator record                         | `ValidatorEntry`                                                           | `spacekit-compute-node/src/consensus_coordinator.rs`                        |
| Validator with PQ keys + reputation hook | `UnifiedConsensusValidator`                                                | `spacekit-unified-consensus/src/validator.rs` (new, wraps `ValidatorEntry`) |
| Block envelope                           | `BlockData` + `signed_block_envelope`                                      | `spacekit-compute-node/src/pq_finisher.rs`                                  |
| PQ signatures                            | Dilithium + SPHINCS+ inner; envelope SPHINCS+ outer                        | `spacekit-compute-node/src/pq_finisher.rs` (`SIGNATURE_POLICY.md`)          |
| Spacetime layer                          | `SpacetimeExtension` plug-in                                               | `spacekit-spacetime-consensus` crate                                        |
| Fingerprint storage                      | At-rest in state Verkle namespace `0xFF..FE`                               | `spacekit-spacetime-consensus/src/fingerprint_verkle.rs`                    |
| Tiered finality                          | `SoftFinalityState` → `HardFinalityState`                                  | `spacekit-spacetime-consensus/src/finality.rs`                              |
| Fraud proofs                             | Submitted via `submit_fraud_proof`; rollback via `on_fraud_proof_accepted` | `spacekit-spacetime-consensus/src/fraud_proof.rs`                           |
| Parameter ratification                   | `evaluate_ratification` (PBFT-quorumed parameter changes)                  | `spacekit-spacetime-consensus/src/growformer_ratification.rs`               |
| Growformer client                        | Network-loaded, brain on disk, cached fallback                             | `spacekit-spacetime-consensus/src/growformer_client.rs`                     |
| Per-validator agent                      | `ConsensusGrowformerAgent`                                                 | `spacekit-compute-node/src/agent/consensus_agent.rs`                        |
| Network ML                               | `MLReputationEngine` (pricing/routing, not block votes)                    | `spacekit-compute-node/src/advanced_network_features.rs`                    |
| Structured event log                     | `LogEvent`, `LogSink`                                                      | `spacekit-log` crate                                                        |
| Operational runbook                      | YAML scenarios + procedures + corpus generator                             | `spacekit-runbook` crate                                                    |


### Status by feature


| Feature                              | Status                                                                   | Notes                                                                |
| ------------------------------------ | ------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| Identity-native validator admission  | ✅                                                                        | DID-gated; `ConsensusCoordinator` enforces                           |
| PQ signatures (Dilithium + SPHINCS+) | ✅                                                                        | On both inner votes and outer envelope; see `SIGNATURE_POLICY.md`    |
| 2/3 PBFT quorum (equal-weight)       | ✅                                                                        | `check_finality` in coordinator                                      |
| Reputation-weighted voting threshold | 🔄 (facade + hook shipped; authoritative threshold deferred to hard fork) | See §1.4 below                                                       |
| GPU batch verification on hot path   | 🔄                                                                       | Library exists; not wired into `pq_finisher`                         |
| Dynamic validator admission/removal  | ⚠️                                                                       | Admission works; reputation-driven removal is a roadmap item         |
| Spacetime extension plug-in          | ✅                                                                        | Spacetime crate complete; integration via `spacetime_integration.rs` |
| Fingerprint anomaly detection        | ✅                                                                        | At-rest in Verkle; attestation gossip via HTTP today                 |
| Tiered finality + fraud proofs       | ✅                                                                        | State machine + recovery dispatch implemented                        |
| Growformer parameter ratification    | ✅                                                                        | Parameter changes flow through PBFT quorum + activation delay        |
| Growformer network-loaded brain      | 🔄                                                                       | On-disk encrypted cache; storage-node fetch needs wiring             |
| Cross-chain unified finality         | ❌                                                                        | Aspirational; not on testnet roadmap                                 |
| P2P fingerprint attestation gossip   | 🔄                                                                       | HTTP ingest works; full mesh gossip pending                          |
| Mempool requeue on rollback          | 🔄                                                                       | Hook exists in spacetime crate; not wired to node mempool            |


### §1.4 Reputation-weighted voting: what changes when

The doc's `ReputationWeightedConsensus` is the headline concept. Today, voting is count-based equal-weight (every registered validator's vote counts the same). To land reputation weighting, three things have to happen:

**Step 1 (shipped):** `ReputationWeightedConsensus` is a thin wrapper around `ConsensusCoordinator` via `CoordinatorHandle`. It exposes the doc's API surface but delegates quorum to the coordinator in count mode. Spacetime **rotor** paths go through the facade; other spacetime surfaces use the coordinator bridge (§1.3). **No protocol change.**

**Step 2 (reputation hook):** Add a `ReputationSource` trait. The facade reads per-validator reputation through it. Default implementation returns 1.0. Custom implementations (backed by `MLReputationEngine` or on-chain reputation) can be plugged in. **No protocol change; reputation is observable but not authoritative.**

**Step 3 (hard fork):** Make reputation-weighted 2/3 threshold authoritative. Every validator must agree on per-validator reputation at every height. This requires reputation to be either on-chain or deterministically derivable from on-chain history. Threshold check changes from `votes.len() >= 2*N/3` to `sum(reputation_weighted_votes) >= 2*sum(all_reputation)/3`. **Hard fork, post-testnet.**

The facade unblocks all of step 2 and 3 without requiring them upfront. That's why it's the right thing to build first.

### §1.5 The integration host (where spacetime plugs in)

**Host:** `UnifiedConsensusHost` in `spacekit-compute-node` (feature
`spacetime-consensus`). It wires `ReputationWeightedConsensus` to
`ConsensusCoordinator` and holds a `SpacetimeExtension` for rotor paths.

**PBFT without spacetime:** build the compute node without
`spacetime-consensus`; `ConsensusCoordinator` still runs. No rotor sidecar.

**With spacetime:** optional `SpacetimeTransition` on `BlockData`; finalize
path runs coordinator finality, facade tripwire, PQ finisher, then
`apply_block_spacetime_side_effects` in `spacetime_integration.rs`.

Example shapes (as-built, not aspirational API names):

```rust
// Block may carry an optional spacetime sidecar on BlockData
block_data.spacetime_transition = Some(transition);

// PBFT votes → coordinator (authoritative quorum today)
coordinator.record_vote_by_did_hash(proposal_id, validator_did_hash, true).await?;
coordinator.check_finality(proposal_id).await?;

// Facade tripwire + telemetry (UnifiedConsensusHost)
host.has_consensus(proposal_id).await?;
host.aggregate_votes(proposal_id, &block_spacetime_data).await?;

// Fingerprints / soft finalize — coordinator bridge, not the facade
spacetime_integration::apply_block_spacetime_side_effects(coordinator, vm, &block).await;
```

Fingerprint attestations, fraud proofs, and parameter ratification are
**coordinator + spacetime crate** surfaces (HTTP routes on standalone today).
The facade is **not** a single re-export of the full spacetime API — only
rotor aggregation and `verify_transition` (§1.3).

---

## §2 Target architecture

This is the design intent. Pieces here that aren't in §1 are roadmap, not gaps.

### §2.1 Identity-native validation

Every validator has a DID verified at high security level (`spacekit_verify_did_high_security`). Pseudonymous validators cannot join. Identity becomes the basis for reputation accumulation.

**Today:** DID admission works; PQ keys generated per validator on admission. Reputation per validator is a roadmap field (`reputation: f64`) once the facade lands.

### §2.2 Reputation-weighted voting

Voting power is a function of stake, identity confidence, historical participation, and behavioral consistency. Validators with strong long-term records carry more weight than newly admitted validators with equal stake. Reputation evolves continuously based on consensus participation.

**Today:** Equal-weight voting. Reputation observed but not authoritative. The path to authoritative reputation weighting is in §1.4.

### §2.3 Post-quantum cryptography throughout

All signatures use Dilithium (block votes) + SPHINCS+ (block envelopes). Key exchange uses Kyber1024. Verkle commitments use SIS-based commitments (quantum-resistant).

**Today:** This is implemented — see `SIGNATURE_POLICY.md`. PQ is the only signature scheme on the wire.

### §2.4 GPU-accelerated verification

Signature verification on the consensus hot path uses GPU batching to amortize per-signature cost across the validator set.

**Today:** GPU verification libraries exist; not wired into the `pq_finisher` hot path. Acceleration kicks in only for offline batch operations. Wiring is a known optimization, not blocking testnet.

### §2.5 Spacetime defense layers

Rotor-valued state transitions, joint signature fingerprinting, geometric median aggregation (50% Byzantine tolerance), clique detection, fingerprint attestation, tiered finality with fraud-proof recovery, and Growformer-mediated parameter ratification.

**Today:** Implemented in `spacekit-spacetime-consensus` and integrated on the
node via `UnifiedConsensusHost`, `ConsensusCoordinator`, and
`spacetime_integration.rs` when `spacetime-consensus` is enabled. PBFT
remains the safety backbone; spacetime augments detection and recovery.

### §2.6 Dynamic validator management

Validators apply through an admission committee; their stake and reputation determine acceptance. Underperforming validators (low reputation, high consecutive misses, low availability) are automatically removed with stake slashing.

**Today:** Admission works; auto-removal is a roadmap item gated on reputation being authoritative (post-fork).

### §2.7 Cross-chain unified finality

A single consensus instance finalizes blocks across multiple chains, with chain-specific bridge connectors propagating finality.

**Today:** Aspirational. Not on testnet roadmap; revisit post-mainnet.

### §2.8 Growformer-tuned consensus parameters

A network-loaded agent (`SpacetimeConsensusAgent`) recommends parameter changes (sigma thresholds, divergence thresholds, challenge window, etc.). Changes flow through PBFT-quorumed `ParameterChangeProposal`s, validated by validator-side re-inference, and activated only after a delay equal to or exceeding the challenge window.

**Today:** Ratification path is implemented. Brain training pipeline is built. The agent runs in production once the storage node serves it and operators load via `SPACEKIT_API_KEY`.

---

## §3 Security analysis (unchanged from previous version, recap)

The defense layering composes:


| Attack class                     | Layer that catches it                                       |
| -------------------------------- | ----------------------------------------------------------- |
| 1/3+ Byzantine                   | PBFT quorum (today)                                         |
| 33%-50% reputation-bomb sleeper  | Geometric median aggregation (spacetime)                    |
| 50%+ attack                      | Recoverable via fraud proof during challenge window         |
| Single sleeper wake-up           | Fingerprint divergence on joint (rotor, residual) signature |
| Coordinated wake-up              | Clique detection on spacelike-separated agreement           |
| Forged residual commitment       | v2 wire format catches; sandwich product verification fails |
| Equivocating proposer            | PBFT detects dual-signing                                   |
| Compromised Growformer optimizer | Ratification + slashable bad YES votes                      |
| Buggy validator fingerprint code | Cross-validator attestation gossip catches divergence       |


None of these is defeated by reputation alone, and none is defeated by spacetime alone. The composition is the strength.

---

## §4 Performance posture

See `[TPS_REFERENCE.md](../TPS_REFERENCE.md)` for calculated theoretical throughput and the honest caveats about what's theoretical vs measured. The unified doc previously claimed "100-500 TPS" as a flat number; that claim has been retired in favor of the longer treatment in the dedicated TPS reference.

---

## §5 Documentation pointers


| If you need                                   | Read                                                   |
| --------------------------------------------- | ------------------------------------------------------ |
| What's actually running today                 | This doc, §1                                           |
| Where the design is heading                   | This doc, §2                                           |
| Block-level PQ details                        | `pq_finisher.rs` source + `SIGNATURE_POLICY.md`        |
| Spacetime layer                               | `spacekit-spacetime-consensus/README.md`               |
| Tiered finality + fraud proofs + ratification | `RECOVERY_AND_RATIFICATION.md`                         |
| Operations and incident response              | `spacekit-runbook/README.md`                           |
| Event-logging contract                        | `spacekit-log/SCHEMA.md`                               |
| Agent training                                | `spacetime-consensus-agent/TRAINING_AND_DEPLOYMENT.md` |
| TPS and measurement honesty                   | `TPS_REFERENCE.md`                                     |
| Protocol summary for non-experts              | `PROTOCOL_OVERVIEW.md`                                 |


---

## §6 Honest assessment

What's strong: the spacetime defense layers compose well, the PQ posture is comprehensive, and the runbook/log/training integration is structured around a single deterministic event hash so everyone reasons about the same facts.

What's load-bearing-but-fragile: brain bootstrap (single point of failure on storage), API key gate (centralized issuance), reputation-weighted authoritative voting (deferred to post-fork). Roadmap addresses all three.

What's not done: cross-chain finality, GPU on the consensus hot path, full P2P attestation gossip, reputation-weighted threshold. Each is a deliberate scope decision, not an oversight.

The system is ready for aggressive testnet deployment with real users producing real corpora. It is not ready for mainnet without an independent audit and 6-12 months of operational learning.

This document supersedes the previous `SPACEKIT_CONSENSUS_UNIFIED.md`.