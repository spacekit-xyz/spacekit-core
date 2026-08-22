//! Subscriber / light-client sync helpers for the **SwtchVM + L1 manifest** plane.
//!
//! This is intentionally **narrower** than [`crate::spacetime_consensus::light_client`], which
//! verifies **rotor chains** for reputation-weighted spacetime consensus. Subscribers that
//! only care about **VM head + optional on-disk L1 snapshot** can follow the HTTP flow below
//! without pulling in the spacetime wire format.
//!
//! ## Suggested subscriber protocol (HTTP)
//!
//! 1. **`GET /v1/sync/subscriber`** on a validator/full node — returns [`SubscriberSyncBundle`]:
//!    logical `chain_id`, SwtchVM head summary, optional latest `SnapshotManifest` from
//!    [`SwtchvmNode::read_l1_manifest`], and relative endpoint hints.
//! 2. **`GET /block/header/{n}`** (SwtchVM dev API, same host when merged in standalone) — verify
//!    parent links and timestamps for the range you care about.
//! 3. **`GET /l1/manifest`** (SwtchVM route) — fetch the raw manifest JSON when you need blob hash
//!    / witness fields not duplicated in the sync bundle.
//! 4. **`POST /v1/consensus/propose`** — submit a [`crate::swtch_consensus::BlockProposal`] whose
//!    [`crate::swtch_consensus::BlockData::l1_manifest`] can be filled from the snapshot when
//!    [`merge_l1_manifest_for_proposal`] accepts the disk manifest (`use_l1_snapshot_manifest` on
//!    the operator API).
//!
//! Rotor / transition proofs for spacetime consensus live in the in-repo
//! **spacekit-spacetime-consensus** package (sources under `src/spacetime_consensus/`, including
//! `light_client.rs`).

use hex;
use serde::Serialize;

use crate::spacekitvm::{minimal_l1_manifest_for_proposal, SnapshotManifest, SwtchvmNode};
use crate::swtch_consensus::normalize_hex_lower;

/// Bumped when the JSON shape of [`SubscriberSyncBundle`] changes.
pub const SUBSCRIBER_SYNC_WIRE_VERSION: u32 = 1;

/// High-level SwtchVM head for subscribers (hex roots match [`BlockData`] / proposals).
#[derive(Debug, Clone, Serialize)]
pub struct HeadSummary {
    pub number: u64,
    pub hash_hex: String,
    pub parent_hash_hex: String,
    pub state_root_hex: String,
    pub timestamp: u64,
}

/// Relative paths on the merged standalone server (no host).
#[derive(Debug, Clone, Serialize)]
pub struct SyncEndpointHints {
    pub block_header_template: &'static str,
    pub l1_manifest_path: &'static str,
    pub consensus_propose_path: &'static str,
    pub consensus_finality_path: &'static str,
    pub spacetime_light_client_note: &'static str,
}

/// Wire bundle for stateless / thin subscribers.
#[derive(Debug, Clone, Serialize)]
pub struct SubscriberSyncBundle {
    pub wire_version: u32,
    /// Logical chain id from compute config / env (`SPACEKIT_CHAIN_ID` flows into L1 config).
    pub chain_id: String,
    pub head: HeadSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub l1_manifest: Option<SnapshotManifest>,
    pub endpoints: SyncEndpointHints,
}

/// Build the subscriber sync payload from an in-process [`SwtchvmNode`].
pub fn build_subscriber_sync_bundle(
    vm: &SwtchvmNode,
    compute_chain_id: &str,
) -> SubscriberSyncBundle {
    let head_block = vm.get_latest_block();
    let head = HeadSummary {
        number: head_block.number,
        hash_hex: format!("0x{}", hex::encode(head_block.hash)),
        parent_hash_hex: format!("0x{}", hex::encode(head_block.parent_hash)),
        state_root_hex: format!("0x{}", hex::encode(head_block.state_root)),
        timestamp: head_block.timestamp,
    };
    let l1_manifest = vm.read_l1_manifest().ok().flatten();
    SubscriberSyncBundle {
        wire_version: SUBSCRIBER_SYNC_WIRE_VERSION,
        chain_id: compute_chain_id.to_string(),
        head,
        l1_manifest,
        endpoints: SyncEndpointHints {
            block_header_template: "/block/header/{n}",
            l1_manifest_path: "/l1/manifest",
            consensus_propose_path: "/v1/consensus/propose",
            consensus_finality_path: "/v1/consensus/finality?proposal_id=",
            spacetime_light_client_note:
                "Rotor-chain proofs: see spacekit-spacetime-consensus / src/spacetime_consensus/light_client.rs.",
        },
    }
}

/// Pick an [`SnapshotManifest`] for a unified [`crate::swtch_consensus::BlockProposal`].
///
/// When `prefer_disk` is true and `disk` is `Some`, the disk manifest is reused only if
/// **`checkpoint.height` == `block_number`** and **`checkpoint.state_root_hex`** matches
/// `state_root` (after [`normalize_hex_lower`]). Otherwise falls back to
/// [`minimal_l1_manifest_for_proposal`].
pub fn merge_l1_manifest_for_proposal(
    disk: Option<&SnapshotManifest>,
    chain_id: &str,
    state_root: &str,
    block_number: u64,
    parent_hash: &str,
    prefer_disk: bool,
) -> SnapshotManifest {
    if prefer_disk {
        if let Some(m) = disk {
            let root_ok = normalize_hex_lower(&m.checkpoint.state_root_hex)
                == normalize_hex_lower(state_root);
            let height_ok = m.checkpoint.height == block_number;
            if root_ok && height_ok {
                return m.clone();
            }
        }
    }
    minimal_l1_manifest_for_proposal(chain_id, state_root, block_number, parent_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_uses_disk_when_height_and_root_match() {
        let disk = minimal_l1_manifest_for_proposal("net-a", "0xdead", 3, "0xparent");
        let got =
            merge_l1_manifest_for_proposal(Some(&disk), "ignored", "0xdead", 3, "0xparent", true);
        assert_eq!(got.blob_sha256_hex, disk.blob_sha256_hex);
        assert_eq!(got.checkpoint.height, 3);
    }

    #[test]
    fn merge_falls_back_when_height_mismatches() {
        let disk = minimal_l1_manifest_for_proposal("net-a", "0xdead", 3, "0xparent");
        let got =
            merge_l1_manifest_for_proposal(Some(&disk), "net-a", "0xdead", 4, "0xparent", true);
        assert_eq!(got.checkpoint.height, 4);
        assert_eq!(got.chain_id, "net-a");
    }

    #[test]
    fn merge_falls_back_when_root_mismatches() {
        let disk = minimal_l1_manifest_for_proposal("net-a", "0xdead", 3, "0xparent");
        let got =
            merge_l1_manifest_for_proposal(Some(&disk), "net-a", "0xbeef", 3, "0xparent", true);
        assert_eq!(normalize_hex_lower(&got.checkpoint.state_root_hex), "beef");
    }
}
