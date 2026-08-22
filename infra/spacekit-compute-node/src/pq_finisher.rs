//! Finalize blocks with Dilithium inner votes + SPHINCS+ outer envelope.
//!
//! See [`SIGNATURE_POLICY.md`](spacetime_consensus/SIGNATURE_POLICY.md).

#[cfg(feature = "spacetime-consensus")]
mod inner {
    use alloy_primitives::{keccak256, B256};
    use anyhow::{anyhow, Result};
    use spacekit_spacetime_consensus::pq_envelope::pq_crypto;
    use spacekit_spacetime_consensus::{
        ConsensusVoteInner, ConsensusVoteType, SignedBlockEnvelope,
    };

    use crate::spacetime_integration::{
        block_envelope_from_data, block_proposal_hash, spacetime_transition_digest,
    };
    use crate::swtch_consensus::BlockData;

    /// Dilithium-signed vote quorum (inner loop). Required to finalize.
    pub struct PqFinisherQuorum {
        pub votes: Vec<ConsensusVoteInner>,
    }

    impl PqFinisherQuorum {
        pub fn new(votes: Vec<ConsensusVoteInner>) -> Result<Self> {
            if votes.is_empty() {
                return Err(anyhow!(
                    "PqFinisherQuorum requires at least one Dilithium vote"
                ));
            }
            Ok(Self { votes })
        }
    }

    /// SPHINCS+ identity key for the outer block envelope (once per block).
    pub struct SphincsEnvelopeKey {
        pub public_key: Vec<u8>,
        pub secret_key: Vec<u8>,
    }

    /// Validator + consensus signing material for this node (finisher / voter).
    #[derive(Clone)]
    pub struct PqFinisherKeys {
        pub dilithium_public_key: Vec<u8>,
        pub dilithium_secret_key: Vec<u8>,
        pub sphincs_public_key: Vec<u8>,
        pub sphincs_secret_key: Vec<u8>,
    }

    impl PqFinisherKeys {
        pub fn generate_ephemeral() -> Self {
            let (dilithium_public_key, dilithium_secret_key) = pq_crypto::dilithium2_keypair();
            let (sphincs_public_key, sphincs_secret_key) = pq_crypto::sphincs_keypair();
            Self {
                dilithium_public_key,
                dilithium_secret_key,
                sphincs_public_key,
                sphincs_secret_key,
            }
        }

        pub fn from_identity_wallet(
            identity: &crate::quantum_security::QuantumResistantDID,
        ) -> Result<Self> {
            let dilithium = Self::generate_ephemeral();
            let kp = identity
                .key_pairs
                .first()
                .ok_or_else(|| anyhow!("identity wallet has no SPHINCS+ key pair"))?;
            Ok(Self {
                dilithium_public_key: dilithium.dilithium_public_key,
                dilithium_secret_key: dilithium.dilithium_secret_key,
                sphincs_public_key: kp.public_key.clone(),
                sphincs_secret_key: kp.private_key.clone(),
            })
        }

        pub fn sphincs_envelope_key(&self) -> SphincsEnvelopeKey {
            SphincsEnvelopeKey {
                public_key: self.sphincs_public_key.clone(),
                secret_key: self.sphincs_secret_key.clone(),
            }
        }
    }

    pub fn validator_id_from_did(did: &str) -> B256 {
        keccak256(did.as_bytes())
    }

    fn l1_manifest_hash(block: &BlockData) -> B256 {
        keccak256(
            serde_json::to_string(&block.l1_manifest)
                .unwrap_or_default()
                .as_bytes(),
        )
    }

    /// Raw tx-batch digest for the envelope struct. Domain tag applied only in
    /// [`BlockEnvelope::sphincs_signing_bytes`] (see SIGNATURE_POLICY.md).
    fn tx_root_raw(block: &BlockData) -> B256 {
        let mut buf = Vec::new();
        for tx in &block.transactions {
            buf.extend_from_slice(tx.as_bytes());
            buf.push(0);
        }
        keccak256(buf)
    }

    fn block_body_hash(block: &BlockData) -> B256 {
        let mut buf = Vec::new();
        buf.extend_from_slice(&block.block_number.to_le_bytes());
        buf.extend_from_slice(block.parent_hash.as_bytes());
        buf.extend_from_slice(block.state_root.as_bytes());
        if let Ok(j) = serde_json::to_vec(&block.transactions) {
            buf.extend_from_slice(&j);
        }
        keccak256(buf)
    }

    fn spacetime_tip_hash(block: &BlockData) -> B256 {
        block
            .spacetime_transition
            .as_ref()
            .map(spacetime_transition_digest)
            .unwrap_or(B256::ZERO)
    }

    /// Build a Dilithium-signed vote; `validator_rotor_digest` binds the validator's rotor witness.
    pub fn sign_pq_vote(
        block: &BlockData,
        voter_did: &str,
        keys: &PqFinisherKeys,
        vote_type: ConsensusVoteType,
        round: u64,
        view: u64,
        validator_rotor_digest: B256,
    ) -> ConsensusVoteInner {
        let proposal_hash = block_proposal_hash(block);
        let validator_id = validator_id_from_did(voter_did);
        let mut vote = ConsensusVoteInner {
            wire_version: spacekit_spacetime_consensus::PQ_ENVELOPE_WIRE_VERSION,
            round,
            view,
            proposal_hash,
            vote_type,
            validator_id,
            validator_rotor_digest,
            dilithium_public_key: Vec::new(),
            dilithium_signature: Vec::new(),
        };
        pq_crypto::sign_consensus_vote(
            &mut vote,
            &keys.dilithium_public_key,
            &keys.dilithium_secret_key,
        );
        vote
    }

    /// Attach PQ fields — **requires** a Dilithium quorum and an SPHINCS+ envelope key (type-level policy).
    pub fn attach_pq_finisher(
        block: &mut BlockData,
        quorum: PqFinisherQuorum,
        sphincs: SphincsEnvelopeKey,
        round: u64,
        view: u64,
        timestamp: u64,
    ) -> Result<SignedBlockEnvelope> {
        let votes = &quorum.votes;
        let messages: Vec<Vec<u8>> = votes.iter().map(|v| v.signing_bytes()).collect();
        let items: Vec<super::gpu_batch::DilithiumVerifyItem<'_>> = votes
            .iter()
            .zip(messages.iter())
            .map(|(v, msg)| super::gpu_batch::DilithiumVerifyItem {
                public_key: &v.dilithium_public_key,
                message: msg.as_slice(),
                signature: &v.dilithium_signature,
            })
            .collect();
        let results = super::gpu_batch::default_verifier().verify_dilithium_batch(&items);
        if results.iter().any(|ok| !ok) {
            return Err(anyhow!("Dilithium vote batch verification failed"));
        }
        let votes_root = spacekit_spacetime_consensus::votes_merkle_root(votes);
        let envelope = block_envelope_from_data(
            block,
            round,
            view,
            votes_root,
            tx_root_raw(block),
            block_body_hash(block),
            l1_manifest_hash(block),
            spacetime_tip_hash(block),
            timestamp,
        )
        .ok_or_else(|| anyhow!("invalid block fields for envelope"))?;

        let signed =
            pq_crypto::sign_block_envelope(envelope, &sphincs.public_key, &sphincs.secret_key);

        block.consensus_votes = Some(votes.clone());
        block.signed_block_envelope = Some(signed.clone());
        Ok(signed)
    }

    pub async fn finalize_proposal_if_ready(
        coordinator: &crate::ConsensusCoordinator,
        keys: &PqFinisherKeys,
        proposal_id: &str,
        round: u64,
        view: u64,
    ) -> Result<Option<BlockData>> {
        match coordinator.check_finality(proposal_id).await {
            crate::consensus_coordinator::FinalityStatus::Finalized { .. } => {}
            _ => return Ok(None),
        }

        let mut block = coordinator
            .take_pending_block(proposal_id)
            .await
            .ok_or_else(|| anyhow!("no pending block for proposal {}", proposal_id))?;

        let rotor_digest = block
            .spacetime_transition
            .as_ref()
            .map(spacetime_transition_digest)
            .unwrap_or(B256::ZERO);

        let mut pq_votes = coordinator.pq_votes_for(proposal_id).await;
        if pq_votes.is_empty() {
            for did in coordinator.approve_dids_for(proposal_id).await {
                let keys_for_voter = coordinator
                    .validator_dilithium_keys(&did)
                    .await
                    .unwrap_or_else(|| keys.clone());
                pq_votes.push(sign_pq_vote(
                    &block,
                    &did,
                    &keys_for_voter,
                    ConsensusVoteType::Yes,
                    round,
                    view,
                    rotor_digest,
                ));
            }
        }

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let quorum = PqFinisherQuorum::new(pq_votes)?;
        attach_pq_finisher(
            &mut block,
            quorum,
            keys.sphincs_envelope_key(),
            round,
            view,
            ts,
        )?;
        Ok(Some(block))
    }

    pub fn verify_finalized_block(block: &BlockData) -> bool {
        crate::spacetime_integration::validate_block_pq_envelope(block)
    }
}

#[cfg(feature = "spacetime-consensus")]
pub use inner::*;

/// Batched PQ verification — CPU and GPU implementations share this interface.
#[cfg(feature = "spacetime-consensus")]
pub mod gpu_batch {
    use spacekit_spacetime_consensus::ConsensusVoteInner;

    pub struct DilithiumVerifyItem<'a> {
        pub public_key: &'a [u8],
        pub message: &'a [u8],
        pub signature: &'a [u8],
    }

    pub struct SphincsVerifyItem<'a> {
        pub public_key: &'a [u8],
        pub message: &'a [u8],
        pub signature: &'a [u8],
    }

    pub trait PqBatchVerifier: Send + Sync {
        fn verify_dilithium_batch(&self, items: &[DilithiumVerifyItem<'_>]) -> Vec<bool>;
        fn verify_sphincs_batch(&self, items: &[SphincsVerifyItem<'_>]) -> Vec<bool>;
        fn optimal_dilithium_batch_size(&self) -> usize;
        fn optimal_sphincs_batch_size(&self) -> usize;
    }

    pub struct CpuBatchVerifier;

    impl PqBatchVerifier for CpuBatchVerifier {
        fn verify_dilithium_batch(&self, items: &[DilithiumVerifyItem<'_>]) -> Vec<bool> {
            items
                .iter()
                .map(|it| {
                    spacekit_spacetime_consensus::pq_envelope::pq_crypto::dilithium2_verify(
                        it.public_key,
                        it.message,
                        it.signature,
                    )
                })
                .collect()
        }

        fn verify_sphincs_batch(&self, items: &[SphincsVerifyItem<'_>]) -> Vec<bool> {
            items
                .iter()
                .map(|it| {
                    spacekit_spacetime_consensus::pq_envelope::pq_crypto::sphincs_verify(
                        it.public_key,
                        it.message,
                        it.signature,
                    )
                })
                .collect()
        }

        fn optimal_dilithium_batch_size(&self) -> usize {
            64
        }

        fn optimal_sphincs_batch_size(&self) -> usize {
            32
        }
    }

    /// GPU path: parallel independent verifies (not intra-signature batching for SPHINCS+).
    pub struct GpuBatchVerifier;

    impl PqBatchVerifier for GpuBatchVerifier {
        fn verify_dilithium_batch(&self, items: &[DilithiumVerifyItem<'_>]) -> Vec<bool> {
            if let Some(v) = dilithium_batch_verify_gpu(items) {
                return v;
            }
            CpuBatchVerifier.verify_dilithium_batch(items)
        }

        fn verify_sphincs_batch(&self, items: &[SphincsVerifyItem<'_>]) -> Vec<bool> {
            if let Some(v) = sphincs_batch_verify_gpu(items) {
                return v;
            }
            CpuBatchVerifier.verify_sphincs_batch(items)
        }

        fn optimal_dilithium_batch_size(&self) -> usize {
            4096
        }

        fn optimal_sphincs_batch_size(&self) -> usize {
            256
        }
    }

    impl CpuBatchVerifier {
        pub fn verify_dilithium_votes(votes: &[ConsensusVoteInner]) -> Vec<bool> {
            votes
                .iter()
                .map(|v| {
                    spacekit_spacetime_consensus::pq_envelope::pq_crypto::dilithium2_verify(
                        &v.dilithium_public_key,
                        &v.signing_bytes(),
                        &v.dilithium_signature,
                    )
                })
                .collect()
        }
    }

    /// Parallel independent verifies (rayon). SPHINCS+ speedup is modest vs Dilithium;
    /// this path runs many detached verifies across CPU cores until CUDA kernels land.
    pub fn dilithium_batch_verify_gpu(items: &[DilithiumVerifyItem<'_>]) -> Option<Vec<bool>> {
        if items.is_empty() {
            return Some(Vec::new());
        }
        #[cfg(feature = "spacetime-consensus")]
        {
            use rayon::prelude::*;
            Some(
                items
                    .par_iter()
                    .map(|it| {
                        spacekit_spacetime_consensus::pq_envelope::pq_crypto::dilithium2_verify(
                            it.public_key,
                            it.message,
                            it.signature,
                        )
                    })
                    .collect(),
            )
        }
        #[cfg(not(feature = "spacetime-consensus"))]
        {
            let _ = items;
            None
        }
    }

    pub fn sphincs_batch_verify_gpu(items: &[SphincsVerifyItem<'_>]) -> Option<Vec<bool>> {
        if items.is_empty() {
            return Some(Vec::new());
        }
        #[cfg(feature = "spacetime-consensus")]
        {
            use rayon::prelude::*;
            Some(
                items
                    .par_iter()
                    .map(|it| {
                        spacekit_spacetime_consensus::pq_envelope::pq_crypto::sphincs_verify(
                            it.public_key,
                            it.message,
                            it.signature,
                        )
                    })
                    .collect(),
            )
        }
        #[cfg(not(feature = "spacetime-consensus"))]
        {
            let _ = items;
            None
        }
    }

    static CPU: CpuBatchVerifier = CpuBatchVerifier;
    static GPU: GpuBatchVerifier = GpuBatchVerifier;

    pub fn default_verifier() -> &'static dyn PqBatchVerifier {
        if std::env::var("SPACEKIT_PQ_GPU_VERIFY").ok().as_deref() == Some("1") {
            &GPU
        } else {
            &CPU
        }
    }
}
