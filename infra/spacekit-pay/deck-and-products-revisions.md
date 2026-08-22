# Deck Slide 10 Revision

## Replace the current "ECONOMICS" slide content with this

**Section header:** ECONOMICS

**Title:** Operators monetize directly. Treasury fee funds the network.

**Subtitle:** Built on three economic rails, each doing one job well.

### Primary stat blocks (top half of slide)

**95%**
to operators
buyers pay operators directly through SpaceKit Pay

**5%**
to network treasury
flat fee on every payment, funds protocol development

### Three economic rails (bottom half of slide)

**SpaceKit Pay**
for AI service payments
Non-custodial payment routing across Ethereum, Base, Polygon, Arbitrum, Optimism, and SpaceKit. Atomic split between operator and treasury. Never holds user funds. Accepts USDC, USDT, DAI.

**ASTRA**
for operators
Native utility token, 2B hard cap. Staked to provide network resources; earned for the work done. Mechanical economics: no DAO yield, no farming.

**x402**
for machines
HTTP-native pay-per-call. Agent-to-agent transactions. Compatible with SpaceKit Pay routing through the same contract surface.

### Footer note

Treasury fee funds protocol development. Compare to Apple's 30%, YouTube's 45%, App Store's 30%. The 5% is structurally smaller and structurally consistent across operators.

---

## What changed from the previous version (v18) of slide 10

**Removed entirely:** aUSD stablecoin block. No aUSD references anywhere on the slide. No "fully collateralized" framing. No mention of 150% over-collateralization or vault contracts.

**Replaced with:** SpaceKit Pay block. Non-custodial payment routing on stablecoins issued by others (Circle, Tether, MakerDAO).

**Treasury rate framing changed:** The previous slide said "5% to network treasury, scales to max 15% with volume." The new slide says "flat 5%." Removing the "scales to 15%" framing because (a) it suggests the rate increases with success which sounds anti-user, (b) flat-rate framing is simpler and matches what the contract actually does, (c) the original "scales to 15%" implied a volume-based discount or tier structure that we are not implementing in v1.

**ASTRA framing slightly tightened:** "No DAO, no farming" became "No DAO yield, no farming" for clarity.

**x402 framing adjusted:** Now positioned as compatible with SpaceKit Pay rather than as a separate payment system. This is accurate: x402 specifies the HTTP layer, SpaceKit Pay provides the contract layer that fulfills x402 payment requirements.

---

## SWTCH Labs Products section revisions

The SWTCH Labs Products list appears in: the resume (under SWTCH Labs Products), the deck slide 11 (Team), the LinkedIn bio, the blog post, the investor page, and the website footer. Update everywhere.

### Old (aUSD-based) text

> **aUSD** — fully collateralized protocol stablecoin (1:1 USDC/USDT/DAI deposits, on-chain vault system, no unbacked mints). Settles AI/ML service fees alongside ASTRA and x402 USDC.

### New (SpaceKit Pay) text

> **SpaceKit Pay** — non-custodial payment routing for the AI economy. Atomic stablecoin splits across Ethereum, Base, Polygon, Arbitrum, Optimism, and SpaceKit. Operators register payout addresses; buyers pay directly to operators (95%) with a flat 5% treasury fee. Never holds user funds.

### LinkedIn bio specifically

The current LinkedIn bio section reads:

> aUSD — fully collateralized protocol stablecoin (1:1 USDC/USDT/DAI deposits, on-chain vault, no unbacked mints, no algorithmic-stability mechanisms). Settles AI/ML fees alongside ASTRA and x402 USDC.

Replace with:

> SpaceKit Pay — non-custodial payment routing for the AI economy. Atomic stablecoin splits (USDC, USDT, DAI) across EVM chains and SpaceKit. Operators receive 95%, flat 5% treasury fee, never custodial. Same protocol on every supported network.

This swap should not affect the LinkedIn bio character count materially. The new text is about the same length as the old.

### Blog post (what-we-built-blog.md) revisions

The blog post has three references to aUSD that need updating:

**Reference 1 in "Three products that compose" section:**

Old text:
> aUSD is the protocol stablecoin. Fully collateralized 1:1 against USDC, USDT, and DAI deposits in an on-chain vault system. No unbacked mints, no algorithmic-stability mechanisms, no Terra/Luna-style failure modes — just deposits backing tokens. Settles AI/ML service fees alongside the native ASTRA token and x402 USDC.

Replace with:
> SpaceKit Pay is the payment layer. Non-custodial routing of established stablecoins (USDC, USDT, DAI) across Ethereum, Base, Polygon, Arbitrum, Optimism, and SpaceKit. Buyers approve the payment, the protocol atomically splits 95/5 between operator and treasury, the contract never holds funds. Same protocol on every supported network; operators register their payout address on each network they want to accept payments on.

**Reference 2 in the "These compose" paragraph:**

Old text:
> These compose. A contract on SpaceKit can call a Growformer agent for inference, pay for the inference in aUSD, and emit a verifiable result to a counterparty — all in one transaction, all with one identity, all with post-quantum signatures end to end.

Replace with:
> These compose. A contract on SpaceKit can call a Growformer agent for inference, route payment in USDC through SpaceKit Pay (95% to the operator, 5% to treasury, all in the same transaction), and emit a verifiable result to a counterparty — all with one identity, all with post-quantum signatures end to end.

**Reference 3 wherever else aUSD appears:** Search for "aUSD" globally in the blog post and replace with appropriate SpaceKit Pay framing.

### Investor page revisions

The investor page references aUSD in the thesis paragraph and elsewhere. Replace with SpaceKit Pay framing throughout. The thesis becomes:

> Thesis: SWTCH Labs is shipping two composable products. SpaceKit is an AI-native Layer 1 with WASM smart contracts that call inference natively, with SpaceKit Pay providing built-in non-custodial payment routing across multiple networks for AI service settlement. Growformer is a foundation-model factory producing sub-100MB CPU agents with structural safety guarantees. Post-quantum cryptography across the stack. The network is live; the contract surface is open-source today.
