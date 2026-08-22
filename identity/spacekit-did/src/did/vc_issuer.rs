// VPN access VC issuance
use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::did_wallet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VpnAccessCredential {
    pub id: String,
    pub subject_did: String,
    pub plan: String,
    pub expires_at: DateTime<Utc>,
    pub issuer_did: String,
}

pub trait VcIssuer {
    fn issue_vpn_access(
        &self,
        subject_did: &str,
        plan: &str,
        expires_at: DateTime<Utc>,
    ) -> anyhow::Result<String>; // serialized VC
}

pub struct DidBasedVcIssuer<W: did_wallet::DidWallet> {
    pub issuer_did: String,
    pub wallet: W,
}

impl<W: did_wallet::DidWallet> VcIssuer for DidBasedVcIssuer<W> {
    fn issue_vpn_access(
        &self,
        subject_did: &str,
        plan: &str,
        expires_at: DateTime<Utc>,
    ) -> anyhow::Result<String> {
        let vc = VpnAccessCredential {
            id: uuid::Uuid::new_v4().to_string(),
            subject_did: subject_did.to_string(),
            plan: plan.to_string(),
            expires_at,
            issuer_did: self.issuer_did.clone(),
        };

        let payload = serde_json::to_vec(&vc)?;
        let sig = self.wallet.sign(&self.issuer_did, &payload)?;
        // wrap as JWS/LD-Proof; here we just bundle for simplicity
        let wrapped = serde_json::json!({
            "vc": vc,
            "proof": {
                "type": "SpacekitSignature2026",
                "signature": base64::engine::general_purpose::STANDARD.encode(&sig),
            }
        });

        Ok(serde_json::to_string(&wrapped)?)
    }
}
