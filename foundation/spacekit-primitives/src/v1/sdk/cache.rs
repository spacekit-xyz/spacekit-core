use crate::v1::identity::Identity;
use sled::Db;
use std::error::Error;

pub struct IdentityCache {
    db: Db,
}

impl IdentityCache {
    pub fn new(path: &str) -> Result<Self, Box<dyn Error>> {
        let db = sled::open(path)?;
        Ok(Self { db })
    }

    pub fn cache_identity(&self, identity: &Identity) -> Result<(), Box<dyn Error>> {
        let key = identity.did.as_bytes();
        let value = serde_json::to_vec(&identity)?;
        self.db.insert(key, value)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn get_cached_identity(&self, did: &str) -> Result<Option<Identity>, Box<dyn Error>> {
        if let Some(data) = self.db.get(did.as_bytes())? {
            let identity: Identity = serde_json::from_slice(&data)?;
            Ok(Some(identity))
        } else {
            Ok(None)
        }
    }
}
