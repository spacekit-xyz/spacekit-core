# Architecture & Technical Specification

## TradingKit trading terminal — v0.1

**Status:** Internal engineering reference  
**Authors:** swtch labs  
**Scope:** Full system architecture, component design, data flows, interfaces, and deployment topology for the TradingKit trading terminal — a vertically integrated AI-powered trading terminal built on RouteKit (swtch.ai) and the SpaceKit network (spacekit.xyz).

---

## Table of contents

1. [System overview](#1-system-overview)
2. [Layer architecture](#2-layer-architecture)
3. [Trading UI](#3-trading-ui)
4. [WebLLM — local AI composition](#4-webllm--local-ai-composition)
5. [RouteKit — intent routing](#5-routekit--intent-routing)
6. [SpaceKit — on-chain execution](#6-spacekit--on-chain-execution)
7. [Intent lifecycle — end to end](#7-intent-lifecycle--end-to-end)
8. [Market data layer](#8-market-data-layer)
9. [Provider configuration — BYOK](#9-provider-configuration--byok)
10. [Quantum-safe transport](#10-quantum-safe-transport)
11. [Agent mode](#11-agent-mode)
12. [Nonce management](#12-nonce-management)
13. [Error handling](#13-error-handling)
14. [Deployment topology](#14-deployment-topology)
15. [Performance targets](#15-performance-targets)
16. [Development roadmap](#16-development-roadmap)

---

## 1. System overview

The TradingKit trading terminal is a vertically integrated stack with three distinct layers, each owning a clearly bounded responsibility:

```
┌─────────────────────────────────────────────────────────────────────┐
│  SURFACE LAYER — Trading UI  (apps/terminal)                        │
│                                                                     │
│  Chart · Portfolio · AI Assistant · Intent Preview · Order History  │
│  spacekit-js SDK · WebLLM worker · Market data feeds                │
└─────────────────────────────┬───────────────────────────────────────┘
                              │  LLMIntentDraft / SimulationResult
                              │  SignedIntent / ExecutionReceipt
┌─────────────────────────────▼───────────────────────────────────────┐
│  INTELLIGENCE LAYER — RouteKit  (apps/relay)                        │
│                                                                     │
│  Intent classifier · Provider health graph · Routing engine         │
│  Profile manager · Streaming proxy · Cost tracker                   │
│  Providers: OpenAI · Anthropic · Mistral · Google · custom          │
└─────────────────────────────┬───────────────────────────────────────┘
                              │  SignedIntent
                              │  [ML-KEM-1024 encrypted]
┌─────────────────────────────▼───────────────────────────────────────┐
│  EXECUTION LAYER — SpaceKit network  (spacekit.xyz)                 │
│                                                                     │
│  SpaceKit Compute Node (Mainnet, nonce authority)                   │
│  WASM intent executor · Agent scope registry · Adapter registry     │
│  Chains: Ethereum:1 · BSC:56 · Base:8453 · (Solana: roadmap)        │
│  Adapters: Uniswap v3 · PancakeSwap · Stargate v2 (bridge)          │
└─────────────────────────────────────────────────────────────────────┘
```

**Separation of concerns:**

| Layer | Owns | Does not own |
|---|---|---|
| Trading UI | User experience, context collection, display | Routing decisions, model selection, chain execution |
| RouteKit | Model selection, provider health, cost optimisation | Business logic, user keys, chain state |
| SpaceKit | On-chain execution, signature verification, state | AI composition, routing, UI |

Each layer communicates with the next through a defined interface. No layer reaches across a non-adjacent layer.

---

## 2. Layer architecture

### 2.1 Dependency direction

```
Trading UI  (apps/terminal)
    │
    ▼ depends on
spacekit-js  (packages/spacekit-js)
    │
    ▼ depends on
intent-schema  (packages/intent-schema)  ◄── RouteKit relay also depends on this
    │
    ▼
SpaceKit Compute Node + WASM contracts  (contracts/)
```

The `intent-schema` package is the shared contract between all three layers. It defines the canonical `Intent`, `SignedIntent`, `LLMIntentDraft`, `SimulationResult`, and `ExecutionReceipt` types in TypeScript, mirrored in Rust for the on-chain contracts.

### 2.2 Runtime environments

| Component | Runtime | Language | Deploy target |
|---|---|---|---|
| Trading UI | Browser (Chrome 118+, WebGPU) | TypeScript / React | CDN static |
| WebLLM worker | Browser Web Worker | TypeScript | Bundled with UI |
| WASM VM (simulation) | Browser WASM | Rust → WASM | Bundled with SDK |
| RouteKit relay | Server | TypeScript / Bun | Docker / cloud |
| SpaceKit Compute Node | Server | Rust | Bare metal / cloud |
| On-chain contracts | EVM bytecode / WASM | Rust | On-chain |

---

## 3. Trading UI

### 3.1 Layout

```
┌──────────────────────────────────────────────────────────────────────┐
│  nav:  symbol search · wallet · network selector · settings          │
├─────────────────────────────┬────────────────────────────────────────┤
│                             │                                        │
│  Chart panel                │  Assistant panel                       │
│  · Candlestick chart        │  · Conversation messages               │
│  · Volume bars              │  · Intent previews (inline)            │
│  · Indicator overlays       │  · Simulation results                  │
│  · Drawing tools            │  · Sign / Reject controls              │
│                             │                                        │
├─────────────────────────────┼────────────────────────────────────────┤
│  Portfolio panel            │  Input bar                             │
│  · Open positions           │  [ type a question or command... ] [▶] │
│  · Balances                 │                                        │
│  · Transaction history      │  model: mistral-nemo · status: ready   │
└─────────────────────────────┴────────────────────────────────────────┘
```

### 3.2 Component tree

```tsx
<Terminal>
  <Nav>
    <SymbolSearch />
    <WalletConnector />
    <NetworkSelector />
  </Nav>

  <MainLayout>
    <ChartPanel>
      <CandlestickChart />         // lightweight-charts or custom canvas
      <IndicatorOverlays />        // RSI, MACD, Bollinger, volume
      <DrawingTools />
    </ChartPanel>

    <AssistantPanel>
      <MessageList>
        <UserMessage />
        <AssistantMessage>
          <Rationale />
          <IntentPreview>          // appears inline when intent is proposed
            <ActionSummary />
            <SimulationResult />
            <SignButton />
            <RejectButton />
          </IntentPreview>
        </AssistantMessage>
      </MessageList>
      <InputBar>
        <ModelStatusIndicator />   // "AI ready · mistral-nemo" | "loading 40%"
        <MessageInput />
        <SubmitButton />
      </InputBar>
    </AssistantPanel>

    <PortfolioPanel>
      <PositionList />
      <BalanceList />
      <TransactionHistory />
    </PortfolioPanel>
  </MainLayout>
</Terminal>
```

### 3.3 Key hooks

```typescript
// Live price, OHLCV, indicators for current symbol
const { snapshot, subscribe } = useMarketData(symbol);

// WebLLM — model loading state and composition
const { status, compose, isReady } = useWebLLM();
// status: "downloading" | "loading" | "ready" | "unavailable"

// Full intent lifecycle
const { draft, simulation, sign, submit, reset } = useIntent();
// draft:      LLMIntentDraft | null
// simulation: SimulationResult | null
// sign:       (intent: Intent) => Promise<SignedIntent>
// submit:     (signed: SignedIntent) => Promise<ExecutionReceipt>

// Wallet
const { address, signer, connect, disconnect } = useWallet();
```

### 3.4 WebLLM loading strategy

The model is loaded in a dedicated Web Worker during app boot — not on first user interaction. By the time the user opens the chat panel, the model is warm.

```
App boot sequence:

  t=0ms     React app hydrates. Chart loads. Portfolio fetches.
  t=0ms     llm.worker.ts initialises in background (invisible to user).
            Worker: detect VRAM → select model tier → begin weight download.
  t=?       Weights download from CDN (~2–8 GB depending on tier).
  t=?       WASM compilation and model load into WebGPU context.
  t=ready   Worker posts { type: "ready", tier: "standard" }
            InputBar shows: "AI ready · mistral-small"
            (trader has been using the chart this whole time)
```

If the user submits a message before the model is ready, the message is queued and processed the moment the model becomes available, with a loading indicator.

```typescript
// workers/llm.worker.ts
import { CreateMLCEngine } from "@mlc-ai/web-llm";

const TIER_MODELS = {
  full:     "Mistral-Large-Instruct-2407-q4f16_1-MLC",
  standard: "Mistral-Small-Instruct-2409-q4f16_1-MLC",
  minimum:  "Mistral-Nemo-Instruct-2407-q4f16_1-MLC",
  fallback: "Mistral-7B-Instruct-v0.3-q4f16_1-MLC",
};

async function selectTier(): Promise<keyof typeof TIER_MODELS | null> {
  const adapter = await navigator.gpu?.requestAdapter();
  if (!adapter) return null;
  const device = await adapter.requestDevice();
  const vramGb = device.limits.maxBufferSize / (1024 ** 3);
  if (vramGb >= 16) return "full";
  if (vramGb >= 8)  return "standard";
  if (vramGb >= 4)  return "minimum";
  if (vramGb >= 2)  return "fallback";
  return null; // LLM disabled — show programmatic builder instead
}
```

---

## 4. WebLLM — local AI composition

### 4.1 Role

WebLLM is the intent *composer*. It reads the user's message and current terminal context, and produces a structured `LLMIntentDraft`. It has no network access, holds no keys, and cannot execute anything. Its output is treated as untrusted input by `spacekit-js`.

### 4.2 Context object

```typescript
interface TradingContext {
  message:   string;

  chart: {
    symbol:        string;        // e.g. "ETH/USDC"
    timeframe:     string;        // "1H", "4H", "1D"
    current_price: number;
    change_24h:    number;        // percentage
    ohlcv:         OHLCV[];       // last 60 candles at current timeframe
    indicators: {
      rsi_14:      number;
      macd:        { value: number; signal: number; histogram: number };
      bb_upper:    number;
      bb_lower:    number;
      volume_sma:  number;
    };
    chart_state: "uptrend" | "downtrend" | "ranging" | "breakout" | "breakdown";
  };

  portfolio: {
    positions: { asset: string; amount: string; avg_cost: number; unrealised_pnl: number }[];
    balances:  { asset: string; amount: string; usd_value: number }[];
    total_usd: number;
  };

  news: { headline: string; source: string; sentiment: number; age_minutes: number }[];

  history: { role: "user" | "assistant"; content: string }[]; // last 8 turns

  agent_scopes?: AgentScopeSummary[];
}
```

### 4.3 System prompt

```
You are a trading assistant for the TradingKit terminal. You help traders
analyse markets and compose trade intents.

RULES:
1. You run entirely on the user's device. Their data never leaves the browser.
2. When proposing a trade, output ONLY valid JSON matching the
   LLMIntentDraft schema. No prose outside the JSON.
3. When answering a question (no trade), output a plain text response
   in the "rationale" field with an empty "actions" array.
4. Never produce: actor, agent, nonce, expiry, intent_id, or
   allowed_venues. These are set by the system.
5. venue_hint is advisory — you may suggest a venue, but the system
   may route differently for better execution.
6. Express uncertainty as conservative constraints (low max_notional_usd,
   tight min_amount_out), not as hedged prose.
7. Always explain WHY in rationale — cite actual values from context
   (RSI reading, price level, portfolio weight, news sentiment score).
8. Your analysis is informational only. It is not financial advice.
   Note this briefly in every rationale.

OUTPUT FORMAT (trade intent):
{
  "label": "brief action description",
  "actions": [ ...Action[] ],
  "constraints": { ...Partial<Constraints> },
  "rationale": "analysis citing actual context data",
  "confidence": "high" | "medium" | "low"
}

OUTPUT FORMAT (question / analysis only):
{
  "label": "summary",
  "actions": [],
  "constraints": {},
  "rationale": "full analysis",
  "confidence": "high" | "medium" | "low"
}
```

### 4.4 Output validation

`spacekit-js` validates every model output before proceeding. Malformed, schema-invalid, or policy-violating outputs are rejected and the user is shown an error with a retry option.

```typescript
function validateDraft(raw: string): LLMIntentDraft {
  // Strip any markdown fences the model may have emitted
  const clean = raw.replace(/```json|```/g, "").trim();

  let parsed: unknown;
  try {
    parsed = JSON.parse(clean);
  } catch {
    throw new IntentError("LLM_OUTPUT_INVALID", "Model output was not valid JSON");
  }

  // Zod schema validation
  const result = LLMIntentDraftSchema.safeParse(parsed);
  if (!result.success) {
    throw new IntentError("LLM_OUTPUT_INVALID", result.error.message);
  }

  // Security: model must not produce these fields
  const forbidden = ["actor", "agent", "nonce", "expiry", "intent_id"];
  for (const field of forbidden) {
    if (field in result.data) {
      throw new IntentError("LLM_OUTPUT_INVALID", `Model produced forbidden field: ${field}`);
    }
  }

  // Policy: model must not set venue constraints (user decision, not model inference)
  if (result.data.constraints?.allowed_venues?.length) {
    throw new IntentError("LLM_OUTPUT_INVALID", "Model must not set allowed_venues");
  }

  return result.data;
}
```

---

## 5. Route AI — intent routing

### 5.1 Relay architecture

```
apps/relay/
│
├── HTTP server (Bun.serve)
│   ├── POST /v1/complete        → streaming completion endpoint
│   ├── POST /v1/intent          → signed intent submission
│   ├── GET  /v1/intent/:id      → intent status
│   ├── GET  /v1/health          → provider health graph (JSON)
│   └── GET  /v1/nonce/:actor/:chain  → proxies to Compute Node
│
├── Intent Classifier
│   reads: message content + declared intent metadata
│   produces: { task_type, complexity, urgency }
│
├── Routing Engine
│   reads: classified intent + live provider health graph
│   matches against active routing profile rules
│   selects winning provider + model in < 2ms
│
├── Provider Health Monitor (background)
│   polls each provider every 30s
│   tracks: p50/p95 latency, error rate, token throughput
│   computes z-score vs 30-day rolling baseline
│   shifts traffic at 2σ deviation (proactive, before user impact)
│
├── Provider Adapters
│   ├── OpenAI     (OpenAI-compatible API)
│   ├── Anthropic  (Messages API)
│   ├── Mistral    (OpenAI-compatible)
│   ├── Google     (Gemini API)
│   └── Custom     (any OpenAI-compatible endpoint)
│
└── Cost Tracker
    records: provider, model, tokens in/out, latency, task_type
    aggregates: per operator / per profile / per day
```

### 5.2 Routing decision algorithm

```typescript
async function route(
  intent: ClassifiedIntent,
  profile: RoutingProfile
): Promise<ProviderTarget> {

  // 1. Find matching route rules in priority order (first match wins)
  const rule = profile.routes.find(r => matchesIntent(r.match, intent));
  if (!rule) throw new RoutingError("NO_MATCHING_RULE");

  // 2. Get candidate providers meeting the rule's constraints
  const candidates = healthGraph.getProviders()
    .filter(p => p.meetsLatencyTarget(rule.target.latency))
    .filter(p => p.meetsQualityTarget(rule.target.quality))
    .filter(p => !profile.constraints.never_send_to?.includes(p.id))
    .filter(p => p.currentCostPerToken <= maxCostFor(rule.target.cost));

  if (!candidates.length) {
    return fallback(intent, profile, rule); // try next quality tier down
  }

  // 3. Score: reliability (40%) + latency (35%) + cost (25%)
  const scored = candidates.map(p => ({
    provider: p,
    score:
      (1 - p.currentErrorRate)  * 0.40 +
      (1 - p.normalizedLatency) * 0.35 +
      (1 - p.normalizedCost)    * 0.25,
  }));

  return scored.sort((a, b) => b.score - a.score)[0].provider;
}
```

### 5.3 Task taxonomy for the trading terminal

```typescript
type TradingTaskType =
  | "price_lookup"           // "What is ETH trading at?"
  | "ticker_info"            // "Tell me about BTC"
  | "indicator_explanation"  // "What does this RSI reading mean?"
  | "news_summary"           // "Summarise NVDA news"
  | "chart_analysis"         // "What does this chart tell you?"
  | "pattern_recognition"    // "Is this a head and shoulders?"
  | "strategy_analysis"      // "Should I enter a long here?"
  | "multi_timeframe"        // "How does this look across 1H, 4H, 1D?"
  | "risk_modelling"         // "What's my downside if ETH drops 15%?"
  | "backtest_reasoning"     // "Would this strategy have worked in 2022?"
  | "intent_composition"     // fallback: WebLLM unavailable, relay composes
  | "general_question";      // anything else

// Routing latency targets by task type
const LATENCY_TARGETS: Record<TradingTaskType, string> = {
  price_lookup:          "p95<100ms",
  ticker_info:           "p95<200ms",
  indicator_explanation: "p95<300ms",
  news_summary:          "p95<500ms",
  chart_analysis:        "p95<800ms",
  pattern_recognition:   "p95<800ms",
  strategy_analysis:     "p95<3s",
  multi_timeframe:       "p95<3s",
  risk_modelling:        "p95<5s",
  backtest_reasoning:    "p95<8s",
  intent_composition:    "p95<5s",
  general_question:      "p95<1s",
};
```

### 5.4 Provider health graph

```typescript
interface ProviderHealth {
  id:              string;
  name:            string;
  models:          string[];
  p50_latency_ms:  number;
  p95_latency_ms:  number;
  error_rate:      number;       // 0–1, rolling 5-minute window
  tokens_per_sec:  number;
  zscore:          number;       // deviation from 30-day baseline
  traffic_weight:  number;       // 0–1 (1.0 = full, 0.0 = failed over)
  status:          "healthy" | "degraded" | "failed";
}

// Traffic weight adjustment on degradation
function trafficWeight(zscore: number): number {
  if (zscore < 2.0) return 1.0;   // healthy — full traffic
  if (zscore < 3.0) return 0.7;   // early degradation — 30% shifted
  if (zscore < 4.0) return 0.3;   // significant degradation — 70% shifted
  return 0.0;                     // failed — full failover
}
```

### 5.5 Streaming

All completion requests stream. The relay proxies token streams from the provider directly to the client with no buffering. A `X-RouteAI-Provider` header on the first SSE event tells the client which model is responding.

```
Client  →  POST /v1/complete  (SSE / EventStream)
Relay   →  classifies intent, selects provider
Relay   →  opens streaming request to provider
Relay   →  pipes provider SSE tokens → client
Client  →  renders tokens in real-time as they arrive
```

---

## 6. SpaceKit — on-chain execution

### 6.1 On-chain components

```
SpaceKit network
│
├── SpaceKit Compute Node  (off-chain, Mainnet-connected)
│   ├── Nonce service        GET /v1/nonce/{actor_id}/{chain_id}
│   ├── Actor state          GET /v1/actor/{actor_id}
│   ├── Event indexer        indexes IntentExecuted chain events
│   └── ML-KEM transport     all channels quantum-safe
│
└── On-chain programs  (Rust, deployed per chain)
    │
    ├── intent-executor/
    │   execute_intent(ctx, signed_intent) → ExecutionReceipt
    │   · verify signature
    │   · verify agent scope (if agent-signed)
    │   · replay protection (nonce + expiry)
    │   · dispatch actions to adapter registry
    │   · verify output constraints post-execution
    │   · emit IntentExecuted event
    │
    ├── agent-scope-registry/
    │   grant_scope(agent_id, scope) → ScopeId
    │   revoke_scope(agent_id)
    │   get_scope(agent_id, actor_id) → AgentScope
    │
    └── adapter-registry/
        ├── uniswap-v3     (Ethereum:1, Base:8453)
        ├── pancakeswap    (BSC:56)
        ├── stargate-v2    (bridge, multi-chain)
        └── erc20          (approve, transfer — all EVM)
```

### 6.2 execute_intent — Rust

```rust
pub fn execute_intent(
    ctx: Context<ExecuteIntent>,
    payload: Vec<u8>,        // canonical JSON intent payload
    signature: [u8; 64],     // Ed25519 or secp256k1 sig over intent_id
    sig_type: SigType,
) -> Result<ExecutionReceipt> {

    let intent: Intent = deserialize_and_validate(&payload)?;

    // 1. Verify signature over intent_id
    verify_signature(&intent.intent_id, &signature, &intent.actor, sig_type)?;

    // 2. If agent-signed, verify scope on-chain
    if let Some(ref agent_id) = intent.agent {
        let scope = ctx.accounts.scope_registry
            .get_scope(agent_id, &intent.actor)
            .ok_or(ErrorCode::AgentScopeNotFound)?;
        verify_agent_scope(&scope, &intent)?;
    }

    // 3. Replay protection
    require!(
        intent.expiry > Clock::get()?.unix_timestamp as u64,
        ErrorCode::IntentExpired
    );
    require!(
        !ctx.accounts.nonce_registry.contains(&intent.actor, &intent.nonce),
        ErrorCode::NonceReplayed
    );
    ctx.accounts.nonce_registry.insert(&intent.actor, &intent.nonce);

    // 4. Execute actions sequentially via adapter registry
    let mut receipts: Vec<ActionReceipt> = vec![];
    for action in &intent.actions {
        let adapter = ctx.accounts.adapter_registry.get(&action.action_type())?;
        let receipt = adapter.execute(ctx, action, &intent.constraints)?;
        receipts.push(receipt);
    }

    // 5. Post-execution output constraint check
    verify_output_constraints(&receipts, &intent.constraints)?;

    // 6. Emit event (indexed by Compute Node)
    emit!(IntentExecuted {
        intent_id: intent.intent_id.clone(),
        actor:     intent.actor.clone(),
        agent:     intent.agent.clone(),
        chain:     intent.chain.clone(),
        receipts:  receipts.clone(),
        timestamp: Clock::get()?.unix_timestamp,
    });

    Ok(ExecutionReceipt { intent_id: intent.intent_id, receipts })
}
```

### 6.3 Supported actions v1

| Action | Chains | Adapter |
|---|---|---|
| `swap` | Ethereum:1, Base:8453 | Uniswap v3 |
| `swap` | BSC:56 | PancakeSwap v3 |
| `bridge` | Ethereum:1 ↔ Base:8453, BSC:56 | Stargate v2 |
| `approve` | All EVM | ERC-20 |
| `transfer` | All EVM | ERC-20 / native |
| `batch` (single-chain) | All EVM | Sequential sub-adapter dispatch |

### 6.4 Direct-to-chain fallback

When the relay is unreachable, `spacekit-js` may submit eligible intents directly to the on-chain contract. Bridge intents and `match_required` intents require the relay and cannot be submitted directly. On-chain verification is identical regardless of submission path.

```typescript
async function submitWithFallback(signed: SignedIntent): Promise<IntentReceipt> {
  try {
    return await relay.submit(signed);
  } catch (e) {
    if (!(e instanceof RelayUnavailableError)) throw e;
    if (!isDirectSubmittable(signed.intent)) {
      throw new IntentError(
        "RELAY_REQUIRED",
        "This intent contains bridge actions that require the relay."
      );
    }
    // User must explicitly confirm before bypassing relay
    await ui.confirmDirectSubmission();
    return await chain.submitDirect(signed);
  }
}

function isDirectSubmittable(intent: Intent): boolean {
  const directTypes = ["swap", "approve", "transfer"];
  return intent.actions.every(a =>
    directTypes.includes(a.type) ||
    (a.type === "batch" &&
      (a as BatchAction).actions.every(sub => directTypes.includes(sub.type)))
  );
}
```

---

## 7. Intent lifecycle — end to end

```
                 BROWSER                              SERVER / CHAIN
                 ───────                              ──────────────

User submits message
        │
        ▼
llm.worker (WebLLM)
  compose(message, TradingContext)
        │  1–8s depending on model tier
        ▼
LLMIntentDraft (raw JSON)
        │
        ▼
validateDraft()          ← schema + security checks
        │
        ▼ if actions.length > 0
spacekit.simulate(draft)
  ├── fetch live quotes ─────────────────────────▶ price feed API
  │   ◄──────────────────────────────── quotes ───┤
  ├── simulate actions in WASM VM
  └── check constraints (slippage, notional)
        │
        ▼
SimulationResult shown to user
[Sign]  [Reject]
        │ user approves
        ▼
GET /v1/nonce/{actor}/{chain} ─────────────────▶ Compute Node
◄──────────────── { nonce, valid_until } ────────┤
        │
        ▼
buildIntent(draft, { actor, nonce, expiry, chain })
intent_id = SHA3-256(canonicalPayload(intent))
        │
        ▼
wallet.sign(intent_id)       ← user confirms in wallet
        │
        ▼
SignedIntent { intent, signature, sig_type }
        │
        │ [ML-KEM-1024 encrypted channel]
        ▼
POST /v1/intent ───────────────────────────────▶ Route AI relay
                                                  │
                                             classify intent
                                             select provider
                                             route to SpaceKit
                                                  │
                                         SpaceKit WASM contract
                                         verify sig
                                         check agent scope
                                         check nonce / expiry
                                         execute actions
                                         emit IntentExecuted
                                                  │
◄──────────────────── ExecutionReceipt ───────────┘
        │
        ▼
Update portfolio · chart · transaction history
Display execution receipt
```

---

## 8. Market data layer

### 8.1 MarketSnapshot interface

```typescript
// packages/market-context/src/types.ts

interface OHLCV {
  timestamp:  number;    // Unix seconds
  open: number; high: number; low: number; close: number; volume: number;
}

interface MarketSnapshot {
  symbol:         string;          // e.g. "ETH/USDC"
  chain:          ChainId;
  price:          number;
  change_1h:      number;          // percentage
  change_24h:     number;
  change_7d:      number;
  ohlcv_1h:       OHLCV[];         // last 48 candles
  ohlcv_4h:       OHLCV[];         // last 30 candles
  ohlcv_1d:       OHLCV[];         // last 30 candles
  indicators: {
    rsi_14:        number;
    macd:          { value: number; signal: number; histogram: number };
    bb:            { upper: number; middle: number; lower: number };
    atr_14:        number;
    volume_sma_20: number;
  };
  chart_state:    "uptrend" | "downtrend" | "ranging" | "breakout" | "breakdown";
  liquidity_usd:  number;
  news:           NewsItem[];
}
```

### 8.2 Data sources

| Data type | Source | Frequency |
|---|---|---|
| Price / OHLCV | DEX subgraph (The Graph) | 15s |
| Indicators | Computed client-side from OHLCV | On new candle |
| Liquidity depth | DEX pool state via RPC | 30s |
| News | Aggregated feed (CryptoPanic + custom) | 5 min |
| Portfolio / positions | SpaceKit Compute Node + RPC | 30s |

Indicators are computed client-side in the WASM VM from raw OHLCV. No indicator values are fetched from external services — this prevents staleness mismatches and keeps computation locally verifiable.

---

## 9. Provider configuration — BYOK

### 9.1 Config schema

```yaml
# config/providers.yaml  (gitignored — never committed)

providers:
  openai:
    api_key: "${OPENAI_API_KEY}"
    base_url: "https://api.openai.com/v1"
    models:
      - id: "gpt-4o"
        context_window: 128000
        cost_per_1m_in: 2.50
        cost_per_1m_out: 10.00
        quality_tier: maximum
      - id: "gpt-4o-mini"
        context_window: 128000
        cost_per_1m_in: 0.15
        cost_per_1m_out: 0.60
        quality_tier: standard

  anthropic:
    api_key: "${ANTHROPIC_API_KEY}"
    base_url: "https://api.anthropic.com"
    models:
      - id: "claude-opus-4-5"
        context_window: 200000
        cost_per_1m_in: 15.00
        cost_per_1m_out: 75.00
        quality_tier: maximum
      - id: "claude-sonnet-4-5"
        context_window: 200000
        cost_per_1m_in: 3.00
        cost_per_1m_out: 15.00
        quality_tier: high
      - id: "claude-haiku-4-5"
        context_window: 200000
        cost_per_1m_in: 0.80
        cost_per_1m_out: 4.00
        quality_tier: standard

  mistral:
    api_key: "${MISTRAL_API_KEY}"
    base_url: "https://api.mistral.ai/v1"
    models:
      - id: "mistral-large-latest"
        context_window: 131072
        cost_per_1m_in: 2.00
        cost_per_1m_out: 6.00
        quality_tier: high
      - id: "mistral-small-latest"
        context_window: 131072
        cost_per_1m_in: 0.10
        cost_per_1m_out: 0.30
        quality_tier: standard
```

### 9.2 Key security model

Provider API keys are:

- Stored in `config/providers.yaml` on the relay server (gitignored, never committed)
- Loaded into relay memory at startup
- Never logged, never returned in API responses, never transmitted to the client
- Configurable as environment variables in production (`$OPENAI_API_KEY`, etc.)
- Scoped per operator — each Route AI operator account has its own isolated key set

The client (trading terminal) only ever holds a Route AI API key (`sk-routeai-...`). Provider keys never appear in browser code, network requests, or client-side storage.

### 9.3 SDK initialisation

```typescript
// Application code — developer holds only the Route AI key
const routeai = new RouteAI({
  apiKey:       "sk-routeai-xxxxxxxx",
  relayUrl:     "https://relay.swtch.ai",
  computeNode:  "https://node.spacekit.xyz",
  profile:      "trading-terminal",
});

// All provider routing, key handling, and model selection
// is managed by the relay. Zero provider configuration in app code.
```

---

## 10. Quantum-safe transport

All network channels between the terminal, relay, and Compute Node use NIST PQC 2024 standards. Classical TLS alone is not acceptable for channels carrying intent payloads or key material.

### 10.1 Algorithm selection

| Purpose | Algorithm | Standard |
|---|---|---|
| Key encapsulation | ML-KEM-1024 + X25519 (hybrid) | FIPS 203 |
| Digital signatures | ML-DSA-87 | FIPS 204 |
| Intent hashing (intent_id) | SHA3-256 | FIPS 202 |
| Symmetric session encryption | AES-256-GCM | FIPS 197 |

The hybrid KEM (`X25519 + ML-KEM-1024`) means an attacker must break *both* classical and post-quantum components to recover the session key. This defends against harvest-now-decrypt-later attacks and provides a fallback if a ML-KEM flaw is discovered.

### 10.2 Channel requirements

| Channel | Protection |
|---|---|
| Browser → Route AI relay | TLS 1.3 + ML-KEM-1024/X25519 hybrid KEM |
| Browser → Compute Node | TLS 1.3 + ML-KEM-1024/X25519 hybrid KEM |
| Relay → Compute Node | mTLS, ML-DSA-87 certificates |
| Relay → Provider APIs | Standard TLS 1.3 (provider-controlled) |
| Operator admin API | ML-DSA-87 signed requests |

### 10.3 Browser-side session establishment

```typescript
// packages/spacekit-js/src/transport.ts

async function establishSession(relayUrl: string): Promise<SecureSession> {
  // 1. Generate ephemeral ML-KEM-1024 key pair in browser
  const { publicKey, privateKey } = await mlkem.generateKeyPair(1024);

  // 2. Fetch relay identity key from Compute Node on-chain directory
  const relayIdentityKey = await computeNode.getRelayPublicKey(relayUrl);

  // 3. Send browser's ML-KEM public key to relay
  const res = await fetch(`${relayUrl}/v1/session`, {
    method: "POST",
    body: JSON.stringify({ mlkem_public_key: publicKey.toHex() }),
  });
  const { ciphertext, signature } = await res.json();

  // 4. Verify relay's ML-DSA-87 signature over the ciphertext
  mlDSA87.verify(relayIdentityKey, ciphertext, signature);

  // 5. Decapsulate to derive shared secret
  const sharedSecret = await mlkem.decapsulate(ciphertext, privateKey);

  // 6. Derive AES-256-GCM session key
  const sessionKey = await hkdf(sharedSecret, "swtch-terminal-v1");

  return new SecureSession(sessionKey);
}
```

### 10.4 What quantum-safe does NOT cover in v1

- **User intent signing keys (Ed25519 / secp256k1):** Wallet-constrained. Migration to ML-DSA is a v2 concern pending wallet ecosystem adoption.
- **On-chain contract storage:** Intent execution receipts are public by design.
- **WebLLM model weight downloads:** Integrity verified by SHA-256 manifest check; standard TLS sufficient for this channel.

---

## 11. Agent mode

### 11.1 Scope grant flow

```
User configures agent limits in terminal UI:
  Allowed assets:  ETH, BTC, USDC
  Allowed actions: swap only
  Max per intent:  $500 notional
  Max per hour:    10 intents
  Duration:        7 days
          │
          ▼
spacekit-js builds AgentScope object + grantScope transaction
          │
          ▼
User signs with wallet (once)
          │
          ▼
SpaceKit agent-scope-registry contract records scope on-chain
          │
          ▼
Agent can now sign and submit intents within scope
without per-intent user wallet confirmation
```

### 11.2 On-chain scope verification

```rust
fn verify_agent_scope(scope: &AgentScope, intent: &Intent) -> Result<()> {
    require!(scope.expiry > Clock::get()?.unix_timestamp as u64,
             ErrorCode::AgentExpired);

    for action in &intent.actions {
        require!(scope.allowed_actions.contains(&action.action_type()),
                 ErrorCode::AgentScopeExceeded);
    }

    if !scope.allowed_assets.is_empty() {
        for asset in intent.all_assets() {
            require!(scope.allowed_assets.contains(&asset),
                     ErrorCode::AgentScopeExceeded);
        }
    }

    if let Some(max_notional) = scope.max_notional_usd {
        require!(intent.estimated_notional_usd()? <= max_notional,
                 ErrorCode::AgentScopeExceeded);
    }

    if let Some(max_freq) = scope.max_frequency {
        let recent = scope_registry.count_recent(
            &scope.agent_id, &scope.actor_id, 3600
        )?;
        require!(recent < max_freq, ErrorCode::AgentScopeExceeded);
    }

    Ok(())
}
```

### 11.3 Revocation

```typescript
// Immediate on-chain revocation
await spacekit.revokeAgentScope(agentId, wallet.signer);
// After confirmation: agent cannot sign any further intents.
// In-flight intents signed before revocation still execute.
```

---

## 12. Nonce management

### 12.1 Authority

The SpaceKit Compute Node on Mainnet is the authoritative nonce issuer. It guarantees uniqueness across all devices and sessions for the same actor on the same chain.

### 12.2 Protocol

```
spacekit-js                              Compute Node
     │                                        │
     │  GET /v1/nonce/{actor}/{chain}         │
     │  [ML-KEM encrypted, actor-authenticated]
     │───────────────────────────────────────▶│
     │                                        │ atomic increment
     │  { nonce: "1042",                      │
     │    valid_until: <unix + 120s> }        │
     │◀───────────────────────────────────────│
     │                                        │
 build intent with nonce                      │
 sign intent                                  │
 submit before valid_until                    │
     │                                        │
     │  [after on-chain confirmation]          │
     │  Compute Node reads IntentExecuted      │
     │  event → marks nonce "1042" consumed    │
```

Multi-device safety: the Compute Node issues nonces with an atomic counter. Device A gets `1042`, Device B gets `1043`. No collision is possible. If a nonce expires before the intent is submitted, the client fetches a fresh nonce.

---

## 13. Error handling

### 13.1 Error taxonomy

| Code | Layer | Meaning | Recovery |
|---|---|---|---|
| `LLM_OUTPUT_INVALID` | Client | Model output failed validation | Retry compose |
| `LLM_UNAVAILABLE` | Client | WebLLM not loaded (low VRAM) | Use programmatic builder |
| `SCHEMA_INVALID` | Client / Relay | Intent schema violation | Fix and recompose |
| `SIMULATION_FAILED` | Client | Stale quotes or RPC failure | Retry |
| `EXPIRY_EXCEEDED` | Relay / Contract | Intent expired | Re-fetch nonce, re-sign |
| `NONCE_REPLAYED` | Contract | Nonce already consumed | Re-fetch nonce |
| `NONCE_STALE` | Compute Node | Nonce expired before use | Re-fetch nonce |
| `SIG_INVALID` | Relay / Contract | Signature verification failed | Re-sign |
| `AGENT_SCOPE_EXCEEDED` | Contract | Action exceeds agent grant | Expand scope or sign directly |
| `AGENT_EXPIRED` | Contract | Agent scope grant expired | Re-grant scope |
| `CONSTRAINT_SLIPPAGE` | Client / Contract | Slippage exceeded tolerance | Widen tolerance or retry |
| `CONSTRAINT_NOTIONAL` | Client / Contract | Notional cap exceeded | Reduce size |
| `RELAY_UNAVAILABLE` | Client | Relay unreachable, direct submit eligible | User confirms direct submit |
| `RELAY_REQUIRED` | Client | Bridge intent, relay required | Wait for relay recovery |
| `PROVIDER_DEGRADED` | Relay | Provider degraded, traffic shifted | Automatic — no user action |
| `NO_MATCHING_RULE` | Relay | Intent matched no profile rule | Check profile config |
| `QS_HANDSHAKE_FAILED` | Transport | ML-KEM handshake failed | Retry connection |
| `QS_SIG_INVALID` | Transport | ML-DSA relay signature invalid | Discard — possible MITM |

### 13.2 User-facing error presentation

Errors appear as inline system messages in the assistant panel — never as blocking modals. Every error includes a human-readable explanation and at least one recovery action button. Error codes are never shown to users.

```
╔══════════════════════════════════════════════════════╗
║  ⚠  Your ETH balance changed since simulation.       ║
║     Simulated: 1.02 ETH · Current: 0.89 ETH         ║
║                                                      ║
║  [Re-simulate]   [Adjust amount]   [Cancel]          ║
╚══════════════════════════════════════════════════════╝
```

---

## 14. Deployment topology

### 14.1 Production

```
                    Cloudflare CDN
                          │
                    Trading UI
                    (static build)
                          │
           ┌──────────────┴──────────────┐
           │                             │
   Route AI relay                 SpaceKit
   (Fly.io / Render)               Compute Node
   2× instances min                (bare metal,
   Docker + Bun                     Mainnet-connected)
           │                             │
  ┌────────┼────────┐          ┌─────────┴────────┐
  │        │        │          │                  │
OpenAI  Anthropic Mistral   EVM RPC            On-chain
API     API       API       endpoints          contracts
```

### 14.2 Environment variables

```bash
# Route AI relay
ROUTEAI_API_SECRET=           # Signs relay-issued session tokens
OPENAI_API_KEY=               # Provider keys (or use providers.yaml)
ANTHROPIC_API_KEY=
MISTRAL_API_KEY=
COMPUTE_NODE_URL=             # SpaceKit Compute Node endpoint
COMPUTE_NODE_PUBLIC_KEY=      # ML-DSA-87 public key for verification

# SpaceKit Compute Node
MAINNET_RPC_URL=
COMPUTE_NODE_IDENTITY_KEY=    # ML-DSA-87 private key (HSM in production)
NONCE_CHECKPOINT_INTERVAL=    # Blocks between on-chain checkpoints (default: 100)

# Trading terminal (Vite build-time)
VITE_RELAY_URL=               # Route AI relay endpoint
VITE_COMPUTE_NODE_URL=        # Compute Node endpoint
VITE_SUPPORTED_CHAINS=        # Comma-separated: ethereum:1,bsc:56,base:8453
```

### 14.3 Health endpoints

```
Route AI relay:    GET /health
  → { status, uptime_ms, providers: ProviderHealth[] }

Compute Node:      GET /v1/health
  → { status, block, lag_ms, nonce_service: "ok" | "degraded" }

Terminal (client): WebLLM worker → main thread postMessage
  → { type: "health", model_loaded, tier, vram_used_gb }
```

---

## 15. Performance targets

| Metric | Target | Notes |
|---|---|---|
| Intent classification | < 1ms | Rules-based, in-memory |
| Routing decision | < 2ms | In-memory health graph |
| Nonce fetch | p95 < 50ms | ML-KEM handshake amortised per session |
| Simulation | p95 < 800ms | Depends on price feed latency |
| Model TTFT — fast tier | < 200ms | Interactive tasks (price, indicators) |
| Model TTFT — capable tier | < 1.5s | Analysis and strategy tasks |
| WebLLM first token (warm) | < 400ms | Model already loaded in GPU context |
| WebLLM cold start | 30–90s | First load including download if uncached |
| Full loop (question → receipt) | < 15s typical | Includes user review and wallet signing |
| Provider failover | < 3s | Proactive at 2σ — before user impact |

---

## 16. Development roadmap

### v0.1 — Foundation (current)

- [ ] Trading UI shell: chart, portfolio, assistant panels
- [ ] Market data layer: price, OHLCV, indicators (computed client-side)
- [ ] WebLLM integration: background worker, tier selection
- [ ] Route AI relay: classification, routing engine, provider adapters, streaming
- [ ] intent-schema package v0.2 (shared types)
- [ ] spacekit-js: simulation, nonce fetch, signing, submission
- [ ] EVM intent executor contract (Ethereum:1 + Base:8453)
- [ ] Uniswap v3 adapter
- [ ] BYOK provider config

### v0.2 — Agent mode + multi-chain

- [ ] Agent scope registry contract
- [ ] Agent mode UI: scope grant, revocation, activity log
- [ ] BSC:56 + PancakeSwap adapter
- [ ] Stargate v2 bridge adapter
- [ ] Quantum-safe transport (ML-KEM-1024 + ML-DSA-87)
- [ ] Provider health dashboard
- [ ] Routing profile editor (UI)

### v0.3 — Platform + external operators

- [ ] Multi-operator BYOK dashboard
- [ ] Route AI SDK (packages/route-ai-sdk) published to npm
- [ ] Solana support (Anchor program, Jupiter adapter)
- [ ] Fine-tuned WebLLM model (trading domain, based on Mistral Nemo)
- [ ] kit.space integration (share strategies, intent history)

### v1.0 — Public launch

- [ ] Third-party security audit of WASM contracts
- [ ] Third-party audit of quantum-safe transport implementation
- [ ] Public Route AI API for external developers
- [ ] TradFi adapter (broker API, ISIN identifiers, order types, partial fills)
- [ ] Mobile-responsive terminal

---

*End of Architecture & Technical Specification v0.1*  
*swtch labs llc · swtch.ai · spacekit.xyz · kit.space*