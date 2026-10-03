//! Intent-Based Payment Processing
//!
//! Extracts payment-related actions from a SpaceKit `Intent`, processes them
//! through the `FeeRouter`, and produces `Credit`s for the VM.
//!
//! Supported intent action types:
//! - `execute_contract` — run a WASM contract with optional attached ASTRA value
//!   and an ASTRA fee cap
//! - `transfer` — native ASTRA transfer between actors
//!
//! Every amount is ASTRA wei. Actions that name another asset are refused
//! (`Unsupported`), never converted.

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
    /// Transfer native ASTRA between actors.
    Transfer(TransferAction),
    /// Any other action type (swaps, …); not processed here.
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
    /// ASTRA wei to attach as `msg_value` (decimal string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_astra: Option<String>,
    /// Maximum fee in ASTRA wei (decimal string).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_fee_astra: Option<String>,
}

/// Transfer native ASTRA between actors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferAction {
    /// Asset identifier (e.g. "spacekit:mainnet:native" for ASTRA).
    pub asset: String,
    /// Destination DID or address.
    pub to: String,
    /// Amount in ASTRA wei (decimal string).
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
    /// Contract executions (with attached value/fee constraints).
    pub contract_executions: Vec<ExecuteContractAction>,
    /// Native ASTRA transfers.
    pub transfers: Vec<TransferAction>,
    /// The actor's DID (payer).
    pub actor_did: String,
    /// Maximum ASTRA wei the intent may move, from `constraints.max_value_wei`.
    pub max_value_wei: Option<u128>,
}

impl IntentPaymentPlan {
    /// The payment plan of an intent.
    pub fn from_intent(intent: &Intent) -> Self {
        let mut contract_executions = Vec::new();
        let mut transfers = Vec::new();

        for action in &intent.actions {
            match action {
                IntentAction::ExecuteContract(ec) => contract_executions.push(ec.clone()),
                IntentAction::Transfer(t) => transfers.push(t.clone()),
                IntentAction::Other => {}
            }
        }

        let max_value_wei = intent.constraints.get("max_value_wei").and_then(|v| match v {
            serde_json::Value::String(s) => s.trim().parse().ok(),
            serde_json::Value::Number(n) => n.as_u64().map(u128::from),
            _ => None,
        });

        Self {
            contract_executions,
            transfers,
            actor_did: intent.actor.clone(),
            max_value_wei,
        }
    }

    /// ASTRA wei the plan moves: attached contract value plus transfers.
    pub fn total_value_wei(&self) -> Result<u128> {
        let mut total = 0u128;
        for ec in &self.contract_executions {
            if let Some(v) = ec.value_astra.as_deref() {
                total = total
                    .checked_add(v.trim().parse().context("invalid value_astra")?)
                    .context("value overflow")?;
            }
        }
        for t in &self.transfers {
            total = total
                .checked_add(t.amount.trim().parse().context("invalid transfer amount")?)
                .context("value overflow")?;
        }
        Ok(total)
    }
}

/// Whether an asset id names native ASTRA (`ASTRA`, `native`,
/// `spacekit:<network>:native`).
pub fn is_astra_asset(asset: &str) -> bool {
    let a = asset.trim();
    a.eq_ignore_ascii_case("astra")
        || a.eq_ignore_ascii_case("native")
        || (a.starts_with("spacekit:") && a.ends_with(":native"))
}

/// Result of processing an intent's payment actions.
#[derive(Debug, Clone, Serialize)]
pub struct IntentPaymentResult {
    /// Credits applied to the VM.
    pub credits: Vec<Credit>,
    /// Total ASTRA credited across all actions.
    pub total_astra_credited: u128,
}

// ─── Intent Payment Processor ────────────────────────────────────────────────

/// Processes the ASTRA actions of an intent through the FeeRouter.
pub struct IntentPaymentProcessor {
    fee_router: Arc<FeeRouter>,
}

impl IntentPaymentProcessor {
    pub fn new(fee_router: Arc<FeeRouter>) -> Self {
        Self { fee_router }
    }

    /// Extract the payment plan from an intent.
    pub fn extract_plan(&self, intent: &Intent) -> IntentPaymentPlan {
        IntentPaymentPlan::from_intent(intent)
    }

    /// Check the plan's limits, then apply its ASTRA transfers through the
    /// fee router. Returns the credits applied.
    ///
    /// Fails without applying anything if a transfer names an asset other
    /// than ASTRA, an amount does not parse, or the plan moves more than the
    /// intent's `max_value_wei`.
    pub async fn process_plan(&self, plan: &IntentPaymentPlan) -> Result<IntentPaymentResult> {
        for t in &plan.transfers {
            anyhow::ensure!(
                is_astra_asset(&t.asset),
                "transfer asset {} is not ASTRA; SpaceKit settles only in ASTRA",
                t.asset
            );
        }
        let total = plan.total_value_wei()?;
        if let Some(max) = plan.max_value_wei {
            anyhow::ensure!(
                total <= max,
                "intent moves {total} wei, more than its max_value_wei {max}"
            );
        }

        let mut credits = Vec::new();
        let mut total_astra = 0u128;
        for t in &plan.transfers {
            let amount: u128 = t.amount.trim().parse().context("Invalid ASTRA transfer amount")?;
            let credit = self
                .fee_router
                .process_astra_payment(amount, &plan.actor_did, &t.to)
                .await
                .context("ASTRA transfer failed")?;
            total_astra += credit.amount_astra;
            credits.push(credit);
        }

        info!(
            "Intent payment plan processed: {} executions, {} transfers, {} wei moved",
            plan.contract_executions.len(),
            plan.transfers.len(),
            total_astra,
        );

        Ok(IntentPaymentResult {
            credits,
            total_astra_credited: total_astra,
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

    fn intent(actions: Vec<IntentAction>, constraints: serde_json::Value) -> Intent {
        Intent {
            intent_id: "abc123".to_string(),
            version: "1.0".to_string(),
            actor: "did:alice".to_string(),
            agent: None,
            chain: "spacekit:mainnet".to_string(),
            constraints,
            actions,
            nonce: "1".to_string(),
            expiry: 9999999999,
            meta: None,
        }
    }

    fn processor() -> (Arc<MockApplier>, IntentPaymentProcessor) {
        let applier = Arc::new(MockApplier::new());
        let config = PaymentConfig {
            network_fee_bps: 100,
            treasury_did: "did:treasury".to_string(),
        };
        let fee_router = Arc::new(FeeRouter::new(config, applier.clone()));
        (applier, IntentPaymentProcessor::new(fee_router))
    }

    #[tokio::test]
    async fn test_intent_with_transfer_and_execute() {
        let (applier, processor) = processor();
        let intent = intent(
            vec![
                IntentAction::Transfer(TransferAction {
                    asset: "spacekit:mainnet:native".to_string(),
                    to: "did:bob".to_string(),
                    amount: "10000".to_string(),
                }),
                IntentAction::ExecuteContract(ExecuteContractAction {
                    contract_id: "did:contract:xyz".to_string(),
                    input: "deadbeef".to_string(),
                    value_astra: Some("500".to_string()),
                    max_fee_astra: None,
                }),
            ],
            serde_json::json!({"max_value_wei": "20000"}),
        );

        let plan = processor.extract_plan(&intent);
        assert_eq!(plan.contract_executions.len(), 1);
        assert_eq!(plan.total_value_wei().unwrap(), 10_500);

        let result = processor.process_plan(&plan).await.unwrap();
        assert_eq!(result.total_astra_credited, 9_900);
        let applied = applier.credits.lock().unwrap().clone();
        assert_eq!(applied.len(), 2);
        assert_eq!(applied[1].beneficiary_did, "did:treasury");
        assert_eq!(applied[1].amount_astra, 100);
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
        // Vault charges no longer exist: they are not a payment action.
        assert!(matches!(actions[0], IntentAction::Other));
        assert!(matches!(actions[1], IntentAction::ExecuteContract(_)));
        assert!(matches!(actions[2], IntentAction::Transfer(_)));
        assert!(matches!(actions[3], IntentAction::Other));
    }

    #[tokio::test]
    async fn test_value_cap_and_foreign_assets_refused() {
        let (applier, processor) = processor();
        let over = intent(
            vec![IntentAction::ExecuteContract(ExecuteContractAction {
                contract_id: "did:c:1".to_string(),
                input: "00".to_string(),
                value_astra: Some("5000".to_string()),
                max_fee_astra: None,
            })],
            serde_json::json!({"max_value_wei": "1000"}),
        );
        let err = processor.process_plan(&processor.extract_plan(&over)).await.unwrap_err();
        assert!(err.to_string().contains("max_value_wei"));

        let usdc = intent(
            vec![IntentAction::Transfer(TransferAction {
                asset: "USDC".to_string(),
                to: "did:bob".to_string(),
                amount: "1".to_string(),
            })],
            serde_json::json!({}),
        );
        let err = processor.process_plan(&processor.extract_plan(&usdc)).await.unwrap_err();
        assert!(err.to_string().contains("not ASTRA"));
        assert!(applier.credits.lock().unwrap().is_empty());
    }
}
