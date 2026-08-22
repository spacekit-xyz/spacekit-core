# AstraRewards Contract Specification

**Status:** Pre-implementation specification
**Version:** 1.0
**Owner:** SWTCH Labs
**Date:** 2026
**Type:** SKCL contract (Rust source compiled to WASM, deployed on SpaceKit network)
**Implementation file:** `AstraRewards.rs`

This document specifies the AstraRewards contract — the protocol-level contract that tracks per-DID ASTRA balances, accepts credit instructions from the protocol's reward accumulator, enforces the 2B hard cap, and processes operator withdrawals to other DIDs.

## 1. Purpose and design properties

AstraRewards is the source of truth for ASTRA balances and emissions on the SpaceKit network. It performs three jobs:

**Receive credits from the reward accumulator.** The protocol-level reward accumulator (a protocol function, not a smart contract) computes per-operator ASTRA rewards from structured logs of service events and instructs AstraRewards to credit operator DID balances accordingly.

**Track per-DID balances.** Each DID has a balance of accumulated ASTRA (earned but not yet transferred elsewhere). Balance increases via credits; decreases via withdrawals.

**Process withdrawals.** Operators can transfer their accumulated balance to other DIDs (typically their own SpaceKit-native receiving addresses, or to other parties as gifts/payments).

**Design properties:**

- **Per-DID accounting.** Balances are keyed by DID hash. Withdrawal authorization is by DID signature.
- **Atomic operations.** Credit and withdrawal each happen in a single transaction; no intermediate states.
- **Cap enforcement.** Total emitted ASTRA cannot exceed 2,000,000,000 × 10^18 (with 18 decimals). The contract is structurally incapable of minting above this.
- **Read-open.** Anyone can query any DID's balance, withdrawal history, and the total emitted ASTRA.
- **Protocol-trusted credits.** Only the protocol (via consensus-validated credit instructions) can credit balances. No arbitrary caller can mint ASTRA.

## 2. Storage layout

The contract maintains the following persistent state:

**`total_emitted: u128`**
Running total of all ASTRA ever credited via this contract. Initialized to genesis treasury allocation (350,000,000 × 10^18). Updated atomically with every credit operation. Cannot exceed `2_000_000_000 * 10^18`.

**`balances: map<did_hash, u128>`**
Per-DID balance, in wei-ASTRA (18 decimals). Indexed by `[u8; 32]` hash of the DID string.

**`withdrawal_count: map<did_hash, u64>`**
Counter of withdrawals per DID. Incremented on each withdrawal. Used for event sequencing and indexer support.

**`total_withdrawn: map<did_hash, u128>`**
Cumulative ASTRA withdrawn from this DID's balance over its lifetime. Useful for dApp display ("Total earnings vs current balance").

**`is_initialized: bool`**
One-time initialization flag. Set to true after genesis allocation; prevents re-initialization.

**`admin: did_hash`**
The protocol's admin DID. Only callable by the protocol-level reward accumulator. Set at deployment, can be rotated via admin operation.

## 3. Opcodes

The contract dispatches operations via a single-byte opcode in the first byte of the input. The opcodes are:

| Op | Opcode | Caller | Description |
|----|--------|--------|-------------|
| INIT | 0x01 | Genesis only | Initialize contract with treasury allocation |
| CREDIT | 0x10 | Reward accumulator only | Credit a DID's balance with earned ASTRA |
| WITHDRAW | 0x20 | Operator DID | Transfer balance to another DID |
| GET_BALANCE | 0x30 | Anyone | Read a DID's current balance |
| GET_WITHDRAWN | 0x31 | Anyone | Read a DID's lifetime withdrawn total |
| GET_TOTAL_EMITTED | 0x32 | Anyone | Read the network's total ever-emitted ASTRA |
| GET_REMAINING_CAP | 0x33 | Anyone | Read the remaining headroom under the 2B cap |
| GET_WITHDRAWAL_COUNT | 0x34 | Anyone | Read withdrawal count for a DID |
| ROTATE_ADMIN | 0xF0 | Current admin | Change the admin DID (governance) |

The opcode ranges:
- 0x00-0x0F: Lifecycle operations (init only)
- 0x10-0x1F: Protocol-only operations (credits)
- 0x20-0x2F: Operator-callable operations (withdrawals)
- 0x30-0x3F: Read-only operations (queries)
- 0xF0-0xFF: Admin operations

### 3.1 INIT (0x01)

**Caller:** Deployer DID, callable only once at protocol genesis.
**Payload:** 
- 32 bytes: treasury_did_hash (DID hash receiving the genesis treasury allocation)

**Logic:**
1. Check `is_initialized` is false. If true, fail.
2. Credit `treasury_did_hash` with 350,000,000 × 10^18 ASTRA.
3. Set `total_emitted` to 350,000,000 × 10^18.
4. Set `is_initialized` to true.
5. Set `admin` to deployer DID.
6. Emit `astra_rewards.initialized` event.

**Returns:** Empty bytes on success.

### 3.2 CREDIT (0x10)

**Caller:** Protocol reward accumulator (admin DID).
**Payload:** 
- 32 bytes: recipient_did_hash
- 16 bytes: amount (u128, little-endian)
- 32 bytes: log_event_hash (the on-chain content hash of the service event being rewarded — for audit trail)

**Logic:**
1. Verify caller's DID matches `admin`.
2. Verify `total_emitted + amount <= 2_000_000_000 * 10^18`. If exceeded, fail with `CapExceeded`.
3. Increment `balances[recipient_did_hash]` by `amount`.
4. Increment `total_emitted` by `amount`.
5. Emit `astra_rewards.credit` event with full payload (DID, amount, log event hash, new balance).

**Returns:** New balance for the recipient DID (16 bytes).

**Error cases:**
- `Unauthorized`: caller is not the admin DID
- `CapExceeded`: credit would push `total_emitted` over 2B
- `InvalidPayload`: payload is malformed

### 3.3 WITHDRAW (0x20)

**Caller:** Operator DID (the DID whose balance is being withdrawn).
**Payload:**
- 32 bytes: recipient_did_hash (the DID receiving the transferred ASTRA)
- 16 bytes: amount (u128, little-endian)

**Logic:**
1. Get caller's DID hash from the contract context.
2. Verify `balances[caller_did_hash] >= amount`. If insufficient, fail with `InsufficientBalance`.
3. Decrement `balances[caller_did_hash]` by `amount`.
4. Increment `balances[recipient_did_hash]` by `amount`.
5. Increment `withdrawal_count[caller_did_hash]` by 1.
6. Increment `total_withdrawn[caller_did_hash]` by `amount`.
7. Emit `astra_rewards.withdraw` event with caller DID, recipient DID, amount, withdrawal_count.

**Returns:** New balance for the caller DID (16 bytes).

**Notes:**
- A DID can withdraw to itself (effectively a no-op but produces audit trail).
- A DID can transfer to any other DID; the receiving DID's balance increases accordingly.
- No minimum amount enforced; even tiny amounts can be withdrawn (gas cost is the natural floor).
- `total_emitted` is NOT decremented on withdrawal — withdrawal transfers between DID balances, doesn't burn ASTRA.

**Error cases:**
- `InsufficientBalance`: balance < amount
- `InvalidPayload`: payload malformed
- `InvalidRecipient`: recipient DID hash is zero or self (self-withdraw allowed but warned)

### 3.4 GET_BALANCE (0x30)

**Caller:** Anyone.
**Payload:** 32 bytes (did_hash).
**Returns:** 16 bytes (current balance as u128 LE).

### 3.5 GET_WITHDRAWN (0x31)

**Caller:** Anyone.
**Payload:** 32 bytes (did_hash).
**Returns:** 16 bytes (lifetime withdrawn total as u128 LE).

### 3.6 GET_TOTAL_EMITTED (0x32)

**Caller:** Anyone.
**Payload:** Empty.
**Returns:** 16 bytes (total_emitted as u128 LE).

### 3.7 GET_REMAINING_CAP (0x33)

**Caller:** Anyone.
**Payload:** Empty.
**Returns:** 16 bytes (`2_000_000_000 * 10^18 - total_emitted` as u128 LE).

### 3.8 GET_WITHDRAWAL_COUNT (0x34)

**Caller:** Anyone.
**Payload:** 32 bytes (did_hash).
**Returns:** 8 bytes (withdrawal count as u64 LE).

### 3.9 ROTATE_ADMIN (0xF0)

**Caller:** Current admin.
**Payload:** 32 bytes (new admin DID hash).
**Returns:** Empty bytes.

**Logic:**
1. Verify caller's DID matches current `admin`.
2. Update `admin` to new DID hash.
3. Emit `astra_rewards.admin_rotated` event.

**Note:** This operation is reserved for governance — the admin is typically the protocol reward accumulator's authorized DID, and rotation is rare. The operation exists for upgrade paths or in case the accumulator's authorization changes.

## 4. Events

The contract emits the following events for indexer support and audit trails:

**`astra_rewards.initialized`**
Emitted at genesis after INIT.
Payload: `treasury_did_hash (32 bytes) + amount (16 bytes)`.

**`astra_rewards.credit`**
Emitted on each successful CREDIT operation.
Payload: `recipient_did_hash (32 bytes) + amount (16 bytes) + log_event_hash (32 bytes) + new_balance (16 bytes) + total_emitted (16 bytes)`.

**`astra_rewards.withdraw`**
Emitted on each successful WITHDRAW operation.
Payload: `from_did_hash (32 bytes) + to_did_hash (32 bytes) + amount (16 bytes) + new_from_balance (16 bytes) + withdrawal_count (8 bytes)`.

**`astra_rewards.cap_reached`**
Emitted if a credit attempt would have pushed `total_emitted` over the cap.
Payload: `attempted_amount (16 bytes) + remaining_cap (16 bytes)`.

**`astra_rewards.admin_rotated`**
Emitted on admin rotation.
Payload: `old_admin (32 bytes) + new_admin (32 bytes)`.

## 5. Integration with the protocol reward accumulator

The protocol reward accumulator is a protocol-level function (not a smart contract) that:

1. Reads structured service log events from each newly-finalized block.
2. For each event, classifies it by service category and computes the operator's earned ASTRA using the current epoch's emission rate.
3. Submits a CREDIT instruction to the AstraRewards contract for each event (or batches them per epoch).

The accumulator is "trusted" by the contract because:
- All validators run the same accumulator code as part of consensus execution.
- All validators compute the same rewards from the same logs.
- Disagreement between validators on credit amounts means consensus failure, not contract bypass.

In practice, the accumulator's credit instructions are part of consensus-included transactions executed atomically as block state changes. The contract sees them as ordinary calls from the admin DID, but they are inseparable from the consensus that produced the underlying log events.

## 6. dApp/UI integration

The contract's read interface supports dApp queries for operator dashboards, leaderboards, and audit trails:

**Display an operator's current balance and history:**

```
GET_BALANCE(did_hash) → current balance
GET_WITHDRAWN(did_hash) → lifetime withdrawn
GET_WITHDRAWAL_COUNT(did_hash) → number of withdrawals
```

Combined dashboard: `balance + total_withdrawn = total_ever_earned`. Withdrawal_count gives history depth.

**Display network state:**

```
GET_TOTAL_EMITTED() → total ASTRA ever credited
GET_REMAINING_CAP() → remaining headroom under 2B cap
```

These let a dApp show "We've emitted X% of the cap" and "Y ASTRA remaining."

**Walk through individual credits and withdrawals:**

The dApp can query the chain's event log (via the indexer or directly) for all `astra_rewards.credit` and `astra_rewards.withdraw` events. Filtering by DID provides per-operator earning history. Filtering by service event hash provides per-event reward trail.

## 7. Audit trail

Every credit operation references the `log_event_hash` of the underlying service event. This creates an audit trail:

```
Service event happens → spacekit-log records it → log content hash computed → 
log committed in block → reward accumulator reads log → credit submitted → 
log_event_hash recorded in credit event
```

A third-party auditor can:

1. Take any credit event from AstraRewards.
2. Get the log_event_hash from the event.
3. Find the service event with that content hash in the chain's log records.
4. Verify the service was actually provided by checking the consensus block where the log was committed.
5. Verify the credit amount matches the protocol's emission schedule for that service category.

This is the verifiability claim: every ASTRA emitted can be traced back to a specific service event on the chain.

## 8. Error handling

The contract uses the SDK's `ContractError` enum:

```rust
ContractError::Unauthorized      // caller not admin (for credit) or not balance holder (for withdraw)
ContractError::InvalidInput      // malformed payload
ContractError::CapExceeded       // credit would push total_emitted over 2B
ContractError::InsufficientBalance  // withdraw amount > balance
ContractError::InvalidRecipient  // zero address or other invalid recipient
ContractError::AlreadyInitialized // INIT called after initialization
ContractError::NotInitialized    // operations called before INIT
```

All errors result in transaction revert. No partial state changes.

## 9. Gas costs

Approximate gas costs for each operation (estimates, to be measured on testnet):

| Operation | Gas estimate | Notes |
|-----------|--------------|-------|
| INIT | 80,000 | One-time setup |
| CREDIT | 35,000 | Per credit; called by accumulator |
| WITHDRAW | 50,000 | Per withdrawal; balance + counter updates |
| GET_BALANCE | 5,000 | Read-only |
| GET_TOTAL_EMITTED | 3,000 | Read-only single u128 read |
| ROTATE_ADMIN | 30,000 | Admin update |

The protocol absorbs CREDIT gas costs (since credits happen as part of consensus). Operator-initiated WITHDRAW pays gas in ASTRA from their own balance (the gas cost reduces the effective withdrawal amount slightly).

## 10. Security considerations

**Cap enforcement is structural.** No code path exists that mints ASTRA beyond the cap. The check is in CREDIT; failure to check would be a coding bug, not a design flaw.

**Admin DID compromise.** If the admin DID is compromised, an attacker could credit arbitrary amounts (up to cap). Mitigation: admin DID is held in a multi-sig wallet (3-of-5 required for any operation), and the credit operations are constrained to events visible on-chain (an off-chain attacker cannot fabricate fake service events because validators won't include credit instructions for non-existent service events).

**Race conditions on balance.** Credit and withdraw operations on the same DID's balance must be atomic. The contract uses single-key reads/writes within each operation; concurrent transactions are serialized by the underlying consensus.

**Integer overflow.** All amounts use u128 with checked arithmetic. Overflow attempts revert the transaction.

**Replay attacks.** Each credit includes a `log_event_hash`. While the contract doesn't enforce uniqueness (multiple credits for different events can share zero history), the reward accumulator guarantees that the same service event is not double-credited (the accumulator tracks which events have been credited).

## 11. Upgrade path

The contract is designed to be non-upgradeable. Once deployed, its behavior cannot be changed except by:

1. Deploying a new version of the contract.
2. On-chain governance proposal to migrate balances from the old contract to the new.
3. Migration transactions executed by the admin DID (with appropriate authorization).

This is intentional. Upgradeable token contracts have repeatedly been exploited. The non-upgradeability makes auditing simpler and removes the attack surface of admin upgrades.

If a critical bug is found, the migration path is:

1. Deploy fixed contract.
2. Pause credit operations (via admin temporarily blocking accumulator submissions).
3. Migrate balances via governance-approved migration transactions.
4. Resume credit operations on the new contract.

## 12. References

- ASTRA Economic Model Decision Memo (internal)
- SpaceKit Tokenomics v2.0
- ASTRA Emission Schedule (Document E)
- Service Reward Accumulator Integration Specification (Document G)
- AstraRewards.rs (the implementation, Document H)

## 13. Contact

For questions on the contract specification:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
