use crate::commitment::multiproof::sha3::Sha3_256QuantumTree;
/// Add optimized multiproof implementation
use sha3::{Digest, Sha3_256};

const WIDTH: usize = 256;

/// Optimized multiproof implementation
#[derive(Debug)]
pub struct OptimizedMultiProof {
    proofs: Vec<Vec<Vec<u8>>>,          // Vector of proofs for each key
    values: Vec<Vec<u8>>,               // Values for each key
    keys: Vec<u8>,                      // Keys being proven
    root: Vec<u8>,                      // Root hash
    common_nodes: Vec<Option<Vec<u8>>>, // Nodes common to all proofs (None for non-common nodes)
}

/// Implementation of the OptimizedMultiProof struct
impl OptimizedMultiProof {
    /// Create a new OptimizedMultiProof
    pub fn new(tree: &Sha3_256QuantumTree, keys: &[u8]) -> Self {
        let mut unique_keys: Vec<u8> = keys.to_vec();
        unique_keys.sort_unstable();
        unique_keys.dedup();

        let mut proofs = Vec::with_capacity(unique_keys.len());
        let mut values = Vec::with_capacity(unique_keys.len());

        for &key in &unique_keys {
            if let Some(value) = tree.get(key) {
                proofs.push(tree.create_proof(key));
                values.push(value.clone());
            }
        }

        OptimizedMultiProof {
            proofs,
            values,
            keys: unique_keys,
            root: tree.root.clone(),
            common_nodes: vec![None; WIDTH],
        }
    }

    /// Verify the multiproof
    pub fn verify(&self, root: &[u8], values: &[&[u8]]) -> bool {
        if values.len() != self.keys.len() {
            return false;
        }

        for ((&key, proof), &value) in self.keys.iter().zip(self.proofs.iter()).zip(values.iter()) {
            let mut hasher = Sha3_256::new();
            hasher.update(&[key]);
            hasher.update(value);
            let leaf_hash = hasher.finalize().to_vec();

            let mut root_hasher = Sha3_256::new();
            let mut proof_idx = 0;
            let mut node_idx = 0;

            while node_idx < WIDTH {
                if node_idx as u8 == key {
                    root_hasher.update(&leaf_hash);
                } else if let Some(ref common_node) = self.common_nodes[node_idx] {
                    root_hasher.update(common_node);
                } else {
                    if proof_idx < proof.len() {
                        root_hasher.update(&proof[proof_idx]);
                        proof_idx += 1;
                    } else {
                        root_hasher.update(&vec![0u8; 32]);
                    }
                }
                node_idx += 1;
            }

            let computed_root = root_hasher.finalize().to_vec();
            if computed_root != root {
                return false;
            }
        }

        true
    }

    /// Get the root hash of the multiproof
    pub fn get_root(&self) -> &[u8] {
        &self.root
    }

    /// Get the keys being proven
    pub fn get_keys(&self) -> &[u8] {
        &self.keys
    }

    /// Get the values being proven
    pub fn get_values(&self) -> &[Vec<u8>] {
        &self.values
    }

    /// Get the proof for a given key
    pub fn get_proof_for_key(&self, key: u8) -> Option<&Vec<Vec<u8>>> {
        self.keys
            .iter()
            .position(|&k| k == key)
            .map(|idx| &self.proofs[idx])
    }

    /// Optimize proof size by combining overlapping proofs
    pub fn optimize(&mut self) {
        if self.proofs.is_empty() {
            return;
        }

        // Reset common nodes
        self.common_nodes = vec![None; WIDTH];

        // For each position in WIDTH
        for i in 0..WIDTH {
            // Skip positions that correspond to proven keys
            if self.keys.contains(&(i as u8)) {
                continue;
            }

            // Check if all proofs have the same node at this position
            let mut common_node = None;
            let mut is_common = true;

            'proof_loop: for proof in &self.proofs {
                let current_node = if i < proof.len() {
                    Some(&proof[i])
                } else {
                    None
                };

                match (common_node, current_node) {
                    (None, Some(node)) => common_node = Some(node),
                    (Some(existing), Some(node)) if existing == node => {}
                    _ => {
                        is_common = false;
                        break 'proof_loop;
                    }
                }
            }

            if is_common {
                if let Some(node) = common_node {
                    self.common_nodes[i] = Some(node.clone());
                }
                // Remove this node from all proofs
                for proof in &mut self.proofs {
                    if i < proof.len() {
                        proof.remove(i);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multiproof_creation() {
        let mut tree = Sha3_256QuantumTree::new();
        tree.insert(1, b"Hello");
        tree.insert(2, b"World");

        let keys = vec![1, 2];
        let multiproof = OptimizedMultiProof::new(&tree, &keys);

        assert_eq!(multiproof.get_keys().len(), 2);
        assert_eq!(multiproof.get_values().len(), 2);
    }

    #[test]
    fn test_multiproof_verification() {
        let mut tree = Sha3_256QuantumTree::new();
        tree.insert(1, b"Hello");
        tree.insert(2, b"World");

        let keys = vec![1, 2];
        let multiproof = OptimizedMultiProof::new(&tree, &keys);

        assert!(multiproof.verify(&tree.root, &[b"Hello", b"World"]));

        // Test with wrong values
        assert!(!multiproof.verify(&tree.root, &[b"Wrong", b"Values"]));
    }

    #[test]
    fn test_multiproof_optimization() {
        let mut tree = Sha3_256QuantumTree::new();
        tree.insert(1, b"Hello");
        tree.insert(2, b"World");
        tree.insert(3, b"Test");

        let keys = vec![1, 2, 3];
        let mut multiproof = OptimizedMultiProof::new(&tree, &keys);

        // Record size before optimization
        let size_before = multiproof.proofs.iter().map(|p| p.len()).sum::<usize>();

        multiproof.optimize();

        // Record size after optimization
        let size_after = multiproof.proofs.iter().map(|p| p.len()).sum::<usize>();

        // Optimization should reduce or maintain size
        assert!(size_after <= size_before);

        // Verify still works after optimization
        assert!(multiproof.verify(&tree.root, &[b"Hello", b"World", b"Test"]));
    }
}
