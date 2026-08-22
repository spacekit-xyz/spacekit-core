# SpaceKit Storage Node Enhancement Plan

**Status:** Strategic and engineering plan
**Version:** 1.0
**Owner:** SWTCH Labs
**Date:** 2026
**Audience:** SWTCH Labs internal team, partners, investors with technical depth

This document is the plan for elevating the SpaceKit storage node from
a credible substrate (which it is today) into a production-grade
workspace hosting platform where humans and AI agents work together as
first-class collaborators. It combines strategic positioning with
engineering work streams because the two are tightly coupled.

The plan locks four positioning decisions, identifies six engineering
work streams, sequences them across three deployment phases, and names
the open decisions the team commits to before execution begins.

## 1. The strategic frame

### 1.1 The unifying primitive: FactPackage

Everything stored on the SpaceKit network is a FactPackage. Code
commits are facts. Agent deployments are facts. Storage envelope
receipts are facts. Reputation observations are facts. Identity
attestations are facts. The same Rust type carries them all.

This unification matters strategically because it means the same
primitives — quantum-safe signature, verification proof, access
policy, encryption, citation graph, confidence score — apply
consistently across everything. A code commit and an agent's
output and a deployment receipt all share the same authentication
guarantees. There is no "code path" versus "agent path" with
different security properties; there is one path with one
verification model.

Most platforms have different types for different things. GitHub
has repos, releases, issues, comments — each with different
access rules and authentication paths. Slack has channels,
messages, threads, files — each with different policies. ChatGPT
has conversations, messages, projects, files — each evolving
separately. The lack of unification is what creates the gaps
where breaches happen.

SpaceKit has one type. Every storage operation is a FactPackage
with a signature, a verification proof, an access policy, an
encryption envelope (optional), and a citation graph. This is
the structural property that makes "audit everything, verify
everything, encrypt everything where appropriate" actually
achievable rather than aspirational.

### 1.2 The positioning: agent-first workspaces

The workspace category SpaceKit is targeting is not "GitHub with
AI features." It is workspaces where humans and AI agents both
work on the same code as first-class collaborators, each with
their own DID, their own signed commits, their own audit trail,
their own verifiable contribution history.

This is a different product than:

- **GitHub** (and GitLab, Bitbucket, etc.). Code hosting with
  humans as users; AI tools (Copilot, autocomplete) are features
  bolted on. Agents have no first-class identity.

- **Cursor** (and Replit, Codeium, etc.). IDEs with AI features.
  Agents operate as plugins inside the developer's session, not
  as independent collaborators with their own identity and audit
  trail.

- **ChatGPT / Claude** (and similar chat-based agent tools).
  Agents work in conversations or projects with limited
  collaborative semantics. No commit history, no verifiable
  contribution graph, limited or no integration with code
  hosting workflows.

- **Radicle** (and earlier decentralized git attempts).
  Decentralized code hosting that hasn't found product-market
  fit because "decentralized" alone doesn't solve the problems
  developers actually have. SpaceKit's pitch is not
  "decentralized GitHub" — it's "agent-native workspaces with
  cryptographic verifiability."

What SpaceKit can claim that none of these can: **agents and
humans share the same workspace primitive, with the same
signature scheme (SPHINCS+), the same identity layer (DIDs), the
same access control (per-DID policies on FactPackages), and the
same audit trail (signed commits and signed facts).**

An agent's pull request is signed by the agent's DID. The
contributor graph shows the agent's contributions alongside
human contributors. The reviewer can verify that the commit
came from a specific agent acting under a specific operator's
authority. If the agent makes a mistake, the audit trail shows
exactly what it did and when.

This is structurally different from "GitHub with AI plugins."
And the product category — workspaces where humans and AI agents
collaborate with cryptographic verifiability — has no clear
incumbent.

### 1.3 The threat model the storage node addresses

GitHub-class hosting has been breached in several specific ways
in recent years. The storage node's design addresses these
explicitly:

**Supply chain attacks via package compromise.** Attackers gain
write access to popular packages and ship malicious versions.
SpaceKit's countermeasure: every commit is signed by the
contributor's DID using post-quantum signatures (SPHINCS+). A
malicious commit signed by an attacker's DID will be visibly
not-from-the-maintainer; a malicious commit attempting to forge
the maintainer's signature requires breaking SPHINCS+, which is
computationally infeasible.

**Insider threats at the hosting provider.** GitHub employees
(or attackers who compromise GitHub's infrastructure) could in
principle access private repos. SpaceKit's countermeasure:
zero-knowledge envelope encryption. The storage node holds
ciphertext only; plaintext is decryptable only by recipients
whose DIDs are listed in the FactPackage's encryption metadata.
Even if a SpaceKit operator is fully compromised, encrypted
workspaces remain confidential.

**Social engineering of maintainer accounts.** Attackers
compromise a maintainer's GitHub credentials and push malicious
code. SpaceKit's countermeasure: DID identity is rooted in
hardware-backed key material (or whatever the operator's
identity provider provides), not in password-recoverable
account credentials. Social engineering attacks on password
recovery have no equivalent against DID-rooted identity.

**Account-level censorship or de-platforming.** GitHub bans
accounts (sometimes correctly, sometimes not), and the account's
content becomes inaccessible. SpaceKit's countermeasure:
federated hosting means a workspace can be hosted on any
operator's storage node; if one operator de-platforms a user,
the user moves their workspace to a different operator. (This
is post-multi-tenant work; not Phase 1 capability but a
designed outcome.)

**Account-level data loss.** GitHub account gets deleted; the
data is gone. SpaceKit's countermeasure: content-addressable
storage with durability proofs and operator earnings tied to
storage durability. The same FactPackage can be replicated
across operators; loss of one operator doesn't destroy the
workspace.

The plan is not to claim SpaceKit fixes all of these
immediately. Phase 1 (self-hosted) addresses several of them
directly. Phase 2 (multi-tenant) addresses more. Phase 3
(federation) closes the rest. The plan sequences the work
accordingly.

## 2. Current state assessment

### 2.1 What works today

The storage node has substantial capabilities already in
production:

- FactPackage primitive: unified type for all stored content
- DID-keyed identity layer (W3C-style DIDs with SPHINCS+
  signatures)
- Content-addressable blob storage (CAS) with deduplication
- Verifiable repo commits (`spacekit:repo:commit:v1` fact
  schema)
- Three-way merge via `spacekit-diff` (Myers diff, diff3
  three-way merge)
- Strongly-typed repo APIs via `spacekit-repo` crate
- `spacekit repo` CLI for command-line workflows
- Server-side sandboxes with transactional commit/discard
  semantics
- Envelope encryption (PQ algorithms: Kyber for KEM)
- MCP integration for agent workflows
- Storage durability proofs and capacity tracking
- Operator earning via ASTRA emission for storage service

### 2.2 Gaps — status (2026)

The three engineering gaps below are **implemented at the storage-node
layer**. Remaining work is mostly operator rollout (strict auth default),
CLI polish, and federation (Phase 3).

**Gap 1 — Blob and fact authentication (Stream A).** **Shipped (opt-in).**
`src/access_policy.rs` evaluates `AccessPolicy` on fact reads (including
`RoleBased` with optional registry and `Conditional` time windows); `POST /facts`
requires `Authorization: DID` matching the package author in `strict` / `hybrid`
modes; `GET /facts/{id}` enforces policy; blob reads in `strict` mode check
`blob_refs/` manifests built from repo commit trees. **Upload tokens**
(`src/upload_token.rs`, `POST /api/upload-tokens`) mint short-lived
`Authorization: UploadToken` credentials for `put_blob` / `get_blob` / `put_fact`.
Configure via `ServerConfig.blob_fact_auth_mode` or env `SPACEKIT_BLOB_FACT_AUTH`
(`permissive` | `strict` | `hybrid`). Default remains `permissive` for
self-hosted compatibility. In **`strict`** mode, `POST /facts` also requires a
verifiable non-empty SPHINCS+ signature (node built with `quantum` feature).

**Gap 2 — Sandbox-to-repo integration (Stream B).** **Shipped.**
`TransactionModification::RepoTree` records a `spacekit:repo:commit:v1` fact plus
`repos/{name}/refs/heads/{branch}` in one transaction apply (sandbox commit
replays the journal). Wired when `FacadeConfig.cas_data_dir` is set.
`POST /api/transactions/{id}/modifications` + optional `X-Sandbox-Id` still
mirrors non-repo mods into the sandbox journal.

**Gap 3 — Workspace as first-class object (Stream C).** **Shipped (v1).**
`spacekit:workspace:v1` facts (`src/workspace.rs`) with HTTP
`POST/GET /api/workspaces` and `GET /api/workspaces?owner_did=...` (agentic
routes). Index rows live in `workspace_index` documents; fact bodies under
`facts/` when `cas_data_dir` is configured. CLI: `spacekit workspace
create/show/list`. Guide: `documentation/guides/workspaces.md`.
Sandboxes accept optional `workspace_id`: create path enforces collaborator ACL,
caps `max_bytes_written` to `max_sandbox_bytes`, and rejects when aggregate
`bytes_written` reaches `max_storage_bytes` (`Facade::create_sandbox`).

Integration tests: `tests/enhancements_gaps.rs`.

### 2.3 What's beyond gaps (production readiness)

Three additional work streams are needed to ship the storage
node as a production product, beyond closing the three gaps:

**Operator-grade abuse handling.** When SpaceKit is positioned
as a GitHub alternative, the question "what happens when someone
hosts illegal content" becomes real. The infrastructure needs to
support content policy decisions: how operators define their
policies, how users discover operator policies, how disputes are
handled, how violations are flagged. This is policy/operations
work, not just engineering.

**Federation between operators.** If multiple operators host
storage nodes, users need to be able to discover and migrate
between them. Federation protocols (workspace handoff,
cross-operator search, operator reputation) are required for the
network-of-operators model.

**Production hardening.** Things that go from "interesting
infrastructure" to "production database": comprehensive
monitoring, alerting, backup/restore procedures, operational
runbooks, performance characterization under load, the
`SPACEKIT_ENABLE_REAL_TRANSACTIONS` defaults to **true** (opt out with
`false`); stub finalize counters remain on `/api/agentic/health` for regression
detection. Incident
response procedures.

## 3. Six work streams

The plan organizes the work into six streams that can be
executed in parallel where dependencies allow:

**Stream A — Blob and fact authentication** (engineering)
**Stream B — Sandbox-to-repo integration** (engineering)
**Stream C — Workspace document convention** (engineering +
documentation)
**Stream D — Operator abuse handling framework** (policy +
documentation)
**Stream E — Federation protocols** (engineering + standards)
**Stream F — Production hardening** (engineering + operations)

Each stream is described below with scope, dependencies,
sequencing, and rough engineering effort estimates. Estimates
assume one engineer working on the stream; parallel execution
can compress the calendar timeline but not the engineering hours.

### 3.1 Stream A — Blob and fact authentication

**Scope.** Wire DID-based access control to the `/blobs` and
`/facts` HTTP endpoints. The FactPackage's existing
`AccessPolicy` field becomes the source of truth; the HTTP
layer checks the requesting DID's signature against the
policy.

**Specific work items:**

1. Define the wire format for DID-authenticated requests:
   `Authorization: DID <did> Signature <hex>` header pattern
   matching what's already used for `/api/documents/*`.

2. Implement signature verification at the `/blobs` PUT and
   GET endpoints. For PUT, verify the requesting DID has write
   access (via FactPackage access policy of any referencing
   fact). For GET, verify the requesting DID has read access.

3. Implement the same at the `/facts` endpoints, with the
   added complexity that a fact's access policy is in the fact
   itself; the verification happens after parsing the fact.

4. Implement tokenized fallback for cases where DID signing is
   too heavyweight (e.g., browser uploads). Tokens are
   short-lived signed credentials that authorize specific
   operations on behalf of a DID.

5. Implement an access-policy-evaluation utility that handles
   the full set of AccessPolicy variants (Public, Private,
   RoleBased, AttributeBased, Dynamic, Conditional).

6. Update the spacekit-storage-node configuration to allow
   operators to choose: enforce strict (every request must
   authenticate), permissive (legacy path stays open for
   self-hosted), or hybrid (some endpoints strict, others
   permissive).

**Dependencies.** None. This is independent of other streams.

**Engineering effort.** 4-6 weeks for one focused engineer,
including unit tests and integration tests. Documentation
update adds another week.

**Why it's first.** This unblocks multi-tenant hosting. Without
it, the entire roadmap stalls at "self-hosted only."

### 3.2 Stream B — Sandbox-to-repo integration

**Scope.** Enable sandbox transactions to include repo tree
modifications. An agent can propose code changes plus update
its memory plus record provenance in a single sandbox session,
and commit all three atomically.

**Specific work items:**

1. Add a `RepoTree` variant to the `TransactionModification`
   enum (this is the documentation-drift item the report flagged
   — bring the type system into alignment with what the
   sandbox policy table claims).

2. Define the wire format for repo tree modifications within
   a sandbox transaction: which paths are being added, modified,
   removed; what the proposed content hashes are; how merge
   conflicts are handled if multiple sandboxes propose
   conflicting changes.

3. Implement the commit path: when a sandbox containing repo
   modifications commits, the system writes the new blobs to
   CAS, creates the `spacekit:repo:commit:v1` fact, updates the
   appropriate ref, and persists the multi-model changes — all
   atomically.

4. Implement the discard path: when a sandbox containing repo
   modifications is discarded, the proposed blobs are not
   persisted to CAS, the commit fact is not created, the ref is
   not updated.

5. Implement the dry-run path: same as commit but doesn't
   persist. Used for agent workflows that want to preview the
   changes before deciding.

6. Add CLI surface: `spacekit sandbox commit --include-repo
   <repo-name>` or equivalent.

7. Update the `spacekit-repo` crate to expose the
   sandbox-aware commit functions.

**Dependencies.** Stream A is helpful (so that the sandbox can
itself be authenticated), but not strictly required for
self-hosted deployment.

**Engineering effort.** 6-8 weeks for one focused engineer,
including the schema changes, the atomicity logic, the test
suite for the cross-system semantics, and the CLI integration.

**Why it's second.** This unblocks the unified agent workspace
experience. Without it, agents can use sandboxes for some state
and repo commits for code, but not together atomically.

### 3.3 Stream C — Workspace document convention

**Scope.** Introduce `workspace` as a first-class FactPackage
type. A workspace document binds an owner DID, a list of
collaborators (human and agent DIDs), references to associated
repos, quotas, access policies, and metadata.

**Specific work items:**

1. Define the `spacekit:workspace:v1` fact schema. Fields:
   workspace ID, owner DID, collaborators (with per-DID roles),
   associated repo names, sandbox quotas, storage quotas, default
   access policy for new content created in the workspace,
   timestamps, status (active/archived).

2. Implement workspace CRUD operations as fact operations.
   Create a workspace = create a fact with the workspace schema.
   Update collaborators = update the workspace fact (creating a
   new version, preserving history).

3. Add CLI surface: `spacekit workspace create/list/show/
   add-collaborator/remove-collaborator/archive`.

4. Update the storage node's discovery/search APIs to allow
   queries like "list all workspaces owned by DID X" and "list
   all workspaces where DID Y is a collaborator."

5. Implement workspace-level quota tracking. The storage node
   reads the workspace fact to determine quotas for sandboxes
   and storage operations performed by the workspace's
   collaborators.

6. Document the workspace conventions in the storage node
   guide and the developer documentation.

**Dependencies.** Streams A and B are useful prerequisites for
making workspaces fully meaningful (otherwise they're just
labels), but workspace documents can be designed independently
and integrated incrementally.

**Engineering effort.** 3-4 weeks for one focused engineer.
Lighter than A and B because it's primarily defining a new fact
schema and adding the CLI/discovery surface on top.

**Why it's third.** Workspaces are the conceptual unit that
ties everything together into a coherent product. Without them,
the product is "use SpaceKit's primitives composed together";
with them, the product is "create a SpaceKit workspace where
your team and your agents work together."

### 3.4 Stream D — Operator abuse handling framework

**Scope.** Define how operators handle problematic content
hosted on their storage nodes. Three layers: per-operator
policies (operator decides), user discoverability (users can
see what operators stand for), federation handoff (users can
move to operators with policies they prefer).

**Specific work items (engineering and policy):**

1. Operator policy framework: operators publish a content
   policy document (signed by the operator's DID) that names
   what content is and isn't acceptable on their node, what
   processes they use for reports and disputes, how they
   handle legal requests (DMCA, takedowns, etc.).

2. Workspace-policy compatibility checking: when a user creates
   a workspace on an operator, the workspace policy must be
   compatible with the operator's policy. If conflicts exist
   (e.g., the workspace wants to host content the operator
   prohibits), the workspace creation fails.

3. Migration tooling: if a workspace needs to migrate to a
   different operator (because the user disagrees with the
   current operator's policy, or the operator changes policy),
   the system supports clean migration: blob copy, fact replay,
   ref handoff, optional ASTRA-based payment for the migration
   itself.

4. Operator reputation: federated reputation tracking that
   makes operator behavior visible — operators who consistently
   handle disputes well get higher reputation; operators who
   abuse takedowns or fail to honor their policies get lower
   reputation. Users can see this when choosing operators.

5. Decision points the team commits to (these are policy
   choices, not engineering choices):

   - What does the SWTCH Labs operator (if any) prohibit?
   - What due-process requirements must operators meet to
     comply with the SpaceKit network's federation standards?
   - How are disputes between users and operators escalated?
     Is there a network-level arbitration process or do
     disputes resolve through operator-user contract terms?
   - How does the network handle operators who violate the
     federation standards? (Disconnection from federation?
     Reputation slashing? Both?)
   - What is the SpaceKit network's relationship to law
     enforcement requests directed at the federation as a
     whole versus individual operators?

   These are decisions the team needs to commit to before
   Stream E (federation) can be implemented. The plan flags
   them as required-decisions, not as proposed answers.

**Dependencies.** Engineering work depends on Stream E
(federation infrastructure) being designed. Policy work can
start in parallel with all other streams.

**Engineering effort.** Policy framework: 2-3 weeks. Migration
tooling: 4-6 weeks. Reputation tracking: 6-8 weeks. Total: 12-17
weeks for the engineering portions. Policy work is roughly
parallel and depends on team decisions.

**Why it's fourth (but starts early).** The policy work is
critical-path for the federation phase. The engineering work
can be sequenced after the core gaps are closed. The team
should start the policy decisions early because they're slow
and consequential.

### 3.5 Stream E — Federation protocols

**Scope.** Enable multiple operators to participate in a
federated network. Users discover operators, choose where to
host, and can move between them. Operators can communicate to
coordinate cross-operator workspaces (e.g., a workspace where
the owner is on one operator and a collaborator is on another).

**Specific work items:**

1. Operator discovery protocol: an operator publishes a
   manifest fact describing what they offer (capacity,
   policies, pricing in stablecoin or ASTRA, supported
   features). Discovery happens via a network-wide index.

2. Workspace migration protocol: extend Stream D's migration
   tooling to handle cross-operator migrations. Source operator
   signs the migration manifest; destination operator verifies
   and accepts. **Shipped (preview):** `src/migration.rs`, HMAC handoff +
   DID v2 manifests, `spacekit:migration_record:v1`, CLI `migration verify/sign` —
   see [`documentation/guides/did-signed-migration.md`](documentation/guides/did-signed-migration.md).

3. Cross-operator collaboration: a workspace owned by an Alice
   on Operator A can have a Bob on Operator B as a collaborator.
   Reads and writes route through the appropriate operator's
   node. Conflict resolution happens at the FactPackage level.

4. Federated search and discovery: users can search across
   operators for workspaces tagged with specific keywords,
   without requiring centralized indexing. Privacy-respecting
   search uses signed query fragments that operators
   selectively respond to.

5. Operator economic settlement: when work crosses operators
   (e.g., a workspace migration, or a cross-operator
   collaboration), the operators settle in ASTRA or stablecoin
   for their respective work. Settlement uses SpaceKit Pay
   routing.

**Dependencies.** Stream A (auth), Stream C (workspaces), and
Stream D (operator policy framework) must be functional. Stream
E is the largest single piece of work in the plan.

**Engineering effort.** 16-24 weeks for one engineer, or more
realistically 8-12 weeks for a team of 2-3 engineers working in
parallel. Federation protocols are notoriously complex; the
budget should assume extension.

**Why it's fifth.** Federation is the final phase because it
depends on everything else. Building it earlier would mean
rewriting parts of it after the dependencies stabilize.

### 3.6 Stream F — Production hardening

**Scope.** Engineering work that makes the storage node
production-ready: monitoring, alerting, backup/restore,
performance characterization, operational runbooks, the
SPACEKIT_ENABLE_REAL_TRANSACTIONS default-true transition,
incident response procedures.

**Specific work items:**

1. Comprehensive structured logging across all storage
   operations (already in place via `spacekit-log`, but extend
   to cover the new work in Streams A/B/C).

2. Metrics collection: per-DID storage usage, per-operator
   throughput, transaction success/failure rates, sandbox
   commit/discard rates, blob deduplication rates, etc.

3. Alerting integration: Prometheus/Grafana-compatible metrics
   export, configurable alert thresholds for operators.

4. Backup and restore: full-node backup procedures, partial-
   workspace export, disaster recovery runbooks.

5. SPACEKIT_ENABLE_REAL_TRANSACTIONS migration: gradual
   transition from default-false to default-true, with
   documentation for operators on how to migrate cleanly.

6. Performance characterization: documented benchmarks for
   storage operations at different scales, capacity planning
   guidance for operators.

7. Operational runbooks: how to onboard a new operator, how to
   handle node failures, how to handle user disputes, how to
   investigate suspected abuse, how to comply with law
   enforcement requests.

**Dependencies.** Some elements (logging, metrics) can start in
parallel with other streams. Backup/restore and runbooks
depend on the new functionality from Streams A/B/C being
stabilized.

**Engineering effort.** 12-16 weeks distributed across the
other streams. Some of this is "after" work (runbooks for
new features); some is "during" work (logging while building);
some is "before" work (performance baseline before launching
multi-tenant).

**Why it's distributed.** Production hardening is not a
separate phase — it's a continuous discipline that happens
alongside the feature work.

## 4. Sequencing across three phases

The six streams sequence into three deployment phases:

### Phase 1 — Self-hosted production (Q1-Q2 2026)

**Goal:** Self-hosted SpaceKit storage node deployments are
production-grade. Organizations running their own node get a
verifiable, post-quantum, agent-native workspace platform with
full functionality.

**Streams active:** B (sandbox-to-repo), C (workspace
documents), F (production hardening). A is helpful but not
strictly required (perimeter trust is acceptable for
self-hosted).

**Outcomes:**
- Unified workspace product where humans and agents can both
  commit code, share memory, leave audit trails
- Workspace as first-class object with documented APIs and CLI
- Production-grade monitoring, backup, and operational
  procedures
- Documentation and developer guides complete
- Reference implementations of typical workspace patterns

**Estimated calendar:** 4-5 months with 2-3 engineers
focused.

### Phase 2 — Multi-tenant SaaS (Q3-Q4 2026)

**Goal:** Operators (SWTCH Labs and/or licensed partners) can
host multiple users' workspaces on shared storage nodes with
security and operational guarantees.

**Streams active:** A (blob/fact auth), D (operator abuse
framework), F (continued).

**Outcomes:**
- DID-authenticated blob and fact operations
- Operator content policy framework
- Workspace migration tooling
- Operator reputation tracking
- Multi-tenant deployment guides
- Legal review of operator role complete (Withers engagement)

**Estimated calendar:** 4-5 months with 2-3 engineers focused.

### Phase 3 — Federation (2027+)

**Goal:** Multiple operators participate in a federated network.
Users discover, choose, and migrate between operators freely.
The network-of-operators model is operational.

**Streams active:** E (federation protocols), continued F.

**Outcomes:**
- Operator discovery infrastructure
- Cross-operator workspace collaboration
- Federated search and discovery
- Operator economic settlement via ASTRA and SpaceKit Pay
- Network-level dispute and reputation procedures

**Estimated calendar:** 8-12 months with 3-4 engineers focused.

## 5. Engineering effort summary

Combined effort across all streams:

| Stream | Engineering weeks | Calendar weeks (with parallelism) |
|--------|-------------------|-----------------------------------|
| A — Blob/fact auth | 5-7 | 5-7 (sequential, blocks Phase 2) |
| B — Sandbox-to-repo | 6-8 | 6-8 (sequential, in Phase 1) |
| C — Workspace docs | 3-4 | 3-4 (parallel with A or B) |
| D — Abuse handling | 12-17 | 6-9 (with 2 engineers) |
| E — Federation | 16-24 | 8-12 (with 2-3 engineers) |
| F — Production hardening | 12-16 | distributed throughout |

**Total engineering effort:** 54-76 engineer-weeks (~13-19
engineer-months).

With a team of 3-4 focused engineers, the full plan executes
in 12-16 calendar months. With smaller teams or partial
allocation, it extends proportionally.

## 6. Decisions the team commits to before execution

Before any engineering begins on Phase 1, the team commits to:

1. **The "agent-native workspaces" positioning** as the primary
   product framing. Marketing, technical documentation, and
   investor materials align on this positioning.

2. **The 95% / 5% split for SpaceKit Pay continues to apply** to
   workspace-related payments (e.g., agent inference fees, paid
   workspace hosting). Operators take 95%, treasury takes 5%.

3. **ASTRA emission for storage service applies to workspace
   storage.** Operators serving workspace storage earn ASTRA via
   the storage service category in the emission schedule.

4. **Operator policy framework gets a formal decision process.**
   The team appoints a decision-maker for the content policy
   questions in Stream D before engineering starts. Without this,
   the engineering work for Streams D and E can't proceed
   coherently.

5. **The SPACEKIT_ENABLE_REAL_TRANSACTIONS transition timeline.**
   The team commits to when default-true becomes the released
   default. (Recommendation: end of Phase 1.)

6. **Withers Worldwide legal review covers operator role.** Before
   Phase 2 launches, Withers reviews and approves the operator-role
   legal posture, including content policy implications and the
   federation handoff model. This is in addition to the existing
   SpaceKit Pay and ASTRA legal posture work.

These decisions are committed before execution. If any of them
change later, the plan revisits accordingly.

## 7. Risk and mitigation

A few risks worth surfacing:

**Risk: GitHub or GitLab announces equivalent agent-native
features.** Both companies have product teams that may move into
this space. Mitigation: SpaceKit's cryptographic and
post-quantum properties are structurally different and harder
to clone; the workspace + agent + verifiable identity story is
more than feature-set parity. Stay focused on what's actually
defensible.

**Risk: Federation protocols don't get adopted.** Phase 3
requires multiple operators participating. If SWTCH Labs is the
only operator, federation is meaningless. Mitigation: plan
operator partner outreach during Phase 2 specifically, with
incentive structures (SpaceKit Pay routing, ASTRA grants,
revenue share) for early federation participants.

**Risk: Content policy disputes damage SpaceKit reputation.**
A high-profile dispute (e.g., a controversial user gets
de-platformed and litigates) could harm SpaceKit's
positioning. Mitigation: Stream D's policy framework is
designed for exactly this scenario; operator policies are
explicit and discoverable so disputes happen at the operator
level, not the network level.

**Risk: Multi-tenant deployment exposes user data via
implementation bugs.** Blob/fact auth (Stream A) is
security-critical; bugs here are catastrophic. Mitigation:
independent security audit of Stream A before Phase 2 launch.
The same audit firm engaged for SpaceKit Pay and ASTRA can
extend their scope.

**Risk: ASTRA emission depletes before the network reaches
self-sustaining storage demand.** If storage service growth is
slow, emission rewards may not be enough to incentivize
operators. Mitigation: the emission curve has decades of
emission ahead; if a specific category needs more support, the
emission allocation between categories is governance-adjustable
(40/30/20/10 currently split between consensus/compute/storage/
messaging, with storage getting 20%).

## 8. What this plan does NOT include

For clarity:

- **Specific operator marketing or business development.**
  Those are separate plans.
- **The pricing of paid hosted workspaces (if applicable).**
  Separate decision.
- **The detailed legal framework for operator roles.** Withers
  develops that in Phase 2 prep.
- **Specific Phase 1 launch announcements.** Marketing and PR
  develop those.
- **The visual/UX design of workspace-related UIs.** The deck
  for "what a workspace looks like to a user" is separate
  from this engineering plan.

## 9. Open questions for follow-up

A few questions worth tracking for resolution as the plan
executes:

**Question 1: Should we publish a "what to expect from a
SpaceKit workspace" public document before Phase 1 launches?**
A short, calibrated document that names what users will get
and what they won't (yet). Helps prospective adopters
calibrate expectations.

**Question 2: What's the right naming for the workspace
product itself?** "SpaceKit Workspaces" is descriptive but
maybe not strong enough. Alternatives: "Spaces," "Folio," or
something distinct. Marketing team should explore.

**Question 3: How does the SpacetimeConsensusAgent's brain
participate in workspace operations?** The agent trained for
consensus advisory could potentially extend into workspace
operations (e.g., flagging suspicious commits, suggesting
PR reviews). Not Phase 1, but worth thinking about for
Phase 2+.

**Question 4: What's the relationship between SpaceKit
Workspaces and Growformer?** Are workspaces a Growformer
product, a SpaceKit network product, both, or distinct?
Affects how the product is positioned in the deck.

## 10. References

- SpaceKit Storage Node review report (the GitHub-vulnerabilities
  document that motivated this plan)
- `spacekit-diff` README
- `spacekit-repo` README
- SpaceKit Storage Node documentation site
- SpaceKit Pay legal posture memo
- ASTRA Economic Model Decision Memo
- Tokenomics v2 specification

## 11. Sign-off

This plan is the canonical reference for storage node
enhancement work. Execution against it begins after the team
commits to the six decisions in Section 6.

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai