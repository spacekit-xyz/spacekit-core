//! Adaptive Compression
//!
//! Intelligently selects the best compression method based on content analysis.

use crate::{BinaryCompressor, CompressionMode, PatternCompressor, PatternFormat};
use anyhow::Result;

const ADAPTIVE_MAGIC: &[u8; 8] = b"SKADCMPR";
const ADAPTIVE_ENVELOPE_VERSION_V1: u8 = 1;
const ADAPTIVE_ENVELOPE_VERSION_V2: u8 = 2;
const ADAPTIVE_HEADER_LEN: usize = 8 + 1 + 1 + 8 + 8;
/// Hard limit for a single adaptive payload and its decoded representation.
pub const MAX_ADAPTIVE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnvelopeMethod {
    Stored,
    PatternV1,
    Binary,
    HybridV1,
    PatternV2,
    HybridV2,
}

struct Envelope<'a> {
    version: u8,
    method: EnvelopeMethod,
    original_len: usize,
    payload: &'a [u8],
}

/// Adaptive compressor that selects best method
pub struct AdaptiveCompressor {
    pattern: PatternCompressor,
    binary: BinaryCompressor,
    small_threshold: usize,
    large_threshold: usize,
}

/// Adaptive compression result with method selected
pub struct AdaptiveResult {
    pub compressed: Vec<u8>,
    pub method: CompressionMode,
}

impl Default for AdaptiveCompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveCompressor {
    pub fn new() -> Self {
        Self {
            pattern: PatternCompressor::new(),
            binary: BinaryCompressor::new(),
            small_threshold: 100,
            large_threshold: 5000,
        }
    }

    /// Compress using adaptive strategy
    pub fn compress(&self, data: &[u8]) -> Result<AdaptiveResult> {
        let size = data.len();

        // Small messages: no compression
        if size < self.small_threshold {
            return Ok(AdaptiveResult {
                compressed: data.to_vec(),
                method: CompressionMode::Stored,
            });
        }

        let mut best = AdaptiveResult {
            compressed: data.to_vec(),
            method: CompressionMode::Stored,
        };

        let binary_result = self.binary.compress(data)?;
        if binary_result.len() < best.compressed.len() {
            best = AdaptiveResult {
                compressed: binary_result,
                method: CompressionMode::Binary,
            };
        }

        // Pattern substitution is text-only and uses reserved marker sequences.
        // Never apply it to arbitrary bytes or text that already contains markers.
        let pattern_safe = std::str::from_utf8(data)
            .map(|text| !text.contains('⚡'))
            .unwrap_or(false);
        if pattern_safe {
            let pattern_result = self.pattern.compress(data)?;
            let worth_trying_hybrid = size >= self.large_threshold
                || pattern_result.len().saturating_mul(5) < size.saturating_mul(4);
            if worth_trying_hybrid {
                let hybrid_result = self.binary.compress(&pattern_result)?;
                if hybrid_result.len() < best.compressed.len() {
                    best = AdaptiveResult {
                        compressed: hybrid_result,
                        method: CompressionMode::Hybrid,
                    };
                }
            }
        }

        Ok(best)
    }

    fn compress_v2(&self, data: &[u8]) -> Result<(AdaptiveResult, EnvelopeMethod)> {
        if data.len() > MAX_ADAPTIVE_BYTES {
            anyhow::bail!(
                "adaptive input is too large: {} bytes exceeds {}",
                data.len(),
                MAX_ADAPTIVE_BYTES
            );
        }
        if data.len() < self.small_threshold {
            return Ok((
                AdaptiveResult {
                    compressed: data.to_vec(),
                    method: CompressionMode::Stored,
                },
                EnvelopeMethod::Stored,
            ));
        }

        let mut best = AdaptiveResult {
            compressed: data.to_vec(),
            method: CompressionMode::Stored,
        };
        let mut best_method = EnvelopeMethod::Stored;

        let binary_result = self.binary.compress(data)?;
        if binary_result.len() < best.compressed.len() {
            best = AdaptiveResult {
                compressed: binary_result,
                method: CompressionMode::Binary,
            };
            best_method = EnvelopeMethod::Binary;
        }

        if std::str::from_utf8(data).is_ok() {
            let pattern_result = self.pattern.compress_with_format(data, PatternFormat::V2)?;
            if pattern_result.len() < best.compressed.len() {
                best = AdaptiveResult {
                    compressed: pattern_result.clone(),
                    method: CompressionMode::Pattern,
                };
                best_method = EnvelopeMethod::PatternV2;
            }

            let worth_trying_hybrid = data.len() >= self.large_threshold
                || pattern_result.len().saturating_mul(5) < data.len().saturating_mul(4);
            if worth_trying_hybrid {
                let hybrid_result = self.binary.compress(&pattern_result)?;
                if hybrid_result.len() < best.compressed.len() {
                    best = AdaptiveResult {
                        compressed: hybrid_result,
                        method: CompressionMode::Hybrid,
                    };
                    best_method = EnvelopeMethod::HybridV2;
                }
            }
        }

        Ok((best, best_method))
    }

    /// Compress into the current self-describing adaptive envelope (v2).
    pub fn compress_enveloped(&self, data: &[u8]) -> Result<AdaptiveResult> {
        let (selected, envelope_method) = self.compress_v2(data)?;
        let mut enveloped = Vec::with_capacity(ADAPTIVE_HEADER_LEN + selected.compressed.len());
        enveloped.extend_from_slice(ADAPTIVE_MAGIC);
        enveloped.push(ADAPTIVE_ENVELOPE_VERSION_V2);
        enveloped.push(method_to_tag(
            ADAPTIVE_ENVELOPE_VERSION_V2,
            envelope_method,
        )?);
        enveloped.extend_from_slice(&(data.len() as u64).to_le_bytes());
        enveloped.extend_from_slice(&(selected.compressed.len() as u64).to_le_bytes());
        enveloped.extend_from_slice(&selected.compressed);
        Ok(AdaptiveResult {
            compressed: enveloped,
            method: selected.method,
        })
    }

    /// Encode the legacy v1 envelope for compatibility tests and migrations.
    pub fn compress_enveloped_v1(&self, data: &[u8]) -> Result<AdaptiveResult> {
        if data.len() > MAX_ADAPTIVE_BYTES {
            anyhow::bail!(
                "adaptive input is too large: {} bytes exceeds {}",
                data.len(),
                MAX_ADAPTIVE_BYTES
            );
        }
        let selected = self.compress(data)?;
        let method = match selected.method {
            CompressionMode::Stored => EnvelopeMethod::Stored,
            CompressionMode::Pattern => EnvelopeMethod::PatternV1,
            CompressionMode::Binary => EnvelopeMethod::Binary,
            CompressionMode::Hybrid => EnvelopeMethod::HybridV1,
            CompressionMode::Adaptive => {
                anyhow::bail!("adaptive cannot be an inner compression method")
            }
        };
        let mut enveloped = Vec::with_capacity(ADAPTIVE_HEADER_LEN + selected.compressed.len());
        enveloped.extend_from_slice(ADAPTIVE_MAGIC);
        enveloped.push(ADAPTIVE_ENVELOPE_VERSION_V1);
        enveloped.push(method_to_tag(ADAPTIVE_ENVELOPE_VERSION_V1, method)?);
        enveloped.extend_from_slice(&(data.len() as u64).to_le_bytes());
        enveloped.extend_from_slice(&(selected.compressed.len() as u64).to_le_bytes());
        enveloped.extend_from_slice(&selected.compressed);
        Ok(AdaptiveResult {
            compressed: enveloped,
            method: selected.method,
        })
    }

    /// Decode a self-describing adaptive envelope (v1 or v2).
    pub fn decompress_enveloped(&self, data: &[u8]) -> Result<Vec<u8>> {
        let envelope = parse_envelope(data)?;
        let decoded = match envelope.method {
            EnvelopeMethod::Stored => envelope.payload.to_vec(),
            EnvelopeMethod::PatternV1 => self.pattern.decompress(envelope.payload)?,
            EnvelopeMethod::PatternV2 => self
                .pattern
                .decompress_with_format(envelope.payload, PatternFormat::V2)?,
            EnvelopeMethod::Binary => self
                .binary
                .decompress_with_limit(envelope.payload, envelope.original_len)?,
            EnvelopeMethod::HybridV1 => {
                let pattern_limit = envelope
                    .original_len
                    .saturating_mul(2)
                    .saturating_add(1024)
                    .min(MAX_ADAPTIVE_BYTES);
                let pattern_bytes = self
                    .binary
                    .decompress_with_limit(envelope.payload, pattern_limit)?;
                self.pattern.decompress(&pattern_bytes)?
            }
            EnvelopeMethod::HybridV2 => {
                let pattern_limit = envelope
                    .original_len
                    .saturating_mul(2)
                    .saturating_add(1024)
                    .min(MAX_ADAPTIVE_BYTES);
                let pattern_bytes = self
                    .binary
                    .decompress_with_limit(envelope.payload, pattern_limit)?;
                self.pattern
                    .decompress_with_format(&pattern_bytes, PatternFormat::V2)?
            }
        };
        if decoded.len() != envelope.original_len {
            anyhow::bail!(
                "adaptive envelope v{} decoded length mismatch: got {}, expected {}",
                envelope.version,
                decoded.len(),
                envelope.original_len
            );
        }
        Ok(decoded)
    }
}

fn parse_envelope(data: &[u8]) -> Result<Envelope<'_>> {
    if data.len() < ADAPTIVE_HEADER_LEN {
        anyhow::bail!("adaptive envelope is truncated");
    }
    if &data[..8] != ADAPTIVE_MAGIC {
        anyhow::bail!("adaptive envelope has invalid magic");
    }
    let version = data[8];
    if version != ADAPTIVE_ENVELOPE_VERSION_V1 && version != ADAPTIVE_ENVELOPE_VERSION_V2 {
        anyhow::bail!("unsupported adaptive envelope version {}", version);
    }
    let method = tag_to_method(version, data[9])?;
    let original_len = usize::try_from(u64::from_le_bytes(data[10..18].try_into().unwrap()))
        .map_err(|_| anyhow::anyhow!("adaptive envelope original length is too large"))?;
    let payload_len = usize::try_from(u64::from_le_bytes(data[18..26].try_into().unwrap()))
        .map_err(|_| anyhow::anyhow!("adaptive envelope payload length is too large"))?;
    if original_len > MAX_ADAPTIVE_BYTES || payload_len > MAX_ADAPTIVE_BYTES {
        anyhow::bail!(
            "adaptive envelope exceeds {} byte safety limit",
            MAX_ADAPTIVE_BYTES
        );
    }
    let expected_len = ADAPTIVE_HEADER_LEN
        .checked_add(payload_len)
        .ok_or_else(|| anyhow::anyhow!("adaptive envelope length overflow"))?;
    if data.len() != expected_len {
        anyhow::bail!(
            "adaptive envelope length mismatch: got {}, expected {}",
            data.len(),
            expected_len
        );
    }
    Ok(Envelope {
        version,
        method,
        original_len,
        payload: &data[ADAPTIVE_HEADER_LEN..],
    })
}

fn method_to_tag(version: u8, method: EnvelopeMethod) -> Result<u8> {
    match (version, method) {
        (ADAPTIVE_ENVELOPE_VERSION_V1, EnvelopeMethod::Stored) => Ok(0),
        (ADAPTIVE_ENVELOPE_VERSION_V1, EnvelopeMethod::PatternV1) => Ok(1),
        (ADAPTIVE_ENVELOPE_VERSION_V1, EnvelopeMethod::Binary) => Ok(2),
        (ADAPTIVE_ENVELOPE_VERSION_V1, EnvelopeMethod::HybridV1) => Ok(3),
        (ADAPTIVE_ENVELOPE_VERSION_V2, EnvelopeMethod::Stored) => Ok(0),
        (ADAPTIVE_ENVELOPE_VERSION_V2, EnvelopeMethod::Binary) => Ok(2),
        (ADAPTIVE_ENVELOPE_VERSION_V2, EnvelopeMethod::PatternV2) => Ok(4),
        (ADAPTIVE_ENVELOPE_VERSION_V2, EnvelopeMethod::HybridV2) => Ok(5),
        _ => anyhow::bail!(
            "compression method {:?} is invalid for adaptive envelope v{}",
            method,
            version
        ),
    }
}

fn tag_to_method(version: u8, tag: u8) -> Result<EnvelopeMethod> {
    match (version, tag) {
        (ADAPTIVE_ENVELOPE_VERSION_V1, 0) | (ADAPTIVE_ENVELOPE_VERSION_V2, 0) => {
            Ok(EnvelopeMethod::Stored)
        }
        (ADAPTIVE_ENVELOPE_VERSION_V1, 1) => Ok(EnvelopeMethod::PatternV1),
        (ADAPTIVE_ENVELOPE_VERSION_V1, 2) | (ADAPTIVE_ENVELOPE_VERSION_V2, 2) => {
            Ok(EnvelopeMethod::Binary)
        }
        (ADAPTIVE_ENVELOPE_VERSION_V1, 3) => Ok(EnvelopeMethod::HybridV1),
        (ADAPTIVE_ENVELOPE_VERSION_V2, 4) => Ok(EnvelopeMethod::PatternV2),
        (ADAPTIVE_ENVELOPE_VERSION_V2, 5) => Ok(EnvelopeMethod::HybridV2),
        _ => anyhow::bail!(
            "unknown adaptive compression method {} for envelope v{}",
            tag,
            version
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_envelope_roundtrips_stored_payload() {
        let compressor = AdaptiveCompressor::new();
        let input = b"short";
        let encoded = compressor.compress_enveloped(input).unwrap();
        assert_eq!(encoded.method, CompressionMode::Stored);
        assert_eq!(
            compressor
                .decompress_enveloped(&encoded.compressed)
                .unwrap(),
            input
        );
    }

    #[test]
    fn adaptive_envelope_roundtrips_compressed_payload() {
        let compressor = AdaptiveCompressor::new();
        let input = "Hello and thank you. ".repeat(1000);
        let encoded = compressor.compress_enveloped(input.as_bytes()).unwrap();
        assert_ne!(encoded.method, CompressionMode::Stored);
        assert_eq!(
            compressor
                .decompress_enveloped(&encoded.compressed)
                .unwrap(),
            input.as_bytes()
        );
    }

    #[test]
    fn adaptive_envelope_rejects_trailing_data() {
        let compressor = AdaptiveCompressor::new();
        let mut encoded = compressor.compress_enveloped(b"short").unwrap().compressed;
        encoded.push(0);
        assert!(compressor.decompress_enveloped(&encoded).is_err());
    }
}
