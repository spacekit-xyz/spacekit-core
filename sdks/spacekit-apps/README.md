# **SpaceKit Apps Specification**

> One declarative description of an application, **data**, **business**, and **view**; that compiles to any stack. On the web, and on-chain.

The SpaceKit App Specification describes an entire application, its data model, its business logic, and its user interface, in a single, versioned document. A companion **target profile** says which stack to build it on. The `spacekit` CLI reads both and generates the data layer, the API/SDK, and the frontend from one source of truth.

A **dApp extension** carries the same model on-chain: it binds to contract ABIs, builds an indexed read model from events, and treats transactions as first-class — without abandoning the spec/profile separation.

```
  bookshop.app.yaml  (what the app is)  +  profile.yaml  (how to build it)
                              │
                   spacekit spec generate
                              │
        data layer   +   business layer   +   frontend
```

---



## Why another spec

Spacekit is **not** the first attempt to describe an app declaratively, and it doesn't pretend to be. Each prior approach describes **one layer**:

- **Framework-agnostic UI codegen**: [TeleportHQ UIDL](https://github.com/teleporthq/teleport-code-generators), [Builder.io Mitosis](https://github.com/BuilderIO/mitosis): one component description → React/Vue/Svelte.
- **Runtime-delivered declarative UI**: Adaptive Cards, server-driven UI, generative-UI protocols.
- **API description + codegen**: OpenAPI describes HTTP APIs and generates clients/servers.

Spacekit's bet is the **seam between layers**. What none of the above gives you is a single document where a view's data binding, the capability it invokes, the entity that capability writes, and the workflow that entity's events trigger are all cross-referenced and checkable together. That unified contract, not "a declarative UI format", is the point.

Two design choices follow:

1. **One document, three layers, explicit links.** Every definition is referenceable by name via `@Name`. Break a reference and generation fails before any code is emitted.
2. **The spec says *what*; the profile says *how*, and the profile can never change *what*.** The same description generates a Vue + Pinia + Mongo app or a React Server Components + Postgres app by swapping profiles. The invariant: **a profile changes realization, never behavior.** `spacekit spec conform` tests it.

---



## On-chain too: the dApp extension

The same prior-art honesty applies to smart contracts, where three separate traditions already exist:

- **Interface description**: the contract **ABI** is, in effect, the OpenAPI of Ethereum: a mandatory, compiler-emitted interface that tools generate typed clients from.
- **Declarative behavior**: [Daml](https://github.com/digital-asset/daml) independently arrived at almost Spacekit's shape: data, authorization policies, and composable workflows as first-class. It's prior art worth learning from, not competing with.
- **Formal specification**: Certora's CVL, Scribble, Scilla, and the SMTChecker write specs to *prove existing code correct* — the opposite direction from generating an app.

The gap none of them fills is a **unified, cross-layer dApp description**. Spacekit's dApp extension fills it on four principles:

1. **Bind to the ABI; never supersede it.** Functions, events, and custom errors are referenced (`@abi.Bookshop.buyBook`), never restated. The ABI is mandatory and authoritative.
2. **The ABI is not the data layer.** On-chain entities are an **indexed read model** over events (The Graph / Ponder style), not contract storage.
3. **A capability is a transaction with a lifecycle**, not a request/response: `building → submitted → mining → confirmed/reverted → finalized`.
4. **Compensation breaks on-chain.** Workflows split three ways — off-chain steps compensate (saga), same-chain on-chain steps bundle atomically (revert = nothing happened), and cross-chain steps are state machines with forward recovery.

---



## A taste

**Web** (`bookshop.app.yaml`):

```yaml
spacekit: "0.2"
capabilities:
  PlaceOrder: { writes: [{ updates: Order }], emits: [OrderPlaced], policy: "@OwnsOrder" }
workflows:
  FulfillOrder:                 # saga with compensation
    on: OrderPlaced
    steps:
      - { do: ReserveStock,  compensate: ReleaseStock }
      - { do: ChargePayment, compensate: RefundPayment }
```

**On-chain** (`bookshop.dapp.yaml`):

```yaml
spacekit: "0.2"
dapp: "0.2"
contracts:
  Bookshop: { abi: ./abi/Bookshop.json, addresses: { main: "0x…" } }
entities:
  Book: { source: { index: Bookshop, from: [BookListed, Restocked, BookPurchased] } }
capabilities:
  BuyBook: { kind: transaction, call: "@abi.Bookshop.buyBook", signer: connectedWallet, value: input.price }  # ASTRA
workflows:
  Purchase:
    steps:
      - { do: BuyBook, chain: onchain }                                   # atomic, no compensation
      - { do: SendReceiptEmail, chain: offchain, compensate: VoidReceipt }
```

Same bookshop, same three-layer model, one of them decentralized.

---



## Repository contents


| File | What it is |
| --- | --- |
| [SPACEKITAPP-SPEC.md](./SPACEKITAPP-SPEC.md) | **The SpaceKit App Specification.** The base format across data, business, and view: entities, capabilities, events, workflows, policies, widgets, views, flows, text, tokens, with reference and compatibility rules. |
| [SPACEKITAPP-PROFILE.md](./SPACEKITAPP-PROFILE.md) | **The SpaceKit Target Profile.** How a profile binds a spec to a concrete stack, with inheritance, environments, and conformance. |
| [SPACEKIT-PLUGIN-SPEC.md](./SPACEKIT-PLUGIN-SPEC.md) | **SpaceKit Plugins.** Features one author publishes and others add to their apps (UI, services, contracts, brains): descriptor, install rules, data ownership, ASTRA pricing, signatures. |
| [SPACEKIT-DAPP-SPEC.md](./SPACEKIT-DAPP-SPEC.md) | **The SpaceKit dApp Specification (draft).** The on-chain extension: networks, ABI import, indexed entities, transaction capabilities, finality/reorg handling, account abstraction, cross-chain workflows, on-chain policies. |
| [examples/bookshop.app.yaml](./examples/bookshop.app.yaml) | Complete web example: the bookshop as an off-chain app, with a fulfillment saga. |
| [examples/gallery.app.yaml](./examples/gallery.app.yaml) | Complete web example: a media gallery with albums, uploads and storage buckets. |
| [examples/nextjs-storage.profile.yaml](./examples/nextjs-storage.profile.yaml) | A profile for the gallery: Next.js + Postgres, with dev/prod environments. |
| [examples/bookshop.dapp.yaml](./examples/bookshop.dapp.yaml) | dApp example (draft): the bookshop on the SpaceKit network, paid in ASTRA with an NFT receipt. |


---



## Quick start

The generator is part of the `spacekit` CLI (in `spacekit-core/tools/spacekit-cli`). It runs offline; no account or `spacekit init` is needed.

```bash
# build the CLI from source
cargo install --path tools/spacekit-cli        # from a spacekit-core checkout

cd sdks/spacekit-apps/examples

# validate a spec (and optionally a profile)
spacekit spec check --spec bookshop.app.yaml
spacekit spec check --profile nextjs-storage.profile.yaml --env dev     # spec comes from the profile's `spec:`

# generate the app (Next.js + Postgres/Prisma by default)
spacekit spec generate --spec bookshop.app.yaml --out ./bookshop
spacekit spec generate --profile nextjs-storage.profile.yaml --env dev --out ./gallery

# check that two profiles realize the same behavior on different stacks
spacekit spec conform --spec bookshop.app.yaml postgres.profile.yaml storage-node.profile.yaml

# classify the changes between two spec versions (exit code 1 on an unbumped breaking change)
spacekit spec diff bookshop-1.2.yaml bookshop-1.3.yaml
```

Then, in the generated app:

```bash
cd bookshop/server && npm install && npx prisma generate --schema ../prisma/schema.prisma && npm run typecheck
cd ../web && npm install && npm run dev
```

Business logic goes in `server/handlers/*.ts` (generated once, then yours). Everything else is regenerated from the spec; hand edits to generated files are detected and kept unless you pass `--force`.

### What the generator does today

| Area | Generated |
| --- | --- |
| Data | Prisma schema (Postgres, MySQL, SQLite) or a spacekit-storage-node document backend; field `rules` enforced on every write. |
| Business | One typed contract per capability: input validation, the policy (compiled from the §9 grammar), the closed error set, declared events. Handler scaffolds for the logic. Workflows as in-process sagas with compensation. An OpenAPI document and a typed client SDK (TypeScript, Python or Rust). |
| View | Next.js app router (server components, server-action forms, `loading`/`empty`/`error` states, `@text` i18n, design tokens) or a Vite + React SPA. |
| Tooling | `check`, `generate` (incremental, with a refusal on unbumped breaking changes), `conform`, `diff`, and `--assist` (a certified brain suggests a design pattern per handler). |

Not yet: dApp generation, durable workflow engines (Temporal, Inngest, queues; the in-process runner is used and a note is printed), Vue and other frameworks, MongoDB, upload/presign adapters for `storage` buckets, view states other than loading/empty/error. The CLI prints a note whenever a spec or profile uses something it does not realize.

---



## Relationship to OpenAPI and the ABI

Spacekit capabilities are **transport-neutral**, they describe use cases, not endpoints. Off-chain, a profile with `business.emit_openapi: true` projects them into an OpenAPI document, so existing OpenAPI-based SDK generators plug straight in. On-chain, the same capabilities **bind to the contract ABI**. Spacekit sits *above* both: it can emit or consume an interface description, alongside the data layer and frontend that neither OpenAPI nor an ABI can describe.

---



## Status & roadmap

**Base spec & profile: v0.2 (draft), generator preview.** Workflows with saga compensation, field validation, view loading/empty/error states, a policy grammar, i18n, and compatibility/diffing rules, all generated by `spacekit spec` (see "What the generator does today").

**dApp spec: v0.2 (draft, not generated yet).** ABI import, indexed read model, transaction lifecycle, upgradeable contracts with version-aware indexing, finality/reorg handling, ERC-4337 account abstraction and sponsored transactions, cross-chain workflows, and on-chain access-control policies.

**Deferred to v0.3.** Base: workflow branching, policy functions, optimistic UI, constraint-based responsive layout, deployment targets. dApp: intent/solver execution, ZK/private-state flows, multi-sig/timelock signer modes, indexer backfill strategy, L2 finality nuances.

The format is a draft: expect breaking changes before v1.0, governed by the spec's compatibility rules.

---



## License

Released under the **Apache License 2.0**, the conventional choice for a specification intended for free adoption.

## Contributing

Issues and proposals welcome. Changes should state whether they are *compatible* or *breaking* under the spec's compatibility rules, and any spec addition that affects behavior should ship with the matching profile realization so the documents stay in lockstep. On-chain note: client-side policies are advisory (UX only), the contract remains the sole enforcer of on-chain authorization.