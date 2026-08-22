use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::v1::identity::{Identity, DIDIdentity};

#[derive(Default)]
pub struct MockIdentityManager {
    identities: Arc<RwLock<HashMap<String, Identity>>>,
}

impl MockIdentityManager {
    pub fn new() -> Self {
        Self {
            identities: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn add_mock_identity(&self, identity: Identity) {
        let mut identities = self.identities.write().await;
        identities.insert(identity.did.clone(), identity);
    }

    pub async fn load_identity(&self, did: &str) -> Result<Identity, Box<dyn Error>> {
        let identities = self.identities.read().await;
        identities
            .get(did)
            .cloned()
            .ok_or_else(|| Box::new(SdkError::IdentityNotFound))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_identity_loading() {
        // Create mock SDK
        let mock_manager = MockIdentityManager::new();
        
        // Add mock identity
        let test_identity = Identity {
            did: "0x123".to_string(),
            username: "test_user".to_string(),
            // ... other fields ...
        };
        mock_manager.add_mock_identity(test_identity.clone()).await;

        // Test loading
        let loaded = mock_manager.load_identity("0x123").await.unwrap();
        assert_eq!(loaded.did, test_identity.did);
        assert_eq!(loaded.username, test_identity.username);
    }
} 