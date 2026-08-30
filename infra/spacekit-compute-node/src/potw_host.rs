//! Host integration for Proof of Tangible Works (PoTW) awards.
//!
//! Wraps the pure [`PoTWAccumulator`](crate::potw::PoTWAccumulator) verifier with
//! the node-facing surface: config loading, a shared (locked) accumulator, and
//! the `enabled` gate. The actual crediting of a recipient lives on
//! [`SwtchvmNode::potw_award`](crate::spacekitvm::SwtchvmNode::potw_award), which
//! moves spendable ASTRA on the **native** ledger — the same ledger the faucet
//! credits and settlement moves — so a PoTW award lands in the recipient's
//! wallet balance directly.
//!
//! Emission discipline for awards is enforced here, not by minting: the
//! reviewer quorum (M-of-N SLH-DSA), the per-work cap, the per-epoch budget, and
//! the permanent `work_id` replay guard together bound how much the system can
//! award. Set `epoch_budget` to the protocol's per-epoch emission schedule so
//! PoTW awards can never outrun supply.
//!
//! Wired from `standalone.rs` when `[compute.potw_config] enabled = true`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::potw::{AwardInstruction, PoTWAccumulator, PoTWConfig, PoTWError, PoTWReceipt};

/// Node configuration for the PoTW award host. Budgets are decimal strings so
/// they survive both TOML (no u128) and JSON config without precision loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoTWHostConfig {
    /// Master switch. When false (default) the node accepts no PoTW awards.
    #[serde(default)]
    pub enabled: bool,
    /// Allow-listed reviewer SLH-DSA public keys, hex-encoded.
    #[serde(default)]
    pub reviewers: Vec<String>,
    /// Quorum size `M`: distinct valid reviewer signatures required per award.
    #[serde(default)]
    pub threshold: u64,
    /// Maximum total uASTRA awardable within a single epoch (decimal string).
    #[serde(default = "default_zero")]
    pub epoch_budget: String,
    /// Maximum uASTRA for any single award (decimal string).
    #[serde(default = "default_zero")]
    pub per_work_cap: String,
    /// Optional path for durable PoTW state (per-epoch spend + replay set).
    /// Relative paths resolve against the node's working directory. When unset,
    /// state is kept only in memory (tests / ephemeral nodes).
    #[serde(default)]
    pub state_path: Option<String>,
}

fn default_zero() -> String {
    "0".to_string()
}

impl Default for PoTWHostConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            reviewers: Vec::new(),
            threshold: 0,
            epoch_budget: default_zero(),
            per_work_cap: default_zero(),
            state_path: None,
        }
    }
}

impl PoTWHostConfig {
    fn to_potw_config(&self) -> Result<PoTWConfig> {
        Ok(PoTWConfig {
            reviewers: self.reviewers.clone(),
            threshold: self.threshold,
            epoch_budget: self
                .epoch_budget
                .trim()
                .parse::<u128>()
                .map_err(|e| anyhow!("invalid potw epoch_budget {:?}: {e}", self.epoch_budget))?,
            per_work_cap: self
                .per_work_cap
                .trim()
                .parse::<u128>()
                .map_err(|e| anyhow!("invalid potw per_work_cap {:?}: {e}", self.per_work_cap))?,
        })
    }
}

/// Shared PoTW award host: a locked accumulator plus its config.
pub struct PoTWHost {
    config: PoTWHostConfig,
    accumulator: RwLock<PoTWAccumulator>,
}

impl PoTWHost {
    /// Build the host, loading any persisted state from `config.state_path`.
    pub fn new(config: PoTWHostConfig) -> Result<Arc<Self>> {
        let potw_config = config.to_potw_config()?;
        let accumulator = match &config.state_path {
            Some(p) if !p.trim().is_empty() => PoTWAccumulator::load(potw_config, PathBuf::from(p))
                .map_err(|e| anyhow!("load PoTW state: {e}"))?,
            _ => PoTWAccumulator::new(potw_config),
        };
        Ok(Arc::new(Self {
            config,
            accumulator: RwLock::new(accumulator),
        }))
    }

    /// The host accepts awards only when explicitly enabled AND a real quorum is
    /// configured (`threshold >= 1`). A misconfigured host (threshold 0) is
    /// fail-closed: it never authorizes an award.
    pub fn enabled(&self) -> bool {
        self.config.enabled && self.config.threshold >= 1
    }

    pub fn config(&self) -> &PoTWHostConfig {
        &self.config
    }

    /// Verify a reviewer-quorum receipt and, on success, commit its budget/replay
    /// state, returning the award to credit. Commit precedes the caller's credit
    /// (mirrors `SraHost`): a returned instruction has already consumed epoch
    /// budget and the `work_id` replay slot, so the caller must treat a failed
    /// downstream credit as needing reconciliation rather than resubmitting.
    pub async fn authorize(&self, receipt: &PoTWReceipt) -> Result<AwardInstruction, PoTWError> {
        self.accumulator.write().await.verify_and_award(receipt)
    }

    /// uASTRA already awarded in `epoch` (audit / metrics).
    pub async fn epoch_spent(&self, epoch: u64) -> u128 {
        self.accumulator.read().await.epoch_spent(epoch)
    }
}
