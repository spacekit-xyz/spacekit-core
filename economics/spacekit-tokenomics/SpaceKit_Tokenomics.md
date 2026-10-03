# SpaceKit Tokenomics v2.1

**Status:** Canonical technical specification
**Version:** 2.1: adds the proof-of-authority bootstrap phase and its operator reward rules (§1.5, §1.7, §1.10, §1.11)
**Date:** 2026
**Owner:** SWTCH Labs
**Location:** [`spacekit-tokenomics/`](./) (canonical). **Supersedes:** [`archive/SpaceKit_Tokenomics_v1.md`](./archive/SpaceKit_Tokenomics_v1.md) (April 2026, aUSD-era)

This document is the authoritative specification of SpaceKit's economic primitives. It describes one primitive, ASTRA, the only currency on the SpaceKit network: how it is created, how it is used to pay for network resources and services, and how it secures and governs the network.

This document supersedes v1.0 entirely. The v1.0 spec described a dual-token model (ASTRA + aUSD) with an aUSD-driven burn flywheel. aUSD is no longer a SpaceKit product. Earlier v2 drafts also specified SpaceKit Pay (stablecoin payment routing) and x402; SpaceKit Pay was retired, and payments are now ASTRA transfers on the SpaceKit chain (see [`PAYMENTS_AND_CURRENCY.md`](../../docs/PAYMENTS_AND_CURRENCY.md)). ASTRA's economic model has been updated to match a fixed-supply utility token earned through service provision.

The decisions in this spec are locked. The supporting rationale is in the ASTRA Economic Model Decision Memo (internal, available to authorized parties on request).

## Part 1 — ASTRA

### 1.1 Overview

ASTRA is SpaceKit's native L1 utility token. It is used to pay for protocol-level network resources (compute, storage, messaging), to stake for validator participation in consensus, and to participate in on-chain governance.

ASTRA is earned exclusively by operators who provide measured network service. There is no public sale, no airdrop, no investment offering of ASTRA. Operators who run nodes that contribute to the network earn ASTRA proportional to their measured contribution.

ASTRA has a hard supply cap of 2,000,000,000 (two billion) tokens, enforced at the protocol level. The cap cannot be exceeded. No inflation mechanism increases supply beyond the cap. No automatic burn mechanism tied to fee volume decreases supply; the only burn is the gas each transaction uses (§1.6).

### 1.2 Supply parameters

| Parameter | Value |
|-----------|-------|
| Total supply | 2,000,000,000 ASTRA (hard cap) |
| Decimals | 18 |
| Atomic unit | 1 wei-ASTRA = 10⁻¹⁸ ASTRA |
| Inflation | None |
| Burn | Used gas only (no burn tied to fee volume) |
| Mint authority | Protocol only, capped at total supply |
| Standard | Native (L1) account balances, keyed by address (DID `did:spacekit:<address>`) |

### 1.3 Emission

ASTRA is emitted to operators through the **Service Reward Accumulator (SRA)** — a protocol-level function that reads structured service logs, computes rewards per the emission schedule, and places **CREDIT** system calls to the rewards address `0x…0003` at the start of a block. The node executes these calls natively (`native_rewards.rs`) and mints each reward straight into the recipient's account balance. See **[`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](./SERVICE_REWARD_ACCUMULATOR_SPEC.md)** and **[`ASTRA_REWARDS_CONTRACT_SPEC.md`](./ASTRA_REWARDS_CONTRACT_SPEC.md)**.

Emission follows a **4-year halving curve** (Bitcoin-style, adapted for continuous service):

- **Initial annual emission (year 1):** 200,000,000 ASTRA (10% of the 2B cap).
- **Decay:** Annual emission at year `t` is `200M × 0.5^(t/4)`; asymptotic cumulative operator emission ≈ **1.15B ASTRA**.
- **Hard cap:** `rewards.total_emitted` in the node's native rewards state cannot exceed 2B; the node rejects any credit that would exceed it.

Per-category shares of annual emission (default): **consensus 40%**, **compute 30%**, **storage 20%**, **messaging 10%**. Within each category, rewards are allocated **per epoch** (default: one day) proportional to measured resource units.

Full tables, governance bounds, and on-chain formulas: **[`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md)**.

### 1.4 Earning model — four service categories

Operators earn ASTRA proportional to measured service contribution in four categories:

**Consensus validation.** Validator nodes participating in consensus earn ASTRA for honest validation activity. The protocol measures:
- Block proposals successfully accepted
- Votes cast on correct proposals
- Block envelope signatures contributed
- Uptime during assigned slots

Misbehavior (double-signing, prolonged unavailability, censorship of valid transactions) results in slashing of the validator's stake.

**Compute service.** Compute nodes executing smart contract calls earn ASTRA proportional to the gas units they serve. The protocol measures:
- Gas units consumed by contract executions the node served
- Verification of correct execution by sampling and cross-validation
- Response time within configured thresholds

Compute that fails verification (incorrect execution, response timeouts) does not earn ASTRA. Repeated failures may result in operator deregistration.

**Storage service.** Storage nodes maintaining content-addressed blobs, FactPackage graphs, and DID-scoped documents earn ASTRA proportional to:
- Durable storage capacity provided (measured continuously)
- Successful read operations served
- Successful write operations served
- Storage proof attestations submitted on time

Storage that fails durability proofs (loss of stored data when proofs are challenged) does not earn ASTRA for the affected storage.

**Messaging service.** Messaging nodes delivering quantum-resistant encrypted messages earn ASTRA proportional to:
- Messages successfully delivered to recipients
- Recipients served with the node's resolved encryption keys
- Group message broadcast operations completed

Messages that fail delivery (recipient unreachable, encryption errors) do not earn ASTRA for the failed delivery.

### 1.5 Validator staking

Validators must lock ASTRA as a security deposit to participate in consensus. The stake serves two purposes:

**Skin in the game.** A validator with locked ASTRA has direct economic exposure to network safety. Misbehavior triggers slashing — partial or full forfeiture of the staked ASTRA — proportional to the severity of the misbehavior.

**Sybil resistance.** Without a stake requirement, an adversary could spin up many validator identities cheaply. Requiring a stake makes Sybil attacks expensive.

**The stake itself does not earn yield.** This is a deliberate design choice and an important distinction. Validators earn ASTRA through the service they provide while staked (proposing, voting, signing) — not through holding the stake passively. A validator who locks ASTRA but provides no service earns nothing. A validator who provides service without staking cannot participate in consensus.

This separates the "access right" (the stake) from the "earning mechanism" (the service). The stake is the price of admission to earn through work, not an investment instrument that pays interest.

Slashing parameters (minimum stake, slashing fractions per misbehavior category, withdrawal delay) are set by protocol governance. Initial parameters are documented separately in the validator operations guide.

**Exception: the proof-of-authority bootstrap.** The staking requirement applies from the end of the bootstrap phase. While the network has fewer than 10 validators, it runs in proof of authority: validators are admitted by vote of the existing authorities and validate without stake. See §1.11.

### 1.6 Network resource pricing

ASTRA is consumed when network resources are used:

**Gas for compute.** Smart contract executions are metered in gas. Gas is paid in ASTRA at a rate that floats based on network demand (similar to Ethereum's EIP-1559 base fee mechanism, though the specific mechanism is SpaceKit-native rather than Ethereum-compatible).

**Storage fees.** Operations that write to storage (deploying contracts, storing blobs, updating FactPackage graphs) consume ASTRA proportional to the bytes written and the durability period.

**Messaging fees.** Sending messages through the messaging layer consumes ASTRA proportional to the message size and the number of recipients.

**Identity operations.** DID registrations, credential issuance, and key rotation consume ASTRA proportional to the operation cost.

Used gas is burned. It is not paid to validators or retained in a central treasury. Operators are paid for serving the relevant requests (compute, storage, messaging, validation) through the service emission described above.

### 1.7 Governance

ASTRA holders participate in on-chain governance for protocol parameters. Governance scope includes:

- Emission schedule adjustments (within the 2B cap)
- Slashing parameters
- Gas pricing mechanism parameters
- Treasury fee rates and beneficiaries
- Protocol upgrade activation
- Reference extension activation (e.g., the spacetime consensus extension's activation status)

Governance votes are weighted by stake — operators who have ASTRA locked as validator stake have voting power proportional to their stake. ASTRA held but not staked does not vote (this prevents passive holders from outweighing active operators in protocol decisions).

The governance mechanism is described in detail in the governance specification (`SpaceKit_Governance.md`).

During the proof-of-authority bootstrap there is no stake to weight votes by. Only validator-set governance runs in that phase, with one vote per authority. It covers admitting authorities, removing them, and lifting PoA (§1.11). Protocol parameters (emission, category shares, slashing, fees) stay at their genesis values until stake-weighted governance begins at the end of the bootstrap. A security fix may be shipped before then as a coordinated protocol upgrade, announced publicly with its reasoning.

### 1.8 No yield products

SpaceKit does not offer (and the protocol does not implement) any of the following:

- ASTRA staking pools that pay yield denominated in ASTRA
- Lending mechanisms where ASTRA holders deposit ASTRA and receive interest
- Liquidity mining programs
- Liquidity seeding: neither SWTCH Labs nor the treasury supplies ASTRA to markets
- Stake lending or delegation from the treasury
- Inflation rewards for passive ASTRA holders
- Any other passive-yield instrument denominated in ASTRA

The only way to earn ASTRA is through measured service contribution. The protocol enforces this by having no mint paths other than the operator service reward emission described in Section 1.3.

Third-party DeFi protocols built on SpaceKit may exist and may create yield products denominated in ASTRA or other assets. These are not SWTCH Labs products and the SpaceKit protocol does not endorse or guarantee them. Operators and users interacting with third-party DeFi do so at their own risk.

### 1.9 No public sale

ASTRA is not sold to investors. There is no public sale, no pre-sale, no presale tiers, no airdrop, no initial coin offering, no investment offering of any kind.

SWTCH Labs has conducted equity-only capital raises. Investors in SWTCH Labs receive shares (or equivalent equity instruments) in the company. No portion of any equity raise is denominated in or settled with ASTRA. No investor is entitled to ASTRA tokens as part of their equity stake.

ASTRA may appear on secondary markets (exchanges, OTC trades) as a result of operators trading their earned ASTRA. SWTCH Labs does not control secondary-market activity, does not list ASTRA on exchanges, and does not endorse any specific exchange or secondary venue.

If SWTCH Labs at any future point considers any form of token distribution beyond the operator-earned emission described above (e.g., a grant program, an ecosystem development fund, etc.), such a distribution requires explicit legal review and a formal public disclosure. As of this specification, no such distribution is planned and the public position is unambiguously "no public sale, ever."

### 1.10 Treasury and bootstrap

**Genesis treasury: 350,000,000 ASTRA (17.5% of cap).** Minted to the on-chain treasury contract (`0x…0004`) in the first block by the rewards `INIT` system call. The treasury contract pays only spends approved by M of its N signers, which are set in the PoA genesis file. Used for protocol development, audits, operational reserves, and ecosystem grants (subject to legal review per Section 1.9). **Not subject to the halving curve** — it exists at genesis and decreases only when spent. **Cannot be expanded** beyond the genesis 350M allocation.

**No bootstrap pool, no lending, no liquidity seeding.** No ASTRA is set aside to stake for validators, lent or delegated to operators, or supplied to markets. In proof of authority, validators need no stake. After it ends, every validator stakes ASTRA it earned itself through service, locked or unlocked (§1.11). Markets for ASTRA form only from operators trading what they earned; neither SWTCH Labs nor the treasury seeds liquidity. The 50M that earlier drafts earmarked for bootstrap stake stays in the genesis treasury under §1.9's rules. See **[`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md)** §8.

**Protocol reserve:** ~496M ASTRA headroom under the 2B cap (cap minus asymptotic operator emission minus treasury) allocatable only by on-chain governance.

Operator emission and the treasury allocation are tracked by the node's native rewards state (`rewards.total_emitted`). ASTRA paid as gas for network resources is burned; it is separate from the SRA emission path.

### 1.11 Proof-of-authority bootstrap and operator rewards

A new SpaceKit network starts with a small set of **authorities**: operators named in the genesis file, run by SWTCH Labs and invited partners. Early on there is no earned ASTRA to stake, and a handful of validators cannot provide meaningful stake-based Sybil resistance. Admission by known operators is the honest security model for that stage.

**Phase rules.**

| | Proof of authority (bootstrap) | Proof of stake |
|---|---|---|
| Who validates | Authorities admitted by genesis or by governance vote | Operators who register the minimum stake |
| Stake | None required | Required, slashable |
| Validator-set changes | Signed proposals, one vote per authority, passes at two-thirds | Stake registration |
| Protocol-parameter governance | Frozen at genesis values | Stake-weighted (§1.7) |
| Service rewards | Full category emission, **locked** for authorities (below) | Full category emission, unlocked |

**Leaving proof of authority.** The authorities can vote to lift PoA only once there are at least **10 validators**. Nothing switches automatically: the lift takes a passed `lift_poa` proposal. After the lift, authorities have a **30-day grace period** to register stake from ASTRA they earned, including their locked balance. Authorities that do not stake stop validating. The mechanism is specified in the compute node's `GOVERNANCE.md`.

**Authority rewards are locked.** Authorities earn the normal emission for the service they provide (consensus, compute, storage, messaging) under §1.3–1.4. Nothing is capped or redirected. But ASTRA credited during the PoA phase to authority DIDs, or to any operator DID affiliated with SWTCH Labs or the SpaceKit Foundation, is locked:

- **Cliff:** nothing unlocks for 12 months from network genesis.
- **Linear vesting:** after the cliff, the locked balance unlocks linearly until month 36.
- **Stakeable while locked:** locked ASTRA can be registered as validator stake when PoS begins, and remains slashable. It cannot be transferred, sold or spent on gas until it unlocks.
- **Scope:** only PoA-phase credits are locked. ASTRA earned after the lift follows the normal rules. Independent operators who are not authorities and not affiliated earn unlocked rewards throughout.

**Why.** With three or four authorities sharing the whole consensus category, and often the other categories too, a few operators receive most of the early emission. For example, with 3 authorities running all four categories in the first three months, about 50M ASTRA would be split roughly 16.7M each. Locking those rewards means the operators who launched the network cannot sell into the first year. It also keeps their incentive tied to the network's long-term health, and puts their early earnings to work as stake rather than as liquid supply. It is not a sale or an allocation: authorities earn only for measured service, like every operator.

**Disclosure.** The current authorities, their operators, and their affiliation with SWTCH Labs are published on spacekit.xyz/governance and spacekit.xyz/economics. Governance proposals and votes are public and signed.

This section is subject to the Withers Worldwide review of ASTRA's regulatory status (see ASTRA.md).

## Part 2 — Paying in ASTRA

### 2.1 Overview

ASTRA is the only currency on the SpaceKit network. There are no USD-denominated balances, no stablecoin rails and no exchange rates. A payment is an ASTRA transfer on the SpaceKit chain, proven by its transaction hash. It moves only if the transaction succeeds.

SpaceKit Pay, the stablecoin payment router specified in earlier v2 drafts, was retired, together with its x402 rail. See [`PAYMENTS_AND_CURRENCY.md`](../../docs/PAYMENTS_AND_CURRENCY.md).

### 2.2 Paying a service

A service names its price in ASTRA. The client pays with a chain transaction and sends the transaction hash with its request (for example in an `X-PAYMENT` header). The service checks the payment with `GET /v1/tx/{hash}` or `POST /v1/payments/verify`, which confirms success, the payee and the amount, and refuses a hash it has already accepted. A service that grants something lasting stores the hash with the grant. The `spacekit-payments` library provides the verifier and pay-per-request middleware.

### 2.3 Apps, content and channels

Paid apps, content and channels are sold through the `astra-entitlement-ledger` contract. A purchase or renewal pays the listing price in ASTRA straight to the publisher's address; the ledger keeps nothing. Storage and messaging nodes check entitlements with read-only calls.

### 2.4 Contract calls and sponsorship

A paid contract call carries its fee as attached value, and the contract forwards it. A contract can pay only from its own balance, never from its caller's. Sponsors can deposit ASTRA in the `spacekit-paymaster` contract, which pays for permitted callers under a policy.

### 2.5 Relationship to emission

Payments do not mint ASTRA. An operator can earn in two ways: emission for the network service its node provides (Part 1), and ASTRA payments from buyers for services it sells. These don't double-count; they pay for two different things.

## Part 3 — What's removed in v2

For clarity, the following constructs from the v1.0 spec are no longer part of SpaceKit:

**aUSD.** The collateralized stablecoin previously specified in v1.0 is removed. Payments are made in ASTRA (Part 2). No vault contracts, no mint engine, no aUSD-denominated fees, no aUSD-driven burn flywheel.

**ASTRABurnModule.** The automatic burn mechanism tied to aUSD fee volume is removed. With no aUSD revenue to feed it, the module has no input. The 2B ASTRA hard cap with no inflation, no burn replaces the v1.0 inflation-with-burn design.

**MintEngine for stablecoins.** The mint mechanism for aUSD is removed. SpaceKit does not mint any payment-token.

**Fee flywheel from aUSD service fees.** The economic loop tying aUSD revenue to ASTRA scarcity is removed. ASTRA's economic model is simpler: fixed supply, earned through service, used for network resources, no algorithmic feedback loops.

**Treasury allocations denominated in aUSD.** The treasury holds native ASTRA in the on-chain treasury contract (`0x…0004`). No aUSD tracking.

**SpaceKit Pay and x402.** The stablecoin payment router (USDC, USDT, DAI) and its x402 rail, specified in earlier v2 drafts, are retired. The legacy aUSD vault and its prepay credits are removed; vault charges are refused by the node.

**AstraRewards contract ledger.** Rewards are no longer kept in a WASM contract ledger. The node executes the rewards system calls natively and mints into native account balances (§1.3).

## Part 4 — Versioning

This spec is v2.1. Future revisions follow semantic versioning:

- **Patch** (v2.0.x): clarifications, corrections, additional examples. No mechanic changes.
- **Minor** (v2.x.0): non-breaking additions (e.g., new service categories).
- **Major** (v3.0.0): breaking changes (e.g., adjustments to the no-public-sale commitment, changes to the hard cap).

**Changelog.** v2.1 (September 2026) adds the proof-of-authority bootstrap phase. There is no bootstrap stake pool, no stake lending or delegation, and no liquidity seeding: validators stake only ASTRA they earned. PoA-phase rewards to authorities and affiliated operators vest (12-month cliff, 36 months), enforced by AstraRewards. Epoch rewards are split in proportion to each operator's measured units when the epoch closes. Supply, cap, emission curve and category shares are unchanged.

**Update (October 2026).** SpaceKit Pay and x402 were retired; ASTRA is the only currency and payments are ASTRA transfers on the SpaceKit chain (Part 2). Rewards, including the PoA-phase lock, are executed natively by the node at `0x…0003` instead of by the AstraRewards contract. The treasury is the on-chain treasury contract `0x…0004`. Supply, cap, emission curve, category shares and vesting terms are unchanged.

Major version changes require explicit on-chain governance for protocol parameters affected, and a fresh round of legal review. The team does not commit to never producing a v3, but commits that v3 would be a deliberate, transparent change rather than a quiet drift from v2.

## References

- **[`ASTRA.md`](./ASTRA.md)** — public-facing ASTRA overview
- **[`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md)** — halving curve, treasury/bootstrap, per-category rates
- **[`ASTRA_REWARDS_CONTRACT_SPEC.md`](./ASTRA_REWARDS_CONTRACT_SPEC.md)** — rewards system calls (native) + cap enforcement
- **[`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](./SERVICE_REWARD_ACCUMULATOR_SPEC.md)** — protocol SRA → CREDIT pipeline

## Contact

For questions on this specification:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
