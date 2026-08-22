#![no_main]

use libfuzzer_sys::fuzz_target;
use spacekit_compressor::AdaptiveCompressor;

fuzz_target!(|data: &[u8]| {
    let compressor = AdaptiveCompressor::new();
    let _ = compressor.decompress_enveloped(data);

    // Built-in seed matrix keeps structured parser paths reachable even when
    // the external corpus starts empty.
    const VALID_V2_STORED_EMPTY: [u8; 26] = [
        b'S', b'K', b'A', b'D', b'C', b'M', b'P', b'R', 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0,
    ];
    let _ = compressor.decompress_enveloped(&VALID_V2_STORED_EMPTY);
    for len in 0..VALID_V2_STORED_EMPTY.len() {
        let _ = compressor.decompress_enveloped(&VALID_V2_STORED_EMPTY[..len]);
    }

    let mut unknown_version = VALID_V2_STORED_EMPTY;
    unknown_version[8] = 99;
    let _ = compressor.decompress_enveloped(&unknown_version);

    let mut unknown_tag = VALID_V2_STORED_EMPTY;
    unknown_tag[9] = 99;
    let _ = compressor.decompress_enveloped(&unknown_tag);

    if data.len() >= 2 {
        let mut structured = Vec::with_capacity(26 + data.len() - 2);
        structured.extend_from_slice(b"SKADCMPR");
        structured.push(data[0]);
        structured.push(data[1]);
        structured.extend_from_slice(&((data.len() - 2) as u64).to_le_bytes());
        structured.extend_from_slice(&((data.len() - 2) as u64).to_le_bytes());
        structured.extend_from_slice(&data[2..]);
        let _ = compressor.decompress_enveloped(&structured);
    }
});
