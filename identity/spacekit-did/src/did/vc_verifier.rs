// VPN access VC verification
use base64::Engine;
use chrono::{DateTime, Utc};

use crate::sphincs::SphincsPlus;

use super::did_registry_client;
use super::did_spacekit::{DidKeyExtractor, DidResolver, SpacekitKeyExtractor};
use super::vc_issuer::VpnAccessCredential;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VpnAccessClaim {
    pub subject_did: String,
    pub plan: String,
    pub expires_at: DateTime<Utc>,
}

pub trait VcVerifier {
    fn verify_vpn_access_vc(&self, vc_str: &str) -> anyhow::Result<VpnAccessClaim>;
}

pub struct SpacekitVcVerifier<R: DidResolver, V: did_registry_client::VerifiableDataRegistry> {
    pub resolver: R,
    /// Reserved for registry-backed credential status checks.
    #[allow(dead_code)]
    pub registry: V,
    pub trusted_issuers: Vec<String>,
    pub key_extractor: SpacekitKeyExtractor,
}

impl<R, V> SpacekitVcVerifier<R, V>
where
    R: DidResolver,
    V: did_registry_client::VerifiableDataRegistry,
{
    pub fn new(resolver: R, registry: V, trusted_issuers: Vec<String>) -> Self {
        Self {
            resolver,
            registry,
            trusted_issuers,
            key_extractor: SpacekitKeyExtractor,
        }
    }
}

impl<R, V> VcVerifier for SpacekitVcVerifier<R, V>
where
    R: DidResolver,
    V: did_registry_client::VerifiableDataRegistry,
{
    fn verify_vpn_access_vc(&self, vc_str: &str) -> anyhow::Result<VpnAccessClaim> {
        let json: serde_json::Value = serde_json::from_str(vc_str)?;
        let vc_value = json
            .get("vc")
            .ok_or_else(|| anyhow::anyhow!("missing vc field"))?;
        let proof = json
            .get("proof")
            .ok_or_else(|| anyhow::anyhow!("missing proof field"))?;

        let vc: VpnAccessCredential = serde_json::from_value(vc_value.clone())?;

        if !self.trusted_issuers.contains(&vc.issuer_did) {
            anyhow::bail!("untrusted issuer");
        }

        if vc.expires_at < Utc::now() {
            anyhow::bail!("credential expired");
        }

        let signature_b64 = proof
            .get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing proof signature"))?;
        let signature = base64::engine::general_purpose::STANDARD
            .decode(signature_b64)
            .map_err(|e| anyhow::anyhow!("invalid base64 signature: {e}"))?;

        let payload = serde_json::to_vec(&vc)?;
        let issuer_doc = self.resolver.resolve(&vc.issuer_did)?;
        let auth_keys = self.key_extractor.authentication_keys(&issuer_doc);

        if auth_keys.is_empty() {
            anyhow::bail!("issuer DID document has no authentication keys");
        }

        let verified = auth_keys
            .iter()
            .any(|key| SphincsPlus::verify(&key.public_key, &payload, &signature));

        if !verified {
            anyhow::bail!("signature verification failed");
        }

        Ok(VpnAccessClaim {
            subject_did: vc.subject_did,
            plan: vc.plan,
            expires_at: vc.expires_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::did::did_registry_client::{
        CredentialStatus, DidDocument, ServiceEndpoint, VerifiableDataRegistry, VerificationMethod,
    };
    use crate::did::did_spacekit::SpacekitDidResolver;
    use crate::did::did_wallet::{InMemoryDidWallet, LocalDid};
    use crate::did::vc_issuer::{DidBasedVcIssuer, VcIssuer};
    use chrono::Duration;

    struct InMemoryRegistry {
        docs: std::collections::HashMap<String, DidDocument>,
    }

    impl InMemoryRegistry {
        fn with_doc(doc: DidDocument) -> Self {
            let mut docs = std::collections::HashMap::new();
            docs.insert(doc.id.clone(), doc);
            Self { docs }
        }
    }

    impl VerifiableDataRegistry for InMemoryRegistry {
        fn resolve_did(&self, did: &str) -> anyhow::Result<DidDocument> {
            self.docs
                .get(did)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("DID not found"))
        }

        fn update_did(&self, _doc: &DidDocument) -> anyhow::Result<()> {
            Ok(())
        }

        fn get_credential_status(&self, _status_id: &str) -> anyhow::Result<CredentialStatus> {
            Ok(CredentialStatus::Active)
        }
    }

    fn sample_doc(did: &str, public_key: Vec<u8>) -> DidDocument {
        let key_id = format!("{did}#key-1");
        DidDocument {
            id: did.to_string(),
            verification_methods: vec![VerificationMethod {
                id: key_id.clone(),
                type_: "SPHINCS+2026".to_string(),
                public_key,
            }],
            authentication: vec![key_id],
            service_endpoints: vec![ServiceEndpoint {
                id: format!("{did}#service-1"),
                type_: "SpacekitService".to_string(),
                service_endpoint: "https://spacekit.xyz".to_string(),
            }],
        }
    }

    #[test]
    fn verify_valid_vpn_vc() {
        let issuer_did = "did:spacekit:testnet:issuer1";
        let subject_did = "did:spacekit:testnet:subject1";
        let keypair = crate::SphincsPlus::generate_keypair();

        let local = LocalDid {
            did: issuer_did.to_string(),
            key_id: format!("{issuer_did}#key-1"),
            public_key: keypair.public_key.clone(),
            private_key: keypair.private_key,
        };
        let wallet = InMemoryDidWallet { keys: vec![local] };
        let issuer = DidBasedVcIssuer {
            issuer_did: issuer_did.to_string(),
            wallet,
        };

        let vc_str = issuer
            .issue_vpn_access(subject_did, "pro", Utc::now() + Duration::days(1))
            .unwrap();

        let doc = sample_doc(issuer_did, keypair.public_key.clone());
        let registry = InMemoryRegistry::with_doc(doc.clone());
        let resolver = SpacekitDidResolver {
            registry: InMemoryRegistry::with_doc(doc),
        };
        let verifier = SpacekitVcVerifier::new(resolver, registry, vec![issuer_did.to_string()]);

        let claim = verifier.verify_vpn_access_vc(&vc_str).unwrap();
        assert_eq!(claim.subject_did, subject_did);
        assert_eq!(claim.plan, "pro");
    }

    #[test]
    fn reject_tampered_vc() {
        let issuer_did = "did:spacekit:testnet:issuer2";
        let keypair = crate::SphincsPlus::generate_keypair();
        let local = LocalDid {
            did: issuer_did.to_string(),
            key_id: format!("{issuer_did}#key-1"),
            public_key: keypair.public_key.clone(),
            private_key: keypair.private_key,
        };
        let wallet = InMemoryDidWallet { keys: vec![local] };
        let issuer = DidBasedVcIssuer {
            issuer_did: issuer_did.to_string(),
            wallet,
        };

        let mut vc_str = issuer
            .issue_vpn_access(
                "did:spacekit:testnet:subject2",
                "basic",
                Utc::now() + Duration::days(1),
            )
            .unwrap();
        vc_str = vc_str.replace("\"plan\":\"basic\"", "\"plan\":\"admin\"");

        let doc = sample_doc(issuer_did, keypair.public_key);
        let registry = InMemoryRegistry::with_doc(doc.clone());
        let resolver = SpacekitDidResolver {
            registry: InMemoryRegistry::with_doc(doc),
        };
        let verifier = SpacekitVcVerifier::new(resolver, registry, vec![issuer_did.to_string()]);

        assert!(verifier.verify_vpn_access_vc(&vc_str).is_err());
    }
}
