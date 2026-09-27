//! Service Reward Accumulator (SRA) — protocol-level reward computation.
//!
//! Spec: `spacekit-tokenomics/Service_Reward_Accumulator_Spec.md`
//! Emission: `spacekit-tokenomics/ASTRA_EMISSION.md`

mod astra_rewards;
mod emission;
mod events;
mod log_topics;

pub use astra_rewards::{
    encode_credit, encode_end_poa, encode_get_locked, encode_get_total_emitted, encode_init,
    encode_release, encode_set_locked_recipient, hash_did_bytes, lock_recipient_hashes,
    topic_label_bytes, treasury_did_hash, OP_CREDIT, OP_END_POA, OP_GET_LOCKED, OP_GET_PHASE,
    OP_INIT, OP_RELEASE, OP_SET_LOCKED_RECIPIENT, TREASURY_DID,
};
pub use emission::{
    category_share_bps, decay_halvings_for_epoch, epoch_category_budget_wei, CategoryShareBps,
    EPOCHS_PER_HALVING, EPOCHS_PER_YEAR, INITIAL_ANNUAL_EMISSION_WEI,
};
pub use events::{
    classify_log_label, classify_log_topic, ServiceCategory, ServiceRewardEvent,
    SRA_TOPIC_COMPUTE_EXECUTED, SRA_TOPIC_CONSENSUS_VOTE, SRA_TOPIC_MESSAGING_DELIVERED,
    SRA_TOPIC_STORAGE_WRITE,
};
pub use log_topics::{
    resource_units_le, COMPUTE_CONTRACT_EXECUTED, CONSENSUS_VOTE_CORRECT,
    MESSAGING_MESSAGE_DELIVERED, STORAGE_BLOB_WRITE,
};

use std::collections::BTreeMap;

use spacekit_primitives::v1::sdk::token::ASTRA_MAX_SUPPLY_WEI;

/// One CREDIT instruction for the AstraRewards contract (opcode 0x10 payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreditInstruction {
    pub recipient_did_hash: [u8; 32],
    pub amount_wei: u128,
    pub log_event_hash: [u8; 32],
}

/// Per-category epoch accumulator (in-memory; persisted by host).
#[derive(Debug, Clone, Default)]
pub struct CategoryEpochState {
    /// ASTRA available to this category for the current epoch (includes rollover).
    pub budget_wei: u128,
    /// ASTRA credited from this category's budget in the current epoch.
    pub consumed_wei: u128,
    /// Total measured units in the current epoch.
    pub resource_units: u128,
    /// Measured units per operator in the current epoch.
    pub units_by_operator: BTreeMap<[u8; 32], u128>,
}

/// Global SRA state advanced once per epoch boundary.
///
/// Rewards follow ASTRA_EMISSION §5: during an epoch (one day) the SRA only
/// records each operator's measured units per category; when the epoch
/// closes, the category's budget is split in proportion to those units. An
/// epoch with no activity in a category rolls its budget into the next one.
///
/// (Crediting each event as it arrived gave the first event of every epoch
/// the category's whole remaining budget.)
#[derive(Debug, Clone)]
pub struct SraState {
    pub genesis_timestamp_secs: u64,
    pub epoch_index: u64,
    pub total_credited_wei: u128,
    pub categories: [CategoryEpochState; 4],
}

/// Keep `budget * units` inside u128: budgets are below 2^80 wei, so units are
/// scaled down (uniformly per category) until the epoch total is below 2^47.
const MAX_UNITS_BITS: u32 = 47;

impl SraState {
    pub fn new(genesis_timestamp_secs: u64) -> Self {
        let mut s = Self {
            genesis_timestamp_secs,
            epoch_index: 0,
            total_credited_wei: 0,
            categories: Default::default(),
        };
        s.refresh_epoch_budgets();
        s
    }

    /// Close every epoch that ended before `block_timestamp` and return the
    /// credits for them. Call before [`record_events`](Self::record_events)
    /// for a block, so the block's events land in its own epoch.
    pub fn maybe_advance_epoch(&mut self, block_timestamp_secs: u64) -> Vec<CreditInstruction> {
        let epoch = block_timestamp_secs.saturating_sub(self.genesis_timestamp_secs) / 86_400;
        let mut credits = Vec::new();
        while self.epoch_index < epoch {
            credits.extend(self.settle_epoch());
            self.epoch_index += 1;
            self.roll_epoch();
        }
        credits
    }

    /// Record approved service events; rewards are paid when the epoch closes.
    pub fn record_events(&mut self, events: &[ServiceRewardEvent]) {
        for ev in events {
            if !ev.approved || ev.resource_units == 0 {
                continue;
            }
            let cat = &mut self.categories[ev.category.index()];
            cat.resource_units = cat.resource_units.saturating_add(ev.resource_units);
            let units = cat.units_by_operator.entry(ev.operator_did_hash).or_insert(0);
            *units = units.saturating_add(ev.resource_units);
        }
    }

    /// Deprecated name for [`record_events`](Self::record_events); returns no
    /// credits because rewards are settled per epoch.
    #[deprecated(note = "use record_events + maybe_advance_epoch")]
    pub fn process_events(&mut self, events: &[ServiceRewardEvent]) -> Vec<CreditInstruction> {
        self.record_events(events);
        Vec::new()
    }

    /// Split each category's budget for the current epoch among its operators.
    fn settle_epoch(&mut self) -> Vec<CreditInstruction> {
        let mut credits = Vec::new();
        let epoch = self.epoch_index;
        for (idx, cat) in self.categories.iter_mut().enumerate() {
            if cat.resource_units == 0 || cat.units_by_operator.is_empty() {
                continue;
            }
            let budget = cat.budget_wei.saturating_sub(cat.consumed_wei);
            let shift = (128 - cat.resource_units.leading_zeros()).saturating_sub(MAX_UNITS_BITS);
            let total = (cat.resource_units >> shift).max(1);
            for (operator, units) in &cat.units_by_operator {
                let scaled = units >> shift;
                let reward = budget.saturating_mul(scaled) / total;
                if reward == 0 {
                    continue;
                }
                if self.total_credited_wei.saturating_add(reward) > ASTRA_MAX_SUPPLY_WEI {
                    return credits;
                }
                cat.consumed_wei = cat.consumed_wei.saturating_add(reward);
                self.total_credited_wei = self.total_credited_wei.saturating_add(reward);
                credits.push(CreditInstruction {
                    recipient_did_hash: *operator,
                    amount_wei: reward,
                    log_event_hash: epoch_credit_hash(epoch, idx as u8, operator),
                });
            }
        }
        credits
    }

    fn roll_epoch(&mut self) {
        for c in &mut self.categories {
            // Unspent budget (no activity, or rounding dust) rolls over.
            let rollover = c.budget_wei.saturating_sub(c.consumed_wei);
            c.budget_wei = rollover;
            c.consumed_wei = 0;
            c.resource_units = 0;
            c.units_by_operator.clear();
        }
        self.refresh_epoch_budgets();
    }

    fn refresh_epoch_budgets(&mut self) {
        let halvings = decay_halvings_for_epoch(self.epoch_index);
        for (i, cat) in ServiceCategory::ALL.iter().enumerate() {
            let add = epoch_category_budget_wei(*cat, halvings);
            self.categories[i].budget_wei = self.categories[i].budget_wei.saturating_add(add);
        }
    }
}

/// Deterministic audit key for an epoch settlement credit:
/// `[epoch 8][category 1][first 23 bytes of the operator key]`.
fn epoch_credit_hash(epoch: u64, category: u8, operator: &[u8; 32]) -> [u8; 32] {
    let mut h = [0u8; 32];
    h[0..8].copy_from_slice(&epoch.to_le_bytes());
    h[8] = category;
    h[9..32].copy_from_slice(&operator[..23]);
    h
}

/// Build compute service events from successful transaction receipts (devnet bridge).
pub fn events_from_compute_gas(
    block_number: u64,
    receipts: &[(bool, u128, [u8; 20])],
) -> Vec<ServiceRewardEvent> {
    let mut out = Vec::new();
    for (i, (success, gas_used, operator_addr)) in receipts.iter().enumerate() {
        if !success || *gas_used == 0 {
            continue;
        }
        let mut log_hash = [0u8; 32];
        log_hash[0..8].copy_from_slice(&block_number.to_le_bytes());
        log_hash[8..16].copy_from_slice(&(i as u64).to_le_bytes());
        // gas_used is u128 (16 bytes); a [16..24] slice panicked here.
        log_hash[16..32].copy_from_slice(&gas_used.to_le_bytes());
        out.push(ServiceRewardEvent {
            operator_did_hash: address_to_did_hash(operator_addr),
            category: ServiceCategory::Compute,
            resource_units: *gas_used,
            log_event_hash: log_hash,
            approved: true,
        });
    }
    out
}

/// Map 20-byte SwtchVM address to 32-byte DID hash (padded; host may replace with real DID hash).
pub fn address_to_did_hash(addr: &[u8; 20]) -> [u8; 32] {
    let mut h = [0u8; 32];
    h[12..32].copy_from_slice(addr);
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_budget_positive_year_zero() {
        let b = epoch_category_budget_wei(ServiceCategory::Compute, 0);
        assert!(b > 0);
    }

    fn ev(op: u8, cat: ServiceCategory, units: u128) -> ServiceRewardEvent {
        ServiceRewardEvent {
            operator_did_hash: [op; 32],
            category: cat,
            resource_units: units,
            log_event_hash: [0u8; 32],
            approved: true,
        }
    }

    #[test]
    fn epoch_budget_is_split_by_measured_units() {
        let g = 1_700_000_000;
        let mut state = SraState::new(g);
        assert!(state.maybe_advance_epoch(g + 10).is_empty());
        state.record_events(&[ev(1, ServiceCategory::Compute, 3_000), ev(2, ServiceCategory::Compute, 1_000)]);
        // Nothing is paid mid-epoch.
        assert!(state.maybe_advance_epoch(g + 86_399).is_empty());

        let credits = state.maybe_advance_epoch(g + 86_400);
        let budget = epoch_category_budget_wei(ServiceCategory::Compute, 0);
        assert_eq!(credits.len(), 2);
        let a = credits.iter().find(|c| c.recipient_did_hash == [1; 32]).unwrap().amount_wei;
        let b = credits.iter().find(|c| c.recipient_did_hash == [2; 32]).unwrap().amount_wei;
        assert_eq!(a, budget * 3 / 4);
        assert_eq!(b, budget / 4);
        // Order of arrival does not matter: the first event no longer takes everything.
        assert!(a + b <= budget);
    }

    #[test]
    fn idle_categories_roll_over_and_huge_units_do_not_overflow() {
        let g = 1_700_000_000;
        let mut state = SraState::new(g);
        let day = epoch_category_budget_wei(ServiceCategory::Storage, 0);
        state.maybe_advance_epoch(g + 86_400); // storage idle on day 0
        state.record_events(&[ev(5, ServiceCategory::Storage, u128::MAX / 2), ev(6, ServiceCategory::Storage, u128::MAX / 2)]);
        let credits = state.maybe_advance_epoch(g + 2 * 86_400);
        let total: u128 = credits.iter().map(|c| c.amount_wei).sum();
        // Scaling leaves at most dust (well under a millionth of an ASTRA), which rolls over.
        assert!(total <= 2 * day && 2 * day - total < 1_000_000_000_000, "{total} vs {}", 2 * day);
        assert_eq!(credits[0].log_event_hash[0..8], 1u64.to_le_bytes());
    }

    #[test]
    fn compute_gas_events_do_not_panic_on_u128_gas() {
        let events = events_from_compute_gas(7, &[(true, u128::MAX, [9u8; 20]), (false, 5, [1u8; 20])]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].log_event_hash[16..32], u128::MAX.to_le_bytes());
    }

    #[test]
    fn halving_reduces_budget() {
        let b0 = epoch_category_budget_wei(ServiceCategory::Consensus, 0);
        let b1 = epoch_category_budget_wei(ServiceCategory::Consensus, 1);
        assert!(b1 < b0);
    }
}
