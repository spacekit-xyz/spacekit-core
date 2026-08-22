//! Emission schedule constants and epoch budget math (`ASTRA_EMISSION.md`).

use crate::events::ServiceCategory;
use spacekit_primitives::v1::sdk::token::ASTRA_INITIAL_ANNUAL_EMISSION_WEI;

/// Epochs per calendar year (one epoch = one day).
pub const EPOCHS_PER_YEAR: u64 = 365;

/// Halving period in epochs (4 years).
pub const EPOCHS_PER_HALVING: u64 = EPOCHS_PER_YEAR * 4;

pub const INITIAL_ANNUAL_EMISSION_WEI: u128 = ASTRA_INITIAL_ANNUAL_EMISSION_WEI;

/// Category shares in basis points (sum = 10_000).
#[derive(Debug, Clone, Copy)]
pub struct CategoryShareBps {
    pub consensus: u32,
    pub compute: u32,
    pub storage: u32,
    pub messaging: u32,
}

impl Default for CategoryShareBps {
    fn default() -> Self {
        Self {
            consensus: 4000,
            compute: 3000,
            storage: 2000,
            messaging: 1000,
        }
    }
}

pub fn category_share_bps(cat: ServiceCategory) -> u32 {
    let d = CategoryShareBps::default();
    match cat {
        ServiceCategory::Consensus => d.consensus,
        ServiceCategory::Compute => d.compute,
        ServiceCategory::Storage => d.storage,
        ServiceCategory::Messaging => d.messaging,
    }
}

/// Number of completed 4-year halvings for a given epoch index.
pub fn decay_halvings_for_epoch(epoch_index: u64) -> u32 {
    (epoch_index / EPOCHS_PER_HALVING).min(63) as u32
}

/// Daily epoch budget for one category at a given halving count.
pub fn epoch_category_budget_wei(category: ServiceCategory, halvings: u32) -> u128 {
    let annual_share =
        INITIAL_ANNUAL_EMISSION_WEI.saturating_mul(category_share_bps(category) as u128) / 10_000;
    let daily = annual_share / EPOCHS_PER_YEAR as u128;
    daily >> halvings
}
