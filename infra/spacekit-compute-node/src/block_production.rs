//! When blocks are produced.
//!
//! Two modes, chosen per network in the PoA genesis file and changeable by a
//! `set_block_production` governance proposal:
//!
//! - `on_demand` (default): a block is produced only when there is work: a
//!   pending transaction (after `batch_window_ms`, so a burst lands in one
//!   block), a protocol system transaction that is due (reward settlement,
//!   the end of PoA in AstraRewards), or a heartbeat when the chain has been
//!   idle for `heartbeat_secs` (0 disables heartbeats).
//! - `interval`: a block every `block_time_ms`, empty or not.
//!
//! In both modes consecutive blocks are at least `block_time_ms` apart, and
//! authorities take turns: the authority at `height mod n` produces; if it has
//! not produced `grace_ms` after the block became due, the turn passes to the
//! next authority, and so on.
//!
//! Block validity does not depend on the mode: a node imports any correctly
//! sealed block. The mode only decides when honest authorities produce, which
//! is why every node of a network must use the same setting.

use serde::{Deserialize, Serialize};

pub const MIN_BLOCK_TIME_MS: u64 = 250;
pub const MAX_BLOCK_TIME_MS: u64 = 600_000;
pub const MAX_BATCH_WINDOW_MS: u64 = 60_000;
pub const MAX_HEARTBEAT_SECS: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProductionMode {
    #[default]
    OnDemand,
    Interval,
}

impl ProductionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ProductionMode::OnDemand => "on_demand",
            ProductionMode::Interval => "interval",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BlockProduction {
    pub mode: ProductionMode,
    /// Minimum gap between blocks (`on_demand`) or the block time (`interval`).
    pub block_time_ms: u64,
    /// `on_demand`: wait this long after the first pending transaction.
    pub batch_window_ms: u64,
    /// `on_demand`: produce an empty block after this much idle time (0 = never).
    pub heartbeat_secs: u64,
}

impl Default for BlockProduction {
    fn default() -> Self {
        Self {
            mode: ProductionMode::OnDemand,
            block_time_ms: 2_000,
            batch_window_ms: 500,
            heartbeat_secs: 300,
        }
    }
}

impl BlockProduction {
    pub fn interval(block_time_ms: u64) -> Self {
        Self {
            mode: ProductionMode::Interval,
            block_time_ms,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(MIN_BLOCK_TIME_MS..=MAX_BLOCK_TIME_MS).contains(&self.block_time_ms) {
            return Err(format!(
                "block_time_ms must be between {MIN_BLOCK_TIME_MS} and {MAX_BLOCK_TIME_MS}"
            ));
        }
        if self.batch_window_ms > MAX_BATCH_WINDOW_MS {
            return Err(format!("batch_window_ms must be at most {MAX_BATCH_WINDOW_MS}"));
        }
        if self.heartbeat_secs > MAX_HEARTBEAT_SECS {
            return Err(format!("heartbeat_secs must be at most {MAX_HEARTBEAT_SECS}"));
        }
        if self.heartbeat_secs != 0 && self.heartbeat_secs * 1_000 < self.block_time_ms {
            return Err("heartbeat_secs must be 0 or at least the block time".into());
        }
        Ok(())
    }

    /// How long the scheduled producer has before the next authority may
    /// produce instead: three block times, at least 3 seconds.
    pub fn grace_ms(&self) -> u64 {
        (self.block_time_ms * 3).max(3_000)
    }

    /// One line for logs and hashes: `on_demand 2000 500 300`.
    pub fn canonical(&self) -> String {
        format!(
            "{} {} {} {}",
            self.mode.as_str(),
            self.block_time_ms,
            self.batch_window_ms,
            self.heartbeat_secs
        )
    }

    /// When the next block becomes due (unix ms), or `None` if there is
    /// nothing to produce yet (`on_demand` with no work and no heartbeat).
    pub fn due_at_ms(&self, work: &PendingWork) -> Option<u64> {
        let earliest = work.parent_ms.saturating_add(self.block_time_ms);
        match self.mode {
            ProductionMode::Interval => Some(earliest),
            ProductionMode::OnDemand => {
                let mut due: Option<u64> = None;
                let mut consider = |t: u64| due = Some(due.map_or(t, |d| d.min(t)));
                if let Some(since) = work.transactions_since_ms {
                    consider(since.saturating_add(self.batch_window_ms));
                }
                if let Some(since) = work.system_since_ms {
                    consider(since);
                }
                if self.heartbeat_secs > 0 {
                    consider(work.parent_ms.saturating_add(self.heartbeat_secs * 1_000));
                }
                due.map(|d| d.max(earliest))
            }
        }
    }
}

/// What a node knows when deciding whether to produce.
#[derive(Debug, Clone, Copy, Default)]
pub struct PendingWork {
    /// The parent block's time (unix ms). For the genesis block, the node's
    /// start time, since the genesis timestamp is 0.
    pub parent_ms: u64,
    /// When the oldest pending transaction arrived here.
    pub transactions_since_ms: Option<u64>,
    /// When protocol system transactions (reward settlement) became due.
    pub system_since_ms: Option<u64>,
}

/// The authority whose turn it is, `waited_ms` after the block became due.
/// `authorities` must be sorted.
pub fn producer_for(authorities: &[String], height: u64, waited_ms: u64, grace_ms: u64) -> Option<&str> {
    if authorities.is_empty() {
        return None;
    }
    let offset = waited_ms / grace_ms.max(1);
    let index = height.wrapping_add(offset) % authorities.len() as u64;
    Some(authorities[index as usize].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(parent_ms: u64, tx: Option<u64>, sys: Option<u64>) -> PendingWork {
        PendingWork {
            parent_ms,
            transactions_since_ms: tx,
            system_since_ms: sys,
        }
    }

    #[test]
    fn interval_is_due_one_block_time_after_the_parent() {
        let p = BlockProduction::interval(2_000);
        assert_eq!(p.due_at_ms(&work(10_000, None, None)), Some(12_000));
    }

    #[test]
    fn on_demand_idles_until_work_or_heartbeat() {
        let p = BlockProduction {
            heartbeat_secs: 0,
            ..BlockProduction::default()
        };
        assert_eq!(p.due_at_ms(&work(10_000, None, None)), None);

        let p = BlockProduction::default(); // heartbeat 300 s
        assert_eq!(p.due_at_ms(&work(10_000, None, None)), Some(310_000));
    }

    #[test]
    fn on_demand_batches_transactions_but_keeps_the_minimum_gap() {
        let p = BlockProduction::default(); // 2000 ms gap, 500 ms batch
        // A transaction long after the parent: due after the batch window.
        assert_eq!(p.due_at_ms(&work(10_000, Some(20_000), None)), Some(20_500));
        // A transaction right after the parent: the minimum gap wins.
        assert_eq!(p.due_at_ms(&work(10_000, Some(10_100), None)), Some(12_000));
        // Due system work does not wait for a batch window.
        assert_eq!(p.due_at_ms(&work(10_000, None, Some(30_000))), Some(30_000));
        assert_eq!(p.due_at_ms(&work(10_000, Some(29_000), Some(30_000))), Some(29_500));
    }

    #[test]
    fn turns_rotate_by_height_and_pass_on_after_grace() {
        let a: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        assert_eq!(producer_for(&a, 5, 0, 6_000), Some("b"));
        assert_eq!(producer_for(&a, 5, 5_999, 6_000), Some("b"));
        assert_eq!(producer_for(&a, 5, 6_000, 6_000), Some("c"));
        assert_eq!(producer_for(&a, 5, 12_000, 6_000), Some("d"));
        assert_eq!(producer_for(&[], 5, 0, 6_000), None);
    }

    #[test]
    fn validation_bounds() {
        assert!(BlockProduction::default().validate().is_ok());
        assert!(BlockProduction::interval(100).validate().is_err());
        let bad = BlockProduction {
            heartbeat_secs: 1,
            block_time_ms: 5_000,
            ..BlockProduction::default()
        };
        assert!(bad.validate().is_err());
        let json = r#"{"mode":"interval","block_time_ms":1000}"#;
        let parsed: BlockProduction = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.mode, ProductionMode::Interval);
        assert_eq!(parsed.batch_window_ms, 500);
        assert!(serde_json::from_str::<BlockProduction>(r#"{"mode":"on_demand","typo":1}"#).is_err());
    }
}
