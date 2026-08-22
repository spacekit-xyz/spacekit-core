// Registry types and VerifiableDataRegistry trait
#[derive(Clone)]
pub struct DidDocument {
    pub id: String,
    pub verification_methods: Vec<VerificationMethod>,
    pub authentication: Vec<String>,
    pub service_endpoints: Vec<ServiceEndpoint>,
}

#[derive(Clone)]
pub struct VerificationMethod {
    pub id: String,
    pub type_: String,
    pub public_key: Vec<u8>,
}

#[derive(Clone)]
pub struct ServiceEndpoint {
    pub id: String,
    pub type_: String,
    pub service_endpoint: String,
}

pub trait VerifiableDataRegistry {
    fn resolve_did(&self, did: &str) -> anyhow::Result<DidDocument>;
    fn update_did(&self, doc: &DidDocument) -> anyhow::Result<()>;
    fn get_credential_status(&self, status_id: &str) -> anyhow::Result<CredentialStatus>;
}

pub enum CredentialStatus {
    Active,
    Revoked,
    Unknown,
}
