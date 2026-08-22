//! Marketplace catalog helpers shared by `spacekit app deploy/undeploy` and
//! `spacekit content unpublish` (app manifests).

use crate::content_integration::WEBSITE_CATALOG_OWNER_DID;
use chrono::Utc;
use serde_json::{json, Value};
use spacekit_primitives::v1::fact::{
    CollectionMethod, DataSource, FactCategory, FactContent, FactMetadata, FactPackage,
    KnowledgeDomain, ProofType, VerificationLevel, VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;

/// Well-known marketplace index fact ID = sha256("spacekit:marketplace:index:v1")
pub const MARKETPLACE_INDEX_FACT_ID: &str =
    "18069b98f553f89911ee4e6bb224fef8c6b0f39b6d1a687da5102d277670decb";

fn trim_slash(url: &str) -> &str {
    url.trim_end_matches('/')
}

pub fn fact_json_is_app_manifest(fact: &Value) -> bool {
    if fact
        .get("content")
        .and_then(|c| c.get("Json"))
        .and_then(|j| j.get("schema"))
        .and_then(|s| s.as_str())
        == Some("spacekit:app-package:v1")
    {
        return true;
    }
    fact.get("metadata")
        .and_then(|m| m.get("tags"))
        .and_then(|t| t.as_array())
        .map(|tags| tags.iter().any(|tag| tag.as_str() == Some("app-package")))
        .unwrap_or(false)
}

pub async fn fetch_remote_fact_json(
    client: &reqwest::Client,
    storage_base_url: &str,
    fact_id_hex: &str,
) -> Result<Option<Value>, String> {
    let url = format!("{}/facts/{}", trim_slash(storage_base_url), fact_id_hex);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("GET {url}: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GET {url} failed (HTTP {status}): {body}"));
    }
    resp.json::<Value>()
        .await
        .map(Some)
        .map_err(|e| format!("Failed to parse fact JSON: {e}"))
}

pub fn app_content_ref_ids_from_manifest_fact(fact: &Value) -> Vec<String> {
    let pkg = fact
        .get("content")
        .and_then(|c| c.get("Json"))
        .and_then(|j| j.get("data"))
        .unwrap_or(fact);
    let Some(refs) = pkg.get("content_refs").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    let mut ids = Vec::new();
    for reference in refs {
        let id = reference.get("fact_id");
        if let Some(hex) = id.and_then(|v| v.as_str()) {
            ids.push(hex.to_string());
            continue;
        }
        if let Some(bytes) = id.and_then(|v| v.as_array()) {
            let mut raw = [0u8; 32];
            if bytes.len() == 32 {
                for (idx, byte) in bytes.iter().enumerate() {
                    if let Some(n) = byte.as_u64() {
                        raw[idx] = n as u8;
                    }
                }
                ids.push(hex::encode(raw));
            }
        }
    }
    ids
}

/// Delete `app_listings/{app_id}` for publisher + website catalog mirror.
pub async fn delete_app_listing_http(
    storage_base_url: &str,
    publisher_did: &str,
    app_id: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/documents/app_listings/{}",
        trim_slash(storage_base_url),
        app_id
    );
    let mut errors = Vec::new();
    for (did, label) in [
        (publisher_did, "owner"),
        (WEBSITE_CATALOG_OWNER_DID, "website-catalog"),
    ] {
        let resp = client
            .delete(&url)
            .header("Authorization", format!("DID {}", did))
            .send()
            .await
            .map_err(|e| format!("DELETE app_listings [{label}]: {e}"))?;
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            errors.push(format!("[{label}] HTTP {status}: {text}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Remove one app from the federated marketplace index fact.
pub async fn remove_app_from_marketplace_index_http(
    storage_base_url: &str,
    publisher_did: &str,
    app_id: &str,
) -> Result<bool, String> {
    let client = reqwest::Client::new();
    let index_url = format!(
        "{}/facts/{}",
        trim_slash(storage_base_url),
        MARKETPLACE_INDEX_FACT_ID
    );
    let existing = match client.get(&index_url).send().await {
        Ok(resp) if resp.status().is_success() => resp.json::<Value>().await.ok(),
        Ok(_) | Err(_) => None,
    };
    let Some(mut fact) = existing else {
        return Ok(false);
    };

    let data = fact
        .get_mut("content")
        .and_then(|c| c.get_mut("Json"))
        .and_then(|j| j.get_mut("data"))
        .ok_or_else(|| "Marketplace index fact has no JSON data".to_string())?;
    let listings = data
        .get("listings")
        .and_then(|l| l.as_array())
        .cloned()
        .unwrap_or_default();
    let app_id_lower = app_id.trim().to_lowercase();
    let filtered: Vec<Value> = listings
        .into_iter()
        .filter(|listing| {
            listing
                .get("app_id")
                .and_then(|id| id.as_str())
                .map(|id| id.to_lowercase() != app_id_lower)
                .unwrap_or(true)
        })
        .collect();
    if filtered.len()
        == data
            .get("listings")
            .and_then(|l| l.as_array())
            .map(|l| l.len())
            .unwrap_or(0)
    {
        return Ok(false);
    }
    data["listings"] = json!(filtered);
    data["updated_at"] = json!(Utc::now().to_rfc3339());

    let resp = client
        .post(format!("{}/facts", trim_slash(storage_base_url)))
        .header("Authorization", format!("DID {}", publisher_did))
        .json(&fact)
        .send()
        .await
        .map_err(|e| format!("POST marketplace index: {e}"))?;
    if resp.status().is_success() {
        Ok(true)
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!(
            "Marketplace index update failed (HTTP {status}): {body}"
        ))
    }
}

/// Remove app catalog documents and marketplace index entry.
pub async fn unpublish_app_marketplace_entries(
    storage_base_url: &str,
    publisher_did: &str,
    app_id: &str,
) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Err(e) = delete_app_listing_http(storage_base_url, publisher_did, app_id).await {
        errors.push(format!("app_listings: {e}"));
    }
    match remove_app_from_marketplace_index_http(storage_base_url, publisher_did, app_id).await {
        Ok(_) => {}
        Err(e) => errors.push(format!("marketplace index: {e}")),
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Merge a listing into the marketplace index fact (used by deploy).
pub async fn upsert_app_in_marketplace_index_http(
    storage_base_url: &str,
    publisher_did: &str,
    app_id_hex: &str,
    listing: &Value,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let index_url = format!(
        "{}/facts/{}",
        trim_slash(storage_base_url),
        MARKETPLACE_INDEX_FACT_ID
    );

    let mut listings: Vec<Value> = Vec::new();
    if let Ok(resp) = client.get(&index_url).send().await {
        if resp.status().is_success() {
            if let Ok(existing) = resp.json::<Value>().await {
                if let Some(data) = existing
                    .get("content")
                    .and_then(|c| c.get("Json"))
                    .and_then(|j| j.get("data"))
                    .and_then(|d| d.get("listings"))
                    .and_then(|l| l.as_array())
                {
                    listings = data
                        .iter()
                        .filter(|l| l.get("app_id").and_then(|a| a.as_str()) != Some(app_id_hex))
                        .cloned()
                        .collect();
                }
            }
        }
    }
    listings.push(listing.clone());

    let author_did = QuantumDID {
        did: publisher_did.to_string(),
    };
    let mut index_fact_id = [0u8; 32];
    hex::decode_to_slice(MARKETPLACE_INDEX_FACT_ID, &mut index_fact_id)
        .map_err(|e| format!("Invalid marketplace index fact id: {e}"))?;

    let index_fact = FactPackage {
        fact_id: index_fact_id,
        version: 1,
        created_at: Utc::now().timestamp() as u64,
        expires_at: None,
        content: FactContent::Json {
            data: json!({
                "type": "marketplace-index",
                "version": "v1",
                "listings": listings,
                "updated_at": Utc::now().to_rfc3339(),
            }),
            schema: Some("spacekit:marketplace-index:v1".to_string()),
        },
        metadata: FactMetadata {
            category: FactCategory::Technical,
            tags: vec!["marketplace-index".to_string()],
            domain: KnowledgeDomain::ComputerScience,
            source: DataSource::UserInput {
                application: author_did.clone(),
                user: author_did.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: spacekit_primitives::v1::fact::LicenseType::MIT,
            size_bytes: 0,
            checksum: [0u8; 32],
        },
        author: author_did,
        signature: spacekit_primitives::v1::crypto::quantum::SPHINCSSignature {
            signature_bytes: Vec::new(),
            algorithm: "sphincs-shake-256f".to_string(),
            public_key: Vec::new(),
        },
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: Vec::new(),
            verification_timestamp: Utc::now().timestamp() as u64,
            verifier: None,
        },
        dependencies: Vec::new(),
        citations: Vec::new(),
        confidence_score: 1.0,
        access_policy: spacekit_primitives::v1::fact::AccessPolicy::Public,
        encryption: None,
    };

    let resp = client
        .post(format!("{}/facts", trim_slash(storage_base_url)))
        .header("Authorization", format!("DID {}", publisher_did))
        .json(&index_fact)
        .send()
        .await
        .map_err(|e| format!("POST marketplace index: {e}"))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(format!(
            "Marketplace index update failed (HTTP {status}): {body}"
        ))
    }
}
