# SpaceKit Compressor

Lossless compression library for SpaceKit payloads, with explicit text-pattern,
binary, hybrid, stored, and self-describing adaptive modes.

## Features

- **Pattern Compression**: SpaceKit pattern dictionary for common phrases
- **Binary Compression**: Gzip and LZMA support
- **Adaptive Mode**: Selects stored, gzip, PatternV2, or HybridV2 and records the choice in a versioned envelope
- **Hybrid Mode**: Pattern + Binary for maximum compression
- **Binary-safe Modes**: Stored, Binary, and Adaptive round-trip arbitrary bytes

## Quick Start

```rust
use spacekit_compressor::{SpaceKitCompressor, CompressionMode};

let compressor = SpaceKitCompressor::new();

// Compress a message
let message = "Hello, how are you doing today?";
let result = compressor.compress(
    message.as_bytes(),
    CompressionMode::Adaptive
)?;

println!("Compressed: {} -> {} bytes ({:.1}% savings)",
    result.original_size,
    result.compressed_size,
    result.savings_percent
);

// Decompress
let decompressed = compressor.decompress(
    &result.compressed,
    result.method
)?;
```

## Compression Modes

- **Pattern**: SWTCH pattern dictionary (fast, good for text)
- **Binary**: Gzip by default; `BinaryCompressor` also supports LZMA
- **Adaptive**: Auto-selects stored, binary, PatternV2, or HybridV2 and records the choice in a versioned envelope
- **Hybrid**: Pattern + Binary (best overall)
- **Stored**: Uncompressed bytes, used when compression would increase size

New adaptive payloads use the `SKADCMPR` envelope version 2, which records the
selected inner method and the original and encoded lengths. The decoder
continues to accept version 1. Callers should pass `CompressionMode::Adaptive`
to both `compress` and `decompress`. Declared and decoded payloads are capped at
256 MiB.

### Pattern-mode constraints

The legacy direct Pattern and Hybrid APIs use format v1. For untrusted UTF-8,
select `PatternFormat::V2` through `compress_with_pattern_format` and
`decompress_with_pattern_format`; v2 strictly validates UTF-8 and safely escapes
literal `⚡` markers. Adaptive v2 does this automatically. Use Binary or Adaptive
for arbitrary bytes, checkpoints, or durable storage.

### Bounded gzip streams

`BinaryCompressor` exposes `gzip_compress_reader_to_writer` and
`gzip_decompress_reader_to_writer` for large native artifacts. Both return the
number of output bytes and stop before writing beyond the caller's limit.
`decompress_with_limit` uses the same bounded path for byte-buffer callers.
Growformer uses these APIs for its versioned brain envelope.

## Performance

Run the deterministic Criterion suite with:

```bash
cargo bench -p spacekit-compressor --bench compression
```

A quick release-profile run on the checked-in synthetic fixtures produced:

| Fixture | Binary ratio | Adaptive v2 ratio |
|---|---:|---:|
| 30-byte chat | 1.5667 | 1.8667 |
| 20 KiB repeated message | 0.0077 | 0.0090 |
| 10 KiB synthetic brain JSON | 0.4489 | 0.4514 |
| 500 KiB synthetic brain JSON | 0.3701 | 0.3700 |
| 2.5 MiB synthetic brain JSON | 0.3664 | 0.3662 |

Ratios include each mode's envelope overhead. They are fixture-specific, not
general performance guarantees. Criterion reports compression, decompression,
and full-roundtrip throughput for every fixture.

Property and malformed-envelope tests run with `cargo test -p
spacekit-compressor`. Compile the fuzz target with:

```bash
cargo check --manifest-path spacekit-ai/spacekit-compressor/fuzz/Cargo.toml
# With cargo-fuzz installed:
cd spacekit-ai/spacekit-compressor
cargo fuzz run adaptive_envelope
```

## Integration

```toml
[dependencies]
spacekit-compressor = { path = "../spacekit-ai/spacekit-compressor" }
```

Adjust the path for the consuming workspace. Growformer currently enables this
crate through its optional `brain-compression` Cargo feature.

## Status

- **Version:** 0.1 integration pilot
- Pattern, gzip/LZMA, Hybrid, and Stored modes are implemented.
- Adaptive envelope v2 is implemented; v1 remains readable.
- PatternV2 marker escaping, property tests, malformed-envelope tests, a fuzz target, and Criterion benchmarks are implemented.
- Growformer uses Binary/gzip behind its own versioned `GWFCMPKG` envelope.
- `spacekit-messaging-node` delegates its legacy gzip/LZMA adapter to this crate without changing its live wire path.
- `spacekit-storage-node` delegates gzip here while retaining storage-specific framing and its existing Zstd/Lz4/Brotli codecs.

Remaining before enabling compression on production messaging traffic:

1. Add capability negotiation or a default-off wire setting.
2. Compress before encryption and detect `SKADCMPR` after decryption.
3. Preserve passthrough decoding for legacy uncompressed messages and history.
4. Run sustained fuzzing and cross-version node interoperability tests in CI.

