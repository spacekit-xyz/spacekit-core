use spacekit_compressor::BinaryCompressor;

#[test]
fn gzip_reader_to_writer_roundtrip_reports_byte_counts() {
    let compressor = BinaryCompressor::new();
    let input = b"streaming gzip payload ".repeat(16_384);

    let mut compressed = Vec::new();
    let compressed_bytes = compressor
        .gzip_compress_reader_to_writer(input.as_slice(), &mut compressed, u64::MAX)
        .unwrap();
    assert_eq!(compressed_bytes, compressed.len() as u64);

    let mut exactly_bounded = Vec::new();
    let exactly_bounded_bytes = compressor
        .gzip_compress_reader_to_writer(input.as_slice(), &mut exactly_bounded, compressed_bytes)
        .unwrap();
    assert_eq!(exactly_bounded_bytes, compressed_bytes);
    assert_eq!(exactly_bounded, compressed);

    let mut decoded = Vec::new();
    let decoded_bytes = compressor
        .gzip_decompress_reader_to_writer(compressed.as_slice(), &mut decoded, input.len() as u64)
        .unwrap();
    assert_eq!(decoded_bytes, input.len() as u64);
    assert_eq!(decoded, input);
}

#[test]
fn gzip_reader_to_writer_rejects_output_past_exact_limits() {
    let compressor = BinaryCompressor::new();
    let input = b"bounded gzip payload ".repeat(8_192);

    let mut compressed = Vec::new();
    compressor
        .gzip_compress_reader_to_writer(input.as_slice(), &mut compressed, u64::MAX)
        .unwrap();

    let compressed_limit = compressed.len() as u64 - 1;
    let mut bounded_compressed = Vec::new();
    let compression_error = compressor
        .gzip_compress_reader_to_writer(input.as_slice(), &mut bounded_compressed, compressed_limit)
        .unwrap_err();
    assert_eq!(bounded_compressed.len() as u64, compressed_limit);
    assert!(compression_error
        .to_string()
        .contains(&format!("exceeds {compressed_limit} byte limit")));

    let decoded_limit = input.len() as u64 - 1;
    let mut bounded_decoded = Vec::new();
    let decompression_error = compressor
        .gzip_decompress_reader_to_writer(
            compressed.as_slice(),
            &mut bounded_decoded,
            decoded_limit,
        )
        .unwrap_err();
    assert_eq!(bounded_decoded.len() as u64, decoded_limit);
    assert!(decompression_error
        .to_string()
        .contains(&format!("exceeds {decoded_limit} byte limit")));
}
