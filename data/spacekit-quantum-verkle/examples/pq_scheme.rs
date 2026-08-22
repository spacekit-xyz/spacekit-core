use alloy_primitives::{Address, B256, U256};
use spacekit_quantum_verkle::commitment::{NistSisScheme, QuantumTree};

fn main() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    let address = Address::from_slice(&[1u8; 20]);
    let key = B256::from_slice(&[2u8; 32]);

    tree.set(&address, &key, U256::from(42u64));
    let proof = tree.create_proof(&address, &key).expect("proof");
    let ok = tree.verify_proof(&proof, &address, &key, U256::from(42u64));
    assert!(ok);
}
