# The SpaceKit App Profile

**Version 0.2: Draft.** Companion to *The Spacekit Specification v0.2*.

A **profile** binds a pattern-free Spacekit document to a concrete stack: datastore, transport, frontend framework, orchestration engine, and the architectural patterns `spacekit-cli` should emit. One spec + many profiles = many generated apps that behave identically and are built differently.

---

## 1. The one non-negotiable rule (unchanged)

> **A profile changes the *realization* of an app, never its *meaning*.**

A profile MUST NOT alter the set of capabilities, the fields or identity of an entity, the effect of a `write`, a `policy`, an `emit`, a `transition`, or new in v0.2, the steps, compensation, or outcomes of a `workflow`. It MAY only choose how those become code and infrastructure. `spacekit-cli` rejects a profile that attempts to change behavior; §8 makes that checkable.

---



## What's new in v0.2

- `business.workflows`: choose the orchestration engine that realizes the spec's new `workflows`. (§4)
- `extends` **+** `environments`: profile inheritance and dev/prod layering. (§2, §7)
- **Conformance checking**: `spacekit-cli conform` verifies two profiles produce behaviorally-equivalent apps. (§8)
- **Token interop**: emit `tokens` as W3C Design Tokens, Style Dictionary, Tailwind config, or CSS variables. (§5)
- Per-environment `binding`. (§7)

---



## 2. Document structure

```yaml
spacekit-profile: "0.2"
name: react-postgres
spec: ./bookshop.spacekit.yaml
extends: ./base.profile.yaml      # inheritance; overrides win v0.2

data:         { ... }
business:     { ... }             # now includes `workflows` v0.2
view:         { ... }
output:       { ... }
environments: { ... }             # dev/prod layering (NEW)
```

`extends` merges a base profile; the extending profile's keys override. Merge is deep; `overrides` maps merge by key.

---



## 3. `data`: realizing `entities`

Unchanged from v0.1: `store`, `orm`, `identity`, `relations` (`referenced`/`embedded`), `migrations`, `naming`, plus per-entity `overrides`. The `rules` added to the spec in v0.2 are realized here as DB constraints and/or validation in the data-access layer; no new profile key is required.

```yaml
data:
  store: postgres
  orm: prisma
  identity: uuid
  relations: referenced
  migrations: true
```

---



## 4. `business`: realizing `capabilities`, `events`, and `workflows`

Capability/event keys are unchanged (`language`, `binding`, `transport`, `architecture`, `errors`, `emit_openapi`, per-capability `overrides`). v0.2 adds a `workflows` block.

```yaml
business:
  language: typescript
  binding: compile
  transport: server-actions
  architecture: hexagonal
  errors: problem-json
  emit_openapi: true
  workflows:                       # NEW — realizes spec §7
    engine: temporal               # in-process | durable-queue | temporal | inngest
    retries: 3                     # per-step retry budget before compensation
    timeout: 30s
```



### `workflows.engine`

The spec's `workflows` define *what* must happen and *how it fails*; the profile picks the runtime that enforces it:

- `in-process` — synchronous orchestration in the same process; compensation in a try/catch. Fine for small apps, no durability across crashes.
- `durable-queue` — steps as queued jobs with a compensation log.
- `temporal` **/** `inngest` — managed durable execution; the generator emits the workflow as that engine's primitive.

Swapping the engine cannot change the saga semantics — `retries` and `timeout` tune execution, they do not alter which compensations run. That invariant is checked by §8.

> The compile-vs-runtime floor still holds: because the spec authors capabilities as exhaustive contracts (Spec §6), `binding: runtime` remains selectable per environment (§7) without re-authoring.

---



## 5. `view`: realizing `widgets`, `views`, `flows`, `text`, `tokens`

Unchanged keys: `framework`, `state`, `router`, `data_fetching`, `styling`. The spec's `states` (loading/empty/error) and `text` (i18n) are realized here without new keys — `data_fetching` drives how `states` are wired; the active `locale` is set under `environments`.

`tokens` gains interop targets:

```yaml
view:
  framework: react
  state: server-components
  router: app-router
  data_fetching: rsc
  styling: tailwind
  tokens: w3c-design-tokens        # css-variables | tailwind-config | style-dictionary | w3c-design-tokens (NEW)
```

---



## 6. `output`: generator mechanics

Unchanged: `dir`, `packageName`, `fileNaming`. No effect on behavior.

---



## 7. `environments`: dev/prod layering (NEW)

A profile may define environments that override the base blocks. Common use: durable orchestration and runtime binding in prod, simpler in dev.

```yaml
environments:
  dev:
    business: { workflows: { engine: in-process } }
    view:     { locale: en }
  prod:
    business: { binding: runtime, workflows: { engine: temporal, retries: 5 } }
    view:     { locale: en }
```

`spacekit-cli generate --env prod` applies the matching layer over the base profile. Environments may only override realization keys; an environment that tries to change behavior is rejected by the same §1 rule.

---



## 8. Conformance v0.2

The §1 invariant is now checkable. `spacekit-cli conform <profileA> <profileB>` generates both against the same spec and asserts behavioral equivalence: identical capability signatures and error sets, identical workflow step/compensation graphs, identical policies, identical view transitions and data bindings. Differences in stack, file layout, orchestration engine, or token format are expected and ignored; any difference in the spec-derived behavior is a conformance failure.

This is what makes "swap the profile, same app" a tested guarantee rather than a claim.

---



## 9. Worked example: two profiles, one spec

Both target `bookshop.spacekit.yaml`. The `FulfillOrder` workflow, `PlaceOrder` contract, policies, and view flow are identical under both; only the stack and orchestration differ.

### Profile A: `flux-vue-mongo`

```yaml
spacekit-profile: "0.2"
name: flux-vue-mongo
spec: ./bookshop.spacekit.yaml

data:     { store: mongo, orm: mongoose, identity: objectid, relations: embedded }
business:
  language: typescript
  binding: compile
  transport: rpc
  architecture: transaction-script
  emit_openapi: false
  workflows: { engine: in-process, retries: 1 }
view:
  framework: vue
  state: pinia
  router: vue-router
  data_fetching: client
  styling: css-modules
  tokens: style-dictionary
environments:
  dev:  { business: { workflows: { engine: in-process } } }
  prod: { business: { workflows: { engine: durable-queue, retries: 3 } } }
```



### Profile B: `react-postgres`

```yaml
spacekit-profile: "0.2"
name: react-postgres
spec: ./bookshop.spacekit.yaml

data:     { store: postgres, orm: prisma, identity: uuid, relations: referenced, migrations: true }
business:
  language: typescript
  binding: compile
  transport: server-actions
  architecture: hexagonal
  errors: problem-json
  emit_openapi: true
  workflows: { engine: temporal, retries: 3, timeout: 30s }
view:
  framework: react
  state: server-components
  router: app-router
  data_fetching: rsc
  styling: tailwind
  tokens: w3c-design-tokens
environments:
  dev:  { business: { binding: compile, workflows: { engine: in-process } } }
  prod: { business: { binding: compile, workflows: { engine: temporal, retries: 5 } } }
```

Run `spacekit-cli generate --spec bookshop.spacekit.yaml --profile flux-vue-mongo`: Vue + Pinia, RPC, Mongo with embedded lines, `FulfillOrder` as an in-process saga. Run it with `react-postgres`: React Server Components, server actions over a hexagonal layer, Postgres, `FulfillOrder` as a Temporal workflow, plus an OpenAPI doc. `spacekit-cli conform flux-vue-mongo react-postgres` passes — same app, two stacks.

---



## 10. How spec + profile combine

```
  bookshop.spacekit.yaml   (what the app is — portable, pattern-free)
            +
  <profile>.profile.yaml --env <env>   (how to build it — stack + patterns + orchestration)
            ↓
        spacekit-cli  →  validate → conform → generate
            ↓
  data layer + business layer (+ workflow engine, + OpenAPI) + frontend
```

`spacekit-cli` validates the pairing before emitting: a profile referencing a definition not in the spec is an error; a profile or environment that tries to alter behavior fails conformance (§8).

---



## 11. Changes from v0.1

- Added `business.workflows` orchestration realization (§4), matching the spec's new `workflows`.
- Added `extends` inheritance and `environments` layering (§2, §7).
- Added `spacekit-cli conform` conformance checking (§8).
- Added `view.tokens` interop targets including W3C Design Tokens (§5).
- Per-environment `binding`.
- Aligned naming/version to the Spacekit brand and `spacekit-cli`.



## 12. Still deferred to v0.3+

Profile composition beyond single `extends` (mixins); deployment/hosting targets (the profile still stops at generated code); cost/scaling hints for orchestration engines; partial regeneration / drift detection against hand-edited output.