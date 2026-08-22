//! Payment verification and refund-on-grant-failure for content access (Sprint 2).

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::content_entitlement::{
    channel_listing_id, content_listing_id, parse_entitlement_id_hex, EntitlementClient,
    EntitlementVerifyStatus,
};

pub const PAYMENTS_FILE: &str = "verified_payments.json";
pub const REFUNDS_FILE: &str = "refund_log.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifiedPayment {
    pub reference: String,
    pub payer_did: String,
    pub recipient_did: String,
    pub amount_astra: f64,
    /// `content:{hex}` or `channel:{did}`
    pub scope: String,
    pub consumed: bool,
    pub recorded_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PaymentStoreFile {
    payments: Vec<VerifiedPayment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefundRecord {
    pub payment_reference: String,
    pub reason: String,
    pub refunded_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RefundStoreFile {
    refunds: Vec<RefundRecord>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaymentVerifyError {
    NotFound,
    AmountTooSmall { required: f64, got: f64 },
    WrongRecipient { expected: String, got: String },
    DuplicateReference,
    InvalidReference(String),
    EntitlementInvalid(EntitlementVerifyStatus),
    Unconfigured,
}

impl std::fmt::Display for PaymentVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "payment not found"),
            Self::AmountTooSmall { required, got } => {
                write!(f, "payment too small: required {required}, got {got}")
            }
            Self::WrongRecipient { expected, got } => {
                write!(f, "wrong recipient: expected {expected}, got {got}")
            }
            Self::DuplicateReference => write!(f, "duplicate payment reference"),
            Self::InvalidReference(s) => write!(f, "invalid payment reference: {s}"),
            Self::EntitlementInvalid(s) => write!(f, "entitlement invalid: {:?}", s),
            Self::Unconfigured => write!(f, "payment verification not configured"),
        }
    }
}

impl std::error::Error for PaymentVerifyError {}

pub struct PaymentReceiptStore {
    path: PathBuf,
    refunds_path: PathBuf,
}

impl PaymentReceiptStore {
    pub fn new(data_dir: &Path) -> Self {
        let dir = data_dir.join("content_payments");
        Self {
            path: dir.join(PAYMENTS_FILE),
            refunds_path: dir.join(REFUNDS_FILE),
        }
    }

    pub fn from_env_or_data_dir(data_dir: &Path) -> Self {
        if let Ok(p) = std::env::var("SPACEKIT_CONTENT_PAYMENTS_FILE") {
            if !p.trim().is_empty() {
                let path = PathBuf::from(p);
                let refunds_path = path
                    .parent()
                    .map(|parent| parent.join(REFUNDS_FILE))
                    .unwrap_or_else(|| data_dir.join("content_payments").join(REFUNDS_FILE));
                return Self { path, refunds_path };
            }
        }
        Self::new(data_dir)
    }

    fn load(&self) -> Result<PaymentStoreFile> {
        if !self.path.exists() {
            return Ok(PaymentStoreFile::default());
        }
        let raw = std::fs::read_to_string(&self.path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    fn save(&self, store: &PaymentStoreFile) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(store)?)?;
        Ok(())
    }

    pub fn record_payment(&self, payment: VerifiedPayment) -> Result<()> {
        let mut store = self.load()?;
        if store
            .payments
            .iter()
            .any(|p| p.reference == payment.reference)
        {
            return Err(anyhow!("duplicate payment reference"));
        }
        store.payments.push(payment);
        self.save(&store)
    }

    pub fn is_reference_consumed(&self, reference: &str) -> bool {
        self.load()
            .ok()
            .map(|s| {
                s.payments
                    .iter()
                    .any(|p| p.reference == reference && p.consumed)
            })
            .unwrap_or(false)
    }

    pub fn reference_exists(&self, reference: &str) -> bool {
        self.load()
            .ok()
            .map(|s| s.payments.iter().any(|p| p.reference == reference))
            .unwrap_or(false)
    }

    fn find_payment(&self, reference: &str) -> Result<Option<VerifiedPayment>> {
        Ok(self
            .load()?
            .payments
            .into_iter()
            .find(|p| p.reference == reference))
    }

    pub fn verify_receipt(
        &self,
        reference: &str,
        payer_did: &str,
        recipient_did: &str,
        scope: &str,
        min_amount_astra: f64,
    ) -> Result<VerifiedPayment, PaymentVerifyError> {
        if self.is_reference_consumed(reference) {
            return Err(PaymentVerifyError::DuplicateReference);
        }
        let payment = self
            .find_payment(reference)
            .map_err(|_| PaymentVerifyError::NotFound)?
            .ok_or(PaymentVerifyError::NotFound)?;
        if payment.payer_did != payer_did {
            return Err(PaymentVerifyError::InvalidReference(
                "payer DID mismatch".into(),
            ));
        }
        if payment.recipient_did != recipient_did {
            return Err(PaymentVerifyError::WrongRecipient {
                expected: recipient_did.to_string(),
                got: payment.recipient_did,
            });
        }
        if payment.scope != scope {
            return Err(PaymentVerifyError::InvalidReference(format!(
                "scope mismatch: expected {scope}, got {}",
                payment.scope
            )));
        }
        if payment.amount_astra + f64::EPSILON < min_amount_astra {
            return Err(PaymentVerifyError::AmountTooSmall {
                required: min_amount_astra,
                got: payment.amount_astra,
            });
        }
        Ok(payment)
    }

    pub fn mark_consumed(&self, reference: &str) -> Result<()> {
        let mut store = self.load()?;
        let payment = store
            .payments
            .iter_mut()
            .find(|p| p.reference == reference)
            .ok_or_else(|| anyhow!("payment not found"))?;
        payment.consumed = true;
        self.save(&store)
    }

    pub fn refund_on_grant_failure(&self, reference: &str, reason: &str) -> Result<()> {
        let mut store = self.load()?;
        if let Some(p) = store.payments.iter_mut().find(|p| p.reference == reference) {
            p.consumed = false;
        }
        self.save(&store)?;
        if let Some(parent) = self.refunds_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut refunds: RefundStoreFile = if self.refunds_path.exists() {
            serde_json::from_str(&std::fs::read_to_string(&self.refunds_path)?).unwrap_or_default()
        } else {
            RefundStoreFile::default()
        };
        refunds.refunds.push(RefundRecord {
            payment_reference: reference.to_string(),
            reason: reason.to_string(),
            refunded_at: chrono::Utc::now().timestamp() as u64,
        });
        std::fs::write(&self.refunds_path, serde_json::to_vec_pretty(&refunds)?)?;
        Ok(())
    }
}

/// Verify payment for content PPV or channel subscription.
#[cfg(feature = "reqwest")]
pub async fn verify_content_payment(
    data_dir: &Path,
    reference: &str,
    payer_did: &str,
    recipient_did: &str,
    scope: &str,
    content_id_hex: Option<&str>,
    min_amount_astra: f64,
) -> Result<(), PaymentVerifyError> {
    if reference.len() == 64 && reference.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Ok(ent_id) = parse_entitlement_id_hex(reference) {
            let file_id = content_id_hex.unwrap_or("");
            if let Some(client) = EntitlementClient::from_env() {
                let pk_hash = [0u8; 32];
                let status = client.verify(&ent_id, payer_did, file_id, &pk_hash).await;
                if status.is_valid() {
                    return Ok(());
                }
                return Err(PaymentVerifyError::EntitlementInvalid(status));
            }
        }
    }

    let store = PaymentReceiptStore::from_env_or_data_dir(data_dir);
    let _ = store.verify_receipt(reference, payer_did, recipient_did, scope, min_amount_astra)?;
    Ok(())
}

#[cfg(not(feature = "reqwest"))]
pub async fn verify_content_payment(
    data_dir: &Path,
    reference: &str,
    payer_did: &str,
    recipient_did: &str,
    scope: &str,
    _content_id_hex: Option<&str>,
    min_amount_astra: f64,
) -> Result<(), PaymentVerifyError> {
    let store = PaymentReceiptStore::from_env_or_data_dir(data_dir);
    let _ = store.verify_receipt(reference, payer_did, recipient_did, scope, min_amount_astra)?;
    Ok(())
}

/// Grant after verified payment; refunds receipt if local grant fails.
pub async fn grant_after_payment<F>(data_dir: &Path, payment_ref: &str, grant: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    let store = PaymentReceiptStore::from_env_or_data_dir(data_dir);
    if store.is_reference_consumed(payment_ref) {
        return Err(anyhow!("duplicate payment reference"));
    }
    match grant() {
        Ok(()) => {
            store.mark_consumed(payment_ref)?;
            Ok(())
        }
        Err(e) => {
            let _ = store.refund_on_grant_failure(payment_ref, &e.to_string());
            Err(e)
        }
    }
}

pub fn payment_scope_content(content_id_hex: &str) -> String {
    content_listing_id(content_id_hex)
}

pub fn payment_scope_channel(channel_did: &str) -> String {
    channel_listing_id(channel_did)
}
