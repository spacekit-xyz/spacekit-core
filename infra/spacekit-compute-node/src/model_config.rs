// Model Configuration System
// Allows operators to configure model paths and loading

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for LLM models
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub models_dir: PathBuf,
    pub enabled_models: Vec<ModelDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDefinition {
    pub model_id: String,
    pub model_name: String,
    pub model_path: PathBuf,
    pub model_type: ModelType,
    pub quantization: Option<String>,
    pub context_size: usize,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelType {
    #[serde(rename = "llama")]
    Llama,
    #[serde(rename = "phi")]
    Phi,
    #[serde(rename = "mistral")]
    Mistral,
    #[serde(rename = "qwen")]
    Qwen,
    #[serde(rename = "gemma")]
    Gemma,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            models_dir: PathBuf::from("./models"),
            enabled_models: vec![
                ModelDefinition {
                    model_id: "bitnet-b1.58-2b".to_string(),
                    model_name: "microsoft/bitnet-b1.58-2B-4T".to_string(),
                    model_path: PathBuf::from("bitnet-b1.58-2b-q8.gguf"),
                    model_type: ModelType::Llama,
                    quantization: Some("Q8_0".to_string()),
                    context_size: 2048,
                    enabled: false,  // Disabled until weights downloaded
                },
                ModelDefinition {
                    model_id: "phi-2".to_string(),
                    model_name: "microsoft/phi-2".to_string(),
                    model_path: PathBuf::from("phi-2-q8.gguf"),
                    model_type: ModelType::Phi,
                    quantization: Some("Q8_0".to_string()),
                    context_size: 2048,
                    enabled: false,
                },
                ModelDefinition {
                    model_id: "qwen-1.5-1.8b".to_string(),
                    model_name: "Qwen/Qwen1.5-1.8B-Chat".to_string(),
                    model_path: PathBuf::from("qwen-1.5-1.8b-q8.gguf"),
                    model_type: ModelType::Qwen,
                    quantization: Some("Q8_0".to_string()),
                    context_size: 32768,
                    enabled: false,
                },
            ],
        }
    }
}

impl ModelConfig {
    /// Load from YAML file
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: ModelConfig = serde_yaml::from_str(&content)?;
        Ok(config)
    }
    
    /// Get full path to model file
    pub fn get_model_path(&self, model_id: &str) -> Option<PathBuf> {
        self.enabled_models.iter()
            .find(|m| m.model_id == model_id && m.enabled)
            .map(|m| self.models_dir.join(&m.model_path))
    }
    
    /// Check if model is enabled
    pub fn is_model_enabled(&self, model_id: &str) -> bool {
        self.enabled_models.iter()
            .any(|m| m.model_id == model_id && m.enabled)
    }
}
