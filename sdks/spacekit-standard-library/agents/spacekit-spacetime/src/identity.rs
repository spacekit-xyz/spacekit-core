use spacekit::prelude::*;
use alloc::string::String;
use alloc::collections::BTreeMap;
#[allow(unused_imports)]
use serde::{Deserialize, Serialize};

#[cfg_attr(not(test), derive(Serialize, Deserialize))]
#[derive(Clone)]
pub struct AgentProfile {
    pub name: String,
    pub model: String,
    pub metadata_ref: Option<String>,
    pub registered_at: u64,
}

#[derive(Default)]
pub struct SpaceTimeIdentity {
    owner: Did,
    agents: BTreeMap<Did, AgentProfile>,
}

impl SpaceTimeIdentity {
    pub fn init(&mut self, owner: Did) {
        self.owner = owner;
    }

    pub fn register_agent(&mut self, did: Did, profile: AgentProfile) {
        self.require_owner();
        self.agents.insert(did, profile);
    }

    pub fn is_agent(&self, did: Did) -> bool {
        self.agents.contains_key(&did)
    }

    pub fn get_profile(&self, did: Did) -> Option<AgentProfile> {
        self.agents.get(&did).cloned()
    }

    fn require_owner(&self) {
        assert!(env::caller() == self.owner, "not owner");
    }
}
