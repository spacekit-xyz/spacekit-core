use alloc::vec::Vec;
use alloy_primitives::{utils::Keccak256, Address, B256, U256};
use core::cmp::Ordering;
use foldhash::fast::RandomState;
use hashbrown::HashMap;

use crate::alloc::string::ToString;
use crate::commitment::errors::VerkleError;
use crate::commitment::schemes::CommitmentScheme;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

type FastHashMap<K, V> = HashMap<K, V, RandomState>;

/// Scheme-agnostic quantum tree for PQ commitments.
pub struct QuantumTree<S: CommitmentScheme> {
    commitments: FastHashMap<(Address, B256), S::Commitment>,
    values: FastHashMap<(Address, B256), U256>,
    aux: FastHashMap<(Address, B256), Vec<u8>>,
    root: B256,
}

pub struct QuantumProof<P> {
    pub root: B256,
    pub address: Address,
    pub key: B256,
    pub proof: P,
}

pub struct QuantumRangeProof<P> {
    pub root: B256,
    pub start: B256,
    pub end: B256,
    pub proofs: Vec<QuantumProof<P>>,
}

pub struct QuantumMultiProof<P> {
    pub root: B256,
    pub keys: Vec<(Address, B256)>,
    pub indices: Vec<u32>,
    pub commitment_bytes: Vec<u8>,
    pub proof: P,
}

#[cfg(feature = "serde")]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuantumTreeSnapshot {
    pub entries: Vec<(Address, B256, U256)>,
    pub aux_entries: Vec<(Address, B256, Vec<u8>)>,
}

impl<P> QuantumProof<P> {
    pub fn to_bytes<S: CommitmentScheme<Proof = P>>(&self) -> Vec<u8> {
        let proof_bytes = S::proof_bytes(&self.proof);
        let mut out = Vec::with_capacity(64 + proof_bytes.len());
        write_header::<S>(&mut out);
        out.extend_from_slice(self.root.as_slice());
        out.extend_from_slice(self.address.as_slice());
        out.extend_from_slice(self.key.as_slice());
        out.extend_from_slice(&(proof_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&proof_bytes);
        out
    }

    pub fn from_bytes<S: CommitmentScheme<Proof = P>>(bytes: &[u8]) -> Result<Self, VerkleError> {
        let mut cursor = 0usize;
        read_header::<S>(bytes, &mut cursor)?;
        let root = read_b256(bytes, &mut cursor)?;
        let address = read_address(bytes, &mut cursor)?;
        let key = read_b256(bytes, &mut cursor)?;
        let proof_len = read_u32(bytes, &mut cursor)? as usize;
        let proof_bytes = read_bytes(bytes, &mut cursor, proof_len)?;
        let proof =
            S::proof_from_bytes(&proof_bytes).map_err(|_| VerkleError::SerializationError)?;
        Ok(Self {
            root,
            address,
            key,
            proof,
        })
    }
}

impl<P> QuantumRangeProof<P> {
    pub fn to_bytes<S: CommitmentScheme<Proof = P>>(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_header::<S>(&mut out);
        out.extend_from_slice(self.root.as_slice());
        out.extend_from_slice(self.start.as_slice());
        out.extend_from_slice(self.end.as_slice());
        out.extend_from_slice(&(self.proofs.len() as u32).to_be_bytes());
        for proof in &self.proofs {
            let proof_bytes = proof.to_bytes::<S>();
            out.extend_from_slice(&(proof_bytes.len() as u32).to_be_bytes());
            out.extend_from_slice(&proof_bytes);
        }
        out
    }

    pub fn from_bytes<S: CommitmentScheme<Proof = P>>(bytes: &[u8]) -> Result<Self, VerkleError> {
        let mut cursor = 0usize;
        read_header::<S>(bytes, &mut cursor)?;
        let root = read_b256(bytes, &mut cursor)?;
        let start = read_b256(bytes, &mut cursor)?;
        let end = read_b256(bytes, &mut cursor)?;
        let count = read_u32(bytes, &mut cursor)? as usize;
        let mut proofs = Vec::with_capacity(count);
        for _ in 0..count {
            let proof_len = read_u32(bytes, &mut cursor)? as usize;
            let proof_bytes = read_bytes(bytes, &mut cursor, proof_len)?;
            let proof = QuantumProof::from_bytes::<S>(&proof_bytes)?;
            proofs.push(proof);
        }
        Ok(Self {
            root,
            start,
            end,
            proofs,
        })
    }
}

impl<P> QuantumMultiProof<P> {
    pub fn to_bytes<S: CommitmentScheme<MultiProof = P>>(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_header::<S>(&mut out);
        out.extend_from_slice(self.root.as_slice());
        out.extend_from_slice(&(self.keys.len() as u32).to_be_bytes());
        for (address, key) in &self.keys {
            out.extend_from_slice(address.as_slice());
            out.extend_from_slice(key.as_slice());
        }
        out.extend_from_slice(&(self.indices.len() as u32).to_be_bytes());
        for index in &self.indices {
            out.extend_from_slice(&index.to_be_bytes());
        }
        out.extend_from_slice(&(self.commitment_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.commitment_bytes);
        let proof_bytes = S::multi_proof_bytes(&self.proof);
        out.extend_from_slice(&(proof_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&proof_bytes);
        out
    }

    pub fn from_bytes<S: CommitmentScheme<MultiProof = P>>(
        bytes: &[u8],
    ) -> Result<Self, VerkleError> {
        let mut cursor = 0usize;
        read_header::<S>(bytes, &mut cursor)?;
        let root = read_b256(bytes, &mut cursor)?;
        let key_count = read_u32(bytes, &mut cursor)? as usize;
        let mut keys = Vec::with_capacity(key_count);
        for _ in 0..key_count {
            let address = read_address(bytes, &mut cursor)?;
            let key = read_b256(bytes, &mut cursor)?;
            keys.push((address, key));
        }
        let index_count = read_u32(bytes, &mut cursor)? as usize;
        let mut indices = Vec::with_capacity(index_count);
        for _ in 0..index_count {
            indices.push(read_u32(bytes, &mut cursor)?);
        }
        let commitment_len = read_u32(bytes, &mut cursor)? as usize;
        let commitment_bytes = read_bytes(bytes, &mut cursor, commitment_len)?;
        let proof_len = read_u32(bytes, &mut cursor)? as usize;
        let proof_bytes = read_bytes(bytes, &mut cursor, proof_len)?;
        let proof =
            S::multi_proof_from_bytes(&proof_bytes).map_err(|_| VerkleError::SerializationError)?;
        Ok(Self {
            root,
            keys,
            indices,
            commitment_bytes,
            proof,
        })
    }
}

impl<S: CommitmentScheme> QuantumTree<S> {
    pub fn new() -> Self {
        QuantumTree {
            commitments: FastHashMap::default(),
            values: FastHashMap::default(),
            aux: FastHashMap::default(),
            root: B256::ZERO,
        }
    }

    pub fn root(&self) -> B256 {
        self.root
    }

    pub fn set(&mut self, address: &Address, key: &B256, value: U256) {
        let value_bytes = u256_to_bytes(value);
        let (commitment, aux) = S::commit_with_aux(&[value_bytes.clone()], None);
        self.commitments.insert((*address, *key), commitment);
        self.values.insert((*address, *key), value);
        if let Some(aux_bytes) = aux {
            self.aux.insert((*address, *key), aux_bytes);
        } else {
            self.aux.remove(&(*address, *key));
        }
        self.update_root();
    }

    pub fn set_with_aux(&mut self, address: &Address, key: &B256, value: U256, aux: &[u8]) {
        let value_bytes = u256_to_bytes(value);
        let (commitment, aux_bytes) = S::commit_with_aux(&[value_bytes.clone()], Some(aux));
        self.commitments.insert((*address, *key), commitment);
        self.values.insert((*address, *key), value);
        if let Some(stored) = aux_bytes {
            self.aux.insert((*address, *key), stored);
        } else {
            self.aux.remove(&(*address, *key));
        }
        self.update_root();
    }

    pub fn get(&self, address: &Address, key: &B256) -> Result<U256, VerkleError> {
        self.values
            .get(&(*address, *key))
            .copied()
            .ok_or(VerkleError::KeyNotFound)
    }

    pub fn delete(&mut self, address: &Address, key: &B256) {
        self.values.remove(&(*address, *key));
        self.commitments.remove(&(*address, *key));
        self.aux.remove(&(*address, *key));
        self.update_root();
    }

    pub fn create_proof(
        &self,
        address: &Address,
        key: &B256,
    ) -> Result<QuantumProof<S::Proof>, VerkleError> {
        let value = self
            .values
            .get(&(*address, *key))
            .ok_or(VerkleError::KeyNotFound)?;
        let value_bytes = u256_to_bytes(*value);
        let aux = self
            .aux
            .get(&(*address, *key))
            .map(|bytes| bytes.as_slice());
        let proof = S::open_with_aux(&[value_bytes], 0, aux)
            .map_err(|e| VerkleError::ProofGenerationFailed(e))?;
        Ok(QuantumProof {
            root: self.root,
            address: *address,
            key: *key,
            proof,
        })
    }

    pub fn verify_proof(
        &self,
        proof: &QuantumProof<S::Proof>,
        address: &Address,
        key: &B256,
        value: U256,
    ) -> bool {
        if proof.address != *address || proof.key != *key || proof.root != self.root {
            return false;
        }
        let commitment = match self.commitments.get(&(*address, *key)) {
            Some(c) => c,
            None => return false,
        };
        let value_bytes = u256_to_bytes(value);
        S::verify(commitment, 0, &value_bytes, &proof.proof)
    }

    pub fn create_range_proof(
        &self,
        start: &B256,
        end: &B256,
    ) -> Result<QuantumRangeProof<S::Proof>, VerkleError> {
        if start.as_slice() > end.as_slice() {
            return Err(VerkleError::InvalidProofOptions(
                "start must be <= end".to_string(),
            ));
        }
        let mut keys: Vec<(Address, B256)> = self.values.keys().copied().collect();
        keys.sort_by(|(addr_a, key_a), (addr_b, key_b)| {
            let key_cmp = key_a.as_slice().cmp(key_b.as_slice());
            if key_cmp == Ordering::Equal {
                addr_a.as_slice().cmp(addr_b.as_slice())
            } else {
                key_cmp
            }
        });

        let mut proofs = Vec::new();
        for (address, key) in keys {
            if key.as_slice() >= start.as_slice() && key.as_slice() <= end.as_slice() {
                proofs.push(self.create_proof(&address, &key)?);
            }
        }

        Ok(QuantumRangeProof {
            root: self.root,
            start: *start,
            end: *end,
            proofs,
        })
    }

    pub fn verify_range_proof(
        &self,
        proof: &QuantumRangeProof<S::Proof>,
        values: Vec<(Address, B256, U256)>,
    ) -> bool {
        if proof.root != self.root || proof.start.as_slice() > proof.end.as_slice() {
            return false;
        }
        let mut map = FastHashMap::default();
        for (address, key, value) in values {
            map.insert((address, key), value);
        }
        for entry in &proof.proofs {
            if entry.key.as_slice() < proof.start.as_slice()
                || entry.key.as_slice() > proof.end.as_slice()
            {
                return false;
            }
            match map.get(&(entry.address, entry.key)) {
                Some(value) => {
                    if !self.verify_proof(entry, &entry.address, &entry.key, *value) {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    pub fn create_multi_proof(
        &self,
        keys: Vec<(Address, B256)>,
    ) -> Result<QuantumMultiProof<S::MultiProof>, VerkleError> {
        let mut values = Vec::with_capacity(keys.len());
        for (address, key) in &keys {
            let value = self
                .values
                .get(&(*address, *key))
                .ok_or(VerkleError::KeyNotFound)?;
            values.push(u256_to_bytes(*value));
        }
        let (commitment, aux) = S::commit_with_aux(&values, None);
        let indices: Vec<usize> = (0..values.len()).collect();
        let proof = S::open_multi(&values, &indices, aux.as_deref())
            .map_err(VerkleError::ProofGenerationFailed)?;
        let indices_u32 = indices.iter().map(|idx| *idx as u32).collect::<Vec<_>>();
        Ok(QuantumMultiProof {
            root: self.root,
            keys,
            indices: indices_u32,
            commitment_bytes: S::commitment_bytes(&commitment),
            proof,
        })
    }

    pub fn verify_multi_proof(
        &self,
        proof: &QuantumMultiProof<S::MultiProof>,
        keys: Vec<(Address, B256)>,
        values: Vec<U256>,
    ) -> bool {
        if proof.root != self.root || proof.keys != keys || keys.len() != values.len() {
            return false;
        }
        let mut values_bytes = Vec::with_capacity(values.len());
        for ((address, key), value) in keys.iter().zip(values.iter()) {
            match self.values.get(&(*address, *key)) {
                Some(stored) if stored == value => {}
                _ => return false,
            }
            values_bytes.push(u256_to_bytes(*value));
        }
        let (commitment, _aux) = S::commit_with_aux(&values_bytes, None);
        if S::commitment_bytes(&commitment) != proof.commitment_bytes {
            return false;
        }
        let indices: Vec<usize> = proof.indices.iter().map(|idx| *idx as usize).collect();
        S::verify_multi(&commitment, &indices, &values_bytes, &proof.proof)
    }

    #[cfg(feature = "serde")]
    pub fn to_postcard(&self) -> Result<Vec<u8>, VerkleError> {
        let entries = self
            .values
            .iter()
            .map(|((address, key), value)| (*address, *key, *value))
            .collect::<Vec<_>>();
        let aux_entries = self
            .aux
            .iter()
            .map(|((address, key), aux)| (*address, *key, aux.clone()))
            .collect::<Vec<_>>();
        let snapshot = QuantumTreeSnapshot {
            entries,
            aux_entries,
        };
        postcard::to_allocvec(&snapshot).map_err(|_| VerkleError::SerializationError)
    }

    #[cfg(feature = "serde")]
    pub fn from_postcard(bytes: &[u8]) -> Result<Self, VerkleError> {
        let snapshot: QuantumTreeSnapshot =
            postcard::from_bytes(bytes).map_err(|_| VerkleError::SerializationError)?;
        let mut tree = Self::new();
        let mut aux_map = FastHashMap::default();
        for (address, key, aux) in snapshot.aux_entries {
            aux_map.insert((address, key), aux);
        }
        for (address, key, value) in snapshot.entries {
            if let Some(aux) = aux_map.get(&(address, key)) {
                tree.set_with_aux(&address, &key, value, aux);
            } else {
                tree.set(&address, &key, value);
            }
        }
        Ok(tree)
    }

    fn update_root(&mut self) {
        if self.commitments.is_empty() {
            self.root = B256::ZERO;
            return;
        }
        let mut items: Vec<_> = self.commitments.iter().collect();
        items.sort_by(|((addr_a, key_a), _), ((addr_b, key_b), _)| {
            let addr_cmp = addr_a.as_slice().cmp(addr_b.as_slice());
            if addr_cmp == Ordering::Equal {
                key_a.as_slice().cmp(key_b.as_slice())
            } else {
                addr_cmp
            }
        });

        let mut hasher = Keccak256::new();
        hasher.update(b"scheme-verkle-root-v1");
        for ((address, key), commitment) in items {
            hasher.update(address.as_slice());
            hasher.update(key.as_slice());
            hasher.update(&S::commitment_bytes(commitment));
        }
        self.root = hasher.finalize().into();
    }
}

fn u256_to_bytes(value: U256) -> Vec<u8> {
    value.to_be_bytes::<32>().to_vec()
}

fn write_header<S: CommitmentScheme>(out: &mut Vec<u8>) {
    out.extend_from_slice(&S::VERSION.to_be_bytes());
    let id_bytes = S::PARAMS_ID.as_bytes();
    out.extend_from_slice(&(id_bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(id_bytes);
}

fn read_header<S: CommitmentScheme>(bytes: &[u8], cursor: &mut usize) -> Result<(), VerkleError> {
    let version = read_u16(bytes, cursor)?;
    if version != S::VERSION {
        return Err(VerkleError::SerializationError);
    }
    let id_len = read_u16(bytes, cursor)? as usize;
    let id_bytes = read_bytes(bytes, cursor, id_len)?;
    if id_bytes != S::PARAMS_ID.as_bytes() {
        return Err(VerkleError::SerializationError);
    }
    Ok(())
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, VerkleError> {
    if *cursor + 4 > bytes.len() {
        return Err(VerkleError::SerializationError);
    }
    let value = u32::from_be_bytes(
        bytes[*cursor..*cursor + 4]
            .try_into()
            .map_err(|_| VerkleError::SerializationError)?,
    );
    *cursor += 4;
    Ok(value)
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, VerkleError> {
    if *cursor + 2 > bytes.len() {
        return Err(VerkleError::SerializationError);
    }
    let value = u16::from_be_bytes(
        bytes[*cursor..*cursor + 2]
            .try_into()
            .map_err(|_| VerkleError::SerializationError)?,
    );
    *cursor += 2;
    Ok(value)
}

fn read_bytes(bytes: &[u8], cursor: &mut usize, len: usize) -> Result<Vec<u8>, VerkleError> {
    if *cursor + len > bytes.len() {
        return Err(VerkleError::SerializationError);
    }
    let slice = bytes[*cursor..*cursor + len].to_vec();
    *cursor += len;
    Ok(slice)
}

fn read_b256(bytes: &[u8], cursor: &mut usize) -> Result<B256, VerkleError> {
    if *cursor + 32 > bytes.len() {
        return Err(VerkleError::SerializationError);
    }
    let value = B256::from_slice(&bytes[*cursor..*cursor + 32]);
    *cursor += 32;
    Ok(value)
}

fn read_address(bytes: &[u8], cursor: &mut usize) -> Result<Address, VerkleError> {
    if *cursor + 20 > bytes.len() {
        return Err(VerkleError::SerializationError);
    }
    let value = Address::from_slice(&bytes[*cursor..*cursor + 20]);
    *cursor += 20;
    Ok(value)
}
