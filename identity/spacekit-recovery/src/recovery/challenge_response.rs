// SWTCH Network Recovery: Challenge-Response Protocol
// Implements behavioral challenges for identity verification

use crate::behavioral::BehavioralPatterns;
use crate::ai::AIAnalysisResult;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::error::Error;

/// Challenge-response protocol for behavioral verification
pub struct ChallengeResponseProtocol {
    challenge_difficulty: f64,
    challenge_count: u32,
    timeout_minutes: u32,
}

/// Recovery challenge generated from behavioral patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryChallenge {
    pub challenge_id: String,
    pub challenge_type: String,
    pub challenge_data: ChallengeData,
    pub expected_response_type: ResponseType,
    pub difficulty_level: f64,
    pub generated_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Challenge data containing the specific behavioral questions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeData {
    pub storage_challenges: Vec<StorageChallenge>,
    pub compute_challenges: Vec<ComputeChallenge>,
    pub economic_challenges: Vec<EconomicChallenge>,
    pub service_challenges: Vec<ServiceChallenge>,
    pub temporal_challenges: Vec<TemporalChallenge>,
}

/// Storage behavior challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageChallenge {
    pub question_type: String,
    pub expected_range: (f64, f64),
    pub challenge_text: String,
}

/// Compute participation challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputeChallenge {
    pub metric: String,
    pub expected_pattern: Vec<f64>,
    pub tolerance: f64,
}

/// Economic behavior challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomicChallenge {
    pub behavior_type: String,
    pub expected_consistency: f64,
    pub verification_method: String,
}

/// Service quality challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceChallenge {
    pub service_metric: String,
    pub historical_performance: f64,
    pub acceptable_variance: f64,
}

/// Temporal pattern challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalChallenge {
    pub time_pattern: String,
    pub expected_frequency: f64,
    pub validation_period: String,
}

/// User response to recovery challenges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeResponse {
    pub challenge_id: String,
    pub response_data: String,
    pub response_timestamp: DateTime<Utc>,
    pub zero_knowledge_proof: Vec<u8>,
}

/// Challenge verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeVerificationResult {
    pub success: bool,
    pub total_challenges: u32,
    pub challenges_passed: u32,
    pub verification_score: f64,
    pub individual_results: Vec<IndividualChallengeResult>,
    pub verification_timestamp: DateTime<Utc>,
}

/// Individual challenge verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndividualChallengeResult {
    pub challenge_id: String,
    pub challenge_type: String,
    pub passed: bool,
    pub confidence_score: f64,
    pub explanation: String,
}

/// Response types for different challenge categories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseType {
    NumericValue,
    PatternMatch,
    TimeSeriesData,
    BehavioralFingerprint,
    ZeroKnowledgeProof,
}

impl ChallengeResponseProtocol {
    /// Create new challenge-response protocol
    pub fn new() -> Self {
        Self {
            challenge_difficulty: 0.7, // Configurable difficulty
            challenge_count: 8, // Number of challenges to generate
            timeout_minutes: 30, // Challenge timeout
        }
    }

    /// Generate behavioral challenges based on patterns and AI analysis
    pub async fn generate_challenges(
        &self,
        patterns: &BehavioralPatterns,
        _ai_analysis: &AIAnalysisResult,
        identity_did: &str,
    ) -> Result<Vec<RecoveryChallenge>, Box<dyn Error>> {
        let mut challenges = Vec::new();
        let challenge_base_id = format!("challenge_{}_{}", 
            identity_did.replace(":", "_"), 
            Utc::now().timestamp()
        );

        // Generate storage behavior challenges
        let storage_challenges = self.generate_storage_challenges(patterns)?;
        if !storage_challenges.is_empty() {
            challenges.push(RecoveryChallenge {
                challenge_id: format!("{}_storage", challenge_base_id),
                challenge_type: "StorageBehavior".to_string(),
                challenge_data: ChallengeData {
                    storage_challenges,
                    compute_challenges: vec![],
                    economic_challenges: vec![],
                    service_challenges: vec![],
                    temporal_challenges: vec![],
                },
                expected_response_type: ResponseType::NumericValue,
                difficulty_level: self.challenge_difficulty,
                generated_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::minutes(self.timeout_minutes as i64),
            });
        }

        // Generate compute participation challenges
        let compute_challenges = self.generate_compute_challenges(patterns)?;
        if !compute_challenges.is_empty() {
            challenges.push(RecoveryChallenge {
                challenge_id: format!("{}_compute", challenge_base_id),
                challenge_type: "ComputeParticipation".to_string(),
                challenge_data: ChallengeData {
                    storage_challenges: vec![],
                    compute_challenges,
                    economic_challenges: vec![],
                    service_challenges: vec![],
                    temporal_challenges: vec![],
                },
                expected_response_type: ResponseType::PatternMatch,
                difficulty_level: self.challenge_difficulty,
                generated_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::minutes(self.timeout_minutes as i64),
            });
        }

        // Generate economic behavior challenges
        let economic_challenges = self.generate_economic_challenges(patterns)?;
        if !economic_challenges.is_empty() {
            challenges.push(RecoveryChallenge {
                challenge_id: format!("{}_economic", challenge_base_id),
                challenge_type: "EconomicBehavior".to_string(),
                challenge_data: ChallengeData {
                    storage_challenges: vec![],
                    compute_challenges: vec![],
                    economic_challenges,
                    service_challenges: vec![],
                    temporal_challenges: vec![],
                },
                expected_response_type: ResponseType::BehavioralFingerprint,
                difficulty_level: self.challenge_difficulty,
                generated_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::minutes(self.timeout_minutes as i64),
            });
        }

        // Generate service quality challenges
        let service_challenges = self.generate_service_challenges(patterns)?;
        if !service_challenges.is_empty() {
            challenges.push(RecoveryChallenge {
                challenge_id: format!("{}_service", challenge_base_id),
                challenge_type: "ServiceQuality".to_string(),
                challenge_data: ChallengeData {
                    storage_challenges: vec![],
                    compute_challenges: vec![],
                    economic_challenges: vec![],
                    service_challenges,
                    temporal_challenges: vec![],
                },
                expected_response_type: ResponseType::NumericValue,
                difficulty_level: self.challenge_difficulty,
                generated_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::minutes(self.timeout_minutes as i64),
            });
        }

        // Generate temporal pattern challenges
        let temporal_challenges = self.generate_temporal_challenges(patterns)?;
        if !temporal_challenges.is_empty() {
            challenges.push(RecoveryChallenge {
                challenge_id: format!("{}_temporal", challenge_base_id),
                challenge_type: "TemporalPatterns".to_string(),
                challenge_data: ChallengeData {
                    storage_challenges: vec![],
                    compute_challenges: vec![],
                    economic_challenges: vec![],
                    service_challenges: vec![],
                    temporal_challenges,
                },
                expected_response_type: ResponseType::TimeSeriesData,
                difficulty_level: self.challenge_difficulty,
                generated_at: Utc::now(),
                expires_at: Utc::now() + chrono::Duration::minutes(self.timeout_minutes as i64),
            });
        }

        Ok(challenges)
    }

    /// Verify challenge responses against expected behavioral patterns
    pub async fn verify_responses(
        &self,
        challenges: &[RecoveryChallenge],
        responses: &[ChallengeResponse],
        patterns: &BehavioralPatterns,
    ) -> Result<ChallengeVerificationResult, Box<dyn Error>> {
        let mut individual_results = Vec::new();
        let mut total_score = 0.0;
        let mut passed_challenges = 0u32;

        // Create response lookup map
        let response_map: HashMap<String, &ChallengeResponse> = responses
            .iter()
            .map(|r| (r.challenge_id.clone(), r))
            .collect();

        // Verify each challenge
        for challenge in challenges {
            if let Some(response) = response_map.get(&challenge.challenge_id) {
                let result = self.verify_individual_challenge(challenge, response, patterns).await?;
                
                if result.passed {
                    passed_challenges += 1;
                }
                total_score += result.confidence_score;
                individual_results.push(result);
            } else {
                // No response provided for this challenge
                individual_results.push(IndividualChallengeResult {
                    challenge_id: challenge.challenge_id.clone(),
                    challenge_type: challenge.challenge_type.clone(),
                    passed: false,
                    confidence_score: 0.0,
                    explanation: "No response provided".to_string(),
                });
            }
        }

        let verification_score = if !challenges.is_empty() {
            total_score / challenges.len() as f64
        } else {
            0.0
        };

        let success = verification_score >= 0.6 && passed_challenges >= (challenges.len() as u32 * 2 / 3);

        Ok(ChallengeVerificationResult {
            success,
            total_challenges: challenges.len() as u32,
            challenges_passed: passed_challenges,
            verification_score,
            individual_results,
            verification_timestamp: Utc::now(),
        })
    }

    /// Verify individual challenge response
    async fn verify_individual_challenge(
        &self,
        challenge: &RecoveryChallenge,
        response: &ChallengeResponse,
        patterns: &BehavioralPatterns,
    ) -> Result<IndividualChallengeResult, Box<dyn Error>> {
        let mut confidence_score = 0.0;
        let mut passed = false;
        let explanation;

        match challenge.challenge_type.as_str() {
            "StorageBehavior" => {
                // Verify storage behavior response
                let expected_storage = patterns.storage_behavior.avg_daily_storage_gb;
                if let Ok(provided_storage) = response.response_data.parse::<f64>() {
                    let difference = (provided_storage - expected_storage).abs();
                    let tolerance = expected_storage * 0.2; // 20% tolerance
                    
                    if difference <= tolerance {
                        confidence_score = 1.0 - (difference / tolerance);
                        passed = confidence_score >= 0.6;
                        explanation = format!("Storage verification: provided {:.2} GB, expected {:.2} GB", 
                                            provided_storage, expected_storage);
                    } else {
                        explanation = "Storage response outside acceptable range".to_string();
                    }
                } else {
                    explanation = "Invalid storage response format".to_string();
                }
            },
            
            "ComputeParticipation" => {
                // Verify compute participation patterns
                let expected_compute = patterns.compute_participation.avg_daily_compute_hours;
                if let Ok(provided_compute) = response.response_data.parse::<f64>() {
                    let difference = (provided_compute - expected_compute).abs();
                    let tolerance = expected_compute * 0.25; // 25% tolerance
                    
                    if difference <= tolerance {
                        confidence_score = 1.0 - (difference / tolerance);
                        passed = confidence_score >= 0.6;
                        explanation = format!("Compute verification: provided {:.2} hours, expected {:.2} hours", 
                                            provided_compute, expected_compute);
                    } else {
                        explanation = "Compute response outside acceptable range".to_string();
                    }
                } else {
                    explanation = "Invalid compute response format".to_string();
                }
            },
            
            "EconomicBehavior" => {
                // Verify economic behavior consistency
                let expected_consistency = patterns.economic_patterns.earning_consistency;
                if let Ok(provided_consistency) = response.response_data.parse::<f64>() {
                    let difference = (provided_consistency - expected_consistency).abs();
                    
                    if difference <= 0.15 { // 15% tolerance for consistency scores
                        confidence_score = 1.0 - (difference / 0.15);
                        passed = confidence_score >= 0.6;
                        explanation = format!("Economic verification: provided {:.3} consistency, expected {:.3}", 
                                            provided_consistency, expected_consistency);
                    } else {
                        explanation = "Economic consistency response outside acceptable range".to_string();
                    }
                } else {
                    explanation = "Invalid economic response format".to_string();
                }
            },
            
            "ServiceQuality" => {
                // Verify service quality metrics
                let expected_rating = patterns.service_quality.peer_rating_avg;
                if let Ok(provided_rating) = response.response_data.parse::<f64>() {
                    let difference = (provided_rating - expected_rating).abs();
                    let tolerance = 0.5; // 0.5 point tolerance on 5-point scale
                    
                    if difference <= tolerance {
                        confidence_score = 1.0 - (difference / tolerance);
                        passed = confidence_score >= 0.6;
                        explanation = format!("Service verification: provided {:.2} rating, expected {:.2}", 
                                            provided_rating, expected_rating);
                    } else {
                        explanation = "Service quality response outside acceptable range".to_string();
                    }
                } else {
                    explanation = "Invalid service quality response format".to_string();
                }
            },
            
            "TemporalPatterns" => {
                // Verify temporal behavioral patterns
                // For demo, we'll check if response contains expected temporal keywords
                let response_lower = response.response_data.to_lowercase();
                if response_lower.contains("daily") || response_lower.contains("regular") || 
                   response_lower.contains("consistent") || response_lower.contains("pattern") {
                    confidence_score = 0.8;
                    passed = true;
                    explanation = "Temporal pattern verification: recognized behavioral keywords".to_string();
                } else {
                    explanation = "Temporal pattern response does not match expected patterns".to_string();
                }
            },
            
            _ => {
                explanation = format!("Unknown challenge type: {}", challenge.challenge_type);
            }
        }

        Ok(IndividualChallengeResult {
            challenge_id: challenge.challenge_id.clone(),
            challenge_type: challenge.challenge_type.clone(),
            passed,
            confidence_score,
            explanation,
        })
    }

    /// Generate storage behavior challenges
    fn generate_storage_challenges(&self, patterns: &BehavioralPatterns) -> Result<Vec<StorageChallenge>, Box<dyn Error>> {
        let mut challenges = Vec::new();

        // Challenge about average daily storage
        challenges.push(StorageChallenge {
            question_type: "average_daily_storage".to_string(),
            expected_range: (
                patterns.storage_behavior.avg_daily_storage_gb * 0.8,
                patterns.storage_behavior.avg_daily_storage_gb * 1.2
            ),
            challenge_text: "What is your typical daily storage contribution in GB?".to_string(),
        });

        // Challenge about storage consistency
        challenges.push(StorageChallenge {
            question_type: "storage_consistency".to_string(),
            expected_range: (
                patterns.storage_behavior.consistency_score * 0.9,
                patterns.storage_behavior.consistency_score * 1.1
            ),
            challenge_text: "Rate your storage contribution consistency (0.0 to 1.0)".to_string(),
        });

        Ok(challenges)
    }

    /// Generate compute participation challenges
    fn generate_compute_challenges(&self, patterns: &BehavioralPatterns) -> Result<Vec<ComputeChallenge>, Box<dyn Error>> {
        let mut challenges = Vec::new();

        challenges.push(ComputeChallenge {
            metric: "daily_compute_hours".to_string(),
            expected_pattern: vec![patterns.compute_participation.avg_daily_compute_hours],
            tolerance: 0.25,
        });

        challenges.push(ComputeChallenge {
            metric: "service_quality".to_string(),
            expected_pattern: vec![patterns.compute_participation.service_quality],
            tolerance: 0.15,
        });

        Ok(challenges)
    }

    /// Generate economic behavior challenges
    fn generate_economic_challenges(&self, patterns: &BehavioralPatterns) -> Result<Vec<EconomicChallenge>, Box<dyn Error>> {
        let mut challenges = Vec::new();

        challenges.push(EconomicChallenge {
            behavior_type: "earning_consistency".to_string(),
            expected_consistency: patterns.economic_patterns.earning_consistency,
            verification_method: "pattern_matching".to_string(),
        });

        challenges.push(EconomicChallenge {
            behavior_type: "payment_punctuality".to_string(),
            expected_consistency: patterns.economic_patterns.payment_punctuality,
            verification_method: "time_series_analysis".to_string(),
        });

        Ok(challenges)
    }

    /// Generate service quality challenges
    fn generate_service_challenges(&self, patterns: &BehavioralPatterns) -> Result<Vec<ServiceChallenge>, Box<dyn Error>> {
        let mut challenges = Vec::new();

        challenges.push(ServiceChallenge {
            service_metric: "peer_rating_average".to_string(),
            historical_performance: patterns.service_quality.peer_rating_avg,
            acceptable_variance: 0.3,
        });

        challenges.push(ServiceChallenge {
            service_metric: "success_ratio".to_string(),
            historical_performance: patterns.service_quality.success_ratio,
            acceptable_variance: 0.1,
        });

        Ok(challenges)
    }

    /// Generate temporal pattern challenges
    fn generate_temporal_challenges(&self, _patterns: &BehavioralPatterns) -> Result<Vec<TemporalChallenge>, Box<dyn Error>> {
        let mut challenges = Vec::new();

        challenges.push(TemporalChallenge {
            time_pattern: "daily_activity".to_string(),
            expected_frequency: 0.8, // 80% of days active
            validation_period: "30_days".to_string(),
        });

        challenges.push(TemporalChallenge {
            time_pattern: "weekly_consistency".to_string(),
            expected_frequency: 0.9, // 90% weekly consistency
            validation_period: "12_weeks".to_string(),
        });

        Ok(challenges)
    }
}
