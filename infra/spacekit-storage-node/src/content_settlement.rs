//! SpaceKit Pay settlement → entitlement purchase orchestration (Sprint 3).

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::content_entitlement::content_listing_id;
use crate::content_grants::ContentGrantStore;
use crate::content_payment::{
    payment_scope_channel, payment_scope_content, PaymentReceiptStore, VerifiedPayment,
};

pub const PENDING_FILE: &str = "pending_purchases.json";
pub const INBOX_FILE: &str = "settlements_inbox.jsonl";
pub const PROCESSED_INBOX_FILE: &str = "processed_inbox_tx.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PurchaseKind {
    ContentPpv,
    ChannelSubscription,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPurchase {
    pub id: String,
    pub kind: PurchaseKind,
    pub buyer_did: String,
    pub publisher_did: String,
    pub listing_id: String,
    pub scope: String,
    pub content_id_hex: Option<String>,
    pub channel_did: Option<String>,
    pub price_astra: f64,
    pub price_units: u64,
    pub status: String,
    pub created_at: u64,
    pub entitlement_id_hex: Option<String>,
    pub payment_reference: Option<String>,
    /// Licensed-feature tier granted on completion (e.g. `personal`).
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub grant_expires_at: Option<u64>,
    #[serde(default)]
    pub quota_remaining: Option<u64>,
}

/// Optional grant metadata for licensed-feature purchases.
#[derive(Debug, Clone, Default)]
pub struct PendingGrantOptions {
    pub tier: Option<String>,
    pub grant_expires_at: Option<u64>,
    pub quota_remaining: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingStore {
    purchases: Vec<PendingPurchase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementReceipt {
    pub tx_hash: String,
    pub amount: String,
    pub asset: String,
    pub payer_did: String,
    pub beneficiary_did: String,
    pub scope: String,
    pub settled_at: i64,
}

pub struct ContentSettlementStore {
    data_dir: PathBuf,
}

impl ContentSettlementStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            data_dir: data_dir.to_path_buf(),
        }
    }

    fn pending_path(&self) -> PathBuf {
        self.data_dir.join("content_payments").join(PENDING_FILE)
    }

    fn inbox_path(&self) -> PathBuf {
        self.data_dir.join("content_payments").join(INBOX_FILE)
    }

    fn processed_inbox_path(&self) -> PathBuf {
        self.data_dir
            .join("content_payments")
            .join(PROCESSED_INBOX_FILE)
    }

    fn load_processed_inbox(&self) -> Result<Vec<String>> {
        let path = self.processed_inbox_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(&path)?).unwrap_or_default())
    }

    fn save_processed_inbox(&self, tx_hashes: &[String]) -> Result<()> {
        let path = self.processed_inbox_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(tx_hashes)?)?;
        Ok(())
    }

    pub fn mark_inbox_processed(&self, tx_hash: &str) -> Result<()> {
        let mut done = self.load_processed_inbox()?;
        if !done.iter().any(|t| t == tx_hash) {
            done.push(tx_hash.to_string());
            self.save_processed_inbox(&done)?;
        }
        Ok(())
    }

    pub fn is_inbox_processed(&self, tx_hash: &str) -> bool {
        self.load_processed_inbox()
            .ok()
            .map(|v| v.iter().any(|t| t == tx_hash))
            .unwrap_or(false)
    }

    fn load_pending(&self) -> Result<PendingStore> {
        let path = self.pending_path();
        if !path.exists() {
            return Ok(PendingStore {
                purchases: Vec::new(),
            });
        }
        Ok(serde_json::from_str(&std::fs::read_to_string(&path)?)?)
    }

    fn save_pending(&self, store: &PendingStore) -> Result<()> {
        let path = self.pending_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_vec_pretty(store)?)?;
        Ok(())
    }

    pub fn create_pending(
        &self,
        kind: PurchaseKind,
        buyer_did: &str,
        publisher_did: &str,
        content_id_hex: Option<&str>,
        channel_did: Option<&str>,
        price_astra: f64,
        grant_opts: Option<PendingGrantOptions>,
    ) -> Result<PendingPurchase> {
        let grant_opts = grant_opts.unwrap_or_default();
        let listing_id = match (&kind, content_id_hex, channel_did) {
            (PurchaseKind::ContentPpv, Some(cid), _) => content_listing_id(cid),
            (PurchaseKind::ChannelSubscription, _, Some(ch)) => {
                crate::content_entitlement::channel_listing_id(ch)
            }
            _ => {
                return Err(anyhow!(
                    "missing content_id or channel for pending purchase"
                ))
            }
        };
        let scope = match &kind {
            PurchaseKind::ContentPpv => payment_scope_content(content_id_hex.unwrap()),
            PurchaseKind::ChannelSubscription => payment_scope_channel(channel_did.unwrap()),
        };
        let pending = PendingPurchase {
            id: format!("pending-{}", uuid::Uuid::new_v4()),
            kind,
            buyer_did: buyer_did.to_string(),
            publisher_did: publisher_did.to_string(),
            listing_id,
            scope,
            content_id_hex: content_id_hex.map(String::from),
            channel_did: channel_did.map(String::from),
            price_astra,
            price_units: (price_astra.max(0.0) * 1_000_000.0) as u64,
            status: "awaiting_payment".into(),
            created_at: chrono::Utc::now().timestamp() as u64,
            entitlement_id_hex: None,
            payment_reference: None,
            tier: grant_opts.tier,
            grant_expires_at: grant_opts.grant_expires_at,
            quota_remaining: grant_opts.quota_remaining,
        };
        let mut store = self.load_pending()?;
        store.purchases.push(pending.clone());
        self.save_pending(&store)?;
        Ok(pending)
    }

    pub fn get_pending(&self, id: &str) -> Result<Option<PendingPurchase>> {
        Ok(self
            .load_pending()?
            .purchases
            .into_iter()
            .find(|p| p.id == id))
    }

    /// Find awaiting-payment pending for buyer + scope (latest first).
    pub fn find_open_pending_for_scope(
        &self,
        buyer_did: &str,
        scope: &str,
    ) -> Result<Option<PendingPurchase>> {
        Ok(self
            .load_pending()?
            .purchases
            .into_iter()
            .rev()
            .find(|p| p.buyer_did == buyer_did && p.scope == scope && p.status != "completed"))
    }

    pub fn push_settlement_inbox(&self, receipt: &SettlementReceipt) -> Result<()> {
        let path = self.inbox_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        writeln!(f, "{}", serde_json::to_string(receipt)?)?;
        Ok(())
    }

    pub fn list_inbox_unprocessed(&self) -> Result<Vec<SettlementReceipt>> {
        let path = self.inbox_path();
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for line in std::fs::read_to_string(&path)?.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(r) = serde_json::from_str::<SettlementReceipt>(line) {
                if !self.is_inbox_processed(&r.tx_hash) {
                    out.push(r);
                }
            }
        }
        Ok(out)
    }

    /// All purchases not yet `completed`.
    pub fn list_open_pending(&self) -> Result<Vec<PendingPurchase>> {
        Ok(self
            .load_pending()?
            .purchases
            .into_iter()
            .filter(|p| p.status != "completed")
            .collect())
    }

    /// Match an inbox receipt to the best open pending purchase.
    pub fn match_pending_for_receipt(
        &self,
        receipt: &SettlementReceipt,
    ) -> Result<Option<PendingPurchase>> {
        Ok(self
            .find_open_pending_for_scope(&receipt.payer_did, &receipt.scope)?
            .filter(|p| p.publisher_did == receipt.beneficiary_did))
    }

    /// Record verified payment, grant local cache, mark pending complete.
    pub fn complete_pending_with_entitlement(
        &self,
        pending_id: &str,
        payment_reference: &str,
        entitlement_id_hex: &str,
        period_secs: Option<u64>,
        license_token_id: Option<u64>,
    ) -> Result<()> {
        let mut store = self.load_pending()?;
        let pending = store
            .purchases
            .iter_mut()
            .find(|p| p.id == pending_id)
            .ok_or_else(|| anyhow!("pending purchase not found"))?;

        let payments = PaymentReceiptStore::from_env_or_data_dir(&self.data_dir);
        let amount: f64 = pending.price_astra;
        if !payments.reference_exists(payment_reference) {
            payments.record_payment(VerifiedPayment {
                reference: payment_reference.to_string(),
                payer_did: pending.buyer_did.clone(),
                recipient_did: pending.publisher_did.clone(),
                amount_astra: amount,
                scope: pending.scope.clone(),
                consumed: false,
                recorded_at: chrono::Utc::now().timestamp() as u64,
            })?;
        }
        if !payments.is_reference_consumed(payment_reference) {
            payments.mark_consumed(payment_reference)?;
        }

        let grants = ContentGrantStore::from_env_or_data_dir(&self.data_dir);
        match pending.kind {
            PurchaseKind::ContentPpv => {
                let cid = pending
                    .content_id_hex
                    .as_deref()
                    .ok_or_else(|| anyhow!("pending missing content_id"))?;
                grants.grant_content_ppv_full(
                    &pending.buyer_did,
                    cid,
                    Some(payment_reference.to_string()),
                    pending.grant_expires_at,
                    Some(entitlement_id_hex.to_string()),
                    pending.tier.clone(),
                    license_token_id,
                    pending.quota_remaining,
                )?;
            }
            PurchaseKind::ChannelSubscription => {
                let ch = pending
                    .channel_did
                    .as_deref()
                    .ok_or_else(|| anyhow!("pending missing channel"))?;
                let expires =
                    chrono::Utc::now().timestamp() as u64 + period_secs.unwrap_or(30 * 24 * 3600);
                grants.grant_channel_subscription(
                    &pending.buyer_did,
                    ch,
                    Some(expires),
                    Some(payment_reference.to_string()),
                )?;
            }
        }

        pending.status = "completed".into();
        pending.entitlement_id_hex = Some(entitlement_id_hex.to_string());
        pending.payment_reference = Some(payment_reference.to_string());
        self.save_pending(&store)?;
        Ok(())
    }

    /// Apply settlement from SpaceKit Pay: record payment + mark pending awaiting on-chain purchase.
    pub fn apply_settlement_to_pending(
        &self,
        pending_id: &str,
        receipt: &SettlementReceipt,
    ) -> Result<()> {
        let amount: f64 = receipt.amount.parse().unwrap_or(0.0);
        let payments = PaymentReceiptStore::from_env_or_data_dir(&self.data_dir);
        if !payments.reference_exists(&receipt.tx_hash) {
            payments.record_payment(VerifiedPayment {
                reference: receipt.tx_hash.clone(),
                payer_did: receipt.payer_did.clone(),
                recipient_did: receipt.beneficiary_did.clone(),
                amount_astra: amount,
                scope: receipt.scope.clone(),
                consumed: false,
                recorded_at: receipt.settled_at as u64,
            })?;
        }
        let mut store = self.load_pending()?;
        let pending = store
            .purchases
            .iter_mut()
            .find(|p| p.id == pending_id)
            .ok_or_else(|| anyhow!("pending purchase not found"))?;
        pending.status = "payment_settled".into();
        pending.payment_reference = Some(receipt.tx_hash.clone());
        self.save_pending(&store)?;
        self.push_settlement_inbox(receipt)?;
        Ok(())
    }
}

/// Validate a settlement receipt against a pending purchase (amount, recipient, scope).
pub fn validate_settlement_for_pending(
    pending: &PendingPurchase,
    receipt: &SettlementReceipt,
) -> Result<()> {
    let amount: f64 = receipt.amount.parse().unwrap_or(0.0);
    if amount + f64::EPSILON < pending.price_astra {
        return Err(anyhow!(
            "payment too small: required {}, got {}",
            pending.price_astra,
            amount
        ));
    }
    if receipt.beneficiary_did != pending.publisher_did {
        return Err(anyhow!("wrong payment recipient"));
    }
    if receipt.scope != pending.scope {
        return Err(anyhow!("payment scope mismatch"));
    }
    Ok(())
}
