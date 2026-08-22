//! SWTCH Pattern Compression
//!
//! Pattern-based compression using a dictionary of common strings and codes.
//! Ported from Python SWTCH Compressor with optimizations for Rust.

use anyhow::Result;
use lazy_static::lazy_static;
use std::collections::HashMap;

const MARKER: &str = "⚡";

lazy_static! {
    /// Common messaging patterns (ported from Python SWTCH Compressor)
    static ref MESSAGING_PATTERNS: HashMap<&'static str, &'static str> = {
        let mut m = HashMap::new();

        // Common greetings
        m.insert("Hello", "⚡H");
        m.insert("Hi", "⚡h");
        m.insert("Hey", "⚡y");
        m.insert("Good morning", "⚡M");
        m.insert("Good afternoon", "⚡A");
        m.insert("Good evening", "⚡E");
        m.insert("Goodnight", "⚡N");

        // Common phrases
        m.insert("How are you", "⚡Q");
        m.insert("Thank you", "⚡T");
        m.insert("You're welcome", "⚡W");
        m.insert("See you later", "⚡L");
        m.insert("Talk to you later", "⚡K");
        m.insert("Be right back", "⚡B");
        m.insert("On my way", "⚡O");

        // Common words
        m.insert("the", "⚡t");
        m.insert("and", "⚡a");
        m.insert("that", "⚡d");
        m.insert("have", "⚡v");
        m.insert("with", "⚡w");
        m.insert("this", "⚡s");
        m.insert("from", "⚡f");
        m.insert("they", "⚡Ty");
        m.insert("would", "⚡u");
        m.insert("there", "⚡r");

        // Programming keywords
        m.insert("function", "⚡fn");
        m.insert("const", "⚡cn");
        m.insert("return", "⚡rt");
        m.insert("import", "⚡im");
        m.insert("export", "⚡ex");
        m.insert("class", "⚡cl");
        m.insert("public", "⚡pb");
        m.insert("private", "⚡pv");
        m.insert("async", "⚡as");
        m.insert("await", "⚡aw");

        m
    };
}

/// On-wire pattern representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternFormat {
    /// Legacy unescaped representation. Only safe when input contains no marker.
    V1,
    /// Strict UTF-8 representation with escaped literal markers.
    V2,
}

/// Pattern-based compressor
pub struct PatternCompressor {
    patterns: &'static HashMap<&'static str, &'static str>,
}

impl Default for PatternCompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl PatternCompressor {
    /// Create a new pattern compressor
    pub fn new() -> Self {
        Self {
            patterns: &MESSAGING_PATTERNS,
        }
    }

    /// Compress using the legacy v1 representation.
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.compress_with_format(data, PatternFormat::V1)
    }

    /// Compress using the requested pattern representation.
    pub fn compress_with_format(&self, data: &[u8], format: PatternFormat) -> Result<Vec<u8>> {
        let text = match format {
            PatternFormat::V1 => String::from_utf8_lossy(data).into_owned(),
            PatternFormat::V2 => std::str::from_utf8(data)
                .map_err(|e| anyhow::anyhow!("pattern v2 requires valid UTF-8: {}", e))?
                .replace(MARKER, "⚡⚡"),
        };
        let mut result = text;

        // Sort patterns by length (longest first) to avoid partial matches
        let mut pattern_vec: Vec<_> = self.patterns.iter().collect();
        pattern_vec.sort_by_key(|(k, _)| std::cmp::Reverse(k.len()));

        // Replace patterns with codes
        for (pattern, code) in pattern_vec {
            result = result.replace(pattern, code);
        }

        Ok(result.into_bytes())
    }

    /// Decompress the legacy v1 representation.
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.decompress_with_format(data, PatternFormat::V1)
    }

    /// Decompress the requested pattern representation.
    pub fn decompress_with_format(&self, data: &[u8], format: PatternFormat) -> Result<Vec<u8>> {
        if format == PatternFormat::V2 {
            return self.decompress_v2(data);
        }
        let text = String::from_utf8_lossy(data);
        let mut result = text.into_owned();

        // Longest codes first so e.g. `⚡rt` (return) is not broken by `⚡r` (there).
        let mut pairs = self.reverse_pairs();
        pairs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));

        for (pattern, code) in pairs {
            result = result.replace(code, pattern);
        }

        Ok(result.into_bytes())
    }

    fn decompress_v2(&self, data: &[u8]) -> Result<Vec<u8>> {
        let text = std::str::from_utf8(data)
            .map_err(|e| anyhow::anyhow!("pattern v2 payload is not valid UTF-8: {}", e))?;
        let mut pairs = self.reverse_pairs();
        pairs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(b.0)));

        let mut result = String::with_capacity(text.len());
        let mut offset = 0;
        while offset < text.len() {
            let rest = &text[offset..];
            if !rest.starts_with(MARKER) {
                let next = rest.find(MARKER).unwrap_or(rest.len());
                result.push_str(&rest[..next]);
                offset += next;
                continue;
            }

            let after_marker = &rest[MARKER.len()..];
            if after_marker.starts_with(MARKER) {
                result.push_str(MARKER);
                offset += MARKER.len() * 2;
                continue;
            }

            if let Some((pattern, code)) = pairs.iter().find(|(_, code)| rest.starts_with(*code)) {
                result.push_str(pattern);
                offset += code.len();
                continue;
            }

            anyhow::bail!(
                "pattern v2 payload contains an unescaped marker at byte {}",
                offset
            );
        }

        Ok(result.into_bytes())
    }

    fn reverse_pairs(&self) -> Vec<(&str, &str)> {
        self.patterns.iter().map(|(p, c)| (*p, *c)).collect()
    }

    /// Estimate compression ratio for given data
    pub fn estimate_ratio(&self, data: &[u8]) -> f64 {
        let text = String::from_utf8_lossy(data);
        let mut matches = 0;
        let mut saved_bytes = 0;

        for (pattern, code) in self.patterns.iter() {
            let count = text.matches(pattern).count();
            if count > 0 && pattern.len() > code.len() {
                matches += count;
                saved_bytes += count * (pattern.len() - code.len());
            }
        }

        if matches > 0 {
            1.0 - (saved_bytes as f64 / data.len() as f64)
        } else {
            1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_compression() {
        let compressor = PatternCompressor::new();
        let message = "Hello, How are you doing today? Thank you for your message!";

        let compressed = compressor.compress(message.as_bytes()).unwrap();
        let compressed_str = String::from_utf8_lossy(&compressed);

        // Should contain pattern codes
        assert!(compressed_str.contains("⚡H")); // Hello
        assert!(compressed_str.contains("⚡Q")); // How are you
        assert!(compressed_str.contains("⚡T")); // Thank you

        // Decompress should restore original
        let decompressed = compressor.decompress(&compressed).unwrap();
        assert_eq!(decompressed, message.as_bytes());
    }

    #[test]
    fn test_code_compression() {
        let compressor = PatternCompressor::new();
        let code = "function calculateTotal(items) { return items.reduce((sum, item) => sum + item.price, 0); }";

        let compressed = compressor.compress(code.as_bytes()).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed, code.as_bytes());
        assert!(compressed.len() <= code.len());
    }

    #[test]
    fn pattern_v2_roundtrips_literal_markers_and_codes() {
        let compressor = PatternCompressor::new();
        let text = "⚡ ⚡rt Hello return ⚡return ⚡⚡ Thank you";
        let compressed = compressor
            .compress_with_format(text.as_bytes(), PatternFormat::V2)
            .unwrap();
        let decompressed = compressor
            .decompress_with_format(&compressed, PatternFormat::V2)
            .unwrap();
        assert_eq!(decompressed, text.as_bytes());
    }

    #[test]
    fn pattern_v2_rejects_invalid_utf8() {
        let compressor = PatternCompressor::new();
        assert!(compressor
            .compress_with_format(&[0xff, 0xfe], PatternFormat::V2)
            .is_err());
    }
}
