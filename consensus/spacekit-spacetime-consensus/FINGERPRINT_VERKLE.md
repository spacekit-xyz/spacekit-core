# Fingerprint Verkle Integration Guide

This document covers wiring `fingerprint_verkle.rs` into the
`spacekit-compute-node` standalone path. The intent stated in
`SIGNATURE_POLICY.md` ("rotors-in-flight in Merkle, fingerprints-at-rest in
Verkle") is now backed by code.

## What's done

- `FingerprintCommitment` — 92-byte serialized form of `RotorFingerprint`,
  with deterministic byte order and version stamp.
- `FINGERPRINT_NAMESPACE = 0xFF...FE` — reserved Verkle address for the
  fingerprint namespace, distinct from user state and from any future
  system namespaces.
- `FingerprintVerkle` (under `feature = "verkle"`) — wraps your
  `QuantumTree<NistSisScheme>` with apply/prove/verify methods.
- `apply_fingerprint_batch` — the only public write entry point; holds an
  unforgeable `ConsensusWriteCap(())` witness inside the spacetime crate.
- Domain-tagged digest (`b"spacekit-fingerprint-v1"`) — matches the
  policy doc's tagging convention.

## What's NOT done (deliberately deferred)

- **Idle sweeping logic.** `sweep_idle()` is a stub. Wiring requires
  adding a "last seen at block N" counter to `FingerprintCommitment`,
  which is a wire format change. Add this when you have production data
  on validator churn patterns.
- **Fingerprint cross-validation.** Two honest validators computing the
  same EWMA update should produce byte-identical commitments. The
  consensus crate should check this and slash mismatches as evidence
  of a buggy or malicious node. The hook for this — `apply_batch`
  returning the touched DIDs — is present; the cross-check logic
  belongs in `spacekit-compute-node`.
- **Reorg handler wiring.** `ConsensusCoordinator::rollback_fingerprints_to_height`
  and `spacetime_integration::rollback_block_spacetime_side_effects` restore
  per-height `FingerprintStoreSnapshot`s (256-block window). Call these from
  the fraud-proof / challenge-window path when a finalized block is reorged out.

## Where to wire it in `spacekit-compute-node`

### 1. Owning the state

The `FingerprintVerkle` lives alongside the existing state Verkle in the
consensus coordinator:

```rust
pub struct ConsensusCoordinator {
    // ... existing fields ...
    pub state_verkle: QuantumTree<NistSisScheme>,
    pub fingerprint_verkle: FingerprintVerkle,  // NEW
}
```

You could also keep it as a separate namespace inside the existing
`state_verkle`. Either works. Separate gives clearer ownership and
simpler reorg snapshots; combined gives a single state root.

**Recommendation:** start separate, with the fingerprint root hashed
into the state root via the envelope:

```rust
// In BlockEnvelope construction:
state_root: combined_state_root(self.state_verkle.root(), self.fingerprint_verkle.tree.root()),
```

The combined-root computation is just a domain-tagged hash of both
roots. Light clients verify either by recomputing from both.

### 2. Update path in finalization

In `finalize_proposal_if_ready`, after collecting vote leaves and before
building the envelope:

```rust
use spacekit_spacetime_consensus::{
    apply_fingerprint_batch, FingerprintVerkle,
};

// Extract (validator_did_hash, rotor) pairs from vote leaves.
let fingerprint_updates: Vec<(B256, Rotor)> = quorum.votes.iter()
    .filter_map(|v| {
        let rotor = v.transition.as_ref()?.rotor;
        Some((v.voter_did_hash, rotor))
    })
    .collect();

// Apply atomically.
let touched = apply_fingerprint_batch(
    &mut self.fingerprint_verkle,
    &fingerprint_updates,
    DEFAULT_DECAY, // e.g. 0.95
    keccak256,
);

// Emit events for any updated fingerprints (debug + slashing pipeline).
for did_hash in touched {
    self.emit_event(FingerprintUpdated { did_hash, block: block_number });
}
```

`ConsensusWriteCap` is a private tuple struct inside the spacetime crate;
compute-node must not construct it. Use `apply_fingerprint_batch` or
`ConsensusCoordinator::apply_fingerprints_from_block` (idempotent, snapshots
per height).

### 3. Reading path for fraud proofs

When a node observes an anomalous rotor and wants to submit
`FingerprintDepartureEvidence`, it reads the fingerprint at the time of
the offending block (from its own Verkle history, or via gossip from
peers):

```rust
let commitment = self.fingerprint_verkle.get(&suspect_did_hash)
    .ok_or(SubmitError::NoFingerprintHistory)?;
let fingerprint = commitment.to_fingerprint()
    .ok_or(SubmitError::CorruptedFingerprint)?;

if fingerprint.is_anomalous(suspect_rotor, sigma_threshold) {
    let evidence = FingerprintDepartureEvidence {
        validator_did: suspect_did_hash,
        transition: suspect_transition,
        fingerprint_at_event: fingerprint,
        sigma_threshold,
    };
    self.submit_fraud_proof(evidence).await?;
}
```

For the evidence to be verifiable by other nodes, attach a Verkle proof:

```rust
let verkle_proof = self.fingerprint_verkle.prove_fingerprint(&suspect_did_hash);
```

Then any verifier with the state root from the offending block can
confirm: yes, that was the fingerprint at the time. They run
`verify_fingerprint_proof()` against the state root and the commitment,
then run `is_anomalous()` themselves.

### 4. Light-client path

Browser VM nodes don't typically need fingerprint state — anomaly
detection is a validator-side activity. But for high-assurance light
clients that want to verify slashing events:

1. Receive `FingerprintDepartureEvidence` + Verkle proof.
2. Verify the Verkle proof against the state root in the cited block's
   envelope.
3. Reconstruct the `RotorFingerprint` from the commitment.
4. Run `is_anomalous()` locally.
5. Verify the offending rotor against the cited block's vote leaf.

All five steps are constant-time (no state replay).

## Storage growth and bounds

Per-validator storage: 92 bytes payload + 32-byte Verkle value + tree
overhead (~96 bytes for the SIS commitment). Round to ~200 bytes per
validator in the tree.

At 1,000 validators: ~200 KB. At 10,000: ~2 MB. Sweep stale fingerprints
periodically (see Idle sweeping note above) to keep this bounded.

Per-block update cost: O(V) Verkle sets, each ~7µs by your bench data,
so ~350µs for V=50, ~7ms for V=1000. This is well under your block-time
budget regardless of validator count.

## Wire format stability

`FINGERPRINT_WIRE_VERSION = 1`. To change the format:

1. Bump `FINGERPRINT_WIRE_VERSION` to 2.
2. Add a `from_bytes_v1` migration path.
3. On node startup, if any stored commitment has `version = 1`, migrate
   in place during the next finalization round (atomic with the existing
   batched update flow).
4. Light clients see a version-2 commitment in the proof; they need to
   be running compatible code OR fall back to "the chain advanced past
   my version, please update."

The version stamp is the *first 4 bytes* of the payload, before any
floats — so detecting an old-version commitment is a 4-byte read.

## Open question for the standalone

How do you want to handle the **first observation** for a new validator?
Two options:

**A. Initialize at first vote.** First-time validators have
`samples = 1` after their first vote. Their first ~16 votes carry no
anomaly detection (warm-up). This is what `apply_batch` does today.

**B. Initialize at admission.** When `DynamicValidatorManager` admits
a new validator, eagerly insert a fingerprint with `samples = 0` and
`centroid = Rotor::IDENTITY`. This makes the state-root delta from
admission explicit (one Verkle set per admission), but adds no
detection capability — anomaly detection still needs 16+ samples.

I'd take **A** unless there's an audit reason to prefer B. It's simpler
and the cost is the same.
