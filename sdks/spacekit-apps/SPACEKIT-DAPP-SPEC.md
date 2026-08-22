# The SpaceKit dApp Specification

**Version 0.2: Draft.** Extends *The Spacekit Specification v0.2* and supersedes *dApp Specification v0.1*.

v0.2 implements everything v0.1 deferred. The throughline: v0.1 assumed contracts are immutable, single-chain, EOA-signed, and instantly final. Each is false in production, and each falsehood gets a construct here.

---

## What's new in v0.2


| Resolves (v0.1 §10)                                       | Section |
| --------------------------------------------------------- | ------- |
| Proxy/upgrade handling and ABI versioning                 | §1      |
| Reorg handling and indexer finality                       | §2      |
| Account abstraction (ERC-4337) and sponsored transactions | §3      |
| Gas/fee strategy                                          | §4      |
| Cross-chain workflows                                     | §5      |
| On-chain access-control introspection                     | §6      |


The v0.1 governing rule is unchanged: **the ABI is authoritative; Spacekit imports it, never supersedes it.**

---



## 1. Upgradeable contracts and ABI versioning

A contract address can outlive its ABI. Proxies keep a stable address while the implementation (and its ABI) changes, so `contracts` gains a `proxy` and an `abiHistory`.

```yaml
contracts:
  Bookshop:
    proxy:
      kind: uups                          # transparent | uups | beacon | diamond
      address: { sepolia: "0xProxy…", mainnet: "0xProxy…" }   # stable across upgrades
    abi: ./abi/Bookshop.v2.json           # current ABI
    abiHistory:                           # ABIs by activation block, for decoding old events
      - { version: 1, abi: ./abi/Bookshop.v1.json, fromBlock: { mainnet: 19000000 } }
      - { version: 2, abi: ./abi/Bookshop.v2.json, fromBlock: { mainnet: 19500000 } }
```

Rules:

- The **proxy address is the call/index target**; the implementation address is never referenced directly.
- The **indexer decodes each event with the ABI version active at that block** (from `abiHistory`), so a single `Book` entity can be built from events emitted across upgrades.
- `kind: diamond` (EIP-2535) means several facets share one address; declare `facets: [ ... ]`, each with its own ABI, and `@abi.Bookshop.<fn>` resolves across facets.
- A reference to an ABI member must exist in the **current** ABI; references used only for historical indexing are declared under `abiHistory`.

This also makes ABI changes subject to the base spec's compatibility rules: adding a function is compatible; removing or changing the signature of one the spec references is breaking.

---



## 2. Finality and reorg handling

v0.1 treated an indexed record as instantly true. A chain can reorganize, so indexed entities now carry a **finality lifecycle** and the indexer declares how it rolls back.

```yaml
indexer:
  source: Bookshop
  events: [BookListed, Restocked, BookPurchased]
  finality:
    confirmations: { sepolia: 12, mainnet: 32 }   # blocks before a record is `final`
    reorgStrategy: rollback                        # rollback | refetch
```

Every indexed entity gains a reserved, read-only `_status` of `pending` (indexed, below the confirmation threshold) or `final`. Views and policies may read it:

```yaml
views:
  OrderConfirmation:
    data: { purchase: { from: @Purchase, where: "id == params.purchaseId" } }
    states:
      pending: { when: "purchase._status == 'pending'", show: @AwaitingFinality }
```

Workflows triggered by on-chain events may require finality before firing, which closes the v0.1 indexing-lag gap (a confirmed-but-not-final purchase no longer triggers downstream work prematurely):

```yaml
workflows:
  Checkout:
    on: { event: @abi.Bookshop.BookPurchased, confirmations: 32 }   # wait for finality
```

`reorgStrategy: rollback` reverts records derived from orphaned blocks and re-derives them; the `_status` transition from `pending` to `final` is the only point at which downstream effects are safe to treat as permanent.

---



## 3. Account abstraction and sponsored transactions

`signer` becomes a mode, not just an address. ERC-4337 smart accounts allow sponsorship (gasless UX) and native call batching.

```yaml
capabilities:
  BuyBook:
    kind: transaction
    call: @abi.Bookshop.buyBook
    signer:
      mode: smartAccount            # eoa (default) | smartAccount
      account: connectedWallet
      paymaster: sponsored          # none (default) | sponsored | @paymasters.Bookshop
    input: { bookId: { type: tokenId }, quantity: { type: integer } }
    with:  { bookId: input.bookId, quantity: input.quantity }
    emits:  [@abi.Bookshop.BookPurchased, @abi.Receipt.Transfer]
    errors: [@abi.Bookshop.OutOfStock, @abi.Bookshop.InsufficientAllowance]
```

Consequence for §7 of v0.1 (the on-chain `bundle`): under `mode: smartAccount`, a `bundle` realizes as **one UserOperation containing batched calls** — natively atomic, no permit/multicall trick required. The atomic-bundle-replaces-compensation rule is unchanged; AA just gives a cleaner realization of it. With `paymaster: sponsored`, the buyer needs no native gas token at all, removing the most common dApp onboarding wall.

---



## 4. Gas and fee strategy

Fee *policy* is a profile concern, but the spec carries the *intent* a transaction needs.

```yaml
capabilities:
  BuyBook:
    fees:
      priority: standard            # standard | fast | custom — realized by the profile's fee oracle
      sponsored: true               # shorthand consistent with signer.paymaster
      cap: { type: money }          # optional user-facing max fee
```

The view layer gains a corresponding state so users see fee conditions before signing:

```yaml
views:
  Checkout:
    states:
      feeTooHigh: { when: "tx.estimatedFee > BuyBook.fees.cap", show: @FeeWarning }
```

The profile (§7) picks the fee oracle and the bundler/paymaster endpoints; the spec never hardcodes gas numbers.

---



## 5. Cross-chain workflows

v0.1 split steps into on-chain (atomic bundle) and off-chain (compensable). Cross-chain adds a third regime, because **there is no atomicity across chains** and a confirmed cross-chain step cannot be compensated. Cross-chain flows are therefore explicit state machines with **forward recovery**, gated on bridge/message confirmation.

```yaml
workflows:
  CrossChainBuy:
    summary: Pay on an L2, fulfill on L1; sequential, gated on the bridge.
    steps:
      - do: PayOnArbitrum
        chain: onchain
        network: arbitrum
      - do: BuyBookOnMainnet
        chain: onchain
        network: mainnet
        awaits: bridged              # blocks until the cross-chain message is confirmed
        onTimeout: -> RecoverPayment # forward recovery; NOT a compensation/undo
      - do: SendReceiptEmail
        chain: offchain
        compensate: VoidReceipt
```

Rules the spec enforces:

- A step gains an optional `network`; steps in different networks **cannot** share a `bundle`.
- A cross-chain step may **not** declare `compensate` (no undo across a confirmed bridge); it may declare `onTimeout` / `onFailure` pointing at a **forward-recovery** capability.
- `awaits: bridged` makes the step wait on a bridge/messaging confirmation; the realized bridge or messaging layer is a profile choice.

So the full regime is now three-way: **off-chain → saga compensation; same-chain on-chain → atomic bundle; cross-chain → state machine with forward recovery.**

---



## 6. On-chain access-control policies

v0.1 limited `policy` to wallet connection. v0.2 lets a policy **read on-chain access control** so the UI can mirror what the contract enforces.

```yaml
policies:
  walletConnected: "actor.wallet != null"
  isSeller:        "actor.wallet == @abi.Bookshop.owner()"                 # reads contract owner
  isAdmin:         "@abi.Bookshop.hasRole('ADMIN_ROLE', actor.wallet)"      # OZ AccessControl
```

These resolve as on-chain view reads against the imported ABI.

> **Security caveat (stated, not buried).** A client-side policy is **advisory — for UX only**. The contract remains the sole enforcer of on-chain authorization. `isSeller` gating a "List a book" button is a convenience; it does not and cannot secure `listBook`, which the contract must guard itself. Spacekit mirrors on-chain access control; it never replaces it.

---



## 7. Realization (dApp profile additions)

The new constructs stay technology-free; the dApp profile chooses the tech:

```yaml
chain:
  client: viem
  indexer:
    engine: thegraph              # thegraph | ponder | custom
    finality: follow-config       # honor spec confirmations; engine-specific reorg handling
  wallet:
    connector: wagmi-rainbowkit
  accountAbstraction:
    bundler: pimlico              # pimlico | alchemy | stackup | none
    paymaster: pimlico-sponsor    # realizes signer.paymaster: sponsored
  fees:
    oracle: blocknative           # realizes capability.fees.priority
  bridge:
    provider: across              # realizes workflow `awaits: bridged` | across | hyperlane | layerzero
  workflows:
    bundling: erc4337-batch       # erc4337-batch | permit-multicall | multicall3 | state-machine
```

The §1-style invariant holds across all of these: swapping `bundler`, `engine`, or `bridge` provider changes realization, never the saga/atomic/forward-recovery semantics or the finality thresholds — checkable by `spacekit-cli conform`.

---



## 8. Changes from v0.1

- Added `contracts.proxy` and `contracts.abiHistory` for upgradeable contracts and version-aware indexing (§1).
- Added `indexer.finality`, the reserved `_status` lifecycle on indexed entities, and `confirmations` gating on event-triggered workflows (§2).
- Promoted `signer` to a mode supporting ERC-4337 smart accounts and sponsored (gasless) transactions; `bundle` may realize as a batched UserOperation (§3).
- Added `capability.fees` intent and a `feeTooHigh` view state (§4).
- Added cross-chain workflow steps (`network`, `awaits`, forward-recovery `onTimeout`) and the three-way execution regime (§5).
- Extended `policy` to read on-chain access control, with an explicit advisory-only security caveat (§6).
- Added the matching dApp profile keys (§7).



## 9. Deferred to v0.3 (dApp)

- Intent-based / solver execution (describe the desired end state, let a solver route it).
- Private-state and ZK flows (commitments, proofs as first-class capability outputs).
- Multi-sig / timelock governance as a `signer` mode.
- Indexer backfill cost and historical-range strategy as profile concerns.
- L2-specific finality nuances (soft vs hard confirmations, forced inclusion).

