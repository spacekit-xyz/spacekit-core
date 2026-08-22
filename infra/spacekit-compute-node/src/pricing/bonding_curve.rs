//! Bonding Curve Implementation for Dynamic Pricing
//!
//! Implements token bonding curve for dynamic service pricing based on network demand

use serde::{Deserialize, Serialize};

/// Bonding curve for ASTRA token pricing
#[derive(Debug, Clone)]
pub struct BondingCurve {
    /// Base price in wei
    pub base_price: u128,
    /// Curve steepness (higher = more dramatic price changes)
    pub curve_steepness: f64,
    /// Total token supply
    pub total_supply: u128,
    /// Current circulating supply
    pub circulating_supply: u128,
    /// Reserve balance
    pub reserve_balance: u128,
}

impl BondingCurve {
    /// Create a new bonding curve
    pub fn new(base_price: u128, curve_steepness: f64, total_supply: u128) -> Self {
        Self {
            base_price,
            curve_steepness,
            total_supply,
            circulating_supply: 0,
            reserve_balance: 0,
        }
    }

    /// Calculate current token price based on supply
    pub fn calculate_price(&self) -> u128 {
        if self.circulating_supply == 0 {
            return self.base_price;
        }

        // Bancor-style formula: Price = Reserve / (Supply × CW)
        // Simplified: Price = base_price × (1 + supply_ratio)^steepness

        let supply_ratio = self.circulating_supply as f64 / self.total_supply as f64;
        let multiplier = (1.0 + supply_ratio).powf(self.curve_steepness);

        (self.base_price as f64 * multiplier) as u128
    }

    /// Calculate service fee based on type and network load
    pub fn calculate_service_fee(
        &self,
        service_type: ServiceType,
        network_load: f64, // 0.0-1.0
    ) -> u128 {
        let base_fee = match service_type {
            ServiceType::Messaging => 1_000_000_000_000_000_u128, // 0.001 ASTRA
            ServiceType::Compute => 10_000_000_000_000_000_u128,  // 0.01 ASTRA
            ServiceType::Storage => 5_000_000_000_000_000_u128,   // 0.005 ASTRA
            ServiceType::Subscription => 100_000_000_000_000_u128, // 0.0001 ASTRA
            ServiceType::Compression => 2_000_000_000_000_000_u128, // 0.002 ASTRA
            ServiceType::GroupOperation => 5_000_000_000_000_000_u128, // 0.005 ASTRA
        };

        // Apply network load multiplier (1.0-3.0x based on congestion)
        let load_multiplier = 1.0 + (network_load * 2.0);

        // Apply token price multiplier
        let price_multiplier = self.calculate_price() as f64 / self.base_price as f64;

        (base_fee as f64 * load_multiplier * price_multiplier) as u128
    }

    /// Buy tokens (increases price)
    pub fn buy_tokens(&mut self, amount_wei: u128) -> u128 {
        let price = self.calculate_price();
        let tokens = amount_wei / price;

        self.circulating_supply += tokens;
        self.reserve_balance += amount_wei;

        tokens
    }

    /// Sell tokens (decreases price)
    pub fn sell_tokens(&mut self, tokens: u128) -> u128 {
        let price = self.calculate_price();
        let wei_returned = tokens * price;

        self.circulating_supply -= tokens;
        self.reserve_balance -= wei_returned;

        wei_returned
    }
}

/// Service types for pricing
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ServiceType {
    Messaging,
    Compute,
    Storage,
    Subscription,
    Compression,
    GroupOperation,
}

/// Network pricing state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkPricing {
    pub current_token_price: f64, // USD per ASTRA
    pub network_load: f64,        // 0.0-1.0
    pub messaging_fee: f64,       // ASTRA
    pub compute_fee: f64,
    pub storage_fee: f64,
    pub subscription_fee: f64,
    pub compression_fee: f64,
    pub total_supply: u128,
    pub circulating_supply: u128,
    pub market_cap_usd: f64,
}

impl Default for BondingCurve {
    fn default() -> Self {
        Self::new(
            1_000_000_000_000_000,                 // 0.001 ASTRA base price
            2.0,                                   // Moderate steepness
            2_000_000_000_000_000_000_000_000_000, // 2B total supply (hard cap)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bonding_curve_price() {
        let mut curve = BondingCurve::default();

        let initial_price = curve.calculate_price();
        assert_eq!(initial_price, curve.base_price);

        // Buy tokens
        curve.circulating_supply = curve.total_supply / 10; // 10% circulating
        let price_at_10_percent = curve.calculate_price();

        assert!(price_at_10_percent > initial_price);
    }

    #[test]
    fn test_service_fees() {
        let curve = BondingCurve::default();

        // Low network load
        let fee_low = curve.calculate_service_fee(ServiceType::Messaging, 0.1);

        // High network load
        let fee_high = curve.calculate_service_fee(ServiceType::Messaging, 0.9);

        assert!(fee_high > fee_low); // Higher load = higher fees
    }
}
