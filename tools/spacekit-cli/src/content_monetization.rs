//! Production monetization: SpaceKit Pay settlement + entitlement OP_PURCHASE.

use anyhow::{anyhow, Result};
use spacekit_compute_node::ComputeNode;
use spacekit_storage_node::content_entitlement::{
    build_create_listing_payload, build_purchase_payload, buyer_pk_hash_from_bytes,
    license_contract_configured, parse_purchase_result, EntitlementClientConfig, PRICING_ONE_TIME,
    PRICING_SUBSCRIPTION,
};
use spacekit_storage_node::content_escrow::{escrow_contract_configured, escrow_id_for_pending};
use spacekit_storage_node::content_license::{build_mint_payload, parse_mint_result};
use spacekit_storage_node::content_payment::payment_scope_content;
use spacekit_storage_node::content_payment::PaymentReceiptStore;
use spacekit_storage_node::content_settlement::{
    validate_settlement_for_pending, ContentSettlementStore, PendingPurchase, PurchaseKind,
    SettlementReceipt,
};
use spacekit_storage_node::StorageNode;
use std::path::PathBuf;
use std::sync::Arc;

use crate::content_integration::{
    conditional_price_from_policy, content_grants_store, content_price_astra,
    finalize_licensed_feature_install, get_fact_storage_engine, licensed_feature_pending_grant,
    load_licensed_feature_document, parse_content_id_hex, storage_data_dir,
};
use spacekit_primitives::v1::fact::AccessPolicy;

fn buyer_pk_hash_for_monetization() -> Result<[u8; 32]> {
    let config = crate::full_client::load_cli_config_sync().map_err(|e| anyhow!("{e}"))?;
    let home = dirs::home_dir().ok_or_else(|| anyhow!("home directory not found"))?;
    let config_dir = home.join(".spacekit");
    let pk_path: PathBuf = {
        let raw = config.identity.public_key_path.as_str();
        if raw.starts_with('/') {
            PathBuf::from(raw)
        } else if let Some(rest) = raw.strip_prefix("~/") {
            home.join(rest)
        } else {
            config_dir.join(raw)
        }
    };
    let pk_hex = std::fs::read_to_string(&pk_path)
        .map_err(|e| anyhow!("read public key {}: {e}", pk_path.display()))?;
    let pk_bytes = hex::decode(pk_hex.trim()).map_err(|e| anyhow!("decode public key hex: {e}"))?;
    Ok(buyer_pk_hash_from_bytes(&pk_bytes))
}

const DEFAULT_GAS: u64 = 2_000_000;

pub struct PaymentQuote {
    pub pending_id: String,
    pub listing_id: String,
    pub scope: String,
    pub price_astra: f64,
    pub price_units: u64,
    pub publisher_did: String,
    pub pay_to: String,
}

pub fn quote_from_pending(pending: &PendingPurchase) -> PaymentQuote {
    PaymentQuote {
        pending_id: pending.id.clone(),
        listing_id: pending.listing_id.clone(),
        scope: pending.scope.clone(),
        price_astra: pending.price_astra,
        price_units: pending.price_units,
        publisher_did: pending.publisher_did.clone(),
        pay_to: pending.publisher_did.clone(),
    }
}

/// Reuse open pending for content scope when awaiting settlement (avoid duplicate pending rows).
pub async fn resolve_content_pay_quote(
    storage_node: &Arc<StorageNode>,
    buyer_did: &str,
    content_id_hex: &str,
    tier_name: Option<&str>,
    pending_id: Option<&str>,
    await_settlement: bool,
) -> Result<PaymentQuote> {
    let data_dir = storage_data_dir(storage_node);
    let store = ContentSettlementStore::new(data_dir.as_path());
    if let Some(pid) = pending_id {
        let pending = store
            .get_pending(pid)?
            .ok_or_else(|| anyhow!("pending purchase not found: {pid}"))?;
        return Ok(quote_from_pending(&pending));
    }
    let scope = payment_scope_content(content_id_hex);
    if await_settlement {
        if let Some(pending) = store.find_open_pending_for_scope(buyer_did, &scope)? {
            return Ok(quote_from_pending(&pending));
        }
    }
    initiate_content_pay(storage_node, buyer_did, content_id_hex, tier_name).await
}

fn astra_to_units(amount: f64) -> u64 {
    (amount.max(0.0) * 1_000_000.0) as u64
}

pub fn entitlement_configured() -> bool {
    EntitlementClientConfig::from_env().is_some()
}

/// Register listing on entitlement-ledger (call on publish).
pub async fn ensure_content_listing(
    compute: &ComputeNode,
    publisher_did: &str,
    content_id_hex: &str,
    price_astra: f64,
    pricing: &str,
) -> Result<()> {
    let contract_id = std::env::var("SPACEKIT_ENTITLEMENT_CONTRACT_ID")
        .map_err(|_| anyhow!("SPACEKIT_ENTITLEMENT_CONTRACT_ID not set"))?;
    let listing_id = format!("content:{content_id_hex}");
    let pricing_type = if pricing == "subscription" {
        PRICING_SUBSCRIPTION
    } else {
        PRICING_ONE_TIME
    };
    let period = if pricing == "subscription" {
        30 * 24 * 3600
    } else {
        0
    };
    let payload = build_create_listing_payload(
        &listing_id,
        content_id_hex,
        astra_to_units(price_astra),
        "ASTRA",
        pricing_type,
        period,
    );
    let _ = compute
        .call_contract_raw(
            &contract_id,
            payload,
            publisher_did.to_string(),
            0,
            DEFAULT_GAS,
        )
        .await?;
    Ok(())
}

/// Mint AppLicenseNFT for content (opcode `main` on license contract).
pub async fn mint_content_license_on_chain(
    compute: &ComputeNode,
    owner_did: &str,
    content_id_hex: &str,
    price_units: u64,
) -> Result<u64> {
    let contract_id = std::env::var("SPACEKIT_LICENSE_CONTRACT_ID")
        .map_err(|_| anyhow!("SPACEKIT_LICENSE_CONTRACT_ID not set"))?;
    let payload = build_mint_payload(owner_did, content_id_hex, price_units);
    let raw = compute
        .call_contract_raw(&contract_id, payload, owner_did.to_string(), 0, DEFAULT_GAS)
        .await?;
    parse_mint_result(&raw)
}

/// OP_PURCHASE via compute node (sets msg_value). Binds buyer Kyber PK hash at purchase.
pub async fn purchase_listing_on_chain(
    compute: &ComputeNode,
    buyer_did: &str,
    listing_id: &str,
    price_units: u64,
    buyer_pk_hash: [u8; 32],
) -> Result<String> {
    let contract_id = std::env::var("SPACEKIT_ENTITLEMENT_CONTRACT_ID")
        .map_err(|_| anyhow!("SPACEKIT_ENTITLEMENT_CONTRACT_ID not set"))?;
    let payload = build_purchase_payload(listing_id, &buyer_pk_hash);
    let raw = compute
        .call_contract_raw(
            &contract_id,
            payload,
            buyer_did.to_string(),
            price_units as u128,
            DEFAULT_GAS,
        )
        .await?;
    let ent_id = parse_purchase_result(&raw)?;
    Ok(hex::encode(ent_id))
}

/// Initiate paid access: create pending purchase and return quote for payer.
pub async fn initiate_content_pay(
    storage_node: &Arc<StorageNode>,
    buyer_did: &str,
    content_id_hex: &str,
    tier_name: Option<&str>,
) -> Result<PaymentQuote> {
    let fact_storage = get_fact_storage_engine(storage_node).await?;
    let fact_id = parse_content_id_hex(content_id_hex)?;
    let fact = fact_storage
        .retrieve_fact(fact_id)
        .await?
        .ok_or_else(|| anyhow!("content not found"))?;
    let publisher = fact.author.as_str().to_string();

    let (price, grant_opts) =
        if let Some(doc) = load_licensed_feature_document(storage_node, content_id_hex).await? {
            let tier = tier_name.ok_or_else(|| {
                anyhow!(
                    "licensed feature '{}' requires --tier (e.g. personal, commercial)",
                    doc.feature_name
                )
            })?;
            licensed_feature_pending_grant(&doc, tier)?
        } else {
            let price = content_price_astra(&fact).unwrap_or(0.0);
            if price <= 0.0 {
                return Err(anyhow!("content is free; use content view directly"));
            }
            (
                price,
                spacekit_storage_node::content_settlement::PendingGrantOptions::default(),
            )
        };

    let store = ContentSettlementStore::new(storage_data_dir(storage_node).as_path());
    let pending = store.create_pending(
        PurchaseKind::ContentPpv,
        buyer_did,
        &publisher,
        Some(content_id_hex),
        None,
        price,
        Some(grant_opts),
    )?;
    try_create_content_escrow(buyer_did, &publisher, &pending).await?;
    Ok(PaymentQuote {
        pending_id: pending.id,
        listing_id: pending.listing_id,
        scope: pending.scope,
        price_astra: price,
        price_units: pending.price_units,
        publisher_did: publisher.clone(),
        pay_to: publisher,
    })
}

/// Initiate paid channel subscription (pending + quote).
pub async fn initiate_channel_pay(
    storage_node: &Arc<StorageNode>,
    buyer_did: &str,
    channel_did: &str,
    publisher_did: &str,
    price_astra: f64,
) -> Result<PaymentQuote> {
    if price_astra <= 0.0 {
        return Err(anyhow!("channel is free; use content subscribe"));
    }
    let store = ContentSettlementStore::new(storage_data_dir(storage_node).as_path());
    let pending = store.create_pending(
        PurchaseKind::ChannelSubscription,
        buyer_did,
        publisher_did,
        None,
        Some(channel_did),
        price_astra,
        None,
    )?;
    try_create_content_escrow(buyer_did, publisher_did, &pending).await?;
    Ok(PaymentQuote {
        pending_id: pending.id,
        listing_id: pending.listing_id,
        scope: pending.scope,
        price_astra,
        price_units: pending.price_units,
        publisher_did: publisher_did.to_string(),
        pay_to: publisher_did.to_string(),
    })
}

/// After SpaceKit Pay settlement: OP_PURCHASE + grant (full click-to-access flow).
pub async fn complete_pay_flow(
    storage_node: &Arc<StorageNode>,
    compute: &ComputeNode,
    pending_id: &str,
    settlement: SettlementReceipt,
) -> Result<String> {
    let data_dir = storage_data_dir(storage_node);
    let store = ContentSettlementStore::new(data_dir.as_path());
    let pending = store
        .get_pending(pending_id)?
        .ok_or_else(|| anyhow!("pending purchase not found"))?;
    if pending.status == "completed" {
        return pending
            .entitlement_id_hex
            .ok_or_else(|| anyhow!("pending already completed without entitlement id"));
    }
    validate_settlement_for_pending(&pending, &settlement)?;

    store.apply_settlement_to_pending(pending_id, &settlement)?;

    let grant_result: Result<(String, Option<u64>)> = async {
        let entitlement_hex = if entitlement_configured() {
            purchase_listing_on_chain(
                compute,
                &pending.buyer_did,
                &pending.listing_id,
                pending.price_units,
                buyer_pk_hash_for_monetization()?,
            )
            .await?
        } else {
            dev_entitlement_id_from_tx(&settlement.tx_hash)
        };

        let license_token_id = if license_contract_configured() {
            match pending.content_id_hex.as_deref() {
                Some(cid) => Some(
                    mint_content_license_on_chain(
                        compute,
                        &pending.buyer_did,
                        cid,
                        pending.price_units,
                    )
                    .await?,
                ),
                None => None,
            }
        } else {
            None
        };

        store.complete_pending_with_entitlement(
            pending_id,
            &settlement.tx_hash,
            &entitlement_hex,
            None,
            license_token_id,
        )?;
        Ok((entitlement_hex, license_token_id))
    }
    .await;

    match grant_result {
        Ok((entitlement_hex, _)) => {
            release_content_escrow(pending_id).await?;
            store.mark_inbox_processed(&settlement.tx_hash)?;
            if let Some(cid) = pending.content_id_hex.as_deref() {
                if load_licensed_feature_document(storage_node, cid)
                    .await?
                    .is_some()
                {
                    finalize_licensed_feature_install(storage_node, &pending.buyer_did, cid)
                        .await?;
                }
            }
            Ok(entitlement_hex)
        }
        Err(e) => {
            refund_content_purchase(
                storage_node,
                compute,
                pending_id,
                &settlement.tx_hash,
                &e.to_string(),
            )
            .await?;
            Err(e)
        }
    }
}

fn dev_entitlement_id_from_tx(tx_hash: &str) -> String {
    hex::encode(blake3::hash(tx_hash.as_bytes()).as_bytes())
}

fn escrow_arbiter_did() -> String {
    std::env::var("SPACEKIT_ESCROW_ARBITER_DID").unwrap_or_else(|_| "did:spacekit:treasury".into())
}

async fn try_create_content_escrow(
    buyer_did: &str,
    publisher_did: &str,
    pending: &spacekit_storage_node::content_settlement::PendingPurchase,
) -> Result<()> {
    if !escrow_contract_configured() {
        return Ok(());
    }
    let Some(client) = spacekit_storage_node::content_escrow::EscrowClient::from_env() else {
        return Ok(());
    };
    let escrow_id = escrow_id_for_pending(&pending.id);
    client
        .create_open(
            &escrow_id,
            buyer_did,
            publisher_did,
            pending.price_units,
            &escrow_arbiter_did(),
        )
        .await
}

async fn release_content_escrow(pending_id: &str) -> Result<()> {
    if !escrow_contract_configured() {
        return Ok(());
    }
    let Some(client) = spacekit_storage_node::content_escrow::EscrowClient::from_env() else {
        return Ok(());
    };
    client.release(&escrow_id_for_pending(pending_id)).await
}

/// Push a settlement receipt into the storage-node inbox (same path as compute webhook).
pub fn push_settlement_to_storage_inbox(
    storage_node: &StorageNode,
    receipt: &SettlementReceipt,
) -> Result<bool> {
    let store = ContentSettlementStore::new(storage_data_dir(storage_node).as_path());
    if store.is_inbox_processed(&receipt.tx_hash) {
        return Ok(false);
    }
    store.push_settlement_inbox(receipt)?;
    Ok(true)
}

/// SpaceKit Pay router path: verify on compute (forwards webhook) + ensure local inbox entry.
pub async fn route_payment_settlement(
    storage_node: &Arc<StorageNode>,
    receipt: &SettlementReceipt,
) -> Result<()> {
    if let Ok(compute_url) = compute_node_http_url() {
        if let Err(e) = verify_payment_on_compute(
            &receipt.tx_hash,
            &receipt.amount,
            &receipt.payer_did,
            &receipt.beneficiary_did,
            &receipt.scope,
        )
        .await
        {
            eprintln!(
                "payment verify on compute ({}) failed (continuing with local inbox): {e}",
                compute_url
            );
        }
    }
    push_settlement_to_storage_inbox(storage_node, receipt)?;
    Ok(())
}

async fn refund_content_purchase(
    storage_node: &Arc<StorageNode>,
    _compute: &ComputeNode,
    pending_id: &str,
    payment_reference: &str,
    reason: &str,
) -> Result<()> {
    let escrow_id = escrow_id_for_pending(pending_id);
    let mut escrow_refunded = false;
    if escrow_contract_configured() {
        if let Some(client) = spacekit_storage_node::content_escrow::EscrowClient::from_env() {
            match client.refund(&escrow_id).await {
                Ok(()) => escrow_refunded = true,
                Err(e) => {
                    if std::env::var("SPACEKIT_ESCROW_REQUIRED")
                        .ok()
                        .is_some_and(|v| {
                            matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
                        })
                    {
                        return Err(anyhow!(
                            "escrow OP_REFUND failed for {}: {e} (payment reference not released)",
                            escrow_id
                        ));
                    }
                    eprintln!(
                        "escrow refund failed for {}: {e} — falling back to local refund log",
                        escrow_id
                    );
                }
            }
        }
    }

    let data_dir = storage_data_dir(storage_node);
    let reason = if escrow_refunded {
        format!("{reason} (escrow OP_REFUND ok)")
    } else {
        reason.to_string()
    };
    PaymentReceiptStore::from_env_or_data_dir(data_dir.as_path())
        .refund_on_grant_failure(payment_reference, &reason)?;
    Ok(())
}

/// One listener pass: match inbox receipts to open pending purchases and complete.
pub async fn process_settlement_inbox_once(
    storage_node: &Arc<StorageNode>,
    compute: &ComputeNode,
) -> Result<Vec<(String, String)>> {
    let data_dir = storage_data_dir(storage_node);
    let store = ContentSettlementStore::new(data_dir.as_path());
    let mut completed = Vec::new();
    for receipt in store.list_inbox_unprocessed()? {
        let Some(pending) = store.match_pending_for_receipt(&receipt)? else {
            continue;
        };
        if validate_settlement_for_pending(&pending, &receipt).is_err() {
            continue;
        }
        match complete_pay_flow(storage_node, compute, &pending.id, receipt.clone()).await {
            Ok(ent) => completed.push((pending.id, ent)),
            Err(e) => eprintln!("settlement listener: pending {}: {e}", pending.id),
        }
    }
    Ok(completed)
}

/// Poll inbox until pending completes or timeout.
pub async fn await_settlement_for_pending(
    storage_node: &Arc<StorageNode>,
    compute: &ComputeNode,
    pending_id: &str,
    poll_interval_ms: u64,
    timeout_secs: u64,
) -> Result<Option<String>> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        if let Some(ent) = try_auto_complete_from_inbox(storage_node, compute, pending_id).await? {
            return Ok(Some(ent));
        }
        let _ = process_settlement_inbox_once(storage_node, compute).await?;
        if let Some(ent) = try_auto_complete_from_inbox(storage_node, compute, pending_id).await? {
            return Ok(Some(ent));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(std::time::Duration::from_millis(poll_interval_ms)).await;
    }
}

/// Verify on compute, then OP_PURCHASE + local grant for a pending purchase.
pub async fn settle_pending_purchase(
    storage_node: &Arc<StorageNode>,
    compute: &ComputeNode,
    pending_id: &str,
    tx_hash: &str,
    amount: &str,
    payer_did: &str,
) -> Result<String> {
    let data_dir = storage_data_dir(storage_node);
    let store = ContentSettlementStore::new(data_dir.as_path());
    let pending = store
        .get_pending(pending_id)?
        .ok_or_else(|| anyhow!("pending purchase not found"))?;

    let receipt = settlement_from_payment_json(
        tx_hash,
        amount,
        payer_did,
        &pending.publisher_did,
        &pending.scope,
    );
    route_payment_settlement(storage_node, &receipt).await?;

    if let Some(ent) = try_auto_complete_from_inbox(storage_node, compute, pending_id).await? {
        return Ok(ent);
    }
    complete_pay_flow(storage_node, compute, pending_id, receipt).await
}

/// Poll settlement inbox for matching scope and complete if found.
pub async fn try_auto_complete_from_inbox(
    storage_node: &Arc<StorageNode>,
    compute: &ComputeNode,
    pending_id: &str,
) -> Result<Option<String>> {
    let data_dir = storage_data_dir(storage_node);
    let store = ContentSettlementStore::new(data_dir.as_path());
    let pending = store
        .get_pending(pending_id)?
        .ok_or_else(|| anyhow!("pending not found"))?;
    for receipt in store.list_inbox_unprocessed()? {
        if receipt.scope == pending.scope
            && receipt.payer_did == pending.buyer_did
            && receipt.beneficiary_did == pending.publisher_did
        {
            let ent = complete_pay_flow(storage_node, compute, pending_id, receipt.clone()).await?;
            return Ok(Some(ent));
        }
    }
    Ok(None)
}

/// Poll settlement inbox on an interval (used when `spacekit network up` runs storage + compute).
pub async fn run_background_settlement_listener(
    storage_node: Arc<StorageNode>,
    compute: ComputeNode,
    interval_secs: u64,
) {
    loop {
        match process_settlement_inbox_once(&storage_node, &compute).await {
            Ok(done) => {
                for (pending_id, ent) in done {
                    tracing::info!(
                        pending_id = %pending_id,
                        entitlement = %ent,
                        "content settlement listener completed purchase"
                    );
                }
            }
            Err(e) => tracing::warn!("content settlement listener: {e}"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs.max(1))).await;
    }
}

/// Background settlement listener (poll inbox + open pending).
pub async fn run_settlement_listener(
    storage_node: Arc<StorageNode>,
    compute: ComputeNode,
    interval_secs: u64,
    once: bool,
) -> Result<()> {
    loop {
        let done = process_settlement_inbox_once(&storage_node, &compute).await?;
        for (pending_id, ent) in &done {
            println!(
                "settlement listener: completed {} → entitlement {}",
                pending_id, ent
            );
        }
        if once {
            if done.is_empty() {
                println!(
                    "   No open pending matched the settlement inbox (check scope, payer, amount)."
                );
            }
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
    }
    Ok(())
}

fn compute_node_http_url() -> Result<String> {
    std::env::var("SPACEKIT_COMPUTE_URL")
        .or_else(|_| std::env::var("SPACEKIT_COMPUTE_NODE_URL"))
        .map_err(|_| anyhow!("SPACEKIT_COMPUTE_URL not set"))
}

/// POST `/v1/payments/verify` on the compute node (SpaceKit Pay settlement ack).
pub async fn verify_payment_on_compute(
    tx_hash: &str,
    amount: &str,
    payer_did: &str,
    beneficiary_did: &str,
    scope: &str,
) -> Result<()> {
    let base = compute_node_http_url()?;
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "tx_hash": tx_hash,
        "amount": amount,
        "asset": "ASTRA",
        "beneficiary_did": beneficiary_did,
        "payer_did": payer_did,
        "scope": scope,
    });
    let resp = client
        .post(format!("{}/v1/payments/verify", base.trim_end_matches('/')))
        .json(&body)
        .send()
        .await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow!("payment verify HTTP {}: {}", status, text));
    }
    Ok(())
}

/// Record settlement from compute node `/v1/payments/verify` response shape.
pub fn settlement_from_payment_json(
    tx_hash: &str,
    amount: &str,
    payer_did: &str,
    beneficiary_did: &str,
    scope: &str,
) -> SettlementReceipt {
    SettlementReceipt {
        tx_hash: tx_hash.to_string(),
        amount: amount.to_string(),
        asset: "ASTRA".to_string(),
        payer_did: payer_did.to_string(),
        beneficiary_did: beneficiary_did.to_string(),
        scope: scope.to_string(),
        settled_at: chrono::Utc::now().timestamp(),
    }
}

/// Manual purchase command (OP_PURCHASE without prior pending).
pub async fn purchase_content_manual(
    compute: &ComputeNode,
    storage_node: &Arc<StorageNode>,
    buyer_did: &str,
    content_id_hex: &str,
) -> Result<String> {
    let fact_storage = get_fact_storage_engine(storage_node).await?;
    let fact_id = parse_content_id_hex(content_id_hex)?;
    let fact = fact_storage
        .retrieve_fact(fact_id)
        .await?
        .ok_or_else(|| anyhow!("content not found"))?;
    let price = content_price_astra(&fact).unwrap_or(0.0);
    let listing_id = format!("content:{content_id_hex}");
    let ent = purchase_listing_on_chain(
        compute,
        buyer_did,
        &listing_id,
        astra_to_units(price),
        buyer_pk_hash_for_monetization()?,
    )
    .await?;
    let license_token = if license_contract_configured() {
        Some(
            mint_content_license_on_chain(
                compute,
                buyer_did,
                content_id_hex,
                astra_to_units(price),
            )
            .await?,
        )
    } else {
        None
    };
    content_grants_store(storage_node).grant_content_ppv_full(
        buyer_did,
        content_id_hex,
        Some(format!("purchase:{ent}")),
        None,
        Some(ent.clone()),
        None,
        license_token,
        None,
    )?;
    Ok(ent)
}

pub fn is_paid_policy(policy: &AccessPolicy) -> bool {
    conditional_price_from_policy(policy)
        .map(|p| p > 0.0)
        .unwrap_or(false)
}
