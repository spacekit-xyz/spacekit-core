//! TRM Recursive Reasoning ML Operation
//!
//! Implements the MLOperation trait for TRM-based puzzle solving and
//! recursive reasoning tasks.

use crate::ml_operation_registry::{MLOperation, OperationMetadata};
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

// Import TRM types (avoiding circular dependency by defining locally when needed)
// These match the types in trm_inference.rs

/// TRM Model Configuration (local copy to avoid circular import)
#[derive(Debug, Clone)]
pub struct TRMConfig {
    pub seq_len: usize,
    pub hidden_size: usize,
    pub vocab_size: usize,
    pub h_cycles: usize,
    pub l_cycles: usize,
    pub max_halt_steps: usize,
    pub puzzle_emb_len: usize,
    pub model_path: PathBuf,
}

impl Default for TRMConfig {
    fn default() -> Self {
        Self {
            seq_len: 1024,
            hidden_size: 384,
            vocab_size: 256,
            h_cycles: 3,
            l_cycles: 6,
            max_halt_steps: 16,
            puzzle_emb_len: 16,
            model_path: PathBuf::from("models/trm/trm_model.onnx"),
        }
    }
}

/// TRM Recursive Reasoning Operation
///
/// Provides recursive reasoning capabilities for:
/// - ARC-AGI puzzle solving
/// - Grid transformations
/// - Pattern completion
/// - Visual reasoning tasks
pub struct TRMRecursiveReasoningOperation {
    config: TRMConfig,
}

impl TRMRecursiveReasoningOperation {
    pub fn new() -> Self {
        Self {
            config: TRMConfig::default(),
        }
    }

    pub fn with_config(config: TRMConfig) -> Self {
        Self { config }
    }

    /// Process grid input and return prediction
    /// Note: This is a simplified implementation that delegates to the
    /// actual TRM inference manager when ORT is enabled
    async fn process_grid(&self, input: &serde_json::Value) -> Result<serde_json::Value> {
        // Extract grid
        let grid = input
            .get("input_grid")
            .or(input.get("grid"))
            .ok_or_else(|| anyhow::anyhow!("Missing input_grid"))?;

        let grid_vec: Vec<Vec<i32>> = serde_json::from_value(grid.clone())?;
        let max_steps = input
            .get("max_steps")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.config.max_halt_steps as u64) as usize;

        // When ORT feature is enabled, this would call the actual TRM inference
        // For now, provide a fallback that indicates the model isn't loaded
        #[cfg(feature = "ort")]
        {
            // Import the actual TRM inference manager
            use crate::trm_inference::{TRMInferenceManager, TRMRequest};

            let manager = TRMInferenceManager::new();

            let request = TRMRequest {
                operation: input
                    .get("operation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("solve_puzzle")
                    .to_string(),
                input_grid: grid_vec.clone(),
                output_shape: input
                    .get("output_shape")
                    .and_then(|v| serde_json::from_value(v.clone()).ok()),
                max_steps: Some(max_steps),
                puzzle_id: input
                    .get("puzzle_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize),
                model_variant: input
                    .get("model_variant")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            };

            match manager.process_request(&request).await {
                Ok(response) => {
                    return Ok(serde_json::to_value(response)?);
                }
                Err(e) => {
                    tracing::warn!("TRM inference failed: {}, using fallback", e);
                }
            }
        }

        // Fallback: return input as output (identity transform) with warning
        let rows = grid_vec.len();
        let cols = if rows > 0 { grid_vec[0].len() } else { 0 };

        Ok(serde_json::json!({
            "success": false,
            "output_grid": grid_vec,
            "reasoning_steps": 0,
            "halt_confidence": 0.0,
            "inference_time_ms": 0.0,
            "model_used": "fallback",
            "error": "TRM model not loaded. Please export and register an ONNX model.",
            "metrics": {
                "input_rows": rows,
                "input_cols": cols,
                "max_steps": max_steps
            }
        }))
    }
}

impl Default for TRMRecursiveReasoningOperation {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MLOperation for TRMRecursiveReasoningOperation {
    fn operation_id(&self) -> &str {
        "trm-recursive-reasoning"
    }

    async fn execute(&self, input: &serde_json::Value) -> Result<Vec<u8>> {
        let result = self.process_grid(input).await?;
        Ok(serde_json::to_vec(&result)?)
    }

    fn validate_input(&self, input: &serde_json::Value) -> Result<()> {
        // Check for required input_grid or grid field
        if input.get("input_grid").is_none() && input.get("grid").is_none() {
            return Err(anyhow::anyhow!(
                "Missing required field: 'input_grid' or 'grid'"
            ));
        }

        // Validate grid format
        let grid = input.get("input_grid").or(input.get("grid")).unwrap();
        if !grid.is_array() {
            return Err(anyhow::anyhow!("'input_grid' must be a 2D array"));
        }

        let grid_arr = grid.as_array().unwrap();
        if grid_arr.is_empty() {
            return Err(anyhow::anyhow!("'input_grid' cannot be empty"));
        }

        // Check that all rows are arrays
        for (i, row) in grid_arr.iter().enumerate() {
            if !row.is_array() {
                return Err(anyhow::anyhow!("Row {} of input_grid must be an array", i));
            }
        }

        // Validate max_steps if provided
        if let Some(max_steps) = input.get("max_steps") {
            if let Some(steps) = max_steps.as_u64() {
                if steps == 0 || steps > 64 {
                    return Err(anyhow::anyhow!("max_steps must be between 1 and 64"));
                }
            }
        }

        Ok(())
    }

    fn metadata(&self) -> OperationMetadata {
        OperationMetadata {
            name: "TRM Recursive Reasoning".to_string(),
            description: "Tiny Recursive Model (TRM) for recursive reasoning on visual puzzles. \
                         Achieves 45% on ARC-AGI-1 with only 7M parameters through iterative \
                         self-refinement. Supports ARC puzzles, grid transformations, Sudoku, \
                         and pattern completion.".to_string(),
            input_schema: r#"{"input_grid": [[int]], "max_steps": int, "model_variant": "string"}"#.to_string(),
            output_schema: r#"{"success": bool, "output_grid": [[int]], "reasoning_steps": int, "halt_confidence": float}"#.to_string(),
            estimated_gas: 5000, // TRM is efficient (7M params)
            model_requirements: vec![
                "TRM ONNX model (~30MB)".to_string(),
                "ORT feature enabled".to_string(),
            ],
            version: "1.0.0".to_string(),
        }
    }
}

/// TRM Operation Factory
pub struct TRMOperationFactory;

impl TRMOperationFactory {
    /// Create TRM operation with default configuration
    pub fn create_default() -> TRMRecursiveReasoningOperation {
        TRMRecursiveReasoningOperation::new()
    }

    /// Create TRM operation optimized for ARC-AGI
    pub fn create_arc_agi() -> TRMRecursiveReasoningOperation {
        TRMRecursiveReasoningOperation::with_config(TRMConfig {
            seq_len: 900, // 30x30 max grid
            hidden_size: 384,
            vocab_size: 11, // ARC uses 0-9 colors + padding
            h_cycles: 3,
            l_cycles: 4,
            max_halt_steps: 16,
            puzzle_emb_len: 16,
            model_path: PathBuf::from("models/trm/trm_arc_agi.onnx"),
        })
    }

    /// Create TRM operation for Sudoku
    pub fn create_sudoku() -> TRMRecursiveReasoningOperation {
        TRMRecursiveReasoningOperation::with_config(TRMConfig {
            seq_len: 81, // 9x9 Sudoku grid
            hidden_size: 384,
            vocab_size: 10, // 0-9
            h_cycles: 3,
            l_cycles: 6,
            max_halt_steps: 32, // Sudoku may need more steps
            puzzle_emb_len: 16,
            model_path: PathBuf::from("models/trm/trm_sudoku.onnx"),
        })
    }

    /// Create TRM operation for Maze solving
    pub fn create_maze() -> TRMRecursiveReasoningOperation {
        TRMRecursiveReasoningOperation::with_config(TRMConfig {
            seq_len: 900, // 30x30 max maze
            hidden_size: 384,
            vocab_size: 4, // wall, path, start, end
            h_cycles: 3,
            l_cycles: 4,
            max_halt_steps: 24,
            puzzle_emb_len: 16,
            model_path: PathBuf::from("models/trm/trm_maze.onnx"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_metadata() {
        let op = TRMRecursiveReasoningOperation::new();

        assert_eq!(op.operation_id(), "trm-recursive-reasoning");
        let metadata = op.metadata();
        assert!(metadata.description.contains("TRM"));
    }

    #[test]
    fn test_input_validation_valid() {
        let op = TRMRecursiveReasoningOperation::new();

        let valid_input = serde_json::json!({
            "input_grid": [[1, 2], [3, 4]],
            "max_steps": 8
        });

        assert!(op.validate_input(&valid_input).is_ok());
    }

    #[test]
    fn test_input_validation_missing_grid() {
        let op = TRMRecursiveReasoningOperation::new();

        let invalid_input = serde_json::json!({
            "max_steps": 8
        });

        assert!(op.validate_input(&invalid_input).is_err());
    }

    #[test]
    fn test_input_validation_invalid_max_steps() {
        let op = TRMRecursiveReasoningOperation::new();

        let invalid_input = serde_json::json!({
            "input_grid": [[1, 2], [3, 4]],
            "max_steps": 100  // Too high
        });

        assert!(op.validate_input(&invalid_input).is_err());
    }

    #[test]
    fn test_factory_variants() {
        let default = TRMOperationFactory::create_default();
        let arc = TRMOperationFactory::create_arc_agi();
        let sudoku = TRMOperationFactory::create_sudoku();
        let maze = TRMOperationFactory::create_maze();

        // All should have same operation_id
        assert_eq!(default.operation_id(), arc.operation_id());
        assert_eq!(arc.operation_id(), sudoku.operation_id());
        assert_eq!(sudoku.operation_id(), maze.operation_id());
    }

    #[tokio::test]
    async fn test_execute_fallback() {
        let op = TRMRecursiveReasoningOperation::new();

        let input = serde_json::json!({
            "input_grid": [[1, 2], [3, 4]],
            "max_steps": 8
        });

        let result = op.execute(&input).await;
        assert!(result.is_ok());

        let output: serde_json::Value = serde_json::from_slice(&result.unwrap()).unwrap();
        // Fallback should return the input grid as output
        assert!(output.get("output_grid").is_some());
    }
}
