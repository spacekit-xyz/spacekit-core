# Spec-Driven SDK Generation (`spacekit agent sdk`)

Generate typed, production-ready client SDKs from an OpenAPI 3.x spec. This is SpaceKit's **Stainless-style** pipeline: the spec is the source of truth; generation is deterministic and reproducible. Growformer is not on the critical path (neural hooks may assist naming/heuristics later where the spec is ambiguous).

```
OpenAPI spec (.yaml / .json)
        │
        ▼
   SpecModel IR          ← language-agnostic (schemas, resources, auth, pagination, …)
        │
        ├──────────┬──────────┬──────────
        ▼          ▼          ▼
     Python    TypeScript    Rust
```

**Implementation:** `spacekit-cli/src/full_client/sdkgen.rs`  
**CLI dispatch:** `spacekit agent sdk`

---

## Quick start

```bash
# Python (stdlib-only client)
spacekit agent sdk \
  --spec openapi.yaml \
  --lang python \
  --out ./sdks \
  --check

# TypeScript (fetch-based, strict tsconfig)
spacekit agent sdk \
  --spec openapi.yaml \
  --lang typescript \
  --out ./sdks \
  --check
# Rust (reqwest + serde, async)
spacekit agent sdk \
  --spec openapi.yaml \
  --lang rust \
  --out ./sdks \
  --check
```

Output layout:

```
<out>/
  README.md                    # generated usage notes
  .sdkgen-manifest.json        # SHA-256 manifest for incremental regen
  <package>/                   # e.g. acme_api/
    … language-specific files …
```

---

## CLI reference

| Flag | Default | Description |
|------|---------|-------------|
| `--spec <path>` | *(required)* | OpenAPI 3.x spec (`.yaml`, `.yml`, or `.json`) |
| `--out <dir>` | `./<package>_sdk` | Output root directory |
| `--package <name>` | derived from spec title | Package / crate name (snake_case for Python) |
| `--lang <lang>` | `python` | Target language: `python` / `py`, `typescript` / `ts`, `rust` / `rs` |
| `--check` | off | Post-generation verification (see below) |
| `--plan` | off | Dry-run: print diff plan, write nothing |
| `--prune` | off | Delete orphaned generated files (see incremental regen) |
| `--force` | off | Overwrite hand-edited generated files |

### `--check` behavior

| Language | Check |
|----------|-------|
| Python | `python3 -c "import <package>"` from `--out` |
| TypeScript | `tsc --noEmit -p tsconfig.json` in package dir (skipped if `tsc` not on PATH) |
| Rust | `cargo check` in package dir (skipped if `cargo` not on PATH) |

---

## Language support

| Language | Status | Runtime | Dependencies |
|----------|--------|---------|--------------|
| **Python** | ✅ stable | stdlib `urllib` | none |
| **TypeScript** | ✅ stable | `fetch` (Node 18+ / browsers) | dev: `typescript` for `--check` |
| **Rust** | ✅ stable | `reqwest` + `serde` + `tokio` | `reqwest`, `serde`, `serde_json`, `tokio`, … |

All three languages consume the same `SpecModel` IR — no spec-parser duplication.

---

## Generated package layout

### Python

```
<package>/
  __init__.py       # exports Client, models, resources
  _client.py        # HTTP client, auth, retries, SSE, multipart
  models.py         # dataclasses + from_dict / to_dict
  resources.py      # one class per API resource + Webhooks (if spec has webhooks)
```

**Usage:**

```python
from acme_api import Client

client = Client(api_key="sk-…")

# Regular call
user = client.users.get_user(id="usr_1")

# Auto-paginated list (cursor / offset detected from spec)
for page in client.users.list_users(status="active"):
    print(page)

# SSE stream
for event in client.events.stream_events():
    print(event)

# Multipart upload
client.users.upload_avatar(id="usr_1", file=open("avatar.png", "rb").read(), caption="hi")

# Webhook verification (Standard Webhooks HMAC-SHA256)
payload = client.webhooks.unwrap(body, headers)
```

**Types:** dataclass fields use `Literal`, `Union`, `Optional`, and nested model refs. Named enums and unions from the spec are emitted as module-level type aliases before the dataclasses.

### TypeScript

```
<package>/
  package.json
  tsconfig.json     # strict
  src/
    index.ts
    client.ts
    models.ts       # interfaces + type aliases
    resources.ts
```

**Usage:**

```typescript
import { Client } from "acme_api";

const client = new Client({ apiKey: "sk-…" });

const user = await client.users.getUser({ id: "usr_1" });

for await (const page of client.users.listUsers({ status: "active" })) {
  console.log(page);
}

for await (const event of client.events.streamEvents()) {
  console.log(event);
}

await client.users.uploadAvatar({
  id: "usr_1",
  file: new Blob([bytes]),
  caption: "hi",
});

const payload = await client.webhooks.unwrap(body, headers);
```

**Types:** string enums → `"a" | "b"`, nullable → `T | null`, unions → `A | B`, `allOf` composition → merged interfaces.

### Rust

```
<crate>/
  Cargo.toml
  src/
    lib.rs
    client.rs       # ClientCore + Client (async reqwest)
    models.rs       # serde structs + enum / union aliases
    resources.rs    # resource methods + Webhooks (if spec has webhooks)
```

**Usage:**

```rust
use acme_api::{Client, ClientOptions};

#[tokio::main]
async fn main() -> Result<(), acme_api::APIError> {
    let client = Client::new(ClientOptions {
        api_key: Some("sk-…".into()),
        ..Default::default()
    })?;

    let user = client.users.get_user("usr_1".to_string()).await?;
    let all_pages = client.users.list_users(None).await?;

    Ok(())
}
```

**Types:** string enums → `enum Status`, nullable → `Option<T>`, `oneOf` → `#[serde(untagged)]` enum, `allOf` → merged struct fields.

---

## Runtime primitives (all languages)

Every emitted client includes:

| Feature | Description |
|---------|-------------|
| **Authentication** | Bearer token or API-key header (from spec `securitySchemes`) |
| **Retries** | Exponential backoff on 429 / 5xx |
| **Timeouts** | Configurable per client |
| **Typed errors** | `APIError` with status, message, request ID, body |
| **Pagination** | Auto-detected cursor / offset list endpoints → async generators (Python/TS) or collected `Vec` (Rust) |
| **Streaming** | SSE (`text/event-stream`) → generator / async iterator |
| **Webhooks** | Standard Webhooks signature verify + unwrap (when spec declares `webhooks`) |
| **Multipart** | `multipart/form-data` file + field uploads |

---

## OpenAPI schema support

The parser builds a typed **SpecModel** IR, then each emitter renders language-specific syntax.

| OpenAPI construct | IR | Python emit | TypeScript emit | Rust emit |
|-------------------|-----|-------------|-----------------|-----------|
| `type: object` + `properties` | `Schema` (dataclass / interface) | `@dataclass` | `interface` | `struct` + `Serialize`/`Deserialize` |
| `type: string` + `enum` | `TypeRef::Enum` | `Status = Literal[…]` | `type Status = "a" \| "b"` | `enum Status` |
| `nullable: true` / `type: [T, null]` | `Union(T, Null)` | `Optional[T]` | `T \| null` | `Option<T>` |
| `oneOf` / `anyOf` | `TypeRef::Union` | `Pet = Union["Cat", "Dog"]` | `type Pet = Cat \| Dog` | `#[serde(untagged)] enum Pet` |
| `allOf` (composition) | merged `Schema` | single dataclass w/ inherited fields | merged interface | merged struct fields |
| `$ref` to component | `TypeRef::Ref` | forward-ref + `from_dict` | interface name | type name + `from_value` |
| `type: array` | `TypeRef::Array` | `List[T]` | `T[]` | `Vec<T>` |
| `additionalProperties` | `TypeRef::Map` | `Dict[str, T]` | `Record<string, T>` | `HashMap<String, T>` |

Component schemas are classified as **objects** (get `from_dict` / interface) or **aliases** (enum / union / scalar type alias). Alias refs never incorrectly call `.from_dict()`.

---

## Incremental regeneration

Re-running `agent sdk` against an evolved spec does **not** blindly overwrite the output tree. A `.sdkgen-manifest.json` at `--out` stores SHA-256 hashes of every generated file from the last run.

| Symbol | Status | Behavior |
|--------|--------|----------|
| `+` | Added | New file → written |
| `~` | Modified | Tracked, unchanged on disk since last gen → overwritten |
| `=` | Unchanged | Identical hash → skipped |
| `!` | Conflict | Hand-edited since last gen → **kept**; use `--force` to overwrite |
| `-` | Orphaned | Previously generated, no longer emitted → reported; `--prune` deletes if still matches manifest |

### Typical workflow

```bash
# 1. Initial generation
spacekit agent sdk --spec openapi.yaml --lang python --out ./sdks

# 2. Preview changes after spec update (no writes)
spacekit agent sdk --spec openapi.yaml --lang python --out ./sdks --plan

# 3. Apply incremental update
spacekit agent sdk --spec openapi.yaml --lang python --out ./sdks

# 4. If you hand-edited a generated file and need to reconcile
spacekit agent sdk --spec openapi.yaml --lang python --out ./sdks --force

# 5. Rename package / remove stale files
spacekit agent sdk --spec openapi.yaml --lang python --out ./sdks \
  --package new_name --prune
```

Hand-edit protection is intentional: developers can patch generated code locally; the next regen surfaces conflicts instead of silently destroying edits.

---

## CI integration (recommended)

```yaml
# Example: regen SDKs when the OpenAPI spec changes
- name: Regenerate Python SDK
  run: |
    spacekit agent sdk \
      --spec api/openapi.yaml \
      --lang python \
      --out sdks \
      --check

- name: Regenerate TypeScript SDK
  run: |
    spacekit agent sdk \
      --spec api/openapi.yaml \
      --lang typescript \
      --out sdks \
      --check

- name: Regenerate Rust SDK
  run: |
    spacekit agent sdk \
      --spec api/openapi.yaml \
      --lang rust \
      --out sdks \
      --check

- name: Review diff
  run: git diff --stat sdks/
```

Use `--plan` in PR comments to show what would change before merging a spec update.

---

## Sample specs

Test fixtures live under `spacekit/tmp_specs/`:

| File | Exercises |
|------|-----------|
| `acme.yaml` | CRUD, bearer auth, SSE, webhooks, multipart |
| `schemas.yaml` | enums, nullable, `oneOf`, `allOf` composition |

---

## Relationship to Growformer

| Layer | Role |
|-------|------|
| **OpenAPI spec** | Authoritative API contract |
| **SpecModel IR + emitters** | Deterministic SDK output (this module) |
| **Growformer** | Optional: naming heuristics, ambiguous pagination, docstrings — not required for generation |

The Python Dev Session (`spacekit agent code`) and SDK generation share the same philosophy: **retrieve/emit deterministically first**, use the brain where the spec or prompt is under-specified.

---

## Roadmap

- [x] OpenAPI 3.x → SpecModel IR
- [x] Python emitter + runtime primitives
- [x] TypeScript emitter + runtime primitives
- [x] Rich schemas (enum, nullable, union, allOf)
- [x] Incremental regen + hand-edit protection
- [x] **Rust emitter** + runtime primitives
- [x] **OpenApp webapp generation** (`spacekit agent webapp`) — see below
- [ ] 3-way merge for conflict resolution (`spacekit-diff`)
- [ ] CI helper: spec change → regen → open PR

---

# Deterministic Webapp Generation (`spacekit agent webapp`)

The OpenAPI SDK generator above describes a *client* for an API. **OpenApp**
goes one layer up: a single declarative document describes a whole application
across three layers — **data**, **business**, **view** — and a **profile**
binds it to a concrete stack. One spec + many profiles = many apps that behave
identically and are built differently.

```
app.openapp.yaml  ─┐
                   ├─► AppModel IR ─► validate(spec) ─► validate(spec × profile)
profile.yaml ──────┘                                          │
        ┌─────────────────────┬───────────────────────┬───────────────────────┐
        │ data emitter        │ business emitter      │ view emitter          │
        │ Prisma  │           │ (TS server actions)   │ Next.js app-router  │ │
        │ spacekit-storage-   │                       │ React (Vite) SPA      │
        │ node document db    │                       │                       │
        └─────────────────────┴───────────────────────┴───────────────────────┘
                                   │
              capabilities ─► synthesize OpenAPI ─► (reuse) the SDK
              generator above ─► a typed client SDK
```

**Spec:** `OPENAPP-SPEC-V0.1.md` · **Profile:** `OPENAPP-PROFILE-V0.1.md`
**Implementation:** `src/full_client/openapp.rs` (+ `openapp_data.rs`,
`openapp_business.rs`, `openapp_view.rs`)

## Quick start

```bash
spacekit agent webapp \
  --spec app.openapp.yaml \
  --profile react-postgres.profile.yaml \
  --out ./myapp \
  --check
```

`--profile` is the OpenApp analogue of the SDK generator's `--lang`: it chooses
the stack (datastore, ORM, transport, framework, styling) and patterns. Omit it
for the built-in default (postgres + prisma + next + server-actions). The same
incremental flags apply: `--plan`, `--prune`, `--force`.

## What it emits

The constant parts plus a data tree and a view tree that vary by profile.

```
<out>/
  openapi.json                 # capabilities projected to OpenAPI
  client/<app>_client/         # generated typed client SDK (reuses the SDK generator)
  server/                      # business layer: one action per capability + db.ts
  .openapp-fingerprint.json    # behavioral contract (profile-independent)
  .sdkgen-manifest.json        # shared incremental-regen manifest
  README.md

  # data layer — store: postgres | mysql | sqlite  (Prisma)
  prisma/schema.prisma         # entities → models, relations, enums
  server/db.ts                 # PrismaClient singleton

  # data layer — store: spacekit-storage-node  (DID-scoped document store)
  server/storage-client.ts     # HTTP client for /api/documents/{collection}/{id}
  server/db.ts                 # Prisma-shaped adapter over the document API
  server/collections.md        # entity → collection map

  # view layer — framework: next  (app-router / RSC / server actions)
  web/app/**/page.tsx          # async server components; db + server-action bindings
  web/components/*.tsx · web/app/tokens.css

  # view layer — framework: react  (Vite SPA)
  web/index.html · web/vite.config.ts
  web/src/{main,App,api}.tsx   # react-router + client-SDK wiring
  web/src/pages/*.tsx          # client components fetching via the client SDK
  web/components/*.tsx · web/src/tokens.css
```

> **Why a Prisma-shaped storage adapter?** The storage-node `server/db.ts`
> exposes the same `findUnique`/`findFirst`/`findMany`/`create`/`update`/`delete`
> surface as the Prisma client, so the business and view layers are **byte-for-byte
> identical** across relational and document stores — only `db.ts` (+
> `storage-client.ts`) change. That is what keeps conformance intact when you swap
> `store: postgres` for `store: spacekit-storage-node`.

## The two validation passes (the headline feature)

Before a single file is emitted, two checks run — both catch whole classes of
bugs that a hand-wired stack would only surface at runtime:

1. **Spec cross-references** (`OPENAPP-SPEC` §13): a view binding a non-existent
   capability, an action passing an input the capability doesn't declare, a
   transition to an undefined view, a capability writing an entity it never
   declared, a dangling policy/event/entity reference.
2. **Profile invariant** (`OPENAPP-PROFILE` §1): a profile may only choose
   *realization*, never *meaning*. Meaning-bearing keys (`capabilities`,
   `entities`, `writes`, …) and overrides of unknown `@Name`s are rejected.

## Profile → realization map

| Layer | Profile keys | Realized as (supported today) |
|-------|--------------|-------------------------------|
| `data` | `store`, `orm`, `identity`, `relations`, `migrations`, `naming` | **`store: postgres\|mysql\|sqlite`** → Prisma schema (uuid/cuid/serial ids, `@@map` for snake_case, synthesized back-relations). **`store: spacekit-storage-node`** → DID-scoped document client + Prisma-shaped `db` adapter (entity → collection, no migrations) |
| `business` | `language`, `binding`, `transport`, `architecture`, `errors`, `emit_openapi` | TypeScript server actions; `problem-json` (throw `ApiError`) or `result-type` (`Result<T>`); REST or RPC OpenAPI projection |
| `view` | `framework`, `state`, `router`, `styling`, `tokens` | **`framework: next`** (default) → app-router server components with server-action + `db` bindings. **`framework: react`** → Vite SPA (react-router pages, client components fetching through the generated client SDK). Both share prop-driven widgets and tokens → CSS custom properties |
| client SDK | (from `business.language`) | Python / TypeScript / Rust via the generator above |

Unsupported choices (e.g. `framework: vue`, `orm: drizzle`, `binding: runtime`)
fail validation with a clear message rather than emitting broken code.

> **SPA caveat.** In `framework: react`, pages fetch through the client SDK
> (capabilities are the API surface). A view binding that reads an `@Entity`
> *directly* has no endpoint in SPA mode, so the emitter inserts a `// TODO`
> marker — expose a capability for that read, or use a `framework: next` profile
> where server components query `db` directly.

## `--check` behavior

Validation always runs. With `--check`, the generated **client SDK** is also
type-checked with the real toolchain (`tsc` / `cargo check` / `python import`) —
the same external gate the SDK generator uses.

## Operating the generated app

Generation produces source, not a running system. After `cd <out>` there are
three things to wire: the **data backend**, the **server** (business actions),
and the **view**. What you do depends on the profile you generated with.

### 1. Data backend

**`store: postgres | mysql | sqlite` (Prisma):**

```bash
cp .env.example .env                 # set DATABASE_URL
cd server && npm install
npx prisma generate --schema ../prisma/schema.prisma   # client + types
npx prisma migrate dev --schema ../prisma/schema.prisma --name init   # create tables
```

`server/db.ts` is the `PrismaClient` singleton everything imports.

**`store: spacekit-storage-node` (DID-scoped document store):**

```bash
# 1. Start a storage node (HTTP API defaults to 127.0.0.1:3030)
cd /path/to/spacekit-storage-node
cargo run --bin standalone -- --port 3030      # or your deployed node

# 2. Point the app at it
cd <out> && cp .env.example .env               # edit if needed
#   SPACEKIT_STORAGE_URL=http://127.0.0.1:3030
#   SPACEKIT_DID=did:spacekit:user:local
cd server && npm install
```

No migrations: collections are created on first write. Every document is scoped
to `SPACEKIT_DID`, so changing that env value switches tenants. Entity → collection
mapping is listed in `server/collections.md`. The Prisma-shaped `server/db.ts`
adapter translates `findFirst`/`findMany`/… into `/api/documents/...` calls and
`POST /query/documents/{collection}` for filtered reads.

> The data backend choice is invisible to the business and view layers — the same
> `import { db } from "../db"` works either way.

### 2. Business layer (server)

`server/actions/<capability>.ts` is one typed action per capability, wired to
`db` and `server/types.ts`. The generated body is a stub that you fill in with
the real logic; inputs/outputs and the error surface are already typed:

- `errors: problem-json` → throw `ApiError` (in `server/errors.ts`).
- `errors: result-type` → return `Result<T>` (`{ ok: true, value } | { ok: false, error }`).

Implemented action bodies are **protected on regeneration** (see day-2 ops
below), so it is safe to edit them in place.

### 3. View layer (web)

**`framework: next` (app-router):** `web/` is a Next.js app-router tree whose
pages are async server components calling `db` and the server actions directly,
so the data backend must be reachable at render time.

```bash
cd web && npm install
# add next scripts to package.json: "dev": "next dev", "build": "next build", "start": "next start"
npm run dev        # http://localhost:3000
```

**`framework: react` (Vite SPA):** `web/` is a Vite + react-router SPA whose
pages fetch through the generated client SDK (`web/src/api.ts`). It needs the
API reachable over HTTP:

```bash
cd web && npm install
echo 'VITE_API_BASE_URL=http://localhost:8080' >> .env   # where your API is served
echo 'VITE_API_KEY=...'                       >> .env     # if the API requires auth
npm run dev        # vite, http://localhost:5173
```

### 4. Client SDK

`client/<app>_client/` is the typed client the SPA consumes and that any external
caller can use. Build/usage notes are in its own `README.md`. It is regenerated
from `openapi.json`, which is itself synthesized from the spec's capabilities.

### Day-2 ops (re-generation)

Re-run the same command after the spec or profile changes. The shared manifest
makes this safe:

```bash
spacekit agent webapp --spec app.openapp.yaml --profile p.yaml --out ./myapp --plan   # preview diff
spacekit agent webapp --spec app.openapp.yaml --profile p.yaml --out ./myapp          # apply
```

- Files you hand-edited (e.g. filled-in action bodies) are detected and **kept**
  as *conflicts* — they are never silently overwritten.
- `--force` overwrites hand-edited files; `--prune` removes files no longer
  emitted (orphans).
- **Switching stack:** change `store` and/or `framework` in the profile and
  re-run. For a store swap the business/view output is identical (only `db.ts`
  + data files change); for a framework swap the `web/` tree is replaced — use
  `--prune` to drop the old framework's files.

## Conformance (behavioral equivalence)

```bash
spacekit agent webapp --spec app.openapp.yaml \
  --profile react-postgres.profile.yaml \
  --conformance react-sqlite.profile.yaml
```

Both profiles are validated, then a canonical **behavioral fingerprint** (the
meaning-bearing surface only: entities, capabilities, writes, policies, events,
view wiring — never stack choices) is hashed for each. Equal hashes prove the
profile invariant held: same app, different build. The fingerprint is also
written to `.openapp-fingerprint.json` on every generation.

## Sample fixtures

| File | Exercises |
|------|-----------|
| `tmp_specs/bookshop.openapp.yaml` | entities/relations/enums, queries + commands, events, policies, widgets, views, flows, tokens |
| `tmp_specs/react-postgres.profile.yaml` | postgres + prisma + uuid + server-actions + problem-json + **next**/tailwind |
| `tmp_specs/react-sqlite.profile.yaml` | sqlite + cuid + snake_case + rpc + result-type + **react SPA**/css-modules (same behavior) |
| `tmp_specs/storage-next.profile.yaml` | **spacekit-storage-node** document store + server-actions + next/tailwind (same behavior — proves the store is realization-only) |

## Relationship to the SDK generator

OpenApp does not replace OpenAPI generation — it **feeds** it. The
transport-neutral `capabilities` are projected to an OpenAPI document
(`emit_openapi: true`), which the Python/TypeScript/Rust emitters above turn
into the app's typed client. The two share one IR philosophy, one incremental
manifest, and one `--check` discipline.

---

*Last updated: June 2026 — added `spacekit-storage-node` data store, `next`/`react`
framework choices, and the "Operating the generated app" guide.*
