//! Host bridge for the Treasury contract.
//!
//! The Treasury WASM contract governs an M-of-N approval process and, when a
//! spend reaches threshold, marks it executed and emits `treasury.disbursed`.
//! The contract has **no mint authority** and moves nothing on the spendable
//! ledger by itself — it is the governance record. This bridge mirrors an
//! already-executed, already-approved disbursement onto the **native** ledger
//! (the same spendable balances the faucet credits and settlement moves),
//! exactly once.
//!
//! # Trust model
//!
//! The bridge does not trust its caller. Given a `spend_id`, it reads the
//! Treasury contract's own state (`GET_PROPOSAL`) and acts only if the contract
//! reports `executed == 1`. The M-of-N authorization therefore lives entirely
//! in the contract; the bridge is a faithful mirror with a replay guard so a
//! single executed spend can never be paid out twice.
//!
//! # Custody
//!
//! Funds move FROM the configured `custodian_address` — by default the Treasury
//! contract's own address (`system_contracts::TREASURY`), which is both the
//! contract and the native holder of the pool. Fund that address to match the
//! balance the contract is INIT'd with, and the two stay in lockstep as spends
//! are approved (contract-internal debit) and bridged (native debit).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::spacekitvm::genesis_node::system_contracts;

/// Node configuration for the Treasury disbursement bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreasuryHostConfig {
    /// Master switch. When false (default) `/treasury/disburse` is inert.
    #[serde(default)]
    pub enabled: bool,
    /// Treasury WASM contract address (hex, 20 bytes).
    #[serde(default = "default_treasury_contract")]
    pub treasury_contract: String,
    /// Native address the pool is held at and disbursed FROM. Defaults to the
    /// Treasury contract address itself. Must be funded to match the contract's
    /// INIT balance.
    #[serde(default = "default_custodian")]
    pub custodian_address: String,
    /// Optional path for the durable set of already-bridged spend ids.
    #[serde(default)]
    pub state_path: Option<String>,
}

fn default_treasury_contract() -> String {
    system_contracts::TREASURY.to_string()
}

fn default_custodian() -> String {
    system_contracts::TREASURY.to_string()
}

impl Default for TreasuryHostConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            treasury_contract: default_treasury_contract(),
            custodian_address: default_custodian(),
            state_path: None,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct BridgeState {
    /// hex(spend_id) of every disbursement already mirrored to the native ledger.
    bridged: BTreeSet<String>,
}

/// Host bridge: config plus the durable set of spend ids already paid out.
pub struct TreasuryHost {
    config: TreasuryHostConfig,
    state: RwLock<BridgeState>,
    path: Option<PathBuf>,
}

impl TreasuryHost {
    pub fn new(config: TreasuryHostConfig) -> Result<Arc<Self>> {
        let (state, path) = match &config.state_path {
            Some(p) if !p.trim().is_empty() => {
                let path = PathBuf::from(p);
                let state = if path.exists() {
                    let bytes = std::fs::read(&path)
                        .map_err(|e| anyhow!("read treasury bridge state: {e}"))?;
                    serde_json::from_slice(&bytes)
                        .map_err(|e| anyhow!("parse treasury bridge state: {e}"))?
                } else {
                    BridgeState::default()
                };
                (state, Some(path))
            }
            _ => (BridgeState::default(), None),
        };
        Ok(Arc::new(Self {
            config,
            state: RwLock::new(state),
            path,
        }))
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn config(&self) -> &TreasuryHostConfig {
        &self.config
    }

    pub async fn is_bridged(&self, spend_id_hex: &str) -> bool {
        self.state.read().await.bridged.contains(spend_id_hex)
    }

    /// Atomically claim a spend id for bridging: inserts it and persists,
    /// returning `true` only for the caller that newly claimed it. Two
    /// concurrent disburse calls for the same spend can never both win.
    pub async fn reserve(&self, spend_id_hex: &str) -> Result<bool> {
        let mut state = self.state.write().await;
        if state.bridged.contains(spend_id_hex) {
            return Ok(false);
        }
        state.bridged.insert(spend_id_hex.to_string());
        self.persist(&state)?;
        Ok(true)
    }

    fn persist(&self, state: &BridgeState) -> Result<()> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let json = serde_json::to_vec_pretty(state)
            .map_err(|e| anyhow!("serialize treasury bridge state: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &json).map_err(|e| anyhow!("write treasury bridge state: {e}"))?;
        std::fs::rename(&tmp, path).map_err(|e| anyhow!("rename treasury bridge state: {e}"))?;
        Ok(())
    }
}
