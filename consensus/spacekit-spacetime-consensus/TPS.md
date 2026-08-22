# Spacetime Consensus TPS Analysis

> **Note on the numbers in this document:** Everything labeled as a TPS
> figure below is a **theoretical calculation derived from per-operation
> costs**, not a measurement from a running network. The SpaceKit
> consensus protocol has not been benchmarked in production. These
> numbers exist to support capacity planning and architectural
> discussion, not as performance claims.
>
> Production networks routinely come in 30–60% below their theoretical
> ceiling once the conditions per-operation math doesn't capture are
> accounted for. In a Rust validator stack, the sources of that gap are
> specific and known: allocator stalls under sustained allocation
> pressure, mutex contention on hot data paths (logging, mempool, vote
> aggregation), RocksDB compaction and fsync latency during state
> persistence, OS scheduling jitter and NIC interrupt coalescing, TCP
> retransmission under load, tail latency in Growformer inference when
> consulted on the hot path, and Verkle state-tree maintenance competing
> for CPU with consensus.
>
> None of these is a flaw, they're the unavoidable reality of running
> a deterministic protocol on non-deterministic hardware. The testnet
> exists to measure how much they actually cost. Plan capacity assuming
> real numbers will sit meaningfully below the calculated ceiling.

## Scope of this document

This document is about the **SpaceKit consensus protocol as a whole**,
not the `spacekit-spacetime-consensus` crate specifically.

The spacetime crate produces 32-byte transition digests that are folded
into the outer SPHINCS+ envelope. It does not perform signature
operations itself. All Dilithium and SPHINCS+ verification lives in
`spacekit-compute-node`, specifically `pq_envelope.rs` (envelope
construction and verification) and `pq_finisher.rs` (the finalization
hot path). The amortization math and bottleneck attribution below refer
to the protocol's hot path, which sits in those compute-node modules.

TPS is bounded by very specific things, and several of them are *not*
what people usually assume.

---

## Signature policy (implemented in `pq_envelope.rs`)


| Layer                 | Algorithm                              | Frequency                           | Purpose                                       |
| --------------------- | -------------------------------------- | ----------------------------------- | --------------------------------------------- |
| **Inner consensus**   | **Dilithium2**                         | Per validator vote (PREPARE/COMMIT) | Fast PBFT loop; ephemeral messages            |
| **Outer block**       | **SPHINCS+-SHAKE-256-128s-simple**     | **Once per finalized block**        | Long-lived anchor for light clients / history |
| **User / browser tx** | **Dilithium2** (recommended)           | Per transaction                     | Avoid 5–50ms SPHINCS+ sign in WASM            |
| **DID / registry**    | **SPHINCS+** (existing `spacekit-did`) | Rare                                | Identity registration, validator key binding  |


Validator **consensus Dilithium keys** are separate from DID **SPHINCS+**
keys. Register the Dilithium public key with a SPHINCS+-signed DID
statement at join time.

Types: `[ConsensusVoteInner](pq_envelope.rs)`, `[BlockEnvelope](pq_envelope.rs)`,
`[SignedBlockEnvelope](pq_envelope.rs)`.
Compute-node validation: `[spacetime_integration.rs](../spacetime_integration.rs)`
(`validate_block_pq_envelope`).

---

## What actually gates throughput (in calculation)

With the split above, **SPHINCS+ no longer scales with batch size or**  
**validator count**, only **block rate** matters for outer envelopes.

**Dilithium2 verification** dominates the hot path at scale (~1µs per
sig GPU-batched). Rotor / Verkle / causal checks remain negligible.

Per-transaction validator cost (approximate, GPU-batched, from
per-operation benchmarks):

- Dilithium verify (user + amortized vote): ~1µs
- SPHINCS+ verify (outer): ~10µs **÷ batch size** (one envelope per block)
- Verkle commit / proof verify: ~7µs (benchmark-dependent)
- Rotor + causal: ~1µs combined

The spacetime layer's per-tx cost is microseconds, well below the  
signature and network costs that dominate the calculation. The layer's  
TPS impact is **positive** for light clients (stateless rotor-chain
verify, no replay required).

---

## Amortization math

With batch size `B`, validator count `V`, block time `T_block`:

```
T_batch ≈ B × t_dilithium_user
        + V × t_dilithium_vote
        + 1 × t_sphincs_block
        + V × t_consensus_round   # network RTTs
```

Per-transaction cost: `T_batch / B`. At large `B`, cost converges to
`**t_dilithium_user**` plus `**(V × t_dilithium_vote) / B**`.

The older formula `(B + V) × t_sphincs` applied when **every** vote
carried SPHINCS+; that path is deprecated. The split-signature design
is what makes the V/B amortization work.

---

## Calculated TPS estimates (50 validators, GPU-assisted)

These are calculations under the assumptions above. Real production
numbers will be lower; how much lower is what testnet measures.


| Load profile | Batch size        | Block cadence | Calculated throughput         |
| ------------ | ----------------- | ------------- | ----------------------------- |
| Light        | 1–100             | 200–400ms     | 100–1,000 TPS (latency-bound) |
| Moderate     | 500–2,000         | 300–500ms     | 3,000–10,000+ TPS             |
| Heavy        | 5,000–20,000      | 400–800ms     | 15,000–50,000+ TPS            |
| Saturation   | bandwidth-limited | 500–1,000ms   | Dilithium + network co-limit  |


**Finality** stays ~2 RTTs for PBFT under the same assumptions; rotor +
Fréchet median add ~10–20ms to the per-round latency.

What these calculations do NOT account for:

- Real geographic distribution effects beyond a single RTT assumption
- Mempool contention and transaction-ordering overhead
- Gossip-layer bandwidth contention between consensus and tx propagation
- GC pauses, OS jitter, and other tail-latency contributors
- Adversarial behavior during high load (clique detection runs, fraud
proof submissions, regime transitions)
- Validator-set churn during operation
- Real workload distribution (large vs small txs, contract invocation costs)

These are the same conditions that pull Solana from its 65K theoretical
ceiling to its 3–4K measured sustained throughput. SpaceKit will face
the same kinds of gaps; the testnet exists to measure them.

---

## Design decisions (unchanged)

- **Browser VMs package proofs**: good for TPS; users should  
**Dilithium-sign** txs, not SPHINCS+ per tx.
- **Validators**: recompute rotor for high-value; verify-only for  
low-value (configurable).
- **Variable batches**: dominant lever for `V/B` amortization.
- **On-demand block time**: min 100–200ms, max 2–3s; trigger on batch  
size or max delay.

---

## Testing checklist

1. **V/B ratio**: keep validators < 20% of typical batch size.
2. **Envelope binding**: tamper `votes_merkle_root` or `state_root` →
  verify fails.
3. **Split sig policy**: inner votes must not require SPHINCS+; outer
  block must.
4. **Geometric median**: stress-test at 500 validators.
5. **GPU queues**: separate Dilithium (continuous) and SPHINCS+
  (per-block) batch verifiers.

---

## Light-client throughput (more defensible than validator TPS)

Browser VM nodes and stateless light clients verify rotor chains
without state replay. Per-transition cost is straightforward CPU
arithmetic and bounded by deterministic work, not by network or
adversarial behavior:


| Check                         | Cost        |
| ----------------------------- | ----------- |
| Norm check `|R̃R - 1| < 1e-4` | ~50 ns      |
| Causal cone check             | ~10 ns      |
| Hash comparison (chain link)  | ~50 ns      |
| Residual commitment hash      | ~200 ns     |
| **Total per transition**      | **~300 ns** |


This calculation produces **~3 million transitions per second**
verifiable on a modest laptop. For a 10K-tx batch, full rotor-chain
verification takes ~3 ms; syncing 1000 blocks of 10K txs each, ~3
seconds total.

This is on firmer ground than the validator-throughput numbers above
because it's bounded by deterministic CPU work, not by network
behavior, GPU throughput, or PBFT round dynamics.

---

## Summary

Calculations suggest the protocol may sustain **5–15K TPS in normal
operation, with potential for higher throughput under load once
batching matures**, alongside **sub-second soft finality** and
**near-disk-speed rotor-chain checks** for light clients.

The bottleneck the calculations identify is **post-quantum signatures
on the inner loop (Dilithium) and network round-trips**, not spacetime
math. **SPHINCS+ on the outer envelope only** is the intended
production configuration; the V/B amortization math relies on it.

These are calculations, not measurements. Production TPS will be
determined by testnet, not by this document. The honest expectation is
that real numbers will sit below the calculated ceiling, and the
purpose of the calculation is to identify *where* the bottlenecks live
(signatures and network, not the spacetime layer) so that engineering
effort targets the right places.