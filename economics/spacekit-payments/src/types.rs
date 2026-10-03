//! Core payment types.
//!
//! SpaceKit has one currency: ASTRA, held as native balances on the SpaceKit
//! chain in wei (18 decimals). Every price, payment, receipt and credit here
//! is an amount of ASTRA wei. There are no USD-denominated balances, no
//! stablecoin rails and no exchange rates.

use serde::{Deserialize, Serialize};

/// Wei per ASTRA (18 decimals).
pub const WEI_PER_ASTRA: u128 = 1_000_000_000_000_000_000;

/// The only payment asset: native ASTRA on the SpaceKit chain.
///
/// Kept as an enum so serialized receipts stay self-describing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentAsset {
    ASTRA,
}

/// What a caller must pay before a paid resource is served.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequirement {
    /// Price in ASTRA wei (decimal string, so it survives JSON number limits).
    pub amount_wei: String,
    pub asset: PaymentAsset,
    /// Chain address (`0x…`, 20 bytes) that must receive the payment.
    pub pay_to: String,
    /// Chain id of the SpaceKit network the payment must be made on.
    pub chain_id: Option<String>,
    /// Optional description shown to the payer.
    pub description: Option<String>,
}

/// Proof that a payment was made: a successful ASTRA transfer on the chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentReceipt {
    /// Hash of the chain transaction that carried the payment.
    pub tx_hash: String,
    /// Sender address (`0x…`).
    pub from: String,
    /// Recipient address (`0x…`).
    pub to: String,
    /// Amount paid, in ASTRA wei.
    pub amount_wei: u128,
    pub asset: PaymentAsset,
    /// Block the transaction was included in.
    pub block_number: u64,
    /// Unix timestamp of that block.
    pub settled_at: i64,
}

/// A movement of ASTRA to apply on the chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credit {
    /// Whose ASTRA pays for this credit. ASTRA is never created by a payment.
    pub payer_did: String,
    /// DID or address that receives it.
    pub beneficiary_did: String,
    /// Amount in ASTRA wei.
    pub amount_astra: u128,
    pub source: PaymentAsset,
    /// Receipt for the audit trail.
    pub receipt: Option<PaymentReceipt>,
}

/// Configuration for the payment service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentConfig {
    /// Network fee in basis points (default 25 = 0.25%).
    pub network_fee_bps: u32,
    /// Treasury DID that collects network fees.
    pub treasury_did: String,
}

impl Default for PaymentConfig {
    fn default() -> Self {
        Self {
            network_fee_bps: 25,
            treasury_did: "did:spacekit:0000000000000000000000000000000000000004".to_string(),
        }
    }
}

/// Validate a 20-byte chain address (`0x` + 40 hex digits).
pub fn validate_address(addr: &str) -> anyhow::Result<()> {
    let hex_part = addr
        .strip_prefix("0x")
        .ok_or_else(|| anyhow::anyhow!("address must be 0x-prefixed, got: {addr}"))?;
    anyhow::ensure!(
        hex_part.len() == 40 && hex_part.chars().all(|c| c.is_ascii_hexdigit()),
        "address must be 0x followed by 40 hex digits, got: {addr}"
    );
    Ok(())
}

/// Parse an amount of ASTRA wei from a decimal integer string.
pub fn parse_wei(amount: &str) -> anyhow::Result<u128> {
    let v: u128 = amount
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid ASTRA wei amount: {amount}"))?;
    anyhow::ensure!(v > 0, "amount must be positive");
    Ok(v)
}

/// Parse a decimal ASTRA amount ("2.5") into wei, exactly (no floats).
pub fn parse_astra(amount: &str) -> anyhow::Result<u128> {
    let s = amount.trim();
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    anyhow::ensure!(
        !whole.is_empty() || !frac.is_empty(),
        "invalid ASTRA amount: {amount}"
    );
    anyhow::ensure!(frac.len() <= 18, "ASTRA has at most 18 decimals: {amount}");
    let digits = |p: &str| p.chars().all(|c| c.is_ascii_digit());
    anyhow::ensure!(digits(whole) && digits(frac), "invalid ASTRA amount: {amount}");
    let whole: u128 = if whole.is_empty() { 0 } else { whole.parse()? };
    let frac_wei: u128 = if frac.is_empty() {
        0
    } else {
        format!("{frac:0<18}").parse()?
    };
    whole
        .checked_mul(WEI_PER_ASTRA)
        .and_then(|w| w.checked_add(frac_wei))
        .ok_or_else(|| anyhow::anyhow!("ASTRA amount too large: {amount}"))
}

/// Format wei as a decimal ASTRA string ("2.5").
pub fn format_astra(wei: u128) -> String {
    let whole = wei / WEI_PER_ASTRA;
    let frac = wei % WEI_PER_ASTRA;
    if frac == 0 {
        return whole.to_string();
    }
    let frac = format!("{frac:018}");
    format!("{whole}.{}", frac.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn astra_amounts_round_trip_exactly() {
        assert_eq!(parse_astra("1").unwrap(), WEI_PER_ASTRA);
        assert_eq!(parse_astra("2.5").unwrap(), 2_500_000_000_000_000_000);
        assert_eq!(parse_astra("0.000000000000000001").unwrap(), 1);
        assert!(parse_astra("0.0000000000000000001").is_err());
        assert!(parse_astra("1e3").is_err());
        assert_eq!(format_astra(2_500_000_000_000_000_000), "2.5");
        assert_eq!(format_astra(7 * WEI_PER_ASTRA), "7");
    }

    #[test]
    fn addresses_are_checked() {
        assert!(validate_address("0x0000000000000000000000000000000000000004").is_ok());
        assert!(validate_address("0x1234").is_err());
        assert!(validate_address("did:spacekit:abc").is_err());
    }
}
