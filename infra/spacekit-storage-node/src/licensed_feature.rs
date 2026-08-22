//! `spacekit:licensed_feature:v1` — library-embedded features (e.g. growformer in CLI).
//!
//! See `GROWFORMER_SPEC.md` §6.

#![deny(clippy::all)]

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use spacekit_primitives::v1::fact::{FactContent, FactPackage};

pub const LICENSED_FEATURE_SCHEMA: &str = "spacekit:licensed_feature:v1";

pub const CAP_TRAIN: &str = "growformer.train";
pub const CAP_INFER: &str = "growformer.infer";
pub const CAP_MERGE: &str = "growformer.merge";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeatureCapability {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeaturePrice {
    pub amount_wei: String,
    pub currency: String,
    pub on_network: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeatureQuota {
    pub operations: u64,
    pub period_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeatureTier {
    pub name: String,
    #[serde(default)]
    pub license_type: Option<String>,
    #[serde(default)]
    pub price: Option<FeaturePrice>,
    pub entitlement_duration_seconds: Option<u64>,
    #[serde(default)]
    pub grant_type: Option<String>,
    #[serde(default)]
    pub eligibility: Option<String>,
    pub capabilities_included: Vec<String>,
    #[serde(default)]
    pub quota: Option<FeatureQuota>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LicensedFeatureDocument {
    pub schema: String,
    pub feature_name: String,
    pub feature_version: String,
    #[serde(default)]
    pub minimum_cli_version: Option<String>,
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub publisher_name: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<FeatureCapability>,
    pub tiers: Vec<FeatureTier>,
    #[serde(default)]
    pub publisher_did: Option<String>,
    #[serde(default)]
    pub storage_operator_did: Option<String>,
    #[serde(default)]
    pub published_at: Option<u64>,
}

impl LicensedFeatureDocument {
    pub fn validate(&self) -> Result<()> {
        if self.schema != LICENSED_FEATURE_SCHEMA {
            return Err(anyhow!(
                "expected schema {}, got {}",
                LICENSED_FEATURE_SCHEMA,
                self.schema
            ));
        }
        if self.feature_name.trim().is_empty() {
            return Err(anyhow!("feature_name is required"));
        }
        if self.tiers.is_empty() {
            return Err(anyhow!("at least one tier is required"));
        }
        Ok(())
    }

    pub fn tier(&self, name: &str) -> Option<&FeatureTier> {
        self.tiers
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    pub fn default_tier(&self) -> Option<&FeatureTier> {
        self.tier("free").or_else(|| self.tiers.first())
    }

    pub fn capabilities_for_tier(&self, tier_name: &str) -> Vec<String> {
        self.tier(tier_name)
            .map(|t| t.capabilities_included.clone())
            .unwrap_or_default()
    }

    pub fn quota_for_tier(&self, tier_name: &str) -> Option<u64> {
        self.tier(tier_name)
            .and_then(|t| t.quota.as_ref())
            .map(|q| q.operations)
    }

    pub fn grant_duration_for_tier(&self, tier_name: &str) -> Option<u64> {
        self.tier(tier_name)
            .and_then(|t| t.entitlement_duration_seconds)
    }

    /// ASTRA price for a tier (`amount_wei` / 1e9 per GROWFORMER_SPEC §6.2).
    pub fn tier_price_astra(&self, tier_name: &str) -> Option<f64> {
        self.tier(tier_name).and_then(tier_price_astra)
    }

    pub fn feature_tag(&self) -> String {
        format!("feature:{}", self.feature_name)
    }
}

/// Default growformer feature document (matches GROWFORMER_SPEC.md §6.2 tiers).
pub fn default_growformer_feature(
    publisher_did: &str,
    title: &str,
    description: &str,
) -> LicensedFeatureDocument {
    LicensedFeatureDocument {
        schema: LICENSED_FEATURE_SCHEMA.to_string(),
        feature_name: "growformer".to_string(),
        feature_version: env!("CARGO_PKG_VERSION").to_string(),
        minimum_cli_version: Some("0.1.0".to_string()),
        title: title.to_string(),
        description: description.to_string(),
        publisher_name: Some("SWTCH Labs".to_string()),
        capabilities: vec![
            FeatureCapability {
                name: CAP_TRAIN.to_string(),
                description: "End-to-end brain training".to_string(),
            },
            FeatureCapability {
                name: CAP_INFER.to_string(),
                description: "Inference and batch/REPL".to_string(),
            },
            FeatureCapability {
                name: CAP_MERGE.to_string(),
                description: "Merge overlay brains".to_string(),
            },
        ],
        tiers: default_growformer_tiers(),
        publisher_did: Some(publisher_did.to_string()),
        storage_operator_did: None,
        published_at: Some(chrono::Utc::now().timestamp() as u64),
    }
}

pub fn default_growformer_tiers() -> Vec<FeatureTier> {
    vec![
        FeatureTier {
            name: "free".to_string(),
            license_type: Some("Personal".to_string()),
            price: Some(FeaturePrice {
                amount_wei: "0".to_string(),
                currency: "ASTRA".to_string(),
                on_network: "spacekit".to_string(),
            }),
            entitlement_duration_seconds: Some(2_592_000),
            grant_type: Some("Free".to_string()),
            eligibility: Some("OpenToAll".to_string()),
            capabilities_included: vec![
                CAP_TRAIN.to_string(),
                CAP_INFER.to_string(),
                CAP_MERGE.to_string(),
            ],
            quota: Some(FeatureQuota {
                operations: 1000,
                period_seconds: 2_592_000,
            }),
        },
        FeatureTier {
            name: "personal".to_string(),
            license_type: Some("Personal".to_string()),
            price: Some(FeaturePrice {
                amount_wei: "20000000000".to_string(),
                currency: "ASTRA".to_string(),
                on_network: "spacekit".to_string(),
            }),
            entitlement_duration_seconds: Some(2_592_000),
            grant_type: Some("Subscription".to_string()),
            eligibility: Some("PaymentRequired".to_string()),
            capabilities_included: vec![
                CAP_TRAIN.to_string(),
                CAP_INFER.to_string(),
                CAP_MERGE.to_string(),
            ],
            quota: None,
        },
        FeatureTier {
            name: "commercial".to_string(),
            license_type: Some("Commercial".to_string()),
            price: Some(FeaturePrice {
                amount_wei: "200000000000".to_string(),
                currency: "ASTRA".to_string(),
                on_network: "spacekit".to_string(),
            }),
            entitlement_duration_seconds: Some(2_592_000),
            grant_type: Some("Subscription".to_string()),
            eligibility: Some("PaymentRequired".to_string()),
            capabilities_included: vec![
                CAP_TRAIN.to_string(),
                CAP_INFER.to_string(),
                CAP_MERGE.to_string(),
            ],
            quota: None,
        },
    ]
}

/// Parse tier price in ASTRA (`amount_wei` is fixed-point with 1e9 units per ASTRA).
pub fn tier_price_astra(tier: &FeatureTier) -> Option<f64> {
    let wei = tier.price.as_ref()?.amount_wei.parse::<u64>().ok()?;
    Some(wei as f64 / 1_000_000_000.0)
}

pub fn tier_requires_payment(tier: &FeatureTier) -> bool {
    tier.eligibility
        .as_deref()
        .is_some_and(|e| e.eq_ignore_ascii_case("PaymentRequired"))
        || tier_price_astra(tier).is_some_and(|p| p > 0.0)
}

pub fn is_growformer_feature_json_bytes(payload: &[u8]) -> bool {
    serde_json::from_slice::<LicensedFeatureDocument>(payload)
        .ok()
        .map(|d| d.feature_name.eq_ignore_ascii_case("growformer"))
        .unwrap_or(false)
}

pub fn is_growformer_licensed_feature_json(value: &serde_json::Value) -> bool {
    serde_json::from_value::<LicensedFeatureDocument>(value.clone())
        .ok()
        .map(|d| d.feature_name.eq_ignore_ascii_case("growformer"))
        .unwrap_or(false)
}
pub fn parse_licensed_feature_fact(fact: &FactPackage) -> Option<LicensedFeatureDocument> {
    let FactContent::Json { data, schema } = &fact.content else {
        return None;
    };
    if schema.as_deref() != Some(LICENSED_FEATURE_SCHEMA) {
        if data.get("schema").and_then(|v| v.as_str()) != Some(LICENSED_FEATURE_SCHEMA) {
            return None;
        }
    }
    serde_json::from_value(data.clone()).ok()
}

pub fn is_growformer_licensed_feature(fact: &FactPackage) -> bool {
    parse_licensed_feature_fact(fact)
        .map(|d| d.feature_name.eq_ignore_ascii_case("growformer"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_growformer_tiers_resolve_capabilities() {
        let doc = default_growformer_feature("did:spacekit:test", "Growformer", "test");
        doc.validate().unwrap();
        let caps = doc.capabilities_for_tier("free");
        assert!(caps.contains(&CAP_TRAIN.to_string()));
        assert_eq!(doc.quota_for_tier("free"), Some(1000));
        assert_eq!(doc.quota_for_tier("personal"), None);
        assert_eq!(doc.tier_price_astra("personal"), Some(20.0));
        assert_eq!(doc.tier_price_astra("commercial"), Some(200.0));
    }

    #[test]
    fn tier_requires_payment_flags() {
        let doc = default_growformer_feature("did:spacekit:test", "Growformer", "test");
        let free = doc.tier("free").unwrap();
        let personal = doc.tier("personal").unwrap();
        assert!(!tier_requires_payment(free));
        assert!(tier_requires_payment(personal));
    }
}
