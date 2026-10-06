//! Fee Router
//!
//! Splits an ASTRA payment into the payee's share and the network fee and
//! hands both to a `CreditApplier`, which moves them on the chain.

use crate::types::*;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Callback for applying a credit to the chain.
///
/// ASTRA exists only as native balances on the chain. An implementation must
/// apply a credit as a **transfer** of `amount_astra` from `payer_did` to
/// `beneficiary_did` (a chain transaction), and fail if the payer cannot
/// cover it. It must never mint.
pub trait CreditApplier: Send + Sync {
    fn apply_credit(&self, credit: &Credit) -> Result<()>;
}

/// Routes payments through verification → conversion → credit application.
pub struct FeeRouter {
    config: PaymentConfig,
    applier: Arc<dyn CreditApplier>,
    /// Running total of fees collected (for metrics).
    total_fees_collected: Arc<RwLock<u128>>,
    total_credits_applied: Arc<RwLock<u128>>,
}

impl FeeRouter {
    pub fn new(config: PaymentConfig, applier: Arc<dyn CreditApplier>) -> Self {
        Self {
            config,
            applier,
            total_fees_collected: Arc::new(RwLock::new(0)),
            total_credits_applied: Arc::new(RwLock::new(0)),
        }
    }

    /// Process a native ASTRA payment (already denominated in VM units).
    /// Network fee is still deducted.
    pub async fn process_astra_payment(
        &self,
        amount_astra: u128,
        from_did: &str,
        to_did: &str,
    ) -> Result<Credit> {
        let fee = amount_astra / 10_000 * (self.config.network_fee_bps as u128)
            + amount_astra % 10_000 * (self.config.network_fee_bps as u128) / 10_000;
        let net = amount_astra - fee;

        let credit = Credit {
            payer_did: from_did.to_string(),
            beneficiary_did: to_did.to_string(),
            amount_astra: net,
            source: PaymentAsset::ASTRA,
            receipt: None,
        };
        self.applier.apply_credit(&credit)?;

        if fee > 0 {
            let treasury_credit = Credit {
                payer_did: from_did.to_string(),
                beneficiary_did: self.config.treasury_did.clone(),
                amount_astra: fee,
                source: PaymentAsset::ASTRA,
                receipt: None,
            };
            self.applier.apply_credit(&treasury_credit)?;
        }

        *self.total_fees_collected.write().await += fee;
        *self.total_credits_applied.write().await += net;
        info!("ASTRA payment {from_did} -> {to_did}: {} ASTRA, fee {}", format_astra(net), format_astra(fee));

        Ok(credit)
    }

    /// The requirement a client must pay: `amount_wei` ASTRA to `pay_to`.
    pub fn create_requirement(
        &self,
        amount_wei: u128,
        pay_to: &str,
        description: Option<&str>,
    ) -> PaymentRequirement {
        PaymentRequirement {
            amount_wei: amount_wei.to_string(),
            asset: PaymentAsset::ASTRA,
            pay_to: pay_to.to_string(),
            chain_id: None,
            description: description.map(|s| s.to_string()),
        }
    }

    pub async fn total_fees_collected(&self) -> u128 {
        *self.total_fees_collected.read().await
    }

    pub async fn total_credits_applied(&self) -> u128 {
        *self.total_credits_applied.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        fn applied(&self) -> Vec<Credit> {
            self.credits.lock().unwrap().clone()
        }
    }

    impl CreditApplier for MockApplier {
        fn apply_credit(&self, credit: &Credit) -> Result<()> {
            self.credits.lock().unwrap().push(credit.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_astra_payment() {
        let applier = Arc::new(MockApplier::new());
        let config = PaymentConfig {
            network_fee_bps: 25,
            ..Default::default()
        };
        let router = FeeRouter::new(config, applier.clone());

        let credit = router
            .process_astra_payment(10_000, "did:alice", "did:contract:xyz")
            .await
            .unwrap();

        // 25 bps = 0.25% of 10_000 = 25, net = 9975
        assert_eq!(credit.amount_astra, 9975);
        let applied = applier.applied();
        assert_eq!(applied.len(), 2);
        assert_eq!(applied[1].amount_astra, 25);
        assert_eq!(applied[1].payer_did, "did:alice");
        assert_eq!(router.total_fees_collected().await, 25);
    }
}
