# Post-quantum signature policy

Wire version: **`PQ_ENVELOPE_WIRE_VERSION = 2`**.

## Merkle for votes, Verkle for state

| Commitment | Structure | Used for |
|------------|-----------|----------|
| **`votes_merkle_root`** | Sorted binary Merkle over vote leaves | Ephemeral per-block set (`V` validators, target tens–low hundreds at maturity) |
| **`tx_root`** | Raw tx-batch digest (tagged once in `sphincs_signing_bytes`) | Transaction batch / L1 manifest binding |
| **`state_root`** | State Verkle root (tagged digest in envelope) | SwtchVM state |
| **`spacetime_tip_hash`** | `SpacetimeTransition::digest()` (tagged) | Rotor transition for this block |

**Votes are not in the Verkle tree** — they are rebuilt every block, bounded by `V`, and archived via the SPHINCS+ envelope. Paying SIS-VC binding cost on vote sets buys nothing.

**Fingerprints (long-lived, per-validator)** belong in **state Verkle** under [`FINGERPRINT_NAMESPACE`](fingerprint_verkle.rs) (`0xFF…FE`), keyed by validator DID hash (32 B). Wire format: [`FingerprintCommitment`](fingerprint_verkle.rs) (92 B). See [`FINGERPRINT_VERKLE.md`](FINGERPRINT_VERKLE.md). **Rotors-in-flight** belong in the **vote Merkle leaf** (see below).

### Light-client sync: which proof to ask for

| Question | Proof source |
|----------|----------------|
| “Was validator X’s vote + rotor in block *N*?” | Per-block **vote Merkle** inclusion (~⌈log₂ V⌉ × 32 B + leaf) |
| “What was validator X’s fingerprint at height *N*?” | **State Verkle** multiproof (SIS-VC binding) |
| “Sync *N* consecutive spacetime transitions?” | **Verkle** batched multiproofs (`verkle.rs` rotor keys) — not *N* separate vote trees |

At **V ≈ 1024**, vote Merkle depth ≈ 10 → ~320 B path + ~200 B leaf — still within typical light-client budgets.

## Domain separation (mandatory)

Never hash a raw 32-byte root into the SPHINCS+ preimage without a domain tag. Tags in [`pq_envelope.rs`](pq_envelope.rs):

| Constant | Bytes |
|----------|-------|
| `DOMAIN_VOTES_MERKLE` | `spacekit-votes-merkle-v1` |
| `DOMAIN_TX_VERKLE` | `spacekit-tx-verkle-v1` |
| `DOMAIN_STATE_VERKLE` | `spacekit-state-verkle-v1` |
| `DOMAIN_SPACETIME_TRANSITION` | `spacekit-spacetime-transition-v1` |
| `DOMAIN_VOTE_MERKLE_LEAF` | `spacekit-vote-merkle-leaf-v1` |
| `DOMAIN_CONSENSUS_VOTE` | `spacekit-consensus-vote-v1` |
| `DOMAIN_BLOCK_ENVELOPE` | `spacekit-block-envelope-v1` |

Tagged commitment: `keccak256(domain || value32)`.

## Two-tier signatures

1. **Inner loop — Dilithium2** on [`ConsensusVoteInner`](pq_envelope.rs) (high frequency).
2. **Outer envelope — SPHINCS+-SHAKE-256-128s-simple** on [`BlockEnvelope::sphincs_signing_bytes`](pq_envelope.rs) (once per finalized block).

The finisher API enforces this structurally: [`attach_pq_finisher`](../pq_finisher.rs) requires [`PqFinisherQuorum`](../pq_finisher.rs) (Dilithium votes) **and** [`SphincsEnvelopeKey`](../pq_finisher.rs).

## Dilithium vote signing bytes

```
DOMAIN_CONSENSUS_VOTE
|| wire_version (u16 LE)
|| round (u64 LE)
|| view (u64 LE)
|| proposal_hash (32)
|| vote_type (u8)
|| validator_id (32)
|| validator_rotor_digest (32)
```

`proposal_hash` binds the block body (including spacetime transition digest when present) via `spacetime_integration::block_proposal_hash`.

## Vote Merkle leaf (rotor folded in)

Leaf preimage (before `DOMAIN_VOTE_MERKLE_LEAF`):

```
round (u64 BE) || view (u64 BE)
|| validator_id (32)
|| vote_type (u8)
|| validator_rotor_digest (32)    // TransitionWitness rotor binding
|| dilithium_sig_digest (32)      // keccak256(full Dilithium sig)
```

Leaf digest: `keccak256(DOMAIN_VOTE_MERKLE_LEAF || preimage)`.

`votes_merkle_root`: sort leaf digests, pairwise `keccak256(left||right)` (duplicate last on odd count).

## Spacetime transition in the block body

- Raw `SpacetimeTransition::to_bytes()` lives in the block next to `consensus_votes`.
- Envelope carries **`spacetime_tip_hash = transition.digest()`** only (same as `proposal.rs::digest` / `spacetime_integration::spacetime_transition_digest`).
- No extra signature or tree — covered transitively by SPHINCS+.

## SPHINCS+ outer envelope (canonical byte order)

Light clients and validators **must** implement exactly this order in [`BlockEnvelope::sphincs_signing_bytes`](pq_envelope.rs):

```
DOMAIN_BLOCK_ENVELOPE
|| wire_version (u16 LE)
|| round (u64 LE)
|| view (u64 LE)
|| parent_hash (32)
|| tagged(DOMAIN_VOTES_MERKLE, votes_merkle_root)
|| tagged(DOMAIN_TX_VERKLE, tx_root)
|| tagged(DOMAIN_STATE_VERKLE, state_root)
|| tagged(DOMAIN_SPACETIME_TRANSITION, spacetime_tip_hash)
|| timestamp (u64 LE)
|| chain_id_len (u32 LE) || chain_id (UTF-8)
|| height (u64 LE)
```

**Domain-tag rule (all envelope roots):** `votes_merkle_root`, `tx_root`, `state_root`, and `spacetime_tip_hash` are stored **raw** (32 bytes) on [`BlockEnvelope`](pq_envelope.rs). Each `DOMAIN_*` tag is applied **exactly once** inside `sphincs_signing_bytes`. The finisher must not pre-tag any root before storing it on the envelope — double-tagging breaks light-client reproduction and creates divergent hash conventions across code paths.

## Key hierarchy

| Key | Algorithm | Used for |
|-----|-----------|----------|
| DID identity | SPHINCS+ | Registry, **block envelope** (proposer/finisher) |
| Validator consensus | Dilithium2 | PREPARE / COMMIT / PQ votes |
| User session (optional) | Dilithium2 | High-frequency contract calls |

Register validator Dilithium keys with a one-time SPHINCS+-signed payload.

## Batch verification (`PqBatchVerifier`)

CPU and GPU paths share [`pq_finisher::gpu_batch::PqBatchVerifier`](../pq_finisher.rs). Callers pass batches; implementations choose chunk sizes (`optimal_dilithium_batch_size` / `optimal_sphincs_batch_size`).

- **Dilithium**: polynomial batching — expect large GPU wins (~50–100× vs CPU at scale).
- **SPHINCS+**: parallel **independent** verifies across SMs, not intra-signature arithmetic — expect ~5–10×, not Dilithium-class speedups.

Set `SPACEKIT_PQ_GPU_VERIFY=1` to use the **parallel batch path** (rayon across CPU cores for independent Dilithium/SPHINCS+ verifies). Native CUDA kernels can plug into the same `dilithium_batch_verify_gpu` hooks later.

## Compute-node integration

On [`BlockData`](../consensus.rs) (`spacetime-consensus` feature):

- `consensus_votes: Option<Vec<ConsensusVoteInner>>`
- `signed_block_envelope: Option<SignedBlockEnvelope>`
- `spacetime_transition: Option<SpacetimeTransition>`

Validated in [`validate_block_pq_envelope`](../spacetime_integration.rs).

### Finalization flow

1. Collect Dilithium-signed votes (each with `validator_rotor_digest`).
2. `votes_merkle_root = votes_merkle_root(&votes)`.
3. Build `BlockEnvelope` via `block_envelope_from_data(block, round, view, …)`.
4. Sign `SignedBlockEnvelope` with DID SPHINCS+ key.
5. Gossip envelope + vote bodies; verifiers batch Dilithium, then one SPHINCS+.

### Dev-only auto-finalize

`POST /v1/consensus/propose` with `"finalize": true` runs the finisher only when:

- `network.dev_mode == true`, **or**
- `network.allow_single_validator_finalize == true` **and** `validator_count() <= 1`.

Otherwise production deploys cannot silently bypass quorum.

### Operator / CLI path

Prefer **`POST /v1/consensus/propose`** on the standalone compute node (`SPACEKIT_COMPUTE_URL`) so CLI and production share one consensus path. The `spacekit` CLI uses HTTP by default; pass `--in-process` only for local engine tests.

## Rust API (`pq-signatures` feature)

```rust
use spacekit_spacetime_consensus::pq_envelope::pq_crypto;

let (d_pk, d_sk) = pq_crypto::dilithium2_keypair();
pq_crypto::sign_consensus_vote(&mut vote, &d_pk, &d_sk);
let signed = pq_crypto::sign_block_envelope(envelope, &s_pk, &s_sk);
```

```rust
verify_quorum_against_envelope(&signed.envelope, &votes)?;
signed.verify();
```
