// SpaceKit DID resolver and key types
use super::did_registry_client::{DidDocument, VerifiableDataRegistry};

/// Trait required by vc_verifier; implement for any resolver that can resolve a DID to a document.
pub trait DidResolver {
    fn resolve(&self, did: &str) -> anyhow::Result<DidDocument>;
}

pub struct SpacekitDidResolver<R: VerifiableDataRegistry> {
    pub registry: R,
}

impl<R: VerifiableDataRegistry> DidResolver for SpacekitDidResolver<R> {
    fn resolve(&self, did: &str) -> anyhow::Result<DidDocument> {
        if !did.starts_with("did:spacekit:") {
            anyhow::bail!("unsupported DID method; expected did:spacekit:...");
        }
        self.registry.resolve_did(did)
    }
}

pub struct DidVerificationKey {
    pub id: String,
    pub public_key: Vec<u8>,
    pub algo: String,
}

pub trait DidKeyExtractor {
    fn authentication_keys(&self, doc: &DidDocument) -> Vec<DidVerificationKey>;
}

/// Extracts verification keys listed in the DID document `authentication` array.
pub struct SpacekitKeyExtractor;

impl DidKeyExtractor for SpacekitKeyExtractor {
    fn authentication_keys(&self, doc: &DidDocument) -> Vec<DidVerificationKey> {
        doc.verification_methods
            .iter()
            .filter(|vm| doc.authentication.contains(&vm.id))
            .map(|vm| DidVerificationKey {
                id: vm.id.clone(),
                public_key: vm.public_key.clone(),
                algo: vm.type_.clone(),
            })
            .collect()
    }
}
