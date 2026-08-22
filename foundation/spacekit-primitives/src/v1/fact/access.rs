//! Access control and authorization types for Fact Packages

use super::*;
use crate::v1::identity::QuantumDID;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Access decision result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessDecision {
    Allow,
    Deny(String), // Reason for denial
    Conditional(Vec<AccessRequirement>),
}

/// Access requirements that must be met
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessRequirement {
    pub requirement_type: RequirementType,
    pub description: String,
    pub deadline: Option<Timestamp>,
}

/// Types of access requirements
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RequirementType {
    PaymentRequired(u64), // Amount in SWTCH tokens
    IdentityVerification,
    DomainExpertise(KnowledgeDomain),
    TrustThreshold(f64),
    TimeBased(TimeWindow),
    GeographicRestriction(Vec<String>), // Allowed countries/regions
}

/// Time window for access restrictions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimeWindow {
    pub start: Timestamp,
    pub end: Timestamp,
    pub timezone: Option<String>,
}

/// Access attempt for audit logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessAttempt {
    pub requester: QuantumDID,
    pub fact_id: FactID,
    pub access_type: AccessType,
    pub decision: AccessDecision,
    pub timestamp: Timestamp,
    pub context: AccessContext,
}

/// Types of access operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessType {
    Read,
    Write,
    Update,
    Delete,
    Share,
    Reference,
}

/// Context information for access evaluation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessContext {
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub session_id: Option<String>,
    pub device_fingerprint: Option<String>,
    pub purpose: AccessPurpose,
}

/// Purpose of the access request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessPurpose {
    Research,
    Commercial,
    Educational,
    Personal,
    AITraining,
    Verification,
    Analysis,
}

/// Temporary access grant for time-limited permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporaryAccessGrant {
    pub id: String,
    pub granter: QuantumDID,
    pub grantee: QuantumDID,
    pub fact_id: FactID,
    pub granted_at: Timestamp,
    pub expires_at: Timestamp,
    pub conditions: Vec<AccessCondition>,
    pub status: GrantStatus,
}

/// Status of an access grant
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GrantStatus {
    Active,
    Expired,
    Revoked,
    Suspended,
}

/// Access control manager configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessControlConfig {
    pub default_policy: AccessPolicy,
    pub enable_audit_logging: bool,
    pub session_timeout_minutes: u32,
    pub max_failed_attempts: u32,
    pub rate_limiting: RateLimitConfig,
    pub geo_blocking: GeographicConfig,
}

/// Rate limiting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub requests_per_minute: u32,
    pub requests_per_hour: u32,
    pub burst_allowance: u32,
}

/// Geographic access configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeographicConfig {
    pub enable_geo_blocking: bool,
    pub allowed_countries: Vec<String>,
    pub blocked_countries: Vec<String>,
    pub require_vpn_detection: bool,
}

/// User attributes for attribute-based access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAttributes {
    pub clearance_level: Option<String>,
    pub department: Option<String>,
    pub roles: Vec<String>,
    pub certifications: Vec<String>,
    pub domain_expertise: Vec<KnowledgeDomain>,
    pub trust_score: f64,
    pub verification_status: VerificationStatus,
    pub custom_attributes: HashMap<String, String>,
}

/// User verification status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VerificationStatus {
    Unverified,
    EmailVerified,
    PhoneVerified,
    IdentityVerified,
    DomainExpertVerified,
    OrganizationVerified,
}

/// Role definition for role-based access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub description: String,
    pub permissions: Vec<Permission>,
    pub inherits_from: Vec<String>, // Parent roles
    pub valid_until: Option<Timestamp>,
}

/// Permission definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Permission {
    pub resource: String,
    pub action: String,
    pub conditions: Vec<String>,
}

impl AccessDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, AccessDecision::Allow)
    }

    pub fn requires_conditions(&self) -> bool {
        matches!(self, AccessDecision::Conditional(_))
    }
}

impl AccessPolicy {
    /// Check if the policy allows access for a given user
    pub fn allows_reader(&self, reader: &QuantumDID) -> bool {
        match self {
            AccessPolicy::Public => true,
            AccessPolicy::Private(authorized_users) => authorized_users.contains(reader),
            AccessPolicy::RoleBased(_) => false, // Requires role checking
            AccessPolicy::AttributeBased(_) => false, // Requires attribute checking
            AccessPolicy::Dynamic(_) => false,   // Requires dynamic evaluation
            AccessPolicy::Conditional(_) => false, // Requires condition evaluation
        }
    }

    /// Get the minimum trust score required by this policy
    pub fn minimum_trust_score(&self) -> Option<f64> {
        match self {
            AccessPolicy::AttributeBased(requirements) => requirements.minimum_trust_score,
            _ => None,
        }
    }
}
