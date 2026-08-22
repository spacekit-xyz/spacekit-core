// ML Operation Registry - Dynamic, Extensible Transformer Operations

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Trait for ML operations (extensible)
#[async_trait]
pub trait MLOperation: Send + Sync {
    /// Operation identifier (e.g., "sentiment-analysis", "text-generation")
    fn operation_id(&self) -> &str;

    /// Execute the operation
    async fn execute(&self, input: &serde_json::Value) -> Result<Vec<u8>>;

    /// Validate input format
    fn validate_input(&self, input: &serde_json::Value) -> Result<()>;

    /// Get operation metadata
    fn metadata(&self) -> OperationMetadata;
}

/// Operation metadata for discovery and documentation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationMetadata {
    pub name: String,
    pub description: String,
    pub input_schema: String,
    pub output_schema: String,
    pub estimated_gas: u64,
    pub model_requirements: Vec<String>,
    pub version: String,
}

/// Dynamic operation registry
pub struct MLOperationRegistry {
    operations: HashMap<String, Arc<dyn MLOperation>>,
}

impl MLOperationRegistry {
    /// Create new registry with built-in operations
    pub fn new() -> Self {
        let mut registry = Self {
            operations: HashMap::new(),
        };

        // Register built-in operations
        // TODO: Add option to spec to initialize operations
        // migrage to growformer agent operations
        // registry.register_builtin_operations();

        registry
    }

    #[allow(dead_code)]
    /// Register built-in operations
    fn register_builtin_operations(&mut self) {
        use crate::ml_operations::*;

        // Register DistilBERT (VERIFIED REAL)
        self.register(Arc::new(DistilBertSentimentOperation::new()));

        // Register Sentence Transformers
        self.register(Arc::new(SentenceTransformersOperation::new()));

        // Register Text Generation (BitNet, GPT-2)
        self.register(Arc::new(TextGenerationOperation::new()));

        // Register Embeddings
        self.register(Arc::new(EmbeddingsOperation::new()));

        // Register TRM Recursive Reasoning (ARC-AGI, Sudoku, Maze)
        self.register(Arc::new(TRMRecursiveReasoningOperation::new()));

        tracing::info!(
            "✅ Registered {} built-in ML operations",
            self.operations.len()
        );
    }

    /// Register a new operation
    pub fn register(&mut self, operation: Arc<dyn MLOperation>) {
        let op_id = operation.operation_id().to_string();
        tracing::info!("📦 Registering ML operation: {}", op_id);
        self.operations.insert(op_id, operation);
    }

    /// Execute operation by ID
    pub async fn execute(&self, operation_id: &str, input: &serde_json::Value) -> Result<Vec<u8>> {
        let operation = self.operations.get(operation_id).ok_or_else(|| {
            anyhow::anyhow!(
                "Unknown ML operation: {} (available: {:?})",
                operation_id,
                self.operations.keys().collect::<Vec<_>>()
            )
        })?;

        // Validate input
        operation.validate_input(input)?;

        // Execute
        tracing::info!("🚀 Executing ML operation: {}", operation_id);
        operation.execute(input).await
    }

    /// List available operations
    pub fn list_operations(&self) -> Vec<OperationMetadata> {
        self.operations.values().map(|op| op.metadata()).collect()
    }

    /// Check if operation exists
    pub fn has_operation(&self, operation_id: &str) -> bool {
        self.operations.contains_key(operation_id)
    }

    /// Get operation metadata
    pub fn get_metadata(&self, operation_id: &str) -> Option<OperationMetadata> {
        self.operations.get(operation_id).map(|op| op.metadata())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = MLOperationRegistry::new();
        assert!(registry.has_operation("sentiment-analysis"));

        let ops = registry.list_operations();
        assert!(!ops.is_empty());
    }

    #[test]
    fn test_trm_operation_registered() {
        let registry = MLOperationRegistry::new();
        assert!(registry.has_operation("trm-recursive-reasoning"));

        let metadata = registry.get_metadata("trm-recursive-reasoning");
        assert!(metadata.is_some());
        assert!(metadata.unwrap().description.contains("TRM"));
    }
}
