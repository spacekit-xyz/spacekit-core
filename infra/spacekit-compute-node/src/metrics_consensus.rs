//! Metrics Consensus Module (Phase 5.4)
//!
//! Critical security layer for production metrics that provides:
//! - VPoS-based metrics attestation for cryptographic verification
//! - Byzantine fault-tolerant aggregation with reputation weighting
//! - Anti-manipulation detection using statistical analysis
//! - Cross-node validation protocols for network-wide consensus
//!
//! This module is essential for preventing economic attacks and ensuring
//! trustworthy network metrics for pricing, load balancing, and rewards.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

// Import our infrastructure
use crate::{
    production_metrics::{
        MetricValue, MetricsSnapshot, NetworkStatistics, PerformanceMetrics, StorageStatistics,
    },
    quantum_security::QuantumResistantDID,
    resource_monitor::ResourceMetrics,
    vpos::{
        ComputationProof, QualityMetrics, ResourceProof, ServiceProof, ServiceType, VPoSManager,
    },
    ComputeResult, ComputeTask, ExecutionMetrics, TaskStatus,
};

/// Metrics Consensus Manager - Byzantine fault-tolerant metrics validation
pub struct MetricsConsensusManager {
    /// VPoS-based attestation system
    vpos_attestation: Arc<VPoSMetricsAttestationManager>,

    /// Byzantine fault-tolerant aggregator
    bft_aggregator: Arc<ByzantineFaultTolerantAggregator>,

    /// Anti-manipulation detection system
    manipulation_detector: Arc<MetricsManipulationDetector>,

    /// Cross-node validation protocol
    cross_node_validator: Arc<CrossNodeMetricsValidator>,

    /// Consensus configuration
    config: MetricsConsensusConfig,

    /// Event broadcaster for consensus events
    event_broadcaster: broadcast::Sender<ConsensusEvent>,
}

/// Configuration for metrics consensus
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConsensusConfig {
    /// Minimum nodes required for consensus
    pub min_consensus_nodes: u32,

    /// Maximum Byzantine nodes tolerated (f < n/3)
    pub max_byzantine_nodes: u32,

    /// Consensus timeout in seconds
    pub consensus_timeout_seconds: u64,

    /// Reputation weight threshold for participation
    pub min_reputation_threshold: f64,

    /// Statistical outlier detection threshold
    pub outlier_threshold: f64,

    /// Manipulation detection sensitivity
    pub manipulation_sensitivity: f64,

    /// Enable attestation verification
    pub enable_vpos_attestation: bool,

    /// Enable Byzantine fault tolerance
    pub enable_bft_aggregation: bool,

    /// Enable manipulation detection
    pub enable_manipulation_detection: bool,
}

/// VPoS-based metrics attestation manager
pub struct VPoSMetricsAttestationManager {
    /// VPoS manager for proof generation
    vpos_manager: Arc<VPoSManager>,

    /// Attestation registry
    attestation_registry: Arc<RwLock<AttestationRegistry>>,

    /// Node identity for signing
    node_identity: Arc<QuantumResistantDID>,
}

/// Byzantine fault-tolerant aggregator
pub struct ByzantineFaultTolerantAggregator {
    /// Minimum nodes for consensus
    min_consensus_nodes: u32,

    /// Maximum Byzantine nodes tolerated
    max_byzantine_nodes: u32,

    /// Outlier detection threshold
    outlier_threshold: f64,

    /// Reputation weights for nodes
    reputation_weights: Arc<RwLock<HashMap<String, f64>>>,
}

/// Anti-manipulation detection system
pub struct MetricsManipulationDetector {
    /// Historical metrics for pattern analysis
    historical_metrics: Arc<RwLock<HashMap<String, Vec<NodeMetrics>>>>,

    /// Statistical analysis engine
    statistical_analyzer: Arc<StatisticalAnalyzer>,

    /// Anomaly detection thresholds
    anomaly_thresholds: AnomalyThresholds,

    /// Manipulation detection sensitivity
    sensitivity: f64,
}

/// Cross-node validation protocol
pub struct CrossNodeMetricsValidator {
    /// Connected nodes for validation
    connected_nodes: Arc<RwLock<HashMap<String, NodeConnection>>>,

    /// Validation timeout
    validation_timeout: Duration,

    /// Minimum validation responses required
    min_validation_responses: u32,
}

/// Metrics attestation with VPoS proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsAttestation {
    /// Node DID providing the attestation
    pub node_did: String,

    /// Hash of the attested metrics
    pub metrics_hash: String,

    /// VPoS proof for authenticity
    pub vpos_proof: ServiceProof,

    /// Timestamp of attestation
    pub timestamp: u64,

    /// Quantum-resistant signature
    pub signature: Vec<u8>,

    /// Attestation nonce for replay protection
    pub nonce: String,
}

/// Attested node metrics with VPoS proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestatedNodeMetrics {
    /// Node identifier
    pub node_id: String,

    /// Node's reported metrics
    pub metrics: NodeMetrics,

    /// VPoS attestation proof
    pub attestation: MetricsAttestation,

    /// Node's reputation score
    pub reputation_score: f64,

    /// Validation timestamp
    pub validation_timestamp: u64,
}

/// Node metrics data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeMetrics {
    /// Network statistics
    pub network_stats: NetworkStatistics,

    /// Storage statistics
    pub storage_stats: StorageStatistics,

    /// Performance metrics
    pub performance_metrics: PerformanceMetrics,

    /// Resource utilization
    pub resource_utilization: ResourceUtilization,

    /// Custom metrics
    pub custom_metrics: HashMap<String, MetricValue>,

    /// Collection timestamp
    pub timestamp: u64,
}

/// Resource utilization metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceUtilization {
    /// CPU utilization percentage
    pub cpu_utilization: f64,

    /// Memory utilization percentage
    pub memory_utilization: f64,

    /// Storage utilization percentage
    pub storage_utilization: f64,

    /// Network utilization percentage
    pub network_utilization: f64,

    /// GPU utilization percentage (if available)
    pub gpu_utilization: Option<f64>,
}

/// Consensus metrics with validation proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusMetrics {
    /// Aggregated network metrics
    pub aggregated_metrics: NetworkMetrics,

    /// Consensus proof
    pub consensus_proof: ConsensusProof,

    /// Participating nodes
    pub participating_nodes: u32,

    /// Excluded nodes (outliers/Byzantine)
    pub excluded_nodes: usize,

    /// Consensus timestamp
    pub consensus_timestamp: u64,

    /// Consensus validity period
    pub validity_period_seconds: u64,
}

/// Aggregated network metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    /// Weighted average network utilization
    pub network_utilization: f64,

    /// Weighted average storage utilization
    pub storage_utilization: f64,

    /// Weighted average performance metrics
    pub performance_metrics: PerformanceMetrics,

    /// Network health score
    pub health_score: f64,

    /// Consensus confidence level
    pub confidence_level: f64,
}

/// Proof of consensus achievement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusProof {
    /// Consensus algorithm used
    pub algorithm: ConsensusAlgorithm,

    /// Participating node attestations
    pub node_attestations: Vec<MetricsAttestation>,

    /// Aggregation method
    pub aggregation_method: AggregationMethod,

    /// Consensus threshold achieved
    pub threshold_achieved: f64,

    /// Byzantine fault tolerance level
    pub bft_level: u32,

    /// Merkle root of all attestations
    pub attestation_merkle_root: String,
}

/// Consensus algorithm types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusAlgorithm {
    /// Reputation-weighted Byzantine fault tolerance
    ReputationWeightedBFT,

    /// VPoS-based consensus
    VPoSConsensus,

    /// Hybrid consensus (VPoS + BFT)
    HybridConsensus,
}

/// Aggregation methods
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregationMethod {
    /// Weighted average by reputation
    ReputationWeighted,

    /// Median aggregation (Byzantine fault tolerant)
    MedianAggregation,

    /// Trimmed mean (remove outliers)
    TrimmedMean,

    /// Hybrid aggregation
    HybridAggregation,
}

/// Manipulation detection result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManipulationDetectionResult {
    /// Suspicious activities detected
    pub suspicious_activities: Vec<SuspiciousActivity>,

    /// Overall network trust score
    pub overall_trust_score: f64,

    /// Recommended actions
    pub recommended_actions: Vec<RecommendedAction>,

    /// Detection timestamp
    pub detection_timestamp: u64,
}

/// Suspicious activity detected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspiciousActivity {
    /// Node involved in suspicious activity
    pub node_id: String,

    /// Type of suspicious activity
    pub activity_type: SuspiciousActivityType,

    /// Confidence level (0.0 to 1.0)
    pub confidence: f64,

    /// Evidence supporting the suspicion
    pub evidence: Vec<String>,

    /// Severity level
    pub severity: SeverityLevel,
}

/// Types of suspicious activities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuspiciousActivityType {
    /// Statistical outlier in metrics
    StatisticalOutlier,

    /// Abnormal pattern in metrics
    PatternAnomaly,

    /// Potential gaming attempt
    GamingAttempt,

    /// Coordinated manipulation
    CoordinatedManipulation,

    /// Replay attack
    ReplayAttack,

    /// Invalid attestation
    InvalidAttestation,
}

/// Severity levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SeverityLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Recommended actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedAction {
    /// Action type
    pub action_type: ActionType,

    /// Target node(s)
    pub target_nodes: Vec<String>,

    /// Action priority
    pub priority: Priority,

    /// Action description
    pub description: String,
}

/// Action types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionType {
    /// Temporarily exclude node from consensus
    TemporaryExclusion,

    /// Reduce node's reputation weight
    ReduceReputationWeight,

    /// Request additional validation
    RequestValidation,

    /// Alert network administrators
    AlertAdministrators,

    /// Investigate further
    InvestigateFurther,
}

/// Priority levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Medium,
    High,
    Urgent,
}

/// Consensus events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsensusEvent {
    /// Consensus achieved
    ConsensusAchieved {
        consensus_metrics: ConsensusMetrics,
        participating_nodes: u32,
    },

    /// Consensus failed
    ConsensusFailed {
        reason: String,
        participating_nodes: u32,
    },

    /// Suspicious activity detected
    SuspiciousActivityDetected { activities: Vec<SuspiciousActivity> },

    /// Node excluded from consensus
    NodeExcluded { node_id: String, reason: String },

    /// Attestation verified
    AttestationVerified {
        node_id: String,
        attestation_hash: String,
    },
}

/// Attestation registry
pub struct AttestationRegistry {
    /// Stored attestations by node
    attestations: HashMap<String, Vec<MetricsAttestation>>,

    /// Attestation timestamps
    attestation_timestamps: HashMap<String, u64>,

    /// Nonce tracking for replay protection
    used_nonces: HashMap<String, Vec<String>>,
}

/// Statistical analysis engine
pub struct StatisticalAnalyzer {
    /// Historical data for analysis
    historical_data: Arc<RwLock<HashMap<String, Vec<f64>>>>,

    /// Analysis algorithms
    algorithms: Vec<AnalysisAlgorithm>,
}

/// Analysis algorithms
#[derive(Debug, Clone)]
pub enum AnalysisAlgorithm {
    /// Z-score based outlier detection
    ZScoreOutlierDetection,

    /// Interquartile range method
    InterquartileRangeMethod,

    /// Isolation forest
    IsolationForest,

    /// DBSCAN clustering
    DBSCANClustering,
}

/// Anomaly detection thresholds
#[derive(Debug, Clone)]
pub struct AnomalyThresholds {
    /// Z-score threshold
    pub z_score_threshold: f64,

    /// IQR multiplier
    pub iqr_multiplier: f64,

    /// Isolation forest contamination
    pub isolation_forest_contamination: f64,

    /// DBSCAN epsilon
    pub dbscan_epsilon: f64,
}

/// Node connection information
#[derive(Debug, Clone)]
pub struct NodeConnection {
    /// Node endpoint
    pub endpoint: String,

    /// Connection status
    pub status: ConnectionStatus,

    /// Last successful validation
    pub last_validation: Option<u64>,

    /// Validation success rate
    pub success_rate: f64,
}

/// Connection status
#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    Connected,
    Disconnected,
    Unreliable,
    Blacklisted,
}

impl Default for MetricsConsensusConfig {
    fn default() -> Self {
        Self {
            min_consensus_nodes: 3,
            max_byzantine_nodes: 1, // f < n/3, so with 3 nodes, max 1 Byzantine
            consensus_timeout_seconds: 30,
            min_reputation_threshold: 0.5,
            outlier_threshold: 2.0, // 2 standard deviations
            manipulation_sensitivity: 0.7,
            enable_vpos_attestation: true,
            enable_bft_aggregation: true,
            enable_manipulation_detection: true,
        }
    }
}

impl MetricsConsensusManager {
    /// Create a new metrics consensus manager
    pub async fn new(
        config: MetricsConsensusConfig,
        vpos_manager: Arc<VPoSManager>,
        node_identity: Arc<QuantumResistantDID>,
    ) -> Result<Self> {
        info!("🔒 Initializing Metrics Consensus Manager - Phase 5.4");

        // Create event broadcaster
        let (event_broadcaster, _) = broadcast::channel(1000);

        // Initialize VPoS attestation manager
        let vpos_attestation = Arc::new(
            VPoSMetricsAttestationManager::new(vpos_manager, node_identity.clone()).await?,
        );

        // Initialize Byzantine fault-tolerant aggregator
        let bft_aggregator = Arc::new(ByzantineFaultTolerantAggregator::new(
            config.min_consensus_nodes,
            config.max_byzantine_nodes,
            config.outlier_threshold,
        ));

        // Initialize manipulation detector
        let manipulation_detector =
            Arc::new(MetricsManipulationDetector::new(config.manipulation_sensitivity).await?);

        // Initialize cross-node validator
        let cross_node_validator = Arc::new(CrossNodeMetricsValidator::new(
            Duration::from_secs(config.consensus_timeout_seconds),
            config.min_consensus_nodes,
        ));

        Ok(Self {
            vpos_attestation,
            bft_aggregator,
            manipulation_detector,
            cross_node_validator,
            config,
            event_broadcaster,
        })
    }

    /// Start the metrics consensus system
    pub async fn start(&self) -> Result<()> {
        info!("🌟 Starting Metrics Consensus System - Phase 5.4");

        // Start VPoS attestation if enabled
        if self.config.enable_vpos_attestation {
            self.vpos_attestation.start().await?;
        }

        // Start BFT aggregation if enabled
        if self.config.enable_bft_aggregation {
            self.bft_aggregator.start().await?;
        }

        // Start manipulation detection if enabled
        if self.config.enable_manipulation_detection {
            self.manipulation_detector.start().await?;
        }

        // Start cross-node validation
        self.cross_node_validator.start().await?;

        info!("✅ Metrics Consensus System started successfully");
        Ok(())
    }

    /// Validate and reach consensus on network metrics
    pub async fn validate_network_metrics(
        &self,
        node_metrics: HashMap<String, NodeMetrics>,
    ) -> Result<ConsensusMetrics> {
        info!(
            "🔍 Validating network metrics from {} nodes",
            node_metrics.len()
        );

        // Step 1: Generate VPoS attestations for each node's metrics
        let mut attestated_metrics = Vec::new();
        for (node_id, metrics) in node_metrics {
            if let Ok(attestation) = self.generate_metrics_attestation(&node_id, &metrics).await {
                attestated_metrics.push(AttestatedNodeMetrics {
                    node_id: node_id.clone(),
                    metrics,
                    attestation,
                    reputation_score: self.get_node_reputation(&node_id).await?,
                    validation_timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                });
            }
        }

        // Step 2: Detect manipulation attempts
        let manipulation_result = self.detect_manipulation(&attestated_metrics).await?;
        if !manipulation_result.suspicious_activities.is_empty() {
            warn!(
                "🚨 Suspicious activities detected: {:?}",
                manipulation_result.suspicious_activities
            );

            // Broadcast suspicious activity event
            let _ = self
                .event_broadcaster
                .send(ConsensusEvent::SuspiciousActivityDetected {
                    activities: manipulation_result.suspicious_activities.clone(),
                });
        }

        // Step 3: Filter out suspicious nodes
        let filtered_metrics = self
            .filter_suspicious_nodes(attestated_metrics, &manipulation_result)
            .await?;

        // Step 4: Perform Byzantine fault-tolerant aggregation
        let consensus_metrics = self
            .bft_aggregator
            .aggregate_metrics(filtered_metrics)
            .await?;

        // Step 5: Broadcast consensus achievement
        let _ = self
            .event_broadcaster
            .send(ConsensusEvent::ConsensusAchieved {
                consensus_metrics: consensus_metrics.clone(),
                participating_nodes: consensus_metrics.participating_nodes,
            });

        info!(
            "✅ Network metrics consensus achieved with {} participating nodes",
            consensus_metrics.participating_nodes
        );

        Ok(consensus_metrics)
    }

    /// Generate VPoS attestation for node metrics
    async fn generate_metrics_attestation(
        &self,
        node_id: &str,
        metrics: &NodeMetrics,
    ) -> Result<MetricsAttestation> {
        self.vpos_attestation
            .generate_metrics_attestation(metrics, node_id)
            .await
    }

    /// Get node reputation score
    async fn get_node_reputation(&self, node_id: &str) -> Result<f64> {
        // For now, return a default score
        Ok(0.8) // Default reputation score
    }

    /// Detect manipulation in attestated metrics
    async fn detect_manipulation(
        &self,
        attestated_metrics: &[AttestatedNodeMetrics],
    ) -> Result<ManipulationDetectionResult> {
        let current_metrics: HashMap<String, NodeMetrics> = attestated_metrics
            .iter()
            .map(|am| (am.node_id.clone(), am.metrics.clone()))
            .collect();

        self.manipulation_detector
            .detect_manipulation(&current_metrics)
            .await
    }

    /// Filter out suspicious nodes from consensus
    async fn filter_suspicious_nodes(
        &self,
        attestated_metrics: Vec<AttestatedNodeMetrics>,
        manipulation_result: &ManipulationDetectionResult,
    ) -> Result<Vec<AttestatedNodeMetrics>> {
        let suspicious_nodes: Vec<String> = manipulation_result
            .suspicious_activities
            .iter()
            .filter(|activity| {
                matches!(
                    activity.severity,
                    SeverityLevel::High | SeverityLevel::Critical
                )
            })
            .map(|activity| activity.node_id.clone())
            .collect();

        let filtered_metrics: Vec<AttestatedNodeMetrics> = attestated_metrics
            .into_iter()
            .filter(|am| !suspicious_nodes.contains(&am.node_id))
            .collect();

        info!(
            "🔍 Filtered {} suspicious nodes from consensus",
            suspicious_nodes.len()
        );

        Ok(filtered_metrics)
    }

    /// Subscribe to consensus events
    pub fn subscribe_to_events(&self) -> broadcast::Receiver<ConsensusEvent> {
        self.event_broadcaster.subscribe()
    }
}

impl Default for AnomalyThresholds {
    fn default() -> Self {
        Self {
            z_score_threshold: 2.0,
            iqr_multiplier: 1.5,
            isolation_forest_contamination: 0.1,
            dbscan_epsilon: 0.5,
        }
    }
}

impl Default for ResourceUtilization {
    fn default() -> Self {
        Self {
            cpu_utilization: 0.0,
            memory_utilization: 0.0,
            storage_utilization: 0.0,
            network_utilization: 0.0,
            gpu_utilization: None,
        }
    }
}

impl AttestationRegistry {
    pub fn new() -> Self {
        Self {
            attestations: HashMap::new(),
            attestation_timestamps: HashMap::new(),
            used_nonces: HashMap::new(),
        }
    }

    pub fn store_attestation(
        &mut self,
        node_id: &str,
        attestation: MetricsAttestation,
    ) -> Result<()> {
        // Check for nonce reuse (replay attack protection)
        if let Some(nonces) = self.used_nonces.get(node_id) {
            if nonces.contains(&attestation.nonce) {
                return Err(anyhow::anyhow!(
                    "Nonce reuse detected - potential replay attack"
                ));
            }
        }

        // Store attestation
        self.attestations
            .entry(node_id.to_string())
            .or_insert_with(Vec::new)
            .push(attestation.clone());

        // Track nonce
        self.used_nonces
            .entry(node_id.to_string())
            .or_insert_with(Vec::new)
            .push(attestation.nonce.clone());

        // Update timestamp
        self.attestation_timestamps
            .insert(node_id.to_string(), attestation.timestamp);

        Ok(())
    }

    pub fn get_attestations(&self, node_id: &str) -> Option<&Vec<MetricsAttestation>> {
        self.attestations.get(node_id)
    }

    pub fn cleanup_old_attestations(&mut self, max_age_seconds: u64) {
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        self.attestations.retain(|node_id, attestations| {
            if let Some(&last_timestamp) = self.attestation_timestamps.get(node_id) {
                let age = current_time.saturating_sub(last_timestamp);
                if age > max_age_seconds {
                    // Also cleanup nonces
                    self.used_nonces.remove(node_id);
                    self.attestation_timestamps.remove(node_id);
                    false
                } else {
                    true
                }
            } else {
                false
            }
        });
    }
}

impl StatisticalAnalyzer {
    pub fn new() -> Self {
        Self {
            historical_data: Arc::new(RwLock::new(HashMap::new())),
            algorithms: vec![
                AnalysisAlgorithm::ZScoreOutlierDetection,
                AnalysisAlgorithm::InterquartileRangeMethod,
            ],
        }
    }

    pub async fn analyze_outliers(&self, node_id: &str, current_value: f64) -> Result<bool> {
        let historical_data = self.historical_data.read().await;

        if let Some(history) = historical_data.get(node_id) {
            if history.len() < 3 {
                return Ok(false); // Not enough data for analysis
            }

            // Calculate z-score
            let mean = history.iter().sum::<f64>() / history.len() as f64;
            let variance =
                history.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / history.len() as f64;
            let std_dev = variance.sqrt();

            if std_dev > 0.0 {
                let z_score = (current_value - mean) / std_dev;
                if z_score.abs() > 2.0 {
                    return Ok(true); // Outlier detected
                }
            }
        }

        Ok(false)
    }

    pub async fn update_historical_data(&self, node_id: &str, value: f64) -> Result<()> {
        let mut historical_data = self.historical_data.write().await;

        let history = historical_data
            .entry(node_id.to_string())
            .or_insert_with(Vec::new);

        history.push(value);

        // Keep only last 100 values
        if history.len() > 100 {
            history.remove(0);
        }

        Ok(())
    }
}

impl VPoSMetricsAttestationManager {
    /// Create a new VPoS metrics attestation manager
    pub async fn new(
        vpos_manager: Arc<VPoSManager>,
        node_identity: Arc<QuantumResistantDID>,
    ) -> Result<Self> {
        let attestation_registry = Arc::new(RwLock::new(AttestationRegistry::new()));

        Ok(Self {
            vpos_manager,
            attestation_registry,
            node_identity,
        })
    }

    /// Start the attestation manager
    pub async fn start(&self) -> Result<()> {
        info!("🔐 Starting VPoS Metrics Attestation Manager");

        // Start cleanup task for old attestations
        let registry = self.attestation_registry.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3600)); // Clean up every hour
            loop {
                interval.tick().await;
                let mut registry = registry.write().await;
                registry.cleanup_old_attestations(24 * 3600); // Keep attestations for 24 hours
            }
        });

        Ok(())
    }

    /// Generate VPoS attestation for metrics
    pub async fn generate_metrics_attestation(
        &self,
        metrics: &NodeMetrics,
        node_id: &str,
    ) -> Result<MetricsAttestation> {
        debug!("🔒 Generating VPoS attestation for node: {}", node_id);

        // Create metrics hash
        let metrics_hash = self.hash_metrics(metrics)?;

        // Generate nonce for replay protection
        let nonce = self.generate_nonce()?;

        // Create VPoS service proof for metrics
        let service_proof = self
            .create_metrics_service_proof(metrics, node_id, &nonce)
            .await?;

        // Create attestation
        let attestation = MetricsAttestation {
            node_did: node_id.to_string(),
            metrics_hash: metrics_hash.clone(),
            vpos_proof: service_proof,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: self.sign_attestation(&metrics_hash, &nonce)?,
            nonce,
        };

        // Store attestation
        {
            let mut registry = self.attestation_registry.write().await;
            registry.store_attestation(node_id, attestation.clone())?;
        }

        debug!("✅ VPoS attestation generated for node: {}", node_id);
        Ok(attestation)
    }

    /// Verify VPoS attestation
    pub async fn verify_attestation(&self, attestation: &MetricsAttestation) -> Result<bool> {
        // Verify VPoS proof
        let proof_valid = self
            .vpos_manager
            .verify_service_proof(&attestation.vpos_proof)
            .await?;

        if !proof_valid {
            warn!(
                "Invalid VPoS proof in attestation for node: {}",
                attestation.node_did
            );
            return Ok(false);
        }

        // Verify signature
        let signature_valid = self.verify_signature(
            &attestation.metrics_hash,
            &attestation.nonce,
            &attestation.signature,
            &attestation.node_did,
        )?;

        if !signature_valid {
            warn!(
                "Invalid signature in attestation for node: {}",
                attestation.node_did
            );
            return Ok(false);
        }

        // Verify timestamp (not too old, not in future)
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let age = current_time.saturating_sub(attestation.timestamp);
        if age > 3600 {
            // 1 hour max age
            warn!("Attestation too old for node: {}", attestation.node_did);
            return Ok(false);
        }

        if attestation.timestamp > current_time + 60 {
            // 1 minute future tolerance
            warn!(
                "Attestation timestamp in future for node: {}",
                attestation.node_did
            );
            return Ok(false);
        }

        Ok(true)
    }

    /// Hash metrics for attestation
    fn hash_metrics(&self, metrics: &NodeMetrics) -> Result<String> {
        let metrics_json = serde_json::to_string(metrics)?;
        let hash = Sha3_256::digest(metrics_json.as_bytes());
        Ok(format!("{:x}", hash))
    }

    /// Generate nonce for replay protection
    fn generate_nonce(&self) -> Result<String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let binding = uuid::Uuid::new_v4();
        let random_bytes = binding.as_bytes();
        let nonce_data = format!("{}:{:?}", timestamp, random_bytes);
        let nonce_hash = Sha3_256::digest(nonce_data.as_bytes());
        Ok(format!("{:x}", nonce_hash))
    }

    /// Create VPoS service proof for metrics
    async fn create_metrics_service_proof(
        &self,
        metrics: &NodeMetrics,
        node_id: &str,
        nonce: &str,
    ) -> Result<ServiceProof> {
        // For metrics attestation, we'll create a simplified service proof
        // that follows the VPoS structure but is specific to metrics collection

        let proof_id = Uuid::new_v4().to_string();
        let metrics_hash = self.hash_metrics(metrics)?;

        // Create computation proof for metrics collection
        let computation_proof = ComputationProof {
            input_hash: metrics_hash.clone(),
            output_hash: metrics_hash.clone(),
            execution_trace_hash: metrics_hash.clone(),
            computation_merkle_root: metrics_hash.clone(),
            challenge_response: crate::vpos::ChallengeResponse {
                challenge: format!("metrics_collection_{}", nonce),
                response: metrics_hash.clone(),
                challenge_timestamp: chrono::Utc::now(),
                response_proof: format!("proof_{}", nonce),
            },
            compute_units: 1,       // Minimal compute for metrics collection
            execution_time_ms: 100, // Minimal execution time
        };

        // Create resource proof - map from our ResourceUtilization to VPoS ResourceProof
        let resource_proof = ResourceProof {
            cpu_utilization: metrics.resource_utilization.cpu_utilization as f32,
            memory_used_mb: (metrics.resource_utilization.memory_utilization * 1024.0) as u64,
            gpu_utilization: metrics
                .resource_utilization
                .gpu_utilization
                .map(|u| u as f32),
            network_bandwidth_mbps: (metrics.resource_utilization.network_utilization * 100.0)
                as f32,
            energy_consumed_kwh: 0.001, // Minimal energy for metrics collection
            efficiency_score: 1.0,      // High efficiency for metrics collection
        };

        // Create quality metrics
        let quality_metrics = QualityMetrics {
            completion_rate: 1.0,
            avg_response_time_ms: 100,
            error_rate: 0.0,
            satisfaction_score: 1.0,
            availability: 1.0,
            security_score: 1.0,
        };

        // Create the service proof directly
        let mut service_proof = ServiceProof {
            proof_id: proof_id.clone(),
            task_id: format!("metrics_attestation_{}", nonce),
            provider_did: node_id.to_string(),
            requester_did: "spacekit_network".to_string(),
            service_type: ServiceType::Compute,
            computation_proof,
            resource_proof,
            quality_metrics,
            service_timestamp: chrono::Utc::now(),
            provider_signature: vec![],
            verification_hash: String::new(),
        };

        // Generate verification hash
        service_proof.verification_hash = self.generate_verification_hash(&service_proof)?;

        // Sign the proof with quantum-resistant signature
        let proof_data = serde_json::to_string(&service_proof)?;
        let signature = self
            .node_identity
            .sign_content(&proof_data)
            .map_err(|e| anyhow::anyhow!("Failed to sign: {}", e))?;
        service_proof.provider_signature = signature.as_bytes().to_vec();

        Ok(service_proof)
    }

    /// Generate verification hash for service proof
    fn generate_verification_hash(&self, proof: &ServiceProof) -> Result<String> {
        let hash_input = format!(
            "{}:{}:{}:{}:{}",
            proof.proof_id,
            proof.task_id,
            proof.provider_did,
            proof.requester_did,
            proof.service_timestamp.timestamp()
        );
        let hash = Sha3_256::digest(hash_input.as_bytes());
        Ok(format!("{:x}", hash))
    }

    /// Sign attestation
    fn sign_attestation(&self, metrics_hash: &str, nonce: &str) -> Result<Vec<u8>> {
        let data_to_sign = format!("{}:{}", metrics_hash, nonce);
        let signature = self
            .node_identity
            .sign_content(&data_to_sign)
            .map_err(|e| anyhow::anyhow!("Failed to sign attestation: {}", e))?;
        Ok(signature.as_bytes().to_vec())
    }

    /// Verify signature
    fn verify_signature(
        &self,
        metrics_hash: &str,
        nonce: &str,
        signature: &[u8],
        node_id: &str,
    ) -> Result<bool> {
        let data_to_verify = format!("{}:{}", metrics_hash, nonce);
        let signature_hex = hex::encode(signature);

        // In a real implementation, we'd look up the node's public key
        // For now, we'll use a simplified verification
        let verified = self
            .node_identity
            .verify_content(&data_to_verify, &signature_hex)
            .map_err(|e| anyhow::anyhow!("Failed to verify signature: {}", e))?;

        Ok(verified)
    }
}

impl ByzantineFaultTolerantAggregator {
    /// Create a new Byzantine fault-tolerant aggregator
    pub fn new(min_consensus_nodes: u32, max_byzantine_nodes: u32, outlier_threshold: f64) -> Self {
        Self {
            min_consensus_nodes,
            max_byzantine_nodes,
            outlier_threshold,
            reputation_weights: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start the BFT aggregator
    pub async fn start(&self) -> Result<()> {
        info!("🛡️ Starting Byzantine Fault-Tolerant Aggregator");
        Ok(())
    }

    /// Aggregate metrics with Byzantine fault tolerance
    pub async fn aggregate_metrics(
        &self,
        attestated_metrics: Vec<AttestatedNodeMetrics>,
    ) -> Result<ConsensusMetrics> {
        info!(
            "🔄 Aggregating metrics from {} nodes with BFT",
            attestated_metrics.len()
        );

        // Step 1: Check minimum consensus requirement
        if attestated_metrics.len() < self.min_consensus_nodes as usize {
            return Err(anyhow::anyhow!(
                "Insufficient nodes for consensus: {} < {}",
                attestated_metrics.len(),
                self.min_consensus_nodes
            ));
        }

        // Step 2: Filter outliers (potential Byzantine nodes)
        let filtered_metrics = self.filter_outliers(&attestated_metrics).await?;

        // Step 3: Update reputation weights
        self.update_reputation_weights(&filtered_metrics).await?;

        // Step 4: Perform weighted aggregation
        let aggregated_metrics = self.weighted_aggregation(&filtered_metrics).await?;

        // Step 5: Generate consensus proof
        let consensus_proof = self.generate_consensus_proof(&filtered_metrics).await?;

        // Step 6: Calculate confidence level
        let confidence_level = self
            .calculate_confidence_level(&filtered_metrics, &attestated_metrics)
            .await?;

        let consensus_metrics = ConsensusMetrics {
            aggregated_metrics,
            consensus_proof,
            participating_nodes: filtered_metrics.len() as u32,
            excluded_nodes: attestated_metrics.len() - filtered_metrics.len(),
            consensus_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            validity_period_seconds: 300, // 5 minutes validity
        };

        info!(
            "✅ BFT aggregation completed: {} participating, {} excluded",
            consensus_metrics.participating_nodes, consensus_metrics.excluded_nodes
        );

        Ok(consensus_metrics)
    }

    /// Filter outliers using statistical methods
    async fn filter_outliers(
        &self,
        attestated_metrics: &[AttestatedNodeMetrics],
    ) -> Result<Vec<AttestatedNodeMetrics>> {
        let mut filtered = Vec::new();

        // Extract values for statistical analysis
        let cpu_values: Vec<f64> = attestated_metrics
            .iter()
            .map(|am| am.metrics.resource_utilization.cpu_utilization)
            .collect();

        let memory_values: Vec<f64> = attestated_metrics
            .iter()
            .map(|am| am.metrics.resource_utilization.memory_utilization)
            .collect();

        let storage_values: Vec<f64> = attestated_metrics
            .iter()
            .map(|am| am.metrics.resource_utilization.storage_utilization)
            .collect();

        // Calculate statistics
        let cpu_stats = self.calculate_statistics(&cpu_values)?;
        let memory_stats = self.calculate_statistics(&memory_values)?;
        let storage_stats = self.calculate_statistics(&storage_values)?;

        // Filter based on outlier detection
        for (i, am) in attestated_metrics.iter().enumerate() {
            let cpu_outlier = self.is_outlier(cpu_values[i], &cpu_stats);
            let memory_outlier = self.is_outlier(memory_values[i], &memory_stats);
            let storage_outlier = self.is_outlier(storage_values[i], &storage_stats);

            // If more than one metric is an outlier, exclude the node
            let outlier_count = [cpu_outlier, memory_outlier, storage_outlier]
                .iter()
                .filter(|&&x| x)
                .count();

            if outlier_count <= 1 {
                filtered.push(am.clone());
            } else {
                warn!(
                    "🚨 Excluding node {} as potential Byzantine (outlier in {} metrics)",
                    am.node_id, outlier_count
                );
            }
        }

        Ok(filtered)
    }

    /// Calculate basic statistics
    fn calculate_statistics(&self, values: &[f64]) -> Result<StatisticalSummary> {
        if values.is_empty() {
            return Err(anyhow::anyhow!(
                "Cannot calculate statistics for empty dataset"
            ));
        }

        let mean = values.iter().sum::<f64>() / values.len() as f64;
        let variance =
            values.iter().map(|&x| (x - mean).powi(2)).sum::<f64>() / values.len() as f64;
        let std_dev = variance.sqrt();

        let mut sorted_values = values.to_vec();
        sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let median = if sorted_values.len() % 2 == 0 {
            (sorted_values[sorted_values.len() / 2 - 1] + sorted_values[sorted_values.len() / 2])
                / 2.0
        } else {
            sorted_values[sorted_values.len() / 2]
        };

        Ok(StatisticalSummary {
            mean,
            median,
            std_dev,
            min: sorted_values[0],
            max: sorted_values[sorted_values.len() - 1],
        })
    }

    /// Check if a value is an outlier
    fn is_outlier(&self, value: f64, stats: &StatisticalSummary) -> bool {
        if stats.std_dev == 0.0 {
            return false; // No variance, no outliers
        }

        let z_score = (value - stats.mean) / stats.std_dev;
        z_score.abs() > self.outlier_threshold
    }

    /// Update reputation weights
    async fn update_reputation_weights(
        &self,
        filtered_metrics: &[AttestatedNodeMetrics],
    ) -> Result<()> {
        let mut weights = self.reputation_weights.write().await;

        for am in filtered_metrics {
            weights.insert(am.node_id.clone(), am.reputation_score);
        }

        Ok(())
    }

    /// Perform weighted aggregation
    async fn weighted_aggregation(
        &self,
        filtered_metrics: &[AttestatedNodeMetrics],
    ) -> Result<NetworkMetrics> {
        let weights = self.reputation_weights.read().await;

        let mut weighted_network_utilization = 0.0;
        let mut weighted_storage_utilization = 0.0;
        let mut weighted_throughput = 0.0;
        let mut weighted_latency = 0.0;
        let mut weighted_error_rate = 0.0;
        let mut total_weight = 0.0;

        for am in filtered_metrics {
            let weight = weights.get(&am.node_id).unwrap_or(&1.0);

            weighted_network_utilization +=
                am.metrics.resource_utilization.network_utilization * weight;
            weighted_storage_utilization +=
                am.metrics.resource_utilization.storage_utilization * weight;
            weighted_throughput += am.metrics.performance_metrics.throughput_ops_per_sec * weight;
            weighted_latency += am.metrics.performance_metrics.latency_p50_ms * weight;
            weighted_error_rate += am.metrics.performance_metrics.error_rate_percent * weight;
            total_weight += weight;
        }

        if total_weight == 0.0 {
            return Err(anyhow::anyhow!("Total weight is zero"));
        }

        let aggregated_performance = PerformanceMetrics {
            throughput_ops_per_sec: weighted_throughput / total_weight,
            latency_p50_ms: weighted_latency / total_weight,
            latency_p95_ms: weighted_latency / total_weight * 1.5, // Estimate
            latency_p99_ms: weighted_latency / total_weight * 2.0, // Estimate
            error_rate_percent: weighted_error_rate / total_weight,
            availability_percent: 99.9, // High availability expected
            resource_utilization_percent: (weighted_network_utilization
                + weighted_storage_utilization)
                / (2.0 * total_weight)
                * 100.0,
            cost_per_operation: 0.001, // Placeholder
            last_updated: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        // Calculate health score
        let health_score = self.calculate_health_score(&aggregated_performance)?;

        Ok(NetworkMetrics {
            network_utilization: weighted_network_utilization / total_weight,
            storage_utilization: weighted_storage_utilization / total_weight,
            performance_metrics: aggregated_performance,
            health_score,
            confidence_level: 0.95, // Will be calculated properly
        })
    }

    /// Calculate network health score
    fn calculate_health_score(&self, performance: &PerformanceMetrics) -> Result<f64> {
        // Health score based on multiple factors
        let throughput_score = (performance.throughput_ops_per_sec / 1000.0).min(1.0);
        let latency_score = (1.0 - (performance.latency_p50_ms / 1000.0)).max(0.0);
        let error_score = (1.0 - (performance.error_rate_percent / 100.0)).max(0.0);
        let availability_score = performance.availability_percent / 100.0;

        let health_score = (throughput_score * 0.3
            + latency_score * 0.3
            + error_score * 0.2
            + availability_score * 0.2)
            .min(1.0)
            .max(0.0);

        Ok(health_score)
    }

    /// Generate consensus proof
    async fn generate_consensus_proof(
        &self,
        filtered_metrics: &[AttestatedNodeMetrics],
    ) -> Result<ConsensusProof> {
        let node_attestations: Vec<MetricsAttestation> = filtered_metrics
            .iter()
            .map(|am| am.attestation.clone())
            .collect();

        // Generate merkle root of attestations
        let attestation_hashes: Vec<String> = node_attestations
            .iter()
            .map(|attestation| attestation.metrics_hash.clone())
            .collect();

        let merkle_root = self.generate_merkle_root(&attestation_hashes)?;

        let threshold_achieved = filtered_metrics.len() as f64
            / (filtered_metrics.len() + self.max_byzantine_nodes as usize) as f64;

        Ok(ConsensusProof {
            algorithm: ConsensusAlgorithm::HybridConsensus,
            node_attestations,
            aggregation_method: AggregationMethod::ReputationWeighted,
            threshold_achieved,
            bft_level: self.max_byzantine_nodes,
            attestation_merkle_root: merkle_root,
        })
    }

    /// Generate merkle root for attestations
    fn generate_merkle_root(&self, hashes: &[String]) -> Result<String> {
        if hashes.is_empty() {
            return Ok("0".repeat(64));
        }

        let mut current_level = hashes.to_vec();

        while current_level.len() > 1 {
            let mut next_level = Vec::new();

            for chunk in current_level.chunks(2) {
                let combined = if chunk.len() == 2 {
                    format!("{}{}", chunk[0], chunk[1])
                } else {
                    chunk[0].clone()
                };
                let hash = Sha3_256::digest(combined.as_bytes());
                next_level.push(format!("{:x}", hash));
            }

            current_level = next_level;
        }

        Ok(current_level[0].clone())
    }

    /// Calculate confidence level
    async fn calculate_confidence_level(
        &self,
        filtered_metrics: &[AttestatedNodeMetrics],
        original_metrics: &[AttestatedNodeMetrics],
    ) -> Result<f64> {
        let participation_rate = filtered_metrics.len() as f64 / original_metrics.len() as f64;
        let reputation_weight = filtered_metrics
            .iter()
            .map(|am| am.reputation_score)
            .sum::<f64>()
            / filtered_metrics.len() as f64;

        let confidence = (participation_rate * 0.6 + reputation_weight * 0.4)
            .min(1.0)
            .max(0.0);

        Ok(confidence)
    }
}

/// Statistical summary for outlier detection
#[derive(Debug, Clone)]
struct StatisticalSummary {
    mean: f64,
    median: f64,
    std_dev: f64,
    min: f64,
    max: f64,
}

impl MetricsManipulationDetector {
    /// Create a new manipulation detector
    pub async fn new(sensitivity: f64) -> Result<Self> {
        Ok(Self {
            historical_metrics: Arc::new(RwLock::new(HashMap::new())),
            statistical_analyzer: Arc::new(StatisticalAnalyzer::new()),
            anomaly_thresholds: AnomalyThresholds::default(),
            sensitivity,
        })
    }

    /// Start the manipulation detector
    pub async fn start(&self) -> Result<()> {
        info!("🔍 Starting Metrics Manipulation Detector");
        Ok(())
    }

    /// Detect manipulation in metrics
    pub async fn detect_manipulation(
        &self,
        current_metrics: &HashMap<String, NodeMetrics>,
    ) -> Result<ManipulationDetectionResult> {
        debug!(
            "🔍 Analyzing {} nodes for manipulation",
            current_metrics.len()
        );

        let mut suspicious_activities = Vec::new();

        for (node_id, metrics) in current_metrics {
            // Statistical outlier detection
            if let Ok(outlier_activities) = self.detect_statistical_outliers(node_id, metrics).await
            {
                suspicious_activities.extend(outlier_activities);
            }

            // Pattern anomaly detection
            if let Ok(pattern_activities) = self.detect_pattern_anomalies(node_id, metrics).await {
                suspicious_activities.extend(pattern_activities);
            }

            // Gaming attempt detection
            if let Ok(gaming_activities) = self.detect_gaming_attempts(node_id, metrics).await {
                suspicious_activities.extend(gaming_activities);
            }
        }

        // Calculate overall trust score
        let trust_score = self
            .calculate_trust_score(current_metrics, &suspicious_activities)
            .await?;

        // Generate recommended actions
        let recommended_actions = self
            .generate_recommended_actions(&suspicious_activities)
            .await?;

        Ok(ManipulationDetectionResult {
            suspicious_activities,
            overall_trust_score: trust_score,
            recommended_actions,
            detection_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })
    }

    /// Detect statistical outliers
    async fn detect_statistical_outliers(
        &self,
        node_id: &str,
        metrics: &NodeMetrics,
    ) -> Result<Vec<SuspiciousActivity>> {
        let mut activities = Vec::new();

        // Check CPU utilization
        if self
            .statistical_analyzer
            .analyze_outliers(
                &format!("{}_cpu", node_id),
                metrics.resource_utilization.cpu_utilization,
            )
            .await?
        {
            activities.push(SuspiciousActivity {
                node_id: node_id.to_string(),
                activity_type: SuspiciousActivityType::StatisticalOutlier,
                confidence: 0.8,
                evidence: vec![
                    "CPU utilization significantly deviates from historical pattern".to_string(),
                ],
                severity: SeverityLevel::Medium,
            });
        }

        // Check memory utilization
        if self
            .statistical_analyzer
            .analyze_outliers(
                &format!("{}_memory", node_id),
                metrics.resource_utilization.memory_utilization,
            )
            .await?
        {
            activities.push(SuspiciousActivity {
                node_id: node_id.to_string(),
                activity_type: SuspiciousActivityType::StatisticalOutlier,
                confidence: 0.8,
                evidence: vec![
                    "Memory utilization significantly deviates from historical pattern".to_string(),
                ],
                severity: SeverityLevel::Medium,
            });
        }

        // Update historical data
        self.statistical_analyzer
            .update_historical_data(
                &format!("{}_cpu", node_id),
                metrics.resource_utilization.cpu_utilization,
            )
            .await?;

        self.statistical_analyzer
            .update_historical_data(
                &format!("{}_memory", node_id),
                metrics.resource_utilization.memory_utilization,
            )
            .await?;

        Ok(activities)
    }

    /// Detect pattern anomalies
    async fn detect_pattern_anomalies(
        &self,
        node_id: &str,
        metrics: &NodeMetrics,
    ) -> Result<Vec<SuspiciousActivity>> {
        let mut activities = Vec::new();

        // Check for impossible combinations
        if metrics.resource_utilization.cpu_utilization > 99.0
            && metrics.resource_utilization.memory_utilization > 99.0
            && metrics.performance_metrics.error_rate_percent < 0.1
        {
            activities.push(SuspiciousActivity {
                node_id: node_id.to_string(),
                activity_type: SuspiciousActivityType::PatternAnomaly,
                confidence: 0.9,
                evidence: vec![
                    "Impossible combination: 99%+ CPU and memory with <0.1% error rate".to_string(),
                ],
                severity: SeverityLevel::High,
            });
        }

        // Check for sudden performance jumps
        if metrics.performance_metrics.throughput_ops_per_sec > 10000.0
            && metrics.performance_metrics.latency_p50_ms < 1.0
        {
            activities.push(SuspiciousActivity {
                node_id: node_id.to_string(),
                activity_type: SuspiciousActivityType::PatternAnomaly,
                confidence: 0.85,
                evidence: vec![
                    "Unrealistic performance: >10k ops/sec with <1ms latency".to_string()
                ],
                severity: SeverityLevel::High,
            });
        }

        Ok(activities)
    }

    /// Detect gaming attempts
    async fn detect_gaming_attempts(
        &self,
        node_id: &str,
        metrics: &NodeMetrics,
    ) -> Result<Vec<SuspiciousActivity>> {
        let mut activities = Vec::new();

        // Check for artificially high utilization
        if metrics.resource_utilization.cpu_utilization > 95.0
            && metrics.resource_utilization.memory_utilization > 95.0
            && metrics.resource_utilization.storage_utilization > 95.0
            && metrics.performance_metrics.availability_percent > 99.9
        {
            activities.push(SuspiciousActivity {
                node_id: node_id.to_string(),
                activity_type: SuspiciousActivityType::GamingAttempt,
                confidence: 0.95,
                evidence: vec![
                    "Suspicious pattern: All resources >95% utilized with >99.9% availability"
                        .to_string(),
                ],
                severity: SeverityLevel::Critical,
            });
        }

        // Check for perfect metrics (likely fake)
        if metrics.performance_metrics.error_rate_percent == 0.0
            && metrics.performance_metrics.availability_percent == 100.0
            && metrics.performance_metrics.throughput_ops_per_sec % 1000.0 == 0.0
        {
            activities.push(SuspiciousActivity {
                node_id: node_id.to_string(),
                activity_type: SuspiciousActivityType::GamingAttempt,
                confidence: 0.7,
                evidence: vec![
                    "Perfect metrics detected: 0% error, 100% availability, round numbers"
                        .to_string(),
                ],
                severity: SeverityLevel::Medium,
            });
        }

        Ok(activities)
    }

    /// Calculate overall trust score
    async fn calculate_trust_score(
        &self,
        _current_metrics: &HashMap<String, NodeMetrics>,
        suspicious_activities: &[SuspiciousActivity],
    ) -> Result<f64> {
        let mut trust_score = 1.0;

        for activity in suspicious_activities {
            let penalty = match activity.severity {
                SeverityLevel::Low => 0.05,
                SeverityLevel::Medium => 0.15,
                SeverityLevel::High => 0.35,
                SeverityLevel::Critical => 0.5,
            };

            trust_score -= penalty * activity.confidence;
        }

        Ok(trust_score.max(0.0))
    }

    /// Generate recommended actions
    async fn generate_recommended_actions(
        &self,
        suspicious_activities: &[SuspiciousActivity],
    ) -> Result<Vec<RecommendedAction>> {
        let mut actions = Vec::new();

        for activity in suspicious_activities {
            let action = match activity.severity {
                SeverityLevel::Critical => RecommendedAction {
                    action_type: ActionType::TemporaryExclusion,
                    target_nodes: vec![activity.node_id.clone()],
                    priority: Priority::Urgent,
                    description: format!(
                        "Temporarily exclude {} due to critical suspicious activity",
                        activity.node_id
                    ),
                },
                SeverityLevel::High => RecommendedAction {
                    action_type: ActionType::ReduceReputationWeight,
                    target_nodes: vec![activity.node_id.clone()],
                    priority: Priority::High,
                    description: format!(
                        "Reduce reputation weight for {} due to high suspicious activity",
                        activity.node_id
                    ),
                },
                SeverityLevel::Medium => RecommendedAction {
                    action_type: ActionType::RequestValidation,
                    target_nodes: vec![activity.node_id.clone()],
                    priority: Priority::Medium,
                    description: format!(
                        "Request additional validation for {} due to suspicious activity",
                        activity.node_id
                    ),
                },
                SeverityLevel::Low => RecommendedAction {
                    action_type: ActionType::InvestigateFurther,
                    target_nodes: vec![activity.node_id.clone()],
                    priority: Priority::Low,
                    description: format!(
                        "Monitor {} for further suspicious activity",
                        activity.node_id
                    ),
                },
            };

            actions.push(action);
        }

        Ok(actions)
    }
}

impl CrossNodeMetricsValidator {
    /// Create a new cross-node validator
    pub fn new(validation_timeout: Duration, min_validation_responses: u32) -> Self {
        Self {
            connected_nodes: Arc::new(RwLock::new(HashMap::new())),
            validation_timeout,
            min_validation_responses,
        }
    }

    /// Start the validator
    pub async fn start(&self) -> Result<()> {
        info!("🌐 Starting Cross-Node Metrics Validator");
        Ok(())
    }

    /// Add a node connection
    pub async fn add_node_connection(&self, node_id: String, endpoint: String) -> Result<()> {
        let mut connections = self.connected_nodes.write().await;
        connections.insert(
            node_id.clone(),
            NodeConnection {
                endpoint,
                status: ConnectionStatus::Connected,
                last_validation: None,
                success_rate: 1.0,
            },
        );
        Ok(())
    }

    /// Validate metrics across nodes
    pub async fn validate_metrics_across_nodes(
        &self,
        node_id: &str,
        metrics: &NodeMetrics,
    ) -> Result<bool> {
        let connections = self.connected_nodes.read().await;

        if connections.len() < self.min_validation_responses as usize {
            warn!("Insufficient connected nodes for validation");
            return Ok(true); // Allow if insufficient nodes
        }

        let mut validation_tasks = Vec::new();

        for (validator_node_id, connection) in connections.iter() {
            if validator_node_id != node_id
                && matches!(connection.status, ConnectionStatus::Connected)
            {
                let task = self.request_validation(validator_node_id, connection, node_id, metrics);
                validation_tasks.push(task);
            }
        }

        if validation_tasks.is_empty() {
            return Ok(true); // No validators available
        }

        // Wait for validation responses with timeout
        let timeout = tokio::time::timeout(
            self.validation_timeout,
            futures::future::join_all(validation_tasks),
        );

        match timeout.await {
            Ok(results) => {
                let successful_validations = results
                    .into_iter()
                    .filter(|r| r.is_ok() && *r.as_ref().unwrap())
                    .count();

                let validation_threshold = (connections.len() as f64 * 0.6) as usize;
                Ok(successful_validations >= validation_threshold)
            }
            Err(_) => {
                warn!("Validation timeout for node {}", node_id);
                Ok(false)
            }
        }
    }

    /// Request validation from a specific node
    async fn request_validation(
        &self,
        _validator_node_id: &str,
        _connection: &NodeConnection,
        _target_node_id: &str,
        _metrics: &NodeMetrics,
    ) -> Result<bool> {
        // TODO: In a real implementation, this would make an HTTP request to the validator node
        // For now, we'll simulate a successful validation
        tokio::time::sleep(Duration::from_millis(100)).await;
        Ok(true)
    }
}

// Helper function to convert ResourceMetrics to ResourceUtilization
pub fn convert_resource_metrics_to_utilization(
    resource_metrics: &ResourceMetrics,
) -> ResourceUtilization {
    // TODO: Review the ResourceMetrics struct and convert it to ResourceUtilization
    ResourceUtilization {
        cpu_utilization: resource_metrics.cpu_usage_percent as f64,
        memory_utilization: (resource_metrics.memory_usage_mb as f64 / 8192.0) * 100.0, // Assume 8GB max
        storage_utilization: 50.0, // Placeholder - would need actual storage info
        network_utilization: 30.0, // Placeholder - would need actual network info
        gpu_utilization: None,     // Not available in ResourceMetrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    // Default implementation for ServiceProof for testing
    impl Default for ServiceProof {
        fn default() -> Self {
            Self {
                proof_id: "test_proof".to_string(),
                task_id: "test_task".to_string(),
                provider_did: "test_provider".to_string(),
                requester_did: "test_requester".to_string(),
                service_type: ServiceType::Compute,
                computation_proof: ComputationProof {
                    input_hash: "test_input".to_string(),
                    output_hash: "test_output".to_string(),
                    execution_trace_hash: "test_trace".to_string(),
                    computation_merkle_root: "test_merkle".to_string(),
                    challenge_response: crate::vpos::ChallengeResponse {
                        challenge: "test_challenge".to_string(),
                        response: "test_response".to_string(),
                        challenge_timestamp: chrono::Utc::now(),
                        response_proof: "test_proof".to_string(),
                    },
                    compute_units: 1,
                    execution_time_ms: 100,
                },
                resource_proof: ResourceProof {
                    cpu_utilization: 50.0,
                    memory_used_mb: 512,
                    gpu_utilization: None,
                    network_bandwidth_mbps: 100.0,
                    energy_consumed_kwh: 0.001,
                    efficiency_score: 0.8,
                },
                quality_metrics: QualityMetrics {
                    completion_rate: 1.0,
                    avg_response_time_ms: 100,
                    error_rate: 0.0,
                    satisfaction_score: 1.0,
                    availability: 1.0,
                    security_score: 1.0,
                },
                service_timestamp: chrono::Utc::now(),
                provider_signature: vec![],
                verification_hash: "test_verification".to_string(),
            }
        }
    }

    #[tokio::test]
    async fn test_metrics_consensus_manager() {
        // This would require setting up VPoS manager and node identity
        // For now, just test the basic structure
        let config = MetricsConsensusConfig::default();
        assert_eq!(config.min_consensus_nodes, 3);
        assert_eq!(config.max_byzantine_nodes, 1);
    }

    #[tokio::test]
    async fn test_attestation_registry() {
        let mut registry = AttestationRegistry::new();

        let attestation = MetricsAttestation {
            node_did: "test_node".to_string(),
            metrics_hash: "test_hash".to_string(),
            vpos_proof: ServiceProof::default(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: vec![1, 2, 3],
            nonce: "test_nonce".to_string(),
        };

        registry
            .store_attestation("test_node", attestation)
            .unwrap();

        let stored = registry.get_attestations("test_node").unwrap();
        assert_eq!(stored.len(), 1);

        // Test nonce reuse detection
        let duplicate_attestation = MetricsAttestation {
            node_did: "test_node".to_string(),
            metrics_hash: "test_hash_2".to_string(),
            vpos_proof: ServiceProof::default(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            signature: vec![4, 5, 6],
            nonce: "test_nonce".to_string(), // Same nonce
        };

        let result = registry.store_attestation("test_node", duplicate_attestation);
        assert!(result.is_err()); // Should fail due to nonce reuse
    }

    #[tokio::test]
    async fn test_statistical_analyzer() {
        let analyzer = StatisticalAnalyzer::new();

        // Add some historical data
        for i in 0..10 {
            analyzer
                .update_historical_data("test_node", i as f64)
                .await
                .unwrap();
        }

        // Test outlier detection
        let is_outlier = analyzer.analyze_outliers("test_node", 100.0).await.unwrap();
        assert!(is_outlier); // 100 should be an outlier for data 0-9

        let is_normal = analyzer.analyze_outliers("test_node", 5.0).await.unwrap();
        assert!(!is_normal); // 5 should be normal for data 0-9
    }
}
