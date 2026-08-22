//! Additional type definitions and utilities for Fact Packages

use super::*;
use crate::v1::identity::QuantumDID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Query parameters for searching facts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactQuery {
    pub author: Option<QuantumDID>,
    pub category: Option<FactCategory>,
    pub tags: Vec<String>,
    pub domain: Option<KnowledgeDomain>,
    pub content_type: Option<String>,
    pub text_search: Option<String>,
    pub verification_level: Option<VerificationLevel>,
    pub min_confidence: Option<f64>,
    pub created_after: Option<Timestamp>,
    pub created_before: Option<Timestamp>,
    pub depends_on: Option<FactID>,
    pub referenced_by: Option<FactID>,
    pub sort_by: SortCriteria,
    pub pagination: PaginationParams,
    pub requester: QuantumDID,
    pub start_time: Timestamp,
}

/// Sorting criteria for fact queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortCriteria {
    CreatedAt(SortOrder),
    Confidence(SortOrder),
    Relevance(SortOrder),
    AuthorReputation(SortOrder),
    Usage(SortOrder),
}

/// Sort order
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Pagination parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginationParams {
    pub offset: u64,
    pub limit: u64,
}

/// Query result structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactQueryResult {
    pub facts: Vec<FactPackage>,
    pub total_count: usize,
    pub query_metadata: QueryMetadata,
}

/// Query execution metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetadata {
    pub execution_time_ms: u64,
    pub filters_applied: u32,
    pub cache_hit: bool,
}

/// Verification result structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    pub signature_valid: bool,
    pub author_verified: bool,
    pub trust_score: f64,
    pub dependency_verification: DependencyVerification,
    pub overall_confidence: f64,
}

/// Dependency verification status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyVerification {
    pub all_dependencies_verified: bool,
    pub verified_count: usize,
    pub total_count: usize,
    pub failed_dependencies: Vec<FactID>,
}

/// Trust score calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    pub overall_score: f64,
    pub components: TrustScoreComponents,
    pub confidence: f64,
    pub last_updated: Timestamp,
}

/// Components of trust score calculation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScoreComponents {
    pub direct_trust: f64,
    pub global_reputation: f64,
    pub domain_expertise: f64,
    pub fact_quality: f64,
    pub propagated_trust: f64,
}

/// Context for trust evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustContext {
    pub domain: Option<KnowledgeDomain>,
    pub evaluation_purpose: String,
    pub risk_level: RiskLevel,
}

/// Risk levels for trust evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

/// Fact statistics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactStatistics {
    pub total_facts: u64,
    pub facts_by_category: HashMap<FactCategory, u64>,
    pub facts_by_domain: HashMap<KnowledgeDomain, u64>,
    pub average_confidence: f64,
    pub verification_rate: f64,
    pub storage_usage_bytes: u64,
}

/// Performance metrics for the fact system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub storage_metrics: StorageMetrics,
    pub query_metrics: QueryMetrics,
    pub verification_metrics: VerificationMetrics,
    pub network_metrics: NetworkMetrics,
}

/// Storage performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageMetrics {
    pub total_facts_stored: u64,
    pub storage_utilization: f64,
    pub average_fact_size: u64,
    pub compression_efficiency: f64,
}

/// Query performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetrics {
    pub average_query_time: f64,
    pub query_throughput: f64,
    pub cache_hit_rate: f64,
    pub index_efficiency: f64,
}

/// Verification performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationMetrics {
    pub average_verification_time: f64,
    pub verification_success_rate: f64,
    pub peer_review_completion_rate: f64,
}

/// Network performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkMetrics {
    pub cross_chain_latency: f64,
    pub consensus_time: f64,
    pub bandwidth_utilization: f64,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 50,
        }
    }
}

impl Default for SortCriteria {
    fn default() -> Self {
        Self::CreatedAt(SortOrder::Descending)
    }
}
