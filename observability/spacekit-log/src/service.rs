//! Service events for operator rewards (SRA input).
//!
//! Spec: `spacekit-tokenomics/Service_Reward_Accumulator_Spec.md` §3.
//! Each variant maps 1:1 to a canonical SwtchVM log `topic0` label consumed by the SRA.

use crate::EventKind;

/// Operator service work eligible for ASTRA emission (four macro-categories).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ServiceEvent {
    // Consensus (40% share)
    ProposalAccepted,
    VoteCorrect,
    EnvelopeSigned,
    UptimeConfirmed,
    // Compute (30%)
    ContractExecuted,
    HostHookInvoked,
    // Storage (20%)
    BlobServedRead,
    BlobServedWrite,
    ProofAttested,
    CapacityMaintained,
    // Messaging (10%)
    MessageDelivered,
    BroadcastSent,
    KeyResolved,
}

impl ServiceEvent {
    /// Canonical `topic0` UTF-8 label for SwtchVM / SRA classification.
    pub const fn sra_topic_label(self) -> &'static str {
        match self {
            Self::ProposalAccepted => topics::CONSENSUS_PROPOSAL_ACCEPTED,
            Self::VoteCorrect => topics::CONSENSUS_VOTE_CORRECT,
            Self::EnvelopeSigned => topics::CONSENSUS_ENVELOPE_SIGNED,
            Self::UptimeConfirmed => topics::CONSENSUS_UPTIME_CONFIRMED,
            Self::ContractExecuted => topics::COMPUTE_CONTRACT_EXECUTED,
            Self::HostHookInvoked => topics::COMPUTE_HOST_HOOK_INVOKED,
            Self::BlobServedRead => topics::STORAGE_BLOB_READ,
            Self::BlobServedWrite => topics::STORAGE_BLOB_WRITE,
            Self::ProofAttested => topics::STORAGE_PROOF_ATTESTED,
            Self::CapacityMaintained => topics::STORAGE_CAPACITY_MAINTAINED,
            Self::MessageDelivered => topics::MESSAGING_MESSAGE_DELIVERED,
            Self::BroadcastSent => topics::MESSAGING_BROADCAST_SENT,
            Self::KeyResolved => topics::MESSAGING_KEY_RESOLVED,
        }
    }

    /// SRA macro-category index (0=consensus … 3=messaging).
    pub const fn category_index(self) -> usize {
        match self {
            Self::ProposalAccepted
            | Self::VoteCorrect
            | Self::EnvelopeSigned
            | Self::UptimeConfirmed => 0,
            Self::ContractExecuted | Self::HostHookInvoked => 1,
            Self::BlobServedRead
            | Self::BlobServedWrite
            | Self::ProofAttested
            | Self::CapacityMaintained => 2,
            Self::MessageDelivered | Self::BroadcastSent | Self::KeyResolved => 3,
        }
    }
}

/// Canonical topic strings (shared with `spacekit-service-rewards` / SRA classifier).
pub mod topics {
    pub const CONSENSUS_PROPOSAL_ACCEPTED: &str = "consensus.proposal.accepted";
    pub const CONSENSUS_VOTE_CORRECT: &str = "consensus.vote.correct";
    pub const CONSENSUS_ENVELOPE_SIGNED: &str = "consensus.envelope.signed";
    pub const CONSENSUS_UPTIME_CONFIRMED: &str = "consensus.uptime.confirmed";

    pub const COMPUTE_CONTRACT_EXECUTED: &str = "compute.contract.executed";
    pub const COMPUTE_HOST_HOOK_INVOKED: &str = "compute.host_hook.invoked";

    pub const STORAGE_BLOB_READ: &str = "storage.blob.served_read";
    pub const STORAGE_BLOB_WRITE: &str = "storage.blob.served_write";
    pub const STORAGE_PROOF_ATTESTED: &str = "storage.proof.attested";
    pub const STORAGE_CAPACITY_MAINTAINED: &str = "storage.capacity.maintained";

    pub const MESSAGING_MESSAGE_DELIVERED: &str = "messaging.message.delivered";
    pub const MESSAGING_BROADCAST_SENT: &str = "messaging.broadcast.sent";
    pub const MESSAGING_KEY_RESOLVED: &str = "messaging.key.resolved";

    /// Pre-spec bridge labels (still accepted by SRA).
    pub const LEGACY_COMPUTE: &str = "spacekit.sra.compute.executed";
    pub const LEGACY_STORAGE: &str = "spacekit.sra.storage.write";
    pub const LEGACY_MESSAGING: &str = "spacekit.sra.messaging.delivered";
    pub const LEGACY_CONSENSUS: &str = "spacekit.sra.consensus.vote";
}

/// Classify a SwtchVM `topic0` label (NUL-padded or exact UTF-8).
pub fn classify_sra_topic(label: &str) -> Option<ServiceEvent> {
    use topics::*;
    match label {
        CONSENSUS_PROPOSAL_ACCEPTED => Some(ServiceEvent::ProposalAccepted),
        CONSENSUS_VOTE_CORRECT | LEGACY_CONSENSUS => Some(ServiceEvent::VoteCorrect),
        CONSENSUS_ENVELOPE_SIGNED => Some(ServiceEvent::EnvelopeSigned),
        CONSENSUS_UPTIME_CONFIRMED => Some(ServiceEvent::UptimeConfirmed),
        COMPUTE_CONTRACT_EXECUTED | LEGACY_COMPUTE => Some(ServiceEvent::ContractExecuted),
        COMPUTE_HOST_HOOK_INVOKED => Some(ServiceEvent::HostHookInvoked),
        STORAGE_BLOB_READ => Some(ServiceEvent::BlobServedRead),
        STORAGE_BLOB_WRITE | LEGACY_STORAGE => Some(ServiceEvent::BlobServedWrite),
        STORAGE_PROOF_ATTESTED => Some(ServiceEvent::ProofAttested),
        STORAGE_CAPACITY_MAINTAINED => Some(ServiceEvent::CapacityMaintained),
        MESSAGING_MESSAGE_DELIVERED | LEGACY_MESSAGING => Some(ServiceEvent::MessageDelivered),
        MESSAGING_BROADCAST_SENT => Some(ServiceEvent::BroadcastSent),
        MESSAGING_KEY_RESOLVED => Some(ServiceEvent::KeyResolved),
        _ => None,
    }
}

/// SwtchVM log bridge: UTF-8 topic0 + 16-byte LE `resource_units` data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SraSwtchvmLog {
    pub topic0: [u8; 32],
    pub data: [u8; 16],
}

impl SraSwtchvmLog {
    pub fn new(service: ServiceEvent, resource_units: u128) -> Self {
        Self {
            topic0: topic_label_bytes(service.sra_topic_label()),
            data: resource_units.to_le_bytes(),
        }
    }
}

/// Pad/truncate a topic label to 32 bytes (SwtchVM convention).
pub fn topic_label_bytes(label: &str) -> [u8; 32] {
    let mut topic = [0u8; 32];
    let bytes = label.as_bytes();
    let n = bytes.len().min(32);
    topic[..n].copy_from_slice(&bytes[..n]);
    topic
}

/// Required structured field for SRA resource accounting.
pub const FIELD_RESOURCE_UNITS: &str = "resource_units";

/// Build `EventKind::Service` for emission sites.
pub fn service_kind(event: ServiceEvent) -> EventKind {
    EventKind::Service(event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_roundtrip() {
        let ev = ServiceEvent::ContractExecuted;
        let topic = topic_label_bytes(ev.sra_topic_label());
        let end = topic.iter().position(|&b| b == 0).unwrap();
        let label = core::str::from_utf8(&topic[..end]).unwrap();
        assert_eq!(classify_sra_topic(label), Some(ev));
    }
}
