//! On-chain vault relay (`SpaceKitDepositVault.charge` or `SpaceKitMultiAssetVault.charge`) after EIP-191 verification.
//! Configured via `ROUTEKIT_VAULT_*` env vars; disabled when unset.
//!
//! Set `ROUTEKIT_VAULT_KIND=multi` when `ROUTEKIT_VAULT_ADDRESS` points at `SpaceKitMultiAssetVault` (aUSD / 18-dec `charge`).

use alloy::network::{EthereumWallet, ReceiptResponse};
use alloy::primitives::{Address, TxHash, B256, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::{BlockNumberOrTag, Filter};
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;
use anyhow::Context;
use serde::Serialize;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Mutex;

use crate::intent::verify_eip191_utf8_message;

alloy::sol! {
    #[sol(rpc)]
    contract SpaceKitDepositVault {
        function charge(address user, uint256 amount) external;
    }
}

alloy::sol! {
    #[sol(rpc)]
    contract SpaceKitMultiAssetVault {
        function charge(address user, uint256 amount) external;
    }
}

/// Builds the same UTF-8 string as `spacekit.xyz-website/src/agentHub/chargeMessage.ts`.
/// `amount_a_usd` is the decimal string of **18-decimal aUSD wei** passed to on-chain `charge`.
pub fn build_agent_hub_charge_message(
    user: &str,
    amount_a_usd: &str,
    agent_id: &str,
    nonce: &str,
) -> String {
    [
        "SpaceKit Agent Hub charge",
        &format!("user:{user}"),
        &format!("amountAUsd:{amount_a_usd}"),
        &format!("agent:{agent_id}"),
        &format!("nonce:{nonce}"),
    ]
    .join("\n")
}

/// Runtime config from env (`ROUTEKIT_RPC_URL`, `ROUTEKIT_CHAIN_ID`, `ROUTEKIT_VAULT_ADDRESS`, `ROUTEKIT_RELAYER_PRIVATE_KEY`).
#[derive(Clone)]
pub struct VaultRelayConfig {
    rpc_url: reqwest::Url,
    signer: PrivateKeySigner,
    pub vault: Address,
    pub chain_id: u64,
    /// When true, `ROUTEKIT_VAULT_ADDRESS` is treated as `SpaceKitMultiAssetVault` (aUSD charge, activity event set).
    pub multi_vault: bool,
}

impl VaultRelayConfig {
    pub fn try_from_env() -> anyhow::Result<Option<Self>> {
        let rpc = match std::env::var("ROUTEKIT_RPC_URL") {
            Ok(s) if !s.trim().is_empty() => s,
            _ => return Ok(None),
        };
        let pk = match std::env::var("ROUTEKIT_RELAYER_PRIVATE_KEY") {
            Ok(s) if !s.trim().is_empty() => s,
            _ => return Ok(None),
        };
        let vault_s = match std::env::var("ROUTEKIT_VAULT_ADDRESS") {
            Ok(s) if !s.trim().is_empty() => s,
            _ => return Ok(None),
        };

        let chain_id: u64 = std::env::var("ROUTEKIT_CHAIN_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let multi_vault = std::env::var("ROUTEKIT_VAULT_KIND")
            .map(|s| s.trim().eq_ignore_ascii_case("multi"))
            .unwrap_or(false);

        let rpc_url: reqwest::Url = rpc.parse().context("ROUTEKIT_RPC_URL")?;
        let signer: PrivateKeySigner = pk
            .trim_start_matches("0x")
            .parse()
            .context("ROUTEKIT_RELAYER_PRIVATE_KEY")?;
        let signer = signer.with_chain_id(Some(chain_id));

        let vault = Address::from_str(vault_s.trim()).context("ROUTEKIT_VAULT_ADDRESS")?;

        Ok(Some(Self {
            rpc_url,
            signer,
            vault,
            chain_id,
            multi_vault,
        }))
    }

    fn write_provider(&self) -> impl Provider + use<'_> {
        let wallet = EthereumWallet::from(self.signer.clone());
        ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(self.rpc_url.clone())
    }

    fn read_provider(&self) -> impl Provider + use<'_> {
        ProviderBuilder::new().connect_http(self.rpc_url.clone())
    }
}

pub async fn submit_charge(
    config: &VaultRelayConfig,
    user: Address,
    amount: U256,
) -> anyhow::Result<TxHash> {
    let provider = config.write_provider();
    let pending = if config.multi_vault {
        let contract = SpaceKitMultiAssetVault::new(config.vault, &provider);
        contract
            .charge(user, amount)
            .send()
            .await
            .context("charge send")?
    } else {
        let contract = SpaceKitDepositVault::new(config.vault, &provider);
        contract
            .charge(user, amount)
            .send()
            .await
            .context("charge send")?
    };
    let receipt = pending.get_receipt().await.context("charge receipt")?;
    Ok(receipt.transaction_hash())
}

#[derive(Serialize)]
pub struct VaultActivityJson {
    pub user: String,
    pub from_block: String,
    pub deposited: Vec<serde_json::Value>,
    pub withdrawn: Vec<serde_json::Value>,
    pub charged: Vec<serde_json::Value>,
}

fn amount_word(data: &[u8]) -> Option<String> {
    if data.len() < 32 {
        return None;
    }
    Some(U256::from_be_slice(&data[..32]).to_string())
}

fn two_amount_words(data: &[u8]) -> (Option<String>, Option<String>) {
    let a = amount_word(data);
    let b = if data.len() >= 64 {
        amount_word(&data[32..])
    } else {
        None
    };
    (a, b)
}

fn log_row_simple_legacy(l: &alloy::rpc::types::Log) -> serde_json::Value {
    let tx = l
        .transaction_hash
        .map(|h| format!("{h:#x}"))
        .unwrap_or_default();
    let amt = amount_word(l.data().data.as_ref());
    serde_json::json!({
        "txHash": tx,
        "amount": amt,
    })
}

fn log_row_charged_legacy(l: &alloy::rpc::types::Log) -> serde_json::Value {
    let tx = l
        .transaction_hash
        .map(|h| format!("{h:#x}"))
        .unwrap_or_default();
    let amt = amount_word(l.data().data.as_ref());
    let treasury = l
        .topics()
        .get(2)
        .map(|t: &B256| format!("{:?}", Address::from_word(*t)));
    serde_json::json!({
        "txHash": tx,
        "amount": amt,
        "treasury": treasury,
    })
}

fn log_row_deposited_erc20(l: &alloy::rpc::types::Log) -> serde_json::Value {
    let tx = l
        .transaction_hash
        .map(|h| format!("{h:#x}"))
        .unwrap_or_default();
    let token = l
        .topics()
        .get(2)
        .map(|t: &B256| format!("{:?}", Address::from_word(*t)));
    let (raw_amt, a_usd) = two_amount_words(l.data().data.as_ref());
    serde_json::json!({
        "kind": "erc20",
        "txHash": tx,
        "token": token,
        "amount": raw_amt,
        "aUsdDelta": a_usd,
    })
}

fn log_row_deposited_eth(l: &alloy::rpc::types::Log) -> serde_json::Value {
    let tx = l
        .transaction_hash
        .map(|h| format!("{h:#x}"))
        .unwrap_or_default();
    let (wei, a_usd) = two_amount_words(l.data().data.as_ref());
    serde_json::json!({
        "kind": "eth",
        "txHash": tx,
        "weiAmount": wei,
        "aUsdDelta": a_usd,
    })
}

fn log_row_withdrawn_erc20(l: &alloy::rpc::types::Log) -> serde_json::Value {
    log_row_deposited_erc20(l)
}

fn log_row_withdrawn_eth(l: &alloy::rpc::types::Log) -> serde_json::Value {
    log_row_deposited_eth(l)
}

fn log_row_charged_multi(l: &alloy::rpc::types::Log) -> serde_json::Value {
    let tx = l
        .transaction_hash
        .map(|h| format!("{h:#x}"))
        .unwrap_or_default();
    let (a_usd, payout) = two_amount_words(l.data().data.as_ref());
    let treasury = l
        .topics()
        .get(2)
        .map(|t: &B256| format!("{:?}", Address::from_word(*t)));
    serde_json::json!({
        "txHash": tx,
        "aUsdAmount": a_usd,
        "payoutAmount": payout,
        "treasury": treasury,
    })
}

async fn fetch_activity_legacy(
    config: &VaultRelayConfig,
    user: Address,
    from_block: u64,
) -> anyhow::Result<VaultActivityJson> {
    let provider = config.read_provider();
    let topic_user = user.into_word();

    let f_dep = Filter::new()
        .address(config.vault)
        .event("Deposited(address,uint256)")
        .topic1(topic_user)
        .from_block(from_block)
        .to_block(BlockNumberOrTag::Latest);
    let f_wdr = Filter::new()
        .address(config.vault)
        .event("Withdrawn(address,uint256)")
        .topic1(topic_user)
        .from_block(from_block)
        .to_block(BlockNumberOrTag::Latest);
    let f_chg = Filter::new()
        .address(config.vault)
        .event("Charged(address,uint256,address)")
        .topic1(topic_user)
        .from_block(from_block)
        .to_block(BlockNumberOrTag::Latest);

    let (logs_d, logs_w, logs_c) = tokio::try_join!(
        provider.get_logs(&f_dep),
        provider.get_logs(&f_wdr),
        provider.get_logs(&f_chg),
    )?;

    Ok(VaultActivityJson {
        user: format!("{user:#x}"),
        from_block: from_block.to_string(),
        deposited: logs_d.iter().map(log_row_simple_legacy).collect(),
        withdrawn: logs_w.iter().map(log_row_simple_legacy).collect(),
        charged: logs_c.iter().map(log_row_charged_legacy).collect(),
    })
}

async fn fetch_activity_multi(
    config: &VaultRelayConfig,
    user: Address,
    from_block: u64,
) -> anyhow::Result<VaultActivityJson> {
    let provider = config.read_provider();
    let topic_user = user.into_word();

    let f_dep_e = Filter::new()
        .address(config.vault)
        .event("DepositedErc20(address,address,uint256,uint256)")
        .topic1(topic_user)
        .from_block(from_block)
        .to_block(BlockNumberOrTag::Latest);
    let f_dep_eth = Filter::new()
        .address(config.vault)
        .event("DepositedEth(address,uint256,uint256)")
        .topic1(topic_user)
        .from_block(from_block)
        .to_block(BlockNumberOrTag::Latest);
    let f_wdr_e = Filter::new()
        .address(config.vault)
        .event("WithdrawnErc20(address,address,uint256,uint256)")
        .topic1(topic_user)
        .from_block(from_block)
        .to_block(BlockNumberOrTag::Latest);
    let f_wdr_eth = Filter::new()
        .address(config.vault)
        .event("WithdrawnEth(address,uint256,uint256)")
        .topic1(topic_user)
        .from_block(from_block)
        .to_block(BlockNumberOrTag::Latest);
    let f_chg = Filter::new()
        .address(config.vault)
        .event("Charged(address,uint256,uint256,address)")
        .topic1(topic_user)
        .from_block(from_block)
        .to_block(BlockNumberOrTag::Latest);

    let (logs_dep_e, logs_dep_eth, logs_wdr_e, logs_wdr_eth, logs_c) = tokio::try_join!(
        provider.get_logs(&f_dep_e),
        provider.get_logs(&f_dep_eth),
        provider.get_logs(&f_wdr_e),
        provider.get_logs(&f_wdr_eth),
        provider.get_logs(&f_chg),
    )?;

    let deposited: Vec<serde_json::Value> = logs_dep_e
        .iter()
        .map(log_row_deposited_erc20)
        .chain(logs_dep_eth.iter().map(log_row_deposited_eth))
        .collect();
    let withdrawn: Vec<serde_json::Value> = logs_wdr_e
        .iter()
        .map(log_row_withdrawn_erc20)
        .chain(logs_wdr_eth.iter().map(log_row_withdrawn_eth))
        .collect();

    Ok(VaultActivityJson {
        user: format!("{user:#x}"),
        from_block: from_block.to_string(),
        deposited,
        withdrawn,
        charged: logs_c.iter().map(log_row_charged_multi).collect(),
    })
}

/// `from_block`: if `None`, uses `latest − 20_000` (same default as the legacy Node relayer).
pub async fn fetch_activity(
    config: &VaultRelayConfig,
    user: Address,
    from_block: Option<u64>,
) -> anyhow::Result<VaultActivityJson> {
    let provider = config.read_provider();
    let from_block = match from_block {
        Some(b) => b,
        None => {
            let latest = provider
                .get_block_number()
                .await
                .context("get_block_number for activity default")?;
            latest.saturating_sub(20_000)
        }
    };

    if config.multi_vault {
        fetch_activity_multi(config, user, from_block).await
    } else {
        fetch_activity_legacy(config, user, from_block).await
    }
}

/// POST /v1/charge body (matches website / `requestVaultCharge.ts`).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultChargeRequest {
    pub user: String,
    pub amount_a_usd: String,
    pub agent_id: String,
    pub nonce: String,
    pub signature: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultChargeResponse {
    pub ok: bool,
    pub transaction_hash: String,
}

pub struct VaultRelayHandles {
    pub config: VaultRelayConfig,
    pub used_nonces: Mutex<HashSet<String>>,
}

impl VaultRelayHandles {
    pub fn new(config: VaultRelayConfig) -> Self {
        Self {
            config,
            used_nonces: Mutex::new(HashSet::new()),
        }
    }

    pub async fn handle_charge(
        &self,
        body: VaultChargeRequest,
    ) -> Result<VaultChargeResponse, String> {
        let user_addr: Address = body
            .user
            .parse()
            .map_err(|_| "invalid user address".to_string())?;
        if !body.amount_a_usd.chars().all(|c| c.is_ascii_digit()) || body.amount_a_usd == "0" {
            return Err(
                "amountAUsd must be a positive decimal integer (18-decimal aUSD wei)".to_string(),
            );
        }
        let amount: U256 = body
            .amount_a_usd
            .parse()
            .map_err(|_| "amountAUsd out of range".to_string())?;

        let msg = build_agent_hub_charge_message(
            &body.user,
            &body.amount_a_usd,
            &body.agent_id,
            &body.nonce,
        );
        verify_eip191_utf8_message(&msg, &body.signature, &body.user)?;

        let nonce_key = format!("{}:{}", body.user.to_lowercase(), body.nonce);
        {
            let mut g = self.used_nonces.lock().map_err(|_| "nonce lock")?;
            if g.contains(&nonce_key) {
                return Err("nonce already used".to_string());
            }
            if g.len() > 500_000 {
                g.clear();
            }
            g.insert(nonce_key);
        }

        let hash = submit_charge(&self.config, user_addr, amount)
            .await
            .map_err(|e| e.to_string())?;

        Ok(VaultChargeResponse {
            ok: true,
            transaction_hash: format!("{hash:#x}"),
        })
    }
}
