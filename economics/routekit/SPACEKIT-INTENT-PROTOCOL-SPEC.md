# SpaceKit Intent Protocol
## Technical Specification  v0.2

**Status:** Draft for internal review  
**Authors:** SpaceKit Core  
**Scope:** Intent schema, authorization model, lifecycle, relay, on-chain execution, quantum-safe transport, and nonce management.  
**Out of scope:** Tokenomics, governance, cross-chain bridge implementation details.

**Changelog v0.1 → v0.2**
- Resolved: Nonce management via SpaceKit Compute Node on Mainnet (§3.1, §11)
- Resolved: `venue_hint` documented as strictly advisory (§3.4.1)
- Resolved: Relay fallback — direct-to-chain path defined for chain-executable intents (§6.2, §11)
- Resolved: WebLLM model recommendation — Mistral Nemo 12B minimum, capability-tiered (§5.1)
- Resolved: Quantum-safe encryption required for all network communications (§10)
- Section 11 (previously Open Questions) is now Resolved Decisions

---

## 1. Design Principles

**1. AI stays in the browser.**  
The local WebLLM model is an intent *composer*, not a network actor. It reads user context, produces a structured intent payload, and hands it to spacekit-js for simulation and signing. The model never touches the relay or the chain directly. This eliminates oracle trust problems entirely.

**2. The intent is the unit of trust.**  
Everything the user authorises is expressed as a signed, versioned, chain-agnostic intent payload. The relay, the on-chain contract, and any future execution environment all operate on this single artifact. Nothing is inferred from out-of-band context.

**3. Simulate before you sign.**  
The spacekit-js VM simulates every intent against live quotes before presenting it to the user. Users sign outcomes they can see, not parameters they have to mentally execute.

**4. Agents are scoped, not trusted.**  
An agent (bot, strategy, LLM cursor) may only act within the explicit, user-signed scope it has been granted. Scope is verified on-chain by the contract before any action executes.

**5. The relay is real infrastructure, not optional.**  
The relay is a required protocol layer. A centralised reference implementation is acceptable for v1. Decentralisation is a roadmap concern, not a v1 concern.

**6. All network communications are quantum-safe.**  
Every channel between the browser, the relay, and the SpaceKit Compute Node uses post-quantum cryptography for key exchange and message authentication. This applies to users, the system, and operators. Classical TLS is not sufficient. See §10.

---

## 2. System Overview

```
┌─────────────────────────────────────────────────────────┐
│  Browser  (spacekit-js)                                  │
│                                                          │
│  ┌──────────────┐    ┌───────────────┐                  │
│  │  WebLLM      │───▶│  Intent       │                  │
│  │  Mistral 4.5+│    │  Builder UI   │                  │
│  └──────────────┘    └───────┬───────┘                  │
│                              │ draft intent              │
│                       ┌──────▼───────┐                  │
│                       │  WASM VM     │                   │
│                       │  Simulate    │                   │
│                       └──────┬───────┘                  │
│                              │ simulation result         │
│                       ┌──────▼───────┐                  │
│                       │  Sign (user  │                   │
│                       │  or agent)   │                   │
│                       └──────┬───────┘                  │
└──────────────────────────────┼──────────────────────────┘
                               │ signed intent
                               │ [ML-KEM encrypted channel]
                        ┌──────▼───────┐
                        │  Relay /     │
                        │  Matcher     │
                        └──────┬───────┘
                               │ routed intent
          ┌────────────────────┼────────────────────┐
          │                    │                    │
   ┌──────▼──────┐   ┌─────────▼──────┐  ┌─────────▼──────┐
   │ SpaceKit    │   │  Ethereum:1    │  │  BSC:56 /      │
   │ Compute     │   │  (WASM         │  │  Base:8453     │
   │ Node        │   │   contract)    │  │  (WASM         │
   │ (Mainnet)   │   │                │  │   contract)    │
   │ Nonce mgmt  │   └────────────────┘  └────────────────┘
   │ State sync  │
   └─────────────┘

  All relay ↔ compute node ↔ chain channels: ML-KEM + ML-DSA
```

**WebLLM** reads the user's conversational context and current portfolio state. It produces a *draft* `Intent` object — a plain JSON payload. It does not sign anything. See §5 for model requirements.

**spacekit-js WASM VM** ingests the draft intent, fetches live quotes from an off-chain price service, simulates execution against current chain state, and renders the outcome to the user. If the user approves, the VM presents it for signing.

**Signing** is performed by the user's key (Ed25519 or secp256k1) or by a pre-authorised agent key. The signature covers the canonical hash of the full intent payload.

**The relay** receives the signed intent over an ML-KEM encrypted channel, validates schema and expiry, optionally nets it against other pending intents, and routes it to the correct chain.

**The SpaceKit Compute Node** runs on Mainnet and is the authoritative source for nonce state. It also acts as a coordination point for multi-chain state queries and event indexing.

**The on-chain WASM contract** verifies the signature, checks agent scope if applicable, enforces constraints, resolves each action via the appropriate adapter, updates state, and emits execution events.

---

## 3. Intent Schema

Defined in TypeScript (spacekit-js) and mirrored in Rust (on-chain contract). Both implementations share a canonical JSON serialisation for signing purposes.

### 3.1 Core Intent Object

```typescript
interface Intent {
  // ── Identity ──────────────────────────────────────────
  intent_id:   string;          // SHA-256 hex of canonical payload (set after construction)
  version:     "1.0";           // Protocol version. Bump on breaking schema changes.

  // ── Actors ────────────────────────────────────────────
  actor:       ActorId;         // User address, DID, or public key
  agent?:      AgentId;         // Authorised agent, if acting on behalf of actor.
                                // Absent = user is signing directly.

  // ── Execution target ──────────────────────────────────
  chain:       ChainId;         // e.g. "ethereum:1", "bsc:56", "base:8453"
                                // For bridge intents, this is the SOURCE chain.
                                // Target chain is expressed in the Bridge action.

  // ── Constraints ───────────────────────────────────────
  constraints: Constraints;

  // ── Actions ───────────────────────────────────────────
  actions:     Action[];        // Ordered list. Executed atomically or sequentially
                                // depending on contract implementation.

  // ── Replay protection ─────────────────────────────────
  nonce:       string;          // Authoritative source: SpaceKit Compute Node on Mainnet.
                                // Client calls GET /v1/nonce/{actor}/{chain} before signing.
                                // Monotonically increasing integer as a decimal string.
                                // The Compute Node guarantees uniqueness across all devices
                                // for the same actor. See §11.1 for the full nonce protocol.
  expiry:      number;          // Unix timestamp (seconds). Intent invalid after this.

  // ── Metadata (off-chain only, excluded from signing) ──
  meta?: {
    created_at:  number;        // Unix timestamp of intent creation in the client.
    composed_by: "user" | "llm" | "agent";  // Origin of the draft intent.
    label?:      string;        // Human-readable label, e.g. "Rotate ETH→BTC"
    source_text?: string;       // The user's original natural language request, if any.
  };
}
```

> **`meta` is excluded from the signing payload.** It is carried by the relay for observability but never verified on-chain. The `composed_by` field records whether the intent was hand-built, LLM-composed, or agent-composed — for analytics and audit, not for trust decisions.

### 3.2 Identifiers

```typescript
// Actor: user address, DID, or raw public key
// Format: "did:key:z6Mk..." | "0x..." | "solana:<base58>"
type ActorId = string;

// Agent: same format as ActorId. Must have an on-chain scope grant from the actor.
type AgentId = string;

// Chain: namespace:chainId
// Supported in v1: ethereum:1, ethereum:11155111(testnet), bsc:56, base:8453
type ChainId = `${string}:${number}`;
```

### 3.3 Constraints

```typescript
interface Constraints {
  // Slippage tolerance as basis points (1 bp = 0.01%)
  // e.g. 50 = 0.5% max adverse price movement
  max_slippage_bps:    number;

  // Maximum gas in the chain's native unit (gwei for EVM chains)
  // Relay rejects intents that would exceed this at current gas prices.
  max_gas_gwei?:       number;

  // Hard expiry. Relay and contract both enforce.
  // Must match the top-level `expiry` field. Duplicated here for constraint clarity.
  expiry_unix:         number;

  // Allowed execution venues. Empty array = any venue.
  // e.g. ["uniswap-v3", "curve", "1inch"]
  allowed_venues:      string[];

  // Maximum notional value of the entire intent in USD.
  // Intent rejected if simulation shows total execution value exceeds this.
  max_notional_usd?:   number;

  // Minimum output amount for the overall intent, in the output asset's base unit.
  // Used as a final backstop in addition to per-action min_amount_out.
  min_total_out?:      string;   // decimal string to avoid float precision issues
}
```

### 3.4 Actions

All `amount` and value fields are decimal strings (e.g. `"1000000"` for 1 USDC with 6 decimals) to avoid floating-point issues across languages.

```typescript
type Action =
  | SwapAction
  | BridgeAction
  | BatchAction
  | ApproveAction
  | TransferAction;

// ── Swap ─────────────────────────────────────────────────────────────────────
interface SwapAction {
  type:           "swap";
  from_asset:     AssetId;        // e.g. "ethereum:1:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48" (USDC)
  to_asset:       AssetId;
  amount_in:      string;         // exact amount in, in from_asset base units
  min_amount_out: string;         // minimum acceptable out, in to_asset base units
  venue_hint?:    string;         // ADVISORY ONLY. See §3.4.1.
}

// ── Bridge ───────────────────────────────────────────────────────────────────
interface BridgeAction {
  type:         "bridge";
  from_chain:   ChainId;
  to_chain:     ChainId;
  asset:        AssetId;          // Asset on source chain
  amount:       string;
  min_received: string;           // Minimum amount to receive on destination chain
  bridge_hint?: string;           // ADVISORY ONLY. Same semantics as venue_hint. See §3.4.1.
}

// ── Batch ────────────────────────────────────────────────────────────────────
// Wraps multiple actions for sequential execution within a single intent.
// Each sub-action is validated independently but submitted as one unit.
interface BatchAction {
  type:     "batch";
  actions:  Exclude<Action, BatchAction>[];  // No nested batches.
}

// ── Approve ──────────────────────────────────────────────────────────────────
// ERC-20 approval. Usually emitted automatically by the contract before a swap.
// Included explicitly when the user wants to pre-approve without executing.
interface ApproveAction {
  type:       "approve";
  asset:      AssetId;
  spender:    string;             // Contract address to approve
  amount:     string;             // Use MaxUint256 string for unlimited approval
}
```

### 3.4.1 `venue_hint` and `bridge_hint` Semantics

**These fields are strictly advisory. They are never binding.**

A `venue_hint` is an expression of preference, not a constraint. The solver (relay matcher or on-chain adapter) is free to route to a different venue if it can achieve better execution — lower slippage, lower fees, or better price — than the hinted venue.

**What advisory means in practice:**

| Scenario | Behaviour |
|---|---|
| Hinted venue available and competitive | Solver routes to hinted venue |
| Hinted venue available but not competitive | Solver routes to better venue |
| Hinted venue unavailable (paused, insufficient liquidity) | Solver routes to best available alternative |
| Hinted venue not in `allowed_venues` list | Solver ignores the hint entirely; `allowed_venues` is binding |
| No `venue_hint` provided | Solver routes to best execution venue |

**`allowed_venues` is the binding counterpart.** If you require a specific venue, do not use `venue_hint` — put the venue in `constraints.allowed_venues`. An intent with `allowed_venues: ["uniswap-v3"]` will fail rather than execute on Curve. An intent with `venue_hint: "uniswap-v3"` and no `allowed_venues` restriction may execute anywhere.

**Do not use `venue_hint` as a security boundary.** It provides no execution guarantee. Applications that require venue exclusivity for regulatory, compliance, or smart contract security reasons must use `allowed_venues`.

**LLM-composed intents:** WebLLM may produce `venue_hint` based on context (e.g. the user mentions preferring Uniswap). This is appropriate. The model must not produce `allowed_venues` restrictions — venue policy is a user decision, not a model inference.



// ── Transfer ─────────────────────────────────────────────────────────────────
interface TransferAction {
  type:       "transfer";
  asset:      AssetId;
  to:         string;             // Destination address
  amount:     string;
}

// Asset identifier: namespace:chainId:contractAddress (or native token symbol)
// e.g. "ethereum:1:0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"  (USDC on mainnet)
// e.g. "ethereum:1:native"                                        (ETH)
type AssetId = string;
```

### 3.5 Canonical Signing Payload

The signature covers a deterministic JSON serialisation of the intent, *excluding* the `meta` field and `intent_id` (which is derived from this serialisation).

```typescript
function canonicalPayload(intent: Omit<Intent, "intent_id" | "meta">): string {
  // Sorted keys, no whitespace, UTF-8 encoded.
  // intent_id = SHA-256(canonicalPayload) as lowercase hex.
  return JSON.stringify(intent, Object.keys(intent).sort());
}
```

Both spacekit-js and the Rust contract implement this function identically. Any deviation breaks signature verification.

---

## 4. Authorization Model

### 4.1 User-signed intents

The simplest case. The user's key signs the canonical intent hash directly.

```
sig = sign(SHA-256(canonicalPayload(intent)), user_private_key)
```

The on-chain contract verifies `sig` against `intent.actor`. No further checks needed.

### 4.2 Agent-signed intents

An agent (automated strategy, LLM cursor operating over multiple turns, third-party bot) may sign intents on behalf of the user. The agent has a key pair; the user has pre-approved it on-chain with explicit scopes.

**On-chain scope grant (Rust struct):**

```rust
pub struct AgentScope {
    pub agent_id:        Address,        // Agent's public key / address
    pub actor_id:        Address,        // Granting user
    pub allowed_assets:  Vec<AssetId>,   // Empty = no asset restriction
    pub allowed_actions: Vec<ActionType>,// e.g. [Swap, Approve]
    pub max_notional_usd:Option<u64>,    // Per-intent notional cap (USD, cents)
    pub max_frequency:   Option<u32>,    // Max intents per hour
    pub expiry:          u64,            // Unix timestamp
    pub policy_hash:     Option<[u8;32]>,// Optional: hash of an off-chain policy doc
}
```

**Verification flow in the on-chain contract:**

1. Verify agent signature over `intent.intent_id`.
2. Load `AgentScope` for `(agent_id, actor_id)`.
3. Check `AgentScope.expiry > now`.
4. For each action in `intent.actions`:
   - Action type is in `allowed_actions`.
   - All assets involved are in `allowed_assets` (or `allowed_assets` is empty).
5. Check `intent.constraints.max_notional_usd <= scope.max_notional_usd` (if scope sets a cap).
6. Check frequency: intents from this agent in the last hour < `max_frequency`.
7. All checks pass → proceed to action execution. Any failure → revert.

**Granting and revoking scope:**

Scope grants are stored in the SpaceKit permission contract on each supported chain. The user calls `grantScope(AgentScope)` or `revokeScope(agent_id)`. Revocation is immediate — the on-chain state is authoritative.

### 4.3 Replay protection

Every intent carries a `nonce` (monotonically increasing, per actor) and an `expiry`. The on-chain contract maintains a `used_nonces` set per actor. An intent is accepted only if:

- `nonce` has not been seen before for this `actor`.
- `expiry > block.timestamp`.

The relay also checks expiry and will reject intents that expire in less than 30 seconds (configurable), to avoid submitting to a chain only to have the contract reject it.

---

## 5. The Role of WebLLM (AI as Composer)

WebLLM is a local, in-browser language model. It has no network access, no keys, and no ability to submit intents. Its only role is to read context and produce a *draft* `Intent` object.

### 5.1 Model Requirements and Capability Tiers

The minimum recommended model is **Mistral Nemo 12B** (Mistral 4.5-class or later). The SDK selects a model tier at initialisation time based on available device memory and GPU capability, detected via WebGPU adapter info.

| Tier | Model | Min VRAM | Context | Use case |
|---|---|---|---|---|
| **Full** | Mistral Large 123B (quantised) | 16 GB | 32k | Desktop, high-end laptops |
| **Standard** | Mistral Small 22B (quantised) | 8 GB | 16k | Mid-range laptops, gaming GPUs |
| **Minimum** | Mistral Nemo 12B (Q4) | 4 GB | 8k | Low-end hardware, integrated graphics |
| **Fallback** | Mistral 7B (Q4) | 2 GB | 4k | Minimum viable. Reduced reasoning quality. |

Below the Fallback tier (< 2 GB available VRAM), LLM composition is **disabled**. spacekit-js falls back to the programmatic intent builder UI. The user is informed that their device does not meet the minimum requirements for AI-assisted composition, and the full programmatic interface is presented instead. This is not an error state — all protocol functionality is available without the LLM.

**Model selection is automatic** based on detected hardware. The user may manually override the tier selection in SDK settings. The application may restrict the minimum tier via SDK configuration.

```typescript
const sk = new SpaceKit({
  // ...
  webllm: {
    min_tier:       "minimum",   // Refuse to load below this tier
    preferred_tier: "standard",  // Target tier if hardware supports it
    model_cache:    "indexeddb", // Where to cache downloaded model weights
    download_on:    "first_use", // "init" | "first_use"
  }
});
```

**Model weights are downloaded once and cached in IndexedDB.** Subsequent loads use the cached weights. The SDK verifies the SHA-256 hash of each weight file against a manifest signed by Mistral AI before loading.

### 5.2 What it reads

```typescript
interface LLMContext {
  user_message:    string;        // Natural language request
  portfolio:       PortfolioSnapshot;
  recent_intents:  IntentSummary[];
  market_data:     MarketSnapshot;
  agent_scopes?:   AgentScopeSummary[];  // Active agent grants, if any
}
```

### 5.3 What it produces

The model outputs a structured JSON object that maps directly to the `Intent` schema. spacekit-js parses and validates this output before doing anything with it.

```typescript
// LLM output format (instructed via system prompt)
interface LLMIntentDraft {
  label:       string;           // Human-readable description of intent
  actions:     Action[];         // The proposed actions
  constraints: Partial<Constraints>;  // Partial — spacekit-js fills defaults
  rationale:   string;           // Model's explanation, shown to the user verbatim
}
```

The model never produces `actor`, `nonce`, `expiry`, or `intent_id`. These are set by spacekit-js after the user approves the draft.

### 5.4 Trust boundary

The model's output is *untrusted input* to spacekit-js. spacekit-js:

1. Validates the draft against the schema. Malformed output is rejected.
2. Checks that proposed assets and venues are on the allowlist for the connected chain.
3. Runs simulation (§6.1). The user sees simulated outcomes, not model claims.
4. Presents the rationale to the user verbatim — no modifications.

The user decides whether to proceed. The model is advisory. The simulation is ground truth.

### 5.5 System prompt constraints

The WebLLM instance is initialised with a system prompt that:

- Restricts output to the `LLMIntentDraft` JSON schema.
- Prohibits producing `actor`, `agent`, `nonce`, `expiry`, or signature-related fields.
- Prohibits producing `constraints.allowed_venues` — venue policy is a user decision. See §3.4.1.
- `venue_hint` may be produced if the user explicitly mentions a venue preference.
- Instructs the model to express uncertainty as a conservative `min_amount_out` or low `max_notional_usd`, not as ambiguous prose.
- Requires `rationale` to explain *why* the proposed action makes sense given the user's context, not just *what* it does.
- All model I/O within the browser is processed locally. No content from the LLM context or output is transmitted to the relay or any external service.

---

## 6. Intent Lifecycle

### 6.1 Client (spacekit-js)

```
User context / natural language request
        │
        ▼
   [WebLLM] ──────▶ LLMIntentDraft (JSON)
                           │
                    spacekit-js validates schema
                           │
                    [WASM VM Simulation]
                     ├─ fetch live quotes (price service)
                     ├─ simulate each action in order
                     ├─ check constraints (slippage, notional)
                     └─ produce SimulationResult
                           │
                    present to user:
                     ├─ SimulationResult (expected outputs, fees, gas est.)
                     └─ LLMIntentDraft.rationale
                           │
                     user approves / rejects / edits
                           │
                     spacekit-js fills:
                     ├─ actor (from connected wallet)
                     ├─ nonce (fetch from chain or local counter)
                     ├─ expiry (now + user-configured window, default 5 min)
                     └─ intent_id = SHA-256(canonicalPayload)
                           │
                     user signs intent_id
                           │
                     SignedIntent → relay
```

**SimulationResult:**

```typescript
interface SimulationResult {
  success:         boolean;
  expected_out:    { asset: AssetId; amount: string }[];
  estimated_gas:   string;        // in chain native units
  estimated_fees:  { venue: string; fee_bps: number }[];
  slippage_check:  "pass" | "warn" | "fail";
  constraint_violations: string[];
  warnings:        string[];
}
```

If `success` is false or `constraint_violations` is non-empty, the UI blocks signing and displays the issues. The user can edit constraints and re-simulate.

### 6.2 Relay / Matcher

The relay is an off-chain service. Its responsibilities in v1 are narrow and well-defined. All client ↔ relay communication occurs over an ML-KEM encrypted channel (see §10).

**Validation (synchronous, before acknowledgement):**

- Schema validation: intent matches current protocol version.
- Expiry: `intent.expiry > now + 30s`.
- Replay: `intent_id` not seen before (relay-side deduplication, separate from on-chain nonce).
- Signature: valid over `intent.intent_id` by `intent.actor` (or `intent.agent` if agent-signed).
- Chain support: `intent.chain` is a supported chain.

Relay returns HTTP 400 with a structured error for any validation failure. No silent drops.

**Routing:**

For v1, routing is simple: route to `intent.chain`. The relay selects the appropriate RPC endpoint and submits the intent to the on-chain contract.

**Optional matching (v1 feature flag, off by default):**

The relay may hold intents briefly (configurable, default 500ms) to check for cross-intent netting opportunities. Example: actor A wants to swap ETH→USDC, actor B wants to swap USDC→ETH at similar prices. The relay can net these internally without touching a DEX, reducing fees and slippage for both.

```typescript
// Added to Constraints for v1.1+
matching_preference?: "immediate" | "match_preferred" | "match_required";
// immediate:        Skip matching window, submit to chain immediately.
// match_preferred:  Wait matching window, fall back to chain if no match (default).
// match_required:   Only execute if matched; reject otherwise.
```

**Relay API (v1):**

```
POST /v1/intent
Body:  SignedIntent
200:   { relay_id: string, status: "accepted" }
400:   { error: string, code: RelayErrorCode }

GET  /v1/intent/:relay_id
200:   IntentStatus

POST /v1/intent/:relay_id/cancel
200:   { cancelled: boolean }
(Only possible if intent has not yet been submitted to chain)
```

**SignedIntent:**

```typescript
interface SignedIntent {
  intent:     Intent;
  signature:  string;     // hex-encoded signature over intent.intent_id
  sig_type:   "ed25519" | "secp256k1";
}
```

---

### 6.2.1 Relay Fallback: Direct-to-Chain Submission

If the relay is unreachable, spacekit-js may submit the signed intent directly to the on-chain contract, **subject to the following constraint:**

**Direct submission is only available for intents whose actions can be fully resolved on-chain by the WASM contract.** Specifically:

| Action | Direct submission | Reason |
|---|---|---|
| `swap` (single-chain) | ✅ Allowed | Fully on-chain via DEX adapter |
| `approve` | ✅ Allowed | Fully on-chain |
| `transfer` | ✅ Allowed | Fully on-chain |
| `batch` (single-chain swap/approve/transfer only) | ✅ Allowed | All sub-actions on-chain |
| `bridge` | ❌ Not allowed | Requires relay coordination for cross-chain state |
| `batch` containing `bridge` | ❌ Not allowed | Relay required for bridge sub-action |
| `match_required` matching preference | ❌ Not allowed | Matching requires relay by definition |

**Behaviour when relay is unreachable:**

```typescript
// spacekit-js fallback logic
async function submitIntent(signed: SignedIntent): Promise<IntentReceipt> {
  try {
    return await relay.submit(signed);
  } catch (RelayUnavailableError) {
    if (!isDirectSubmittable(signed.intent)) {
      throw new Error("RELAY_REQUIRED: This intent contains actions that require " +
                      "the relay (bridge, cross-chain). Direct submission is not available.");
    }
    // Warn the user: relay is unavailable, submitting directly.
    // Direct submission bypasses matching — fees may be higher.
    return await chain.submitDirect(signed);
  }
}
```

The user is shown a warning when falling back to direct submission:

> *"The SpaceKit relay is currently unreachable. Your intent will be submitted directly to the chain. Matching and fee optimisation are unavailable. Estimated fees may be higher."*

The user must explicitly confirm before direct submission proceeds.

**Direct submission does not bypass on-chain verification.** The WASM contract verifies the signature and all constraints identically whether the intent arrives via the relay or directly. The relay provides matching and routing convenience, not security.



### 6.3 On-chain WASM Contract (Rust)

The contract is the final trust boundary. It trusts nothing from the relay except the signed intent payload.

```rust
pub fn execute_intent(
    ctx: Context,
    signed_intent: SignedIntent,
) -> Result<ExecutionReceipt> {

    let intent = &signed_intent.intent;

    // 1. Verify signature
    verify_signature(&intent.intent_id, &signed_intent.signature,
                     &intent.actor, signed_intent.sig_type)?;

    // 2. If agent-signed, verify agent scope
    if let Some(agent_id) = &intent.agent {
        verify_agent_scope(ctx.state, agent_id, &intent.actor, intent)?;
    }

    // 3. Replay protection
    require!(!ctx.state.used_nonces[&intent.actor].contains(&intent.nonce),
             ErrorCode::NonceReplayed);
    require!(intent.expiry > ctx.clock.unix_timestamp,
             ErrorCode::IntentExpired);
    ctx.state.used_nonces[&intent.actor].insert(intent.nonce.clone());

    // 4. Execute actions in order
    let mut receipts: Vec<ActionReceipt> = vec![];
    for action in &intent.actions {
        let receipt = execute_action(ctx, action, &intent.constraints)?;
        receipts.push(receipt);
    }

    // 5. Final constraint check (post-execution)
    verify_output_constraints(&receipts, &intent.constraints)?;

    // 6. Emit event
    emit!(IntentExecuted {
        intent_id:  intent.intent_id.clone(),
        actor:      intent.actor.clone(),
        agent:      intent.agent.clone(),
        receipts:   receipts.clone(),
        timestamp:  ctx.clock.unix_timestamp,
    });

    Ok(ExecutionReceipt { intent_id: intent.intent_id.clone(), receipts })
}
```

**Action adapters** (v1 supported):

| Action | Adapter |
|---|---|
| `swap` | DEX adapter (Uniswap v3 on Ethereum, PancakeSwap on BSC) |
| `bridge` | Bridge adapter (Stargate v2 reference implementation) |
| `batch` | Sequential dispatch to sub-adapters |
| `approve` | ERC-20 approve |
| `transfer` | ERC-20 / native transfer |

Adapters are registered in an adapter registry on-chain. New adapters can be added without upgrading the core contract.

**Execution events** are indexed by the SpaceKit event indexer and surfaced in the spacekit-js UI and any connected analytics.

---

## 7. Error Taxonomy

Consistent error codes across client, relay, and contract allow the UI to display actionable messages.

| Code | Layer | Meaning | User-facing action |
|---|---|---|---|
| `SCHEMA_INVALID` | Client / Relay | Intent does not match schema | Client bug or LLM output parse failure |
| `EXPIRY_EXCEEDED` | Relay / Contract | Intent has expired | Re-compose and sign |
| `NONCE_REPLAYED` | Contract | Nonce already used | Client fetches fresh nonce from Compute Node |
| `NONCE_STALE` | Compute Node | Fetched nonce is already consumed | Retry nonce fetch |
| `SIG_INVALID` | Relay / Contract | Signature verification failed | Re-sign the intent |
| `AGENT_SCOPE_EXCEEDED` | Contract | Agent action exceeds granted scope | User must expand scope or sign directly |
| `AGENT_EXPIRED` | Contract | Agent scope grant has expired | User must re-grant scope |
| `CONSTRAINT_SLIPPAGE` | Client (sim) / Contract | Slippage exceeds `max_slippage_bps` | Widen slippage or retry at better price |
| `CONSTRAINT_GAS` | Relay | Estimated gas exceeds `max_gas_gwei` | Raise gas limit or wait for lower gas |
| `CONSTRAINT_NOTIONAL` | Client (sim) / Contract | Total value exceeds `max_notional_usd` | Reduce size or raise notional cap |
| `VENUE_NOT_ALLOWED` | Contract | Chosen venue not in `allowed_venues` | Update venue list or remove restriction |
| `CHAIN_UNSUPPORTED` | Relay | Target chain not supported | Check supported chain list |
| `SIMULATION_FAILED` | Client | Simulation could not produce a result | Retry; may indicate stale quotes |
| `RELAY_REQUIRED` | Client | Intent requires relay (bridge/match) but relay is unreachable | Wait for relay to recover |
| `RELAY_UNAVAILABLE` | Client | Relay unreachable; direct submission available for eligible intents | Confirm direct submission in UI |
| `QS_HANDSHAKE_FAILED` | Transport | ML-KEM key exchange failed | Retry connection; may indicate MITM attempt |
| `QS_SIG_INVALID` | Transport | ML-DSA signature on relay message failed verification | Discard message; possible tampering |

---

## 8. spacekit-js Integration Points

These are the surfaces spacekit-js exposes to application code.

```typescript
// Initialise the SDK
const sk = new SpaceKit({
  chain:        "ethereum:1",
  rpc:          "https://...",
  relay:        "https://relay.spacekit.io",
  computeNode:  "https://node.spacekit.io",   // SpaceKit Compute Node for nonce management
  priceApi:     "https://prices.spacekit.io",
  webllm?: {
    min_tier:       "minimum",
    preferred_tier: "standard",
    model_cache:    "indexeddb",
    download_on:    "first_use",
  },
  crypto: {
    qs_profile: "mlkem-1024+mldsa-87",  // Quantum-safe profile. See §10.
  },
});

// Compose an intent from natural language (requires webllm)
const draft: LLMIntentDraft = await sk.compose("Should I rotate ETH into BTC?");

// Build an intent programmatically
const intent: Intent = sk.buildIntent({
  actions: [{
    type: "swap", from_asset: ETH, to_asset: BTC,
    amount_in: "1e18", min_amount_out: "...",
  }],
  constraints: {
    max_slippage_bps: 50,
    expiry_unix: Date.now()/1000 + 300,
    allowed_venues: [],
  },
});

// Fetch nonce from Compute Node before signing (automatic in signAndSubmit)
const nonce: string = await sk.getNonce(actorId, "ethereum:1");

// Simulate
const sim: SimulationResult = await sk.simulate(intent);

// Sign and submit (fetches nonce automatically, then signs, then submits)
const receipt: IntentReceipt = await sk.signAndSubmit(intent, signer);

// Track status
const status: IntentStatus = await sk.getStatus(receipt.relay_id);

// Agent scope management
await sk.grantAgentScope(agentId, scope, signer);
await sk.revokeAgentScope(agentId, signer);
const scope: AgentScope = await sk.getAgentScope(agentId, actorId);

// Check direct submission eligibility (for relay fallback UI)
const eligible: boolean = sk.isDirectSubmittable(intent);
```

---

## 9. Compute Node API

The SpaceKit Compute Node is the authoritative service for nonce management and cross-chain state queries. All channels use ML-KEM encrypted transport (§10).

```
# Nonce
GET  /v1/nonce/{actor_id}/{chain_id}
→    { nonce: string, valid_until: number }

# Nonce status (check if a nonce has been consumed on-chain)
GET  /v1/nonce/{actor_id}/{chain_id}/{nonce}
→    { status: "issued" | "consumed" | "expired", consumed_at?: number }

# Actor state (latest nonce sequence, active agent scopes)
GET  /v1/actor/{actor_id}
→    { chains: { [chain_id]: { latest_nonce: string } }, agent_scopes: AgentScope[] }

# Health
GET  /v1/health
→    { status: "ok", block: number, lag_ms: number }
```

All requests to the Compute Node are authenticated by presenting the actor's public key in the `X-SpaceKit-Actor` header and a fresh ML-DSA-87 signed timestamp in `X-SpaceKit-Sig`. The Compute Node verifies the signature before responding.

---

## 10. Quantum-Safe Transport and Cryptography

All network communications in the SpaceKit Intent Protocol — between users, the system, and operators — are protected using NIST-standardised post-quantum cryptography. Classical TLS alone is not acceptable for any channel carrying intent payloads, nonces, or key material.

### 10.1 Algorithm Selection

SpaceKit uses the NIST PQC 2024 final standards exclusively:

| Purpose | Algorithm | Standard | Notes |
|---|---|---|---|
| Key encapsulation (key exchange) | ML-KEM-1024 | FIPS 203 | Replaces ECDH/X25519 for session key establishment |
| Digital signatures | ML-DSA-87 | FIPS 204 | Replaces Ed25519/ECDSA for all relay and Compute Node message signing |
| Hash functions | SHA-3 / SHAKE-256 | FIPS 202 | All intent hashes use SHA3-256 |
| Symmetric encryption | AES-256-GCM | FIPS 197 | Session data after ML-KEM key exchange |

> **Note on Ed25519/secp256k1 for intent signing:** User intent signatures remain Ed25519 or secp256k1 because these are constrained by the key types available in existing Ethereum and DID wallets. Quantum safety for intent signing is a migration concern tracked separately. The quantum-safe requirement applies to all *transport and infrastructure* channels in v1.

### 10.2 Channel Requirements

| Channel | Required Protection |
|---|---|
| Browser ↔ Relay | TLS 1.3 + ML-KEM-1024 hybrid KEM (X25519 + ML-KEM-1024) |
| Browser ↔ Compute Node | TLS 1.3 + ML-KEM-1024 hybrid KEM |
| Relay ↔ Compute Node | mTLS with ML-DSA-87 certificates |
| Relay ↔ Chain RPC | TLS 1.3 minimum; ML-KEM hybrid where RPC provider supports it |
| Operator admin access | ML-DSA-87 signed requests; ML-KEM encrypted channel; no plaintext admin API |
| Compute Node ↔ Compute Node (if clustered) | ML-KEM-1024 + ML-DSA-87 mutual authentication |

**Hybrid KEM rationale:** `X25519 + ML-KEM-1024` means a passive attacker must break *both* the classical and post-quantum components to recover the session key. This protects against "harvest now, decrypt later" attacks where an adversary records ciphertext today and decrypts it when quantum computers are available. It also provides a fallback if a flaw is discovered in ML-KEM before widespread adoption.

### 10.3 Key Management

**Relay and Compute Node keys:**

- Each node generates an ML-DSA-87 identity key pair at initialisation.
- Public keys are published to the SpaceKit key directory (on-chain, Mainnet).
- spacekit-js fetches relay and Compute Node public keys from the on-chain directory at SDK initialisation.
- Key rotation: nodes may rotate keys with a 7-day overlap period. Old keys remain valid for in-flight intents.

**Browser-side (user) keys:**

- ML-KEM ephemeral key pairs are generated per-session in the browser using `SubtleCrypto` (WebCrypto API).
- Ephemeral keys are never persisted. A new key pair is generated on every session.
- The ML-KEM public key is sent to the relay in the initial handshake.
- The relay responds with its ML-DSA-87-signed ML-KEM ciphertext to complete the KEM.

**Operator access:**

- All operator API calls must carry an ML-DSA-87 signature over the request payload + timestamp.
- Operator keys are stored in hardware security modules (HSM). No software-only operator keys in production.

### 10.4 Implementation in spacekit-js

The quantum-safe transport layer is transparent to application code. The SDK handles all handshake, key generation, and encryption internally.

```typescript
// SDK crypto configuration (set at init, not per-request)
interface CryptoConfig {
  qs_profile: "mlkem-768+mldsa-65"   // NIST Security Level 3
            | "mlkem-1024+mldsa-87"; // NIST Security Level 5 (recommended)
  
  // Override for testing only. Never set in production.
  disable_qs_transport?: boolean;
}
```

The `mlkem-1024+mldsa-87` profile is the default and recommended setting. The `mlkem-768+mldsa-65` profile is available for environments where the performance cost of Level 5 is prohibitive (e.g. very low-end mobile). Do not use `disable_qs_transport` outside of local development.

**WebAssembly implementation:** The ML-KEM and ML-DSA primitives are compiled to WASM and included in the spacekit-js bundle. They do not depend on browser native crypto support for the PQC operations. SHA-3 and AES-256-GCM use the WebCrypto API where available, falling back to WASM implementations.

### 10.5 What Quantum-Safe Does NOT Cover in v1

- **Intent signature keys (user/agent Ed25519 or secp256k1):** These are wallet-constrained. A migration path to ML-DSA or SLH-DSA for intent signing is a v2 concern, dependent on wallet ecosystem adoption.
- **On-chain smart contract storage:** Chain state is not encrypted. Intent execution receipts are public. This is by design — on-chain transparency is a feature.
- **WebLLM model weights:** Model weight downloads use standard TLS. The integrity check (SHA-256 over weights) is sufficient for the threat model here.

---

## 11. Resolved Design Decisions

This section records decisions that were open in v0.1 and are now resolved.

### 11.1 Nonce Management: SpaceKit Compute Node

**Decision:** The SpaceKit Compute Node running on Mainnet is the authoritative nonce service.

**Rationale:** Monotonically increasing nonces require a single authoritative source to prevent collision when a user operates from multiple devices simultaneously. Options considered:

- *On-chain nonce counter* — correct but costs gas on every intent, even ones that are rejected before submission.
- *Client-side counter* — breaks with multi-device use; no safe recovery path.
- *Relay-managed* — creates a dependency between intent submission and nonce assignment, complicating the relay fallback path.
- *Compute Node* — purpose-built, off-chain, Mainnet-anchored. The Compute Node periodically checkpoints its nonce state on-chain for auditability without paying gas on every increment.

**Protocol:**

```
GET /v1/nonce/{actor_id}/{chain_id}
→ { nonce: string, valid_until: number }
// valid_until: Unix timestamp. Client must use this nonce before valid_until.
// If valid_until passes before the intent is signed and submitted, fetch a new nonce.

// The Compute Node guarantees:
// - Each (actor_id, chain_id) nonce is issued exactly once.
// - Nonces are monotonically increasing.
// - A nonce is "consumed" when the on-chain contract records it in used_nonces.
// - The Compute Node syncs used_nonce state from the chain every block.
```

The Compute Node exposes this endpoint over an ML-KEM encrypted channel (§10.2). Nonce requests are authenticated by the actor's public key to prevent enumeration.

**spacekit-js** calls this endpoint automatically in `signAndSubmit()` immediately before constructing the signing payload. Application code does not need to manage nonces directly.

### 11.2 `venue_hint` Semantics

**Decision:** `venue_hint` and `bridge_hint` are strictly advisory. See §3.4.1 for full documentation.

**Summary:** Advisory means the solver may ignore the hint for better execution. `allowed_venues` is the binding counterpart. Do not use `venue_hint` as a security or compliance control.

### 11.3 Relay Fallback: Direct-to-Chain Submission

**Decision:** Users may submit intents directly to the on-chain contract when the relay is unavailable, subject to action eligibility. See §6.2.1.

**Summary:** Direct submission is available for single-chain actions (`swap`, `approve`, `transfer`, `batch` of same). It is not available for `bridge` or `match_required` intents. Users are warned and must confirm before direct submission proceeds. On-chain verification is identical regardless of submission path.

### 11.4 WebLLM Model Recommendation

**Decision:** Minimum recommended model is Mistral Nemo 12B (Mistral 4.5-class). See §5.1 for capability tiers.

**Summary:** The SDK selects a tier automatically based on detected VRAM. Below 2 GB VRAM, LLM composition is disabled and the programmatic builder is presented. All protocol functionality remains available without the LLM.

### 11.5 Quantum-Safe Encryption

**Decision:** All network channels use NIST PQC 2024 standards (ML-KEM-1024 + ML-DSA-87 at Level 5). See §10.

**Summary:** This applies to users, system services, and operators. Classical TLS is not acceptable for channels carrying intent payloads or key material. User intent signatures remain Ed25519/secp256k1 in v1 due to wallet ecosystem constraints; this is a tracked migration item for v2.

---

## 12. Out of Scope for v1

The following are explicitly deferred.

- **Decentralised relay network.** v1 relay is a trusted centralised service operated by SpaceKit. Decentralisation is a v2 design problem.
- **Cross-chain atomic intents.** Multi-chain actions within a single intent require atomic cross-chain execution guarantees. v1 supports bridge actions, but atomicity across chains is not guaranteed.
- **On-chain intent registry.** v1 intents are not stored on-chain except as execution event logs. A queryable on-chain registry is a v2 concern.
- **AI model node registration.** AI runs client-side via WebLLM in v1. Network-registered model nodes are out of scope until the client-side pattern is stable and the trust model for remote AI execution is designed separately.
- **Intent composition chaining.** v1 supports `BatchAction` within a single chain only.
- **Partial fills.** v1 intents are all-or-nothing.
- **Quantum-safe intent signing (user keys).** Deferred pending wallet ecosystem adoption of ML-DSA or SLH-DSA.
- **`policy_hash` in AgentScope.** Field is reserved. Policy format and resolution mechanism to be defined before exposing in UI.

---

*End of SpaceKit Intent Protocol v0.2*
