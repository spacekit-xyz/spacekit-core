pub mod anomaly_detection;
pub mod pattern_recognition;
pub mod cortex_integration;
pub mod attack_detection;

use crate::behavioral::{BehavioralPatterns, BehavioralFingerprint, ConfidenceScore};
use std::error::Error;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};


// Re-export main AI types
pub use anomaly_detection::{AnomalyDetector, AnomalyReport, AnomalyType};
pub use pattern_recognition::{PatternRecognizer, PatternModel, RecognitionResult};
pub use cortex_integration::{CortexNode, CortexRequest, CortexResponse};
pub use attack_detection::{AttackDetector, AttackType, ThreatLevel};

/// Main AI system for behavioral analysis and security
pub struct BehavioralAI {
    anomaly_detector: AnomalyDetector,
    pattern_recognizer: PatternRecognizer,
    attack_detector: AttackDetector,
    cortex_node: Option<CortexNode>,
    learning_enabled: bool,
}

/// AI analysis result combining multiple detection systems
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIAnalysisResult {
    /// Anomaly detection results
    pub anomaly_report: AnomalyReport,
    /// Pattern recognition results  
    pub recognition_result: RecognitionResult,
    /// Attack detection results
    pub threat_assessment: ThreatAssessment,
    /// Overall AI confidence in the behavioral patterns
    pub ai_confidence: f64,
    /// Timestamp of analysis
    pub analyzed_at: DateTime<Utc>,
    /// Recommendations for the recovery system
    pub recommendations: Vec<AIRecommendation>,
}

/// Threat assessment from attack detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatAssessment {
    pub threat_level: ThreatLevel,
    pub detected_attacks: Vec<AttackType>,
    pub confidence: f64,
    pub risk_factors: Vec<String>,
}

/// AI recommendations for behavioral recovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AIRecommendation {
    pub recommendation_type: RecommendationType,
    pub description: String,
    pub confidence: f64,
    pub priority: Priority,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecommendationType {
    ApproveRecovery,
    DenyRecovery,
    RequireAdditionalVerification,
    IncreaseMonitoring,
    UpdateBehavioralModel,
    FlagForManualReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Medium,
    High,
    Critical,
}

impl BehavioralAI {
    /// Create new AI system with default configuration
    pub fn new() -> Self {
        Self {
            anomaly_detector: AnomalyDetector::new(),
            pattern_recognizer: PatternRecognizer::new(),
            attack_detector: AttackDetector::new(),
            cortex_node: None,
            learning_enabled: true,
        }
    }

    /// Create AI system with Cortex node integration
    pub fn with_cortex(cortex_endpoint: String) -> Result<Self, Box<dyn Error>> {
        let cortex_node = CortexNode::connect(cortex_endpoint)?;
        
        Ok(Self {
            anomaly_detector: AnomalyDetector::new(),
            pattern_recognizer: PatternRecognizer::new(),
            attack_detector: AttackDetector::new(),
            cortex_node: Some(cortex_node),
            learning_enabled: true,
        })
    }

    /// Perform comprehensive AI analysis of behavioral patterns
    pub async fn analyze_behavioral_patterns(
        &mut self,
        patterns: &BehavioralPatterns,
        fingerprint: &BehavioralFingerprint,
        _confidence_score: &ConfidenceScore,
        identity_did: &str,
    ) -> Result<AIAnalysisResult, Box<dyn Error>> {
        // Step 1: Anomaly detection
        let anomaly_report = self.anomaly_detector.detect_anomalies(patterns, identity_did).await?;
        
        // Step 2: Pattern recognition
        let recognition_result = self.pattern_recognizer.analyze_patterns(patterns, fingerprint).await?;
        
        // Step 3: Attack detection
        let threat_assessment = self.attack_detector.assess_threats(patterns, &anomaly_report).await?;
        
        // Step 4: Compute overall AI confidence
        let ai_confidence = self.compute_ai_confidence(&anomaly_report, &recognition_result, &threat_assessment)?;
        
        // Step 5: Generate recommendations
        let recommendations = self.generate_recommendations(&anomaly_report, &recognition_result, &threat_assessment, ai_confidence)?;
        
        // Step 6: Cortex node consultation if available
        if let Some(cortex) = &mut self.cortex_node {
            let cortex_response = cortex.consult_behavioral_analysis(patterns, &anomaly_report).await?;
            // Integrate cortex insights into the analysis
        }
        
        // Step 7: Learn from this analysis if enabled
        if self.learning_enabled {
            self.update_models(patterns, &anomaly_report, &recognition_result).await?;
        }

        Ok(AIAnalysisResult {
            anomaly_report,
            recognition_result,
            threat_assessment,
            ai_confidence,
            analyzed_at: Utc::now(),
            recommendations,
        })
    }

    /// Compute overall AI confidence combining all detection systems
    fn compute_ai_confidence(
        &self,
        anomaly_report: &AnomalyReport,
        recognition_result: &RecognitionResult,
        threat_assessment: &ThreatAssessment,
    ) -> Result<f64, Box<dyn Error>> {
        // Weight the different confidence scores
        let anomaly_weight = 0.4;
        let pattern_weight = 0.35;
        let threat_weight = 0.25;

        let anomaly_confidence = 1.0 - anomaly_report.anomaly_score; // Invert anomaly score
        let pattern_confidence = recognition_result.confidence;
        let threat_confidence = 1.0 - (threat_assessment.confidence * threat_assessment.threat_level.as_severity());

        let combined_confidence = (
            anomaly_confidence * anomaly_weight +
            pattern_confidence * pattern_weight +
            threat_confidence * threat_weight
        ).max(0.0).min(1.0);

        Ok(combined_confidence)
    }

    /// Generate AI recommendations based on analysis results
    fn generate_recommendations(
        &self,
        anomaly_report: &AnomalyReport,
        recognition_result: &RecognitionResult,
        threat_assessment: &ThreatAssessment,
        ai_confidence: f64,
    ) -> Result<Vec<AIRecommendation>, Box<dyn Error>> {
        let mut recommendations = Vec::new();

        // High confidence and low threats -> approve recovery
        if ai_confidence > 0.8 && threat_assessment.threat_level.as_severity() < 0.3 {
            recommendations.push(AIRecommendation {
                recommendation_type: RecommendationType::ApproveRecovery,
                description: "AI analysis indicates high confidence in behavioral authenticity".to_string(),
                confidence: ai_confidence,
                priority: Priority::High,
            });
        }

        // Low confidence or high threats -> deny recovery
        if ai_confidence < 0.4 || threat_assessment.threat_level.as_severity() > 0.7 {
            recommendations.push(AIRecommendation {
                recommendation_type: RecommendationType::DenyRecovery,
                description: "AI analysis indicates potential security risks or insufficient behavioral evidence".to_string(),
                confidence: 1.0 - ai_confidence,
                priority: Priority::Critical,
            });
        }

        // Medium confidence -> additional verification
        if ai_confidence >= 0.4 && ai_confidence <= 0.8 {
            recommendations.push(AIRecommendation {
                recommendation_type: RecommendationType::RequireAdditionalVerification,
                description: "AI analysis suggests additional verification steps are needed".to_string(),
                confidence: 0.8,
                priority: Priority::Medium,
            });
        }

        // High anomaly score -> increase monitoring
        if anomaly_report.anomaly_score > 0.6 {
            recommendations.push(AIRecommendation {
                recommendation_type: RecommendationType::IncreaseMonitoring,
                description: "Anomalous behavioral patterns detected, increased monitoring recommended".to_string(),
                confidence: anomaly_report.anomaly_score,
                priority: Priority::High,
            });
        }

        // Poor pattern recognition -> update models
        if recognition_result.confidence < 0.5 {
            recommendations.push(AIRecommendation {
                recommendation_type: RecommendationType::UpdateBehavioralModel,
                description: "Pattern recognition confidence low, model updates recommended".to_string(),
                confidence: 1.0 - recognition_result.confidence,
                priority: Priority::Medium,
            });
        }

        // Complex threat landscape -> manual review
        if threat_assessment.detected_attacks.len() > 2 {
            recommendations.push(AIRecommendation {
                recommendation_type: RecommendationType::FlagForManualReview,
                description: "Multiple attack vectors detected, manual security review required".to_string(),
                confidence: 0.9,
                priority: Priority::Critical,
            });
        }

        Ok(recommendations)
    }

    /// Update ML models based on new behavioral data
    async fn update_models(
        &mut self,
        patterns: &BehavioralPatterns,
        anomaly_report: &AnomalyReport,
        recognition_result: &RecognitionResult,
    ) -> Result<(), Box<dyn Error>> {
        // Update anomaly detection models
        self.anomaly_detector.update_models(patterns, anomaly_report).await?;
        
        // Update pattern recognition models
        self.pattern_recognizer.update_models(patterns, recognition_result).await?;
        
        // Update attack detection models
        self.attack_detector.update_models(patterns).await?;

        Ok(())
    }

    /// Real-time behavioral monitoring for ongoing identity verification
    pub async fn monitor_behavioral_changes(
        &self,
        current_patterns: &BehavioralPatterns,
        baseline_patterns: &BehavioralPatterns,
        _identity_did: &str,
    ) -> Result<Vec<AIRecommendation>, Box<dyn Error>> {
        let mut recommendations = Vec::new();

        // Detect significant behavioral deviations
        let deviation_score = self.compute_behavioral_deviation(current_patterns, baseline_patterns)?;
        
        if deviation_score > 0.7 {
            recommendations.push(AIRecommendation {
                recommendation_type: RecommendationType::IncreaseMonitoring,
                description: format!("Significant behavioral deviation detected (score: {:.2})", deviation_score),
                confidence: deviation_score,
                priority: Priority::High,
            });
        }

        // Check for potential attack patterns
        let attack_indicators = self.attack_detector.detect_realtime_attacks(current_patterns).await?;
        
        if !attack_indicators.is_empty() {
            recommendations.push(AIRecommendation {
                recommendation_type: RecommendationType::FlagForManualReview,
                description: "Real-time attack indicators detected".to_string(),
                confidence: 0.85,
                priority: Priority::Critical,
            });
        }

        Ok(recommendations)
    }

    /// Compute behavioral deviation between current and baseline patterns
    fn compute_behavioral_deviation(
        &self,
        current: &BehavioralPatterns,
        baseline: &BehavioralPatterns,
    ) -> Result<f64, Box<dyn Error>> {
        let mut deviations = Vec::new();

        // Storage behavior deviation
        let storage_dev = (current.storage_behavior.avg_daily_storage_gb - baseline.storage_behavior.avg_daily_storage_gb).abs()
            / baseline.storage_behavior.avg_daily_storage_gb.max(1.0);
        deviations.push(storage_dev);

        // Compute behavior deviation  
        let compute_dev = (current.compute_participation.avg_daily_compute_hours - baseline.compute_participation.avg_daily_compute_hours).abs()
            / baseline.compute_participation.avg_daily_compute_hours.max(1.0);
        deviations.push(compute_dev);

        // Economic behavior deviation
        let economic_dev = (current.economic_patterns.earning_consistency - baseline.economic_patterns.earning_consistency).abs();
        deviations.push(economic_dev);

        // Service quality deviation
        let service_dev = (current.service_quality.success_ratio - baseline.service_quality.success_ratio).abs();
        deviations.push(service_dev);

        // Multi-chain deviation
        let chain_dev = (current.multi_chain_activity.identity_consistency - baseline.multi_chain_activity.identity_consistency).abs();
        deviations.push(chain_dev);

        // Compute weighted average deviation
        let overall_deviation = deviations.iter().sum::<f64>() / deviations.len() as f64;
        Ok(overall_deviation.min(1.0))
    }

    /// Enable or disable learning from behavioral data
    pub fn set_learning_enabled(&mut self, enabled: bool) {
        self.learning_enabled = enabled;
    }

    /// Get current AI system status
    pub fn get_system_status(&self) -> AISystemStatus {
        AISystemStatus {
            anomaly_detector_ready: self.anomaly_detector.is_ready(),
            pattern_recognizer_ready: self.pattern_recognizer.is_ready(),
            attack_detector_ready: self.attack_detector.is_ready(),
            cortex_connected: self.cortex_node.is_some(),
            learning_enabled: self.learning_enabled,
        }
    }
}

/// AI system status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AISystemStatus {
    pub anomaly_detector_ready: bool,
    pub pattern_recognizer_ready: bool,
    pub attack_detector_ready: bool,
    pub cortex_connected: bool,
    pub learning_enabled: bool,
}

impl Default for BehavioralAI {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for ThreatAssessment {
    fn default() -> Self {
        Self {
            threat_level: ThreatLevel::Low,
            detected_attacks: Vec::new(),
            confidence: 0.5,
            risk_factors: Vec::new(),
        }
    }
}

impl Default for AIAnalysisResult {
    fn default() -> Self {
        Self {
            anomaly_report: AnomalyReport::default(),
            recognition_result: RecognitionResult::default(),
            threat_assessment: ThreatAssessment::default(),
            ai_confidence: 0.5,
            analyzed_at: chrono::Utc::now(),
            recommendations: Vec::new(),
        }
    }
}