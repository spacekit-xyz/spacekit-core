# SpaceKit Content System — Completion Specification

**Status:** Sprint 1–2 shipped; Sprint 3 Theme A (production monetization) in progress
**Version:** 1.0
**Owner:** SWTCH Labs
**Date:** 2026
**Audience:** Storage node engineering, CLI engineering, smart contract integration

## Implementation status (2026-05)

| Work item | MVP state | Remaining |
|-----------|-----------|-----------|
| WI1 Fact-based `content view` + access evaluation | **Done** | HTTP API parity if needed |
| WI2 `PaymentRequired` grant lookup | **Partial** — local grants + entitlement + AppLicenseNFT opcodes | Per-content license contract deploy in prod |
| WI3 `subscribe` | **Partial** — payment verify + local grant | Channel `OP_PURCHASE` + live router auto-settle |
| WI4 Paid PPV flow | **Partial (Sprint 3)** — listener, compute→storage webhook (`POST /api/content/settlements`), `/v1/payments/verify` | Payment-router native push; no manual `record-payment` in prod |
| WI5 Publish listing | **Partial** — `OP_CREATE_LISTING` on publish when `SPACEKIT_ENTITLEMENT_CONTRACT_ID` set | Listing sync if price changes post-publish |
| Polish | **Done** — ASTRA, SPHINCS+ on publish, channel facts | Federation soak; live sign-off with real Pay |

**Modules:** `content_grants.rs`, `content_access.rs`, `content_entitlement.rs`, `content_payment.rs`, `content_settlement.rs`; CLI: `content_integration.rs`, `content_monetization.rs`. Tests: `content_sprint2` (15), `content_e2e_soak` (14). CLI soak: [content-monetization-soak.md](documentation/guides/content-monetization-soak.md).

**Production flow (paid PPV):** `publish --pricing pay_per_view` (+ entitlement env) → `content view --pay` or `content pay` → pay publisher → `content settle` (or `pay --tx-hash --amount`) → `OP_PURCHASE` + local grant → `view --output`.

**Dev fallback:** `record-payment` + `access --payment-ref` (no on-chain purchase).

---

This document specifies the completion of the existing SpaceKit content publishing system. The CLI surface (`spacekit content publish/subscribe/view/list-*`) is already shipped; the FactPackage scaffolding correctly records access policies and pricing metadata; encryption-on-publish for paid content works. What's missing is the wiring that turns the scaffold into an end-to-end paid content flow.

This is not a new system. It is the completion plan for the existing one, with explicit integration with the standard library contracts (AppStore, AppLicenseNFT, astra-entitlement-ledger, astra-payment-router, astra-escrow).

## 1. Current state assessment

### 1.1 What works today

The `spacekit content publish` command, when run with `--pricing pay_per_view --price 10`:

- Creates a `FactPackage` with proper metadata (title, description, content tags, MIME-based tags)
- Records `AccessPolicy::Conditional` with `PaymentRequired` condition, storing price and content_id in the condition parameters
- Encrypts the binary payload before persisting (via `should_encrypt_fact` returning true for paid content)
- Persists the fact via `FactStorageEngine::store_fact`
- Best-effort calls `register_content_with_governance` (typically fails silently if governance contract isn't deployed; publish succeeds with warning)

The data flow is correct. The fact lives in `fact_storage/` with the right access policy structure. The payload is encrypted. The metadata is queryable.

### 1.2 What doesn't work today

The completion gap is in three places:

**Gap A (resolved in MVP):** `content view` uses fact retrieval + `evaluate_content_access`. Paid content returns a payment-required message until a local grant exists (`content access` or on-chain in Sprint 2).

**Gap B (partial):** `PaymentRequired` checks `{data_dir}/content_grants/grants.json` (or `SPACEKIT_CONTENT_GRANTS_FILE`). On-chain AppLicenseNFT / entitlement-ledger lookup is not wired.

**Gap C (partial):** `subscribe` records a local channel subscription grant. No payment-router or entitlement-ledger yet.

### 1.3 Other inconsistencies to fix

Three smaller items worth correcting as part of completion:

- **Currency string discrepancy:** CLI help text says "ASTRA" but code writes "SWTCHX" as the currency parameter. Should be "ASTRA" consistently (or whatever the canonical token name is across the system).
- **Placeholder signatures:** CLI-published facts use placeholder SPHINCS+ signatures, which may be rejected by strict-mode storage nodes. Real signatures should be used.
- **CLI documentation drift:** The CLI says "free or pay_per_view" but the docs reference "subscription" and "mixed" as additional pricing models. The implementation handles `free` and `pay_per_view`; subscription and mixed need to be added.

## 2. Completion goals

The goal is to make `spacekit content publish` (and the related surface) function as documented end-to-end. After completion:

- A user can publish content with pricing free, pay-per-view, subscription, or mixed
- A consumer can view free content directly
- A consumer can view paid content after paying via SpaceKit Pay
- A consumer can subscribe to a channel and view all content within it
- Channel-level subscription state is tracked on-chain via the entitlement-ledger
- Per-content payment is tracked on-chain via AppLicenseNFT or similar
- The view path correctly evaluates access policy before unlocking content
- Strict-mode storage nodes accept the published facts (real signatures, not placeholders)

## 3. Architecture: how the completion plan integrates with existing contracts

The completion plan reuses existing standard library contracts. Each piece of the flow maps to a specific contract:

| Concern | Existing contract | Integration approach |
|---------|---------------------|----------------------|
| Channel registry | (none required; channels are FactPackage records) | Persist channel facts on `create-channel` |
| Per-content registration | AppStore (`app-store/appstore.rs`) | Optional: register paid content as "apps" for discoverability |
| License-to-view (pay-per-view) | AppLicenseNFT (`app-store/app_license_nft.rs`) | Mint per content_id on payment |
| Channel subscription | astra-entitlement-ledger (`marketplace/`) | Record subscription with expiration |
| Payment routing | astra-payment-router (`payments/`) | Route payments for pay-per-view and subscription |
| Payment escrow | astra-escrow (`payments/`) | Hold payment until grant is recorded |
| Access checks | astra-access-control + fact_storage policy evaluation | Evaluate against on-chain grant state |

The completion is about wiring these together, not building new contracts.

## 4. The four completion work items

### Work Item 1: Wire `content view` to fact-based retrieval with access policy evaluation

**Current:** `content view` calls `retrieve_file` (legacy storage API), bypassing access policy.

**Target:** `content view` should:

1. Look up the fact by content_id (`FactStorageEngine::retrieve_fact` or equivalent)
2. Evaluate the fact's access policy
3. For `Public` content: decrypt (if encrypted) and return
4. For `PaymentRequired` content: check if requesting DID has paid (via on-chain grant lookup); if yes, decrypt and return; if no, return payment-required response
5. For subscription-based content: check if requesting DID has active subscription to channel; if yes, decrypt and return; if no, return subscription-required response

**Specific changes:**

```rust
// In content view handler (full_client.rs::handle_content_command)

let fact = fact_storage.retrieve_fact(&content_id)?;
let access_decision = evaluate_content_access(&fact, &requester_did)?;

match access_decision {
    AccessDecision::Allowed => {
        let decrypted = decrypt_if_encrypted(&fact, &requester_keys)?;
        write_output(decrypted, &output_path)?;
    }
    AccessDecision::PaymentRequired { price, currency, payment_endpoint } => {
        return PaymentRequired { 
            price, currency, payment_endpoint,
            payment_metadata: build_payment_metadata(&fact, &requester_did),
        };
    }
    AccessDecision::SubscriptionRequired { channel_id, tier } => {
        return SubscriptionRequired { channel_id, tier };
    }
    AccessDecision::Denied { reason } => {
        return Denied { reason };
    }
}
```

**Estimated work:** 1 week (single engineer).

### Work Item 2: Implement `PaymentRequired` evaluation against on-chain grants

**Current:** `PaymentRequired` always returns `false` in `evaluate_access_conditions` (and in `access_policy.rs`).

**Target:** `PaymentRequired` should query on-chain state to determine if the requesting DID has paid for this specific content.

**Specific changes:**

```rust
// In fact_storage.rs::evaluate_access_conditions

ConditionType::PaymentRequired => {
    let content_id_param = condition.parameters.get("content_id")
        .ok_or(AccessError::MalformedPolicy)?;
    let requester_did = context.requester_did;
    
    // Check on-chain: does requester have a license/grant for this content?
    let grant_exists = on_chain_lookup::has_active_license_for_content(
        requester_did,
        &content_id_param,
    )?;
    
    grant_exists  // Allow if grant exists; deny otherwise
}
```

The `on_chain_lookup::has_active_license_for_content` function:
- Queries AppLicenseNFT for the content's per-content NFT contract
- Checks if requester_did owns a token from that contract
- If yes, checks the corresponding entitlement-ledger record for expiration
- Returns true if NFT exists and entitlement hasn't expired

**Estimated work:** 1.5 weeks (single engineer; requires smart contract integration).

### Work Item 3: Wire `subscribe` to entitlement-ledger via payment

**Current:** `subscribe` prints success without on-chain action.

**Target:** `subscribe` should:

1. Look up the channel fact (channel pricing, subscription terms)
2. If free channel: record entitlement directly via entitlement-ledger (no payment)
3. If paid channel: initiate payment via astra-payment-router → astra-escrow
4. On payment confirmation: record entitlement via entitlement-ledger with appropriate expiration
5. Optionally: notify channel publisher of new subscriber

**Specific flow:**

```bash
# User initiates subscription
spacekit content subscribe --channel <channel_did> [--tier <tier_name>]

# CLI behavior:
# 1. Look up channel fact
# 2. Determine pricing (from channel metadata)
# 3. If paid: 
#    - Request payment quote from astra-payment-router
#    - Initiate payment with appropriate metadata
#    - Wait for payment confirmation
# 4. On payment confirmation (or for free channels):
#    - Call astra-entitlement-ledger.grant_entitlement(
#        recipient_did: requester_did,
#        entitlement_type: "channel.subscription",
#        scope: channel_did,
#        expires_at: now + subscription_period,
#        payment_reference: payment_tx_hash,
#      )
# 5. Display confirmation with expiration
```

**Estimated work:** 2 weeks (single engineer; involves CLI, payment integration, contract calls).

### Work Item 4: Add `content access` and `content renew` commands

The existing surface has `subscribe` (for channel subscriptions) and `publish/view` (for individual content). The previous Licensed Content spec proposed `content access` as a unified verb for one-time purchases (pay-per-view) and renewable subscriptions. This extends the surface naturally:

**New commands:**

```bash
# Pay for one-time access to a specific piece of content
spacekit content access --content-id <id> [--tier <name>]

# Renew an expiring subscription or content access
spacekit content renew --content-id <id>       # for content
spacekit content renew --channel <channel_did> # for channel subscription

# List all access the user currently has
spacekit content list-access
```

These integrate the same way `subscribe` does (payment → entitlement-ledger grant) but with content-scoped rather than channel-scoped state.

**Estimated work:** 1.5 weeks (single engineer; mostly CLI work with reuse of subscription infrastructure).

### Total estimated work

Approximately **6 weeks** of focused engineering for the four work items. This is the completion of the existing scaffold, not a from-scratch implementation. Phase 1 of the previous Licensed Content spec collapses into this completion plan.

## 5. The fixed CLI surface (after completion)

After the four work items are complete:

```bash
# Publishing (existing surface, fix currency string)
spacekit content publish --channel <CHANNEL> --file <FILE> --title <TITLE> \
  [--description <DESCRIPTION>] \
  [--pricing free|pay_per_view|subscription|mixed] \
  [--price <PRICE>]    # in ASTRA (not SWTCHX)

# Subscription (currently stub; complete in Work Item 3)
spacekit content subscribe --channel <CHANNEL>

# One-time content access (new in Work Item 4)
spacekit content access --content-id <ID> [--tier <NAME>]

# Renewal (new in Work Item 4)
spacekit content renew --content-id <ID>
spacekit content renew --channel <CHANNEL_DID>

# View/download (existing; fix to use fact-based retrieval in Work Item 1)
spacekit content view --content-id <ID> [--output <PATH>]

# Lists (existing)
spacekit content list-channels [--detailed]
spacekit content list-content --channel <CHANNEL> [--limit <N>]
spacekit content list-access      # new: list user's current access

# Channels (existing; persist channel fact in Work Item 5 if needed)
spacekit content create-channel --name <NAME> [--pricing <PRICING>]
```

The existing surface is preserved. New commands extend it. No breaking changes for users running existing commands.

## 6. Channel subscriptions vs. per-content access

The system supports both models cleanly:

**Per-content access (`pay_per_view`):**

- User pays once to view a specific piece of content
- Recorded as an AppLicenseNFT or similar on-chain grant scoped to that specific content_id
- Permanent access (no expiration) by default; can have time-scoped variants
- Suitable for: individual movie rentals, single-document purchases, one-off agent training runs

**Channel subscription:**

- User pays per period (monthly, annually) to access all content in a channel
- Recorded in entitlement-ledger with expiration tied to the subscription period
- Renewable via `renew`
- Suitable for: ongoing creator subscriptions (Patreon-style), streaming services, periodic dataset access

**Mixed (channel with some free + some paid content):**

- Channel has subscription tier for some content
- Additional pay-per-view items within the channel that aren't covered by the subscription
- Common pattern for tiered streaming services

The access check during `content view` examines both:
1. Does the requester have an active subscription to the content's channel that covers this content?
2. If not, does the requester have a per-content grant for this specific content_id?
3. If neither, deny with appropriate "payment required" or "subscription required" response.

## 7. Encryption and signature handling

The current implementation correctly encrypts paid content before persistence (via `should_encrypt_fact`). This needs to remain true after completion.

After completion, when a user gains access via either payment or subscription:

- The storage operator confirms the on-chain grant exists
- The operator uses its private key to decrypt the symmetric key from the fact's envelope
- The operator re-encrypts the symmetric key for the requester's Kyber public key
- The requester downloads the envelope with their recipient slot
- The requester decrypts locally using their Kyber private key

This is the same envelope-encryption pattern from the previous Licensed Content spec, integrated with the existing fact storage path.

For signatures, the placeholder SPHINCS+ signatures in CLI-published facts need to be replaced with real signatures (signed by the publisher's DID). Strict-mode storage nodes already require real signatures; this brings the CLI path into consistency.

## 8. Payment metadata corrections

Three specific data corrections in `content_integration.rs`:

**Fix 1: Currency string.**

```rust
// Current (incorrect)
params.insert("currency".to_string(), "SWTCHX".to_string());

// Corrected
params.insert("currency".to_string(), "ASTRA".to_string());
```

**Fix 2: Add network field.**

```rust
// Add to PaymentRequired parameters
params.insert("on_network".to_string(), "spacekit".to_string());
// (or "ethereum", "base", etc., for cross-network pricing)
```

**Fix 3: Add tier/license_type metadata.**

```rust
// For pay_per_view, the license is typically Personal
params.insert("license_type".to_string(), "Personal".to_string());

// For higher tiers (commercial, enterprise), would be specified at publish time
```

These corrections make the on-chain payment lookup unambiguous about what was being purchased.

## 9. The minimum viable completion (MVP)

If full completion is too much for the current sprint, here's the minimum viable subset that delivers value:

**MVP (3 weeks):**

1. Fix `content view` to use fact-based retrieval (Work Item 1, simplified — just the public/free path correctly)
2. Implement `PaymentRequired` evaluation against on-chain grants (Work Item 2)
3. Wire `subscribe` to entitlement-ledger for free channels first; paid channels deferred

This delivers: free content access works correctly; paid content correctly denies until payment lookup is wired; subscription scaffold exists for free channels. The remaining work (paid subscriptions, `content access`, `content renew`) layers on top.

**Phase 2 (additional 3 weeks):**

4. Complete paid subscription via SpaceKit Pay integration
5. Add `content access` for pay-per-view via SpaceKit Pay
6. Add `content renew` for renewable subscriptions
7. Currency string fix and other corrections

After MVP + Phase 2, the system delivers end-to-end paid content flow.

## 10. Open questions and decisions for the team

A few decisions worth surfacing before implementation:

**Decision 1: AppStore registration for paid content — required or optional?**

For pay-per-view content, registration with AppStore would enable discoverability ("browse all content in the marketplace"). But it adds complexity for publishers who just want to share a single file.

Recommendation: optional. Publishers can choose to register; default is unregistered (channel-scoped discoverability via `list-content` queries).

**Decision 2: Channel ownership and management — on-chain or in-fact?**

Channels are currently created as FactPackages but their ownership/permissions structure isn't fully clear. Should:
- Channel ownership be recorded on-chain (via AppLicenseNFT-like contract)?
- Or remain in the FactPackage with publisher_did as canonical owner?

Recommendation: in-fact for v1 (simpler), with optional AppStore registration for channels that want to be discoverable in the marketplace.

**Decision 3: Currency display vs. underlying token.**

The CLI currently mismatches (says "ASTRA" but writes "SWTCHX"). The underlying token name should be canonical. Options:
- Both ASTRA and SWTCHX are accepted (multi-currency)
- ASTRA is the canonical name; SWTCHX is a historical artifact to remove
- ASTRA is the network-native fee token; SWTCHX is a separate stable token

Recommendation: Resolve this question explicitly. The CLI and code should be consistent.

**Decision 4: Subscription cancellation.**

Users should be able to cancel subscriptions. The mechanism:
- On-chain transaction to entitlement-ledger marking the entitlement as cancelled
- Cancelled subscriptions still grant access until current period expiration (paid for through then)
- Auto-renewal stops after cancellation

Recommendation: implement as part of Work Item 3.

**Decision 5: Free vs. quota-limited tiers.**

The MVP treats free as "no payment required, no expiration." But many publishers want quota-limited free tiers (e.g., 10 free views per month, then pay). Adding quota tracking adds complexity.

Recommendation: defer quota tracking to a follow-on phase. v1 supports binary free/paid.

## 11. Implementation timeline

Combined timeline for the completion work:

**Sprint 1 (Weeks 1-3): MVP completion**
- Fix `content view` for public content
- Implement `PaymentRequired` evaluation
- Free channel subscription scaffold

**Sprint 2 (Weeks 4-6): Paid flow completion**
- Paid subscription via SpaceKit Pay
- `content access` for pay-per-view
- `content renew` for renewable subscriptions

**Sprint 3 (Theme A): Production-ready monetization**
- Live SpaceKit Pay: `POST /v1/payments/verify` from `content settle` / `content pay --tx-hash`
- `OP_PURCHASE` via `call_contract_raw` + `content purchase` / auto on settle
- Pending purchases + settlement inbox (`content_settlement.rs`)
- **Done:** astra-escrow (`content_escrow.rs`) — OP_CREATE on pay quote, OP_RELEASE on grant, OP_REFUND on `complete_pay_flow` failure + local `refund_on_grant_failure`
- **Dev soak:** CLI `./scripts/content-monetization-soak.sh dev` passes (5/5) — reference [scripts/README.md](scripts/README.md)
- **Remaining:** operator live soak sign-off (`content-monetization-live-deploy.md` + `./scripts/content-monetization-soak.sh live`)

Total: about 8 weeks. After completion, the existing CLI surface delivers end-to-end paid content publishing and consumption.

## 12. Coordination with other work

This completion work coordinates with:

**Phase 2 storage node enhancement plan.** The completion delivers a critical capability for the Phase 2 launch — paid content access. Phase 2 announcement can include "paid content distribution operational" as one of the capabilities.

**Growformer launch.** Growformer is **not** distributed as downloadable content. See [GROWFORMER_SPEC.md](GROWFORMER_SPEC.md) (library embedded in the `spacekit` CLI, feature entitlements). General publish/view/soak flows for other content types are in [CONTENT_PUBLISHING.md](CONTENT_PUBLISHING.md).

**SpaceKit Pay integration.** The completion is one of the larger SpaceKit Pay integration projects. Successful completion validates the SpaceKit Pay routing for general content purchase flows.

**Security audit.** The audit currently in progress should review the completion as part of the launch readiness. The access policy evaluation, payment verification, and on-chain grant logic are security-critical paths.

## 13. What this spec is NOT

For clarity:

- **NOT a redesign.** The existing CLI surface is preserved; existing commands continue to work.
- **NOT a parallel system.** The completion uses existing standard library contracts (AppStore, AppLicenseNFT, entitlement-ledger).
- **NOT a documentation-only effort.** Real engineering work is required; the spec quantifies it.
- **NOT a discussion of pricing.** The pricing model is publisher-determined; the spec covers the mechanics of how pricing is enforced.
- **NOT a replacement for individual security audit.** Implementation correctness needs to be verified independently.

## 14. Sign-off

This completion specification is the canonical reference for finishing the SpaceKit content system. The four work items, totaling about 6-8 weeks of focused engineering, deliver end-to-end paid content publishing and consumption.

The work integrates with existing standard library contracts. No new contracts are required. The CLI surface is preserved with minor extensions. The completion fits within the timeline of the Phase 2 storage node enhancement plan.

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
