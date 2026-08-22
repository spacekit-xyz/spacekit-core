use crate::behavioral::BehavioralPatterns;
use crate::ai::anomaly_detection::AnomalyReport;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use ndarray::Array1;
use std::collections::HashMap;
use std::error::Error;

/// Attack detection system for behavioral security threats
pub struct AttackDetector {
    /// Attack pattern signatures
    attack_signatures: HashMap<String, AttackSignature>,
    /// Detection sensitivity settings
    sensitivity_settings: SensitivitySettings,
    /// Learning parameters
    learning_enabled: bool,
}

/// Attack signature for pattern matching
#[derive(Debug, Clone)]
pub struct AttackSignature {
    pub signature_id: String,
    pub attack_type: AttackType,
    pub feature_pattern: Array1<f64>,
    pub confidence_threshold: f64,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

/// Types of behavioral attacks that can be detected
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttackType {
    /// Sybil attack with multiple fake identities
    SybilAttack {
        identity_count: u32,
        correlation_strength: f64,
    },
    /// Artificial inflation of behavioral metrics
    BehavioralInflation {
        inflated_components: Vec<String>,
        inflation_factor: f64,
    },
    /// Economic manipulation attacks
    EconomicManipulation {
        manipulation_type: String,
        economic_impact: f64,
    },
    /// Reputation manipulation attacks
    ReputationManipulation {
        reputation_inflation: f64,
        fake_endorsements: u32,
    },
    /// Coordinated manipulation by multiple actors
    CoordinatedManipulation {
        participant_count: u32,
        coordination_score: f64,
    },
    /// Cross-chain identity manipulation
    CrossChainManipulation {
        affected_chains: Vec<String>,
        inconsistency_score: f64,
    },
    /// Temporal pattern manipulation
    TemporalManipulation {
        time_windows: Vec<String>,
        pattern_deviation: f64,
    },
    /// Eclipse attack targeting network isolation
    EclipseAttack {
        network_isolation_score: f64,
        affected_connections: u32,
    },
}

/// Threat level classification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThreatLevel {
    None,
    Low,
    Medium,
    High,
    Critical,
}

/// Sensitivity settings for attack detection
#[derive(Debug, Clone)]
pub struct SensitivitySettings {
    pub sybil_threshold: f64,
    pub inflation_threshold: f64,
    pub coordination_threshold: f64,
    pub economic_threshold: f64,
    pub reputation_threshold: f64,
}

impl AttackDetector {
    /// Create new attack detector with default settings
    pub fn new() -> Self {
        Self {
            attack_signatures: Self::initialize_attack_signatures(),
            sensitivity_settings: SensitivitySettings::default(),
            learning_enabled: true,
        }
    }

    /// Initialize default attack signatures
    fn initialize_attack_signatures() -> HashMap<String, AttackSignature> {
        let mut signatures = HashMap::new();

        // Sybil attack signature
        signatures.insert("sybil_basic".to_string(), AttackSignature {
            signature_id: "sybil_basic".to_string(),
            attack_type: AttackType::SybilAttack {
                identity_count: 1,
                correlation_strength: 0.0,
            },
            feature_pattern: Array1::from(vec![0.1, 0.1, 0.9, 0.1, 0.1]), // Low activity, high correlation
            confidence_threshold: 0.7,
            description: "Basic Sybil attack pattern".to_string(),
            created_at: Utc::now(),
        });

        // Behavioral inflation signature
        signatures.insert("behavioral_inflation".to_string(), AttackSignature {
            signature_id: "behavioral_inflation".to_string(),
            attack_type: AttackType::BehavioralInflation {
                inflated_components: vec!["storage".to_string()],
                inflation_factor: 2.0,
            },
            feature_pattern: Array1::from(vec![0.9, 0.3, 0.3, 0.9, 0.3]), // High storage, low other metrics
            confidence_threshold: 0.75,
            description: "Artificial behavioral metric inflation".to_string(),
            created_at: Utc::now(),
        });

        signatures
    }

    /// Assess threats in behavioral patterns
    pub async fn assess_threats(
        &mut self,
        patterns: &BehavioralPatterns,
        anomaly_report: &AnomalyReport,
    ) -> Result<crate::ai::ThreatAssessment, Box<dyn Error>> {
        // Extract threat assessment features
        let threat_features = self.extract_threat_features(patterns, anomaly_report)?;
        
        // Detect specific attack types
        let detected_attacks = self.detect_attack_types(patterns, &threat_features).await?;
        
        // Assess overall threat level
        let threat_level = self.assess_overall_threat_level(&detected_attacks, anomaly_report)?;
        
        // Calculate confidence in threat assessment
        let confidence = self.calculate_threat_confidence(&detected_attacks, &threat_features)?;
        
        // Generate risk factors
        let risk_factors = self.identify_risk_factors(patterns, anomaly_report, &detected_attacks)?;

        Ok(crate::ai::ThreatAssessment {
            threat_level,
            detected_attacks,
            confidence,
            risk_factors,
        })
    }

    /// Extract features relevant to threat detection
    fn extract_threat_features(
        &self,
        patterns: &BehavioralPatterns,
        anomaly_report: &AnomalyReport,
    ) -> Result<Array1<f64>, Box<dyn Error>> {
        let mut features = Vec::new();

        // Behavioral consistency features
        features.push(patterns.storage_behavior.consistency_score);
        features.push(patterns.compute_participation.service_quality);
        features.push(patterns.economic_patterns.earning_consistency);
        features.push(patterns.service_quality.success_ratio);
        features.push(patterns.multi_chain_activity.identity_consistency);

        // Anomaly indicators
        features.push(anomaly_report.anomaly_score);
        features.push(anomaly_report.component_scores.storage_anomaly);
        features.push(anomaly_report.component_scores.compute_anomaly);
        features.push(anomaly_report.component_scores.economic_anomaly);
        features.push(anomaly_report.component_scores.service_anomaly);

        Ok(Array1::from(features))
    }

    /// Detect specific attack types in behavioral data
    async fn detect_attack_types(
        &mut self,
        patterns: &BehavioralPatterns,
        threat_features: &Array1<f64>,
    ) -> Result<Vec<AttackType>, Box<dyn Error>> {
        let mut detected_attacks = Vec::new();

        // Check each attack signature
        for (_, signature) in &self.attack_signatures {
            let match_score = self.calculate_signature_match(threat_features, &signature.feature_pattern)?;
            
            if match_score > signature.confidence_threshold {
                let attack = match &signature.attack_type {
                    AttackType::SybilAttack { .. } => {
                        self.detect_sybil_attack(patterns, match_score).await?
                    }
                    AttackType::BehavioralInflation { .. } => {
                        self.detect_behavioral_inflation(patterns, match_score).await?
                    }
                    _ => None,
                };

                if let Some(attack_type) = attack {
                    detected_attacks.push(attack_type);
                }
            }
        }

        Ok(detected_attacks)
    }

    /// Detect Sybil attacks
    async fn detect_sybil_attack(
        &self,
        patterns: &BehavioralPatterns,
        match_score: f64,
    ) -> Result<Option<AttackType>, Box<dyn Error>> {
        // Look for patterns indicating multiple fake identities
        let low_service_quality = patterns.service_quality.success_ratio < 0.3;
        let low_reputation = patterns.service_quality.reputation_accumulation < 0.2;
        let minimal_services = patterns.service_quality.total_services_completed < 10;
        
        if low_service_quality && low_reputation && minimal_services && match_score > 0.7 {
            return Ok(Some(AttackType::SybilAttack {
                identity_count: 1,
                correlation_strength: match_score,
            }));
        }

        Ok(None)
    }

    /// Detect behavioral inflation attacks
    async fn detect_behavioral_inflation(
        &self,
        patterns: &BehavioralPatterns,
        match_score: f64,
    ) -> Result<Option<AttackType>, Box<dyn Error>> {
        let mut inflated_components = Vec::new();
        let mut max_inflation = 1.0;

        // Check for unusual ratios between components
        if patterns.storage_behavior.avg_daily_storage_gb > 100.0 && 
           patterns.service_quality.success_ratio < 0.5 {
            inflated_components.push("storage".to_string());
            max_inflation = patterns.storage_behavior.avg_daily_storage_gb / 50.0;
        }

        if patterns.compute_participation.avg_daily_compute_hours > 20.0 && 
           patterns.service_quality.peer_rating_avg < 3.0 {
            inflated_components.push("compute".to_string());
            max_inflation = max_inflation.max(patterns.compute_participation.avg_daily_compute_hours / 8.0);
        }

        if !inflated_components.is_empty() && match_score > 0.75 {
            return Ok(Some(AttackType::BehavioralInflation {
                inflated_components,
                inflation_factor: max_inflation,
            }));
        }

        Ok(None)
    }

    /// Assess overall threat level
    fn assess_overall_threat_level(
        &self,
        detected_attacks: &[AttackType],
        anomaly_report: &AnomalyReport,
    ) -> Result<ThreatLevel, Box<dyn Error>> {
        if detected_attacks.is_empty() && anomaly_report.anomaly_score < 0.3 {
            return Ok(ThreatLevel::None);
        }

        let attack_severity: f64 = detected_attacks.iter().map(|attack| {
            match attack {
                AttackType::SybilAttack { correlation_strength, .. } => *correlation_strength,
                AttackType::BehavioralInflation { inflation_factor, .. } => (*inflation_factor - 1.0).min(1.0),
                AttackType::EconomicManipulation { economic_impact, .. } => *economic_impact,
                AttackType::ReputationManipulation { reputation_inflation, .. } => (*reputation_inflation / 2.0).min(1.0),
                AttackType::CoordinatedManipulation { coordination_score, .. } => *coordination_score,
                AttackType::CrossChainManipulation { inconsistency_score, .. } => *inconsistency_score,
                AttackType::TemporalManipulation { pattern_deviation, .. } => (*pattern_deviation / 2.0).min(1.0),
                AttackType::EclipseAttack { network_isolation_score, .. } => *network_isolation_score,
            }
        }).sum::<f64>() / detected_attacks.len() as f64;

        let combined_severity = (attack_severity + anomaly_report.anomaly_score) / 2.0;

        let threat_level = match combined_severity {
            x if x >= 0.8 => ThreatLevel::Critical,
            x if x >= 0.6 => ThreatLevel::High,
            x if x >= 0.4 => ThreatLevel::Medium,
            x if x >= 0.2 => ThreatLevel::Low,
            _ => ThreatLevel::None,
        };

        Ok(threat_level)
    }

    /// Calculate confidence in threat assessment
    fn calculate_threat_confidence(
        &self,
        detected_attacks: &[AttackType],
        threat_features: &Array1<f64>,
    ) -> Result<f64, Box<dyn Error>> {
        if detected_attacks.is_empty() {
            return Ok(0.9); // High confidence in no threats
        }

        let feature_quality = threat_features.iter()
            .map(|&x| if x.is_finite() { 1.0 } else { 0.0 })
            .sum::<f64>() / threat_features.len() as f64;

        let attack_confidence = detected_attacks.len() as f64 * 0.2;
        
        let combined_confidence = (feature_quality + attack_confidence).min(1.0);
        Ok(combined_confidence)
    }

    /// Identify risk factors contributing to threats
    fn identify_risk_factors(
        &self,
        patterns: &BehavioralPatterns,
        anomaly_report: &AnomalyReport,
        detected_attacks: &[AttackType],
    ) -> Result<Vec<String>, Box<dyn Error>> {
        let mut risk_factors = Vec::new();

        if patterns.service_quality.success_ratio < 0.5 {
            risk_factors.push("Low service success ratio".to_string());
        }

        if anomaly_report.anomaly_score > 0.7 {
            risk_factors.push("High behavioral anomaly score".to_string());
        }

        for attack in detected_attacks {
            match attack {
                AttackType::SybilAttack { .. } => {
                    risk_factors.push("Potential Sybil attack indicators".to_string());
                }
                AttackType::BehavioralInflation { .. } => {
                    risk_factors.push("Artificial behavioral metric inflation".to_string());
                }
                _ => {
                    risk_factors.push("Advanced attack patterns detected".to_string());
                }
            }
        }

        Ok(risk_factors)
    }

    /// Real-time attack detection for ongoing monitoring
    pub async fn detect_realtime_attacks(
        &self,
        patterns: &BehavioralPatterns,
    ) -> Result<Vec<AttackType>, Box<dyn Error>> {
        let mut realtime_attacks = Vec::new();

        // Quick checks for immediate threats
        if patterns.service_quality.success_ratio == 0.0 && 
           patterns.service_quality.total_services_completed > 50 {
            realtime_attacks.push(AttackType::ReputationManipulation {
                reputation_inflation: 5.0,
                fake_endorsements: 50,
            });
        }

        Ok(realtime_attacks)
    }

    /// Helper method for signature matching
    fn calculate_signature_match(&self, features: &Array1<f64>, signature: &Array1<f64>) -> Result<f64, Box<dyn Error>> {
        if features.len() != signature.len() {
            let min_len = features.len().min(signature.len());
            let feature_subset = features.slice(ndarray::s![..min_len]);
            let signature_subset = signature.slice(ndarray::s![..min_len]);
            
            let distance = (&feature_subset.to_owned() - &signature_subset.to_owned()).mapv(|x| x * x).sum().sqrt();
            return Ok(1.0 / (1.0 + distance));
        }

        let distance = (features - signature).mapv(|x| x * x).sum().sqrt();
        Ok(1.0 / (1.0 + distance))
    }

    /// Update models with new attack data
    pub async fn update_models(&mut self, _patterns: &BehavioralPatterns) -> Result<(), Box<dyn Error>> {
        // Model update logic would go here
        Ok(())
    }

    /// Check if detector is ready
    pub fn is_ready(&self) -> bool {
        !self.attack_signatures.is_empty()
    }
}

impl Default for SensitivitySettings {
    fn default() -> Self {
        Self {
            sybil_threshold: 0.7,
            inflation_threshold: 0.75,
            coordination_threshold: 0.8,
            economic_threshold: 0.8,
            reputation_threshold: 0.7,
        }
    }
}

impl Default for AttackDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreatLevel {
    /// Convert threat level to numeric severity for calculations
    pub fn as_severity(&self) -> f64 {
        match self {
            ThreatLevel::None => 0.0,
            ThreatLevel::Low => 0.2,
            ThreatLevel::Medium => 0.5,
            ThreatLevel::High => 0.8,
            ThreatLevel::Critical => 1.0,
        }
    }
} 