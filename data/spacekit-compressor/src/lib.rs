//! SpaceKit Compressor - Rust Implementation
//!
//! High-performance pattern-based compression optimized for messaging, code, and text.
//! Port of the Python SpaceKit Compressor with native Rust performance.

pub mod adaptive;
pub mod binary;
pub mod patterns;

use anyhow::Result;

pub use adaptive::{AdaptiveCompressor, MAX_ADAPTIVE_BYTES};
pub use binary::{BinaryAlgorithm, BinaryCompressor};
pub use patterns::{PatternCompressor, PatternFormat};

/// Compression mode selection
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompressionMode {
    /// Store bytes without compression
    Stored,
    /// Pattern-based compression (SpaceKit patterns)
    Pattern,
    /// Binary compression (Gzip/LZMA)
    Binary,
    /// Adaptive (selects best method)
    Adaptive,
    /// Pattern + Binary (multi-stage)
    Hybrid,
}

/// Compression result with detailed metrics
#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub compressed: Vec<u8>,
    pub original_size: usize,
    pub compressed_size: usize,
    pub method: CompressionMode,
    pub compression_ratio: f64,
    pub savings_percent: f64,
    pub execution_time_ms: f64,
}

/// Main SpaceKit compressor
pub struct SpaceKitCompressor {
    pattern_compressor: PatternCompressor,
    binary_compressor: BinaryCompressor,
    adaptive_compressor: AdaptiveCompressor,
}

impl Default for SpaceKitCompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl SpaceKitCompressor {
    /// Create a new SpaceKit compressor with default settings
    pub fn new() -> Self {
        Self {
            pattern_compressor: PatternCompressor::new(),
            binary_compressor: BinaryCompressor::new(),
            adaptive_compressor: AdaptiveCompressor::new(),
        }
    }

    /// Compress data using specified mode
    pub fn compress(&self, data: &[u8], mode: CompressionMode) -> Result<CompressionResult> {
        let start = std::time::Instant::now();
        let original_size = data.len();

        let (compressed, method) = match mode {
            CompressionMode::Stored => (data.to_vec(), CompressionMode::Stored),
            CompressionMode::Pattern => {
                let result = self.pattern_compressor.compress(data)?;
                (result, CompressionMode::Pattern)
            }
            CompressionMode::Binary => {
                let result = self.binary_compressor.compress(data)?;
                (result, CompressionMode::Binary)
            }
            CompressionMode::Adaptive => {
                let result = self.adaptive_compressor.compress_enveloped(data)?;
                (result.compressed, CompressionMode::Adaptive)
            }
            CompressionMode::Hybrid => {
                // Pattern first, then binary
                let pattern_result = self.pattern_compressor.compress(data)?;
                let binary_result = self.binary_compressor.compress(&pattern_result)?;
                (binary_result, CompressionMode::Hybrid)
            }
        };

        let compressed_size = compressed.len();
        let execution_time_ms = start.elapsed().as_secs_f64() * 1000.0;

        let compression_ratio = if original_size == 0 {
            1.0
        } else {
            compressed_size as f64 / original_size as f64
        };
        Ok(CompressionResult {
            compressed,
            original_size,
            compressed_size,
            method,
            compression_ratio,
            savings_percent: (1.0 - compression_ratio) * 100.0,
            execution_time_ms,
        })
    }

    /// Compress Pattern or Hybrid data using an explicit pattern representation.
    pub fn compress_with_pattern_format(
        &self,
        data: &[u8],
        mode: CompressionMode,
        format: PatternFormat,
    ) -> Result<CompressionResult> {
        let start = std::time::Instant::now();
        let original_size = data.len();
        let compressed = match mode {
            CompressionMode::Pattern => {
                self.pattern_compressor.compress_with_format(data, format)?
            }
            CompressionMode::Hybrid => {
                let patterned = self.pattern_compressor.compress_with_format(data, format)?;
                self.binary_compressor.compress(&patterned)?
            }
            _ => anyhow::bail!("explicit pattern format requires Pattern or Hybrid mode"),
        };
        let compressed_size = compressed.len();
        let compression_ratio = if original_size == 0 {
            1.0
        } else {
            compressed_size as f64 / original_size as f64
        };
        Ok(CompressionResult {
            compressed,
            original_size,
            compressed_size,
            method: mode,
            compression_ratio,
            savings_percent: (1.0 - compression_ratio) * 100.0,
            execution_time_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Decompress data
    pub fn decompress(&self, data: &[u8], mode: CompressionMode) -> Result<Vec<u8>> {
        match mode {
            CompressionMode::Stored => Ok(data.to_vec()),
            CompressionMode::Pattern => self.pattern_compressor.decompress(data),
            CompressionMode::Binary => self.binary_compressor.decompress(data),
            CompressionMode::Adaptive => self.adaptive_compressor.decompress_enveloped(data),
            CompressionMode::Hybrid => {
                // Binary first, then pattern
                let binary_result = self.binary_compressor.decompress(data)?;
                self.pattern_compressor.decompress(&binary_result)
            }
        }
    }

    /// Decompress Pattern or Hybrid data using an explicit pattern representation.
    pub fn decompress_with_pattern_format(
        &self,
        data: &[u8],
        mode: CompressionMode,
        format: PatternFormat,
    ) -> Result<Vec<u8>> {
        match mode {
            CompressionMode::Pattern => {
                self.pattern_compressor.decompress_with_format(data, format)
            }
            CompressionMode::Hybrid => {
                let patterned = self.binary_compressor.decompress(data)?;
                self.pattern_compressor
                    .decompress_with_format(&patterned, format)
            }
            _ => anyhow::bail!("explicit pattern format requires Pattern or Hybrid mode"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_compression() {
        let compressor = SpaceKitCompressor::new();
        let message = "Hello, how are you doing today? I hope everything is going well!";

        let result = compressor
            .compress(message.as_bytes(), CompressionMode::Pattern)
            .unwrap();
        assert!(result.compressed_size <= result.original_size);

        let decompressed = compressor
            .decompress(&result.compressed, CompressionMode::Pattern)
            .unwrap();
        assert_eq!(decompressed, message.as_bytes());
    }

    #[test]
    fn test_binary_compression() {
        let compressor = SpaceKitCompressor::new();
        let message = "This is a test message. ".repeat(10);

        let result = compressor
            .compress(message.as_bytes(), CompressionMode::Binary)
            .unwrap();
        assert!(result.compressed_size < result.original_size);

        let decompressed = compressor
            .decompress(&result.compressed, CompressionMode::Binary)
            .unwrap();
        assert_eq!(decompressed, message.as_bytes());
    }

    #[test]
    fn test_adaptive_compression_roundtrip() {
        let compressor = SpaceKitCompressor::new();
        for message in [
            b"small".as_slice(),
            b"This medium payload repeats. This medium payload repeats. This medium payload repeats. This medium payload repeats.".as_slice(),
            "Hello and thank you. ".repeat(1000).as_bytes(),
        ] {
            let result = compressor.compress(message, CompressionMode::Adaptive).unwrap();
            assert_eq!(result.method, CompressionMode::Adaptive);
            let decompressed = compressor
                .decompress(&result.compressed, CompressionMode::Adaptive)
                .unwrap();
            assert_eq!(decompressed, message);
        }
    }

    #[test]
    fn test_hybrid_compression() {
        let compressor = SpaceKitCompressor::new();
        let message = "function calculateTotal(items) { return items.reduce((sum, item) => sum + item.price, 0); }".repeat(5);

        let result = compressor
            .compress(message.as_bytes(), CompressionMode::Hybrid)
            .unwrap();
        println!("Hybrid compression: {}% savings", result.savings_percent);
        assert!(result.savings_percent > 0.0);

        let decompressed = compressor
            .decompress(&result.compressed, CompressionMode::Hybrid)
            .unwrap();
        assert_eq!(decompressed, message.as_bytes());
    }
}
