# Intent-Based Payment Architecture

**How RouteKit's Intent Protocol unifies with the SpaceKit Payment System**

## Current State

### Two separate flows exist today:

**1. RouteKit Intent Flow** (`POST /v1/intent`)
- User/LLM composes an `Intent` (actions, constraints, expiry, nonce)
- spacekit-js VM simulates against live state
- User signs (ed25519 / secp256k1 / ML-DSA / SLH-DSA)
- Relay validates schema + signature → `relay_id` + `"accepted"`
- **Gap**: accepted intents are not yet forwarded to the compute node for execution

**2. RouteKit Vault Charge Flow** (`POST /v1/charge`)
- EIP-191 signed charge message (user, amountAUsd, agentId, nonce)
- Relay verifies signature, replays on-chain via `SpaceKitMultiAssetVault.charge()`
- Returns tx_hash
- **Gap**: charge is decoupled from intent execution — no atomic "pay and execute"

**3. spacekit-payments Flow** (compute node `POST /v1/payments/*`)
- x402 (USDC on Base), aUSD vault credits, native ASTRA
- `FeeRouter` converts receipts → ASTRA VM credits
- **Gap**: no intent awareness — charges are standalone HTTP calls

### The Missing Link

All three flows exist in isolation. A user who wants to execute a paid contract today must:
1. Fund their aUSD vault (website → on-chain deposit)
2. Separately charge their vault (routekit `/v1/charge`)
3. Separately submit an intent (routekit `/v1/intent`)
4. Separately execute a contract (compute node API)

## Proposed: Intent-Based Payment System

The `Intent` type already has the right primitives:
- `constraints.max_notional_usd` — willingness to pay
- `actions` — what to execute
- `nonce` + `expiry` — replay protection and time-bounding
- `actor` — who pays (DID or address)
- `agent` — optional delegation
- Multi-sig support (ed25519, secp256k1, post-quantum)

### New Action Type: `ExecuteContractAction`

```typescript
interface ExecuteContractAction {
  type:          "execute_contract";
  contract_id:   string;          // DID or address of the WASM contract
  input:         string;          // hex-encoded input bytes
  value_astra?:  string;          // native ASTRA to attach (msg_value)
  max_fee_usdc?: string;          // max fee the user will pay via x402/aUSD
  max_fee_astra?: string;         // max fee in native ASTRA
}
```

### New Action Type: `VaultChargeAction`

```typescript
interface VaultChargeAction {
  type:           "vault_charge";
  amount_ausd:    string;         // 18-decimal aUSD wei
  beneficiary:    string;         // DID of the contract/service
}
```

### Unified Flow

```
Browser (spacekit-js)                RouteKit Relay              Compute Node
     │                                    │                          │
     │  1. LLM composes draft intent      │                          │
     │     with ExecuteContractAction      │                          │
     │     + VaultChargeAction             │                          │
     │                                    │                          │
     │  2. VM simulates: estimates gas,   │                          │
     │     resolves fee, shows user       │                          │
     │     "Execute Foo for 0.02 aUSD"    │                          │
     │                                    │                          │
     │  3. User signs intent              │                          │
     │                                    │                          │
     │── POST /v1/intent ────────────────▶│                          │
     │                                    │                          │
     │                                    │  4. Relay validates:     │
     │                                    │     schema, sig, expiry  │
     │                                    │                          │
     │                                    │  5. Route by chain:      │
     │                                    │     "spacekit:mainnet"   │
     │                                    │     → compute node       │
     │                                    │                          │
     │                                    │── POST /v1/execute ─────▶│
     │                                    │   (signed intent)        │
     │                                    │                          │
     │                                    │                     6. Compute node:
     │                                    │                        a. Verify sig
     │                                    │                        b. Check nonce
     │                                    │                        c. Process VaultChargeAction
     │                                    │                           → AusdVault.process_charge()
     │                                    │                           → FeeRouter.process_payment()
     │                                    │                        d. Execute contract WASM
     │                                    │                           with msg_value from intent
     │                                    │                        e. Emit events + receipt
     │                                    │                          │
     │                                    │◀── result + receipt ─────│
     │◀── result ────────────────────────│                          │
```

### Why This Is Better

| Current | Intent-Based |
|---------|-------------|
| 3 separate API calls (charge + intent + execute) | 1 signed intent, atomic execution |
| Payment and execution are decoupled — race conditions possible | Payment is a constraint within the intent — atomic |
| User must manually coordinate vault balance | Fee constraint in intent, auto-charged at execution |
| No simulation of total cost | spacekit-js simulates fee + execution before signing |
| Agent delegation requires separate auth for charge vs execute | Single intent signature covers both payment and execution |

### What Changes in Each Crate

| Crate | Change |
|-------|--------|
| **routekit** | Add `"spacekit:mainnet"` chain routing → forward signed intents to compute node. Add `ExecuteContractAction` and `VaultChargeAction` to action dispatch. |
| **spacekit-payments** | Add `IntentPaymentProcessor` that extracts payment actions from an `Intent`, processes them through `FeeRouter`, and returns a `Credit` for the VM. |
| **spacekit-compute-node** | Add `POST /v1/execute` endpoint that accepts a `SignedIntent`, verifies signature (reuse routekit's `verify_signature`), processes payment actions, and executes the contract. |
| **spacekit-js** | Add intent builder helpers for `ExecuteContractAction` + fee estimation in the VM simulation step. |
| **spacekit-contract-sdk** | No changes needed — contracts already use `msg_value()` and `require_payment()`. |

### Backward Compatibility

- Existing `POST /v1/charge` continues to work for standalone vault charges
- Existing `POST /v1/payments/*` endpoints remain for direct x402/aUSD operations
- The intent-based flow is additive — new `chain: "spacekit:mainnet"` routing

### Implementation Status

All five priorities are **implemented**:

1. **`chain: "spacekit:mainnet"` routing in routekit** — `routekit/src/api.rs` forwards `spacekit:*` intents to the compute node via `SPACEKIT_COMPUTE_URL`. Non-spacekit chains return `relay_id` as before.
2. **`POST /v1/execute` in compute node** — `spacekit-compute-node/src/bin/standalone.rs` accepts raw `SignedIntent` JSON, parses typed `IntentAction`s, processes payments atomically, and returns execution results.
3. **`IntentPaymentProcessor` in spacekit-payments** — `spacekit-payments/src/intent.rs` defines `ExecuteContractAction`, `VaultChargeAction`, `TransferAction`, `IntentPaymentPlan`, and the processor that orchestrates vault charges → fee routing → constraint validation.
4. **Intent builder in spacekit-js** — `spacekit-js/src/intent_builder.ts` provides `IntentBuilder` (fluent API), `estimateIntentFees()`, and `buildExecuteContractIntent()` convenience helper.
5. **Intent-wrapped vault charges** — `POST /v1/charge-intent` in routekit accepts a `SignedIntent` with `vault_charge` actions, validates the signature, and either forwards to compute node (spacekit chains) or returns accepted status.

### Security Considerations

- **Atomic payment + execution**: The compute node processes payment and contract execution in a single transaction. If execution fails, the charge is reverted.
- **Nonce management**: The compute node is already the authoritative nonce source (per the spec §3.1). Intent nonce and vault charge nonce can be unified.
- **Agent scope**: An agent's `ExecuteContractAction` is bounded by the user's signed constraints (`max_fee_usdc`, `max_notional_usd`). The compute node enforces these before execution.
- **Post-quantum signatures**: All five sig types (ed25519, secp256k1, mldsa65, slh_dsa_sha2_128s, slh_dsa_sha2_192s) are already verified by routekit's `verify_signature`. The compute node reuses the same verification.
