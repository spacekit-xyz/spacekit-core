//! Verification and peer review types for Fact Packages

use super::*;
use crate::v1::identity::QuantumDID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export VerificationStatus from access module for convenience
pub use crate::v1::fact::access::VerificationStatus;

/// Peer review request structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRequest {
    pub id: String,
    pub fact_id: FactID,
    pub requester: QuantumDID,
    pub requirements: ReviewRequirements,
    pub reward_amount: u64, // SWTCH tokens
    pub deadline: Timestamp,
    pub qualified_reviewers: Vec<QuantumDID>,
    pub status: ReviewStatus,
}

/// Review requirements specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRequirements {
    pub minimum_reviewers: u32,
    pub review_period_days: u32,
    pub domain_expertise_required: bool,
    pub required_qualifications: Vec<String>,
    pub conflict_of_interest_check: bool,
}

/// Review status tracking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReviewStatus {
    Open,
    InProgress,
    Completed,
    Expired,
    Cancelled,
}

/// Peer review submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReview {
    pub id: String,
    pub fact_id: FactID,
    pub reviewer: QuantumDID,
    pub review_content: PeerReviewContent,
    pub submitted_at: Timestamp,
    pub signature: Vec<u8>, // Quantum-safe signature
    pub verification_status: ReviewVerificationStatus,
}

/// Detailed peer review content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerReviewContent {
    pub accuracy_score: f64,     // 0.0 to 1.0
    pub completeness_score: f64, // 0.0 to 1.0
    pub relevance_score: f64,    // 0.0 to 1.0
    pub methodology_score: f64,  // 0.0 to 1.0
    pub overall_recommendation: ReviewRecommendation,
    pub detailed_comments: String,
    pub suggested_improvements: Vec<String>,
    pub confidence_in_review: f64, // Reviewer's confidence in their own review
    pub time_spent_hours: Option<f64>,
}

/// Review recommendation types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReviewRecommendation {
    Accept,
    AcceptWithMinorRevisions,
    AcceptWithMajorRevisions,
    Reject,
    NeedsMoreInformation,
}

/// Review verification status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReviewVerificationStatus {
    Pending,
    Verified,
    Invalid,
    Disputed,
}

/// Consensus calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusScore {
    pub score: f64,
    pub agreement_level: f64,
    pub review_count: usize,
    pub total_reviewer_weight: f64,
    pub confidence_interval: ConfidenceInterval,
    pub outlier_reviews: Vec<String>, // Review IDs of outliers
}

/// Statistical confidence interval
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfidenceInterval {
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub confidence_level: f64, // e.g., 0.95 for 95%
}

/// Reviewer reputation and qualifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewerReputation {
    pub overall_score: f64,
    pub review_count: u32,
    pub accuracy_history: Vec<f64>,
    pub domain_expertise: HashMap<KnowledgeDomain, ExpertiseLevel>,
    pub peer_ratings: Vec<PeerRating>,
    pub conflict_of_interest_history: Vec<ConflictRecord>,
}

/// Expertise level in a domain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertiseLevel {
    pub score: f64,
    pub evidence: Vec<ExpertiseEvidence>,
    pub last_updated: Timestamp,
}

/// Evidence of expertise
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertiseEvidence {
    pub evidence_type: EvidenceType,
    pub description: String,
    pub verification_status: VerificationStatus,
    pub weight: f64,
}

/// Types of expertise evidence
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EvidenceType {
    AcademicDegree,
    ProfessionalCertification,
    PublishedResearch,
    WorkExperience,
    PeerEndorsement,
    TrainingCompletion,
}

/// Peer rating of a reviewer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRating {
    pub rater: QuantumDID,
    pub rating: f64,
    pub feedback: String,
    pub timestamp: Timestamp,
}

/// Conflict of interest record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictRecord {
    pub conflict_type: ConflictType,
    pub description: String,
    pub disclosed: bool,
    pub timestamp: Timestamp,
}

/// Types of conflicts of interest
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConflictType {
    Financial,
    Professional,
    Personal,
    Institutional,
    Competitive,
}

/// Trust relationship between entities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustRelationship {
    pub evaluator: QuantumDID,
    pub target: QuantumDID,
    pub trust_level: f64,
    pub relationship_type: TrustRelationshipType,
    pub established_at: Timestamp,
    pub last_updated: Timestamp,
    pub evidence: Vec<TrustEvidence>,
}

/// Types of trust relationships
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrustRelationshipType {
    Direct,        // First-hand experience
    Transitive,    // Through mutual connections
    Reputation,    // Based on public reputation
    Institutional, // Based on institutional affiliation
}

/// Evidence supporting trust relationship
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustEvidence {
    pub evidence_type: TrustEvidenceType,
    pub weight: f64,
    pub description: String,
    pub timestamp: Timestamp,
}

/// Types of trust evidence
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrustEvidenceType {
    SuccessfulCollaboration,
    QualityWork,
    Reliability,
    Expertise,
    Integrity,
    Recommendation,
}

/// Fact quality history for reputation calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactQualityHistory {
    pub author: QuantumDID,
    pub fact_performances: Vec<FactPerformance>,
    pub average_quality_score: f64,
    pub trend: QualityTrend,
    pub last_updated: Timestamp,
}

/// Individual fact performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactPerformance {
    pub fact_id: FactID,
    pub peer_review_scores: Vec<f64>,
    pub usage_frequency: u32,
    pub citation_count: u32,
    pub correction_needed: bool,
    pub time_to_consensus: Option<u64>, // seconds
}

/// Quality trend analysis
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QualityTrend {
    Improving,
    Stable,
    Declining,
    Inconsistent,
}

impl ConsensusScore {
    /// Check if consensus has been reached based on thresholds
    pub fn has_consensus(&self, threshold: f64, min_agreement: f64) -> bool {
        self.score >= threshold && self.agreement_level >= min_agreement
    }

    /// Get the consensus strength as a qualitative measure
    pub fn consensus_strength(&self) -> ConsensusStrength {
        match (self.score, self.agreement_level) {
            (s, a) if s >= 0.9 && a >= 0.8 => ConsensusStrength::Strong,
            (s, a) if s >= 0.75 && a >= 0.7 => ConsensusStrength::Moderate,
            (s, a) if s >= 0.6 && a >= 0.6 => ConsensusStrength::Weak,
            _ => ConsensusStrength::None,
        }
    }
}

/// Qualitative consensus strength
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConsensusStrength {
    Strong,
    Moderate,
    Weak,
    None,
}

impl ReviewerReputation {
    /// Calculate overall trustworthiness weight for review aggregation
    pub fn calculate_weight(&self) -> f64 {
        let base_weight = self.overall_score;
        let experience_factor = (self.review_count as f64).ln().max(1.0) / 10.0;
        let consistency_factor = self.calculate_consistency_factor();

        base_weight * (1.0 + experience_factor) * consistency_factor
    }

    fn calculate_consistency_factor(&self) -> f64 {
        if self.accuracy_history.len() < 2 {
            return 1.0;
        }

        let variance = self.calculate_variance();
        1.0 / (1.0 + variance * 2.0) // Lower variance = higher consistency factor
    }

    fn calculate_variance(&self) -> f64 {
        let mean = self.accuracy_history.iter().sum::<f64>() / self.accuracy_history.len() as f64;
        let variance = self
            .accuracy_history
            .iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>()
            / self.accuracy_history.len() as f64;
        variance
    }
}
