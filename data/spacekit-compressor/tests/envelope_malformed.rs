use spacekit_compressor::{AdaptiveCompressor, BinaryCompressor, MAX_ADAPTIVE_BYTES};

const HEADER_LEN: usize = 26;

fn envelope(version: u8, tag: u8, original_len: u64, payload: &[u8]) -> Vec<u8> {
    let mut data = Vec::with_capacity(HEADER_LEN + payload.len());
    data.extend_from_slice(b"SKADCMPR");
    data.push(version);
    data.push(tag);
    data.extend_from_slice(&original_len.to_le_bytes());
    data.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    data.extend_from_slice(payload);
    data
}

#[test]
fn every_truncated_header_is_rejected() {
    let compressor = AdaptiveCompressor::new();
    let valid = envelope(2, 0, 0, &[]);
    for len in 0..HEADER_LEN {
        assert!(
            compressor.decompress_enveloped(&valid[..len]).is_err(),
            "accepted header truncated to {len} bytes"
        );
    }
}

#[test]
fn malformed_header_fields_are_rejected() {
    let compressor = AdaptiveCompressor::new();

    let mut bad_magic = envelope(2, 0, 0, &[]);
    bad_magic[0] ^= 0xff;
    assert!(compressor.decompress_enveloped(&bad_magic).is_err());

    assert!(compressor
        .decompress_enveloped(&envelope(99, 0, 0, &[]))
        .is_err());
    assert!(compressor
        .decompress_enveloped(&envelope(2, 99, 0, &[]))
        .is_err());
    assert!(compressor
        .decompress_enveloped(&envelope(2, 1, 0, &[]))
        .is_err());
}

#[test]
fn declared_lengths_are_bounded_and_exact() {
    let compressor = AdaptiveCompressor::new();

    let mut trailing = envelope(2, 0, 0, &[]);
    trailing.push(0);
    assert!(compressor.decompress_enveloped(&trailing).is_err());

    let decoded_mismatch = envelope(2, 0, 2, b"x");
    assert!(compressor.decompress_enveloped(&decoded_mismatch).is_err());

    let oversized = envelope(2, 0, (MAX_ADAPTIVE_BYTES as u64) + 1, &[]);
    assert!(compressor.decompress_enveloped(&oversized).is_err());

    let mut oversized_payload = envelope(2, 0, 0, &[]);
    oversized_payload[18..26].copy_from_slice(&((MAX_ADAPTIVE_BYTES as u64) + 1).to_le_bytes());
    assert!(compressor.decompress_enveloped(&oversized_payload).is_err());
}

#[test]
fn pattern_v2_rejects_unescaped_marker_payload() {
    let compressor = AdaptiveCompressor::new();
    let malformed = envelope(2, 4, "⚡?".len() as u64, "⚡?".as_bytes());
    assert!(compressor.decompress_enveloped(&malformed).is_err());
}

#[test]
fn compressed_output_cannot_exceed_declared_size() {
    let compressor = AdaptiveCompressor::new();
    let gzip = BinaryCompressor::new()
        .compress(&vec![0u8; 1024 * 1024])
        .unwrap();
    let disguised_bomb = envelope(2, 2, 1, &gzip);
    assert!(compressor
        .decompress_enveloped(&disguised_bomb)
        .unwrap_err()
        .to_string()
        .contains("exceeds 1 byte limit"));
}
