use alloy_primitives::{Address, B256, U256};
use spacekit_quantum_verkle::commitment::{NistSisScheme, QuantumTree, SisOpening};
use spacekit_quantum_verkle::{QuantumMultiProof, QuantumProof, QuantumRangeProof};

fn addr(byte: u8) -> Address {
    Address::from_slice(&[byte; 20])
}

fn key(byte: u8) -> B256 {
    B256::from_slice(&[byte; 32])
}

#[test]
fn crud_and_proof_lifecycle() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    let address = addr(1);
    let key = key(1);

    tree.set(&address, &key, U256::from(42u64));
    assert_eq!(tree.get(&address, &key).unwrap(), U256::from(42u64));

    let proof = tree.create_proof(&address, &key).unwrap();
    assert!(tree.verify_proof(&proof, &address, &key, U256::from(42u64)));

    tree.delete(&address, &key);
    assert!(tree.get(&address, &key).is_err());
}

#[test]
fn batch_update_optimized_updates_root() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    let before = tree.root();

    tree.set(&addr(1), &key(1), U256::from(1u64));
    tree.set(&addr(2), &key(2), U256::from(2u64));

    assert_ne!(before, tree.root());
}

#[test]
fn range_proof_round_trip() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    tree.set(&addr(1), &key(1), U256::from(10u64));
    tree.set(&addr(2), &key(2), U256::from(20u64));
    tree.set(&addr(3), &key(3), U256::from(30u64));

    let proof = tree.create_range_proof(&key(1), &key(2)).unwrap();
    let values = vec![
        (addr(1), key(1), U256::from(10u64)),
        (addr(2), key(2), U256::from(20u64)),
    ];
    assert!(tree.verify_range_proof(&proof, values));
}

#[test]
fn multi_proof_single_leaf_round_trip() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    tree.set(&addr(1), &key(1), U256::from(42u64));

    let keys = vec![(addr(1), key(1))];
    let proof = tree.create_multi_proof(keys.clone()).unwrap();
    let values = vec![U256::from(42u64)];
    assert!(tree.verify_multi_proof(&proof, keys, values));
}

#[test]
fn multi_proof_serialization_single_leaf() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    tree.set(&addr(7), &key(7), U256::from(99u64));
    let keys = vec![(addr(7), key(7))];
    let proof = tree.create_multi_proof(keys.clone()).unwrap();
    let bytes = proof.to_bytes::<NistSisScheme>();
    let decoded =
        QuantumMultiProof::<Vec<SisOpening>>::from_bytes::<NistSisScheme>(&bytes).unwrap();
    let values = vec![U256::from(99u64)];
    assert!(tree.verify_multi_proof(&decoded, keys, values));
}

#[test]
fn multi_proof_round_trip() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    tree.set(&addr(1), &key(1), U256::from(7u64));
    tree.set(&addr(2), &key(2), U256::from(9u64));

    let keys = vec![(addr(1), key(1)), (addr(2), key(2))];
    let proof = tree.create_multi_proof(keys.clone()).unwrap();
    let values = vec![U256::from(7u64), U256::from(9u64)];
    assert!(tree.verify_multi_proof(&proof, keys, values));
}

#[test]
fn proof_serialization_round_trip() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    let address = addr(9);
    let key = key(9);
    let value = U256::from(99u64);
    tree.set(&address, &key, value);

    let proof = tree.create_proof(&address, &key).unwrap();
    let bytes = proof.to_bytes::<NistSisScheme>();
    let decoded = QuantumProof::<SisOpening>::from_bytes::<NistSisScheme>(&bytes).unwrap();
    assert!(tree.verify_proof(&decoded, &address, &key, value));
}

#[test]
fn range_proof_serialization_round_trip() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    tree.set(&addr(1), &key(1), U256::from(10u64));
    tree.set(&addr(2), &key(2), U256::from(20u64));
    tree.set(&addr(3), &key(3), U256::from(30u64));

    let proof = tree.create_range_proof(&key(1), &key(2)).unwrap();
    let bytes = proof.to_bytes::<NistSisScheme>();
    let decoded = QuantumRangeProof::<SisOpening>::from_bytes::<NistSisScheme>(&bytes).unwrap();
    let values = vec![
        (addr(1), key(1), U256::from(10u64)),
        (addr(2), key(2), U256::from(20u64)),
    ];
    assert!(tree.verify_range_proof(&decoded, values));
}

#[test]
fn multi_proof_serialization_round_trip() {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    tree.set(&addr(1), &key(1), U256::from(7u64));
    tree.set(&addr(2), &key(2), U256::from(9u64));

    let keys = vec![(addr(1), key(1)), (addr(2), key(2))];
    let proof = tree.create_multi_proof(keys.clone()).unwrap();
    let bytes = proof.to_bytes::<NistSisScheme>();
    let decoded =
        QuantumMultiProof::<Vec<SisOpening>>::from_bytes::<NistSisScheme>(&bytes).unwrap();
    let values = vec![U256::from(7u64), U256::from(9u64)];
    assert!(tree.verify_multi_proof(&decoded, keys, values));
}
