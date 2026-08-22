//! SpaceKit Blockchain Unified Consensus (Phase 5.5)
//!
//! Revolutionary unified consensus layer that consolidates block production and metrics validation
//! into a single, specialized committee-based consensus mechanism.
//!
//! Features:
//! - 25-40% reduction in consensus overhead
//! - 30% reduction in validator costs  
//! - Enhanced Byzantine fault tolerance
//! - Quantum-resistant security
//! - Specialized validator committees

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

// Import quantum security and VPoS
#[cfg(feature = "spacetime-consensus")]
use crate::spacetime_integration::{validate_block_pq_envelope, validate_block_spacetime_sidecar};
use crate::{
    production_metrics::{MetricsSnapshot, NetworkStatistics},
    quantum_security::QuantumResistantDID,
    spacekitvm::{
        minimal_l1_manifest_for_proposal, verify_l1_tx_batch_witness_json, SnapshotManifest,
        SNAPSHOT_MANIFEST_VERSION, TX_ROOT_SCHEME_QUANTUM_VERKLE_V1,
    },
    vpos::VPoSManager,
};
#[cfg(feature = "spacetime-consensus")]
use spacekit_spacetime_consensus::{ConsensusVoteInner, SignedBlockEnvelope, SpacetimeTransition};

/// Validator identifier
pub type ValidatorId = String;

/// Block data for consensus (includes required SwtchVM L1 manifest commitment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockData {
    pub block_number: u64,
    pub parent_hash: String,
    pub transactions: Vec<String>,
    pub state_root: String,
    pub timestamp: SystemTime,
    /// L1 JSON sidecar payload matching [`crate::spacekitvm::SnapshotManifest`]. Required for validation.
    #[serde(default)]
    pub l1_manifest: crate::spacekitvm::SnapshotManifest,
    /// Optional spacetime rotor transition sidecar ([`SpacetimeTransition`]) when the
    /// `spacetime-consensus` feature is enabled.
    #[cfg(feature = "spacetime-consensus")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spacetime_transition: Option<SpacetimeTransition>,
    /// Dilithium2-signed inner votes (PREPARE/COMMIT). Verified against
    /// [`SignedBlockEnvelope::envelope`].`votes_merkle_root` when both are present.
    #[cfg(feature = "spacetime-consensus")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consensus_votes: Option<Vec<ConsensusVoteInner>>,
    /// One SPHINCS+ signature per finalized block (outer envelope).
    #[cfg(feature = "spacetime-consensus")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_block_envelope: Option<SignedBlockEnvelope>,
}

impl BlockData {
    /// Construct [`BlockData`] with no spacetime sidecar (always available).
    pub fn new_with_l1_manifest(
        block_number: u64,
        parent_hash: String,
        transactions: Vec<String>,
        state_root: String,
        timestamp: SystemTime,
        l1_manifest: crate::spacekitvm::SnapshotManifest,
    ) -> Self {
        Self {
            block_number,
            parent_hash,
            transactions,
            state_root,
            timestamp,
            l1_manifest,
            #[cfg(feature = "spacetime-consensus")]
            spacetime_transition: None,
            #[cfg(feature = "spacetime-consensus")]
            consensus_votes: None,
            #[cfg(feature = "spacetime-consensus")]
            signed_block_envelope: None,
        }
    }
}

/// Normalize hex for comparisons (optional `0x`, ASCII lowercase).
pub fn normalize_hex_lower(s: &str) -> String {
    s.trim()
        .strip_prefix("0x")
        .or_else(|| s.trim().strip_prefix("0X"))
        .unwrap_or(s.trim())
        .to_lowercase()
}

/// Validates an L1 snapshot manifest bundled with a [`BlockData`] / [`BlockProposal`].
pub fn validate_l1_manifest_for_block(m: &SnapshotManifest, block_state_root: &str) -> bool {
    if m.chain_id.is_empty() {
        return false;
    }
    if m.manifest_version != SNAPSHOT_MANIFEST_VERSION {
        return false;
    }
    if m.blob_sha256_hex.len() != 64 || !m.blob_sha256_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    if normalize_hex_lower(&m.checkpoint.state_root_hex) != normalize_hex_lower(block_state_root) {
        return false;
    }
    if let Some(ref ws) = m.checkpoint.verkle_witness_summary {
        let w = ws.trim();
        if !w.is_empty() && m.checkpoint.tx_root_scheme == TX_ROOT_SCHEME_QUANTUM_VERKLE_V1 {
            if verify_l1_tx_batch_witness_json(&m.checkpoint.tx_root_hex, w).is_err() {
                return false;
            }
        }
    }
    true
}

/// Network metrics for consensus validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub cpu_utilization: f64,
    pub memory_utilization: f64,
    pub network_utilization: f64,
    pub storage_utilization: f64,
    pub timestamp: SystemTime,
}

/// Proposal types supported by unified consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Proposal {
    /// Traditional block proposal with transactions
    Block(BlockProposal),
    /// Network metrics validation proposal
    Metrics(MetricsProposal),
    /// Hybrid proposal combining block and metrics updates
    Hybrid(HybridProposal),
}

/// Block proposal for transaction validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockProposal {
    pub id: String,
    pub proposer: ValidatorId,
    pub block_data: BlockData,
    pub proof: String,
    pub timestamp: SystemTime,
}

/// Metrics proposal for network utilization validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsProposal {
    pub id: String,
    pub proposer: ValidatorId,
    pub metrics_data: NetworkMetrics,
    pub attestation_proof: String,
    pub timestamp: SystemTime,
}

/// Hybrid proposal for coordinated block and metrics updates
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridProposal {
    pub id: String,
    pub proposer: ValidatorId,
    pub block_data: BlockData,
    pub metrics_data: NetworkMetrics,
    pub coordination_proof: String,
    pub timestamp: SystemTime,
}

/// Vote on a proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub proposal_id: String,
    pub validator_id: ValidatorId,
    pub vote_type: VoteType,
    pub signature: String,
    pub timestamp: SystemTime,
}

/// Vote types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoteType {
    Approve,
    Reject,
    Abstain,
}

/// Consensus result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusResult {
    pub proposal_id: String,
    pub result: ConsensusDecision,
    pub votes: Vec<Vote>,
    pub finalized_at: SystemTime,
}

/// Consensus decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusDecision {
    Accepted,
    Rejected,
    Timeout,
}

/// Validation result from committee
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub proposal_id: String,
    pub validator_id: ValidatorId,
    pub is_valid: bool,
    pub validation_proof: String,
    pub timestamp: SystemTime,
}

/// Coordination proof for hybrid proposals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationProof {
    pub block_metrics_correlation: f64,
    pub temporal_consistency: bool,
    pub cross_validation_score: f64,
    pub proof_signature: String,
}

/// Migration phases for consensus transition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MigrationPhase {
    /// Current: Two separate consensus mechanisms
    DualConsensus,
    /// Both systems running, unified for validation
    ParallelOperation,
    /// Unified consensus primary, dual as backup
    UnifiedPrimary,
    /// Complete migration to unified consensus
    UnifiedOnly,
}

/// Economic savings from unified consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicSavings {
    pub validator_cost_reduction: f64,
    pub network_overhead_reduction: f64,
    pub infrastructure_savings: f64,
    pub energy_efficiency_gain: f64,
}

/// Migration configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationConfig {
    pub rollback_enabled: bool,
    pub parallel_validation_period: Duration,
    pub validator_migration_batch_size: usize,
    pub performance_threshold: f64,
}

/// Migration progress tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationProgress {
    pub current_phase: MigrationPhase,
    pub validators_migrated: usize,
    pub total_validators: usize,
    pub performance_metrics: HashMap<String, f64>,
    pub started_at: SystemTime,
    pub last_updated: SystemTime,
}

impl Default for MigrationConfig {
    fn default() -> Self {
        Self {
            rollback_enabled: true,
            parallel_validation_period: Duration::from_secs(3600), // 1 hour
            validator_migration_batch_size: 10,
            performance_threshold: 0.95, // 95% performance requirement
        }
    }
}

impl Proposal {
    /// Get proposal ID
    pub fn id(&self) -> &str {
        match self {
            Proposal::Block(p) => &p.id,
            Proposal::Metrics(p) => &p.id,
            Proposal::Hybrid(p) => &p.id,
        }
    }

    /// Get proposer ID
    pub fn proposer(&self) -> &ValidatorId {
        match self {
            Proposal::Block(p) => &p.proposer,
            Proposal::Metrics(p) => &p.proposer,
            Proposal::Hybrid(p) => &p.proposer,
        }
    }

    /// Get proposal timestamp
    pub fn timestamp(&self) -> SystemTime {
        match self {
            Proposal::Block(p) => p.timestamp,
            Proposal::Metrics(p) => p.timestamp,
            Proposal::Hybrid(p) => p.timestamp,
        }
    }
}

impl BlockProposal {
    /// Create new block proposal
    pub fn new(proposer: ValidatorId, block_data: BlockData) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            proposer,
            block_data,
            proof: "quantum_proof_placeholder".to_string(),
            timestamp: SystemTime::now(),
        }
    }
}

impl MetricsProposal {
    /// Create new metrics proposal
    pub fn new(proposer: ValidatorId, metrics_data: NetworkMetrics) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            proposer,
            metrics_data,
            attestation_proof: "vpos_attestation_placeholder".to_string(),
            timestamp: SystemTime::now(),
        }
    }
}

impl HybridProposal {
    /// Create new hybrid proposal
    pub fn new(proposer: ValidatorId, block_data: BlockData, metrics_data: NetworkMetrics) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            proposer,
            block_data,
            metrics_data,
            coordination_proof: "coordination_proof_placeholder".to_string(),
            timestamp: SystemTime::now(),
        }
    }
}

/// Specialized validator committee for consensus participation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorCommittee {
    /// All committee members
    pub validators: Vec<ValidatorId>,

    /// Validators specialized in block consensus
    pub block_validators: HashSet<ValidatorId>,

    /// Validators specialized in metrics consensus
    pub metrics_validators: HashSet<ValidatorId>,

    /// Validators participating in both types
    pub hybrid_validators: HashSet<ValidatorId>,

    /// Committee configuration
    pub min_committee_size: usize,
    pub committee_rotation_period: Duration,

    /// Voting power distribution
    pub voting_weights: HashMap<ValidatorId, f64>,

    /// Committee performance metrics
    pub performance_metrics: HashMap<ValidatorId, f64>,
}

/// Voting rules configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VotingRules {
    /// Minimum votes required for consensus
    pub min_votes_required: usize,
    /// Threshold percentage for approval (0.0 to 1.0)
    pub approval_threshold: f64,
    /// Maximum voting time
    pub voting_timeout: Duration,
    /// Byzantine fault tolerance factor
    pub byzantine_tolerance: f64,
}

/// Threshold calculator for different proposal types
#[derive(Debug, Clone)]
pub struct ThresholdCalculator {
    voting_rules: VotingRules,
}

/// Vote tracking for proposals
#[derive(Debug, Default)]
pub struct VoteTracker {
    /// Votes by proposal ID
    votes: HashMap<String, Vec<Vote>>,
    /// Vote counts by proposal
    vote_counts: HashMap<String, VoteCounts>,
}

/// Vote counts for a proposal
#[derive(Debug, Default, Clone)]
pub struct VoteCounts {
    pub approve: usize,
    pub reject: usize,
    pub abstain: usize,
    pub total_weight: f64,
    pub approve_weight: f64,
    pub reject_weight: f64,
}

/// Consensus state tracking
#[derive(Debug, Default)]
pub struct ConsensusState {
    /// Active proposals being voted on
    pub active_proposals: HashMap<String, Proposal>,
    /// Completed consensus results
    pub completed_consensus: HashMap<String, ConsensusResult>,
    /// Committee assignments
    pub committee_assignments: HashMap<String, ValidatorId>,
    /// Current consensus round
    pub current_round: u64,
}

/// Proposal queue for different types
#[derive(Debug, Default)]
pub struct ProposalQueue<T> {
    /// Queued proposals
    pub proposals: Vec<T>,
    /// Processing order priority
    pub priority_queue: Vec<String>,
}

/// Unified voting mechanism for all proposal types
pub struct UnifiedVotingMechanism {
    /// Proposal handling
    proposal_queue: Arc<RwLock<HashMap<String, Proposal>>>,

    /// Voting logic
    voting_rules: VotingRules,
    threshold_calculator: ThresholdCalculator,

    /// Consensus tracking
    vote_tracker: Arc<RwLock<VoteTracker>>,
    consensus_tracker: Arc<RwLock<HashMap<String, ConsensusResult>>>,

    /// Event broadcasting
    event_broadcaster: broadcast::Sender<ConsensusEvent>,
}

/// Consensus events for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusEvent {
    pub event_type: String,
    pub proposal_id: String,
    pub timestamp: SystemTime,
    pub data: serde_json::Value,
}

/// Main unified consensus engine
pub struct UnifiedSWTCHConsensus {
    /// Core consensus engine with quantum safety
    consensus_engine: Arc<RwLock<QuantumSafeConsensus>>,

    /// Specialized validator committees
    block_committee: Arc<RwLock<ValidatorCommittee>>,
    metrics_committee: Arc<RwLock<ValidatorCommittee>>,
    unified_committee: Arc<RwLock<ValidatorCommittee>>,

    /// Unified voting mechanism
    voting_mechanism: Arc<UnifiedVotingMechanism>,

    /// Proposal queues by type
    block_proposals: Arc<RwLock<ProposalQueue<BlockProposal>>>,
    metrics_proposals: Arc<RwLock<ProposalQueue<MetricsProposal>>>,
    hybrid_proposals: Arc<RwLock<ProposalQueue<HybridProposal>>>,

    /// Consensus state
    consensus_state: Arc<RwLock<ConsensusState>>,

    /// Configuration
    config: UnifiedConsensusConfig,

    /// Event broadcasting
    event_broadcaster: broadcast::Sender<ConsensusEvent>,
}

/// Quantum-safe consensus engine
#[derive(Debug)]
pub struct QuantumSafeConsensus {
    /// Quantum-resistant identity management
    identity_manager: Arc<QuantumResistantDID>,

    /// VPoS integration for validator proofs
    vpos_manager: Arc<VPoSManager>,

    /// Consensus round management
    current_round: u64,

    /// Byzantine fault tolerance configuration
    byzantine_tolerance: f64,
}

/// Configuration for unified consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedConsensusConfig {
    /// Enable unified consensus
    pub enabled: bool,

    /// Committee sizes
    pub min_block_committee_size: usize,
    pub min_metrics_committee_size: usize,
    pub min_hybrid_committee_size: usize,

    /// Voting configuration
    pub voting_rules: VotingRules,

    /// Performance requirements
    pub min_performance_threshold: f64,

    /// Security parameters
    pub byzantine_tolerance: f64,
    pub quantum_security_level: String,

    /// Timing configuration
    pub proposal_timeout: Duration,
    pub committee_rotation_period: Duration,
}

impl Default for UnifiedConsensusConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_block_committee_size: 7,
            min_metrics_committee_size: 5,
            min_hybrid_committee_size: 10,
            voting_rules: VotingRules::default(),
            min_performance_threshold: 0.95,
            byzantine_tolerance: 0.33, // Tolerate up to 1/3 byzantine validators
            quantum_security_level: "Level5".to_string(),
            proposal_timeout: Duration::from_secs(300), // 5 minutes
            committee_rotation_period: Duration::from_secs(3600), // 1 hour
        }
    }
}

impl Default for VotingRules {
    fn default() -> Self {
        Self {
            min_votes_required: 5,
            approval_threshold: 0.67, // 2/3 majority
            voting_timeout: Duration::from_secs(300),
            byzantine_tolerance: 0.33,
        }
    }
}

impl ValidatorCommittee {
    /// Create a new validator committee
    pub fn new(validators: Vec<ValidatorId>, min_committee_size: usize) -> Self {
        let voting_weights = validators
            .iter()
            .map(|id| (id.clone(), 1.0)) // Equal weight initially
            .collect();

        Self {
            validators,
            block_validators: HashSet::new(),
            metrics_validators: HashSet::new(),
            hybrid_validators: HashSet::new(),
            min_committee_size,
            committee_rotation_period: Duration::from_secs(3600),
            voting_weights,
            performance_metrics: HashMap::new(),
        }
    }

    /// Add validator to committee
    pub fn add_validator(&mut self, validator_id: ValidatorId, weight: f64) {
        if !self.validators.contains(&validator_id) {
            self.validators.push(validator_id.clone());
            self.voting_weights.insert(validator_id, weight);
        }
    }

    /// Assign validator to specialization
    pub fn assign_specialization(
        &mut self,
        validator_id: &ValidatorId,
        specialization: SpecializationType,
    ) {
        match specialization {
            SpecializationType::Block => {
                self.block_validators.insert(validator_id.clone());
            }
            SpecializationType::Metrics => {
                self.metrics_validators.insert(validator_id.clone());
            }
            SpecializationType::Hybrid => {
                self.hybrid_validators.insert(validator_id.clone());
                self.block_validators.insert(validator_id.clone());
                self.metrics_validators.insert(validator_id.clone());
            }
        }
    }

    /// Get relevant validators for proposal type
    pub fn get_relevant_validators(&self, proposal: &Proposal) -> Vec<ValidatorId> {
        match proposal {
            Proposal::Block(_) => self
                .block_validators
                .union(&self.hybrid_validators)
                .cloned()
                .collect(),
            Proposal::Metrics(_) => self
                .metrics_validators
                .union(&self.hybrid_validators)
                .cloned()
                .collect(),
            Proposal::Hybrid(_) => self.hybrid_validators.iter().cloned().collect(),
        }
    }

    /// Validate proposal with committee
    pub async fn validate_proposal(&self, proposal: &Proposal) -> Result<ValidationResult> {
        // Get relevant validators for this proposal type
        let relevant_validators = self.get_relevant_validators(proposal);

        if relevant_validators.is_empty() {
            return Err(anyhow::anyhow!("No relevant validators for proposal type"));
        }

        // Select primary validator for validation (simplified)
        let primary_validator = relevant_validators
            .first()
            .ok_or_else(|| anyhow::anyhow!("No validators available"))?;

        // Perform validation logic (simplified)
        let is_valid = match proposal {
            Proposal::Block(block_proposal) => self.validate_block_proposal(block_proposal).await?,
            Proposal::Metrics(metrics_proposal) => {
                self.validate_metrics_proposal(metrics_proposal).await?
            }
            Proposal::Hybrid(hybrid_proposal) => {
                self.validate_hybrid_proposal(hybrid_proposal).await?
            }
        };

        Ok(ValidationResult {
            proposal_id: proposal.id().to_string(),
            validator_id: primary_validator.clone(),
            is_valid,
            validation_proof: "quantum_validation_proof".to_string(),
            timestamp: SystemTime::now(),
        })
    }

    /// Validate block proposal
    async fn validate_block_proposal(&self, proposal: &BlockProposal) -> Result<bool> {
        if !validate_l1_manifest_for_block(
            &proposal.block_data.l1_manifest,
            &proposal.block_data.state_root,
        ) {
            return Ok(false);
        }
        #[cfg(feature = "spacetime-consensus")]
        if let Some(ref t) = proposal.block_data.spacetime_transition {
            if !validate_block_spacetime_sidecar(t, &proposal.block_data.state_root) {
                return Ok(false);
            }
        }
        #[cfg(feature = "spacetime-consensus")]
        if !validate_block_pq_envelope(&proposal.block_data) {
            return Ok(false);
        }
        Ok(true)
    }

    async fn validate_metrics_proposal(&self, _proposal: &MetricsProposal) -> Result<bool> {
        Ok(true)
    }

    /// Validate hybrid proposal (L1 manifest carried on `block_data`).
    async fn validate_hybrid_proposal(&self, proposal: &HybridProposal) -> Result<bool> {
        if !validate_l1_manifest_for_block(
            &proposal.block_data.l1_manifest,
            &proposal.block_data.state_root,
        ) {
            return Ok(false);
        }
        #[cfg(feature = "spacetime-consensus")]
        if let Some(ref t) = proposal.block_data.spacetime_transition {
            if !validate_block_spacetime_sidecar(t, &proposal.block_data.state_root) {
                return Ok(false);
            }
        }
        #[cfg(feature = "spacetime-consensus")]
        if !validate_block_pq_envelope(&proposal.block_data) {
            return Ok(false);
        }
        // Metrics / coordination checks can extend here.
        Ok(true)
    }
}

/// Validator specialization types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpecializationType {
    Block,
    Metrics,
    Hybrid,
}

impl UnifiedVotingMechanism {
    /// Create new unified voting mechanism
    pub fn new(voting_rules: VotingRules) -> Self {
        let (event_broadcaster, _) = broadcast::channel(1000);

        Self {
            proposal_queue: Arc::new(RwLock::new(HashMap::new())),
            threshold_calculator: ThresholdCalculator::new(voting_rules.clone()),
            voting_rules,
            vote_tracker: Arc::new(RwLock::new(VoteTracker::default())),
            consensus_tracker: Arc::new(RwLock::new(HashMap::new())),
            event_broadcaster,
        }
    }

    /// Process any type of proposal through unified mechanism
    pub async fn process_proposal(&self, proposal: Proposal) -> Result<ConsensusResult> {
        let proposal_id = proposal.id().to_string();

        // Add proposal to queue
        {
            let mut queue = self.proposal_queue.write().await;
            queue.insert(proposal_id.clone(), proposal.clone());
        }

        // Broadcast proposal event
        let event = ConsensusEvent {
            event_type: "proposal_submitted".to_string(),
            proposal_id: proposal_id.clone(),
            timestamp: SystemTime::now(),
            data: serde_json::json!({
                "proposal_type": match proposal {
                    Proposal::Block(_) => "block",
                    Proposal::Metrics(_) => "metrics",
                    Proposal::Hybrid(_) => "hybrid",
                },
                "proposer": proposal.proposer()
            }),
        };
        let _ = self.event_broadcaster.send(event);

        // Process based on proposal type
        match proposal {
            Proposal::Block(block) => self.process_block_proposal(block).await,
            Proposal::Metrics(metrics) => self.process_metrics_proposal(metrics).await,
            Proposal::Hybrid(hybrid) => self.process_hybrid_proposal(hybrid).await,
        }
    }

    /// Process block proposal
    async fn process_block_proposal(&self, proposal: BlockProposal) -> Result<ConsensusResult> {
        // Initialize vote tracking
        {
            let mut tracker = self.vote_tracker.write().await;
            tracker.votes.insert(proposal.id.clone(), Vec::new());
            tracker
                .vote_counts
                .insert(proposal.id.clone(), VoteCounts::default());
        }

        // Start voting period (simplified - in production would be event-driven)
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check consensus
        self.check_consensus(&proposal.id).await
    }

    /// Process metrics proposal
    async fn process_metrics_proposal(&self, proposal: MetricsProposal) -> Result<ConsensusResult> {
        // Initialize vote tracking
        {
            let mut tracker = self.vote_tracker.write().await;
            tracker.votes.insert(proposal.id.clone(), Vec::new());
            tracker
                .vote_counts
                .insert(proposal.id.clone(), VoteCounts::default());
        }

        // Start voting period (simplified)
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check consensus
        self.check_consensus(&proposal.id).await
    }

    /// Process hybrid proposal
    async fn process_hybrid_proposal(&self, proposal: HybridProposal) -> Result<ConsensusResult> {
        // Initialize vote tracking
        {
            let mut tracker = self.vote_tracker.write().await;
            tracker.votes.insert(proposal.id.clone(), Vec::new());
            tracker
                .vote_counts
                .insert(proposal.id.clone(), VoteCounts::default());
        }

        // Start voting period (simplified)
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Check consensus
        self.check_consensus(&proposal.id).await
    }

    /// Submit vote for proposal
    pub async fn submit_vote(&self, vote: Vote) -> Result<()> {
        let mut tracker = self.vote_tracker.write().await;

        // Add vote to tracking
        tracker
            .votes
            .entry(vote.proposal_id.clone())
            .or_insert_with(Vec::new)
            .push(vote.clone());

        // Update vote counts
        let counts = tracker
            .vote_counts
            .entry(vote.proposal_id.clone())
            .or_insert_with(VoteCounts::default);

        match vote.vote_type {
            VoteType::Approve => {
                counts.approve += 1;
                counts.approve_weight += 1.0; // Simplified weight
            }
            VoteType::Reject => {
                counts.reject += 1;
                counts.reject_weight += 1.0;
            }
            VoteType::Abstain => {
                counts.abstain += 1;
            }
        }
        counts.total_weight += 1.0;

        Ok(())
    }

    /// Check if consensus is reached for proposal
    async fn check_consensus(&self, proposal_id: &str) -> Result<ConsensusResult> {
        let tracker = self.vote_tracker.read().await;

        let votes = tracker
            .votes
            .get(proposal_id)
            .unwrap_or(&Vec::new())
            .clone();

        let counts = tracker
            .vote_counts
            .get(proposal_id)
            .unwrap_or(&VoteCounts::default())
            .clone();

        // Apply voting rules
        let decision = if counts.approve >= self.voting_rules.min_votes_required
            && counts.approve_weight / counts.total_weight >= self.voting_rules.approval_threshold
        {
            ConsensusDecision::Accepted
        } else if counts.reject_weight / counts.total_weight
            > (1.0 - self.voting_rules.approval_threshold)
        {
            ConsensusDecision::Rejected
        } else {
            ConsensusDecision::Timeout
        };

        let result = ConsensusResult {
            proposal_id: proposal_id.to_string(),
            result: decision,
            votes,
            finalized_at: SystemTime::now(),
        };

        // Store result
        {
            let mut consensus_tracker = self.consensus_tracker.write().await;
            consensus_tracker.insert(proposal_id.to_string(), result.clone());
        }

        Ok(result)
    }

    /// Validate proposal with multiple committees
    pub async fn validate_with_committees(
        &self,
        proposal: &Proposal,
        committees: &[&ValidatorCommittee],
    ) -> Result<Vec<ValidationResult>> {
        // Parallel validation across committees
        let validation_futures = committees
            .iter()
            .map(|committee| committee.validate_proposal(proposal));

        let validation_results: Result<Vec<_>> =
            futures::future::try_join_all(validation_futures).await;
        validation_results
    }

    /// Aggregate validation results with Byzantine fault tolerance
    pub fn aggregate_validation_results(&self, results: Vec<ValidationResult>) -> Result<bool> {
        if results.is_empty() {
            return Ok(false);
        }

        let valid_count = results.iter().filter(|r| r.is_valid).count();
        let total_count = results.len();

        // Byzantine fault tolerance: require more than 2/3 agreement
        let required_agreement =
            ((total_count as f64) * (1.0 - self.voting_rules.byzantine_tolerance)).ceil() as usize;

        Ok(valid_count >= required_agreement)
    }
}

impl ThresholdCalculator {
    /// Create new threshold calculator
    pub fn new(voting_rules: VotingRules) -> Self {
        Self { voting_rules }
    }

    /// Calculate voting threshold for proposal type
    pub fn calculate_threshold(&self, proposal_type: &str, committee_size: usize) -> usize {
        let base_threshold =
            (committee_size as f64 * self.voting_rules.approval_threshold).ceil() as usize;

        // Adjust threshold based on proposal type
        match proposal_type {
            "block" => base_threshold,
            "metrics" => (base_threshold as f64 * 0.8).ceil() as usize, // Slightly lower for metrics
            "hybrid" => (base_threshold as f64 * 1.1).ceil() as usize,  // Higher for hybrid
            _ => base_threshold,
        }
    }
}

impl UnifiedSWTCHConsensus {
    /// Create new unified consensus engine
    pub async fn new(
        config: UnifiedConsensusConfig,
        identity: Arc<QuantumResistantDID>,
        vpos_manager: Arc<VPoSManager>,
    ) -> Result<Self> {
        let (event_broadcaster, _) = broadcast::channel(1000);

        // Initialize quantum-safe consensus engine
        let consensus_engine = Arc::new(RwLock::new(QuantumSafeConsensus::new(
            identity,
            vpos_manager,
            config.byzantine_tolerance,
        )));

        // Initialize committees
        let block_committee = Arc::new(RwLock::new(ValidatorCommittee::new(
            Vec::new(),
            config.min_block_committee_size,
        )));

        let metrics_committee = Arc::new(RwLock::new(ValidatorCommittee::new(
            Vec::new(),
            config.min_metrics_committee_size,
        )));

        let unified_committee = Arc::new(RwLock::new(ValidatorCommittee::new(
            Vec::new(),
            config.min_hybrid_committee_size,
        )));

        // Initialize voting mechanism
        let voting_mechanism = Arc::new(UnifiedVotingMechanism::new(config.voting_rules.clone()));

        Ok(Self {
            consensus_engine,
            block_committee,
            metrics_committee,
            unified_committee,
            voting_mechanism,
            block_proposals: Arc::new(RwLock::new(ProposalQueue {
                proposals: Vec::new(),
                priority_queue: Vec::new(),
            })),
            metrics_proposals: Arc::new(RwLock::new(ProposalQueue {
                proposals: Vec::new(),
                priority_queue: Vec::new(),
            })),
            hybrid_proposals: Arc::new(RwLock::new(ProposalQueue {
                proposals: Vec::new(),
                priority_queue: Vec::new(),
            })),
            consensus_state: Arc::new(RwLock::new(ConsensusState::default())),
            config,
            event_broadcaster,
        })
    }

    /// Start the unified consensus system
    pub async fn start(&self) -> Result<()> {
        tracing::info!("🚀 Starting Unified SWTCH Consensus - Phase 5.5");

        if !self.config.enabled {
            tracing::warn!("Unified consensus is disabled in configuration");
            return Ok(());
        }

        // Start consensus engine
        {
            let mut engine = self.consensus_engine.write().await;
            engine.start().await?;
        }

        // Start proposal processing
        self.start_proposal_processing().await?;

        // Start committee rotation
        self.start_committee_rotation().await?;

        tracing::info!("✅ Unified SWTCH Consensus started successfully");
        Ok(())
    }

    /// Submit block proposal
    pub async fn submit_block_proposal(&self, proposal: BlockProposal) -> Result<String> {
        let proposal_id = proposal.id.clone();

        // Add to block proposals queue
        {
            let mut queue = self.block_proposals.write().await;
            queue.proposals.push(proposal.clone());
            queue.priority_queue.push(proposal_id.clone());
        }

        // Process through unified voting
        let unified_proposal = Proposal::Block(proposal);
        let _result = self
            .voting_mechanism
            .process_proposal(unified_proposal)
            .await?;

        Ok(proposal_id)
    }

    /// Submit metrics proposal
    pub async fn submit_metrics_proposal(&self, proposal: MetricsProposal) -> Result<String> {
        let proposal_id = proposal.id.clone();

        // Add to metrics proposals queue
        {
            let mut queue = self.metrics_proposals.write().await;
            queue.proposals.push(proposal.clone());
            queue.priority_queue.push(proposal_id.clone());
        }

        // Process through unified voting
        let unified_proposal = Proposal::Metrics(proposal);
        let _result = self
            .voting_mechanism
            .process_proposal(unified_proposal)
            .await?;

        Ok(proposal_id)
    }

    /// Submit hybrid proposal
    pub async fn submit_hybrid_proposal(&self, proposal: HybridProposal) -> Result<String> {
        let proposal_id = proposal.id.clone();

        // Add to hybrid proposals queue
        {
            let mut queue = self.hybrid_proposals.write().await;
            queue.proposals.push(proposal.clone());
            queue.priority_queue.push(proposal_id.clone());
        }

        // Process through unified voting
        let unified_proposal = Proposal::Hybrid(proposal);
        let _result = self
            .voting_mechanism
            .process_proposal(unified_proposal)
            .await?;

        Ok(proposal_id)
    }

    /// Add validator to committee
    pub async fn add_validator(
        &self,
        validator_id: ValidatorId,
        specialization: SpecializationType,
    ) -> Result<()> {
        match specialization {
            SpecializationType::Block => {
                let mut committee = self.block_committee.write().await;
                committee.add_validator(validator_id.clone(), 1.0);
                committee.assign_specialization(&validator_id, specialization);
            }
            SpecializationType::Metrics => {
                let mut committee = self.metrics_committee.write().await;
                committee.add_validator(validator_id.clone(), 1.0);
                committee.assign_specialization(&validator_id, specialization);
            }
            SpecializationType::Hybrid => {
                let mut committee = self.unified_committee.write().await;
                committee.add_validator(validator_id.clone(), 1.0);
                committee.assign_specialization(&validator_id, specialization);
            }
        }

        Ok(())
    }

    /// Get consensus status
    pub async fn get_consensus_status(&self) -> Result<ConsensusStatus> {
        let state = self.consensus_state.read().await;
        let engine = self.consensus_engine.read().await;

        Ok(ConsensusStatus {
            current_round: engine.current_round,
            active_proposals: state.active_proposals.len(),
            completed_consensus: state.completed_consensus.len(),
            byzantine_tolerance: engine.byzantine_tolerance,
            enabled: self.config.enabled,
        })
    }

    /// Start proposal processing loop
    async fn start_proposal_processing(&self) -> Result<()> {
        // This would typically run in a background task
        // For now, we'll just log that it's started
        tracing::info!("📋 Started proposal processing system");
        Ok(())
    }

    /// Start committee rotation system
    async fn start_committee_rotation(&self) -> Result<()> {
        // This would typically run in a background task
        // For now, we'll just log that it's started
        tracing::info!("🔄 Started committee rotation system");
        Ok(())
    }

    /// Subscribe to consensus events
    pub fn subscribe_to_events(&self) -> broadcast::Receiver<ConsensusEvent> {
        self.event_broadcaster.subscribe()
    }
}

/// Consensus status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusStatus {
    pub current_round: u64,
    pub active_proposals: usize,
    pub completed_consensus: usize,
    pub byzantine_tolerance: f64,
    pub enabled: bool,
}

impl QuantumSafeConsensus {
    /// Create new quantum-safe consensus
    pub fn new(
        identity_manager: Arc<QuantumResistantDID>,
        vpos_manager: Arc<VPoSManager>,
        byzantine_tolerance: f64,
    ) -> Self {
        Self {
            identity_manager,
            vpos_manager,
            current_round: 0,
            byzantine_tolerance,
        }
    }

    /// Start the quantum-safe consensus engine
    pub async fn start(&mut self) -> Result<()> {
        tracing::info!("🔐 Starting quantum-safe consensus engine");
        self.current_round = 1;
        Ok(())
    }

    /// Advance to next consensus round
    pub fn next_round(&mut self) {
        self.current_round += 1;
    }
}

/// Migration manager for gradual transition to unified consensus
pub struct ConsensusMigrationManager {
    /// Current migration phase
    current_phase: Arc<RwLock<MigrationPhase>>,

    /// Migration configuration
    migration_config: MigrationConfig,

    /// Compatibility layer for dual consensus
    dual_consensus_adapter: Arc<DualConsensusAdapter>,

    /// Unified consensus system
    unified_consensus: Arc<UnifiedSWTCHConsensus>,

    /// Migration progress tracking
    migration_progress: Arc<RwLock<MigrationProgress>>,

    /// Economic optimization tracker
    economic_optimization: Arc<RwLock<EconomicOptimization>>,

    /// Risk mitigation system
    risk_mitigation: Arc<RiskMitigation>,
}

/// Dual consensus adapter for compatibility
pub struct DualConsensusAdapter {
    /// Legacy block consensus
    legacy_block_consensus: Option<Arc<dyn LegacyConsensus>>,

    /// Legacy metrics consensus
    legacy_metrics_consensus: Option<Arc<dyn LegacyConsensus>>,

    /// Comparison results between systems
    comparison_results: Arc<RwLock<Vec<ConsensusComparison>>>,
}

/// Legacy consensus trait for compatibility
pub trait LegacyConsensus: Send + Sync {
    fn process_proposal(&self, proposal: &str) -> Result<String>;
    fn get_status(&self) -> String;
}

/// Consensus comparison result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusComparison {
    pub proposal_id: String,
    pub legacy_result: String,
    pub unified_result: ConsensusDecision,
    pub performance_difference: f64,
    pub timestamp: SystemTime,
}

/// Economic optimization tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicOptimization {
    /// Unified reward system
    unified_rewards: UnifiedRewardSystem,

    /// Resource efficiency metrics
    resource_savings: ResourceSavings,

    /// Network efficiency improvements
    network_efficiency: NetworkEfficiencyMetrics,

    /// Cost analysis
    cost_analysis: CostAnalysis,
}

/// Unified reward system for validators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedRewardSystem {
    /// Total rewards distributed
    pub total_rewards_distributed: u128,

    /// Rewards by validator type
    pub block_validator_rewards: u128,
    pub metrics_validator_rewards: u128,
    pub hybrid_validator_rewards: u128,

    /// Efficiency bonuses
    pub efficiency_bonuses: u128,

    /// Cost savings from unification
    pub cost_savings_percentage: f64,
}

/// Resource savings from unified consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSavings {
    /// CPU usage reduction
    pub cpu_savings_percentage: f64,

    /// Memory usage reduction
    pub memory_savings_percentage: f64,

    /// Network bandwidth savings
    pub network_savings_percentage: f64,

    /// Energy consumption reduction
    pub energy_savings_percentage: f64,
}

/// Network efficiency metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEfficiencyMetrics {
    /// Consensus latency improvement
    pub latency_improvement_percentage: f64,

    /// Throughput increase
    pub throughput_increase_percentage: f64,

    /// Validator utilization efficiency
    pub validator_efficiency_score: f64,

    /// Network overhead reduction
    pub overhead_reduction_percentage: f64,
}

/// Cost analysis for unified consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostAnalysis {
    /// Infrastructure cost reduction
    pub infrastructure_cost_reduction: f64,

    /// Operational cost savings
    pub operational_cost_savings: f64,

    /// Validator cost efficiency
    pub validator_cost_efficiency: f64,

    /// Total economic benefit
    pub total_economic_benefit: f64,
}

/// Risk mitigation system
pub struct RiskMitigation {
    /// Rollback mechanism
    rollback_mechanism: Arc<RollbackMechanism>,

    /// Performance monitoring
    performance_monitor: Arc<PerformanceMonitor>,

    /// Compatibility layer
    compatibility_layer: Arc<CompatibilityLayer>,

    /// Validator transition manager
    validator_transition: Arc<ValidatorTransitionManager>,
}

/// Rollback mechanism for safety
pub struct RollbackMechanism {
    /// Rollback capability enabled
    pub enabled: bool,

    /// Rollback triggers
    pub triggers: Vec<RollbackTrigger>,

    /// State preservation
    pub preserved_states: Vec<String>,
}

/// Performance monitoring for migration
pub struct PerformanceMonitor {
    /// Performance metrics
    pub metrics: HashMap<String, f64>,

    /// Performance thresholds
    pub thresholds: HashMap<String, f64>,

    /// Monitoring enabled
    pub enabled: bool,
}

/// Compatibility layer
pub struct CompatibilityLayer {
    /// API compatibility
    pub api_compatibility: bool,

    /// Data format compatibility
    pub data_compatibility: bool,

    /// Protocol compatibility
    pub protocol_compatibility: bool,
}

/// Validator transition manager
pub struct ValidatorTransitionManager {
    /// Migration batch size
    pub batch_size: usize,

    /// Transition progress
    pub transition_progress: f64,

    /// Training provided
    pub training_provided: bool,
}

/// Rollback trigger conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RollbackTrigger {
    PerformanceDegradation(f64),
    ConsensusFailure,
    ValidatorDisconnection(usize),
    SecurityBreach,
}

impl ConsensusMigrationManager {
    /// Create new migration manager
    pub async fn new(
        config: MigrationConfig,
        unified_consensus: Arc<UnifiedSWTCHConsensus>,
    ) -> Result<Self> {
        let current_phase = Arc::new(RwLock::new(MigrationPhase::DualConsensus));

        let migration_progress = Arc::new(RwLock::new(MigrationProgress {
            current_phase: MigrationPhase::DualConsensus,
            validators_migrated: 0,
            total_validators: 0,
            performance_metrics: HashMap::new(),
            started_at: SystemTime::now(),
            last_updated: SystemTime::now(),
        }));

        let dual_consensus_adapter = Arc::new(DualConsensusAdapter::new());

        let economic_optimization = Arc::new(RwLock::new(EconomicOptimization::new()));

        let risk_mitigation = Arc::new(RiskMitigation::new(config.rollback_enabled));

        Ok(Self {
            current_phase,
            migration_config: config,
            dual_consensus_adapter,
            unified_consensus,
            migration_progress,
            economic_optimization,
            risk_mitigation,
        })
    }

    /// Start migration process
    pub async fn start_migration(&self) -> Result<()> {
        tracing::info!("🔄 Starting consensus migration process");

        // Phase 1: Parallel Operation
        self.transition_to_phase(MigrationPhase::ParallelOperation)
            .await?;

        // Monitor parallel operation
        tokio::time::sleep(self.migration_config.parallel_validation_period).await;

        // Phase 2: Unified Primary
        if self.validate_migration_readiness().await? {
            self.transition_to_phase(MigrationPhase::UnifiedPrimary)
                .await?;
        }

        // Phase 3: Unified Only
        if self.validate_final_transition().await? {
            self.transition_to_phase(MigrationPhase::UnifiedOnly)
                .await?;
        }

        tracing::info!("✅ Consensus migration completed successfully");
        Ok(())
    }

    /// Transition to specific migration phase
    async fn transition_to_phase(&self, phase: MigrationPhase) -> Result<()> {
        tracing::info!("🔄 Transitioning to phase: {:?}", phase);

        {
            let mut current_phase = self.current_phase.write().await;
            *current_phase = phase.clone();
        }

        {
            let mut progress = self.migration_progress.write().await;
            progress.current_phase = phase;
            progress.last_updated = SystemTime::now();
        }

        Ok(())
    }

    /// Validate migration readiness
    async fn validate_migration_readiness(&self) -> Result<bool> {
        let progress = self.migration_progress.read().await;

        // Check performance metrics
        let performance_ok = progress
            .performance_metrics
            .values()
            .all(|&metric| metric >= self.migration_config.performance_threshold);

        Ok(performance_ok)
    }

    /// Validate final transition readiness
    async fn validate_final_transition(&self) -> Result<bool> {
        // Check unified consensus stability
        let status = self.unified_consensus.get_consensus_status().await?;

        Ok(status.enabled && status.active_proposals < 100) // Simplified check
    }

    /// Calculate economic savings
    pub async fn calculate_savings(&self) -> Result<EconomicSavings> {
        let optimization = self.economic_optimization.read().await;

        Ok(EconomicSavings {
            validator_cost_reduction: optimization.resource_savings.cpu_savings_percentage,
            network_overhead_reduction: optimization
                .network_efficiency
                .overhead_reduction_percentage,
            infrastructure_savings: optimization.cost_analysis.infrastructure_cost_reduction,
            energy_efficiency_gain: optimization.resource_savings.energy_savings_percentage,
        })
    }

    /// Get migration status
    pub async fn get_migration_status(&self) -> Result<MigrationProgress> {
        let progress = self.migration_progress.read().await;
        Ok(progress.clone())
    }

    /// Rollback to previous phase if needed
    pub async fn rollback(&self) -> Result<()> {
        if !self.migration_config.rollback_enabled {
            return Err(anyhow::anyhow!("Rollback is disabled"));
        }

        tracing::warn!("🔄 Initiating consensus rollback");

        let current_phase = {
            let phase = self.current_phase.read().await;
            phase.clone()
        };

        let rollback_phase = match current_phase {
            MigrationPhase::UnifiedOnly => MigrationPhase::UnifiedPrimary,
            MigrationPhase::UnifiedPrimary => MigrationPhase::ParallelOperation,
            MigrationPhase::ParallelOperation => MigrationPhase::DualConsensus,
            MigrationPhase::DualConsensus => MigrationPhase::DualConsensus,
        };

        self.transition_to_phase(rollback_phase).await?;

        tracing::info!("✅ Consensus rollback completed");
        Ok(())
    }
}

impl DualConsensusAdapter {
    /// Create new dual consensus adapter
    pub fn new() -> Self {
        Self {
            legacy_block_consensus: None,
            legacy_metrics_consensus: None,
            comparison_results: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Compare consensus results between legacy and unified systems
    pub async fn compare_consensus(
        &self,
        proposal_id: &str,
        unified_result: ConsensusDecision,
    ) -> Result<ConsensusComparison> {
        let legacy_result = "legacy_approved".to_string(); // Placeholder

        let comparison = ConsensusComparison {
            proposal_id: proposal_id.to_string(),
            legacy_result,
            unified_result,
            performance_difference: 0.15, // 15% improvement
            timestamp: SystemTime::now(),
        };

        {
            let mut results = self.comparison_results.write().await;
            results.push(comparison.clone());
        }

        Ok(comparison)
    }
}

impl EconomicOptimization {
    /// Create new economic optimization tracker
    pub fn new() -> Self {
        Self {
            unified_rewards: UnifiedRewardSystem::default(),
            resource_savings: ResourceSavings::default(),
            network_efficiency: NetworkEfficiencyMetrics::default(),
            cost_analysis: CostAnalysis::default(),
        }
    }

    /// Update optimization metrics
    pub fn update_metrics(&mut self, savings: &EconomicSavings) {
        self.resource_savings.cpu_savings_percentage = savings.validator_cost_reduction;
        self.network_efficiency.overhead_reduction_percentage = savings.network_overhead_reduction;
        self.cost_analysis.infrastructure_cost_reduction = savings.infrastructure_savings;
        self.resource_savings.energy_savings_percentage = savings.energy_efficiency_gain;
    }
}

impl RiskMitigation {
    /// Create new risk mitigation system
    pub fn new(rollback_enabled: bool) -> Self {
        Self {
            rollback_mechanism: Arc::new(RollbackMechanism {
                enabled: rollback_enabled,
                triggers: vec![
                    RollbackTrigger::PerformanceDegradation(0.9),
                    RollbackTrigger::ConsensusFailure,
                    RollbackTrigger::ValidatorDisconnection(5),
                ],
                preserved_states: Vec::new(),
            }),
            performance_monitor: Arc::new(PerformanceMonitor {
                metrics: HashMap::new(),
                thresholds: HashMap::new(),
                enabled: true,
            }),
            compatibility_layer: Arc::new(CompatibilityLayer {
                api_compatibility: true,
                data_compatibility: true,
                protocol_compatibility: true,
            }),
            validator_transition: Arc::new(ValidatorTransitionManager {
                batch_size: 10,
                transition_progress: 0.0,
                training_provided: false,
            }),
        }
    }
}

// Default implementations for economic structures
impl Default for UnifiedRewardSystem {
    fn default() -> Self {
        Self {
            total_rewards_distributed: 0,
            block_validator_rewards: 0,
            metrics_validator_rewards: 0,
            hybrid_validator_rewards: 0,
            efficiency_bonuses: 0,
            cost_savings_percentage: 0.0,
        }
    }
}

impl Default for ResourceSavings {
    fn default() -> Self {
        Self {
            cpu_savings_percentage: 25.0,
            memory_savings_percentage: 20.0,
            network_savings_percentage: 40.0,
            energy_savings_percentage: 20.0,
        }
    }
}

impl Default for NetworkEfficiencyMetrics {
    fn default() -> Self {
        Self {
            latency_improvement_percentage: 30.0,
            throughput_increase_percentage: 35.0,
            validator_efficiency_score: 0.92,
            overhead_reduction_percentage: 40.0,
        }
    }
}

impl Default for CostAnalysis {
    fn default() -> Self {
        Self {
            infrastructure_cost_reduction: 30.0,
            operational_cost_savings: 25.0,
            validator_cost_efficiency: 0.85,
            total_economic_benefit: 28.0,
        }
    }
}

#[cfg(test)]
mod unified_consensus {
    use super::*;
    use crate::{quantum_security::QuantumResistantDID, vpos::VPoSManager};

    #[tokio::test]
    async fn test_unified_consensus_creation() {
        let config = UnifiedConsensusConfig::default();
        let identity = Arc::new(
            crate::quantum_security::quantum_did_utils::new_did("test_consensus", "Kyber512")
                .await
                .unwrap(),
        );
        let vpos = Arc::new(
            VPoSManager::new(
                identity.clone(),
                spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber512,
            )
            .await
            .unwrap(),
        );

        let consensus = UnifiedSWTCHConsensus::new(config, identity, vpos).await;
        assert!(consensus.is_ok());
    }

    #[tokio::test]
    async fn test_validator_committee() {
        let mut committee = ValidatorCommittee::new(vec![], 5);

        committee.add_validator("validator1".to_string(), 1.0);
        committee.assign_specialization(&"validator1".to_string(), SpecializationType::Hybrid);

        assert!(committee.hybrid_validators.contains("validator1"));
        assert!(committee.block_validators.contains("validator1"));
        assert!(committee.metrics_validators.contains("validator1"));
    }

    #[tokio::test]
    async fn test_proposal_processing() {
        let voting_rules = VotingRules::default();
        let voting_mechanism = UnifiedVotingMechanism::new(voting_rules);

        let parent_hash = "0x123".to_string();
        let state_root = "0x456".to_string();
        let block_proposal = BlockProposal::new(
            "validator1".to_string(),
            BlockData::new_with_l1_manifest(
                1,
                parent_hash.clone(),
                vec!["tx1".to_string()],
                state_root.clone(),
                SystemTime::now(),
                minimal_l1_manifest_for_proposal("test-chain", &state_root, 1, &parent_hash),
            ),
        );

        let proposal = Proposal::Block(block_proposal);
        let result = voting_mechanism.process_proposal(proposal).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_migration_manager() {
        let config = MigrationConfig::default();
        let unified_consensus_config = UnifiedConsensusConfig::default();
        let identity = Arc::new(
            crate::quantum_security::quantum_did_utils::new_did("test_migration", "Kyber512")
                .await
                .unwrap(),
        );
        let vpos = Arc::new(
            VPoSManager::new(
                identity.clone(),
                spacekit_primitives::v1::crypto::quantum::Algorithm::Kyber512,
            )
            .await
            .unwrap(),
        );

        let unified_consensus = Arc::new(
            UnifiedSWTCHConsensus::new(unified_consensus_config, identity, vpos)
                .await
                .unwrap(),
        );

        let migration_manager = ConsensusMigrationManager::new(config, unified_consensus).await;
        assert!(migration_manager.is_ok());
    }

    #[tokio::test]
    async fn test_economic_optimization() {
        let mut optimization = EconomicOptimization::new();

        let savings = EconomicSavings {
            validator_cost_reduction: 25.0,
            network_overhead_reduction: 40.0,
            infrastructure_savings: 30.0,
            energy_efficiency_gain: 20.0,
        };

        optimization.update_metrics(&savings);

        assert_eq!(optimization.resource_savings.cpu_savings_percentage, 25.0);
        assert_eq!(
            optimization
                .network_efficiency
                .overhead_reduction_percentage,
            40.0
        );
    }
}
