//! Fact Package Primitives for SWTCH Network
//!
//! This module provides the core data structures and types for the SWTCH Fact Package system,
//! enabling quantum-safe knowledge verification and storage.

pub mod access;
pub mod content;
pub mod types;
pub mod verification;

use crate::v1::crypto::quantum::SPHINCSSignature;
use crate::v1::identity::QuantumDID;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Core Fact Package structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactPackage {
    // Core Identification
    pub fact_id: FactID,
    pub version: FactVersion,
    pub created_at: Timestamp,
    pub expires_at: Option<Timestamp>,

    // Content
    pub content: FactContent,
    pub metadata: FactMetadata,

    // Verification
    pub author: QuantumDID,
    pub signature: SPHINCSSignature,
    pub verification_proof: VerificationProof,

    // Relationships
    pub dependencies: Vec<FactID>,
    pub citations: Vec<Citation>,
    pub confidence_score: ConfidenceScore,

    // Access Control
    pub access_policy: AccessPolicy,
    pub encryption: Option<QuantumEncryption>,
}

/// Unique identifier for a fact (SHA-256 hash of content + author + timestamp)
pub type FactID = [u8; 32];
pub type FactVersion = u32;
pub type Timestamp = u64;
pub type ConfidenceScore = f64; // 0.0 to 1.0

/// Fact content types supporting various data formats
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FactContent {
    /// Basic text content
    Text {
        content: String,
        language: Option<String>,
        encoding: TextEncoding,
    },

    /// Numerical data with precision
    Numerical {
        value: String, // Using string to handle BigDecimal
        unit: Option<String>,
        precision: u8,
    },

    /// Boolean value with certainty
    Boolean {
        value: bool,
        certainty: f64, // 0.0 to 1.0
    },

    /// Structured JSON data
    Json {
        data: serde_json::Value,
        schema: Option<String>, // JSON Schema reference
    },

    /// Binary data with metadata
    Binary {
        data: Vec<u8>,
        mime_type: String,
        hash: [u8; 32], // SHA-256 hash
    },

    /// Reference to another fact
    Reference {
        target_fact_id: FactID,
        relationship_type: RelationshipType,
        context: Option<String>,
    },

    /// Aggregation of multiple facts
    Aggregation {
        source_facts: Vec<FactID>,
        aggregation_method: AggregationMethod,
        result: Box<FactContent>,
    },
}

/// Text encoding types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TextEncoding {
    UTF8,
    ASCII,
    Latin1,
}

/// Relationship types between facts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RelationshipType {
    Citation,
    Contradiction,
    Support,
    Derivation,
    Update,
    DuplicateContent,
    Semantic,
    Temporal,
}

/// Aggregation methods for combining facts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AggregationMethod {
    CuratedCollection,
    WeightedAverage,
    Consensus,
    Summary,
    Synthesis,
}

/// Fact metadata containing classification and provenance information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FactMetadata {
    pub category: FactCategory,
    pub tags: Vec<String>,
    pub domain: KnowledgeDomain,
    pub source: DataSource,
    pub collection_method: CollectionMethod,
    pub verification_level: VerificationLevel,
    pub license: LicenseType,
    pub size_bytes: u64,
    pub checksum: [u8; 32], // SHA-256
}

/// Fact categories for classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FactCategory {
    Scientific,
    Historical,
    Statistical,
    Legal,
    Medical,
    Financial,
    Technical,
    Geographic,
    Biographical,
    Reference,
    Opinion,
    Prediction,
    Enterprise,
    UserGenerated,
}

/// Knowledge domains for subject classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum KnowledgeDomain {
    Physics,
    Chemistry,
    Biology,
    Mathematics,
    ComputerScience,
    Medicine,
    Law,
    Economics,
    History,
    Geography,
    Engineering,
    Philosophy,
    Custom(String),
}

/// Data source types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataSource {
    Academic {
        institution: String,
        paper_id: String,
    },
    Authoritative {
        organization: String,
        publication: String,
    },
    AIGenerated {
        ai_agent: QuantumDID,
        model_version: String,
        generation_method: String,
    },
    UserInput {
        application: QuantumDID,
        user: QuantumDID,
    },
    Enterprise {
        organization: QuantumDID,
    },
}

/// Data collection methods
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CollectionMethod {
    Manual,
    Automated,
    Crowdsourced,
    Synthetic,
    Survey,
    Experiment,
}

/// Verification levels indicating trust and validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VerificationLevel {
    Unverified,
    SelfClaimed,
    PeerReviewed,
    Consensus,
    Authoritative,
    Cryptographic,
}

/// License types for fact usage
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LicenseType {
    PublicDomain,
    CreativeCommons,
    MIT,
    Apache2,
    Proprietary,
    UserOwned,
    Custom(String),
}

/// Citations for fact references
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Citation {
    pub citation_type: CitationType,
    pub reference: String,
    pub context: Option<String>,
}

/// Citation types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CitationType {
    Academic,
    Legal,
    Web,
    Book,
    Journal,
    Conference,
}

/// Verification proof for fact authenticity
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationProof {
    pub proof_type: ProofType,
    pub proof_data: Vec<u8>,
    pub verification_timestamp: Timestamp,
    pub verifier: Option<QuantumDID>,
}

/// Proof types for verification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProofType {
    QuantumSignature,
    ZeroKnowledge,
    Consensus,
    Merkle,
}

/// Access control policies for facts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessPolicy {
    Public,
    Private(HashSet<QuantumDID>),
    RoleBased(HashSet<String>),
    AttributeBased(AttributeRequirements),
    Dynamic(String), // Policy ID
    Conditional(Vec<AccessCondition>),
}

/// Attribute requirements for access control
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AttributeRequirements {
    pub required_attributes: HashMap<String, String>,
    pub minimum_trust_score: Option<f64>,
    pub domain_expertise: Option<KnowledgeDomain>,
}

/// Access conditions for conditional policies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessCondition {
    pub condition_type: ConditionType,
    pub parameters: HashMap<String, String>,
}

/// Condition types for access control
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConditionType {
    TimeWindow,
    LocationBased,
    DeviceType,
    NetworkCondition,
    TrustLevel,
    ReputationThreshold,
    PaymentRequired,
    MultiFactor,
}

/// Quantum encryption metadata for private facts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuantumEncryption {
    pub algorithm: EncryptionAlgorithm,
    pub reader_keys: Vec<(QuantumDID, Vec<u8>)>, // Encrypted keys for each reader
    pub metadata: EncryptionMetadata,
}

/// Encryption algorithms
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EncryptionAlgorithm {
    Kyber512,
    Kyber768,
    Kyber1024,
    NTRU,
    Classic,
}

/// Encryption metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncryptionMetadata {
    pub original_content_type: String,
    pub encryption_timestamp: Timestamp,
    pub key_rotation_schedule: Option<u64>,
}

impl FactPackage {
    /// Compute content hash for the fact package
    pub fn compute_content_hash(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();

        // Hash core content
        hasher.update(
            serde_json::to_string(&self.content)
                .unwrap_or_default()
                .as_bytes(),
        );
        hasher.update(&self.author.to_bytes());
        hasher.update(&self.created_at.to_le_bytes());

        hasher.finalize().into()
    }

    /// Check if the fact has expired
    pub fn is_expired(&self, current_time: Timestamp) -> bool {
        match self.expires_at {
            Some(expiry) => current_time > expiry,
            None => false,
        }
    }

    /// Get fact age in seconds
    pub fn age_seconds(&self, current_time: Timestamp) -> u64 {
        current_time.saturating_sub(self.created_at)
    }
}
