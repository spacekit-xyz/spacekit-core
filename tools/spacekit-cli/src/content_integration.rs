//! Content Publishing Integration Module
//!
//! Provides helper functions for:
//! - Converting files to Fact Packages
//! - Messaging Node integration
//! - Smart contract interactions

use anyhow::{anyhow, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use spacekit_compute_node::ComputeNode;
use spacekit_messaging_node::{MessagingConfig, MessagingNode};
use spacekit_primitives::v1::crypto::quantum::{
    generate_sphincs_keypair, sign_sphincs_detached, SPHINCSSignature,
};
use spacekit_primitives::v1::fact::{
    AccessCondition, AccessPolicy, CollectionMethod, ConditionType, DataSource, FactCategory,
    FactContent, FactID, FactMetadata, FactPackage, KnowledgeDomain, LicenseType, ProofType,
    VerificationLevel, VerificationProof,
};
use spacekit_primitives::v1::identity::QuantumDID;
use spacekit_storage_node::{
    access_policy::create_fact_verification_message,
    content_access::{evaluate_content_access, ContentAccessDecision},
    content_entitlement::{on_chain_content_grant, parse_entitlement_id_hex},
    content_grants::ContentGrantStore,
    content_installs::{
        self, app_slug_from_tags, build_install_record, get_install, grant_entitlement_for_content,
        is_growformer_install, register_install, resolve_installed_executable,
        should_use_embedded_growformer, storage_fact_reference, ContentInstall,
        ContentInstallRuntime,
    },
    content_payment::{
        grant_after_payment, payment_scope_channel, payment_scope_content, verify_content_payment,
        PaymentReceiptStore, PaymentVerifyError, VerifiedPayment,
    },
    licensed_feature::{
        self, default_growformer_feature, LicensedFeatureDocument, LICENSED_FEATURE_SCHEMA,
    },
    migration::load_or_create_migration_signer_keypair,
    CompressionAlgorithm, FactStorageConfig, FactStorageEngine, StorageNode, StorageTierConfig,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const SPHINCS_ALG: &str = "sphincs-128s";
const CHANNEL_SCHEMA: &str = "spacekit:channel:v1";

/// Extract pay-per-view price from conditional access policy.
pub fn conditional_price_from_policy(policy: &AccessPolicy) -> Option<f64> {
    match policy {
        AccessPolicy::Conditional(conditions) => conditions.iter().find_map(|c| {
            if c.condition_type == ConditionType::PaymentRequired {
                c.parameters.get("price")?.parse().ok()
            } else {
                None
            }
        }),
        _ => None,
    }
}

/// PPV price from access policy, with tag fallback when policy round-trip is Public (legacy store).
pub fn content_price_astra(fact: &FactPackage) -> Option<f64> {
    conditional_price_from_policy(&fact.access_policy).or_else(|| {
        let pricing = fact
            .metadata
            .tags
            .iter()
            .find_map(|t| t.strip_prefix("pricing:").map(str::to_string))?;
        if pricing != "pay_per_view" && pricing != "mixed" {
            return None;
        }
        fact.metadata
            .tags
            .iter()
            .find_map(|t| t.strip_prefix("price:").and_then(|v| v.parse().ok()))
    })
}

fn placeholder_signature() -> SPHINCSSignature {
    SPHINCSSignature::new(vec![0u8; 64], SPHINCS_ALG.to_string(), vec![0u8; 32])
}

/// Convert a file to a Fact Package for content publishing
pub async fn file_to_fact_package(
    file_path: &str,
    file_data: &[u8],
    publisher_did: &str,
    channel_did: &str,
    title: &str,
    description: Option<&str>,
    pricing: &str,
    price: Option<f64>,
    tags: Vec<String>,
) -> Result<FactPackage> {
    // Generate content ID (hash of content + publisher + timestamp)
    let mut hasher = Sha256::new();
    hasher.update(file_data);
    hasher.update(publisher_did.as_bytes());
    hasher.update(&chrono::Utc::now().timestamp().to_le_bytes());
    let fact_id: FactID = hasher.finalize().into();

    // Calculate content hash
    let content_hash: [u8; 32] = Sha256::digest(file_data).into();

    // Determine MIME type from file extension
    let mime_type = determine_mime_type(file_path);

    // Create FactContent::Binary
    let fact_content = FactContent::Binary {
        data: file_data.to_vec(),
        mime_type: mime_type.clone(),
        hash: content_hash,
    };

    // Parse publisher DID
    let publisher_quantum_did = QuantumDID::parse(publisher_did)
        .map_err(|e| anyhow!("Invalid publisher DID format: {}", e))?;

    // Parse channel DID
    let channel_quantum_did =
        QuantumDID::parse(channel_did).map_err(|e| anyhow!("Invalid channel DID format: {}", e))?;

    // Create FactMetadata
    let fact_metadata = FactMetadata {
        category: FactCategory::UserGenerated,
        tags: {
            let mut all_tags = tags;
            all_tags.push("content".to_string());
            all_tags.push("published".to_string());
            all_tags.push(format!("title:{}", title));
            if let Some(desc) = description.filter(|s| !s.is_empty()) {
                all_tags.push(format!("description:{}", desc));
            }
            if let Some(name) = Path::new(file_path)
                .file_name()
                .and_then(|s| s.to_str())
                .filter(|s| !s.is_empty())
            {
                all_tags.push(format!("filename:{}", name));
                if name.eq_ignore_ascii_case("growformer") {
                    all_tags.push("app:growformer".to_string());
                }
            }
            all_tags.push(format!("channel:{}", channel_did));
            all_tags.push(format!("pricing:{}", pricing));
            if let Some(p) = price {
                all_tags.push(format!("price:{}", p));
            }
            if mime_type.starts_with("video/") {
                all_tags.push("video".to_string());
            } else if mime_type.starts_with("image/") {
                all_tags.push("image".to_string());
            } else if mime_type.starts_with("audio/") {
                all_tags.push("audio".to_string());
            }
            all_tags
        },
        domain: KnowledgeDomain::Custom("Content Publishing".to_string()),
        source: DataSource::UserInput {
            application: channel_quantum_did.clone(),
            user: publisher_quantum_did.clone(),
        },
        collection_method: CollectionMethod::Manual,
        verification_level: VerificationLevel::SelfClaimed,
        license: LicenseType::Proprietary,
        size_bytes: file_data.len() as u64,
        checksum: content_hash,
    };

    // Create access policy based on pricing
    let access_policy = match pricing {
        "free" => AccessPolicy::Public,
        "pay_per_view" => {
            if let Some(p) = price {
                AccessPolicy::Conditional(vec![AccessCondition {
                    condition_type: ConditionType::PaymentRequired,
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("price".to_string(), p.to_string());
                        params.insert("currency".to_string(), "ASTRA".to_string());
                        params.insert("content_id".to_string(), hex::encode(fact_id));
                        params
                    },
                }])
            } else {
                AccessPolicy::Public
            }
        }
        "subscription" => AccessPolicy::Conditional(vec![AccessCondition {
            condition_type: ConditionType::TrustLevel,
            parameters: {
                let mut params = HashMap::new();
                params.insert("subscription_required".to_string(), "true".to_string());
                params.insert("channel_id".to_string(), channel_did.to_string());
                params
            },
        }]),
        "mixed" => {
            let mut conditions = vec![AccessCondition {
                condition_type: ConditionType::TrustLevel,
                parameters: {
                    let mut params = HashMap::new();
                    params.insert("subscription_required".to_string(), "true".to_string());
                    params.insert("channel_id".to_string(), channel_did.to_string());
                    params
                },
            }];
            if let Some(p) = price {
                conditions.push(AccessCondition {
                    condition_type: ConditionType::PaymentRequired,
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("price".to_string(), p.to_string());
                        params.insert("currency".to_string(), "ASTRA".to_string());
                        params.insert("content_id".to_string(), hex::encode(fact_id));
                        params
                    },
                });
            }
            AccessPolicy::Conditional(conditions)
        }
        _ => AccessPolicy::Public,
    };

    let verification_proof = VerificationProof {
        proof_type: ProofType::QuantumSignature,
        proof_data: vec![],
        verification_timestamp: chrono::Utc::now().timestamp() as u64,
        verifier: Some(publisher_quantum_did.clone()),
    };

    let mut fact_package = FactPackage {
        fact_id,
        version: 1,
        created_at: chrono::Utc::now().timestamp() as u64,
        expires_at: None,
        content: fact_content,
        metadata: fact_metadata,
        author: publisher_quantum_did,
        signature: placeholder_signature(),
        verification_proof,
        dependencies: Vec::new(),
        citations: Vec::new(),
        confidence_score: 0.8,
        access_policy,
        encryption: None,
    };
    sign_content_fact(&mut fact_package, publisher_did, None)?;
    Ok(fact_package)
}

/// Persist a channel as a FactPackage (`spacekit:channel:v1`).
pub async fn channel_to_fact_package(
    owner_did: &str,
    channel_did: &str,
    name: &str,
    description: Option<&str>,
    pricing: &str,
    price: Option<f64>,
) -> Result<FactPackage> {
    let owner = QuantumDID::parse(owner_did).map_err(|e| anyhow!("Invalid owner DID: {}", e))?;
    let channel =
        QuantumDID::parse(channel_did).map_err(|e| anyhow!("Invalid channel DID: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(b"spacekit-channel-v1\0");
    hasher.update(channel_did.as_bytes());
    hasher.update(owner_did.as_bytes());
    let fact_id: FactID = hasher.finalize().into();
    let body = json!({
        "schema": CHANNEL_SCHEMA,
        "channel_did": channel_did,
        "name": name,
        "description": description,
        "pricing": pricing,
        "price": price,
    });
    let content_hash: [u8; 32] = Sha256::digest(body.to_string().as_bytes()).into();
    let mut tags = vec![
        "channel".to_string(),
        format!("channel:{}", channel_did),
        name.to_string(),
    ];
    if let Some(d) = description {
        tags.push(d.to_string());
    }
    let access_policy = if pricing == "free" {
        AccessPolicy::Public
    } else {
        AccessPolicy::Conditional(vec![AccessCondition {
            condition_type: ConditionType::TrustLevel,
            parameters: {
                let mut p = HashMap::new();
                p.insert("subscription_required".to_string(), "true".into());
                p.insert("channel_id".to_string(), channel_did.to_string());
                p
            },
        }])
    };
    let mut fact = FactPackage {
        fact_id,
        version: 1,
        created_at: chrono::Utc::now().timestamp() as u64,
        expires_at: None,
        content: FactContent::Json {
            data: body,
            schema: Some(CHANNEL_SCHEMA.to_string()),
        },
        metadata: FactMetadata {
            category: FactCategory::UserGenerated,
            tags,
            domain: KnowledgeDomain::Custom("Content Channel".to_string()),
            source: DataSource::UserInput {
                application: channel.clone(),
                user: owner.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: LicenseType::Proprietary,
            size_bytes: 0,
            checksum: content_hash,
        },
        author: owner,
        signature: placeholder_signature(),
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: vec![],
            verification_timestamp: chrono::Utc::now().timestamp() as u64,
            verifier: None,
        },
        dependencies: vec![],
        citations: vec![],
        confidence_score: 1.0,
        access_policy,
        encryption: None,
    };
    sign_content_fact(&mut fact, owner_did, None)?;
    Ok(fact)
}

/// Publish a library-embedded licensed feature (`spacekit:licensed_feature:v1`).
pub async fn licensed_feature_to_fact_package(
    publisher_did: &str,
    channel_did: &str,
    document: LicensedFeatureDocument,
) -> Result<FactPackage> {
    document.validate()?;
    let publisher =
        QuantumDID::parse(publisher_did).map_err(|e| anyhow!("Invalid publisher DID: {}", e))?;
    let channel =
        QuantumDID::parse(channel_did).map_err(|e| anyhow!("Invalid channel DID: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(b"spacekit-licensed-feature-v1\0");
    hasher.update(document.feature_name.as_bytes());
    hasher.update(channel_did.as_bytes());
    hasher.update(publisher_did.as_bytes());
    hasher.update(document.feature_version.as_bytes());
    let fact_id: FactID = hasher.finalize().into();
    let mut body = serde_json::to_value(&document)?;
    if let Some(obj) = body.as_object_mut() {
        obj.insert("schema".to_string(), json!(LICENSED_FEATURE_SCHEMA));
        if !obj.contains_key("published_at") {
            obj.insert(
                "published_at".to_string(),
                json!(chrono::Utc::now().timestamp()),
            );
        }
    }
    let content_hash: [u8; 32] = Sha256::digest(body.to_string().as_bytes()).into();
    let feature_tag = document.feature_tag();
    let tags = vec![
        "licensed_feature".to_string(),
        feature_tag.clone(),
        document.feature_name.clone(),
        document.title.clone(),
        format!("channel:{}", channel_did),
    ];
    let access_policy = AccessPolicy::Public;
    let mut fact = FactPackage {
        fact_id,
        version: 1,
        created_at: chrono::Utc::now().timestamp() as u64,
        expires_at: None,
        content: FactContent::Json {
            data: body,
            schema: Some(LICENSED_FEATURE_SCHEMA.to_string()),
        },
        metadata: FactMetadata {
            category: FactCategory::UserGenerated,
            tags,
            domain: KnowledgeDomain::Custom("Licensed Feature".to_string()),
            source: DataSource::UserInput {
                application: channel.clone(),
                user: publisher.clone(),
            },
            collection_method: CollectionMethod::Manual,
            verification_level: VerificationLevel::SelfClaimed,
            license: LicenseType::Proprietary,
            size_bytes: 0,
            checksum: content_hash,
        },
        author: publisher,
        signature: placeholder_signature(),
        verification_proof: VerificationProof {
            proof_type: ProofType::QuantumSignature,
            proof_data: vec![],
            verification_timestamp: chrono::Utc::now().timestamp() as u64,
            verifier: None,
        },
        dependencies: vec![],
        citations: vec![],
        confidence_score: 1.0,
        access_policy,
        encryption: None,
    };
    sign_content_fact(&mut fact, publisher_did, None)?;
    Ok(fact)
}

/// Build the default growformer licensed-feature fact for publishing.
pub fn growformer_feature_document(
    publisher_did: &str,
    title: &str,
    description: &str,
) -> LicensedFeatureDocument {
    default_growformer_feature(publisher_did, title, description)
}

/// Resolve a licensed feature's content id (64-hex fact id) by feature name.
pub async fn find_licensed_feature_content_id(
    storage_node: &Arc<StorageNode>,
    requester_did: &str,
    feature_name: &str,
) -> Result<Option<String>> {
    use spacekit_primitives::v1::fact::types::{
        FactQuery, PaginationParams, SortCriteria, SortOrder,
    };
    let fact_storage = get_fact_storage_engine(storage_node).await?;
    let query = FactQuery {
        requester: QuantumDID::parse(requester_did).map_err(|e| anyhow!("Invalid DID: {}", e))?,
        author: None,
        category: None,
        tags: vec![
            "licensed_feature".to_string(),
            format!("feature:{}", feature_name),
        ],
        domain: None,
        content_type: None,
        text_search: None,
        verification_level: None,
        min_confidence: None,
        created_after: None,
        created_before: None,
        depends_on: None,
        referenced_by: None,
        sort_by: SortCriteria::CreatedAt(SortOrder::Descending),
        pagination: PaginationParams {
            offset: 0,
            limit: 8,
        },
        start_time: chrono::Utc::now().timestamp() as u64,
    };
    let result = fact_storage.query_facts(query).await?;
    Ok(result.facts.first().map(|f| hex::encode(f.fact_id)))
}

/// Load licensed feature document from storage by content id.
pub async fn load_licensed_feature_document(
    storage_node: &Arc<StorageNode>,
    content_id_hex: &str,
) -> Result<Option<LicensedFeatureDocument>> {
    let fact_id = parse_content_id_hex(content_id_hex)?;
    let fact_storage = get_fact_storage_engine(storage_node).await?;
    let fact = fact_storage.retrieve_fact(fact_id).await?;
    Ok(fact.and_then(|f| licensed_feature::parse_licensed_feature_fact(&f)))
}

/// Grant access to a licensed feature tier and register embedded install metadata.
pub async fn grant_licensed_feature_tier(
    storage_node: Arc<StorageNode>,
    requester_did: &str,
    content_id_hex: &str,
    tier_name: &str,
    payment_reference: Option<String>,
    entitlement_id_hex: Option<String>,
) -> Result<(String, String)> {
    let doc = load_licensed_feature_document(&storage_node, content_id_hex)
        .await?
        .ok_or_else(|| anyhow!("content {} is not a licensed_feature fact", content_id_hex))?;
    let tier = doc.tier(tier_name).ok_or_else(|| {
        anyhow!(
            "tier '{}' not found for feature '{}'",
            tier_name,
            doc.feature_name
        )
    })?;
    if licensed_feature::tier_requires_payment(tier)
        && payment_reference.is_none()
        && entitlement_id_hex.is_none()
    {
        let price = licensed_feature::tier_price_astra(tier).unwrap_or(0.0);
        return Err(anyhow!(
            "tier '{}' requires payment ({} ASTRA) — use `spacekit content pay --content-id {} --tier {}`",
            tier.name,
            price,
            content_id_hex,
            tier.name
        ));
    }
    let feature_name = doc.feature_name.clone();
    let tier_name = tier.name.clone();
    let now = chrono::Utc::now().timestamp() as u64;
    let expires = tier
        .entitlement_duration_seconds
        .map(|d| now.saturating_add(d));
    let quota_remaining = doc.quota_for_tier(&tier_name);
    content_grants_store(&storage_node).grant_content_ppv_full(
        requester_did,
        content_id_hex,
        payment_reference,
        expires,
        entitlement_id_hex,
        Some(tier_name.clone()),
        None,
        quota_remaining,
    )?;
    finalize_licensed_feature_install(&storage_node, requester_did, content_id_hex).await?;
    Ok((feature_name, tier_name))
}

/// Register growformer install record after grant (idempotent if already installed).
pub async fn finalize_licensed_feature_install(
    storage_node: &Arc<StorageNode>,
    requester_did: &str,
    content_id_hex: &str,
) -> Result<()> {
    if get_content_install(storage_node, requester_did, content_id_hex)?.is_some() {
        return Ok(());
    }
    match view_content_fact(storage_node, content_id_hex, requester_did).await? {
        ViewContentOutcome::Bytes {
            data,
            filename,
            app_slug,
        } => {
            register_content_install_after_view(
                storage_node,
                requester_did,
                content_id_hex,
                None,
                &filename,
                data.len() as u64,
                app_slug,
                Some(data.as_slice()),
            )?;
            Ok(())
        }
        ViewContentOutcome::PaymentRequired { .. } => {
            return Err(anyhow!(
                "grant recorded but feature fact still requires payment"
            ));
        }
        ViewContentOutcome::SubscriptionRequired { channel_did } => {
            return Err(anyhow!("subscription required for channel {}", channel_did));
        }
        ViewContentOutcome::Denied { reason } => Err(anyhow!("access denied: {}", reason)),
    }
}

/// Build pending-grant metadata for a licensed-feature tier purchase.
pub fn licensed_feature_pending_grant(
    doc: &LicensedFeatureDocument,
    tier_name: &str,
) -> Result<(
    f64,
    spacekit_storage_node::content_settlement::PendingGrantOptions,
)> {
    use spacekit_storage_node::content_settlement::PendingGrantOptions;
    use spacekit_storage_node::licensed_feature::{tier_price_astra, tier_requires_payment};
    let tier = doc
        .tier(tier_name)
        .ok_or_else(|| anyhow!("tier '{}' not found", tier_name))?;
    let price = tier_price_astra(tier).unwrap_or(0.0);
    if tier_requires_payment(tier) && price <= 0.0 {
        return Err(anyhow!(
            "tier '{}' is marked paid but has no price",
            tier_name
        ));
    }
    if !tier_requires_payment(tier) {
        return Err(anyhow!(
            "tier '{}' is free — use `spacekit content access --feature {}`",
            tier_name,
            doc.feature_name
        ));
    }
    let now = chrono::Utc::now().timestamp() as u64;
    let expires = tier
        .entitlement_duration_seconds
        .map(|d| now.saturating_add(d));
    Ok((
        price,
        PendingGrantOptions {
            tier: Some(tier.name.clone()),
            grant_expires_at: expires,
            quota_remaining: doc.quota_for_tier(tier_name),
        },
    ))
}

/// Grant free/open tier access to a licensed feature and register embedded install.
pub async fn access_licensed_feature(
    storage_node: Arc<StorageNode>,
    requester_did: &str,
    content_id_hex: &str,
    tier_name: Option<&str>,
) -> Result<(String, String)> {
    let doc = load_licensed_feature_document(&storage_node, content_id_hex)
        .await?
        .ok_or_else(|| anyhow!("content {} is not a licensed_feature fact", content_id_hex))?;
    let feature_name = doc.feature_name.clone();

    if tier_name.is_none() {
        let now = chrono::Utc::now().timestamp() as u64;
        if let Some(existing) = content_grants_store(&storage_node)
            .list_for_requester(requester_did)
            .ok()
            .and_then(|list| {
                list.into_iter().find(|g| {
                    g.content_id_hex.as_deref() == Some(content_id_hex)
                        && g.expires_at.map(|e| e > now).unwrap_or(true)
                })
            })
        {
            if existing
                .tier
                .as_deref()
                .is_some_and(|t| !t.eq_ignore_ascii_case("free"))
            {
                let tier = existing.tier.clone().unwrap_or_else(|| "free".to_string());
                finalize_licensed_feature_install(&storage_node, requester_did, content_id_hex)
                    .await?;
                return Ok((feature_name, tier));
            }
        }
    }

    let tier = if let Some(name) = tier_name {
        doc.tier(name).ok_or_else(|| {
            anyhow!(
                "tier '{}' not found for feature '{}'",
                name,
                doc.feature_name
            )
        })?
    } else {
        doc.default_tier()
            .ok_or_else(|| anyhow!("feature '{}' has no tiers", doc.feature_name))?
    };
    if licensed_feature::tier_requires_payment(tier) {
        let price = licensed_feature::tier_price_astra(tier).unwrap_or(0.0);
        return Err(anyhow!(
            "tier '{}' requires payment ({} ASTRA) — use `spacekit content pay --content-id {} --tier {}`",
            tier.name,
            price,
            content_id_hex,
            tier.name
        ));
    }
    grant_licensed_feature_tier(
        storage_node,
        requester_did,
        content_id_hex,
        &tier.name,
        None,
        None,
    )
    .await
}

/// Sign a fact package with the publisher's migration signer key (or ephemeral key).
pub fn sign_content_fact(
    fact: &mut FactPackage,
    publisher_did: &str,
    data_dir: Option<&Path>,
) -> Result<()> {
    fact.signature.algorithm = SPHINCS_ALG.to_string();
    let msg = create_fact_verification_message(fact)?;
    let (pk, sk) = if let Some(dir) = data_dir {
        match load_or_create_migration_signer_keypair(dir, publisher_did) {
            Ok(kp) => (kp.public_key, kp.secret_key),
            Err(_) => generate_sphincs_keypair(SPHINCS_ALG)?,
        }
    } else {
        generate_sphincs_keypair(SPHINCS_ALG)?
    };
    fact.signature = sign_sphincs_detached(&msg, SPHINCS_ALG, &pk, &sk)?;
    Ok(())
}

pub fn parse_content_id_hex(content_id: &str) -> Result<FactID> {
    let hex_str = content_id.trim_start_matches("0x");
    let bytes = hex::decode(hex_str).map_err(|e| anyhow!("Invalid content id hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(anyhow!("Content id must be 32 bytes (64 hex chars)"));
    }
    let mut id = [0u8; 32];
    id.copy_from_slice(&bytes);
    Ok(id)
}

pub fn content_grants_store(storage_node: &StorageNode) -> ContentGrantStore {
    ContentGrantStore::from_env_or_data_dir(storage_node.config().data_dir.as_path())
}

#[derive(Debug)]
pub enum ViewContentOutcome {
    Bytes {
        data: Vec<u8>,
        /// Basename from publish (`filename:` tag) or `{content_id}.bin`.
        filename: String,
        /// From publish `app:` tag (e.g. `growformer`).
        app_slug: Option<String>,
    },
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

/// Basename recorded at publish time (`filename:growformer` tag).
pub fn filename_from_publish_tags(tags: &[String]) -> Option<String> {
    tags.iter()
        .find_map(|t| t.strip_prefix("filename:"))
        .map(|s| sanitize_content_filename(s))
}

fn sanitize_content_filename(name: &str) -> String {
    Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name)
        .chars()
        .filter(|c| *c != '/' && *c != '\\' && *c != '\0')
        .collect()
}

/// Materialized download path under the storage-node data directory.
pub fn content_materialized_path(data_dir: &Path, content_id_hex: &str, filename: &str) -> PathBuf {
    let id = content_id_hex.trim().to_lowercase();
    let prefix: String = id.chars().take(2).collect();
    data_dir
        .join("content")
        .join("materialized")
        .join(prefix)
        .join(&id)
        .join(sanitize_content_filename(filename))
}

/// Resolve output path: explicit `--output` or storage-node materialized path.
pub fn resolve_content_view_output(
    data_dir: &Path,
    content_id_hex: &str,
    explicit_output: Option<&str>,
    filename: &str,
) -> PathBuf {
    match explicit_output {
        Some(path) => PathBuf::from(path),
        None => content_materialized_path(data_dir, content_id_hex, filename),
    }
}

/// Write viewed bytes, creating parent directories (storage-node materialized tree).
/// Open a materialized path with the OS default handler (QuickTime, Preview, etc.).
pub fn open_materialized_path(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).status();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).status();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .status();
    }
}

pub fn write_content_view_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.extension().is_none() || path.extension().is_some_and(|e| e == "bin") {
            let mut perms = std::fs::metadata(path)?.permissions();
            perms.set_mode(perms.mode() | 0o111);
            std::fs::set_permissions(path, perms)?;
        }
    }
    Ok(())
}

/// Drop entitlement flags mistakenly captured in `exec` trailing args.
pub fn strip_entitlement_flags_from_exec_args(
    args: &[String],
) -> (Option<String>, Option<String>, Vec<String>) {
    let mut content_id = None;
    let mut app = None;
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--content-id" | "-content-id" => {
                if i + 1 < args.len() {
                    content_id = Some(args[i + 1].clone());
                    i += 2;
                    continue;
                }
            }
            s if s.starts_with("--content-id=") => {
                content_id = Some(
                    s.split_once('=')
                        .map(|(_, v)| v.to_string())
                        .unwrap_or_default(),
                );
                i += 1;
                continue;
            }
            "--app" => {
                if i + 1 < args.len() {
                    app = Some(args[i + 1].clone());
                    i += 2;
                    continue;
                }
            }
            s if s.starts_with("--app=") => {
                app = Some(
                    s.split_once('=')
                        .map(|(_, v)| v.to_string())
                        .unwrap_or_default(),
                );
                i += 1;
                continue;
            }
            _ => {}
        }
        out.push(args[i].clone());
        i += 1;
    }
    (content_id, app, out)
}

/// Persist install metadata in the storage-node DB after a successful view (links grant/entitlement).
pub fn register_content_install_after_view(
    storage_node: &StorageNode,
    requester_did: &str,
    content_id_hex: &str,
    materialized_path: Option<&Path>,
    filename: &str,
    size_bytes: u64,
    app_slug: Option<String>,
    payload: Option<&[u8]>,
) -> Result<ContentInstall> {
    let grants = content_grants_store(storage_node);
    let (entitlement_id_hex, tier) =
        grant_entitlement_for_content(&grants, requester_did, content_id_hex)
            .unwrap_or((None, None));
    let app_slug = app_slug
        .or_else(|| {
            filename
                .strip_suffix(".exe")
                .or_else(|| filename.strip_suffix(".bin"))
                .and_then(|b| {
                    b.eq_ignore_ascii_case("growformer")
                        .then_some("growformer".to_string())
                })
        })
        .or_else(|| {
            payload
                .filter(|d| {
                    spacekit_storage_node::licensed_feature::is_growformer_feature_json_bytes(d)
                })
                .map(|_| "growformer".to_string())
        });
    let use_embedded =
        should_use_embedded_growformer(content_id_hex, app_slug.as_deref(), None, payload);
    let (path_ref, runtime) = if use_embedded {
        (
            storage_fact_reference(content_id_hex),
            ContentInstallRuntime::EmbeddedGrowformer,
        )
    } else {
        let path = materialized_path
            .ok_or_else(|| anyhow!("materialized path required for non-growformer content"))?;
        (
            path.display().to_string(),
            ContentInstallRuntime::MaterializedFile,
        )
    };
    let install = build_install_record(
        content_id_hex,
        &path_ref,
        filename,
        size_bytes,
        app_slug.clone(),
        entitlement_id_hex,
        tier,
        runtime,
    );
    register_install(storage_node.database().as_ref(), requester_did, &install)?;
    Ok(install)
}

pub fn get_content_install(
    storage_node: &StorageNode,
    requester_did: &str,
    content_id_hex: &str,
) -> Result<Option<ContentInstall>> {
    get_install(
        storage_node.database().as_ref(),
        requester_did,
        content_id_hex,
    )
}

pub fn entitled_app_uses_embedded_growformer(
    content_id_hex: &str,
    app_flag: Option<&str>,
    install: Option<&ContentInstall>,
) -> bool {
    should_use_embedded_growformer(content_id_hex, app_flag, install, None)
}

/// Verify entitlement/access without requiring a materialized install (embedded agent path).
pub async fn ensure_content_entitlement_for_agent(
    storage_node: &Arc<StorageNode>,
    content_id_hex: &str,
    requester_did: &str,
) -> Result<()> {
    let grants = content_grants_store(storage_node);
    match view_content_fact(storage_node, content_id_hex, requester_did).await? {
        ViewContentOutcome::Bytes { .. } => Ok(()),
        ViewContentOutcome::PaymentRequired {
            price, currency, ..
        } => {
            if grants.has_content_grant(requester_did, content_id_hex) {
                Ok(())
            } else {
                Err(anyhow!(
                    "payment required ({} {}) — pay or `content access` first",
                    price,
                    currency
                ))
            }
        }
        ViewContentOutcome::SubscriptionRequired { channel_did } => {
            if grants.has_channel_subscription(requester_did, &channel_did) {
                Ok(())
            } else {
                Err(anyhow!("subscription required for channel {}", channel_did))
            }
        }
        ViewContentOutcome::Denied { reason } => Err(anyhow!("access denied: {}", reason)),
    }
}

/// Verify access (same rules as view) and return the DB-backed materialized executable path.
pub async fn resolve_entitled_executable_for_agent(
    storage_node: &Arc<StorageNode>,
    content_id_hex: &str,
    requester_did: &str,
) -> Result<PathBuf> {
    let grants = content_grants_store(storage_node);
    match view_content_fact(storage_node, content_id_hex, requester_did).await? {
        ViewContentOutcome::Bytes { .. } => {}
        ViewContentOutcome::PaymentRequired {
            price, currency, ..
        } => {
            if !grants.has_content_grant(requester_did, content_id_hex) {
                return Err(anyhow!(
                    "payment required ({} {}) — pay or `content access` before running the app",
                    price,
                    currency
                ));
            }
        }
        ViewContentOutcome::SubscriptionRequired { channel_did } => {
            if !grants.has_channel_subscription(requester_did, &channel_did) {
                return Err(anyhow!("subscription required for channel {}", channel_did));
            }
        }
        ViewContentOutcome::Denied { reason } => {
            return Err(anyhow!("access denied: {}", reason));
        }
    }
    resolve_installed_executable(
        storage_node.database().as_ref(),
        requester_did,
        content_id_hex,
    )
}

/// Run a materialized content binary (non-growformer) with entitlement + install checks.
pub async fn run_entitled_content_binary(
    storage_node: &Arc<StorageNode>,
    content_id_hex: &str,
    requester_did: &str,
    args: &[&str],
) -> Result<()> {
    let install = get_content_install(storage_node, requester_did, content_id_hex)?;
    if install.as_ref().is_some_and(is_growformer_install) {
        return Err(anyhow!(
            "growformer runs via embedded runtime in spacekit — use `spacekit agent --app growformer exec …`"
        ));
    }
    let exe =
        resolve_entitled_executable_for_agent(storage_node, content_id_hex, requester_did).await?;
    ensure_executable(&exe)?;
    let status = std::process::Command::new(&exe)
        .args(args)
        .status()
        .map_err(|e| anyhow!("failed to run {}: {}", exe.display(), e))?;
    if !status.success() {
        return Err(anyhow!(
            "entitled app exited with status {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    if perms.mode() & 0o111 == 0 {
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_executable(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn list_content_installs(
    storage_node: &StorageNode,
    requester_did: &str,
) -> Result<Vec<ContentInstall>> {
    content_installs::list_installs(storage_node.database().as_ref(), requester_did)
}

/// Resolve content id from `--content-id`, `--app`, or `GROWFORMER_CONTENT_ID`.
pub fn resolve_agent_content_id(
    content_id: Option<&str>,
    app_slug: Option<&str>,
    storage_node: &StorageNode,
    requester_did: &str,
) -> Result<String> {
    let content_id = content_id.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    let app_slug = app_slug.and_then(|s| {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    });
    if let Some(id) = content_id {
        return Ok(id.to_string());
    }
    if let Some(slug) = app_slug {
        let install = content_installs::find_install_by_app_slug(
            storage_node.database().as_ref(),
            requester_did,
            slug,
        )?
        .ok_or_else(|| {
            anyhow!(
                "no install for app '{}' — publish, view, then retry (or pass --content-id)",
                slug
            )
        })?;
        return Ok(install.content_id_hex);
    }
    if let Ok(id) = std::env::var("GROWFORMER_CONTENT_ID") {
        if !id.trim().is_empty() {
            return Ok(id.trim().to_string());
        }
    }
    Err(anyhow!(
        "pass --content-id <64-hex> or --app growformer (after content view)"
    ))
}

pub async fn view_content_fact(
    storage_node: &Arc<StorageNode>,
    content_id: &str,
    requester_did: &str,
) -> Result<ViewContentOutcome> {
    let fact_id = parse_content_id_hex(content_id)?;
    let fact_storage = get_fact_storage_engine(storage_node).await?;
    let fact = fact_storage
        .retrieve_fact(fact_id)
        .await?
        .ok_or_else(|| anyhow!("Content not found: {}", content_id))?;
    let grants = content_grants_store(storage_node);
    let decision = evaluate_content_access(&fact, requester_did, &grants)?;
    let decision = if decision == ContentAccessDecision::Allowed {
        decision
    } else if spacekit_storage_node::content_license::on_chain_has_content_license(
        requester_did,
        content_id,
    )
    .await
    {
        ContentAccessDecision::Allowed
    } else if let Some(grant) = grants
        .list_for_requester(requester_did)
        .ok()
        .and_then(|list| {
            list.into_iter().find(|g| {
                g.content_id_hex.as_deref() == Some(content_id) && g.entitlement_id_hex.is_some()
            })
        })
    {
        if let Some(ref ent) = grant.entitlement_id_hex {
            if on_chain_content_grant(requester_did, content_id, Some(ent.as_str()))
                .await
                .is_some()
            {
                ContentAccessDecision::Allowed
            } else {
                decision
            }
        } else {
            decision
        }
    } else {
        decision
    };
    match decision {
        ContentAccessDecision::Allowed => {
            let bytes = match &fact.content {
                FactContent::Binary { data, .. } => data.clone(),
                FactContent::Json { data, .. } => serde_json::to_vec(data)?,
                _ => return Err(anyhow!("Unsupported content type")),
            };
            let filename = filename_from_publish_tags(&fact.metadata.tags)
                .unwrap_or_else(|| format!("{}.bin", content_id));
            let app_slug = app_slug_from_tags(&fact.metadata.tags);
            Ok(ViewContentOutcome::Bytes {
                data: bytes,
                filename,
                app_slug,
            })
        }
        ContentAccessDecision::PaymentRequired {
            price,
            currency,
            content_id_hex,
        } => Ok(ViewContentOutcome::PaymentRequired {
            price,
            currency,
            content_id_hex,
        }),
        ContentAccessDecision::SubscriptionRequired { channel_did } => {
            Ok(ViewContentOutcome::SubscriptionRequired { channel_did })
        }
        ContentAccessDecision::Denied { reason } => Ok(ViewContentOutcome::Denied { reason }),
    }
}

pub fn grant_content_ppv(
    storage_node: &StorageNode,
    requester_did: &str,
    content_id_hex: &str,
    payment_reference: Option<String>,
) -> Result<()> {
    content_grants_store(storage_node).grant_content_ppv(
        requester_did,
        content_id_hex,
        payment_reference,
        None,
    )
}

fn map_payment_error(e: PaymentVerifyError) -> anyhow::Error {
    anyhow!("{}", e)
}

/// Pay-per-view access with payment verification (receipt file or 64-char entitlement id).
pub async fn access_content_with_payment(
    storage_node: &StorageNode,
    requester_did: &str,
    publisher_did: &str,
    content_id_hex: &str,
    price_astra: f64,
    payment_ref: Option<&str>,
    skip_payment: bool,
) -> Result<()> {
    let data_dir = storage_data_dir(storage_node);
    let scope = payment_scope_content(content_id_hex);
    let ent_id = payment_ref.and_then(|r| parse_entitlement_id_hex(r).ok().map(|_| r.to_string()));

    if !skip_payment && price_astra > 0.0 {
        let pref = payment_ref.ok_or_else(|| {
            anyhow!("payment required: pass --payment-ref (tx id or 64-char entitlement id)")
        })?;
        verify_content_payment(
            &data_dir,
            pref,
            requester_did,
            publisher_did,
            &scope,
            Some(content_id_hex),
            price_astra,
        )
        .await
        .map_err(map_payment_error)?;
        grant_after_payment(&data_dir, pref, || {
            content_grants_store(storage_node).grant_content_ppv_full(
                requester_did,
                content_id_hex,
                Some(pref.to_string()),
                None,
                ent_id.clone(),
                None,
                None,
                None,
            )
        })
        .await?;
    } else {
        content_grants_store(storage_node).grant_content_ppv_full(
            requester_did,
            content_id_hex,
            payment_ref.map(String::from),
            None,
            ent_id,
            None,
            None,
            None,
        )?;
    }
    Ok(())
}

/// Channel subscription with optional payment verification.
pub async fn subscribe_channel_with_payment(
    storage_node: &StorageNode,
    requester_did: &str,
    publisher_did: &str,
    channel_did: &str,
    price_astra: f64,
    period_secs: u64,
    payment_ref: Option<&str>,
    skip_payment: bool,
) -> Result<()> {
    let data_dir = storage_data_dir(storage_node);
    let scope = payment_scope_channel(channel_did);
    let expires = chrono::Utc::now().timestamp() as u64 + period_secs.max(3600);

    if !skip_payment && price_astra > 0.0 {
        let pref = payment_ref.ok_or_else(|| anyhow!("payment required for paid channel"))?;
        verify_content_payment(
            &data_dir,
            pref,
            requester_did,
            publisher_did,
            &scope,
            None,
            price_astra,
        )
        .await
        .map_err(map_payment_error)?;
        grant_after_payment(&data_dir, pref, || {
            content_grants_store(storage_node).grant_channel_subscription(
                requester_did,
                channel_did,
                Some(expires),
                Some(pref.to_string()),
            )
        })
        .await?;
    } else {
        content_grants_store(storage_node).grant_channel_subscription(
            requester_did,
            channel_did,
            Some(expires),
            payment_ref.map(String::from),
        )?;
    }
    Ok(())
}

/// Renew PPV or channel access (extends active grant or creates new after expiration).
pub async fn renew_content_access(
    storage_node: &StorageNode,
    requester_did: &str,
    publisher_did: &str,
    content_id_hex: Option<&str>,
    channel_did: Option<&str>,
    extend_secs: u64,
    tier: Option<&str>,
    price_astra: f64,
    payment_ref: Option<&str>,
) -> Result<()> {
    let data_dir = storage_data_dir(storage_node);
    if let Some(cid) = content_id_hex {
        let scope = payment_scope_content(cid);
        if price_astra > 0.0 {
            let pref = payment_ref.ok_or_else(|| anyhow!("payment required for renewal"))?;
            verify_content_payment(
                &data_dir,
                pref,
                requester_did,
                publisher_did,
                &scope,
                Some(cid),
                price_astra,
            )
            .await
            .map_err(map_payment_error)?;
            grant_after_payment(&data_dir, pref, || {
                content_grants_store(storage_node)
                    .renew_content_ppv(
                        requester_did,
                        cid,
                        extend_secs,
                        tier.map(String::from),
                        Some(pref.to_string()),
                    )
                    .map(|_| ())
            })
            .await?;
        } else {
            content_grants_store(storage_node).renew_content_ppv(
                requester_did,
                cid,
                extend_secs,
                tier.map(String::from),
                payment_ref.map(String::from),
            )?;
        }
    } else if let Some(ch) = channel_did {
        let scope = payment_scope_channel(ch);
        if price_astra > 0.0 {
            let pref = payment_ref.ok_or_else(|| anyhow!("payment required for renewal"))?;
            verify_content_payment(
                &data_dir,
                pref,
                requester_did,
                publisher_did,
                &scope,
                None,
                price_astra,
            )
            .await
            .map_err(map_payment_error)?;
            grant_after_payment(&data_dir, pref, || {
                content_grants_store(storage_node).renew_channel_subscription(
                    requester_did,
                    ch,
                    extend_secs,
                    tier.map(String::from),
                    Some(pref.to_string()),
                )
            })
            .await?;
        } else {
            content_grants_store(storage_node).renew_channel_subscription(
                requester_did,
                ch,
                extend_secs,
                tier.map(String::from),
                payment_ref.map(String::from),
            )?;
        }
    } else {
        return Err(anyhow!("specify --content-id or --channel for renew"));
    }
    Ok(())
}

/// Dev/test: record a verified payment receipt (simulates SpaceKit Pay settlement).
pub fn record_test_payment(
    storage_node: &StorageNode,
    reference: &str,
    payer_did: &str,
    recipient_did: &str,
    scope: &str,
    amount_astra: f64,
) -> Result<()> {
    let data_dir = storage_data_dir(storage_node);
    PaymentReceiptStore::from_env_or_data_dir(data_dir.as_path()).record_payment(
        VerifiedPayment {
            reference: reference.to_string(),
            payer_did: payer_did.to_string(),
            recipient_did: recipient_did.to_string(),
            amount_astra,
            scope: scope.to_string(),
            consumed: false,
            recorded_at: chrono::Utc::now().timestamp() as u64,
        },
    )?;
    // Dev parity: push settlement inbox so `content pay --await-settlement` can auto-complete.
    use spacekit_storage_node::content_settlement::{ContentSettlementStore, SettlementReceipt};
    ContentSettlementStore::new(data_dir.as_path()).push_settlement_inbox(&SettlementReceipt {
        tx_hash: reference.to_string(),
        amount: amount_astra.to_string(),
        asset: "ASTRA".to_string(),
        payer_did: payer_did.to_string(),
        beneficiary_did: recipient_did.to_string(),
        scope: scope.to_string(),
        settled_at: chrono::Utc::now().timestamp(),
    })?;
    Ok(())
}

pub fn grant_channel_subscription(
    storage_node: &StorageNode,
    requester_did: &str,
    channel_did: &str,
    payment_reference: Option<String>,
) -> Result<()> {
    let period = 30 * 24 * 3600;
    let expires = chrono::Utc::now().timestamp() as u64 + period;
    content_grants_store(storage_node).grant_channel_subscription(
        requester_did,
        channel_did,
        Some(expires),
        payment_reference,
    )
}

pub fn list_content_grants(
    storage_node: &StorageNode,
    requester_did: &str,
) -> Result<Vec<spacekit_storage_node::content_grants::ContentGrant>> {
    content_grants_store(storage_node).list_for_requester(requester_did)
}

pub fn storage_data_dir(storage_node: &StorageNode) -> PathBuf {
    storage_node.config().data_dir.clone()
}

/// DID used when indexing catalog rows for `spacekit.xyz-website-api` (document owner scope).
pub const WEBSITE_CATALOG_OWNER_DID: &str = "did:spacekit:admin:website-api";

/// POST a signed [`FactPackage`] to the storage node HTTP `/facts` endpoint.
///
/// Website-api streams via `GET /facts/{id}`; CLI `FactStorageEngine` alone is local to the
/// embedded data dir and is not visible to a running `spacekit network` storage node.
pub async fn post_fact_package_http(
    storage_base_url: &str,
    fact: &FactPackage,
) -> Result<(), String> {
    let url = format!("{}/facts", storage_base_url.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .json(fact)
        .send()
        .await
        .map_err(|e| format!("POST /facts: {e}"))?;
    let status = resp.status();
    if status.is_success() || status == reqwest::StatusCode::CREATED {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(format!("POST /facts HTTP {status}: {body}"))
}

/// Catalog row for website-api `GET /api/content` (stored in `content_listings` documents).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContentListingDocument {
    pub content_id: String,
    pub fact_id: String,
    pub channel_did: String,
    pub publisher_did: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub pricing: String,
    #[serde(default)]
    pub price_astra: Option<f64>,
    pub mime_type: String,
    pub media_kind: String,
    pub size_bytes: u64,
    pub filename: String,
    #[serde(default = "default_published_status")]
    pub status: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_time: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_name: Option<String>,
}

fn default_published_status() -> String {
    "published".to_string()
}

pub fn media_kind_from_mime(mime_type: &str) -> &'static str {
    if mime_type.starts_with("video/") {
        "video"
    } else if mime_type.starts_with("image/") {
        "image"
    } else if mime_type.starts_with("audio/") {
        "audio"
    } else {
        "data"
    }
}

pub fn title_from_fact_tags(fact: &FactPackage) -> Option<String> {
    fact.metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("title:").map(str::to_string))
}

pub fn description_from_fact_tags(fact: &FactPackage) -> Option<String> {
    fact.metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("description:").map(str::to_string))
}

pub fn channel_did_from_fact_tags(fact: &FactPackage) -> Option<String> {
    fact.metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("channel:").map(str::to_string))
}

pub fn filename_from_fact_tags(fact: &FactPackage) -> Option<String> {
    fact.metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("filename:").map(str::to_string))
}

pub fn mime_type_from_fact(fact: &FactPackage) -> String {
    match &fact.content {
        FactContent::Binary { mime_type, .. } => mime_type.clone(),
        _ => "application/octet-stream".to_string(),
    }
}

pub fn thumbnail_time_from_fact_tags(fact: &FactPackage) -> Option<f64> {
    fact.metadata.tags.iter().find_map(|t| {
        t.strip_prefix("thumbnail_time:")
            .and_then(|v| v.parse().ok())
    })
}

pub fn duration_from_fact_tags(fact: &FactPackage) -> Option<f64> {
    fact.metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("duration:").and_then(|v| v.parse().ok()))
}

pub fn channel_name_from_fact_tags(fact: &FactPackage) -> Option<String> {
    fact.metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("channel_name:").map(str::to_string))
}

pub fn build_content_listing_from_fact(
    fact: &FactPackage,
    channel_did: &str,
    title: &str,
    description: Option<&str>,
) -> ContentListingDocument {
    let content_id = hex::encode(fact.fact_id);
    let mime_type = mime_type_from_fact(fact);
    let filename = filename_from_fact_tags(fact)
        .unwrap_or_else(|| format!("{}.bin", &content_id[..8.min(content_id.len())]));
    let pricing = fact
        .metadata
        .tags
        .iter()
        .find_map(|t| t.strip_prefix("pricing:").map(str::to_string))
        .unwrap_or_else(|| "free".to_string());
    let price_astra = content_price_astra(fact);
    ContentListingDocument {
        content_id: content_id.clone(),
        fact_id: content_id,
        channel_did: channel_did.to_string(),
        publisher_did: fact.author.to_string(),
        title: title_from_fact_tags(fact)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| title.to_string()),
        description: description
            .map(str::to_string)
            .or_else(|| description_from_fact_tags(fact))
            .unwrap_or_default(),
        pricing,
        price_astra,
        mime_type: mime_type.clone(),
        media_kind: media_kind_from_mime(&mime_type).to_string(),
        size_bytes: fact.metadata.size_bytes,
        filename,
        status: "published".to_string(),
        created_at: chrono::DateTime::from_timestamp(fact.created_at as i64, 0)
            .map(|t| t.to_rfc3339())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339()),
        thumbnail_time: thumbnail_time_from_fact_tags(fact),
        duration_seconds: duration_from_fact_tags(fact),
        channel_name: channel_name_from_fact_tags(fact),
    }
}

/// Index a published fact in the storage node document API for website catalog playback.
///
/// Writes twice (same pattern as agent `deployments` + `app_listings`):
/// - under `publisher_did` (owner copy)
/// - under [`WEBSITE_CATALOG_OWNER_DID`] so website-api can query with admin DID
pub async fn upsert_content_listing_http(
    storage_base_url: &str,
    publisher_did: &str,
    listing: &ContentListingDocument,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let body = serde_json::to_string(listing).map_err(|e| format!("listing JSON: {}", e))?;
    let url = format!(
        "{}/api/documents/content_listings/{}",
        storage_base_url.trim_end_matches('/'),
        listing.content_id
    );
    let mut last_err: Option<String> = None;
    for (did, label) in [
        (publisher_did, "owner"),
        (WEBSITE_CATALOG_OWNER_DID, "website-catalog"),
    ] {
        let resp = client
            .put(&url)
            .header("Authorization", format!("DID {}", did))
            .header("content-type", "application/json")
            .body(body.clone())
            .send()
            .await
            .map_err(|e| format!("PUT content_listings [{label}]: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            last_err = Some(format!(
                "PUT content_listings [{label}] HTTP {status}: {text}"
            ));
        }
    }
    if let Some(e) = last_err {
        return Err(e);
    }
    Ok(())
}

/// Remove a content listing from the website catalog on the storage node.
///
/// Deletes from both owner and [`WEBSITE_CATALOG_OWNER_DID`] so the content
/// disappears from the website immediately.
pub async fn delete_content_listing_http(
    storage_base_url: &str,
    publisher_did: &str,
    content_id: &str,
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!(
        "{}/api/documents/content_listings/{}",
        storage_base_url.trim_end_matches('/'),
        content_id
    );
    let fact_index_url = format!(
        "{}/api/documents/fact_index/{}",
        storage_base_url.trim_end_matches('/'),
        content_id
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
            .map_err(|e| format!("DELETE content_listings [{label}]: {e}"))?;
        if !resp.status().is_success() && resp.status().as_u16() != 404 {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            errors.push(format!("[{label}] HTTP {status}: {text}"));
        }
    }
    // Also remove the fact_index entry
    let _ = client
        .delete(&fact_index_url)
        .header("Authorization", format!("DID {}", publisher_did))
        .send()
        .await;
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok(())
}

/// Determine MIME type from file extension (web packages, media, documents).
fn determine_mime_type(file_path: &str) -> String {
    let path = Path::new(file_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8".to_string(),
        "css" => "text/css; charset=utf-8".to_string(),
        "js" | "mjs" => "application/javascript; charset=utf-8".to_string(),
        "json" => "application/json; charset=utf-8".to_string(),
        "wasm" => "application/wasm".to_string(),
        "svg" => "image/svg+xml".to_string(),
        "mp4" | "mov" | "webm" => "video/mp4".to_string(),
        "mp3" | "wav" | "ogg" => "audio/mpeg".to_string(),
        "jpg" | "jpeg" => "image/jpeg".to_string(),
        "png" => "image/png".to_string(),
        "gif" => "image/gif".to_string(),
        "webp" => "image/webp".to_string(),
        "woff" => "font/woff".to_string(),
        "woff2" => "font/woff2".to_string(),
        "pdf" => "application/pdf".to_string(),
        "txt" => "text/plain; charset=utf-8".to_string(),
        "md" | "markdown" => "text/markdown; charset=utf-8".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

/// Get or create FactStorageEngine from StorageNode
pub async fn get_fact_storage_engine(
    storage_node: &Arc<StorageNode>,
) -> Result<Arc<FactStorageEngine>> {
    // Create FactStorageConfig from StorageNode config
    let storage_config = storage_node.config();
    let fact_storage_config = FactStorageConfig {
        storage_dir: storage_config.data_dir.join("fact_storage"),
        max_fact_size: 100 * 1024 * 1024 * 1024, // 100GB
        enable_compression: true,
        compression_algorithm: CompressionAlgorithm::Gzip,
        enable_deduplication: true,
        verification_cache_size: 10000,
        enable_auto_indexing: true,
        storage_tiers: StorageTierConfig {
            hot_storage_dir: storage_config.data_dir.join("fact_storage/hot"),
            cold_storage_dir: storage_config.data_dir.join("fact_storage/cold"),
            archive_threshold_days: 30,
            max_hot_storage_bytes: 10 * 1024 * 1024 * 1024, // 10GB
        },
    };

    // Create FactStorageEngine
    let fact_storage = FactStorageEngine::new(
        storage_node.database(),
        storage_node.quantum_crypto(),
        fact_storage_config,
    )
    .await?;

    Ok(Arc::new(fact_storage))
}

/// Publish content notification via Messaging Node Gossipsub
pub async fn publish_content_notification(
    messaging_node: &MessagingNode,
    channel_id: &str,
    content_id: &str,
    fact_package_id: &str,
    title: &str,
    pricing: &str,
    price: Option<f64>,
) -> Result<()> {
    // Create notification message
    let notification = json!({
        "type": "content_published",
        "channel_id": channel_id,
        "content_id": content_id,
        "fact_package_id": fact_package_id,
        "title": title,
        "pricing": pricing,
        "price": price,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    // TODO: Use MessagingNode's Gossipsub to publish to topic
    // Topic format: "channel:{channel_id}"
    let topic = format!("channel:{}", channel_id);

    // For now, log the notification
    // In production, this would use: messaging_node.publish_to_topic(&topic, &notification).await?
    println!("Would publish to topic {}: {:?}", topic, notification);

    Ok(())
}

/// Register content with Storage Governance Contract
pub async fn register_content_with_governance(
    compute_node: &ComputeNode,
    contract_id: &str,
    content_id: &str,
    fact_package_id: &str,
    publisher_did: String,
    storage_policy: &StoragePolicy,
    distribution_rule: &DistributionRule,
) -> Result<serde_json::Value> {
    // Call smart contract
    let result = compute_node
        .execute_contract(
            contract_id,
            "register_content",
            vec![
                json!({ "content_id": content_id }),
                json!({ "fact_package_id": fact_package_id }),
                json!({ "storage_policy": storage_policy }),
                json!({ "distribution_rule": distribution_rule }),
            ],
            publisher_did,
            1_000_000, // Gas limit
        )
        .await?;

    Ok(result)
}

/// Storage policy for governance contract
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StoragePolicy {
    pub requires_payment: bool,
    pub payment_amount: Option<f64>,
    pub access_control: String, // "ChannelSubscribers", "Public", etc.
    pub replication_factor: u32,
}

/// Distribution rule for P2P governance contract
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DistributionRule {
    pub p2p_enabled: bool,
    pub chunk_size: u64,
    pub replication_factor: u32,
    pub storage_nodes: Vec<String>,
}
