# RouteKit Review: Intent Signing/Verification & Model Prices

**Goal:** Build the RouteKit relay as a **Rust service** that integrates **smart-contract-compatible intent signing/verification** and **LiteLLM model prices** (startup + 6h refresh). The WASM contracts are consumed by **spacekit-js** in the browser/VM for ASTRA; RouteKit is the routing/orchestration layer that validates and forwards signed intents.

**Doc focus:** RouteKit only. TradingKit, SpaceKit execution, and other surfaces are built and specified elsewhere. The biggest spec gaps for the overall network — **Messaging Node**, **Storage Node**, and **Witness protocol** — are summarised in the RouteKit README so the relay’s role is clear; full specs belong in the SpaceKit protocol and architecture docs. The **encrypted intent envelope** is the highest-priority security addition before institutional onboarding.

---

## 1. Current State

| Item | Status |
|------|--------|
| **routekit crate** | Stub: `main.rs` is `println!("Hello, world!")`, no deps. Cargo.toml names package `tradingkit`, edition `2024`. |
| **Intent executor / agent-scope** | Specified in ARCHITECTURE.md and SPACEKIT-INTENT-PROTOCOL-SPEC.md; **no Rust impl found** in repo (on-chain contracts to be implemented). |
| **spacekit-js** | Has VM, `signatures.ts` (ed25519/dilithium), `SpacekitIntentClassifierClient`; intent lifecycle (simulate → sign → submit) and Zod validation are in the spec; relay fallback (direct-to-chain) specified. |
| **Relay (Bun/TS)** | Described in ARCHITECTURE (apps/relay) but **no `apps/relay` in repo** — relay is the thing to build in Rust under `routekit/`. |

So: the **Rust service** to build is the **RouteKit relay** (classification, routing, provider adapters, cost tracking, intent validation). It should remain the single endpoint that replaces provider-specific API calls and that forwards **SignedIntent** to SpaceKit after validation.

---

## 2. Model Prices (LiteLLM)

**Requirement:** On relay startup and every 6 hours (cron), fetch and cache model pricing/capabilities from LiteLLM.

**URL:**  
`https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json`

**Relevant shape per model** (chat/completion; you can ignore image/embedding-only entries):

```json
{
  "claude-sonnet-4-5-20250929": {
    "input_cost_per_token": 0.000003,
    "output_cost_per_token": 0.000015,
    "max_tokens": 200000,
    "max_input_tokens": 200000,
    "max_output_tokens": 8192,
    "litellm_provider": "anthropic",
    "supports_function_calling": true,
    "supports_vision": true
  }
}
```

**Design for Rust relay:**

- **Module:** e.g. `routekit/src/prices.rs` (or `model_prices/`).
- **Fetch:** HTTP GET on startup; store in a shared struct (e.g. `ModelPrices`) keyed by model id string. Filter to `mode: "chat"` (or presence of `input_cost_per_token` / `output_cost_per_token`) so you only cache chat/completion models.
- **Refresh:** Every 6 hours via:
  - A background task (e.g. `tokio::spawn` loop with `tokio::time::interval`), or
  - External cron hitting an internal admin endpoint that triggers refresh (e.g. `POST /internal/refresh-prices`), or
  - Both: internal interval + optional cron for predictability.
- **Use:** Routing and cost tracking use this cache only (no live calls to LiteLLM per request). Map provider model ids (e.g. from your YAML) to LiteLLM keys; if a model isn’t in the map, fall back to your existing static config or a default cost.
- **Types:** Mirror the fields you need, e.g.:

```rust
pub struct ModelPriceEntry {
    pub input_cost_per_token: f64,
    pub output_cost_per_token: f64,
    pub max_tokens: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub litellm_provider: String,
    pub supports_function_calling: Option<bool>,
    pub supports_vision: Option<bool>,
}
```

- **Config:** Make `COST_MAP_URL` configurable (env or config file), defaulting to the LiteLLM URL above.

**LiteLLM JSON note:** The file contains a `sample_spec` key and many entries (image generation, embedding, chat). Filter by `mode == "chat"` (or by presence of `input_cost_per_token` and `output_cost_per_token`) so the in-memory map only holds completion/chat models. Model ids in the JSON are the keys (e.g. `claude-sonnet-4-5-20250929`); map your provider config model ids to these (exact or prefix) for cost lookup.

---

## 3. Smart Contracts: Signing & Verifying Intent

Intent trust model is already defined; RouteKit’s job is to **validate and forward**, not to replace on-chain verification.

### 3.1 Flow (recap)

1. **Composition:** WebLLM (or Claude via RouteKit) produces **LLMIntentDraft** (label, actions, constraints, rationale). No `actor`, `nonce`, `expiry`, `intent_id`.
2. **Client (spacekit-js):** Validates draft (Zod), simulates in WASM VM, shows user; user approves; fills `actor`, `nonce`, `expiry`, builds canonical payload, `intent_id = SHA-256(canonicalPayload)`; user or agent signs `intent_id` → **SignedIntent** `{ intent, signature, sig_type }`.
3. **RouteKit relay:** Receives SignedIntent (over ML-KEM channel in v2; for private beta, TLS is acceptable). Validates:
   - Schema (version, required fields, action types).
   - Expiry: `intent.expiry > now + 30s`.
   - Signature: verify `signature` over `intent.intent_id` with `intent.actor` (or `intent.agent` if agent-signed) using `sig_type` (ed25519 / secp256k1).
   - If `intent.agent` is set: **do not** enforce scope here — scope is enforced on-chain only. Optionally you can do a **best-effort** scope check (e.g. call Compute Node or a read-only view) to fail fast; the contract remains authoritative.
4. **Relay** then routes to the correct chain and forwards the signed intent to the SpaceKit network (Compute Node or direct to chain, per your deployment).
5. **On-chain WASM contract** (intent-executor): `verify_signature` → `verify_agent_scope` (if agent) → replay (nonce + expiry) → execute actions → emit `IntentExecuted`.

So: **signing** happens in the client (spacekit-js + wallet or agent key). **Verification** happens in the relay (signature + schema + expiry) and again on-chain (signature + agent scope + nonce + expiry). Contracts are WASM libs run by the SpaceKit VM (ASTRA); the relay never runs the contract, it only checks the signature and passes the payload through.

### 3.2 What to implement in RouteKit (Rust)

- **Intent schema in Rust:** Mirror the Intent and SignedIntent types used by spacekit-js and the on-chain contract (canonical serialization order, field set). Share a single source of truth for “canonical payload” (e.g. serde with sorted keys or a small canonical-json crate) so that `intent_id` computed client-side matches what the contract expects.
- **Signature verification:** Use `ed25519-dalek` and `k256` (or equivalent) for ed25519 and secp256k1. Input: `intent_id` (bytes), `signature` (bytes), `actor` (public key or address). Return success/failure; do not execute any action.
- **No private keys in the relay:** Relay never signs intents; it only verifies. Agent and user keys stay in the client or in a separate agent runtime.

### 3.3 Encrypted intent envelope (v1.1 — highest-priority security)

Before institutional onboarding, the relay must **not** see intent action contents in plaintext during the matching window (front-running risk). The encrypted envelope is the required addition.

- **Envelope shape:** `SignedIntent { envelope: { recipient, ciphertext, intent_hash, actor, chain, expiry }, signature }`. Only `actor`, `chain`, `expiry` (and optional `matching_hint`) are visible to the relay; `ciphertext` is ML-KEM encapsulated for the Compute Node.
- **Relay:** Verifies signature over `intent_hash`; checks expiry; routes by chain (and optionally actor). Does **not** decrypt `ciphertext`.
- **Compute Node:** Decrypts envelope, reconstructs intent, submits to chain. Already trusted; holds ML-KEM key.
- **Matching:** Optional `matching_hint` (e.g. asset pair + direction + size bucket) allows netting without exposing exact order details.

v1 can ship with plaintext intent to the relay for private beta; encrypted envelope is the gate for institutional use. See RouteKit README for the full envelope structure.

### 3.4 Agent scope (Claude / “Clawdbot”)

- Scope is **granted on-chain** (agent-scope registry contract); the contract’s `verify_agent_scope` is the authority.
- RouteKit can optionally call a read-only “get scope” endpoint (Compute Node or chain view) to reject obviously out-of-scope intents early; if you don’t have that yet, relay only does signature + schema + expiry and lets the chain reject scope violations.
- For **persistent agents** (e.g. Claude with standing authority): the agent runs elsewhere (e.g. your backend or a dedicated service), composes intents, signs with the **agent key**, and submits **SignedIntent** to RouteKit like any other client. RouteKit doesn’t treat “Claude” specially — same validation, same routing. The leash is entirely on-chain (scope contract).

---

## 4. Private Beta (Weeks 1–8)

- **Onboarding:** `npm install @swtch/routekit` → set provider keys in YAML → one endpoint replaces all provider-specific calls.
- **Retention:** Health dashboard + cost tracker (using the model prices above).
- **Pricing:** Free tier (e.g. 1k requests/day, 2 providers); paid usage-based, unlimited providers, SLA on routing latency. Focus on **developer count**, not revenue.
- **Metric:** Weekly active developers making **>100 requests** (real traffic through RouteKit).

The Rust relay should expose:

- **Complete/chat endpoint:** Classify → select model (using health + cost from model prices) → proxy to provider → stream back. Optional: return which model was used (e.g. `X-RouteKit-Provider`, `X-RouteKit-Model`).
- **Intent endpoint:** `POST /v1/intent` with SignedIntent; validate (schema + signature + expiry); forward to SpaceKit; return relay_id and status.
- **Health:** `GET /health` with provider status and, if you want, a hint that model prices are loaded (e.g. `prices_loaded: true`, `prices_updated_at: unix_ts`).
- **Cost tracking:** Use the cached model prices + token counts (from provider responses or streaming) to attribute cost per request and expose in dashboard/API.

---

## 5. Claude as an Agent in Your System

- **Remote composer:** Claude (or any frontier model) slots in as a **scoped agent** from the protocol’s point of view: it composes intents (LLMIntentDraft), and either the user signs or an agent key signs within a pre-granted scope.
- **RouteKit’s role:** RouteKit classifies the task (e.g. strategy_analysis vs price_lookup); routes to Claude for complex tasks and to local WebLLM for simple/fast ones; returns the same **LLMIntentDraft** shape. spacekit-js then validates (Zod), simulates, and presents for signing — **same trust boundary** as WebLLM.
- **Blending local + remote:** Privacy-sensitive context stays local (WebLLM); heavy reasoning goes to Claude via RouteKit. User doesn’t manage which model; task taxonomy and routing do.
- **Persistent agent (“Clawdbot”):** User grants scope on-chain (e.g. “rebalance, swaps only, ETH/BTC/USDC, max $500/intent, 10 intents/hour, 7 days”). A Claude-powered service monitors (e.g. via Messaging Node), composes intents, signs with the agent key, submits through RouteKit → SpaceKit. The on-chain scope contract is the leash; RouteKit is the nervous system.

So: build the Rust relay so that **any** client (terminal with WebLLM, terminal with Claude-over-RouteKit, or a headless Clawdbot) sends the same SignedIntent and gets the same validation and routing. No special path for “Claude”; the only difference is who composes the draft and who holds the key that signs.

---

## 6. Concrete Checklist for the Rust Service

- [ ] **Model prices:** Fetch LiteLLM JSON on startup; refresh every 6h (or via cron); parse into `ModelPrices`; use for routing and cost tracking; configurable `COST_MAP_URL`.
- [ ] **Intent types:** Define Intent / SignedIntent (and canonical serialization) in Rust to match spacekit-js and on-chain spec; use for validation only.
- [ ] **Signature verification:** Implement ed25519 and secp256k1 verify for `intent_id`; no signing in relay.
- [ ] **Encrypted intent envelope (v1.1):** Envelope format so relay routes on metadata only (actor, chain, expiry); Compute Node decrypts; optional `matching_hint`. Gate for institutional onboarding.
- [ ] **Relay API:** Complete endpoint (streaming), intent endpoint (POST /v1/intent), health endpoint; optional internal refresh for prices.
- [ ] **Provider adapters:** OpenAI, Anthropic, Mistral (and others as needed); BYOK from YAML/env.
- [ ] **Routing:** Use profile/task + health + model prices to select model; &lt; 2ms routing decision.
- [ ] **Cost tracker:** Per-request cost from cached prices + token usage; expose for dashboard.
- [ ] **Agent scope:** Leave enforcement on-chain; optional early rejection via scope lookup if you have a read-only API.

Once this is in place, the smart contracts (intent-executor, agent-scope) can be implemented as WASM libs for the SpaceKit VM (ASTRA) and consumed by spacekit-js for simulation and for direct-to-chain submission when the relay is unavailable. RouteKit stays the central routing and validation layer that respects the same intent and signature format the contracts expect.

---

## 7. Network context & spec gaps

These are core to the SpaceKit network but were absent from earlier RouteKit/Intent docs. They are documented from RouteKit’s perspective in the **RouteKit README**; full protocol and architecture specs should add dedicated sections.

- **Messaging Node:** Real-time pub/sub; intent status streaming (replace polling); agent activity notifications; foundation for relay decentralisation (v2 gossip). RouteKit consumes status/events; it does not implement the Messaging Node.
- **Storage Node:** p2p content-addressed CDN; data availability for contract state and model weights; sub-network state. RouteKit does not serve storage; model price cache is separate (LiteLLM).
- **Witness protocol:** Cross-chain proof anchoring (BTC, ETH, Solana, SpaceKit); proof generation; witness unrolling/verification; quantum-verkle and stateless sync. RouteKit forwards intents to the Compute Node; witnesses are part of the execution/security layer.

**Naming:** Use RouteKit everywhere (relay, API, keys `sk-routekit-*`, package `@swtch/routekit`). No “Route AI” in public docs.

**Pricing:** Provider pricing via LiteLLM sync (startup + 6h). Update BYOK examples to current (Feb 2026) indicative prices or rely on LiteLLM; see README for examples.

**Filling the spec gaps and build order:** See [SPEC-GAPS-AND-BUILD-PLAN.md](SPEC-GAPS-AND-BUILD-PLAN.md) for how to add Messaging Node, Storage Node, and Witness protocol to the protocol/architecture docs, and the sequence: RouteKit node → Messaging integration → encrypted envelope → Witness (spec then cross-chain witness service) → Storage spec.
