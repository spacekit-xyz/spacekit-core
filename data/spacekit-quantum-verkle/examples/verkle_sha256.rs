// verkle tree implementation using sha256
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const WIDTH: usize = 256;

#[derive(Debug)]
pub struct Sha256QuantumTree {
    pub nodes: HashMap<[u8; 1], Vec<u8>>,
    pub root: Vec<u8>,
}

impl Sha256QuantumTree {
    pub fn new() -> Self {
        Sha256QuantumTree {
            nodes: HashMap::new(),
            root: vec![0; 32],
        }
    }

    pub fn insert(&mut self, key: u8, value: &[u8]) {
        let mut hasher = Sha256::new();
        hasher.update(&[key]);
        hasher.update(value);
        let leaf = hasher.finalize().to_vec();

        self.nodes.insert([key], leaf);
        self.update_root();
    }

    pub fn update_root(&mut self) {
        let mut hasher = Sha256::new();
        for i in 0..WIDTH {
            if let Some(node) = self.nodes.get(&[i as u8]) {
                hasher.update(node);
            } else {
                hasher.update(&[0u8; 32]);
            }
        }
        self.root = hasher.finalize().to_vec();
    }

    pub fn generate_proof(&self, key: u8) -> Vec<Vec<u8>> {
        let mut proof = Vec::new();
        for i in 0..WIDTH {
            if i as u8 != key {
                if let Some(node) = self.nodes.get(&[i as u8]) {
                    proof.push(node.clone());
                } else {
                    proof.push(vec![0u8; 32]);
                }
            } else {
                // Include the actual node for the key in the proof
                if let Some(node) = self.nodes.get(&[key]) {
                    proof.push(node.clone());
                }
            }
        }
        proof
    }

    pub fn verify_proof(&self, key: u8, value: &[u8], proof: &[Vec<u8>]) -> bool {
        // Calculate the leaf hash for the given key and value
        let mut hasher = Sha256::new();
        hasher.update(&[key]);
        hasher.update(value);
        let leaf_hash = hasher.finalize().to_vec();

        // Build root hash using the proof, ensuring we use our computed leaf hash
        let mut root_hasher = Sha256::new();
        for (i, node) in proof.iter().enumerate() {
            if i as u8 == key {
                root_hasher.update(&leaf_hash);
            } else {
                root_hasher.update(node);
            }
        }
        let computed_root = root_hasher.finalize().to_vec();

        println!("Computed root: {:?}", computed_root);
        println!("Actual root:   {:?}", self.root);

        computed_root == self.root
    }
}

fn main() {
    let mut tree = Sha256QuantumTree::new();
    tree.insert(1, b"Hello");
    tree.insert(2, b"World");
    let proof = tree.generate_proof(1);
    let is_valid = tree.verify_proof(1, b"Hello", &proof);
    println!("Proof is valid: {}", is_valid);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn test_new_tree() {
        let tree = Sha256QuantumTree::new();
        assert!(tree.nodes.is_empty());
        assert_eq!(tree.root, vec![0; 32]);
    }

    #[test]
    fn test_single_insert() {
        let mut tree = Sha256QuantumTree::new();
        tree.insert(1, b"Hello");

        assert_eq!(tree.nodes.len(), 1);
        assert!(tree.nodes.contains_key(&[1]));
    }

    #[test]
    fn test_multiple_inserts() {
        let mut tree = Sha256QuantumTree::new();
        tree.insert(1, b"Hello");
        tree.insert(2, b"World");
        tree.insert(3, b"!");

        assert_eq!(tree.nodes.len(), 3);
        assert!(tree.nodes.contains_key(&[1]));
        assert!(tree.nodes.contains_key(&[2]));
        assert!(tree.nodes.contains_key(&[3]));
    }

    #[test]
    fn test_proof_verification() {
        let mut tree = Sha256QuantumTree::new();
        tree.insert(1, b"Hello");
        tree.insert(2, b"World");

        // Test valid proof
        let proof = tree.generate_proof(1);
        assert!(tree.verify_proof(1, b"Hello", &proof));

        // Test invalid value
        assert!(!tree.verify_proof(1, b"Invalid", &proof));

        // Test invalid key
        assert!(!tree.verify_proof(3, b"Hello", &proof));
    }

    #[test]
    fn test_proof_length() {
        let mut tree = Sha256QuantumTree::new();
        tree.insert(1, b"Hello");

        let proof = tree.generate_proof(1);
        assert_eq!(proof.len(), WIDTH);
    }

    #[test]
    fn test_overwrite_value() {
        let mut tree = Sha256QuantumTree::new();
        tree.insert(1, b"Hello");
        let original_root = tree.root.clone();

        // Overwrite same key with new value
        tree.insert(1, b"NewValue");
        assert_ne!(tree.root, original_root);

        // Verify new value
        let proof = tree.generate_proof(1);
        assert!(tree.verify_proof(1, b"NewValue", &proof));
        assert!(!tree.verify_proof(1, b"Hello", &proof));
    }

    #[test]
    fn test_large_values() {
        let mut tree = Sha256QuantumTree::new();

        // Create a 1MB value
        let large_value = vec![0x42; 1_000_000];
        tree.insert(1, &large_value);

        let proof = tree.generate_proof(1);
        assert!(tree.verify_proof(1, &large_value, &proof));

        // Create a 10MB value
        let very_large_value = vec![0x42; 10_000_000];
        tree.insert(2, &very_large_value);

        let proof = tree.generate_proof(2);
        assert!(tree.verify_proof(2, &very_large_value, &proof));
    }

    #[test]
    fn test_edge_case_keys() {
        let mut tree = Sha256QuantumTree::new();

        // Test minimum key (0)
        tree.insert(0, b"Min key");
        let proof = tree.generate_proof(0);
        assert!(tree.verify_proof(0, b"Min key", &proof));

        // Test maximum key (255)
        tree.insert(255, b"Max key");
        let proof = tree.generate_proof(255);
        assert!(tree.verify_proof(255, b"Max key", &proof));

        // Verify both keys coexist correctly
        assert_eq!(tree.nodes.len(), 2);
        assert!(tree.nodes.contains_key(&[0]));
        assert!(tree.nodes.contains_key(&[255]));
    }

    #[test]
    fn test_empty_values() {
        let mut tree = Sha256QuantumTree::new();

        // Test empty value
        tree.insert(1, b"");
        let proof = tree.generate_proof(1);
        assert!(tree.verify_proof(1, b"", &proof));

        // Test single byte value
        tree.insert(2, b"x");
        let proof = tree.generate_proof(2);
        assert!(tree.verify_proof(2, b"x", &proof));
    }

    #[test]
    fn test_concurrent_modifications() {
        let tree = Arc::new(Mutex::new(Sha256QuantumTree::new()));
        let mut handles = vec![];

        // Spawn 10 threads that each insert 10 values
        for i in 0..10 {
            let tree_clone = Arc::clone(&tree);
            let handle = thread::spawn(move || {
                let base_key = (i * 10) as u8;
                for j in 0..10 {
                    let mut tree = tree_clone.lock().unwrap();
                    tree.insert(base_key + j as u8, format!("Value-{}-{}", i, j).as_bytes());
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all insertions
        let final_tree = tree.lock().unwrap();
        assert_eq!(final_tree.nodes.len(), 100);

        // Verify some random keys
        // Key 5 is in the first thread's range (i=0)
        let proof = final_tree.generate_proof(5);
        assert!(final_tree.verify_proof(5, b"Value-0-5", &proof));

        // Key 95 is in the last thread's range (i=9, j=5)
        let proof = final_tree.generate_proof(95);
        assert!(final_tree.verify_proof(95, b"Value-9-5", &proof));
    }

    #[test]
    fn test_performance_large_insertions() {
        let mut tree = Sha256QuantumTree::new();
        let start = Instant::now();

        // Insert 1000 values
        for i in 0..100 {
            let value = format!("Value-{}", i);
            tree.insert(i as u8, value.as_bytes());
        }

        let duration = start.elapsed();
        println!("Time to insert 100 values: {:?}", duration);

        // Measure proof generation time
        let start = Instant::now();
        let proof = tree.generate_proof(50);
        let duration = start.elapsed();
        println!("Time to generate proof: {:?}", duration);

        // Measure verification time
        let start = Instant::now();
        assert!(tree.verify_proof(50, b"Value-50", &proof));
        let duration = start.elapsed();
        println!("Time to verify proof: {:?}", duration);
    }
}
