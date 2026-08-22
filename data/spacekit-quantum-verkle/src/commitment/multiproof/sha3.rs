use sha3::{Digest, Sha3_256};
use std::collections::HashMap;

const WIDTH: usize = 256;

/// Implementation of the Sha3_256QuantumTree struct
#[derive(Debug, Clone)]
pub struct Sha3_256QuantumTree {
    pub nodes: HashMap<[u8; 1], Vec<u8>>,
    pub root: Vec<u8>,
}

/// Implementation of the Sha3_256QuantumTree struct
impl Sha3_256QuantumTree {
    /// Create a new Sha3_256QuantumTree
    pub fn new() -> Self {
        Sha3_256QuantumTree {
            nodes: HashMap::new(),
            root: vec![0; 32],
        }
    }

    /// Insert a key-value pair into the tree
    pub fn insert(&mut self, key: u8, value: &[u8]) {
        let mut hasher = Sha3_256::new();
        hasher.update(&[key]);
        hasher.update(value);
        let hash = hasher.finalize().to_vec();
        self.nodes.insert([key], hash);
        self.update_root();
    }

    /// Get the value for a given key
    pub fn get(&self, key: u8) -> Option<&Vec<u8>> {
        self.nodes.get(&[key])
    }

    /// Update the root hash of the tree
    pub fn update_root(&mut self) {
        let mut hasher = Sha3_256::new();
        for i in 0..WIDTH {
            if let Some(node) = self.nodes.get(&[i as u8]) {
                hasher.update(node);
            } else {
                hasher.update(&[0u8; 32]);
            }
        }
        self.root = hasher.finalize().to_vec();
    }

    /// Create a proof for a given key
    pub fn create_proof(&self, key: u8) -> Vec<Vec<u8>> {
        let mut proof = Vec::with_capacity(WIDTH - 1);

        // Add nodes in the same order as root calculation
        for i in 0..WIDTH {
            if i as u8 != key {
                let node = if let Some(n) = self.nodes.get(&[i as u8]) {
                    n.clone()
                } else {
                    vec![0u8; 32]
                };
                proof.push(node);
            }
        }

        proof
    }

    /// Verify a proof for a given key
    pub fn verify_proof(&self, key: u8, value: &[u8], proof: &[Vec<u8>]) -> bool {
        if proof.len() != WIDTH - 1 {
            return false;
        }

        // Calculate the leaf hash for the given key and value
        let mut hasher = Sha3_256::new();
        hasher.update(&[key]);
        hasher.update(value);
        let leaf_hash = hasher.finalize().to_vec();

        // Build root hash using the proof
        let mut root_hasher = Sha3_256::new();
        let mut proof_idx = 0;

        for i in 0..WIDTH {
            if i as u8 == key {
                root_hasher.update(&leaf_hash);
            } else {
                root_hasher.update(&proof[proof_idx]);
                proof_idx += 1;
            }
        }

        let computed_root = root_hasher.finalize().to_vec();
        computed_root == self.root
    }

    /// Get the root hash of the tree
    #[cfg(test)]
    fn get_root_calculation(&self) -> Vec<Vec<u8>> {
        let mut nodes = Vec::with_capacity(WIDTH);
        for i in 0..WIDTH {
            if let Some(node) = self.nodes.get(&[i as u8]) {
                nodes.push(node.clone());
            } else {
                nodes.push(vec![0u8; 32]);
            }
        }
        nodes
    }
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
        let tree = Sha3_256QuantumTree::new();
        assert!(tree.nodes.is_empty());
        assert_eq!(tree.root, vec![0; 32]);
    }

    #[test]
    fn test_single_insert() {
        let mut tree = Sha3_256QuantumTree::new();
        tree.insert(1, b"Hello");

        assert!(tree.nodes.contains_key(&[1]));
        assert_ne!(tree.root, vec![0; 32]);
    }

    #[test]
    fn test_multiple_inserts() {
        let mut tree = Sha3_256QuantumTree::new();
        tree.insert(1, b"Hello");
        tree.insert(2, b"World");

        assert!(tree.nodes.contains_key(&[1]));
        assert!(tree.nodes.contains_key(&[2]));
        assert_ne!(tree.root, vec![0; 32]);
    }

    #[test]
    fn test_proof_verification() {
        let mut tree = Sha3_256QuantumTree::new();
        tree.insert(1, b"Hello");
        tree.insert(2, b"World");

        let proof = tree.create_proof(1);
        assert!(tree.verify_proof(1, b"Hello", &proof));
        assert!(!tree.verify_proof(1, b"Wrong", &proof));
    }

    #[test]
    fn test_concurrent_modifications() {
        let tree = Arc::new(Mutex::new(Sha3_256QuantumTree::new()));
        let mut handles = vec![];

        for i in 0..10 {
            let tree_clone = Arc::clone(&tree);
            let handle = thread::spawn(move || {
                let mut tree = tree_clone.lock().unwrap();
                tree.insert(i as u8, b"Test");
            });
            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }

        let final_tree = tree.lock().unwrap();
        assert_eq!(final_tree.nodes.len(), 10);
    }

    #[test]
    fn test_performance_large_insertions() {
        let mut tree = Sha3_256QuantumTree::new();
        let start = Instant::now();

        for i in 0..1000 {
            tree.insert(i as u8 % 255, &[i as u8; 1024]);
        }

        let duration = start.elapsed();
        println!("Time taken for 1000 insertions: {:?}", duration);
    }

    #[test]
    fn test_proof_verification_detailed() {
        let mut tree = Sha3_256QuantumTree::new();
        tree.insert(1, b"Hello");
        tree.insert(2, b"World");

        // Get proof for key 1
        let proof = tree.create_proof(1);

        // Verify proof length
        assert_eq!(
            proof.len(),
            WIDTH - 1,
            "Proof should contain WIDTH-1 elements"
        );

        // Get the actual root calculation order
        let root_nodes = tree.get_root_calculation();

        // Verify proof elements match root calculation (excluding the proven element)
        let mut proof_idx = 0;
        for i in 0..WIDTH {
            if i as u8 != 1 {
                // Skip the proven key
                assert_eq!(
                    proof[proof_idx], root_nodes[i],
                    "Proof element {} doesn't match root calculation at position {}",
                    proof_idx, i
                );
                proof_idx += 1;
            }
        }

        // Final verification
        assert!(
            tree.verify_proof(1, b"Hello", &proof),
            "Proof verification failed for valid proof"
        );
        assert!(
            !tree.verify_proof(1, b"Wrong", &proof),
            "Proof verification succeeded for invalid value"
        );
    }
}
