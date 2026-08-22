# spacekit-spacetime-consensus

The SpaceKit consensus protocol. A Cl(1,3) spacetime-algebra extension
that adds rotor-valued state transitions, causal-set ordering, robust
geometric aggregation, behavioral fingerprinting, tiered finality with
fraud-proof recovery, and Growformer-mediated parameter ratification.

**License:** Apache-2.0 (matches `spacekit-quantum-verkle`).

**Status:** Library complete. Production host:
[`spacekit-compute-node`](../spacekit-compute-node/README.md) feature
`spacetime-consensus` via
[`UnifiedConsensusHost`](../spacekit-compute-node/src/unified_consensus_host.rs)
and [`spacekit-unified-consensus`](../spacekit-unified-consensus/README.md).

---

## About the SpaceKit consensus protocol

SpaceKit's consensus is built on geometric algebra — specifically
Clifford algebra Cl(1,3), the algebra of relativistic spacetime.
Validator agreement is expressed not just as a yes/no vote on a block
hash, but as a geometric transformation (a Spin(1,3) rotor) describing
how state changed. Aggregating those transformations using the geometric
median on the Spin manifold gives the protocol three properties standard
consensus designs can't compose:

- **Robust aggregation up to 50% Byzantine inputs** (breakdown-point
  theory from robust statistics), versus the 33% threshold of standard
  BFT protocols
- **Stateless light-client verification in constant time per block** —
  browser nodes verify the chain without holding state and without
  trusting an indexer
- **Behavioral signatures that catch sleeper attacks** where validators
  build clean reputations for months or years before activating

The math is geometric algebra (Cl(1,3)). The protocol runs on classical
computers; there are no quantum-mechanical effects in the consensus
itself. Cryptography is post-quantum — Dilithium + SPHINCS+ + Kyber1024
+ SIS-VC Verkle — meaning classical algorithms chosen to resist attack
by a future quantum computer.

This crate is where the math and the optional defense layers live.

**Relationship to unified consensus:** this is **not** a second consensus
tier underneath PBFT. [`spacekit-unified-consensus`](../spacekit-unified-consensus/README.md)
is the standalone BFT facade (`ReputationWeightedConsensus` +
`CoordinatorHandle` + `ReputationSource`). This crate is the **reference
extension** operators enable via feature `spacetime` / node
`spacetime-consensus`: it **augments** vote/finality paths with rotors,
fingerprints, and recovery — it does not replace the 2/3 quorum check.

---

## Why this crate exists

A consensus that already has identity verification and post-quantum
cryptography is well-defended against most attack classes — but is
still vulnerable to:

1. **Coordinated reputation-bomb sleepers** — validators that build
   clean records over months or years before coordinating an attack
   at a chosen moment
2. **Silent state-delta manipulation that passes signature checks** —
   transitions where the rotor part looks innocent but the residual
   carries malicious payload
3. **ML-optimizer compromise that silently weakens thresholds** — a
   compromised parameter-tuning agent that lowers detection thresholds
   over time
4. **Buggy or malicious fingerprint divergence** — validators whose
   internal state diverges from the network consensus without dual-signing
5. **Lone-actor sandwich-mismatch fraud** — a single validator producing
   a transition whose claimed sandwich product is inconsistent with the
   stated rotor

The spacetime layer addresses each of these with a different defense:

| Attack | Defense layer |
|--------|---------------|
| Coordinated reputation-bomb sleeper | Geometric median (50% breakdown) |
| Silent state-delta manipulation | Joint signature: rotor + residual commitment |
| ML-optimizer compromise | PBFT-ratified parameter changes with slashable bad YES votes |
| Buggy/malicious fingerprint divergence | Cross-validator attestation gossip |
| Lone-actor sandwich-mismatch fraud | Per-block sandwich product verification + fraud proofs |

Recovery is structural: even if every detection layer fails and an attack
reaches soft finality, a single honest validator can revert it via a
fraud proof during the challenge window before hard finality.

---

## Architecture at a glance

```
┌─────────────────────────────────────────────────────────────┐
│ Application / VM                                            │
├─────────────────────────────────────────────────────────────┤
│ Block envelope (SPHINCS+, locked byte order)                │
│   ├─ votes_merkle_root  ── PBFT vote leaves                 │
│   ├─ tx_root            ── Verkle (raw, single-tag at sign) │
│   ├─ state_root         ── Verkle (account state)           │
│   └─ spacetime_xition   ── 32-byte digest, domain-tagged    │
├─────────────────────────────────────────────────────────────┤
│ Spacetime layer (THIS CRATE)                                │
│   ├─ Cl(1,3) algebra + Spin⁺(1,3) rotors                    │
│   ├─ Light-cone causal ordering                             │
│   ├─ Geometric median aggregation (>50% Byzantine resist.)  │
│   ├─ Rolling rotor fingerprints (anomaly detection)         │
│   ├─ Clique detection (coordinated wake-up)                 │
│   ├─ Fingerprint Verkle (long-lived evidence storage)       │
│   ├─ Attestation gossip (cross-validator fingerprint check) │
│   ├─ Tiered finality (Soft → Hard via challenge window)     │
│   ├─ Fraud-proof submission and verification                │
│   └─ Growformer ratification (PBFT-gated param changes)     │
├─────────────────────────────────────────────────────────────┤
│ Growformer client (network-loaded, SPACEKIT_API_KEY)        │
│   ├─ Cached fallback                                        │
│   ├─ Circuit breaker (model-mismatch trips fast)            │
│   └─ Pinned model_hash agreement                            │
├─────────────────────────────────────────────────────────────┤
│ Quantum Verkle (SIS-VC, NIST profiles)                      │
│   └─ Account state + fingerprint namespace + rotor sequences│
└─────────────────────────────────────────────────────────────┘
```

Integration on [`spacekit-compute-node`](../spacekit-compute-node/README.md)
(feature `spacetime-consensus`):

| Path | Entry |
|------|--------|
| **Rotor commit** | [`UnifiedConsensusHost`](../spacekit-compute-node/src/unified_consensus_host.rs) → facade `aggregate_votes` → `SpacetimeExtension::aggregate_votes_robust` → **`geometric_median_rotor`** (~50% Byzantine breakdown). Does **not** use Fréchet mean (`aggregate_votes` / `aggregate_rotors`, breakdown `1/N`). Weights on `(Rotor, f64)` pairs are divergence metadata only; the median step is unweighted. |
| **Transition verify** | Facade `verify_transition` during the voting round |
| **Fingerprints, finality, fraud, ratification** | `ConsensusCoordinator` + [`spacetime_integration.rs`](../spacekit-compute-node/src/spacetime_integration.rs) (not re-exported through the facade) |

PBFT quorum remains in `ConsensusCoordinator`. See
[`spacekit-unified-consensus/README.md`](../spacekit-unified-consensus/README.md)
for the layer table (facade vs optional extension).

---

## Module index

| Module | Purpose | Lines | Tests |
|--------|---------|-------|-------|
| `algebra` | Cl(1,3) multivector core, geometric product, serialization | ~300 | 4 |
| `rotor` | Spin⁺(1,3) rotors, exp/log maps, geodesic distance | ~290 | 4 |
| `causal` | Minkowski coordinates, light-cone partial order, antichains | ~200 | 3 |
| `aggregation` | Fréchet mean on Spin⁺(1,3) (Karcher iteration) | ~145 | 3 |
| `proposal` | `SpacetimeTransition` side-car, 208/240-byte serialization (v2) | ~170 | 4 |
| `consensus` | `SpacetimeExtension` — main plug-in to PBFT consensus | ~270 | 3 |
| `light_client` | Stateless rotor-chain verification (browser/light) | ~180 | 4 |
| `defense` | Geometric median, fingerprints, clique detection | ~410 | 4 |
| `equivocation` | Evidence types: dual-rotor, sandwich-mismatch, departure | ~250 | 2 |
| `fingerprint_verkle` | At-rest fingerprint storage in state Verkle | ~405 | 4 |
| `fingerprint_attestation` | Cross-validator fingerprint root agreement gossip | ~315 | 6 |
| `finality` | Soft/Hard finality state machine | ~280 | 5 |
| `fraud_proof` | Submission, verification, rollback dispatch | ~315 | 3 |
| `growformer_ratification` | PBFT-gated parameter changes from Growformer | ~470 | 8 |
| `growformer_client` | Network-loaded inference with caching + breakers | ~310 | 4 |
| `verkle` | Rotor-sequence Verkle binding (cross-block proofs) | ~115 | 1 |
| `kyber_aux` | Kyber1024 sealing for hiding-mode Verkle aux | ~65 | — |
| **tests/e2e_cli_http_finalize** | Integration tests for CLI→HTTP→finalize | ~590 | 7 |

Total: ~4,000 lines of library code + ~600 lines of tests.

---

## What the spacetime layer adds to consensus

### 1. Rotor-valued transitions

Each block proposal includes a `SpacetimeTransition` — a Spin⁺(1,3) rotor `R`,
the residual commitment for non-rotation state changes, integrity hashes,
and a causal coordinate. State update semantics: `S' = R̃ · S · R + Δ`
(sandwich product plus residual). Validators verify both the sandwich
product and the residual commitment independently.

**Cost:** 208 bytes (no hiding) or 240 bytes (with Kyber-sealed hiding mode)
per block. Verification: O(1) sandwich product + norm check + residual hash.

**Buys:** Stateless light clients verify rotor chains in constant time per
transition. No state replay required. The joint signature (rotor magnitude,
residual norm) closes the residual-channel attack surface.

### 2. Causal-set ordering

Every consensus event carries a Minkowski (1+3)-D coordinate. Forward
light cone defines the partial order; spacelike-separated events are
concurrent and resolved by deterministic tie-break (content hash).
Replaces ad-hoc DAG ordering with a metric structure.

**Buys:** Browser VM nodes get correct ordering without holding state.
Coordinated attacks at spacelike-separated coordinates are visible in
the geometry.

### 3. Robust geometric aggregation

`geometric_median_rotor` replaces the Fréchet mean for any commit-path
aggregation. Breakdown-point theory raises the Byzantine threshold from
~1/3 to ~1/2.

**Buys:** A coordinated sleeper attack now needs strict majority of
reputation-weighted voting power, not 1/3.

### 4. Behavioral fingerprinting

Each validator's rolling EWMA centroid + dispersion in joint
(rotor, residual_norm) space. Anomalous transitions (>σ from validator's
own historical centroid) trigger warnings; persistent anomalies trigger
slashing evidence.

**Buys:** Sleeper detonation produces a transition far from the
validator's established neighborhood. Detection is independent of
whether the transition is locally valid.

### 5. Clique detection

Validators at spacelike-separated coordinates producing nearly-identical
transitions in the same round are surfaced as coordination candidates.
Honest validators with independent mempools shouldn't agree this tightly
by chance.

**Buys:** Catches multi-validator coordinated wake-ups that individual
fingerprint checks would miss.

### 6. Cross-validator fingerprint attestation

After each block, every validator broadcasts a signed claim about the
new fingerprint Verkle root. Mismatches between validators on the same
block are slashable evidence with zero false-positive rate (EWMA is
fully deterministic given inputs).

**Buys:** Catches buggy or malicious nodes whose fingerprint
computations diverge from the network consensus.

### 7. Tiered finality with fraud proofs

Two finality stages: Soft (PBFT quorum reached) and Hard (Soft +
challenge window of N successor blocks elapsed). Any block in the Soft
stage can be reverted by a valid fraud proof.

**Buys:** Even a successful >50% attack is recoverable if a single
honest validator survives to submit a fraud proof within the window.

### 8. Growformer ratification

Security-critical parameters (sigma thresholds, divergence thresholds,
challenge windows) cannot be silently updated by the optimizer.
Growformer *proposes* via PBFT-quorumed `ParameterChangeProposal`s;
validators *ratify* by running their own inference and voting;
activation is delayed by `≥ challenge_window` so a poisoned change is
itself recoverable. YES votes on a change later shown to enable an
attack become slashable evidence.

**Buys:** Closes the optimizer-compromise attack surface.

---

## Defense layering summary

| Attack | Without spacetime | With spacetime |
|--------|------------------|----------------|
| Naive 1/3 Byzantine | Safety lost | Safety lost (no help) |
| 1/3–1/2 reputation-bomb sleeper | Safety lost | **Caught by geometric median** |
| 1/2+ reputation-bomb sleeper | Safety lost forever | **Recoverable via fraud proof in challenge window** |
| Single sleeper wake-up | May go undetected | **Caught by fingerprint divergence** |
| Coordinated wake-up | May go undetected | **Caught by clique detection** |
| Residual-channel manipulation | Not detectable | **Caught by joint signature + residual commitment** |
| Malicious sandwich product | Not detectable | **Provable slashing evidence** |
| Equivocating proposer | PBFT catches | PBFT catches (no help) |
| Compromised Growformer optimizer | Silent threshold weakening | **Blocked by ratification + slashable YES votes** |
| Buggy validator fingerprint code | Silently diverges | **Caught by attestation gossip** |

---

## Growformer integration model

Growformer runs as a network service, not a linked dependency.
`SpacetimeConsensusAgent` is the brain consumed by every node.

### Operator on-boarding (testnet phase)

1. Operator registers email + data, receives an `SPACEKIT_API_KEY`.
2. The API key is the participation gate: nodes refuse to run without one.
3. On first startup, node fetches `SpacetimeConsensusAgent.brain` from
   the storage node, encrypts with the operator key, and persists to
   local disk.
4. On subsequent startups, node loads the brain from local disk — no
   network fetch unless the on-disk hash differs from the
   network-canonical hash.
5. New brain releases trigger a ratification cycle (see
   [`RECOVERY_AND_RATIFICATION.md`](RECOVERY_AND_RATIFICATION.md)); after
   activation, nodes re-fetch and re-cache.

Post-testnet, the API key gate is planned to relax so any holder of the
Growformer Runtime binary can participate. The on-disk caching model
carries over unchanged.

### What the agent classifies

| Domain | Output classes | Used by |
|--------|----------------|---------|
| `consensus_tuning` | tighten / loosen / no_change / alert | `growformer_ratification` path |
| `anomaly_scoring` | consistent / mild_drift / strong_anomaly / wake_up_pattern | Per-validator fingerprint augmentation |
| `clique_assessment` | incidental / suspicious / coordinated | Soft signal to slashing severity |
| `fraud_classification` | isolated_bug / minority_attack / network_attack | Slashing severity selection |
| `policy_regime_recommendation` | default / secure / permissive | Regime transition voting |

The `growformer_client` module provides:

- A trait abstraction so HTTP/MCP plumbing lives outside this crate
- On-disk caching encrypted with operator key
- Caching with bounded staleness (configurable per use case)
- Circuit breakers that trip slowly on transient unavailability, fast
  on model mismatches
- Graceful degradation: when Growformer is unreachable, consensus
  continues with the last-ratified static thresholds (defense degrades
  but does not stall)

---

## Integration guides

| Guide | Covers |
|-------|--------|
| [`DEFENSE.md`](DEFENSE.md) | Sleeper-attack defense layers, deployment order |
| [`FINGERPRINT_VERKLE.md`](FINGERPRINT_VERKLE.md) | At-rest fingerprint wiring in coordinator |
| [`RECOVERY_AND_RATIFICATION.md`](RECOVERY_AND_RATIFICATION.md) | Tiered finality, fraud proofs, Growformer ratification |
| [`../spacekit-unified-consensus/README.md`](../spacekit-unified-consensus/README.md) | The facade that brings this crate into the consensus host |
| [`../SPACEKIT_CONSENSUS_UNIFIED.md`](../SPACEKIT_CONSENSUS_UNIFIED.md) | File-level mapping across the SpaceKit stack |

---

## Wire-format invariants (locked)

These are committed to the testnet wire format and bound to envelope
signatures. Changes require a hard fork.

- **Spacetime wire version:** `SPACETIME_WIRE_VERSION = 2`
- **`SpacetimeTransition` serialization (v2):** 208 bytes (no aux) or 240
  bytes (with aux), little-endian floats, big-endian counters where
  relevant, fixed field order. v2 added `residual_commitment` (32 B) and
  `residual_norm` (8 B) to close the residual-channel attack surface.
- **Domain tag:** `b"spacekit-spacetime-transition-v2"` (transition digest)
- **Domain tag:** `b"spacekit-spacetime-residual-v2"` (residual commitment hash)
- **Domain tag:** `b"spacekit-fingerprint-v1"` (fingerprint commitment digest)
- **Domain tag:** `b"spacekit-fingerprint-attestation-v1"` (attestation signing bytes)
- **Fingerprint namespace:** `Address(0xFF...FE)` (reserved in state Verkle)
- **Fingerprint payload:** 92 bytes, version stamp first
- **Rotor encoding:** 8 even-grade f64 coefficients, canonical order
  (scalar, 6 bivectors, pseudoscalar)
- **Rotor norm tolerance:** `|R̃R - 1| < 1e-4` (light client) or `< 1e-6`
  (validator)
- **State reconstruction tolerance:** `|S_new - (R̃·S_old·R + Δ)| < 1e-6`
  (residual must match committed)

### The v2 attack surface fix

In v1, state changes that weren't exact Spin⁺(1,3) rotations had a
residual `Δ` that was opaque to the spacetime layer. An attacker could
craft transitions where the rotor looked innocent (passed fingerprint,
geometric median, clique checks) and the malicious payload lived
entirely in the residual.

In v2, every transition carries:

- `residual_commitment`: a domain-tagged 32-byte hash of the residual
  multivector
- `residual_norm`: the f64 magnitude of the residual

Validators verify BOTH the sandwich product AND the residual commitment
matches what they independently compute. Fingerprints and clique
detection use the **joint signature** `(rotor_magnitude, residual_norm)`
so an attacker can no longer move payload between the two buckets to
evade detection.

Per-transition cost: 40 extra bytes. Per-1000-tx batch: 40 KB extra —
negligible vs. signatures.

---

## Honest assessment

This crate is a serious piece of infrastructure that earns its
sophistication by closing concrete attack surfaces. Each layer addresses
a specific threat the previous layers couldn't catch. The architectural
separation is real — each module can fail independently — and the wire
formats are locked early enough that drift is mechanical to detect.

**What it is not:**

- **A drop-in replacement for PBFT.** It augments an optional sidecar on
  blocks and post-finalize paths. PBFT does the heavy lifting on quorum
  safety; this crate adds detection, robust rotor aggregation, and recovery
  layers on top when enabled.
- **Lightweight in absolute terms.** Each individual operation is fast
  (microsecond rotor arithmetic, microsecond fingerprint updates,
  sub-second Growformer inference), but the system has many moving
  parts. Operational complexity is real and needs ops investment to
  match.
- **Audited.** No independent security review has been performed yet.
  Required before mainnet.
- **A guarantee against ML-driven attacks beyond the optimizer.** The
  ratification path closes the *parameter-tuning* compromise vector. It
  does not close the broader question of how to verify ML behavior is
  faithful to training intent. That's an unsolved research problem
  industry-wide.

**Operational dependencies introduced:**

- The Growformer storage node (single point of bootstrap; brain cached
  on disk after first fetch so steady-state operation doesn't depend on
  it)
- The `SPACEKIT_API_KEY` issuance system (centralized for testnet,
  gating participation; planned to decentralize post-testnet so any
  Growformer Runtime holder can participate)
- The agent.brain artifact (must be signed by genesis authority and
  version-pinned at network level; encrypted on disk with operator key)
- The genesis authority signing key (root of trust for brain updates;
  compromised key compromises agent updates network-wide)

**The training data and pipeline are independent of this crate.**
Training happens in the `spacetime-consensus-agent` project (separate
repo): TOML grounding files + JSONL corpora + Growformer training →
`agent.brain` artifact. This crate consumes only the trained binary at
inference time via the Growformer Runtime.

**Suitable for:** Aggressive testnet deployment with real users, real
bugs, and real adversarial pressure. Targeted at maturation through
operational learning over 6-12 months minimum before mainnet.

---

## Build

```bash
cargo build                                          # std, full features
cargo build --no-default-features --target wasm32-unknown-unknown
cargo test --features "verkle,serde"
cargo test --test e2e_cli_http_finalize
```

Feature flags:

- `std` (default) — Standard library; otherwise no_std.
- `verkle` (default) — Pull in `spacekit-quantum-verkle` for at-rest storage.
- `serde` (default) — Enable serde derives.
- `kyber-aux` — Kyber1024 sealing for hiding-mode aux (requires `pqcrypto-kyber`).
- `growformer-hook` — Expose hook tuples for the optimizer.

---

Made with care by the SpaceKit.xyz team.