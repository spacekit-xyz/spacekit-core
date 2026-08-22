//! Build `growformer::entitlement::EntitlementContext` from SpaceKit storage grants + installs.
//!
//! See `spacekit-storage-node/GROWFORMER_SPEC.md` §4.3.

use anyhow::{anyhow, Result};
use growformer::entitlement::{EntitlementContext, CAP_INFER, CAP_MERGE, CAP_TRAIN};
use spacekit_storage_node::StorageNode;
use std::sync::Arc;

use crate::content_integration::{
    ensure_content_entitlement_for_agent, find_licensed_feature_content_id, get_content_install,
    list_content_installs, load_licensed_feature_document,
};
use spacekit_storage_node::content_installs::find_install_by_app_slug;

pub const FEATURE_NAME: &str = "growformer";

fn skip_entitlement_check() -> bool {
    std::env::var("SPACEKIT_GROWFORMER_SKIP_ENTITLEMENT")
        .ok()
        .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

pub fn skip_entitlement_for_env() -> bool {
    skip_entitlement_check()
}

/// Local `.bin` infer does not need storage-node grants; avoids redb lock when training runs.
pub fn local_dev_entitlement_context(requester_did: &str) -> EntitlementContext {
    EntitlementContext {
        user_did: requester_did.to_string(),
        tier_name: "dev".to_string(),
        active_capabilities: vec![
            CAP_TRAIN.to_string(),
            CAP_INFER.to_string(),
            CAP_MERGE.to_string(),
        ],
        expires_at: 0,
        quota_remaining: None,
        on_chain_verified: false,
    }
}

/// Resolve the canonical growformer content id for entitlement (env → install → feature fact → single install).
pub async fn resolve_growformer_content_id(
    storage_node: &Arc<StorageNode>,
    requester_did: &str,
) -> Result<String> {
    if let Ok(id) = std::env::var("GROWFORMER_CONTENT_ID") {
        let t = id.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Some(install) = find_install_by_app_slug(
        storage_node.database().as_ref(),
        requester_did,
        FEATURE_NAME,
    )? {
        return Ok(install.content_id_hex);
    }
    if let Some(id) =
        find_licensed_feature_content_id(storage_node, requester_did, FEATURE_NAME).await?
    {
        return Ok(id);
    }
    let installs = list_content_installs(storage_node, requester_did)?;
    if installs.len() == 1 {
        return Ok(installs[0].content_id_hex.clone());
    }
    Err(anyhow!(
        "growformer content id unknown — run `spacekit content view --content-id <id>` \
         or `spacekit content access --feature growformer`, or set GROWFORMER_CONTENT_ID"
    ))
}

/// Construct entitlement context after verifying access (local grants + fact policy).
pub async fn build_growformer_entitlement_context(
    storage_node: &Arc<StorageNode>,
    requester_did: &str,
) -> Result<EntitlementContext> {
    if skip_entitlement_check() {
        return Ok(dev_bypass_context(requester_did));
    }

    let content_id = resolve_growformer_content_id(storage_node, requester_did).await?;
    ensure_content_entitlement_for_agent(storage_node, &content_id, requester_did).await?;

    let install = get_content_install(storage_node, requester_did, &content_id)?;
    let tier_name = install
        .as_ref()
        .and_then(|i| i.tier.clone())
        .unwrap_or_else(|| "free".to_string());
    let expires_at = grant_expires_at(storage_node, requester_did, &content_id);
    let on_chain = install
        .as_ref()
        .and_then(|i| i.entitlement_id_hex.clone())
        .is_some();

    let feature_doc = load_licensed_feature_document(storage_node, &content_id).await?;
    let active_capabilities = if let Some(ref doc) = feature_doc {
        doc.capabilities_for_tier(&tier_name)
    } else {
        vec![
            CAP_TRAIN.to_string(),
            CAP_INFER.to_string(),
            CAP_MERGE.to_string(),
        ]
    };
    let quota_remaining = {
        use spacekit_storage_node::content_grants::ContentGrantStore;
        let store =
            ContentGrantStore::from_env_or_data_dir(storage_node.config().data_dir.as_path());
        store
            .quota_remaining_for_content(requester_did, &content_id)
            .or_else(|| {
                feature_doc
                    .as_ref()
                    .and_then(|d| d.quota_for_tier(&tier_name))
            })
    };

    if active_capabilities.is_empty() {
        return Err(anyhow!(
            "tier '{}' for growformer does not include any capabilities",
            tier_name
        ));
    }

    Ok(EntitlementContext {
        user_did: requester_did.to_string(),
        tier_name,
        active_capabilities,
        expires_at,
        quota_remaining,
        on_chain_verified: on_chain,
    })
}

/// Persist one growformer operation against the local grant quota (after successful CLI run).
pub fn consume_growformer_quota(
    storage_node: &StorageNode,
    requester_did: &str,
    content_id_hex: &str,
) -> Result<()> {
    use spacekit_storage_node::content_grants::ContentGrantStore;
    let store = ContentGrantStore::from_env_or_data_dir(storage_node.config().data_dir.as_path());
    let _ = store.consume_content_quota(requester_did, content_id_hex)?;
    Ok(())
}

fn grant_expires_at(storage_node: &StorageNode, requester_did: &str, content_id_hex: &str) -> u64 {
    use spacekit_storage_node::content_grants::ContentGrantStore;
    let store = ContentGrantStore::from_env_or_data_dir(storage_node.config().data_dir.as_path());
    store
        .list_for_requester(requester_did)
        .ok()
        .and_then(|grants| {
            grants
                .into_iter()
                .find(|g| g.content_id_hex.as_deref() == Some(content_id_hex))
                .and_then(|g| g.expires_at)
        })
        .unwrap_or(0)
}

fn dev_bypass_context(requester_did: &str) -> EntitlementContext {
    local_dev_entitlement_context(requester_did)
}

/// Verify a single growformer capability before in-process paths (e.g. `agent infer --name`).
pub async fn ensure_growformer_capability(
    storage_node: &Arc<StorageNode>,
    requester_did: &str,
    capability: &str,
) -> Result<()> {
    let ctx = build_growformer_entitlement_context(storage_node, requester_did).await?;
    if ctx.has_active_entitlement_for(capability) {
        ctx.consume_quota(capability).map_err(|e| anyhow!(e))?;
        Ok(())
    } else {
        Err(anyhow!(
            "No active entitlement for {} (tier: {}). \
             Obtain access: spacekit content view --content-id <growformer_id> \
             or spacekit content access --feature growformer",
            capability,
            ctx.tier_name
        ))
    }
}
