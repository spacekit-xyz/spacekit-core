//! Exchange Rate Oracle for aUSD Marketplace
//!
//! Maintains ETH/USDC/USDT/DAI to USD exchange rates.
//! On testnet, rates are admin-set. On mainnet, they can be
//! derived from LayerZero bridge pool ratios or external oracles.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRate {
    pub token: String,
    pub usd_price: f64,
    pub updated_at: u64,
    pub source: RateSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RateSource {
    AdminSet,
    BridgePool,
    Oracle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionResult {
    pub source_token: String,
    pub source_amount: f64,
    pub ausd_amount: u64,
    pub rate_used: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedemptionResult {
    pub ausd_amount: u64,
    pub target_token: String,
    pub target_amount: f64,
    pub rate_used: f64,
}

pub struct ExchangeRateOracle {
    rates: Arc<RwLock<HashMap<String, ExchangeRate>>>,
    is_testnet: bool,
}

impl ExchangeRateOracle {
    pub fn new(is_testnet: bool) -> Self {
        let mut rates = HashMap::new();

        // Stablecoins are 1:1 to USD by definition
        for token in &["USDC", "USDT", "DAI"] {
            rates.insert(
                token.to_string(),
                ExchangeRate {
                    token: token.to_string(),
                    usd_price: 1.0,
                    updated_at: 0,
                    source: if is_testnet {
                        RateSource::AdminSet
                    } else {
                        RateSource::Oracle
                    },
                },
            );
        }

        // ETH testnet default (admin-set, can be overridden)
        rates.insert(
            "ETH".to_string(),
            ExchangeRate {
                token: "ETH".to_string(),
                usd_price: 3200.0,
                updated_at: 0,
                source: RateSource::AdminSet,
            },
        );

        Self {
            rates: Arc::new(RwLock::new(rates)),
            is_testnet,
        }
    }

    pub async fn set_rate(&self, token: &str, usd_price: f64) {
        let mut rates = self.rates.write().await;
        rates.insert(
            token.to_string(),
            ExchangeRate {
                token: token.to_string(),
                usd_price,
                updated_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                source: RateSource::AdminSet,
            },
        );
    }

    pub async fn get_rate(&self, token: &str) -> Option<ExchangeRate> {
        let rates = self.rates.read().await;
        rates.get(token).cloned()
    }

    pub async fn get_all_rates(&self) -> Vec<ExchangeRate> {
        let rates = self.rates.read().await;
        rates.values().cloned().collect()
    }

    /// Convert a source token amount to aUSD (smallest units, 2 decimals).
    /// e.g. 1.0 USDC → 100 aUSD units (= $1.00)
    pub async fn convert_to_ausd(&self, token: &str, amount: f64) -> Option<ConversionResult> {
        let rates = self.rates.read().await;
        let rate = rates.get(token)?;

        let usd_value = amount * rate.usd_price;
        let ausd_units = (usd_value * 100.0).round() as u64; // 2 decimal places

        Some(ConversionResult {
            source_token: token.to_string(),
            source_amount: amount,
            ausd_amount: ausd_units,
            rate_used: rate.usd_price,
        })
    }

    /// Convert aUSD units back to a target token amount for redemption.
    /// e.g. 1000 aUSD units (= $10.00) → 10.0 USDC
    pub async fn convert_from_ausd(
        &self,
        ausd_units: u64,
        target_token: &str,
    ) -> Option<RedemptionResult> {
        let rates = self.rates.read().await;
        let rate = rates.get(target_token)?;

        let usd_value = ausd_units as f64 / 100.0;
        let target_amount = usd_value / rate.usd_price;

        Some(RedemptionResult {
            ausd_amount: ausd_units,
            target_token: target_token.to_string(),
            target_amount,
            rate_used: rate.usd_price,
        })
    }

    pub fn is_testnet(&self) -> bool {
        self.is_testnet
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stablecoin_1_to_1() {
        let oracle = ExchangeRateOracle::new(true);
        let result = oracle.convert_to_ausd("USDC", 10.0).await.unwrap();
        assert_eq!(result.ausd_amount, 1000); // $10.00 = 1000 units
    }

    #[tokio::test]
    async fn eth_conversion() {
        let oracle = ExchangeRateOracle::new(true);
        oracle.set_rate("ETH", 3200.0).await;
        let result = oracle.convert_to_ausd("ETH", 1.0).await.unwrap();
        assert_eq!(result.ausd_amount, 320000); // 1 ETH = $3200 = 320000 units
    }

    #[tokio::test]
    async fn redemption_stablecoin() {
        let oracle = ExchangeRateOracle::new(true);
        let result = oracle.convert_from_ausd(5000, "USDC").await.unwrap();
        assert!((result.target_amount - 50.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn redemption_eth() {
        let oracle = ExchangeRateOracle::new(true);
        let result = oracle.convert_from_ausd(320000, "ETH").await.unwrap();
        assert!((result.target_amount - 1.0).abs() < 0.001);
    }
}
