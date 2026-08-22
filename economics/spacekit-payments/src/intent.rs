//! Intent-Based Payment Processing
//!
//! Extracts payment-related actions from a SpaceKit `Intent`, processes them
//! through the `FeeRouter`, and produces `Credit`s for the VM.
//!
//! Supported intent action types:
//! - `execute_contract` — run a WASM contract with optional attached value and fee caps
//! - `vault_charge` — deduct aUSD from the actor's vault balance
//! - `transfer` — native ASTRA transfer between actors

use crate::ausd::AusdVault;
use crate::fee_router::FeeRouter;
use crate::types::*;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

// ─── Intent Action Types ─────────────────────────────────────────────────────

/// An action within a SpaceKit intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IntentAction {
    /// Execute a WASM contract on SpacekitVM.
    ExecuteContract(ExecuteContractAction),
    /// Charge the actor's aUSD vault.
    VaultCharge(VaultChargeAction),
    /// Transfer native ASTRA between actors.
    Transfer(TransferAction),
    /// Swap (forwarded to chain, not processed by compute node).
    #[serde(other)]
    Other,
}

/// Execute a WASM contract with optional payment constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteContractAction {
    /// DID or address of the target WASM contract.
    pub contract_id: String,
    /// Hex-encoded input bytes for the contract.
    pub input: String,
    /// Native ASTRA to attach as `msg_value` (decimal string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_astra: Option<String>,
    /// Maximum fee the user will pay in USDC (x402 / aUSD equivalent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_fee_usdc: Option<String>,
    /// Maximum fee in native ASTRA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_fee_astra: Option<String>,
}

/// Charge the actor's aUSD vault as part of an intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultChargeAction {
    /// Amount of aUSD to charge (decimal string, e.g. "1.50").
    pub amount_ausd: String,
    /// DID of the beneficiary (contract/service receiving the credit).
    pub beneficiary: String,
}

/// Transfer native ASTRA between actors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferAction {
    /// Asset identifier (e.g. "spacekit:mainnet:native" for ASTRA).
    pub asset: String,
    /// Destination DID or address.
    pub to: String,
    /// Amount in base units (decimal string).
    pub amount: String,
}

// ─── Minimal Intent representation ───────────────────────────────────────────
// The full `Intent` and `SignedIntent` types live in routekit. This is the
// subset that spacekit-payments needs for payment processing.

/// Minimal intent representation for payment extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub intent_id: String,
    pub version: String,
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    pub chain: String,
    pub constraints: serde_json::Value,
    pub actions: Vec<IntentAction>,
    pub nonce: String,
    pub expiry: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// Signed intent as received from the relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedIntent {
    pub intent: Intent,
    pub signature: String,
    pub sig_type: String,
}

// ─── Execution Plan ──────────────────────────────────────────────────────────

/// Extracted payment plan from an intent's actions.
#[derive(Debug, Clone)]
pub struct IntentPaymentPlan {
    /// aUSD vault charges to process before execution.
    pub vault_charges: Vec<VaultChargeAction>,
    /// Contract executions (with attached value/fee constraints).
    pub contract_executions: Vec<ExecuteContractAction>,
    /// Native ASTRA transfers.
    pub transfers: Vec<TransferAction>,
    /// The actor's DID (payer).
    pub actor_did: String,
    /// Maximum notional USD from constraints (if set).
    pub max_notional_usd: Option<f64>,
}

/// Result of processing an intent's payment actions.
#[derive(Debug, Clone, Serialize)]
pub struct IntentPaymentResult {
    /// Credits applied to the VM.
    pub credits: Vec<Credit>,
    /// Total ASTRA credited across all actions.
    pub total_astra_credited: u128,
    /// Receipts for audit trail.
    pub receipts: Vec<PaymentReceipt>,
}

// ─── Intent Payment Processor ────────────────────────────────────────────────

/// Processes payment-related actions from intents through the FeeRouter and AusdVault.
pub struct IntentPaymentProcessor {
    fee_router: Arc<FeeRouter>,
    ausd_vault: Arc<AusdVault>,
}

impl IntentPaymentProcessor {
    pub fn new(fee_router: Arc<FeeRouter>, ausd_vault: Arc<AusdVault>) -> Self {
        Self {
            fee_router,
            ausd_vault,
        }
    }

    /// Extract the payment plan from an intent.
    pub fn extract_plan(&self, intent: &Intent) -> IntentPaymentPlan {
        let mut vault_charges = Vec::new();
        let mut contract_executions = Vec::new();
        let mut transfers = Vec::new();

        for action in &intent.actions {
            match action {
                IntentAction::VaultCharge(vc) => vault_charges.push(vc.clone()),
                IntentAction::ExecuteContract(ec) => contract_executions.push(ec.clone()),
                IntentAction::Transfer(t) => transfers.push(t.clone()),
                IntentAction::Other => {}
            }
        }

        let max_notional_usd = intent
            .constraints
            .get("max_notional_usd")
            .and_then(|v| v.as_f64());

        IntentPaymentPlan {
            vault_charges,
            contract_executions,
            transfers,
            actor_did: intent.actor.clone(),
            max_notional_usd,
        }
    }

    /// Process all payment actions in the plan. Returns credits and receipts.
    ///
    /// Order:
    /// 1. Vault charges (aUSD → FeeRouter → ASTRA credit)
    /// 2. Fee constraints on contract executions are validated
    /// 3. Native ASTRA transfers are processed
    ///
    /// If any step fails, the entire intent should be rejected (the caller
    /// is responsible for atomicity).
    pub async fn process_plan(
        &self,
        plan: &IntentPaymentPlan,
        intent_nonce: &str,
    ) -> Result<IntentPaymentResult> {
        let mut credits = Vec::new();
        let mut receipts = Vec::new();
        let mut total_astra = 0u128;

        // 1. Process vault charges
        for (i, vc) in plan.vault_charges.iter().enumerate() {
            let nonce_num: u64 = format!("{}{:04}", intent_nonce, i)
                .parse()
                .unwrap_or(i as u64 + 1);

            let charge_req = crate::ausd::VaultChargeRequest {
                user_did: plan.actor_did.clone(),
                amount_ausd: vc.amount_ausd.clone(),
                nonce: nonce_num,
                signature: "intent-authorized".to_string(),
                description: Some(format!("Intent vault charge for {}", vc.beneficiary)),
            };

            let receipt = self
                .ausd_vault
                .process_charge(&charge_req)
                .await
                .context("Vault charge failed")?;

            let credit = self
                .fee_router
                .process_payment(receipt.clone(), &vc.beneficiary)
                .await
                .context("Fee routing failed for vault charge")?;

            total_astra += credit.amount_astra;
            credits.push(credit);
            receipts.push(receipt);
        }

        // 2. Validate fee constraints on contract executions
        for ec in &plan.contract_executions {
            if let Some(ref max_usdc) = ec.max_fee_usdc {
                let max: f64 = max_usdc.parse().unwrap_or(0.0);
                if let Some(max_notional) = plan.max_notional_usd {
                    anyhow::ensure!(
                        max <= max_notional,
                        "Contract fee cap {} USDC exceeds intent max_notional_usd {}",
                        max,
                        max_notional
                    );
                }
            }

            if let Some(ref value_str) = ec.value_astra {
                let value: u128 = value_str.parse().unwrap_or(0);
                total_astra += value;
            }
        }

        // 3. Process native ASTRA transfers
        for t in &plan.transfers {
            if t.asset.contains("native") || t.asset.contains("ASTRA") {
                let amount: u128 = t.amount.parse().context("Invalid ASTRA transfer amount")?;
                let credit = self
                    .fee_router
                    .process_astra_payment(amount, &plan.actor_did, &t.to)
                    .await
                    .context("ASTRA transfer failed")?;
                total_astra += credit.amount_astra;
                credits.push(credit);
            }
        }

        info!(
            "Intent payment plan processed: {} charges, {} executions, {} transfers → {} total ASTRA",
            plan.vault_charges.len(),
            plan.contract_executions.len(),
            plan.transfers.len(),
            total_astra,
        );

        Ok(IntentPaymentResult {
            credits,
            total_astra_credited: total_astra,
            receipts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fee_router::CreditApplier;
    use std::sync::Mutex;

    struct MockApplier {
        credits: Mutex<Vec<Credit>>,
    }
    impl MockApplier {
        fn new() -> Self {
            Self {
                credits: Mutex::new(Vec::new()),
            }
        }
    }
    impl CreditApplier for MockApplier {
        fn apply_credit(&self, credit: &Credit) -> Result<()> {
            self.credits.lock().unwrap().push(credit.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_intent_with_vault_charge_and_execute() {
        let applier = Arc::new(MockApplier::new());
        let config = PaymentConfig {
            network_fee_bps: 100,
            usdc_to_astra_rate: 1_000_000.0,
            treasury_did: "did:treasury".to_string(),
            ..Default::default()
        };
        let fee_router = Arc::new(FeeRouter::new(config, applier.clone()));
        let vault = Arc::new(AusdVault::new());
        vault.credit("did:alice", 10.0).await;

        let processor = IntentPaymentProcessor::new(fee_router, vault);

        let intent = Intent {
            intent_id: "abc123".to_string(),
            version: "1.0".to_string(),
            actor: "did:alice".to_string(),
            agent: None,
            chain: "spacekit:mainnet".to_string(),
            constraints: serde_json::json!({"max_notional_usd": 5.0}),
            actions: vec![
                IntentAction::VaultCharge(VaultChargeAction {
                    amount_ausd: "2.00".to_string(),
                    beneficiary: "did:contract:xyz".to_string(),
                }),
                IntentAction::ExecuteContract(ExecuteContractAction {
                    contract_id: "did:contract:xyz".to_string(),
                    input: "deadbeef".to_string(),
                    value_astra: Some("500".to_string()),
                    max_fee_usdc: Some("3.00".to_string()),
                    max_fee_astra: None,
                }),
            ],
            nonce: "1".to_string(),
            expiry: 9999999999,
            meta: None,
        };

        let plan = processor.extract_plan(&intent);
        assert_eq!(plan.vault_charges.len(), 1);
        assert_eq!(plan.contract_executions.len(), 1);
        assert_eq!(plan.actor_did, "did:alice");

        let result = processor.process_plan(&plan, "1").await.unwrap();
        assert!(!result.credits.is_empty());
        assert!(result.total_astra_credited > 0);
    }

    #[tokio::test]
    async fn test_intent_action_deserialization() {
        let json = r#"[
            {"type": "vault_charge", "amount_ausd": "1.50", "beneficiary": "did:contract:abc"},
            {"type": "execute_contract", "contract_id": "did:c:1", "input": "00", "value_astra": "100"},
            {"type": "transfer", "asset": "spacekit:mainnet:native", "to": "did:bob", "amount": "500"},
            {"type": "swap", "from_asset": "ETH", "to_asset": "USDC"}
        ]"#;

        let actions: Vec<IntentAction> = serde_json::from_str(json).unwrap();
        assert!(matches!(actions[0], IntentAction::VaultCharge(_)));
        assert!(matches!(actions[1], IntentAction::ExecuteContract(_)));
        assert!(matches!(actions[2], IntentAction::Transfer(_)));
        assert!(matches!(actions[3], IntentAction::Other));
    }

    #[tokio::test]
    async fn test_fee_cap_exceeds_max_notional() {
        let applier = Arc::new(MockApplier::new());
        let config = PaymentConfig::default();
        let fee_router = Arc::new(FeeRouter::new(config, applier));
        let vault = Arc::new(AusdVault::new());

        let processor = IntentPaymentProcessor::new(fee_router, vault);

        let intent = Intent {
            intent_id: "test".to_string(),
            version: "1.0".to_string(),
            actor: "did:alice".to_string(),
            agent: None,
            chain: "spacekit:mainnet".to_string(),
            constraints: serde_json::json!({"max_notional_usd": 1.0}),
            actions: vec![IntentAction::ExecuteContract(ExecuteContractAction {
                contract_id: "did:c:1".to_string(),
                input: "00".to_string(),
                value_astra: None,
                max_fee_usdc: Some("5.00".to_string()),
                max_fee_astra: None,
            })],
            nonce: "1".to_string(),
            expiry: 9999999999,
            meta: None,
        };

        let plan = processor.extract_plan(&intent);
        let result = processor.process_plan(&plan, "1").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("exceeds intent max_notional_usd"));
    }
}
