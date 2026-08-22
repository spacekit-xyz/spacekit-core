//! Service event types for SRA classification.

pub use crate::log_topics::{
    COMPUTE_CONTRACT_EXECUTED, COMPUTE_HOST_HOOK_INVOKED, CONSENSUS_PROPOSAL_ACCEPTED,
    CONSENSUS_VOTE_CORRECT, LEGACY_COMPUTE, LEGACY_CONSENSUS, LEGACY_MESSAGING, LEGACY_STORAGE,
    MESSAGING_BROADCAST_SENT, MESSAGING_MESSAGE_DELIVERED, STORAGE_BLOB_READ, STORAGE_BLOB_WRITE,
    STORAGE_PROOF_ATTESTED,
};

/// Legacy SRA log topic prefixes (still accepted by the classifier).
pub const SRA_TOPIC_COMPUTE_EXECUTED: &str = LEGACY_COMPUTE;
pub const SRA_TOPIC_STORAGE_WRITE: &str = LEGACY_STORAGE;
pub const SRA_TOPIC_MESSAGING_DELIVERED: &str = LEGACY_MESSAGING;
pub const SRA_TOPIC_CONSENSUS_VOTE: &str = LEGACY_CONSENSUS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceCategory {
    Consensus,
    Compute,
    Storage,
    Messaging,
}

impl ServiceCategory {
    pub const ALL: [ServiceCategory; 4] = [
        ServiceCategory::Consensus,
        ServiceCategory::Compute,
        ServiceCategory::Storage,
        ServiceCategory::Messaging,
    ];

    pub fn index(self) -> usize {
        match self {
            Self::Consensus => 0,
            Self::Compute => 1,
            Self::Storage => 2,
            Self::Messaging => 3,
        }
    }

    pub fn from_service_event(event: spacekit_log::service::ServiceEvent) -> Self {
        match event.category_index() {
            0 => Self::Consensus,
            1 => Self::Compute,
            2 => Self::Storage,
            _ => Self::Messaging,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRewardEvent {
    pub operator_did_hash: [u8; 32],
    pub category: ServiceCategory,
    pub resource_units: u128,
    pub log_event_hash: [u8; 32],
    pub approved: bool,
}

/// Classify an SRA topic string (topic0 UTF-8 or keccak topic label).
pub fn classify_log_topic(topic0: &[u8]) -> Option<(ServiceCategory, u128)> {
    let label = topic_label_str(topic0)?;
    classify_log_label(label).map(|c| (c, 0))
}

/// Classify a NUL-padded or exact UTF-8 topic label.
pub fn classify_log_label(label: &str) -> Option<ServiceCategory> {
    spacekit_log::service::classify_sra_topic(label).map(ServiceCategory::from_service_event)
}

fn topic_label_str(topic0: &[u8]) -> Option<&str> {
    let end = topic0.iter().position(|&b| b == 0).unwrap_or(topic0.len());
    if end == 0 {
        return None;
    }
    std::str::from_utf8(&topic0[..end]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use spacekit_log::service::topic_label_bytes;

    #[test]
    fn canonical_compute_topic() {
        let topic = topic_label_bytes(COMPUTE_CONTRACT_EXECUTED);
        assert_eq!(
            classify_log_topic(&topic),
            Some((ServiceCategory::Compute, 0))
        );
    }
}
