# spacekit-unified-consensus

The integration facade for the SpaceKit consensus protocol. Wraps a
`CoordinatorHandle` (in production: `ConsensusCoordinator` from
`spacekit-compute-node`) and exposes **`ReputationWeightedConsensus`** —
the documented API for reputation-aware BFT quorum, with a path to
authoritative weighted thresholds after a hard fork.

**This is not a two-tier consensus stack.** In equal-weight / count-mode
(the testnet default), the facade plus coordinator is a **complete** BFT
integration surface: PBFT quorum, vote collection, and finality semantics
live in the coordinator; the facade unifies reputation math and the
documented call shape. **[`spacekit-spacetime-consensus`](../spacekit-spacetime-consensus/README.md)**
is an **optional reference extension** (compile-time feature `spacetime`):
geometric-algebra rotors, robust aggregation, fingerprints, tiered
finality, and fraud proofs **augment** PBFT — they do not replace the
2/3 quorum check.

This crate does not contain PBFT machinery or spacetime math. It is thin
glue (~600 lines plus tests) over the coordinator and, when enabled, the
spacetime extension.

**License:** Apache-2.0.

**Status:** **Wired in `spacekit-compute-node`** (feature `spacetime-consensus`)
via [`UnifiedConsensusHost`](../spacekit-compute-node/src/unified_consensus_host.rs).
Equal-weight reputation matches testnet today (`EqualWeightReputation`).
Weighted-threshold mode is implemented and tested but **not authoritative**
until a hard fork sets `FacadeConfig::use_weighted_threshold = true`.

For aspirational / whitepaper-style narrative (GPU committees, cross-chain
finality, etc.), see
[`spacekit-compute-node/documentation/SPACEKIT_CONSENSUS_UNIFIED.md`](../spacekit-compute-node/documentation/SPACEKIT_CONSENSUS_UNIFIED.md)
— that document is **not** the as-built map; this README and the compute-node
README below describe what ships.

---

## Architecture (standalone facade + optional spacetime)

| Layer | Component | Role |
|-------|-----------|------|
| 1 | `ReputationSource` | Pluggable per-validator reputation (trait) |
| 2 | `CoordinatorHandle` | Pluggable BFT coordinator seam (trait) |
| 3 | `ReputationWeightedConsensus` | **The protocol facade** — complete for count-mode BFT today |
| Optional | `SpacetimeExtension` | Reference extension in `spacekit-spacetime-consensus`; augments, does not replace quorum |

**Safety backbone:** `ConsensusCoordinator` (PBFT, P2P, PQ finisher) in
`spacekit-compute-node`. The facade does not reimplement it.

**Spacetime is not a trait** like `ReputationSource`. Operators enable it
via Cargo feature `spacetime` (default in this crate) and, on the node,
`spacetime-consensus`. `UnifiedConsensusHost` owns a `SpacetimeExtension`
and passes it into facade rotor methods.

**Two call sites on the node** (by design):

| Surface | Routed through | Examples |
|---------|----------------|----------|
| Facade + host | `ReputationWeightedConsensus` / `UnifiedConsensusHost` | `aggregate_votes`, `verify_transition`, weighted vote telemetry |
| Coordinator + bridge | `ConsensusCoordinator`, `spacetime_integration.rs` | Fingerprints, tiered finality, fraud proofs, parameter ratification, post-finalize side effects |

Do not read “facade” as “every spacetime API.” The facade is the single
entry for **rotor aggregation and transition verification**; the
coordinator remains authoritative for the rest when spacetime is enabled.

---

## About the SpaceKit consensus protocol

**Core (always):** identity-native validators, post-quantum signatures, and
BFT quorum via `ConsensusCoordinator`, exposed through this facade for
reputation-weighted voting (observable today; authoritative after fork).

**Optional spacetime extension:** when enabled, validator agreement can
also be expressed as a Spin(1,3) rotor (Clifford algebra Cl(1,3)) on
[`BlockData`](../spacekit-compute-node/src/consensus.rs). Geometric-median
aggregation and defense layers (fingerprints, tiered finality, fraud
proofs) live in
[`spacekit-spacetime-consensus`](../spacekit-spacetime-consensus/README.md).
That crate’s README describes what the extension **adds** on top of PBFT;
this crate describes how to **wire** reputation and the facade.

---

## Why this crate exists

Most blockchains protect against one or two attack patterns and assume
the rest are rare. SpaceKit builds defense in depth against five distinct
attack classes: Byzantine behavior, sleeper detonation, coordinated
cliques, residual-channel manipulation, and ML governance compromise.
Each is caught by a different layer; structural recovery via tiered
finality and fraud proofs handles the case where any of them succeed.

But "defense in depth" only works if the layers actually compose into
one API. The SpaceKit documentation has historically described a
`ReputationWeightedConsensus` type that nobody could `cargo doc --open`
because it didn't exist as code — the actual host was
`ConsensusCoordinator` in `spacekit-compute-node`, with the spacetime
extension reached through side-band integration shims, and reputation
sat in `MLReputationEngine` with no path into the vote tally.

This crate makes the documented type real and gives the protocol one
integration surface for the **facade concerns**:

- Reputation flows in through a single trait (`ReputationSource`), not
  ad-hoc per-call lookups
- Rotor aggregation and transition verification go through the facade when
  `spacetime` is enabled (not scattered ad-hoc shims)
- Fingerprints, finality, fraud proofs, and ratification stay on the
  coordinator + `spacetime_integration.rs` — see the call-site table above
- The path from equal-weight voting (today) to reputation-authoritative
  voting (post-fork) lives in one place with one config flag

The facade is thin (~600 lines plus tests) and delegates everything to
the crates below it. It does not reimplement consensus; it makes the
existing pieces addressable as one thing.

We're building for a threat model where quantum computers, nation-state
actors, and AI-augmented adversaries become routine concerns. If that's
the next decade, this architecture is calibrated for it. If it isn't,
there are simpler protocols that will outperform on throughput and
operational complexity. That's the bet.

See [`SPACEKIT_CONSENSUS_UNIFIED.md`](../SPACEKIT_CONSENSUS_UNIFIED.md) §1
for the file-level mapping across the SpaceKit stack and §1.4 for the
migration plan from equal-weight to reputation-authoritative voting.

---

## What's in the crate

| Module | Purpose |
|--------|---------|
| `facade` | `ReputationWeightedConsensus<C>`, `CoordinatorHandle` trait, `FacadeConfig` |
| `validator` | `UnifiedConsensusValidator` — facade-side view augmenting `ValidatorEntry` |
| `voting_power` | `effective_voting_power` calculation (sqrt-stake × reputation × performance) |
| `reputation_hook` | `ReputationSource` trait + default `EqualWeightReputation` + `CachedReputationMap` |
| `spacetime_integration` | Bridge to `spacekit-spacetime-consensus` (feature `spacetime`, default on) |

---

## The two modes

### Mode 1 — Equal weight (today, testnet)

```rust
use spacekit_unified_consensus::ReputationWeightedConsensus;

// Coordinator is whatever implements CoordinatorHandle; in production this
// wraps spacekit-compute-node's ConsensusCoordinator via a thin adapter.
let facade = ReputationWeightedConsensus::new_equal_weight(coordinator);

// All validators have effective_voting_power = sqrt(stake) * 1.0 * 1.0.
// Threshold check defers to the coordinator (count-based 2/3).
let voting = facade.collect_weighted_votes(block_hash);
match facade.has_consensus(&voting) {
    Ok(()) => /* quorum reached, finalize */,
    Err(e) => /* not yet */,
}
```

### Mode 2 — Reputation-authoritative (post-fork)

```rust
use spacekit_unified_consensus::{ReputationWeightedConsensus, FacadeConfig};

// An authoritative reputation source derives weights deterministically
// from on-chain state. Every validator at the same height computes the
// same weights.
let rep = Box::new(my_on_chain_reputation_source);

let mut config = FacadeConfig::default();
config.use_weighted_threshold = true;  // Threshold check uses weights

let facade = ReputationWeightedConsensus::new(coordinator, rep, config)?;

// Now has_consensus uses sum-of-supporting-power / total-power
// against the 2/3 threshold instead of count-based 2/3.
```

Switching from Mode 1 to Mode 2 is a hard fork. The facade refuses to
construct with `use_weighted_threshold = true` and a non-authoritative
reputation source, so the failure mode is "won't start" rather than
"silently disagrees with peers."

---

## Production wiring (as-built)

The facade is generic over `CoordinatorHandle`. In production,
**`spacekit-compute-node`** implements the seam without changing
coordinator semantics:

| Piece | Location |
|--------|----------|
| Facade type | `ReputationWeightedConsensus<C>` (this crate) |
| Host | `UnifiedConsensusHost` — owns `Arc<ConsensusCoordinator>` + `SpacetimeExtension` |
| Per-round adapter | `CoordinatorRoundHandle` — sync view over `CoordinatorRoundSnapshot` |
| Snapshot source | `ConsensusCoordinator::capture_round_snapshot(proposal_id)` |
| Block key | `keccak256(proposal_id.as_bytes())` — stable `B256` for facade rounds |
| P2P telemetry | `UnifiedConsensusHost::start_p2p_listener()` → after each vote, `observe_vote_round` (non-gating, `debug!`) |
| PQ finalize tripwire | Standalone: coordinator `check_finality` **then** `host.has_consensus` (count-mode consistency check) |

**Coordinator additions (additive only):** `capture_round_snapshot`,
`record_vote_by_did_hash`, `proposal_block_hash`, snapshot field
`supporting: Vec<B256>` (approve-set DID hashes).

**Locking:** The host is stateless across calls. Each facade use builds a
fresh handle from a coordinator snapshot; durable state stays in the
coordinator's existing `RwLock`s. `submit_vote_raw` uses `block_on` only
to reach `record_vote_by_did_hash`, which must **not** call back into the
facade (documented in `CoordinatorRoundHandle`).

### `CoordinatorHandle` contract

| Method | Purpose |
|--------|---------|
| `eligible_validators` | `(did_hash, stake)` for the round |
| `submit_vote_raw` | Record approve/reject by DID hash |
| `supporting_vote_count` | Count of approve votes |
| `supporting_validators` | DID hashes that voted approve (for weighted sums) |
| `eligible_validator_count` | Eligible validator count |
| `is_soft_finalized` | Coordinator considers the block finalized |

`collect_weighted_votes` sums `effective_voting_power` over
**`supporting_validators`**, not a count-based approximation — required
for correct post-fork weighted threshold.

### Post-fork finality ordering

Today (count-mode): coordinator finality is authoritative; `has_consensus`
defers to `coordinator_finalized` and acts as a **tripwire** if the two
ever disagree.

After `use_weighted_threshold = true`: the facade's weighted quorum check
becomes **authoritative**. Finalization paths must flip to **host-first**
(or replace the coordinator count check). Anchor comments live in
`standalone.rs` (`pq_finalize_after_propose`) for the fork PR.

---

## Spacetime integration (optional extension)

Disable with `cargo build --no-default-features --features std` (no spacetime
dependency). On `spacekit-compute-node`, the whole stack is gated by feature
`spacetime-consensus`.

When `spacetime` is enabled, the facade exposes:

- `aggregate_votes(…)` — calls `SpacetimeExtension::aggregate_votes_robust`,
  which uses **`geometric_median_rotor`** (Spin⁺(1,3) median, ~50% Byzantine
  breakdown point). It does **not** call `aggregate_votes` / Fréchet mean
  (`aggregate_rotors`), whose breakdown point is `1/N`. Rotor weights in the
  `(Rotor, f64)` pairs are for **divergence metadata** in `ConsensusRotor`;
  the median step itself is unweighted (reweighting would reintroduce
  leverage from high-weight adversarial inputs).
- `verify_transition(…)` — verifies a single validator's transition against
  the proposer's claim during the voting round.

Other spacetime surfaces (fingerprint updates, attestation gossip, tiered
finality, fraud proofs, parameter ratification) remain on
`SpacetimeExtension` / `ConsensusCoordinator` and are invoked from the
node when needed. The facade brokers rotor aggregation and transition
verification, not the full spacetime API.

For the spacetime crate's own architecture, see
[`spacekit-spacetime-consensus/README.md`](../spacekit-spacetime-consensus/README.md).

---

## Reputation source contract

`ReputationSource` is a trait with two methods:

```rust
fn reputation_of(&self, validator_did: &B256) -> Option<f64>;
fn is_authoritative(&self) -> bool { false }
```

Implementations document which mode they operate in.

**Observable mode (default).** Returns reputation values that may differ
across nodes. Useful for monitoring and per-node UX. Not safe for quorum
threshold computation.

**Authoritative mode.** Returns reputation values that are deterministic
from on-chain data and identical across all nodes at a given height.
Required for `use_weighted_threshold = true`.

Built-in implementations:

- `EqualWeightReputation` — always 1.0. Default.
- `CachedReputationMap` — in-memory map. Useful for tests; can be
  authoritative if the populator guarantees determinism.

---

## Build

```bash
cargo build --release
cargo test
cargo test --no-default-features --features std  # without spacetime
```

`spacekit-log` integration is behind feature `log` and exposes emission
hooks the facade can call when integrated with `LogSink` implementations
in the node.

---

## What this crate does NOT include

- The consensus math itself ([`spacekit-spacetime-consensus`](../spacekit-spacetime-consensus/README.md) owns this)
- The PBFT machinery (`ConsensusCoordinator` owns this)
- The P2P layer (lives in the node)
- The on-chain reputation derivation (separate concern; this crate
  consumes whatever a `ReputationSource` provides)
- Stake calculation or slashing (separate concerns)
- Block storage, mempool, RPC (all in the node)

The facade is glue. The substance is in the crates it delegates to.

---

## Roadmap (foundation is solid; these extend it)

| Item | Intent | Notes |
|------|--------|--------|
| **Growformer-through-host (#1)** | Telemetry only | Non-blocking `observe_*` after inference — mirrors P2P vote telemetry. **No gating.** |
| **Growformer-through-host (#2)** | Parameter ratification routing | Host packages `ParameterChangeProposal`; coordinator votes; activation tracked. Governance hot path. |
| **Growformer-through-host (#3)** | Per-block inference in `collect_weighted_votes` | **Avoid** unless measured need — puts variable ML latency on the consensus hot path. |
| **Growformer failure mode** | Graceful degradation | Unreachable or low-confidence inference → log, continue with last-ratified static thresholds; **do not** refuse consensus. Document before wiring. |
| **`MLReputationEngine` → `ReputationSource`** | Authoritative weights post-fork | Plug in when enabling `use_weighted_threshold`. |
| **`GET /v1/consensus/weighted-votes`** | Observability | Facade telemetry for operators. |
| **P2P fingerprint attestation gossip** | Cross-validator fingerprint roots | Spacetime crate + coordinator; not facade-owned. |
| **Post-fork weighted threshold** | Hard fork | Flip finality ordering; enable authoritative `ReputationSource`. |

---

Made with care by the SpaceKit.xyz team.