use std::collections::HashMap;

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use parking_lot::RwLock;
use zeroize::Zeroizing;

use crate::pq_crypto::sign;
use crate::types::{EntitlementStatus, Hex32, SlaQuote, SlaTier};

pub struct PaymentsState {
    pricing_sk: Zeroizing<Vec<u8>>,
    entitlements: RwLock<HashMap<Hex32, EntitlementStatus>>,
}

impl PaymentsState {
    pub fn new(pricing_sk: Zeroizing<Vec<u8>>) -> Self {
        Self {
            pricing_sk,
            entitlements: RwLock::new(HashMap::new()),
        }
    }

    pub fn quote(&self, subject: &str, tier: SlaTier) -> Result<SlaQuote> {
        let (tier_label, amount, days) = match tier {
            SlaTier::Shield => ("Shield — monthly".to_string(), "10 USDC".to_string(), 30),
            SlaTier::ShieldAnnual => ("Shield — annual".to_string(), "96 USDC".to_string(), 365),
        };
        let valid_until = unix_now() + 600;
        let body = serde_json::json!({
            "subject": subject,
            "tier": tier,
            "duration_days": days,
            "valid_until": valid_until,
        });
        let sig = sign(&self.pricing_sk, body.to_string().as_bytes())?;
        Ok(SlaQuote {
            subject: subject.to_string(),
            tier,
            tier_label,
            token: "USDC".into(),
            amount_display: amount,
            duration_days: days,
            valid_until,
            quote_sig: B64.encode(sig),
        })
    }

    pub fn pay(&self, quote: &SlaQuote) -> Result<(String, i64)> {
        let now = unix_now();
        let mut ent = self.entitlements.write();
        let base = ent
            .get(&quote.subject)
            .and_then(|e| e.paid_until)
            .unwrap_or(0)
            .max(now);
        let paid_until = base + quote.duration_days as i64 * 86400;
        ent.insert(
            quote.subject.clone(),
            EntitlementStatus {
                active: true,
                paid_until: Some(paid_until),
                tier: Some(quote.tier_label.clone()),
            },
        );
        Ok(("0xmock_tx".into(), paid_until))
    }

    pub fn status(&self, subject: &str) -> EntitlementStatus {
        self.entitlements
            .read()
            .get(subject)
            .cloned()
            .unwrap_or(EntitlementStatus {
                active: false,
                paid_until: None,
                tier: None,
            })
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
