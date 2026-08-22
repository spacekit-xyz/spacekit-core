use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use spacekit_compressor::{CompressionMode, PatternFormat, SpaceKitCompressor};
use std::hint::black_box;
use std::time::Duration;

struct Fixture {
    name: &'static str,
    bytes: Vec<u8>,
    text_safe: bool,
}

fn deterministic_bytes(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

fn synthetic_brain(target_bytes: usize) -> Vec<u8> {
    let mut json = String::from(
        r#"{"format":"GWFBRPKG-synthetic","header":{"id":"compression-benchmark","version":2},"weights":["#,
    );
    let mut i = 0usize;
    while json.len() < target_bytes.saturating_sub(64) {
        let value = ((i.wrapping_mul(2654435761) % 100_000) as f64) / 100_000.0;
        json.push_str(&format!("{value:.5},"));
        i += 1;
    }
    json.push_str(r#"0.0],"plugins":"[sentiment]\nmeta_gk_margin=0.06\n"}"#);
    json.into_bytes()
}

fn fixtures() -> Vec<Fixture> {
    vec![
        Fixture {
            name: "short_chat",
            bytes: b"Hello, how are you? Thank you!".to_vec(),
            text_safe: true,
        },
        Fixture {
            name: "repeated_chat_20k",
            bytes: "Hello and thank you. This message is from the SpaceKit network. "
                .repeat(320)
                .into_bytes(),
            text_safe: true,
        },
        Fixture {
            name: "code_8k",
            bytes: "public async function route(input) { const value = await lookup(input); return value; }\n"
                .repeat(100)
                .into_bytes(),
            text_safe: true,
        },
        Fixture {
            name: "marker_utf8",
            bytes: "Status ⚡ fast; literal ⚡rt; Hello and Thank you. "
                .repeat(200)
                .into_bytes(),
            text_safe: true,
        },
        Fixture {
            name: "invalid_utf8_64k",
            bytes: {
                let mut bytes = deterministic_bytes(64 * 1024);
                bytes[0] = 0xff;
                bytes[1] = 0xfe;
                bytes
            },
            text_safe: false,
        },
        Fixture {
            name: "random_64k",
            bytes: deterministic_bytes(64 * 1024),
            text_safe: false,
        },
        Fixture {
            name: "brain_small_10k",
            bytes: synthetic_brain(10 * 1024),
            text_safe: true,
        },
        Fixture {
            name: "brain_medium_500k",
            bytes: synthetic_brain(500 * 1024),
            text_safe: true,
        },
        Fixture {
            name: "brain_large_2500k",
            bytes: synthetic_brain(2_500 * 1024),
            text_safe: true,
        },
    ]
}

fn encode(
    compressor: &SpaceKitCompressor,
    data: &[u8],
    mode: CompressionMode,
    text_safe: bool,
) -> Vec<u8> {
    match mode {
        CompressionMode::Pattern | CompressionMode::Hybrid if text_safe => {
            compressor
                .compress_with_pattern_format(data, mode, PatternFormat::V2)
                .unwrap()
                .compressed
        }
        _ => compressor.compress(data, mode).unwrap().compressed,
    }
}

fn decode(
    compressor: &SpaceKitCompressor,
    data: &[u8],
    mode: CompressionMode,
    text_safe: bool,
) -> Vec<u8> {
    match mode {
        CompressionMode::Pattern | CompressionMode::Hybrid if text_safe => compressor
            .decompress_with_pattern_format(data, mode, PatternFormat::V2)
            .unwrap(),
        _ => compressor.decompress(data, mode).unwrap(),
    }
}

fn modes(text_safe: bool) -> Vec<CompressionMode> {
    let mut modes = vec![
        CompressionMode::Stored,
        CompressionMode::Binary,
        CompressionMode::Adaptive,
    ];
    if text_safe {
        modes.push(CompressionMode::Pattern);
        modes.push(CompressionMode::Hybrid);
    }
    modes
}

fn benchmark_compression(c: &mut Criterion) {
    let compressor = SpaceKitCompressor::new();
    for fixture in fixtures() {
        let mut group = c.benchmark_group(format!("compress/{}", fixture.name));
        group.throughput(Throughput::Bytes(fixture.bytes.len() as u64));
        for mode in modes(fixture.text_safe) {
            let encoded = encode(&compressor, &fixture.bytes, mode, fixture.text_safe);
            eprintln!(
                "ratio fixture={} mode={mode:?} original={} encoded={} ratio={:.4}",
                fixture.name,
                fixture.bytes.len(),
                encoded.len(),
                encoded.len() as f64 / fixture.bytes.len().max(1) as f64
            );
            group.bench_with_input(
                BenchmarkId::new(format!("{mode:?}"), fixture.bytes.len()),
                &fixture.bytes,
                |b, data| b.iter(|| encode(&compressor, black_box(data), mode, fixture.text_safe)),
            );
        }
        group.finish();
    }
}

fn benchmark_decompression(c: &mut Criterion) {
    let compressor = SpaceKitCompressor::new();
    for fixture in fixtures() {
        let mut group = c.benchmark_group(format!("decompress/{}", fixture.name));
        group.throughput(Throughput::Bytes(fixture.bytes.len() as u64));
        for mode in modes(fixture.text_safe) {
            let encoded = encode(&compressor, &fixture.bytes, mode, fixture.text_safe);
            group.bench_with_input(
                BenchmarkId::new(format!("{mode:?}"), encoded.len()),
                &encoded,
                |b, data| b.iter(|| decode(&compressor, black_box(data), mode, fixture.text_safe)),
            );
        }
        group.finish();
    }
}

fn benchmark_roundtrip(c: &mut Criterion) {
    let compressor = SpaceKitCompressor::new();
    let message_fixture = Fixture {
        name: "message_20k",
        bytes: "Hello and thank you. This message is from the SpaceKit network. "
            .repeat(320)
            .into_bytes(),
        text_safe: true,
    };
    let cases = [
        ("message_20k", message_fixture),
        (
            "brain_500k",
            Fixture {
                name: "brain_500k",
                bytes: synthetic_brain(500 * 1024),
                text_safe: true,
            },
        ),
    ];
    let mut group = c.benchmark_group("roundtrip");
    for (name, fixture) in cases {
        group.throughput(Throughput::Bytes(fixture.bytes.len() as u64));
        for mode in modes(fixture.text_safe) {
            group.bench_function(BenchmarkId::new(name, format!("{mode:?}")), |b| {
                b.iter(|| {
                    let encoded = encode(
                        &compressor,
                        black_box(&fixture.bytes),
                        mode,
                        fixture.text_safe,
                    );
                    decode(&compressor, black_box(&encoded), mode, fixture.text_safe)
                })
            });
        }
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(1))
        .warm_up_time(Duration::from_millis(500));
    targets = benchmark_compression, benchmark_decompression, benchmark_roundtrip
}
criterion_main!(benches);
