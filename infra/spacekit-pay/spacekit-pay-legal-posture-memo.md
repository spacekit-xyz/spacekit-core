# SpaceKit Pay: Legal Posture Memorandum

**Prepared by:** SWTCH Labs (internal)
**For:** Withers Worldwide review
**Subject:** Regulatory posture for SpaceKit Pay non-custodial payment routing
**Status:** Internal working document for legal review

This memorandum sets out our internal understanding of the regulatory
posture for SpaceKit Pay. It is not legal advice. We are providing it
to counsel for review and validation. Our intent is to operate within
clear regulatory frameworks and to obtain a written legal opinion
before mainnet launch.

## 1. What SpaceKit Pay is

SpaceKit Pay is a set of smart contracts deployed across multiple
blockchain networks (Ethereum, Base, Polygon, Arbitrum, Optimism, and
SpaceKit at v1; additional networks planned). Each deployment routes
same-network stablecoin payments between buyers and operators of AI
inference services.

The protocol consists of two contract types per network deployment:

- **OperatorRegistry:** A self-service registry where AI service
  operators register their DID and a payout address on a given
  network. Operators control their own registrations; SWTCH Labs has
  no admin discretion over the operator set.

- **PaymentRouter:** An atomic, non-custodial payment splitter. A
  buyer initiates a payment by approving the router and calling
  `payForService`. The router pulls the payment, splits it 95% to
  the operator's registered address and 5% to a treasury address,
  and forwards both transfers atomically in the same transaction.
  The router holds zero balance at the end of every successful call.

## 2. What SpaceKit Pay is not

We are explicit about what the protocol does not do:

**Not a stablecoin issuer.** SpaceKit Pay does not issue any token.
There is no SpaceKit-issued asset called aUSD, SUSD, or anything
similar. The protocol routes USDC, USDT, DAI, and other established
stablecoins issued by their respective issuers (Circle, Tether,
MakerDAO).

**Not a custodian.** The atomic routing structure means the router
contract never holds user funds across transactions. A buyer's funds
enter and exit the contract in the same transaction. There is no
balance sheet, no held assets, no holding period.

**Not a money transmitter (we believe).** Under FinCEN guidance
FIN-2019-G001, non-custodial software that facilitates transactions
between parties is not a money transmitter. The relevant test is
custody. Since the router never has the ability to access or move
user funds outside the user-initiated, atomic transaction, we believe
the protocol falls outside the money-transmitter definition.

**Not an exchange.** SpaceKit Pay does not match counterparties or
facilitate trading. It routes specific payments between specific
identified parties (buyer to operator) at a fixed split.

**Not a yield product.** Neither the buyer nor the operator earns
yield from holding any asset issued by SpaceKit Pay. The protocol
does not offer staking, lending, or any return-bearing mechanism.
(Note: SpaceKit's network has a native utility token, ASTRA, earned
by operators running nodes through proof-of-work-equivalent
contribution. ASTRA is separate from SpaceKit Pay; routing payments
through SpaceKit Pay does not mint, distribute, or affect ASTRA. See
[`SpaceKit_Tokenomics.md`](../../economics/spacekit-tokenomics/SpaceKit_Tokenomics.md)
and [`ASTRA_EMISSION.md`](../../economics/spacekit-tokenomics/ASTRA_EMISSION.md).

## 3. Regulatory frameworks reviewed

We have reviewed the following frameworks for applicability:

### FinCEN (US federal money transmission)

The relevant guidance is FIN-2019-G001 (May 2019), which addresses
how the Bank Secrecy Act applies to convertible virtual currency
activities. Key sections:

- Section 4.5.1 discusses providers of anonymizing software services.
  The guidance states: "An anonymizing software provider is not a
  money transmitter. FinCEN regulations exempt from the definition
  of money transmitter those persons providing 'the delivery,
  communication, or network access services used by a money
  transmitter to support money transmission services.'"

- The non-custodial property is the key factor. A provider whose
  software facilitates transactions without ever taking custody is
  outside the money-transmitter definition.

**Our position:** SpaceKit Pay is non-custodial by design. The
contracts never hold funds across transactions. SWTCH Labs is the
software provider, not the money transmitter. We request Withers
validate this position.

### State money transmitter regulations

Most US states model their money transmitter regulations on the
federal definition but vary in detail. New York's BitLicense regime
is the most comprehensive. Texas, California, and Wyoming have
their own frameworks.

**Our position:** If the federal analysis holds (non-custodial =
not a money transmitter), the state-level analysis likely follows.
However, individual state interpretations may differ. We request
Withers identify any state-level concerns, particularly New York,
where the BitLicense regime captures activities the federal
framework does not.

### Securities laws (Howey, Reves)

The Howey test for investment contracts requires (1) an investment
of money, (2) in a common enterprise, (3) with an expectation of
profits, (4) derived from the efforts of others.

**Our position (SpaceKit Pay):** SpaceKit Pay does not create an
investment contract. Users pay for AI services and receive the
services in exchange. There is no investment, no common enterprise
(each payment is bilateral), no profit expectation (users receive
services, not returns), and SWTCH Labs' efforts do not generate
profits for users (the operators provide the services). We request
Withers confirm this Howey analysis for SpaceKit Pay specifically.

**Separate question (ASTRA):** ASTRA is SpaceKit's native utility
token, earned by operators running nodes (no airdrop, no public sale
to retail). We believe ASTRA is utility earned through service
provision, not a security under Howey, but that analysis is
distinct from SpaceKit Pay. See question 11 below.

### Bank Secrecy Act / AML

Even if not a money transmitter, the protocol may attract AML
attention if it facilitates large dollar flows.

**Our position:** The underlying stablecoins (USDC, USDT, DAI)
implement issuer-level sanctions enforcement at the token contract
layer. Sanctioned addresses cannot send those tokens. SpaceKit Pay
inherits this compliance by routing tokens whose sanctioned-address
behavior is enforced by the issuers. We do not maintain our own
sanctions list; we rely on the source-token compliance.

For higher-risk operational hygiene, the SpaceKit Pay front-end
will:
- Geofence sanctioned jurisdictions (Cuba, Iran, North Korea,
  Syria, Crimea, Donetsk, Luhansk)
- Display terms of service that prohibit use by sanctioned parties
- Cooperate with valid law enforcement requests

We request Withers advise on whether additional AML measures are
needed beyond inheriting source-token compliance.

### MiCA (EU regulation of crypto-asset service providers)

MiCA defines crypto-asset service providers (CASPs) broadly and
came into force June 2024. The "transfer of crypto-assets on behalf
of clients" definition could potentially apply to payment routing.

**Our position:** SpaceKit Pay does not transfer crypto-assets on
behalf of clients; the user themselves initiates and signs each
transfer. The protocol facilitates a user-controlled transaction
rather than executing one on the user's behalf. However, the EU
definitions are broad enough that we may need explicit guidance.

We request Withers advise on whether EU operations require explicit
CASP registration, and whether geofencing the EU until clarity
emerges is warranted.

### OFAC sanctions

The Tornado Cash sanctions (August 2022) established that protocol
smart contracts themselves can be designated. SWTCH Labs operations
must respect OFAC designations.

**Our position:** The PaymentRouter inherits OFAC compliance from
the source tokens (USDC, USDT, DAI). If OFAC designates an address,
that address cannot send USDC, and therefore cannot pay through
SpaceKit Pay. We do not maintain a separate sanctions screening,
but we will not knowingly enable transactions involving designated
parties. The geofencing and terms-of-service measures above support
this position.

## 4. Entity and operational structure

### Wyoming entity

SWTCH Labs is a Wyoming-registered entity. Wyoming has the most
favorable framework for crypto-protocol companies in the US,
including its Decentralized Unincorporated Nonprofit Association
(DUNA) and Special Purpose Depository Institution (SPDI) frameworks.

**Position:** SpaceKit Pay operates as part of SWTCH Labs' software
development activities. The protocol itself is open-source smart
contracts deployed on public blockchains; SWTCH Labs maintains the
source code, deploys the contracts initially, and receives the
treasury fee accrued to the treasury address (which is owned by
SWTCH Labs).

### Treasury fee revenue

The 5% treasury fee accumulates in a treasury address controlled by
SWTCH Labs. This is software fee revenue, recognized as ordinary
business income.

**Position:** This is software service revenue, not deposit-taking
or fund management. It is taxable as ordinary income. We request
Withers confirm there are no separate financial-services
classifications that apply.

### Admin functions

The PaymentRouter contracts have administrative functions:
- Update the treasury address
- Update the OperatorRegistry contract address
- Allowlist or revoke supported tokens
- Rotate the admin DID/address
- Sweep tokens accidentally sent directly to the contract (not
  funds in transit, which are structurally impossible)

These admin functions are held by a multi-signature wallet
requiring 3-of-5 signatures from independent SWTCH Labs operators.
The admin functions do not enable interception of user funds in
transit during routing; they enable parameter changes and recovery
of misdirected funds.

**Position:** The admin functions are operational, not custodial.
They affect parameters and emergency recovery, not user-funds
custody. The 3-of-5 multi-sig structure provides reasonable
governance.

We request Withers advise on whether the admin functions create
regulatory exposure we should mitigate further (e.g., timelock,
on-chain governance, etc.).

## 5. Specific questions for Withers

We ask Withers to address the following questions in a written
legal opinion before mainnet launch:

1. **Is SpaceKit Pay a money transmitter under FinCEN or US state
   regulations?** We believe no, per FIN-2019-G001. Please confirm
   or identify exposures.

2. **Is SpaceKit Pay's treasury fee revenue subject to any
   financial-services classifications beyond ordinary business
   income?** We believe no. Please confirm.

3. **Does SpaceKit Pay create securities under Howey or Reves?**
   We believe no. Please confirm.

4. **Does MiCA require CASP registration for SpaceKit Pay operations
   targeting EU users?** Please advise. If yes, we may geofence the
   EU until registration is obtained.

5. **Are there state-level concerns we should address before
   mainnet, particularly New York BitLicense?** Please advise.

6. **What user-facing disclosures are required on the SpaceKit Pay
   product page?** We will draft a "How SpaceKit Pay works" page
   and submit it for review.

7. **Are the admin functions on the PaymentRouter acceptable as
   designed, or should they be timelocked or moved to on-chain
   governance?**

8. **Is the inheritance of source-token sanctions compliance
   sufficient, or do we need independent screening?**

9. **What jurisdictions should we explicitly avoid for the initial
   launch?**

10. **Are there registrations (FinCEN registration as a Money Services
    Business, state licenses, OFAC reporting) we should obtain
    proactively even if not strictly required, for operational
    safety?**

11. **ASTRA token regulatory treatment.** ASTRA is SpaceKit's native
    utility token, earned by operators running nodes (no airdrop, no
    public sale). We believe ASTRA is utility earned through service
    provision, not a security under Howey, but request a separate
    analysis from the SpaceKit Pay questions above.

## 6. Documents accompanying this memo

For Withers's review:

- **PaymentRouter contract source** (Solidity, for Ethereum and EVM
  chains): `SpaceKitPayRouter.sol`
- **OperatorRegistry and PaymentRouter contract source** (SKCL,
  for SpaceKit): `spacekit-pay-operator-registry.rs`,
  `spacekit-pay-payment-router.rs`
- **Sequence diagram** showing the atomic routing flow
- **SpaceKit Pay product page draft** (forthcoming)
- **SpaceKit Pay terms of service draft** (forthcoming)

## 7. Timeline

We are committing to:

- **Months 1-2:** Contract development and unit testing, in parallel
  with Withers review.
- **Months 3-4:** Withers review and remediation of any issues
  raised in the legal opinion.
- **Months 5-7:** Independent technical audit of the contracts.
- **Month 8:** Bug bounty program.
- **Month 9-10:** Mainnet launch on Ethereum and SpaceKit; other
  networks (Base, Polygon, Arbitrum, Optimism) follow once initial
  launch is stable.

Withers's input affects this timeline. If material legal changes
are required, we will adjust accordingly. We prefer to ship right
rather than to ship fast.

## 8. Contact

For questions on this memo or the SpaceKit Pay design:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
