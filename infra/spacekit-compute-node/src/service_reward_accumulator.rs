//! Host integration for the Service Reward Accumulator (SRA).
//!
//! Wired from `SwtchvmNode::mine_block` when `SraHostConfig::enabled` is true.
//! Spec: `spacekit-tokenomics/Service_Reward_Accumulator_Spec.md`

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use spacekit_service_rewards::{
    address_to_did_hash, classify_log_topic, encode_credit, encode_get_total_emitted, encode_init,
    treasury_did_hash, CreditInstruction, ServiceCategory, ServiceRewardEvent, SraState,
};
use tokio::sync::RwLock;

use crate::spacekitvm::{
    genesis_node::system_contracts, SwtchvmAddress, SwtchvmContext, SwtchvmLog, SwtchvmReceipt,
    SwtchvmRuntime, SwtchvmTransaction,
};

/// Configuration for SRA block hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SraHostConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_genesis_ts")]
    pub genesis_timestamp_secs: u64,
    /// Submit CREDIT ops to AstraRewards after computing credits.
    #[serde(default = "default_apply_onchain")]
    pub apply_credits_onchain: bool,
    /// AstraRewards contract address (hex). Defaults to system `0x…0003`.
    #[serde(default = "default_astra_rewards_contract")]
    pub astra_rewards_contract: String,
    /// Caller address for CREDIT (must match AstraRewards admin / INIT deployer).
    #[serde(default = "default_sra_admin")]
    pub sra_admin_address: String,
}

fn default_genesis_ts() -> u64 {
    1_700_000_000
}

fn default_apply_onchain() -> bool {
    true
}

fn default_astra_rewards_contract() -> String {
    system_contracts::ASTRA_REWARDS.to_string()
}

fn default_sra_admin() -> String {
    system_contracts::FAUCET.to_string()
}

impl Default for SraHostConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            genesis_timestamp_secs: default_genesis_ts(),
            apply_credits_onchain: default_apply_onchain(),
            astra_rewards_contract: default_astra_rewards_contract(),
            sra_admin_address: default_sra_admin(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SraBlockCredits {
    pub block_number: u64,
    pub credits: Vec<SraCreditRecord>,
    pub onchain_applied: usize,
    pub onchain_failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SraCreditRecord {
    pub recipient_did_hash_hex: String,
    pub amount_wei: String,
    pub log_event_hash_hex: String,
    pub onchain_ok: Option<bool>,
}

pub struct SraHost {
    config: SraHostConfig,
    state: RwLock<SraState>,
    astra_rewards_initialized: RwLock<bool>,
    pub credits_by_block: RwLock<Vec<SraBlockCredits>>,
}

impl SraHost {
    pub fn new(config: SraHostConfig) -> Arc<Self> {
        Arc::new(Self {
            config: config.clone(),
            state: RwLock::new(SraState::new(config.genesis_timestamp_secs)),
            astra_rewards_initialized: RwLock::new(false),
            credits_by_block: RwLock::new(Vec::new()),
        })
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    pub async fn on_block_finalized(
        &self,
        runtime: &SwtchvmRuntime,
        block_number: u64,
        block_timestamp: u64,
        transactions: &[SwtchvmTransaction],
        receipts: &[SwtchvmReceipt],
    ) -> Result<Vec<CreditInstruction>> {
        if !self.config.enabled {
            return Ok(Vec::new());
        }

        let mut events = extract_events_from_logs(block_number, receipts);
        events.extend(events_from_tx_gas(block_number, transactions, receipts));

        let mut state = self.state.write().await;
        state.maybe_advance_epoch(block_timestamp);
        let credits = state.process_events(&events);
        drop(state);

        let mut onchain_applied = 0usize;
        let mut onchain_failed = 0usize;
        let mut records = Vec::with_capacity(credits.len());

        if self.config.apply_credits_onchain && !credits.is_empty() {
            if let Err(e) = self
                .ensure_astra_rewards_initialized(runtime, block_number, block_timestamp)
                .await
            {
                tracing::warn!(error = %e, "AstraRewards INIT skipped or failed");
            }
        }

        for credit in &credits {
            let mut onchain_ok = None;
            if self.config.apply_credits_onchain && credit.amount_wei > 0 {
                match self
                    .apply_credit_onchain(runtime, block_number, block_timestamp, credit)
                    .await
                {
                    Ok(()) => {
                        onchain_applied += 1;
                        onchain_ok = Some(true);
                    }
                    Err(e) => {
                        onchain_failed += 1;
                        onchain_ok = Some(false);
                        tracing::warn!(
                            error = %e,
                            block_number,
                            amount = credit.amount_wei,
                            "AstraRewards CREDIT failed"
                        );
                    }
                }
            }
            records.push(SraCreditRecord {
                recipient_did_hash_hex: hex::encode(credit.recipient_did_hash),
                amount_wei: credit.amount_wei.to_string(),
                log_event_hash_hex: hex::encode(credit.log_event_hash),
                onchain_ok,
            });
        }

        self.credits_by_block.write().await.push(SraBlockCredits {
            block_number,
            credits: records,
            onchain_applied,
            onchain_failed,
        });

        if !credits.is_empty() {
            tracing::info!(
                block_number,
                credit_count = credits.len(),
                onchain_applied,
                onchain_failed,
                "SRA processed block credits"
            );
        }

        Ok(credits)
    }

    async fn ensure_astra_rewards_initialized(
        &self,
        runtime: &SwtchvmRuntime,
        block_number: u64,
        block_timestamp: u64,
    ) -> Result<()> {
        if *self.astra_rewards_initialized.read().await {
            return Ok(());
        }

        let contract = parse_address(&self.config.astra_rewards_contract)?;
        let admin = parse_address(&self.config.sra_admin_address)?;

        {
            let state_guard = runtime.get_state();
            let state = state_guard.read().await;
            let account = state.get_account(&contract).ok_or_else(|| {
                anyhow!(
                    "AstraRewards contract not deployed at {}",
                    self.config.astra_rewards_contract
                )
            })?;
            if account.code.is_none() {
                return Err(anyhow!(
                    "no WASM at AstraRewards address {}; build astra-rewards for wasm32-unknown-unknown",
                    self.config.astra_rewards_contract
                ));
            }
        }

        // Probe initialization via GET_TOTAL_EMITTED.
        let probe = runtime
            .call_contract_public(
                &admin,
                &contract,
                &encode_get_total_emitted(),
                protocol_context(&admin, block_number, block_timestamp, 500_000),
            )
            .await;

        if probe.is_ok() {
            *self.astra_rewards_initialized.write().await = true;
            return Ok(());
        }

        let init_payload = encode_init(treasury_did_hash());
        runtime
            .call_contract_public(
                &admin,
                &contract,
                &init_payload,
                protocol_context(&admin, block_number, block_timestamp, 1_000_000),
            )
            .await
            .context("AstraRewards INIT")?;

        *self.astra_rewards_initialized.write().await = true;
        tracing::info!("AstraRewards initialized (treasury credited via INIT)");
        Ok(())
    }

    async fn apply_credit_onchain(
        &self,
        runtime: &SwtchvmRuntime,
        block_number: u64,
        block_timestamp: u64,
        credit: &CreditInstruction,
    ) -> Result<()> {
        let contract = parse_address(&self.config.astra_rewards_contract)?;
        let admin = parse_address(&self.config.sra_admin_address)?;
        let payload = encode_credit(
            credit.recipient_did_hash,
            credit.amount_wei,
            credit.log_event_hash,
        );

        let result = runtime
            .call_contract_public(
                &admin,
                &contract,
                &payload,
                protocol_context(&admin, block_number, block_timestamp, 200_000),
            )
            .await
            .context("AstraRewards CREDIT call")?;

        if !result.success {
            return Err(anyhow!(
                "CREDIT reverted: {}",
                String::from_utf8_lossy(&result.return_data)
            ));
        }
        Ok(())
    }
}

fn protocol_context(
    caller: &SwtchvmAddress,
    block_number: u64,
    block_timestamp: u64,
    gas_limit: u128,
) -> SwtchvmContext {
    SwtchvmContext {
        caller: *caller,
        origin: *caller,
        gas_price: 0,
        gas_limit,
        gas_used: 0,
        block_number,
        block_timestamp,
        value: 0,
    }
}

fn parse_address(hex_addr: &str) -> Result<SwtchvmAddress> {
    SwtchvmAddress::from_hex(hex_addr).map_err(|e| anyhow!("invalid address {}: {}", hex_addr, e))
}

fn events_from_tx_gas(
    block_number: u64,
    transactions: &[SwtchvmTransaction],
    receipts: &[SwtchvmReceipt],
) -> Vec<ServiceRewardEvent> {
    let mut out = Vec::new();
    for (i, (tx, receipt)) in transactions.iter().zip(receipts.iter()).enumerate() {
        if !receipt.success || receipt.gas_used == 0 {
            continue;
        }
        let mut log_hash = [0u8; 32];
        log_hash[0..8].copy_from_slice(&block_number.to_le_bytes());
        log_hash[8..16].copy_from_slice(&(i as u64).to_le_bytes());
        log_hash[16..24].copy_from_slice(&receipt.gas_used.to_le_bytes());
        out.push(ServiceRewardEvent {
            operator_did_hash: address_to_did_hash(tx.from.as_bytes()),
            category: ServiceCategory::Compute,
            resource_units: receipt.gas_used,
            log_event_hash: log_hash,
            approved: true,
        });
    }
    out
}

fn extract_events_from_logs(
    block_number: u64,
    receipts: &[SwtchvmReceipt],
) -> Vec<ServiceRewardEvent> {
    let mut out = Vec::new();
    for (ri, receipt) in receipts.iter().enumerate() {
        if !receipt.success {
            continue;
        }
        for (li, log) in receipt.logs.iter().enumerate() {
            if let Some(ev) = event_from_log(block_number, ri, li, log) {
                out.push(ev);
            }
        }
    }
    out
}

fn event_from_log(
    block_number: u64,
    receipt_index: usize,
    log_index: usize,
    log: &SwtchvmLog,
) -> Option<ServiceRewardEvent> {
    let topic0 = log.topics.first()?;
    let category = classify_log_topic(topic0)?.0;

    let resource_units = read_resource_units(&log.data);

    let mut log_hash = [0u8; 32];
    log_hash[0..8].copy_from_slice(&block_number.to_le_bytes());
    log_hash[8..12].copy_from_slice(&(receipt_index as u32).to_le_bytes());
    log_hash[12..16].copy_from_slice(&(log_index as u32).to_le_bytes());
    log_hash[16..32].copy_from_slice(topic0);

    Some(ServiceRewardEvent {
        operator_did_hash: address_to_did_hash(log.address.as_bytes()),
        category,
        resource_units,
        log_event_hash: log_hash,
        approved: true,
    })
}

fn read_resource_units(data: &[u8]) -> u128 {
    if data.len() >= 16 {
        u128::from_le_bytes(data[0..16].try_into().expect("16 bytes"))
    } else if data.len() >= 8 {
        let mut a = [0u8; 16];
        a[..data.len()].copy_from_slice(data);
        u128::from_le_bytes(a)
    } else {
        1
    }
}
