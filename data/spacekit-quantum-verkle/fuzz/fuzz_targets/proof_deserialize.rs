#![no_main]

use libfuzzer_sys::fuzz_target;
use spacekit_quantum_verkle::commitment::{NistSisScheme, SisOpening};
use spacekit_quantum_verkle::QuantumProof;

fuzz_target!(|data: &[u8]| {
    if let Ok(proof) = QuantumProof::<SisOpening>::from_bytes::<NistSisScheme>(data) {
        let _ = proof.to_bytes::<NistSisScheme>();
    }
});
