//! Canonical SRA topic labels — re-exported from `spacekit-log` (single source of truth).

pub use spacekit_log::service::topics::{
    COMPUTE_CONTRACT_EXECUTED, COMPUTE_HOST_HOOK_INVOKED, CONSENSUS_PROPOSAL_ACCEPTED,
    CONSENSUS_VOTE_CORRECT, LEGACY_COMPUTE, LEGACY_CONSENSUS, LEGACY_MESSAGING, LEGACY_STORAGE,
    MESSAGING_BROADCAST_SENT, MESSAGING_MESSAGE_DELIVERED, STORAGE_BLOB_READ, STORAGE_BLOB_WRITE,
    STORAGE_PROOF_ATTESTED,
};

pub use spacekit_log::service::topic_label_bytes;

/// Resource units as log `data` (16-byte LE u128).
pub fn resource_units_le(units: u128) -> [u8; 16] {
    units.to_le_bytes()
}
