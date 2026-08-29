//! Host for [`spacekit_unified_consensus`] on the compute node.
//!
//! Wires [`ConsensusCoordinator`] into [`ReputationWeightedConsensus`] and
//! owns an optional [`SpacetimeExtension`] for rotor paths through the facade.
//! PBFT quorum stays in the coordinator; spacetime augments when feature
//! `spacetime-consensus` is enabled. Fingerprints / finality / fraud proofs
//! use `spacetime_integration.rs` + coordinator state (see unified-consensus README).
//!
//! ## Growformer-through-host (planned)
//!
//! 1. **Telemetry** — non-blocking observation after inference (like P2P vote telemetry).
//! 2. **Parameter ratification routing** — host intermediates `ParameterChangeProposal` flow.
//! 3. **Avoid** per-block inference inside `collect_weighted_votes` unless measured need.
//!
//! **Failure mode:** when Growformer is unreachable or low-confidence, log and continue
//! with last-ratified static thresholds — **no gating** of consensus or the facade.

extern crate alloc;

use std::sync::Arc;

use alloy_primitives::{keccak256, B256};
use spacekit_spacetime_consensus::{
    consensus::SpacetimeExtension, proposal::SpacetimeTransition, rotor::Rotor,
};
use spacekit_unified_consensus::{
    spacetime_integration::{BlockSpacetimeData, SpacetimeIntegrationError},
    CoordinatorHandle, EqualWeightReputation, FacadeConfig, FacadeError, ReputationSource,
    ReputationWeightedConsensus, WeightedVotingResult,
};

use crate::consensus_coordinator::{ConsensusCoordinator, CoordinatorRoundSnapshot};

/// Sync handle over a [`CoordinatorRoundSnapshot`] — implements [`CoordinatorHandle`].
pub struct CoordinatorRoundHandle {
    snapshot: CoordinatorRoundSnapshot,
    coordinator: Arc<ConsensusCoordinator>,
}

impl CoordinatorRoundHandle {
    pub fn from_snapshot(
        coordinator: Arc<ConsensusCoordinator>,
        snapshot: CoordinatorRoundSnapshot,
    ) -> Self {
        Self {
            snapshot,
            coordinator,
        }
    }
}

impl CoordinatorHandle for CoordinatorRoundHandle {
    fn eligible_validators(&self) -> Vec<(B256, u128)> {
        self.snapshot.eligible.clone()
    }

    fn submit_vote_raw(
        &mut self,
        validator_did: B256,
        block_hash: B256,
        support: bool,
    ) -> Result<bool, alloc::string::String> {
        if block_hash != self.snapshot.block_hash {
            return Ok(false);
        }
        // `block_on` is safe here: `record_vote_by_did_hash` only touches coordinator
        // vote maps and must not call back into the facade (would deadlock on re-entry).
        let rt = tokio::runtime::Handle::current();
        rt.block_on(self.coordinator.record_vote_by_did_hash(
            &self.snapshot.proposal_id,
            validator_did,
            support,
        ))
    }

    fn supporting_vote_count(&self, block_hash: &B256) -> u64 {
        if *block_hash == self.snapshot.block_hash {
            self.snapshot.approve_count
        } else {
            0
        }
    }

    fn eligible_validator_count(&self) -> u64 {
        self.snapshot.eligible_count
    }

    fn is_soft_finalized(&self, block_hash: &B256) -> bool {
        *block_hash == self.snapshot.block_hash && self.snapshot.finalized
    }

    fn supporting_validators(&self, block_hash: &B256) -> Vec<B256> {
        if *block_hash == self.snapshot.block_hash {
            self.snapshot.supporting.clone()
        } else {
            Vec::new()
        }
    }
}

/// Production integration point: coordinator + reputation facade + spacetime extension.
pub struct UnifiedConsensusHost {
    coordinator: Arc<ConsensusCoordinator>,
    reputation: Arc<dyn ReputationSource>,
    config: FacadeConfig,
    spacetime: SpacetimeExtension,
}

impl UnifiedConsensusHost {
    pub fn new(coordinator: Arc<ConsensusCoordinator>) -> Self {
        Self {
            coordinator,
            reputation: Arc::new(EqualWeightReputation),
            config: FacadeConfig::default(),
            spacetime: SpacetimeExtension::default(),
        }
    }

    pub fn with_reputation(
        coordinator: Arc<ConsensusCoordinator>,
        reputation: Arc<dyn ReputationSource>,
        config: FacadeConfig,
    ) -> Result<Self, FacadeError> {
        if config.use_weighted_threshold && !reputation.is_authoritative() {
            return Err(FacadeError::InvalidConfiguration(
                "use_weighted_threshold=true requires an authoritative ReputationSource".into(),
            ));
        }
        Ok(Self {
            coordinator,
            reputation,
            config,
            spacetime: SpacetimeExtension::default(),
        })
    }

    pub fn coordinator(&self) -> &Arc<ConsensusCoordinator> {
        &self.coordinator
    }

    pub fn spacetime_extension(&self) -> &SpacetimeExtension {
        &self.spacetime
    }

    pub fn spacetime_extension_mut(&mut self) -> &mut SpacetimeExtension {
        &mut self.spacetime
    }

    /// `keccak256(proposal_id)` — stable key for facade vote aggregation.
    pub fn proposal_block_hash(proposal_id: &str) -> B256 {
        keccak256(proposal_id.as_bytes())
    }

    /// Start the P2P vote listener with per-vote facade telemetry (non-gating).
    pub fn start_p2p_listener(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        self.coordinator.start_listener(Some(self.clone()))
    }

    /// Observational facade telemetry after a vote is recorded. Does not gate acceptance.
    pub async fn observe_vote_round(&self, proposal_id: &str) {
        let voting = self.collect_weighted_votes(proposal_id).await;
        tracing::debug!(
            proposal_id = %proposal_id,
            supporting_count = voting.supporting_count,
            eligible_count = voting.eligible_count,
            participation = voting.participation_rate(),
            supporting_power_ratio = voting.supporting_power_ratio(),
            coordinator_finalized = voting.coordinator_finalized,
            "unified consensus facade vote telemetry"
        );
    }

    fn reputation_box(&self) -> alloc::boxed::Box<dyn ReputationSource> {
        struct ArcReputation(Arc<dyn ReputationSource>);
        impl ReputationSource for ArcReputation {
            fn reputation_of(&self, did: &B256) -> Option<f64> {
                self.0.reputation_of(did)
            }
            fn is_authoritative(&self) -> bool {
                self.0.is_authoritative()
            }
        }
        alloc::boxed::Box::new(ArcReputation(self.reputation.clone()))
    }

    fn facade_for_round(
        &self,
        snapshot: CoordinatorRoundSnapshot,
    ) -> ReputationWeightedConsensus<CoordinatorRoundHandle> {
        let handle = CoordinatorRoundHandle::from_snapshot(self.coordinator.clone(), snapshot);
        ReputationWeightedConsensus::new(handle, self.reputation_box(), self.config.clone())
            .expect("host config validated at construction")
    }

    /// Collect weighted vote telemetry for a proposal (equal-weight threshold today).
    pub async fn collect_weighted_votes(&self, proposal_id: &str) -> WeightedVotingResult {
        let snapshot = self.coordinator.capture_round_snapshot(proposal_id).await;
        let block_hash = snapshot.block_hash;
        let facade = self.facade_for_round(snapshot);
        facade.collect_weighted_votes(block_hash)
    }

    /// Whether the facade considers quorum reached for this proposal.
    pub async fn has_consensus(&self, proposal_id: &str) -> Result<(), FacadeError> {
        let voting = self.collect_weighted_votes(proposal_id).await;
        let snapshot = self.coordinator.capture_round_snapshot(proposal_id).await;
        let facade = self.facade_for_round(snapshot);
        facade.has_consensus(&voting)
    }

    /// Aggregate validator rotors for a block via the spacetime extension (geometric median).
    pub async fn aggregate_votes(
        &self,
        proposal_id: &str,
        block_data: &BlockSpacetimeData,
    ) -> Result<Rotor, SpacetimeIntegrationError> {
        let snapshot = self.coordinator.capture_round_snapshot(proposal_id).await;
        let block_hash = snapshot.block_hash;
        let facade = self.facade_for_round(snapshot);
        let voting = facade.collect_weighted_votes(block_hash);
        facade.aggregate_votes(&self.spacetime, block_data, &voting)
    }

    /// Verify a validator transition against the proposer claim.
    pub fn verify_transition(
        &self,
        transition: &SpacetimeTransition,
        prev_state: &spacekit_spacetime_consensus::algebra::Multivector,
        new_state: &spacekit_spacetime_consensus::algebra::Multivector,
        prev_coord: &spacekit_spacetime_consensus::causal::CausalCoord,
        state_tolerance: f64,
        hash_fn: impl Fn(&[u8]) -> [u8; 32] + Copy,
    ) -> Result<(), SpacetimeIntegrationError> {
        let snapshot = CoordinatorRoundSnapshot {
            proposal_id: String::new(),
            block_hash: B256::ZERO,
            eligible: Vec::new(),
            supporting: Vec::new(),
            approve_count: 0,
            eligible_count: 0,
            finalized: false,
        };
        let facade = self.facade_for_round(snapshot);
        facade.verify_transition(
            &self.spacetime,
            transition,
            prev_state,
            new_state,
            prev_coord,
            state_tolerance,
            hash_fn,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::{NetworkConfig, NetworkService};
    use crate::quantum_security::{QuantumResistantDID, QuantumResistantEncryption};

    async fn test_coordinator() -> (Arc<ConsensusCoordinator>, String) {
        let identity = Arc::new(QuantumResistantDID::new());
        let encryption = Arc::new(
            QuantumResistantEncryption::new("kyber512", &["kyber512".to_string()])
                .await
                .unwrap(),
        );
        let net = NetworkService::new(
            NetworkConfig {
                network_name: "test".into(),
                listen_address: "127.0.0.1".into(),
                listen_port: 0,
                bootstrap_nodes: vec![],
                max_peers: 1,
            },
            identity.clone(),
            encryption,
        )
        .await
        .unwrap();
        let did = crate::quantum_security::quantum_did_utils::get_did(&identity);
        let cc = Arc::new(ConsensusCoordinator::new(net, did.clone()));
        cc.register_validator(did.clone()).await;
        (cc, did)
    }

    #[tokio::test]
    async fn host_collects_weighted_votes_for_proposal() {
        let (cc, did) = test_coordinator().await;
        let host = UnifiedConsensusHost::new(cc.clone());
        let pid = "proposal-test-1";
        let did_hash = keccak256(did.as_bytes());
        assert!(cc
            .record_vote_by_did_hash(pid, did_hash, true)
            .await
            .unwrap());
        let voting = host.collect_weighted_votes(pid).await;
        assert_eq!(
            voting.block_hash,
            UnifiedConsensusHost::proposal_block_hash(pid)
        );
        assert_eq!(voting.supporting_count, 1);
    }
}
