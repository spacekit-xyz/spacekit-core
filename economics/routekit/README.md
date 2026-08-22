# RouteKit

**Authenticated AI completion relay with bounded provider failover.**

RouteKit P0 accepts an explicit task hint, selects among configured OpenAI, Anthropic,
and Mistral models, and streams the completion. It uses SpaceKit Storage Node for
hashed client-key records and durable completion receipts.

- **swtch.ai** — RouteKit (this product)
- **spacekit-js** — SpaceKit JavaScript, Node.js, and Bun VM (blockchain execution)
- **spacekit-sdk** — SpaceKit SDK for React, TypeScript, and other frameworks

**Status:** P0 completion-only production candidate. Intent, vault, charge, activity,
compute-forwarding, and public metrics routes are not mounted.

---

## What RouteKit does

- **Single completion endpoint** — Proxies configured OpenAI, Anthropic, and Mistral providers.
- **Explicit task contract** — Clients provide one of the supported task labels; P0 does not run an automatic classifier.
- **Routing engine** — Orders provider/model candidates by task and available price data, then performs bounded sequential failover on transport and upstream server failures.
- **BYOK** — You bring your own provider API keys (YAML or env); RouteKit never stores them.
- **Authenticated tenants** — Opaque `sk-routekit-*` keys are stored only as hashes in SpaceKit Storage Node.
- **Cost tracking** — Completion receipts and best-effort token/cost records persist after each stream.
- **Production safety shell** — Restricted CORS, request limits, per-key rate limits, bounded concurrency, readiness checks, and internal-only metrics.

### MicroGPT ↔ RouteKit

Chat UIs (e.g. Kit page) use **spacekit-agent-microgpt** for efficient local LLM routing: a tiny on-device model predicts which tool/op to run (search, summarize, classify, code_review, analyze, chat) from the user message. RouteKit uses the **same task taxonomy** so you can send the result of microgpt-router as a hint and get the right model without a second classification step.

- **Client flow:** User sends message → (optional) run microgpt contract locally → get task token (e.g. `classify`) → `POST /v1/complete` with `task: "classify"` and `messages`. RouteKit selects a model for that task (e.g. cheaper for classify, higher quality for code_review) and streams the completion.
- **Task values:** `chat` | `search` | `summarize` | `classify` | `code_review` | `analyze` | `status`. Same labels as microgpt vocab and Kit agent operations.
- **Required in P0:** Requests without `task` or `task_hint` are rejected. Automatic routing requires the separately certified RouteKit brain planned for P1.

### P0 deployment

Build from the SpaceKit repository root:

```bash
docker build --file routekit/Dockerfile --tag routekit:p0 .
```

Start from `.env.production.example`, provision client keys with
`scripts/provision-api-key.sh`, and follow `PRODUCTION_RUNBOOK.md`. Production must
use Storage Node; bootstrap keys are isolated-development only.

---

## RouteKit in context

```
┌──────────────────────────────────────────────────────────────────────┐
│  CLIENT  (any app using RouteKit + SpaceKit)                         │
│  e.g. TradingKit terminal, headless agent, custom UI                 │
│                                                                      │
│  Composes draft → Simulates (WASM) → User/agent signs → SignedIntent │
└──────────────────────────────┬───────────────────────────────────────┘
                               │ SignedIntent (or encrypted envelope v1.1)
                               │ [ML-KEM-1024 encrypted in production]
┌──────────────────────────────▼───────────────────────────────────────┐
│  ROUTEKIT RELAY  (this service)                                      │
│                                                                      │
│  Classifier · Routing Engine · Provider Health · Model Prices (6h)   │
│  Provider Adapters (OpenAI, Anthropic, Mistral, …)                   │
│  Cost Tracker · Streaming Proxy · Intent validation & forward        │
└──────────────────────────────┬───────────────────────────────────────┘
                               │ completions / routed intent
                               │
┌──────────────────────────────▼───────────────────────────────────────┐
│  SPACEKIT NETWORK  (execution layer — see spacekit.xyz)              │
│  Compute Node · Storage Node · Messaging Node · Witnesses            │
│  Chains: Ethereum:1 · BSC:56 · Base:8453 · (Solana: roadmap)         │
└──────────────────────────────────────────────────────────────────────┘
```

---

## SpaceKit network components (relevant to RouteKit)

RouteKit integrates with the SpaceKit network. These components are core to the architecture and are specified here so the relay’s role is clear. Full specs live in the SpaceKit protocol and architecture docs.

### Messaging Node

Real-time pub/sub coordination between clients, the RouteKit relay, and the Compute Node.

- **Intent status streaming** — Push-based status updates for intents (replaces polling on `GET /v1/intent/:id`).
- **Agent notifications** — Alerts to the granting user when a scoped agent uses its delegation (e.g. intent executed within scope).
- **Market / event propagation** — Time-sensitive data to agents and UIs.
- **Relay decentralisation (v2)** — Foundation for gossip-based intent propagation; in v1 the relay is centralised.

RouteKit may consume Messaging Node streams for status and agent alerts; it does not replace the Messaging Node.

### Storage Node

p2p content-addressed CDN backbone run by operators (rewarded in ASTRA).

- **Content-addressed storage** — Data availability for contract state, manifests, and artifacts.
- **Model weight distribution** — Optional path for WebLLM and other model assets.
- **Sub-network state** — Sharding and availability for public/private sub-networks.

RouteKit does not serve storage; clients and Compute Node use Storage Nodes for data and state. RouteKit’s model price cache is independent (LiteLLM JSON).

### Witness protocol

Cross-chain proof anchoring and verification.

- **Proof generation** — SpaceKit state transitions produce proofs (e.g. quantum-verkle).
- **Anchors** — Proofs are stored on Bitcoin, Ethereum/EVM, Solana, and SpaceKit mainnet.
- **Witnesses** — Operators unroll and verify proofs on any chain so that state can be trusted without running a full SpaceKit node.
- **Stateless sync** — Quantum-verkle proofs enable lightweight sync of mainnet and sub-networks.

RouteKit does not run witnesses; it forwards signed intents to the Compute Node. Witness protocol is part of the execution/security layer, not the routing layer.

---

## Encrypted intent envelope (v1.1 — highest-priority security)

Before institutional onboarding, the relay must not see intent action contents in plaintext during the matching window (front-running risk). The **encrypted intent envelope** is the required security addition.

**Current (v1):** Relay receives full `SignedIntent`; it can read actions during the optional matching window.

**Target (v1.1):** Relay routes on **metadata only**. Action payload is encrypted for the executor (Compute Node); only the Compute Node decrypts and submits to chain.

```
SignedIntent {
  envelope: {
    recipient:   executor_pubkey,   // ML-KEM encapsulated for Compute Node
    ciphertext:  encrypt(canonical_intent_payload),
    intent_hash: SHA3-256(canonical_payload),   // for signature verification
    actor:       public,            // visible for nonce lookup & routing
    chain:       public,            // visible for routing
    expiry:      public,            // visible for relay rejection
  },
  signature: sign(intent_hash, actor_key | agent_key)
}
```

- **Relay:** Validates signature over `intent_hash`, checks `expiry`, routes by `chain` (and optionally `actor`). Does **not** decrypt `ciphertext`.
- **Compute Node:** Holds ML-KEM key; decrypts envelope, reconstructs intent, submits to on-chain contract. Already trusted in the current design.
- **Matching (optional):** A `matching_hint` field (e.g. asset pair + direction + size bucket) can support netting without exposing exact order details.

Document as a v1.1 security enhancement. v1 can ship with plaintext intent to the relay for private beta; encrypted envelope is the gate for institutional use.

---

## Agent delegation

The SpaceKit `AgentScope` contract lets an AI agent (e.g. Claude, a bot, a strategy) act on behalf of a user within on-chain limits. RouteKit does not enforce scope; it validates signature and expiry and forwards the signed intent. Scope is enforced by the WASM contract.

- **Persistent agents** — User grants scope once; agent signs intents with its key; RouteKit treats agent-signed intents like user-signed (same validation). Status and alerts can be pushed via the Messaging Node.

---

## Quantum-safe transport

All channels carrying intent payloads or key material use NIST PQC 2024 standards (ML-KEM-1024, ML-DSA-87). Classical TLS alone is not acceptable for these channels.

| Channel | Protection |
|---------|------------|
| Client ↔ RouteKit relay | TLS 1.3 + ML-KEM-1024/X25519 hybrid KEM |
| Client ↔ Compute Node | TLS 1.3 + ML-KEM-1024/X25519 hybrid KEM |
| Relay ↔ Compute Node | mTLS with ML-DSA-87 certificates |
| Relay ↔ Provider APIs | Standard TLS 1.3 (provider-controlled) |

---

## Pricing and model data

- **Provider pricing:** Use LiteLLM’s community-maintained `model_prices_and_context_window.json` (300+ models). RouteKit syncs on startup and every 6 hours. Configurable `COST_MAP_URL`; default:  
  `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json`
- **Pricing (Feb 2026, indicative):** Anthropic Opus 4.6 ($5/$25 per 1M in/out), Sonnet 4.6 ($3/$15), Haiku 4.5 ($1/$5); OpenAI GPT-5.x range ($0.25–$21 input); Mistral Large 3 ($2/$6), Medium 3 ($0.40/$2). BYOK config should reference current prices or rely on LiteLLM sync.
- **RouteKit product pricing:** Free tier (e.g. 1k requests/day, 2 providers); Pro usage-based, unlimited providers, SLA on routing latency. Focus on developer count for beta.

---

## Naming and spec alignment

- **RouteKit** everywhere: relay, API, keys (`sk-routekit-*`), package `@swtch/routekit`. No “Route AI” in public docs.
- **Protocol spec (SPACEKIT-INTENT-PROTOCOL-SPEC.md)** and **architecture (ARCHITECTURE.md)** should be updated to include:
  - Messaging Node (responsibilities, intent status, agent notifications, v2 gossip).
  - Storage Node (content-addressed CDN, data availability, model weights, sub-networks).
  - Witness protocol (proof generation, anchors per chain, witness selection and incentives, unrolling/verification, quantum-verkle and stateless sync).
  - Encrypted intent envelope (v1.1) as above.

**How to fill these spec gaps and in what order to build components** (RouteKit node → Messaging integration → encrypted envelope → Witness) is set out in [docs/SPEC-GAPS-AND-BUILD-PLAN.md](docs/SPEC-GAPS-AND-BUILD-PLAN.md).

---

## Repository structure (this repo)

```
routekit/
├── src/
│   ├── main.rs             # Entry: config, price fetch, server
│   ├── config.rs           # BYOK provider config (YAML + env)
│   ├── prices.rs           # LiteLLM model prices (startup + 6h refresh)
│   ├── router.rs           # Task types (microgpt-aligned), model selection
│   ├── providers/          # OpenAI, Anthropic, and Mistral streaming
│   ├── auth.rs             # Hashed API-key verification and per-key limits
│   ├── storage_client.rs   # Durable Storage Node key/receipt records
│   ├── intent.rs           # Feature-gated future signed-intent implementation
│   ├── vault_relay.rs      # Feature-gated future financial implementation
│   └── api.rs              # P0 HTTP: health, readiness, authenticated completion
├── config/
│   ├── providers.example.yaml
│   └── providers.yaml      # BYOK (gitignored)
├── docs/
│   ├── ROUTEKIT-REVIEW-INTENT-AND-PRICES.md
│   ├── SPEC-GAPS-AND-BUILD-PLAN.md
│   └── ARCHITECTURE.md
├── Cargo.toml
└── README.md
```

The relay implementation lives in this crate. TradingKit terminal, SpaceKit contracts (intent-executor, agent-scope, adapters), and spacekit-js are built in their respective repositories.

---

## Quick start

```bash
# Isolated development only. Production requires Storage Node.
export ROUTEKIT_STORAGE_REQUIRED=false
export ROUTEKIT_BOOTSTRAP_KEYS=sk-routekit-local-development-key
export OPENAI_API_KEY=...
cargo run -p routekit
# RouteKit relay listening on http://0.0.0.0:3001

# Optional: BYOK provider keys (env or YAML)
cp config/providers.example.yaml config/providers.yaml   # Edit with your keys or use ${VAR}
ROUTEKIT_CONFIG=routekit/config/providers.yaml cargo run -p routekit

# Env overrides
ROUTEKIT_PORT=3002 cargo run -p routekit
```

- **GET /health** — Process liveness.
- **GET /ready** — Provider, authentication, and Storage Node readiness.
- **POST /v1/complete** — Authenticated streaming completion. A supported `task` or `task_hint` is required.
- **GET /internal/metrics** — Prometheus text on the private metrics listener only.

### SpaceKit vault relay (future; not mounted in P0)

When SpaceKit.xyz Agent Hub bills against an on-chain vault, **RouteKit** is the HTTP relay that verifies the user’s EIP-191 signature and submits `vault.charge`. This replaces any standalone Node “deposit relayer” service.

The historical implementation is retained behind non-default Cargo features for
future hardening. Environment variables do not expose these routes in the P0 binary.
Canonical intent hashing, durable replay protection, relayer key custody, and a
separate testnet gate must land before this surface can return.

The future configuration is expected to include:

| Env | Purpose |
|-----|---------|
| `ROUTEKIT_RPC_URL` | JSON-RPC for the vault’s chain |
| `ROUTEKIT_CHAIN_ID` | Chain id (e.g. `1`) |
| `ROUTEKIT_VAULT_ADDRESS` | Deployed vault contract |
| `ROUTEKIT_RELAYER_PRIVATE_KEY` | Hot wallet allowed as `relayer` on the vault |
| `ROUTEKIT_VAULT_KIND` | Omit or `legacy` = `SpaceKitDepositVault` (USDC units). Set **`multi`** for `SpaceKitMultiAssetVault` (18-decimal **aUSD** `charge` + multi event layout for activity). |

**`POST /v1/charge`** — JSON body (camelCase): `user`, `amountAUsd`, `agentId`, `nonce`, `signature`. The signed UTF-8 message lines must match the website’s `buildAgentHubChargeMessage` (including `amountAUsd:` as a **decimal integer string** in 18-decimal aUSD wei). Returns `{ ok, transactionHash }` on success.

**`GET /v1/activity/:user`** — Optional `?fromBlock=<u64>`. Returns recent vault logs for that user (`deposited` / `withdrawn` / `charged`); shape differs for legacy vs `multi` vault (multi includes ERC20/ETH variants and `aUsdAmount` / `payoutAmount` on charges).

All intent, charge, and activity paths return **404** in P0.

### SDK usage (when using RouteKit from an app)

```typescript
import { RouteKit } from "@swtch/routekit";

const rk = new RouteKit({
  apiKey:  "sk-routekit-xxxxxxxx",
  relay:   "https://routekit.swtch.ai",
  profile: "default",
});

const stream = await rk.complete({
  messages: [{ role: "user", content: "Analyze ETH/BTC 4H chart" }],
  context:  myContext,
});
```

---

## What’s next

- **P1:** Independently certify and shadow a dedicated RouteKit task-classification brain.
- **P2:** Canonical signed intents with durable replay protection, testnet only.
- **P3:** Audited vault execution with hardware-backed relayer custody.

See [docs/NEXT.md](docs/NEXT.md) for the full list and priorities.

---

## Links

| | |
|---|---|
| **swtch.ai** | RouteKit — AI model routing |
| **spacekit.xyz** | SpaceKit — blockchain execution |
| **kit.space** | TradingKit — reference terminal |

*swtch labs llc*
