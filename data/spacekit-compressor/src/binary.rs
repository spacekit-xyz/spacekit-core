//! Binary Compression Module
//!
//! Provides gzip and LZMA compression for maximum compression ratios.

use anyhow::Result;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{self, Read, Write};

struct OutputLimitWriter<W> {
    inner: W,
    written: u64,
    max_output: u64,
    output_name: &'static str,
}

impl<W> OutputLimitWriter<W> {
    fn new(inner: W, max_output: u64, output_name: &'static str) -> Self {
        Self {
            inner,
            written: 0,
            max_output,
            output_name,
        }
    }

    fn bytes_written(&self) -> u64 {
        self.written
    }

    fn limit_error(&self) -> io::Error {
        io::Error::new(
            io::ErrorKind::Other,
            format!(
                "{} output exceeds {} byte limit",
                self.output_name, self.max_output
            ),
        )
    }
}

impl<W: Write> Write for OutputLimitWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let remaining = self.max_output.saturating_sub(self.written);
        if remaining == 0 {
            return Err(self.limit_error());
        }

        let allowed = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let written = self.inner.write(&buf[..allowed])?;
        self.written += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Binary compression algorithms
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryAlgorithm {
    Gzip,
    Lzma,
}

/// Binary compressor
pub struct BinaryCompressor {
    algorithm: BinaryAlgorithm,
    level: u32,
}

impl Default for BinaryCompressor {
    fn default() -> Self {
        Self::new()
    }
}

impl BinaryCompressor {
    /// Create with default settings (Gzip, level 6)
    pub fn new() -> Self {
        Self {
            algorithm: BinaryAlgorithm::Gzip,
            level: 6,
        }
    }

    /// Create with specific algorithm and level
    pub fn with_settings(algorithm: BinaryAlgorithm, level: u32) -> Self {
        Self {
            algorithm,
            level: level.clamp(1, 9),
        }
    }

    /// Compress data
    pub fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.algorithm {
            BinaryAlgorithm::Gzip => self.gzip_compress(data),
            BinaryAlgorithm::Lzma => self.lzma_compress(data),
        }
    }

    /// Decompress data
    pub fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        match self.algorithm {
            BinaryAlgorithm::Gzip => self.gzip_decompress(data),
            BinaryAlgorithm::Lzma => self.lzma_decompress(data),
        }
    }

    /// Decompress gzip while refusing output larger than `max_output`.
    ///
    /// LZMA callers retain the normal decoder because this crate currently
    /// uses bounded decoding only for gzip-backed adaptive envelopes.
    pub fn decompress_with_limit(&self, data: &[u8], max_output: usize) -> Result<Vec<u8>> {
        match self.algorithm {
            BinaryAlgorithm::Gzip => {
                let mut decompressed = Vec::with_capacity(max_output.min(64 * 1024));
                self.gzip_decompress_reader_to_writer(data, &mut decompressed, max_output as u64)?;
                Ok(decompressed)
            }
            BinaryAlgorithm::Lzma => {
                let decompressed = self.lzma_decompress(data)?;
                if decompressed.len() > max_output {
                    anyhow::bail!("decompressed payload exceeds {} byte limit", max_output);
                }
                Ok(decompressed)
            }
        }
    }

    /// Compress gzip data from `reader` into `writer`.
    ///
    /// At most `max_output` compressed bytes are written. The returned count is
    /// the number of compressed bytes written, including the gzip header and
    /// trailer.
    pub fn gzip_compress_reader_to_writer<R: Read, W: Write>(
        &self,
        mut reader: R,
        writer: W,
        max_output: u64,
    ) -> Result<u64> {
        let bounded = OutputLimitWriter::new(writer, max_output, "compressed");
        let mut encoder = GzEncoder::new(bounded, Compression::new(self.level));
        io::copy(&mut reader, &mut encoder)?;
        let bounded = encoder.finish()?;
        Ok(bounded.bytes_written())
    }

    /// Decompress gzip data from `reader` into `writer`.
    ///
    /// At most `max_output` decompressed bytes are written. The returned count
    /// is the number of decompressed bytes written.
    pub fn gzip_decompress_reader_to_writer<R: Read, W: Write>(
        &self,
        reader: R,
        writer: W,
        max_output: u64,
    ) -> Result<u64> {
        let mut decoder = GzDecoder::new(reader);
        let mut bounded = OutputLimitWriter::new(writer, max_output, "decompressed");
        io::copy(&mut decoder, &mut bounded)?;
        Ok(bounded.bytes_written())
    }

    fn gzip_compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut compressed = Vec::new();
        self.gzip_compress_reader_to_writer(data, &mut compressed, u64::MAX)?;
        Ok(compressed)
    }

    fn gzip_decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        let mut decompressed = Vec::new();
        self.gzip_decompress_reader_to_writer(data, &mut decompressed, u64::MAX)?;
        Ok(decompressed)
    }

    fn lzma_compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        use lzma_rs::lzma_compress;
        let mut compressed = Vec::new();
        lzma_compress(&mut std::io::Cursor::new(data), &mut compressed)?;
        Ok(compressed)
    }

    fn lzma_decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        use lzma_rs::lzma_decompress;
        let mut decompressed = Vec::new();
        lzma_decompress(&mut std::io::Cursor::new(data), &mut decompressed)?;
        Ok(decompressed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_limit_writer_never_writes_past_limit() {
        let mut output = Vec::new();
        let mut writer = OutputLimitWriter::new(&mut output, 3, "test");

        let error = writer.write_all(b"abcdef").unwrap_err();
        let bytes_written = writer.bytes_written();
        drop(writer);

        assert_eq!(bytes_written, 3);
        assert_eq!(output, b"abc");
        assert!(error.to_string().contains("exceeds 3 byte limit"));
    }
}
