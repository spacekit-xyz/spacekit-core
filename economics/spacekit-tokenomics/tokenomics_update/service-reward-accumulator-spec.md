# Service Reward Accumulator Integration Specification

**Status:** Pre-implementation specification
**Version:** 1.0
**Owner:** SWTCH Labs
**Date:** 2026
**Type:** Protocol-level function (not a smart contract)
**References:** AstraRewards Contract Specification; ASTRA Emission Schedule

This document specifies the Service Reward Accumulator (SRA) — the protocol-level function that reads structured service log events, computes ASTRA rewards per the emission schedule, and submits credit instructions to the AstraRewards contract.

The SRA is the bridge between service events (operators actually doing work) and ASTRA emissions (operators getting paid). It runs as part of consensus execution, not as a smart contract called by users.

## 1. Architectural position

The SRA is a protocol-level function that lives in the consensus execution path:

```
Validator block proposer creates block
  ↓
Block contains: transactions + structured log events from this block's executions
  ↓
Block proposal goes through consensus (proposed → voted → finalized)
  ↓
On finalization, validators execute the block atomically:
  1. Transactions in the block are executed (state changes applied)
  2. Service log events are emitted to the structured log
  3. SRA reads the log events for this block
  4. SRA computes rewards per the emission schedule
  5. SRA submits CREDIT operations to AstraRewards for each rewarded event
  6. Block is committed to state
  ↓
Next block begins; cycle continues
```

The SRA's submissions to AstraRewards happen as part of the same block's execution. They are inseparable from the block's other state changes. All validators executing the same block compute the same SRA outputs (deterministic), so consensus naturally agrees.

This is similar to how Ethereum's beacon chain credits validators for consensus work: not via smart contract calls from arbitrary users, but as part of the protocol's execution layer.

## 2. Data sources

The SRA reads from three sources in each block:

**Structured log events.** The `spacekit-log` crate produces deterministic content-hashed log events for each service operation. The SRA reads these from the block's log emissions.

**Operator registry.** The protocol maintains a registry of registered operators (their DIDs, the service categories they're providing, their current reputation score if reputation is active). The SRA uses this to associate log events with operators.

**Current emission schedule state.** The SRA computes rewards using the current epoch's emission rates. The epoch state is maintained at the protocol level (current_epoch_number, current_year_decimal, per-category remaining emission for this epoch).

## 3. Service event classification

The SRA classifies each structured log event into one of four service categories:

### 3.1 Consensus validation events

Log events tagged with the `consensus.*` namespace:

- `consensus.proposal.accepted`: A block proposal was successfully proposed by this operator and accepted by the network.
  - Reward unit: 1 (per accepted proposal)
  - Operator: the block proposer's DID

- `consensus.vote.correct`: A vote was cast on a proposal that was subsequently finalized correctly.
  - Reward unit: 1 (per correct vote; weighted by reputation if reputation is active)
  - Operator: the voter's DID

- `consensus.envelope.signed`: A block envelope signature was contributed.
  - Reward unit: 1 (per signature; weighted by signature complexity)
  - Operator: the signer's DID

- `consensus.uptime.confirmed`: Validator uptime during an assigned slot.
  - Reward unit: proportional to uptime fraction
  - Operator: the validator's DID

### 3.2 Compute service events

Log events tagged with the `compute.*` namespace:

- `compute.contract.executed`: A smart contract was executed by this compute node.
  - Reward unit: gas units consumed (linear)
  - Operator: the compute node operator's DID

- `compute.host_hook.invoked`: A host hook (e.g., growformer_generation, storage operation) was invoked.
  - Reward unit: gas units consumed by the host hook
  - Operator: the compute node operator's DID

### 3.3 Storage service events

Log events tagged with the `storage.*` namespace:

- `storage.blob.served_read`: A storage read was successfully served.
  - Reward unit: 1 base + (bytes_returned / 1024) (per KB)
  - Operator: the storage node operator's DID

- `storage.blob.served_write`: A storage write was successfully accepted and durably stored.
  - Reward unit: 1 base + (bytes_written / 1024)
  - Operator: the storage node operator's DID

- `storage.proof.attested`: A storage durability proof was submitted on time.
  - Reward unit: 1 base + (bytes_proven / 1024)
  - Operator: the attester's DID

- `storage.capacity.maintained`: Durable storage capacity over an epoch (continuous).
  - Reward unit: 1 per byte-hour stored
  - Operator: the capacity provider's DID

### 3.4 Messaging service events

Log events tagged with the `messaging.*` namespace:

- `messaging.message.delivered`: A direct message was successfully delivered.
  - Reward unit: 1 per delivered message
  - Operator: the messaging node operator's DID

- `messaging.broadcast.sent`: A group broadcast was completed.
  - Reward unit: 1 per recipient (broadcast to N recipients = N units)
  - Operator: the messaging node operator's DID

- `messaging.key.resolved`: A recipient's encryption key was resolved for a delivery.
  - Reward unit: 1 per resolution
  - Operator: the messaging node operator's DID

## 4. Reward computation

For each classified service event, the SRA computes the ASTRA reward as follows:

```
reward = (epoch_remaining_emission_for_category) * (event_resource_units) / (epoch_remaining_resource_units_for_category)
```

Where:
- `epoch_remaining_emission_for_category`: ASTRA budget for this category in the current epoch, minus what's already been credited this epoch
- `event_resource_units`: the resource units for this specific event (gas, bytes, messages, etc.)
- `epoch_remaining_resource_units_for_category`: estimated total resource consumption remaining in this epoch (computed from running total + projected remaining activity)

In practice, the formula is implemented incrementally. The SRA maintains per-epoch accumulators for each category:

```
epoch_emission_budget[category] = annual_emission * category_share / epochs_per_year * decay_factor(t)
epoch_consumed_emission[category] = sum of rewards credited this epoch for this category
epoch_resource_total[category] = sum of resource units served this epoch for this category
```

For each event, the SRA does:

1. Read the event's category and resource units.
2. Get the current `epoch_emission_budget[category]` and `epoch_consumed_emission[category]`.
3. Compute the per-unit rate: `rate = epoch_emission_budget[category] / epoch_resource_total[category]`.
4. Compute the reward: `reward = rate * event_resource_units`.
5. Check that `epoch_consumed_emission[category] + reward <= epoch_emission_budget[category]`. If exceeded, cap to remaining.
6. Update `epoch_consumed_emission[category] += reward`.
7. Submit a CREDIT to AstraRewards with: operator_did, reward, log_event_hash.

### 4.1 Dynamic rate adjustment within an epoch

Per-event rates adjust dynamically as events accumulate within an epoch. Early events in an epoch have higher per-unit rates (less competition for the epoch's budget); later events have lower rates (budget being spent).

This is intentional: it produces stable per-epoch emission totals without requiring forecasting, while still rewarding per-event service contribution.

A practical example:
- Epoch budget for compute: 1,000 ASTRA
- 10 compute events served, each consuming 100 gas units
- Per-unit rate after each event: 1, 1.1, 1.2, 1.3, 1.4, 1.6, 1.8, 2.2, 2.8, 4.0
- Each event earns: 100, 110, 120, 130, 140, 160, 180, 220, 280, 400
- Total epoch emission: 1,000 ASTRA (capped)

This produces a strong incentive for operators to provide service early in an epoch and a self-correcting dynamic where heavy use later in the epoch is rewarded less per event.

### 4.2 Epoch finalization

At the end of each epoch:

1. Any unused emission rolls over to the next epoch's budget (caps don't reset).
2. The decay factor for the next epoch is computed: `decay_factor(t+1) = decay_factor(t) * exp(-1/4 * ln(2) / epochs_per_year)`.
3. New per-category epoch budgets are computed: `epoch_emission_budget[category] = annual_emission * category_share / epochs_per_year * new_decay_factor + rolled_over_amount`.
4. New epoch begins.

## 5. Approved vs. unapproved service events

Not all logged events earn ASTRA. The SRA applies validation rules to filter:

**Approved events earn ASTRA:**
- Event is in a known service category (matches one of the 18 event types above)
- Event has a valid log_event_hash that's been committed to the chain
- The operator's DID is registered and active
- The operator hasn't been slashed in this epoch (slashing temporarily suspends rewards)
- The event's claimed resource consumption is within plausible bounds (sanity-checked against the contract's gas accounting)

**Unapproved events do NOT earn ASTRA:**
- Events from contracts that reverted (contract executed but failed)
- Events from unregistered operators (no DID in the registry)
- Events that fail integrity checks (manipulated log entries)
- Events from operators currently under slashing (rewards suspended)
- Events with implausible resource claims (e.g., a single message with 1B bytes — too large to be real)

The SRA logs the unapproved events for auditing but does not submit credits for them. This means the operator did provide some work but it didn't earn ASTRA. Operators can investigate why their events were not rewarded by checking the SRA's audit log.

## 6. Integration with the AstraRewards contract

The SRA submits CREDIT operations to AstraRewards in a batched fashion:

**Per-block batching.** All credits earned from a block's events are submitted as a batch when the block is executed. The SRA executes:

```
for each approved event in this block:
  AstraRewards.CREDIT(
    recipient_did_hash = event.operator_did_hash,
    amount = computed_reward,
    log_event_hash = event.content_hash
  )
```

The batch is part of the block's execution. All validators execute the same batch. Consensus naturally agrees because all validators see the same logs and compute the same rewards.

**Atomicity.** If any CREDIT operation in the batch fails (e.g., cap exceeded), only that specific credit reverts. Other credits in the same batch succeed. The SRA logs the failure for auditing.

**Cap enforcement at the SRA level.** Before submitting a CREDIT, the SRA verifies `AstraRewards.total_emitted + this_credit_amount <= 2B * 10^18`. If exceeded, the CREDIT is cancelled (and the cap_reached event is emitted in the contract). The remaining budget is split proportionally among the affected events.

## 7. Reputation integration (post-fork)

In the post-fork model where reputation-weighted consensus is active, the SRA uses reputation as a weighting factor for consensus validation rewards:

```
vote_reward_units = base_unit * (1 + reputation_bonus)
```

Where `reputation_bonus` is a function of the operator's reputation score (e.g., 0 for new operators, 0.5 for established, 1.0 for highly trusted). The bonus is capped to prevent extreme reputation from completely dominating rewards.

For other service categories (compute, storage, messaging), reputation is a secondary signal — the primary reward driver is measured resource consumption. Reputation can be used as a slashing modifier (better-reputation operators take less of a slashing hit for the same offense) but doesn't significantly affect base rewards.

## 8. Slashing interaction

When an operator is slashed for misbehavior:

- Their staked ASTRA is slashed at the AstraRewards level (separate operation, handled by the slashing contract).
- Their pending rewards (balance in AstraRewards) are NOT slashed. Earned rewards are payment for service already provided, not at risk.
- Their reward stream is suspended for a slashing penalty period. During the penalty period, the SRA does not submit CREDIT operations for events from this operator. The operator can still earn for their work during the penalty period — the work just doesn't yield ASTRA until the penalty ends.
- After the penalty period, normal rewards resume.

Slashing reduces the operator's stake (and their voting/reputation power) but does not retroactively reduce earned rewards.

## 9. Per-block determinism

The SRA's output must be deterministic across all validators. To ensure this:

- All inputs are deterministic: log events committed in the block, current emission schedule state, operator registry state at the block's height.
- The reward computation is integer arithmetic in fixed-point (18 decimals).
- The decay factor uses pre-computed lookup tables, not floating-point math.
- The order of event processing is deterministic (events ordered by transaction-position-in-block).

Any non-determinism in SRA output would cause validators to disagree on state, leading to consensus failure. The SRA is designed and tested to produce identical output on identical input.

## 10. Implementation as compute-node function

The SRA is implemented as a function in `spacekit-compute-node`. It is not a separate process; it runs as part of the validator's block execution pipeline. Specifically:

```rust
// in spacekit-compute-node, somewhere in the block execution code:

fn execute_block(block: &Block, state: &mut State) -> Result<(), ExecutionError> {
    // 1. Execute transactions
    for tx in &block.transactions {
        execute_transaction(tx, state)?;
    }

    // 2. Collect log events from this block
    let log_events = collect_log_events(state)?;

    // 3. Run the service reward accumulator
    let credits = service_reward_accumulator(log_events, &block.epoch, &state.registry);

    // 4. Submit credits to AstraRewards (as part of state)
    for credit in credits {
        astra_rewards_credit(state, credit)?;
    }

    // 5. Other block-end processing
    finalize_block(state)?;

    Ok(())
}

fn service_reward_accumulator(
    events: Vec<LogEvent>,
    epoch: &EpochState,
    registry: &OperatorRegistry,
) -> Vec<CreditInstruction> {
    let mut credits = Vec::new();
    let mut epoch_state = epoch.clone();  // mutable working copy

    for event in events {
        let category = classify_event(&event);
        let resource_units = extract_resource_units(&event, category);

        if !is_approved(&event, &registry) {
            continue;  // skip unapproved events
        }

        let reward = compute_reward(
            category,
            resource_units,
            &mut epoch_state,
        );

        if reward > 0 {
            credits.push(CreditInstruction {
                recipient_did_hash: event.operator_did_hash,
                amount: reward,
                log_event_hash: event.content_hash,
            });
        }
    }

    credits
}
```

The actual implementation will be in Rust in spacekit-compute-node; this sketch illustrates the structure.

## 11. Testing and validation

The SRA needs to be tested under several scenarios:

**Per-block accuracy.** For a specific block with specific log events, the SRA produces specific credit operations. Tests verify expected outputs.

**Cross-validator consistency.** Multiple validator implementations executing the same block produce identical SRA outputs.

**Edge cases:**
- Empty block (no events) → no credits
- Block with non-existent operator DIDs → unapproved events filtered
- Cap-exceeded scenarios → credits clamped or cancelled appropriately
- Epoch transitions mid-block → new rates applied correctly
- Concurrent service across categories → independent reward streams

**Performance.** SRA computation should not significantly slow block execution. Target: <10ms per block of typical size.

**Audit trail.** Every credit's log_event_hash should map to a real log event in the chain's audit trail.

## 12. Open questions and follow-ups

A few items that need future decision:

**Reputation integration timing.** Reputation-weighted consensus is post-fork. Until activation, all operators get base rewards. The SRA needs a clean migration path when reputation-weighting activates.

**Aggregation vs per-event.** Currently described as per-event credits. For very high-throughput networks, this could result in many credit operations per block. An optimization: aggregate per-operator per-block credits into single CREDIT operations. The audit trail is preserved (one credit per operator with summed amount, plus the list of contributing log_event_hashes).

**Curve adjustments.** The decay curve parameters are set; governance can adjust within constraints. The SRA needs a clean path to apply governance-approved adjustments at the next epoch boundary.

**Validator slashing for SRA misbehavior.** What if a validator's SRA implementation diverges from others (e.g., due to a bug)? The protocol detects this as consensus failure. The slashing for SRA divergence is part of the broader validator slashing policy.

## 13. References

- AstraRewards Contract Specification (Document F)
- ASTRA Emission Schedule (Document E)
- ASTRA Economic Model Decision Memo (internal)
- SpaceKit Tokenomics v2.0
- spacekit-log crate documentation
- spacekit-compute-node block execution documentation

## 14. Contact

For questions on the reward accumulator integration:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
