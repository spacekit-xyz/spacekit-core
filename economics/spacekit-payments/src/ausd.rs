//! aUSD Vault Bridge
//!
//! Converts aUSD vault charges (signed by the user via EIP-191 RouteKit relay
//! or website API) into verified `PaymentReceipt`s that the `FeeRouter` can
//! process into ASTRA VM credits.
//!
//! ## Flow
//! 1. User deposits USDC/ETH on-chain → website API mints internal aUSD.
//! 2. User sends a signed vault-charge request (amount, nonce, signature).
//! 3. This module verifies the signature and deducts from the cached balance.
//! 4. A `PaymentReceipt` with asset=AUSD is emitted for the `FeeRouter`.

use crate::types::*;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Represents a signed vault charge request from a user.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VaultChargeRequest {
    /// DID of the user being charged.
    pub user_did: String,
    /// Amount of aUSD to charge (2 decimal places, e.g. "1.50").
    pub amount_ausd: String,
    /// Monotonically increasing nonce to prevent replay.
    pub nonce: u64,
    /// EIP-191 signature over `keccak256(did || amount || nonce)`.
    /// For compute-node-originated charges, this can be the node's own signature.
    pub signature: String,
    /// Optional: what the charge is for.
    pub description: Option<String>,
}

/// Tracks aUSD balances and nonces per DID for deduplication.
pub struct AusdVault {
    balances: Arc<RwLock<HashMap<String, f64>>>,
    nonces: Arc<RwLock<HashMap<String, u64>>>,
}

impl AusdVault {
    pub fn new() -> Self {
        Self {
            balances: Arc::new(RwLock::new(HashMap::new())),
            nonces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Credit a user's aUSD balance (called when deposits arrive from the website API).
    pub async fn credit(&self, did: &str, amount: f64) {
        let mut balances = self.balances.write().await;
        let bal = balances.entry(did.to_string()).or_insert(0.0);
        *bal += amount;
        info!("aUSD credited: {} now has {:.2} aUSD", did, bal);
    }

    /// Get a user's current aUSD balance.
    pub async fn balance_of(&self, did: &str) -> f64 {
        let balances = self.balances.read().await;
        balances.get(did).copied().unwrap_or(0.0)
    }

    /// Process a vault charge request and produce a `PaymentReceipt`.
    ///
    /// Checks:
    /// 1. Nonce is strictly greater than the last seen nonce for this DID
    /// 2. Balance is sufficient
    /// 3. Signature is valid (TODO: full EIP-191 verification)
    pub async fn process_charge(&self, req: &VaultChargeRequest) -> Result<PaymentReceipt> {
        let amount: f64 = req
            .amount_ausd
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid aUSD amount: {}", req.amount_ausd))?;
        anyhow::ensure!(amount > 0.0, "Charge amount must be positive");

        // Nonce replay protection
        {
            let mut nonces = self.nonces.write().await;
            let last = nonces.get(&req.user_did).copied().unwrap_or(0);
            anyhow::ensure!(
                req.nonce > last,
                "Nonce {} is not greater than last seen nonce {}",
                req.nonce,
                last
            );
            nonces.insert(req.user_did.clone(), req.nonce);
        }

        // Balance check + deduction
        {
            let mut balances = self.balances.write().await;
            let bal = balances.entry(req.user_did.clone()).or_insert(0.0);
            anyhow::ensure!(
                *bal >= amount,
                "Insufficient aUSD balance: have {:.2}, need {:.2}",
                bal,
                amount
            );
            *bal -= amount;
        }

        // TODO: verify EIP-191 signature over (did, amount, nonce).
        // For now, trust requests from authenticated compute-node endpoints.

        let receipt = PaymentReceipt {
            tx_hash: format!("ausd:{}:{}", req.user_did, req.nonce),
            amount: req.amount_ausd.clone(),
            asset: PaymentAsset::AUSD,
            network: None,
            settled_at: chrono::Utc::now().timestamp(),
        };

        info!(
            "aUSD vault charged: {} for {:.2} aUSD (nonce {})",
            req.user_did, amount, req.nonce
        );

        Ok(receipt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vault_charge_flow() {
        let vault = AusdVault::new();

        // Credit 10 aUSD
        vault.credit("did:alice", 10.0).await;
        assert_eq!(vault.balance_of("did:alice").await, 10.0);

        // Charge 3.50 aUSD
        let req = VaultChargeRequest {
            user_did: "did:alice".to_string(),
            amount_ausd: "3.50".to_string(),
            nonce: 1,
            signature: "mock".to_string(),
            description: None,
        };
        let receipt = vault.process_charge(&req).await.unwrap();
        assert_eq!(receipt.amount, "3.50");
        assert_eq!(vault.balance_of("did:alice").await, 6.5);
    }

    #[tokio::test]
    async fn test_insufficient_balance() {
        let vault = AusdVault::new();
        vault.credit("did:bob", 1.0).await;

        let req = VaultChargeRequest {
            user_did: "did:bob".to_string(),
            amount_ausd: "5.00".to_string(),
            nonce: 1,
            signature: "mock".to_string(),
            description: None,
        };
        assert!(vault.process_charge(&req).await.is_err());
    }

    #[tokio::test]
    async fn test_nonce_replay_rejection() {
        let vault = AusdVault::new();
        vault.credit("did:carol", 100.0).await;

        let req = VaultChargeRequest {
            user_did: "did:carol".to_string(),
            amount_ausd: "1.00".to_string(),
            nonce: 1,
            signature: "mock".to_string(),
            description: None,
        };
        vault.process_charge(&req).await.unwrap();

        // Replay same nonce → rejected
        assert!(vault.process_charge(&req).await.is_err());

        // Higher nonce → accepted
        let req2 = VaultChargeRequest { nonce: 2, ..req };
        vault.process_charge(&req2).await.unwrap();
    }
}
