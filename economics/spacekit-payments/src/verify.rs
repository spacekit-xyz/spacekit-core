//! Verifying ASTRA payments against the chain.
//!
//! A payment is a successful transaction on the SpaceKit chain that moved at
//! least the required ASTRA to the required address. The payer proves it by
//! naming the transaction hash; the verifier looks the transaction up in the
//! chain and checks it. Nothing is taken on the payer's word.

use crate::types::*;
use anyhow::{bail, ensure, Result};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// A value transfer as recorded on the chain.
#[derive(Debug, Clone)]
pub struct ChainTransfer {
    pub tx_hash: String,
    /// Sender address, `0x…`.
    pub from: String,
    /// Recipient address, `0x…`.
    pub to: String,
    /// Value moved, in ASTRA wei.
    pub value_wei: u128,
    /// Whether the transaction succeeded (a failed one moved nothing).
    pub success: bool,
    pub block_number: u64,
    pub block_timestamp: i64,
}

/// Read access to the chain's transactions.
pub trait ChainLookup: Send + Sync {
    /// The transaction with this hash, if it is in a block.
    fn transfer(&self, tx_hash: &str) -> Option<ChainTransfer>;
    /// Current head block number.
    fn head(&self) -> u64;
}

/// Checks payment proofs and refuses to accept one transaction twice.
///
/// The set of used transactions is kept in memory. A service that grants
/// something lasting for a payment must also record the transaction hash
/// with the grant, so a restart cannot make it accept the same payment again.
pub struct PaymentVerifier {
    chain: Arc<dyn ChainLookup>,
    min_confirmations: u64,
    used: Mutex<HashSet<String>>,
}

fn same_address(a: &str, b: &str) -> bool {
    a.trim_start_matches("0x").eq_ignore_ascii_case(b.trim_start_matches("0x"))
}

impl PaymentVerifier {
    pub fn new(chain: Arc<dyn ChainLookup>) -> Self {
        Self {
            chain,
            min_confirmations: 1,
            used: Mutex::new(HashSet::new()),
        }
    }

    /// Blocks that must follow the payment's block (1 = included).
    pub fn with_min_confirmations(mut self, n: u64) -> Self {
        self.min_confirmations = n.max(1);
        self
    }

    /// Check that `tx_hash` pays `requirement` and has not been used before.
    /// On success the transaction is marked used.
    pub fn verify(&self, tx_hash: &str, requirement: &PaymentRequirement) -> Result<PaymentReceipt> {
        let receipt = self.check(tx_hash, requirement)?;
        let key = receipt.tx_hash.trim_start_matches("0x").to_ascii_lowercase();
        if !self.used.lock().unwrap().insert(key) {
            bail!("payment {tx_hash} has already been used");
        }
        Ok(receipt)
    }

    /// Check without marking the transaction used.
    pub fn check(&self, tx_hash: &str, requirement: &PaymentRequirement) -> Result<PaymentReceipt> {
        let required = parse_wei(&requirement.amount_wei)?;
        let Some(tx) = self.chain.transfer(tx_hash) else {
            bail!("transaction {tx_hash} is not on the chain");
        };
        ensure!(tx.success, "transaction {tx_hash} failed, so it moved no ASTRA");
        ensure!(
            same_address(&tx.to, &requirement.pay_to),
            "transaction {tx_hash} paid {}, not {}",
            tx.to,
            requirement.pay_to
        );
        ensure!(
            tx.value_wei >= required,
            "transaction {tx_hash} paid {} ASTRA, {} ASTRA required",
            format_astra(tx.value_wei),
            format_astra(required)
        );
        let confirmations = self.chain.head().saturating_sub(tx.block_number) + 1;
        ensure!(
            confirmations >= self.min_confirmations,
            "transaction {tx_hash} has {confirmations} of {} confirmations",
            self.min_confirmations
        );
        Ok(PaymentReceipt {
            tx_hash: tx.tx_hash,
            from: tx.from,
            to: tx.to,
            amount_wei: tx.value_wei,
            asset: PaymentAsset::ASTRA,
            block_number: tx.block_number,
            settled_at: tx.block_timestamp,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeChain(Vec<ChainTransfer>);
    impl ChainLookup for FakeChain {
        fn transfer(&self, tx_hash: &str) -> Option<ChainTransfer> {
            self.0.iter().find(|t| t.tx_hash == tx_hash).cloned()
        }
        fn head(&self) -> u64 {
            10
        }
    }

    const SHOP: &str = "0x00000000000000000000000000000000000000aa";

    fn tx(hash: &str, to: &str, value: u128, success: bool) -> ChainTransfer {
        ChainTransfer {
            tx_hash: hash.into(),
            from: "0x00000000000000000000000000000000000000bb".into(),
            to: to.into(),
            value_wei: value,
            success,
            block_number: 9,
            block_timestamp: 1_700_000_000,
        }
    }

    fn requirement(amount: u128) -> PaymentRequirement {
        PaymentRequirement {
            amount_wei: amount.to_string(),
            asset: PaymentAsset::ASTRA,
            pay_to: SHOP.into(),
            chain_id: None,
            description: None,
        }
    }

    #[test]
    fn accepts_a_matching_payment_once() {
        let chain = FakeChain(vec![
            tx("0x01", SHOP, 5 * WEI_PER_ASTRA, true),
            tx("0x02", SHOP, 5 * WEI_PER_ASTRA, false),
            tx("0x03", "0x00000000000000000000000000000000000000cc", 5 * WEI_PER_ASTRA, true),
            tx("0x04", SHOP, WEI_PER_ASTRA, true),
        ]);
        let v = PaymentVerifier::new(Arc::new(chain));
        let req = requirement(5 * WEI_PER_ASTRA);

        let receipt = v.verify("0x01", &req).unwrap();
        assert_eq!(receipt.amount_wei, 5 * WEI_PER_ASTRA);
        assert!(v.verify("0x01", &req).unwrap_err().to_string().contains("already been used"));
        assert!(v.verify("0x02", &req).unwrap_err().to_string().contains("failed"));
        assert!(v.verify("0x03", &req).unwrap_err().to_string().contains("not 0x"));
        assert!(v.verify("0x04", &req).unwrap_err().to_string().contains("required"));
        assert!(v.verify("0x05", &req).unwrap_err().to_string().contains("not on the chain"));
    }

    #[test]
    fn waits_for_confirmations() {
        let v = PaymentVerifier::new(Arc::new(FakeChain(vec![tx("0x01", SHOP, 1, true)])))
            .with_min_confirmations(5);
        assert!(v.verify("0x01", &requirement(1)).unwrap_err().to_string().contains("confirmations"));
    }
}
