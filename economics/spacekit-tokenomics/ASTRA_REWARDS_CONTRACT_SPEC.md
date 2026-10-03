# AstraRewards Contract Specification

> **Status (October 2026): implemented natively by the node, not as a WASM contract.** The rewards "contract" is the system address `0x…0003`. The node executes its calls itself (`spacekit-compute-node` `native_rewards.rs`). The Service Reward Accumulator places them at the start of a block, and every importer re-derives and checks them. There is no contract ledger: **INIT** mints the 350M treasury allocation to the treasury contract `0x…0004`, **CREDIT** mints straight into the recipient address's native balance, **CREDIT_LOCKED** records a locked amount that vests into that balance, and **END_POA** stops locking. The 2B cap is enforced on `rewards.total_emitted`. Holders spend rewards with ordinary transfers. The withdrawal, per-DID query and admin operations below describe the earlier WASM design and are not part of the native implementation. See [`ASTRA_LEDGER.md`](../../infra/spacekit-compute-node/ASTRA_LEDGER.md).

**Status:** Implemented natively by the node (see above); originally a pre-implementation specification
**Version:** 1.0
**Owner:** SWTCH Labs
**Date:** 2026
**Type:** Node-native system address `0x…0003` (originally specified as an SKCL contract compiled to WASM)
**Implementation file:** `spacekit-compute-node` `native_rewards.rs` (originally `AstraRewards.rs`)

This document specifies AstraRewards — the protocol-level rewards calls that accept credit instructions from the protocol's reward accumulator, mint ASTRA into native account balances, and enforce the 2B hard cap.

## 1. Purpose and design properties

AstraRewards is the only path by which ASTRA is created on the SpaceKit network. Balances themselves are native account balances (`SwtchvmAccount::balance`), not entries in AstraRewards. It performs three jobs:

**Receive credits from the reward accumulator.** The protocol-level reward accumulator (a protocol function, not a smart contract) computes per-operator ASTRA rewards from structured logs of service events and places the matching credit calls in the block.

**Mint into native balances.** CREDIT mints the reward straight into the recipient address's native balance. CREDIT_LOCKED (during PoA) records a locked amount that is released into that balance as it vests.

**Enforce the cap.** Every mint, locked amounts included, counts against the 2B cap.

Withdrawals are not needed: rewarded ASTRA is already in the holder's native balance and is spent with ordinary transfers.

**Design properties:**

- **Per-address accounting.** Rewards are keyed by 20-byte address (a DID `did:spacekit:<address>` designates its address).
- **Atomic operations.** Each credit happens in full or not at all; no intermediate states.
- **Cap enforcement.** Total emitted ASTRA cannot exceed 2,000,000,000 × 10^18 (with 18 decimals). The node's rewards code is structurally incapable of minting above this.
- **Read-open.** Anyone can query any address's balance and locked amount (`GET /v1/balance/{address}`).
- **Protocol-trusted credits.** Only the protocol (via consensus-validated system calls at the start of a block) can mint. No arbitrary caller can mint ASTRA.

## 2. Storage layout

The node keeps the following rewards state, covered by the block state root:

**`total_emitted: u128`** (`rewards.total_emitted`)
Running total of all ASTRA ever minted by the rewards calls, locked amounts included. Initialized to genesis treasury allocation (350,000,000 × 10^18). Updated atomically with every credit operation. Cannot exceed `2_000_000_000 * 10^18`.

**Locked amounts** (per address)
PoA-phase credits recorded by CREDIT_LOCKED and not yet released. They vest from the genesis block time and are released into the native balance block by block.

**`is_initialized: bool`**
One-time initialization flag. Set to true after genesis allocation; prevents re-initialization.

Balances are **not** kept here. Minted ASTRA goes into the recipient's native account balance (`SwtchvmAccount::balance`, in wei). The WASM design's `balances`, `withdrawal_count`, `total_withdrawn` and `admin` fields do not exist in the native implementation: there is no admin key, because credits are system calls that every importer re-derives and checks.

## 3. Opcodes

Operations are dispatched via a single-byte opcode in the first byte of the input. The opcodes are:

| Op | Opcode | Caller | Description | Native implementation |
|----|--------|--------|-------------|-----------------------|
| INIT | 0x01 | Genesis only | Mint the treasury allocation to `0x…0004`; start PoA | Yes |
| CREDIT | 0x10 | Reward accumulator only | Mint earned ASTRA into the recipient's native balance | Yes |
| CREDIT_LOCKED | 0x11 | Reward accumulator only | During PoA: record a locked credit that vests into the recipient's balance | Yes |
| END_POA | 0x41 | Reward accumulator only | Stop locking new credits after the PoA lift | Yes |
| WITHDRAW | 0x20 | Operator DID | Transfer balance to another DID | No: use an ordinary transfer |
| GET_BALANCE | 0x30 | Anyone | Read a DID's current balance | No: use `GET /v1/balance/{address}` |
| GET_WITHDRAWN | 0x31 | Anyone | Read a DID's lifetime withdrawn total | No |
| GET_TOTAL_EMITTED | 0x32 | Anyone | Read the network's total ever-emitted ASTRA | No (WASM design) |
| GET_REMAINING_CAP | 0x33 | Anyone | Read the remaining headroom under the 2B cap | No (WASM design) |
| GET_WITHDRAWAL_COUNT | 0x34 | Anyone | Read withdrawal count for a DID | No |
| ROTATE_ADMIN | 0xF0 | Current admin | Change the admin DID (governance) | No: there is no admin key |

The opcode ranges:
- 0x00-0x0F: Lifecycle operations (init only)
- 0x10-0x1F: Protocol-only operations (credits)
- 0x20-0x2F: Operator-callable operations (withdrawals; WASM design only)
- 0x30-0x3F: Read-only operations (queries; WASM design only)
- 0x41: Phase operation (END_POA)
- 0xF0-0xFF: Admin operations (WASM design only)

### 3.1 INIT (0x01)

**Caller:** Protocol system call in the first block, once only.
**Recipient:** the treasury contract `0x…0004` (its signers and threshold come from the PoA genesis `treasury` section).

**Logic:**
1. Check `is_initialized` is false. If true, fail.
2. Mint 350,000,000 × 10^18 ASTRA into the treasury contract's native balance.
3. Set `total_emitted` to 350,000,000 × 10^18.
4. Set `is_initialized` to true.
5. Start proof of authority.
6. Emit `astra_rewards.initialized` event.

**Returns:** Empty bytes on success.

### 3.2 CREDIT (0x10)

**Caller:** Protocol reward accumulator, as a system call at the start of a block.
**Payload:** 
- recipient: the recipient's 20-byte address. A recipient key that is not an address is refused, and nothing is minted.
- 16 bytes: amount (u128, little-endian)
- 32 bytes: log_event_hash (the on-chain content hash of the service event being rewarded — for audit trail)

**Logic:**
1. Verify the call is a protocol system call that matches what the importer re-derives for this block.
2. Verify `total_emitted + amount <= 2_000_000_000 * 10^18`. If exceeded, fail with `CapExceeded`.
3. Mint `amount` into the recipient address's native balance.
4. Increment `total_emitted` by `amount`.
5. Emit `astra_rewards.credit` event with full payload (recipient, amount, log event hash, new balance).

**Returns:** New balance for the recipient (16 bytes).

**Error cases:**
- `Unauthorized`: not a valid protocol system call
- `CapExceeded`: credit would push `total_emitted` over 2B
- `InvalidPayload`: payload is malformed, or the recipient is not an address

### 3.2a CREDIT_LOCKED (0x11)

**Caller:** Protocol reward accumulator, as a system call, during proof of authority, for credits to authorities and affiliated operators.
**Payload:** as CREDIT.

**Logic:** As CREDIT, except the amount is recorded as locked instead of added to the balance. It counts against the cap when credited. It vests from the genesis block time: nothing before 365 days, then linearly until 1,095 days. Every block releases what has vested into the address's native balance. Locked ASTRA counts toward validator holdings but cannot be transferred or spent until released.

### 3.2b END_POA (0x41)

**Caller:** Protocol system call, after a passed `lift_poa` proposal.
**Logic:** Stops locking. Later credits use CREDIT. Amounts already locked keep vesting on their schedule.

### 3.3 WITHDRAW (0x20)

> **Not in the native implementation.** Rewards are minted into the native balance, so there is nothing to withdraw. Holders spend with ordinary transfers. Sections 3.3–3.9 describe the earlier WASM design and are kept for reference.

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

The WASM design specified the following events for indexer support and audit trails. The `withdraw` and `admin_rotated` events have no counterpart in the native implementation:

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
3. Places the CREDIT (or CREDIT_LOCKED) system calls to `0x…0003` at the start of a block, one per recipient when an epoch settles.

The accumulator is "trusted" because:
- All validators run the same accumulator code as part of consensus execution.
- All validators compute the same rewards from the same logs.
- Disagreement between validators on credit amounts means consensus failure, not contract bypass.

In practice, the accumulator's credit instructions are system transactions at the start of a block, executed natively by the node as block state changes. Every importer re-derives them from the chain and rejects a block whose leading system transactions differ, so they are inseparable from the consensus that produced the underlying log events.

## 6. dApp/UI integration

Rewards land in native balances, so dApps read them like any other balance:

**Display an operator's current balance:**

```
GET /v1/balance/{address} → balance_wei, locked_wei, nonce
```

Earnings history comes from the credit events, not from a contract counter. The WASM design's `GET_WITHDRAWN`, `GET_WITHDRAWAL_COUNT`, `GET_TOTAL_EMITTED` and `GET_REMAINING_CAP` queries are not part of the native implementation.

**Walk through individual credits:**

The dApp can query the chain's event log (via the indexer or directly) for all `astra_rewards.credit` events. Filtering by DID provides per-operator earning history. Filtering by service event hash provides per-event reward trail.

## 7. Audit trail

Every credit operation references the `log_event_hash` of the underlying service event. This creates an audit trail:

```
Service event happens → spacekit-log records it → log content hash computed → 
log committed in block → reward accumulator reads log → credit submitted → 
log_event_hash recorded in credit event
```

A third-party auditor can:

1. Take any rewards credit event.
2. Get the log_event_hash from the event.
3. Find the service event with that content hash in the chain's log records.
4. Verify the service was actually provided by checking the consensus block where the log was committed.
5. Verify the credit amount matches the protocol's emission schedule for that service category.

This is the verifiability claim: every ASTRA emitted can be traced back to a specific service event on the chain.

## 8. Error handling

The WASM design used the SDK's `ContractError` enum (the native implementation applies the same cap, payload and initialization checks to the rewards system calls; `InsufficientBalance` applied only to the WASM-era withdrawal):

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

Approximate gas costs for each operation in the WASM design (estimates). In the native implementation, rewards calls are system transactions executed by the node; WITHDRAW, GET_* and ROTATE_ADMIN do not exist (an ordinary transfer costs 21,000 gas):

| Operation | Gas estimate | Notes |
|-----------|--------------|-------|
| INIT | 80,000 | One-time setup |
| CREDIT | 35,000 | Per credit; called by accumulator |
| WITHDRAW | 50,000 | Per withdrawal; balance + counter updates |
| GET_BALANCE | 5,000 | Read-only |
| GET_TOTAL_EMITTED | 3,000 | Read-only single u128 read |
| ROTATE_ADMIN | 30,000 | Admin update |

The protocol absorbs CREDIT gas costs (since credits happen as part of consensus). Operators spend their rewards with ordinary transfers and pay gas for those like any other transaction.

## 10. Security considerations

**Cap enforcement is structural.** No code path exists that mints ASTRA beyond the cap. The check is in CREDIT; failure to check would be a coding bug, not a design flaw.

**No admin key.** The native implementation has no admin DID that could be compromised. Credits are system calls that every importer re-derives from the chain and checks; a block with credits that do not match is rejected. Credit operations are constrained to events visible on-chain (an off-chain attacker cannot fabricate fake service events because validators won't accept credit instructions for non-existent service events).

**Race conditions on balance.** Credits are applied at the start of a block, before its transactions; transactions on the same balance are serialized by the underlying consensus.

**Integer overflow.** All amounts use u128 with checked arithmetic. Overflow attempts revert the transaction.

**Replay attacks.** Each credit includes a `log_event_hash`. While the contract doesn't enforce uniqueness (multiple credits for different events can share zero history), the reward accumulator guarantees that the same service event is not double-credited (the accumulator tracks which events have been credited).

## 11. Upgrade path

The rewards code is part of the node, not a deployed contract, and there is no admin upgrade path. Its behavior changes only through a node release that every validator runs, shipped as a coordinated protocol upgrade.

This is intentional. Upgradeable token contracts have repeatedly been exploited. Having no admin upgrade makes auditing simpler and removes that attack surface.

Because balances are native account balances, a fix to the rewards code never requires migrating balances.

## 12. References

- ASTRA Economic Model Decision Memo (internal)
- SpaceKit Tokenomics v2.0
- [`ASTRA_EMISSION.md`](./ASTRA_EMISSION.md)
- [`SERVICE_REWARD_ACCUMULATOR_SPEC.md`](./SERVICE_REWARD_ACCUMULATOR_SPEC.md)
- [`ASTRA_LEDGER.md`](../../infra/spacekit-compute-node/ASTRA_LEDGER.md)
- Implementation: `spacekit-compute-node` `native_rewards.rs`

## 13. Contact

For questions on the contract specification:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
