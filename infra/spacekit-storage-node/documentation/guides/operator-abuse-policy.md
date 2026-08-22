# Operator abuse handling (Stream D)

Policy and operations framework for self-hosted and federated storage operators.
Engineering hooks (workspace export/import, handoff signatures, reputation) build on
decisions documented here.

## Three layers

1. **Per-operator policy** — what content and behavior you allow on your node.
2. **User discoverability** — users can read your policy before creating workspaces.
3. **Federation handoff** — users can migrate workspaces to operators whose policies they prefer.

## Operator policy document (required for federation)

Publish a **content policy** URI referenced from your operator manifest
(`content_policy_uri` in `spacekit:operator:v1`). Recommended sections:

| Section | Purpose |
|---------|---------|
| Prohibited content | Illegal material, malware distribution, harassment, etc. |
| Reporting | How users and third parties submit abuse reports |
| Due process | Review timeline, appeals, mistaken takedown remediation |
| Legal requests | DMCA / law-enforcement contact and response SLA |
| Data retention | Logs, blob retention after takedown, backup handling |
| Workspace compatibility | How workspace-level rules interact with operator rules |

Sign the policy document (or a hash thereof) with the **operator DID** so clients can
pin the version they accepted at workspace creation time.

## Workspace ↔ operator compatibility (planned)

When creating a workspace, the node will verify that `WorkspaceContent` rules do not
conflict with the operator policy (e.g. a workspace tagged `allows-user-uploads:unmoderated`
on an operator that prohibits unmoderated public uploads). Today this is **manual**:
operators review workspace defaults and quotas at creation.

## Migration and disputes

| Scenario | Tooling today | Target |
|----------|---------------|--------|
| User disagrees with operator policy | `workspace export` + `import` on another node | Stream D migration billing optional |
| Operator removes content | Manual CAS/fact deletion + runbook | Automated takedown journal |
| Cross-operator dispute | N/A | Stream E reputation + arbitration policy |

Use [`federation-workspace-handoff.md`](./federation-workspace-handoff.md) for technical migration steps.

## Reputation (Stream E — not implemented)

Federated reputation will surface:

- Takedown response time and appeal outcomes
- Policy change frequency and user notification
- Handoff success rate (signed exports accepted by peers)

## Decisions the team must commit to

These are **policy choices**, not code (see ENHANCEMENTS.md Stream D):

- SWTCH Labs operator prohibitions (if any)
- Minimum due-process bar for federation membership
- Escalation path (operator-only vs network arbitration)
- Consequences for operators who violate federation standards
- Federation-wide vs per-operator law-enforcement posture

Document answers in your operator policy URI before advertising federation membership.

## Related

- [`operator-discovery.md`](./operator-discovery.md) — manifest fact schema
- [`federation-roadmap.md`](./federation-roadmap.md) — Stream E engineering timeline
- [`blob-fact-auth-staging.md`](./blob-fact-auth-staging.md) — auth rollout
