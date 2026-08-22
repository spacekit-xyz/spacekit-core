use proptest::prelude::*;
use spacekit_compressor::{CompressionMode, PatternCompressor, PatternFormat, SpaceKitCompressor};

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn adaptive_v2_roundtrips_arbitrary_bytes(data in prop::collection::vec(any::<u8>(), 0..16_384)) {
        let compressor = SpaceKitCompressor::new();
        let encoded = compressor.compress(&data, CompressionMode::Adaptive).unwrap();
        let decoded = compressor
            .decompress(&encoded.compressed, CompressionMode::Adaptive)
            .unwrap();
        prop_assert_eq!(decoded, data);
    }

    #[test]
    fn pattern_v2_roundtrips_arbitrary_utf8(text in any::<String>()) {
        let compressor = PatternCompressor::new();
        let encoded = compressor
            .compress_with_format(text.as_bytes(), PatternFormat::V2)
            .unwrap();
        let decoded = compressor
            .decompress_with_format(&encoded, PatternFormat::V2)
            .unwrap();
        prop_assert_eq!(decoded, text.into_bytes());
    }

    #[test]
    fn hybrid_v2_roundtrips_arbitrary_utf8(text in any::<String>()) {
        let compressor = SpaceKitCompressor::new();
        let encoded = compressor
            .compress_with_pattern_format(
                text.as_bytes(),
                CompressionMode::Hybrid,
                PatternFormat::V2,
            )
            .unwrap();
        let decoded = compressor
            .decompress_with_pattern_format(
                &encoded.compressed,
                CompressionMode::Hybrid,
                PatternFormat::V2,
            )
            .unwrap();
        prop_assert_eq!(decoded, text.into_bytes());
    }
}

#[test]
fn adaptive_v1_envelopes_remain_readable() {
    let compressor = spacekit_compressor::AdaptiveCompressor::new();
    let inputs = [
        b"legacy".as_slice(),
        b"Hello and Thank you from the legacy adaptive format. Hello and Thank you.".as_slice(),
        &[0xff, 0xfe, 0xfd, 0x00][..],
    ];
    for input in inputs {
        let encoded = compressor.compress_enveloped_v1(input).unwrap();
        assert_eq!(encoded.compressed[8], 1);
        assert_eq!(
            compressor
                .decompress_enveloped(&encoded.compressed)
                .unwrap(),
            input
        );
    }
}
