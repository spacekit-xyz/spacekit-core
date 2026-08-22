//! Service Reward Accumulator (SRA) — protocol-level reward computation.
//!
//! Spec: `spacekit-tokenomics/Service_Reward_Accumulator_Spec.md`
//! Emission: `spacekit-tokenomics/ASTRA_EMISSION.md`

mod astra_rewards;
mod emission;
mod events;
mod log_topics;

pub use astra_rewards::{
    encode_credit, encode_get_total_emitted, encode_init, hash_did_bytes, topic_label_bytes,
    treasury_did_hash, OP_CREDIT, OP_INIT, TREASURY_DID,
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
    pub budget_wei: u128,
    pub consumed_wei: u128,
    pub resource_units: u128,
}

/// Global SRA state advanced once per epoch boundary.
#[derive(Debug, Clone)]
pub struct SraState {
    pub genesis_timestamp_secs: u64,
    pub epoch_index: u64,
    pub total_credited_wei: u128,
    pub categories: [CategoryEpochState; 4],
}

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

    /// Advance epoch if `block_timestamp` crossed a day boundary since genesis.
    pub fn maybe_advance_epoch(&mut self, block_timestamp_secs: u64) {
        let epoch = block_timestamp_secs.saturating_sub(self.genesis_timestamp_secs) / 86_400;
        while self.epoch_index < epoch {
            self.epoch_index += 1;
            self.roll_epoch();
        }
    }

    fn roll_epoch(&mut self) {
        for c in &mut self.categories {
            let rollover = c.budget_wei.saturating_sub(c.consumed_wei);
            c.budget_wei = rollover;
            c.consumed_wei = 0;
            c.resource_units = 0;
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

    /// Process approved service events from one block; returns CREDIT instructions.
    pub fn process_events(&mut self, events: &[ServiceRewardEvent]) -> Vec<CreditInstruction> {
        let mut credits = Vec::new();
        for ev in events {
            if !ev.approved {
                continue;
            }
            let idx = ev.category.index();
            let cat = &mut self.categories[idx];
            let remaining_budget = cat.budget_wei.saturating_sub(cat.consumed_wei);
            if remaining_budget == 0 || ev.resource_units == 0 {
                continue;
            }
            let new_total = cat.resource_units.saturating_add(ev.resource_units);
            let reward = remaining_budget
                .saturating_mul(ev.resource_units)
                .checked_div(new_total.max(1))
                .unwrap_or(0);
            if reward == 0 {
                cat.resource_units = new_total;
                continue;
            }
            if self.total_credited_wei.saturating_add(reward) > ASTRA_MAX_SUPPLY_WEI {
                break;
            }
            cat.consumed_wei = cat.consumed_wei.saturating_add(reward);
            cat.resource_units = new_total;
            self.total_credited_wei = self.total_credited_wei.saturating_add(reward);
            credits.push(CreditInstruction {
                recipient_did_hash: ev.operator_did_hash,
                amount_wei: reward,
                log_event_hash: ev.log_event_hash,
            });
        }
        credits
    }
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
        log_hash[16..24].copy_from_slice(&gas_used.to_le_bytes());
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

    #[test]
    fn process_compute_events_mints_credits() {
        let mut state = SraState::new(1_700_000_000);
        let events = vec![ServiceRewardEvent {
            operator_did_hash: [1u8; 32],
            category: ServiceCategory::Compute,
            resource_units: 1000,
            log_event_hash: [2u8; 32],
            approved: true,
        }];
        let credits = state.process_events(&events);
        assert_eq!(credits.len(), 1);
        assert!(credits[0].amount_wei > 0);
    }

    #[test]
    fn halving_reduces_budget() {
        let b0 = epoch_category_budget_wei(ServiceCategory::Consensus, 0);
        let b1 = epoch_category_budget_wei(ServiceCategory::Consensus, 1);
        assert!(b1 < b0);
    }
}
