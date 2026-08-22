//! Bridges unified [`crate::consensus::BlockData`] to **spacekit-spacetime-consensus** (rotor
//! transitions + light-client chain checks).

#[cfg(feature = "spacetime-consensus")]
mod inner {
    use alloy_primitives::keccak256;
    use alloy_primitives::B256;
    use spacekit_spacetime_consensus::causal::CausalCoord;
    use spacekit_spacetime_consensus::{
        verify_quorum_against_envelope, verify_rotor_chain, BlockEnvelope, RotorChainProof,
        SpacetimeTransition, TransitionWitness, SPACETIME_WIRE_VERSION,
    };

    use crate::spacekitvm::swtchvm_node::SwtchvmNode;
    use crate::spacetime_state;
    use crate::swtch_consensus::BlockData;

    fn normalize_state_root_hex(s: &str) -> String {
        s.trim()
            .strip_prefix("0x")
            .or_else(|| s.trim().strip_prefix("0X"))
            .unwrap_or(s.trim())
            .to_lowercase()
    }

    /// Parse a 64-char lowercase hex state root (no `0x`) into [`B256`].
    pub fn state_root_hex_to_b256(state_root_hex: &str) -> Option<B256> {
        let s = normalize_state_root_hex(state_root_hex);
        if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let bytes = hex::decode(s.as_bytes()).ok()?;
        if bytes.len() != 32 {
            return None;
        }
        Some(B256::from_slice(&bytes))
    }

    /// When [`SpacetimeTransition`] is present on block data, require:
    /// - `transition.new_state_hash` matches the 32-byte body of `block_data.state_root` hex, and
    /// - [`verify_rotor_chain`] accepts a singleton [`RotorChainProof`] anchored at `prev_state_hash`.
    pub fn validate_block_spacetime_sidecar(
        transition: &SpacetimeTransition,
        block_state_root_hex: &str,
    ) -> bool {
        let Some(root) = state_root_hex_to_b256(block_state_root_hex) else {
            return false;
        };
        if transition.new_state_hash != root {
            return false;
        }
        let proof = RotorChainProof {
            wire_version: SPACETIME_WIRE_VERSION,
            anchor_state_hash: transition.prev_state_hash,
            anchor_coord: CausalCoord::ORIGIN,
            transitions: vec![*transition],
        };
        verify_rotor_chain(&proof).is_ok()
    }

    /// When PQ envelope fields are present on [`BlockData`], verify SPHINCS+ outer signature,
    /// Dilithium inner votes, and binding to `state_root` / `block_number` / `chain_id`.
    pub fn validate_block_pq_envelope(block: &BlockData) -> bool {
        let Some(ref signed) = block.signed_block_envelope else {
            return true;
        };
        if !signed.verify() {
            return false;
        }
        let env = &signed.envelope;
        if env.height != block.block_number {
            return false;
        }
        if env.chain_id != block.l1_manifest.chain_id {
            return false;
        }
        let Some(root) = state_root_hex_to_b256(&block.state_root) else {
            return false;
        };
        if env.state_root != root {
            return false;
        }
        if let Some(ref votes) = block.consensus_votes {
            if verify_quorum_against_envelope(env, votes).is_err() {
                return false;
            }
        }
        true
    }

    /// Proposal hash for inner Dilithium votes (binds block body + optional spacetime digest).
    pub fn block_proposal_hash(block: &BlockData) -> alloy_primitives::B256 {
        const DOMAIN: &[u8] = b"spacekit-block-proposal-v1";
        let state_root = state_root_hex_to_b256(&block.state_root).unwrap_or_default();
        let parent = keccak256(block.parent_hash.as_bytes());
        let mut buf = Vec::new();
        buf.extend_from_slice(DOMAIN);
        buf.extend_from_slice(&block.block_number.to_le_bytes());
        buf.extend_from_slice(parent.as_slice());
        buf.extend_from_slice(state_root.as_slice());
        buf.extend_from_slice(block.l1_manifest.chain_id.as_bytes());
        if let Some(ref t) = block.spacetime_transition {
            buf.extend_from_slice(t.digest(|b| *keccak256(b)).as_slice());
        }
        keccak256(buf)
    }

    /// Build a [`BlockEnvelope`] skeleton from [`BlockData`] and a vote Merkle root (finisher fills timestamp).
    pub fn block_envelope_from_data(
        block: &BlockData,
        round: u64,
        view: u64,
        votes_merkle_root: alloy_primitives::B256,
        tx_root: alloy_primitives::B256,
        block_body_hash: alloy_primitives::B256,
        l1_manifest_hash: alloy_primitives::B256,
        spacetime_tip_hash: alloy_primitives::B256,
        timestamp: u64,
    ) -> Option<BlockEnvelope> {
        let state_root = state_root_hex_to_b256(&block.state_root)?;
        let parent_hash = {
            let h = block
                .parent_hash
                .trim()
                .strip_prefix("0x")
                .unwrap_or(block.parent_hash.trim());
            if h.len() == 64 {
                alloy_primitives::B256::from_slice(&hex::decode(h).ok()?)
            } else {
                keccak256(block.parent_hash.as_bytes())
            }
        };
        Some(BlockEnvelope {
            wire_version: spacekit_spacetime_consensus::PQ_ENVELOPE_WIRE_VERSION,
            round,
            view,
            chain_id: block.l1_manifest.chain_id.clone(),
            height: block.block_number,
            parent_hash,
            state_root,
            tx_root,
            l1_manifest_hash,
            spacetime_tip_hash,
            votes_merkle_root,
            block_body_hash,
            timestamp,
        })
    }

    /// Transition digest for envelope binding (same as `SpacetimeTransition::digest`).
    pub fn spacetime_transition_digest(transition: &SpacetimeTransition) -> alloy_primitives::B256 {
        transition.digest(|b| *keccak256(b))
    }

    /// Reconstruct transition witnesses from PQ votes + block sidecar (no separate gossip).
    pub fn transition_witnesses_from_block(block: &BlockData) -> Vec<TransitionWitness> {
        let Some(ref transition) = block.spacetime_transition else {
            return Vec::new();
        };
        let Some(ref votes) = block.consensus_votes else {
            return Vec::new();
        };
        votes
            .iter()
            .filter_map(|v| TransitionWitness::from_vote(v, transition, |b| *keccak256(b)))
            .collect()
    }

    /// Coordinator fingerprint store + SwtchVM state Verkle sync after finalize.
    ///
    /// Idempotent: duplicate finalize / retry does not double-apply EWMA (coordinator dedup).
    pub async fn apply_block_spacetime_side_effects(
        coordinator: &crate::ConsensusCoordinator,
        node: &SwtchvmNode,
        block: &BlockData,
    ) {
        const DEFAULT_DECAY: f64 = 0.95;
        if block.consensus_votes.is_none() {
            return;
        }
        let touched = coordinator
            .apply_fingerprints_from_block(block, DEFAULT_DECAY)
            .await;
        if !touched.is_empty() {
            coordinator.sync_fingerprints_to_swtchvm(node).await;
        }
        coordinator.record_soft_finalize(block).await;
    }

    /// Roll back fingerprint EWMA state to a prior height snapshot (reorg / fraud-proof window).
    pub async fn rollback_block_spacetime_side_effects(
        coordinator: &crate::ConsensusCoordinator,
        revert_to_height: u64,
    ) -> bool {
        coordinator
            .rollback_fingerprints_to_height(revert_to_height)
            .await
    }

    /// Process a fraud-proof submission: tiered finality rollback + fingerprint restore.
    pub async fn handle_fraud_proof_submission(
        coordinator: &crate::ConsensusCoordinator,
        submission: spacekit_spacetime_consensus::FraudProofSubmission,
    ) -> Result<
        spacekit_spacetime_consensus::FraudProofAcceptance,
        spacekit_spacetime_consensus::FraudProofError,
    > {
        coordinator.submit_fraud_proof(submission).await
    }

    /// Load consensus-tuning Growformer from storage when features are enabled.
    #[cfg(all(feature = "growformer-inference", feature = "storage-integration"))]
    pub async fn bootstrap_consensus_growformer_agent(
        node: std::sync::Arc<SwtchvmNode>,
        storage: std::sync::Arc<spacekit_storage_node::StorageNode>,
        coordinator: std::sync::Arc<crate::ConsensusCoordinator>,
        wallet_did: &str,
    ) -> Option<std::sync::Arc<crate::consensus_growformer_agent::ConsensusGrowformerAgent>> {
        use tracing::warn;

        match crate::consensus_growformer_agent::ConsensusGrowformerAgent::bootstrap(
            node, storage, wallet_did, None,
        )
        .await
        {
            Ok(agent) => {
                let agent = std::sync::Arc::new(agent);
                let hook_agent = agent.clone();
                let hook_cc = coordinator;
                tokio::spawn(async move {
                    let mut tick = tokio::time::interval(std::time::Duration::from_secs(
                        hook_agent.manifest.inference_interval_secs.max(30),
                    ));
                    loop {
                        tick.tick().await;
                        let threshold = hook_cc.divergence_threshold().await;
                        let prompt = format!(
                            "{{\"domain\":\"consensus_tuning\",\"task_id\":\"periodic\",\"divergence_threshold\":{threshold}}}"
                        );
                        if let Ok(inf) = hook_agent.infer_consensus_tuning(&prompt).await {
                            let height = hook_cc.consensus_tuning_height().await;
                            let _ = hook_cc.maybe_propose_from_inference(inf, height).await;
                        }
                    }
                });
                Some(agent)
            }
            Err(e) => {
                warn!(
                    "consensus Growformer agent not started (continuing with static thresholds): {}",
                    e
                );
                None
            }
        }
    }
}

#[cfg(feature = "spacetime-consensus")]
pub use inner::{
    apply_block_spacetime_side_effects, block_envelope_from_data, block_proposal_hash,
    handle_fraud_proof_submission, rollback_block_spacetime_side_effects,
    spacetime_transition_digest, state_root_hex_to_b256, transition_witnesses_from_block,
    validate_block_pq_envelope, validate_block_spacetime_sidecar,
};

#[cfg(all(
    feature = "spacetime-consensus",
    feature = "growformer-inference",
    feature = "storage-integration"
))]
pub use inner::bootstrap_consensus_growformer_agent;

#[cfg(all(test, feature = "spacetime-consensus"))]
mod tests {
    use super::{state_root_hex_to_b256, validate_block_spacetime_sidecar};
    use alloy_primitives::B256;
    use spacekit_spacetime_consensus::causal::CausalCoord;
    use spacekit_spacetime_consensus::{Rotor, SpacetimeTransition};

    fn example_state_root_hex() -> String {
        "0x0101010101010101010101010101010101010101010101010101010101010101".to_string()
    }

    #[test]
    fn state_root_hex_to_b256_accepts_0x_prefix() {
        let b = state_root_hex_to_b256(&example_state_root_hex()).expect("parse");
        assert_eq!(b, B256::from([1u8; 32]));
    }

    #[test]
    fn validate_sidecar_accepts_identity_rotor() {
        let new_hash = B256::from([1u8; 32]);
        let (residual_commitment, residual_norm) =
            SpacetimeTransition::zero_residual_fields(|b| *keccak256(b));
        let t = SpacetimeTransition {
            transition_id: 0,
            rotor: Rotor::IDENTITY,
            prev_state_hash: B256::ZERO,
            new_state_hash: new_hash,
            causal_coord: CausalCoord {
                t: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment,
            residual_norm,
            aux_commit: None,
        };
        assert!(validate_block_spacetime_sidecar(
            &t,
            &example_state_root_hex()
        ));
    }

    #[test]
    fn validate_sidecar_rejects_state_root_mismatch() {
        let (residual_commitment, residual_norm) =
            SpacetimeTransition::zero_residual_fields(|b| *keccak256(b));
        let t = SpacetimeTransition {
            transition_id: 0,
            rotor: Rotor::IDENTITY,
            prev_state_hash: B256::ZERO,
            new_state_hash: B256::from([2u8; 32]),
            causal_coord: CausalCoord {
                t: 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment,
            residual_norm,
            aux_commit: None,
        };
        assert!(!validate_block_spacetime_sidecar(
            &t,
            &example_state_root_hex()
        ));
    }
}
