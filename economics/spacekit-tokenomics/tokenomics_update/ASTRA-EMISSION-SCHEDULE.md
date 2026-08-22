# ASTRA Emission Schedule

**Status:** Canonical technical reference
**Version:** 1.0
**Owner:** SWTCH Labs
**Date:** 2026
**References:** ASTRA Economic Model Decision Memo; SpaceKit Tokenomics v2.0

This document specifies the emission schedule for ASTRA — the rate at which new ASTRA is minted to operators as rewards for service. It provides the constants used by the AstraRewards contract and the protocol-level reward accumulator.

The decisions in this document operationalize the ASTRA Economic Model Decision Memo. The 2B hard cap is enforced; the decay curve ensures asymptotic approach to the cap without exceeding it.

## 1. Overview

ASTRA emission has three properties enforced at the protocol level:

**Hard cap.** Total ever-emitted ASTRA does not exceed 2,000,000,000 ASTRA (2 billion).

**Decay over time.** Per-event emission rates decrease as the network ages, governed by a decay curve that asymptotically approaches the cap.

**Per-category rates.** Different service categories (consensus, compute, storage, messaging) earn different ASTRA per unit of measured contribution. Rates are calibrated to each category's resource cost and network value.

The schedule is set at protocol genesis and adjusted only by on-chain governance proposals. The 2B cap itself is not subject to governance adjustment — it is a constant of the protocol.

## 2. The decay curve

ASTRA emission follows a halving curve similar to Bitcoin's, adapted for SpaceKit's continuous service model rather than discrete blocks.

**Halving period:** 4 years.

**Initial annual emission (year 1):** 200,000,000 ASTRA (10% of cap).

**Decay formula:** Annual emission at year `t` equals `initial * 0.5^(t / 4)`. For continuous time, the formula is `emission_rate(t) = (initial_rate) * exp(-t * ln(2) / 4)`.

**Asymptotic total:** Integrating the decay function from t=0 to infinity yields `initial * 4 / ln(2) ≈ 1.154 * initial`. With initial = 200M, asymptotic total ever-emitted = ~1.154 * 200M = ~1,154 million = 1.154B ASTRA.

**Unused headroom:** 2B cap minus 1.154B asymptotic emission = ~846M ASTRA reserved for treasury allocation, governance reserves, and bootstrap subsidies.

## 3. Year-by-year emission projection

| Year | Annual Emission (ASTRA) | Cumulative Emitted (ASTRA) | % of Cap Used |
|------|-------------------------|----------------------------|---------------|
| 1    | 200,000,000             | 200,000,000                | 10.0%         |
| 2    | 168,179,283             | 368,179,283                | 18.4%         |
| 4    | 100,000,000             | 631,775,138                | 31.6%         |
| 8    | 50,000,000               | 884,962,720                | 44.2%         |
| 12   | 25,000,000               | 1,011,553,851              | 50.6%         |
| 16   | 12,500,000               | 1,074,830,418              | 53.7%         |
| 20   | 6,250,000                | 1,106,477,704              | 55.3%         |
| 30   | 1,562,500                | 1,135,635,772              | 56.8%         |
| 40   | 390,625                  | 1,144,776,290              | 57.2%         |
| 50   | 97,656                   | 1,147,609,961              | 57.4%         |
| 100  | 23.4                     | 1,148,000,000+             | ~57.4%        |

By year 100, annual emission has decayed to a level where the network's emission has effectively ceased; new ASTRA can technically still be minted, but at rates below practical relevance. Cumulative emission approaches the mathematical asymptote of ~1.15B.

The ~846M ASTRA between 1.15B asymptotic emission and 2B hard cap is the protocol's treasury allocation pool, plus reserves for governance-approved future use.

## 4. Per-category emission allocation

The annual emission is divided among the four service categories. The split prioritizes consensus security (validators are critical for network safety) while providing meaningful rewards to compute, storage, and messaging operators.

| Category | Share of Annual Emission | Rationale |
|----------|--------------------------|-----------|
| Consensus validation | 40% | Validators secure consensus; their work has the highest network-protective value. |
| Compute service | 30% | Compute is the primary network resource consumed by smart contracts and dApps. |
| Storage service | 20% | Storage is consumed less frequently per service event but persistently durable. |
| Messaging service | 10% | Messaging has lower per-event resource cost but supports the network's communication layer. |

Specific values for year 1:

| Category | Year 1 Annual Emission |
|----------|------------------------|
| Consensus validation | 80,000,000 ASTRA |
| Compute service | 60,000,000 ASTRA |
| Storage service | 40,000,000 ASTRA |
| Messaging service | 20,000,000 ASTRA |

These category shares are set by protocol governance and may be adjusted within a constrained range (each category between 5% and 60% of total emission, total summing to 100%) by on-chain proposal. Adjustments may be needed as the network's service mix evolves.

## 5. Per-event emission rates

Within each category, per-event emission is proportional to measured resource consumption. The formula is:

```
ASTRA_earned_per_event = (annual_category_emission / annual_total_resource_in_category) 
                       * resource_consumed_by_this_event
```

The denominator (annual total resource consumed in the category) is the running total of all service activity in that category during the year. Per-event rates are computed dynamically based on the rate at which the resource is being consumed — events earn more ASTRA early in the year when fewer events have happened, and less per event later in the year when many events have happened.

In practice this is implemented as a per-epoch (e.g., per-day) computation:

1. At the start of each epoch, the protocol computes `epoch_emission_per_category = (annual_emission_per_category) / (epochs_per_year)`.
2. During the epoch, events accumulate against the epoch's allocation.
3. At the end of the epoch, each event's earned ASTRA is computed as `(event_resource / epoch_total_resource) * epoch_emission_per_category`.

If an epoch sees no activity in a category, its allocation rolls over to the next epoch. If an epoch sees activity exceeding the natural distribution, all events in that epoch share the epoch's fixed allocation (no overshoot of annual budget).

## 6. Resource measurement per category

The protocol measures different resources for different service categories:

**Consensus validation:**
- Block proposals successfully accepted (each proposal = 1 unit)
- Votes cast on correctly-finalized proposals (each vote = 1 unit, weighted by reputation if reputation tracking is active)
- Block envelope signatures contributed (each signature = 1 unit, weighted by complexity)
- Validator uptime during assigned slots (proportional to uptime fraction)

**Compute service:**
- Gas units consumed by contract executions served (linear with gas)
- Successful contract calls served (each call = a base unit + gas-proportional component)

**Storage service:**
- Bytes-hours of durable storage (linear with bytes × hours stored)
- Successful read operations (each read = 1 unit)
- Successful write operations (each write = 1 unit + bytes-proportional component)
- Storage proof attestations submitted on time (each attestation = 1 unit)

**Messaging service:**
- Messages successfully delivered (each delivery = 1 unit)
- Recipients served with resolved encryption keys (additional unit per recipient)
- Group broadcast operations completed (1 unit per recipient in the broadcast)

The exact measurement units, the relative weighting between sub-categories within a service category, and the formulas for converting raw activity into "resource units" are protocol parameters set in the governance specification and may be adjusted by on-chain proposal.

## 7. Treasury allocation

A portion of the 2B cap is held in a multi-signature wallet controlled by SWTCH Labs as the protocol treasury. Treasury ASTRA is used for:

- Protocol development funding (paying contributors who build SpaceKit)
- Audit and security work (paying for independent audits, bug bounties)
- Operational reserves (covering operational costs during the network's pre-revenue period)
- Bootstrap subsidies (initial allocations to early validators when no operator has earned enough through service to stake)
- Future ecosystem grants (subject to legal review and on-chain governance approval)

**Treasury initial allocation: 350,000,000 ASTRA (17.5% of cap).**

This is minted to the treasury wallet at protocol genesis. It is NOT subject to the decay curve — it exists at genesis and decreases only as treasury spending decisions are made.

**Treasury counts against the 2B cap.** Total ever-emitted = treasury allocation + cumulative operator emission. With ~1.154B asymptotic operator emission + 350M treasury = ~1.504B total, leaving ~496M as additional reserve.

**Treasury cannot be expanded.** Once the 350M treasury is allocated at genesis, the cap is binding. No additional treasury minting is possible. If the treasury depletes, governance can extend operator emission (if cap allows), but no new treasury allocation can be created beyond the genesis 350M.

## 8. Bootstrap allocation

The protocol faces a chicken-and-egg problem: validators need staked ASTRA to participate in consensus, but ASTRA is earned through service (including validation). At genesis, no one has earned ASTRA yet.

**Solution: Bootstrap pool of 50,000,000 ASTRA (2.5% of cap), drawn from treasury allocation at genesis.**

This is distributed to initial validators (the set that launches the network) as stake. Each initial validator receives a fixed allocation sufficient to meet the minimum stake requirement.

After bootstrap, validators self-fund their stake from earned ASTRA. The bootstrap pool is one-time only and not refilled.

Bootstrap allocations are subject to vesting (e.g., 4-year linear vesting from network genesis). This prevents early validators from immediately liquidating their bootstrap stake.

## 9. Headroom utilization

Total cap: 2,000,000,000 ASTRA

Allocations:
- Operator emission (asymptotic): ~1,154,000,000 (~57.7%)
- Treasury initial allocation: 350,000,000 (17.5%)
- (Bootstrap drawn from treasury, included above)

Total allocated/projected emission: ~1,504,000,000 (~75.2%)
Unused headroom: ~496,000,000 (~24.8%)

**The 496M of unused headroom is held in protocol reserve.** It can only be allocated by:

1. On-chain governance proposal to extend operator emission (e.g., adjusting the decay curve to be slower)
2. On-chain governance proposal to fund a specific ecosystem program (with legal review and disclosure)
3. On-chain governance proposal to refill the bootstrap pool (if the network needs additional bootstrap allocation in the future)

The reserve cannot be allocated unilaterally by SWTCH Labs. Governance approval is required.

## 10. Cap enforcement

The AstraRewards contract enforces the 2B cap at the protocol level:

- `total_emitted` tracks cumulative ASTRA minted via the credit operation.
- Each credit operation checks `total_emitted + credit_amount <= 2_000_000_000 * 10^18`.
- If a credit would exceed the cap, it is rejected (the entire credit, not partial).
- This is the backstop. The decay curve should prevent the cap from being reached in practice, but the contract enforces the cap regardless.

The contract does NOT have any path to mint above the cap, regardless of caller, signer, or governance action. This is a constant of the protocol.

## 11. Decay curve as on-chain function

The protocol-level reward accumulator computes per-epoch emission limits using the decay formula:

```
epoch_emission_for_category(t, category) = 
    (initial_annual_emission * category_share / epochs_per_year) * 0.5^(t / 4)
```

Where:
- `t` = years since network genesis (epoch-level granularity)
- `initial_annual_emission` = 200,000,000 ASTRA
- `category_share` = 0.4, 0.3, 0.2, 0.1 for consensus, compute, storage, messaging
- `epochs_per_year` = 365 (one epoch per day)

The formula is implemented in fixed-point arithmetic in the reward accumulator to maintain determinism across all validators computing the same emission limits.

Per-event emission within an epoch is computed dynamically:

```
event_emission = epoch_remaining_allocation * (event_resource / epoch_remaining_resource)
```

Updated incrementally as events accumulate.

## 12. Governance over the schedule

The following parameters may be adjusted by on-chain governance:

**Adjustable:**
- Per-category emission shares (each between 5% and 60%, summing to 100%)
- Resource weighting within categories (e.g., balance between block proposal rewards and vote rewards in consensus category)
- Bootstrap pool refill (using unused headroom)
- Treasury spending decisions (within the 350M treasury allocation)

**Constrained but adjustable with high quorum:**
- Decay curve halving period (between 2 years and 8 years; default 4)
- Initial annual emission rate (between 50M and 350M ASTRA; default 200M)
- These parameters can be changed only with super-majority governance and a 30-day notice period.

**Not adjustable by governance:**
- 2B hard cap
- Treasury initial allocation (350M, set at genesis)
- The non-burnability property (no automatic burn mechanism)

## 13. Honest limitations

A few honest acknowledgments:

**The numbers above are calibrated, not battle-tested.** The category shares (40/30/20/10), the initial emission rate (200M), the halving period (4 years), the treasury allocation (350M) are all values informed by analysis but not yet validated under sustained network operation. They may be adjusted via governance after testnet observation.

**The decay curve assumes service volume grows over time.** If the network grows slowly, per-event ASTRA earnings remain relatively high (per-operator economics good early). If the network grows rapidly, per-event ASTRA earnings drop fast (consistent with mature-network economics). Either way the schedule converges; only the per-operator rate trajectory differs.

**Treasury depletion is a real long-term consideration.** 350M ASTRA is meaningful but finite. If the protocol needs ongoing funding beyond what 350M plus emission allocation provides, future governance will need to consider extending emission or accepting external funding for protocol development. This is the same problem Bitcoin faces with declining block rewards; we're aware of it and design with it in mind.

**Per-category share adjustments could create gaming opportunities.** If governance shifts shares from compute to storage, operators might shift their behavior to maximize storage earnings, possibly distorting network resource provisioning. Future governance proposals affecting category shares need to be carefully analyzed for second-order effects.

## 14. References

- ASTRA Economic Model Decision Memo (internal)
- SpaceKit Tokenomics v2.0 (canonical technical spec)
- Public ASTRA documentation page
- AstraRewards Contract Specification (Document F)
- Service Reward Accumulator Integration Specification (Document G)
- SpaceKit Governance Specification

## 15. Contact

For questions on the emission schedule:

Astor Rivera
Founder & CTO, SWTCH Labs
astor@swtch.ai
