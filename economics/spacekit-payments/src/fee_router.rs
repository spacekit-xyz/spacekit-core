//! Fee Router
//!
//! Converts verified payments (x402 receipts, aUSD charges, ASTRA transfers)
//! into `Credit`s and applies them to VM balances via a pluggable callback.

use crate::types::*;
use anyhow::Result;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Callback for applying a credit to the VM's balance state.
/// Implementations should debit/credit the appropriate storage keys.
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

    /// Process a verified x402 or aUSD payment receipt and convert it to a VM credit.
    ///
    /// Flow:
    /// 1. Parse the receipt amount
    /// 2. Deduct network fee (goes to treasury)
    /// 3. Convert remaining USD amount to ASTRA at configured rate
    /// 4. Apply credits to both the beneficiary and the treasury
    pub async fn process_payment(
        &self,
        receipt: PaymentReceipt,
        beneficiary_did: &str,
    ) -> Result<Credit> {
        let amount_usd = parse_amount(&receipt.amount)?;

        let fee_amount = amount_usd * (self.config.network_fee_bps as f64) / 10_000.0;
        let net_amount = amount_usd - fee_amount;

        let astra_credit = (net_amount * self.config.usdc_to_astra_rate) as u128;
        let astra_fee = (fee_amount * self.config.usdc_to_astra_rate) as u128;

        // Credit the beneficiary
        let credit = Credit {
            beneficiary_did: beneficiary_did.to_string(),
            amount_astra: astra_credit,
            source: receipt.asset,
            receipt: Some(receipt.clone()),
        };
        self.applier.apply_credit(&credit)?;

        // Credit the treasury with the network fee
        if astra_fee > 0 {
            let treasury_credit = Credit {
                beneficiary_did: self.config.treasury_did.clone(),
                amount_astra: astra_fee,
                source: receipt.asset,
                receipt: None,
            };
            self.applier.apply_credit(&treasury_credit)?;
        }

        // Update metrics
        {
            let mut fees = self.total_fees_collected.write().await;
            *fees += astra_fee;
        }
        {
            let mut credits = self.total_credits_applied.write().await;
            *credits += astra_credit;
        }

        info!(
            "Processed {} {} payment for {}: {} ASTRA credited, {} ASTRA fee",
            receipt.amount,
            match receipt.asset {
                PaymentAsset::USDC => "USDC",
                PaymentAsset::AUSD => "aUSD",
                PaymentAsset::ASTRA => "ASTRA",
            },
            beneficiary_did,
            astra_credit,
            astra_fee,
        );

        Ok(credit)
    }

    /// Process a native ASTRA payment (already denominated in VM units).
    /// Network fee is still deducted.
    pub async fn process_astra_payment(
        &self,
        amount_astra: u128,
        from_did: &str,
        to_did: &str,
    ) -> Result<Credit> {
        let fee = amount_astra * (self.config.network_fee_bps as u128) / 10_000;
        let net = amount_astra - fee;

        let credit = Credit {
            beneficiary_did: to_did.to_string(),
            amount_astra: net,
            source: PaymentAsset::ASTRA,
            receipt: Some(PaymentReceipt {
                tx_hash: format!("astra:{}:{}", from_did, chrono::Utc::now().timestamp()),
                amount: amount_astra.to_string(),
                asset: PaymentAsset::ASTRA,
                network: None,
                settled_at: chrono::Utc::now().timestamp(),
            }),
        };
        self.applier.apply_credit(&credit)?;

        if fee > 0 {
            let treasury_credit = Credit {
                beneficiary_did: self.config.treasury_did.clone(),
                amount_astra: fee,
                source: PaymentAsset::ASTRA,
                receipt: None,
            };
            self.applier.apply_credit(&treasury_credit)?;
        }

        Ok(credit)
    }

    /// Create a `PaymentRequirement` for an x402 response.
    pub fn create_x402_requirement(
        &self,
        price_usdc: &str,
        description: Option<&str>,
    ) -> PaymentRequirement {
        PaymentRequirement {
            amount: price_usdc.to_string(),
            asset: PaymentAsset::USDC,
            pay_to: self.config.pay_to_address.clone(),
            network: Some(PaymentNetwork::select(self.config.testnet)),
            description: description.map(|s| s.to_string()),
        }
    }

    /// Create a `PaymentRequirement` for an aUSD vault charge.
    pub fn create_ausd_requirement(
        &self,
        price_ausd: &str,
        description: Option<&str>,
    ) -> PaymentRequirement {
        PaymentRequirement {
            amount: price_ausd.to_string(),
            asset: PaymentAsset::AUSD,
            pay_to: self.config.pay_to_address.clone(),
            network: None,
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
    async fn test_usdc_payment_with_fee() {
        let applier = Arc::new(MockApplier::new());
        let config = PaymentConfig {
            network_fee_bps: 100, // 1%
            usdc_to_astra_rate: 1_000_000.0,
            treasury_did: "did:treasury".to_string(),
            ..Default::default()
        };
        let router = FeeRouter::new(config, applier.clone());

        let receipt = PaymentReceipt {
            tx_hash: "0xabc".to_string(),
            amount: "1.00".to_string(),
            asset: PaymentAsset::USDC,
            network: Some(PaymentNetwork::BaseSepolia),
            settled_at: 0,
        };

        let credit = router
            .process_payment(receipt, "did:user:alice")
            .await
            .unwrap();

        assert_eq!(credit.amount_astra, 990_000); // 0.99 USDC * 1M
        assert_eq!(credit.beneficiary_did, "did:user:alice");

        let applied = applier.applied();
        assert_eq!(applied.len(), 2); // user + treasury
        assert_eq!(applied[1].beneficiary_did, "did:treasury");
        assert_eq!(applied[1].amount_astra, 10_000); // 0.01 USDC * 1M
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
    }
}
