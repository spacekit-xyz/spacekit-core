# SpaceKit Pay

SpaceKit Pay is the payment layer for the AI economy. Same-network
stablecoin routing across Ethereum, Base, Polygon, Arbitrum, Optimism,
and SpaceKit at launch. Solana and additional networks follow.

Live: contracts deployed on SpaceKit testnet
Source: github.com/spacekit-xyz/spacekit-pay
Status: Pre-mainnet. Independent audit engagement in progress.

## What SpaceKit Pay is

SpaceKit Pay is two smart contracts deployed on every network it
supports:

**OperatorRegistry** is the source of truth for operator payout
addresses. An AI service operator registers their DID and a payout
address on each network they want to receive payments on. A single
operator might register addresses on Ethereum (USDC at one address),
Base (USDC at another), and SpaceKit (ASTRA at a third). The registry
is self-managed: only the operator can update their own entries.

**PaymentRouter** is an atomic, non-custodial payment splitter. A
buyer approves the router for a specific amount of a specific
stablecoin, then calls payForService with the operator's DID and the
amount. The router pulls the payment, looks up the operator's address
on the current network, splits the payment 95/5 between operator and
treasury, and forwards both transfers atomically in the same
transaction.

The router never holds a balance. The atomic-routing structure means
funds enter the contract and exit the contract in the same call. The
contract is non-custodial by construction.

## What makes this different

Most payment processors are custodial. Stripe takes payments from
buyers, holds the funds during a settlement period, and disburses to
merchants. The holding period creates regulatory complexity and
operational risk.

SpaceKit Pay is non-custodial. The buyer's payment moves to the
operator and treasury in the same transaction that the buyer initiated.
No holding period. No intermediate custody. No discretion over user
funds. The router contract is software that helps the user complete a
payment, not a service that handles money on the user's behalf.

This design choice matters for three reasons:

**Regulatory clarity.** Per FinCEN guidance FIN-2019-G001,
non-custodial software that facilitates user transactions is not a
money transmitter. The custody question is the key test, and SpaceKit
Pay structurally cannot custody funds. The regulatory framework that
applies is software development, not financial services.

**Operational simplicity.** No reserves to maintain, no settlement
periods to manage, no chargebacks to process. The contract logic is
small (a few hundred lines per deployment) and the operational
surface is minimal.

**User trust.** A buyer can verify the contract behavior on-chain. A
single transaction either succeeds entirely (the buyer pays, the
operator and treasury receive) or fails entirely (no funds move).
There is no scenario where the buyer's funds get stuck in transit
or where the protocol withholds payment from the operator.

## Same-network routing in v1

For the initial launch, SpaceKit Pay routes payments on a single
network: the buyer's network and the operator's payout address must
be on the same chain. A buyer paying USDC on Ethereum needs the
operator to have registered an Ethereum payout address.

This is a deliberate design choice for v1. Cross-network routing
introduces bridge dependencies and additional risk. Building
single-network routing first lets us validate the protocol with the
simpler trust model. Cross-network routing through established
bridges (LayerZero, Wormhole, or others) is on the roadmap for v2.

Operators register payout addresses on every network they want to
accept payments on. A buyer's interface looks up the operator's
addresses and chooses one based on what the buyer holds.

## The 5% treasury fee

Every payment routed through SpaceKit Pay deducts a 5% treasury fee.
The remaining 95% goes to the operator. The treasury fee accumulates
in a treasury address owned by SWTCH Labs.

The fee is flat. There are no tiers, no discounts, no premium
operators. Every payment is split the same way. The simplicity is the
point: operators can predict their take-home for any payment
instantly, and buyers can predict the operator's revenue instantly.

The fee funds protocol development, audits, and operations. It is
software fee revenue, not custodial deposits or held assets. It is
recognized as ordinary business income.

If a treasury fee changes in the future (subject to community input
and on-chain governance), the change applies to future payments only.
Historical payments stay at the rate that applied when they happened.

## Supported networks and tokens at launch

Networks at v1:

- Ethereum mainnet
- Base
- Polygon
- Arbitrum One
- Optimism
- SpaceKit mainnet

Tokens accepted at v1:

- USDC (Circle, on each network)
- USDT (Tether, where available)
- DAI (MakerDAO, on Ethereum)

Additional tokens may be added via governance after launch. The
allowlist is administrative; only stablecoins from established
issuers are accepted to prevent the protocol from being used to
route exotic or potentially fraudulent assets.

Solana support is planned but not in v1. Solana's account model and
SPL tokens require a different contract implementation than the EVM
chains; we will ship it once the EVM and SpaceKit deployments have
been operating cleanly for some time.

## How operators register

An operator who wants to receive payments through SpaceKit Pay calls
the OperatorRegistry contract on the network where they want to
receive payments:

```
register(network: "ethereum", address: 0x...)
```

The registration is signed by the operator's DID (using SPHINCS+ on
SpaceKit, ECDSA on EVM chains). Only the operator can update or
remove their own registration; SWTCH Labs has no admin discretion
over the operator set.

An operator can register on multiple networks. Each network has its
own OperatorRegistry deployment; the operator registers separately
on each network they want to receive payments on.

## How buyers pay

A buyer paying for an AI service through SpaceKit Pay:

1. The buyer's wallet (or a contract acting on the buyer's behalf)
   calls `approve(routerAddress, amount)` on the stablecoin contract.

2. The buyer's wallet (or contract) calls `payForService(token,
   operatorDID, amount)` on the SpaceKit Pay router.

3. The router pulls the payment from the buyer's wallet, looks up
   the operator's address on the current network, splits 95/5, and
   forwards both transfers.

4. The router emits a PaymentRouted event that the buyer's wallet
   and any indexers can use to confirm the payment.

All of this happens in a single transaction. If any step fails, the
entire transaction reverts and no funds move.

## Integration with the SpaceKit ecosystem

SpaceKit Pay is the payment rail for AI services on the SpaceKit
network. Three integration points:

**RouteKit and other agent contracts.** The SDK exposes a
`payment_pay_for_service` helper that wraps the SpaceKit Pay router
call. Agent contracts that charge for inference services route their
payments through this helper. The buyer experience is "approve once
per agent, then pay per call."

**Growformer service marketplace.** When a contract pays for a
Growformer inference call, the payment routes through SpaceKit Pay on
the relevant network. The operator address registered for that
operator's DID receives 95%; 5% goes to the SpaceKit treasury.

**x402 compatibility.** SpaceKit Pay is x402-compatible: an HTTP
response with `402 Payment Required` can specify a SpaceKit Pay
payment as the required payment. This works the same way as the x402
USDC-on-Base flow, with the addition of operator-DID-keyed routing
through the registry.

## What SpaceKit Pay is not

We want to be explicit about what the protocol does not do:

- **Not a stablecoin.** SpaceKit Pay does not issue any token. The
  payments routed are USDC, USDT, DAI, issued by their respective
  issuers.
- **Not a custodian.** The router holds zero balance between
  transactions. Funds enter and exit the contract in the same call.
- **Not a money transmitter (we believe).** Per FinCEN guidance,
  non-custodial software is not money transmission. We are obtaining
  a written legal opinion on this before mainnet.
- **Not a yield product.** SpaceKit Pay does not issue any token,
  and routing payments through it does not generate yield for any
  party. The protocol is payment infrastructure, not an investment
  product. (SpaceKit's network has a native utility token, ASTRA,
  earned by operators running nodes — but ASTRA is unrelated to
  SpaceKit Pay and is not issued, distributed, or affected by
  routing payments.) Users who pay for AI services receive AI
  services.
- **Not an exchange.** SpaceKit Pay does not match counterparties or
  facilitate trading.

## ASTRA is separate

SpaceKit's network has a native utility token, **ASTRA**, earned by
operators running nodes (no airdrop, no public sale to retail). ASTRA
is unrelated to SpaceKit Pay: routing payments does not mint,
distribute, or affect ASTRA. Regulatory treatment of ASTRA is a
distinct question from SpaceKit Pay.

## Read more

- Canonical tokenomics:
  [`SpaceKit_Tokenomics.md`](../../economics/spacekit-tokenomics/SpaceKit_Tokenomics.md)
  (Part 2 — SpaceKit Pay)
- ASTRA emission:
  [`ASTRA_EMISSION.md`](../../economics/spacekit-tokenomics/ASTRA_EMISSION.md)
- Source: github.com/spacekit-xyz/spacekit-pay
- Legal posture memorandum: available on request to accredited
  investors and regulators
- /technology/runtime (the runtime that calls SpaceKit Pay during
  service payments)
- /technology/identity (the DID layer SpaceKit Pay identifies
  operators by)
- /technology/l1 (the SpaceKit network where the SKCL deployments
  live)
