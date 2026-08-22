//! Verkle State Anchor — computes a quantum-resistant Verkle root over
//! all DID registry documents and provides helpers for anchoring the root
//! to the `SpacekitStateAnchor` EVM contract.
//!
//! The Verkle tree uses the `NistSisScheme` (SIS-128 / WeeWu) commitment
//! scheme from `spacekit-quantum-verkle-tree`, giving post-quantum security
//! for inclusion proofs while keeping proof sizes small.

use alloy_primitives::{Address, B256, U256};
use sha2::{Digest, Sha256};
use spacekit_quantum_verkle::{NistSisScheme, QuantumProof, QuantumTree};

/// The fixed "contract address" slot used for DID documents inside the
/// Verkle tree.  Matches `system_contracts::DID_REGISTRY` in genesis.
const DID_REGISTRY_ADDRESS: [u8; 20] = {
    let mut addr = [0u8; 20];
    addr[19] = 0x02;
    addr
};

/// An epoch anchor ready for submission to the EVM contract.
#[derive(Debug, Clone)]
pub struct EpochAnchor {
    pub epoch: u64,
    pub verkle_root: [u8; 32],
    pub document_count: usize,
}

/// Builds a Verkle tree over the supplied DID documents and returns the root.
///
/// Each `(did_string, doc_bytes)` pair is inserted into the tree at:
///   address = `DID_REGISTRY_ADDRESS`
///   key     = `SHA-256(did_string)`  (first 32 bytes)
///   value   = `SHA-256(doc_bytes)`   (as U256)
///   aux     = `doc_bytes`
///
/// The value is a hash of the document so the tree commitment covers the
/// content without embedding the full blob in the U256 slot.
pub fn compute_verkle_root(documents: &[(&str, &[u8])]) -> (QuantumTree<NistSisScheme>, [u8; 32]) {
    let mut tree = QuantumTree::<NistSisScheme>::new();
    let address = Address::from(DID_REGISTRY_ADDRESS);

    for (did, doc_bytes) in documents {
        let key = did_to_key(did);
        let value = doc_hash_to_u256(doc_bytes);
        tree.set_with_aux(&address, &key, value, doc_bytes);
    }

    let root: B256 = tree.root();
    (tree, root.0)
}

/// Create an inclusion proof for a single DID in the tree.
pub fn create_did_proof(
    tree: &QuantumTree<NistSisScheme>,
    did: &str,
) -> Result<
    QuantumProof<
        <NistSisScheme as spacekit_quantum_verkle::commitment::schemes::CommitmentScheme>::Proof,
    >,
    spacekit_quantum_verkle::VerkleError,
> {
    let address = Address::from(DID_REGISTRY_ADDRESS);
    let key = did_to_key(did);
    tree.create_proof(&address, &key)
}

/// Build the epoch anchor from a set of DID documents.
pub fn build_epoch_anchor(
    epoch: u64,
    documents: &[(&str, &[u8])],
) -> (QuantumTree<NistSisScheme>, EpochAnchor) {
    let (tree, root) = compute_verkle_root(documents);
    let anchor = EpochAnchor {
        epoch,
        verkle_root: root,
        document_count: documents.len(),
    };
    (tree, anchor)
}

/// Encode the `updateRoot(uint256 epoch, bytes32 root)` calldata for
/// the `SpacekitStateAnchor` contract.
///
/// Selector: `keccak256("updateRoot(uint256,bytes32)")[:4]`
pub fn encode_update_root_calldata(epoch: u64, root: &[u8; 32]) -> Vec<u8> {
    use sha3::Keccak256 as K;
    let selector = {
        let mut hasher = <K as sha3::Digest>::new();
        sha3::Digest::update(&mut hasher, b"updateRoot(uint256,bytes32)");
        let hash: [u8; 32] = sha3::Digest::finalize(hasher).into();
        [hash[0], hash[1], hash[2], hash[3]]
    };

    let mut calldata = Vec::with_capacity(4 + 32 + 32);
    calldata.extend_from_slice(&selector);

    // epoch as uint256 (big-endian, left-padded)
    let mut epoch_bytes = [0u8; 32];
    epoch_bytes[24..32].copy_from_slice(&epoch.to_be_bytes());
    calldata.extend_from_slice(&epoch_bytes);

    // root as bytes32
    calldata.extend_from_slice(root);

    calldata
}

fn did_to_key(did: &str) -> B256 {
    let hash = Sha256::digest(did.as_bytes());
    B256::from_slice(&hash)
}

fn doc_hash_to_u256(doc_bytes: &[u8]) -> U256 {
    let hash = Sha256::digest(doc_bytes);
    U256::from_be_bytes::<32>(hash.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verkle_root_deterministic() {
        let docs = vec![
            ("did:spacekit:testnet:aabbccdd", b"doc1" as &[u8]),
            ("did:spacekit:testnet:11223344", b"doc2" as &[u8]),
        ];
        let (_, root1) = compute_verkle_root(&docs);
        let (_, root2) = compute_verkle_root(&docs);
        assert_eq!(root1, root2, "same inputs must produce same root");
    }

    #[test]
    fn verkle_root_changes_on_mutation() {
        let docs_a = vec![("did:spacekit:testnet:aabbccdd", b"doc1" as &[u8])];
        let docs_b = vec![("did:spacekit:testnet:aabbccdd", b"doc2" as &[u8])];
        let (_, root_a) = compute_verkle_root(&docs_a);
        let (_, root_b) = compute_verkle_root(&docs_b);
        assert_ne!(root_a, root_b, "different data must produce different root");
    }

    #[test]
    fn calldata_encoding_length() {
        let root = [0xABu8; 32];
        let calldata = encode_update_root_calldata(42, &root);
        assert_eq!(calldata.len(), 4 + 32 + 32);
    }

    #[test]
    fn epoch_anchor_round_trip() {
        let docs = vec![("did:spacekit:mainnet:deadbeef", b"mainnet-doc" as &[u8])];
        let (tree, anchor) = build_epoch_anchor(7, &docs);
        assert_eq!(anchor.epoch, 7);
        assert_eq!(anchor.document_count, 1);
        assert_ne!(anchor.verkle_root, [0u8; 32]);

        let proof = create_did_proof(&tree, "did:spacekit:mainnet:deadbeef");
        assert!(proof.is_ok());
    }
}
