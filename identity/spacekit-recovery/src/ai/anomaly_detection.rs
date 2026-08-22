use crate::behavioral::{BehavioralPatterns, StoragePattern, ComputePattern, EconomicPattern, ServiceQualityMetrics, MultiChainPattern};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use ndarray::{Array1, Array2, Axis};
use candle_core::{Tensor, Device, DType};
use std::collections::HashMap;
use std::error::Error;

/// Anomaly detection system for behavioral patterns
pub struct AnomalyDetector {
    /// Statistical models for different behavioral components
    storage_model: StatisticalModel,
    compute_model: StatisticalModel,
    economic_model: StatisticalModel,
    service_model: StatisticalModel,
    chain_model: StatisticalModel,
    /// Historical baselines for comparison
    baseline_patterns: Option<BehavioralPatterns>,
    /// Detection thresholds
    thresholds: AnomalyThresholds,
    /// Learning parameters
    learning_rate: f64,
    adaptation_enabled: bool,
}

/// Statistical model for anomaly detection
#[derive(Debug, Clone)]
pub struct StatisticalModel {
    /// Mean values for each feature
    means: Array1<f64>,
    /// Standard deviations for each feature
    std_devs: Array1<f64>,
    /// Covariance matrix for multivariate analysis
    covariance: Array2<f64>,
    /// Sample count for statistical significance
    sample_count: u64,
    /// Model confidence level
    confidence_level: f64,
}

/// Anomaly detection thresholds
#[derive(Debug, Clone)]
pub struct AnomalyThresholds {
    /// Z-score threshold for outlier detection
    z_score_threshold: f64,
    /// Mahalanobis distance threshold for multivariate outliers
    mahalanobis_threshold: f64,
    /// Minimum samples required for reliable detection
    min_samples: u64,
    /// Confidence level for statistical tests
    confidence_level: f64,
}

/// Comprehensive anomaly detection report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyReport {
    /// Overall anomaly score (0.0 = normal, 1.0 = highly anomalous)
    pub anomaly_score: f64,
    /// Detected anomaly types
    pub detected_anomalies: Vec<AnomalyType>,
    /// Component-specific anomaly scores
    pub component_scores: AnomalyComponentScores,
    /// Statistical significance of detections
    pub statistical_significance: f64,
    /// Detailed explanations for each anomaly
    pub explanations: Vec<AnomalyExplanation>,
    /// Detection timestamp
    pub detected_at: DateTime<Utc>,
    /// Detector confidence in the results
    pub detector_confidence: f64,
}

/// Anomaly scores for different behavioral components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyComponentScores {
    pub storage_anomaly: f64,
    pub compute_anomaly: f64,
    pub economic_anomaly: f64,
    pub service_anomaly: f64,
    pub chain_anomaly: f64,
}

/// Types of behavioral anomalies that can be detected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnomalyType {
    /// Unusual storage contribution patterns
    StoragePatternAnomaly {
        severity: f64,
        description: String,
    },
    /// Abnormal compute participation
    ComputePatternAnomaly {
        severity: f64,
        description: String,
    },
    /// Economic behavior inconsistencies
    EconomicPatternAnomaly {
        severity: f64,
        description: String,
    },
    /// Service quality deviations
    ServiceQualityAnomaly {
        severity: f64,
        description: String,
    },
    /// Multi-chain activity anomalies
    ChainActivityAnomaly {
        severity: f64,
        description: String,
    },
    /// Cross-component correlation anomalies
    CorrelationAnomaly {
        severity: f64,
        description: String,
        components: Vec<String>,
    },
    /// Temporal pattern anomalies
    TemporalAnomaly {
        severity: f64,
        description: String,
        time_window: String,
    },
}

/// Detailed explanation for detected anomalies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyExplanation {
    pub anomaly_type: String,
    pub severity: f64,
    pub confidence: f64,
    pub description: String,
    pub affected_metrics: Vec<String>,
    pub statistical_evidence: StatisticalEvidence,
    pub recommendations: Vec<String>,
}

/// Statistical evidence supporting anomaly detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatisticalEvidence {
    pub z_scores: HashMap<String, f64>,
    pub p_values: HashMap<String, f64>,
    pub mahalanobis_distances: HashMap<String, f64>,
    pub confidence_intervals: HashMap<String, (f64, f64)>,
}

impl AnomalyDetector {
    /// Create new anomaly detector with default parameters
    pub fn new() -> Self {
        Self {
            storage_model: StatisticalModel::new(5), // 5 storage features
            compute_model: StatisticalModel::new(4), // 4 compute features  
            economic_model: StatisticalModel::new(5), // 5 economic features
            service_model: StatisticalModel::new(5), // 5 service features
            chain_model: StatisticalModel::new(6), // 6 chain features
            baseline_patterns: None,
            thresholds: AnomalyThresholds::default(),
            learning_rate: 0.01,
            adaptation_enabled: true,
        }
    }

    /// Create detector with custom thresholds
    pub fn with_thresholds(thresholds: AnomalyThresholds) -> Self {
        let mut detector = Self::new();
        detector.thresholds = thresholds;
        detector
    }

    /// Detect anomalies in behavioral patterns
    pub async fn detect_anomalies(
        &mut self,
        patterns: &BehavioralPatterns,
        _identity_did: &str,
    ) -> Result<AnomalyReport, Box<dyn Error>> {
        // Extract feature vectors for each component
        let storage_features = self.extract_storage_features(&patterns.storage_behavior)?;
        let compute_features = self.extract_compute_features(&patterns.compute_participation)?;
        let economic_features = self.extract_economic_features(&patterns.economic_patterns)?;
        let service_features = self.extract_service_features(&patterns.service_quality)?;
        let chain_features = self.extract_chain_features(&patterns.multi_chain_activity)?;

        // Detect anomalies in each component - moved to separate method to avoid borrow issues
        let storage_anomaly = self.storage_model.detect_anomaly(&storage_features, &self.thresholds)?;
        let compute_anomaly = self.compute_model.detect_anomaly(&compute_features, &self.thresholds)?;
        let economic_anomaly = self.economic_model.detect_anomaly(&economic_features, &self.thresholds)?;
        let service_anomaly = self.service_model.detect_anomaly(&service_features, &self.thresholds)?;
        let chain_anomaly = self.chain_model.detect_anomaly(&chain_features, &self.thresholds)?;

        // Update models if adaptation is enabled
        if self.adaptation_enabled {
            if storage_anomaly < 0.7 { self.storage_model.update_with_sample(storage_features); }
            if compute_anomaly < 0.7 { self.compute_model.update_with_sample(compute_features); }
            if economic_anomaly < 0.7 { self.economic_model.update_with_sample(economic_features); }
            if service_anomaly < 0.7 { self.service_model.update_with_sample(service_features); }
            if chain_anomaly < 0.7 { self.chain_model.update_with_sample(chain_features); }
        }

        // Compute overall anomaly score
        let component_scores = AnomalyComponentScores {
            storage_anomaly,
            compute_anomaly,
            economic_anomaly,
            service_anomaly,
            chain_anomaly,
        };

        let overall_anomaly_score = self.compute_overall_anomaly_score(&component_scores)?;

        // Detect specific anomaly types
        let detected_anomalies = self.classify_anomalies(patterns, &component_scores).await?;

        // Generate explanations
        let explanations = self.generate_explanations(&detected_anomalies, patterns)?;

        // Compute statistical significance
        let statistical_significance = self.compute_statistical_significance(&component_scores)?;

        // Update baseline if this is normal behavior
        if overall_anomaly_score < self.thresholds.z_score_threshold / 3.0 && self.adaptation_enabled {
            self.update_baseline(patterns).await?;
        }

        Ok(AnomalyReport {
            anomaly_score: overall_anomaly_score,
            detected_anomalies,
            component_scores,
            statistical_significance,
            explanations,
            detected_at: Utc::now(),
            detector_confidence: self.compute_detector_confidence()?,
        })
    }

    /// Extract storage behavior features for analysis
    fn extract_storage_features(&self, storage: &StoragePattern) -> Result<Array1<f64>, Box<dyn Error>> {
        let mut features = Array1::zeros(5);
        
        features[0] = storage.avg_daily_storage_gb;
        features[1] = storage.consistency_score;
        features[2] = storage.avg_retention_days;
        features[3] = storage.geographic_preferences.mean().unwrap_or(0.0);
        features[4] = storage.preferred_storage_hours.std(0.0);

        Ok(features)
    }

    /// Extract compute participation features for analysis
    fn extract_compute_features(&self, compute: &ComputePattern) -> Result<Array1<f64>, Box<dyn Error>> {
        let mut features = Array1::zeros(4);
        
        features[0] = compute.avg_daily_compute_hours;
        features[1] = compute.avg_daily_bandwidth_gb;
        features[2] = compute.service_quality;
        features[3] = compute.availability_pattern.std(0.0);

        Ok(features)
    }

    /// Extract economic behavior features for analysis
    fn extract_economic_features(&self, economic: &EconomicPattern) -> Result<Array1<f64>, Box<dyn Error>> {
        let mut features = Array1::zeros(5);
        
        features[0] = economic.earning_consistency;
        features[1] = economic.avg_stake_duration;
        features[2] = economic.payment_punctuality;
        features[3] = economic.bonding_curve_interactions as f64;
        features[4] = economic.participation_score;

        Ok(features)
    }

    /// Extract service quality features for analysis
    fn extract_service_features(&self, service: &ServiceQualityMetrics) -> Result<Array1<f64>, Box<dyn Error>> {
        let mut features = Array1::zeros(5);
        
        features[0] = service.peer_rating_avg;
        features[1] = service.success_ratio;
        features[2] = service.avg_response_time_ms.ln().max(0.0); // Log transform
        features[3] = service.reputation_accumulation;
        features[4] = service.total_services_completed as f64;

        Ok(features)
    }

    /// Extract multi-chain activity features for analysis
    fn extract_chain_features(&self, chain: &MultiChainPattern) -> Result<Array1<f64>, Box<dyn Error>> {
        let mut features = Array1::zeros(6);
        
        features[0] = chain.cross_chain_tx_frequency;
        features[1] = chain.bridge_usage_frequency;
        features[2] = chain.identity_consistency;
        features[3] = chain.chain_usage_distribution.std(0.0);
        features[4] = chain.chain_usage_distribution.iter().fold(0.0f64, |a, &b| a.max(b));
        features[5] = chain.preferred_networks.len() as f64;

        Ok(features)
    }

    /// Detect anomalies in a single component using statistical methods
    fn detect_component_anomaly(
        &self,
        features: &Array1<f64>,
        model: &mut StatisticalModel,
    ) -> Result<f64, Box<dyn Error>> {
        if model.sample_count < self.thresholds.min_samples {
            // Not enough samples for reliable detection
            return Ok(0.0);
        }

        // Compute Z-scores for each feature
        let z_scores = (features - &model.means) / &model.std_devs;
        let max_z_score = z_scores.mapv(|x| x.abs()).iter().fold(0.0f64, |a, &b| a.max(b));

        // Compute Mahalanobis distance for multivariate outlier detection
        let mahalanobis_distance = self.compute_mahalanobis_distance(features, model)?;

        // Combine Z-score and Mahalanobis distance
        let z_score_anomaly = (max_z_score / self.thresholds.z_score_threshold).min(1.0);
        let mahalanobis_anomaly = (mahalanobis_distance / self.thresholds.mahalanobis_threshold).min(1.0);

        // Weighted combination
        let anomaly_score = (z_score_anomaly * 0.6 + mahalanobis_anomaly * 0.4).max(0.0).min(1.0);

        // Update model with new data point if it's not too anomalous
        if anomaly_score < 0.7 && self.adaptation_enabled {
            model.update_with_sample(features.clone());
        }

        Ok(anomaly_score)
    }

    /// Compute Mahalanobis distance for multivariate outlier detection
    fn compute_mahalanobis_distance(
        &self,
        features: &Array1<f64>,
        model: &StatisticalModel,
    ) -> Result<f64, Box<dyn Error>> {
        // Simplified Mahalanobis distance using diagonal covariance
        let diff = features - &model.means;
        let normalized_diff = &diff / &model.std_devs;
        let distance = normalized_diff.mapv(|x| x * x).sum().sqrt();
        
        Ok(distance)
    }

    /// Compute overall anomaly score from component scores
    fn compute_overall_anomaly_score(
        &self,
        component_scores: &AnomalyComponentScores,
    ) -> Result<f64, Box<dyn Error>> {
        // Weighted combination of component scores
        let weights = [0.25, 0.25, 0.2, 0.15, 0.15]; // Storage, Compute, Economic, Service, Chain
        let scores = [
            component_scores.storage_anomaly,
            component_scores.compute_anomaly,
            component_scores.economic_anomaly,
            component_scores.service_anomaly,
            component_scores.chain_anomaly,
        ];

        let weighted_score = scores.iter()
            .zip(weights.iter())
            .map(|(score, weight)| score * weight)
            .sum::<f64>();

        Ok(weighted_score.max(0.0).min(1.0))
    }

    /// Classify detected anomalies into specific types
    async fn classify_anomalies(
        &self,
        patterns: &BehavioralPatterns,
        component_scores: &AnomalyComponentScores,
    ) -> Result<Vec<AnomalyType>, Box<dyn Error>> {
        let mut anomalies = Vec::new();

        // Storage anomalies
        if component_scores.storage_anomaly > 0.5 {
            anomalies.push(AnomalyType::StoragePatternAnomaly {
                severity: component_scores.storage_anomaly,
                description: "Unusual storage contribution patterns detected".to_string(),
            });
        }

        // Compute anomalies
        if component_scores.compute_anomaly > 0.5 {
            anomalies.push(AnomalyType::ComputePatternAnomaly {
                severity: component_scores.compute_anomaly,
                description: "Abnormal compute participation patterns".to_string(),
            });
        }

        // Economic anomalies
        if component_scores.economic_anomaly > 0.5 {
            anomalies.push(AnomalyType::EconomicPatternAnomaly {
                severity: component_scores.economic_anomaly,
                description: "Economic behavior inconsistencies detected".to_string(),
            });
        }

        // Service quality anomalies
        if component_scores.service_anomaly > 0.5 {
            anomalies.push(AnomalyType::ServiceQualityAnomaly {
                severity: component_scores.service_anomaly,
                description: "Service quality metrics deviate from expected patterns".to_string(),
            });
        }

        // Chain activity anomalies
        if component_scores.chain_anomaly > 0.5 {
            anomalies.push(AnomalyType::ChainActivityAnomaly {
                severity: component_scores.chain_anomaly,
                description: "Multi-chain activity patterns are unusual".to_string(),
            });
        }

        // Cross-component correlation anomalies
        let correlation_anomaly = self.detect_correlation_anomaly(component_scores)?;
        if correlation_anomaly > 0.6 {
            anomalies.push(AnomalyType::CorrelationAnomaly {
                severity: correlation_anomaly,
                description: "Unusual correlations between behavioral components".to_string(),
                components: vec!["storage".to_string(), "compute".to_string(), "economic".to_string()],
            });
        }

        // Temporal anomalies (if we have baseline)
        if let Some(baseline) = &self.baseline_patterns {
            let temporal_anomaly = self.detect_temporal_anomaly(patterns, baseline)?;
            if temporal_anomaly > 0.6 {
                anomalies.push(AnomalyType::TemporalAnomaly {
                    severity: temporal_anomaly,
                    description: "Behavioral patterns have changed significantly over time".to_string(),
                    time_window: "30 days".to_string(),
                });
            }
        }

        Ok(anomalies)
    }

    /// Detect anomalies in cross-component correlations
    fn detect_correlation_anomaly(&self, component_scores: &AnomalyComponentScores) -> Result<f64, Box<dyn Error>> {
        // Check for unusual combinations of high/low scores
        let scores = vec![
            component_scores.storage_anomaly,
            component_scores.compute_anomaly,
            component_scores.economic_anomaly,
            component_scores.service_anomaly,
            component_scores.chain_anomaly,
        ];

        // High variance in component scores might indicate manipulation
        let mean_score = scores.iter().sum::<f64>() / scores.len() as f64;
        let variance = scores.iter()
            .map(|s| (s - mean_score).powi(2))
            .sum::<f64>() / scores.len() as f64;

        // Normalize variance to 0-1 range
        let correlation_anomaly = (variance * 4.0).min(1.0);

        Ok(correlation_anomaly)
    }

    /// Detect temporal anomalies compared to baseline
    fn detect_temporal_anomaly(
        &self,
        current: &BehavioralPatterns,
        baseline: &BehavioralPatterns,
    ) -> Result<f64, Box<dyn Error>> {
        let mut deviations = Vec::new();

        // Storage deviation
        let storage_dev = (current.storage_behavior.avg_daily_storage_gb - baseline.storage_behavior.avg_daily_storage_gb).abs()
            / baseline.storage_behavior.avg_daily_storage_gb.max(1.0);
        deviations.push(storage_dev);

        // Compute deviation
        let compute_dev = (current.compute_participation.avg_daily_compute_hours - baseline.compute_participation.avg_daily_compute_hours).abs()
            / baseline.compute_participation.avg_daily_compute_hours.max(1.0);
        deviations.push(compute_dev);

        // Economic deviation
        let economic_dev = (current.economic_patterns.earning_consistency - baseline.economic_patterns.earning_consistency).abs();
        deviations.push(economic_dev);

        // Service deviation
        let service_dev = (current.service_quality.success_ratio - baseline.service_quality.success_ratio).abs();
        deviations.push(service_dev);

        // Multi-chain deviation
        let chain_dev = (current.multi_chain_activity.identity_consistency - baseline.multi_chain_activity.identity_consistency).abs();
        deviations.push(chain_dev);

        // Compute overall temporal deviation
        let temporal_anomaly = deviations.iter().sum::<f64>() / deviations.len() as f64;
        Ok(temporal_anomaly.min(1.0))
    }

    /// Generate detailed explanations for detected anomalies
    fn generate_explanations(
        &self,
        anomalies: &[AnomalyType],
        patterns: &BehavioralPatterns,
    ) -> Result<Vec<AnomalyExplanation>, Box<dyn Error>> {
        let mut explanations = Vec::new();

        for anomaly in anomalies {
            let explanation = match anomaly {
                AnomalyType::StoragePatternAnomaly { severity, description } => {
                    AnomalyExplanation {
                        anomaly_type: "Storage Pattern Anomaly".to_string(),
                        severity: *severity,
                        confidence: 0.85,
                        description: description.clone(),
                        affected_metrics: vec![
                            "avg_daily_storage_gb".to_string(),
                            "consistency_score".to_string(),
                            "avg_retention_days".to_string(),
                        ],
                        statistical_evidence: StatisticalEvidence {
                            z_scores: HashMap::new(),
                            p_values: HashMap::new(),
                            mahalanobis_distances: HashMap::new(),
                            confidence_intervals: HashMap::new(),
                        },
                        recommendations: vec![
                            "Investigate storage contribution patterns".to_string(),
                            "Verify storage node authenticity".to_string(),
                            "Monitor for coordinated storage manipulation".to_string(),
                        ],
                    }
                }
                AnomalyType::ComputePatternAnomaly { severity, description } => {
                    AnomalyExplanation {
                        anomaly_type: "Compute Pattern Anomaly".to_string(),
                        severity: *severity,
                        confidence: 0.82,
                        description: description.clone(),
                        affected_metrics: vec![
                            "avg_daily_compute_hours".to_string(),
                            "avg_daily_bandwidth_gb".to_string(),
                            "service_quality".to_string(),
                        ],
                        statistical_evidence: StatisticalEvidence {
                            z_scores: HashMap::new(),
                            p_values: HashMap::new(),
                            mahalanobis_distances: HashMap::new(),
                            confidence_intervals: HashMap::new(),
                        },
                        recommendations: vec![
                            "Verify compute node resources".to_string(),
                            "Check for artificial compute inflation".to_string(),
                            "Monitor compute quality consistency".to_string(),
                        ],
                    }
                }
                AnomalyType::EconomicPatternAnomaly { severity, description } => {
                    AnomalyExplanation {
                        anomaly_type: "Economic Pattern Anomaly".to_string(),
                        severity: *severity,
                        confidence: 0.88,
                        description: description.clone(),
                        affected_metrics: vec![
                            "earning_consistency".to_string(),
                            "payment_punctuality".to_string(),
                            "participation_score".to_string(),
                        ],
                        statistical_evidence: StatisticalEvidence {
                            z_scores: HashMap::new(),
                            p_values: HashMap::new(),
                            mahalanobis_distances: HashMap::new(),
                            confidence_intervals: HashMap::new(),
                        },
                        recommendations: vec![
                            "Audit economic transaction history".to_string(),
                            "Verify token earning legitimacy".to_string(),
                            "Check for economic manipulation patterns".to_string(),
                        ],
                    }
                }
                _ => continue, // Handle other anomaly types similarly
            };
            explanations.push(explanation);
        }

        Ok(explanations)
    }

    /// Compute statistical significance of anomaly detections
    fn compute_statistical_significance(&self, component_scores: &AnomalyComponentScores) -> Result<f64, Box<dyn Error>> {
        // Simple significance based on how many components show anomalies
        let anomalous_components = [
            component_scores.storage_anomaly > 0.5,
            component_scores.compute_anomaly > 0.5,
            component_scores.economic_anomaly > 0.5,
            component_scores.service_anomaly > 0.5,
            component_scores.chain_anomaly > 0.5,
        ].iter().filter(|&&x| x).count();

        let significance = match anomalous_components {
            0 => 0.1,
            1 => 0.4,
            2 => 0.7,
            3 => 0.85,
            4 => 0.95,
            5 => 0.99,
            _ => 0.99,
        };

        Ok(significance)
    }

    /// Compute detector confidence based on model maturity
    fn compute_detector_confidence(&self) -> Result<f64, Box<dyn Error>> {
        let model_confidences = [
            self.storage_model.confidence_level,
            self.compute_model.confidence_level,
            self.economic_model.confidence_level,
            self.service_model.confidence_level,
            self.chain_model.confidence_level,
        ];

        let avg_confidence = model_confidences.iter().sum::<f64>() / model_confidences.len() as f64;
        Ok(avg_confidence)
    }

    /// Update baseline patterns with new normal behavior
    async fn update_baseline(&mut self, patterns: &BehavioralPatterns) -> Result<(), Box<dyn Error>> {
        // Update baseline with exponential smoothing
        if let Some(baseline) = &mut self.baseline_patterns {
            let alpha = self.learning_rate;
            
            // Update storage baseline
            baseline.storage_behavior.avg_daily_storage_gb = 
                alpha * patterns.storage_behavior.avg_daily_storage_gb + 
                (1.0 - alpha) * baseline.storage_behavior.avg_daily_storage_gb;
            
            baseline.storage_behavior.consistency_score = 
                alpha * patterns.storage_behavior.consistency_score + 
                (1.0 - alpha) * baseline.storage_behavior.consistency_score;
            
            // Update other components similarly...
        } else {
            // First time - set current patterns as baseline
            self.baseline_patterns = Some(patterns.clone());
        }

        Ok(())
    }

    /// Update statistical models with new training data
    pub async fn update_models(
        &mut self,
        patterns: &BehavioralPatterns,
        anomaly_report: &AnomalyReport,
    ) -> Result<(), Box<dyn Error>> {
        // Only update models with non-anomalous data
        if anomaly_report.anomaly_score < 0.3 {
            // Extract features and update models
            let storage_features = self.extract_storage_features(&patterns.storage_behavior)?;
            self.storage_model.update_with_sample(storage_features);

            let compute_features = self.extract_compute_features(&patterns.compute_participation)?;
            self.compute_model.update_with_sample(compute_features);

            let economic_features = self.extract_economic_features(&patterns.economic_patterns)?;
            self.economic_model.update_with_sample(economic_features);

            let service_features = self.extract_service_features(&patterns.service_quality)?;
            self.service_model.update_with_sample(service_features);

            let chain_features = self.extract_chain_features(&patterns.multi_chain_activity)?;
            self.chain_model.update_with_sample(chain_features);
        }

        Ok(())
    }

    /// Check if detector is ready for reliable anomaly detection
    pub fn is_ready(&self) -> bool {
        self.storage_model.sample_count >= self.thresholds.min_samples &&
        self.compute_model.sample_count >= self.thresholds.min_samples &&
        self.economic_model.sample_count >= self.thresholds.min_samples &&
        self.service_model.sample_count >= self.thresholds.min_samples &&
        self.chain_model.sample_count >= self.thresholds.min_samples
    }

    /// Set adaptation learning rate
    pub fn set_learning_rate(&mut self, rate: f64) {
        self.learning_rate = rate.max(0.001).min(0.1);
    }

    /// Enable or disable adaptive learning
    pub fn set_adaptation_enabled(&mut self, enabled: bool) {
        self.adaptation_enabled = enabled;
    }
}

impl StatisticalModel {
    /// Create new statistical model for given feature count
    pub fn new(feature_count: usize) -> Self {
        Self {
            means: Array1::zeros(feature_count),
            std_devs: Array1::ones(feature_count),
            covariance: Array2::eye(feature_count),
            sample_count: 0,
            confidence_level: 0.0,
        }
    }

    /// Detect anomaly in features using this model
    pub fn detect_anomaly(&self, features: &Array1<f64>, thresholds: &AnomalyThresholds) -> Result<f64, Box<dyn Error>> {
        if self.sample_count < thresholds.min_samples {
            // Not enough samples for reliable detection
            return Ok(0.0);
        }

        // Compute Z-scores for each feature
        let z_scores = (features - &self.means) / &self.std_devs;
        let max_z_score = z_scores.mapv(|x| x.abs()).iter().fold(0.0f64, |a, &b| a.max(b));

        // Compute Mahalanobis distance for multivariate outlier detection
        let mahalanobis_distance = self.compute_mahalanobis_distance(features)?;

        // Combine Z-score and Mahalanobis distance
        let z_score_anomaly = (max_z_score / thresholds.z_score_threshold).min(1.0);
        let mahalanobis_anomaly = (mahalanobis_distance / thresholds.mahalanobis_threshold).min(1.0);

        // Weighted combination
        let anomaly_score = (z_score_anomaly * 0.6 + mahalanobis_anomaly * 0.4).max(0.0).min(1.0);

        Ok(anomaly_score)
    }

    /// Compute Mahalanobis distance for multivariate outlier detection
    fn compute_mahalanobis_distance(&self, features: &Array1<f64>) -> Result<f64, Box<dyn Error>> {
        // Simplified Mahalanobis distance using diagonal covariance
        let diff = features - &self.means;
        let normalized_diff = &diff / &self.std_devs;
        let distance = normalized_diff.mapv(|x| x * x).sum().sqrt();
        
        Ok(distance)
    }

    /// Update model with new sample using online learning
    pub fn update_with_sample(&mut self, features: Array1<f64>) {
        self.sample_count += 1;
        let n = self.sample_count as f64;

        if self.sample_count == 1 {
            self.means = features.clone();
            return;
        }

        // Online update of mean
        let delta = &features - &self.means;
        self.means = &self.means + &delta / n;

        // Online update of standard deviation (simplified)
        if self.sample_count > 2 {
            let new_delta = &features - &self.means;
            for i in 0..self.std_devs.len() {
                let variance = ((n - 2.0) * self.std_devs[i].powi(2) + delta[i] * new_delta[i]) / (n - 1.0);
                self.std_devs[i] = variance.sqrt().max(0.001); // Prevent division by zero
            }
        }

        // Update confidence level based on sample count
        self.confidence_level = (1.0 - (-0.1 * n).exp()).min(0.95);
    }
}

impl Default for AnomalyThresholds {
    fn default() -> Self {
        Self {
            z_score_threshold: 2.5,      // 99% confidence for normal distribution
            mahalanobis_threshold: 3.0,  // Conservative threshold for multivariate outliers
            min_samples: 10,             // Minimum samples for reliable detection
            confidence_level: 0.95,      // 95% confidence level
        }
    }
}

impl Default for AnomalyDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for AnomalyComponentScores {
    fn default() -> Self {
        Self {
            storage_anomaly: 0.0,
            compute_anomaly: 0.0,
            economic_anomaly: 0.0,
            service_anomaly: 0.0,
            chain_anomaly: 0.0,
        }
    }
}

impl Default for StatisticalEvidence {
    fn default() -> Self {
        Self {
            z_scores: HashMap::new(),
            p_values: HashMap::new(),
            confidence_intervals: HashMap::new(),
            mahalanobis_distances: HashMap::new(),
        }
    }
}

impl Default for AnomalyExplanation {
    fn default() -> Self {
        Self {
            anomaly_type: "Unknown".to_string(),
            severity: 0.0,
            confidence: 0.5,
            description: "No description available".to_string(),
            affected_metrics: Vec::new(),
            statistical_evidence: StatisticalEvidence::default(),
            recommendations: Vec::new(),
        }
    }
}

impl Default for AnomalyReport {
    fn default() -> Self {
        Self {
            anomaly_score: 0.0,
            detected_anomalies: Vec::new(),
            component_scores: AnomalyComponentScores::default(),
            statistical_significance: 0.5,
            explanations: Vec::new(),
            detected_at: chrono::Utc::now(),
            detector_confidence: 0.5,
        }
    }
}
