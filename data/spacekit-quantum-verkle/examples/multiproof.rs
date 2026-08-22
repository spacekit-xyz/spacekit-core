use spacekit_quantum_verkle::commitment::multiproof::optimized::OptimizedMultiProof;
use spacekit_quantum_verkle::commitment::multiproof::sha3::Sha3_256QuantumTree;

fn main() {
    let mut tree = Sha3_256QuantumTree::new();
    tree.insert(1, b"Hello");
    tree.insert(2, b"World");
    tree.insert(3, b"Test");

    let keys = vec![1, 2, 3];
    let mut multiproof = OptimizedMultiProof::new(&tree, &keys);
    println!("Multiproof: {:?}", multiproof);

    multiproof.optimize();
    println!("Optimized Multiproof: {:?}", multiproof);

    // Verify the multiproof
    let is_valid = multiproof.verify(&tree.root, &[b"Hello", b"World", b"Test"]);
    println!("Is valid: {}", is_valid);

    println!("Hello, world!");
}
