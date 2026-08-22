# SpaceKit SDK & Backend Integration Reference

> Single-file developer reference for integrating with SpaceKit's backend services.
> Provide this document to your LLM or team for context on APIs, CLI, identity, storage, repositories, workspaces, payments, and app packaging.
>
> **Published on the site:** [spacekit.xyz/docs/backend-integration](https://spacekit.xyz/docs/backend-integration)
>
> **Vibe coders / laypeople:** start with [Ship a SpaceKit App](#ship-a-spacekit-app-vibe-coders), then skim [Embedded App SDK](#embedded-app-sdk-iframe). The rest is deep reference.

---

## Table of Contents

0. [Ship a SpaceKit App (vibe coders)](#ship-a-spacekit-app-vibe-coders)
1. [Architecture Overview](#architecture-overview)
2. [Identity & Authentication](#identity--authentication)
3. [Storage Node API](#storage-node-api)
4. [Website API](#website-api)
5. [CLI Reference](#cli-reference)
6. [Repository System](#repository-system)
7. [Workspaces](#workspaces)
8. [App Packaging (.spkg)](#app-packaging-spkg)
9. [Embedded App SDK (iframe)](#embedded-app-sdk-iframe)
10. [SpaceKit Pay (Payments)](#spacekit-pay-payments)
11. [Content Monetization](#content-monetization)
12. [MCP Server (Agent Tools)](#mcp-server-agent-tools)
13. [Growformer Agents](#growformer-agents)
14. [Spec-Driven SDK Generation (OpenAPI)](#spec-driven-sdk-generation-openapi)
15. [Sandbox & Transaction System](#sandbox--transaction-system)
16. [Federation](#federation)
17. [Configuration](#configuration)

---

## Ship a SpaceKit App (vibe coders)

### 30-second mental model

Your web app (React, Vue, vanilla — any SPA) runs inside an **iframe** on spacekit.xyz. SpaceKit injects `window.spacekit` into that iframe. The parent frame handles **identity, storage, messaging, and payments**. You do **not** need your own backend for most apps. You ship a signed **`.spkg`** under your **DID**.

```
Your SPA  →  packaged as .spkg  →  loaded in iframe
                                      │
                                      ▼
                            window.spacekit  (injected)
                                      │
                                      ▼
                     parent host (identity · storage · pay)
```

### Glossary

| Term | Meaning |
|------|---------|
| **DID** | Your SpaceKit identity, e.g. `did:spacekit:user:alice` |
| **`.spkg`** | Signed app package (HTML/JS/CSS + manifest) |
| **iframe** | Sandboxed frame where your app runs on the site |
| **bridge** | Parent-frame code that answers `window.spacekit` calls |
| **marketplace** | Where deployed apps are listed and sold (USDC) |

### What are you building?

| Kind | Use when | Path |
|------|----------|------|
| **App / game UI** | Interactive web UI users open on SpaceKit | SPA → `.spkg` → iframe SDK (this section) |
| **Companion** | Chat / pet / domain “brain” | Growformer (`*.gf.toml` train/infer) — see [Growformer Agents](#growformer-agents) |
| **Both** | UI that talks to a trained brain | Package the UI as `.spkg`; train the brain separately |

### Five commands to ship

```bash
spacekit init                              # identity + config (~/.spacekit)
# …build your SPA (npm run build → dist/)…
spacekit app package --name my-app \
  --did did:spacekit:user:YOU --dir dist/  # → my-app.spkg
spacekit app deploy my-app.spkg \
  --storage-url http://127.0.0.1:3030      # or production storage
# → live, discoverable, sellable on the marketplace
```

Typical project layout (matches `spacekit-projects/apps/*`):

```
my-app/
├── ui/                 # Vite/React (or any SPA)
├── scripts/            # build.sh · package.sh · deploy.sh
└── spacekit.toml       # name, version, did
```

### New apps just work (no website PR)

After deploy, SpaceKit loads your `.spkg` through **`SpacekitAppFrame`** (in `@spacekit/sdk`). The website’s `WebPackageFrame` is a thin host: it resolves storage, wires payments, and picks a bridge.

**Default bridge = `app-data`.** Documents and subscription payments work automatically under the package creator DID. You do **not** need a website code change, env var, or registry entry.

A few built-in apps use custom bridges (Notes/Cairn, Quay, Hermes, Harmonia CRM, Dragon Layerz, Token Wall). Everyone else gets `app-data`.

### Externalized embed host (how the pieces fit)

| Layer | Package / file | Role |
|-------|----------------|------|
| Portable runtime | `@spacekit/sdk` → `SpacekitAppFrame`, package loader, inject shim | Load `.spkg`, inject `window.spacekit`, route postMessage calls |
| Website shell | `WebPackageFrame` | Resolve storage origin, SpaceKit Pay, bridge registry, desktop cache |
| Your app | iframe SPA | Call `window.spacekit.*` only — never talk to parent APIs directly |

Do **not** invent per-app website wiring for a new marketplace app unless you need a custom bridge (files, contracts facets, etc.).

### Hello SpaceKit (copy-paste)

Prefer the convenience API (what real apps use). Fall back to local/demo when not embedded:

```javascript
function isSpacekitEmbedded() {
  return typeof window !== "undefined" && Boolean(window.spacekit);
}

async function boot() {
  if (!isSpacekitEmbedded()) {
    // Local vite preview / offline: use localStorage or in-memory demo
    console.log("Not embedded — demo mode");
    return;
  }

  const sk = window.spacekit;
  await sk.storage?.ready?.();

  const { did } = await sk.identity.did();
  console.log("viewer:", did);

  await sk.documents.put("notes", "hello", {
    text: "shipped on SpaceKit",
    at: Date.now(),
  });
  const doc = await sk.documents.get("notes", "hello");
  console.log("saved:", doc);
}

boot();
```

### Paste this into your LLM (Cursor / Claude / Codex)

```
You are helping me ship a SpaceKit app.

Context: SpaceKit embeds my SPA in an iframe and injects window.spacekit.
Identity, document storage, and payments go through that bridge. I do not
need a custom backend for basic save/load/pay. New .spkg apps use the
generic app-data bridge automatically after deploy.

Tasks:
1. Keep my existing UI; add isSpacekitEmbedded() + window.spacekit calls.
2. Persist user data with window.spacekit.documents or storage.putRecord.
3. Optionally gate features with window.spacekit.payments.status / subscribe.
4. Build to dist/, package with spacekit app package, deploy with spacekit app deploy.
5. Do not invent website PRs, custom bridges, or parent-frame APIs.

Full API reference is in this document under "Embedded App SDK (iframe)".
```

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        SpaceKit Platform                            │
│                                                                     │
│  ┌───────────────┐  ┌──────────────────┐  ┌───────────────────────┐ │
│  │  spacekit.xyz │  │  website-api     │  │  storage-node         │ │
│  │  (frontend)   │──│  (Rust/Axum)     │──│  (Rust/Warp)          │ │
│  │  React/Vite   │  │  port 3001       │  │  port 3030            │ │
│  └───────────────┘  └──────────────────┘  └───────────────────────┘ │
│         │                                         │                 │
│         │ iframe (.spkg apps)                     │                 │
│  ┌───────────────┐                        ┌───────────────────────┐ │
│  │ Embedded Apps │                        │  compute-node         │ │
│  │ SignFlow,     │                        │  (WASM VM, contracts) │ │
│  │ Athena, Janus │                        │  port 8545            │ │
│  └───────────────┘                        └───────────────────────┘ │
│                                                                     │
│  ┌───────────────┐  ┌──────────────────┐  ┌───────────────────────┐ │
│  │  spacekit-cli │  │  spacekit-pay    │  │  Ethereum / Base      │ │
│  │  (Rust binary)│  │  (Solidity)      │  │  (USDC payments)      │ │
│  └───────────────┘  └──────────────────┘  └───────────────────────┘ │
└─────────────────────────────────────────────────────────────────────┘
```

**Key components:**
- **Storage Node** — persistent data layer: files, facts, blobs, documents, workspaces
- **Website API** — proxy/orchestration layer: auth sessions, repos, social, marketplace
- **Compute Node** — WASM VM, smart contracts, ASTRA token
- **CLI** — `spacekit` binary for identity, repos, workspaces, agents, contracts
- **SpaceKit Pay** — non-custodial USDC payment router (Ethereum/Base)
- **`@spacekit/sdk` embed** — `SpacekitAppFrame` loads `.spkg` packages, injects `window.spacekit`; website `WebPackageFrame` is the thin host

> Shipping an app or game UI? Jump to [Ship a SpaceKit App](#ship-a-spacekit-app-vibe-coders).

---

## Identity & Authentication

### DID Format

SpaceKit uses decentralized identifiers (DIDs):

```
did:spacekit:user:<username>     # human user
did:spacekit:agent:<name>        # AI agent
did:spacekit:admin:<service>     # service account
```

### Creating Identity (CLI)

```bash
spacekit init                            # creates ~/.spacekit/config.toml
spacekit identity create --name alice    # generates DID + SPHINCS+ keypair
spacekit login                           # authenticates with website-api (session token)
spacekit identity link                   # links CLI identity to a website session
```

Identity files are stored at `~/.spacekit/`:
- `config.toml` — connections, default DID
- `did_wallet.json` — SPHINCS+ private key for signing

### Authentication Headers

**Storage Node:**
```http
Authorization: DID did:spacekit:user:alice
X-Storage-Secret: <optional shared secret>
```

**Website API (session-based):**
```http
Authorization: Bearer <session-token>
owner-did: did:spacekit:user:alice
```

### Website sign-in (browser)

Production auth uses **passkeys** or **email magic links** (no username/password). Sessions are Bearer tokens stored server-side (~30 day TTL).

1. **Claim a username** (once): `POST /api/did/register` with `{ "username": "alice", "kyber_public_key": "..." }` → `did:spacekit:user:alice`
2. **Sign in** (pick one):
   - **Passkey:** `POST /api/auth/passkey/login/options` → WebAuthn ceremony → `POST /api/auth/passkey/login/verify` → `{ session_token, did, method: "passkey" }`
   - **Email:** `POST /api/auth/email/send-link` with `{ email, username?, purpose: "login" }` → user opens link → `POST /api/auth/email/verify` with `{ token }` → `{ session_token, did, method: "email" }`
3. **Use the session** on protected website-api routes:
   ```http
   Authorization: Bearer <session_token>
   owner-did: did:spacekit:user:alice
   ```
4. **Validate or end:** `GET /api/auth/session` · `POST /api/auth/session/logout`

Add a passkey after email sign-up: `POST /api/auth/passkey/register/options` (with `owner-did`) → `POST /api/auth/passkey/register/verify`.

Protected routes (e.g. `POST /api/did/link-kyber`, `POST /api/repos/authorize-push`) require a **valid Bearer session**, not `owner-did` alone.

### CLI session

```bash
spacekit init                            # creates ~/.spacekit/config.toml
spacekit identity create --name alice    # generates DID + SPHINCS+ keypair
spacekit login                           # website sign-in → stores session token locally
spacekit identity link                   # links CLI identity to website session
```

Expired or missing sessions return **401** with `{ "error": "Invalid or expired session" }`. Re-run `spacekit login` or sign in again in the browser.

---

## Storage Node API

Base URL: `http://127.0.0.1:3030` (default)

### Files (Encrypted)

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/files/upload` | Upload encrypted file (multipart) |
| `GET` | `/files/{id}` | Get file metadata |
| `GET` | `/files/{id}/content` | Download file content |
| `GET` | `/files/list/{owner_did}` | List files for an owner |
| `DELETE` | `/files/{id}` | Delete file |

**Upload example:**
```bash
curl -X POST http://127.0.0.1:3030/files/upload \
  -H "Authorization: DID did:spacekit:user:alice" \
  -F "file=@document.pdf" \
  -F "filename=document.pdf"
```

**Response:**
```json
{
  "file_id": "uuid",
  "filename": "document.pdf",
  "size": 12345,
  "hash": "blake3hex..."
}
```

### Content-Addressed Blobs (CAS)

| Method | Path | Purpose |
|--------|------|---------|
| `PUT` | `/blobs/{blake3_hash}` | Store raw bytes (64 hex char BLAKE3 hash) |
| `GET` | `/blobs/{blake3_hash}` | Retrieve raw bytes |
| `HEAD` | `/blobs/{blake3_hash}` | Existence probe |
| `POST` | `/blobs/exists` | Batch existence check |

**Batch existence check:**
```json
// POST /blobs/exists
{ "hashes": ["abc123...", "def456..."] }
// Response:
{ "found": ["abc123..."], "missing": ["def456..."] }
```

### Facts (Signed Data Packages)

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/facts` | Store a `FactPackage` JSON |
| `GET` | `/facts/{fact_id}` | Retrieve a fact |
| `POST` | `/facts/batch` | Batch retrieve facts |
| `POST` | `/query/facts` | Query facts with filters |

**FactPackage structure:**
```json
{
  "fact_id": "hex...",
  "metadata": {
    "author_did": "did:spacekit:user:alice",
    "created_at": "2026-01-01T00:00:00Z",
    "category": "Repository",
    "knowledge_domain": "SoftwareEngineering"
  },
  "content": {
    "Json": {
      "schema": "spacekit:repo:commit:v1",
      "data": { "tree": {}, "message": "Initial commit" }
    }
  },
  "dependencies": [],
  "signature": { "sphincs_plus": "base64..." }
}
```

### Documents (Key-Value Store)

| Method | Path | Purpose |
|--------|------|---------|
| `PUT` | `/api/documents/{collection}/{id}` | Create/update document |
| `GET` | `/api/documents/{collection}/{id}` | Get document |
| `GET` | `/api/documents/{collection}` | List documents in collection |
| `DELETE` | `/api/documents/{collection}/{id}` | Delete document |

**Auth:** `Authorization: DID <owner-did>`

```bash
# Store a document
curl -X PUT http://127.0.0.1:3030/api/documents/settings/user-prefs \
  -H "Authorization: DID did:spacekit:user:alice" \
  -H "Content-Type: application/json" \
  -d '{"theme": "dark", "language": "en"}'

# Retrieve
curl http://127.0.0.1:3030/api/documents/settings/user-prefs \
  -H "Authorization: DID did:spacekit:user:alice"
```

### Structured Queries

```bash
# POST /query/facts
curl -X POST http://127.0.0.1:3030/query/facts \
  -H "Content-Type: application/json" \
  -d '{
    "filters": { "author_did": "did:spacekit:user:alice" },
    "limit": 50,
    "offset": 0
  }'
```

### Users

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/users` | Register user |
| `GET` | `/users/{id}` | Get user |
| `POST` | `/users/encrypted` | Register encrypted user |

---

## Website API

Base URL: `http://127.0.0.1:3001` (local) or `https://api.spacekit.xyz` (production)

### Authentication

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/auth/session` | Validate session (`Authorization: Bearer` and/or `owner-did`) |
| `POST` | `/api/auth/session/logout` | End session |
| `GET` | `/api/auth/passkey/status` | Check if user has passkey registered |
| `POST` | `/api/auth/passkey/register/options` | Start passkey registration (requires `owner-did`) |
| `POST` | `/api/auth/passkey/register/verify` | Complete passkey registration → issues session |
| `POST` | `/api/auth/passkey/login/options` | Start passkey login |
| `POST` | `/api/auth/passkey/login/verify` | Complete passkey login → issues session |
| `POST` | `/api/auth/email/send-link` | Send email magic link (`purpose`: `login` or `link`) |
| `POST` | `/api/auth/email/verify` | Verify magic-link token → issues session |
| `POST` | `/api/auth/email/link` | Attach recovery email to signed-in DID |

### Repositories

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/repos` | List repositories |
| `POST` | `/api/repos` | Create repository |
| `GET` | `/api/repos/:owner/:repo` | Get repository details |
| `GET` | `/api/repos/:owner/:repo/commits/:branch` | List commits |
| `GET` | `/api/repos/:owner/:repo/tree/:commit_id` | Browse file tree |
| `GET` | `/api/repos/:owner/:repo/blob/:blob_hash` | Get file content |
| `POST` | `/api/repos/authorize-push` | Verify push access (CLI gate) |

**Create repository:**
```json
// POST /api/repos
// Headers: owner-did: did:spacekit:user:alice
{
  "owner_slug": "alice",
  "owner_type": "user",
  "repo_slug": "my-project",
  "owner_did": "did:spacekit:user:alice",
  "description": "My project",
  "visibility": "public"
}
```

### Repo Collaborators

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/repos/:owner/:repo/collaborators` | List collaborators |
| `POST` | `/api/repos/:owner/:repo/collaborators` | Invite collaborator |
| `DELETE` | `/api/repos/:owner/:repo/collaborators/:did` | Remove collaborator |
| `POST` | `/api/repos/:owner/:repo/collaborators/invites/:id/accept` | Accept invite |

### Repo Organizations

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/repo-orgs` | List organizations |
| `POST` | `/api/repo-orgs` | Create organization |
| `GET` | `/api/repo-orgs/:slug/members` | List members |
| `POST` | `/api/repo-orgs/:slug/invites` | Invite member |

### Workspaces

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/workspaces` | List workspaces |
| `POST` | `/api/workspaces` | Create workspace |
| `GET` | `/api/workspaces/:id` | Get workspace |

### Storage & Billing

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/account/storage` | Get storage usage + tier |
| `POST` | `/api/account/storage/upgrade` | Record tier upgrade |

**Response:**
```json
{
  "tier": "basic",
  "used_bytes": 5242880,
  "limit_bytes": 52428800,
  "expires_at": null,
  "upgrade_price_cents": 400
}
```

Tiers: `basic` (free, 50 MB) / `team` ($4/mo, 1 GB)

### Social

| Method | Path | Purpose |
|--------|------|---------|
| `POST` | `/api/social/posts` | Create post |
| `GET` | `/api/social/feed/public` | Public feed |
| `GET` | `/api/social/feed` | Following feed |
| `POST` | `/api/social/follow` | Follow user |
| `GET` | `/api/social/followers/:did` | Get followers |

### Marketplace

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/marketplace` | List marketplace apps |
| `POST` | `/api/social/purchase` | Purchase listing |
| `GET` | `/api/social/purchases` | Get purchases |

---

## CLI Reference

Install:
```bash
cd spacekit-cli && cargo build --features full --release
# Binary: target/release/spacekit
```

### Identity & Auth

```bash
spacekit init                              # initialize config
spacekit identity create --name <name>     # create DID + keypair
spacekit identity show                     # show current identity
spacekit login                             # authenticate with website
spacekit identity link                     # link CLI to web session
```

### Repository Commands

```bash
spacekit repo init --name <owner/repo> --remote <url>
spacekit repo add                          # stage files
spacekit repo status                       # show staged/unstaged
spacekit repo commit -m "message"          # SPHINCS+-signed commit
spacekit repo push [-b branch] [-f]        # push to remote
spacekit repo pull [-b branch]             # pull from remote
spacekit repo clone <remote> <name> [dir]
spacekit repo log [--limit N] [--graph]
spacekit repo diff [--content]
spacekit repo branch [name]                # list or create branch
spacekit repo checkout <branch>
spacekit repo merge <branch>               # 3-way merge
spacekit repo tag [name [commit]]
spacekit repo verify [commit] [--all]      # verify signatures
spacekit repo show [commit]
spacekit repo reset [commit] [--soft|--mixed|--hard]
spacekit repo cherry-pick <commit>
spacekit repo revert <commit>
spacekit repo reflog
spacekit repo gc                           # garbage collect
```

### Workspace Commands

```bash
spacekit workspace create <name> --storage-url <url>
spacekit workspace show <name>
spacekit workspace list --owner-did <did>
spacekit workspace export <name> -o bundle.json
spacekit workspace import bundle.json
spacekit workspace publish                 # publish to registry
```

### App Packaging

```bash
spacekit app package --name <name> --did <publisher-did> --dir dist/
# Creates: <name>.spkg

spacekit app deploy <name>.spkg --storage-url <url>
```

### Agent / Brain Commands

```bash
spacekit agent train --project brain.gf.toml
spacekit agent infer --brain agent/brain.bin --project brain.gf.toml --prompt "..."
spacekit agent info agent/brain.bin
spacekit agent merge brain-a.bin brain-b.bin -o merged.bin
```

### Spec-Driven SDK Generation

```bash
spacekit agent sdk --spec openapi.yaml --lang python --out ./sdks --check
spacekit agent sdk --spec openapi.yaml --lang typescript --out ./sdks --check
spacekit agent sdk --spec openapi.yaml --out ./sdks --plan    # dry-run diff
```

See [Spec-Driven SDK Generation (OpenAPI)](#spec-driven-sdk-generation-openapi) and `spacekit-cli/documentation/AGENT_SDK_GENERATION.md`.

### Smart Contracts

```bash
spacekit contract deploy --wasm contract.wasm
spacekit contract call --contract <id> --method <name> --args '{...}'
spacekit contract query --contract <id> --method <name>
```

---

## Repository System

Repositories use a content-addressed storage model layered on the storage node:

### Data Model

| Layer | Purpose | Storage |
|-------|---------|---------|
| **Blobs** | Immutable file bytes, deduplicated by BLAKE3 hash | `/blobs/{hash}` |
| **Commits** | Version snapshots (path→hash, message, ancestry) | `/facts/{id}` as `FactPackage` |
| **Refs** | Mutable branch pointers | `/api/documents/repos/<name>/refs/heads/<branch>` |

### Commit Schema (`spacekit:repo:commit:v1`)

```json
{
  "tree": {
    "src/main.rs": "blake3hexhash...",
    "README.md": "blake3hexhash..."
  },
  "modes": {
    "scripts/build.sh": 493
  },
  "message": "Add feature X",
  "author_name": "alice",
  "author_did": "did:spacekit:user:alice",
  "timestamp": 1719532800
}
```

Parent commits are stored as `dependencies` on the `FactPackage`.

### Custom Client Integration

Without the CLI, integrate directly via HTTP:

```python
import hashlib, requests, json

NODE = "http://127.0.0.1:3030"
DID = "did:spacekit:user:alice"

# 1. Hash and upload blobs
with open("main.py", "rb") as f:
    data = f.read()
    h = blake3(data).hexdigest()  # use blake3 library
    requests.put(f"{NODE}/blobs/{h}", data=data)

# 2. Create commit fact
commit = {
    "fact_id": "...",  # deterministic from content
    "content": {
        "Json": {
            "schema": "spacekit:repo:commit:v1",
            "data": {"tree": {"main.py": h}, "message": "init"}
        }
    }
}
requests.post(f"{NODE}/facts", json=commit)

# 3. Update branch ref
requests.put(
    f"{NODE}/api/documents/repos/myproject/refs/heads/main",
    headers={"Authorization": f"DID {DID}"},
    json={"tip": commit["fact_id"]}
)
```

---

## Workspaces

Workspaces bind an owner, collaborators, repos, and quotas.

### Create Workspace

```bash
spacekit workspace create my-team \
  --storage-url http://127.0.0.1:3030 \
  --collaborator did:spacekit:agent:bot:agent \
  --repo my-repo
```

Or via HTTP:
```json
// POST /api/workspaces
// Authorization: DID did:spacekit:user:alice
{
  "workspace_id": "my-team",
  "collaborators": [
    { "did": "did:spacekit:agent:bot", "role": "agent" }
  ],
  "associated_repos": ["my-repo"],
  "quotas": {
    "max_sandbox_bytes": 67108864,
    "max_storage_bytes": 1073741824
  }
}
```

### Quotas

- `max_sandbox_bytes` — max bytes per sandbox session (default 64 MB)
- `max_storage_bytes` — total storage across all sandboxes (default 1 GB)

---

## App Packaging (.spkg)

SpaceKit apps are single-page web applications packaged as `.spkg` bundles and embedded in iframes on the website.

### Creating an App

1. Build your web app (React, Vue, vanilla JS — any SPA):
```bash
npm run build   # outputs to dist/
```

2. Create `spacekit.toml` manifest:
```toml
name = "my-app"
version = "1.0.0"
did = "did:spacekit:user:alice"

[network.local]
storage_url = "http://127.0.0.1:3030"
api_url = "http://127.0.0.1:3001"
```

3. Package:
```bash
spacekit app package --name my-app --did did:spacekit:user:alice --dir dist/
# Creates: my-app.spkg
```

4. Deploy:
```bash
spacekit app deploy my-app.spkg --storage-url http://127.0.0.1:3030
```

### App Lifecycle

On the website, `WebPackageFrame` resolves storage and payments, then renders **`SpacekitAppFrame`** from `@spacekit/sdk/react/embed`. That frame loads the `.spkg`, injects the SDK shim into the HTML, and hosts the iframe.

The iframe receives `window.__SPACEKIT_EMBED__`:
- `parentOrigin` — parent frame origin
- `messagingBase` — messaging HTTP base (direct node or `{api}/api/messaging` proxy)
- `apiBase` — social/directory API base URL
- `identityDid` — viewer's DID
- `reposApiBase` — repos API base
- `workspacesApiBase` — workspaces API base
- `assetUrls` — map of package-relative paths → blob URLs
- `appId` — also available as `window.spacekit.appId`

---

## Embedded App SDK (iframe)

Apps running inside the embed frame call the parent via **`window.spacekit`**. The inject shim (`@spacekit/sdk/embed`) exposes both:

- **Convenience methods** — prefer these (`window.spacekit.documents.put(...)`)
- **Generic RPC** — `window.spacekit.call(module, method, params)`

Always guard for local preview:

```javascript
const embedded = Boolean(window.spacekit);
```

### Cheat sheet

| Module | Methods | Typical use |
|--------|---------|-------------|
| `identity` | `did`, `getState`, `setState`, `authHeaders` | Who is viewing |
| `documents` | `get`, `put`, `list`, `delete` | Per-app JSON docs (app-data default) |
| `storage` | `ready`, `get`/`set`/`list`/`delete`, `putRecord`/`getRecord`/`listRecords`/`deleteRecord`, `putBlob`/`getBlob` | KV + blobs |
| `payments` | `status`, `subscribe`, `config`, `charge` | Subscriptions / one-shot pay |
| `messaging` | `publish`, `send`, `list`, `subscribe` | Pub/sub + DMs |
| `contracts` | `anchor`, `verify`, `createShare`, `revokeShare`, `invoke`, `call`, `status` | On-chain / WASM facets |
| `crypto` | `encryptUpload`, `decryptBlob` | Encrypted uploads |
| `http` | `fetch`, `sseSubscribe`, `sseClose` | Parent-proxied fetch (auth injected) |
| `app` | `ready`, `isOwner`, `ownerDid` | Package metadata |

`window.fetch` and `EventSource` inside the iframe are patched to route through the parent HTTP bridge when appropriate.

### Identity

```javascript
const { did } = await window.spacekit.identity.did();
// did → "did:spacekit:user:alice" or null

const state = await window.spacekit.identity.getState();
await window.spacekit.identity.setState({ theme: "dark" });

const headers = await window.spacekit.identity.authHeaders();
// Use when calling website-api yourself (usually unnecessary — http.fetch injects auth)
```

Equivalent RPC: `await window.spacekit.call("identity", "did", {})`.

### Document Storage (per-app)

Default **app-data** bridge scopes documents to your package. Collections starting with `__` are reserved (e.g. `__subscriptions`).

```javascript
await window.spacekit.documents.put("user-data", "doc-1", {
  name: "Alice",
  score: 42,
});

const doc = await window.spacekit.documents.get("user-data", "doc-1");
const docs = await window.spacekit.documents.list("user-data");
await window.spacekit.documents.delete("user-data", "doc-1");
```

### Key / blob Storage

```javascript
await window.spacekit.storage.ready();

await window.spacekit.storage.set("highscore", "9001");
const v = await window.spacekit.storage.get("highscore");
await window.spacekit.storage.list("high");
await window.spacekit.storage.delete("highscore");

// Structured records (common in editors / workspaces)
await window.spacekit.storage.putRecord("ws/main", { files: [] });
const ws = await window.spacekit.storage.getRecord("ws/main");

// Binary blobs → content id
const cid = await window.spacekit.storage.putBlob(arrayBufferOrBlob);
const blob = await window.spacekit.storage.getBlob(cid);
```

### Payments (Subscriptions)

```javascript
const status = await window.spacekit.payments.status();
// status.active → boolean
// status.expiresAt → unix ms or null
// status.viewerDid → string | null

const result = await window.spacekit.payments.subscribe({
  /* optional bridge-specific options */
});
// result.success → boolean
// result.txHash → "0x..."

const config = await window.spacekit.payments.config();
// config.configured → boolean

await window.spacekit.payments.charge(400, "USDC"); // amount + token hint
```

Parent host runs SpaceKit Pay (wallet) and records marketplace purchase when configured.

### Messaging

```javascript
await window.spacekit.messaging.publish("room:lobby", { text: "hi" });
await window.spacekit.messaging.send("did:spacekit:user:bob", { text: "dm" });
const inbox = await window.spacekit.messaging.list();

const unsub = window.spacekit.messaging.subscribe("room:lobby", (msg) => {
  console.log(msg);
});
// later: unsub();
```

### Contracts

```javascript
await window.spacekit.contracts.anchor("note-1", contentHash);
await window.spacekit.contracts.verify("note-1");

await window.spacekit.contracts.createShare({ /* bridge-specific input */ });
await window.spacekit.contracts.revokeShare(shareId);

// Generic facet invoke (e.g. Token Wall)
await window.spacekit.contracts.invoke("methodName", [arg1, arg2]);
await window.spacekit.contracts.call("methodName", [arg1, arg2]);
const st = await window.spacekit.contracts.status();
```

Custom bridges may expose richer contract surfaces; the generic app-data bridge may return “unsupported” for some methods.

### Crypto

```javascript
const enc = await window.spacekit.crypto.encryptUpload(blob, ownerPubkey);
const plain = await window.spacekit.crypto.decryptBlob(cid, iv, wrappedKey);
```

### HTTP (parent-proxied)

Prefer normal `fetch()` — it is rewritten through the bridge. Explicit API:

```javascript
const res = await window.spacekit.http.fetch("/api/somewhere", {
  method: "GET",
  headers: { Accept: "application/json" },
});
// res is a Response (status, headers, body)

const streamId = await window.spacekit.http.sseSubscribe(url);
await window.spacekit.http.sseClose(streamId);
```

### App metadata

```javascript
await window.spacekit.app.ready();
const owner = await window.spacekit.app.isOwner();
const ownerDid = await window.spacekit.app.ownerDid();
console.log(window.spacekit.appId);
```

### Embed vs local-dev fallback

```javascript
async function loadScore() {
  if (window.spacekit?.documents) {
    const doc = await window.spacekit.documents.get("game", "score");
    return doc?.data?.value ?? 0;
  }
  return Number(localStorage.getItem("score") || 0);
}

async function saveScore(value) {
  if (window.spacekit?.documents) {
    await window.spacekit.documents.put("game", "score", { value });
    return;
  }
  localStorage.setItem("score", String(value));
}
```

This pattern keeps Vite `npm run dev` working while the same build ships as a `.spkg`.

---

## SpaceKit Pay (Payments)

Non-custodial USDC payment router on Ethereum/Base. 95% goes to the publisher, 5% to the SpaceKit treasury.

### Contracts

| Contract | Purpose |
|----------|---------|
| `SpaceKitPayRouter` | Routes USDC payments (publisher 95% / treasury 5%) |
| `SpaceKitOperatorRegistry` | Maps DIDs to Ethereum payout addresses |

### Payment Flow

1. User connects wallet (MetaMask, WalletConnect, etc.)
2. App calls `payPublisher(publisherDid, amountCents)`
3. Router resolves DID → payout address via OperatorRegistry
4. USDC is transferred: 95% to publisher, 5% to treasury
5. Transaction hash is returned for verification

### React Hook

```typescript
import { useSpaceKitPay } from "../hooks/useSpaceKitPay";

function PayButton() {
  const { payPublisher, isConfigured, isBusy } = useSpaceKitPay();

  const handlePay = async () => {
    const { txHash } = await payPublisher(
      "did:spacekit:user:publisher",
      400  // $4.00 in cents
    );
    console.log("Paid:", txHash);
  };

  return <button onClick={handlePay} disabled={!isConfigured || isBusy}>Pay</button>;
}
```

### Local Testing (Anvil)

```bash
# Start local EVM chain
anvil --chain-id 31337

# Deploy contracts (from spacekit.xyz-contracts/)
forge script script/DeploySpaceKitPayLocal.s.sol --rpc-url http://127.0.0.1:8545 --broadcast

# Configure .env.local
VITE_SPACEKIT_PAY_CHAIN_ID=31337
VITE_SPACEKIT_PAY_RPC_URL=http://127.0.0.1:8545
VITE_SPACEKIT_PAY_ROUTER_ADDRESS=0x...
VITE_SPACEKIT_PAY_USDC_ADDRESS=0x...
VITE_SPACEKIT_PAY_REGISTRY_ADDRESS=0x...
```

---

## Content Monetization

### Entitlement Flow

1. Publisher creates a listing on the entitlement ledger (WASM contract)
2. Access is minted by either:
   - **Paid:** buyer `OP_PURCHASE(listing, buyer_pk_hash)`, or
   - **Owner approve:** publisher `OP_GRANT(listing, recipient_did, recipient_pk_hash)`
3. Storage verifies via `OP_VERIFY` on `POST /files/{id}/rewrap`
4. Delivery:
   - **True E2E:** owner posts a recipient-wrapped DEK capsule (`PUT .../delivery-capsule`); storage streams ciphertext without unwrapping the DEK
   - **Server-wrapped blobs:** storage re-wraps the DEK header (bounded stream)

See `spacekit-js/docs/ENTITLEMENT_PROTOCOL.md` and helpers: `grantAndPrepareDelivery`, `downloadWithEntitlement`, `encryptEnvelopeWithFileKey`.

### Content APIs

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/content` | List content catalog |
| `GET` | `/api/content/:id/stream` | Stream content |
| `POST` | `/api/content/:id/view` | Record view |
| `POST` | `/api/content/:id/like` | Toggle like |
| `POST` | `/files/:id/rewrap` | Entitled download (storage node) |
| `PUT` | `/files/:id/delivery-capsule` | Owner posts E2E DEK capsule |

---

## MCP Server (Agent Tools)

The storage node provides a Model Context Protocol (MCP) server for AI agents:

```bash
spacekit-storage-node mcp \
  --data-dir /var/lib/spacekit \
  --did did:spacekit:mcp:operator
```

### Available Tools

| Tool | Purpose |
|------|---------|
| `tx_begin.v1` | Open ACID transaction |
| `tx_commit.v1` | Commit transaction |
| `tx_rollback.v1` | Rollback transaction |
| `tx_trace.v1` | Transaction audit log |
| `sandbox_create.v1` | Create ephemeral sandbox |
| `sandbox_commit.v1` | Commit sandbox |
| `sandbox_discard.v1` | Discard sandbox |
| `sandbox_journal.v1` | Inspect sandbox journal |
| `workspace_create.v1` | Create workspace |
| `workspace_get.v1` | Get workspace |
| `workspace_list.v1` | List workspaces |
| `workspace_export.v1` | Export workspace bundle |
| `workspace_import.v1` | Import workspace bundle |
| `blobs_replicate.v1` | Pull CAS blobs from remote |
| `upload_token_mint.v1` | Mint upload token |
| `graph_traverse.v1` | BFS over fact dependency DAG |

### Idempotency

MCP derives deterministic idempotency keys:
```
BLAKE3("mcp:" || tool_name || ":" || canonical_json(args))[..16]
```

---

## Growformer Agents

Growformer is SpaceKit's neural architecture for small, domain-specific "brains." Use this path for **companions** (e.g. Luna, Pete) and other train/infer agents — not for packaging a React SPA (that is [Ship a SpaceKit App](#ship-a-spacekit-app-vibe-coders)).

### Project Manifest (`brain.gf.toml`)

```toml
schema_version = 1

[project]
name = "My Brain"
description = "Domain-specific micro-brain"

[train]
code_brain = true          # enable code generation lattices
auto = true
encoder = "clifford_e8"
data_dir = "data"
brain_output = "agent/brain.bin"

[inference]
toml = "data/inference.toml"

[infer]
brain = "agent/brain.bin"
```

### Training Data Format (JSONL)

```json
{
  "task_id": "example_001",
  "text": "Implement binary search in Python",
  "semantic_intent": "coding_implementation",
  "domain": "coding_python",
  "action_target": "coding_algorithms",
  "code_language": "python",
  "expected_response": "Binary search works on sorted arrays...",
  "expected_code": "def binary_search(arr, target):\n    ..."
}
```

### Knowledge Graph (Topic Routing)

Topics are defined in `knowledge_graph.toml`:

```toml
[[nodes]]
topic = "binary_search_operation"
concept = "SearchAlgorithm"
category = "coding"
priority = 32
[[nodes.rules]]
any = ["binary search", "bisect", "search a sorted"]
```

User-contributed topics can be added via the SDK adapter (see `sdk/knowledge_adapter.py`).

---

## Spec-Driven SDK Generation (OpenAPI)

Generate production-ready client SDKs from an OpenAPI 3.x spec. This follows the **Stainless model**: spec → typed IR (`SpecModel`) → language emitter. Generation is deterministic; Growformer is optional for ambiguous heuristics.

### Supported languages

| Language | CLI `--lang` | Status |
|----------|--------------|--------|
| Python | `python`, `py` | ✅ stdlib client |
| TypeScript | `typescript`, `ts` | ✅ fetch-based, strict types |
| Rust | `rust` | 🔜 next (same IR) |

### Command

```bash
spacekit agent sdk \
  --spec path/to/openapi.yaml \
  --lang python \              # or typescript
  --out ./sdks \               # default: ./<package>_sdk
  --package my_api \           # optional; derived from spec title
  --check                      # python import or tsc --noEmit
```

**Incremental regen flags:**

| Flag | Purpose |
|------|---------|
| `--plan` | Dry-run: show `+ ~ = ! -` diff, write nothing |
| `--prune` | Delete orphaned generated files |
| `--force` | Overwrite hand-edited generated files |

Output includes `.sdkgen-manifest.json` (SHA-256 per file) for safe re-generation when the spec evolves.

### Runtime primitives (all languages)

Auth, retries/backoff, timeouts, typed errors with request IDs, auto-pagination, SSE streaming, webhook verification (Standard Webhooks), multipart uploads.

### Schema mapping

Enums → `Literal` / string unions; nullable → `Optional` / `| null`; `oneOf`/`anyOf` → union types; `allOf` → merged object types.

### Python example

```python
from acme_api import Client

client = Client(api_key="sk-…")
user = client.users.get_user(id="usr_1")
for page in client.users.list_users():
    print(page)
```

### TypeScript example

```typescript
import { Client } from "acme_api";

const client = new Client({ apiKey: "sk-…" });
const user = await client.users.getUser({ id: "usr_1" });
for await (const page of client.users.listUsers()) {
  console.log(page);
}
```

**Full reference:** `spacekit-cli/documentation/AGENT_SDK_GENERATION.md`  
**Implementation:** `spacekit-cli/src/full_client/sdkgen.rs`

---

## Deterministic Webapp Generation (OpenApp)

One layer above SDK generation: an **OpenApp v0.1** document describes a whole
application across **data / business / view**, and a **profile** binds it to a
stack. One spec + many profiles = many apps, identical behavior, different build.

```bash
spacekit agent webapp \
  --spec app.openapp.yaml \
  --profile react-postgres.profile.yaml \
  --out ./myapp \
  --check
```

Emits a data layer (`prisma/schema.prisma` **or** a `spacekit-storage-node`
document client + `server/storage-client.ts`), `server/` TS server actions
(business), a view layer (`web/app/` Next.js app-router **or** a `web/` Vite
React SPA), a typed `client/` SDK (capabilities → OpenAPI → the SDK generator
above), plus a profile-independent `.openapp-fingerprint.json`.

Two pre-emit validation passes: **spec cross-references** (dangling capability /
view / entity / policy refs, bad action inputs) and the **profile invariant** (a
profile may change realization, never meaning). Conformance:

```bash
spacekit agent webapp --spec app.openapp.yaml \
  --profile a.yaml --conformance b.yaml   # asserts identical behavioral hash
```

| Layer | Profile keys | Realized as |
|-------|--------------|-------------|
| data | `store`/`orm`/`identity`/`relations`/`naming` | Prisma (postgres/mysql/sqlite) **or** `spacekit-storage-node` (DID-scoped document store via a Prisma-shaped `db` adapter) |
| business | `language`/`transport`/`errors`/`emit_openapi` | TypeScript server actions |
| view | `framework`/`router`/`styling`/`tokens` | `next` → React app-router + Tailwind, **or** `react` → Vite SPA + react-router (fetches via the client SDK) |

Because the storage-node `db` mirrors the Prisma surface, swapping
`store: postgres` for `store: spacekit-storage-node` leaves the business/view
output identical — the behavioral fingerprint is unchanged (conformance holds).

**Spec:** `spacekit-cli/documentation/OPENAPP-SPEC-V0.1.md`  
**Profile:** `spacekit-cli/documentation/OPENAPP-PROFILE-V0.1.md`  
**Full reference:** `spacekit-cli/documentation/AGENT_SDK_GENERATION.md` (Webapp section)  
**Implementation:** `spacekit-cli/src/full_client/openapp.rs`

---

## Sandbox & Transaction System

### Sandbox Lifecycle

```
POST /api/sandboxes  →  201 Created { id, expires_at, quotas }
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
    POST /commit        POST /commit?dry_run    POST /discard
```

### Create Sandbox

```json
// POST /api/sandboxes
// Authorization: DID did:spacekit:user:alice
{
  "owner_did": "did:spacekit:user:alice",
  "workspace_id": "my-team",
  "max_bytes_written": 10485760,
  "collaborator_dids": ["did:spacekit:agent:bot"]
}
```

### Transaction Modifications

```json
// POST /api/transactions/{tx_id}/modifications
// X-Sandbox-Id: <sandbox-id>
// Idempotency-Key: <unique-key>
{
  "modification": { "...": "TransactionModification variant" },
  "conflict_policy": "Reject",
  "bytes_written": 0
}
```

---

## Federation

Workspaces and data can be migrated between SpaceKit operators:

### Export

```bash
spacekit workspace export my-team -o bundle.json
# Or: GET /api/workspaces/{id}/export
```

### Import

```bash
spacekit workspace import bundle.json --owner-did did:spacekit:dest:owner
# Or: POST /api/workspaces/import
```

The bundle includes workspace metadata, facts, blob manifests, and collaborator lists. Blob data is replicated separately via `blobs_replicate.v1`.

---

## Configuration

### Storage Node Environment

```bash
DATA_DIR=/var/lib/spacekit          # persistent data directory
BIND_ADDRESS=0.0.0.0:3030          # listen address
STORAGE_SECRET=<optional>          # shared secret for auth
```

### Website API Environment

```bash
STORAGE_NODE_URL=http://127.0.0.1:3030
STORAGE_NODE_SECRET=<optional>
ADMIN_DID=did:spacekit:admin:website-api
API_SECRET=<secret>
RESEND_API_KEY=<key>               # email service
FROM_EMAIL=hello@spacekit.xyz
MESSAGING_NODE_URL=http://127.0.0.1:3040
```

### Website Frontend Environment

```bash
VITE_API_URL=http://127.0.0.1:3001
VITE_STORAGE_NODE_URL=http://127.0.0.1:3030
VITE_SPACEKIT_PAY_CHAIN_ID=31337
VITE_SPACEKIT_PAY_RPC_URL=http://127.0.0.1:8545
VITE_SPACEKIT_PAY_ROUTER_ADDRESS=0x...
VITE_SPACEKIT_PAY_USDC_ADDRESS=0x...
VITE_SPACEKIT_PAY_REGISTRY_ADDRESS=0x...
```

### CLI Configuration (`~/.spacekit/config.toml`)

```toml
[identity]
did = "did:spacekit:user:alice"

[connections]
storage = "http://127.0.0.1:3030"
compute = "http://127.0.0.1:8545"
api = "http://127.0.0.1:3001"
```

---

## Common Patterns

### Uploading Content (Publisher)

```bash
# 1. Create identity
spacekit init && spacekit identity create --name publisher

# 2. Create a repository
spacekit repo init --name publisher/my-content --remote http://127.0.0.1:3030

# 3. Add files and push
spacekit repo add
spacekit repo commit -m "Initial content"
spacekit repo push

# 4. Register on marketplace (via website-api)
curl -X POST https://api.spacekit.xyz/api/repos \
  -H "owner-did: did:spacekit:user:publisher" \
  -H "Content-Type: application/json" \
  -d '{"owner_slug":"publisher","repo_slug":"my-content","owner_did":"did:spacekit:user:publisher","owner_type":"user"}'
```

### Building an Embedded App

```bash
# 1. Create React app
npm create vite@latest my-app -- --template react-ts
cd my-app && npm install

# 2. Use the iframe SDK
# In your app code:
const did = await window.spacekit?.call("identity", "did");

# 3. Package and deploy
npm run build
spacekit app package --name my-app --did did:spacekit:user:alice --dir dist/
spacekit app deploy my-app.spkg --storage-url http://127.0.0.1:3030
```

### Agent Integration (MCP)

```python
import json, sys

# Send MCP tool call via stdio
request = {
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
        "name": "workspace_list.v1",
        "arguments": {"owner_did": "did:spacekit:user:alice"}
    }
}
sys.stdout.write(json.dumps(request) + "\n")
sys.stdout.flush()

# Read response
response = json.loads(sys.stdin.readline())
```

---

## Website API Proxies

The website-api proxies requests to backend services so browser apps only need one origin:

| Browser Path | Forwards To |
|-------------|-------------|
| `/api/storage/*` | `STORAGE_NODE_URL/*` |
| `/api/documents/*` | `STORAGE_NODE_URL/api/documents/*` |
| `/api/messaging/*` | `MESSAGING_NODE_URL/*` |

Headers like `owner-did`, `owner-public-key`, `requester-did`, and challenge headers are forwarded transparently. The proxy may inject PQ keys from the DID registry for uploads.

---

## DID Registration (Username DIDs)

### Check Availability

```bash
GET /api/did/check/alice
# → { "available": true }
```

### Register

```json
// POST /api/did/register
{
  "username": "alice",
  "kyber_public_key": "<hex>",
  "eth_address": "0x...",
  "wallet_signature": "...",
  "signed_message": "..."
}
// → { "ok": true, "did": "did:spacekit:user:alice", "username": "alice", "airdrop_astra": 100 }
```

### Resolve

```bash
GET /api/did/resolve/alice
# → DID document with public keys
```

### Link Wallet / Kyber Key (Authenticated)

```bash
POST /api/did/link-wallet   # Authorization: Bearer <session>
POST /api/did/link-kyber    # Authorization: Bearer <session>
```

---

## Upload Tokens (Browser CAS Uploads)

For browser-based blob uploads without exposing the DID private key:

1. **Mint token** (server-side or CLI):
```bash
POST /api/upload-tokens
Authorization: DID did:spacekit:user:alice
# → { "token": "skut1...", "expires_at": "..." }
```

2. **Use token for CAS uploads:**
```bash
PUT /blobs/{blake3-hash}
Authorization: UploadToken skut1...
Content-Type: application/octet-stream
<body: raw bytes>
```

See `upload-tokens.md` in the storage node docs.

---

## Structured Query DSL

All query endpoints (`POST /query/files`, `/query/facts`, `/query/users`, `/query/documents/{collection}`, `/query/aggregate`) accept the same DSL:

```json
{
  "filters": {
    "author_did": { "op": "Equals", "value": "did:spacekit:user:alice" },
    "size": { "op": "GreaterThanOrEqual", "value": 1024 }
  },
  "sort_by": "created_at",
  "sort_order": "desc",
  "limit": 50,
  "offset": 0
}
```

**Filter operators:** `Equals`, `NotEquals`, `Contains`, `StartsWith`, `EndsWith`, `In`, `NotIn`, `GreaterThan`, `GreaterThanOrEqual`, `LessThan`, `LessThanOrEqual`, `Exists`, `NotExists`

**Aggregation:**
```json
// POST /query/aggregate
{
  "collection": "facts",
  "group_by": "category",
  "aggregate": "count"
}
```

---

## JavaScript SDK (`@spacekit/sdk`)

Published as `@spacekit/sdk` — available import paths:

| Import | Contents |
|--------|----------|
| `@spacekit/sdk` | `SpacekitClient`, Kyber crypto, encoding, validation, tokens, errors |
| `@spacekit/sdk/react` | `SpacekitProvider`, `useSpacekit`, `useIdentity`, `useBalance`, `useExplorer`, `useVm`, `useKeys` |
| `@spacekit/sdk/kyber` | Post-quantum encryption (WASM) |
| `@spacekit/sdk/tokens` | ERC20/721 adapters |
| `@spacekit/sdk/encoding` | WASM contract byte encoding |
| `@spacekit/sdk/validation` | DID/amount/hex validators |
| `@spacekit/sdk/styles` | CSS styles |

### React Usage

```tsx
import { SpacekitProvider, useSpacekit, useIdentity } from "@spacekit/sdk/react";

function App() {
  return (
    <SpacekitProvider>
      <MyComponent />
    </SpacekitProvider>
  );
}

function MyComponent() {
  const { client } = useSpacekit();
  const { did, displayName } = useIdentity();
  return <p>Hello {displayName} ({did})</p>;
}
```

### Components

- `SpacekitWallet` — wallet UI
- `SpacekitExplorer` — block explorer
- `SpacekitIdentityCard` — DID card

---

## Blob/Fact Auth Modes (Operator Config)

Storage node operators can configure CAS auth strictness:

| Mode | Behavior |
|------|----------|
| `permissive` | No auth required for blob/fact reads or writes |
| `hybrid` | Writes require DID or UploadToken; reads open |
| `strict` | All access requires DID auth |

Set via storage node config. Default is `permissive` for local development.

---

## Agent Hub (Website)

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/agents` | List published agents |
| `GET` | `/api/agents/:agent_id/artifacts/:type/challenge` | Get download challenge |
| `GET` | `/api/agents/:agent_id/artifacts/:type/stream` | Stream artifact (no timeout) |
| `GET` | `/api/agents/:agent_id/artifacts/:type/fetch` | Download artifact |

### Publishing an Agent

```bash
# 1. Train the brain
spacekit agent train --project brain.gf.toml

# 2. Deploy artifacts to storage
spacekit storage deploy \
  --wasm agent/brain.wasm \
  --bin agent/brain.bin \
  --receipt deploy.json \
  --agent-id my-agent \
  --publish

# 3. Build registry manifest
spacekit brain-registry build \
  --gf-toml brain.gf.toml \
  --receipt deploy.json

# 4. Publish to brain registry
spacekit brain-registry publish --manifest brain-manifest.json
```

---

## Quick Integration Paths

| Goal | Primary APIs / Commands |
|------|------------------------|
| Store JSON by DID | `PUT /api/documents/{collection}/{id}` with `Authorization: DID` header |
| Upload encrypted file | `POST /files/upload` with `owner-did` + `owner-public-key` headers |
| Git-like code hosting | `/blobs` + `/facts` + ref docs, or `spacekit repo` CLI |
| Website login | `/api/auth/passkey/*` or `/api/auth/email/*` → Bearer session (~30 days) |
| Browser CAS upload | Mint upload token → `PUT /blobs/{hash}` with `UploadToken` |
| Buy app/content | SpaceKit Pay `payForService` → `POST /api/marketplace/purchase` |
| Run AI brain | `spacekit agent infer --brain brain.bin --prompt "..."` |
| Generate API SDK from OpenAPI | `spacekit agent sdk --spec openapi.yaml --lang python\|typescript` |
| Publish agent | `storage deploy` → `brain-registry build` → `brain-registry publish` |
| Agent sandbox | `POST /api/sandboxes` + `/api/transactions` API |
| Browser React app | `@spacekit/sdk/react` + proxied storage via website API |
| Federation | `workspace export` → `workspace import` on destination node |

---

## Source of Truth

| Surface | Canonical File |
|---------|---------------|
| **This integration reference (LLM handoff)** | `SPACEKIT_SDK_REFERENCE.md` (also rendered at `/docs/backend-integration`) |
| Storage Node HTTP routes | `spacekit-storage-node/src/api/mod.rs` |
| Agentic/Federation routes | `spacekit-storage-node/src/api/agentic_routes.rs` |
| Website API routes | `spacekit.xyz-website-api/src/main.rs` |
| CLI commands | `spacekit-cli/src/full_client.rs` + submodules |
| OpenAPI SDK generation | `spacekit-cli/documentation/AGENT_SDK_GENERATION.md` · `spacekit-cli/src/full_client/sdkgen.rs` |
| JS SDK | `spacekit-sdk/lib/spacekit-sdk/index.ts` |
| SpaceKit Pay contracts | `spacekit-pay/SpaceKitPayRouter.sol` |
| Repo types | `spacekit-repo/src/lib.rs` |
| Storage node docs | `spacekit-storage-node/documentation/` |

*Last updated: June 2026.*
