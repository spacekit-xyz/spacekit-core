use crate::behavioral::{BehavioralPatterns, BehavioralFingerprint};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use ndarray::{Array1, Array2, Axis};
use candle_core::{Tensor, Device, DType, Shape};
use std::collections::HashMap;
use std::error::Error;

/// Pattern recognition system for behavioral analysis
pub struct PatternRecognizer {
    /// Clustering model for grouping similar behavioral patterns
    clustering_model: ClusteringModel,
    similarity_model: SimilarityModel,
    /// Model training parameters
    learning_rate: f64,
    /// Pattern database for comparison
    known_patterns: HashMap<String, PatternTemplate>,
    /// Model readiness status
    model_ready: bool,
}

/// Clustering model for grouping similar behavioral patterns
#[derive(Debug, Clone)]
pub struct ClusteringModel {
    /// K-means cluster centers
    cluster_centers: Array2<f64>,
    /// Number of clusters
    num_clusters: usize,
    /// Cluster assignments for training data
    cluster_assignments: Vec<usize>,
    /// Cluster quality metrics
    inertia: f64,
}

/// Similarity model for comparing behavioral fingerprints
#[derive(Debug, Clone)]
pub struct SimilarityModel {
    /// Feature weights for similarity computation
    feature_weights: Array1<f64>,
    /// Similarity thresholds for different confidence levels
    similarity_thresholds: SimilarityThresholds,
}

/// Pattern recognition result
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecognitionResult {
    /// Overall confidence in pattern recognition
    pub confidence: f64,
    /// Recognized pattern types
    pub recognized_patterns: Vec<RecognizedPattern>,
    /// Similarity scores to known patterns
    pub similarity_scores: HashMap<String, f64>,
    /// Cluster assignment and confidence
    pub cluster_info: ClusterInfo,
    /// Anomaly indicators from pattern perspective
    pub pattern_anomalies: Vec<String>,
    /// Feature importance scores
    pub feature_importance: HashMap<String, f64>,
    /// Recognition timestamp
    pub recognized_at: DateTime<Utc>,
}

/// Individual recognized pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecognizedPattern {
    pub pattern_type: PatternType,
    pub confidence: f64,
    pub description: String,
    pub temporal_window: String,
    pub affected_components: Vec<String>,
}

/// Types of behavioral patterns that can be recognized
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PatternType {
    /// Consistent daily activity patterns
    DailyRoutine {
        consistency_score: f64,
        peak_hours: Vec<u8>,
    },
    /// Stable long-term behavior
    StabilityPattern {
        stability_score: f64,
        duration_days: u32,
    },
    /// Economic behavior patterns
    EconomicPattern {
        earning_pattern: String,
        spending_pattern: String,
        risk_profile: String,
    },
    /// Multi-chain usage patterns
    MultiChainPattern {
        primary_chains: Vec<String>,
        cross_chain_frequency: f64,
        consistency_score: f64,
    },
    /// Cross-component correlation patterns
    CorrelationPattern {
        correlation_strength: f64,
        correlated_components: Vec<String>,
    },
}

/// Cluster information for pattern grouping
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClusterInfo {
    pub cluster_id: usize,
    pub cluster_confidence: f64,
    pub distance_to_center: f64,
    pub cluster_size: usize,
    pub cluster_characteristics: Vec<String>,
}

/// Pattern template for known behavioral patterns
#[derive(Debug, Clone)]
pub struct PatternTemplate {
    pub template_id: String,
    pub pattern_type: PatternType,
    pub feature_vector: Array1<f64>,
    pub weight: f64,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

/// Model for patterns and learning
#[derive(Debug, Clone)]
pub struct PatternModel {
    pub model_type: String,
    pub parameters: HashMap<String, f64>,
    pub accuracy: f64,
    pub last_updated: DateTime<Utc>,
}

/// Similarity thresholds for different confidence levels
#[derive(Debug, Clone)]
pub struct SimilarityThresholds {
    pub high_confidence: f64,      // > 0.8
    pub medium_confidence: f64,    // 0.6 - 0.8
    pub low_confidence: f64,       // 0.4 - 0.6
    pub no_match: f64,            // < 0.4
}

impl PatternRecognizer {
    /// Create new pattern recognizer
    pub fn new() -> Self {
        Self {
            clustering_model: ClusteringModel::new(5), // 5 default clusters
            similarity_model: SimilarityModel::new(),
            learning_rate: 0.001,
            known_patterns: HashMap::new(),
            model_ready: false,
        }
    }

    /// Analyze patterns in behavioral data
    pub async fn analyze_patterns(
        &mut self,
        patterns: &BehavioralPatterns,
        fingerprint: &BehavioralFingerprint,
    ) -> Result<RecognitionResult, Box<dyn Error>> {
        // Extract comprehensive feature vector
        let feature_vector = self.extract_comprehensive_features(patterns)?;
        
        // Recognize specific pattern types
        let recognized_patterns = self.recognize_pattern_types(patterns, &feature_vector).await?;
        
        // Compute similarity to known patterns
        let similarity_scores = self.compute_similarity_scores(&feature_vector)?;
        
        // Perform clustering analysis
        let cluster_info = self.perform_clustering_analysis(&feature_vector)?;
        
        // Detect pattern-based anomalies
        let pattern_anomalies = self.detect_pattern_anomalies(patterns, &feature_vector)?;
        
        // Compute feature importance
        let feature_importance = self.compute_feature_importance(&feature_vector, patterns)?;
        
        // Calculate overall confidence
        let confidence = self.calculate_overall_confidence(&recognized_patterns, &similarity_scores, &cluster_info)?;

        Ok(RecognitionResult {
            confidence,
            recognized_patterns,
            similarity_scores,
            cluster_info,
            pattern_anomalies,
            feature_importance,
            recognized_at: Utc::now(),
        })
    }

    /// Extract comprehensive feature vector from behavioral patterns
    fn extract_comprehensive_features(&self, patterns: &BehavioralPatterns) -> Result<Array1<f64>, Box<dyn Error>> {
        let mut features = Vec::new();

        // Storage behavior features
        features.push(patterns.storage_behavior.avg_daily_storage_gb);
        features.push(patterns.storage_behavior.consistency_score);
        features.push(patterns.storage_behavior.avg_retention_days);
        
        // Geographic distribution entropy
        let geo_entropy = self.compute_entropy(&patterns.storage_behavior.geographic_preferences)?;
        features.push(geo_entropy);
        
        // Temporal patterns (hourly distribution)
        let hourly_entropy = self.compute_entropy(&patterns.storage_behavior.preferred_storage_hours)?;
        features.push(hourly_entropy);
        
        // Peak activity hours
        let peak_hour = patterns.storage_behavior.preferred_storage_hours
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
            .map(|(i, _)| i as f64)
            .unwrap_or(0.0);
        features.push(peak_hour);

        // Compute behavior features
        features.push(patterns.compute_participation.avg_daily_compute_hours);
        features.push(patterns.compute_participation.avg_daily_bandwidth_gb);
        features.push(patterns.compute_participation.service_quality);
        
        // Availability consistency
        let availability_consistency = 1.0 - patterns.compute_participation.availability_pattern.std(0.0);
        features.push(availability_consistency);

        // Economic behavior features
        features.push(patterns.economic_patterns.earning_consistency);
        features.push(patterns.economic_patterns.avg_stake_duration);
        features.push(patterns.economic_patterns.payment_punctuality);
        features.push(patterns.economic_patterns.bonding_curve_interactions as f64);
        features.push(patterns.economic_patterns.participation_score);

        // Service quality features
        features.push(patterns.service_quality.peer_rating_avg);
        features.push(patterns.service_quality.success_ratio);
        features.push(patterns.service_quality.avg_response_time_ms.ln().max(0.0));
        features.push(patterns.service_quality.reputation_accumulation);
        features.push((patterns.service_quality.total_services_completed as f64).ln().max(0.0));

        // Multi-chain features
        features.push(patterns.multi_chain_activity.cross_chain_tx_frequency);
        features.push(patterns.multi_chain_activity.bridge_usage_frequency);
        features.push(patterns.multi_chain_activity.identity_consistency);
        
        // Chain diversity
        let chain_diversity = patterns.multi_chain_activity.preferred_networks.len() as f64;
        features.push(chain_diversity);
        
        // Chain distribution entropy
        let chain_entropy = self.compute_entropy(&patterns.multi_chain_activity.chain_usage_distribution)?;
        features.push(chain_entropy);

        // Temporal features
        let days_since_collection = (Utc::now() - patterns.collected_at).num_days() as f64;
        features.push(days_since_collection);
        features.push(patterns.privacy_budget_used);

        Ok(Array1::from(features))
    }

    /// Recognize specific pattern types in behavioral data
    async fn recognize_pattern_types(
        &self,
        patterns: &BehavioralPatterns,
        feature_vector: &Array1<f64>,
    ) -> Result<Vec<RecognizedPattern>, Box<dyn Error>> {
        let mut recognized_patterns = Vec::new();

        // Daily routine pattern recognition
        let daily_routine = self.recognize_daily_routine(patterns)?;
        if let Some(pattern) = daily_routine {
            recognized_patterns.push(pattern);
        }

        // Stability pattern recognition
        let stability_pattern = self.recognize_stability_pattern(patterns)?;
        if let Some(pattern) = stability_pattern {
            recognized_patterns.push(pattern);
        }

        // Economic pattern recognition
        let economic_pattern = self.recognize_economic_pattern(patterns)?;
        if let Some(pattern) = economic_pattern {
            recognized_patterns.push(pattern);
        }

        // Multi-chain pattern recognition
        let multichain_pattern = self.recognize_multichain_pattern(patterns)?;
        if let Some(pattern) = multichain_pattern {
            recognized_patterns.push(pattern);
        }

        // Correlation pattern recognition
        let correlation_pattern = self.recognize_correlation_pattern(patterns, feature_vector)?;
        if let Some(pattern) = correlation_pattern {
            recognized_patterns.push(pattern);
        }

        Ok(recognized_patterns)
    }

    /// Recognize daily routine patterns
    fn recognize_daily_routine(&self, patterns: &BehavioralPatterns) -> Result<Option<RecognizedPattern>, Box<dyn Error>> {
        // Analyze hourly patterns for consistency
        let hourly_std = patterns.storage_behavior.preferred_storage_hours.std(0.0);
        let consistency_score = 1.0 / (1.0 + hourly_std);
        
        if consistency_score > 0.7 {
            // Find peak hours
            let peak_hours: Vec<u8> = patterns.storage_behavior.preferred_storage_hours
                .iter()
                .enumerate()
                .filter(|(_, val)| **val > 0.1) // Significant activity
                .map(|(hour, _)| hour as u8)
                .collect();

            return Ok(Some(RecognizedPattern {
                pattern_type: PatternType::DailyRoutine {
                    consistency_score,
                    peak_hours,
                },
                confidence: consistency_score,
                description: "Consistent daily activity pattern detected".to_string(),
                temporal_window: "24 hours".to_string(),
                affected_components: vec!["storage".to_string(), "compute".to_string()],
            }));
        }

        Ok(None)
    }

    /// Recognize stability patterns
    fn recognize_stability_pattern(&self, patterns: &BehavioralPatterns) -> Result<Option<RecognizedPattern>, Box<dyn Error>> {
        // Check consistency across different behavioral components
        let storage_consistency = patterns.storage_behavior.consistency_score;
        let economic_consistency = patterns.economic_patterns.earning_consistency;
        let service_consistency = patterns.service_quality.success_ratio;
        
        let overall_stability = (storage_consistency + economic_consistency + service_consistency) / 3.0;
        
        if overall_stability > 0.8 {
            return Ok(Some(RecognizedPattern {
                pattern_type: PatternType::StabilityPattern {
                    stability_score: overall_stability,
                    duration_days: 30, // Assume 30-day observation window
                },
                confidence: overall_stability,
                description: "High behavioral stability detected across components".to_string(),
                temporal_window: "30 days".to_string(),
                affected_components: vec!["storage".to_string(), "economic".to_string(), "service".to_string()],
            }));
        }

        Ok(None)
    }

    /// Recognize economic behavior patterns
    fn recognize_economic_pattern(&self, patterns: &BehavioralPatterns) -> Result<Option<RecognizedPattern>, Box<dyn Error>> {
        let earning_consistency = patterns.economic_patterns.earning_consistency;
        let payment_punctuality = patterns.economic_patterns.payment_punctuality;
        let participation_score = patterns.economic_patterns.participation_score;
        
        // Classify economic behavior
        let earning_pattern = if earning_consistency > 0.8 {
            "steady_earner"
        } else if earning_consistency > 0.5 {
            "variable_earner"
        } else {
            "irregular_earner"
        };

        let spending_pattern = if payment_punctuality > 0.9 {
            "prompt_payer"
        } else if payment_punctuality > 0.7 {
            "regular_payer"
        } else {
            "delayed_payer"
        };

        let risk_profile = if participation_score > 0.8 && earning_consistency > 0.7 {
            "low_risk"
        } else if participation_score > 0.5 {
            "medium_risk"
        } else {
            "high_risk"
        };

        let economic_confidence = (earning_consistency + payment_punctuality + participation_score) / 3.0;

        Ok(Some(RecognizedPattern {
            pattern_type: PatternType::EconomicPattern {
                earning_pattern: earning_pattern.to_string(),
                spending_pattern: spending_pattern.to_string(),
                risk_profile: risk_profile.to_string(),
            },
            confidence: economic_confidence,
            description: format!("Economic behavior: {} / {} / {}", earning_pattern, spending_pattern, risk_profile),
            temporal_window: "ongoing".to_string(),
            affected_components: vec!["economic".to_string()],
        }))
    }

    /// Recognize multi-chain usage patterns
    fn recognize_multichain_pattern(&self, patterns: &BehavioralPatterns) -> Result<Option<RecognizedPattern>, Box<dyn Error>> {
        let cross_chain_freq = patterns.multi_chain_activity.cross_chain_tx_frequency;
        let identity_consistency = patterns.multi_chain_activity.identity_consistency;
        let chain_count = patterns.multi_chain_activity.preferred_networks.len();
        
        if chain_count >= 2 && identity_consistency > 0.7 {
            let primary_chains = patterns.multi_chain_activity.preferred_networks.clone();
            let pattern_confidence = (cross_chain_freq + identity_consistency) / 2.0;
            
            return Ok(Some(RecognizedPattern {
                pattern_type: PatternType::MultiChainPattern {
                    primary_chains,
                    cross_chain_frequency: cross_chain_freq,
                    consistency_score: identity_consistency,
                },
                confidence: pattern_confidence,
                description: "Multi-chain user with consistent identity".to_string(),
                temporal_window: "ongoing".to_string(),
                affected_components: vec!["multi_chain".to_string()],
            }));
        }

        Ok(None)
    }

    /// Recognize correlation patterns between components
    fn recognize_correlation_pattern(
        &self,
        patterns: &BehavioralPatterns,
        feature_vector: &Array1<f64>,
    ) -> Result<Option<RecognizedPattern>, Box<dyn Error>> {
        // Compute correlations between different behavioral components
        let storage_score = patterns.storage_behavior.consistency_score;
        let compute_score = patterns.compute_participation.service_quality;
        let economic_score = patterns.economic_patterns.participation_score;
        let service_score = patterns.service_quality.success_ratio;
        
        // Check for strong correlations
        let correlations = vec![
            ("storage-compute", (storage_score - compute_score).abs()),
            ("storage-economic", (storage_score - economic_score).abs()),
            ("compute-service", (compute_score - service_score).abs()),
        ];

        let avg_correlation = correlations.iter().map(|(_, corr)| 1.0 - corr).sum::<f64>() / correlations.len() as f64;
        
        if avg_correlation > 0.8 {
            let correlated_components: Vec<String> = correlations
                .iter()
                .filter(|(_, corr)| (1.0 - corr) > 0.8)
                .map(|(name, _)| name.to_string())
                .collect();

            return Ok(Some(RecognizedPattern {
                pattern_type: PatternType::CorrelationPattern {
                    correlation_strength: avg_correlation,
                    correlated_components: correlated_components.clone(),
                },
                confidence: avg_correlation,
                description: "Strong correlations detected between behavioral components".to_string(),
                temporal_window: "current".to_string(),
                affected_components: correlated_components,
            }));
        }

        Ok(None)
    }

    /// Compute similarity scores to known patterns
    fn compute_similarity_scores(&self, feature_vector: &Array1<f64>) -> Result<HashMap<String, f64>, Box<dyn Error>> {
        let mut similarity_scores = HashMap::new();

        for (template_id, template) in &self.known_patterns {
            let similarity = self.similarity_model.compute_similarity(feature_vector, &template.feature_vector)?;
            similarity_scores.insert(template_id.clone(), similarity);
        }

        Ok(similarity_scores)
    }

    /// Perform clustering analysis
    fn perform_clustering_analysis(&mut self, feature_vector: &Array1<f64>) -> Result<ClusterInfo, Box<dyn Error>> {
        let cluster_id = self.clustering_model.assign_to_cluster(feature_vector)?;
        let distance_to_center = self.clustering_model.distance_to_cluster_center(feature_vector, cluster_id)?;
        let cluster_confidence = 1.0 / (1.0 + distance_to_center);
        
        Ok(ClusterInfo {
            cluster_id,
            cluster_confidence,
            distance_to_center,
            cluster_size: self.clustering_model.get_cluster_size(cluster_id),
            cluster_characteristics: vec![format!("Cluster {} characteristics", cluster_id)],
        })
    }

    /// Detect pattern-based anomalies
    fn detect_pattern_anomalies(
        &self,
        patterns: &BehavioralPatterns,
        feature_vector: &Array1<f64>,
    ) -> Result<Vec<String>, Box<dyn Error>> {
        let mut anomalies = Vec::new();

        // Check for unusual feature combinations
        if feature_vector.iter().any(|&x| x > 10.0 || x < -5.0) {
            anomalies.push("Extreme feature values detected".to_string());
        }

        // Check for inconsistent behavior
        let storage_consistency = patterns.storage_behavior.consistency_score;
        let economic_consistency = patterns.economic_patterns.earning_consistency;
        
        if (storage_consistency - economic_consistency).abs() > 0.5 {
            anomalies.push("Inconsistent behavior between storage and economic patterns".to_string());
        }

        Ok(anomalies)
    }

    /// Compute feature importance scores
    fn compute_feature_importance(
        &self,
        feature_vector: &Array1<f64>,
        patterns: &BehavioralPatterns,
    ) -> Result<HashMap<String, f64>, Box<dyn Error>> {
        let mut importance = HashMap::new();
        
        let feature_names = vec![
            "avg_daily_storage_gb", "consistency_score", "avg_retention_days",
            "geographic_entropy", "hourly_entropy", "peak_hour",
            "avg_daily_compute_hours", "avg_daily_bandwidth_gb", "service_quality",
            "availability_consistency", "earning_consistency", "avg_stake_duration", 
            "payment_punctuality", "bonding_curve_interactions", "participation_score",
            "peer_rating_avg", "success_ratio", "avg_response_time_log",
            "reputation_accumulation", "total_services_log", "cross_chain_tx_frequency",
            "bridge_usage_frequency", "identity_consistency", "chain_diversity", 
            "chain_entropy", "days_since_collection", "privacy_budget_used"
        ];

        for (i, &value) in feature_vector.iter().enumerate() {
            if i < feature_names.len() {
                let normalized_value = (value / (1.0 + value.abs())).abs();
                importance.insert(feature_names[i].to_string(), normalized_value);
            }
        }

        Ok(importance)
    }

    /// Calculate overall confidence in pattern recognition
    fn calculate_overall_confidence(
        &self,
        recognized_patterns: &[RecognizedPattern],
        similarity_scores: &HashMap<String, f64>,
        cluster_info: &ClusterInfo,
    ) -> Result<f64, Box<dyn Error>> {
        let pattern_confidence = if recognized_patterns.is_empty() {
            0.3
        } else {
            recognized_patterns.iter().map(|p| p.confidence).sum::<f64>() / recognized_patterns.len() as f64
        };

        let similarity_confidence = similarity_scores.values().max_by(|a, b| a.partial_cmp(b).unwrap()).unwrap_or(&0.0);
        let cluster_confidence = cluster_info.cluster_confidence;

        let overall_confidence = (pattern_confidence * 0.5 + similarity_confidence * 0.3 + cluster_confidence * 0.2)
            .max(0.0)
            .min(1.0);

        Ok(overall_confidence)
    }

    /// Compute entropy for diversity measurement
    fn compute_entropy(&self, distribution: &Array1<f64>) -> Result<f64, Box<dyn Error>> {
        let sum = distribution.sum();
        if sum <= 0.0 {
            return Ok(0.0);
        }

        let normalized = distribution / sum;
        let entropy = -normalized.iter()
            .filter(|&&p| p > 0.0)
            .map(|&p| p * p.ln())
            .sum::<f64>();

        Ok(entropy)
    }

    /// Update models with new training data
    pub async fn update_models(
        &mut self,
        patterns: &BehavioralPatterns,
        recognition_result: &RecognitionResult,
    ) -> Result<(), Box<dyn Error>> {
        if recognition_result.confidence > 0.7 {
            let feature_vector = self.extract_comprehensive_features(patterns)?;
            self.clustering_model.add_training_sample(feature_vector)?;
        }

        Ok(())
    }

    /// Check if recognizer is ready
    pub fn is_ready(&self) -> bool {
        self.model_ready
    }

    /// Set model as ready after training
    pub fn set_ready(&mut self, ready: bool) {
        self.model_ready = ready;
    }
}

impl SimilarityModel {
    pub fn new() -> Self {
        Self {
            feature_weights: Array1::ones(27),
            similarity_thresholds: SimilarityThresholds::default(),
        }
    }

    pub fn compute_similarity(&self, vector1: &Array1<f64>, vector2: &Array1<f64>) -> Result<f64, Box<dyn Error>> {
        let distance = ((vector1 - vector2).mapv(|x| x * x).sum()).sqrt();
        let similarity = 1.0 / (1.0 + distance);
        Ok(similarity)
    }
}

impl ClusteringModel {
    pub fn new(num_clusters: usize) -> Self {
        Self {
            cluster_centers: Array2::zeros((num_clusters, 27)),
            num_clusters,
            cluster_assignments: Vec::new(),
            inertia: 0.0,
        }
    }

    pub fn assign_to_cluster(&self, feature_vector: &Array1<f64>) -> Result<usize, Box<dyn Error>> {
        let mut min_distance = f64::INFINITY;
        let mut best_cluster = 0;

        for (i, center) in self.cluster_centers.axis_iter(Axis(0)).enumerate() {
            let distance = ((feature_vector - &center.to_owned()).mapv(|x| x * x).sum()).sqrt();
            if distance < min_distance {
                min_distance = distance;
                best_cluster = i;
            }
        }

        Ok(best_cluster)
    }

    pub fn distance_to_cluster_center(&self, feature_vector: &Array1<f64>, cluster_id: usize) -> Result<f64, Box<dyn Error>> {
        if cluster_id >= self.num_clusters {
            return Err("Invalid cluster ID".into());
        }

        let center = self.cluster_centers.row(cluster_id);
        let distance = ((feature_vector - &center.to_owned()).mapv(|x| x * x).sum()).sqrt();
        Ok(distance)
    }

    pub fn get_cluster_size(&self, cluster_id: usize) -> usize {
        self.cluster_assignments.iter().filter(|&&id| id == cluster_id).count()
    }

    pub fn add_training_sample(&mut self, feature_vector: Array1<f64>) -> Result<(), Box<dyn Error>> {
        let cluster_id = self.assign_to_cluster(&feature_vector)?;
        self.cluster_assignments.push(cluster_id);
        
        let cluster_size = self.get_cluster_size(cluster_id) as f64;
        let alpha = 1.0 / cluster_size.max(1.0);
        
        for (i, &value) in feature_vector.iter().enumerate() {
            self.cluster_centers[[cluster_id, i]] = 
                alpha * value + (1.0 - alpha) * self.cluster_centers[[cluster_id, i]];
        }

        Ok(())
    }
}

impl Default for SimilarityThresholds {
    fn default() -> Self {
        Self {
            high_confidence: 0.8,
            medium_confidence: 0.6,
            low_confidence: 0.4,
            no_match: 0.2,
        }
    }
}

impl Default for PatternRecognizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for PatternType {
    fn default() -> Self {
        Self::DailyRoutine {
            consistency_score: 0.5,
            peak_hours: Vec::new(),
        }
    }
}

impl Default for RecognizedPattern {
    fn default() -> Self {
        Self {
            pattern_type: PatternType::default(),
            confidence: 0.5,
            description: "Default pattern".to_string(),
            temporal_window: "Unknown".to_string(),
            affected_components: Vec::new(),
        }
    }
} 