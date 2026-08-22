//! Simulation Interface for Production Recovery System Testing
//! 
//! This module provides interfaces between the behavioral simulation system
//! and the production recovery system, enabling comprehensive testing of
//! behavioral cryptography recovery mechanisms.

pub mod recovery_simulation;
pub mod archetype_testing;
pub mod performance_benchmarks;

use crate::{
    BehavioralRecoverySystem,
    behavioral::{BehavioralPatterns, ConfidenceScore},
    recovery::RecoveryOrchestrator,
    zkp::BehavioralZKSystem,
};
use spacekit_primitives::v1::behavioral_types::{
    UserArchetype, PersonalityTraits, ArchetypeBehavioralExpectations
};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;


/// Main simulation coordinator that integrates behavioral simulation with production recovery
pub struct BehavioralRecoverySimulationCoordinator {
    /// Production recovery system
    recovery_system: BehavioralRecoverySystem,
    /// ZK proof system for privacy-preserving verification
    zk_system: BehavioralZKSystem,
    /// Recovery orchestrator for managing sessions
    recovery_orchestrator: RecoveryOrchestrator,
    /// Simulation configuration
    config: SimulationConfig,
    /// Test scenarios by archetype
    archetype_scenarios: HashMap<UserArchetype, Vec<ArchetypeTestScenario>>,
    /// Performance benchmarks
    _performance_metrics: PerformanceMetrics,
}

/// Configuration for production recovery testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationConfig {
    /// Number of test users per archetype
    pub users_per_archetype: usize,
    /// Number of recovery attempts to simulate
    pub recovery_attempts_per_user: usize,
    /// Test duration in days
    pub simulation_duration_days: u64,
    /// Enable ZK proof verification
    pub enable_zkp_verification: bool,
    /// Enable differential privacy
    pub enable_differential_privacy: bool,
    /// Privacy budget parameters
    pub epsilon: f64,
    pub delta: f64,
    /// Fraud simulation percentage
    pub fraud_simulation_rate: f64,
    /// Performance benchmarking enabled
    pub enable_performance_benchmarks: bool,
}

/// Test scenario for specific archetype
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeTestScenario {
    /// User archetype being tested
    pub archetype: UserArchetype,
    /// User's personality traits
    pub personality: PersonalityTraits,
    /// Behavioral patterns to simulate
    pub behavioral_patterns: BehavioralPatterns,
    /// Expected confidence score range
    pub expected_confidence_range: (f64, f64),
    /// Expected recovery success rate
    pub expected_recovery_rate: f64,
    /// Test complexity level
    pub complexity_level: TestComplexity,
    /// Fraud scenario (if applicable)
    pub fraud_scenario: Option<FraudScenario>,
}

/// Test complexity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TestComplexity {
    /// Basic behavioral patterns, single service
    Basic,
    /// Multiple services, moderate complexity
    Intermediate,
    /// Complex cross-chain behavior, multiple patterns
    Advanced,
    /// Comprehensive testing with edge cases
    Expert,
}

/// Fraud simulation scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudScenario {
    /// Type of fraud being simulated
    pub fraud_type: FraudType,
    /// Intensity of fraudulent behavior (0.0-1.0)
    pub intensity: f64,
    /// Duration of fraud simulation
    pub duration_days: u64,
    /// Expected detection rate
    pub expected_detection_rate: f64,
}

/// Types of fraud to simulate
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FraudType {
    /// Sybil attack simulation
    SybilAttack,
    /// Behavioral pattern mimicking
    PatternMimicking,
    /// Identity theft attempt
    IdentityTheft,
    /// Economic manipulation
    EconomicManipulation,
    /// Cross-chain fraud
    CrossChainFraud,
    /// Reputation manipulation
    ReputationManipulation,
}

/// Comprehensive simulation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResults {
    /// Overall simulation configuration
    pub config: SimulationConfig,
    /// Results by archetype
    pub archetype_results: HashMap<UserArchetype, ArchetypeResults>,
    /// Overall performance metrics
    pub performance_metrics: PerformanceMetrics,
    /// ZK proof verification results
    pub zkp_results: Option<ZKProofResults>,
    /// Fraud detection results
    pub fraud_detection_results: FraudDetectionResults,
    /// Recovery system statistics
    pub recovery_statistics: RecoveryStatistics,
    /// Execution time and resource usage
    pub execution_metrics: ExecutionMetrics,
    /// Comparison with simulation-only results
    pub simulation_comparison: SimulationComparison,
}

/// Results for specific archetype testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchetypeResults {
    /// Archetype tested
    pub archetype: UserArchetype,
    /// Number of test scenarios executed
    pub scenarios_executed: usize,
    /// Overall success rate
    pub success_rate: f64,
    /// Average confidence score achieved
    pub average_confidence: f64,
    /// Average recovery time
    pub average_recovery_time_ms: u64,
    /// ZK proof verification success rate
    pub zkp_success_rate: f64,
    /// Fraud detection effectiveness
    pub fraud_detection_rate: f64,
    /// Performance by complexity level
    pub complexity_performance: HashMap<TestComplexity, ComplexityResults>,
    /// Detailed test scenarios
    pub test_scenarios: Vec<ScenarioResult>,
}

/// Performance metrics for the production recovery system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Average time to generate behavioral patterns (ms)
    pub avg_pattern_generation_time: u64,
    /// Average time to calculate confidence score (ms)
    pub avg_confidence_calculation_time: u64,
    /// Average time to generate ZK proofs (ms)
    pub avg_zkp_generation_time: u64,
    /// Average time to verify ZK proofs (ms)
    pub avg_zkp_verification_time: u64,
    /// Average recovery session duration (ms)
    pub avg_recovery_session_duration: u64,
    /// Memory usage statistics
    pub memory_usage: MemoryUsage,
    /// Throughput measurements
    pub throughput: ThroughputMetrics,
}

/// ZK proof verification results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZKProofResults {
    /// Total proofs generated
    pub total_proofs_generated: u64,
    /// Successful proof generations
    pub successful_generations: u64,
    /// Total proofs verified
    pub total_proofs_verified: u64,
    /// Successful verifications
    pub successful_verifications: u64,
    /// Average proof size in bytes
    pub average_proof_size: usize,
    /// Average proof generation time
    pub average_generation_time_ms: u64,
    /// Average verification time
    pub average_verification_time_ms: u64,
    /// Privacy budget consumption
    pub privacy_budget_used: f64,
}

/// Fraud detection effectiveness results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FraudDetectionResults {
    /// Total fraud scenarios tested
    pub total_fraud_scenarios: usize,
    /// Successfully detected fraud attempts
    pub detected_fraud_attempts: usize,
    /// False positive rate
    pub false_positive_rate: f64,
    /// False negative rate
    pub false_negative_rate: f64,
    /// Detection accuracy by fraud type
    pub detection_by_fraud_type: HashMap<FraudType, f64>,
    /// Average detection time
    pub average_detection_time_ms: u64,
}

/// Recovery system statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryStatistics {
    /// Total recovery sessions initiated
    pub total_sessions: usize,
    /// Successful recoveries
    pub successful_recoveries: usize,
    /// Failed recoveries
    pub failed_recoveries: usize,
    /// Average session duration
    pub average_session_duration_ms: u64,
    /// Recovery success rate by archetype
    pub success_rate_by_archetype: HashMap<UserArchetype, f64>,
    /// Challenge response accuracy
    pub challenge_response_accuracy: f64,
    /// Byzantine fault tolerance effectiveness
    pub byzantine_tolerance_effectiveness: f64,
}

/// Memory and resource usage metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsage {
    /// Peak memory usage in MB
    pub peak_memory_mb: u64,
    /// Average memory usage in MB
    pub average_memory_mb: u64,
    /// Memory usage by component
    pub component_memory_usage: HashMap<String, u64>,
}

/// Throughput performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputMetrics {
    /// Recovery sessions per second
    pub sessions_per_second: f64,
    /// Confidence calculations per second
    pub confidence_calculations_per_second: f64,
    /// ZK proofs per second
    pub zkp_proofs_per_second: f64,
    /// Pattern analyses per second
    pub pattern_analyses_per_second: f64,
}

/// Execution time and resource metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    /// Total simulation execution time
    pub total_execution_time_ms: u64,
    /// CPU utilization percentage
    pub cpu_utilization: f64,
    /// Disk I/O operations
    pub disk_io_operations: u64,
    /// Network operations (if applicable)
    pub network_operations: u64,
}

/// Comparison between simulation-only and production recovery results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationComparison {
    /// Simulation-only success rate
    pub simulation_success_rate: f64,
    /// Production recovery success rate
    pub production_success_rate: f64,
    /// Difference in confidence scores
    pub confidence_score_difference: f64,
    /// Performance overhead of production system
    pub performance_overhead_factor: f64,
    /// Accuracy improvement with ZK proofs
    pub zkp_accuracy_improvement: f64,
}

/// Results for specific complexity level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComplexityResults {
    /// Complexity level tested
    pub complexity: TestComplexity,
    /// Number of scenarios at this complexity
    pub scenario_count: usize,
    /// Success rate for this complexity
    pub success_rate: f64,
    /// Average recovery time
    pub average_time_ms: u64,
    /// Resource usage factor compared to basic
    pub resource_usage_factor: f64,
}

/// Individual scenario test result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// Scenario identifier
    pub scenario_id: String,
    /// Archetype tested
    pub archetype: UserArchetype,
    /// Test complexity
    pub complexity: TestComplexity,
    /// Recovery success
    pub recovery_success: bool,
    /// Confidence score achieved
    pub confidence_score: f64, 
    /// Recovery time in milliseconds
    pub recovery_time_ms: u64,
    /// ZK proof verification success
    pub zkp_verification_success: bool,
    /// Fraud detection result (if fraud scenario)
    pub fraud_detected: Option<bool>,
    /// Detailed performance metrics
    pub performance_breakdown: HashMap<String, u64>,
}

impl BehavioralRecoverySimulationCoordinator {
    /// Create new simulation coordinator
    pub async fn new(config: SimulationConfig) -> Result<Self> {
        let recovery_system = BehavioralRecoverySystem::new(config.epsilon, config.delta);
        let zk_system = BehavioralZKSystem::new();
        let recovery_orchestrator = RecoveryOrchestrator::new(
            0.7,   // recovery_threshold
            0.8,   // consensus_threshold  
            100    // network_size
        );
        
        Ok(Self {
            recovery_system,
            zk_system,
            recovery_orchestrator,
            config,
            archetype_scenarios: HashMap::new(),
            _performance_metrics: PerformanceMetrics::default(),
        })
    }
    
    /// Generate test scenarios for all archetypes
    pub fn generate_archetype_scenarios(&mut self) -> Result<()> {
        for archetype in [
            UserArchetype::BaseUser,
            UserArchetype::Validator,
            UserArchetype::Developer,
            UserArchetype::Researcher,
            UserArchetype::Investor,
            UserArchetype::Regulator,
            UserArchetype::Other,
        ] {
            let scenarios = self.generate_scenarios_for_archetype(&archetype)?;
            self.archetype_scenarios.insert(archetype, scenarios);
        }
        
        Ok(())
    }
    
    /// Run comprehensive simulation testing production recovery system
    pub async fn run_comprehensive_simulation(&mut self) -> Result<SimulationResults> {
        let start_time = std::time::Instant::now();
        
        // Generate test scenarios
        self.generate_archetype_scenarios()?;
        
        // Initialize results
        let mut results = SimulationResults {
            config: self.config.clone(),
            archetype_results: HashMap::new(),
            performance_metrics: PerformanceMetrics::default(),
            zkp_results: None,
            fraud_detection_results: FraudDetectionResults::default(),
            recovery_statistics: RecoveryStatistics::default(),
            execution_metrics: ExecutionMetrics::default(),
            simulation_comparison: SimulationComparison::default(),
        };
        
        // Test each archetype
        let archetype_scenarios_clone = self.archetype_scenarios.clone();
        for (archetype, scenarios) in &archetype_scenarios_clone {
            let archetype_result = self.test_archetype_scenarios(archetype, scenarios).await?;
            results.archetype_results.insert(archetype.clone(), archetype_result);
        }
        
        // Collect overall metrics
        results.performance_metrics = self.collect_performance_metrics().await?;
        
        if self.config.enable_zkp_verification {
            results.zkp_results = Some(self.collect_zkp_metrics().await?);
        }
        
        results.fraud_detection_results = self.collect_fraud_detection_metrics().await?;
        results.recovery_statistics = self.collect_recovery_statistics().await?;
        
        // Calculate execution metrics
        let execution_time = start_time.elapsed().as_millis() as u64;
        results.execution_metrics = ExecutionMetrics {
            total_execution_time_ms: execution_time,
            cpu_utilization: 0.0, // Would be collected from system metrics
            disk_io_operations: 0,
            network_operations: 0,
        };
        
        Ok(results)
    }
    
    /// Generate test scenarios for specific archetype
    fn generate_scenarios_for_archetype(&self, archetype: &UserArchetype) -> Result<Vec<ArchetypeTestScenario>> {
        let mut scenarios = Vec::new();
        let expectations = archetype.default_expectations();
        
        // Generate scenarios for each complexity level
        for complexity in [TestComplexity::Basic, TestComplexity::Intermediate, TestComplexity::Advanced, TestComplexity::Expert] {
            for _ in 0..self.config.users_per_archetype {
                let scenario = self.create_scenario_for_complexity(archetype, &expectations, &complexity)?;
                scenarios.push(scenario);
            }
        }
        
        Ok(scenarios)
    }
    
    /// Create scenario for specific complexity level
    fn create_scenario_for_complexity(
        &self,
        archetype: &UserArchetype,
        expectations: &ArchetypeBehavioralExpectations,
        complexity: &TestComplexity,
    ) -> Result<ArchetypeTestScenario> {
        // This would generate appropriate test scenarios based on complexity
        // For now, creating a basic scenario structure
        
        let personality = PersonalityTraits {
            archetype: archetype.clone(),
            activity_level: 7,
            consistency: 8,
            collaboration: 6,
            innovation: 5,
            security_consciousness: 8,
            economic_engagement: 6,
            cross_chain_preference: 4,
            peak_hours: expectations.expected_peak_hours.clone(),
            service_preferences: expectations.expected_services.clone(),
            risk_tolerance: 5,
        };
        
        // Create properly initialized behavioral patterns for this archetype
        let behavioral_patterns = BehavioralPatterns::new_for_archetype(archetype);
        
        let fraud_scenario = if self.config.fraud_simulation_rate > 0.0 {
            // Some percentage of scenarios include fraud
            Some(FraudScenario {
                fraud_type: FraudType::PatternMimicking,
                intensity: 0.3,
                duration_days: 7,
                expected_detection_rate: 0.85,
            })
        } else {
            None
        };
        
        Ok(ArchetypeTestScenario {
            archetype: archetype.clone(),
            personality,
            behavioral_patterns,
            expected_confidence_range: expectations.expected_consistency_range,
            expected_recovery_rate: 0.85, // Would be calculated based on archetype
            complexity_level: complexity.clone(),
            fraud_scenario,
        })
    }
    
    /// Test scenarios for specific archetype
    async fn test_archetype_scenarios(
        &mut self,
        archetype: &UserArchetype,
        scenarios: &[ArchetypeTestScenario],
    ) -> Result<ArchetypeResults> {
        let mut scenario_results = Vec::new();
        let mut success_count = 0;
        let mut total_confidence = 0.0;
        let mut total_recovery_time = 0;
        let mut zkp_success_count = 0;
        let mut fraud_detection_count = 0;
        let mut fraud_scenario_count = 0;
        
        for (i, scenario) in scenarios.iter().enumerate() {
            let scenario_result = self.test_individual_scenario(&format!("{}_{}", archetype.description(), i), scenario).await?;
            
            if scenario_result.recovery_success {
                success_count += 1;
            }
            
            total_confidence += scenario_result.confidence_score;
            total_recovery_time += scenario_result.recovery_time_ms;
            
            if scenario_result.zkp_verification_success {
                zkp_success_count += 1;
            }
            
            if let Some(fraud_detected) = scenario_result.fraud_detected {
                fraud_scenario_count += 1;
                if fraud_detected {
                    fraud_detection_count += 1;
                }
            }
            
            scenario_results.push(scenario_result);
        }
        
        let total_scenarios = scenarios.len();
        
        Ok(ArchetypeResults {
            archetype: archetype.clone(),
            scenarios_executed: total_scenarios,
            success_rate: success_count as f64 / total_scenarios as f64,
            average_confidence: total_confidence / total_scenarios as f64,
            average_recovery_time_ms: total_recovery_time / total_scenarios as u64,
            zkp_success_rate: zkp_success_count as f64 / total_scenarios as f64,
            fraud_detection_rate: if fraud_scenario_count > 0 {
                fraud_detection_count as f64 / fraud_scenario_count as f64
            } else {
                0.0
            },
            complexity_performance: HashMap::new(), // Would be populated
            test_scenarios: scenario_results,
        })
    }
    
    /// Test individual scenario using production recovery system
    async fn test_individual_scenario(
        &mut self,
        scenario_id: &str,
        scenario: &ArchetypeTestScenario,
    ) -> Result<ScenarioResult> {
        let start_time = std::time::Instant::now();
        
        // Generate behavioral patterns using production system
        let confidence_score = self.recovery_system.compute_confidence_score(
            &scenario.behavioral_patterns,
            &Default::default(), // Would use actual peer endorsements
            scenario_id,
        ).map_err(|e| anyhow::anyhow!("{}", e))?;
        
        // Test recovery session
        let recovery_success = self.test_recovery_session(scenario_id, &scenario.behavioral_patterns, &confidence_score).await?;
        
        // Test ZK proof generation and verification if enabled
        let zkp_verification_success = if self.config.enable_zkp_verification {
            self.test_zkp_verification(scenario_id, &scenario.behavioral_patterns).await?
        } else {
            true
        };
        
        // Test fraud detection if fraud scenario
        let fraud_detected = if let Some(fraud_scenario) = &scenario.fraud_scenario {
            Some(self.test_fraud_detection(scenario_id, fraud_scenario, &scenario.behavioral_patterns).await?)
        } else {
            None
        };
        
        let recovery_time = start_time.elapsed().as_millis() as u64;
        
        Ok(ScenarioResult {
            scenario_id: scenario_id.to_string(),
            archetype: scenario.archetype.clone(),
            complexity: scenario.complexity_level.clone(),
            recovery_success,
            confidence_score: confidence_score.threshold,
            recovery_time_ms: recovery_time,
            zkp_verification_success,
            fraud_detected,
            performance_breakdown: HashMap::new(), // Would be populated with detailed metrics
        })
    }
    
    /// Test recovery session using production orchestrator
    async fn test_recovery_session(
        &mut self,
        _session_id: &str,
        patterns: &BehavioralPatterns,
        confidence: &ConfidenceScore,
    ) -> Result<bool> {
        // Run through recovery phases
        let result = self.recovery_orchestrator.initiate_recovery_workflow(
            &spacekit_primitives::v1::identity::Identity {
                did: "sim_identity".to_string(),
                username: "simulation_user".to_string(),
                master_password: "sim_password".to_string(),
                default_profile: true,
                profiles: vec![],
                authenticated: true,
                key_pairs: vec![],
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }, // Simulation identity
            patterns,
            &Default::default(), // Peer endorsements 
            confidence
        ).await.map_err(|e| anyhow::anyhow!("{}", e))?;
        
        Ok(result.recovery_successful)
    }
    
    /// Test ZK proof generation and verification
    async fn test_zkp_verification(
        &mut self,
        _session_id: &str,
        patterns: &BehavioralPatterns,
    ) -> Result<bool> {
        // Generate ZK proof
        let proof = self.zk_system.generate_behavioral_recovery_proof(
            patterns,
            &Default::default(), // AI analysis
            &Default::default(), // Recovery session
            &Default::default(), // Confidence score
        ).await.map_err(|e| anyhow::anyhow!("{}", e))?;
        
        // Verify proof
        let verification_result = self.zk_system.verify_behavioral_recovery_proof(&proof).await.map_err(|e| anyhow::anyhow!("{}", e))?;
        
        // Extract boolean result from verification result
        Ok(verification_result.verification_successful)
    }
    
    /// Test fraud detection capabilities
    async fn test_fraud_detection(
        &mut self,
        _session_id: &str,
        _fraud_scenario: &FraudScenario,
        _patterns: &BehavioralPatterns,
    ) -> Result<bool> {
        // This would simulate fraud injection and test detection
        // For now, returning a simulated result
        Ok(true) // Simplified fraud detection test
    }
    
    // Additional metric collection methods would be implemented here...
    async fn collect_performance_metrics(&self) -> Result<PerformanceMetrics> {
        Ok(PerformanceMetrics::default())
    }
    
    async fn collect_zkp_metrics(&self) -> Result<ZKProofResults> {
        Ok(ZKProofResults::default())
    }
    
    async fn collect_fraud_detection_metrics(&self) -> Result<FraudDetectionResults> {
        Ok(FraudDetectionResults::default())
    }
    
    async fn collect_recovery_statistics(&self) -> Result<RecoveryStatistics> {
        Ok(RecoveryStatistics::default())
    }
}

impl Default for PerformanceMetrics {
    fn default() -> Self {
        Self {
            avg_pattern_generation_time: 0,
            avg_confidence_calculation_time: 0,
            avg_zkp_generation_time: 0,
            avg_zkp_verification_time: 0,
            avg_recovery_session_duration: 0,
            memory_usage: MemoryUsage {
                peak_memory_mb: 0,
                average_memory_mb: 0,
                component_memory_usage: HashMap::new(),
            },
            throughput: ThroughputMetrics {
                sessions_per_second: 0.0,
                confidence_calculations_per_second: 0.0,
                zkp_proofs_per_second: 0.0,
                pattern_analyses_per_second: 0.0,
            },
        }
    }
}

impl Default for ZKProofResults {
    fn default() -> Self {
        Self {
            total_proofs_generated: 0,
            successful_generations: 0,
            total_proofs_verified: 0,
            successful_verifications: 0,
            average_proof_size: 0,
            average_generation_time_ms: 0,
            average_verification_time_ms: 0,
            privacy_budget_used: 0.0,
        }
    }
}

impl Default for FraudDetectionResults {
    fn default() -> Self {
        Self {
            total_fraud_scenarios: 0,
            detected_fraud_attempts: 0,
            false_positive_rate: 0.0,
            false_negative_rate: 0.0,
            detection_by_fraud_type: HashMap::new(),
            average_detection_time_ms: 0,
        }
    }
}

impl Default for RecoveryStatistics {
    fn default() -> Self {
        Self {
            total_sessions: 0,
            successful_recoveries: 0,
            failed_recoveries: 0,
            average_session_duration_ms: 0,
            success_rate_by_archetype: HashMap::new(),
            challenge_response_accuracy: 0.0,
            byzantine_tolerance_effectiveness: 0.0,
        }
    }
}

impl Default for SimulationComparison {
    fn default() -> Self {
        Self {
            simulation_success_rate: 0.0,
            production_success_rate: 0.0,
            confidence_score_difference: 0.0,
            performance_overhead_factor: 1.0,
            zkp_accuracy_improvement: 0.0,
        }
    }
}

impl Default for ExecutionMetrics {
    fn default() -> Self {
        Self {
            total_execution_time_ms: 0,
            cpu_utilization: 0.0,
            disk_io_operations: 0,
            network_operations: 0,
        }
    }
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            users_per_archetype: 10,
            recovery_attempts_per_user: 3,
            simulation_duration_days: 7,
            enable_zkp_verification: true,
            enable_differential_privacy: true,
            epsilon: 1.0,
            delta: 1e-6,
            fraud_simulation_rate: 0.1,
            enable_performance_benchmarks: true,
        }
    }
}