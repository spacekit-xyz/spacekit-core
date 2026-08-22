# The SpaceKit App Specification

**Version 0.2 — Draft**

A single, versioned, declarative description of a web application across all three layers of the stack; **data**, **business**, and **view**, so one source of truth can drive code generation (via `spacekit-cli`) for the database, the API/SDK, and the frontend simultaneously.

---

## What's new in v0.2

- `workflows`: event-triggered, multi-step orchestration with saga-style compensation. *New top-level construct.* (§7)
- **Field validation rules** on entities. (§5)
- **View data states**: loading / empty / error are now first-class. (§10)
- **A real policy expression grammar**, replacing v0.1's opaque strings. (§9)
- `text`: an i18n string registry. (§11)
- **Compatibility rules** defining breaking vs compatible changes across versions: the basis for spec diffing. (§14)
- **Capability contract floor** clarified so a single spec supports both compile and runtime bindings. (§6)

Full change list in §15.

---



## 1. Philosophy

Unchanged from v0.1. Three commitments: one document spanning three layers with explicit cross-references; a transport-agnostic business layer; and "the document is the registry", every top-level definition is referenceable by name via the `@` sigil, with no separate reuse bucket.

The v0.2 additions hold the line: `workflows` describe *behavior and failure semantics* (spec concern), never *which orchestration engine runs them* (profile concern).

---



## 2. Document structure

```yaml
spacekit: "0.2"
app:          { ... }   # metadata
entities:     { ... }   # data layer
capabilities: { ... }   # business layer — use cases
events:       { ... }   # business layer — domain events
workflows:    { ... }   # business layer — orchestration v0.2
policies:     { ... }   # business layer — access rules
widgets:      { ... }   # view layer — leaf building blocks
views:        { ... }   # view layer — screens
flows:        { ... }   # view layer — journeys
text:         { ... }   # view layer — i18n strings v0.2
tokens:       { ... }   # view layer — design tokens
```

`app`, `entities`, `capabilities`, and `views` are required; the rest are optional. Names are unique across all registries.

---



## 3. References and scope

`@Name` resolves to any top-level definition. Bare identifiers inside a view's `data`, `params`, an action's `with`, or a workflow step refer to local scope. Dotted access (`cart.owner.email`) walks declared relations. New in v0.2: `@text.<key>` resolves into the `text` registry, and inside workflows `event.<field>` refers to the triggering event's payload.

---



## 4. The `app` object

```yaml
app:
  name: Bookshop
  version: 1.2.0
  description: A small online bookstore.
```

---



## 5. Data layer: `entities`

Entities gain a `rules` modifier for field-level validation.

```yaml
entities:
  User:
    identity: id
    fields:
      id:          { type: id, generated: true }
      email:       { type: email, required: true, unique: true }
      displayName: { type: text, rules: { minLength: 1, maxLength: 80 } }
      createdAt:   { type: timestamp, generated: true }
    relations:
      orders: { hasMany: Order, inverse: buyer }

  Book:
    identity: id
    fields:
      id:     { type: id, generated: true }
      title:  { type: text, required: true, indexed: true, rules: { minLength: 1, maxLength: 200 } }
      price:  { type: money, required: true, rules: { min: 0 } }
      stock:  { type: integer, default: 0, rules: { min: 0 } }
      status: { type: enum, values: [draft, listed, retired], default: draft }

  Order:
    identity: id
    fields:
      id:       { type: id, generated: true }
      total:    { type: money, generated: true }
      placedAt: { type: timestamp }
      state:    { type: enum, values: [cart, placed, fulfilled, failed, cancelled], default: cart }
    relations:
      buyer: { belongsTo: User }
      lines: { hasMany: OrderLine }

  OrderLine:
    identity: id
    fields:
      id:       { type: id, generated: true }
      quantity: { type: integer, required: true, rules: { min: 1 } }
    relations:
      order: { belongsTo: Order }
      book:  { belongsTo: Book }
```

**Field types, modifiers, relations** are unchanged from v0.1.

`rules` v0.2 declares validation enforced at the data boundary and surfaced to generated forms: `min`, `max`, `minLength`, `maxLength`, `pattern` (regex), `oneOf` (literal set). Rules are *properties*, not patterns — they describe what's valid, not how validation is implemented.

---



## 6. Business layer: `capabilities`

Capabilities are unchanged in shape from v0.1 (`input`, `output`, `reads`, `writes`, `emits`, `policy`, `errors`, `idempotent`) but v0.2 settles their contract status.

```yaml
capabilities:
  ListBooks:
    summary: Return books available for purchase.
    input:  { query: { type: text } }
    output: { books: { list: @Book } }
    reads:  [Book]
    policy: public

  PlaceOrder:
    summary: Convert the buyer's cart into a placed order.
    input:
      orderId:       { type: id, required: true }
      paymentMethod: { type: enum, values: [card, paypal], required: true }
    output: { order: @Order }
    reads:  [Book, Order, OrderLine]
    writes: [{ updates: Order }]
    emits:  [OrderPlaced]
    policy: @OwnsOrder
    errors: [EmptyCart, OutOfStock]

  # Capabilities that the FulfillOrder workflow orchestrates:
  ReserveStock:   { input: { orderId: { type: id, required: true } }, writes: [{ updates: Book }],  errors: [OutOfStock] }
  ReleaseStock:   { input: { orderId: { type: id, required: true } }, writes: [{ updates: Book }] }
  ChargePayment:  { input: { orderId: { type: id, required: true } }, writes: [{ updates: Order }], errors: [PaymentDeclined] }
  RefundPayment:  { input: { orderId: { type: id, required: true } }, writes: [{ updates: Order }] }
  ScheduleShipment: { input: { orderId: { type: id, required: true } }, writes: [{ updates: Order }] }
```



### Capability contract floor (resolves the v0.1 open decision)

The compile-vs-runtime fork is settled at the spec level by *requiring the stricter contract always*: a capability's `input`, `output`, and `errors` are **exhaustive and versioned**, sufficient to serve as a wire contract. This means one spec can be realized by a `compile` profile (typed source) or a `runtime` profile (message contract) without re-authoring. The *choice* of binding lives in the profile (§4 of the Profile doc); the *floor* lives here, permanently.

Practical consequence: an `errors` list is now closed — a capability may only fail in named ways. Generators may rely on that exhaustiveness.

---



## 7. Business layer: `workflows` v0.2

A workflow is event-triggered, multi-step business behavior with explicit failure semantics. It is *not* code organization, which is why it belongs in the spec rather than a profile: "charge after reserving, and refund if charging fails" is an observable guarantee.

```yaml
workflows:
  FulfillOrder:
    summary: Reserve stock, charge, and ship a placed order; compensate on failure.
    on: OrderPlaced                     # triggering event
    steps:
      - do: ReserveStock
        with: { orderId: event.order.id }
        compensate: ReleaseStock        # inverse run if a later step fails
      - do: ChargePayment
        with: { orderId: event.order.id }
        compensate: RefundPayment
      - do: ScheduleShipment
        with: { orderId: event.order.id }
    onComplete:
      emit: OrderFulfilled              # all steps succeeded
    onFailure:
      emit: OrderFailed                 # emitted after compensations run
```



### Semantics

- **Trigger.** `on` names an event from the `events` registry. The event payload is in scope as `event`.
- **Steps.** Each step `do`s a capability with mapped `with` inputs. Steps run in order.
- **Compensation (saga).** If step *N* fails (the capability returns one of its declared `errors`), the `compensate` capabilities of steps *N-1 … 1* run in reverse order. A step without `compensate` is treated as having no side effect to undo.
- **Outcomes.** `onComplete` and `onFailure` may `emit` an event, enabling chaining. `onFailure` fires only after compensation completes.
- **Determinism.** A workflow may only call capabilities; it has no inline logic. Branching beyond linear-with-compensation is **deferred to v0.3** (a `when` guard on steps is the planned extension).

A generator must realize these guarantees but chooses the orchestration mechanism (in-process, durable queue, Temporal, Inngest) via the profile.

---



## 8. `events`

```yaml
events:
  OrderPlaced:    { payload: { order: @Order, at: { type: timestamp } } }
  OrderFulfilled: { payload: { order: @Order } }
  OrderFailed:    { payload: { order: @Order, reason: { type: text } } }
```

---



## 9. `policies` — now with a grammar (resolves v0.1 deferral)

A policy is a named boolean expression over an implicit `actor` (the caller) and `resource` (the entity acted on).

```yaml
policies:
  OwnsOrder: "actor.id == resource.buyer.id"
  AdminOnly: "actor.role == 'admin'"
  StaffOrOwner: "actor.role == 'staff' || actor.id == resource.buyer.id"
```



### Grammar (v0.2)

- **Operands:** `actor` and `resource` with dotted field access; string (`'...'`), number, and boolean literals.
- **Comparison:** `==` `!=` `<` `<=` `>` `>=`.
- **Membership:** `x in [a, b, c]`.
- **Boolean:** `&&` `||` `!`, with parentheses.
- **Built-in policy keywords:** `public` (anyone) and `authenticated` (any signed-in actor) may be used in place of an expression.

No function calls in v0.2; a function library is deferred to v0.3. Policies may be referenced by capabilities (`policy:`) and views (`policy:`).

---



## 10. View layer — `widgets` and `views`

`widgets` are unchanged: typed `props`, named `slots`, optional token styling; no direct data or capability access.

`views` gain a `states` block so loading / empty / error are declared, not improvised.

```yaml
widgets:
  Button:      { props: { label: { type: text, required: true }, variant: { type: enum, values: [primary, ghost], default: primary } } }
  BookCard:    { props: { book: @Book }, slots: [actions] }
  Spinner:     { props: {} }
  EmptyState:  { props: { message: { type: text } } }
  ErrorBanner: { props: { message: { type: text } } }

views:
  Catalog:
    route: /
    summary: Browse and search books.
    params: { q: { type: text } }
    data:
      books: { from: @ListBooks, with: { query: params.q } }
    states:                                   # NEW
      loading: @Spinner
      empty:   { when: "books.length == 0", show: @EmptyState, props: { message: @text.noBooks } }
      error:   @ErrorBanner
    layout:
      - repeat: book in books
        widget: @BookCard
        props: { book: book }
        slots:
          actions:
            - widget: @Button
              props: { label: @text.view }
              on: { tap: -> @BookDetail(id: book.id) }
    transitions:
      - to: @BookDetail
        label: "Open a book"

  BookDetail:
    route: /books/:id
    params: { id: { type: id, required: true } }
    data:  { book: { from: @Book, where: "id == params.id" } }
    states:
      loading: @Spinner
      error:   @ErrorBanner
    layout:
      - widget: @BookCard
        props: { book: book }
    actions:
      addToCart:
        invokes: @PlaceOrder
        with: { orderId: book.id, paymentMethod: card }
        onSuccess: -> @OrderConfirmation(orderId: result.order.id)
        onError:   stay
    transitions:
      - to: @Catalog
        label: "Back to catalog"

  OrderConfirmation:
    route: /orders/:orderId
    params: { orderId: { type: id, required: true } }
    data:   { order: { from: @Order, where: "id == params.orderId" } }
    policy: @OwnsOrder
    layout:
      - widget: @Button
        props: { label: @text.continueShopping }
        on: { tap: -> @Catalog }
```



### `states` (new)

A view declares how it renders before data resolves and when it's empty or errored:

- `loading` — widget/layout shown while any `data` binding is pending.
- `empty` — `when` is a boolean expression; `show` is the widget/layout to render; `props` optional.
- `error` — widget/layout shown when a binding fails.

This also discharges part of the v0.1 "binding states" deferral. Optimistic updates remain **deferred to v0.3**.

---



## 11. `text` — i18n string registry v0.2

```yaml
text:
  view:             { en: "View",              es: "Ver" }
  noBooks:          { en: "No books found.",   es: "No se encontraron libros." }
  continueShopping: { en: "Continue shopping", es: "Seguir comprando" }
```

Referenced anywhere a literal string is expected via `@text.<key>`. The active locale is a runtime/profile concern, not a spec concern. Pluralization and interpolation are **deferred to v0.3**.

---



## 12. `flows`

Unchanged from v0.1: named, optionally-branching journeys over views.

```yaml
flows:
  Purchase:
    steps:
      - at: @Catalog
      - at: @BookDetail
      - at: @OrderConfirmation
        on: { OrderPlaced: complete }
```

---



## 13. `tokens`

Tokens gain `breakpoints` so the view layer can express responsive intent; full constraint-based responsive layout remains **deferred to v0.3**.

```yaml
tokens:
  color: { brand: "#1f6feb", surface: "#ffffff" }
  space: { sm: 8, md: 16, lg: 24 }
  type:  { body: { family: Inter, size: 16 } }
  breakpoints: { sm: 480, md: 768, lg: 1200 }   # NEW
```

---



## 14. Compatibility and diffing v0.2

Defines what changes between two spec versions are safe. This is the basis for the diff tooling (`spacekit-cli diff`) and for the contract floor in §6.

**Compatible (minor bump):** add an entity, capability, event, workflow, view, widget, or policy; add an *optional* field or input; widen an enum; add a `text` key; relax a validation rule.

**Breaking (major bump):** remove or rename any named definition or field; change a field/parameter type; narrow an enum; make an optional input required; change a capability's `writes` effects; add or change a member of a capability's `errors` set; tighten a policy; remove a workflow step's `compensate`.

`app.version` carries the app's own semantic version; a generator may refuse to regenerate across a breaking change without an explicit flag.

---



## 15. Changes from v0.1

- Added `workflows` (§7) — the saga construct discussed during design; the one architecture-category item that is spec-level.
- Added field `rules` validation (§5).
- Added view `states` for loading/empty/error (§10).
- Promoted `policies` from opaque strings to a defined grammar (§9).
- Added `text` i18n registry (§11).
- Added `tokens.breakpoints` (§13).
- Defined the capability contract floor, settling compile-vs-runtime at the spec level (§6).
- Added compatibility/diffing rules (§14).
- Aligned the root key to `spacekit:` and the format name to the tooling (see naming note).



## 16. Still deferred to v0.3+

Workflow branching beyond linear-with-compensation; policy functions; optimistic updates; constraint-based responsive layout; i18n pluralization/interpolation; field-level read policies.