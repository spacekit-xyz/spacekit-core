use std::sync::Arc;

use parking_lot::RwLock;

use crate::guardian::GuardianState;
use crate::types::GuardianInfo;

pub struct RegistryState {
    guardians: RwLock<Vec<Arc<GuardianState>>>,
    infos: RwLock<Vec<GuardianInfo>>,
}

impl RegistryState {
    pub fn new() -> Self {
        Self {
            guardians: RwLock::new(Vec::new()),
            infos: RwLock::new(Vec::new()),
        }
    }

    pub fn register(&self, g: Arc<GuardianState>) {
        self.infos.write().push(g.info.clone());
        self.guardians.write().push(g);
    }

    pub fn register_info(&self, info: GuardianInfo) {
        self.infos.write().push(info);
    }

    pub fn list(&self) -> Vec<Arc<GuardianState>> {
        self.guardians.read().clone()
    }

    pub fn list_info(&self) -> Vec<GuardianInfo> {
        self.infos.read().clone()
    }
}
