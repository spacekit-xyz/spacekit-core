//! Bounded-memory byte streaming for large files (videos, archives, etc.).
//!
//! ## What this fixes
//!
//! If you currently "stream" files by reading the whole file — or by
//! accumulating chunks — into a `Vec<u8>` / `BytesMut` before sending,
//! memory grows as `file_size × concurrent_streams`. Worse, if those buffers
//! live in a long-lived structure (a global cache, a connection pool, a
//! `BytesMut` whose capacity only ever climbs), they're never reclaimed and
//! memory keeps rising across plays.
//!
//! ## What this does
//!
//! Reads files in fixed-size chunks and yields each chunk as soon as it's
//! read. The consumer (an HTTP response body, a websocket sink, etc.) drives
//! the stream, so reading naturally pauses when the network is slow —
//! backpressure stops chunks piling up. Memory per active stream stays at
//! roughly `chunk_size` regardless of file size.
//!
//! ## Cargo.toml
//!
//! ```toml
//! [dependencies]
//! tokio        = { version = "1", features = ["fs", "io-util", "rt-multi-thread"] }
//! tokio-util   = { version = "0.7", features = ["io"] }
//! bytes        = "1"
//! futures-core = "0.3"
//!
//! [dev-dependencies]
//! futures-util = "0.3"
//! tempfile     = "3"
//! ```
//!
//! ## Quick start
//!
//! ```no_run
//! use std::path::Path;
//! use streaming::{file_stream, ByteRange, StreamingConfig};
//!
//! # async fn demo() -> std::io::Result<()> {
//! // Whole file, default 64 KiB chunks
//! let (stream, meta) = file_stream(
//!     Path::new("movie.mp4"),
//!     StreamingConfig::default(),
//!     None,
//! ).await?;
//!
//! // A byte range (e.g. from an HTTP Range header)
//! let (range_stream, range_meta) = file_stream(
//!     Path::new("movie.mp4"),
//!     StreamingConfig::default(),
//!     Some(ByteRange { start: 1024, end: Some(8191) }),
//! ).await?;
//! # let _ = (stream, meta, range_stream, range_meta); Ok(()) }
//! ```
//!
//! ## With axum
//!
//! ```ignore
//! use axum::{body::Body, http::{header, HeaderMap, StatusCode}, response::Response};
//! use streaming::{file_stream, ByteRange, StreamingConfig};
//! use std::path::PathBuf;
//!
//! pub async fn serve_video(headers: HeaderMap) -> Result<Response, StatusCode> {
//!     let path = PathBuf::from("/videos/movie.mp4");
//!     let size = tokio::fs::metadata(&path).await
//!         .map_err(|_| StatusCode::NOT_FOUND)?.len();
//!
//!     let range = headers.get(header::RANGE)
//!         .and_then(|v| v.to_str().ok())
//!         .and_then(|s| ByteRange::parse(s, size));
//!
//!     let (stream, meta) = file_stream(&path, StreamingConfig::default(), range)
//!         .await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
//!
//!     let status = if range.is_some() { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK };
//!     let mut resp = Response::builder()
//!         .status(status)
//!         .header(header::CONTENT_TYPE, "video/mp4")
//!         .header(header::ACCEPT_RANGES, "bytes")
//!         .header(header::CONTENT_LENGTH, meta.length);
//!     if range.is_some() {
//!         resp = resp.header(header::CONTENT_RANGE, meta.content_range());
//!     }
//!     resp.body(Body::from_stream(stream)).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
//! }
//! ```

use std::io;
use std::path::Path;

use bytes::Bytes;
use futures_core::Stream;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, SeekFrom};
use tokio_util::io::ReaderStream;

/// Configuration for the streaming reader.
#[derive(Clone, Copy, Debug)]
pub struct StreamingConfig {
    /// Bytes per chunk. Memory per active stream ≈ this value.
    ///
    /// - 64 KiB (default) is a good balance for HTTP video streaming.
    /// - Larger values (256 KiB – 1 MiB) reduce syscalls but use more memory.
    /// - Smaller values (8–16 KiB) reduce time-to-first-byte.
    pub chunk_size: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            chunk_size: 64 * 1024,
        }
    }
}

/// HTTP-style inclusive byte range. `end = None` means "to end of file".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: Option<u64>,
}

impl ByteRange {
    /// Parse a single-range `Range: bytes=...` header value.
    /// Supports `bytes=START-`, `bytes=START-END`, and `bytes=-SUFFIX`.
    /// Returns `None` for malformed, empty, multi-range, or out-of-bounds inputs.
    pub fn parse(header: &str, file_size: u64) -> Option<Self> {
        let spec = header.trim().strip_prefix("bytes=")?;
        if spec.contains(',') {
            // Multi-range not supported; let the caller decide what to do.
            return None;
        }
        let (a, b) = spec.split_once('-')?;
        match (a.trim(), b.trim()) {
            ("", "") => None,
            ("", suffix) => {
                let n: u64 = suffix.parse().ok()?;
                if n == 0 || file_size == 0 {
                    return None;
                }
                let n = n.min(file_size);
                Some(Self {
                    start: file_size - n,
                    end: Some(file_size - 1),
                })
            }
            (start, "") => {
                let s: u64 = start.parse().ok()?;
                if s >= file_size {
                    return None;
                }
                Some(Self {
                    start: s,
                    end: None,
                })
            }
            (start, end) => {
                let s: u64 = start.parse().ok()?;
                let e: u64 = end.parse().ok()?;
                if s > e || s >= file_size {
                    return None;
                }
                Some(Self {
                    start: s,
                    end: Some(e.min(file_size - 1)),
                })
            }
        }
    }

    /// Number of bytes this range covers, clamped to the file.
    pub fn length(&self, file_size: u64) -> u64 {
        if self.start >= file_size {
            return 0;
        }
        let end = self.end.unwrap_or(file_size - 1).min(file_size - 1);
        end - self.start + 1
    }
}

/// Metadata about the stream being served — useful for setting HTTP headers
/// like `Content-Length` and `Content-Range`.
#[derive(Clone, Copy, Debug)]
pub struct StreamMeta {
    pub total_size: u64,
    pub start: u64,
    pub length: u64,
}

impl StreamMeta {
    /// Format a `Content-Range` header value: `bytes START-END/TOTAL`.
    pub fn content_range(&self) -> String {
        let end = self.start + self.length.saturating_sub(1);
        format!("bytes {}-{}/{}", self.start, end, self.total_size)
    }
}

/// Wrap any `AsyncRead` into a bounded-memory chunked stream.
///
/// Each item is a `Bytes` of up to `config.chunk_size` bytes. The underlying
/// reader is consumed lazily — memory usage stays at ~`chunk_size` regardless
/// of total content size.
pub fn byte_stream<R>(reader: R, config: StreamingConfig) -> impl Stream<Item = io::Result<Bytes>>
where
    R: AsyncRead,
{
    ReaderStream::with_capacity(reader, config.chunk_size)
}

/// Open a file and produce a bounded-memory chunk stream, optionally limited
/// to a byte range.
///
/// Returns `InvalidInput` if `range.start` is past the end of the file.
/// Prepend a fixed byte prefix before a tail stream (e.g. re-wrapped envelope header + ciphertext tail).
pub fn prepend_bytes(
    prefix: Bytes,
    tail: impl Stream<Item = io::Result<Bytes>> + Send + 'static,
) -> impl Stream<Item = io::Result<Bytes>> {
    use futures::stream::{self, StreamExt};
    stream::once(async move { Ok(prefix) }).chain(tail)
}

pub async fn file_stream(
    path: &Path,
    config: StreamingConfig,
    range: Option<ByteRange>,
) -> io::Result<(impl Stream<Item = io::Result<Bytes>>, StreamMeta)> {
    let mut file = File::open(path).await?;
    let total_size = file.metadata().await?.len();

    let (start, length) = match range {
        Some(r) => {
            if r.start >= total_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "range start past end of file",
                ));
            }
            (r.start, r.length(total_size))
        }
        None => (0, total_size),
    };

    if start > 0 {
        file.seek(SeekFrom::Start(start)).await?;
    }

    // `take` caps how many bytes will be read — important for ranges and for
    // making the stream terminate at exactly the requested end byte.
    let limited = file.take(length);
    let stream = ReaderStream::with_capacity(limited, config.chunk_size);

    Ok((
        stream,
        StreamMeta {
            total_size,
            start,
            length,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_file(contents: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(contents).unwrap();
        f.flush().unwrap();
        f
    }

    #[tokio::test]
    async fn streams_whole_file_and_respects_chunk_size() {
        let data: Vec<u8> = (0..10_000u32).map(|i| i as u8).collect();
        let f = make_file(&data);

        let (mut s, meta) = file_stream(f.path(), StreamingConfig { chunk_size: 1024 }, None)
            .await
            .unwrap();

        assert_eq!(meta.total_size, 10_000);
        assert_eq!(meta.length, 10_000);

        let mut collected = Vec::new();
        let mut max_chunk = 0usize;
        while let Some(chunk) = s.next().await {
            let c = chunk.unwrap();
            max_chunk = max_chunk.max(c.len());
            collected.extend_from_slice(&c);
        }

        assert_eq!(collected, data);
        assert!(max_chunk <= 1024, "chunks must respect chunk_size");
    }

    #[tokio::test]
    async fn streams_byte_range() {
        let data: Vec<u8> = (0..5_000u32).map(|i| i as u8).collect();
        let f = make_file(&data);

        let (mut s, meta) = file_stream(
            f.path(),
            StreamingConfig { chunk_size: 256 },
            Some(ByteRange {
                start: 1000,
                end: Some(1999),
            }),
        )
        .await
        .unwrap();

        assert_eq!(meta.start, 1000);
        assert_eq!(meta.length, 1000);
        assert_eq!(meta.content_range(), "bytes 1000-1999/5000");

        let mut collected = Vec::new();
        while let Some(chunk) = s.next().await {
            collected.extend_from_slice(&chunk.unwrap());
        }
        assert_eq!(collected, data[1000..2000]);
    }

    #[test]
    fn parses_range_header() {
        let size = 10_000;
        assert_eq!(
            ByteRange::parse("bytes=0-499", size),
            Some(ByteRange {
                start: 0,
                end: Some(499)
            })
        );
        assert_eq!(
            ByteRange::parse("bytes=500-", size),
            Some(ByteRange {
                start: 500,
                end: None
            })
        );
        // suffix range: last 100 bytes
        assert_eq!(
            ByteRange::parse("bytes=-100", size),
            Some(ByteRange {
                start: 9900,
                end: Some(9999)
            })
        );
        // out of bounds
        assert_eq!(ByteRange::parse("bytes=99999-", size), None);
        // multi-range -> reject
        assert_eq!(ByteRange::parse("bytes=0-100,200-300", size), None);
    }
}
