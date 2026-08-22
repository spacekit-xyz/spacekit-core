# spacekit-mempool

Transaction mempool for the SpaceKit network. Single ingress, drain on
propose, requeue on fraud-proof rollback, pluggable priority and
(optionally) visibility strategies.

This crate is the consolidated home for what was previously three loose
pieces in `spacekit-compute-node`: the SwtchVM `pending_transactions`
list, the propose-path body-of-block ingress, and the unimplemented
fraud-rollback `requeue_block` hook documented in
[`RECOVERY_AND_RATIFICATION.md`](../spacekit-spacetime-consensus/RECOVERY_AND_RATIFICATION.md).
It puts those three behind one type with one lifecycle.

**License:** Apache-2.0.

**Status:** Core lifecycle complete. Integration adapter for
`spacekit-compute-node` is the next step; that adapter is a thin layer
mirroring the pattern used for `UnifiedConsensusHost`.

---

## About the SpaceKit mempool

The mempool stores **references and metadata** for pending transactions,
not transaction bodies. Body storage lives in the VM or in a separate
content-addressed blob store; the mempool's job is ordering, lifecycle,
and eviction.

Lifecycle:

```
              ┌──────────────────────┐
              │                      │
              ▼                      │
 submit ──► Pending ──► InFlight ──► Finalized
              ▲           │
              │           │ requeue_block (fraud proof rollback)
              └───────────┘
```

- **Pending.** Eligible for inclusion in a proposed block.
- **InFlight.** Drained into a proposed block, awaiting soft finality.
  Not eligible for re-drain (would double-spend) but kept in the index
  in case the block is reverted.
- **Finalized.** The containing block reached hard finality. Dropped on
  the next eviction cycle.
- **Requeue.** On fraud-proof acceptance of a soft-finalized block, all
  InFlight entries for that block return to Pending with their
  `requeue_count` incremented.

---

## Why this crate exists

The compute-node previously had **three partial pools** doing different
parts of mempool work, none of them aligned with what the protocol
documents assume exists:

| Pool | Where | What it does |
|------|--------|----------------|
| SwtchVM tx pool | `swtchvm_node.rs` → `pending_transactions` | `submit_transaction` queues txs; `mine_block` drains them |
| Consensus staging | `consensus_coordinator.rs` → `pending_blocks`, `pq_votes` | Blocks/votes for finality, not user txs |
| Propose path | `standalone.rs` `POST /v1/consensus/propose` | Txs come from JSON body, not from the SwtchVM pool |
| Fraud rollback requeue | Documented in spacetime docs | `mempool.requeue_block` — **not implemented** |

This split has three consequences:

1. **Inconsistent ingress.** A transaction submitted via HTTP doesn't
   necessarily end up in the SwtchVM pool, and vice versa. Which pool
   gets drained at propose time depends on the call path.
2. **No fraud-rollback recovery.** Soft-finalized blocks reverted by
   fraud proofs lose their transactions. The recovery doc assumes a
   `requeue_block` hook that doesn't exist.
3. **No unified observability.** Operators can't ask "how big is the
   mempool" because there are several mempools.

This crate consolidates the three into one type with one lifecycle and
one set of observability hooks.

---

## What's in the crate

| Module | Purpose |
|--------|---------|
| `types` | `TransactionRef`, `MempoolEntry`, `EntryState`, `MempoolStats` |
| `priority` | `PriorityStrategy` trait + `FeePerByteDescending` (default) + `ObservationOrder` (FIFO) |
| `mempool` | `Mempool<P>` — the main type, generic over priority strategy |
| `visibility` (feature) | `VisibilityStrategy` trait + `EverythingPublic` (default) |

The mempool is `Send + Sync` but not internally synchronized. The
integration adapter in `spacekit-compute-node` wraps it in
`Arc<RwLock<Mempool<P>>>` — the same pattern used for
`UnifiedConsensusHost`.

---

## Basic usage

```rust
use spacekit_mempool::{Mempool, MempoolConfig, FeePerByteDescending, TransactionRef};

let config = MempoolConfig::default();
let mut mempool = Mempool::new(FeePerByteDescending, config);

// Submit (called from HTTP handler, P2P listener, etc.):
let tx_ref = TransactionRef { /* hash, sender, nonce, fee, ... */ };
mempool.submit(tx_ref)?;

// Propose path:
let selected = mempool.drain_for_block(/* max_count */ 5000, /* max_bytes */ 1_000_000);
// ... build block proposal with `selected` ...
mempool.mark_in_flight(&selected, block_height)?;

// PQ finalize path (hard finality):
mempool.mark_finalized(&selected, block_height)?;

// Fraud-proof rollback path (in finality.rs `on_fraud_proof_accepted`):
let requeued_count = mempool.requeue_block(block_height);

// Eviction (called periodically, e.g., once per block):
let evicted = mempool.evict_expired(current_block);

// Observability:
let stats = mempool.stats();
println!("pending={} in_flight={} senders={}",
    stats.pending_count, stats.in_flight_count, stats.distinct_senders);
```

---

## Lifecycle invariants

The mempool enforces three properties that matter for consensus correctness:

**Per-sender nonce monotonicity in drain.** `drain_for_block` will not
include a transaction with sender S, nonce N+1 unless a transaction with
sender S, nonce N was already drained (or is already in flight). This
prevents proposing blocks with nonce gaps.

**No re-drain of in-flight transactions.** Once an entry transitions to
InFlight, it's excluded from future `drain_for_block` calls until either
`mark_finalized` or `requeue_block` resolves it. This prevents
double-spending under proposer race conditions.

**Requeue is idempotent.** Calling `requeue_block(H)` after the entries
for block H have been finalized returns 0; calling it twice on the same
reverted block returns the count the first time and 0 the second.

---

## Integration with `spacekit-compute-node`

The integration adapter is similar to `UnifiedConsensusHost`. The shape:

```rust
// In spacekit-compute-node:
pub struct ComputeNodeMempool {
    inner: Arc<RwLock<Mempool<FeePerByteDescending>>>,
    /// Body resolver. Given a TxHash, return the full transaction body.
    body_store: Arc<dyn TransactionBodyStore>,
}

impl ComputeNodeMempool {
    /// HTTP and P2P ingress.
    pub async fn submit(&self, tx: SignedTransaction) -> Result<()> {
        // 1. Verify signature (NOT done by mempool — it assumes verified)
        // 2. Persist body to body_store
        // 3. Convert to TransactionRef
        // 4. submit() to inner mempool
    }

    /// Called from propose path.
    pub async fn build_block_body(&self, max_count: usize, max_bytes: u64)
        -> Result<Vec<SignedTransaction>>
    {
        let hashes = self.inner.read().await.drain_for_block(max_count, max_bytes);
        // Resolve hashes to bodies via body_store
        // Mark in_flight only AFTER the block proposal goes out on the wire
    }

    /// Called from fraud-proof acceptance handler.
    pub async fn handle_rollback(&self, block_height: u64) -> u64 {
        self.inner.write().await.requeue_block(block_height)
    }
}
```

No changes to `Mempool`'s own API are required. The adapter is the seam
where the integration sits, exactly mirroring the `CoordinatorHandle`
adapter pattern.

---

## On encrypted mempools

The `VisibilityStrategy` trait (feature `visibility`) is a hook for
future integration with encrypted-mempool primitives. **This crate does
not implement any encryption**, and explicitly does not commit to any
specific encryption scheme.

If/when SpaceKit integrates an encrypted mempool, the recommended path is:

1. The encryption primitive ships as a separate crate
   (`spacekit-threshold-encryption`, `spacekit-commit-reveal`, etc.)
2. That crate implements `VisibilityStrategy`
3. The integration adapter in `spacekit-compute-node` selects between
   strategies based on configuration

This matches established practice from teams like Shutter Network
(threshold encryption) and Flashbots (commit-reveal). It does **not**
require novel cryptography in this crate.

The trait surface is intentionally narrow (two methods,
`entry_visible_to` and `body_visible_to`) so the actual cryptographic
work stays in dedicated crates that can be audited independently.

---

## What this crate does NOT include

- **Transaction body storage.** The mempool indexes references; bodies
  live elsewhere.
- **Signature verification.** Callers verify signatures before
  submitting.
- **Fee market dynamics.** Pricing, fee bumping, and replacement-by-fee
  policies are outside scope. The `PriorityStrategy` trait gives the
  integration layer a place to implement them.
- **P2P transaction gossip.** Ingress and observation APIs are
  here; the gossip transport is a separate concern.
- **Novel cryptography.** The visibility hook exists for future
  encrypted-mempool work; encryption primitives ship as separate crates.

---

## Build

```bash
cargo build --release
cargo test
cargo test --features visibility  # exercise the optional trait
```

---

Made with care by the SpaceKit.xyz team.
