# Sleeper-Attack Defenses — Integration Guide

This document is the operational complement to the analysis. It tells you
exactly where each defense plugs in, what it costs, and what the residual
risk is afterward.

## Threat model

**Reputation-bomb sleeper attack.** N validators with verified DIDs
participate honestly for months, accumulating high `effective_voting_power`.
At T-day, they coordinate a malicious commit — invalid state transition,
censorship, deep reorg, or finalization of a forged transition. Every
cryptographic signature is valid; the attack is in the *content* the
signatures cover.

Sleeper success threshold without defenses: ~33% of reputation-weighted
voting power.

Goal: raise this to 50% (geometric median) AND make coordinated wake-ups
*detectable* before, during, and after detonation.

---

## Defense Layer 1 — Reputation Hygiene

**Where:** `UnifiedConsensusValidator` in `spacekit-consensus`. Not provided
by this crate; standard PoS hygiene.

```rust
impl UnifiedConsensusValidator {
    pub fn apply_reputation_decay(&mut self, elapsed_epochs: u64) {
        // Logarithmic cap: r_max = 1 - exp(-k * history_length)
        let k = 0.001;
        let cap = 1.0 - (-k * (self.epochs_active as f64)).exp();
        self.reputation_score = self.reputation_score.min(cap);

        // Inactivity decay: lose 0.5% per inactive epoch.
        if self.consecutive_misses > 0 {
            self.reputation_score *= 0.995_f64.powi(elapsed_epochs as i32);
        }
    }
}
```

Effect: a 5-year sleeper still has reputation ≤ 0.85, not 0.99+. Total
bombable mass is bounded.

Cost: negligible (a few floating-point ops per validator per epoch).
Residual risk: high (this slows the attack, doesn't prevent it).

---

## Defense Layer 2 — Bonded Reputation

**Where:** `DynamicValidatorManager` in `spacekit-consensus`. Also standard.

Require a stake bond proportional to the *square* of reputation:
`bond_required = base_bond * reputation²`. Slashing on any caught
equivocation slashes the entire bond AND zeros reputation. Combined with
Layer 1's cap on max reputation, this puts a hard upper bound on net
attack profitability.

Effect: makes the attack cost-positive only if the corruption payoff
exceeds the slashed bond × N sleepers.
Cost: liquidity drag on validator operators. Tune `base_bond` to match
expected attack payoff.
Residual risk: medium. Wealthy attackers can still pay the bond if the
payoff is large enough.

---

## Defense Layer 3 — Geometric Median Aggregation **(spacetime-specific)**

**Where:** This crate, `defense::geometric_median_rotor`. Replace
`SpacetimeExtension::aggregate_votes`'s call to `aggregate_rotors` with
`geometric_median_rotor`.

```rust
// in src/consensus.rs, aggregate_votes
let mean = geometric_median_rotor(validator_rotors, &GeometricMedianConfig::default())?;
```

Effect: **Byzantine threshold for moving the consensus rotor rises from
~33% to ~50%**. A sleeper quorum must control strictly more than half
of reputation-weighted voting power to drag the median, not a third.

Cost: ~3× the iteration cost of Fréchet mean. Still O(N·k) per round
with k ≤ 32 iterations; negligible vs. signature verification.
Residual risk: low for the aggregation step itself. The threshold raise
is provable from breakdown-point theory.

When to keep the mean: parameter optimization, validator-set statistics —
any aggregation where honest disagreement is dispersion-like rather than
adversarial. Use the median for any commit-path aggregation.

---

## Defense Layer 4 — Behavioral Fingerprinting **(spacetime-specific)**

**Where:** This crate, `defense::FingerprintRegistry`. Hold one inside
`SpacetimeExtension`:

```rust
pub struct SpacetimeExtension {
    // ... existing fields ...
    pub fingerprints: FingerprintRegistry,
    pub sigma_threshold: f64, // recommended: 5.0 for warn, 7.0 for slash
}
```

Update on every observed rotor (proposal or vote):

```rust
let anomaly = self.fingerprints.observe(did_hash, transition.rotor);
if anomaly > self.sigma_threshold {
    // Emit FingerprintDepartureEvidence, downweight this validator's
    // contribution for this round, and queue it for committee review.
}
```

Effect: a sleeper's wake-up rotor is in a different region of the manifold
than their historical centroid; this is detectable as a high anomaly
score *regardless of whether the rotor is locally valid*. Validators
maintaining identical fingerprints converge on the same wake-up
detection independently.

Cost: O(1) per observation. Fingerprint state per validator: ~200 bytes.
Residual risk: low if `sigma_threshold` is tuned well via Growformer.
A sophisticated sleeper could keep their wake-up rotor close to their
fingerprint, but doing so constrains the attack — they can only
"detonate" in directions consistent with their established behavior.

---

## Defense Layer 5 — Clique Detection **(spacetime-specific)**

**Where:** This crate, `defense::detect_coordination_clique`. Run once
per round before finalization, over the round's `RoundSubmission`s.

```rust
let cliques = detect_coordination_clique(&submissions, 0.3, 3);
for clique in cliques.iter().filter(|c| c.coordination_score > 5.0) {
    // Soft response: pause clique members, ramp up committee scrutiny,
    // log evidence. Do NOT auto-slash on this signal alone.
}
```

Effect: catches coordinated wake-ups even when individual rotors don't
trigger fingerprint alerts. Honest validators with different mempools and
different execution traces should *not* produce identical rotors by
chance, especially across spacelike separations.

Cost: O(N²) pairwise comparisons. For N = 1000, ~500K rotor distance
computations per round — still well under signature verification cost.
Residual risk: medium for false positives. This is why we use it as a
*soft* signal feeding into Pause severity, not auto-slashing.

---

## Defense Layer 6 — Tiered Finality with Fraud Proofs

**Where:** `spacekit-consensus` finalization logic. Two-stage:

- **Soft finality:** 2/3 reputation-weighted PBFT quorum. Same as now.
- **Hard finality:** soft + Δ block challenge window elapsed with no
  fraud proof submitted.

Light clients accept soft finality for low-value txns; high-value txns
wait for hard finality (typical Δ ≈ 100 blocks, ~3 min at 2s blocks).

Fraud proofs to accept during the challenge window:

```rust
pub enum FraudProof {
    InvalidTransition(SandwichMismatchEvidence),
    DualSigning(DualRotorEvidence),
    FingerprintDeparture(FingerprintDepartureEvidence),
}
```

Any one honest validator that survived the attack can submit any of these
and force a rollback within Δ.

Effect: even a successful >50% attack is unwound if any honest validator
remains. The attack-cost-to-payoff ratio degrades further.
Cost: latency tradeoff for hard-finality txns. Application-tunable.
Residual risk: low. The attack must defeat *all* honest validators
within Δ, which is far harder than crossing a single threshold.

---

## Defense summary table

| Layer | Spacetime-specific? | Σ-threshold to defeat | Latency | Code home |
|-------|--------------------|-----------------------|---------|-----------|
| 1. Reputation hygiene | No | — | 0 | `spacekit-consensus` |
| 2. Bonded reputation | No | economic | 0 | `spacekit-consensus` |
| 3. Geometric median | **Yes** | **>50% weight** | +negligible | `defense.rs` |
| 4. Fingerprint | **Yes** | requires σ-bounded rotor | +0 | `defense.rs` |
| 5. Clique | **Yes** | requires causal cover | +O(N²) | `defense.rs` |
| 6. Tiered finality | No | all honest validators offline | +Δ | `spacekit-consensus` |

The four spacetime-specific items (3, 4, 5, plus the evidence types in
`equivocation.rs`) are what this layer contributes to the defense that you
*could not get* from a generic reputation-weighted PoS without rotor
structure.

---

## Recommended deployment order

1. **First**: Layer 1 + Layer 2 (cryptoeconomics) — caps the
   accumulation pre-detonation.
2. **Next**: Layer 3 (geometric median) — single biggest threshold raise.
3. **Then**: Layer 6 (tiered finality + fraud proofs) — recovery path.
4. **Then**: Layer 4 (fingerprinting) — needs ~50 rounds of warm-up to
   be effective; deploy early to start accumulating history.
5. **Last**: Layer 5 (clique detection) — Growformer-tuned thresholds
   benefit from production data.

Each layer is additive. None require changing the others.

---

## The Bottom Line

PBFT alone can't solve reputation-bombing because the attack succeeds before any equivocation occurs — the buildup phase is honest by construction. PBFT does contribute equivocation evidence at detonation, but only catches sleepers who slip up. The structural fix is to layer defenses so each one closes a different escape route:

Layer 3 (geometric median) is the strongest single contribution the spacetime structure makes. Breakdown-point theory gives you a provable raise from 33% to 50% — and it's a one-line swap from aggregate_rotors to geometric_median_rotor in your aggregation path.
Layers 4 + 5 (fingerprint + clique) make the coordination visible. Sleepers can fake a single rotor but not their own multi-month rotor history, and they can't make spacelike-separated validators independently produce identical rotors. These are detectors, not blockers, but they raise the operational cost of staying coordinated through detonation.
Layer 6 (tiered finality + fraud proofs) is your recovery floor. Even if everything else fails and a >50% sleeper quorum forms, any single honest validator with the evidence types in equivocation.rs can unwind the attack within Δ blocks.

The pattern: 1 + 2 are cryptoeconomic (slow the buildup), 3 is algorithmic (raise the threshold), 4 + 5 are detective (surface the coordination), 6 is recovery (unwind on failure). Each is independently deployable; the most leverage per code-change-cost is 3 first, then 6, then 4, with 1, 2, 5 in parallel.
One residual risk worth naming: a sophisticated attacker who controls Growformer-tuned threshold values (e.g., by gradually pushing divergence_threshold and sigma_threshold up via small bad reports over a long period) can disarm Layers 3 and 4 from the inside. The mitigation is to make optimizer parameter changes themselves PBFT-quorumed and slashable — Growformer proposes, validators ratify, and a malicious tune is provable evidence. Want me to spec that ratification path next, or build the tiered-finality + fraud-proof submission flow in code?