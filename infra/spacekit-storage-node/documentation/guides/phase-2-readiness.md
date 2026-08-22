# Phase 2 multi-tenant readiness assessment

Snapshot after Stream A soak (hybrid), federation handoff, operator discovery, and
DID-signed migration (phases 1–6).

## Executive summary

**Auth substrate for multi-tenant SaaS is ready for staged rollout.** Hybrid mode is
soak-validated on a production-shaped node (builtin storage-node + upload tokens).
Federation **migration** works with layered auth: HMAC handoff (layer 1) plus optional
SPHINCS+ migration manifests (layer 2). **Discovery** has publish (`spacekit operator
publish`) and read (`GET /api/operators/self`). **Strict** cutover and **workspace-owner
counter-sign** remain before calling Phase 2 “fully launched” for untrusted federated
operators with settlement.

## Done (ship-quality)

| Area | Evidence |
|------|----------|
| Blob/fact auth modes | `access_policy.rs`, env + `network.toml` `blob_fact_auth` |
| Upload tokens | `POST /api/upload-tokens`, hybrid soak PASS |
| Workspaces | `/api/workspaces/*`, quotas on sandboxes, CLI |
| Sandbox + RepoTree | `TransactionModification::RepoTree`, real transactions default on |
| Federation handoff | export/import, blob replicate, HMAC `handoff_signature` |
| DID-signed migration | `src/migration.rs`, export/import, audit facts, CLI verify/sign |
| Operator manifest | `spacekit:operator:v1`, CLI publish, `GET /api/operators/self` |
| Operator metrics | `/api/agentic/health` (`migration_signing_configured`) |
| MCP agent tools | workspaces, upload tokens, export/import, replicate |

## Partial (usable with caveats)

| Area | Gap |
|------|-----|
| RoleBased / Conditional policies | Implemented in `access_policy`; not soak-tested across all fact types |
| Strict mode | `strict_auth_soak` exists; not yet run on all production nodes |
| Workspace ↔ operator policy | Documented (Stream D); not enforced at create time |
| Federation discovery at scale | No network index; pull URL + `/api/operators/self` only |
| Cross-operator collaboration | Collaborator list only; no dual-node write routing |
| Migration owner counter-sign | Spec’d; requires wallet/UI (bilateral scenario) |
| v1↔v2 matrix automation | Manual runbook; not full CI matrix yet |

## Not started (Phase 2 launch blockers for *open* federated SaaS)

| Area | Notes |
|------|-------|
| Operator reputation / index | Stream E |
| Federated search | Stream E |
| SpaceKit Pay cross-operator settlement | Stream E + Pay |
| Automated two-node CI handoff | Manual runbook in [`federation-testing.md`](./federation-testing.md) |

## Recommended launch sequence

1. **Hybrid in production** — monitor 24–72h after soak.
2. **Strict soak** — `strict_auth_soak` on one node; then `blob_fact_auth = "strict"` for new tenants.
3. **`spacekit operator publish`** — verify `operator show` / `curl /api/operators/self` includes `sphincs_public_key_hex`.
4. **Two-node handoff** — shared `SPACEKIT_HANDOFF_SECRET`; verify `migration_manifest` + `migration verify`.
5. **Strict + `SPACEKIT_REQUIRE_MIGRATION_ATTESTATION`** — before onboarding unaudited third-party operators or settlement.

## What “Phase 2 launch” can mean now

| Claim | Defensible? |
|-------|-------------|
| Multi-tenant workspaces with DID auth and sandboxes | **Yes** (single operator) |
| Browser uploads via upload tokens | **Yes** (hybrid/strict with tokens) |
| Operator-hosted migration between nodes | **Yes** (HMAC + optional DID v2) |
| Audit-grade migration records | **Yes** (when v2 + counter-sign + migration record fact) |
| Open federation marketplace | **No** (no index, no reputation, no settlement) |

## Related

- [`federation-design.md`](./federation-design.md)
- [`did-signed-migration.md`](./did-signed-migration.md)
- [`blob-fact-auth-staging.md`](./blob-fact-auth-staging.md)
- [`operator-discovery.md`](./operator-discovery.md)
