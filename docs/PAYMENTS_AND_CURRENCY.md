# Payments and currency

SpaceKit has one currency: **ASTRA**. This page summarizes how money works on the network today, what was removed, and what we recommend doing before launch. The ledger-level detail is in [`infra/spacekit-compute-node/ASTRA_LEDGER.md`](../infra/spacekit-compute-node/ASTRA_LEDGER.md).

## How it works

**One ledger.** ASTRA exists only as native account balances on the SpaceKit chain, in wei (18 decimals), covered by each block's state root. Every node computes the same balances by executing the same blocks. Nothing else holds value:

- the browser runtime (spacekit-js) keeps no balances;
- contracts hold only the ASTRA sent to them.

**Supply.** ASTRA is created only inside blocks, by the node's native rewards calls (system address `0x…0003`):

| Call | Effect |
|---|---|
| `INIT` | Mints the genesis treasury allocation (350,000,000 ASTRA) to the treasury contract `0x…0004`. |
| `CREDIT` | Mints operator rewards for measured service into the operator's balance. |
| `CREDIT_LOCKED` | During proof of authority: locked rewards that vest from day 365 to day 1,095. |

- Total emission is capped at 2,000,000,000 ASTRA.
- Nothing mints on a consensus network: no faucet, bridge, rollup or payment.

**Moving ASTRA.**

- Every transaction pays gas. Used gas is burned.
- A transaction's value moves only if it succeeds.
- A contract receives value with the call and can pay only from its own balance. It can never spend its caller's ASTRA.

**Paying for things.**

| What | How |
|---|---|
| Any payment | An ASTRA transfer on the chain, proven by its transaction hash. `GET /v1/tx/{hash}` shows it; `POST /v1/payments/verify` checks payee, amount and success, and refuses a hash it has seen before. |
| Paid content, channels and apps | The `astra-entitlement-ledger` contract. Purchases and renewals pay the publisher's address directly; the ledger keeps nothing. Storage and messaging nodes check entitlements with read-only calls. |
| Per-call fees in contracts | The caller attaches the fee as value; `collect_fee` forwards it. The standard-library agents and anchor pay their fees to the treasury. |
| Sponsored use ("gasless") | The `spacekit-paymaster` contract holds ASTRA deposited by a sponsor and pays permitted callers within the sponsor's policy (per-call and daily limits, allowed DIDs and operations, expiry). |
| Signed intents (`/v1/execute`) | ASTRA wei only, capped by `max_value_wei`. The node validates the intent and lists the chain transactions to sign. |

**Treasury, staking and governance.**

- **Treasury.** The treasury is the on-chain contract `0x…0004`. It pays M-of-N approved spends; the signers come from the PoA genesis.
- **Staking.** Staking is native: a validator's stake is backed by its balance plus locked rewards. There is no receipt token.
- **Governance.** The network starts in proof of authority. The authorities can lift it to proof of stake once there are at least 10 validators.

## What was removed

| Removed | Replaced by |
|---|---|
| aUSD and the aUSD vault (USD balances, vault charges, `payment_vault_charge`) | ASTRA payments; vault charges are refused |
| x402 (USDC on Base) and the USD→ASTRA exchange rate | Payment verification by ASTRA transaction hash |
| Ethereum DAI/USDC "entitlement" deposits (micro-USD) and `/v1/entitlements*` | Native balances and the entitlement ledger |
| RouteKit's Ethereum vault relay (`/v1/charge`) | — |
| Host-side paymaster budgets in spacekit-js | The paymaster contract |
| The AstraRewards contract ledger | Native rewards minted into account balances |
| **SpaceKit Pay** (USDC/USDT/DAI router on Ethereum/Base) | Retired; marketplace sales are paid in ASTRA through the entitlement ledger. `infra/spacekit-pay/` is kept for history only. |

## Recommendations

### Before launch

1. **Decommission SpaceKit Pay on chain.**
   - Pause or retire any deployed `SpaceKitPayRouter` contracts, and the Ethereum entitlement and deposit vault contracts in `spacekit.xyz-contracts`.
   - Remove the `VITE_SPACEKIT_PAY_*` settings from the websites.
   - Tell any existing users how to withdraw what those contracts hold.
2. **Move the websites to the ASTRA flows.**
   - **Marketplace and agent hub.** In `spacekit.xyz-website`, `EconomicsPage.tsx` and `.env.production` still mention USDC, and the removed RouteKit relay was built for the agent hub's aUSD charge message (`agentHub/chargeMessage.ts`, `amountAUsd`). Switch these to the entitlement ledger and `chainContractCaller`.
   - **Anchor client.** It must attach the anchor fee (`spacekitAnchorWire.ts`).
3. **Set real prices.** The agent and anchor fees are placeholders: the old numbers, read as micro-ASTRA (for example, a RouteKit completion costs 0.0001 ASTRA). The large-transfer threshold proposed in `UNIFICATION_STRATEGY.md` also needs a value.
4. **Decide on protocol revenue.** Today the protocol takes nothing from usage:
   - gas is burned;
   - marketplace payments go entirely to publishers;
   - fees from the standard-library contracts go to the treasury;
   - the `FeeRouter` network fee exists in the library but no node applies it.

   If you want a protocol fee, put it in the contracts on chain, with the destination stated openly. Keep it away from buybacks.
5. **Plan how users get ASTRA.**
   - With one currency, every paid feature and every transaction needs ASTRA.
   - Have apps sponsor first use through the paymaster, and make it clear how operators earn ASTRA through service.
   - The faucet works only on development chains.
6. **Have counsel review the updated tokenomics.** Pre-sale prices, valuations and price-curve material are withdrawn from the fact sheet and executive summary; the retired SpaceKit Pay legal memo no longer applies. Keep marketing to using the network, consistent with the points from the SEC staff FAQ:
   - the PoA→PoS lift is the decentralization milestone;
   - staking has no receipt token;
   - no buybacks;
   - no return or price claims.

### Engineering follow-ups

1. **Make payment replay protection durable.** `PaymentVerifier` remembers accepted transaction hashes in memory only. The storage settlement inbox already records processed hashes; any other service that grants something lasting for a payment must store the hash with the grant.
2. **Remove dead paths:**
   - the CLI devnet flag `--astra-rewards-wasm`, which the node no longer reads;
   - the legacy `sdks/spacekit-standard-library/rewards/astra-rewards` contract;
   - the escrow arbiter default `did:spacekit:treasury`, which is not an address. Use `did:spacekit:0000000000000000000000000000000000000004` or a real arbiter DID.
3. **Fix RouteKit's optional `intent` build.** It fails against the current `ml-dsa` crate in `spacekit-primitives`; the default build is fine.
4. **Reconcile the storage tokenomics page.** It still mentions staking multipliers and a permanent "+50%" genesis-operator bonus that `ASTRA_EMISSION.md` does not specify. Either specify them in the emission spec or remove them.
5. **Archive or drop old copies.** `infra/spacekit-compute-node/.build-context/`, the `archive/` folders and `docs/old_docs/` still describe aUSD, x402 and SpaceKit Pay. Leave them out of published docs, or delete them.
