//! Network Consensus Coordinator
//!
//! Bridges the P2P `NetworkService` with the `UnifiedVotingMechanism` to
//! achieve finality across multiple compute nodes.
//!
//! Flow:
//!   1. Proposer produces a block → calls `announce_block` → `BlockAnnounce` broadcast.
//!   2. Peers receive `BlockAnnounce`, validate, → call `cast_vote` → `ConsensusVote` broadcast.
//!   3. All nodes accumulate votes via `submit_vote`.
//!   4. When ≥ 2/3 of registered validators approve, the block is finalised.

use anyhow::Result;
use chrono::Utc;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::network::{NetworkService, P2PMessage};

/// A validator registered in the local view of the network.
#[derive(Debug, Clone)]
pub struct ValidatorEntry {
    pub did: String,
    pub joined_at: chrono::DateTime<Utc>,
    /// SPHINCS+ public key used to verify this validator's votes. `None` means
    /// the entry was created by local bootstrap and cannot have its votes
    /// verified — such entries are never credited with a peer vote.
    pub sphincs_public_key: Option<Vec<u8>>,
    /// Stake backing this validator, in micro-USD.
    pub stake_units: u128,
}

/// Domain separator for validator registration proofs.
const VALIDATOR_REGISTER_DOMAIN: &str = "SPACEKIT-VALIDATOR-REGISTER-v1";

/// Domain separator for consensus votes.
const CONSENSUS_VOTE_DOMAIN: &str = "SPACEKIT-CONSENSUS-VOTE-v1";

/// Minimum stake to register as a validator, in micro-USD.
///
/// Registration is otherwise free, which makes vote counts meaningless: an
/// attacker can register enough DIDs to hold a supermajority for nothing.
pub fn min_validator_stake_units() -> u128 {
    std::env::var("SPACEKIT_MIN_VALIDATOR_STAKE_UNITS")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1_000_000_000) // 1,000 USD
}

/// Exact bytes a validator signs to prove key possession at registration.
pub fn validator_registration_payload(did: &str, sphincs_public_key: &[u8]) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(VALIDATOR_REGISTER_DOMAIN.as_bytes());
    msg.push(b'\n');
    msg.extend_from_slice(did.as_bytes());
    msg.push(b'\n');
    msg.extend_from_slice(sphincs_public_key);
    msg
}

/// Exact bytes a validator signs for a vote.
///
/// `round` is included so a vote cannot be replayed into a later round, and
/// `vote_type` so an approve cannot be re-presented as a reject.
pub fn vote_signing_payload(proposal_id: &str, vote_type: &str, round: u64) -> Vec<u8> {
    format!("{CONSENSUS_VOTE_DOMAIN}\n{proposal_id}\n{vote_type}\n{round}").into_bytes()
}

/// Tracks per-proposal vote state with DID deduplication.
#[derive(Debug, Default, Clone)]
struct ProposalVoteState {
    approve: HashSet<String>,
    reject: HashSet<String>,
    abstain: HashSet<String>,
}

impl ProposalVoteState {
    #[allow(dead_code)]
    fn total_votes(&self) -> usize {
        self.approve.len() + self.reject.len() + self.abstain.len()
    }
}

/// Sync snapshot of one proposal round for [`spacekit_unified_consensus::CoordinatorHandle`].
#[cfg(feature = "spacetime-consensus")]
#[derive(Debug, Clone)]
pub struct CoordinatorRoundSnapshot {
    pub proposal_id: String,
    pub block_hash: alloy_primitives::B256,
    pub eligible: Vec<(alloy_primitives::B256, u128)>,
    /// `keccak256(did)` for each validator that voted approve on this proposal.
    pub supporting: Vec<alloy_primitives::B256>,
    pub approve_count: u64,
    pub eligible_count: u64,
    pub finalized: bool,
}

/// Result of a finality check.
#[derive(Debug, Clone)]
pub enum FinalityStatus {
    Pending {
        approve: usize,
        reject: usize,
        total_validators: usize,
    },
    Finalized {
        block_number: u64,
        approve_count: usize,
    },
    Rejected {
        block_number: u64,
        reject_count: usize,
    },
}

/// Coordinates consensus across network peers.
pub struct ConsensusCoordinator {
    network: NetworkService,
    local_did: String,
    /// Known validators (DID → entry). The coordinator accepts votes only from registered validators.
    validators: Arc<RwLock<HashMap<String, ValidatorEntry>>>,
    /// Per-proposal vote tracking with DID deduplication.
    proposal_votes: Arc<RwLock<HashMap<String, ProposalVoteState>>>,
    /// Maps proposal_id → (block_number, proposer_did) for announced blocks.
    announced_blocks: Arc<RwLock<HashMap<String, (u64, String)>>>,
    /// Proposals that have reached finality (proposal_id → FinalityStatus).
    finalized: Arc<RwLock<HashMap<String, FinalityStatus>>>,
    /// Supermajority threshold (default 2/3).
    threshold: f64,
    /// Minimum stake to register as a validator, in micro-USD. Resolved once
    /// at construction so a mid-flight environment change cannot lower the bar.
    min_stake_units: u128,
    #[cfg(feature = "spacetime-consensus")]
    pending_blocks: Arc<RwLock<HashMap<String, crate::swtch_consensus::BlockData>>>,
    #[cfg(feature = "spacetime-consensus")]
    pq_votes: Arc<RwLock<HashMap<String, Vec<spacekit_spacetime_consensus::ConsensusVoteInner>>>>,
    #[cfg(feature = "spacetime-consensus")]
    validator_dilithium: Arc<RwLock<HashMap<String, (Vec<u8>, Vec<u8>)>>>,
    /// Long-lived validator fingerprints (state Verkle namespace `0xFF…FE`).
    #[cfg(feature = "spacetime-consensus")]
    fingerprint_verkle: Arc<RwLock<spacekit_spacetime_consensus::FingerprintVerkle>>,
    /// Monotonic height guard — skip fingerprint EWMA if `block_number <= last`.
    #[cfg(feature = "spacetime-consensus")]
    fingerprint_last_applied_height: Arc<RwLock<u64>>,
    /// Content-addressed finalize dedup (retry / duplicate finalize).
    #[cfg(feature = "spacetime-consensus")]
    fingerprint_applied_digests: Arc<RwLock<HashSet<alloy_primitives::B256>>>,
    /// Per-height snapshots for reorg rollback (challenge-window depth).
    #[cfg(feature = "spacetime-consensus")]
    fingerprint_snapshots:
        Arc<RwLock<BTreeMap<u64, spacekit_spacetime_consensus::FingerprintStoreSnapshot>>>,
    /// Soft / hard finality state machine (challenge window).
    #[cfg(feature = "spacetime-consensus")]
    tiered_finality: Arc<RwLock<spacekit_spacetime_consensus::TieredFinality>>,
    /// Post-finalize fingerprint root attestations from peers.
    #[cfg(feature = "spacetime-consensus")]
    fingerprint_attestations:
        Arc<RwLock<spacekit_spacetime_consensus::FingerprintAttestationCollector>>,
    /// Fingerprint Verkle root after the last applied block (attestation chain).
    #[cfg(feature = "spacetime-consensus")]
    fingerprint_last_root: Arc<RwLock<alloy_primitives::B256>>,
    /// Growformer parameter ratification policy.
    #[cfg(feature = "spacetime-consensus")]
    ratification_config: spacekit_spacetime_consensus::RatificationConfig,
    #[cfg(feature = "spacetime-consensus")]
    policy_regime: Arc<RwLock<spacekit_spacetime_consensus::PolicyRegime>>,
    #[cfg(feature = "spacetime-consensus")]
    pending_parameter_proposals: Arc<
        RwLock<
            BTreeMap<alloy_primitives::B256, spacekit_spacetime_consensus::ParameterChangeProposal>,
        >,
    >,
    #[cfg(feature = "spacetime-consensus")]
    parameter_ratification_votes: Arc<
        RwLock<
            BTreeMap<
                alloy_primitives::B256,
                Vec<spacekit_spacetime_consensus::ParameterChangeVote>,
            >,
        >,
    >,
    #[cfg(feature = "spacetime-consensus")]
    pending_slashing_proposals: Arc<RwLock<Vec<spacekit_spacetime_consensus::SlashingProposal>>>,
    /// Parameter changes that activated (for malign-ratification slash window).
    #[cfg(feature = "spacetime-consensus")]
    activated_parameters: Arc<RwLock<Vec<spacekit_spacetime_consensus::ActivatedParameterChange>>>,
    /// Live divergence threshold (ratified via Growformer proposals).
    #[cfg(feature = "spacetime-consensus")]
    divergence_threshold: Arc<RwLock<f64>>,
}

#[cfg(feature = "spacetime-consensus")]
const FINGERPRINT_SNAPSHOT_WINDOW: u64 = 256;

impl ConsensusCoordinator {
    /// Override the validator stake floor. Intended for tests and for
    /// operators wiring the value from a config file rather than the
    /// environment.
    pub fn with_min_stake_units(mut self, units: u128) -> Self {
        self.min_stake_units = units;
        self
    }

    pub fn new(network: NetworkService, local_did: String) -> Self {
        Self {
            network,
            local_did,
            validators: Arc::new(RwLock::new(HashMap::new())),
            proposal_votes: Arc::new(RwLock::new(HashMap::new())),
            announced_blocks: Arc::new(RwLock::new(HashMap::new())),
            finalized: Arc::new(RwLock::new(HashMap::new())),
            threshold: 2.0 / 3.0,
            min_stake_units: min_validator_stake_units(),
            #[cfg(feature = "spacetime-consensus")]
            pending_blocks: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "spacetime-consensus")]
            pq_votes: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "spacetime-consensus")]
            validator_dilithium: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "spacetime-consensus")]
            fingerprint_verkle: Arc::new(RwLock::new(
                spacekit_spacetime_consensus::FingerprintVerkle::new(),
            )),
            #[cfg(feature = "spacetime-consensus")]
            fingerprint_last_applied_height: Arc::new(RwLock::new(0)),
            #[cfg(feature = "spacetime-consensus")]
            fingerprint_applied_digests: Arc::new(RwLock::new(HashSet::new())),
            #[cfg(feature = "spacetime-consensus")]
            fingerprint_snapshots: Arc::new(RwLock::new(BTreeMap::new())),
            #[cfg(feature = "spacetime-consensus")]
            tiered_finality: Arc::new(RwLock::new(
                spacekit_spacetime_consensus::TieredFinality::new(
                    spacekit_spacetime_consensus::TieredFinalityConfig::default(),
                    0,
                ),
            )),
            #[cfg(feature = "spacetime-consensus")]
            fingerprint_attestations: Arc::new(RwLock::new(
                spacekit_spacetime_consensus::FingerprintAttestationCollector::new(
                    FINGERPRINT_SNAPSHOT_WINDOW,
                ),
            )),
            #[cfg(feature = "spacetime-consensus")]
            fingerprint_last_root: Arc::new(RwLock::new(alloy_primitives::B256::ZERO)),
            #[cfg(feature = "spacetime-consensus")]
            ratification_config: spacekit_spacetime_consensus::RatificationConfig::default(),
            #[cfg(feature = "spacetime-consensus")]
            policy_regime: Arc::new(RwLock::new(
                spacekit_spacetime_consensus::PolicyRegime::Default,
            )),
            #[cfg(feature = "spacetime-consensus")]
            pending_parameter_proposals: Arc::new(RwLock::new(BTreeMap::new())),
            #[cfg(feature = "spacetime-consensus")]
            parameter_ratification_votes: Arc::new(RwLock::new(BTreeMap::new())),
            #[cfg(feature = "spacetime-consensus")]
            pending_slashing_proposals: Arc::new(RwLock::new(Vec::new())),
            #[cfg(feature = "spacetime-consensus")]
            activated_parameters: Arc::new(RwLock::new(Vec::new())),
            #[cfg(feature = "spacetime-consensus")]
            divergence_threshold: Arc::new(RwLock::new(0.5)),
        }
    }

    /// Register the local node as a validator during bootstrap.
    ///
    /// No key is attached, so this entry alone cannot cast a verifiable peer
    /// vote — [`record_peer_vote`](Self::record_peer_vote) rejects votes for
    /// validators with no registered key. Use
    /// [`register_validator_with_key`](Self::register_validator_with_key) for
    /// anything arriving over the network.
    pub async fn register_validator(&self, did: String) {
        let mut validators = self.validators.write().await;
        validators
            .entry(did.clone())
            .or_insert_with(|| ValidatorEntry {
                did,
                joined_at: Utc::now(),
                sphincs_public_key: None,
                stake_units: 0,
            });
    }

    /// Register a validator that proved possession of its SPHINCS+ key and
    /// meets the stake minimum.
    ///
    /// The DID must be derived from the key (`SHA-256(pk)[..20]`), which stops
    /// a caller from binding someone else's DID to a key they control.
    pub async fn register_validator_with_key(
        &self,
        did: String,
        sphincs_public_key: Vec<u8>,
        stake_units: u128,
        proof_signature: &[u8],
    ) -> Result<()> {
        let minimum = self.min_stake_units;
        if stake_units < minimum {
            anyhow::bail!(
                "stake {stake_units} is below the {minimum} micro-USD minimum for a validator"
            );
        }

        let expected_address = {
            use sha2::Digest;
            let hash = sha2::Sha256::digest(&sphincs_public_key);
            hex::encode(&hash[..20])
        };
        if !did.ends_with(&expected_address) {
            anyhow::bail!("DID {did} is not derived from the supplied public key");
        }

        let payload = validator_registration_payload(&did, &sphincs_public_key);
        if !spacekit_did::sphincs::SphincsPlus::verify(
            &sphincs_public_key,
            &payload,
            proof_signature,
        ) {
            anyhow::bail!("validator registration proof failed verification");
        }

        let mut validators = self.validators.write().await;
        match validators.get(&did) {
            // Refuse to swap the key under an established validator; that would
            // let a replayed registration hijack an existing voting identity.
            Some(existing)
                if existing
                    .sphincs_public_key
                    .as_ref()
                    .is_some_and(|k| k != &sphincs_public_key) =>
            {
                anyhow::bail!("validator {did} is already registered with a different key");
            }
            _ => {}
        }
        validators.insert(
            did.clone(),
            ValidatorEntry {
                did,
                joined_at: Utc::now(),
                sphincs_public_key: Some(sphincs_public_key),
                stake_units,
            },
        );
        Ok(())
    }

    /// Remove a validator.
    pub async fn remove_validator(&self, did: &str) {
        let mut validators = self.validators.write().await;
        validators.remove(did);
    }

    pub async fn validator_count(&self) -> usize {
        self.validators.read().await.len()
    }

    // ── Block announcement ──────────────────────────────────────────────

    /// Announce a locally produced block to all peers.
    pub fn announce_block(
        &self,
        proposal_id: &str,
        block_number: u64,
        block_hash: &str,
        state_root: &str,
        parent_hash: &str,
    ) -> Result<()> {
        let msg = P2PMessage::BlockAnnounce {
            block_number,
            block_hash: block_hash.to_string(),
            proposer_did: self.local_did.clone(),
            state_root: state_root.to_string(),
            parent_hash: parent_hash.to_string(),
            timestamp: Utc::now().timestamp(),
        };
        self.network.broadcast(msg)?;
        info!(
            "Announced block {} (proposal {})",
            block_number, proposal_id
        );

        // Track it locally
        let announced = self.announced_blocks.clone();
        let pid = proposal_id.to_string();
        let proposer = self.local_did.clone();
        tokio::spawn(async move {
            let mut guard = announced.write().await;
            guard.insert(pid, (block_number, proposer));
        });

        Ok(())
    }

    // ── Voting ──────────────────────────────────────────────────────────

    /// Cast a vote on a proposal and broadcast it to the network.
    /// `signature_hex` should be a SPHINCS+ signature over (proposal_id || vote_type || round).
    pub fn cast_vote(
        &self,
        proposal_id: &str,
        vote_type: &str,
        round: u64,
        signature_hex: &str,
    ) -> Result<()> {
        let msg = P2PMessage::ConsensusVote {
            proposal_id: proposal_id.to_string(),
            voter_did: self.local_did.clone(),
            vote_type: vote_type.to_string(),
            signature_hex: signature_hex.to_string(),
            round,
            pq_vote_json: None,
        };
        self.network.broadcast(msg.clone())?;

        // Also record locally
        let votes = self.proposal_votes.clone();
        let did = self.local_did.clone();
        let vt = vote_type.to_string();
        let pid = proposal_id.to_string();
        let validators = self.validators.clone();
        tokio::spawn(async move {
            Self::record_vote_inner(&votes, &validators, &pid, &did, &vt).await;
        });

        Ok(())
    }

    /// Record a vote from a peer, after verifying it was signed by that peer's
    /// registered validator key.
    ///
    /// Returns `true` if the vote was accepted. A vote is rejected when the
    /// voter is not a registered validator, has no registered key, or the
    /// signature does not cover exactly this `(proposal_id, vote_type, round)`.
    pub async fn record_peer_vote(
        &self,
        proposal_id: &str,
        voter_did: &str,
        vote_type: &str,
        round: u64,
        signature_hex: &str,
    ) -> bool {
        Self::verify_and_record_vote(
            &self.proposal_votes,
            &self.validators,
            proposal_id,
            voter_did,
            vote_type,
            round,
            signature_hex,
        )
        .await
    }

    /// Shared verification path for both direct calls and the P2P listener.
    async fn verify_and_record_vote(
        proposal_votes: &Arc<RwLock<HashMap<String, ProposalVoteState>>>,
        validators: &Arc<RwLock<HashMap<String, ValidatorEntry>>>,
        proposal_id: &str,
        voter_did: &str,
        vote_type: &str,
        round: u64,
        signature_hex: &str,
    ) -> bool {
        let public_key = {
            let validators = validators.read().await;
            match validators.get(voter_did) {
                Some(entry) => match &entry.sphincs_public_key {
                    Some(pk) => pk.clone(),
                    None => {
                        warn!(
                            "Rejecting vote from {voter_did}: validator has no registered \
                             signing key, so the vote cannot be attributed"
                        );
                        return false;
                    }
                },
                None => {
                    debug!("Ignoring vote from unregistered validator: {voter_did}");
                    return false;
                }
            }
        };

        let Ok(signature) = hex::decode(signature_hex) else {
            warn!("Rejecting vote from {voter_did}: signature is not valid hex");
            return false;
        };

        let payload = vote_signing_payload(proposal_id, vote_type, round);
        if !spacekit_did::sphincs::SphincsPlus::verify(&public_key, &payload, &signature) {
            warn!(
                "Rejecting vote from {voter_did} on {proposal_id}: signature verification failed"
            );
            return false;
        }

        Self::record_vote_inner(
            proposal_votes,
            validators,
            proposal_id,
            voter_did,
            vote_type,
        )
        .await;
        true
    }

    async fn record_vote_inner(
        proposal_votes: &Arc<RwLock<HashMap<String, ProposalVoteState>>>,
        validators: &Arc<RwLock<HashMap<String, ValidatorEntry>>>,
        proposal_id: &str,
        voter_did: &str,
        vote_type: &str,
    ) {
        // Only accept votes from registered validators
        {
            let v = validators.read().await;
            if !v.contains_key(voter_did) {
                debug!("Ignoring vote from unregistered validator: {}", voter_did);
                return;
            }
        }

        let mut votes = proposal_votes.write().await;
        let state = votes.entry(proposal_id.to_string()).or_default();

        // DID deduplication: a validator can only vote once per proposal
        if state.approve.contains(voter_did)
            || state.reject.contains(voter_did)
            || state.abstain.contains(voter_did)
        {
            debug!("Duplicate vote from {} on {}", voter_did, proposal_id);
            return;
        }

        match vote_type {
            "approve" | "Approve" => {
                state.approve.insert(voter_did.to_string());
            }
            "reject" | "Reject" => {
                state.reject.insert(voter_did.to_string());
            }
            "abstain" | "Abstain" => {
                state.abstain.insert(voter_did.to_string());
            }
            other => {
                warn!("Unknown vote type '{}' from {}", other, voter_did);
            }
        }

        debug!(
            "Recorded {} vote from {} on {} (approve={}, reject={}, abstain={})",
            vote_type,
            voter_did,
            proposal_id,
            state.approve.len(),
            state.reject.len(),
            state.abstain.len(),
        );
    }

    // ── Finality check ──────────────────────────────────────────────────

    /// Check whether a proposal has reached finality (≥ 2/3 DID supermajority).
    pub async fn check_finality(&self, proposal_id: &str) -> FinalityStatus {
        let validators = self.validators.read().await;
        let total = validators.len();
        if total == 0 {
            return FinalityStatus::Pending {
                approve: 0,
                reject: 0,
                total_validators: 0,
            };
        }

        let votes = self.proposal_votes.read().await;
        let state = match votes.get(proposal_id) {
            Some(s) => s.clone(),
            None => {
                return FinalityStatus::Pending {
                    approve: 0,
                    reject: 0,
                    total_validators: total,
                }
            }
        };

        let approve = state.approve.len();
        let reject = state.reject.len();
        let required = ((total as f64) * self.threshold).ceil() as usize;

        // Check for block number from announced blocks
        let block_number = {
            let announced = self.announced_blocks.read().await;
            announced.get(proposal_id).map(|(n, _)| *n).unwrap_or(0)
        };

        if approve >= required {
            let status = FinalityStatus::Finalized {
                block_number,
                approve_count: approve,
            };
            info!(
                "Block {} FINALIZED with {}/{} approvals (threshold {})",
                block_number, approve, total, required
            );
            // Store finality
            let finalized = self.finalized.clone();
            let pid = proposal_id.to_string();
            let s = status.clone();
            tokio::spawn(async move {
                finalized.write().await.insert(pid, s);
            });
            status
        } else if reject >= required {
            let status = FinalityStatus::Rejected {
                block_number,
                reject_count: reject,
            };
            let finalized = self.finalized.clone();
            let pid = proposal_id.to_string();
            let s = status.clone();
            tokio::spawn(async move {
                finalized.write().await.insert(pid, s);
            });
            status
        } else {
            FinalityStatus::Pending {
                approve,
                reject,
                total_validators: total,
            }
        }
    }

    /// Returns true if the proposal has already been finalized.
    pub async fn is_finalized(&self, proposal_id: &str) -> bool {
        let f = self.finalized.read().await;
        matches!(f.get(proposal_id), Some(FinalityStatus::Finalized { .. }))
    }

    /// Stable `B256` key for [`spacekit_unified_consensus::CoordinatorHandle`] /
    /// [`crate::unified_consensus_host::UnifiedConsensusHost`] (derived from `proposal_id`).
    pub fn proposal_block_hash(proposal_id: &str) -> alloy_primitives::B256 {
        alloy_primitives::keccak256(proposal_id.as_bytes())
    }

    /// DID string whose `keccak256(did)` equals `did_hash`, if registered.
    pub async fn validator_did_for_hash(
        &self,
        did_hash: &alloy_primitives::B256,
    ) -> Option<String> {
        use alloy_primitives::keccak256;
        let validators = self.validators.read().await;
        validators
            .keys()
            .find(|did| &keccak256(did.as_bytes()) == did_hash)
            .cloned()
    }

    /// Point-in-time view of one proposal round for the unified-consensus facade.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn capture_round_snapshot(&self, proposal_id: &str) -> CoordinatorRoundSnapshot {
        use alloy_primitives::keccak256;

        let validators = self.validators.read().await;
        let eligible: Vec<(alloy_primitives::B256, u128)> = validators
            .keys()
            .map(|did| (keccak256(did.as_bytes()), 0))
            .collect();
        let eligible_count = eligible.len() as u64;

        let votes = self.proposal_votes.read().await;
        let supporting: Vec<alloy_primitives::B256> = votes
            .get(proposal_id)
            .map(|s| {
                s.approve
                    .iter()
                    .map(|did| keccak256(did.as_bytes()))
                    .collect()
            })
            .unwrap_or_default();
        let approve_count = supporting.len() as u64;

        let finalized = self.is_finalized(proposal_id).await;

        CoordinatorRoundSnapshot {
            proposal_id: proposal_id.to_string(),
            block_hash: Self::proposal_block_hash(proposal_id),
            eligible,
            supporting,
            approve_count,
            eligible_count,
            finalized,
        }
    }

    /// Record an approve/reject vote by validator DID hash (facade → coordinator).
    #[cfg(feature = "spacetime-consensus")]
    pub async fn record_vote_by_did_hash(
        &self,
        proposal_id: &str,
        validator_did_hash: alloy_primitives::B256,
        support: bool,
    ) -> Result<bool, String> {
        let Some(did) = self.validator_did_for_hash(&validator_did_hash).await else {
            return Ok(false);
        };
        let vote_type = if support { "approve" } else { "reject" };
        Self::record_vote_inner(
            &self.proposal_votes,
            &self.validators,
            proposal_id,
            &did,
            vote_type,
        )
        .await;
        Ok(true)
    }

    // ── Background listener ─────────────────────────────────────────────

    /// Spawn a background task that listens for P2P messages and feeds votes
    /// into the coordinator. Returns a join handle.
    ///
    /// When `host` is `Some`, records observational facade telemetry after each
    /// vote (non-gating until post-fork weighted threshold). Prefer
    /// [`crate::unified_consensus_host::UnifiedConsensusHost::start_p2p_listener`].
    #[cfg(not(feature = "spacetime-consensus"))]
    pub fn start_listener(&self) -> tokio::task::JoinHandle<()> {
        self.start_listener_inner(None)
    }

    #[cfg(feature = "spacetime-consensus")]
    pub fn start_listener(
        &self,
        host: Option<Arc<crate::unified_consensus_host::UnifiedConsensusHost>>,
    ) -> tokio::task::JoinHandle<()> {
        self.start_listener_inner(host)
    }

    fn start_listener_inner(
        &self,
        #[cfg(feature = "spacetime-consensus")] host: Option<
            Arc<crate::unified_consensus_host::UnifiedConsensusHost>,
        >,
        #[cfg(not(feature = "spacetime-consensus"))] _host: Option<()>,
    ) -> tokio::task::JoinHandle<()> {
        let mut rx = self.network.subscribe();
        let proposal_votes = self.proposal_votes.clone();
        let validators = self.validators.clone();
        let announced = self.announced_blocks.clone();
        #[cfg(feature = "spacetime-consensus")]
        let pq_votes = self.pq_votes.clone();
        #[cfg(feature = "spacetime-consensus")]
        let host = host;

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(P2PMessage::ConsensusVote {
                        proposal_id,
                        voter_did,
                        vote_type,
                        signature_hex,
                        round,
                        pq_vote_json,
                    }) => {
                        // Anyone can publish on the gossip topic, so a vote is
                        // only credited once its signature verifies against the
                        // voter's registered validator key.
                        if !Self::verify_and_record_vote(
                            &proposal_votes,
                            &validators,
                            &proposal_id,
                            &voter_did,
                            &vote_type,
                            round,
                            &signature_hex,
                        )
                        .await
                        {
                            continue;
                        }
                        #[cfg(feature = "spacetime-consensus")]
                        if let Some(ref h) = host {
                            let pid = proposal_id.clone();
                            let h = h.clone();
                            tokio::spawn(async move {
                                h.observe_vote_round(&pid).await;
                            });
                        }
                        #[cfg(feature = "spacetime-consensus")]
                        if let Some(json) = pq_vote_json {
                            if let Ok(vote) = serde_json::from_str::<
                                spacekit_spacetime_consensus::ConsensusVoteInner,
                            >(&json)
                            {
                                if vote.verify_dilithium() {
                                    let mut pq = pq_votes.write().await;
                                    pq.entry(proposal_id.clone()).or_default().push(vote);
                                }
                            }
                        }
                    }
                    Ok(P2PMessage::BlockAnnounce {
                        block_number,
                        block_hash,
                        proposer_did,
                        ..
                    }) => {
                        let proposal_id = block_hash.clone();
                        let mut guard = announced.write().await;
                        guard.insert(proposal_id.clone(), (block_number, proposer_did.clone()));
                        info!(
                            "Received block announcement #{} from {}",
                            block_number, proposer_did
                        );
                    }
                    Ok(_) => { /* ignore other message types */ }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("Consensus listener lagged {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("Network broadcast channel closed, stopping consensus listener");
                        break;
                    }
                }
            }
        })
    }

    /// Get a summary of all finalized proposals.
    pub async fn finalized_proposals(&self) -> HashMap<String, FinalityStatus> {
        self.finalized.read().await.clone()
    }

    // ── PQ finisher (Dilithium votes + SPHINCS+ envelope) ─────────────────

    #[cfg(feature = "spacetime-consensus")]
    pub async fn register_pending_block(
        &self,
        proposal_id: &str,
        block: crate::swtch_consensus::BlockData,
    ) {
        self.pending_blocks
            .write()
            .await
            .insert(proposal_id.to_string(), block);
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn take_pending_block(
        &self,
        proposal_id: &str,
    ) -> Option<crate::swtch_consensus::BlockData> {
        self.pending_blocks.write().await.remove(proposal_id)
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn push_pq_vote(
        &self,
        proposal_id: &str,
        vote: spacekit_spacetime_consensus::ConsensusVoteInner,
    ) {
        self.pq_votes
            .write()
            .await
            .entry(proposal_id.to_string())
            .or_default()
            .push(vote);
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn pq_votes_for(
        &self,
        proposal_id: &str,
    ) -> Vec<spacekit_spacetime_consensus::ConsensusVoteInner> {
        self.pq_votes
            .read()
            .await
            .get(proposal_id)
            .cloned()
            .unwrap_or_default()
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn approve_dids_for(&self, proposal_id: &str) -> Vec<String> {
        self.proposal_votes
            .read()
            .await
            .get(proposal_id)
            .map(|s| s.approve.iter().cloned().collect())
            .unwrap_or_default()
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn register_validator_dilithium(
        &self,
        did: String,
        public_key: Vec<u8>,
        secret_key: Vec<u8>,
    ) {
        self.validator_dilithium
            .write()
            .await
            .insert(did, (public_key, secret_key));
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn validator_dilithium_keys(
        &self,
        did: &str,
    ) -> Option<crate::pq_finisher::PqFinisherKeys> {
        self.validator_dilithium
            .read()
            .await
            .get(did)
            .map(|(pk, sk)| crate::pq_finisher::PqFinisherKeys {
                dilithium_public_key: pk.clone(),
                dilithium_secret_key: sk.clone(),
                sphincs_public_key: Vec::new(),
                sphincs_secret_key: Vec::new(),
            })
    }

    /// Cast vote and broadcast optional Dilithium [`ConsensusVoteInner`] JSON (feature `spacetime-consensus`).
    #[cfg(feature = "spacetime-consensus")]
    pub fn cast_pq_vote(
        &self,
        proposal_id: &str,
        vote_type: &str,
        round: u64,
        signature_hex: &str,
        pq_vote: &spacekit_spacetime_consensus::ConsensusVoteInner,
    ) -> Result<()> {
        let pq_vote_json = serde_json::to_string(pq_vote)?;
        let msg = P2PMessage::ConsensusVote {
            proposal_id: proposal_id.to_string(),
            voter_did: self.local_did.clone(),
            vote_type: vote_type.to_string(),
            signature_hex: signature_hex.to_string(),
            round,
            pq_vote_json: Some(pq_vote_json),
        };
        self.network.broadcast(msg)?;

        let votes = self.proposal_votes.clone();
        let pq = self.pq_votes.clone();
        let did = self.local_did.clone();
        let vt = vote_type.to_string();
        let pid = proposal_id.to_string();
        let validators = self.validators.clone();
        let pq_vote = pq_vote.clone();
        tokio::spawn(async move {
            Self::record_vote_inner(&votes, &validators, &pid, &did, &vt).await;
            pq.write().await.entry(pid).or_default().push(pq_vote);
        });
        Ok(())
    }

    #[cfg(feature = "spacetime-consensus")]
    fn block_fingerprint_dedup_key(
        block: &crate::swtch_consensus::BlockData,
    ) -> alloy_primitives::B256 {
        use alloy_primitives::keccak256;
        if let Some(ref signed) = block.signed_block_envelope {
            return keccak256(signed.envelope.sphincs_signing_bytes());
        }
        let mut buf = Vec::new();
        buf.extend_from_slice(&block.block_number.to_le_bytes());
        buf.extend_from_slice(block.parent_hash.as_bytes());
        keccak256(buf)
    }

    /// Apply fingerprint EWMA updates from finalized block PQ votes (consensus-only path).
    ///
    /// Idempotent: skips if height ≤ last applied, or if this block's dedup key was seen.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn apply_fingerprints_from_block(
        &self,
        block: &crate::swtch_consensus::BlockData,
        default_decay: f64,
    ) -> Vec<alloy_primitives::B256> {
        use alloy_primitives::{keccak256, B256};
        use spacekit_spacetime_consensus::fingerprint_verkle::store::apply_fingerprint_batch;
        use spacekit_spacetime_consensus::proposal::TransitionWitness;

        let height = block.block_number;
        {
            let last = *self.fingerprint_last_applied_height.read().await;
            if height <= last {
                return Vec::new();
            }
        }
        let dedup_key = Self::block_fingerprint_dedup_key(block);
        if self
            .fingerprint_applied_digests
            .read()
            .await
            .contains(&dedup_key)
        {
            return Vec::new();
        }

        let Some(ref transition) = block.spacetime_transition else {
            return Vec::new();
        };
        let votes = block.consensus_votes.as_deref().unwrap_or(&[]);
        let updates: Vec<(B256, spacekit_spacetime_consensus::Rotor, f64)> = votes
            .iter()
            .filter_map(|v| {
                TransitionWitness::from_vote(v, transition, |b| *keccak256(b)).map(|w| {
                    (
                        v.validator_id,
                        w.transition.rotor,
                        w.transition.residual_norm,
                    )
                })
            })
            .collect();
        if updates.is_empty() {
            return Vec::new();
        }
        let mut store = self.fingerprint_verkle.write().await;
        let touched =
            apply_fingerprint_batch(&mut store, &updates, default_decay, |b| *keccak256(b));
        let snap = store.snapshot();
        drop(store);

        {
            let mut last = self.fingerprint_last_applied_height.write().await;
            if height > *last {
                *last = height;
            }
        }
        self.fingerprint_applied_digests
            .write()
            .await
            .insert(dedup_key);
        let mut snaps = self.fingerprint_snapshots.write().await;
        snaps.insert(height, snap);
        let floor = height.saturating_sub(FINGERPRINT_SNAPSHOT_WINDOW);
        snaps.retain(|h, _| *h >= floor);

        if !touched.is_empty() {
            self.finish_fingerprint_round(block).await;
        }

        touched
    }

    /// Local validator DID hash (32-byte keccak of DID string).
    #[cfg(feature = "spacetime-consensus")]
    pub fn local_did_hash(&self) -> alloy_primitives::B256 {
        use alloy_primitives::keccak256;
        keccak256(self.local_did.as_bytes())
    }

    /// Content hash binding a finalized block for attestations / finality.
    #[cfg(feature = "spacetime-consensus")]
    pub fn block_commitment_hash(
        block: &crate::swtch_consensus::BlockData,
    ) -> alloy_primitives::B256 {
        Self::block_fingerprint_dedup_key(block)
    }

    /// Record soft finality and return heights that just became hard-final.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn record_soft_finalize(
        &self,
        block: &crate::swtch_consensus::BlockData,
    ) -> Vec<u64> {
        let height = block.block_number;
        let hash = Self::block_commitment_hash(block);
        self.tiered_finality
            .write()
            .await
            .on_soft_finalize(height, hash)
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn finality_stage_of(
        &self,
        height: u64,
    ) -> spacekit_spacetime_consensus::FinalityStage {
        self.tiered_finality.read().await.stage_of(height)
    }

    /// After fingerprint EWMA apply: update last root and record local attestation.
    #[cfg(feature = "spacetime-consensus")]
    async fn finish_fingerprint_round(&self, block: &crate::swtch_consensus::BlockData) {
        let root = self.fingerprint_verkle.read().await.root_hash();
        let prev = *self.fingerprint_last_root.read().await;
        *self.fingerprint_last_root.write().await = root;

        let att = spacekit_spacetime_consensus::FingerprintAttestation {
            height: block.block_number,
            block_hash: Self::block_commitment_hash(block),
            attester_did_hash: self.local_did_hash(),
            fingerprint_root: root,
            prev_fingerprint_root: prev,
            signature_digest: alloy_primitives::B256::ZERO,
        };
        let height = block.block_number;
        let mut collector = self.fingerprint_attestations.write().await;
        let _ = collector.ingest(att);
        collector.sweep(height);
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn ingest_fingerprint_attestation(
        &self,
        att: spacekit_spacetime_consensus::FingerprintAttestation,
    ) -> Result<(), spacekit_spacetime_consensus::AttestationError> {
        self.fingerprint_attestations.write().await.ingest(att)
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn detect_fingerprint_mismatches(
        &self,
        height: u64,
    ) -> Vec<spacekit_spacetime_consensus::FingerprintAttestationMismatchEvidence> {
        self.fingerprint_attestations
            .read()
            .await
            .detect_mismatches(height)
    }

    /// Verify proof, roll back fingerprint state tip-first, queue slashing proposals.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn submit_fraud_proof(
        &self,
        submission: spacekit_spacetime_consensus::FraudProofSubmission,
    ) -> Result<
        spacekit_spacetime_consensus::FraudProofAcceptance,
        spacekit_spacetime_consensus::FraudProofError,
    > {
        use alloy_primitives::keccak256;
        use spacekit_spacetime_consensus::submit_fraud_proof;

        let acceptance = {
            let mut fin = self.tiered_finality.write().await;
            submit_fraud_proof(&mut fin, &submission, |b| *keccak256(b))?
        };
        for h in acceptance.rolled_back_heights.iter().rev() {
            self.rollback_fingerprints_to_height(*h).await;
        }
        self.tiered_finality.write().await.drain_reverted();
        self.pending_slashing_proposals
            .write()
            .await
            .extend(acceptance.slashing_proposals.clone());

        let fraud_digest = {
            use alloy_primitives::keccak256;
            let mut buf = Vec::with_capacity(40);
            buf.extend_from_slice(submission.target_block_hash.as_slice());
            buf.extend_from_slice(&submission.target_height.to_le_bytes());
            keccak256(buf)
        };
        let malign = self
            .malign_ratification_slashings(submission.target_height, fraud_digest)
            .await;
        self.pending_slashing_proposals.write().await.extend(malign);

        Ok(acceptance)
    }

    /// Store a Growformer-backed parameter proposal for PBFT ratification.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn propose_parameter_change(
        &self,
        proposal: spacekit_spacetime_consensus::ParameterChangeProposal,
    ) -> Result<(), spacekit_spacetime_consensus::RatificationError> {
        use spacekit_spacetime_consensus::validator_should_ratify;
        if proposal.inference.confidence < self.ratification_config.min_confidence {
            return Err(spacekit_spacetime_consensus::RatificationError::ConfidenceTooLow);
        }
        if proposal.activation_delay < self.ratification_config.min_activation_delay {
            return Err(spacekit_spacetime_consensus::RatificationError::ActivationDelayTooShort);
        }
        let regime = *self.policy_regime.read().await;
        validator_should_ratify(
            &proposal,
            &proposal.inference,
            regime,
            &self.ratification_config,
        )?;
        self.pending_parameter_proposals
            .write()
            .await
            .insert(proposal.proposal_id, proposal);
        Ok(())
    }

    /// Cast a local vote on a pending parameter proposal.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn ingest_parameter_vote(
        &self,
        vote: spacekit_spacetime_consensus::ParameterChangeVote,
    ) -> Result<(), spacekit_spacetime_consensus::RatificationError> {
        if !self
            .pending_parameter_proposals
            .read()
            .await
            .contains_key(&vote.proposal_id)
        {
            return Err(spacekit_spacetime_consensus::RatificationError::ProposalNotFound);
        }
        let mut votes = self.parameter_ratification_votes.write().await;
        let list = votes.entry(vote.proposal_id).or_default();
        if list.iter().any(|v| v.voter_did_hash == vote.voter_did_hash) {
            return Err(spacekit_spacetime_consensus::RatificationError::AlreadyVoted);
        }
        list.push(vote);
        Ok(())
    }

    /// If quorum is met, queue activation at `at_height + proposal.activation_delay`.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn try_finalize_ratification(
        &self,
        proposal_id: alloy_primitives::B256,
        at_height: u64,
    ) -> Option<spacekit_spacetime_consensus::ActivatedParameterChange> {
        use spacekit_spacetime_consensus::evaluate_ratification;

        let proposal = self
            .pending_parameter_proposals
            .read()
            .await
            .get(&proposal_id)
            .cloned()?;
        let votes = self
            .parameter_ratification_votes
            .read()
            .await
            .get(&proposal_id)
            .cloned()
            .unwrap_or_default();
        let voting_powers = self.validator_voting_powers().await;
        let regime = *self.policy_regime.read().await;
        evaluate_ratification(
            &proposal,
            &votes,
            &voting_powers,
            regime,
            &self.ratification_config,
        )
        .ok()?;

        let yes_votes: Vec<_> = votes.into_iter().filter(|v| v.vote).collect();
        let activated_at_height = at_height.saturating_add(proposal.activation_delay);
        let activated = spacekit_spacetime_consensus::ActivatedParameterChange {
            proposal: proposal.clone(),
            activated_at_height,
            yes_votes,
        };
        self.pending_parameter_proposals
            .write()
            .await
            .remove(&proposal_id);
        self.parameter_ratification_votes
            .write()
            .await
            .remove(&proposal_id);
        self.activated_parameters
            .write()
            .await
            .push(activated.clone());
        self.apply_activated_parameter(&activated).await;
        Some(activated)
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn divergence_threshold(&self) -> f64 {
        *self.divergence_threshold.read().await
    }

    /// Chain height used for Growformer parameter proposals (last fingerprint apply).
    #[cfg(feature = "spacetime-consensus")]
    pub async fn consensus_tuning_height(&self) -> u64 {
        *self.fingerprint_last_applied_height.read().await
    }

    /// Build a PBFT proposal from a Growformer inference (local proposer path).
    #[cfg(feature = "spacetime-consensus")]
    pub async fn build_parameter_proposal_from_inference(
        &self,
        inference: &spacekit_spacetime_consensus::GrowformerInference,
        at_height: u64,
    ) -> Option<spacekit_spacetime_consensus::ParameterChangeProposal> {
        use alloy_primitives::keccak256;
        use spacekit_spacetime_consensus::GrowformerIntent;

        if inference.semantic_intent == GrowformerIntent::NoChange {
            return None;
        }
        if inference.confidence < self.ratification_config.min_confidence {
            return None;
        }
        if inference.action_target != "spacetime.divergence_threshold" {
            return None;
        }

        let current = *self.divergence_threshold.read().await;
        let proposed = match inference.semantic_intent {
            GrowformerIntent::Tighten | GrowformerIntent::Alert => current * 0.9,
            GrowformerIntent::Loosen => current * 1.1,
            GrowformerIntent::NoChange => return None,
        };
        let proposed = proposed.clamp(
            self.ratification_config.global_min,
            self.ratification_config.global_max,
        );

        let mut buf = Vec::new();
        buf.extend_from_slice(b"spacekit-parameter-proposal-v1");
        buf.extend_from_slice(inference.task_id.as_bytes());
        buf.extend_from_slice(&at_height.to_le_bytes());
        let proposal_id = keccak256(buf);

        Some(spacekit_spacetime_consensus::ParameterChangeProposal {
            proposal_id,
            proposer_did_hash: self.local_did_hash(),
            proposed_at_height: at_height,
            inference: inference.clone(),
            current_value: current.to_le_bytes(),
            proposed_value: proposed.to_le_bytes(),
            activation_delay: self.ratification_config.min_activation_delay,
        })
    }

    /// Propose a parameter change when Growformer recommends one.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn maybe_propose_from_inference(
        &self,
        inference: spacekit_spacetime_consensus::GrowformerInference,
        at_height: u64,
    ) -> Result<(), spacekit_spacetime_consensus::RatificationError> {
        let Some(proposal) = self
            .build_parameter_proposal_from_inference(&inference, at_height)
            .await
        else {
            return Ok(());
        };
        self.propose_parameter_change(proposal).await
    }

    #[cfg(feature = "spacetime-consensus")]
    async fn apply_activated_parameter(
        &self,
        activated: &spacekit_spacetime_consensus::ActivatedParameterChange,
    ) {
        if activated.proposal.inference.action_target == "spacetime.divergence_threshold" {
            *self.divergence_threshold.write().await = activated.proposal.proposed_f64();
        }
        if activated.proposal.inference.policy_regime
            != spacekit_spacetime_consensus::PolicyRegime::Default
        {
            *self.policy_regime.write().await = activated.proposal.inference.policy_regime;
        }
    }

    #[cfg(feature = "spacetime-consensus")]
    async fn validator_voting_powers(&self) -> Vec<(alloy_primitives::B256, f64)> {
        use alloy_primitives::keccak256;
        let validators = self.validators.read().await;
        validators
            .keys()
            .map(|did| (keccak256(did.as_bytes()), 1.0))
            .collect()
    }

    /// YES voters on recent parameter activations within the safety window.
    #[cfg(feature = "spacetime-consensus")]
    async fn malign_ratification_slashings(
        &self,
        attack_height: u64,
        fraud_proof_digest: alloy_primitives::B256,
    ) -> Vec<spacekit_spacetime_consensus::SlashingProposal> {
        use spacekit_spacetime_consensus::MalignRatificationEvidence;

        let safety_window = self.ratification_config.min_activation_delay;
        let activated = self.activated_parameters.read().await;
        let mut out = Vec::new();
        for act in activated.iter() {
            for vote in &act.yes_votes {
                let ev = MalignRatificationEvidence {
                    proposal_id: act.proposal.proposal_id,
                    bad_voter_did_hash: vote.voter_did_hash,
                    vote: *vote,
                    activated_at_height: act.activated_at_height,
                    attack_height,
                    fraud_proof_digest,
                };
                if ev.verify(safety_window) {
                    out.push(ev.to_slashing_proposal());
                }
            }
        }
        out
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn take_pending_slashing_proposals(
        &self,
    ) -> Vec<spacekit_spacetime_consensus::SlashingProposal> {
        std::mem::take(&mut *self.pending_slashing_proposals.write().await)
    }

    /// Restore fingerprint store to a prior height snapshot (reorg / fraud-proof window).
    #[cfg(feature = "spacetime-consensus")]
    pub async fn rollback_fingerprints_to_height(&self, height: u64) -> bool {
        use alloy_primitives::keccak256;
        let snap = self
            .fingerprint_snapshots
            .read()
            .await
            .get(&height)
            .cloned();
        let Some(snap) = snap else {
            return false;
        };
        let mut store = self.fingerprint_verkle.write().await;
        store.restore(snap, |b| *keccak256(b));
        drop(store);
        *self.fingerprint_last_applied_height.write().await = height;
        self.fingerprint_snapshots
            .write()
            .await
            .retain(|h, _| *h <= height);
        self.fingerprint_applied_digests.write().await.clear();
        true
    }

    /// Mirror coordinator fingerprint payloads into unified SwtchVM state Verkle.
    #[cfg(feature = "spacetime-consensus")]
    pub async fn sync_fingerprints_to_swtchvm(
        &self,
        node: &crate::spacekitvm::swtchvm_node::SwtchvmNode,
    ) {
        let store = self.fingerprint_verkle.read().await;
        let state_arc = node.runtime_state();
        let mut state = state_arc.write().await;
        for (did_hash, commitment) in &store.payloads {
            if let Some(fp) = commitment.to_fingerprint() {
                crate::spacetime_state::set_validator_fingerprint(&mut state, *did_hash, fp);
            }
        }
    }

    #[cfg(feature = "spacetime-consensus")]
    pub async fn fingerprint_commitment(
        &self,
        validator_id: alloy_primitives::B256,
    ) -> Option<spacekit_spacetime_consensus::FingerprintCommitment> {
        self.fingerprint_verkle
            .read()
            .await
            .get(&validator_id)
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::NetworkService;

    use spacekit_did::sphincs::SphincsPlus;
    use std::sync::OnceLock;

    struct TestValidator {
        did: String,
        public_key: Vec<u8>,
        secret_key: Vec<u8>,
    }

    /// SPHINCS+ keygen and signing are deliberately expensive, so the test
    /// validator set is generated once and shared across every test.
    fn test_validators() -> &'static Vec<TestValidator> {
        static VALIDATORS: OnceLock<Vec<TestValidator>> = OnceLock::new();
        VALIDATORS.get_or_init(|| {
            (0..3)
                .map(|_| {
                    let kp = SphincsPlus::generate_keypair();
                    use sha2::Digest;
                    let address = hex::encode(&sha2::Sha256::digest(&kp.public_key)[..20]);
                    TestValidator {
                        did: format!("did:spacekit:testnet:{address}"),
                        public_key: kp.public_key,
                        secret_key: kp.private_key,
                    }
                })
                .collect()
        })
    }

    fn sign_vote(v: &TestValidator, proposal_id: &str, vote_type: &str, round: u64) -> String {
        let payload = vote_signing_payload(proposal_id, vote_type, round);
        hex::encode(SphincsPlus::sign(&v.secret_key, &payload).unwrap())
    }

    async fn make_coordinator(n_validators: usize) -> ConsensusCoordinator {
        let net = NetworkService::new_simple("test", "127.0.0.1", 0)
            .await
            .unwrap();
        // Keep the stake floor below what the tests register.
        let coord = ConsensusCoordinator::new(net, "did:spacekit:testnet:proposer".to_string())
            .with_min_stake_units(1);
        for v in test_validators().iter().take(n_validators) {
            let proof = SphincsPlus::sign(
                &v.secret_key,
                &validator_registration_payload(&v.did, &v.public_key),
            )
            .unwrap();
            coord
                .register_validator_with_key(v.did.clone(), v.public_key.clone(), 10, &proof)
                .await
                .unwrap();
        }
        coord
    }

    #[tokio::test]
    async fn test_finality_with_supermajority() {
        let coord = make_coordinator(3).await;
        let vs = test_validators();
        let pid = "proposal_1";

        // 2/3 of 3 = 2 needed
        assert!(
            coord
                .record_peer_vote(
                    pid,
                    &vs[0].did,
                    "approve",
                    1,
                    &sign_vote(&vs[0], pid, "approve", 1)
                )
                .await
        );
        let status = coord.check_finality(pid).await;
        assert!(matches!(status, FinalityStatus::Pending { approve: 1, .. }));

        assert!(
            coord
                .record_peer_vote(
                    pid,
                    &vs[1].did,
                    "approve",
                    1,
                    &sign_vote(&vs[1], pid, "approve", 1)
                )
                .await
        );
        let status = coord.check_finality(pid).await;
        assert!(matches!(
            status,
            FinalityStatus::Finalized {
                approve_count: 2,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_duplicate_votes_ignored() {
        let coord = make_coordinator(3).await;
        let vs = test_validators();
        let pid = "dup_test";

        let approve = sign_vote(&vs[0], pid, "approve", 1);
        coord
            .record_peer_vote(pid, &vs[0].did, "approve", 1, &approve)
            .await;
        coord
            .record_peer_vote(pid, &vs[0].did, "approve", 1, &approve)
            .await;
        coord
            .record_peer_vote(
                pid,
                &vs[0].did,
                "reject",
                1,
                &sign_vote(&vs[0], pid, "reject", 1),
            )
            .await;

        let status = coord.check_finality(pid).await;
        match status {
            FinalityStatus::Pending {
                approve, reject, ..
            } => {
                assert_eq!(approve, 1);
                assert_eq!(reject, 0);
            }
            _ => panic!("expected pending"),
        }
    }

    #[tokio::test]
    async fn test_unregistered_validator_ignored() {
        let coord = make_coordinator(3).await;
        let pid = "unreg_test";

        assert!(
            !coord
                .record_peer_vote(pid, "did:spacekit:testnet:unknown", "approve", 1, "00")
                .await
        );
        let status = coord.check_finality(pid).await;
        assert!(matches!(status, FinalityStatus::Pending { approve: 0, .. }));
    }

    #[tokio::test]
    async fn forged_vote_signature_is_rejected() {
        let coord = make_coordinator(3).await;
        let vs = test_validators();
        let pid = "forge_test";

        // Signature made by val1 but presented as val0's vote.
        let stolen = sign_vote(&vs[1], pid, "approve", 1);
        assert!(
            !coord
                .record_peer_vote(pid, &vs[0].did, "approve", 1, &stolen)
                .await
        );

        // Garbage signature.
        assert!(
            !coord
                .record_peer_vote(pid, &vs[0].did, "approve", 1, "deadbeef")
                .await
        );

        let status = coord.check_finality(pid).await;
        assert!(matches!(status, FinalityStatus::Pending { approve: 0, .. }));
    }

    #[tokio::test]
    async fn vote_signature_does_not_transfer_across_rounds_or_types() {
        let coord = make_coordinator(3).await;
        let vs = test_validators();
        let pid = "bind_test";

        let round1 = sign_vote(&vs[0], pid, "approve", 1);

        // Same signature replayed into round 2 must not count.
        assert!(
            !coord
                .record_peer_vote(pid, &vs[0].did, "approve", 2, &round1)
                .await
        );
        // Nor re-presented as a reject.
        assert!(
            !coord
                .record_peer_vote(pid, &vs[0].did, "reject", 1, &round1)
                .await
        );
        // The original is still valid.
        assert!(
            coord
                .record_peer_vote(pid, &vs[0].did, "approve", 1, &round1)
                .await
        );
    }

    #[tokio::test]
    async fn validator_registration_requires_stake_and_matching_did() {
        let net = NetworkService::new_simple("test", "127.0.0.1", 0)
            .await
            .unwrap();
        let coord = ConsensusCoordinator::new(net, "did:spacekit:testnet:proposer".to_string())
            .with_min_stake_units(100);
        let v = &test_validators()[0];
        let proof = SphincsPlus::sign(
            &v.secret_key,
            &validator_registration_payload(&v.did, &v.public_key),
        )
        .unwrap();

        // Below the stake floor.
        assert!(coord
            .register_validator_with_key(v.did.clone(), v.public_key.clone(), 99, &proof)
            .await
            .is_err());

        // DID not derived from the key.
        assert!(coord
            .register_validator_with_key(
                "did:spacekit:testnet:notmine".into(),
                v.public_key.clone(),
                1000,
                &proof
            )
            .await
            .is_err());

        // Valid.
        assert!(coord
            .register_validator_with_key(v.did.clone(), v.public_key.clone(), 1000, &proof)
            .await
            .is_ok());
    }

    /// The default floor must be high enough that free registration is not
    /// possible; an unset environment variable must not mean "no stake".
    #[test]
    fn default_stake_floor_is_not_zero() {
        assert!(min_validator_stake_units() > 0);
    }

    #[tokio::test]
    async fn test_rejection_finality() {
        let coord = make_coordinator(3).await;
        let vs = test_validators();
        let pid = "reject_test";

        coord
            .record_peer_vote(
                pid,
                &vs[0].did,
                "reject",
                1,
                &sign_vote(&vs[0], pid, "reject", 1),
            )
            .await;
        coord
            .record_peer_vote(
                pid,
                &vs[1].did,
                "reject",
                1,
                &sign_vote(&vs[1], pid, "reject", 1),
            )
            .await;

        let status = coord.check_finality(pid).await;
        assert!(matches!(
            status,
            FinalityStatus::Rejected {
                reject_count: 2,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_announce_block() {
        let coord = make_coordinator(1).await;
        // broadcast with no peers should not error
        let result = coord.announce_block("p1", 42, "hash", "root", "parent");
        assert!(result.is_ok());
    }
}

#[cfg(all(test, feature = "spacetime-consensus"))]
mod fingerprint_coordinator_tests {
    use super::*;
    use crate::network::NetworkService;
    use crate::swtch_consensus::BlockData;
    use alloy_primitives::{keccak256, B256};
    use spacekit_spacetime_consensus::causal::CausalCoord;
    use spacekit_spacetime_consensus::pq_envelope::{
        pq_crypto, ConsensusVoteInner, ConsensusVoteType, PQ_ENVELOPE_WIRE_VERSION,
    };
    use spacekit_spacetime_consensus::{proposal::SpacetimeTransition, rotor::Bivector, Rotor};
    use std::time::{Duration, UNIX_EPOCH};

    fn state_root_hex(height: u64) -> String {
        let mut bytes = [0u8; 32];
        bytes[..8].copy_from_slice(&height.to_le_bytes());
        format!("0x{}", hex::encode(bytes))
    }

    fn fingerprint_test_block(height: u64, validator_byte: u8) -> BlockData {
        let parent = state_root_hex(height.saturating_sub(1));
        let state_root = state_root_hex(height);
        let (residual_commitment, residual_norm) =
            SpacetimeTransition::zero_residual_fields(|b| *keccak256(b));
        let transition = SpacetimeTransition {
            transition_id: height,
            rotor: Rotor::exp(&Bivector {
                b: [0.0, 0.0, 0.0, 0.05 * (height as f64 + 1.0), 0.0, 0.0],
            }),
            prev_state_hash: B256::from([height.saturating_sub(1) as u8 + 1; 32]),
            new_state_hash: B256::from([height as u8 + 2; 32]),
            causal_coord: CausalCoord {
                t: height as f64 + 1.0,
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            residual_commitment,
            residual_norm,
            aux_commit: None,
        };
        let (d_pk, d_sk) = pq_crypto::dilithium2_keypair();
        let mut vote = ConsensusVoteInner {
            wire_version: PQ_ENVELOPE_WIRE_VERSION,
            round: height,
            view: 0,
            proposal_hash: keccak256(format!("proposal-{height}").as_bytes()),
            vote_type: ConsensusVoteType::Yes,
            validator_id: B256::from([validator_byte; 32]),
            validator_rotor_digest: transition.digest(|b| *keccak256(b)),
            dilithium_public_key: Vec::new(),
            dilithium_signature: Vec::new(),
        };
        pq_crypto::sign_consensus_vote(&mut vote, &d_pk, &d_sk);
        let state_root_clone = state_root.clone();
        BlockData {
            block_number: height,
            parent_hash: parent.clone(),
            transactions: Vec::new(),
            state_root,
            timestamp: UNIX_EPOCH + Duration::from_secs(height),
            l1_manifest: crate::spacekitvm::minimal_l1_manifest_for_proposal(
                "fingerprint-test",
                &state_root_clone,
                height,
                &parent,
            ),
            spacetime_transition: Some(transition),
            consensus_votes: Some(vec![vote]),
            signed_block_envelope: None,
        }
    }

    async fn coordinator() -> ConsensusCoordinator {
        let net = NetworkService::new_simple("fp-test", "127.0.0.1", 0)
            .await
            .unwrap();
        ConsensusCoordinator::new(net, "did:spacekit:testnet:fp".to_string())
    }

    #[tokio::test]
    async fn fingerprint_apply_is_idempotent_per_block() {
        let coord = coordinator().await;
        let block = fingerprint_test_block(1, 0xA1);
        let validator = B256::from([0xA1; 32]);

        let first = coord.apply_fingerprints_from_block(&block, 0.95).await;
        assert_eq!(first, vec![validator]);
        let samples_after_first = coord
            .fingerprint_commitment(validator)
            .await
            .unwrap()
            .samples;

        let second = coord.apply_fingerprints_from_block(&block, 0.95).await;
        assert!(second.is_empty());
        let samples_after_retry = coord
            .fingerprint_commitment(validator)
            .await
            .unwrap()
            .samples;
        assert_eq!(samples_after_retry, samples_after_first);
    }

    #[tokio::test]
    async fn fingerprint_rollback_restores_prior_height() {
        let coord = coordinator().await;
        let validator = B256::from([0xB2; 32]);
        let block1 = fingerprint_test_block(1, 0xB2);
        let block2 = fingerprint_test_block(2, 0xB2);

        coord.apply_fingerprints_from_block(&block1, 0.95).await;
        let after_h1 = coord.fingerprint_commitment(validator).await.unwrap();

        coord.apply_fingerprints_from_block(&block2, 0.95).await;
        let after_h2 = coord.fingerprint_commitment(validator).await.unwrap();
        assert!(after_h2.samples > after_h1.samples);

        assert!(coord.rollback_fingerprints_to_height(1).await);
        let restored = coord.fingerprint_commitment(validator).await.unwrap();
        assert_eq!(restored.samples, after_h1.samples);
        assert_eq!(restored.centroid_coeffs, after_h1.centroid_coeffs);
    }
}
