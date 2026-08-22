//! Content types and utilities for Fact Packages

use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::v1::fact::verification::ConfidenceInterval;

/// AI-specific content types for AI-generated facts
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AIContent {
    /// Generated text with metadata
    GeneratedText {
        text: String,
        model_used: String,
        generation_parameters: GenerationParameters,
        prompt_hash: [u8; 32],
    },

    /// AI-generated summary of multiple sources
    Summary {
        source_content: Vec<String>,
        summary_text: String,
        compression_ratio: f64,
        key_points: Vec<String>,
    },

    /// AI-generated predictions or forecasts
    Prediction {
        prediction_text: String,
        confidence_interval: ConfidenceInterval,
        methodology: String,
        time_horizon: Option<u64>, // seconds from now
    },

    /// Embeddings and vector representations
    Embedding {
        vector: Vec<f32>,
        dimension: usize,
        model_used: String,
        normalization: VectorNormalization,
    },

    /// AI analysis or insights
    Analysis {
        analysis_text: String,
        data_sources: Vec<FactID>,
        methodology: AnalysisMethod,
        statistical_measures: Option<StatisticalMeasures>,
    },
}

/// Parameters used for AI content generation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GenerationParameters {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<u32>,
    pub seed: Option<u64>,
    pub custom_parameters: HashMap<String, String>,
}

/// Vector normalization methods
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VectorNormalization {
    None,
    L1,
    L2,
    MinMax,
    ZScore,
}

/// Analysis methodologies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnalysisMethod {
    Statistical,
    MachineLearning,
    NaturalLanguageProcessing,
    ComputerVision,
    TimeSeriesAnalysis,
    NetworkAnalysis,
    Custom(String),
}

/// Statistical measures for analysis results
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatisticalMeasures {
    pub mean: Option<f64>,
    pub median: Option<f64>,
    pub standard_deviation: Option<f64>,
    pub confidence_level: Option<f64>,
    pub p_value: Option<f64>,
    pub sample_size: Option<u64>,
}

/// Multimedia content with enhanced metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultimediaContent {
    pub content_type: MultimediaType,
    pub data: Vec<u8>,
    pub metadata: MultimediaMetadata,
    pub processing_history: Vec<ProcessingStep>,
}

/// Types of multimedia content
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MultimediaType {
    Image(ImageFormat),
    Video(VideoFormat),
    Audio(AudioFormat),
    Document(DocumentFormat),
    ThreeD(ThreeDFormat),
}

/// Image format specifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ImageFormat {
    JPEG,
    PNG,
    WEBP,
    SVG,
    TIFF,
    RAW(String), // Camera RAW format
}

/// Video format specifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VideoFormat {
    MP4,
    WebM,
    AVI,
    MOV,
    MKV,
}

/// Audio format specifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AudioFormat {
    MP3,
    WAV,
    FLAC,
    OGG,
    AAC,
}

/// Document format specifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DocumentFormat {
    PDF,
    Word,
    HTML,
    Markdown,
    PlainText,
    LaTeX,
}

/// 3D format specifications
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ThreeDFormat {
    OBJ,
    STL,
    GLTF,
    FBX,
    PLY,
}

/// Multimedia metadata
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MultimediaMetadata {
    pub resolution: Option<Resolution>,
    pub duration: Option<f64>, // seconds
    pub file_size: u64,
    pub creation_date: Option<Timestamp>,
    pub camera_info: Option<CameraInfo>,
    pub location: Option<GeoLocation>,
    pub color_profile: Option<String>,
    pub compression_info: Option<CompressionInfo>,
}

/// Resolution specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
    pub depth: Option<u32>, // For 3D content
}

/// Camera information for media capture
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CameraInfo {
    pub make: String,
    pub model: String,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub aperture: Option<f64>,
    pub shutter_speed: Option<String>,
    pub focal_length: Option<f64>,
}

/// Geographic location
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: Option<f64>,
    pub accuracy: Option<f64>,
}

/// Compression information
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompressionInfo {
    pub algorithm: String,
    pub quality: Option<f64>,
    pub compression_ratio: f64,
    pub lossless: bool,
}

/// Processing step in content history
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProcessingStep {
    pub step_type: ProcessingType,
    pub processor: String, // Software/service used
    pub timestamp: Timestamp,
    pub parameters: HashMap<String, String>,
    pub hash_before: [u8; 32],
    pub hash_after: [u8; 32],
}

/// Types of content processing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProcessingType {
    Compression,
    Resize,
    Crop,
    Filter,
    ColorCorrection,
    NoiseReduction,
    Enhancement,
    Watermarking,
    Encryption,
    FormatConversion,
}

/// Structured data content with schema validation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StructuredData {
    pub data: serde_json::Value,
    pub schema: DataSchema,
    pub validation_result: ValidationResult,
    pub transformations: Vec<DataTransformation>,
}

/// Data schema specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataSchema {
    pub schema_type: SchemaType,
    pub version: String,
    pub definition: String, // JSON Schema, XSD, etc.
    pub validation_rules: Vec<ValidationRule>,
}

/// Schema types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SchemaType {
    JSONSchema,
    XMLSchema,
    Avro,
    Protobuf,
    Custom(String),
}

/// Validation rule specification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationRule {
    pub rule_type: ValidationRuleType,
    pub field_path: String,
    pub constraint: String,
    pub error_message: String,
}

/// Types of validation rules
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ValidationRuleType {
    Required,
    Type,
    Range,
    Pattern,
    Enum,
    Custom,
}

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub validated_at: Timestamp,
}

/// Validation error
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationError {
    pub field_path: String,
    pub error_type: String,
    pub message: String,
    pub suggested_fix: Option<String>,
}

/// Validation warning
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationWarning {
    pub field_path: String,
    pub warning_type: String,
    pub message: String,
    pub severity: WarningSeverity,
}

/// Warning severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WarningSeverity {
    Low,
    Medium,
    High,
}

/// Data transformation record
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataTransformation {
    pub transformation_type: TransformationType,
    pub applied_at: Timestamp,
    pub description: String,
    pub reversible: bool,
    pub parameters: HashMap<String, String>,
}

/// Types of data transformations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransformationType {
    Normalization,
    Aggregation,
    Filtering,
    Sorting,
    GroupBy,
    Join,
    Union,
    Projection,
    Custom(String),
}

impl FactContent {
    /// Get the size of the content in bytes
    pub fn size_bytes(&self) -> usize {
        match self {
            FactContent::Text { content, .. } => content.len(),
            FactContent::Binary { data, .. } => data.len(),
            FactContent::Json { data, .. } => serde_json::to_string(data).unwrap_or_default().len(),
            FactContent::Reference { .. } => 32, // Size of FactID
            FactContent::Aggregation {
                source_facts,
                result,
                ..
            } => source_facts.len() * 32 + result.size_bytes(),
            _ => 0,
        }
    }

    /// Get the content type as a string
    pub fn content_type(&self) -> &'static str {
        match self {
            FactContent::Text { .. } => "text",
            FactContent::Numerical { .. } => "numerical",
            FactContent::Boolean { .. } => "boolean",
            FactContent::Json { .. } => "json",
            FactContent::Binary { .. } => "binary",
            FactContent::Reference { .. } => "reference",
            FactContent::Aggregation { .. } => "aggregation",
        }
    }

    /// Check if the content is multimedia
    pub fn is_multimedia(&self) -> bool {
        matches!(self, FactContent::Binary { mime_type, .. } if 
            mime_type.starts_with("image/") || 
            mime_type.starts_with("video/") || 
            mime_type.starts_with("audio/"))
    }
}
