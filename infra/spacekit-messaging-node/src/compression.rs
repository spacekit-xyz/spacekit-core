//! Message Compression Module
//!
//! Provides efficient compression for messaging to reduce bandwidth and storage.
//! Uses adaptive compression based on message size and type.

use anyhow::Result;
use spacekit_compressor::{BinaryAlgorithm, BinaryCompressor};

/// Compression method used
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompressionMethod {
    None,
    Gzip,
    Lzma,
}

/// Compression result with metadata
#[derive(Debug, Clone)]
pub struct CompressionResult {
    pub compressed: Vec<u8>,
    pub original_size: usize,
    pub compressed_size: usize,
    pub method: CompressionMethod,
    pub compression_ratio: f64,
}

/// Message compressor with adaptive strategies
pub struct MessageCompressor {
    /// Minimum size to compress (bytes)
    min_compress_size: usize,
    /// Default compression level (1-9)
    compression_level: u32,
}

impl Default for MessageCompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageCompressor {
    /// Create a new message compressor with default settings
    pub fn new() -> Self {
        Self {
            min_compress_size: 100, // Don't compress messages < 100 bytes
            compression_level: 6,   // Balanced compression level
        }
    }

    /// Create with custom settings
    pub fn with_settings(min_compress_size: usize, compression_level: u32) -> Self {
        Self {
            min_compress_size,
            compression_level: compression_level.clamp(1, 9),
        }
    }

    /// Compress message data adaptively
    pub fn compress(&self, data: &[u8]) -> Result<CompressionResult> {
        let original_size = data.len();

        // Skip compression for small messages
        if original_size < self.min_compress_size {
            return Ok(CompressionResult {
                compressed: data.to_vec(),
                original_size,
                compressed_size: original_size,
                method: CompressionMethod::None,
                compression_ratio: 1.0,
            });
        }

        // Use gzip for general compression
        let compressed = self.gzip_compress(data)?;
        let compressed_size = compressed.len();

        // Only use compressed version if it's actually smaller
        if compressed_size < original_size {
            Ok(CompressionResult {
                compressed,
                original_size,
                compressed_size,
                method: CompressionMethod::Gzip,
                compression_ratio: compressed_size as f64 / original_size as f64,
            })
        } else {
            // Compression didn't help - return original
            Ok(CompressionResult {
                compressed: data.to_vec(),
                original_size,
                compressed_size: original_size,
                method: CompressionMethod::None,
                compression_ratio: 1.0,
            })
        }
    }

    /// Decompress message data
    pub fn decompress(&self, data: &[u8], method: CompressionMethod) -> Result<Vec<u8>> {
        match method {
            CompressionMethod::None => Ok(data.to_vec()),
            CompressionMethod::Gzip => self.gzip_decompress(data),
            CompressionMethod::Lzma => self.lzma_decompress(data),
        }
    }

    /// Gzip compression
    fn gzip_compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        BinaryCompressor::with_settings(BinaryAlgorithm::Gzip, self.compression_level)
            .compress(data)
    }

    /// Gzip decompression
    fn gzip_decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        BinaryCompressor::with_settings(BinaryAlgorithm::Gzip, self.compression_level)
            .decompress(data)
    }

    /// LZMA compression (higher ratio, slower)
    fn lzma_compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        BinaryCompressor::with_settings(BinaryAlgorithm::Lzma, self.compression_level)
            .compress(data)
    }

    /// LZMA decompression
    fn lzma_decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        BinaryCompressor::with_settings(BinaryAlgorithm::Lzma, self.compression_level)
            .decompress(data)
    }

    /// Compress with automatic method selection
    pub fn compress_adaptive(&self, data: &[u8]) -> Result<CompressionResult> {
        let original_size = data.len();

        if original_size < self.min_compress_size {
            return self.compress(data); // Use default behavior
        }

        // Try both methods for large data, pick best
        if original_size > 5000 {
            let gzip_result = self.gzip_compress(data)?;
            let lzma_result = self.lzma_compress(data)?;

            if lzma_result.len() < gzip_result.len() {
                Ok(CompressionResult {
                    compressed: lzma_result.clone(),
                    original_size,
                    compressed_size: lzma_result.len(),
                    method: CompressionMethod::Lzma,
                    compression_ratio: lzma_result.len() as f64 / original_size as f64,
                })
            } else {
                Ok(CompressionResult {
                    compressed: gzip_result.clone(),
                    original_size,
                    compressed_size: gzip_result.len(),
                    method: CompressionMethod::Gzip,
                    compression_ratio: gzip_result.len() as f64 / original_size as f64,
                })
            }
        } else {
            // Use gzip for medium messages
            self.compress(data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_small_message_no_compression() {
        let compressor = MessageCompressor::new();
        let message = b"Hello!";

        let result = compressor.compress(message).unwrap();
        assert_eq!(result.method, CompressionMethod::None);
        assert_eq!(result.compressed, message);
    }

    #[test]
    fn test_medium_message_compression() {
        let compressor = MessageCompressor::new();
        let message = "This is a longer message that should be compressed because it has enough content to make compression worthwhile. ".repeat(3);

        let result = compressor.compress(message.as_bytes()).unwrap();
        assert_eq!(result.method, CompressionMethod::Gzip);
        assert!(result.compressed_size < result.original_size);

        // Test decompression
        let decompressed = compressor
            .decompress(&result.compressed, result.method)
            .unwrap();
        assert_eq!(decompressed, message.as_bytes());
    }

    #[test]
    fn test_compression_ratio() {
        let compressor = MessageCompressor::new();
        let message = "Hello World! This is a test message. ".repeat(10);

        let result = compressor.compress(message.as_bytes()).unwrap();
        println!("Original: {} bytes", result.original_size);
        println!("Compressed: {} bytes", result.compressed_size);
        println!("Ratio: {:.2}%", (1.0 - result.compression_ratio) * 100.0);

        assert!(result.compression_ratio < 0.9); // At least 10% compression
    }

    #[test]
    fn test_large_adaptive_message_roundtrip() {
        let compressor = MessageCompressor::with_settings(100, 6);
        let message = "SpaceKit messaging payload with repeated structured content. ".repeat(200);
        let result = compressor.compress_adaptive(message.as_bytes()).unwrap();
        assert!(matches!(
            result.method,
            CompressionMethod::Gzip | CompressionMethod::Lzma
        ));
        let decompressed = compressor
            .decompress(&result.compressed, result.method)
            .unwrap();
        assert_eq!(decompressed, message.as_bytes());
    }
}
