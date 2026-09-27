//! Host integration for the Service Reward Accumulator (SRA).
//!
//! Rewards are part of consensus: at the start of every block the producer
//! asks [`SraHost::plan_block`] for the protocol *system transactions* (INIT,
//! END_POA, CREDIT / CREDIT_LOCKED) and executes them first; importers derive
//! the same plan from their own chain state and reject a block whose leading
//! system transactions differ. The plan depends only on chain history (the
//! SRA epoch accumulator, rebuilt from blocks on restart), on-chain contract
//! state, and the governance lock policy, so every node computes the same one.
//!
//! Spec: `spacekit-tokenomics/Service_Reward_Accumulator_Spec.md`

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use spacekit_service_rewards::{
    address_to_did_hash, classify_log_topic, encode_credit, encode_credit_locked, encode_end_poa,
    encode_init, lock_recipient_hashes, treasury_did_hash, CreditInstruction, ServiceCategory,
    ServiceRewardEvent, SraState,
};
use tokio::sync::RwLock;

use crate::spacekitvm::swtchvm_node::{SwtchvmState, TransactionSignature};
use crate::spacekitvm::{
    genesis_node::system_contracts, SwtchvmAddress, SwtchvmBlock, SwtchvmLog,
    SwtchvmReceipt, SwtchvmTransaction,
};

/// Gas limit for each SRA system transaction. System transactions are not
/// counted against the block gas limit.
pub const SYSTEM_TX_GAS_LIMIT: u128 = 1_000_000;

const KEY_INITIALIZED: &[u8] = b"astra_rewards.is_initialized";
const KEY_PHASE: &[u8] = b"astra_rewards.phase";

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
    /// Operator DIDs affiliated with SWTCH Labs or the SpaceKit Foundation.
    /// Their proof-of-authority credits are locked like the authorities'
    /// (Tokenomics §1.11). Also read from `SPACEKIT_AFFILIATED_OPERATOR_DIDS`
    /// (comma-separated).
    #[serde(default)]
    pub affiliated_operator_dids: Vec<String>,
    /// Epoch length in seconds (default 86,400). Also `SPACEKIT_SRA_EPOCH_SECS`.
    /// Test networks shorten it so settlement can be observed quickly.
    #[serde(default)]
    pub epoch_secs: Option<u64>,
}

/// Which recipients' credits AstraRewards must lock, derived from governance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RewardLockPolicy {
    /// The network is still in proof of authority.
    pub proof_of_authority: bool,
    /// Recipient keys (see `lock_recipient_hashes`) whose PoA credits are locked.
    pub recipients: BTreeSet<[u8; 32]>,
}

impl RewardLockPolicy {
    /// Authorities plus affiliated operators, each under every key SRA may credit.
    pub fn from_dids<'a>(proof_of_authority: bool, dids: impl IntoIterator<Item = &'a str>) -> Self {
        let recipients = dids
            .into_iter()
            .filter(|d| !d.trim().is_empty())
            .flat_map(lock_recipient_hashes)
            .collect();
        Self {
            proof_of_authority,
            recipients,
        }
    }
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
            affiliated_operator_dids: Vec::new(),
            epoch_secs: None,
        }
    }
}

impl SraHostConfig {
    /// Genesis time for epoch numbering: `SPACEKIT_SRA_GENESIS_TS` overrides the config.
    pub fn resolved_genesis_ts(&self) -> u64 {
        std::env::var("SPACEKIT_SRA_GENESIS_TS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(self.genesis_timestamp_secs)
    }

    pub fn resolved_epoch_secs(&self) -> u64 {
        std::env::var("SPACEKIT_SRA_EPOCH_SECS")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .or(self.epoch_secs)
            .unwrap_or(86_400)
            .max(1)
    }

    /// Configured affiliated DIDs plus `SPACEKIT_AFFILIATED_OPERATOR_DIDS`.
    pub fn affiliated_dids(&self) -> Vec<String> {
        let mut dids = self.affiliated_operator_dids.clone();
        if let Ok(raw) = std::env::var("SPACEKIT_AFFILIATED_OPERATOR_DIDS") {
            dids.extend(raw.split(',').map(|d| d.trim().to_string()).filter(|d| !d.is_empty()));
        }
        dids.sort();
        dids.dedup();
        dids
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SraBlockCredits {
    pub block_number: u64,
    pub credits: Vec<SraCreditRecord>,
    /// System transactions that executed successfully.
    pub onchain_applied: usize,
    /// System transactions that reverted.
    pub onchain_failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SraCreditRecord {
    pub recipient_did_hash_hex: String,
    pub amount_wei: String,
    pub log_event_hash_hex: String,
    /// Credited to the locked balance (proof-of-authority reward lock).
    #[serde(default)]
    pub locked: bool,
    pub onchain_ok: Option<bool>,
}

/// What the SRA contributes to one block.
#[derive(Debug, Clone)]
pub struct SraBlockPlan {
    /// Leading system transactions, in execution order.
    pub system_txs: Vec<SwtchvmTransaction>,
    /// Credits settled at this block (epoch boundary), with their lock flag.
    pub credits: Vec<(CreditInstruction, bool)>,
    state_after: SraState,
}

pub struct SraHost {
    config: SraHostConfig,
    /// Epoch accumulator. Advanced only by [`commit_block`](Self::commit_block),
    /// so a rejected block leaves it untouched.
    state: std::sync::Mutex<SraState>,
    pub credits_by_block: RwLock<Vec<SraBlockCredits>>,
    /// Desired lock policy, set from governance.
    lock_policy: std::sync::RwLock<RewardLockPolicy>,
}

impl SraHost {
    pub fn new(config: SraHostConfig) -> Arc<Self> {
        Arc::new(Self {
            config: config.clone(),
            state: std::sync::Mutex::new(SraState::with_epoch_secs(
                config.resolved_genesis_ts(),
                config.resolved_epoch_secs(),
            )),
            credits_by_block: RwLock::new(Vec::new()),
            // Until governance says otherwise, treat the network as proof of
            // authority and lock the affiliated operators.
            lock_policy: std::sync::RwLock::new(RewardLockPolicy::from_dids(
                true,
                config.affiliated_dids().iter().map(String::as_str),
            )),
        })
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Update which recipients are locked: the current authorities and the
    /// affiliated operators while `proof_of_authority` is true.
    ///
    /// The policy feeds consensus: nodes whose governance view differs at a
    /// settlement block compute different system transactions and reject each
    /// other's block until their governance state converges.
    pub fn set_lock_policy<'a>(
        &self,
        proof_of_authority: bool,
        authority_dids: impl IntoIterator<Item = &'a str>,
    ) {
        let mut dids: Vec<String> = authority_dids.into_iter().map(str::to_string).collect();
        dids.extend(self.config.affiliated_dids());
        let policy =
            RewardLockPolicy::from_dids(proof_of_authority, dids.iter().map(String::as_str));
        let mut guard = self
            .lock_policy
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *guard != policy {
            tracing::info!(
                proof_of_authority,
                recipients = policy.recipients.len(),
                "SRA reward lock policy updated"
            );
            *guard = policy;
        }
    }

    pub fn lock_policy(&self) -> RewardLockPolicy {
        self.lock_policy
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// The lock policy in force at this point of the chain: governance's
    /// mode and authorities from chain state, plus the affiliated operators.
    /// Chains without a PoA genesis have no PoA phase.
    pub fn lock_policy_for(&self, world: &SwtchvmState) -> RewardLockPolicy {
        let (poa, mut dids) = crate::chain_consensus::lock_policy(world).unwrap_or((false, Vec::new()));
        dids.extend(self.config.affiliated_dids());
        RewardLockPolicy::from_dids(poa, dids.iter().map(String::as_str))
    }

    /// Copy of the epoch accumulator (undo record for fork choice).
    pub fn snapshot(&self) -> SraState {
        self.sra_state()
    }

    /// Restore the accumulator from before `block_number` and forget that
    /// block's settlement record (the block was rolled back).
    pub async fn restore(&self, before: SraState, block_number: u64) {
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = before;
        self.credits_by_block
            .write()
            .await
            .retain(|b| b.block_number < block_number);
    }

    fn sra_state(&self) -> SraState {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// True for a transaction shaped like an SRA system transaction. Such
    /// transactions are only valid as the leading transactions of a block that
    /// match [`plan_block`](Self::plan_block).
    pub fn is_system_tx(&self, tx: &SwtchvmTransaction) -> bool {
        let (Ok(admin), Ok(contract)) = (
            parse_address(&self.config.sra_admin_address),
            parse_address(&self.config.astra_rewards_contract),
        ) else {
            return false;
        };
        tx.from == admin
            && tx.to == Some(contract)
            && tx.signature.r == [0u8; 32]
            && tx.signature.s == [0u8; 32]
    }

    /// Settle any epoch that closed before `block_timestamp` and return the
    /// system transactions the block must start with.
    pub fn plan_block(
        &self,
        world: &SwtchvmState,
        block_number: u64,
        block_timestamp: u64,
    ) -> SraBlockPlan {
        let mut state = self.sra_state();
        let settled = if self.config.enabled {
            state.maybe_advance_epoch(block_timestamp)
        } else {
            Vec::new()
        };
        let policy = self.lock_policy_for(world);
        let credits: Vec<(CreditInstruction, bool)> = settled
            .into_iter()
            .map(|c| {
                let locked = policy.proof_of_authority
                    && policy.recipients.contains(&c.recipient_did_hash);
                (c, locked)
            })
            .collect();

        let mut system_txs = Vec::new();
        if self.config.enabled && self.config.apply_credits_onchain {
            match (
                parse_address(&self.config.astra_rewards_contract),
                parse_address(&self.config.sra_admin_address),
            ) {
                (Ok(contract), Ok(admin)) => {
                    let deployed = world
                        .get_account(&contract)
                        .is_some_and(|a| a.code.is_some());
                    if deployed {
                        let initialized = world
                            .contract_kv
                            .contains_key(&(contract, KEY_INITIALIZED.to_vec()));
                        let phase_is_poa = if initialized {
                            world
                                .contract_kv
                                .get(&(contract, KEY_PHASE.to_vec()))
                                .and_then(|v| v.first().copied())
                                == Some(0)
                        } else {
                            // INIT starts the contract in proof of authority.
                            true
                        };
                        let mut payloads = Vec::new();
                        if !initialized && (!credits.is_empty() || !policy.proof_of_authority) {
                            payloads.push(encode_init(treasury_did_hash()));
                        }
                        let will_be_initialized = initialized || !payloads.is_empty();
                        if will_be_initialized && !policy.proof_of_authority && phase_is_poa {
                            payloads.push(encode_end_poa());
                        }
                        for (c, locked) in &credits {
                            if c.amount_wei == 0 {
                                continue;
                            }
                            payloads.push(if *locked {
                                encode_credit_locked(
                                    c.recipient_did_hash,
                                    c.amount_wei,
                                    c.log_event_hash,
                                )
                            } else {
                                encode_credit(c.recipient_did_hash, c.amount_wei, c.log_event_hash)
                            });
                        }
                        let base_nonce = world.get_account(&admin).map(|a| a.nonce).unwrap_or(0);
                        system_txs = payloads
                            .into_iter()
                            .enumerate()
                            .map(|(i, data)| SwtchvmTransaction {
                                from: admin,
                                to: Some(contract),
                                data,
                                gas_limit: SYSTEM_TX_GAS_LIMIT,
                                gas_price: 0,
                                value: 0,
                                nonce: base_nonce + i as u64,
                                signature: TransactionSignature {
                                    v: 0,
                                    r: [0u8; 32],
                                    s: [0u8; 32],
                                },
                            })
                            .collect();
                    } else if !credits.is_empty() {
                        tracing::warn!(
                            block_number,
                            contract = %self.config.astra_rewards_contract,
                            "AstraRewards is not deployed; settled credits are recorded but not applied"
                        );
                    }
                }
                _ => tracing::warn!("invalid SRA contract or admin address; credits not applied"),
            }
        }
        SraBlockPlan {
            system_txs,
            credits,
            state_after: state,
        }
    }

    /// Adopt a block: keep the plan's settled state and record the block's
    /// service events (system transactions excluded) for the current epoch.
    pub async fn commit_block(
        &self,
        plan: SraBlockPlan,
        block_number: u64,
        proposer_did: Option<&str>,
        transactions: &[SwtchvmTransaction],
        receipts: &[SwtchvmReceipt],
    ) {
        let system = plan.system_txs.len();
        let user_txs = transactions.get(system..).unwrap_or(&[]);
        let user_receipts = receipts.get(system..).unwrap_or(&[]);
        let system_receipts = receipts.get(..system).unwrap_or(&[]);
        let mut state = plan.state_after;
        if self.config.enabled {
            let mut events = extract_events_from_logs(block_number, user_receipts);
            events.extend(events_from_tx_gas(block_number, user_txs, user_receipts));
            events.extend(proposer_event(block_number, proposer_did));
            state.record_events(&events);
        }
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;

        if !self.config.enabled {
            return;
        }
        let applied = system_receipts.iter().filter(|r| r.success).count();
        let failed = system_receipts.len() - applied;
        // Credit transactions are the trailing system transactions.
        let credit_receipts = &system_receipts[system_receipts.len().saturating_sub(
            plan.credits.iter().filter(|(c, _)| c.amount_wei > 0).count(),
        )..];
        let mut receipt_iter = credit_receipts.iter();
        let records: Vec<SraCreditRecord> = plan
            .credits
            .iter()
            .map(|(c, locked)| SraCreditRecord {
                recipient_did_hash_hex: hex::encode(c.recipient_did_hash),
                amount_wei: c.amount_wei.to_string(),
                log_event_hash_hex: hex::encode(c.log_event_hash),
                locked: *locked,
                onchain_ok: if c.amount_wei > 0 && !plan.system_txs.is_empty() {
                    receipt_iter.next().map(|r| r.success)
                } else {
                    None
                },
            })
            .collect();
        if !records.is_empty() {
            tracing::info!(
                block_number,
                credits = records.len(),
                locked = records.iter().filter(|r| r.locked).count(),
                applied,
                failed,
                "SRA settled epoch rewards"
            );
        }
        let mut log = self.credits_by_block.write().await;
        log.push(SraBlockCredits {
            block_number,
            credits: records,
            onchain_applied: applied,
            onchain_failed: failed,
        });
        let len = log.len();
        if len > 4096 {
            log.drain(..len - 4096);
        }
    }

    /// Read-only summary for status endpoints: contract phase, recent
    /// settlements, and the spendable/locked balances of their recipients
    /// (read straight from contract storage, no transaction).
    pub async fn status_json(&self, world: &SwtchvmState) -> serde_json::Value {
        let contract = parse_address(&self.config.astra_rewards_contract).ok();
        let kv_u128 = |key: String| -> Option<u128> {
            let contract = contract?;
            let raw = world.contract_kv.get(&(contract, key.into_bytes()))?;
            let bytes: [u8; 16] = raw.get(..16)?.try_into().ok()?;
            Some(u128::from_le_bytes(bytes))
        };
        let initialized = contract.is_some_and(|c| {
            world.contract_kv.contains_key(&(c, KEY_INITIALIZED.to_vec()))
        });
        let phase = contract
            .and_then(|c| world.contract_kv.get(&(c, KEY_PHASE.to_vec())).cloned())
            .and_then(|v| v.first().copied())
            .map(|p| if p == 0 { "proof_of_authority" } else { "proof_of_stake" });
        let log = self.credits_by_block.read().await;
        let recent: Vec<serde_json::Value> = log
            .iter()
            .rev()
            .filter(|b| !b.credits.is_empty())
            .take(5)
            .map(|b| {
                serde_json::json!({
                    "block_number": b.block_number,
                    "applied": b.onchain_applied,
                    "failed": b.onchain_failed,
                    "credits": b.credits.iter().map(|c| serde_json::json!({
                        "recipient": c.recipient_did_hash_hex,
                        "amount_wei": c.amount_wei,
                        "locked": c.locked,
                        "onchain_ok": c.onchain_ok,
                        "balance_wei": kv_u128(format!("astra_rewards.balance.{}", c.recipient_did_hash_hex)).map(|v| v.to_string()),
                        "locked_wei": kv_u128(format!("astra_rewards.locked.{}", c.recipient_did_hash_hex)).map(|v| v.to_string()),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let state = self.sra_state();
        serde_json::json!({
            "enabled": self.config.enabled,
            "epoch_secs": state.epoch_secs,
            "epoch_index": state.epoch_index,
            "lock_policy": {
                "proof_of_authority": self.lock_policy().proof_of_authority,
                "locked_recipients": self.lock_policy().recipients.len(),
            },
            "contract": {
                "address": self.config.astra_rewards_contract,
                "initialized": initialized,
                "phase": phase,
                "total_emitted_wei": kv_u128("astra_rewards.total_emitted".to_string()).map(|v| v.to_string()),
            },
            "recent_settlements": recent,
        })
    }

    /// Rebuild the epoch accumulator from stored blocks after a restart. Only
    /// the accumulator is replayed; world state already contains the effects.
    pub fn rebuild_from_blocks<'a>(&self, blocks: impl IntoIterator<Item = &'a SwtchvmBlock>) {
        let mut state = SraState::with_epoch_secs(
            self.config.resolved_genesis_ts(),
            self.config.resolved_epoch_secs(),
        );
        let mut replayed = 0u64;
        for block in blocks {
            if block.number == 0 {
                continue;
            }
            if !self.config.enabled {
                continue;
            }
            state.maybe_advance_epoch(block.timestamp);
            let system = block
                .transactions
                .iter()
                .take_while(|tx| self.is_system_tx(tx))
                .count();
            let user_txs = block.transactions.get(system..).unwrap_or(&[]);
            let user_receipts = block.receipts.get(system..).unwrap_or(&[]);
            let mut events = extract_events_from_logs(block.number, user_receipts);
            events.extend(events_from_tx_gas(block.number, user_txs, user_receipts));
            events.extend(proposer_event(block.number, block.proposer_did.as_deref()));
            state.record_events(&events);
            replayed += 1;
        }
        *self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
        if replayed > 0 {
            tracing::info!(blocks = replayed, "SRA epoch accumulator rebuilt from chain");
        }
    }
}

fn parse_address(hex_addr: &str) -> Result<SwtchvmAddress> {
    SwtchvmAddress::from_hex(hex_addr).map_err(|e| anyhow!("invalid address {}: {}", hex_addr, e))
}

/// Consensus work: one unit to the block's producer. This is how validators
/// earn the consensus share of emission (locked for authorities during PoA),
/// which they later stake.
fn proposer_event(block_number: u64, proposer_did: Option<&str>) -> Option<ServiceRewardEvent> {
    let did = proposer_did?.trim();
    if did.is_empty() {
        return None;
    }
    let mut log_hash = [0u8; 32];
    log_hash[0..8].copy_from_slice(&block_number.to_le_bytes());
    log_hash[8..16].copy_from_slice(b"proposer");
    Some(ServiceRewardEvent {
        operator_did_hash: spacekit_service_rewards::hash_did_bytes(did.as_bytes()),
        category: ServiceCategory::Consensus,
        resource_units: 1,
        log_event_hash: log_hash,
        approved: true,
    })
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
        // gas_used is u128 (16 bytes); a [16..24] slice panicked on every block.
        log_hash[16..32].copy_from_slice(&receipt.gas_used.to_le_bytes());
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

#[cfg(test)]
mod lock_policy_tests {
    use super::*;

    #[test]
    fn policy_covers_authorities_and_affiliated_under_both_keys() {
        let host = SraHost::new(SraHostConfig {
            affiliated_operator_dids: vec![
                "did:spacekit:testnet:1111111111111111111111111111111111111111".into(),
            ],
            ..SraHostConfig::default()
        });
        // Before governance syncs: PoA, affiliated operators locked.
        let initial = host.lock_policy();
        assert!(initial.proof_of_authority);
        assert_eq!(initial.recipients.len(), 2);

        host.set_lock_policy(
            true,
            ["did:spacekit:testnet:2222222222222222222222222222222222222222"],
        );
        let p = host.lock_policy();
        assert_eq!(p.recipients.len(), 4);
        let mut padded = [0u8; 32];
        padded[12..].copy_from_slice(&[0x22; 20]);
        assert!(p.recipients.contains(&padded));

        host.set_lock_policy(false, std::iter::empty());
        assert!(!host.lock_policy().proof_of_authority);
    }
}
