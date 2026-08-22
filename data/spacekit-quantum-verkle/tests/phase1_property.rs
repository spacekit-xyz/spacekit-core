use alloy_primitives::{Address, B256, U256};
use proptest::prelude::*;
use spacekit_quantum_verkle::commitment::{NistSisScheme, QuantumTree};

fn addr(byte: u8) -> Address {
    Address::from_slice(&[byte; 20])
}

fn key(byte: u8) -> B256 {
    B256::from_slice(&[byte; 32])
}

proptest! {
    #[test]
    fn root_changes_only_on_mutation(values in prop::collection::vec(any::<u8>(), 1..10)) {
    let mut tree = QuantumTree::<NistSisScheme>::new();
        let initial_root = tree.root();

        for (i, value) in values.iter().enumerate() {
            tree.set(&addr(1), &key(i as u8), U256::from(*value as u64));
        }

        let after_root = tree.root();
        prop_assert!(initial_root != after_root);
    }

    #[test]
    fn set_then_get_returns_value(value in any::<u64>()) {
    let mut tree = QuantumTree::<NistSisScheme>::new();
        tree.set(&addr(9), &key(9), U256::from(value));
        let fetched = tree.get(&addr(9), &key(9)).unwrap();
        prop_assert_eq!(fetched, U256::from(value));
    }

    #[test]
    fn set_then_delete_removes_key(value in any::<u64>()) {
    let mut tree = QuantumTree::<NistSisScheme>::new();
        tree.set(&addr(2), &key(2), U256::from(value));
        tree.delete(&addr(2), &key(2));
        prop_assert!(tree.get(&addr(2), &key(2)).is_err());
    }
}
