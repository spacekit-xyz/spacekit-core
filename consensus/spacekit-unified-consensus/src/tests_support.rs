//! Shared test helpers. Not part of the public API; only compiled under
//! `#[cfg(test)]` to keep the production binary lean.

use crate::facade::CoordinatorHandle;
use alloc::vec::Vec;
use alloy_primitives::B256;

extern crate alloc;

/// Mock coordinator used across facade tests.
pub struct MockCoordinator {
    pub validators: Vec<(B256, u128)>,
    pub votes: alloc::collections::BTreeMap<B256, alloc::collections::BTreeMap<B256, bool>>,
    pub finalized: alloc::collections::BTreeSet<B256>,
}

impl MockCoordinator {
    pub fn new(validators: Vec<(B256, u128)>) -> Self {
        Self {
            validators,
            votes: alloc::collections::BTreeMap::new(),
            finalized: alloc::collections::BTreeSet::new(),
        }
    }
    pub fn mark_finalized(&mut self, block: B256) {
        self.finalized.insert(block);
    }
}

impl CoordinatorHandle for MockCoordinator {
    fn eligible_validators(&self) -> Vec<(B256, u128)> {
        self.validators.clone()
    }

    fn submit_vote_raw(
        &mut self,
        validator_did: B256,
        block_hash: B256,
        support: bool,
    ) -> Result<bool, alloc::string::String> {
        self.votes
            .entry(block_hash)
            .or_default()
            .insert(validator_did, support);
        Ok(true)
    }

    fn supporting_vote_count(&self, block_hash: &B256) -> u64 {
        self.votes
            .get(block_hash)
            .map(|m| m.values().filter(|v| **v).count() as u64)
            .unwrap_or(0)
    }

    fn eligible_validator_count(&self) -> u64 {
        self.validators.len() as u64
    }

    fn is_soft_finalized(&self, block_hash: &B256) -> bool {
        self.finalized.contains(block_hash)
    }

    fn supporting_validators(&self, block_hash: &B256) -> Vec<B256> {
        self.votes
            .get(block_hash)
            .map(|m| {
                m.iter()
                    .filter(|(_, support)| **support)
                    .map(|(did, _)| *did)
                    .collect()
            })
            .unwrap_or_default()
    }
}
