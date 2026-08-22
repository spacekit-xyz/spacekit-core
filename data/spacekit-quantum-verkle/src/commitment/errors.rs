use alloc::string::String;
#[cfg(not(feature = "std"))]
use core::fmt;
#[cfg(feature = "std")]
use thiserror::Error;

/// Enum to represent errors in the Verkle commitment scheme
#[cfg_attr(feature = "std", derive(Error))]
#[derive(Debug, PartialEq, Eq)]
pub enum VerkleError {
    #[cfg_attr(feature = "std", error("Database error"))]
    DatabaseError,
    #[cfg_attr(feature = "std", error("Invalid proof"))]
    InvalidProof,
    #[cfg_attr(feature = "std", error("Node not found"))]
    NodeNotFound,
    #[cfg_attr(feature = "std", error("Invalid commitment"))]
    InvalidCommitment,
    #[cfg_attr(feature = "std", error("Key not found"))]
    KeyNotFound,
    #[cfg_attr(feature = "std", error("Batch operation failed"))]
    BatchOperationFailed,

    #[cfg_attr(feature = "std", error("Serialization error"))]
    SerializationError,

    #[cfg_attr(feature = "std", error("Invalid cache size"))]
    InvalidCacheSize,

    #[cfg_attr(feature = "std", error("Invalid tree structure: {0}"))]
    InvalidTreeStructure(String),
    #[cfg_attr(feature = "std", error("Max depth exceeded"))]
    MaxDepthExceeded,
    #[cfg_attr(feature = "std", error("Invalid node format"))]
    InvalidNodeFormat,
    #[cfg_attr(feature = "std", error("Proof generation failed: {0}"))]
    ProofGenerationFailed(String),
    #[cfg_attr(feature = "std", error("Optimization failed: {0}"))]
    OptimizationFailed(String),
    #[cfg_attr(feature = "std", error("Validation failed: {0}"))]
    ValidationFailed(String),

    #[cfg_attr(feature = "std", error("Invalid proof options"))]
    InvalidProofOptions(String),
    #[cfg_attr(feature = "std", error("Range proof error"))]
    RangeProofError(String),
    #[cfg_attr(feature = "std", error("Tree optimization error"))]
    TreeOptimizationError(String),
    #[cfg_attr(feature = "std", error("Node validation error"))]
    NodeValidationError(String),

    #[cfg_attr(feature = "std", error("Invalid polynomial"))]
    InvalidPolynomial(String),
}

#[cfg(not(feature = "std"))]
impl fmt::Display for VerkleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerkleError::DatabaseError => write!(f, "Database error"),
            VerkleError::InvalidProof => write!(f, "Invalid proof"),
            VerkleError::NodeNotFound => write!(f, "Node not found"),
            VerkleError::InvalidCommitment => write!(f, "Invalid commitment"),
            VerkleError::KeyNotFound => write!(f, "Key not found"),
            VerkleError::BatchOperationFailed => write!(f, "Batch operation failed"),
            VerkleError::SerializationError => write!(f, "Serialization error"),
            VerkleError::InvalidTreeStructure(message) => {
                write!(f, "Invalid tree structure: {}", message)
            }
            VerkleError::MaxDepthExceeded => write!(f, "Max depth exceeded"),
            VerkleError::InvalidNodeFormat => write!(f, "Invalid node format"),
            VerkleError::ProofGenerationFailed(message) => {
                write!(f, "Proof generation failed: {}", message)
            }
            VerkleError::OptimizationFailed(message) => {
                write!(f, "Optimization failed: {}", message)
            }
            VerkleError::ValidationFailed(message) => {
                write!(f, "Validation failed: {}", message)
            }
            VerkleError::InvalidProofOptions(message) => {
                write!(f, "Invalid proof options: {}", message)
            }
            VerkleError::RangeProofError(message) => write!(f, "Range proof error: {}", message),
            VerkleError::TreeOptimizationError(message) => {
                write!(f, "Tree optimization error: {}", message)
            }
            VerkleError::NodeValidationError(message) => {
                write!(f, "Node validation error: {}", message)
            }
            VerkleError::InvalidPolynomial(message) => {
                write!(f, "Invalid polynomial: {}", message)
            }
            VerkleError::InvalidCacheSize => write!(f, "Invalid cache size"),
        }
    }
}
