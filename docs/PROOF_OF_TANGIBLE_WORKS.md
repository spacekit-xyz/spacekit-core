# Proof of Tangible Works (PoTW) & the Treasury contract — design advice

Advice for two related asks: (1) a **Treasury WASM contract** that holds funds
for the project's future, and (2) **Proof of Tangible Works** — reviewers reach
consensus that a work is acceptable and an account is awarded ASTRA, where *the
system, not the treasury, awards it.*

This is grounded in the existing tokenomics (`SPACEKIT-WHITEPAPER.md` §9,
`spacekit-service-rewards`, and the native rewards described in
[`ASTRA_LEDGER.md`](../infra/spacekit-compute-node/ASTRA_LEDGER.md)).

> **Current status (2026-10).** PoTW is a design concept, not a live reward.
> On a consensus (PoA/PoS) network the node **refuses PoTW awards**: ASTRA is
> created only inside blocks by the native rewards system calls to `0x…0003`
> (INIT / CREDIT / CREDIT_LOCKED / END_POA), and there is no AstraRewards
> contract ledger any more. The treasury is the on-chain contract `0x…0004`,
> holding native ASTRA and paying M-of-N approved spends itself; disbursement
> through the host bridge is also refused on consensus networks. An award is an
> amount of ASTRA, the network's utility token; it carries no USD value and
> this document makes no claim about its price or value. Where this document
> says "native rewards `CREDIT`", it means a rewards system call placed in a
> block. See [`PAYMENTS_AND_CURRENCY.md`](PAYMENTS_AND_CURRENCY.md).

---

## 1. The one decision that shapes everything: mint vs. spend

Your model already has **two distinct value flows**, and PoTW must pick one:

| Flow | What it is | Source | Cap effect | Contract |
|---|---|---|---|---|
| **Emission (mint)** | Protocol creates *new* ASTRA for measured contribution | Native rewards `CREDIT` (system calls to `0x…0003`, executed by the node) | Counts toward 2B cap; cap-enforced | `native_rewards.rs`, driven by `SRA` |
| **Treasury (spend)** | Moves *already-minted* ASTRA from the 350M genesis pool | Genesis treasury balance | No new mint (already counted) | Treasury contract `0x…0004` (M-of-N) |

Your phrase — *"the system, not the treasury, awards"* — is the correct choice,
and it maps to **emission**. PoTW should **mint** the award via a native rewards `CREDIT`,
not transfer it out of the treasury. Reasons:

- The **treasury is finite (350M) and cannot be expanded** (§9.8). Paying
  per-work rewards from it would drain a pool earmarked for dev/audits/grants
  and conflate two purposes.
- Emission is *literally the mechanism the protocol already uses to reward
  measured contribution* (§9.5–9.7). PoTW is simply a **new contribution
  category** — human-reviewed work products — alongside the machine-measured
  ones (consensus, compute, storage, messaging).
- Minting through native rewards **inherits the 2B cap enforcement for free**
  (§9.9): every award is a `CREDIT` that is rejected in full if it would
  breach the cap. No new trust surface for the supply ceiling.

So: **PoTW = a new emission category that mints via native rewards `CREDIT`. The Treasury
contract is separate and is not involved in awards.**

---

## 2. The hard part: human consensus inside a deterministic chain

The existing emission categories are **machine-measured** — gas served,
bytes-hours, deliveries. They are objective, Sybil-resistant, and
**deterministic**, so every validator computes the same reward and agrees. PoTW
is **human-reviewed**, i.e. subjective. Dropping subjective judgment into a
deterministic mint is the whole design challenge, and it has one clean answer:

> **Keep the *review* off-chain; put only a deterministic *approval receipt*
> on-chain.** The contract never "runs a vote." It verifies an artifact that
> every validator can check identically: **M-of-N reviewer signatures** over
> `(work_id, recipient, amount)`.

Concretely, a PoTW award is authorized by an on-chain object like:

```
PoTWApproval {
  work_id:        unique id (content hash of the submitted work)
  recipient:      20-byte address
  amount:         uASTRA, ≤ per-work cap
  reviewer_set:   epoch's registered reviewer roster id
  approvals:      [ { reviewer_pubkey, signature } ]   // ≥ threshold M-of-N
  epoch:          which PoTW budget epoch this draws from
}
```

The PoTW contract verifies, **deterministically**:
1. each `signature` is valid over `sha256(work_id ‖ recipient ‖ amount ‖ epoch)`,
   using **SLH-DSA** (same PQ signatures as the rest of the stack — reuse
   `verify_slh_dsa_signature`);
2. the signers are in the epoch's registered reviewer set and distinct;
3. the count ≥ threshold **M-of-N**;
4. `work_id` has not been awarded before (replay guard);
5. the award fits the **PoTW epoch budget** (see §4);

then a `CREDIT(recipient, amount)` rewards system call is placed in the block. Validators re-run
exactly this and agree. The messy human part — reading the work, debating,
reaching consensus — happens off-chain and is compressed into the signatures.

This mirrors patterns you already have: M-of-N/threshold like the rollup
operator gate, PQ signatures like settlement, and a replay guard by id like
`/rollup/submit`.

---

## 3. The Treasury WASM contract — what it should and shouldn't do

A Treasury contract is a real upgrade over a bare multi-sig, **as long as it
stays on the spend side of §1**:

**Should:**
- Hold the genesis treasury balance (or a portion) and **spend only what it
  holds** — grants, audits, bounties, ops — via `OP_TRANSFER`.
- Enforce spending **rules on-chain**: M-of-N authorization, per-category
  sub-budgets, rate limits / vesting, and an **auditable spend log** event per
  disbursement. This turns opaque multi-sig transfers into transparent,
  rule-bound spending — a governance and credibility win.
- Optionally hold a **PoTW reviewer-stake escrow** (see §5) so reviewer bonds
  and slashing are visible on-chain.

**Must NOT:**
- **Have mint authority.** Only native rewards (in-block system calls) mint, cap-enforced. The Treasury
  contract can never create ASTRA — it only moves its own balance. This
  separation is what lets you say "the system, not the treasury, awards": the
  treasury literally *cannot* be the award source.
- Fund PoTW awards. Awards mint; the treasury is for discretionary grants that
  are a *different* decision with legal review (§9.8).

Keep them two contracts with two authorities: Treasury (governance/M-of-N,
spend-only) and PoTW→native rewards (reviewer-consensus, mint-only, budgeted).

---

## 4. Fitting PoTW into the cap and the emission budget

PoTW mints count against the **2B cap** via native rewards no matter what, but
you must also give it a **bounded periodic budget** or it becomes an inflation
hole (unbounded human-approved minting). Two governance-clean options:

- **Add a 5th category.** Rebalance the shares (still 5–60% each, sum 100%, per
  §9.10) — e.g. consensus 35 / compute 25 / storage 15 / messaging 10 / **PoTW
  15**. PoTW then gets a per-epoch budget with the same **no-overshoot** rule as
  the others (§9.6): high approval volume dilutes per-work reward rather than
  breaching the budget.
- **Draw from the reserve headroom** (~496M, §9.8) via an on-chain governance
  allocation dedicated to an ecosystem/works program with disclosure.

Either way: a fixed per-epoch PoTW allocation, per-work caps, and the
"idle epochs roll over, no epoch overshoots" discipline from §9.6. This keeps
PoTW on the same rails as the rest of emission and inside the halving envelope.

---

## 5. Economic & security guardrails (do not skip these)

Reviewers who can trigger minting are a lucrative attack target. The subjective
nature of PoTW makes capture the central risk. Mitigations, reusing machinery
you already have:

- **Bounded budget + per-work caps** (§4) — caps the damage of any capture.
- **Reviewer staking + slashing.** Reviewers post an ASTRA bond; approvals enter
  a **challenge window** (you already built this pattern for rollup bundles).
  A fraud proof against a bad approval slashes the approving reviewers' bonds.
- **Reputation weighting.** Weight reviewer influence by the consensus layer's
  existing reputation (`ReputationWeightedConsensus`), so new/low-rep accounts
  can't swing approvals.
- **Rotation & set registration.** Reviewer rosters are epoch-scoped and
  registered on-chain; rotate membership; require diversity.
- **Conflict-of-interest rules.** `recipient` must not be an approving reviewer;
  disclose reviewer↔author relationships; all approvals are public.
- **Transparency.** Every `PoTWApproval` and every treasury spend emits a log
  event; the whole thing is auditable after the fact.

State this honestly in the whitepaper the way §9.12 already states limitations:
PoTW introduces a **human trust assumption** the machine-measured categories do
not have, and its integrity rests on the reviewer-set honesty-majority plus
staking/slashing — not on cryptography alone.

---

## 6. Where it plugs into the existing monorepo

| Piece | Reuse / add |
|---|---|
| Mint + cap | **reuse** the native rewards `CREDIT` system call (`native_rewards.rs`; the AstraRewards contract ledger no longer exists) |
| Emission budget | **extend** `SRA` / category config to add the PoTW category (§4) |
| Reviewer signatures | **reuse** `verify_slh_dsa_signature` (SLH-DSA, `spacekit-primitives`) |
| Replay / challenge / slashing | **reuse** the rollup patterns (`rollup_registry`, challenge window, `SlashRecord`) |
| Reputation weighting | **reuse** `ReputationWeightedConsensus` (unified consensus) |
| Treasury contract | **new** WASM contract, spend-only, M-of-N, no mint authority |
| PoTW contract | **new** WASM contract, verifies approvals → `OP_CREDIT`, budgeted |

---

## 7. Recommendation, in one paragraph

Build **two** contracts. A **Treasury** contract that holds and spends the finite
350M under governance rules, with **no mint authority** — this is your
"future of the project" reserve, and keeping it mint-less is exactly what lets the
system (not the treasury) be the award source. And a **PoTW** contract that
verifies an on-chain **M-of-N SLH-DSA reviewer-approval receipt**, enforces a
**bounded per-epoch budget** as a new emission category, and mints the award via
a cap-enforced native rewards `CREDIT` in a block. Off-chain review, on-chain receipt;
minted not spent; budgeted not unbounded; staked-and-slashable reviewers, not
blind trust. That is coherent with §9 end to end and defensible to a technical
reader — which, given the crypto-claims scrutiny noted in
`CRYPTO_PRIMITIVES_BY_LAYER.md`, matters as much as the code.

---

## 8. As-built (2026-08) — what actually shipped, and one deliberate divergence

Sections 1–7 are the design advice. This section records the implementation as
of 2026-08. It has since changed (see "Since 2026-08" at the end of this
section).

### PoTW

- **Pure verifier + bookkeeper:** `infra/spacekit-compute-node/src/potw.rs`
  (`PoTWAccumulator`, `PoTWReceipt`, `PoTWConfig`, `award_digest`,
  `verify_and_award`). Award digest is
  `SHA-256("SPACEKIT-POTW-AWARD-v1\n" ‖ work_id(32) ‖ recipient(20) ‖ amount_le16 ‖ epoch_le8)`;
  reviewers sign the digest with **SLH-DSA-SHA2-128s** (FIPS-205), verified by
  the same `verify_slh_dsa_signature` the settlement layer uses. Pinned to a real
  browser (`wasm-did`) determinism vector.
- **Guardrails enforced, exactly as §2/§4/§5 prescribe:** allow-listed reviewers,
  distinct M-of-N quorum, per-work cap, per-epoch budget, and a permanent
  `work_id` replay guard. Budget/replay state is persisted with an atomic write.
- **Host + route:** `potw_host.rs` holds the accumulator; `/potw/award` (open —
  the reviewer quorum is the authorization, mirroring `/rollup/submit`);
  configured under `[compute.potw_config]` (`enabled`, `reviewers`, `threshold`,
  `epoch_budget`, `per_work_cap`, `state_path`). Fail-closed: disabled or
  `threshold = 0` awards nothing.

**Deliberate divergence from §1 (mint via `AstraRewards`).** As built, a PoTW
award **credits the recipient's native, spendable balance directly** (the same
uASTRA ledger the faucet credits, settlement moves, and the wallet displays) —
*not* the `AstraRewards` contract's internal per-DID wei ledger. Reason: on this
node the spendable balance the wallet reads and settlement spends is the native
account balance; crediting `AstraRewards`' internal ledger would not be spendable
without a further withdraw bridge, and the two ledgers use different units
(uASTRA vs wei). Emission discipline is therefore enforced by PoTW's own
**bounded per-epoch budget + per-work cap + replay** (set `epoch_budget` to the
protocol's per-epoch emission schedule so awards can never outrun supply), rather
than by the `AstraRewards` cap. A future refinement can additionally record each
award into `AstraRewards` for unified cap accounting; the native credit is what
makes the award immediately usable.

### Treasury

- **Contract** (`sdks/spacekit-standard-library/treasury`): M-of-N governed
  disbursement of a **pre-funded** pool, **no mint authority** — exactly §3.
  Opcodes INIT / PROPOSE / APPROVE / DEPOSIT / GET_BALANCE / GET_PROPOSAL /
  GET_CONFIG. Recipients are **20-byte native addresses** so a disbursement maps
  directly to a spendable transfer. On reaching threshold it debits its internal
  balance and emits `treasury.disbursed(spend_id, recipient, amount, memo)`.
  Deployed as a system contract at `0x…04`.
- **Bridge** (`treasury_host.rs`, route `/treasury/disburse`, operator-gated):
  reads the contract's own `GET_PROPOSAL` and acts only when `executed == 1`,
  then mirrors the disbursement to the native ledger **once** (atomic,
  persisted spend-id replay guard), moving funds FROM the configured custodian
  (default: the treasury contract address, which must be native-funded to match
  the balance the contract is INIT'd with). Configured under
  `[compute.treasury_config]`.

The Treasury still **cannot mint** and PoTW does **not** draw from it — the §1/§3
separation holds. The only change from the advice is that both land value on the
native spendable ledger rather than the `AstraRewards` wei ledger.

### Settlement finality (Phase B)

- `account_root()` (all account balance/nonce/code) is pinned by a determinism
  vector test; the operator-sequencer must reproduce it and stamp it into a
  bundle's `quantum_state_roots` to settle `Verified`. `/rollup/account-root`
  exposes the current root read-only so a sequencer can self-check.
- `SPACEKIT_ROLLUP_STRICT_ACCOUNT_ROOT=1` flips a committed-root mismatch from
  optimistic `Pending` to **`Rejected`** (strict finality). Off by default so it
  is enabled only once a sequencer's root is proven against the vector.

### Since 2026-08

- **One ledger.** The AstraRewards contract ledger is gone. Rewards mint
  natively into account balances (in wei) by system calls to `0x…0003` that the
  SRA places in each block and every importer checks. The "divergence" above is
  therefore moot: there is only the native balance.
- **PoTW awards are refused on PoA/PoS networks.** `/potw/award` writes outside
  blocks, so a consensus network rejects it. For PoTW to award ASTRA there, it
  would have to become an emission category whose awards are placed as in-block
  rewards system calls (§4), not a host route.
- **Treasury.** The treasury contract `0x…0004` now holds native ASTRA and pays
  approved spends itself with `transfer_u128`; its 350M is minted to it by INIT.
  The `/treasury/disburse` host bridge is refused on consensus networks.
- **Faucet and rollup settlement** are refused on consensus networks too (the
  faucet works only on single-node development chains).
