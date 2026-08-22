//! Core payment types shared across all payment methods.

use serde::{Deserialize, Serialize};

/// Supported payment networks for x402 settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentNetwork {
    /// Base mainnet (chain ID 8453)
    Base,
    /// Base Sepolia testnet (chain ID 84532)
    BaseSepolia,
}

impl PaymentNetwork {
    pub fn chain_id(self) -> u64 {
        match self {
            Self::Base => 8453,
            Self::BaseSepolia => 84532,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::BaseSepolia => "base-sepolia",
        }
    }

    pub fn is_testnet(self) -> bool {
        matches!(self, Self::BaseSepolia)
    }

    pub fn select(testnet: bool) -> Self {
        if testnet {
            Self::BaseSepolia
        } else {
            Self::Base
        }
    }
}

/// Supported payment assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentAsset {
    /// USDC stablecoin (x402 primary)
    USDC,
    /// aUSD internal credit (vault deposits)
    AUSD,
    /// Native ASTRA (in-VM token)
    ASTRA,
}

/// How much the caller must pay, and in what asset/network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequirement {
    /// Human-readable amount (e.g. "0.01" for USDC, "100" for ASTRA)
    pub amount: String,
    pub asset: PaymentAsset,
    /// Destination: EVM address (0x…) for x402/aUSD, or DID for ASTRA.
    pub pay_to: String,
    /// Which network to settle on (only relevant for x402).
    pub network: Option<PaymentNetwork>,
    /// Optional description shown to the payer.
    pub description: Option<String>,
}

/// Proof that a payment was completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentReceipt {
    /// On-chain transaction hash (x402/aUSD) or internal tx ID (ASTRA).
    pub tx_hash: String,
    /// Amount actually paid.
    pub amount: String,
    pub asset: PaymentAsset,
    pub network: Option<PaymentNetwork>,
    /// Unix timestamp of settlement.
    pub settled_at: i64,
}

impl PaymentReceipt {
    pub fn explorer_url(&self) -> Option<String> {
        self.network.map(|n| {
            let base = match n {
                PaymentNetwork::Base => "https://basescan.org/tx/",
                PaymentNetwork::BaseSepolia => "https://sepolia.basescan.org/tx/",
            };
            format!("{}{}", base, self.tx_hash)
        })
    }
}

/// A verified credit ready to be applied to a VM balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credit {
    /// DID or address that should receive the credit in the VM.
    pub beneficiary_did: String,
    /// Amount in the smallest unit of the VM token (ASTRA wei).
    pub amount_astra: u128,
    /// Source payment that produced this credit.
    pub source: PaymentAsset,
    /// Receipt for audit trail.
    pub receipt: Option<PaymentReceipt>,
}

/// Configuration for the payment service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentConfig {
    /// EVM address that receives x402 USDC payments.
    pub pay_to_address: String,
    /// x402 facilitator URL (default: https://x402.org/facilitator).
    pub facilitator_url: String,
    /// Whether to use testnet (Base Sepolia).
    pub testnet: bool,
    /// Exchange rate: 1 USDC = N ASTRA. Used to convert x402/aUSD credits to VM balance.
    pub usdc_to_astra_rate: f64,
    /// Network fee in basis points (default 25 = 0.25%).
    pub network_fee_bps: u32,
    /// Treasury DID that collects network fees.
    pub treasury_did: String,
}

impl Default for PaymentConfig {
    fn default() -> Self {
        Self {
            pay_to_address: String::new(),
            facilitator_url: "https://x402.org/facilitator".to_string(),
            testnet: true,
            usdc_to_astra_rate: 1_000_000.0,
            network_fee_bps: 25,
            treasury_did: "did:spacekit:treasury".to_string(),
        }
    }
}

/// Validate that a wallet address looks plausible.
pub fn validate_evm_address(addr: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        addr.starts_with("0x") && addr.len() == 42,
        "Address must be 0x-prefixed and 42 characters, got: {}",
        addr
    );
    Ok(())
}

/// Parse and validate a decimal amount string.
pub fn parse_amount(amount: &str) -> anyhow::Result<f64> {
    let v: f64 = amount
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid amount: {}", amount))?;
    anyhow::ensure!(v > 0.0, "Amount must be positive, got: {}", v);
    Ok(v)
}
