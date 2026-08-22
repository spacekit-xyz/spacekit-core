use alloy_primitives::{Address, B256, U256};
use spacekit_quantum_verkle::{new_quantum_tree, VerkleError};

fn main() -> Result<(), VerkleError> {
    // Create a new PQ-focused tree
    let mut tree = new_quantum_tree();

    // Insert key-value data
    let address = Address::new([1; 20]);
    let key = B256::new([1; 32]);
    let value = U256::from(7);

    // Set tree with the address, key, and value
    tree.set(&address, &key, value); // set the value to 7

    // Retrieve and verify the value
    let data = tree.get(&address, &key)?;
    println!("Retrieved data: {:?}", data);
    assert_eq!(data, value); // assert the value is 7

    Ok(())
}
