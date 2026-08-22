//! Content access evaluation for published FactPackages.

#![deny(clippy::all)]

use anyhow::Result;
use spacekit_primitives::v1::fact::{AccessCondition, AccessPolicy, ConditionType, FactPackage};
use std::path::{Path, PathBuf};

use crate::content_grants::ContentGrantStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentAccessDecision {
    Allowed,
    PaymentRequired {
        price: String,
        currency: String,
        content_id_hex: String,
    },
    SubscriptionRequired {
        channel_did: String,
    },
    Denied {
        reason: String,
    },
}

pub fn channel_did_from_fact(fact: &FactPackage) -> Option<String> {
    fact.metadata
        .tags
        .iter()
        .find(|t| t.starts_with("channel:"))
        .map(|t| t.trim_start_matches("channel:").to_string())
        .or_else(|| {
            if let spacekit_primitives::v1::fact::FactContent::Json { data, .. } = &fact.content {
                data.get("channel_did")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            } else {
                None
            }
        })
}

pub fn evaluate_content_access(
    fact: &FactPackage,
    requester_did: &str,
    grants: &ContentGrantStore,
) -> Result<ContentAccessDecision> {
    let content_id_hex = hex::encode(fact.fact_id);
    let author = fact.author.as_str();

    if requester_did == author {
        return Ok(ContentAccessDecision::Allowed);
    }

    if grants.has_keychain_content_access(requester_did, author, &content_id_hex) {
        return Ok(ContentAccessDecision::Allowed);
    }

    if grants.has_content_grant(requester_did, &content_id_hex) {
        return Ok(ContentAccessDecision::Allowed);
    }

    if let Some(channel) = channel_did_from_fact(fact) {
        if grants.has_channel_subscription(requester_did, &channel) {
            return Ok(ContentAccessDecision::Allowed);
        }
    }

    match &fact.access_policy {
        AccessPolicy::Public => {
            if let Some(decision) =
                payment_required_from_tags(fact, requester_did, &content_id_hex, grants)?
            {
                return Ok(decision);
            }
            Ok(ContentAccessDecision::Allowed)
        }
        AccessPolicy::Private(_) => Ok(ContentAccessDecision::Denied {
            reason: "private content".into(),
        }),
        AccessPolicy::Conditional(conditions) => {
            evaluate_conditional(conditions, requester_did, &content_id_hex, fact, grants)
        }
        _ => Ok(ContentAccessDecision::Denied {
            reason: "access policy not satisfied".into(),
        }),
    }
}

fn payment_required_from_tags(
    fact: &FactPackage,
    requester_did: &str,
    content_id_hex: &str,
    grants: &ContentGrantStore,
) -> Result<Option<ContentAccessDecision>> {
    let pricing = fact
        .metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("pricing:").map(str::to_string));
    let Some(pricing) = pricing else {
        return Ok(None);
    };
    if pricing != "pay_per_view" && pricing != "mixed" {
        return Ok(None);
    }
    let price = fact
        .metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("price:").map(str::to_string))
        .unwrap_or_else(|| "0".into());
    if price == "0" {
        return Ok(None);
    }
    if grants.has_content_grant(requester_did, content_id_hex) {
        return Ok(Some(ContentAccessDecision::Allowed));
    }
    Ok(Some(ContentAccessDecision::PaymentRequired {
        price,
        currency: "ASTRA".into(),
        content_id_hex: content_id_hex.to_string(),
    }))
}

fn evaluate_conditional(
    conditions: &[AccessCondition],
    requester_did: &str,
    content_id_hex: &str,
    fact: &FactPackage,
    grants: &ContentGrantStore,
) -> Result<ContentAccessDecision> {
    let mut payment_required: Option<ContentAccessDecision> = None;
    let mut subscription_required: Option<ContentAccessDecision> = None;

    for condition in conditions {
        match condition.condition_type {
            ConditionType::PaymentRequired => {
                let price = condition
                    .parameters
                    .get("price")
                    .cloned()
                    .unwrap_or_else(|| "0".into());
                let currency = condition
                    .parameters
                    .get("currency")
                    .cloned()
                    .unwrap_or_else(|| "ASTRA".into());
                let cid = condition
                    .parameters
                    .get("content_id")
                    .cloned()
                    .unwrap_or_else(|| content_id_hex.to_string());
                if grants.has_content_grant(requester_did, &cid) {
                    return Ok(ContentAccessDecision::Allowed);
                }
                payment_required = Some(ContentAccessDecision::PaymentRequired {
                    price,
                    currency,
                    content_id_hex: cid,
                });
            }
            ConditionType::TrustLevel => {
                if condition
                    .parameters
                    .get("subscription_required")
                    .map(|s| s == "true")
                    .unwrap_or(false)
                {
                    let channel = condition
                        .parameters
                        .get("channel_id")
                        .cloned()
                        .or_else(|| channel_did_from_fact(fact))
                        .unwrap_or_default();
                    if grants.has_channel_subscription(requester_did, &channel) {
                        return Ok(ContentAccessDecision::Allowed);
                    }
                    subscription_required = Some(ContentAccessDecision::SubscriptionRequired {
                        channel_did: channel,
                    });
                }
            }
            _ => {}
        }
    }

    if let Some(decision) = payment_required {
        return Ok(decision);
    }
    if let Some(decision) = subscription_required {
        return Ok(decision);
    }
    Ok(ContentAccessDecision::Denied {
        reason: "conditional policy not satisfied".into(),
    })
}

/// Sync grant check for `access_policy` HTTP path.
pub fn payment_grant_satisfied(
    data_dir: Option<&Path>,
    requester_did: &str,
    content_id_hex: &str,
) -> bool {
    let dir = data_dir.map(Path::to_path_buf).or_else(|| {
        std::env::var("SPACEKIT_DATA_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from)
    });
    let Some(dir) = dir.as_deref() else {
        return false;
    };
    ContentGrantStore::from_env_or_data_dir(dir).has_content_grant(requester_did, content_id_hex)
}
