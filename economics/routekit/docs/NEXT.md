# What’s next

**Current state:** RouteKit relay runs with config, model prices (LiteLLM 6h), **cost-aware routing** (nominal cost 100/500 tokens), **cost tracker** (usage from SSE → GET `/metrics`), microgpt-aligned task routing, **OpenAI + Anthropic + Mistral** streaming for `/v1/complete`, health, and full intent validation on `POST /v1/intent`. Signatures: ed25519, secp256k1 (EVM EIP-191), mldsa65, slh_dsa_sha2_128s, slh_dsa_sha2_192s.

---

## 1. RouteKit (this repo) — next

| Priority | What | Notes |
|----------|------|------|
| **1** | ~~Anthropic streaming~~ | Done. |
| **2** | ~~Mistral streaming~~ | Done. |
| **3** | ~~Intent validation~~ | Done. `SignedIntent` parsed; `validate_intent` + `verify_signature`; 400 with `error_code`/`error_message` on failure. |
| **4** | ~~Cost-aware routing~~ | Done. Multiple candidates sorted by nominal cost (100 in / 500 out); prefer cheap for fast tasks, expensive for quality. |
| **5** | ~~Cost tracker~~ | Done. Parse OpenAI-style `usage` from final SSE chunk; accumulate via `wrap_stream_usage`; GET `/metrics` returns `request_count`, `total_input_tokens`, `total_output_tokens`, `total_cost_usd`. |

---

## 2. RouteKit — v1.1 (institutional)

| Item | Notes |
|------|------|
| **Encrypted intent envelope** | Relay only sees metadata (actor, chain, expiry); payload encrypted for Compute Node. Requires envelope format in spec, then spacekit-js (build), relay (forward), Compute Node (decrypt). |
| **Provider health** | Track latency and errors per provider; z-score or threshold to shift traffic (e.g. &lt; 2σ). Optional health dashboard. |

---

## 3. Outside this repo (build plan)

| Phase | Where | What |
|-------|--------|------|
| **Messaging integration** | spacekit-compute-node / relay + spacekit-messaging-node | Intent status streaming (replace polling), agent notifications. |
| **Witness protocol** | Architecture doc + `spacekit-witness` (or compute-node) | Spec anchor format and witness behaviour; then cross-chain witness service. |
| **Storage Node spec** | Architecture doc | Document existing storage node; no new code in RouteKit. |

---

## Quick reference

- **Checklist (review doc):** [ROUTEKIT-REVIEW-INTENT-AND-PRICES.md §6](ROUTEKIT-REVIEW-INTENT-AND-PRICES.md).
- **Build order:** [SPEC-GAPS-AND-BUILD-PLAN.md](SPEC-GAPS-AND-BUILD-PLAN.md).
