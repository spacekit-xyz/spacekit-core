//! State commitment and per-block undo for `SwtchvmState`.
//!
//! **Commitment.** A block's `state_root` commits to every account (balance,
//! nonce, code, storage, compute used), every contract KV entry and every
//! storage slot. It is a lattice hash (LtHash16): each entry maps to 1,024
//! 16-bit lanes (a BLAKE3 XOF), the lanes of all entries are added with
//! wrap-around, and the root is BLAKE3 over the sum. Addition commutes, so the
//! root does not depend on iteration order, and it is updated incrementally:
//! when an entry changes, its old lanes are subtracted and its new lanes
//! added. A block costs work in proportion to what it touched, not to the size
//! of the state.
//!
//! **Journal.** Every mutation records the entry's value from before its
//! first change since the last checkpoint. Folding the journal into the
//! commitment yields that record, which is exactly what undoing the block
//! needs (fork choice rolls back blocks with it).

use std::collections::HashMap;

use super::swtchvm_node::{SwtchvmAccount, SwtchvmAddress};

pub const LANES: usize = 1_024;
const DOMAIN: &[u8] = b"SPACEKIT-STATE-v2";

/// Sum of per-entry lane vectors (LtHash16).
#[derive(Clone, PartialEq, Eq)]
pub struct LtHash(pub Box<[u16; LANES]>);

impl Default for LtHash {
    fn default() -> Self {
        Self(Box::new([0u16; LANES]))
    }
}

impl std::fmt::Debug for LtHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LtHash({})", hex::encode(self.root()))
    }
}

impl LtHash {
    fn lanes(entry: &[u8]) -> [u16; LANES] {
        let mut bytes = [0u8; LANES * 2];
        let mut hasher = blake3::Hasher::new();
        hasher.update(entry);
        hasher.finalize_xof().fill(&mut bytes);
        let mut out = [0u16; LANES];
        for (i, lane) in out.iter_mut().enumerate() {
            *lane = u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]);
        }
        out
    }

    pub fn add(&mut self, entry: &[u8]) {
        let l = Self::lanes(entry);
        for (a, b) in self.0.iter_mut().zip(l.iter()) {
            *a = a.wrapping_add(*b);
        }
    }

    pub fn remove(&mut self, entry: &[u8]) {
        let l = Self::lanes(entry);
        for (a, b) in self.0.iter_mut().zip(l.iter()) {
            *a = a.wrapping_sub(*b);
        }
    }

    pub fn root(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(DOMAIN);
        for lane in self.0.iter() {
            hasher.update(&lane.to_le_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

/// Canonical bytes of an account entry. The account's own storage map is
/// sorted so the encoding does not depend on hash-map order.
pub fn account_entry(address: &SwtchvmAddress, account: &SwtchvmAccount) -> Vec<u8> {
    let mut out = Vec::with_capacity(128);
    out.push(b'A');
    out.extend_from_slice(address.as_bytes());
    out.extend_from_slice(&account.balance.to_le_bytes());
    out.extend_from_slice(&account.nonce.to_le_bytes());
    out.extend_from_slice(&account.compute_used.to_le_bytes());
    match &account.code {
        Some(code) => {
            out.push(1);
            out.extend_from_slice(blake3::hash(code).as_bytes());
        }
        None => out.push(0),
    }
    let mut slots: Vec<(&[u8; 32], &[u8; 32])> = account.storage.iter().collect();
    slots.sort();
    out.extend_from_slice(&(slots.len() as u64).to_le_bytes());
    for (k, v) in slots {
        out.extend_from_slice(k);
        out.extend_from_slice(v);
    }
    out
}

pub fn kv_entry(address: &SwtchvmAddress, key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 20 + 8 + key.len() + value.len());
    out.push(b'K');
    out.extend_from_slice(address.as_bytes());
    out.extend_from_slice(&(key.len() as u64).to_le_bytes());
    out.extend_from_slice(key);
    out.extend_from_slice(value);
    out
}

pub fn storage_entry(address: &SwtchvmAddress, key: &[u8; 32], value: &[u8; 32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + 20 + 64);
    out.push(b'S');
    out.extend_from_slice(address.as_bytes());
    out.extend_from_slice(key);
    out.extend_from_slice(value);
    out
}

/// Values entries had before their first change since the last checkpoint.
/// `None` means the entry did not exist.
#[derive(Debug, Clone, Default)]
pub struct StateJournal {
    pub accounts: HashMap<SwtchvmAddress, Option<SwtchvmAccount>>,
    pub kv: HashMap<(SwtchvmAddress, Vec<u8>), Option<Vec<u8>>>,
    pub storage: HashMap<(SwtchvmAddress, [u8; 32]), Option<[u8; 32]>>,
}

impl StateJournal {
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty() && self.kv.is_empty() && self.storage.is_empty()
    }

    pub fn len(&self) -> usize {
        self.accounts.len() + self.kv.len() + self.storage.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lthash_is_order_independent_and_invertible() {
        let mut a = LtHash::default();
        a.add(b"one");
        a.add(b"two");
        let mut b = LtHash::default();
        b.add(b"two");
        b.add(b"one");
        assert_eq!(a.root(), b.root());
        b.remove(b"two");
        let mut c = LtHash::default();
        c.add(b"one");
        assert_eq!(b.root(), c.root());
        assert_ne!(a.root(), c.root());
    }
}
