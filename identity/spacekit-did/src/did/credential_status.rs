// Credential status checking via registry
use super::did_registry_client::{CredentialStatus, VerifiableDataRegistry};

pub trait CredentialStatusChecker {
    fn is_active(&self, status_id: &str) -> anyhow::Result<bool>;
}

pub struct RegistryStatusChecker<R: VerifiableDataRegistry> {
    pub registry: R,
}

impl<R: VerifiableDataRegistry> CredentialStatusChecker for RegistryStatusChecker<R> {
    fn is_active(&self, status_id: &str) -> anyhow::Result<bool> {
        match self.registry.get_credential_status(status_id)? {
            CredentialStatus::Active => Ok(true),
            CredentialStatus::Revoked => Ok(false),
            CredentialStatus::Unknown => Ok(false),
        }
    }
}
