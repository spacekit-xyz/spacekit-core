# ASTRA

ASTRA is SpaceKit's native L1 utility token. It is used to pay for network resources, to stake as a validator, and to participate in protocol governance. Operators earn ASTRA by running nodes that serve the network. The total supply is capped at 2 billion. There is no public sale.

## What ASTRA is

Three things ASTRA does:

**Pays for network resources.** Smart contract executions, storage operations, messaging, identity operations — all of these consume ASTRA. Used gas is burned. Operators are paid for the resources they provide through service emission (below), not from the gas.

**Secures the network.** Validators lock ASTRA as a security deposit when participating in consensus. The locked ASTRA is at risk: misbehavior (double-signing, prolonged unavailability, censorship) results in slashing. The stake itself does not earn yield — validators earn ASTRA by providing service while staked, not by holding the stake.

**Governs the protocol.** Operators with staked ASTRA participate in on-chain governance proposals — emission schedule adjustments, slashing parameters, treasury operations, protocol upgrades. Voting power is proportional to active stake.

## How ASTRA is earned

ASTRA is earned by operators who run nodes contributing measured service to the network. There are four service categories:

**Consensus validation.** Run a validator node. Earn ASTRA proportional to honest validation activity — block proposals, votes on correct proposals, block envelope signatures, uptime during assigned slots.

**Compute service.** Run a compute node. Earn ASTRA proportional to the gas units of contract execution your node serves to network users.

**Storage service.** Run a storage node. Earn ASTRA proportional to durable storage capacity provided, successful read and write operations served, and storage proof attestations submitted on time.

**Messaging service.** Run a messaging node. Earn ASTRA proportional to messages successfully delivered to recipients.

You earn ASTRA for what you do, not for what you hold. A validator who locks stake but provides no service earns nothing. A storage node that maintains capacity but fails durability proofs earns nothing for the affected storage. The protocol measures contribution and pays for measured contribution.

## What ASTRA is not

A few things ASTRA is not, stated explicitly:

**Not a security.** ASTRA is a utility token earned through service provision. Operators earn it for measured work; there is no investment of money in ASTRA. The earning model is structured to be defensible under Howey: no investment of money, no common enterprise, no expectation of profits derived from the efforts of others, intrinsic utility consumed for network operations.

We have requested a separate written legal opinion from Withers Worldwide on ASTRA's regulatory status. Until that opinion is in hand, ASTRA's regulatory characterization is informed by our analysis and not yet legally finalized.

**Not a stablecoin.** ASTRA is not pegged to any external asset. Its value, to the extent any exists, derives from network utility — the demand for SpaceKit network resources priced in ASTRA. There is no peg mechanism, no collateral backing, no algorithmic stability.

**Not sold to investors.** There is no public sale of ASTRA. No pre-sale, no airdrop, no presale tiers, no initial coin offering, no investment offering of any kind. Operators earn ASTRA exclusively through service provision.

SWTCH Labs has raised equity capital ($3M+ at $55M+ valuation floor) from accredited investors. The raise is equity in SWTCH Labs as a company. No portion of any equity raise is denominated in ASTRA. No investor is entitled to ASTRA tokens as part of their equity stake.

**Not yield-bearing.** Holding ASTRA passively does not earn yield. There is no staking pool that pays interest. There is no lending mechanism. There is no liquidity mining, and SWTCH Labs does not seed liquidity: markets form only from ASTRA that operators earned. There is no inflation reward for passive holders. The only mechanism to acquire newly-emitted ASTRA is through operator service.

Third-party DeFi protocols built on SpaceKit may exist and may create yield products denominated in ASTRA. These are not SWTCH Labs products and the SpaceKit protocol does not endorse or guarantee them.

**Not one of several currencies.** ASTRA is the only currency on the SpaceKit network. Services, apps, content and channels are priced and paid in ASTRA. See [Paying for services and content](#paying-for-services-and-content) below.

## Supply

| Parameter | Value |
|-----------|-------|
| Total supply (hard cap) | 2,000,000,000 ASTRA |
| Decimals | 18 |
| Atomic unit | 1 wei-ASTRA = 10⁻¹⁸ ASTRA |
| Inflation | None |
| Burn | Used gas only (no burn tied to fee volume) |

The 2 billion cap is enforced at the protocol level. The protocol has no mechanism to create ASTRA beyond the cap. The supply does not inflate.

ASTRA is emitted to operators through the **Service Reward Accumulator (SRA)**, which places rewards system calls at the start of a block. The node executes them natively and mints each operator's reward straight into its account balance, per measured service. Emission follows a **4-year halving curve**: **200M ASTRA** in year 1, decaying toward an asymptotic **~1.15B** cumulative operator total under the 2B cap.

**Genesis treasury: 350M ASTRA (17.5%)** is minted at protocol INIT to the on-chain treasury contract (not subject to halving). There is no bootstrap stake pool: nobody is given or lent ASTRA to stake, and nobody seeds markets. Validators stake only what they earned.

Default annual category split: consensus **40%**, compute **30%**, storage **20%**, messaging **10%**. Governance may adjust shares within bounds; the **2B cap is not adjustable**.

Details: [`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md), [`ASTRA_REWARDS_CONTRACT_SPEC.md`](./ASTRA_REWARDS_CONTRACT_SPEC.md), [`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](./SERVICE_REWARD_ACCUMULATOR_SPEC.md).

## Validator staking

Operators who want to participate in consensus validation must lock ASTRA as a security deposit.

**Why staking exists.** Two reasons. First, skin in the game: a validator with locked ASTRA has direct economic exposure to network safety. Misbehavior triggers slashing — partial or full forfeiture of staked ASTRA — proportional to the severity of the misbehavior. Second, Sybil resistance: without a stake requirement, an adversary could spin up many validator identities cheaply.

**The stake doesn't earn yield.** This is the important distinction. Validators earn ASTRA through the service they provide while staked — proposing blocks, voting on proposals, signing envelopes. They do not earn ASTRA by holding the stake passively. A validator who locks ASTRA but provides no service earns nothing. The stake is the price of admission to participate as a validator and earn through service, not an investment that pays interest.

**Slashing.** Staked ASTRA is at risk. Specific misbehavior categories and their slashing fractions are documented in the validator operations guide. Severe misbehavior (double-signing, signing equivocating block proposals) can result in loss of the full stake. Less severe misbehavior (prolonged unavailability) results in partial slashing.

**Withdrawal delay.** When a validator wants to exit and reclaim their stake, there is a withdrawal delay (currently several days). The delay exists so that any misbehavior the validator may have committed in their final rounds has time to be detected and prosecuted before the stake is released.

Specific staking parameters (minimum stake amount, slashing fractions, withdrawal delay) are protocol parameters set by governance. Current values are in the validator operations guide.

**Bootstrap: proof of authority first.** A new network starts with a small set of authorities: known operators, including SWTCH Labs and invited partners, who validate without stake. New validators are admitted by a two-thirds vote of the authorities. Once the network has at least 10 validators, the authorities can vote to switch to proof of stake. Staking and stake-weighted governance start then.

**Launch operators' rewards are locked.** Authorities earn the normal ASTRA for the service they provide, but anything they (or any SWTCH Labs–affiliated operator) earn during proof of authority is locked. It has a 12-month cliff from genesis, then vests linearly to month 36. Locked ASTRA can be staked but not sold or transferred. Details: [Tokenomics §1.11](./SpaceKit_Tokenomics.md).

## Governance

ASTRA holders with active validator stake participate in on-chain governance. Governance scope includes:

- Emission schedule parameters (within the hard cap)
- Slashing fractions and rules
- Gas pricing mechanism
- Treasury fee rates (where applicable)
- Protocol upgrade activation
- Reference extension activation status (e.g., the spacetime consensus extension)

Voting power is proportional to active validator stake. ASTRA held but not staked as validator stake does not vote. This is deliberate: it ensures protocol decisions are made by parties with active operational responsibility for the network, not by passive holders.

The governance mechanism — proposal format, voting period, quorum requirements, activation timing — is documented in the [governance specification](/docs/governance).

## Paying for services and content

ASTRA is the only currency on the SpaceKit network. A payment is an ASTRA transfer on the chain, proven by its transaction hash.

- **Services.** A service names its price in ASTRA. The client pays with a chain transaction, and the service checks the hash (`POST /v1/payments/verify`) before serving.
- **Apps, content and channels.** Sales go through the `astra-entitlement-ledger` contract. The buyer pays the listing price straight to the publisher's address; the ledger keeps nothing.
- **Contract calls.** A paid call carries its fee as attached value. A contract can pay only from its own balance, never from its caller's.

SpaceKit Pay, an earlier stablecoin payment router, was retired. See [`PAYMENTS_AND_CURRENCY.md`](../../docs/PAYMENTS_AND_CURRENCY.md).

An operator can earn in two ways: emission for the network service its node provides (paid by the protocol), and ASTRA payments from buyers for services it sells. These don't double-count. They pay for two different things.

## ASTRA on secondary markets

Operators who have earned ASTRA through service may trade it on secondary markets. SWTCH Labs does not:

- Sell ASTRA on any exchange
- List ASTRA on any specific exchange
- Endorse any specific secondary venue
- Control secondary-market prices
- Make forward-looking statements about ASTRA's market value

If ASTRA appears on a secondary market, that's a result of operators trading their earned ASTRA. SWTCH Labs does not operate, support or promote such trading, and makes no claim about ASTRA's price or value.

## Honest limitations

A few things worth surfacing directly:

**ASTRA's regulatory status is not yet finalized.** We are obtaining a written legal opinion from Withers Worldwide. Until that opinion is in hand, ASTRA's regulatory characterization is our considered analysis but not yet legally confirmed.

**The emission schedule is set but may evolve.** The current emission curve is based on our best understanding of how network participation will scale. As the network grows and operator behavior becomes observable, the schedule may need adjustment. Adjustments happen through on-chain governance, not through unilateral SWTCH Labs decision.

**Validator economics are early.** The slashing parameters, minimum stake amount, and reward weighting between service categories are initial values informed by analysis but not yet validated under sustained adversarial operation. Mainnet launch will refine these.

**Secondary-market price is not under SWTCH Labs control.** Market dynamics for utility tokens are driven by network demand for the resources the token unlocks. SWTCH Labs does not make price predictions or commitments. Operators who expect ASTRA to appreciate in market value should treat that expectation as their own analysis, not a SWTCH Labs claim.

**Cross-jurisdiction operator participation needs legal clarity.** If non-US operators earn ASTRA, the regulatory treatment in their jurisdictions may differ from the US analysis. We are working with Withers on jurisdiction-specific guidance and will publish updates as it becomes available.

## How to earn ASTRA

For operators interested in earning ASTRA, the path is:

1. **Get a DID.** Run `spacekit did create` from the SpaceKit CLI. Your DID is your network identity.

2. **Set up your node.** Decide which service category (or categories) you want to provide: consensus validation, compute, storage, or messaging. Configure your node accordingly. The [node operator guide](/docs/operating-nodes) walks through setup.

3. **Register on-chain.** Your node registers with the SpaceKit protocol announcing the services you provide. Registration is permissionless and DID-keyed.

4. **For validation only: get admitted or stake ASTRA.** While the network is in proof of authority (fewer than 10 validators), validators are admitted by a vote of the existing authorities, with no stake; apply through [spacekit.xyz](https://spacekit.xyz/contact). After the switch to proof of stake, you lock ASTRA you earned as a security deposit. Run a compute, storage or messaging node to earn it; there is no stake lending or delegation.

5. **Provide service.** Run your node, serve the requests assigned to it, and earn ASTRA proportional to your measured contribution. Rewards accrue continuously and are credited to your DID's balance on a regular cadence.

The [operator guide](/docs/operating-nodes) covers the operational specifics. The [validator operations guide](/docs/validator-operations) covers the validator-specific setup.

## Read more

- [Node operator guide](/docs/operating-nodes)
- [Validator operations guide](/docs/validator-operations)
- [Payments and currency](../../docs/PAYMENTS_AND_CURRENCY.md)
- [Governance specification](/docs/governance)
- [Tokenomics technical specification v2.0](./SpaceKit_Tokenomics.md) (canonical reference)
- [ASTRA emission schedule](./ASTRA_EMISSION.md)
- [Rewards system calls spec](./ASTRA_REWARDS_CONTRACT_SPEC.md)
- [Service Reward Accumulator spec](./SERVICE_REWARD_ACCUMULATOR_SPEC.md)
- [What is SpaceKit?](/docs/what-is-spacekit) (network overview)

For specific questions about ASTRA not covered here: astor@swtch.ai.
