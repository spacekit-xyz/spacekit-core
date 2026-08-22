# SpaceKit Content Publishing Guide

**Status:** Active (dev monetization soak green; live Pay Phase 4 in progress)
**Version:** 1.0
**Owner:** SWTCH Labs
**Date:** 2026
**Audience:** CLI engineering, storage-node engineering, content publishers, technical due diligence

This document describes how to **publish downloadable content** through the SpaceKit content system: channels, FactPackages, access policies, grants, settlement, and consumer `content view`.

**Growformer is not published through this guide.** SWTCH Labs IP for growformer uses the **library-embedded** model in [GROWFORMER_SPEC.md](GROWFORMER_SPEC.md) (compiled into the `spacekit` CLI, entitlement at function entry). This guide applies to videos, datasets, third-party agents, binaries, and other envelope-backed content.

**Related specs:**

| Document | Scope |
|----------|--------|
| [CONTENT-SYSTEM-SPEC.md](CONTENT-SYSTEM-SPEC.md) | System completion, contracts, storage modules |
| [GROWFORMER_SPEC.md](GROWFORMER_SPEC.md) | Growformer library-in-CLI distribution (supersedes binary publish for growformer) |
| [documentation/guides/content-monetization-soak.md](documentation/guides/content-monetization-soak.md) | Soak runbook |
| [scripts/README.md](scripts/README.md) | Soak scripts and CI |

---

## Gate before production publish

Prove the paid path on a local stack:

```bash
spacekit network up
spacekit content soak dev
# expect: Soak summary: 5 passed, 0 failed
```

Also: `./spacekit-cli/scripts/content-monetization-soak.sh dev` or `./spacekit-storage-node/scripts/content-monetization-soak.sh dev` from repo root.

---

## CLI reality (engineering, 2026-05)

| Topic | Documented intent | Current CLI |
|-------|-------------------|-------------|
| **Tier flag** | `content access --tier free\|commercial` | **Not implemented.** Use separate publications: `--pricing free` vs `--pricing pay_per_view --price N`, or two `content_id`s. |
| **Channel** | Publisher channel | `spacekit content create-channel` |
| **Paid settlement** | Pay → settle → view | **Dev:** `record-payment` + `listen-settlements` + `pay --pending-id … --await-settlement`. **Live:** SpaceKit Pay router (Phase 4). |
| **Multi-tier on one fact** | One publication, many tiers | **One pricing model per publish today.** Prefer two publishes until tier metadata exists. |
| **Encryption at rest** | Envelope on publish | Paid facts use access policy + grants; full at-rest KEM for `Conditional` policies is deferred (tags carry `pricing:` / `price:`). |
| **Publisher = buyer in tests** | Soak realism | Soak often uses one `spacekit init` DID; use **two identities** for commercial sign-off. |
| **Install record** | After `content view` | DB collection `content_installs` under storage-node data dir; optional materialized file under `content/materialized/`. |
| **Large binaries** | Soak / diligence | Publish real artifacts (e.g. ~10MB) to validate fact storage; not required for every content type. |

---

## 1. Goals

Demonstrate and operate **general** commercial content distribution:

- Publishers create channels and publish files as FactPackages with pricing metadata.
- Consumers obtain access via free policy, pay-per-view, or subscription (as implemented).
- Storage-node persists facts, grants, settlements, and optional materialized copies.
- Monetization soak and CI prove the dev payment path end-to-end.

Subsequent publications (agents, datasets, media) follow the same pattern unless they are **licensed features** (see `spacekit:licensed_feature:v1` in GROWFORMER_SPEC.md).

---

## 2. Publisher and consumer flows

### Free content

```bash
spacekit content create-channel --name "My Publisher" --pricing mixed

spacekit content publish \
  --channel "<channel_did>" \
  --file ./artifact.bin \
  --title "My Asset" \
  --description "…" \
  --pricing free

# Save content_id from output (64-hex)

spacekit content view --content-id <content_id>
# Default: materialized under <storage-data-dir>/content/materialized/…
# Install recorded in DB (content_installs)
```

### Paid content (pay-per-view, dev)

```bash
spacekit content publish \
  --channel "<channel_did>" \
  --file ./premium.bin \
  --title "Premium Asset" \
  --pricing pay_per_view \
  --price 10

spacekit content pay --content-id <content_id>
spacekit content record-payment --reference <tx> --recipient <publisher> --scope content:<id> --amount 10
spacekit content listen-settlements --once
spacekit content pay --pending-id <pending_id> --await-settlement

spacekit content view --content-id <content_id>
```

See [documentation/guides/content-monetization-soak.md](documentation/guides/content-monetization-soak.md) for the canonical H2 chain.

### Materialized files and security

- Bytes materialized by `content view` live under the **storage-node data directory** (e.g. `~/.spacekit/storage` or `~/.spacekit/data/storage` when using `network up`).
- **Entitlement gates SpaceKit CLI commands**; it does not hide files from the OS user. Any local process with your user permissions can read a materialized path.
- Use `--output <path>` only when an explicit export path is needed.

---

## 3. Content-type adapters (optional pattern)

For content that needs a **type-specific runner** after access (third-party CLI, custom agent binary):

1. **Verify access** — `content_grants` + `evaluate_content_access` (and on-chain entitlement when configured).
2. **Resolve artifact** — `content view` and/or `content_installs` DB record (`storage_ref` or materialized path).
3. **Invoke** — spawn the published binary or call an app-specific handler.

Growformer **does not** use this pattern for distribution; see [GROWFORMER_SPEC.md](GROWFORMER_SPEC.md).

---

## 4. Implementation checklist (publishers)

### Step 1 — Channel and free publish

```bash
spacekit content soak dev   # gate

spacekit content create-channel \
  --name "Publisher Name" \
  --description "…" \
  --pricing mixed

spacekit content publish \
  --channel "<channel_did>" \
  --file <path> \
  --title "…" \
  --description "…" \
  --pricing free
```

Validate: `content view --content-id <id>` returns payload; `content installs` lists the install.

### Step 2 — Commercial tier

Second publish on the same channel (recommended until `--tier` exists):

```bash
spacekit content publish \
  --channel "<channel_did>" \
  --file <path> \
  --title "… (Commercial)" \
  --pricing pay_per_view \
  --price <ASTRA>
```

Run dev settlement chain or live Pay per [content-monetization-live-deploy.md](documentation/guides/content-monetization-live-deploy.md).

### Step 3 — Docs and soak

- Update `spacekit-cli/COMMANDS.md` for any new flags.
- Keep `content-monetization-soak` green in CI (`.github/workflows/content-monetization.yml`).
- Add type-specific soak scripts only when a new content class needs regression coverage.

---

## 5. Pricing and policy (leadership)

Decisions before broad paid launch:

- **Free tier:** open vs allow-list vs quota (recommendation: open for initial adoption window).
- **Commercial price model:** PPV vs subscription vs hybrid.
- **Currency:** ASTRA + stablecoins via SpaceKit Pay where enabled.
- **Refunds:** e.g. 14-day window for commercial PPV; document in terms of service.

---

## 6. Launch readiness (downloadable content)

- [ ] `content soak dev` passes (5/5)
- [ ] Free publish → view → non-empty payload
- [ ] Paid publish → dev settlement → view after grant
- [ ] Live Pay path documented and signed off (when required)
- [ ] `CONTENT-SYSTEM-SPEC.md` work items tracked for production gaps
- [ ] Publisher onboarding doc / website quickstart aligned with real commands

Growformer launch criteria are in [GROWFORMER_SPEC.md](GROWFORMER_SPEC.md) §12–13.

---

## 7. Out of scope

- **Growformer binary publication** — superseded by GROWFORMER_SPEC.md
- **Licensed features in CLI** (`spacekit:licensed_feature:v1`) — GROWFORMER_SPEC.md Phase 3
- **Full envelope encryption at rest** for all policy types — CONTENT-SYSTEM-SPEC.md
- **Third-party publisher onboarding** — separate product doc

---

## 8. Sign-off

This guide is the canonical reference for **publishing downloadable content** on SpaceKit. Growformer distribution is defined only in [GROWFORMER_SPEC.md](GROWFORMER_SPEC.md).

Astor Rivera  
Founder & CTO, SWTCH Labs  
astor@swtch.ai
