# SpaceKit Tokenomics v2.0

**Status:** Canonical technical specification
**Date:** 2026
**Owner:** SWTCH Labs
**Location:** [`spacekit-tokenomics/`](./) (canonical). **Supersedes:** [`archive/SpaceKit_Tokenomics_v1.md`](./archive/SpaceKit_Tokenomics_v1.md) (April 2026, aUSD-era)

This document is the authoritative specification of SpaceKit's economic primitives. It describes three distinct primitives — ASTRA, SpaceKit Pay, and x402 — each with a specific role in the network's economic operation. Together they form the complete picture of how value flows through the SpaceKit ecosystem.

This document supersedes v1.0 entirely. The v1.0 spec described a dual-token model (ASTRA + aUSD) with an aUSD-driven burn flywheel. aUSD is no longer a SpaceKit product; SpaceKit Pay replaces its functionality with non-custodial routing of established stablecoins. ASTRA's economic model has been updated to match a fixed-supply utility token earned through service provision.

The decisions in this spec are locked. The supporting rationale is in the ASTRA Economic Model Decision Memo (internal, available to authorized parties on request).

## Part 1 — ASTRA

### 1.1 Overview

ASTRA is SpaceKit's native L1 utility token. It is used to pay for protocol-level network resources (compute, storage, messaging), to stake for validator participation in consensus, and to participate in on-chain governance.

ASTRA is earned exclusively by operators who provide measured network service. There is no public sale, no airdrop, no investment offering of ASTRA. Operators who run nodes that contribute to the network earn ASTRA proportional to their measured contribution.

ASTRA has a hard supply cap of 2,000,000,000 (two billion) tokens, enforced at the protocol level. The cap cannot be exceeded. No inflation mechanism increases supply beyond the cap. No automatic burn mechanism decreases supply on transactions.

### 1.2 Supply parameters

| Parameter | Value |
|-----------|-------|
| Total supply | 2,000,000,000 ASTRA (hard cap) |
| Decimals | 18 |
| Atomic unit | 1 wei-ASTRA = 10⁻¹⁸ ASTRA |
| Inflation | None |
| Automatic burn | None |
| Mint authority | Protocol only, capped at total supply |
| Standard | Native (L1), DID-keyed balances |

### 1.3 Emission

ASTRA is emitted to operators through the **Service Reward Accumulator (SRA)** — a protocol-level function that reads structured service logs, computes rewards per the emission schedule, and submits **CREDIT** instructions to the **AstraRewards** contract. See **[`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](./SERVICE_REWARD_ACCUMULATOR_SPEC.md)** and **[`ASTRA_REWARDS_CONTRACT_SPEC.md`](./ASTRA_REWARDS_CONTRACT_SPEC.md)**.

Emission follows a **4-year halving curve** (Bitcoin-style, adapted for continuous service):

- **Initial annual emission (year 1):** 200,000,000 ASTRA (10% of the 2B cap).
- **Decay:** Annual emission at year `t` is `200M × 0.5^(t/4)`; asymptotic cumulative operator emission ≈ **1.15B ASTRA**.
- **Hard cap:** `total_emitted` in AstraRewards cannot exceed 2B; the contract rejects any credit that would exceed it.

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

### 1.6 Network resource pricing

ASTRA is consumed when network resources are used:

**Gas for compute.** Smart contract executions are metered in gas. Gas is paid in ASTRA at a rate that floats based on network demand (similar to Ethereum's EIP-1559 base fee mechanism, though the specific mechanism is SpaceKit-native rather than Ethereum-compatible).

**Storage fees.** Operations that write to storage (deploying contracts, storing blobs, updating FactPackage graphs) consume ASTRA proportional to the bytes written and the durability period.

**Messaging fees.** Sending messages through the messaging layer consumes ASTRA proportional to the message size and the number of recipients.

**Identity operations.** DID registrations, credential issuance, and key rotation consume ASTRA proportional to the operation cost.

ASTRA consumed for resources flows to operators serving the relevant requests (compute → compute nodes, storage → storage nodes, etc.) as part of the service earning mechanism described above. The protocol does not retain consumed ASTRA in a central treasury; it flows directly from users to the operators providing the service.

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

### 1.8 No yield products

SpaceKit does not offer (and the protocol does not implement) any of the following:

- ASTRA staking pools that pay yield denominated in ASTRA
- Lending mechanisms where ASTRA holders deposit ASTRA and receive interest
- Liquidity mining programs
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

**Genesis treasury: 350,000,000 ASTRA (17.5% of cap).** Minted to the treasury DID at protocol genesis via the AstraRewards `INIT` operation. Held under multi-sig control by SWTCH Labs. Used for protocol development, audits, operational reserves, and ecosystem grants (subject to legal review per Section 1.9). **Not subject to the halving curve** — it exists at genesis and decreases only when spent. **Cannot be expanded** beyond the genesis 350M allocation.

**Bootstrap pool: 50,000,000 ASTRA (2.5% of cap)** drawn from treasury at genesis for initial validator stake. One-time only; subject to vesting (e.g. 4-year linear from genesis). See **[`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md)** sections 7–8.

**Protocol reserve:** ~496M ASTRA headroom under the 2B cap (cap minus asymptotic operator emission minus treasury) allocatable only by on-chain governance.

Operator emission and treasury credits are tracked in **AstraRewards** (`total_emitted`). ASTRA paid as gas for network resources flows to operators serving work; it is separate from the SRA emission path.

## Part 2 — SpaceKit Pay

### 2.1 Overview

SpaceKit Pay is non-custodial payment routing for AI service settlements. It moves established stablecoins (USDC, USDT, DAI) atomically between buyers (AI service consumers) and operators (AI service providers) with a flat 5% treasury fee.

SpaceKit Pay is not a token. It does not issue any asset. It does not custody funds. The contracts that implement SpaceKit Pay pull payment from the buyer, split it 95/5 between operator and treasury, and forward both transfers in a single transaction. The contract's balance is zero at the end of every successful payment.

SpaceKit Pay is deployed on multiple networks. Each deployment routes same-network payments only:

- Ethereum mainnet
- Base
- Polygon
- Arbitrum One
- Optimism
- SpaceKit mainnet

Solana support is planned but not in v1.

### 2.2 The two contracts

Each network deployment has two contracts:

**OperatorRegistry.** Self-service registry where operators register their DID and a payout address on the network. Operators control their own registrations; SWTCH Labs has no admin discretion over the operator set. The registry is read-open: anyone can look up an operator's payout address.

**PaymentRouter.** Atomic, non-custodial payment splitter. A buyer initiates a payment by approving the router and calling `payForService`. The router pulls the payment, looks up the operator's address from the registry, splits the payment 95/5 between operator and treasury, and forwards both transfers atomically in the same transaction.

The contracts are implemented in SKCL on the SpaceKit network and in Solidity on EVM-compatible networks. Identical event schemas across all deployments so indexers can treat the network as one logical protocol.

### 2.3 Treasury fee

Every payment routed through SpaceKit Pay deducts a 5% treasury fee. The remaining 95% goes to the operator. The treasury fee accumulates in a treasury address controlled by SWTCH Labs.

The fee is flat. There are no tiers, no discounts, no premium operators. Every payment is split the same way. The simplicity is the point: operators can predict their take-home for any payment instantly, and buyers can predict the operator's revenue instantly.

The fee funds protocol development, audits, and operations. It is software fee revenue, not custodial deposits or held assets. It is recognized as ordinary business income.

Treasury fee revenue is collected in the same stablecoin as the underlying payment (USDC payments produce USDC fee revenue, etc.). SWTCH Labs manages the collected treasury funds through standard business operations; the funds are not held in any SpaceKit-issued asset.

If the treasury fee rate changes in the future (subject to community input and on-chain governance for the SpaceKit-network deployment, or to SWTCH Labs board decision for EVM deployments), the change applies to future payments only. Historical payments stay at the rate that applied when they happened.

### 2.4 Supported tokens

At v1, SpaceKit Pay accepts payments in:

- USDC (Circle, on each network)
- USDT (Tether, where available)
- DAI (MakerDAO, on Ethereum)

Additional tokens may be added via governance after launch. The allowlist is administrative; only stablecoins from established issuers are accepted to prevent the protocol from being used to route exotic or potentially fraudulent assets.

The protocol does not interact with token bridges. Each network deployment routes only same-network payments. A buyer paying USDC on Ethereum routes to an operator with a registered Ethereum payout address. Cross-network routing is not in v1.

### 2.5 Non-custodial property

The router contract is structurally non-custodial. The atomic-routing pattern (pull, split, push) means funds enter and exit the contract in the same transaction. The contract holds zero balance at the end of every successful call.

This is a structural property, not an operational promise. There is no admin function that can intercept funds in transit. There is no upgrade path that could introduce a delay or hold. The router cannot custody funds even if SWTCH Labs wanted it to, because the contract logic forces every successful payment to settle in the same transaction.

The non-custodial property is the basis for the regulatory posture (per FinCEN FIN-2019-G001, non-custodial software that facilitates transactions is not a money transmitter). The legal posture is documented separately in the SpaceKit Pay Legal Posture Memorandum.

### 2.6 Operator registration

Operators wanting to receive payments through SpaceKit Pay register with the OperatorRegistry on each network they want to accept payments on. Registration requires:

- A DID (the operator's identity)
- A payout address on the network (the address the operator wants to receive 95% of payments at)
- A signature proving control of the DID

Operators can update or remove their registrations at any time. Only the operator's DID can modify their own registration; no third party can change another operator's payout address.

A single operator may register on multiple networks with different payout addresses (e.g., different addresses for Ethereum vs Base vs SpaceKit). The buyer's payment routes to whichever address the operator has registered on the network where the payment occurs.

### 2.7 Relationship to ASTRA

SpaceKit Pay does not mint, distribute, or affect ASTRA. The two are independent economic primitives:

- ASTRA is earned by operators for service on the SpaceKit network (compute, storage, messaging, validation).
- SpaceKit Pay routes USDC/USDT/DAI for AI service payments across multiple networks.

An operator might earn both: ASTRA for running a compute node that serves SpaceKit network requests, and USDC (via SpaceKit Pay) for AI inference services delivered to buyers paying with USDC. These are separate earning streams from separate activities.

On the SpaceKit network specifically, SpaceKit Pay can be configured to accept ASTRA as a payment asset (in addition to the stablecoin allowlist). When ASTRA is used as the payment asset, the router still operates non-custodially — pulling ASTRA from the buyer, splitting 95/5, pushing to operator and treasury — but the funds are ASTRA rather than stablecoins. This is a SpaceKit-network-only configuration and does not apply to the EVM deployments.

## Part 3 — x402

### 3.1 Overview

x402 is an HTTP-native standard for machine-to-machine payments. It defines how HTTP servers can request payment for resources (using the `402 Payment Required` status code with structured payment-requirement headers) and how clients can fulfill payment requirements transparently.

x402 is a protocol layer, not a payment system. The standard specifies how payment requirements are expressed and how payments are confirmed; it does not specify a particular payment mechanism. Multiple payment mechanisms can satisfy x402 requirements.

### 3.2 SpaceKit's role

SpaceKit Pay is one of the payment mechanisms that can fulfill x402 payment requirements. An HTTP server that wants to accept payments via SpaceKit Pay can specify SpaceKit Pay payment requirements in its `402` responses; clients with SpaceKit Pay capability can satisfy those requirements with a SpaceKit Pay transaction.

This composition is natural: x402 specifies the request/response semantics of payment-required HTTP exchanges; SpaceKit Pay handles the on-chain settlement. The two are designed to compose.

x402 is also compatible with other payment mechanisms (USDC on Base via the x402 reference implementation, other stablecoin networks, etc.). SpaceKit Pay does not depend on x402, and x402 does not depend on SpaceKit Pay. Each is useful on its own; they happen to compose cleanly when both are used.

### 3.3 No SpaceKit-specific x402 extension

SpaceKit does not define a custom variant of x402. We use the standard x402 protocol as defined by its specification, with SpaceKit Pay as the payment mechanism for SpaceKit-network-routed payments.

If future improvements to x402 are warranted (e.g., for better integration with SpaceKit's DID-based identity), those improvements are contributed to the x402 standard rather than forked into a SpaceKit-specific variant.

## Part 4 — How the three primitives compose

The three primitives are designed to be separate. They don't blur into each other. But they compose cleanly when an application needs more than one.

**Example: a contract that pays for an AI inference call.**

A contract on SpaceKit network wants to call an AI inference service. The contract holds USDC balance. The flow:

1. Contract calls SpaceKit Pay's `payForService` on SpaceKit network, specifying the operator's DID and the USDC amount.
2. SpaceKit Pay pulls USDC from the contract's balance, splits 95/5, pushes 95% to the operator's registered payout address (USDC on SpaceKit network) and 5% to the treasury.
3. The operator's compute node receives the inference request (via off-chain signaling outside the payment flow), serves the inference, and returns the result.
4. Separately, the operator's compute node earns ASTRA for the gas consumed during inference (this is a different earning stream from the SpaceKit Pay USDC payment).

The contract used SpaceKit Pay for the inference fee (USDC, routed to the operator). The operator earned ASTRA for the compute work (separate earning, paid by the SpaceKit protocol's emission mechanism for compute service). These don't double-count; the operator is paid for two different things — the AI inference service (paid by the buyer in USDC via SpaceKit Pay) and the network compute service (paid by the SpaceKit protocol in ASTRA via emission).

**Example: a pay-per-call API.**

A service operator runs an AI API. The operator wants to charge $0.01 per call. The flow:

1. A client requests an API endpoint without payment.
2. The server returns HTTP 402 with x402-formatted payment requirements specifying SpaceKit Pay payment ($0.01 USDC on Base).
3. The client constructs a SpaceKit Pay transaction satisfying the requirements, submits it, and includes the transaction reference in a retry of the API request.
4. The server verifies the SpaceKit Pay payment routed correctly (95% USDC to the operator's Base address, 5% to treasury), and serves the API response.

The client used x402 for the protocol semantics, SpaceKit Pay for the on-chain settlement. ASTRA is not involved unless the API is hosted on SpaceKit network and the protocol's compute service rewards apply.

## Part 5 — What's removed in v2

For clarity, the following constructs from the v1.0 spec are no longer part of SpaceKit:

**aUSD.** The collateralized stablecoin previously specified in v1.0 is removed. SpaceKit Pay replaces this functionality with non-custodial routing of established stablecoins (USDC, USDT, DAI). No vault contracts, no mint engine, no aUSD-denominated fees, no aUSD-driven burn flywheel.

**ASTRABurnModule.** The automatic burn mechanism tied to aUSD fee volume is removed. With no aUSD revenue to feed it, the module has no input. The 2B ASTRA hard cap with no inflation, no burn replaces the v1.0 inflation-with-burn design.

**MintEngine for stablecoins.** The mint mechanism for aUSD is removed. SpaceKit does not mint any payment-token.

**Fee flywheel from aUSD service fees.** The economic loop tying aUSD revenue to ASTRA scarcity is removed. ASTRA's economic model is simpler: fixed supply, earned through service, used for network resources, no algorithmic feedback loops.

**Treasury allocations denominated in aUSD.** Treasury operations are now denominated in the underlying stablecoins received from SpaceKit Pay (USDC, USDT, DAI), or in ASTRA from the treasury allocation. No aUSD tracking.

Any existing implementation code referencing aUSD (specifically the legacy `aUsdCredit` mechanism in the multi-asset vault contract used for Agent Hub prepay) is legacy testnet code being migrated to SpaceKit Pay routing. Production deployments use SpaceKit Pay; the legacy code does not appear in mainnet plans.

## Part 6 — Versioning

This spec is v2.0. Future revisions follow semantic versioning:

- **Patch** (v2.0.x): clarifications, corrections, additional examples. No mechanic changes.
- **Minor** (v2.x.0): non-breaking additions (new service categories, new supported tokens, new payment networks).
- **Major** (v3.0.0): breaking changes (e.g., adjustments to the no-public-sale commitment, changes to the hard cap, removal of the non-custodial property).

Major version changes require explicit on-chain governance for protocol parameters affected, and a fresh round of legal review. The team does not commit to never producing a v3, but commits that v3 would be a deliberate, transparent change rather than a quiet drift from v2.

## References

- **[`ASTRA.md`](./ASTRA.md)** — public-facing ASTRA overview
- **[`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md)** — halving curve, treasury/bootstrap, per-category rates
- **[`ASTRA_REWARDS_CONTRACT_SPEC.md`](./ASTRA_REWARDS_CONTRACT_SPEC.md)** — on-chain balance ledger + cap enforcement
- **[`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](./SERVICE_REWARD_ACCUMULATOR_SPEC.md)** — protocol SRA → CREDIT pipeline

## Contact

For questions on this specification:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
