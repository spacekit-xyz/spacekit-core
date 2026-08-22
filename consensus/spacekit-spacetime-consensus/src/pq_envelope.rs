//! Post-quantum signature policy for spacetime-aware consensus.
//!
//! - **Inner loop (high frequency):** [`ConsensusVoteInner`] signed with **Dilithium2**.
//! - **Outer block envelope (once per block):** [`BlockEnvelope`] signed with **SPHINCS+**
//!   (SHAKE-256-128s-simple, same parameter set as `spacekit-did`).
//!
//! Validators register a Dilithium2 consensus key (separate from long-lived DID SPHINCS+ keys).
//! Light clients and cross-chain anchors trust the **envelope** + Merkle inclusion of vote leaves.

use alloc::string::String;
use alloc::vec::Vec;

use alloy_primitives::{keccak256, B256};

/// Wire format for PQ envelope types (bump on breaking layout changes).
pub const PQ_ENVELOPE_WIRE_VERSION: u16 = 2;

/// Dilithium2 public key length (ML-DSA-44 class).
pub const DILITHIUM2_PUBLIC_KEY_BYTES: usize = 1312;
/// Dilithium2 detached signature length.
pub const DILITHIUM2_SIGNATURE_BYTES: usize = 2420;

/// Dilithium vote message domain.
pub const DOMAIN_CONSENSUS_VOTE: &[u8] = b"spacekit-consensus-vote-v1";
/// SPHINCS+ outer envelope preimage domain.
pub const DOMAIN_BLOCK_ENVELOPE: &[u8] = b"spacekit-block-envelope-v1";
/// Vote Merkle leaf domain (ephemeral per-block vote set).
pub const DOMAIN_VOTE_MERKLE_LEAF: &[u8] = b"spacekit-vote-merkle-leaf-v1";
/// Tagged commitment for [`BlockEnvelope::votes_merkle_root`].
pub const DOMAIN_VOTES_MERKLE: &[u8] = b"spacekit-votes-merkle-v1";
/// Tagged commitment for [`BlockEnvelope::tx_root`] (quantum Verkle when enabled).
pub const DOMAIN_TX_VERKLE: &[u8] = b"spacekit-tx-verkle-v1";
/// Tagged commitment for [`BlockEnvelope::state_root`] (state Verkle root).
pub const DOMAIN_STATE_VERKLE: &[u8] = b"spacekit-state-verkle-v1";
/// Tagged commitment for spacetime transition digest (see `proposal.rs::digest`).
pub const DOMAIN_SPACETIME_TRANSITION: &[u8] = b"spacekit-spacetime-transition-v2";

/// Domain-separated 32-byte commitment so roots cannot be reinterpreted across trees.
#[inline]
pub fn tagged_commitment(domain: &[u8], value: &B256) -> B256 {
    let mut buf = Vec::with_capacity(domain.len() + 32);
    buf.extend_from_slice(domain);
    buf.extend_from_slice(value.as_slice());
    keccak256(buf)
}

/// Digest of a Dilithium detached signature for Merkle leaf binding (not the full sig).
#[inline]
pub fn dilithium_sig_digest(signature: &[u8]) -> B256 {
    keccak256(signature)
}

/// Vote semantics for the inner PBFT-style loop.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConsensusVoteType {
    No = 0,
    Yes = 1,
    Abstain = 2,
}

impl ConsensusVoteType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::No),
            1 => Some(Self::Yes),
            2 => Some(Self::Abstain),
            _ => None,
        }
    }
}

/// Inner consensus message — **Dilithium2 only** (no SPHINCS+ on this path).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ConsensusVoteInner {
    pub wire_version: u16,
    pub round: u64,
    pub view: u64,
    /// Hash of the proposal payload (includes spacetime transition digest when present).
    pub proposal_hash: B256,
    pub vote_type: ConsensusVoteType,
    /// 32-byte validator identity (DID digest or `SwtchvmAddress` bytes).
    pub validator_id: B256,
    /// Rotor transition digest this validator independently computed (folded into vote Merkle leaf).
    pub validator_rotor_digest: B256,
    pub dilithium_public_key: Vec<u8>,
    pub dilithium_signature: Vec<u8>,
}

impl ConsensusVoteInner {
    /// Canonical bytes covered by [`Self::dilithium_signature`] (domain-separated).
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(DOMAIN_CONSENSUS_VOTE.len() + 2 + 8 + 8 + 32 + 1 + 32);
        out.extend_from_slice(DOMAIN_CONSENSUS_VOTE);
        out.extend_from_slice(&self.wire_version.to_le_bytes());
        out.extend_from_slice(&self.round.to_le_bytes());
        out.extend_from_slice(&self.view.to_le_bytes());
        out.extend_from_slice(self.proposal_hash.as_slice());
        out.push(self.vote_type as u8);
        out.extend_from_slice(self.validator_id.as_slice());
        out.extend_from_slice(self.validator_rotor_digest.as_slice());
        out
    }

    /// Canonical vote Merkle leaf preimage (hashed under [`DOMAIN_VOTE_MERKLE_LEAF`]).
    pub fn merkle_leaf_preimage(&self) -> Vec<u8> {
        let sig_d = dilithium_sig_digest(&self.dilithium_signature);
        let mut out = Vec::with_capacity(8 + 8 + 32 + 1 + 32 + 32);
        out.extend_from_slice(&self.round.to_le_bytes());
        out.extend_from_slice(&self.view.to_le_bytes());
        out.extend_from_slice(self.validator_id.as_slice());
        out.push(self.vote_type as u8);
        out.extend_from_slice(self.validator_rotor_digest.as_slice());
        out.extend_from_slice(sig_d.as_slice());
        out
    }

    /// Merkle leaf digest for inclusion under [`BlockEnvelope::votes_merkle_root`].
    pub fn merkle_leaf_digest(&self) -> B256 {
        let mut buf = Vec::with_capacity(DOMAIN_VOTE_MERKLE_LEAF.len() + 128);
        buf.extend_from_slice(DOMAIN_VOTE_MERKLE_LEAF);
        buf.extend_from_slice(&self.merkle_leaf_preimage());
        keccak256(buf)
    }

    #[cfg(feature = "pq-signatures")]
    pub fn verify_dilithium(&self) -> bool {
        pq_crypto::dilithium2_verify(
            &self.dilithium_public_key,
            &self.signing_bytes(),
            &self.dilithium_signature,
        )
    }
}

/// Finalized block header material — **one SPHINCS+ signature** per block on [`Self::signing_bytes`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlockEnvelope {
    pub wire_version: u16,
    pub round: u64,
    pub view: u64,
    pub chain_id: String,
    pub height: u64,
    pub parent_hash: B256,
    /// Raw SwtchVM / Verkle state root (32 bytes). Tagged only at signing.
    pub state_root: B256,
    /// Raw tx-batch digest (32 bytes). Tagged only at signing — never pre-tagged in storage.
    pub tx_root: B256,
    pub l1_manifest_hash: B256,
    /// [`crate::proposal::SpacetimeTransition::digest`] output (not raw transition bytes).
    pub spacetime_tip_hash: B256,
    pub votes_merkle_root: B256,
    pub block_body_hash: B256,
    pub timestamp: u64,
}

impl BlockEnvelope {
    /// Locked byte order for the SPHINCS+ outer envelope (light-client canonical digest).
    ///
    /// **Domain-tag rule:** every root field on this struct is stored **raw** (32 bytes).
    /// `sphincs_signing_bytes` applies each `DOMAIN_*` tag exactly once. Never store
    /// pre-tagged roots in the envelope — that would double-tag at signing time.
    ///
    /// `SPHINCS+( DOMAIN_BLOCK_ENVELOPE || wire_version || round || view || parent_hash
    ///           || tagged(votes_merkle) || tagged(tx_verkle) || tagged(state_verkle)
    ///           || tagged(spacetime_transition) || timestamp || chain_id || height )`
    pub fn sphincs_signing_bytes(&self) -> Vec<u8> {
        let chain = self.chain_id.as_bytes();
        let tagged_votes = tagged_commitment(DOMAIN_VOTES_MERKLE, &self.votes_merkle_root);
        let tagged_tx = tagged_commitment(DOMAIN_TX_VERKLE, &self.tx_root);
        let tagged_state = tagged_commitment(DOMAIN_STATE_VERKLE, &self.state_root);
        let tagged_st = tagged_commitment(DOMAIN_SPACETIME_TRANSITION, &self.spacetime_tip_hash);
        let mut out = Vec::with_capacity(
            DOMAIN_BLOCK_ENVELOPE.len() + 2 + 8 + 8 + 32 + 32 * 4 + 8 + 4 + chain.len() + 8,
        );
        out.extend_from_slice(DOMAIN_BLOCK_ENVELOPE);
        out.extend_from_slice(&self.wire_version.to_le_bytes());
        out.extend_from_slice(&self.round.to_le_bytes());
        out.extend_from_slice(&self.view.to_le_bytes());
        out.extend_from_slice(self.parent_hash.as_slice());
        out.extend_from_slice(tagged_votes.as_slice());
        out.extend_from_slice(tagged_tx.as_slice());
        out.extend_from_slice(tagged_state.as_slice());
        out.extend_from_slice(tagged_st.as_slice());
        out.extend_from_slice(&self.timestamp.to_le_bytes());
        out.extend_from_slice(&(chain.len() as u32).to_le_bytes());
        out.extend_from_slice(chain);
        out.extend_from_slice(&self.height.to_le_bytes());
        out
    }

    #[cfg(feature = "pq-signatures")]
    pub fn verify_sphincs(&self, public_key: &[u8], signature: &[u8]) -> bool {
        pq_crypto::sphincs_verify(public_key, &self.sphincs_signing_bytes(), signature)
    }
}

/// Outer envelope plus SPHINCS+ attestation (proposer / finisher identity key).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SignedBlockEnvelope {
    pub envelope: BlockEnvelope,
    pub sphincs_public_key: Vec<u8>,
    pub sphincs_signature: Vec<u8>,
}

impl SignedBlockEnvelope {
    #[cfg(feature = "pq-signatures")]
    pub fn verify(&self) -> bool {
        self.envelope
            .verify_sphincs(&self.sphincs_public_key, &self.sphincs_signature)
    }
}

/// Build a binary Merkle root over vote leaf digests (sorted for determinism).
pub fn votes_merkle_root(votes: &[ConsensusVoteInner]) -> B256 {
    let mut leaves: Vec<B256> = votes
        .iter()
        .map(ConsensusVoteInner::merkle_leaf_digest)
        .collect();
    if leaves.is_empty() {
        return B256::ZERO;
    }
    leaves.sort();
    merkle_root_sorted(&leaves)
}

fn merkle_root_sorted(leaves: &[B256]) -> B256 {
    if leaves.len() == 1 {
        return leaves[0];
    }
    let mut level: Vec<B256> = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0;
        while i < level.len() {
            if i + 1 < level.len() {
                let mut pair = [0u8; 64];
                pair[..32].copy_from_slice(level[i].as_slice());
                pair[32..].copy_from_slice(level[i + 1].as_slice());
                next.push(keccak256(pair));
                i += 2;
            } else {
                let mut pair = [0u8; 64];
                pair[..32].copy_from_slice(level[i].as_slice());
                pair[32..].copy_from_slice(level[i].as_slice());
                next.push(keccak256(pair));
                i += 1;
            }
        }
        level = next;
    }
    level[0]
}

/// Verify all inner votes (Dilithium) and that their Merkle root matches the envelope.
#[cfg(feature = "pq-signatures")]
pub fn verify_quorum_against_envelope(
    envelope: &BlockEnvelope,
    votes: &[ConsensusVoteInner],
) -> Result<(), PqEnvelopeError> {
    if envelope.wire_version != PQ_ENVELOPE_WIRE_VERSION {
        return Err(PqEnvelopeError::WireVersionMismatch);
    }
    for (i, v) in votes.iter().enumerate() {
        if v.wire_version != PQ_ENVELOPE_WIRE_VERSION {
            return Err(PqEnvelopeError::VoteWireVersion { index: i });
        }
        if !v.verify_dilithium() {
            return Err(PqEnvelopeError::InvalidDilithiumVote { index: i });
        }
    }
    let root = votes_merkle_root(votes);
    if root != envelope.votes_merkle_root {
        return Err(PqEnvelopeError::VotesMerkleMismatch);
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PqEnvelopeError {
    WireVersionMismatch,
    VoteWireVersion { index: usize },
    InvalidDilithiumVote { index: usize },
    VotesMerkleMismatch,
    InvalidSphincsEnvelope,
}

#[cfg(feature = "pq-signatures")]
pub mod pq_crypto {
    use super::{DILITHIUM2_PUBLIC_KEY_BYTES, DILITHIUM2_SIGNATURE_BYTES, DOMAIN_BLOCK_ENVELOPE};
    use pqcrypto_dilithium::dilithium2::{
        detached_sign, verify_detached_signature, DetachedSignature, PublicKey, SecretKey,
    };
    use pqcrypto_sphincsplus::sphincsshake256ssimple;
    use pqcrypto_traits::sign::{DetachedSignature as _, PublicKey as _, SecretKey as _};

    pub fn dilithium2_keypair() -> (Vec<u8>, Vec<u8>) {
        let (pk, sk) = pqcrypto_dilithium::dilithium2::keypair();
        (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
    }

    pub fn dilithium2_sign(secret_key: &[u8], message: &[u8]) -> Vec<u8> {
        let sk = SecretKey::from_bytes(secret_key).expect("invalid Dilithium2 secret key");
        detached_sign(message, &sk).as_bytes().to_vec()
    }

    pub fn dilithium2_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        if public_key.len() != DILITHIUM2_PUBLIC_KEY_BYTES
            || signature.len() != DILITHIUM2_SIGNATURE_BYTES
        {
            return false;
        }
        let pk = match PublicKey::from_bytes(public_key) {
            Ok(k) => k,
            Err(_) => return false,
        };
        let sig = match DetachedSignature::from_bytes(signature) {
            Ok(s) => s,
            Err(_) => return false,
        };
        verify_detached_signature(&sig, message, &pk).is_ok()
    }

    pub fn sphincs_keypair() -> (Vec<u8>, Vec<u8>) {
        let (pk, sk) = sphincsshake256ssimple::keypair();
        (pk.as_bytes().to_vec(), sk.as_bytes().to_vec())
    }

    pub fn sphincs_sign(secret_key: &[u8], message: &[u8]) -> Vec<u8> {
        let sk = pqcrypto_sphincsplus::sphincsshake256ssimple::SecretKey::from_bytes(secret_key)
            .expect("invalid SPHINCS+ secret key");
        sphincsshake256ssimple::detached_sign(message, &sk)
            .as_bytes()
            .to_vec()
    }

    pub fn sphincs_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
        use pqcrypto_sphincsplus::sphincsshake256ssimple::{DetachedSignature, PublicKey};
        let pk = match PublicKey::from_bytes(public_key) {
            Ok(k) => k,
            Err(_) => return false,
        };
        let sig = match DetachedSignature::from_bytes(signature) {
            Ok(s) => s,
            Err(_) => return false,
        };
        sphincsshake256ssimple::verify_detached_signature(&sig, message, &pk).is_ok()
    }

    /// Sign a [`super::ConsensusVoteInner`] in place (fills pk + sig).
    pub fn sign_consensus_vote(
        vote: &mut super::ConsensusVoteInner,
        dilithium_pk: &[u8],
        dilithium_sk: &[u8],
    ) {
        vote.dilithium_public_key = dilithium_pk.to_vec();
        vote.dilithium_signature = dilithium2_sign(dilithium_sk, &vote.signing_bytes());
    }

    /// Build and SPHINCS+-sign a [`super::SignedBlockEnvelope`].
    pub fn sign_block_envelope(
        envelope: super::BlockEnvelope,
        sphincs_pk: &[u8],
        sphincs_sk: &[u8],
    ) -> super::SignedBlockEnvelope {
        let msg = envelope.sphincs_signing_bytes();
        super::SignedBlockEnvelope {
            envelope,
            sphincs_public_key: sphincs_pk.to_vec(),
            sphincs_signature: sphincs_sign(sphincs_sk, &msg),
        }
    }

    #[allow(dead_code)]
    pub fn domain_block_envelope() -> &'static [u8] {
        DOMAIN_BLOCK_ENVELOPE
    }
}

#[cfg(all(test, feature = "pq-signatures"))]
mod tests {
    use super::*;
    use crate::pq_envelope::pq_crypto;

    #[test]
    fn dilithium_vote_round_trip() {
        let (pk, sk) = pq_crypto::dilithium2_keypair();
        let mut vote = ConsensusVoteInner {
            wire_version: PQ_ENVELOPE_WIRE_VERSION,
            round: 1,
            view: 0,
            proposal_hash: B256::from([7u8; 32]),
            vote_type: ConsensusVoteType::Yes,
            validator_id: B256::from([9u8; 32]),
            validator_rotor_digest: B256::ZERO,
            dilithium_public_key: Vec::new(),
            dilithium_signature: Vec::new(),
        };
        pq_crypto::sign_consensus_vote(&mut vote, &pk, &sk);
        assert!(vote.verify_dilithium());
    }

    #[test]
    fn envelope_binds_vote_merkle_root() {
        let (d_pk, d_sk) = pq_crypto::dilithium2_keypair();
        let (s_pk, s_sk) = pq_crypto::sphincs_keypair();

        let mut vote = ConsensusVoteInner {
            wire_version: PQ_ENVELOPE_WIRE_VERSION,
            round: 2,
            view: 1,
            proposal_hash: B256::from([1u8; 32]),
            vote_type: ConsensusVoteType::Yes,
            validator_id: B256::from([2u8; 32]),
            validator_rotor_digest: B256::from([10u8; 32]),
            dilithium_public_key: Vec::new(),
            dilithium_signature: Vec::new(),
        };
        pq_crypto::sign_consensus_vote(&mut vote, &d_pk, &d_sk);
        let votes = vec![vote];
        let root = votes_merkle_root(&votes);

        let envelope = BlockEnvelope {
            wire_version: PQ_ENVELOPE_WIRE_VERSION,
            round: 2,
            view: 1,
            chain_id: "test-chain".into(),
            height: 10,
            parent_hash: B256::ZERO,
            state_root: B256::from([3u8; 32]),
            tx_root: B256::from([4u8; 32]),
            l1_manifest_hash: B256::from([5u8; 32]),
            spacetime_tip_hash: B256::from([6u8; 32]),
            votes_merkle_root: root,
            block_body_hash: B256::from([8u8; 32]),
            timestamp: 1_700_000_000,
        };

        let signed = pq_crypto::sign_block_envelope(envelope, &s_pk, &s_sk);
        assert!(signed.verify());
        verify_quorum_against_envelope(&signed.envelope, &votes).unwrap();

        let mut bad = signed.envelope.clone();
        bad.height = 11;
        assert!(!bad.verify_sphincs(&signed.sphincs_public_key, &signed.sphincs_signature));
    }

    #[test]
    fn envelope_applies_domain_tag_once_for_tx_root() {
        let raw_tx = B256::from([4u8; 32]);
        let tagged_once = tagged_commitment(DOMAIN_TX_VERKLE, &raw_tx);
        let tagged_twice = tagged_commitment(DOMAIN_TX_VERKLE, &tagged_once);
        let envelope = BlockEnvelope {
            wire_version: PQ_ENVELOPE_WIRE_VERSION,
            round: 0,
            view: 0,
            chain_id: "t".into(),
            height: 0,
            parent_hash: B256::ZERO,
            state_root: B256::ZERO,
            tx_root: raw_tx,
            l1_manifest_hash: B256::ZERO,
            spacetime_tip_hash: B256::ZERO,
            votes_merkle_root: B256::ZERO,
            block_body_hash: B256::ZERO,
            timestamp: 0,
        };
        let signing = envelope.sphincs_signing_bytes();
        let tagged_tx_offset = DOMAIN_BLOCK_ENVELOPE.len() + 2 + 8 + 8 + 32 + 32; // after tagged votes_merkle
        let in_signing = &signing[tagged_tx_offset..tagged_tx_offset + 32];
        assert_eq!(in_signing, tagged_once.as_slice());
        assert_ne!(in_signing, tagged_twice.as_slice());
    }
}
